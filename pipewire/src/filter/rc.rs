// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::{
    ffi::{CStr, CString},
    ops::Deref,
    ptr,
    rc::{Rc, Weak},
};

use crate::{core::CoreRc, properties::PropertiesBox, Error};

use super::{Filter, FilterBox};

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
