// Copyright The pipewire-rs contributors
// SPDX-License-Identifier: MIT

//! Buffer metadata

use crate::param::video::VideoFormat;
use crate::utils::{Point, Rectangle, Region};

use std::fmt::Debug;

pub trait Metadata {
    const META_TYPE: u32;
}

bitflags::bitflags! {
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    /// Flags for [`MetaHeader::flags`]
    pub struct MetaHeaderFlags: u32 {
        /// Data is not continuous with previous buffer
        const DISCONT = spa_sys::SPA_META_HEADER_FLAG_DISCONT;
        /// Data might be corrupted
        const CORRUPTED = spa_sys::SPA_META_HEADER_FLAG_CORRUPTED;
        /// Data contains a media specific marker
        const MARKER = spa_sys::SPA_META_HEADER_FLAG_MARKER;
        /// Data contains a codec specific header
        const HEADER = spa_sys::SPA_META_HEADER_FLAG_HEADER;
        /// Data contains media neutral data
        const GAP = spa_sys::SPA_META_HEADER_FLAG_GAP;
        /// Data cannot be decoded independently
        const DELTA_UNIT = spa_sys::SPA_META_HEADER_FLAG_DELTA_UNIT;
    }
}

/// Describes essential buffer header metadata such as flags and timestamps.
#[derive(Clone)]
#[repr(transparent)]
pub struct MetaHeader(spa_sys::spa_meta_header);

impl MetaHeader {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_header {
        &self.0
    }

    pub fn flags(&self) -> MetaHeaderFlags {
        MetaHeaderFlags::from_bits_retain(self.0.flags)
    }

    /// Sets the header flags.
    pub fn set_flags(&mut self, flags: MetaHeaderFlags) {
        self.0.flags = flags.bits();
    }

    /// Offset in current cycle
    pub fn offset(&self) -> u32 {
        self.0.offset
    }

    /// Sets the offset in the current cycle.
    pub fn set_offset(&mut self, offset: u32) {
        self.0.offset = offset;
    }

    /// Presentation timestamp in nanoseconds
    pub fn pts(&self) -> i64 {
        self.0.pts
    }

    /// Sets the presentation timestamp in nanoseconds.
    pub fn set_pts(&mut self, pts: i64) {
        self.0.pts = pts;
    }

    /// Decoding timestamp as a difference with pts
    pub fn dts_offset(&self) -> i64 {
        self.0.dts_offset
    }

    /// Sets the decoding timestamp difference from the presentation timestamp.
    pub fn set_dts_offset(&mut self, dts_offset: i64) {
        self.0.dts_offset = dts_offset;
    }

    /// Sequence number, increments with a media specific frequency
    pub fn seq(&self) -> u64 {
        self.0.seq
    }

    /// Sets the media-specific sequence number.
    pub fn set_seq(&mut self, seq: u64) {
        self.0.seq = seq;
    }

    /// Copies all header fields from another buffer header.
    pub fn copy_from(&mut self, other: &Self) {
        self.0 = other.0;
    }
}

impl Debug for MetaHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaHeader")
            .field("flags", &self.flags())
            .field("offset", &self.offset())
            .field("pts", &self.pts())
            .field("dts_offset", &self.dts_offset())
            .field("seq", &self.seq())
            .finish()
    }
}

impl Metadata for MetaHeader {
    const META_TYPE: u32 = spa_sys::SPA_META_Header;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_fields_can_be_updated_without_raw_pointer_access() {
        let mut header = MetaHeader(spa_sys::spa_meta_header {
            flags: 0,
            offset: 0,
            pts: 0,
            dts_offset: 0,
            seq: 0,
        });

        header.set_flags(MetaHeaderFlags::DISCONT | MetaHeaderFlags::MARKER);
        header.set_offset(3);
        header.set_pts(5);
        header.set_dts_offset(-2);
        header.set_seq(7);

        assert_eq!(
            header.flags(),
            MetaHeaderFlags::DISCONT | MetaHeaderFlags::MARKER
        );
        assert_eq!(header.offset(), 3);
        assert_eq!(header.pts(), 5);
        assert_eq!(header.dts_offset(), -2);
        assert_eq!(header.seq(), 7);
    }
}

#[derive(Clone)]
#[repr(transparent)]
pub struct MetaRegion(spa_sys::spa_meta_region);

impl MetaRegion {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_region {
        &self.0
    }

    pub fn region(&self) -> &Region {
        &self.0.region
    }

    pub fn position(&self) -> Point {
        self.0.region.position
    }

    pub fn size(&self) -> Rectangle {
        self.0.region.size
    }

    pub fn is_valid(&self) -> bool {
        unsafe { spa_sys::spa_meta_region_is_valid(self.as_raw()) }
    }
}

impl Debug for MetaRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaRegion")
            .field("region", &self.region())
            .finish()
    }
}

/// MetaRegion with cropping data
#[derive(Clone, Debug)]
#[repr(transparent)]
pub struct MetaVideoCrop(MetaRegion);

