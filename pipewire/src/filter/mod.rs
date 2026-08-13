// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Multi-port client-side processing filters.
//!
//! A filter appears in the PipeWire graph as one node with any number of
//! input and output ports. Applications normally add ports, register a
//! process callback, and then call [`Filter::connect`].

use crate::{buffer::Buffer, error::Error, properties::Properties, properties::PropertiesBox};
use bitflags::bitflags;
use spa::utils::{dict::DictRef, result::SpaResult};
use std::{
    cell::UnsafeCell,
    ffi::{self, CStr, CString},
    marker::PhantomData,
    os,
    pin::Pin,
    ptr,
};

mod box_;
pub use box_::*;
mod rc;
pub use rc::*;

/// Current connection state of a filter.
#[derive(Debug, PartialEq)]
pub enum FilterState {
    Error(String),
    Unconnected,
    Connecting,
    Paused,
    Streaming,
}

impl FilterState {
    fn from_raw(state: pw_sys::pw_filter_state, error: *const os::raw::c_char) -> Self {
        match state {
            pw_sys::pw_filter_state_PW_FILTER_STATE_UNCONNECTED => Self::Unconnected,
            pw_sys::pw_filter_state_PW_FILTER_STATE_CONNECTING => Self::Connecting,
            pw_sys::pw_filter_state_PW_FILTER_STATE_PAUSED => Self::Paused,
            pw_sys::pw_filter_state_PW_FILTER_STATE_STREAMING => Self::Streaming,
            _ => {
                let error = if error.is_null() {
                    String::new()
                } else {
                    unsafe { ffi::CStr::from_ptr(error).to_string_lossy().into_owned() }
                };
                Self::Error(error)
            }
        }
    }
}

/// Transparent, non-owning wrapper around `pw_filter`.
#[repr(transparent)]
pub struct Filter(pw_sys::pw_filter);

impl Filter {
    pub fn as_raw(&self) -> &pw_sys::pw_filter {
        &self.0
    }

    pub fn as_raw_ptr(&self) -> *mut pw_sys::pw_filter {
        ptr::addr_of!(self.0).cast_mut()
    }

