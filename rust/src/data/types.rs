/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Common LERC types shared by the Rust port.

use core::fmt;

/// Status integer type used by the public LERC C API.
///
/// This mirrors the `typedef unsigned int lerc_status` declaration in
/// `Lerc_c_api.h`.
pub type LercStatus = u32;

/// Status codes matching the public LERC C API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrCode {
    /// Operation completed successfully.
    Ok = 0,
    /// Operation failed for a reason that does not map to a more specific code.
    Failed = 1,
    /// One or more caller parameters are invalid.
    WrongParam = 2,
    /// The caller-provided input or output buffer is too small.
    BufferTooSmall = 3,
    /// Input data contains NaN values where unsupported.
    NaN = 4,
    /// The blob uses no-data values that require the 4D decode API shape.
    HasNoData = 5,
}

/// Native LERC scalar data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DataType {
    /// Signed 8-bit integer.
    Char = 0,
    /// Unsigned 8-bit integer.
    UChar = 1,
    /// Signed 16-bit integer.
    Short = 2,
    /// Unsigned 16-bit integer.
    UShort = 3,
    /// Signed 32-bit integer.
    Int = 4,
    /// Unsigned 32-bit integer.
    UInt = 5,
    /// 32-bit floating point.
    Float = 6,
    /// 64-bit floating point.
    Double = 7,
}

/// Index order for the C API-style blob info array.
///
/// These values mirror `LercNS::InfoArrOrder` from `Lerc_types.h` and can be
/// used to index arrays filled by `lerc_getBlobInfo`-compatible helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum InfoArrOrder {
    /// Codec version.
    Version = 0,
    /// Scalar data type.
    DataType = 1,
    /// Legacy name for number of values per pixel; equal to [`Self::Depth`].
    Dim = 2,
    /// Number of columns.
    Cols = 3,
    /// Number of rows.
    Rows = 4,
    /// Number of bands.
    Bands = 5,
    /// Number of valid pixels in the first band.
    ValidPixels = 6,
    /// Total blob byte count.
    BlobSize = 7,
    /// Number of masks represented by the blob.
    Masks = 8,
    /// Number of values per pixel.
    Depth = 9,
    /// Number of per-band no-data values required by 4D decode APIs.
    UsesNoDataValue = 10,
    /// Sentinel equal to the current number of blob-info array entries.
    Last = 11,
}

impl InfoArrOrder {
    /// Returns this array-order value as a `usize` index.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Index order for the C API-style data range summary array.
///
/// These values mirror `LercNS::DataRangeArrOrder` from `Lerc_types.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DataRangeArrOrder {
    /// Minimum data value.
    Min = 0,
    /// Maximum data value.
    Max = 1,
    /// Maximum z error stored in the blob.
    MaxZErrorUsed = 2,
    /// Sentinel equal to the current number of data-range array entries.
    Last = 3,
}

impl DataRangeArrOrder {
    /// Returns this array-order value as a `usize` index.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Caller-provided shape for encode and compute-size operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeSpec {
    /// Scalar data type of the source values.
    pub data_type: DataType,
    /// Number of values per pixel.
    pub n_depth: usize,
    /// Number of columns.
    pub n_cols: usize,
    /// Number of rows.
    pub n_rows: usize,
    /// Number of bands.
    pub n_bands: usize,
    /// Number of byte masks supplied: 0, 1, or `n_bands`.
    pub n_masks: usize,
}

impl EncodeSpec {
    /// Validates dimensions and mask count for encode-style operations.
    pub fn validate(self) -> Result<()> {
        if self.n_depth == 0 || self.n_cols == 0 || self.n_rows == 0 || self.n_bands == 0 {
            return Err(LercError::WrongParam("encode dimensions must be positive"));
        }
        if !(self.n_masks == 0 || self.n_masks == 1 || self.n_masks == self.n_bands) {
            return Err(LercError::WrongParam(
                "encode mask count must be 0, 1, or n_bands",
            ));
        }
        Ok(())
    }

    /// Returns the number of source scalar values described by this shape.
    pub fn value_count(self) -> Result<usize> {
        self.n_bands
            .checked_mul(self.n_rows)
            .and_then(|count| count.checked_mul(self.n_cols))
            .and_then(|count| count.checked_mul(self.n_depth))
            .ok_or(LercError::WrongParam("encode value count overflow"))
    }

    /// Returns the byte count needed for source scalar values.
    pub fn data_byte_len(self) -> Result<usize> {
        self.value_count()?
            .checked_mul(self.data_type.size_in_bytes())
            .ok_or(LercError::WrongParam("encode data byte count overflow"))
    }