impl MetaVideoCrop {
    pub fn meta_region(&self) -> &MetaRegion {
        &self.0
    }
}

impl Metadata for MetaVideoCrop {
    const META_TYPE: u32 = spa_sys::SPA_META_VideoCrop;
}

/// Array of [`MetaRegion`] with damage data. Use [`iter`][Self::iter] to access the regions.
#[repr(transparent)]
pub struct MetaVideoDamage(spa_sys::spa_meta);

impl MetaVideoDamage {
    pub fn as_raw(&self) -> &spa_sys::spa_meta {
        &self.0
    }

    pub fn iter(&self) -> VideoDamageIter<'_> {
        VideoDamageIter::new(self)
    }
}

pub struct VideoDamageIter<'a> {
    video_damage: &'a MetaVideoDamage,
    pos: *const spa_sys::spa_meta_region,
}

impl<'a> VideoDamageIter<'a> {
    fn new(video_damage: &'a MetaVideoDamage) -> Self {
        Self {
            video_damage,
            pos: unsafe { spa_sys::spa_meta_first(video_damage.as_raw()) }.cast(),
        }
    }
}

impl<'a> Iterator for VideoDamageIter<'a> {
    type Item = &'a MetaRegion;

    fn next(&mut self) -> Option<Self::Item> {
        if !unsafe { spa_sys::spa_meta_check(self.pos.cast(), self.video_damage.as_raw()) } {
            return None;
        }

        let region = unsafe { self.pos.cast::<MetaRegion>().as_ref()? };
        if !region.is_valid() {
            return None;
        }

        self.pos = unsafe { self.pos.add(1) };

        Some(region)
    }
}

impl Metadata for MetaVideoDamage {
    const META_TYPE: u32 = spa_sys::SPA_META_VideoDamage;
}

/// Bitmap information
///
/// This metadata contains a bitmap image in the given format and size.
/// It is typically used for cursor images or other small images that are
/// better transferred inline.
#[repr(transparent)]
pub struct MetaBitmap(spa_sys::spa_meta_bitmap);

impl MetaBitmap {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_bitmap {
        &self.0
    }

    /// Bitmap video format
    pub fn format(&self) -> VideoFormat {
        VideoFormat(self.0.format)
    }

    /// Width and height of bitmap
    pub fn size(&self) -> Rectangle {
        self.0.size
    }

    /// Stride of bitmap data
    pub fn stride(&self) -> i32 {
        self.0.stride
    }

    /// Offset of bitmap data in this structure.
    /// Use [`bitmap_data`](Self::bitmap_data) to access the data.
    pub fn offset(&self) -> u32 {
        self.0.offset
    }

    pub fn is_valid(&self) -> bool {
        unsafe { spa_sys::spa_meta_bitmap_is_valid(self.as_raw()) }
    }

    /// Returns a slice with the bitmap data if this bitmap is valid and [`offset`](Self::offset) points to memory after this structure.
    pub fn bitmap_data(&self) -> Option<&[u8]> {
        if !self.is_valid()
            || (self.0.offset as usize) < std::mem::size_of::<spa_sys::spa_meta_bitmap>()
        {
            return None;
        }

        let height = self.0.size.height as usize;
        let stride = self.0.stride.unsigned_abs() as usize;
        let data_size = height * stride;

        unsafe {
            let base_ptr = self as *const _ as *const u8;
            let data_ptr = base_ptr.add(self.0.offset as usize);
            Some(std::slice::from_raw_parts(data_ptr, data_size))
        }
    }
}

impl Debug for MetaBitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaBitmap")
            .field("format", &self.format())
            .field("size", &self.size())
            .field("stride", &self.stride())
            .field("offset", &self.offset())
            .finish()
    }
}

impl Metadata for MetaBitmap {
    const META_TYPE: u32 = spa_sys::SPA_META_Bitmap;
}

/// Cursor information
///
/// Metadata to describe the position and appearance of a pointing device.
#[repr(transparent)]
pub struct MetaCursor(spa_sys::spa_meta_cursor);

impl MetaCursor {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_cursor {
        &self.0
    }

    /// Cursor id. An id of `0` is an invalid id and means there is no new cursor data.
    pub fn id(&self) -> u32 {
        self.0.id
    }

    /// Extra flags
    pub fn flags(&self) -> u32 {
        self.0.flags
    }

    /// Position on screen
    pub fn position(&self) -> Point {
        self.0.position
    }

    /// Offsets for hotspot in bitmap, this has no meaning when there is no valid bitmap.
    pub fn hotspot(&self) -> Point {
        self.0.hotspot
    }

    /// Offset of bitmap meta in this structure. Use [`bitmap`](Self::bitmap) to access the bitmap
    /// meta.
    pub fn bitmap_offset(&self) -> u32 {
        self.0.bitmap_offset
    }

    pub fn is_valid(&self) -> bool {
        unsafe { spa_sys::spa_meta_cursor_is_valid(self.as_raw()) }
    }