    /// Creates a listener builder carrying application-owned callback data.
    #[must_use = "Use the builder to register event callbacks"]
    pub fn add_local_listener_with_user_data<D>(
        &self,
        user_data: D,
    ) -> ListenerLocalBuilder<'_, D> {
        let mut callbacks = ListenerLocalCallbacks::with_user_data(user_data);
        callbacks.filter = ptr::NonNull::new(self.as_raw_ptr());
        ListenerLocalBuilder {
            filter: self,
            callbacks,
        }
    }

    /// Creates a listener builder with default callback data.
    #[must_use = "Use the builder to register event callbacks"]
    pub fn add_local_listener<D: Default>(&self) -> ListenerLocalBuilder<'_, D> {
        self.add_local_listener_with_user_data(D::default())
    }

    /// Adds a port and stores `user_data` in its Rust handle.
    ///
    /// The returned handle owns the port registration. Dropping it removes the
    /// port, so it must be kept alive while the filter is connected.
    pub fn add_port_with_user_data<D>(
        &self,
        direction: spa::utils::Direction,
        flags: FilterPortFlags,
        properties: PropertiesBox,
        params: &mut [&spa::pod::Pod],
        user_data: D,
    ) -> Result<FilterPort<'_, D>, Error> {
        let port_data = unsafe {
            pw_sys::pw_filter_add_port(
                self.as_raw_ptr(),
                direction.as_raw(),
                flags.bits(),
                0,
                properties.into_raw(),
                params.as_mut_ptr().cast(),
                params.len() as u32,
            )
        };
        let port_data = ptr::NonNull::new(port_data).ok_or(Error::CreationFailed)?;
        Ok(FilterPort {
            filter: self,
            port_data,
            user_data,
        })
    }

    /// Adds a port without application-specific port data.
    pub fn add_port(
        &self,
        direction: spa::utils::Direction,
        flags: FilterPortFlags,
        properties: PropertiesBox,
        params: &mut [&spa::pod::Pod],
    ) -> Result<FilterPort<'_, ()>, Error> {
        self.add_port_with_user_data(direction, flags, properties, params, ())
    }

    /// Connects all filter ports to the graph.
    pub fn connect(&self, flags: FilterFlags, params: &mut [&spa::pod::Pod]) -> Result<(), Error> {
        let result = unsafe {
            pw_sys::pw_filter_connect(
                self.as_raw_ptr(),
                flags.bits(),
                params.as_mut_ptr().cast(),
                params.len() as u32,
            )
        };
        SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Disconnects the filter from the graph.
    pub fn disconnect(&self) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_disconnect(self.as_raw_ptr()) };
        SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Updates global filter parameters.
    pub fn update_params(&self, params: &mut [&spa::pod::Pod]) -> Result<(), Error> {
        self.update_params_raw(ptr::null_mut(), params)
    }

    fn update_params_raw(
        &self,
        port_data: *mut ffi::c_void,
        params: &mut [&spa::pod::Pod],
    ) -> Result<(), Error> {
        let result = unsafe {
            pw_sys::pw_filter_update_params(
                self.as_raw_ptr(),
                port_data,
                params.as_mut_ptr().cast(),
                params.len() as u32,
            )
        };
        SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Updates global filter properties.
    pub fn update_properties(&self, properties: &DictRef) -> Result<(), Error> {
        self.update_properties_raw(ptr::null_mut(), properties)
    }

    fn update_properties_raw(
        &self,
        port_data: *mut ffi::c_void,
        properties: &DictRef,
    ) -> Result<(), Error> {
        let result = unsafe {
            pw_sys::pw_filter_update_properties(
                self.as_raw_ptr(),
                port_data,
                properties.as_raw_ptr(),
            )
        };
        SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Activates or deactivates processing.
    pub fn set_active(&self, active: bool) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_set_active(self.as_raw_ptr(), active) };
        SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Flushes queued data, optionally draining first.
    pub fn flush(&self, drain: bool) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_flush(self.as_raw_ptr(), drain) };
        SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Returns the filter name.
    pub fn name(&self) -> String {
        unsafe { ffi::CStr::from_ptr(pw_sys::pw_filter_get_name(self.as_raw_ptr())) }
            .to_string_lossy()
            .into_owned()
    }

    /// Returns the current connection state.
    pub fn state(&self) -> FilterState {
        let mut error = ptr::null();
        let state = unsafe { pw_sys::pw_filter_get_state(self.as_raw_ptr(), &mut error) };
        FilterState::from_raw(state, error)
    }

    /// Moves the filter to PipeWire's error state.
    ///
    /// This method must run on the filter's main loop. It is not real-time
    /// safe because PipeWire formats and allocates the retained error text.
    ///
    /// # Panics
    ///
    /// Panics if `res` is not a negative errno-style result, or if `error`
    /// contains a NUL byte.
    pub fn set_error(&self, res: i32, error: &str) {
        let error = CString::new(error).expect("filter error contains a NUL byte");
        self.set_error_cstr(res, &error);
    }

    /// Moves the filter to PipeWire's error state using retained C text.
    ///
    /// This method must run on the filter's main loop and is not real-time
    /// safe. See [`Filter::set_error`].
    ///
    /// # Panics
    ///
    /// Panics if `res` is not a negative errno-style result.
    pub fn set_error_cstr(&self, res: i32, error: &CStr) {
        assert!(res < 0, "a PipeWire filter error result must be negative");
        unsafe {
            pw_sys::pw_filter_set_error(self.as_raw_ptr(), res, c"%s".as_ptr(), error.as_ptr());
        }
    }

    /// Returns global filter properties.
    pub fn properties(&self) -> &Properties {
        self.properties_raw(ptr::null_mut())
    }

    fn properties_raw(&self, port_data: *mut ffi::c_void) -> &Properties {
        unsafe {
            let properties = pw_sys::pw_filter_get_properties(self.as_raw_ptr(), port_data);
            ptr::NonNull::new(properties.cast_mut())
                .expect("filter properties is NULL")
                .cast()
                .as_ref()
        }
    }

    /// Returns the node ID assigned by PipeWire.
    pub fn node_id(&self) -> u32 {
        unsafe { pw_sys::pw_filter_get_node_id(self.as_raw_ptr()) }
    }

    /// Returns whether this filter is driving the graph.
    #[cfg(feature = "v0_3_77")]
    pub fn is_driving(&self) -> bool {
        unsafe { pw_sys::pw_filter_is_driving(self.as_raw_ptr()) }
    }

    /// Requests one graph iteration for a trigger or driver filter.
    #[cfg(feature = "v0_3_77")]
    pub fn trigger_process(&self) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_trigger_process(self.as_raw_ptr()) };
        SpaResult::from_c(result).into_result()?;
        Ok(())
    }

    /// Returns a lifetime-bound handle that can trigger this filter from an
    /// RT callback on another thread.
    #[cfg(feature = "v0_3_77")]
    pub fn trigger_handle(&self) -> FilterTrigger<'_> {
        FilterTrigger {
            filter: ptr::NonNull::new(self.as_raw_ptr()).expect("filter cannot be null"),
            lifetime: PhantomData,
        }
    }

    /// Returns PipeWire's monotonic time in nanoseconds.
    #[cfg(feature = "v1_2_0")]
    pub fn nsec(&self) -> u64 {
        unsafe { pw_sys::pw_filter_get_nsec(self.as_raw_ptr()) }
    }

    /// Returns the data loop assigned to this connected filter.
    ///
    /// The loop is assigned by PipeWire during [`Filter::connect`]. Use
    /// [`crate::loop_::Loop::add_timer_from_thread`] when the assigned loop is
    /// already dispatching on its data thread.
    #[cfg(feature = "v1_2_0")]
    pub fn data_loop(&self) -> Option<&crate::loop_::Loop> {
        unsafe {
            ptr::NonNull::new(pw_sys::pw_filter_get_data_loop(self.as_raw_ptr()))
                .map(|loop_| loop_.cast::<crate::loop_::Loop>().as_ref())
        }
    }
}

