// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Types for dealing with SPA formats.

use std::cmp::Ordering;
use std::ffi::CStr;
use std::fmt::Debug;
use std::ops::Range;

use crate::{
    pod::{ChoiceValue, Property, Value, ValueArray},
    utils::{fmt_pascal_case, Choice, ChoiceEnum, ChoiceFlags, Fraction, Id},
};

/// Different media types
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct MediaType(pub spa_sys::spa_media_type);

#[allow(non_upper_case_globals)]
impl MediaType {
    pub const Unknown: Self = Self(spa_sys::SPA_MEDIA_TYPE_unknown);
    pub const Audio: Self = Self(spa_sys::SPA_MEDIA_TYPE_audio);
    pub const Video: Self = Self(spa_sys::SPA_MEDIA_TYPE_video);
    pub const Image: Self = Self(spa_sys::SPA_MEDIA_TYPE_image);
    pub const Binary: Self = Self(spa_sys::SPA_MEDIA_TYPE_binary);
    pub const Stream: Self = Self(spa_sys::SPA_MEDIA_TYPE_stream);
    pub const Application: Self = Self(spa_sys::SPA_MEDIA_TYPE_application);

    /// Obtain a [`MediaType`] from a raw `spa_media_type` variant.
    pub fn from_raw(raw: spa_sys::spa_media_type) -> Self {
        Self(raw)
    }

    /// Get the raw [`spa_sys::spa_media_type`] representing this `MediaType`.
    pub fn as_raw(&self) -> spa_sys::spa_media_type {
        self.0
    }
}

impl Debug for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c_str = unsafe {
            let c_buf = spa_sys::spa_debug_type_find_short_name(
                spa_sys::spa_type_media_type,
                self.as_raw(),
            );
            if c_buf.is_null() {
                return f.write_str("Unsupported media type");
            }
            CStr::from_ptr(c_buf)
        };
        f.write_str("MediaType::")?;
        fmt_pascal_case(f, &c_str.to_string_lossy())
    }
}

/// Different media sub-types
#[derive(PartialEq, PartialOrd, Eq, Clone, Copy)]
pub struct MediaSubtype(pub spa_sys::spa_media_subtype);

#[allow(non_upper_case_globals)]
impl MediaSubtype {
    pub const Unknown: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_unknown);
    pub const Raw: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_raw);
    pub const Dsp: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_dsp);
    /// S/PDIF
    pub const Iec958: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_iec958);
    pub const Dsd: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_dsd);

    pub const Mp3: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_mp3);
    pub const Aac: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_aac);
    pub const Vorbis: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_vorbis);
    pub const Wma: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_wma);
    pub const Ra: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_ra);
    pub const Sbc: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_sbc);
    pub const Adpcm: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_adpcm);
    pub const G723: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_g723);
    pub const G726: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_g726);
    pub const G729: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_g729);
    pub const Amr: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_amr);
    pub const Gsm: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_gsm);
    #[cfg(feature = "v0_3_65")]
    pub const Alac: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_alac);
    #[cfg(feature = "v0_3_65")]
    pub const Flac: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_flac);
    #[cfg(feature = "v0_3_65")]
    pub const Ape: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_ape);
    #[cfg(feature = "v0_3_65")]
    pub const Opus: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_opus);

    pub const H264: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_h264);
    pub const Mjpg: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_mjpg);
    pub const Dv: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_dv);
    pub const Mpegts: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_mpegts);
    pub const H263: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_h263);
    pub const Mpeg1: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_mpeg1);
    pub const Mpeg2: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_mpeg2);
    pub const Mpeg4: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_mpeg4);
    pub const Xvid: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_xvid);
    pub const Vc1: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_vc1);
    pub const Vp8: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_vp8);
    pub const Vp9: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_vp9);
    pub const Bayer: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_bayer);

    pub const Jpeg: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_jpeg);

    pub const Midi: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_midi);

    /// control stream, data contains spa_pod_sequence with control info.
    pub const Control: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_control);
    /// Packed N-dimensional typed elements.
    pub const NdArray: Self = Self(spa_sys::SPA_MEDIA_SUBTYPE_ndarray);

    const AUDIO_RANGE: Range<Self> = Self::Mp3..Self(spa_sys::SPA_MEDIA_SUBTYPE_START_Video);
    const VIDEO_RANGE: Range<Self> = Self::H264..Self(spa_sys::SPA_MEDIA_SUBTYPE_START_Image);
    const IMAGE_RANGE: Range<Self> = Self::Jpeg..Self(spa_sys::SPA_MEDIA_SUBTYPE_START_Binary);
    const BINARY_RANGE: Range<Self> = Self(spa_sys::SPA_MEDIA_SUBTYPE_START_Binary)
        ..Self(spa_sys::SPA_MEDIA_SUBTYPE_START_Stream);
    const STREAM_RANGE: Range<Self> =
        Self::Midi..Self(spa_sys::SPA_MEDIA_SUBTYPE_START_Application);
    const APPLICATION_RANGE: Range<Self> = Self::Control..Self(spa_sys::spa_media_subtype::MAX);

    pub fn is_audio(&self) -> bool {
        Self::AUDIO_RANGE.contains(self)
    }

    pub fn is_video(&self) -> bool {
        Self::VIDEO_RANGE.contains(self)
    }

    pub fn is_image(&self) -> bool {
        Self::IMAGE_RANGE.contains(self)
    }

    pub fn is_binary(&self) -> bool {
        Self::BINARY_RANGE.contains(self)
    }

    pub fn is_stream(&self) -> bool {
        Self::STREAM_RANGE.contains(self)
    }

    pub fn is_application(&self) -> bool {
        Self::APPLICATION_RANGE.contains(self)
    }

    /// Obtain a [`MediaSubtype`] from a raw `spa_media_subtype` variant.
    pub fn from_raw(raw: spa_sys::spa_media_subtype) -> Self {
        Self(raw)
    }

    /// Get the raw [`spa_sys::spa_media_subtype`] representing this `MediaSubtype`.
    pub fn as_raw(&self) -> spa_sys::spa_media_subtype {
        self.0
    }
}

impl Debug for MediaSubtype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c_str = unsafe {
            let c_buf = spa_sys::spa_debug_type_find_short_name(
                spa_sys::spa_type_media_subtype,
                self.as_raw(),
            );
            if c_buf.is_null() {
                return f.write_str("Unsupported media subtype");
            }
            CStr::from_ptr(c_buf)
        };
        f.write_str("MediaSubtype::")?;
        fmt_pascal_case(f, &c_str.to_string_lossy())
    }
}

