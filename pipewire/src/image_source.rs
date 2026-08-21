// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Fixed-pool image publication backed by PipeWireAO's native image source.

use std::{
    cell::Cell,
    io,
    marker::PhantomData,
    ptr::{self, NonNull},
};

use spa::buffer::{
    meta::{MetaAcquisition, MetaHeaderFlags, ProgressiveFlags},
    ChunkFlags, Data,
};

use crate::stream::Stream;

bitflags::bitflags! {
    /// Metadata and publication capabilities required by an [`ImageSource`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ImageSourceFlags: u32 {
        const REQUIRE_HEADER =
            pw_sys::pw_image_source_flag_PW_IMAGE_SOURCE_FLAG_REQUIRE_HEADER;
        const REQUIRE_ACQUISITION =
            pw_sys::pw_image_source_flag_PW_IMAGE_SOURCE_FLAG_REQUIRE_ACQUISITION;
        const ALLOW_PROGRESSIVE =
            pw_sys::pw_image_source_flag_PW_IMAGE_SOURCE_FLAG_ALLOW_PROGRESSIVE;
    }
}

/// Fixed-pool bounds and metadata requirements for an [`ImageSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSourceConfig {
    pub min_buffers: u32,
    pub max_buffers: u32,
    pub flags: ImageSourceFlags,
}

impl ImageSourceConfig {
    fn as_raw(self) -> pw_sys::pw_image_source_config {
        pw_sys::pw_image_source_config {
            version: pw_sys::PW_VERSION_IMAGE_SOURCE_CONFIG,
            min_buffers: self.min_buffers,
            max_buffers: self.max_buffers,
            flags: self.flags.bits(),
        }
    }
}

/// Producer-supplied payload and observation metadata for one image.
#[derive(Debug, Clone, Copy)]
pub struct ImageFrame<'a> {
    pub data_index: u32,
    pub header_flags: MetaHeaderFlags,
    pub chunk_flags: ChunkFlags,
    pub offset: u32,
    pub size: u32,
    pub stride: i32,
    pub sequence: u64,
    pub pts: i64,
    pub acquisition: Option<&'a MetaAcquisition>,
}

impl ImageFrame<'_> {
    fn as_raw(&self) -> pw_sys::pw_image_frame {
        pw_sys::pw_image_frame {
            version: pw_sys::PW_VERSION_IMAGE_FRAME,
            data_index: self.data_index,
            header_flags: self.header_flags.bits(),
            chunk_flags: self.chunk_flags.bits() as u32,
            offset: self.offset,
            size: self.size,
            stride: self.stride,
            reserved: 0,
            sequence: self.sequence,
            pts: self.pts,
            acquisition: self.acquisition.map_or(ptr::null(), |value| value.as_raw()),
        }
    }
}

/// Initial publication state for a progressive image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageProgressive {
    pub payload_size: u32,
    pub commit_granularity: u32,
    pub committed: u32,
}

impl ImageProgressive {
    fn as_raw(self) -> pw_sys::pw_image_progressive {
        pw_sys::pw_image_progressive {
            version: pw_sys::PW_VERSION_IMAGE_PROGRESSIVE,
            payload_size: self.payload_size,
            commit_granularity: self.commit_granularity,
            committed: self.committed,
        }
    }
}

/// Native image-source slot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageBufferState {
    Unused,
    Available,
    Producer,
    Progressive,
    Published,
}

impl ImageBufferState {
    fn from_raw(raw: pw_sys::pw_image_buffer_state) -> io::Result<Self> {
        match raw {
            pw_sys::pw_image_buffer_state_PW_IMAGE_BUFFER_STATE_UNUSED => Ok(Self::Unused),
            pw_sys::pw_image_buffer_state_PW_IMAGE_BUFFER_STATE_AVAILABLE => Ok(Self::Available),
            pw_sys::pw_image_buffer_state_PW_IMAGE_BUFFER_STATE_PRODUCER => Ok(Self::Producer),
            pw_sys::pw_image_buffer_state_PW_IMAGE_BUFFER_STATE_PROGRESSIVE => {
                Ok(Self::Progressive)
            }
            pw_sys::pw_image_buffer_state_PW_IMAGE_BUFFER_STATE_PUBLISHED => Ok(Self::Published),
            _ => Err(io::Error::from_raw_os_error(libc::EPROTO)),
        }
    }
}