/// A lifetime-bound handle for PipeWire's RT-safe filter trigger operation.
#[cfg(feature = "v0_3_77")]
pub struct FilterTrigger<'f> {
    filter: ptr::NonNull<pw_sys::pw_filter>,
    lifetime: PhantomData<&'f Filter>,
}

impl FilterTrigger<'_> {
    /// Requests one graph iteration for the associated trigger or driver
    /// filter.
    pub fn trigger_process(&self) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_trigger_process(self.filter.as_ptr()) };
        SpaResult::from_c(result).into_result()?;
        Ok(())
    }
}

// SAFETY: `FilterTrigger` exposes only `pw_filter_trigger_process`, documented
// by PipeWire as RT-safe. The lifetime prevents use after filter destruction.
unsafe impl Send for FilterTrigger<'_> {}

/// An owned registration for one filter port.
pub struct FilterPort<'f, D = ()> {
    filter: &'f Filter,
    port_data: ptr::NonNull<ffi::c_void>,
    user_data: D,
}

impl<D> FilterPort<'_, D> {
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
            port_data: self.port_data,
            filter: ptr::NonNull::new(self.filter.as_raw_ptr()).expect("filter cannot be null"),
            lifetime: PhantomData,
        }
    }

    /// Takes an available buffer from this port.
    pub fn dequeue_buffer(&self) -> Option<Buffer<'_>> {
        unsafe { Buffer::from_filter_raw(self.dequeue_raw_buffer(), self.port_data) }
    }

    /// Takes an available raw buffer from this port.
    ///
    /// # Safety
    ///
    /// The returned buffer, when non-null, must be queued back to this port.
    pub unsafe fn dequeue_raw_buffer(&self) -> *mut pw_sys::pw_buffer {
        pw_sys::pw_filter_dequeue_buffer(self.port_data.as_ptr())
    }

    /// Queues a raw buffer back to this port.
    ///
    /// # Safety
    ///
    /// `buffer` must have been dequeued from this port and not already queued.
    pub unsafe fn queue_raw_buffer(&self, buffer: *mut pw_sys::pw_buffer) -> Result<(), Error> {
        let result = pw_sys::pw_filter_queue_buffer(self.port_data.as_ptr(), buffer);
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
        pw_sys::pw_filter_get_dsp_buffer(self.port_data.as_ptr(), n_samples)
    }

    /// Returns this port's properties.
    pub fn properties(&self) -> &Properties {
        self.filter.properties_raw(self.port_data.as_ptr())
    }

    /// Updates this port's properties.
    pub fn update_properties(&self, properties: &DictRef) -> Result<(), Error> {
        self.filter
            .update_properties_raw(self.port_data.as_ptr(), properties)
    }

    /// Updates this port's parameters.
    pub fn update_params(&self, params: &mut [&spa::pod::Pod]) -> Result<(), Error> {
        self.filter
            .update_params_raw(self.port_data.as_ptr(), params)
    }
}