/// Scalar representation of an [`NdArrayFormat`] element.
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct ElementType(pub spa_sys::spa_element_type);

#[allow(non_upper_case_globals)]
impl ElementType {
    pub const Unknown: Self = Self(spa_sys::SPA_ELEMENT_TYPE_UNKNOWN);
    /// Boolean byte; only zero and one are valid.
    pub const Bool8: Self = Self(spa_sys::SPA_ELEMENT_TYPE_BOOL8);
    pub const I8: Self = Self(spa_sys::SPA_ELEMENT_TYPE_I8);
    pub const U8: Self = Self(spa_sys::SPA_ELEMENT_TYPE_U8);
    pub const I16Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_I16_LE);
    pub const U16Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_U16_LE);
    pub const I32Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_I32_LE);
    pub const U32Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_U32_LE);
    pub const I64Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_I64_LE);
    pub const U64Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_U64_LE);
    pub const I128Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_I128_LE);
    pub const U128Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_U128_LE);
    pub const F8E4M3Fn: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F8_E4M3FN);
    pub const F8E4M3Fnuz: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F8_E4M3FNUZ);
    pub const F8E5M2: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F8_E5M2);
    pub const F8E5M2Fnuz: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F8_E5M2FNUZ);
    /// IEEE 754 binary16, little-endian.
    pub const F16Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F16_LE);
    /// bfloat16, little-endian.
    pub const Bf16Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_BF16_LE);
    /// IEEE 754 binary32, little-endian.
    pub const F32Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F32_LE);
    /// IEEE 754 binary64, little-endian.
    pub const F64Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F64_LE);
    /// IEEE 754 binary128, little-endian.
    pub const F128Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_F128_LE);
    pub const ComplexF16Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_COMPLEX_F16_LE);
    pub const ComplexBf16Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_COMPLEX_BF16_LE);
    pub const ComplexF32Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_COMPLEX_F32_LE);
    pub const ComplexF64Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_COMPLEX_F64_LE);
    pub const ComplexF128Le: Self = Self(spa_sys::SPA_ELEMENT_TYPE_COMPLEX_F128_LE);
    /// First application-defined fixed-width scalar type ID.
    pub const StartCustom: Self = Self(spa_sys::SPA_ELEMENT_TYPE_START_CUSTOM);

    /// Obtain an [`ElementType`] from a raw `spa_element_type` variant.
    pub const fn from_raw(raw: spa_sys::spa_element_type) -> Self {
        Self(raw)
    }

    /// Get the raw `spa_element_type` value.
    pub const fn as_raw(self) -> spa_sys::spa_element_type {
        self.0
    }

    /// Packed size of one element, or `None` for an unsupported value.
    pub const fn size(self) -> Option<usize> {
        match self {
            Self::Bool8
            | Self::I8
            | Self::U8
            | Self::F8E4M3Fn
            | Self::F8E4M3Fnuz
            | Self::F8E5M2
            | Self::F8E5M2Fnuz => Some(1),
            Self::I16Le | Self::U16Le | Self::F16Le | Self::Bf16Le => Some(2),
            Self::I32Le | Self::U32Le | Self::F32Le | Self::ComplexF16Le | Self::ComplexBf16Le => {
                Some(4)
            }
            Self::I64Le | Self::U64Le | Self::F64Le | Self::ComplexF32Le => Some(8),
            Self::I128Le | Self::U128Le | Self::F128Le | Self::ComplexF64Le => Some(16),
            Self::ComplexF128Le => Some(32),
            _ => None,
        }
    }
}

impl Debug for ElementType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Self::Unknown => "ElementType::Unknown",
            Self::Bool8 => "ElementType::Bool8",
            Self::I8 => "ElementType::I8",
            Self::U8 => "ElementType::U8",
            Self::I16Le => "ElementType::I16Le",
            Self::U16Le => "ElementType::U16Le",
            Self::I32Le => "ElementType::I32Le",
            Self::U32Le => "ElementType::U32Le",
            Self::I64Le => "ElementType::I64Le",
            Self::I128Le => "ElementType::I128Le",
            Self::U128Le => "ElementType::U128Le",
            Self::F8E4M3Fn => "ElementType::F8E4M3Fn",
            Self::F8E4M3Fnuz => "ElementType::F8E4M3Fnuz",
            Self::F8E5M2 => "ElementType::F8E5M2",
            Self::F8E5M2Fnuz => "ElementType::F8E5M2Fnuz",
            Self::F16Le => "ElementType::F16Le",
            Self::Bf16Le => "ElementType::Bf16Le",
            Self::F32Le => "ElementType::F32Le",
            Self::F64Le => "ElementType::F64Le",
            Self::F128Le => "ElementType::F128Le",
            Self::ComplexF16Le => "ElementType::ComplexF16Le",
            Self::ComplexBf16Le => "ElementType::ComplexBf16Le",
            Self::ComplexF32Le => "ElementType::ComplexF32Le",
            Self::ComplexF64Le => "ElementType::ComplexF64Le",
            Self::ComplexF128Le => "ElementType::ComplexF128Le",
            Self::StartCustom => "ElementType::StartCustom",
            Self(raw) => return f.debug_tuple("ElementType").field(&raw).finish(),
        })
    }
}

/// Contiguous storage order of an [`NdArrayFormat`].
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct NdArrayLayout(pub spa_sys::spa_ndarray_layout);

#[allow(non_upper_case_globals)]
impl NdArrayLayout {
    pub const Unknown: Self = Self(spa_sys::SPA_NDARRAY_LAYOUT_UNKNOWN);
    /// The last logical axis is contiguous.
    pub const RowMajor: Self = Self(spa_sys::SPA_NDARRAY_LAYOUT_ROW_MAJOR);
    /// The first logical axis is contiguous.
    pub const ColumnMajor: Self = Self(spa_sys::SPA_NDARRAY_LAYOUT_COLUMN_MAJOR);

    /// Obtain an [`NdArrayLayout`] from a raw `spa_ndarray_layout` variant.
    pub const fn from_raw(raw: spa_sys::spa_ndarray_layout) -> Self {
        Self(raw)
    }

    /// Get the raw `spa_ndarray_layout` value.
    pub const fn as_raw(self) -> spa_sys::spa_ndarray_layout {
        self.0
    }
}

