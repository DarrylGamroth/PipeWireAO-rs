// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Validated, borrowed per-buffer typed properties.

use crate::pod::Pod;
use std::{fmt, marker::PhantomData};

/// A required property key and exact SPA value type.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct BufferPropertyRequirement(spa_sys::spa_meta_property_requirement);

impl BufferPropertyRequirement {
    /// Require one key with the given SPA type identifier.
    pub const fn new(key: u32, type_: u32) -> Self {
        Self(spa_sys::spa_meta_property_requirement { key, type_ })
    }
}

/// Optical content is independent of electrical trigger classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpticalClassification {
    /// The owner explicitly provides no optical classification.
    Absent,
    /// The exposure contains no fringes.
    Unfringed,
    /// The exposure contains fringes.
    Fringed,
    /// The exposure mixes optical classes.
    Mixed,
}

/// Invalid allocation, malformed property, or unsatisfied required key/type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferPropertiesError;

impl fmt::Display for BufferPropertiesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid or incomplete per-buffer properties")
    }
}
impl std::error::Error for BufferPropertiesError {}

/// An allocation validated by the native bounded BufferProperties contract.
///
/// The immutable storage borrow prevents safe callers from mutating properties
/// after validation or retaining their values beyond the enclosing buffer lease.
pub struct BufferProperties<'a> {
    raw: spa_sys::spa_meta,
    _storage: PhantomData<&'a [u8]>,
}

impl<'a> BufferProperties<'a> {
    /// Validate aligned storage and every required key/type before exposing values.
    /// Unknown optional properties are retained and may be skipped by the caller.
    pub fn from_bytes(
        storage: &'a [u8],
        requirements: &[BufferPropertyRequirement],
    ) -> Result<Self, BufferPropertiesError> {
        let raw = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_Properties,
            size: u32::try_from(storage.len()).map_err(|_| BufferPropertiesError)?,
            data: storage.as_ptr().cast_mut().cast(),
        };
        let count = u32::try_from(requirements.len()).map_err(|_| BufferPropertiesError)?;
        // The native validator checks alignment and all size/field bounds before
        // reading values. The transparent requirement wrapper has native layout.
        if !unsafe {
            spa_sys::spa_meta_properties_is_valid(&raw, requirements.as_ptr().cast(), count)
        } {
            return Err(BufferPropertiesError);
        }
        Ok(Self {
            raw,
            _storage: PhantomData,
        })
    }

    /// Borrow a typed POD value after complete allocation validation.
    pub fn get(&self, key: u32) -> Option<&Pod> {
        let value = unsafe { spa_sys::spa_meta_properties_find(&self.raw, key) };
        // Native validation establishes a complete aligned POD within storage.
        (!value.is_null()).then(|| unsafe { Pod::from_raw(value) })
    }

    /// Distinguish no optical property from explicit absent or mixed content.
    pub fn optical_classification(&self) -> Option<OpticalClassification> {
        let value = self.get(spa_sys::SPA_META_PROPERTY_opticalClassification)?;
        let id = value
            .get_id()
            .expect("native optical classification is an Id");
        Some(match id.0 {
            spa_sys::SPA_META_OPTICAL_ABSENT => OpticalClassification::Absent,
            spa_sys::SPA_META_OPTICAL_UNFRINGED => OpticalClassification::Unfringed,
            spa_sys::SPA_META_OPTICAL_FRINGED => OpticalClassification::Fringed,
            spa_sys::SPA_META_OPTICAL_MIXED => OpticalClassification::Mixed,
            _ => unreachable!("native optical classification was validated"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage(optical: Option<u32>) -> [u64; 16] {
        let mut words = [0u64; 16];
        let mut raw = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_Properties,
            size: std::mem::size_of_val(&words) as u32,
            data: words.as_mut_ptr().cast(),
        };
        assert!(unsafe { spa_sys::spa_meta_properties_init(&mut raw) });
        if let Some(value) = optical {
            assert!(unsafe {
                spa_sys::spa_meta_properties_add(
                    &mut raw,
                    spa_sys::SPA_META_PROPERTY_opticalClassification,
                    spa_sys::SPA_TYPE_Id,
                    4,
                    std::ptr::from_ref(&value).cast(),
                )
            });
        }
        // An optional unknown typed property must not disable known properties.
        let value = 17i32;
        assert!(unsafe {
            spa_sys::spa_meta_properties_add(
                &mut raw,
                123,
                spa_sys::SPA_TYPE_Int,
                4,
                std::ptr::from_ref(&value).cast(),
            )
        });
        words
    }
    fn bytes(words: &[u64]) -> &[u8] {
        unsafe { std::slice::from_raw_parts(words.as_ptr().cast(), std::mem::size_of_val(words)) }
    }

    #[test]
    fn optical_states_and_unknown_optional_properties_are_typed() {
        assert_eq!(
            crate::utils::SpaTypes::ObjectBufferProperties.as_raw(),
            spa_sys::SPA_TYPE_OBJECT_BufferProperties
        );
        for (raw, expected) in [
            (0, OpticalClassification::Absent),
            (1, OpticalClassification::Unfringed),
            (2, OpticalClassification::Fringed),
            (3, OpticalClassification::Mixed),
        ] {
            let words = storage(Some(raw));
            let properties = BufferProperties::from_bytes(bytes(&words), &[]).unwrap();
            assert_eq!(properties.optical_classification(), Some(expected));
            assert_eq!(properties.get(123).unwrap().get_int().unwrap(), 17);
            assert!(properties.get(124).is_none());
        }
    }

    #[test]
    fn required_properties_fail_when_missing_or_wrong_type() {
        let required = [BufferPropertyRequirement::new(
            spa_sys::SPA_META_PROPERTY_opticalClassification,
            spa_sys::SPA_TYPE_Id,
        )];
        let words = storage(None);
        assert!(BufferProperties::from_bytes(bytes(&words), &required).is_err());
        assert_eq!(
            BufferProperties::from_bytes(bytes(&words), &[])
                .unwrap()
                .optical_classification(),
            None
        );
        let words = storage(Some(2));
        assert!(BufferProperties::from_bytes(bytes(&words), &required).is_ok());
        let wrong = [BufferPropertyRequirement::new(
            spa_sys::SPA_META_PROPERTY_opticalClassification,
            spa_sys::SPA_TYPE_Int,
        )];
        assert!(BufferProperties::from_bytes(bytes(&words), &wrong).is_err());
    }

    #[test]
    fn unknown_mandatory_property_needs_explicit_required_type() {
        let mut words = storage(Some(2));
        words[5] |= (spa_sys::SPA_POD_PROP_FLAG_MANDATORY as u64) << 32;
        assert!(BufferProperties::from_bytes(bytes(&words), &[]).is_err());
        let requirements = [BufferPropertyRequirement::new(123, spa_sys::SPA_TYPE_Int)];
        let properties = BufferProperties::from_bytes(bytes(&words), &requirements).unwrap();
        assert_eq!(properties.get(123).unwrap().get_int().unwrap(), 17);
    }

    #[test]
    fn malformed_size_alignment_padding_and_scalar_are_rejected() {
        let mut words = storage(Some(2));
        assert!(BufferProperties::from_bytes(&bytes(&words)[1..], &[]).is_err());
        assert!(BufferProperties::from_bytes(&bytes(&words)[..15], &[]).is_err());
        words[4] = 99; // optical Id body and its padding
        assert!(BufferProperties::from_bytes(bytes(&words), &[]).is_err());
    }
}