impl<D> Drop for FilterPort<'_, D> {
    fn drop(&mut self) {
        unsafe {
            pw_sys::pw_filter_remove_port(self.port_data.as_ptr());
        }
    }
}

/// Opaque port identity passed to callbacks that can refer to either a port or
/// the filter itself.
#[derive(Clone, Copy)]
pub struct FilterPortRef<'f> {
    port_data: ptr::NonNull<ffi::c_void>,
    filter: ptr::NonNull<pw_sys::pw_filter>,
    lifetime: PhantomData<&'f Filter>,
}

impl<'f> FilterPortRef<'f> {
    /// Takes an available buffer from this port.
    pub fn dequeue_buffer(&self) -> Option<Buffer<'f>> {
        unsafe { Buffer::from_filter_raw(self.dequeue_raw_buffer(), self.port_data) }
    }

    /// Takes an available raw buffer from this port.
    ///
    /// # Safety
    ///
    /// The returned buffer, when non-null, must be queued back to this port.
    pub unsafe fn dequeue_raw_buffer(&self) -> *mut pw_sys::pw_buffer {
        pw_sys::pw_filter_dequeue_buffer(self.port_data.as_ptr())
    }

    /// Queues a raw buffer back to this port.
    ///
    /// # Safety
    ///
    /// `buffer` must have been dequeued from this port and not already queued.
    pub unsafe fn queue_raw_buffer(&self, buffer: *mut pw_sys::pw_buffer) -> Result<(), Error> {
        let result = pw_sys::pw_filter_queue_buffer(self.port_data.as_ptr(), buffer);
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
        pw_sys::pw_filter_get_dsp_buffer(self.port_data.as_ptr(), n_samples)
    }

    /// Tests whether this reference identifies an owned port handle.
    pub fn is<D>(&self, port: &FilterPort<'_, D>) -> bool {
        self.port_data == port.port_data && self.filter.as_ptr() == port.filter.as_raw_ptr()
    }
}

// A port reference exposes only PipeWire's documented RT-safe buffer
// operations and immutable pointer identity. Its owner and filter are kept
// alive by its lifetime.
unsafe impl Sync for FilterPortRef<'_> {}

