// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::{
    ffi,
    ffi::{CStr, CString},
    ops::Deref,
    pin::Pin,
    ptr,
    rc::{Rc, Weak},
    sync::Arc,
    time::Duration,
};

use crate::{
    buffer::{Buffer, RetainedFilterBufferRc},
    core::CoreRc,
    properties::PropertiesBox,
    Error,
};
use spa::utils::result::SpaResult;

use super::{
    Filter, FilterBox, FilterPortFlags, FilterPortRef, FilterState, ListenerLocalCallbacks,
};

#[derive(Debug)]
struct FilterRcInner {
    filter: FilterBox<'static>,
    _core: CoreRc,
}

/// Reference-counted smart pointer providing shared ownership of a PipeWire
/// [filter](super).
#[derive(Clone, Debug)]
pub struct FilterRc {
    inner: Rc<FilterRcInner>,
}

impl FilterRc {
    /// Creates an unconnected filter while retaining its core.
    pub fn new(core: CoreRc, name: &str, properties: PropertiesBox) -> Result<Self, Error> {
        let name = CString::new(name).expect("Invalid byte in filter name");
        Self::new_cstr(core, &name, properties)
    }

    /// Creates an unconnected filter with a C string name.
    pub fn new_cstr(core: CoreRc, name: &CStr, properties: PropertiesBox) -> Result<Self, Error> {
        unsafe {
            let filter =
                pw_sys::pw_filter_new(core.as_raw_ptr(), name.as_ptr(), properties.into_raw());
            let filter = ptr::NonNull::new(filter).ok_or(Error::CreationFailed)?;
            let filter = FilterBox::from_raw(filter);
            Ok(Self {
                inner: Rc::new(FilterRcInner {
                    filter,
                    _core: core,
                }),
            })
        }
    }

    /// Creates a weak reference to this filter.
    pub fn downgrade(&self) -> FilterWeak {
        FilterWeak {
            weak: Rc::downgrade(&self.inner),
        }
    }

    /// Returns an owned RT trigger that retains this filter.
    #[cfg(feature = "v0_3_77")]
    pub fn trigger_handle_rc(&self) -> FilterTriggerRc {
        FilterTriggerRc {
            filter: ptr::NonNull::new(self.as_raw_ptr()).expect("filter cannot be null"),
            _owner: self.clone(),
        }
    }

    /// Registers a timer on this filter's assigned data loop while retaining
    /// the filter for the complete timer lifetime.
    #[cfg(feature = "v1_2_0")]
    pub fn add_timer_from_thread_rc<F>(&self, callback: F) -> Result<FilterTimerRc, Error>
    where
        F: Fn(u64) + Send + 'static,
    {
        let data_loop = self.data_loop().ok_or(Error::CreationFailed)?;
        let source = data_loop.add_timer_from_thread(callback)?;
        // SAFETY: `FilterTimerRc` retains this filter, which retains the
        // assigned PipeWire data loop. Its source field is dropped first.
        let source = unsafe {
            std::mem::transmute::<crate::loop_::TimerSource<'_>, crate::loop_::TimerSource<'static>>(
                source,
            )
        };
        Ok(FilterTimerRc {
            source,
            _owner: self.clone(),
        })
    }

    /// Creates an owned listener builder that retains this filter.
    ///
    /// Callback data may own [`FilterPortRc`] values, allowing the registered
    /// listener to be retained in a reusable node value without self-references.
    #[must_use = "Use the builder to register event callbacks"]
    pub fn add_local_listener_with_user_data<'a, D: 'a>(
        &self,
        user_data: D,
    ) -> ListenerLocalRcBuilder<'a, D> {
        let mut callbacks = ListenerLocalCallbacks::with_user_data(user_data);
        callbacks.filter = ptr::NonNull::new(self.as_raw_ptr());
        ListenerLocalRcBuilder {
            filter: self.clone(),
            callbacks,
        }
    }

    /// Creates an owned listener builder with default callback data.
    #[must_use = "Use the builder to register event callbacks"]
    pub fn add_local_listener<'a, D: Default + 'a>(&self) -> ListenerLocalRcBuilder<'a, D> {
        self.add_local_listener_with_user_data(D::default())
    }

    /// Adds an owned port whose registration retains this filter.
    pub fn add_port_with_user_data<D>(
        &self,
        direction: spa::utils::Direction,
        flags: FilterPortFlags,
        properties: PropertiesBox,
        params: &mut [&spa::pod::Pod],
        user_data: D,
    ) -> Result<FilterPortRc<D>, Error> {
        let port_data = self.add_port_raw(direction, flags, properties, params)?;
        // The atomic count supports the explicitly non-Send retained-buffer
        // handoff. The registration itself remains thread-affine because it
        // contains `FilterRc`; only a narrower owner may transfer its guard.
        #[allow(clippy::arc_with_non_send_sync)]
        let registration = Arc::new(FilterPortRegistration {
            filter: self.clone(),
            port_data,
        });
        Ok(FilterPortRc {
            registration,
            user_data,
        })
    }

    /// Adds an owned port without application-specific port data.
    pub fn add_port(
        &self,
        direction: spa::utils::Direction,
        flags: FilterPortFlags,
        properties: PropertiesBox,
        params: &mut [&spa::pod::Pod],
    ) -> Result<FilterPortRc<()>, Error> {
        self.add_port_with_user_data(direction, flags, properties, params, ())
    }
}

