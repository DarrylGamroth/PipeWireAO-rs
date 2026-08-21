// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Typed views of complete, native-endian ndarray buffers.

use std::{error, fmt, mem};

use spa::{
    buffer::ChunkFlags,
    param::format::{ElementType, NdArrayFormat, NdArrayLayout},
};

use super::Buffer;

/// A complete buffer does not match its negotiated ndarray format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The Rust scalar type differs from the negotiated element type.
    ElementType,
    /// Direct typed views require a native little-endian representation.
    Endianness,
    /// The first data plane is absent or not mapped.
    Data,
    /// The chunk is empty or corrupted.
    ChunkFlags,
    /// The chunk offset, size, or stride differs from the packed format.
    ChunkLayout,
    /// The mapped address does not satisfy the Rust scalar alignment.
    Alignment,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ElementType => "Rust scalar type does not match the ndarray element type",
            Self::Endianness => "ndarray element type is not native-endian on this target",
            Self::Data => "ndarray buffer has no mapped first data plane",
            Self::ChunkFlags => "ndarray chunk is empty or corrupted",
            Self::ChunkLayout => "ndarray chunk does not match its packed format",
            Self::Alignment => "ndarray data is not aligned for its Rust scalar type",
        })
    }
}

impl error::Error for Error {}

mod sealed {
    pub trait Sealed {}
}

/// Rust scalar with a direct native representation in an ndarray buffer.
///
/// This sealed trait excludes scalar formats without a matching Rust primitive
/// and formats, such as `Bool8`, for which not every bit pattern is valid.
pub trait Element: sealed::Sealed + Copy {
    /// Negotiated SPA element type corresponding to this Rust scalar.
    const ELEMENT_TYPE: ElementType;
}

macro_rules! elements {
    ($($rust:ty => $element:ident),+ $(,)?) => {
        $(
            impl sealed::Sealed for $rust {}
            impl Element for $rust {
                const ELEMENT_TYPE: ElementType = ElementType::$element;
            }
        )+
    };
}

elements! {
    i8 => I8,
    u8 => U8,
    i16 => I16Le,
    u16 => U16Le,
    i32 => I32Le,
    u32 => U32Le,
    i64 => I64Le,
    u64 => U64Le,
    i128 => I128Le,
    u128 => U128Le,
    f32 => F32Le,
    f64 => F64Le,
}

/// Borrows one packed ndarray input directly from its mapped data plane.
///
/// The returned slice remains valid only while `buffer` stays dequeued. The
/// element type, complete chunk extent, contiguous-axis stride, and alignment
/// are checked before the byte storage is reinterpreted.
pub fn input<'buffer, T, S>(
    buffer: &'buffer mut Buffer<'_>,
    format: &NdArrayFormat<S>,
) -> Result<&'buffer [T], Error>
where
    T: Element,
    S: AsRef<[u32]>,
{
    validate_type::<T, S>(format)?;
    let byte_count = format.byte_count();
    let stride = stride(format)?;
    let data = buffer.datas_mut().first_mut().ok_or(Error::Data)?;
    let (offset, size, chunk_stride) = {
        let chunk = data.chunk();
        if chunk
            .flags()
            .intersects(ChunkFlags::CORRUPTED | ChunkFlags::EMPTY)
        {
            return Err(Error::ChunkFlags);
        }
        (
            chunk.offset() as usize,
            chunk.size() as usize,
            chunk.stride(),
        )
    };
    if size != byte_count || (chunk_stride != 0 && chunk_stride != stride) {
        return Err(Error::ChunkLayout);
    }
    let bytes = data.data().ok_or(Error::Data)?;
    if bytes.is_empty() {
        return Err(Error::Data);
    }
    let offset = offset % bytes.len();
    let end = offset.checked_add(byte_count).ok_or(Error::ChunkLayout)?;
    let bytes = bytes.get(offset..end).ok_or(Error::ChunkLayout)?;
    cast(bytes)
}