type StateChangedCb<'a, D> = dyn FnMut(&Filter, &D, FilterState, FilterState) + Send + 'a;
type IoChangedCb<'a, D> =
    dyn FnMut(&Filter, &D, Option<FilterPortRef<'_>>, u32, *mut ffi::c_void, u32) + Send + 'a;
type ParamChangedCb<'a, D> =
    dyn FnMut(&Filter, &D, Option<FilterPortRef<'_>>, u32, Option<&spa::pod::Pod>) + Send + 'a;
type BufferCb<'a, D> =
    dyn FnMut(&Filter, &D, FilterPortRef<'_>, *mut pw_sys::pw_buffer) + Send + 'a;
type ProcessCb<'a, D> = dyn FnMut(&Filter, &D, Option<&spa_sys::spa_io_position>) + Send + 'a;

#[allow(clippy::type_complexity)]
struct ListenerLocalCallbacks<'a, D> {
    state_changed: UnsafeCell<Option<Box<StateChangedCb<'a, D>>>>,
    io_changed: UnsafeCell<Option<Box<IoChangedCb<'a, D>>>>,
    param_changed: UnsafeCell<Option<Box<ParamChangedCb<'a, D>>>>,
    add_buffer: UnsafeCell<Option<Box<BufferCb<'a, D>>>>,
    remove_buffer: UnsafeCell<Option<Box<BufferCb<'a, D>>>>,
    process: UnsafeCell<Option<Box<ProcessCb<'a, D>>>>,
    drained: UnsafeCell<Option<Box<dyn FnMut(&Filter, &D) + Send + 'a>>>,
    #[cfg(feature = "v0_3_39")]
    command:
        UnsafeCell<Option<Box<dyn FnMut(&Filter, &D, *const spa_sys::spa_command) + Send + 'a>>>,
    user_data: D,
    filter: Option<ptr::NonNull<pw_sys::pw_filter>>,
}

impl<'a, D> ListenerLocalCallbacks<'a, D> {
    fn with_user_data(user_data: D) -> Self {
        Self {
            state_changed: UnsafeCell::new(None),
            io_changed: UnsafeCell::new(None),
            param_changed: UnsafeCell::new(None),
            add_buffer: UnsafeCell::new(None),
            remove_buffer: UnsafeCell::new(None),
            process: UnsafeCell::new(None),
            drained: UnsafeCell::new(None),
            #[cfg(feature = "v0_3_39")]
            command: UnsafeCell::new(None),
            user_data,
            filter: None,
        }
    }

    fn into_raw(self) -> (Pin<Box<pw_sys::pw_filter_events>>, Box<Self>)
    where
        D: Sync,
    {
        let callbacks = Box::new(self);

        unsafe fn filter<'a>(filter: Option<ptr::NonNull<pw_sys::pw_filter>>) -> &'a Filter {
            filter
                .expect("filter cannot be null")
                .cast::<Filter>()
                .as_ref()
        }

        unsafe fn port<'a>(
            filter: &'a Filter,
            port_data: *mut ffi::c_void,
        ) -> Option<FilterPortRef<'a>> {
            ptr::NonNull::new(port_data).map(|port_data| FilterPortRef {
                port_data,
                filter: ptr::NonNull::new(filter.as_raw_ptr()).expect("filter cannot be null"),
                lifetime: PhantomData,
            })
        }

        unsafe extern "C" fn on_state_changed<D: Sync>(
            data: *mut ffi::c_void,
            old: pw_sys::pw_filter_state,
            new: pw_sys::pw_filter_state,
            error: *const os::raw::c_char,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).state_changed.get()).as_mut() {
                    callback(
                        filter,
                        user_data,
                        FilterState::from_raw(old, error),
                        FilterState::from_raw(new, error),
                    );
                }
            }
        }

        unsafe extern "C" fn on_io_changed<D: Sync>(
            data: *mut ffi::c_void,
            port_data: *mut ffi::c_void,
            id: u32,
            area: *mut ffi::c_void,
            size: u32,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let port = port(filter, port_data);
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).io_changed.get()).as_mut() {
                    callback(filter, user_data, port, id, area, size);
                }
            }
        }

        unsafe extern "C" fn on_param_changed<D: Sync>(
            data: *mut ffi::c_void,
            port_data: *mut ffi::c_void,
            id: u32,
            param: *const spa_sys::spa_pod,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let port = port(filter, port_data);
                let param = (!param.is_null()).then(|| spa::pod::Pod::from_raw(param));
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).param_changed.get()).as_mut() {
                    callback(filter, user_data, port, id, param);
                }
            }
        }

        unsafe extern "C" fn on_add_buffer<D: Sync>(
            data: *mut ffi::c_void,
            port_data: *mut ffi::c_void,
            buffer: *mut pw_sys::pw_buffer,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let Some(port) = port(filter, port_data) else {
                    return;
                };
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).add_buffer.get()).as_mut() {
                    callback(filter, user_data, port, buffer);
                }
            }
        }

        unsafe extern "C" fn on_remove_buffer<D: Sync>(
            data: *mut ffi::c_void,
            port_data: *mut ffi::c_void,
            buffer: *mut pw_sys::pw_buffer,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let Some(port) = port(filter, port_data) else {
                    return;
                };
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).remove_buffer.get()).as_mut() {
                    callback(filter, user_data, port, buffer);
                }
            }
        }

        unsafe extern "C" fn on_process<D: Sync>(
            data: *mut ffi::c_void,
            position: *mut spa_sys::spa_io_position,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).process.get()).as_mut() {
                    callback(filter, user_data, position.as_ref());
                }
            }
        }

        unsafe extern "C" fn on_drained<D: Sync>(data: *mut ffi::c_void) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).drained.get()).as_mut() {
                    callback(filter, user_data);
                }
            }
        }

        #[cfg(feature = "v0_3_39")]
        unsafe extern "C" fn on_command<D: Sync>(
            data: *mut ffi::c_void,
            command: *const spa_sys::spa_command,
        ) {
            let callbacks = data.cast::<ListenerLocalCallbacks<'_, D>>();
            if !callbacks.is_null() {
                let filter = filter((*callbacks).filter);
                let user_data = &*ptr::addr_of!((*callbacks).user_data);
                if let Some(callback) = (&mut *(*callbacks).command.get()).as_mut() {
                    callback(filter, user_data, command);
                }
            }
        }

        let events = unsafe {
            let mut events: Pin<Box<pw_sys::pw_filter_events>> = Box::pin(std::mem::zeroed());
            events.version = pw_sys::PW_VERSION_FILTER_EVENTS;
            events.state_changed = (&*callbacks.state_changed.get())
                .as_ref()
                .map(|_| on_state_changed::<D> as _);
            events.io_changed = (&*callbacks.io_changed.get())
                .as_ref()
                .map(|_| on_io_changed::<D> as _);
            events.param_changed = (&*callbacks.param_changed.get())
                .as_ref()
                .map(|_| on_param_changed::<D> as _);
            events.add_buffer = (&*callbacks.add_buffer.get())
                .as_ref()
                .map(|_| on_add_buffer::<D> as _);
            events.remove_buffer = (&*callbacks.remove_buffer.get())
                .as_ref()
                .map(|_| on_remove_buffer::<D> as _);
            events.process = (&*callbacks.process.get())
                .as_ref()
                .map(|_| on_process::<D> as _);
            events.drained = (&*callbacks.drained.get())
                .as_ref()
                .map(|_| on_drained::<D> as _);
            #[cfg(feature = "v0_3_39")]
            {
                events.command = (&*callbacks.command.get())
                    .as_ref()
                    .map(|_| on_command::<D> as _);
            }
            events
        };

        (events, callbacks)
    }
}