/// Bounded single-writer counters from the native image source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSourceStats {
    pub prepare_calls: u64,
    pub acquire_calls: u64,
    pub available_acquisitions: u64,
    pub reusable_acquisitions: u64,
    pub pool_exhaustions: u64,
    pub forced_reclaims: u64,
    pub producer_returns: u64,
    pub complete_publications: u64,
    pub progressive_started: u64,
    pub progressive_updates: u64,
    pub progressive_completed: u64,
    pub progressive_aborted: u64,
    pub invalid_transitions: u64,
    pub metadata_errors: u64,
    pub teardown_returns: u64,
    pub pool_size: u32,
    pub max_available_probes: u32,
}

impl From<pw_sys::pw_image_source_stats> for ImageSourceStats {
    fn from(raw: pw_sys::pw_image_source_stats) -> Self {
        Self {
            prepare_calls: raw.prepare_calls,
            acquire_calls: raw.acquire_calls,
            available_acquisitions: raw.available_acquisitions,
            reusable_acquisitions: raw.reusable_acquisitions,
            pool_exhaustions: raw.pool_exhaustions,
            forced_reclaims: raw.forced_reclaims,
            producer_returns: raw.producer_returns,
            complete_publications: raw.complete_publications,
            progressive_started: raw.progressive_started,
            progressive_updates: raw.progressive_updates,
            progressive_completed: raw.progressive_completed,
            progressive_aborted: raw.progressive_aborted,
            invalid_transitions: raw.invalid_transitions,
            metadata_errors: raw.metadata_errors,
            teardown_returns: raw.teardown_returns,
            pool_size: raw.pool_size,
            max_available_probes: raw.max_available_probes,
        }
    }
}

/// One native PipeWireAO image source tied to a stream's lifetime.
pub struct ImageSource<'stream> {
    ptr: NonNull<pw_sys::pw_image_source>,
    stream: PhantomData<&'stream Stream>,
    not_sync: PhantomData<Cell<()>>,
}

impl Stream {
    /// Create a native image source for this output stream.
    pub fn image_source(&self, config: ImageSourceConfig) -> io::Result<ImageSource<'_>> {
        let raw_config = config.as_raw();
        let source = unsafe { pw_sys::pw_image_source_new(self.as_raw_ptr(), &raw_config) };
        let ptr = NonNull::new(source).ok_or_else(io::Error::last_os_error)?;
        Ok(ImageSource {
            ptr,
            stream: PhantomData,
            not_sync: PhantomData,
        })
    }
}

impl ImageSource<'_> {
    /// Claim the negotiated pool and its exclusive latest-buffer worker.
    pub fn prepare(&mut self) -> io::Result<u32> {
        let result = unsafe { pw_sys::pw_image_source_prepare(self.ptr.as_ptr()) };
        if result <= 0 {
            return Err(io::Error::from_raw_os_error(if result < 0 {
                -result
            } else {
                libc::EPROTO
            }));
        }
        Ok(result as u32)
    }

    /// Return local slots and release exclusive latest-buffer ownership.
    pub fn teardown(&mut self) -> io::Result<()> {
        result(unsafe { pw_sys::pw_image_source_teardown(self.ptr.as_ptr()) })?;
        Ok(())
    }

    pub fn pool_size(&self) -> u32 {
        unsafe { pw_sys::pw_image_source_get_n_buffers(self.ptr.as_ptr()) }
    }

    /// Acquire one producer slot without withdrawing a visible publication.
    pub fn try_acquire(&mut self) -> io::Result<Option<ImageBuffer<'_>>> {
        self.acquire_with(pw_sys::pw_image_source_try_acquire)
    }

    /// Explicitly withdraw one visible unclaimed publication under lossy policy.
    pub fn try_reclaim(&mut self) -> io::Result<Option<ImageBuffer<'_>>> {
        self.acquire_with(pw_sys::pw_image_source_try_reclaim)
    }

    fn acquire_with(
        &mut self,
        operation: unsafe extern "C" fn(
            *mut pw_sys::pw_image_source,
            *mut *mut pw_sys::pw_image_buffer,
        ) -> i32,
    ) -> io::Result<Option<ImageBuffer<'_>>> {
        let mut buffer = ptr::null_mut();
        let result = unsafe { operation(self.ptr.as_ptr(), &mut buffer) };
        match result {
            0 => Ok(None),
            1 => NonNull::new(buffer)
                .map(|buffer| {
                    Some(ImageBuffer {
                        source: self.ptr,
                        buffer,
                        owned: true,
                        lifetime: PhantomData,
                    })
                })
                .ok_or_else(|| io::Error::from_raw_os_error(libc::EPROTO)),
            value if value < 0 => Err(io::Error::from_raw_os_error(-value)),
            _ => Err(io::Error::from_raw_os_error(libc::EPROTO)),
        }
    }

    pub fn stats(&self) -> io::Result<ImageSourceStats> {
        let mut raw = std::mem::MaybeUninit::<pw_sys::pw_image_source_stats>::uninit();
        result(unsafe {
            pw_sys::pw_image_source_get_stats(
                self.ptr.as_ptr(),
                raw.as_mut_ptr(),
                std::mem::size_of::<pw_sys::pw_image_source_stats>(),
            )
        })?;
        Ok(unsafe { raw.assume_init() }.into())
    }
}

