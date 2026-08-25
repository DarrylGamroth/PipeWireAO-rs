use super::stream::Stream;

use spa::buffer::meta::Metadata;
use spa::buffer::Data;
use std::convert::TryFrom;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use crate::filter::FilterPortRegistration;

pub mod ndarray;

pub struct Buffer<'s> {
    buf: NonNull<pw_sys::pw_buffer>,

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
            owner: BufferOwner::Stream(stream),
        })
    }

    pub(crate) unsafe fn from_filter_raw<'a>(
        buf: *mut pw_sys::pw_buffer,
        port_data: NonNull<std::ffi::c_void>,
    ) -> Option<Buffer<'a>> {
        NonNull::new(buf).map(|buf| Buffer {
            buf,
            owner: BufferOwner::FilterPort(port_data, std::marker::PhantomData),
        })
    }

    pub(crate) unsafe fn from_filter_rc_raw(
        buf: *mut pw_sys::pw_buffer,
        registration: Arc<FilterPortRegistration>,
    ) -> Option<Buffer<'static>> {
        NonNull::new(buf).map(|buf| Buffer {
            buf,
            owner: BufferOwner::FilterPortRc(registration),
        })
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