/// Builder for local filter event callbacks.
pub struct ListenerLocalBuilder<'a, D> {
    filter: &'a Filter,
    callbacks: ListenerLocalCallbacks<'a, D>,
}

impl<'a, D> ListenerLocalBuilder<'a, D> {
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

    /// Registers the selected callbacks.
    pub fn register(self) -> Result<FilterListener<'a, D>, Error>
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
        Ok(FilterListener {
            listener,
            _events: events,
            _data: data,
        })
    }
}

/// Owned filter event listener that unregisters itself when dropped.
#[must_use = "Keep the listener alive in order to receive events"]
pub struct FilterListener<'a, D> {
    listener: Box<spa_sys::spa_hook>,
    _events: Pin<Box<pw_sys::pw_filter_events>>,
    _data: Box<ListenerLocalCallbacks<'a, D>>,
}

impl<D> FilterListener<'_, D> {
    /// Stops receiving events by consuming this listener.
    pub fn unregister(self) {}
}

impl<D> Drop for FilterListener<'_, D> {
    fn drop(&mut self) {
        spa::utils::hook::remove(*self.listener);
    }
}

bitflags! {
    /// Flags accepted by [`Filter::connect`].
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct FilterFlags: pw_sys::pw_filter_flags {
        const INACTIVE = pw_sys::pw_filter_flags_PW_FILTER_FLAG_INACTIVE;
        const DRIVER = pw_sys::pw_filter_flags_PW_FILTER_FLAG_DRIVER;
        const RT_PROCESS = pw_sys::pw_filter_flags_PW_FILTER_FLAG_RT_PROCESS;
        const CUSTOM_LATENCY = pw_sys::pw_filter_flags_PW_FILTER_FLAG_CUSTOM_LATENCY;
        #[cfg(feature = "v0_3_77")]
        const TRIGGER = pw_sys::pw_filter_flags_PW_FILTER_FLAG_TRIGGER;
        #[cfg(feature = "v0_3_77")]
        const ASYNC = pw_sys::pw_filter_flags_PW_FILTER_FLAG_ASYNC;
    }
}

bitflags! {
    /// Flags accepted when adding a filter port.
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct FilterPortFlags: pw_sys::pw_filter_port_flags {
        const MAP_BUFFERS = pw_sys::pw_filter_port_flags_PW_FILTER_PORT_FLAG_MAP_BUFFERS;
        const ALLOC_BUFFERS = pw_sys::pw_filter_port_flags_PW_FILTER_PORT_FLAG_ALLOC_BUFFERS;
    }
}