impl Debug for NdArrayLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Self::Unknown => "NdArrayLayout::Unknown",
            Self::RowMajor => "NdArrayLayout::RowMajor",
            Self::ColumnMajor => "NdArrayLayout::ColumnMajor",
            Self(raw) => return f.debug_tuple("NdArrayLayout").field(&raw).finish(),
        })
    }
}

/// Invalid native ndarray, vector, or matrix format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NdArrayFormatError {
    UnsupportedElementType,
    UnsupportedLayout,
    EmptyShape,
    ZeroDimension { axis: usize },
    DimensionTooLarge { axis: usize },
    ElementCountOverflow,
    ByteCountOverflow,
    InvalidRate,
    MissingProperty(FormatProperties),
    DuplicateProperty(FormatProperties),
    InvalidProperty(FormatProperties),
    WrongRank { expected: usize, actual: usize },
    NonCanonicalVectorLayout,
    UnsupportedElementTypeAlternative { index: usize },
    UnsupportedLayoutAlternative { index: usize },
    MissingRateForChoice,
    InvalidRateChoice,
}

impl std::fmt::Display for NdArrayFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::UnsupportedElementType => f.write_str("unsupported ndarray element type"),
            Self::UnsupportedLayout => f.write_str("unsupported ndarray layout"),
            Self::EmptyShape => f.write_str("ndarray shape must contain at least one dimension"),
            Self::ZeroDimension { axis } => write!(f, "ndarray dimension {axis} is zero"),
            Self::DimensionTooLarge { axis } => {
                write!(f, "ndarray dimension {axis} does not fit a SPA Int")
            }
            Self::ElementCountOverflow => f.write_str("ndarray element count overflows usize"),
            Self::ByteCountOverflow => f.write_str("ndarray byte count overflows usize"),
            Self::InvalidRate => f.write_str("ndarray rate must be a positive fraction"),
            Self::MissingProperty(property) => {
                write!(f, "missing required ndarray property {property:?}")
            }
            Self::DuplicateProperty(property) => {
                write!(f, "duplicate ndarray property {property:?}")
            }
            Self::InvalidProperty(property) => {
                write!(f, "invalid ndarray property {property:?}")
            }
            Self::WrongRank { expected, actual } => {
                write!(f, "expected ndarray rank {expected}, got {actual}")
            }
            Self::NonCanonicalVectorLayout => {
                f.write_str("rank-one vectors must use the canonical row-major layout")
            }
            Self::UnsupportedElementTypeAlternative { index } => {
                write!(f, "unsupported ndarray element-type alternative {index}")
            }
            Self::UnsupportedLayoutAlternative { index } => {
                write!(f, "unsupported ndarray layout alternative {index}")
            }
            Self::MissingRateForChoice => {
                f.write_str("an ndarray rate choice requires a default rate")
            }
            Self::InvalidRateChoice => f.write_str("invalid ndarray rate choice"),
        }
    }
}

fn property_value(
    properties: &[Property],
    key: FormatProperties,
    required: bool,
) -> Result<Option<&Value>, NdArrayFormatError> {
    let mut matching = properties
        .iter()
        .filter(|property| property.key == key.as_raw());
    let value = matching.next().map(|property| &property.value);
    if matching.next().is_some() {
        return Err(NdArrayFormatError::DuplicateProperty(key));
    }
    if required && value.is_none() {
        return Err(NdArrayFormatError::MissingProperty(key));
    }
    Ok(value)
}