/// Owned handle that requests processing while retaining its filter.
#[cfg(feature = "v0_3_77")]
pub struct FilterTriggerRc {
    filter: ptr::NonNull<pw_sys::pw_filter>,
    _owner: FilterRc,
}

#[cfg(feature = "v0_3_77")]
impl FilterTriggerRc {
    /// Requests one graph iteration for the retained trigger filter.
    pub fn trigger_process(&self) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_trigger_process(self.filter.as_ptr()) };
        SpaResult::from_c(result).into_result()?;
        Ok(())
    }
}

// SAFETY: the Rc owner is retained but never accessed or cloned by the data
// thread. The handle exposes only PipeWire's documented RT-safe trigger call
// and is dropped with its listener on the control thread.
#[cfg(feature = "v0_3_77")]
unsafe impl Send for FilterTriggerRc {}

/// Owned timer registered on a retained filter's assigned data loop.
#[cfg(feature = "v1_2_0")]
pub struct FilterTimerRc {
    source: crate::loop_::TimerSource<'static>,
    _owner: FilterRc,
}

#[cfg(feature = "v1_2_0")]
impl FilterTimerRc {
    /// Arms, rearms, or disables this timer.
    pub fn update_timer(&self, value: Option<Duration>, interval: Option<Duration>) -> SpaResult {
        self.source.update_timer(value, interval)
    }
}

impl Deref for FilterRc {
    type Target = Filter;

    fn deref(&self) -> &Self::Target {
        self.inner.filter.deref()
    }
}

impl AsRef<Filter> for FilterRc {
    fn as_ref(&self) -> &Filter {
        self.deref()
    }
}

/// Non-owning reference to a filter managed by [`FilterRc`].
pub struct FilterWeak {
    weak: Weak<FilterRcInner>,
}

impl FilterWeak {
    /// Attempts to upgrade this weak reference.
    pub fn upgrade(&self) -> Option<FilterRc> {
        self.weak.upgrade().map(|inner| FilterRc { inner })
    }
}

/// An owned filter-port registration that retains its shared filter.
///
/// This handle exposes only port operations. It deliberately does not expose
/// or clone the retained [`FilterRc`] from a process callback.
pub struct FilterPortRc<D = ()> {
    pub(super) registration: Arc<FilterPortRegistration>,
    user_data: D,
}

pub(crate) struct FilterPortRegistration {
    pub(crate) filter: FilterRc,
    pub(crate) port_data: ptr::NonNull<ffi::c_void>,
}

impl Drop for FilterPortRegistration {
    fn drop(&mut self) {
        unsafe {
            pw_sys::pw_filter_remove_port(self.port_data.as_ptr());
        }
    }
}

impl<D> FilterPortRc<D> {
    /// Returns the application data stored with this port handle.
    pub fn user_data(&self) -> &D {
        &self.user_data
    }

    /// Returns mutable application data stored with this port handle.
    pub fn user_data_mut(&mut self) -> &mut D {
        &mut self.user_data
    }

