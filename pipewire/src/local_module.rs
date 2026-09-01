// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Ownership of modules loaded into a local PipeWire context.

use std::{
    cell::Cell,
    ffi::{c_void, CStr},
    ptr,
    ptr::NonNull,
};

use crate::{context::ContextRc, properties::PropertiesBox, Error};

#[derive(Debug)]
struct LocalModuleState {
    module: Cell<Option<NonNull<pw_sys::pw_impl_module>>>,
    listener: spa_sys::spa_hook,
}

/// Unique owner of a module loaded into a local [`ContextRc`].
///
/// This is an implementation-side module, not the remote
/// [`Module`](crate::module::Module) proxy exposed through the registry. The
/// context is retained until after the module has been destroyed.
#[derive(Debug)]
pub struct LocalModule {
    // Both allocations have stable addresses because PipeWire retains their
    // pointers until the listener is removed or the module is freed.
    state: Box<LocalModuleState>,
    _events: Box<pw_sys::pw_impl_module_events>,
    context: ContextRc,
}

impl LocalModule {
    /// Load a module into `context`.
    ///
    /// PipeWire takes ownership of `properties`, including when loading
    /// fails. The module is destroyed when the returned owner is dropped.
    pub fn load(
        context: &ContextRc,
        name: &CStr,
        args: Option<&CStr>,
        properties: Option<PropertiesBox>,
    ) -> Result<Self, Error> {
        let args = args.map_or(ptr::null(), CStr::as_ptr);
        let properties = properties.map_or(ptr::null_mut(), PropertiesBox::into_raw);
        // SAFETY: ContextRc keeps the context live, the C strings remain live
        // for the call, and pw_context_load_module consumes properties. A
        // non-null result identifies a live pw_impl_module.
        let module = unsafe {
            pw_sys::pw_context_load_module(context.as_raw_ptr(), name.as_ptr(), args, properties)
        };
        let module = NonNull::new(module).ok_or(Error::CreationFailed)?;
        let mut state = Box::new(LocalModuleState {
            module: Cell::new(Some(module)),
            // SAFETY: An all-zero spa_hook is the required unregistered state.
            listener: unsafe { std::mem::zeroed() },
        });
        let events = Box::new(pw_sys::pw_impl_module_events {
            version: pw_sys::PW_VERSION_IMPL_MODULE_EVENTS,
            destroy: None,
            free: Some(module_freed),
            initialized: None,
            registered: None,
        });
        // SAFETY: module is live. state and events are heap allocations whose
        // addresses remain stable in LocalModule. The free callback cannot
        // unwind and only marks the saved pointer unavailable.
        unsafe {
            pw_sys::pw_impl_module_add_listener(
                module.as_ptr(),
                &mut state.listener,
                events.as_ref(),
                ptr::from_mut(state.as_mut()).cast::<c_void>(),
            );
        }
        Ok(Self {
            state,
            _events: events,
            context: context.clone(),
        })
    }

    /// Return the context that owns this module.
    #[must_use]
    pub fn context(&self) -> &ContextRc {
        &self.context
    }
}

impl Drop for LocalModule {
    fn drop(&mut self) {
        let Some(module) = self.state.module.take() else {
            // A module may schedule its own destruction. Its hook list has
            // already removed the listener in that case.
            return;
        };
        // Unregister before initiating destruction so our callback cannot run
        // through a partially dropped LocalModule. LocalModule is the unique
        // remaining owner of this live module, and its ContextRc remains live.
        spa::utils::hook::remove(self.state.listener);
        unsafe { pw_sys::pw_impl_module_destroy(module.as_ptr()) };
    }
}

unsafe extern "C" fn module_freed(data: *mut c_void) {
    // SAFETY: data is the stable LocalModuleState allocation registered by
    // LocalModule::load. PipeWire invokes this callback before it removes the
    // listener and before LocalModule can release that allocation.
    let state = unsafe { &*(data.cast::<LocalModuleState>()) };
    state.module.set(None);
}