impl NdArrayFormat<Vec<u32>> {
    /// Parse a fixed native ndarray format while ignoring application properties.
    pub fn from_properties(properties: &[Property]) -> Result<Self, NdArrayFormatError> {
        let media_type = property_value(properties, FormatProperties::MediaType, true)?;
        if media_type != Some(&Value::Id(Id(MediaType::Application.as_raw()))) {
            return Err(NdArrayFormatError::InvalidProperty(
                FormatProperties::MediaType,
            ));
        }
        let media_subtype = property_value(properties, FormatProperties::MediaSubtype, true)?;
        if media_subtype != Some(&Value::Id(Id(MediaSubtype::NdArray.as_raw()))) {
            return Err(NdArrayFormatError::InvalidProperty(
                FormatProperties::MediaSubtype,
            ));
        }

        let element_type =
            match property_value(properties, FormatProperties::NdArrayElementType, true)? {
                Some(Value::Id(Id(raw))) => ElementType::from_raw(*raw),
                _ => {
                    return Err(NdArrayFormatError::InvalidProperty(
                        FormatProperties::NdArrayElementType,
                    ));
                }
            };
        let shape = match property_value(properties, FormatProperties::NdArrayShape, true)? {
            Some(Value::ValueArray(ValueArray::Int(shape))) => shape
                .iter()
                .map(|&dimension| {
                    u32::try_from(dimension).map_err(|_| {
                        NdArrayFormatError::InvalidProperty(FormatProperties::NdArrayShape)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => {
                return Err(NdArrayFormatError::InvalidProperty(
                    FormatProperties::NdArrayShape,
                ));
            }
        };
        let layout = match property_value(properties, FormatProperties::NdArrayLayout, true)? {
            Some(Value::Id(Id(raw))) => NdArrayLayout::from_raw(*raw),
            _ => {
                return Err(NdArrayFormatError::InvalidProperty(
                    FormatProperties::NdArrayLayout,
                ));
            }
        };
        let rate = match property_value(properties, FormatProperties::NdArrayRate, false)? {
            Some(Value::Fraction(rate)) => Some(*rate),
            Some(_) => {
                return Err(NdArrayFormatError::InvalidProperty(
                    FormatProperties::NdArrayRate,
                ));
            }
            None => None,
        };
        Self::new(element_type, shape, layout, rate)
    }
}

impl std::error::Error for NdArrayFormatError {}

/// Native packed ndarray format with a caller-selected shape container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdArrayFormat<S> {
    element_type: ElementType,
    shape: S,
    layout: NdArrayLayout,
    rate: Option<Fraction>,
}

impl<S: AsRef<[u32]>> NdArrayFormat<S> {
    pub fn new(
        element_type: ElementType,
        shape: S,
        layout: NdArrayLayout,
        rate: Option<Fraction>,
    ) -> Result<Self, NdArrayFormatError> {
        let element_size = element_type
            .size()
            .ok_or(NdArrayFormatError::UnsupportedElementType)?;
        if layout != NdArrayLayout::RowMajor && layout != NdArrayLayout::ColumnMajor {
            return Err(NdArrayFormatError::UnsupportedLayout);
        }
        if rate.is_some_and(|rate| rate.num == 0 || rate.denom == 0) {
            return Err(NdArrayFormatError::InvalidRate);
        }

        let dimensions = shape.as_ref();
        if dimensions.is_empty() {
            return Err(NdArrayFormatError::EmptyShape);
        }
        let mut element_count = 1usize;
        for (axis, &dimension) in dimensions.iter().enumerate() {
            if dimension == 0 {
                return Err(NdArrayFormatError::ZeroDimension { axis });
            }
            if dimension > i32::MAX as u32 {
                return Err(NdArrayFormatError::DimensionTooLarge { axis });
            }
            element_count = element_count
                .checked_mul(dimension as usize)
                .ok_or(NdArrayFormatError::ElementCountOverflow)?;
        }
        element_count
            .checked_mul(element_size)
            .ok_or(NdArrayFormatError::ByteCountOverflow)?;

        Ok(Self {
            element_type,
            shape,
            layout,
            rate,
        })
    }

    pub fn element_type(&self) -> ElementType {
        self.element_type
    }

    pub fn shape(&self) -> &[u32] {
        self.shape.as_ref()
    }

    pub fn layout(&self) -> NdArrayLayout {
        self.layout
    }

    pub fn rate(&self) -> Option<Fraction> {
        self.rate
    }

    pub fn element_count(&self) -> usize {
        self.shape()
            .iter()
            .fold(1usize, |count, &dimension| count * dimension as usize)
    }

    pub fn byte_count(&self) -> usize {
        self.element_count() * self.element_type.size().expect("validated element type")
    }

    /// Build the native SPA properties for this format.
    ///
    /// Application-specific semantic properties can be appended to the result.
    pub fn properties(&self) -> Vec<Property> {
        let mut properties = Vec::with_capacity(if self.rate.is_some() { 6 } else { 5 });
        properties.push(Property::new(
            FormatProperties::MediaType.as_raw(),
            Value::Id(Id(MediaType::Application.as_raw())),
        ));
        properties.push(Property::new(
            FormatProperties::MediaSubtype.as_raw(),
            Value::Id(Id(MediaSubtype::NdArray.as_raw())),
        ));
        properties.push(Property::new(
            FormatProperties::NdArrayElementType.as_raw(),
            Value::Id(Id(self.element_type.as_raw())),
        ));
        properties.push(Property::new(
            FormatProperties::NdArrayShape.as_raw(),
            Value::ValueArray(ValueArray::Int(
                self.shape()
                    .iter()
                    .map(|&dimension| dimension as i32)
                    .collect(),
            )),
        ));
        properties.push(Property::new(
            FormatProperties::NdArrayLayout.as_raw(),
            Value::Id(Id(self.layout.as_raw())),
        ));
        if let Some(rate) = self.rate {
            properties.push(Property::new(
                FormatProperties::NdArrayRate.as_raw(),
                Value::Fraction(rate),
            ));
        }
        properties
    }
}

/// Alternatives for the default rate of an enumerated ndarray format.
///
/// The default itself comes from [`NdArrayFormat::rate`]. Enum alternatives
/// do not need to repeat it; serialization adds the repetition required by
/// SPA's preference-plus-admissible-list representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NdArrayRateChoice {
    /// Discrete additional rates.
    Enum(Vec<Fraction>),
    /// Inclusive rate range.
    Range { min: Fraction, max: Fraction },
    /// Inclusive rate range and positive step.
    Step {
        min: Fraction,
        max: Fraction,
        step: Fraction,
    },
}

fn fraction_is_positive(value: Fraction) -> bool {
    value.num != 0 && value.denom != 0
}

fn fraction_cmp(left: Fraction, right: Fraction) -> Ordering {
    let left_product = u64::from(left.num) * u64::from(right.denom);
    let right_product = u64::from(right.num) * u64::from(left.denom);
    left_product.cmp(&right_product)
}

fn validate_rate_choice(
    default: Option<Fraction>,
    choice: &NdArrayRateChoice,
) -> Result<(), NdArrayFormatError> {
    let default = default.ok_or(NdArrayFormatError::MissingRateForChoice)?;
    match choice {
        NdArrayRateChoice::Enum(alternatives) => {
            if alternatives.is_empty()
                || alternatives
                    .iter()
                    .copied()
                    .any(|rate| !fraction_is_positive(rate))
            {
                return Err(NdArrayFormatError::InvalidRateChoice);
            }
        }
        NdArrayRateChoice::Range { min, max } | NdArrayRateChoice::Step { min, max, .. } => {
            if !fraction_is_positive(*min)
                || !fraction_is_positive(*max)
                || fraction_cmp(*min, default) == Ordering::Greater
                || fraction_cmp(default, *max) == Ordering::Greater
            {
                return Err(NdArrayFormatError::InvalidRateChoice);
            }
            if let NdArrayRateChoice::Step { step, .. } = choice {
                if !fraction_is_positive(*step) {
                    return Err(NdArrayFormatError::InvalidRateChoice);
                }
            }
        }
    }
    Ok(())
}

/// One exact ndarray shape with negotiable scalar, layout, and rate values.
///
/// Endpoints supporting more than one shape expose one `NdArrayEnumFormat`
/// per shape. This keeps shape negotiation unambiguous and compatible with
/// the ordinary SPA POD filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdArrayEnumFormat<S> {
    default: NdArrayFormat<S>,
    element_type_alternatives: Vec<ElementType>,
    layout_alternatives: Vec<NdArrayLayout>,
    rate_choice: Option<NdArrayRateChoice>,
}

impl<S: AsRef<[u32]>> NdArrayEnumFormat<S> {
    /// Start an enumerated format from a validated fixed default.
    pub fn new(default: NdArrayFormat<S>) -> Self {
        Self {
            default,
            element_type_alternatives: Vec::new(),
            layout_alternatives: Vec::new(),
            rate_choice: None,
        }
    }

    pub fn default(&self) -> &NdArrayFormat<S> {
        &self.default
    }

    pub fn with_element_type_alternatives(
        mut self,
        alternatives: impl IntoIterator<Item = ElementType>,
    ) -> Result<Self, NdArrayFormatError> {
        self.element_type_alternatives = alternatives.into_iter().collect();
        for (index, element_type) in self.element_type_alternatives.iter().enumerate() {
            if element_type.size().is_none() {
                return Err(NdArrayFormatError::UnsupportedElementTypeAlternative { index });
            }
        }
        Ok(self)
    }