    /// Returns the byte count needed for source byte masks.
    pub fn mask_byte_len(self) -> Result<usize> {
        self.n_masks
            .checked_mul(self.n_rows)
            .and_then(|count| count.checked_mul(self.n_cols))
            .ok_or(LercError::WrongParam("encode mask byte count overflow"))
    }
}

/// Error type returned by safe Rust LERC APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LercError {
    /// A caller parameter is invalid.
    WrongParam(&'static str),
    /// A provided input or output buffer is too small.
    BufferTooSmall,
    /// Input data contains NaN values in a shape that cannot be encoded.
    NaN,
    /// No-data values are present and cannot be represented by the requested API.
    HasNoData,
    /// Encoded input is malformed or internally inconsistent.
    CorruptInput(&'static str),
    /// The requested safe helper does not support this blob or payload mode.
    Unsupported(&'static str),
}

impl LercError {
    /// Converts this Rust error to the closest LERC C API status code.
    pub fn err_code(&self) -> ErrCode {
        match self {
            Self::WrongParam(_) => ErrCode::WrongParam,
            Self::BufferTooSmall => ErrCode::BufferTooSmall,
            Self::NaN => ErrCode::NaN,
            Self::HasNoData => ErrCode::HasNoData,
            Self::CorruptInput(_) | Self::Unsupported(_) => ErrCode::Failed,
        }
    }
}

impl TryFrom<i32> for ErrCode {
    type Error = LercError;

    fn try_from(value: i32) -> Result<Self> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::Failed),
            2 => Ok(Self::WrongParam),
            3 => Ok(Self::BufferTooSmall),
            4 => Ok(Self::NaN),
            5 => Ok(Self::HasNoData),
            _ => Err(LercError::CorruptInput("invalid Lerc status code")),
        }
    }
}

impl TryFrom<LercStatus> for ErrCode {
    type Error = LercError;

    fn try_from(value: LercStatus) -> Result<Self> {
        i32::try_from(value)
            .map_err(|_| LercError::CorruptInput("invalid Lerc status code"))
            .and_then(Self::try_from)
    }
}

