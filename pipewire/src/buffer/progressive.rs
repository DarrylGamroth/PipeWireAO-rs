//! Safe range-limited access to PipeWireAO progressive buffers.

use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use spa::buffer::{
    meta::{MetaHeader, MetaProgressive, ProgressiveFlags, ProgressiveSnapshot, ProgressiveState},
    ChunkFlags,
};

use super::{Buffer, ProgressiveFilterBuffer};

/// A progressive buffer does not satisfy the negotiated ownership protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressiveBufferError {
    /// Required progressive metadata is absent or malformed.
    InvalidMetadata,
    /// The selected data plane, chunk, or payload range is invalid.
    InvalidLayout,
    /// A lifecycle or committed-prefix transition is invalid.
    InvalidState,
    /// PipeWireAO rejected the progressive producer announcement.
    AnnouncementFailed,
}

impl std::fmt::Display for ProgressiveBufferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMetadata => "PipeWireAO buffer has invalid progressive metadata",
            Self::InvalidLayout => "PipeWireAO buffer has an invalid progressive payload layout",
            Self::InvalidState => "progressive buffer lifecycle transition is invalid",
            Self::AnnouncementFailed => "PipeWireAO rejected the progressive buffer announcement",
        })
    }
}

impl std::error::Error for ProgressiveBufferError {}

/// An active progressive output whose committed prefix is visible downstream.
///
/// This owner never creates a mutable reference spanning the whole payload.
/// Call [`Self::write_until`] to borrow only the producer-owned suffix and
/// publish it with [`ProgressiveWrite::commit`]. Dropping an unterminated owner
/// release-publishes `Aborted | Cancelled` before ending the PipeWireAO
/// producer lease.
pub struct ProgressiveOutputBuffer<'f> {
    _lease: ProgressiveFilterBuffer<'f>,
    metadata: NonNull<MetaProgressive>,
    header: Option<NonNull<MetaHeader>>,
    payload: NonNull<u8>,
    payload_size: usize,
    commit_granularity: usize,
    stride: i32,
    committed: usize,
    terminal: bool,
}

impl<'f> ProgressiveOutputBuffer<'f> {
    /// Initializes and announces one mapped output buffer.
    ///
    /// The ordinary output path must configure the selected chunk before this
    /// call. Its offset and size must exactly match `payload_offset` and
    /// `payload_size`. Once this call succeeds, access is range-limited until
    /// the owner reaches `Complete` or `Aborted` and is dropped.
    pub fn begin(
        mut buffer: Buffer<'f>,
        data_index: u32,
        payload_offset: u32,
        payload_size: u32,
        commit_granularity: u32,
    ) -> Result<Self, ProgressiveBufferError> {
        if payload_size == 0 || commit_granularity == 0 || commit_granularity > payload_size {
            return Err(ProgressiveBufferError::InvalidLayout);
        }
        let payload = payload_layout(&mut buffer, data_index, payload_offset, payload_size)?;
        let metadata = buffer
            .find_meta_mut::<MetaProgressive>()
            .ok_or(ProgressiveBufferError::InvalidMetadata)?;
        metadata
            .initialize(data_index, payload_offset, payload_size, commit_granularity)
            .map_err(|_| ProgressiveBufferError::InvalidMetadata)?;
        metadata.store_release(ProgressiveSnapshot::new(0, ProgressiveState::Active));
        let metadata = NonNull::from(metadata);
        let header = buffer.find_meta_mut::<MetaHeader>().map(NonNull::from);

        let lease = match buffer.begin_progressive() {
            Ok(lease) => lease,
            Err(error) => {
                let mut buffer = error.into_buffer();
                if let Some(metadata) = buffer.find_meta_mut::<MetaProgressive>() {
                    metadata.set_terminal_flags(
                        ProgressiveFlags::INCOMPLETE | ProgressiveFlags::PROTOCOL_ERROR,
                    );
                    metadata.store_release(ProgressiveSnapshot::new(0, ProgressiveState::Aborted));
                }
                return Err(ProgressiveBufferError::AnnouncementFailed);
            }
        };

        Ok(Self {
            _lease: lease,
            metadata,
            header,
            payload: payload.pointer,
            payload_size: payload_size as usize,
            commit_granularity: commit_granularity as usize,
            stride: payload.stride,
            committed: 0,
            terminal: false,
        })
    }