    /// Returns the bitmap meta if the cursor is valid and [`bitmap_offset`](Self::bitmap_offset)
    /// points to memory after this structure.
    pub fn bitmap(&self) -> Option<&MetaBitmap> {
        if !self.is_valid()
            || (self.0.bitmap_offset as usize) < std::mem::size_of::<spa_sys::spa_meta_cursor>()
        {
            return None;
        }

        unsafe {
            let base_ptr = self as *const _ as *const u8;
            let bitmap_ptr = base_ptr.add(self.0.bitmap_offset as usize);
            let bitmap_ptr = bitmap_ptr as *const MetaBitmap;
            bitmap_ptr.as_ref()
        }
    }
}

impl Debug for MetaCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaCursor")
            .field("id", &self.id())
            .field("flags", &self.flags())
            .field("position", &self.position())
            .field("hotspot", &self.hotspot())
            .field("bitmap_offset", &self.bitmap_offset())
            .finish()
    }
}

impl Metadata for MetaCursor {
    const META_TYPE: u32 = spa_sys::SPA_META_Cursor;
}

/// A timed set of events associated with the buffer
#[repr(transparent)]
pub struct MetaControl(spa_sys::spa_meta_control);

impl MetaControl {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_control {
        &self.0
    }

    pub fn sequence(&self) -> &spa_sys::spa_pod_sequence {
        &self.0.sequence
    }
}

impl Metadata for MetaControl {
    const META_TYPE: u32 = spa_sys::SPA_META_Control;
}

/// A busy counter for the buffer
#[cfg(feature = "v0_3_21")]
#[derive(Clone)]
#[repr(transparent)]
pub struct MetaBusy(spa_sys::spa_meta_busy);

#[cfg(feature = "v0_3_21")]
impl MetaBusy {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_busy {
        &self.0
    }

    pub fn flags(&self) -> u32 {
        self.0.flags
    }

    /// Number of users busy with the buffer
    pub fn count(&self) -> u32 {
        self.0.count
    }
}

#[cfg(feature = "v0_3_21")]
impl Debug for MetaBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaBusy")
            .field("flags", &self.flags())
            .field("count", &self.count())
            .finish()
    }
}

#[cfg(feature = "v0_3_21")]
impl Metadata for MetaBusy {
    const META_TYPE: u32 = spa_sys::SPA_META_Busy;
}

#[cfg(feature = "v0_3_62")]
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct MetaVideoTransformValue(spa_sys::spa_meta_videotransform_value);

#[cfg(feature = "v0_3_62")]
impl MetaVideoTransformValue {
    /// No transform
    pub const NONE: Self = Self(spa_sys::SPA_META_TRANSFORMATION_None);
    /// 90 degree counter-clockwise
    pub const ROTATED90: Self = Self(spa_sys::SPA_META_TRANSFORMATION_90);
    /// 180 degree counter-clockwise
    pub const ROTATED180: Self = Self(spa_sys::SPA_META_TRANSFORMATION_180);
    /// 270 degree counter-clockwise
    pub const ROTATED270: Self = Self(spa_sys::SPA_META_TRANSFORMATION_270);
    /// 180 degree flipped around the vertical axis. Equivalent
    /// to a reflection through the vertical line splitting the
    /// buffer in two equal sized parts
    pub const FLIPPED: Self = Self(spa_sys::SPA_META_TRANSFORMATION_Flipped);
    /// Flip then rotate around 90 degree counter-clockwise
    pub const FLIPPED90: Self = Self(spa_sys::SPA_META_TRANSFORMATION_Flipped90);
    /// Flip then rotate around 180 degree counter-clockwise
    pub const FLIPPED180: Self = Self(spa_sys::SPA_META_TRANSFORMATION_Flipped180);
    /// Flip then rotate around 270 degree counter-clockwise
    pub const FLIPPED270: Self = Self(spa_sys::SPA_META_TRANSFORMATION_Flipped270);

    pub fn from_raw(raw: spa_sys::spa_meta_videotransform_value) -> Self {
        Self(raw)
    }

    pub fn as_raw(&self) -> spa_sys::spa_meta_videotransform_value {
        self.0
    }
}

#[cfg(feature = "v0_3_62")]
impl Debug for MetaVideoTransformValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MetaVideoTransformValue::{}",
            match *self {
                Self::NONE => "None",
                Self::ROTATED90 => "ROTATED90",
                Self::ROTATED180 => "ROTATED180",
                Self::ROTATED270 => "ROTATED270",
                Self::FLIPPED => "FLIPPED",
                Self::FLIPPED90 => "FLIPPED90",
                Self::FLIPPED180 => "FLIPPED180",
                Self::FLIPPED270 => "FLIPPED270",
                _ => "Unknown",
            }
        )
    }
}

/// A transformation of the buffer
#[cfg(feature = "v0_3_62")]
#[repr(transparent)]
pub struct MetaVideoTransform(spa_sys::spa_meta_videotransform);

#[cfg(feature = "v0_3_62")]
impl MetaVideoTransform {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_videotransform {
        &self.0
    }