impl fmt::Display for LercError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongParam(msg) => write!(f, "wrong parameter: {msg}"),
            Self::BufferTooSmall => f.write_str("buffer too small"),
            Self::NaN => f.write_str("input contains unsupported NaN values"),
            Self::HasNoData => f.write_str("has no-data values"),
            Self::CorruptInput(msg) => write!(f, "corrupt input: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

impl std::error::Error for LercError {}

/// Result alias used by this crate.
pub type Result<T> = core::result::Result<T, LercError>;

impl TryFrom<i32> for DataType {
    type Error = LercError;

    fn try_from(value: i32) -> Result<Self> {
        match value {
            0 => Ok(Self::Char),
            1 => Ok(Self::UChar),
            2 => Ok(Self::Short),
            3 => Ok(Self::UShort),
            4 => Ok(Self::Int),
            5 => Ok(Self::UInt),
            6 => Ok(Self::Float),
            7 => Ok(Self::Double),
            _ => Err(LercError::CorruptInput("invalid Lerc data type")),
        }
    }
}

impl TryFrom<u32> for DataType {
    type Error = LercError;

    fn try_from(value: u32) -> Result<Self> {
        i32::try_from(value)
            .map_err(|_| LercError::CorruptInput("invalid Lerc data type"))
            .and_then(Self::try_from)
    }
}

impl DataType {
    /// Returns the scalar size in bytes for this LERC data type.
    pub fn size_in_bytes(self) -> usize {
        match self {
            Self::Char | Self::UChar => 1,
            Self::Short | Self::UShort => 2,
            Self::Int | Self::UInt | Self::Float => 4,
            Self::Double => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DataRangeArrOrder, DataType, EncodeSpec, ErrCode, InfoArrOrder, LercError, LercStatus,
    };

    #[test]
    fn encode_spec_validates_shape_and_reports_lengths() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 2,
            n_cols: 3,
            n_rows: 5,
            n_bands: 7,
            n_masks: 1,
        };

        spec.validate().unwrap();
        assert_eq!(spec.value_count().unwrap(), 210);
        assert_eq!(spec.data_byte_len().unwrap(), 840);
        assert_eq!(spec.mask_byte_len().unwrap(), 15);

        let bad_masks = EncodeSpec { n_masks: 2, ..spec };
        assert_eq!(
            bad_masks.validate().unwrap_err(),
            LercError::WrongParam("encode mask count must be 0, 1, or n_bands")
        );

        let zero_dim = EncodeSpec { n_cols: 0, ..spec };
        assert_eq!(
            zero_dim.validate().unwrap_err(),
            LercError::WrongParam("encode dimensions must be positive")
        );

        let overflow = EncodeSpec {
            data_type: DataType::Double,
            n_depth: usize::MAX,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        assert_eq!(
            overflow.value_count().unwrap_err(),
            LercError::WrongParam("encode value count overflow")
        );
        assert_eq!(
            overflow.data_byte_len().unwrap_err(),
            LercError::WrongParam("encode value count overflow")
        );

        let mask_overflow = EncodeSpec {
            n_masks: usize::MAX,
            ..spec
        };
        assert_eq!(
            mask_overflow.mask_byte_len().unwrap_err(),
            LercError::WrongParam("encode mask byte count overflow")
        );
    }

    #[test]
    fn c_api_array_order_enums_match_public_header() {
        assert_eq!(InfoArrOrder::Version.index(), 0);
        assert_eq!(InfoArrOrder::DataType.index(), 1);
        assert_eq!(InfoArrOrder::Dim.index(), 2);
        assert_eq!(InfoArrOrder::Cols.index(), 3);
        assert_eq!(InfoArrOrder::Rows.index(), 4);
        assert_eq!(InfoArrOrder::Bands.index(), 5);
        assert_eq!(InfoArrOrder::ValidPixels.index(), 6);
        assert_eq!(InfoArrOrder::BlobSize.index(), 7);
        assert_eq!(InfoArrOrder::Masks.index(), 8);
        assert_eq!(InfoArrOrder::Depth.index(), 9);
        assert_eq!(InfoArrOrder::UsesNoDataValue.index(), 10);
        assert_eq!(InfoArrOrder::Last.index(), 11);

        assert_eq!(DataRangeArrOrder::Min.index(), 0);
        assert_eq!(DataRangeArrOrder::Max.index(), 1);
        assert_eq!(DataRangeArrOrder::MaxZErrorUsed.index(), 2);
        assert_eq!(DataRangeArrOrder::Last.index(), 3);
    }

    #[test]
    fn public_status_and_data_type_values_match_public_header() {
        assert_eq!(ErrCode::Ok as i32, 0);
        assert_eq!(ErrCode::Failed as i32, 1);
        assert_eq!(ErrCode::WrongParam as i32, 2);
        assert_eq!(ErrCode::BufferTooSmall as i32, 3);
        assert_eq!(ErrCode::NaN as i32, 4);
        assert_eq!(ErrCode::HasNoData as i32, 5);
        assert_eq!(
            core::mem::size_of::<LercStatus>(),
            core::mem::size_of::<u32>()
        );

        assert_eq!(DataType::Char as i32, 0);
        assert_eq!(DataType::UChar as i32, 1);
        assert_eq!(DataType::Short as i32, 2);
        assert_eq!(DataType::UShort as i32, 3);
        assert_eq!(DataType::Int as i32, 4);
        assert_eq!(DataType::UInt as i32, 5);
        assert_eq!(DataType::Float as i32, 6);
        assert_eq!(DataType::Double as i32, 7);
    }

    #[test]
    fn status_code_converts_from_signed_and_unsigned_c_api_values() {
        for (value, status) in [
            (0u32, ErrCode::Ok),
            (1, ErrCode::Failed),
            (2, ErrCode::WrongParam),
            (3, ErrCode::BufferTooSmall),
            (4, ErrCode::NaN),
            (5, ErrCode::HasNoData),
        ] {
            assert_eq!(ErrCode::try_from(value).unwrap(), status);
            assert_eq!(ErrCode::try_from(value as i32).unwrap(), status);
        }

        assert_eq!(
            ErrCode::try_from(6u32).unwrap_err(),
            LercError::CorruptInput("invalid Lerc status code")
        );
        assert_eq!(
            ErrCode::try_from(u32::MAX).unwrap_err(),
            LercError::CorruptInput("invalid Lerc status code")
        );
        assert_eq!(
            ErrCode::try_from(-1i32).unwrap_err(),
            LercError::CorruptInput("invalid Lerc status code")
        );
    }

    #[test]
    fn data_type_converts_from_signed_and_unsigned_c_api_values() {
        for (value, data_type) in [
            (0u32, DataType::Char),
            (1, DataType::UChar),
            (2, DataType::Short),
            (3, DataType::UShort),
            (4, DataType::Int),
            (5, DataType::UInt),
            (6, DataType::Float),
            (7, DataType::Double),
        ] {
            assert_eq!(DataType::try_from(value).unwrap(), data_type);
            assert_eq!(DataType::try_from(value as i32).unwrap(), data_type);
        }

        assert_eq!(
            DataType::try_from(8u32).unwrap_err(),
            LercError::CorruptInput("invalid Lerc data type")
        );
        assert_eq!(
            DataType::try_from(u32::MAX).unwrap_err(),
            LercError::CorruptInput("invalid Lerc data type")
        );
        assert_eq!(
            DataType::try_from(-1i32).unwrap_err(),
            LercError::CorruptInput("invalid Lerc data type")
        );
    }
}