    /// Returns the number of bytes already published as immutable.
    pub fn committed_bytes(&self) -> usize {
        self.committed
    }

    /// Returns the negotiated progressive payload extent in bytes.
    pub fn payload_size(&self) -> usize {
        self.payload_size
    }

    /// Returns the negotiated progressive commit granularity in bytes.
    pub fn commit_granularity(&self) -> usize {
        self.commit_granularity
    }

    /// Returns the selected SPA chunk stride.
    pub fn stride(&self) -> i32 {
        self.stride
    }

    /// Copies a scientific-sample header before terminal publication.
    ///
    /// Header contents are terminal-only in the progressive profile. The
    /// following [`Self::complete`] or [`Self::abort`] release store publishes
    /// this copy to consumers.
    pub fn set_terminal_header(
        &mut self,
        source: &MetaHeader,
    ) -> Result<(), ProgressiveBufferError> {
        if self.terminal {
            return Err(ProgressiveBufferError::InvalidState);
        }
        let header = self
            .header
            .as_mut()
            .ok_or(ProgressiveBufferError::InvalidMetadata)?;
        unsafe {
            // SAFETY: only the active producer writes the output header. The
            // producer lease keeps its mapped metadata alive until terminal
            // publication and return.
            header.as_mut().copy_from(source);
        }
        Ok(())
    }

    /// Borrows the unpublished range from the current prefix to `end`.
    ///
    /// A non-final boundary must be aligned to the negotiated commit
    /// granularity. The returned range is not visible to consumers until its
    /// [`ProgressiveWrite::commit`] method is called.
    pub fn write_until(
        &mut self,
        end: usize,
    ) -> Result<ProgressiveWrite<'_, 'f>, ProgressiveBufferError> {
        if self.terminal
            || end <= self.committed
            || end > self.payload_size
            || (end != self.payload_size && !end.is_multiple_of(self.commit_granularity))
        {
            return Err(ProgressiveBufferError::InvalidState);
        }
        let start = self.committed;
        let payload = unsafe {
            // SAFETY: `begin` validated the complete range. Only
            // `[committed, end)` will be exposed by the write guard, and
            // `start` cannot exceed the payload size.
            NonNull::new_unchecked(self.payload.as_ptr().add(start))
        };
        Ok(ProgressiveWrite {
            output: self,
            payload,
            length: end - start,
            end,
        })
    }

    /// Publishes a successful terminal state after the whole payload commits.
    pub fn complete(&mut self) -> Result<(), ProgressiveBufferError> {
        if self.terminal || self.committed != self.payload_size {
            return Err(ProgressiveBufferError::InvalidState);
        }
        self.metadata().store_release(ProgressiveSnapshot::new(
            self.payload_size as u32,
            ProgressiveState::Complete,
        ));
        self.terminal = true;
        Ok(())
    }

    /// Publishes a terminal failure while preserving the valid prefix.
    pub fn abort(&mut self, mut flags: ProgressiveFlags) -> Result<(), ProgressiveBufferError> {
        if self.terminal {
            return Err(ProgressiveBufferError::InvalidState);
        }
        if self.committed < self.payload_size {
            flags |= ProgressiveFlags::INCOMPLETE;
        }
        unsafe {
            // SAFETY: only the active producer writes terminal flags, and the
            // following release store publishes them to terminal consumers.
            self.metadata.as_mut().set_terminal_flags(flags);
        }
        self.metadata().store_release(ProgressiveSnapshot::new(
            self.committed as u32,
            ProgressiveState::Aborted,
        ));
        self.terminal = true;
        Ok(())
    }