    pub fn with_layout_alternatives(
        mut self,
        alternatives: impl IntoIterator<Item = NdArrayLayout>,
    ) -> Result<Self, NdArrayFormatError> {
        self.layout_alternatives = alternatives.into_iter().collect();
        for (index, layout) in self.layout_alternatives.iter().enumerate() {
            if *layout != NdArrayLayout::RowMajor && *layout != NdArrayLayout::ColumnMajor {
                return Err(NdArrayFormatError::UnsupportedLayoutAlternative { index });
            }
        }
        Ok(self)
    }

    pub fn with_rate_choice(
        mut self,
        choice: NdArrayRateChoice,
    ) -> Result<Self, NdArrayFormatError> {
        validate_rate_choice(self.default.rate(), &choice)?;
        self.rate_choice = Some(choice);
        Ok(self)
    }

    /// Build an `SPA_PARAM_EnumFormat` property set.
    pub fn properties(&self) -> Vec<Property> {
        let mut properties = self.default.properties();

        if !self.element_type_alternatives.is_empty() {
            let default = Id(self.default.element_type().as_raw());
            let mut alternatives = Vec::with_capacity(self.element_type_alternatives.len() + 1);
            alternatives.push(default);
            alternatives.extend(
                self.element_type_alternatives
                    .iter()
                    .map(|element_type| Id(element_type.as_raw())),
            );
            properties[2].value = Value::Choice(ChoiceValue::Id(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default,
                    alternatives,
                },
            )));
        }

        if !self.layout_alternatives.is_empty() {
            let default = Id(self.default.layout().as_raw());
            let mut alternatives = Vec::with_capacity(self.layout_alternatives.len() + 1);
            alternatives.push(default);
            alternatives.extend(
                self.layout_alternatives
                    .iter()
                    .map(|layout| Id(layout.as_raw())),
            );
            properties[4].value = Value::Choice(ChoiceValue::Id(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default,
                    alternatives,
                },
            )));
        }

        if let Some(choice) = &self.rate_choice {
            let default = self.default.rate().expect("validated rate choice");
            let choice = match choice {
                NdArrayRateChoice::Enum(additional) => {
                    let mut alternatives = Vec::with_capacity(additional.len() + 1);
                    alternatives.push(default);
                    alternatives.extend(additional.iter().copied());
                    ChoiceEnum::Enum {
                        default,
                        alternatives,
                    }
                }
                NdArrayRateChoice::Range { min, max } => ChoiceEnum::Range {
                    default,
                    min: *min,
                    max: *max,
                },
                NdArrayRateChoice::Step { min, max, step } => ChoiceEnum::Step {
                    default,
                    min: *min,
                    max: *max,
                    step: *step,
                },
            };
            let rate = properties
                .iter_mut()
                .find(|property| property.key == FormatProperties::NdArrayRate.as_raw())
                .expect("validated default rate");
            rate.value = Value::Choice(ChoiceValue::Fraction(Choice(ChoiceFlags::empty(), choice)));
        }

        properties
    }
}

/// Canonical rank-one ndarray format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorFormat(NdArrayFormat<[u32; 1]>);

impl VectorFormat {
    pub fn new(
        element_type: ElementType,
        length: u32,
        rate: Option<Fraction>,
    ) -> Result<Self, NdArrayFormatError> {
        NdArrayFormat::new(element_type, [length], NdArrayLayout::RowMajor, rate).map(Self)
    }

    pub fn as_ndarray(&self) -> &NdArrayFormat<[u32; 1]> {
        &self.0
    }

    pub fn properties(&self) -> Vec<Property> {
        self.0.properties()
    }

    pub fn enum_format(&self) -> NdArrayEnumFormat<[u32; 1]> {
        NdArrayEnumFormat::new(self.0.clone())
    }

    /// Parse a fixed native rank-one format.
    pub fn from_properties(properties: &[Property]) -> Result<Self, NdArrayFormatError> {
        let format = NdArrayFormat::<Vec<u32>>::from_properties(properties)?;
        let [length] = format.shape() else {
            return Err(NdArrayFormatError::WrongRank {
                expected: 1,
                actual: format.shape().len(),
            });
        };
        if format.layout() != NdArrayLayout::RowMajor {
            return Err(NdArrayFormatError::NonCanonicalVectorLayout);
        }
        Self::new(format.element_type(), *length, format.rate())
    }
}

/// Rank-two ndarray format with explicit row-major or column-major storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixFormat(NdArrayFormat<[u32; 2]>);

impl MatrixFormat {
    pub fn new(
        element_type: ElementType,
        rows: u32,
        columns: u32,
        layout: NdArrayLayout,
        rate: Option<Fraction>,
    ) -> Result<Self, NdArrayFormatError> {
        NdArrayFormat::new(element_type, [rows, columns], layout, rate).map(Self)
    }

    pub fn as_ndarray(&self) -> &NdArrayFormat<[u32; 2]> {
        &self.0
    }

    pub fn properties(&self) -> Vec<Property> {
        self.0.properties()
    }

    pub fn enum_format(&self) -> NdArrayEnumFormat<[u32; 2]> {
        NdArrayEnumFormat::new(self.0.clone())
    }

    /// Parse a fixed native rank-two format in either contiguous storage order.
    pub fn from_properties(properties: &[Property]) -> Result<Self, NdArrayFormatError> {
        let format = NdArrayFormat::<Vec<u32>>::from_properties(properties)?;
        let [rows, columns] = format.shape() else {
            return Err(NdArrayFormatError::WrongRank {
                expected: 2,
                actual: format.shape().len(),
            });
        };
        Self::new(
            format.element_type(),
            *rows,
            *columns,
            format.layout(),
            format.rate(),
        )
    }
}

#[derive(PartialEq, PartialOrd, Eq, Clone, Copy)]
pub struct FormatProperties(pub spa_sys::spa_format);

#[allow(non_upper_case_globals)]
impl FormatProperties {
    /// media type (Id enum spa_media_type)
    pub const MediaType: Self = Self(spa_sys::SPA_FORMAT_mediaType);
    /// media subtype (Id enum spa_media_subtype)
    pub const MediaSubtype: Self = Self(spa_sys::SPA_FORMAT_mediaSubtype);

