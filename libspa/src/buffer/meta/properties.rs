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

    /// Electrical trigger classification, independent of optical classification.
    /// Missing key differs from explicit absent or mixed classification.
    pub fn trigger_classification(&self) -> Option<spa_sys::spa_meta_trigger_classification> {
        self.get(spa_sys::SPA_META_PROPERTY_triggerClassification)
            .map(|value| {
                value
                    .get_id()
                    .expect("native trigger classification is an Id")
                    .0
            })
    }

    /// Full-width unsigned fact encoded as eight little-endian bytes.
    pub fn uint64(&self, key: u32) -> Option<u64> {
        let mut value = 0;
        unsafe { spa_sys::spa_meta_properties_get_uint64(&self.raw, key, &mut value) }
            .then_some(value)
    }

    pub fn acquisition_identity(&self) -> Option<super::AcquisitionIdentity> {
        let mut raw = std::mem::MaybeUninit::<spa_sys::spa_acquisition_identity>::uninit();
        if !unsafe {
            spa_sys::spa_meta_properties_get_acquisition_identity(&self.raw, raw.as_mut_ptr())
        } {
            return None;
        }
        let raw = unsafe { raw.assume_init() };
        Some(super::AcquisitionIdentity::new(
            super::AcquisitionDomain::new(raw.domain).expect("validated domain"),
            raw.generation,
            raw.sequence,
        ))
    }

    pub fn exposure_duration(&self) -> Option<u64> {
        self.uint64(spa_sys::SPA_META_PROPERTY_exposureDuration)
    }

    pub fn exposure_start(&self) -> Option<super::ExposureStart> {
        let mut raw = std::mem::MaybeUninit::<spa_sys::spa_exposure_start>::uninit();
        if !unsafe { spa_sys::spa_meta_properties_get_exposure_start(&self.raw, raw.as_mut_ptr()) }
        {
            return None;
        }
        let raw = unsafe { raw.assume_init() };
        let ptp = (raw.timebase == spa_sys::SPA_EXPOSURE_TIMEBASE_TAI).then(|| {
            super::AcquisitionPtpReference::new(
                super::PtpClockIdentity::new(raw.ptp_grandmaster_identity)
                    .expect("validated grandmaster"),
                raw.ptp_domain_number,
            )
        });
        Some(
            super::ExposureStart::new(raw.nsec, raw.uncertainty_nsec, ptp)
                .expect("validated start"),
        )
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

/// Private mutable builder. Only `finish` exposes a complete validated Object.
/// Appends must follow ascending key order, including complete semantic groups.
pub struct BufferPropertiesBuilder<'a> {
    raw: spa_sys::spa_meta,
    _storage: PhantomData<&'a mut [u8]>,
}