    pub fn transform(&self) -> MetaVideoTransformValue {
        MetaVideoTransformValue::from_raw(self.0.transform)
    }
}

#[cfg(feature = "v0_3_62")]
impl Debug for MetaVideoTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaVideoTransform")
            .field("transform", &self.transform())
            .finish()
    }
}

#[cfg(feature = "v0_3_62")]
impl Metadata for MetaVideoTransform {
    const META_TYPE: u32 = spa_sys::SPA_META_VideoTransform;
}

#[cfg(feature = "v1_2_0")]
bitflags::bitflags! {
    /// Flags for [`MetaSyncTimeline::flags`]
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct MetaSyncTimelineFlags: u32 {
        /// This flag is set by the producer and cleared by the consumer
        /// when it promises to signal the release point.
        #[cfg(feature = "v1_6_0")]
        const UNSCHEDULED_RELEASE = spa_sys::SPA_META_SYNC_TIMELINE_UNSCHEDULED_RELEASE;

    }
}

/// A timeline point for explicit sync
///
/// Metadata to describe the time on the timeline when the buffer can be acquired and when it can be reused.
///
/// This metadata will require negotiation of 2 extra fds for the acquire
/// and release timelines respectively.  One way to achieve this is to place
/// this metadata as SPA_PARAM_BUFFERS_metaType when negotiating a buffer
/// layout with 2 extra fds.
#[cfg(feature = "v1_2_0")]
#[repr(transparent)]
pub struct MetaSyncTimeline(spa_sys::spa_meta_sync_timeline);

#[cfg(feature = "v1_2_0")]
impl MetaSyncTimeline {
    pub fn as_raw(&self) -> &spa_sys::spa_meta_sync_timeline {
        &self.0
    }

    pub fn flags(&self) -> MetaSyncTimelineFlags {
        MetaSyncTimelineFlags::from_bits_retain(self.0.flags)
    }

    pub fn padding(&self) -> u32 {
        self.0.padding
    }

    /// The timeline acquire point, this is when the data can be accessed.
    pub fn acquire_point(&self) -> u64 {
        self.0.acquire_point
    }

    /// The timeline release point, this timeline point should be signaled when data is no longer accessed.
    pub fn release_point(&self) -> u64 {
        self.0.release_point
    }
}

#[cfg(feature = "v1_2_0")]
impl Debug for MetaSyncTimeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaSyncTimeline")
            .field("flags", &self.flags())
            .field("padding", &self.padding())
            .field("acquire_point", &self.acquire_point())
            .field("release_point", &self.release_point())
            .finish()
    }
}

#[cfg(feature = "v1_2_0")]
impl Metadata for MetaSyncTimeline {
    const META_TYPE: u32 = spa_sys::SPA_META_SyncTimeline;
}

/// Number of bytes in a PipeWireAO acquisition-domain identifier.
pub const ACQUISITION_DOMAIN_SIZE: usize = spa_sys::SPA_META_ACQUISITION_DOMAIN_SIZE as usize;

bitflags::bitflags! {
    /// Valid fields in [`MetaAcquisition`].
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct AcquisitionFlags: u32 {
        const IDENTITY_VALID = spa_sys::SPA_META_ACQUISITION_FLAG_IDENTITY_VALID;
        const EXPOSURE_START_VALID =
            spa_sys::SPA_META_ACQUISITION_FLAG_EXPOSURE_START_VALID;
        const EXPOSURE_DURATION_VALID =
            spa_sys::SPA_META_ACQUISITION_FLAG_EXPOSURE_DURATION_VALID;
    }
}

/// Opaque, nonzero acquisition-domain identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct AcquisitionDomain([u8; ACQUISITION_DOMAIN_SIZE]);

impl AcquisitionDomain {
    pub fn new(bytes: [u8; ACQUISITION_DOMAIN_SIZE]) -> Result<Self, AcquisitionMetaError> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err(AcquisitionMetaError);
        }
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; ACQUISITION_DOMAIN_SIZE] {
        &self.0
    }
}

/// Complete physical-acquisition identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcquisitionIdentity {
    domain: AcquisitionDomain,
    generation: u64,
    sequence: u64,
}

impl AcquisitionIdentity {
    pub const fn new(domain: AcquisitionDomain, generation: u64, sequence: u64) -> Self {
        Self {
            domain,
            generation,
            sequence,
        }
    }

    pub const fn domain(self) -> AcquisitionDomain {
        self.domain
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}

/// A mapped acquisition metadata allocation violates the native Version 1 ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcquisitionMetaError;

impl std::fmt::Display for AcquisitionMetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid PipeWireAO acquisition metadata")
    }
}

impl std::error::Error for AcquisitionMetaError {}

/// Version 1 acquisition identity and qualified exposure time.
#[repr(transparent)]
pub struct MetaAcquisition(spa_sys::spa_meta_acquisition);

