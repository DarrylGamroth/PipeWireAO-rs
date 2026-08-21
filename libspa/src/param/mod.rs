// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Types for dealing with SPA parameters.

pub mod audio;
pub mod format;
pub mod format_utils;
pub mod video;

mod parameters;

pub use parameters::Parameters;

use std::ffi::CStr;
use std::fmt::Debug;

/// Properties of a `SPA_TYPE_OBJECT_ParamBuffers` object.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufferProperties(pub spa_sys::spa_param_buffers);

#[allow(non_upper_case_globals)]
impl BufferProperties {
    pub const Buffers: Self = Self(spa_sys::SPA_PARAM_BUFFERS_buffers);
    pub const Blocks: Self = Self(spa_sys::SPA_PARAM_BUFFERS_blocks);
    pub const Size: Self = Self(spa_sys::SPA_PARAM_BUFFERS_size);
    pub const Stride: Self = Self(spa_sys::SPA_PARAM_BUFFERS_stride);
    pub const Align: Self = Self(spa_sys::SPA_PARAM_BUFFERS_align);
    pub const DataType: Self = Self(spa_sys::SPA_PARAM_BUFFERS_dataType);
    pub const MetaType: Self = Self(spa_sys::SPA_PARAM_BUFFERS_metaType);
    /// Best-effort backing page size (`Id` enum [`BufferPageSizeHint`]).
    pub const PageSizeHint: Self = Self(spa_sys::SPA_PARAM_BUFFERS_pageSizeHint);

    pub const fn from_raw(raw: spa_sys::spa_param_buffers) -> Self {
        Self(raw)
    }

    pub const fn as_raw(self) -> spa_sys::spa_param_buffers {
        self.0
    }
}

/// Best-effort backing page size for host-allocated shared buffers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufferPageSizeHint(pub spa_sys::spa_buffer_page_size_hint);

#[allow(non_upper_case_globals)]
impl BufferPageSizeHint {
    /// Use ordinary system pages.
    pub const Normal: Self = Self(spa_sys::SPA_BUFFER_PAGE_SIZE_NORMAL);
    /// Try the system default hugetlb size, then ordinary pages.
    pub const HugeDefault: Self = Self(spa_sys::SPA_BUFFER_PAGE_SIZE_HUGE_DEFAULT);
    /// Try 2 MiB hugetlb pages, then ordinary pages.
    pub const Huge2Mb: Self = Self(spa_sys::SPA_BUFFER_PAGE_SIZE_HUGE_2MB);
    /// Try 1 GiB hugetlb pages, then ordinary pages.
    pub const Huge1Gb: Self = Self(spa_sys::SPA_BUFFER_PAGE_SIZE_HUGE_1GB);

    pub const fn from_raw(raw: spa_sys::spa_buffer_page_size_hint) -> Self {
        Self(raw)
    }

    pub const fn as_raw(self) -> spa_sys::spa_buffer_page_size_hint {
        self.0
    }
}

/// Different parameter types that can be queried
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct ParamType(pub spa_sys::spa_param_type);