    /// audio format, (Id enum spa_audio_format)
    pub const AudioFormat: Self = Self(spa_sys::SPA_FORMAT_AUDIO_format);
    /// optional flags (Int)
    pub const AudioFlags: Self = Self(spa_sys::SPA_FORMAT_AUDIO_flags);
    /// sample rate (Int)
    pub const AudioRate: Self = Self(spa_sys::SPA_FORMAT_AUDIO_rate);
    /// number of audio channels (Int)
    pub const AudioChannels: Self = Self(spa_sys::SPA_FORMAT_AUDIO_channels);
    /// channel positions (Id enum spa_audio_position)
    pub const AudioPosition: Self = Self(spa_sys::SPA_FORMAT_AUDIO_position);

    /// codec used (IEC958) (Id enum spa_audio_iec958_codec)
    pub const AudioIec958Codec: Self = Self(spa_sys::SPA_FORMAT_AUDIO_iec958Codec);

    /// bit order (Id enum spa_param_bitorder)
    pub const AudioBitorder: Self = Self(spa_sys::SPA_FORMAT_AUDIO_bitorder);
    /// Interleave bytes (Int)
    pub const AudioInterleave: Self = Self(spa_sys::SPA_FORMAT_AUDIO_interleave);
    /// bit rate (Int)
    #[cfg(feature = "v0_3_65")]
    pub const AudioBitrate: Self = Self(spa_sys::SPA_FORMAT_AUDIO_bitrate);
    /// audio data block alignment (Int)
    #[cfg(feature = "v0_3_65")]
    pub const AudioBlockAlign: Self = Self(spa_sys::SPA_FORMAT_AUDIO_blockAlign);

    /// AAC stream format, (Id enum spa_audio_aac_stream_format)
    #[cfg(feature = "v0_3_65")]
    pub const AudioAacStreamFormat: Self = Self(spa_sys::SPA_FORMAT_AUDIO_AAC_streamFormat);

    /// WMA profile (Id enum spa_audio_wma_profile)
    #[cfg(feature = "v0_3_65")]
    pub const AudioWmaProfile: Self = Self(spa_sys::SPA_FORMAT_AUDIO_WMA_profile);

    /// AMR band mode (Id enum spa_audio_amr_band_mode)
    #[cfg(feature = "v0_3_65")]
    pub const AudioAmrBandMode: Self = Self(spa_sys::SPA_FORMAT_AUDIO_AMR_bandMode);

    /// video format (Id enum spa_video_format)
    pub const VideoFormat: Self = Self(spa_sys::SPA_FORMAT_VIDEO_format);
    /// format modifier (Long), use only with DMA-BUF and omit for other buffer types
    pub const VideoModifier: Self = Self(spa_sys::SPA_FORMAT_VIDEO_modifier);
    /// size (Rectangle)
    pub const VideoSize: Self = Self(spa_sys::SPA_FORMAT_VIDEO_size);
    /// frame rate (Fraction)
    pub const VideoFramerate: Self = Self(spa_sys::SPA_FORMAT_VIDEO_framerate);
    /// maximum frame rate (Fraction)
    pub const VideoMaxFramerate: Self = Self(spa_sys::SPA_FORMAT_VIDEO_maxFramerate);
    /// number of views (Int)
    pub const VideoViews: Self = Self(spa_sys::SPA_FORMAT_VIDEO_views);
    /// (Id enum spa_video_interlace_mode)
    pub const VideoInterlaceMode: Self = Self(spa_sys::SPA_FORMAT_VIDEO_interlaceMode);
    /// (Rectangle)
    pub const VideoPixelAspectRatio: Self = Self(spa_sys::SPA_FORMAT_VIDEO_pixelAspectRatio);
    /// (Id enum spa_video_multiview_mode)
    pub const VideoMultiviewMode: Self = Self(spa_sys::SPA_FORMAT_VIDEO_multiviewMode);
    /// (Id enum spa_video_multiview_flags)
    pub const VideoMultiviewFlags: Self = Self(spa_sys::SPA_FORMAT_VIDEO_multiviewFlags);
    /// /Id enum spa_video_chroma_site)
    pub const VideoChromaSite: Self = Self(spa_sys::SPA_FORMAT_VIDEO_chromaSite);
    /// /Id enum spa_video_color_range)
    pub const VideoColorRange: Self = Self(spa_sys::SPA_FORMAT_VIDEO_colorRange);
    /// /Id enum spa_video_color_matrix)
    pub const VideoColorMatrix: Self = Self(spa_sys::SPA_FORMAT_VIDEO_colorMatrix);
    /// /Id enum spa_video_transfer_function)
    pub const VideoTransferFunction: Self = Self(spa_sys::SPA_FORMAT_VIDEO_transferFunction);
    ///  /Id enum spa_video_color_primaries)
    pub const VideoColorPrimaries: Self = Self(spa_sys::SPA_FORMAT_VIDEO_colorPrimaries);
    /// (Int)
    pub const VideoProfile: Self = Self(spa_sys::SPA_FORMAT_VIDEO_profile);
    /// (Int)
    pub const VideoLevel: Self = Self(spa_sys::SPA_FORMAT_VIDEO_level);
    /// (Id enum spa_h264_stream_format)
    pub const VideoH264StreamFormat: Self = Self(spa_sys::SPA_FORMAT_VIDEO_H264_streamFormat);
    /// (Id enum spa_h264_alignment)
    pub const VideoH264Alignment: Self = Self(spa_sys::SPA_FORMAT_VIDEO_H264_alignment);

    /// ndarray element type (Id enum spa_element_type)
    pub const NdArrayElementType: Self = Self(spa_sys::SPA_FORMAT_NDARRAY_elementType);
    /// positive logical dimensions (Array of Int)
    pub const NdArrayShape: Self = Self(spa_sys::SPA_FORMAT_NDARRAY_shape);
    /// contiguous storage order (Id enum spa_ndarray_layout)
    pub const NdArrayLayout: Self = Self(spa_sys::SPA_FORMAT_NDARRAY_layout);
    /// optional sample rate (Fraction)
    pub const NdArrayRate: Self = Self(spa_sys::SPA_FORMAT_NDARRAY_rate);