impl MetaAcquisition {
    /// Creates valid metadata with no identity or exposure time established.
    pub fn new() -> Self {
        let mut raw = std::mem::MaybeUninit::<spa_sys::spa_meta_acquisition>::uninit();
        let initialized = unsafe { spa_sys::spa_meta_acquisition_init(raw.as_mut_ptr()) };
        assert!(
            initialized,
            "aligned Rust acquisition metadata failed to initialize"
        );
        Self(unsafe { raw.assume_init() })
    }

    /// Clears reusable metadata to its valid, empty Version 1 state.
    pub fn initialize(&mut self) {
        let initialized = unsafe { spa_sys::spa_meta_acquisition_init(self.as_raw_mut()) };
        assert!(
            initialized,
            "aligned Rust acquisition metadata failed to initialize"
        );
    }

    pub fn as_raw(&self) -> &spa_sys::spa_meta_acquisition {
        &self.0
    }

    pub fn as_raw_mut(&mut self) -> &mut spa_sys::spa_meta_acquisition {
        &mut self.0
    }

    pub fn version(&self) -> u32 {
        self.0.version
    }

    pub fn abi_size(&self) -> u32 {
        self.0.abi_size
    }

    pub fn flags(&self) -> Result<AcquisitionFlags, AcquisitionMetaError> {
        self.validate()?;
        AcquisitionFlags::from_bits(self.0.flags).ok_or(AcquisitionMetaError)
    }

    pub fn set_identity(
        &mut self,
        identity: AcquisitionIdentity,
    ) -> Result<(), AcquisitionMetaError> {
        let valid = unsafe {
            spa_sys::spa_meta_acquisition_set_identity(
                self.as_raw_mut(),
                identity.domain.as_bytes().as_ptr(),
                identity.generation,
                identity.sequence,
            )
        };
        valid.then_some(()).ok_or(AcquisitionMetaError)
    }

    pub fn identity(&self) -> Result<Option<AcquisitionIdentity>, AcquisitionMetaError> {
        let flags = self.flags()?;
        if !flags.contains(AcquisitionFlags::IDENTITY_VALID) {
            return Ok(None);
        }
        Ok(Some(AcquisitionIdentity::new(
            AcquisitionDomain(self.0.domain),
            self.0.generation,
            self.0.sequence,
        )))
    }

    /// Sets exposure start in the local Linux `CLOCK_MONOTONIC` domain.
    pub fn set_exposure_start(
        &mut self,
        nanoseconds: i64,
        uncertainty_nanoseconds: u64,
    ) -> Result<(), AcquisitionMetaError> {
        let valid = unsafe {
            spa_sys::spa_meta_acquisition_set_exposure_start(
                self.as_raw_mut(),
                nanoseconds,
                uncertainty_nanoseconds,
            )
        };
        valid.then_some(()).ok_or(AcquisitionMetaError)
    }

    /// Returns exposure start and its inclusive uncertainty bound.
    pub fn exposure_start(&self) -> Result<Option<(i64, u64)>, AcquisitionMetaError> {
        let flags = self.flags()?;
        Ok(flags
            .contains(AcquisitionFlags::EXPOSURE_START_VALID)
            .then_some((
                self.0.exposure_start_nsec,
                self.0.timestamp_uncertainty_nsec,
            )))
    }

    pub fn set_exposure_duration(&mut self, nanoseconds: u64) -> Result<(), AcquisitionMetaError> {
        let valid = unsafe {
            spa_sys::spa_meta_acquisition_set_exposure_duration(self.as_raw_mut(), nanoseconds)
        };
        valid.then_some(()).ok_or(AcquisitionMetaError)
    }

    pub fn exposure_duration(&self) -> Result<Option<u64>, AcquisitionMetaError> {
        let flags = self.flags()?;
        Ok(flags
            .contains(AcquisitionFlags::EXPOSURE_DURATION_VALID)
            .then_some(self.0.exposure_duration_nsec))
    }

    /// Uses the native helper to compare complete, valid identity tuples.
    pub fn identity_equal(&self, other: &Self) -> bool {
        unsafe { spa_sys::spa_meta_acquisition_identity_equal(self.as_raw(), other.as_raw()) }
    }

    /// Validates this allocation with the authoritative native helper.
    pub fn validate(&self) -> Result<(), AcquisitionMetaError> {
        let wrapper = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_Acquisition,
            size: std::mem::size_of::<spa_sys::spa_meta_acquisition>() as u32,
            data: std::ptr::addr_of!(self.0).cast_mut().cast(),
        };
        let valid = unsafe { spa_sys::spa_meta_acquisition_is_valid(&wrapper) };
        valid.then_some(()).ok_or(AcquisitionMetaError)
    }
}

impl Default for MetaAcquisition {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for MetaAcquisition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaAcquisition")
            .field("version", &self.version())
            .field("abi_size", &self.abi_size())
            .field("flags", &self.flags())
            .field("identity", &self.identity())
            .field("exposure_start", &self.exposure_start())
            .field("exposure_duration", &self.exposure_duration())
            .finish()
    }
}

impl Metadata for MetaAcquisition {
    const META_TYPE: u32 = spa_sys::SPA_META_Acquisition;
}

#[cfg(test)]
mod acquisition_tests {
    use super::*;

