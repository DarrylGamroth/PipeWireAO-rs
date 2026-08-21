use super::stream::Stream;

use spa::buffer::meta::Metadata;
use spa::buffer::Data;
use spa::utils::result::SpaResult;
use std::cell::Cell;
use std::convert::TryFrom;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use crate::filter::FilterPortRegistration;
use crate::Error;

pub mod ndarray;
mod progressive;

pub use progressive::{
    ProgressiveBufferError, ProgressiveInput, ProgressiveOutputBuffer, ProgressiveRead,
    ProgressiveWrite,
};

pub struct Buffer<'s> {
    buf: NonNull<pw_sys::pw_buffer>,

    /// Nonzero only when the specialized latest-input API returned the
    /// transport submission sequence with this buffer.
    submission_sequence: u64,

    /// In PipeWire, buffers are owned by the stream or filter port that
    /// generated them. The lifetime ensures that owner remains valid until
    /// the buffer is returned.
    owner: BufferOwner<'s>,
}

enum BufferOwner<'a> {
    Stream(&'a Stream),
    FilterPort(NonNull<std::ffi::c_void>, std::marker::PhantomData<&'a ()>),
    FilterPortRc(Arc<FilterPortRegistration>),
}

impl<'s> Buffer<'s> {
    pub(crate) unsafe fn from_raw(
        buf: *mut pw_sys::pw_buffer,
        stream: &Stream,
    ) -> Option<Buffer<'_>> {
        NonNull::new(buf).map(|buf| Buffer {
            buf,
            submission_sequence: 0,
            owner: BufferOwner::Stream(stream),
        })
    }

    pub(crate) unsafe fn from_filter_raw<'a>(
        buf: *mut pw_sys::pw_buffer,
        port_data: NonNull<std::ffi::c_void>,
    ) -> Option<Buffer<'a>> {
        NonNull::new(buf).map(|buf| Buffer {
            buf,
            submission_sequence: 0,
            owner: BufferOwner::FilterPort(port_data, std::marker::PhantomData),
        })
    }

    pub(crate) unsafe fn from_filter_latest_raw<'a>(
        buf: NonNull<pw_sys::pw_buffer>,
        port_data: NonNull<std::ffi::c_void>,
        submission_sequence: std::num::NonZeroU64,
    ) -> Buffer<'a> {
        Buffer {
            buf,
            submission_sequence: submission_sequence.get(),
            owner: BufferOwner::FilterPort(port_data, std::marker::PhantomData),
        }
    }

    pub(crate) unsafe fn from_filter_rc_raw(
        buf: *mut pw_sys::pw_buffer,
        registration: Arc<FilterPortRegistration>,
    ) -> Option<Buffer<'static>> {
        NonNull::new(buf).map(|buf| Buffer {
            buf,
            submission_sequence: 0,
            owner: BufferOwner::FilterPortRc(registration),
        })
    }

    /// Returns the transport sequence for a specialized latest-input dequeue.
    ///
    /// The sequence is publisher-local, monotonically increasing, and shared
    /// by every subscriber that receives the same fan-out publication. It is
    /// distinct from acquisition metadata used for semantic joins. Ordinary
    /// stream and filter dequeues return `None` because those APIs do not
    /// expose a latest-channel submission sequence.
    pub fn submission_sequence(&self) -> Option<std::num::NonZeroU64> {
        std::num::NonZeroU64::new(self.submission_sequence)
    }

    /// Retains a filter-port buffer beyond the callback that dequeued it.
    ///
    /// The returned guard keeps exclusive ownership of the mapped SPA buffer
    /// and returns that buffer to its filter port when dropped. It deliberately
    /// does not implement `Send`: PipeWire documents filter dequeue and queue
    /// as RT-safe, but does not give this general wrapper a cross-thread queue
    /// ownership contract. Stream buffers cannot be retained with this method.
    pub fn retain_filter(self) -> Result<RetainedFilterBuffer<'s>, Self> {
        if matches!(&self.owner, BufferOwner::FilterPort(_, _)) {
            Ok(RetainedFilterBuffer(self, PhantomData))
        } else {
            Err(self)
        }
    }

    /// Announces this filter output while retaining its progressive writer
    /// lease.
    ///
    /// The caller must initialize the negotiated progressive metadata to its
    /// active state before this operation. On success, the ordinary buffer
    /// interface is hidden because whole-payload access is not safe while a
    /// consumer may observe committed ranges. Dropping the returned guard ends
    /// the producer lease exactly once; the producer must publish its terminal
    /// state before doing so.
    ///
    /// This operation succeeds only for an output buffer dequeued from a
    /// graph-independent latest-buffer filter port. On failure, the error
    /// retains the buffer so the caller may recover it with
    /// [`BeginProgressiveBufferError::into_buffer`].
    pub fn begin_progressive(
        self,
    ) -> Result<ProgressiveFilterBuffer<'s>, BeginProgressiveBufferError<'s>> {
        let port_data = match &self.owner {
            BufferOwner::FilterPort(port_data, _) => *port_data,
            _ => {
                return Err(BeginProgressiveBufferError {
                    kind: BeginProgressiveBufferErrorKind::WrongOwner,
                    buffer: self,
                });
            }
        };
        let result = unsafe {
            pw_sys::pw_filter_begin_progressive_buffer(port_data.as_ptr(), self.buf.as_ptr())
        };
        if let Err(error) = SpaResult::from_c(result).into_result() {
            return Err(BeginProgressiveBufferError {
                kind: BeginProgressiveBufferErrorKind::PipeWire(error.into()),
                buffer: self,
            });
        }

        Ok(ProgressiveFilterBuffer {
            buffer: ManuallyDrop::new(self),
            port_data,
            not_sync: PhantomData,
        })
    }

    pub fn datas_mut(&mut self) -> &mut [Data] {
        let buffer: *mut spa_sys::spa_buffer = unsafe { self.buf.as_ref().buffer };

        let slice_of_data = if !buffer.is_null()
            && unsafe { (*buffer).n_datas > 0 && !(*buffer).datas.is_null() }
        {
            unsafe {
                let datas = (*buffer).datas as *mut Data;
                std::slice::from_raw_parts_mut(datas, usize::try_from((*buffer).n_datas).unwrap())
            }
        } else {
            &mut []
        };

        slice_of_data
    }

    pub fn find_meta<T>(&self) -> Option<&T>
    where
        T: Metadata,
    {
        let buffer: *mut spa_sys::spa_buffer = unsafe { self.buf.as_ref().buffer };
        if !buffer.is_null() && unsafe { (*buffer).n_metas != 0 } {
            unsafe {
                match T::META_TYPE {
                    spa_sys::SPA_META_VideoDamage => {
                        let meta_data =
                            spa_sys::spa_buffer_find_meta(buffer, T::META_TYPE) as *const T;
                        if !meta_data.is_null() {
                            return Some(&*meta_data);
                        }
                    }
                    _ => {
                        let meta_data = spa_sys::spa_buffer_find_meta_data(
                            buffer,
                            T::META_TYPE,
                            std::mem::size_of::<T>(),
                        ) as *const T;
                        if !meta_data.is_null() {
                            return Some(&*meta_data);
                        }
                    }
                }
            }
        }
        None
    }

    /// Finds mutable metadata of type `T` attached to this buffer.
    pub fn find_meta_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Metadata,
    {
        let buffer: *mut spa_sys::spa_buffer = unsafe { self.buf.as_ref().buffer };
        if !buffer.is_null() && unsafe { (*buffer).n_metas != 0 } {
            unsafe {
                let meta_data = match T::META_TYPE {
                    spa_sys::SPA_META_VideoDamage => {
                        spa_sys::spa_buffer_find_meta(buffer, T::META_TYPE) as *mut T
                    }
                    _ => spa_sys::spa_buffer_find_meta_data(
                        buffer,
                        T::META_TYPE,
                        std::mem::size_of::<T>(),
                    ) as *mut T,
                };
                if !meta_data.is_null() {
                    return Some(&mut *meta_data);
                }
            }
        }
        None
    }

    #[cfg(feature = "v0_3_49")]
    pub fn requested(&self) -> u64 {
        unsafe { self.buf.as_ref().requested }
    }
}