    /// Publishes cancellation while preserving the last committed prefix.
    pub fn cancel(&mut self) -> Result<(), ProgressiveBufferError> {
        self.abort(ProgressiveFlags::CANCELLED)
    }

    fn metadata(&self) -> &MetaProgressive {
        unsafe {
            // SAFETY: the PipeWireAO producer lease keeps the mapped buffer
            // alive, and this owner never changes the allocation identity.
            self.metadata.as_ref()
        }
    }
}

impl Drop for ProgressiveOutputBuffer<'_> {
    fn drop(&mut self) {
        if !self.terminal {
            let _ = self.cancel();
        }
        // `_lease` is dropped next and ends the PipeWireAO producer lease.
    }
}

/// A producer-owned unpublished payload range.
///
/// Dropping this value without calling [`Self::commit`] leaves the shared
/// prefix unchanged. This permits cancellation without exposing partially
/// written bytes.
pub struct ProgressiveWrite<'a, 'f> {
    output: &'a mut ProgressiveOutputBuffer<'f>,
    payload: NonNull<u8>,
    length: usize,
    end: usize,
}

impl ProgressiveWrite<'_, '_> {
    /// Borrows this exact unpublished range as aligned native F32 values.
    ///
    /// PipeWireAO's supported target profile is little-endian. No reference
    /// is created outside this guard's producer-owned suffix.
    pub fn as_f32_mut(&mut self) -> Result<&mut [f32], ProgressiveBufferError> {
        progressive_f32_mut(self)
    }

    /// Release-publishes the written range as part of the immutable prefix.
    pub fn commit(self) {
        self.output
            .metadata()
            .store_release(ProgressiveSnapshot::new(
                self.end as u32,
                ProgressiveState::Active,
            ));
        self.output.committed = self.end;
    }
}

impl Deref for ProgressiveWrite<'_, '_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe {
            // SAFETY: the parent owner grants this guard only the validated
            // unpublished suffix, and borrowing the guard keeps `commit` from
            // running until this reference expires.
            std::slice::from_raw_parts(self.payload.as_ptr(), self.length)
        }
    }
}

impl DerefMut for ProgressiveWrite<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // SAFETY: the guard is the sole producer owner of this unpublished
            // suffix, and its mutable borrow prevents publication while the
            // returned reference exists.
            std::slice::from_raw_parts_mut(self.payload.as_ptr(), self.length)
        }
    }
}

/// A consumer lease that exposes only acquire-observed committed prefixes.
pub struct ProgressiveInput<'b, 'f> {
    metadata: NonNull<MetaProgressive>,
    header: Option<NonNull<MetaHeader>>,
    payload: NonNull<u8>,
    payload_size: usize,
    commit_granularity: usize,
    stride: i32,
    previous: Option<ProgressiveSnapshot>,
    buffer_borrow: PhantomData<&'b mut Buffer<'f>>,
}

impl<'b, 'f> ProgressiveInput<'b, 'f> {
    /// Validates one dequeued mapped input without borrowing its full payload.
    pub fn new(buffer: &'b mut Buffer<'f>) -> Result<Self, ProgressiveBufferError> {
        let metadata = buffer
            .find_meta::<MetaProgressive>()
            .ok_or(ProgressiveBufferError::InvalidMetadata)?;
        metadata
            .validate()
            .map_err(|_| ProgressiveBufferError::InvalidMetadata)?;
        let data_index = metadata.data_index();
        let payload_offset = metadata.payload_offset();
        let payload_size = metadata.payload_size();
        let commit_granularity = metadata.commit_granularity();
        let metadata = NonNull::from(metadata);
        let payload = payload_layout(buffer, data_index, payload_offset, payload_size)?;
        let header = buffer.find_meta::<MetaHeader>().map(NonNull::from);

        Ok(Self {
            metadata,
            header,
            payload: payload.pointer,
            payload_size: payload_size as usize,
            commit_granularity: commit_granularity as usize,
            stride: payload.stride,
            previous: None,
            buffer_borrow: PhantomData,
        })
    }