    fn domain(first_byte: u8) -> AcquisitionDomain {
        let mut bytes = [0; ACQUISITION_DOMAIN_SIZE];
        bytes[0] = first_byte;
        AcquisitionDomain::new(bytes).unwrap()
    }

    #[test]
    fn acquisition_metadata_matches_native_version_one_abi() {
        assert_eq!(ACQUISITION_DOMAIN_SIZE, 16);
        assert_eq!(
            std::mem::size_of::<MetaAcquisition>(),
            spa_sys::SPA_META_ACQUISITION_SIZE as usize
        );
        assert_eq!(std::mem::align_of::<MetaAcquisition>(), 8);
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_acquisition, domain),
            16
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_acquisition, generation),
            32
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_acquisition, sequence),
            40
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_acquisition, exposure_start_nsec),
            48
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_acquisition, reserved),
            72
        );
        assert_eq!(MetaAcquisition::META_TYPE, spa_sys::SPA_META_Acquisition);
        assert_eq!(
            AcquisitionFlags::all().bits(),
            spa_sys::SPA_META_ACQUISITION_FLAG_ALL
        );
    }

    #[test]
    fn acquisition_metadata_exposes_valid_identity_and_time() {
        assert_eq!(
            AcquisitionDomain::new([0; ACQUISITION_DOMAIN_SIZE]),
            Err(AcquisitionMetaError)
        );

        let identity = AcquisitionIdentity::new(domain(1), 7, 42);
        let mut meta = MetaAcquisition::new();
        meta.validate().unwrap();
        assert_eq!(meta.flags().unwrap(), AcquisitionFlags::empty());
        assert_eq!(meta.identity().unwrap(), None);
        assert_eq!(meta.exposure_start().unwrap(), None);
        assert_eq!(meta.exposure_duration().unwrap(), None);

        meta.set_identity(identity).unwrap();
        meta.set_exposure_start(123_456, 9).unwrap();
        meta.set_exposure_duration(5_000).unwrap();
        meta.validate().unwrap();
        assert_eq!(meta.identity().unwrap(), Some(identity));
        assert_eq!(meta.exposure_start().unwrap(), Some((123_456, 9)));
        assert_eq!(meta.exposure_duration().unwrap(), Some(5_000));

        let mut same = MetaAcquisition::new();
        same.set_identity(identity).unwrap();
        assert!(meta.identity_equal(&same));
        same.set_identity(AcquisitionIdentity::new(domain(1), 7, 43))
            .unwrap();
        assert!(!meta.identity_equal(&same));

        meta.initialize();
        assert_eq!(meta.identity().unwrap(), None);
        assert_eq!(meta.as_raw().exposure_start_nsec, spa_sys::SPA_TIME_INVALID);
    }

    #[test]
    fn acquisition_metadata_rejects_malformed_values() {
        let mut meta = MetaAcquisition::new();
        meta.set_identity(AcquisitionIdentity::new(domain(1), 7, 42))
            .unwrap();
        meta.as_raw_mut().flags |= 1 << 31;
        assert_eq!(meta.validate(), Err(AcquisitionMetaError));

        meta.initialize();
        meta.as_raw_mut().reserved[0] = 1;
        assert_eq!(meta.validate(), Err(AcquisitionMetaError));

        meta.initialize();
        meta.as_raw_mut().exposure_start_nsec = 0;
        assert_eq!(meta.validate(), Err(AcquisitionMetaError));

        meta.initialize();
        assert_eq!(
            meta.set_exposure_start(spa_sys::SPA_TIME_INVALID, 0),
            Err(AcquisitionMetaError)
        );
        assert_eq!(meta.set_exposure_duration(0), Err(AcquisitionMetaError));
    }
}

bitflags::bitflags! {
    /// Terminal outcome flags for [`MetaProgressive`].
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct ProgressiveFlags: u32 {
        const INCOMPLETE = spa_sys::SPA_META_PROGRESSIVE_FLAG_INCOMPLETE;
        const INVALID_LAYOUT = spa_sys::SPA_META_PROGRESSIVE_FLAG_INVALID_LAYOUT;
        const CANCELLED = spa_sys::SPA_META_PROGRESSIVE_FLAG_CANCELLED;
        const DEVICE_ERROR = spa_sys::SPA_META_PROGRESSIVE_FLAG_DEVICE_ERROR;
        const CORRUPTED = spa_sys::SPA_META_PROGRESSIVE_FLAG_CORRUPTED;
        const PROTOCOL_ERROR = spa_sys::SPA_META_PROGRESSIVE_FLAG_PROTOCOL_ERROR;
    }
}

/// Producer lifecycle state published in one atomic progressive snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ProgressiveState {
    Prepared = spa_sys::SPA_META_PROGRESSIVE_STATE_PREPARED,
    Active = spa_sys::SPA_META_PROGRESSIVE_STATE_ACTIVE,
    Complete = spa_sys::SPA_META_PROGRESSIVE_STATE_COMPLETE,
    Aborted = spa_sys::SPA_META_PROGRESSIVE_STATE_ABORTED,
}