/// Error returned when a buffer cannot begin a progressive producer lease.
pub struct BeginProgressiveBufferError<'f> {
    kind: BeginProgressiveBufferErrorKind,
    buffer: Buffer<'f>,
}

#[derive(Debug)]
enum BeginProgressiveBufferErrorKind {
    WrongOwner,
    PipeWire(Error),
}

impl<'f> BeginProgressiveBufferError<'f> {
    /// Returns the underlying PipeWire error, when the operation reached
    /// PipeWire.
    pub fn pipewire_error(&self) -> Option<&Error> {
        match &self.kind {
            BeginProgressiveBufferErrorKind::WrongOwner => None,
            BeginProgressiveBufferErrorKind::PipeWire(error) => Some(error),
        }
    }

    /// Recovers the buffer after a failed progressive announcement.
    pub fn into_buffer(self) -> Buffer<'f> {
        self.buffer
    }
}

impl std::fmt::Debug for BeginProgressiveBufferError<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BeginProgressiveBufferError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for BeginProgressiveBufferError<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            BeginProgressiveBufferErrorKind::WrongOwner => {
                formatter.write_str("buffer is not owned by a filter port")
            }
            BeginProgressiveBufferErrorKind::PipeWire(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BeginProgressiveBufferError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.pipewire_error()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

/// An announced progressive filter output with an active producer lease.
///
/// This guard deliberately does not expose the ordinary safe [`Buffer`] API.
/// The producer and consumer may own disjoint payload ranges concurrently, so
/// a protocol-aware wrapper must use [`Self::buffer_mut_unchecked`] to create
/// only references within the producer-owned suffix. Dropping the guard ends
/// the producer lease; it does not publish application metadata on the
/// producer's behalf.
pub struct ProgressiveFilterBuffer<'f> {
    buffer: ManuallyDrop<Buffer<'f>>,
    port_data: NonNull<std::ffi::c_void>,
    not_sync: PhantomData<Cell<()>>,
}

