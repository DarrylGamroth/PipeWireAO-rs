//! Borrowed, bounded independent SPA metadata records.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataRecordsError;

impl std::fmt::Display for MetadataRecordsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid independent SPA metadata records")
    }
}
impl std::error::Error for MetadataRecordsError {}

/// Validated descriptors and payloads borrowed from one immutable buffer lease.
#[derive(Clone, Copy)]
pub struct MetadataRecords<'a> {
    records: &'a [spa_sys::spa_meta],
}

impl<'a> MetadataRecords<'a> {
    /// Validates envelopes, understood records, and the declared requirements.
    ///
    /// # Safety
    /// Each nonempty payload must cover its advertised allocation and remain
    /// alive for `'a`. Fact payloads must remain immutable. Transport-owned state
    /// is not exposed as bytes. Descriptors alone do not own their payloads.
    pub unsafe fn from_raw(
        records: &'a [spa_sys::spa_meta],
        requirements: &[spa_sys::spa_meta_requirement],
    ) -> Result<Self, MetadataRecordsError> {
        let count = u32::try_from(records.len()).map_err(|_| MetadataRecordsError)?;
        let required = u32::try_from(requirements.len()).map_err(|_| MetadataRecordsError)?;
        if !unsafe {
            spa_sys::spa_meta_records_validate(
                records.as_ptr(),
                count,
                requirements.as_ptr(),
                required,
            )
        } {
            return Err(MetadataRecordsError);
        }
        Ok(Self { records })
    }

    /// Native descriptors remain borrowed for the same payload lifetime.
    pub fn as_raw(self) -> &'a [spa_sys::spa_meta] {
        self.records
    }

    /// Iterates every negotiated type, including opaque optional types.
    pub fn iter(self) -> impl ExactSizeIterator<Item = MetadataRecord<'a>> {
        self.records.iter().map(|raw| MetadataRecord { raw })
    }

    pub fn find(self, type_id: u32) -> Option<MetadataRecord<'a>> {
        self.iter().find(|record| record.type_id() == type_id)
    }
}

/// A read-only record; payload bytes cannot outlive their source lease.
#[derive(Clone, Copy)]
pub struct MetadataRecord<'a> {
    raw: &'a spa_sys::spa_meta,
}

impl<'a> MetadataRecord<'a> {
    pub fn type_id(self) -> u32 {
        self.raw.type_
    }
    pub fn size(self) -> usize {
        self.raw.size as usize
    }
    /// Borrow immutable fact bytes; transport-owned state is never exposed.
    pub fn bytes(self) -> Option<&'a [u8]> {
        if unsafe { spa_sys::spa_meta_type_is_transport_owned(self.type_id()) } {
            return None;
        }
        Some(if self.raw.size == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.raw.data.cast(), self.size()) }
        })
    }
    /// `None` means an opaque optional type whose validity is unknown here.
    pub fn validity(self) -> Option<bool> {
        (unsafe { spa_sys::spa_meta_record_size(self.type_id()) } != 0)
            .then(|| unsafe { spa_sys::spa_meta_record_validate(self.raw, true) })
    }
}