    const AUDIO_RANGE: Range<Self> = Self::AudioFormat..Self(spa_sys::SPA_FORMAT_START_Video);
    const VIDEO_RANGE: Range<Self> = Self::VideoFormat..Self(spa_sys::SPA_FORMAT_START_Image);
    const IMAGE_RANGE: Range<Self> =
        Self(spa_sys::SPA_FORMAT_START_Image)..Self(spa_sys::SPA_FORMAT_START_Binary);
    const BINARY_RANGE: Range<Self> =
        Self(spa_sys::SPA_FORMAT_START_Binary)..Self(spa_sys::SPA_FORMAT_START_Stream);
    const STREAM_RANGE: Range<Self> =
        Self(spa_sys::SPA_FORMAT_START_Stream)..Self(spa_sys::SPA_FORMAT_START_Application);
    const APPLICATION_RANGE: Range<Self> =
        Self(spa_sys::SPA_FORMAT_START_Application)..Self(spa_sys::spa_format::MAX);

    pub fn is_audio(&self) -> bool {
        Self::AUDIO_RANGE.contains(self)
    }

    pub fn is_video(&self) -> bool {
        Self::VIDEO_RANGE.contains(self)
    }

    pub fn is_image(&self) -> bool {
        Self::IMAGE_RANGE.contains(self)
    }

    pub fn is_binary(&self) -> bool {
        Self::BINARY_RANGE.contains(self)
    }

    pub fn is_stream(&self) -> bool {
        Self::STREAM_RANGE.contains(self)
    }

    pub fn is_application(&self) -> bool {
        Self::APPLICATION_RANGE.contains(self)
    }

    /// Obtain a [`FormatProperties`] from a raw `spa_format` variant.
    pub fn from_raw(raw: spa_sys::spa_format) -> Self {
        Self(raw)
    }

    /// Get the raw [`spa_sys::spa_format`] representing this `FormatProperties`.
    pub fn as_raw(&self) -> spa_sys::spa_format {
        self.0
    }
}