impl<'f> ProgressiveFilterBuffer<'f> {
    /// Borrows the underlying buffer for protocol-aware, range-limited access.
    ///
    /// # Safety
    ///
    /// The caller must not construct any reference spanning payload bytes
    /// concurrently owned by the consumer. Payload access must remain within
    /// the producer-owned uncommitted suffix, and metadata access must follow
    /// the negotiated progressive protocol.
    pub unsafe fn buffer_mut_unchecked(&mut self) -> &mut Buffer<'f> {
        &mut self.buffer
    }

    /// Returns the underlying PipeWire buffer pointer without granting payload
    /// access.
    pub fn as_raw_ptr(&self) -> *mut pw_sys::pw_buffer {
        self.buffer.buf.as_ptr()
    }
}

impl Drop for ProgressiveFilterBuffer<'_> {
    fn drop(&mut self) {
        let result = unsafe {
            pw_sys::pw_filter_end_progressive_buffer(
                self.port_data.as_ptr(),
                self.buffer.buf.as_ptr(),
            )
        };
        debug_assert_eq!(result, 0, "progressive producer lease ended twice");
    }
}

// SAFETY: construction verifies the filter-port owner and the C operation
// accepts only an output latest-buffer port. The lifetime retains the exclusive
// port borrow that produced the buffer, and this guard exposes no Sync access.
unsafe impl Send for ProgressiveFilterBuffer<'_> {}

/// An exclusively owned filter-port buffer retained beyond one callback.
///
/// PipeWire documents filter dequeue and queue operations as RT-safe. The
/// buffer remains owned by the caller between those operations, and the
/// filter port must outlive this guard. Dropping the guard queues the buffer
/// back to the same port. Cross-thread transfer requires a narrower owner that
/// can prove the producer/consumer topology for that particular port.
///
/// ```compile_fail
/// fn require_send<T: Send>() {}
/// require_send::<pipewire::buffer::RetainedFilterBuffer<'static>>();
/// ```
pub struct RetainedFilterBuffer<'f>(Buffer<'f>, PhantomData<Rc<()>>);

impl<'f> Deref for RetainedFilterBuffer<'f> {
    type Target = Buffer<'f>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RetainedFilterBuffer<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// An exclusively owned buffer that retains its filter-port registration.
///
/// This guard does not implement `Send`. Cross-thread transfer requires a
/// narrower owner that proves a single-producer/single-consumer topology and
/// returns the buffer before filter teardown.
pub struct RetainedFilterBufferRc(Buffer<'static>, PhantomData<Rc<()>>);

impl RetainedFilterBufferRc {
    pub(crate) fn from_buffer(buffer: Buffer<'static>) -> Self {
        Self(buffer, PhantomData)
    }
}

impl Deref for RetainedFilterBufferRc {
    type Target = Buffer<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RetainedFilterBufferRc {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Buffer<'_> {
    fn drop(&mut self) {
        unsafe {
            match &self.owner {
                BufferOwner::Stream(stream) => stream.queue_raw_buffer(self.buf.as_ptr()),
                BufferOwner::FilterPort(port_data, _) => {
                    pw_sys::pw_filter_queue_buffer(port_data.as_ptr(), self.buf.as_ptr());
                }
                BufferOwner::FilterPortRc(registration) => {
                    pw_sys::pw_filter_queue_buffer(
                        registration.port_data.as_ptr(),
                        self.buf.as_ptr(),
                    );
                }
            }
        }
    }
}