    /// Returns an opaque borrowed port reference suitable for identity checks.
    pub fn as_ref(&self) -> FilterPortRef<'_> {
        FilterPortRef {
            port_data: self.registration.port_data,
            filter: ptr::NonNull::new(self.registration.filter.as_raw_ptr())
                .expect("filter cannot be null"),
            lifetime: std::marker::PhantomData,
        }
    }

    /// Takes an available buffer from this port.
    pub fn dequeue_buffer(&self) -> Option<Buffer<'_>> {
        unsafe { Buffer::from_filter_raw(self.dequeue_raw_buffer(), self.registration.port_data) }
    }

    /// Takes an available buffer while retaining its owned port registration.
    ///
    /// The returned guard is intended for a bounded handoff that returns the
    /// buffer after the callback. It does not implement `Send`; a narrower
    /// application wrapper must prove any cross-thread topology.
    pub fn dequeue_retained_buffer(&self) -> Option<RetainedFilterBufferRc> {
        unsafe {
            Buffer::from_filter_rc_raw(self.dequeue_raw_buffer(), Arc::clone(&self.registration))
                .map(RetainedFilterBufferRc::from_buffer)
        }
    }

    /// Takes an available raw buffer from this port.
    ///
    /// # Safety
    ///
    /// The returned buffer, when non-null, must be queued back to this port.
    pub unsafe fn dequeue_raw_buffer(&self) -> *mut pw_sys::pw_buffer {
        pw_sys::pw_filter_dequeue_buffer(self.registration.port_data.as_ptr())
    }

    /// Queues a raw buffer back to this port.
    ///
    /// # Safety
    ///
    /// `buffer` must have been dequeued from this port and not already queued.
    pub unsafe fn queue_raw_buffer(&self, buffer: *mut pw_sys::pw_buffer) -> Result<(), Error> {
        let result = pw_sys::pw_filter_queue_buffer(self.registration.port_data.as_ptr(), buffer);
        SpaResult::from_c(result).into_result()?;
        Ok(())
    }

    /// Gets the port's DSP buffer pointer.
    ///
    /// # Safety
    ///
    /// The caller must use the negotiated DSP format and `n_samples` to access
    /// the returned memory with the correct type and extent.
    pub unsafe fn dsp_buffer(&self, n_samples: u32) -> *mut ffi::c_void {
        pw_sys::pw_filter_get_dsp_buffer(self.registration.port_data.as_ptr(), n_samples)
    }
}

// SAFETY: the retained Rc is never exposed or mutated from a callback. Shared
// access is limited to PipeWire's documented RT-safe port operations and
// immutable pointer identity. Listener removal precedes callback-data drop.
unsafe impl<D: Sync> Sync for FilterPortRc<D> {}

/// Builder for local callbacks that retains a shared filter.
pub struct ListenerLocalRcBuilder<'a, D> {
    filter: FilterRc,
    callbacks: ListenerLocalCallbacks<'a, D>,
}

impl<'a, D> ListenerLocalRcBuilder<'a, D> {
    #[must_use = "Call `.register()` to start receiving events"]
    pub fn state_changed<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, FilterState, FilterState) + Send + 'a,
    {
        *self.callbacks.state_changed.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn io_changed<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, Option<FilterPortRef<'_>>, u32, *mut ffi::c_void, u32) + Send + 'a,
    {
        *self.callbacks.io_changed.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn param_changed<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, Option<FilterPortRef<'_>>, u32, Option<&spa::pod::Pod>) + Send + 'a,
    {
        *self.callbacks.param_changed.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn add_buffer<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, FilterPortRef<'_>, *mut pw_sys::pw_buffer) + Send + 'a,
    {
        *self.callbacks.add_buffer.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn remove_buffer<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, FilterPortRef<'_>, *mut pw_sys::pw_buffer) + Send + 'a,
    {
        *self.callbacks.remove_buffer.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn process<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, Option<&spa_sys::spa_io_position>) + Send + 'a,
    {
        *self.callbacks.process.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn drained<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D) + Send + 'a,
    {
        *self.callbacks.drained.get_mut() = Some(Box::new(callback));
        self
    }

    #[cfg(feature = "v0_3_39")]
    #[must_use = "Call `.register()` to start receiving events"]
    pub fn command<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, *const spa_sys::spa_command) + Send + 'a,
    {
        *self.callbacks.command.get_mut() = Some(Box::new(callback));
        self
    }

    /// Registers the selected callbacks and retains their filter.
    pub fn register(self) -> Result<FilterListenerRc<'a, D>, Error>
    where
        D: Sync,
    {
        let (events, data) = self.callbacks.into_raw();
        let (listener, data) = unsafe {
            let listener: Box<spa_sys::spa_hook> = Box::new(std::mem::zeroed());
            let raw_listener = Box::into_raw(listener);
            let raw_data = Box::into_raw(data);
            pw_sys::pw_filter_add_listener(
                self.filter.as_raw_ptr(),
                raw_listener,
                events.as_ref().get_ref(),
                raw_data.cast(),
            );
            (Box::from_raw(raw_listener), Box::from_raw(raw_data))
        };
        Ok(FilterListenerRc {
            listener,
            _events: events,
            _data: data,
            _filter: self.filter,
        })
    }
}

/// Owned filter callbacks that retain their shared filter.
#[must_use = "Keep the listener alive in order to receive events"]
pub struct FilterListenerRc<'a, D> {
    listener: Box<spa_sys::spa_hook>,
    _events: Pin<Box<pw_sys::pw_filter_events>>,
    _data: Box<ListenerLocalCallbacks<'a, D>>,
    _filter: FilterRc,
}

impl<D> FilterListenerRc<'_, D> {
    /// Stops receiving events by consuming this listener.
    pub fn unregister(self) {}
}

impl<D> Drop for FilterListenerRc<'_, D> {
    fn drop(&mut self) {
        spa::utils::hook::remove(*self.listener);
    }
}