impl<'a> BufferPropertiesBuilder<'a> {
    /// Clear the entire allocation before appending this buffer's facts.
    pub fn new(storage: &'a mut [u8]) -> Result<Self, BufferPropertiesError> {
        let mut raw = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_Properties,
            size: u32::try_from(storage.len()).map_err(|_| BufferPropertiesError)?,
            data: storage.as_mut_ptr().cast(),
        };
        if !unsafe { spa_sys::spa_meta_properties_init(&mut raw) } {
            return Err(BufferPropertiesError);
        }
        Ok(Self {
            raw,
            _storage: PhantomData,
        })
    }
    /// Append a POD body with explicit flags and type. Native validation checks sizes and bounds.
    pub fn add(
        &mut self,
        key: u32,
        flags: u32,
        type_: u32,
        body: &[u8],
    ) -> Result<(), BufferPropertiesError> {
        let size = u32::try_from(body.len()).map_err(|_| BufferPropertiesError)?;
        unsafe {
            spa_sys::spa_meta_properties_add(
                &mut self.raw,
                key,
                flags,
                type_,
                size,
                body.as_ptr().cast(),
            )
        }
        .then_some(())
        .ok_or(BufferPropertiesError)
    }
    pub fn add_uint64(
        &mut self,
        key: u32,
        flags: u32,
        value: u64,
    ) -> Result<(), BufferPropertiesError> {
        unsafe { spa_sys::spa_meta_properties_add_uint64(&mut self.raw, key, flags, value) }
            .then_some(())
            .ok_or(BufferPropertiesError)
    }
    pub fn add_acquisition_identity(
        &mut self,
        identity: super::AcquisitionIdentity,
    ) -> Result<(), BufferPropertiesError> {
        let flags = spa_sys::SPA_POD_PROP_FLAG_MANDATORY;
        self.add(
            spa_sys::SPA_META_PROPERTY_acquisitionDomain,
            flags,
            spa_sys::SPA_TYPE_Bytes,
            identity.domain().as_bytes(),
        )?;
        self.add_uint64(
            spa_sys::SPA_META_PROPERTY_acquisitionGeneration,
            flags,
            identity.generation(),
        )?;
        self.add_uint64(
            spa_sys::SPA_META_PROPERTY_acquisitionSequence,
            flags,
            identity.sequence(),
        )
    }
    pub fn add_exposure_duration(&mut self, duration: u64) -> Result<(), BufferPropertiesError> {
        self.add_uint64(
            spa_sys::SPA_META_PROPERTY_exposureDuration,
            spa_sys::SPA_POD_PROP_FLAG_MANDATORY,
            duration,
        )
    }
    pub fn add_exposure_start(
        &mut self,
        start: super::ExposureStart,
    ) -> Result<(), BufferPropertiesError> {
        let flags = spa_sys::SPA_POD_PROP_FLAG_MANDATORY;
        self.add(
            spa_sys::SPA_META_PROPERTY_exposureStart,
            flags,
            spa_sys::SPA_TYPE_Long,
            &start.nanoseconds().to_ne_bytes(),
        )?;
        let timebase = match start.timebase() {
            super::AcquisitionTimebase::Monotonic => spa_sys::SPA_EXPOSURE_TIMEBASE_MONOTONIC,
            super::AcquisitionTimebase::Tai => spa_sys::SPA_EXPOSURE_TIMEBASE_TAI,
        };
        self.add(
            spa_sys::SPA_META_PROPERTY_exposureTimebase,
            flags,
            spa_sys::SPA_TYPE_Id,
            &timebase.to_ne_bytes(),
        )?;
        self.add_uint64(
            spa_sys::SPA_META_PROPERTY_timestampUncertainty,
            flags,
            start.uncertainty(),
        )?;
        if let Some(ptp) = start.ptp_reference() {
            self.add(
                spa_sys::SPA_META_PROPERTY_ptpGrandmasterIdentity,
                flags,
                spa_sys::SPA_TYPE_Bytes,
                ptp.grandmaster().as_bytes(),
            )?;
            self.add(
                spa_sys::SPA_META_PROPERTY_ptpDomainNumber,
                flags,
                spa_sys::SPA_TYPE_Id,
                &u32::from(ptp.domain_number()).to_ne_bytes(),
            )?;
        }
        Ok(())
    }
    pub fn finish(
        self,
        requirements: &[BufferPropertyRequirement],
    ) -> Result<BufferProperties<'a>, BufferPropertiesError> {
        let storage =
            unsafe { std::slice::from_raw_parts(self.raw.data.cast(), self.raw.size as usize) };
        BufferProperties::from_bytes(storage, requirements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::meta::{
        AcquisitionDomain, AcquisitionIdentity, AcquisitionPtpReference, ExposureStart,
        PtpClockIdentity,
    };
    fn bytes(words: &mut [u64]) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(words.as_mut_ptr().cast(), std::mem::size_of_val(words))
        }
    }
    #[test]
    fn full_unsigned_facts_and_ptp_round_trip() {
        let mut words = [0; 64];
        let identity = AcquisitionIdentity::new(
            AcquisitionDomain::new([1; 16]).unwrap(),
            u64::MAX,
            u64::MAX - 1,
        );
        let start = ExposureStart::new(
            i64::MAX,
            u64::MAX,
            Some(AcquisitionPtpReference::new(
                PtpClockIdentity::new([2; 8]).unwrap(),
                255,
            )),
        )
        .unwrap();
        let mut builder = BufferPropertiesBuilder::new(bytes(&mut words)).unwrap();
        builder.add_acquisition_identity(identity).unwrap();
        builder.add_exposure_duration(u64::MAX).unwrap();
        builder.add_exposure_start(start).unwrap();
        let properties = builder.finish(&[]).unwrap();
        assert_eq!(properties.acquisition_identity(), Some(identity));
        assert_eq!(properties.exposure_duration(), Some(u64::MAX));
        assert_eq!(properties.exposure_start(), Some(start));
        assert_eq!(start.time_difference(start), Some((0, u64::MAX)));
        assert!(start.times_match(start, 0));
    }
    #[test]
    fn exposure_comparison_requires_matching_qualified_authority() {
        let authority = AcquisitionPtpReference::new(PtpClockIdentity::new([1; 8]).unwrap(), 7);
        let first = ExposureStart::new(100, 2, Some(authority)).unwrap();
        let second = ExposureStart::new(110, 3, Some(authority)).unwrap();
        assert_eq!(first.time_difference(second), Some((-10, 5)));
        assert!(!first.times_match(second, 4));
        assert!(first.times_match(second, 5));
        assert_eq!(
            first.time_difference(ExposureStart::new(100, 0, None).unwrap()),
            None
        );
        let other = AcquisitionPtpReference::new(PtpClockIdentity::new([2; 8]).unwrap(), 7);
        assert_eq!(
            first.time_difference(ExposureStart::new(100, 0, Some(other)).unwrap()),
            None
        );
        assert!(ExposureStart::new(-1, 0, None).is_err());
    }

    #[test]
    fn declared_custom_mandatory_fact_does_not_hide_known_semantic_facts() {
        let mut words = [0; 64];
        let mut builder = BufferPropertiesBuilder::new(bytes(&mut words)).unwrap();
        builder.add_exposure_duration(u64::MAX).unwrap();
        builder
            .add(
                123,
                spa_sys::SPA_POD_PROP_FLAG_MANDATORY,
                spa_sys::SPA_TYPE_Int,
                &17i32.to_ne_bytes(),
            )
            .unwrap();
        assert!(builder.finish(&[]).is_err());
        let properties = BufferProperties::from_bytes(
            bytes(&mut words),
            &[BufferPropertyRequirement::new(123, spa_sys::SPA_TYPE_Int)],
        )
        .unwrap();
        assert_eq!(properties.exposure_duration(), Some(u64::MAX));
    }

    #[test]
    fn incomplete_groups_missing_requirements_and_reuse_fail_closed() {
        let mut words = [0; 64];
        let mut builder = BufferPropertiesBuilder::new(bytes(&mut words)).unwrap();
        assert!(builder
            .add_uint64(spa_sys::SPA_META_PROPERTY_acquisitionGeneration, 0, 1)
            .is_err());
        builder
            .add_uint64(
                spa_sys::SPA_META_PROPERTY_acquisitionGeneration,
                spa_sys::SPA_POD_PROP_FLAG_MANDATORY,
                1,
            )
            .unwrap();
        assert!(builder.finish(&[]).is_err());
        let required = [BufferPropertyRequirement::new(
            spa_sys::SPA_META_PROPERTY_acquisitionDomain,
            spa_sys::SPA_TYPE_Bytes,
        )];
        assert!(BufferPropertiesBuilder::new(bytes(&mut words))
            .unwrap()
            .finish(&required)
            .is_err());
        let properties = BufferPropertiesBuilder::new(bytes(&mut words))
            .unwrap()
            .finish(&[])
            .unwrap();
        assert_eq!(properties.acquisition_identity(), None);
    }
    #[test]
    fn optical_and_electrical_are_independent_and_unknown_optional_is_retained() {
        for optical in 0u32..=3 {
            for trigger in 0u32..=3 {
                let mut words = [0; 64];
                let mut builder = BufferPropertiesBuilder::new(bytes(&mut words)).unwrap();
                builder
                    .add(
                        spa_sys::SPA_META_PROPERTY_opticalClassification,
                        0,
                        spa_sys::SPA_TYPE_Id,
                        &optical.to_ne_bytes(),
                    )
                    .unwrap();
                builder
                    .add(
                        spa_sys::SPA_META_PROPERTY_triggerClassification,
                        0,
                        spa_sys::SPA_TYPE_Id,
                        &trigger.to_ne_bytes(),
                    )
                    .unwrap();
                builder
                    .add(123, 0, spa_sys::SPA_TYPE_Int, &17i32.to_ne_bytes())
                    .unwrap();
                let properties = builder.finish(&[]).unwrap();
                assert_eq!(properties.trigger_classification(), Some(trigger));
                assert!(properties.optical_classification().is_some());
                assert_eq!(properties.get(123).unwrap().get_int().unwrap(), 17);
            }
        }
    }
}