impl Debug for FormatProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c_str = unsafe {
            let c_buf = spa_sys::spa_debug_type_find_name(spa_sys::spa_type_format, self.as_raw());
            if c_buf.is_null() {
                return f.write_str("Unsupported format");
            }
            CStr::from_ptr(c_buf)
        };
        let replaced = c_str
            .to_string_lossy()
            .replace("Spa:Pod:Object:Param:Format:", "")
            .replace(':', " ");
        f.write_str("FormatProperties::")?;
        fmt_pascal_case(f, &replaced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn debug_format() {
        assert_eq!("MediaType::Audio", format!("{:?}", MediaType::Audio));
        assert_eq!("MediaSubtype::Raw", format!("{:?}", MediaSubtype::Raw));
        assert_eq!(
            "MediaSubtype::Ndarray",
            format!("{:?}", MediaSubtype::NdArray)
        );
        assert_eq!(
            "FormatProperties::VideoTransferFunction",
            format!("{:?}", FormatProperties::VideoTransferFunction)
        );
        assert_eq!(
            "FormatProperties::NdArrayElementType",
            format!("{:?}", FormatProperties::NdArrayElementType)
        );
    }

    #[test]
    fn ndarray_profiles_validate_shape_layout_and_size() {
        let vector = VectorFormat::new(
            ElementType::F32Le,
            2048,
            Some(Fraction {
                num: 1000,
                denom: 1,
            }),
        )
        .unwrap();
        assert_eq!(vector.as_ndarray().shape(), &[2048]);
        assert_eq!(vector.as_ndarray().layout(), NdArrayLayout::RowMajor);
        assert_eq!(vector.as_ndarray().byte_count(), 8192);

        let matrix =
            MatrixFormat::new(ElementType::F64Le, 48, 64, NdArrayLayout::ColumnMajor, None)
                .unwrap();
        assert_eq!(matrix.as_ndarray().shape(), &[48, 64]);
        assert_eq!(matrix.as_ndarray().layout(), NdArrayLayout::ColumnMajor);
        assert_eq!(matrix.as_ndarray().byte_count(), 48 * 64 * 8);

        assert_eq!(
            VectorFormat::new(ElementType::F32Le, 0, None),
            Err(NdArrayFormatError::ZeroDimension { axis: 0 })
        );
        assert_eq!(
            MatrixFormat::new(ElementType::Unknown, 48, 64, NdArrayLayout::RowMajor, None),
            Err(NdArrayFormatError::UnsupportedElementType)
        );
        assert_eq!(
            MatrixFormat::new(ElementType::F32Le, 48, 64, NdArrayLayout::Unknown, None),
            Err(NdArrayFormatError::UnsupportedLayout)
        );
    }

    #[test]
    fn ndarray_element_types_cover_native_scalar_vocabulary() {
        let expected = [
            (ElementType::Bool8, 1),
            (ElementType::I8, 1),
            (ElementType::U8, 1),
            (ElementType::I16Le, 2),
            (ElementType::U16Le, 2),
            (ElementType::I32Le, 4),
            (ElementType::U32Le, 4),
            (ElementType::I64Le, 8),
            (ElementType::U64Le, 8),
            (ElementType::I128Le, 16),
            (ElementType::U128Le, 16),
            (ElementType::F8E4M3Fn, 1),
            (ElementType::F8E4M3Fnuz, 1),
            (ElementType::F8E5M2, 1),
            (ElementType::F8E5M2Fnuz, 1),
            (ElementType::F16Le, 2),
            (ElementType::Bf16Le, 2),
            (ElementType::F32Le, 4),
            (ElementType::F64Le, 8),
            (ElementType::F128Le, 16),
            (ElementType::ComplexF16Le, 4),
            (ElementType::ComplexBf16Le, 4),
            (ElementType::ComplexF32Le, 8),
            (ElementType::ComplexF64Le, 16),
            (ElementType::ComplexF128Le, 32),
        ];

        for (element_type, size) in expected {
            assert_eq!(element_type.size(), Some(size));
        }
        assert_eq!(ElementType::Unknown.size(), None);
        assert_eq!(ElementType::StartCustom.size(), None);
    }

    #[test]
    fn ndarray_c_information_helpers_are_bound() {
        let mut shape = [0; spa_sys::SPA_NDARRAY_MAX_DIMENSIONS as usize];
        shape[..2].copy_from_slice(&[48, 64]);
        let info = spa_sys::spa_ndarray_info {
            element_type: spa_sys::SPA_ELEMENT_TYPE_F32_LE,
            layout: spa_sys::SPA_NDARRAY_LAYOUT_COLUMN_MAJOR,
            rate: Fraction {
                num: 1000,
                denom: 1,
            },
            n_dimensions: 2,
            shape,
        };
        let mut n_elements = 0;
        let mut n_bytes = 0;

        assert_eq!(unsafe { spa_sys::spa_ndarray_info_validate(&info) }, 0);
        assert_eq!(
            unsafe { spa_sys::spa_ndarray_info_get_n_elements(&info, &mut n_elements) },
            0
        );
        assert_eq!(
            unsafe { spa_sys::spa_ndarray_info_get_size(&info, &mut n_bytes) },
            0
        );
        assert_eq!(n_elements, 48 * 64);
        assert_eq!(n_bytes, 48 * 64 * std::mem::size_of::<f32>());
    }

    #[test]
    fn ndarray_properties_use_native_shape_and_layout_ids() {
        let matrix =
            MatrixFormat::new(ElementType::F32Le, 3, 5, NdArrayLayout::ColumnMajor, None).unwrap();
        let properties = matrix.properties();

        assert_eq!(properties.len(), 5);
        assert_eq!(
            properties[1],
            Property::new(
                FormatProperties::MediaSubtype.as_raw(),
                Value::Id(Id(MediaSubtype::NdArray.as_raw()))
            )
        );
        assert_eq!(
            properties[3],
            Property::new(
                FormatProperties::NdArrayShape.as_raw(),
                Value::ValueArray(ValueArray::Int(vec![3, 5]))
            )
        );
        assert_eq!(
            properties[4],
            Property::new(
                FormatProperties::NdArrayLayout.as_raw(),
                Value::Id(Id(NdArrayLayout::ColumnMajor.as_raw()))
            )
        );
        assert_eq!(MatrixFormat::from_properties(&properties).unwrap(), matrix);
        assert_eq!(
            VectorFormat::from_properties(&properties),
            Err(NdArrayFormatError::WrongRank {
                expected: 1,
                actual: 2,
            })
        );
    }

    #[test]
    fn ndarray_property_parser_rejects_missing_duplicate_and_noncanonical_values() {
        let vector = VectorFormat::new(ElementType::F32Le, 8, None).unwrap();
        let mut properties = vector.properties();
        properties.retain(|property| property.key != FormatProperties::NdArrayShape.as_raw());
        assert_eq!(
            VectorFormat::from_properties(&properties),
            Err(NdArrayFormatError::MissingProperty(
                FormatProperties::NdArrayShape
            ))
        );

        let mut properties = vector.properties();
        properties.push(properties[3].clone());
        assert_eq!(
            VectorFormat::from_properties(&properties),
            Err(NdArrayFormatError::DuplicateProperty(
                FormatProperties::NdArrayShape
            ))
        );

        let mut properties = vector.properties();
        properties[4] = Property::new(
            FormatProperties::NdArrayLayout.as_raw(),
            Value::Id(Id(NdArrayLayout::ColumnMajor.as_raw())),
        );
        assert_eq!(
            VectorFormat::from_properties(&properties),
            Err(NdArrayFormatError::NonCanonicalVectorLayout)
        );
    }

    #[test]
    fn ndarray_enum_format_uses_standard_choices_and_exact_shape() {
        let default = MatrixFormat::new(
            ElementType::F64Le,
            16,
            16,
            NdArrayLayout::ColumnMajor,
            Some(Fraction {
                num: 1000,
                denom: 1,
            }),
        )
        .unwrap();
        let format = default
            .enum_format()
            .with_element_type_alternatives([ElementType::F32Le])
            .unwrap()
            .with_layout_alternatives([NdArrayLayout::RowMajor])
            .unwrap()
            .with_rate_choice(NdArrayRateChoice::Range {
                min: Fraction { num: 500, denom: 1 },
                max: Fraction {
                    num: 2000,
                    denom: 1,
                },
            })
            .unwrap();
        let properties = format.properties();

        assert_eq!(
            properties[2].value,
            Value::Choice(ChoiceValue::Id(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default: Id(ElementType::F64Le.as_raw()),
                    alternatives: vec![
                        Id(ElementType::F64Le.as_raw()),
                        Id(ElementType::F32Le.as_raw()),
                    ],
                },
            )))
        );
        assert_eq!(
            properties[3].value,
            Value::ValueArray(ValueArray::Int(vec![16, 16]))
        );
        assert_eq!(
            properties[4].value,
            Value::Choice(ChoiceValue::Id(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default: Id(NdArrayLayout::ColumnMajor.as_raw()),
                    alternatives: vec![
                        Id(NdArrayLayout::ColumnMajor.as_raw()),
                        Id(NdArrayLayout::RowMajor.as_raw()),
                    ],
                },
            )))
        );
        assert_eq!(
            properties[5].value,
            Value::Choice(ChoiceValue::Fraction(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Range {
                    default: Fraction {
                        num: 1000,
                        denom: 1,
                    },
                    min: Fraction { num: 500, denom: 1 },
                    max: Fraction {
                        num: 2000,
                        denom: 1,
                    },
                },
            )))
        );
    }

    #[test]
    fn ndarray_enum_format_rejects_invalid_alternatives() {
        let without_rate = VectorFormat::new(ElementType::F32Le, 8, None).unwrap();
        assert_eq!(
            without_rate
                .enum_format()
                .with_rate_choice(NdArrayRateChoice::Range {
                    min: Fraction { num: 1, denom: 1 },
                    max: Fraction { num: 2, denom: 1 },
                }),
            Err(NdArrayFormatError::MissingRateForChoice)
        );
        assert_eq!(
            without_rate
                .enum_format()
                .with_element_type_alternatives([ElementType::Unknown]),
            Err(NdArrayFormatError::UnsupportedElementTypeAlternative { index: 0 })
        );
        assert_eq!(
            without_rate
                .enum_format()
                .with_layout_alternatives([NdArrayLayout::Unknown]),
            Err(NdArrayFormatError::UnsupportedLayoutAlternative { index: 0 })
        );

        let with_rate = VectorFormat::new(
            ElementType::F32Le,
            8,
            Some(Fraction {
                num: 1000,
                denom: 1,
            }),
        )
        .unwrap();
        assert_eq!(
            with_rate
                .enum_format()
                .with_rate_choice(NdArrayRateChoice::Range {
                    min: Fraction {
                        num: 1500,
                        denom: 1,
                    },
                    max: Fraction {
                        num: 2000,
                        denom: 1,
                    },
                }),
            Err(NdArrayFormatError::InvalidRateChoice)
        );
    }
}