    /// Returns the negotiated progressive payload extent in bytes.
    pub fn payload_size(&self) -> usize {
        self.payload_size
    }

    /// Returns the negotiated progressive commit granularity in bytes.
    pub fn commit_granularity(&self) -> usize {
        self.commit_granularity
    }

    /// Returns the selected SPA chunk stride.
    pub fn stride(&self) -> i32 {
        self.stride
    }

    /// Acquire-loads progress and borrows exactly the observed valid prefix.
    pub fn acquire(&mut self) -> Result<ProgressiveRead<'_>, ProgressiveBufferError> {
        let metadata = unsafe {
            // SAFETY: the exclusive buffer borrow keeps this mapped metadata
            // alive for the duration of the consumer lease.
            self.metadata.as_ref()
        };
        let observation = metadata
            .observe_acquire()
            .map_err(|_| ProgressiveBufferError::InvalidMetadata)?;
        let snapshot = observation.snapshot();
        validate_observation(
            snapshot,
            self.previous,
            self.payload_size,
            self.commit_granularity,
        )?;
        self.previous = Some(snapshot);
        let committed = snapshot.committed_bytes() as usize;
        let bytes = unsafe {
            // SAFETY: the acquire-loaded snapshot grants immutable access only
            // to `[0, committed)`. The producer never writes this prefix again,
            // and the underlying buffer cannot be returned while this view or
            // its parent consumer lease is borrowed.
            std::slice::from_raw_parts(self.payload.as_ptr(), committed)
        };
        let terminal_header = match snapshot.state() {
            ProgressiveState::Complete | ProgressiveState::Aborted => {
                self.header.map(|header| unsafe {
                    // SAFETY: the terminal release store publishes header
                    // writes, and the consumer lease keeps metadata alive.
                    header.as_ref().clone()
                })
            }
            ProgressiveState::Prepared | ProgressiveState::Active => None,
        };
        Ok(ProgressiveRead {
            bytes,
            snapshot,
            terminal_flags: observation.terminal_flags(),
            terminal_header,
        })
    }
}

/// One coherent progressive snapshot and its acquire-observed valid prefix.
pub struct ProgressiveRead<'a> {
    bytes: &'a [u8],
    snapshot: ProgressiveSnapshot,
    terminal_flags: Option<ProgressiveFlags>,
    terminal_header: Option<MetaHeader>,
}

impl ProgressiveRead<'_> {
    /// Returns the lifecycle state and committed byte count for this view.
    pub fn snapshot(&self) -> ProgressiveSnapshot {
        self.snapshot
    }

    /// Returns terminal flags, or `None` for a nonterminal view.
    pub fn terminal_flags(&self) -> Option<ProgressiveFlags> {
        self.terminal_flags
    }

    /// Returns the acquire-observed terminal sample header when allocated.
    ///
    /// Active observations always return `None`; header fields are not
    /// authoritative until the producer publishes a terminal state.
    pub fn terminal_header(&self) -> Option<&MetaHeader> {
        self.terminal_header.as_ref()
    }
}

impl Deref for ProgressiveRead<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.bytes
    }
}

struct PayloadLayout {
    pointer: NonNull<u8>,
    stride: i32,
}