macro_rules! typed_record {
    ($method:ident, $type:ident, $id:ident) => {
        impl<'a> MetadataRecords<'a> {
            pub fn $method(self) -> Option<&'a spa_sys::$type> {
                let record = self.find(spa_sys::$id)?;
                if record.validity() != Some(true) {
                    return None;
                }
                Some(unsafe { &*record.raw.data.cast::<spa_sys::$type>() })
            }
        }
    };
}
typed_record!(header, spa_meta_header, SPA_META_Header);
typed_record!(
    acquisition_identity,
    spa_meta_acquisition_identity,
    SPA_META_AcquisitionIdentity
);
typed_record!(
    exposure_timing,
    spa_meta_exposure_timing,
    SPA_META_ExposureTiming
);
typed_record!(
    reference_timestamp,
    spa_meta_reference_timestamp,
    SPA_META_ReferenceTimestamp
);
typed_record!(
    optical_chopping,
    spa_meta_optical_chopping,
    SPA_META_OpticalChopping
);
typed_record!(
    electrical_trigger,
    spa_meta_electrical_trigger,
    SPA_META_ElectricalTrigger
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_unsigned_identity_and_optional_opaque_bytes_remain_borrowed() {
        let mut identity = spa_sys::spa_meta_acquisition_identity {
            flags: spa_sys::SPA_META_RECORD_VALID,
            reserved: 0,
            domain: [7; 16],
            generation: u64::MAX,
            sequence: u64::MAX - 1,
        };
        let mut opaque = [0x1234_5678_9abc_def0u64];
        let raw = [
            spa_sys::spa_meta {
                type_: spa_sys::SPA_META_AcquisitionIdentity,
                size: 40,
                data: (&mut identity as *mut spa_sys::spa_meta_acquisition_identity).cast(),
            },
            spa_sys::spa_meta {
                type_: 0x3fe,
                size: 8,
                data: opaque.as_mut_ptr().cast(),
            },
        ];
        let records = unsafe { MetadataRecords::from_raw(&raw, &[]) }.unwrap();
        assert_eq!(
            records
                .find(spa_sys::SPA_META_AcquisitionIdentity)
                .unwrap()
                .bytes()
                .unwrap()[24..32],
            u64::MAX.to_ne_bytes()
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_acquisition_identity, generation),
            24
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_exposure_timing, start_nsec),
            8
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_reference_timestamp, timestamp_nsec),
            8
        );
        assert_eq!(records.acquisition_identity().unwrap().generation, u64::MAX);
        assert_eq!(
            records.acquisition_identity().unwrap().sequence,
            u64::MAX - 1
        );
        assert_eq!(
            records.find(0x3fe).unwrap().bytes().unwrap(),
            opaque[0].to_ne_bytes()
        );
        assert_eq!(records.find(0x3fe).unwrap().validity(), None);
        assert_eq!(std::mem::size_of::<spa_sys::spa_meta_exposure_timing>(), 24);
        assert_eq!(
            std::mem::size_of::<spa_sys::spa_meta_reference_timestamp>(),
            24
        );
    }

    #[test]
    fn reference_timestamp_keeps_unsigned_nanoseconds_and_rejects_invalid_required_facts() {
        let mut timestamp = spa_sys::spa_meta_reference_timestamp {
            clock: spa_sys::SPA_META_CLOCK_TAI,
            reserved: 0,
            timestamp_nsec: u64::MAX - 1,
            duration_nsec: u64::MAX,
        };
        let raw = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_ReferenceTimestamp,
            size: 24,
            data: std::ptr::from_mut(&mut timestamp).cast(),
        };
        let requirement = spa_sys::spa_meta_requirement {
            type_: raw.type_,
            size: raw.size,
            flags: spa_sys::SPA_META_REQUIREMENT_REQUIRED,
            reserved: 0,
        };
        let raws = [raw];
        let records = unsafe { MetadataRecords::from_raw(&raws, &[requirement]) }.unwrap();
        assert_eq!(
            records.reference_timestamp().unwrap().timestamp_nsec,
            u64::MAX - 1
        );
        assert_eq!(
            records.reference_timestamp().unwrap().duration_nsec,
            u64::MAX
        );
        assert_eq!(
            &records.find(raw.type_).unwrap().bytes().unwrap()[8..16],
            &(u64::MAX - 1).to_ne_bytes()
        );
        assert!(unsafe { MetadataRecords::from_raw(&[], &[requirement]) }.is_err());
        timestamp.timestamp_nsec = u64::MAX;
        assert!(unsafe { MetadataRecords::from_raw(&raws, &[requirement]) }.is_err());
        timestamp = unsafe { std::mem::zeroed() };
        let records = unsafe { MetadataRecords::from_raw(&raws, &[]) }.unwrap();
        assert!(records.reference_timestamp().is_none());
        assert!(unsafe { MetadataRecords::from_raw(&raws, &[requirement]) }.is_err());
        // Keep the payload observably live through the raw descriptor mutations.
        assert_eq!(timestamp.clock, 0);
    }

    #[test]
    fn transport_owned_state_is_not_exposed_as_immutable_fact_bytes() {
        let mut state = [77u64; 3];
        for (type_, size) in [
            (spa_sys::SPA_META_Busy, 8),
            (spa_sys::SPA_META_SyncTimeline, 24),
        ] {
            let raw = [spa_sys::spa_meta {
                type_,
                size,
                data: state.as_mut_ptr().cast(),
            }];
            let records = unsafe { MetadataRecords::from_raw(&raw, &[]) }.unwrap();
            assert_eq!(records.iter().next().unwrap().bytes(), None);
        }
    }

    #[test]
    fn unavailable_required_identity_and_duplicate_records_fail() {
        let mut identity: spa_sys::spa_meta_acquisition_identity = unsafe { std::mem::zeroed() };
        let raw = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_AcquisitionIdentity,
            size: 40,
            data: (&mut identity as *mut spa_sys::spa_meta_acquisition_identity).cast(),
        };
        let requirement = spa_sys::spa_meta_requirement {
            type_: raw.type_,
            size: raw.size,
            flags: spa_sys::SPA_META_REQUIREMENT_REQUIRED,
            reserved: 0,
        };
        assert!(unsafe { MetadataRecords::from_raw(&[raw], &[requirement]) }.is_err());
        assert!(unsafe { MetadataRecords::from_raw(&[raw, raw], &[]) }.is_err());
        let raw = [raw];
        let records = unsafe { MetadataRecords::from_raw(&raw, &[]) }.unwrap();
        assert_eq!(records.find(raw[0].type_).unwrap().validity(), Some(false));
        assert!(records.acquisition_identity().is_none());
    }
}