/// Borrows packed ndarray output storage and publishes its complete chunk.
///
/// The returned slice covers exactly the negotiated ndarray extent. The caller
/// must initialize every element before queueing the buffer.
pub fn output<'buffer, T, S>(
    buffer: &'buffer mut Buffer<'_>,
    format: &NdArrayFormat<S>,
) -> Result<&'buffer mut [T], Error>
where
    T: Element,
    S: AsRef<[u32]>,
{
    validate_type::<T, S>(format)?;
    let byte_count = format.byte_count();
    let stride = stride(format)?;
    let data = buffer.datas_mut().first_mut().ok_or(Error::Data)?;
    {
        let bytes = data.data().ok_or(Error::Data)?;
        let bytes = bytes.get(..byte_count).ok_or(Error::ChunkLayout)?;
        validate_alignment::<T>(bytes)?;
    }
    {
        let chunk = data.chunk_mut();
        *chunk.offset_mut() = 0;
        *chunk.size_mut() = u32::try_from(byte_count).map_err(|_| Error::ChunkLayout)?;
        *chunk.stride_mut() = stride;
        chunk.set_flags(ChunkFlags::empty());
    }
    let bytes = data
        .data()
        .ok_or(Error::Data)?
        .get_mut(..byte_count)
        .ok_or(Error::ChunkLayout)?;
    cast_mut(bytes)
}

fn validate_type<T: Element, S: AsRef<[u32]>>(format: &NdArrayFormat<S>) -> Result<(), Error> {
    if format.element_type() != T::ELEMENT_TYPE {
        return Err(Error::ElementType);
    }
    if !cfg!(target_endian = "little") && mem::size_of::<T>() > 1 {
        return Err(Error::Endianness);
    }
    Ok(())
}

fn stride<S: AsRef<[u32]>>(format: &NdArrayFormat<S>) -> Result<i32, Error> {
    let dimension = match format.layout() {
        NdArrayLayout::RowMajor => format.shape().last(),
        NdArrayLayout::ColumnMajor => format.shape().first(),
        _ => None,
    }
    .ok_or(Error::ChunkLayout)?;
    let bytes = (*dimension as usize)
        .checked_mul(format.element_type().size().ok_or(Error::ElementType)?)
        .ok_or(Error::ChunkLayout)?;
    i32::try_from(bytes).map_err(|_| Error::ChunkLayout)
}

fn validate_alignment<T>(bytes: &[u8]) -> Result<(), Error> {
    if !(bytes.as_ptr() as usize).is_multiple_of(mem::align_of::<T>()) {
        return Err(Error::Alignment);
    }
    Ok(())
}

#[allow(unsafe_code)]
fn cast<T: Element>(bytes: &[u8]) -> Result<&[T], Error> {
    validate_alignment::<T>(bytes)?;
    // SAFETY: the element type, length, and alignment were checked. Every bit
    // pattern is valid for each sealed primitive implementation of `Element`.
    Ok(unsafe {
        std::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len() / mem::size_of::<T>())
    })
}

#[allow(unsafe_code)]
fn cast_mut<T: Element>(bytes: &mut [u8]) -> Result<&mut [T], Error> {
    validate_alignment::<T>(bytes)?;
    // SAFETY: the element type, length, and alignment were checked. The unique
    // byte borrow becomes one unique typed borrow of the same extent.
    Ok(unsafe {
        std::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast(), bytes.len() / mem::size_of::<T>())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(
        element_type: ElementType,
        shape: [u32; 2],
        layout: NdArrayLayout,
    ) -> NdArrayFormat<[u32; 2]> {
        NdArrayFormat::new(element_type, shape, layout, None).unwrap()
    }

    #[test]
    fn primitive_types_use_their_canonical_element_types() {
        assert_eq!(<u8 as Element>::ELEMENT_TYPE, ElementType::U8);
        assert_eq!(<u16 as Element>::ELEMENT_TYPE, ElementType::U16Le);
        assert_eq!(<f32 as Element>::ELEMENT_TYPE, ElementType::F32Le);
        assert_eq!(<f64 as Element>::ELEMENT_TYPE, ElementType::F64Le);
    }

    #[test]
    fn stride_tracks_the_contiguous_axis() {
        assert_eq!(
            stride(&format(ElementType::F32Le, [3, 5], NdArrayLayout::RowMajor)),
            Ok(20)
        );
        assert_eq!(
            stride(&format(
                ElementType::F32Le,
                [3, 5],
                NdArrayLayout::ColumnMajor
            )),
            Ok(12)
        );
    }

    #[test]
    fn direct_view_requires_the_matching_rust_type() {
        let format = format(ElementType::F32Le, [3, 5], NdArrayLayout::RowMajor);
        assert_eq!(validate_type::<f32, _>(&format), Ok(()));
        assert_eq!(validate_type::<u32, _>(&format), Err(Error::ElementType));
    }
}