fn payload_layout(
    buffer: &mut Buffer<'_>,
    data_index: u32,
    payload_offset: u32,
    payload_size: u32,
) -> Result<PayloadLayout, ProgressiveBufferError> {
    let index = usize::try_from(data_index).map_err(|_| ProgressiveBufferError::InvalidLayout)?;
    let data = buffer
        .datas_mut()
        .get_mut(index)
        .ok_or(ProgressiveBufferError::InvalidLayout)?;
    let raw = data.as_raw();
    let end = payload_offset
        .checked_add(payload_size)
        .ok_or(ProgressiveBufferError::InvalidLayout)?;
    let chunk = data.chunk();
    if raw.data.is_null()
        || end > raw.maxsize
        || chunk.offset() != payload_offset
        || chunk.size() != payload_size
        || chunk
            .flags()
            .intersects(ChunkFlags::EMPTY | ChunkFlags::CORRUPTED)
    {
        return Err(ProgressiveBufferError::InvalidLayout);
    }
    let base = NonNull::new(raw.data.cast::<u8>()).ok_or(ProgressiveBufferError::InvalidLayout)?;
    let pointer = unsafe {
        // SAFETY: the checked end does not exceed the mapped data plane.
        NonNull::new_unchecked(base.as_ptr().add(payload_offset as usize))
    };
    Ok(PayloadLayout {
        pointer,
        stride: chunk.stride(),
    })
}

fn progressive_f32_mut(bytes: &mut [u8]) -> Result<&mut [f32], ProgressiveBufferError> {
    if !cfg!(target_endian = "little")
        || !bytes.len().is_multiple_of(size_of::<f32>())
        || !(bytes.as_ptr() as usize).is_multiple_of(align_of::<f32>())
    {
        return Err(ProgressiveBufferError::InvalidLayout);
    }
    Ok(unsafe {
        // SAFETY: byte length and alignment were checked, the caller owns the
        // complete mutable byte range, and every bit pattern is a valid f32.
        std::slice::from_raw_parts_mut(
            bytes.as_mut_ptr().cast::<f32>(),
            bytes.len() / size_of::<f32>(),
        )
    })
}

fn validate_observation(
    current: ProgressiveSnapshot,
    previous: Option<ProgressiveSnapshot>,
    payload_size: usize,
    commit_granularity: usize,
) -> Result<(), ProgressiveBufferError> {
    let committed = current.committed_bytes() as usize;
    if committed > payload_size
        || matches!(current.state(), ProgressiveState::Prepared)
        || (matches!(current.state(), ProgressiveState::Complete) && committed != payload_size)
        || (matches!(
            current.state(),
            ProgressiveState::Active | ProgressiveState::Aborted
        ) && committed != payload_size
            && !committed.is_multiple_of(commit_granularity))
    {
        return Err(ProgressiveBufferError::InvalidState);
    }
    if let Some(previous) = previous {
        let invalid = committed < previous.committed_bytes() as usize
            || match previous.state() {
                ProgressiveState::Prepared => true,
                ProgressiveState::Active => false,
                ProgressiveState::Complete | ProgressiveState::Aborted => current != previous,
            };
        if invalid {
            return Err(ProgressiveBufferError::InvalidState);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progressive_observations_are_monotonic_and_terminal() {
        let active = ProgressiveSnapshot::new(256, ProgressiveState::Active);
        let complete = ProgressiveSnapshot::new(1024, ProgressiveState::Complete);
        assert_eq!(validate_observation(active, None, 1024, 256), Ok(()));
        assert_eq!(
            validate_observation(complete, Some(active), 1024, 256),
            Ok(())
        );
        assert_eq!(
            validate_observation(active, Some(complete), 1024, 256),
            Err(ProgressiveBufferError::InvalidState)
        );
        assert_eq!(
            validate_observation(
                ProgressiveSnapshot::new(128, ProgressiveState::Active),
                None,
                1024,
                256,
            ),
            Err(ProgressiveBufferError::InvalidState)
        );
    }

    #[test]
    fn f32_view_never_expands_the_unpublished_range() {
        let mut aligned = [0.0_f32; 4];
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), size_of_val(&aligned))
        };
        let values = progressive_f32_mut(&mut bytes[4..12]).unwrap();
        values.copy_from_slice(&[3.0, 4.0]);
        assert_eq!(aligned, [0.0, 3.0, 4.0, 0.0]);
        assert_eq!(
            progressive_f32_mut(&mut bytes[1..5]),
            Err(ProgressiveBufferError::InvalidLayout)
        );
    }
}