/// One coherent view of committed bytes and producer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressiveSnapshot {
    committed_bytes: u32,
    state: ProgressiveState,
}

impl ProgressiveSnapshot {
    pub const fn new(committed_bytes: u32, state: ProgressiveState) -> Self {
        Self {
            committed_bytes,
            state,
        }
    }

    pub const fn committed_bytes(self) -> u32 {
        self.committed_bytes
    }

    pub const fn state(self) -> ProgressiveState {
        self.state
    }

    pub const fn encode(self) -> u64 {
        self.committed_bytes as u64
            | ((self.state as u64) << spa_sys::SPA_META_PROGRESSIVE_STATE_SHIFT)
    }

    pub const fn decode(value: u64) -> Option<Self> {
        if value & spa_sys::SPA_META_PROGRESSIVE_RESERVED_MASK as u64 != 0 {
            return None;
        }
        let state = match ((value & spa_sys::SPA_META_PROGRESSIVE_STATE_MASK)
            >> spa_sys::SPA_META_PROGRESSIVE_STATE_SHIFT) as u32
        {
            spa_sys::SPA_META_PROGRESSIVE_STATE_PREPARED => ProgressiveState::Prepared,
            spa_sys::SPA_META_PROGRESSIVE_STATE_ACTIVE => ProgressiveState::Active,
            spa_sys::SPA_META_PROGRESSIVE_STATE_COMPLETE => ProgressiveState::Complete,
            spa_sys::SPA_META_PROGRESSIVE_STATE_ABORTED => ProgressiveState::Aborted,
            _ => return None,
        };
        Some(Self::new(
            (value & spa_sys::SPA_META_PROGRESSIVE_COMMITTED_MASK as u64) as u32,
            state,
        ))
    }
}

/// Acquire-observed progressive state and terminal flags when quiescent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressiveObservation {
    snapshot: ProgressiveSnapshot,
    terminal_flags: Option<ProgressiveFlags>,
}

impl ProgressiveObservation {
    pub const fn snapshot(self) -> ProgressiveSnapshot {
        self.snapshot
    }

    pub const fn terminal_flags(self) -> Option<ProgressiveFlags> {
        self.terminal_flags
    }
}

/// A mapped progressive metadata allocation violates the native Version 1 ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressiveMetaError;

impl std::fmt::Display for ProgressiveMetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid PipeWireAO progressive metadata")
    }
}

impl std::error::Error for ProgressiveMetaError {}

/// Version 1 progressive-buffer metadata shared by PipeWireAO endpoints.
#[repr(transparent)]
pub struct MetaProgressive(spa_sys::spa_meta_progressive);

impl MetaProgressive {
    pub fn new(
        data_index: u32,
        payload_offset: u32,
        payload_size: u32,
        commit_granularity: u32,
    ) -> Result<Self, ProgressiveMetaError> {
        let mut meta = Self(spa_sys::spa_meta_progressive {
            version: 0,
            abi_size: 0,
            data_index: 0,
            payload_offset: 0,
            payload_size: 0,
            commit_granularity: 0,
            terminal_flags: 0,
            reserved0: 0,
            snapshot: 0,
            reserved1: 0,
        });
        meta.initialize(data_index, payload_offset, payload_size, commit_granularity)?;
        Ok(meta)
    }

    pub fn initialize(
        &mut self,
        data_index: u32,
        payload_offset: u32,
        payload_size: u32,
        commit_granularity: u32,
    ) -> Result<(), ProgressiveMetaError> {
        let valid = unsafe {
            spa_sys::spa_meta_progressive_init(
                self.as_raw_mut(),
                data_index,
                payload_offset,
                payload_size,
                commit_granularity,
            )
        };
        valid.then_some(()).ok_or(ProgressiveMetaError)
    }

    pub fn as_raw(&self) -> &spa_sys::spa_meta_progressive {
        &self.0
    }

    pub fn as_raw_mut(&mut self) -> &mut spa_sys::spa_meta_progressive {
        &mut self.0
    }

    pub fn version(&self) -> u32 {
        self.0.version
    }

    pub fn abi_size(&self) -> u32 {
        self.0.abi_size
    }

    pub fn data_index(&self) -> u32 {
        self.0.data_index
    }

    pub fn payload_offset(&self) -> u32 {
        self.0.payload_offset
    }

    pub fn payload_size(&self) -> u32 {
        self.0.payload_size
    }

    pub fn commit_granularity(&self) -> u32 {
        self.0.commit_granularity
    }

    pub fn set_terminal_flags(&mut self, flags: ProgressiveFlags) {
        self.0.terminal_flags = flags.bits();
    }

    fn snapshot_atomic(&self) -> &std::sync::atomic::AtomicU64 {
        unsafe {
            // SAFETY: the native ABI fixes `snapshot` at an 8-byte aligned
            // offset and requires all accesses to use atomic operations.
            &*std::ptr::addr_of!(self.0.snapshot).cast()
        }
    }