#[allow(non_upper_case_globals)]
impl ParamType {
    /// invalid
    pub const Invalid: Self = Self(spa_sys::SPA_PARAM_Invalid);
    /// property information as SPA_TYPE_OBJECT_PropInfo
    pub const PropInfo: Self = Self(spa_sys::SPA_PARAM_PropInfo);
    /// properties as SPA_TYPE_OBJECT_Props
    pub const Props: Self = Self(spa_sys::SPA_PARAM_Props);
    /// available formats as SPA_TYPE_OBJECT_Format
    pub const EnumFormat: Self = Self(spa_sys::SPA_PARAM_EnumFormat);
    /// configured format as SPA_TYPE_OBJECT_Format
    pub const Format: Self = Self(spa_sys::SPA_PARAM_Format);
    /// buffer configurations as SPA_TYPE_OBJECT_ParamBuffers
    pub const Buffers: Self = Self(spa_sys::SPA_PARAM_Buffers);
    /// allowed metadata for buffers as SPA_TYPE_OBJECT_ParamMeta
    pub const Meta: Self = Self(spa_sys::SPA_PARAM_Meta);
    /// configurable IO areas as SPA_TYPE_OBJECT_ParamIO
    pub const IO: Self = Self(spa_sys::SPA_PARAM_IO);
    /// profile enumeration as SPA_TYPE_OBJECT_ParamProfile
    pub const EnumProfile: Self = Self(spa_sys::SPA_PARAM_EnumProfile);
    /// profile configuration as SPA_TYPE_OBJECT_ParamProfile
    pub const Profile: Self = Self(spa_sys::SPA_PARAM_Profile);
    /// port configuration enumeration as SPA_TYPE_OBJECT_ParamPortConfig
    pub const EnumPortConfig: Self = Self(spa_sys::SPA_PARAM_EnumPortConfig);
    /// port configuration as SPA_TYPE_OBJECT_ParamPortConfig
    pub const PortConfig: Self = Self(spa_sys::SPA_PARAM_PortConfig);
    /// routing enumeration as SPA_TYPE_OBJECT_ParamRoute
    pub const EnumRoute: Self = Self(spa_sys::SPA_PARAM_EnumRoute);
    /// routing configuration as SPA_TYPE_OBJECT_ParamRoute
    pub const Route: Self = Self(spa_sys::SPA_PARAM_Route);
    /// Control parameter, a SPA_TYPE_Sequence
    pub const Control: Self = Self(spa_sys::SPA_PARAM_Control);
    /// latency reporting, a SPA_TYPE_OBJECT_ParamLatency
    pub const Latency: Self = Self(spa_sys::SPA_PARAM_Latency);
    /// processing latency, a SPA_TYPE_OBJECT_ParamProcessLatency
    pub const ProcessLatency: Self = Self(spa_sys::SPA_PARAM_ProcessLatency);

    /// Obtain a [`ParamType`] from a raw `spa_param_type` variant.
    pub fn from_raw(raw: spa_sys::spa_param_type) -> Self {
        Self(raw)
    }

    /// Get the raw [`spa_sys::spa_param_type`] representing this `ParamType`.
    pub fn as_raw(&self) -> spa_sys::spa_param_type {
        self.0
    }
}

impl Debug for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c_str = unsafe {
            let c_buf =
                spa_sys::spa_debug_type_find_short_name(spa_sys::spa_type_param, self.as_raw());
            if c_buf.is_null() {
                return f.write_str("Unknown");
            }
            CStr::from_ptr(c_buf)
        };
        let name = format!("ParamType::{}", c_str.to_string_lossy());
        f.write_str(&name)
    }
}

bitflags::bitflags! {
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct ParamInfoFlags: u32 {
        const SERIAL = 1<<0;
        const READ   = 1<<1;
        const WRITE  = 1<<2;
        const READWRITE = Self::READ.bits() | Self::WRITE.bits();
    }
}

/// Information about a parameter
#[repr(transparent)]
pub struct ParamInfo(spa_sys::spa_param_info);

impl ParamInfo {
    pub fn id(&self) -> ParamType {
        ParamType::from_raw(self.0.id)
    }

    pub fn flags(&self) -> ParamInfoFlags {
        ParamInfoFlags::from_bits_truncate(self.0.flags)
    }
}

impl Debug for ParamInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParamInfo")
            .field("id", &self.id())
            .field("flags", &self.flags())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_page_size_hint_matches_native_abi() {
        assert_eq!(BufferProperties::PageSizeHint.as_raw(), 0x0100_0000);
        assert_eq!(BufferPageSizeHint::Normal.as_raw(), 0);
        assert_eq!(BufferPageSizeHint::HugeDefault.as_raw(), 1);
        assert_eq!(BufferPageSizeHint::Huge2Mb.as_raw(), 2);
        assert_eq!(BufferPageSizeHint::Huge1Gb.as_raw(), 3);
    }
}