impl Drop for ImageSource<'_> {
    fn drop(&mut self) {
        unsafe { pw_sys::pw_image_source_destroy(self.ptr.as_ptr()) }
    }
}

// SAFETY: the native API explicitly permits transferring exclusive source
// ownership to one application worker. It remains deliberately !Sync.
unsafe impl Send for ImageSource<'_> {}

/// One producer-owned image slot. Dropping it returns an unpublished slot.
pub struct ImageBuffer<'source> {
    source: NonNull<pw_sys::pw_image_source>,
    buffer: NonNull<pw_sys::pw_image_buffer>,
    owned: bool,
    lifetime: PhantomData<&'source mut pw_sys::pw_image_source>,
}

impl<'source> ImageBuffer<'source> {
    pub fn index(&self) -> u32 {
        unsafe { pw_sys::pw_image_buffer_get_index(self.buffer.as_ptr()) }
    }

    pub fn state(&self) -> io::Result<ImageBufferState> {
        ImageBufferState::from_raw(unsafe {
            pw_sys::pw_image_buffer_get_state(self.buffer.as_ptr())
        })
    }

    /// Access the mapped SPA data descriptors owned by this producer lease.
    pub fn datas_mut(&mut self) -> &mut [Data] {
        let pw_buffer = unsafe { pw_sys::pw_image_buffer_get_pw_buffer(self.buffer.as_ptr()) };
        if pw_buffer.is_null() {
            return &mut [];
        }
        let spa_buffer = unsafe { (*pw_buffer).buffer };
        if spa_buffer.is_null()
            || unsafe { (*spa_buffer).n_datas == 0 || (*spa_buffer).datas.is_null() }
        {
            return &mut [];
        }
        unsafe {
            std::slice::from_raw_parts_mut(
                (*spa_buffer).datas.cast::<Data>(),
                (*spa_buffer).n_datas as usize,
            )
        }
    }

    /// Publish this slot as one terminal complete image.
    pub fn publish_complete(mut self, frame: &ImageFrame<'_>) -> io::Result<()> {
        let raw = frame.as_raw();
        result(unsafe {
            pw_sys::pw_image_source_publish_complete(
                self.source.as_ptr(),
                self.buffer.as_ptr(),
                &raw,
            )
        })?;
        self.owned = false;
        Ok(())
    }

    /// Announce this mapped host-memory slot while its producer is writing.
    pub fn begin_progressive(
        mut self,
        frame: &ImageFrame<'_>,
        progressive: ImageProgressive,
    ) -> io::Result<ProgressiveImageBuffer<'source>> {
        let raw_frame = frame.as_raw();
        let raw_progressive = progressive.as_raw();
        result(unsafe {
            pw_sys::pw_image_source_begin_progressive(
                self.source.as_ptr(),
                self.buffer.as_ptr(),
                &raw_frame,
                &raw_progressive,
            )
        })?;
        self.owned = false;
        Ok(ProgressiveImageBuffer {
            source: self.source,
            buffer: self.buffer,
            committed: progressive.committed,
            active: true,
            lifetime: PhantomData,
        })
    }
}

impl Drop for ImageBuffer<'_> {
    fn drop(&mut self) {
        if self.owned {
            let result = unsafe {
                pw_sys::pw_image_source_return_buffer(self.source.as_ptr(), self.buffer.as_ptr())
            };
            debug_assert_eq!(result, 0, "producer image slot could not be returned");
        }
    }
}

/// Active progressive producer lease. Dropping it publishes cancellation.
pub struct ProgressiveImageBuffer<'source> {
    source: NonNull<pw_sys::pw_image_source>,
    buffer: NonNull<pw_sys::pw_image_buffer>,
    committed: u32,
    active: bool,
    lifetime: PhantomData<&'source mut pw_sys::pw_image_source>,
}

