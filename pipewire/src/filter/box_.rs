// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use crate::{core::Core, error::Error, properties::PropertiesBox};
use std::{
    ffi::{CStr, CString},
    marker::PhantomData,
    ptr,
};

use super::Filter;

/// Smart pointer providing unique ownership of a PipeWire [filter](super).
///
/// For shared ownership, see [`FilterRc`](super::FilterRc).
pub struct FilterBox<'c> {
    ptr: ptr::NonNull<pw_sys::pw_filter>,
    core: PhantomData<&'c Core>,
}

impl<'c> FilterBox<'c> {
    /// Creates an unconnected filter.
    pub fn new(
        core: &'c Core,
        name: &str,
        properties: PropertiesBox,
    ) -> Result<FilterBox<'c>, Error> {
        let name = CString::new(name).expect("Invalid byte in filter name");
        Self::new_cstr(core, &name, properties)
    }

    /// Creates an unconnected filter with a C string name.
    pub fn new_cstr(
        core: &'c Core,
        name: &CStr,
        properties: PropertiesBox,
    ) -> Result<FilterBox<'c>, Error> {
        unsafe {
            let filter =
                pw_sys::pw_filter_new(core.as_raw_ptr(), name.as_ptr(), properties.into_raw());
            let filter = ptr::NonNull::new(filter).ok_or(Error::CreationFailed)?;
            Ok(Self::from_raw(filter))
        }
    }

    /// Takes ownership of a raw `pw_filter`.
    ///
    /// # Safety
    ///
    /// The pointer must identify a valid filter that is not destroyed or moved
    /// for the returned value's lifetime. The filter's core must outlive it.
    pub unsafe fn from_raw(raw: ptr::NonNull<pw_sys::pw_filter>) -> FilterBox<'c> {
        Self {
            ptr: raw,
            core: PhantomData,
        }
    }

    /// Transfers ownership of the raw filter to the caller.
    pub fn into_raw(self) -> ptr::NonNull<pw_sys::pw_filter> {
        std::mem::ManuallyDrop::new(self).ptr
    }
}

impl std::ops::Deref for FilterBox<'_> {
    type Target = Filter;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.cast().as_ref() }
    }
}

impl std::fmt::Debug for FilterBox<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterBox")
            .field("name", &self.name())
            .field("state", &self.state())
            .field("node-id", &self.node_id())
            .field("properties", &self.properties())
            .finish()
    }
}

impl Drop for FilterBox<'_> {
    fn drop(&mut self) {
        unsafe { pw_sys::pw_filter_destroy(self.as_raw_ptr()) }
    }
}