    /// Acquire-load the authoritative committed prefix and producer state.
    pub fn load_acquire(&self) -> Result<ProgressiveSnapshot, ProgressiveMetaError> {
        let value = self
            .snapshot_atomic()
            .load(std::sync::atomic::Ordering::Acquire);
        ProgressiveSnapshot::decode(value).ok_or(ProgressiveMetaError)
    }

    /// Acquire-load state and read terminal flags only after producer quiescence.
    pub fn observe_acquire(&self) -> Result<ProgressiveObservation, ProgressiveMetaError> {
        let snapshot = self.load_acquire()?;
        let terminal_flags = match snapshot.state() {
            ProgressiveState::Complete | ProgressiveState::Aborted => Some(
                ProgressiveFlags::from_bits(self.0.terminal_flags).ok_or(ProgressiveMetaError)?,
            ),
            ProgressiveState::Prepared | ProgressiveState::Active => None,
        };
        Ok(ProgressiveObservation {
            snapshot,
            terminal_flags,
        })
    }

    /// Release-publish a committed prefix and producer state.
    pub fn store_release(&self, snapshot: ProgressiveSnapshot) {
        self.snapshot_atomic()
            .store(snapshot.encode(), std::sync::atomic::Ordering::Release);
    }

    /// Validate immutable Version 1 fields and the current atomic observation.
    pub fn validate(&self) -> Result<(), ProgressiveMetaError> {
        let observation = self.observe_acquire()?;
        if self.0.version != spa_sys::SPA_META_PROGRESSIVE_VERSION
            || self.0.abi_size != spa_sys::SPA_META_PROGRESSIVE_SIZE
            || self.0.reserved0 != 0
            || self.0.reserved1 != 0
            || self.0.payload_size == 0
            || self.0.commit_granularity == 0
            || self.0.commit_granularity > self.0.payload_size
            || observation.snapshot().committed_bytes() > self.0.payload_size
        {
            return Err(ProgressiveMetaError);
        }
        Ok(())
    }
}

impl Debug for MetaProgressive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let observation = self.observe_acquire();
        f.debug_struct("MetaProgressive")
            .field("version", &self.version())
            .field("abi_size", &self.abi_size())
            .field("data_index", &self.data_index())
            .field("payload_offset", &self.payload_offset())
            .field("payload_size", &self.payload_size())
            .field("commit_granularity", &self.commit_granularity())
            .field("observation", &observation)
            .finish()
    }
}

impl Metadata for MetaProgressive {
    const META_TYPE: u32 = spa_sys::SPA_META_Progressive;
}

#[cfg(test)]
mod progressive_tests {
    use super::*;

    #[test]
    fn progressive_metadata_matches_native_version_one_abi() {
        assert_eq!(
            std::mem::size_of::<MetaProgressive>(),
            spa_sys::SPA_META_PROGRESSIVE_SIZE as usize
        );
        assert_eq!(std::mem::align_of::<MetaProgressive>(), 8);
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_progressive, snapshot),
            32
        );
        assert_eq!(
            std::mem::offset_of!(spa_sys::spa_meta_progressive, reserved1),
            40
        );
        assert_eq!(MetaProgressive::META_TYPE, spa_sys::SPA_META_Progressive);
    }

    #[test]
    fn progressive_metadata_publishes_atomic_observations() {
        let mut meta = MetaProgressive::new(1, 128, 4096, 256).unwrap();
        meta.validate().unwrap();
        assert_eq!(
            meta.observe_acquire().unwrap(),
            ProgressiveObservation {
                snapshot: ProgressiveSnapshot::new(0, ProgressiveState::Prepared),
                terminal_flags: None,
            }
        );

        meta.store_release(ProgressiveSnapshot::new(1024, ProgressiveState::Active));
        assert_eq!(meta.load_acquire().unwrap().committed_bytes(), 1024);
        assert_eq!(meta.observe_acquire().unwrap().terminal_flags(), None);

        meta.set_terminal_flags(ProgressiveFlags::INCOMPLETE | ProgressiveFlags::CANCELLED);
        meta.store_release(ProgressiveSnapshot::new(1024, ProgressiveState::Aborted));
        assert_eq!(
            meta.observe_acquire().unwrap().terminal_flags(),
            Some(ProgressiveFlags::INCOMPLETE | ProgressiveFlags::CANCELLED)
        );
        meta.validate().unwrap();
    }

    #[test]
    fn progressive_metadata_rejects_malformed_values() {
        assert!(MetaProgressive::new(0, 0, 0, 1).is_err());
        assert!(MetaProgressive::new(0, 0, 1024, 0).is_err());
        assert!(MetaProgressive::new(0, 0, 1024, 2048).is_err());

        let meta = MetaProgressive::new(0, 0, 1024, 256).unwrap();
        meta.snapshot_atomic().store(
            spa_sys::SPA_META_PROGRESSIVE_RESERVED_MASK as u64,
            std::sync::atomic::Ordering::Release,
        );
        assert_eq!(meta.validate(), Err(ProgressiveMetaError));
    }
}