impl ProgressiveImageBuffer<'_> {
    /// Access mapped SPA data while retaining the progressive producer lease.
    pub fn datas_mut(&mut self) -> &mut [Data] {
        let pw_buffer = unsafe { pw_sys::pw_image_buffer_get_pw_buffer(self.buffer.as_ptr()) };
        if pw_buffer.is_null() {
            return &mut [];
        }
        let spa_buffer = unsafe { (*pw_buffer).buffer };
        if spa_buffer.is_null()
            || unsafe { (*spa_buffer).n_datas == 0 || (*spa_buffer).datas.is_null() }
        {
            return &mut [];
        }
        unsafe {
            std::slice::from_raw_parts_mut(
                (*spa_buffer).datas.cast::<Data>(),
                (*spa_buffer).n_datas as usize,
            )
        }
    }

    /// Release-publish a larger immutable prefix.
    pub fn update(&mut self, committed: u32) -> io::Result<bool> {
        let result = unsafe {
            pw_sys::pw_image_source_update_progressive(
                self.source.as_ptr(),
                self.buffer.as_ptr(),
                committed,
            )
        };
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }
        self.committed = committed;
        Ok(result == 1)
    }

    /// Finish a fully committed image successfully.
    pub fn complete(mut self, committed: u32) -> io::Result<()> {
        result(unsafe {
            pw_sys::pw_image_source_finish_progressive(
                self.source.as_ptr(),
                self.buffer.as_ptr(),
                committed,
                spa_sys::SPA_META_PROGRESSIVE_STATE_COMPLETE,
                0,
            )
        })?;
        self.active = false;
        Ok(())
    }

    /// Finish an image as aborted with explicit terminal flags.
    pub fn abort(mut self, committed: u32, flags: ProgressiveFlags) -> io::Result<()> {
        result(unsafe {
            pw_sys::pw_image_source_finish_progressive(
                self.source.as_ptr(),
                self.buffer.as_ptr(),
                committed,
                spa_sys::SPA_META_PROGRESSIVE_STATE_ABORTED,
                flags.bits(),
            )
        })?;
        self.active = false;
        Ok(())
    }
}

impl Drop for ProgressiveImageBuffer<'_> {
    fn drop(&mut self) {
        if self.active {
            let result = unsafe {
                pw_sys::pw_image_source_finish_progressive(
                    self.source.as_ptr(),
                    self.buffer.as_ptr(),
                    self.committed,
                    spa_sys::SPA_META_PROGRESSIVE_STATE_ABORTED,
                    ProgressiveFlags::CANCELLED.bits(),
                )
            };
            debug_assert_eq!(result, 0, "progressive image slot could not be cancelled");
        }
    }
}

fn result(value: i32) -> io::Result<()> {
    if value < 0 {
        Err(io::Error::from_raw_os_error(-value))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_preserves_pool_and_capability_contract() {
        let raw = ImageSourceConfig {
            min_buffers: 2,
            max_buffers: 8,
            flags: ImageSourceFlags::REQUIRE_HEADER
                | ImageSourceFlags::REQUIRE_ACQUISITION
                | ImageSourceFlags::ALLOW_PROGRESSIVE,
        }
        .as_raw();
        assert_eq!(raw.version, pw_sys::PW_VERSION_IMAGE_SOURCE_CONFIG);
        assert_eq!(raw.min_buffers, 2);
        assert_eq!(raw.max_buffers, 8);
        assert_eq!(
            raw.flags,
            pw_sys::pw_image_source_flag_PW_IMAGE_SOURCE_FLAG_ALL
        );
    }

    #[test]
    fn frame_passes_valid_acquisition_by_reference() {
        let acquisition = MetaAcquisition::new();
        let frame = ImageFrame {
            data_index: 0,
            header_flags: MetaHeaderFlags::DISCONT,
            chunk_flags: ChunkFlags::empty(),
            offset: 16,
            size: 1024,
            stride: 64,
            sequence: 7,
            pts: 42,
            acquisition: Some(&acquisition),
        };
        let raw = frame.as_raw();
        assert_eq!(raw.version, pw_sys::PW_VERSION_IMAGE_FRAME);
        assert_eq!(raw.header_flags, MetaHeaderFlags::DISCONT.bits());
        assert_eq!(raw.acquisition, acquisition.as_raw());
        assert_eq!(raw.reserved, 0);
    }

    #[test]
    fn progressive_descriptor_preserves_prefix_contract() {
        let raw = ImageProgressive {
            payload_size: 4096,
            commit_granularity: 256,
            committed: 512,
        }
        .as_raw();
        assert_eq!(raw.version, pw_sys::PW_VERSION_IMAGE_PROGRESSIVE);
        assert_eq!(raw.payload_size, 4096);
        assert_eq!(raw.commit_granularity, 256);
        assert_eq!(raw.committed, 512);
    }
}
