/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Common LERC types shared by the Rust port.

use core::fmt;

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
    /// No-data values are present and cannot be represented by the requested API.
    HasNoData,
    /// Encoded input is malformed or internally inconsistent.
    CorruptInput(&'static str),
    /// The input requires a codec feature not ported yet.
    Unsupported(&'static str),
}

impl LercError {
    /// Converts this Rust error to the closest LERC C API status code.
    pub fn err_code(&self) -> ErrCode {
        match self {
            Self::WrongParam(_) => ErrCode::WrongParam,
            Self::BufferTooSmall => ErrCode::BufferTooSmall,
            Self::HasNoData => ErrCode::HasNoData,
            Self::CorruptInput(_) | Self::Unsupported(_) => ErrCode::Failed,
        }
    }
}

impl fmt::Display for LercError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongParam(msg) => write!(f, "wrong parameter: {msg}"),
            Self::BufferTooSmall => f.write_str("buffer too small"),
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
    use super::{DataType, EncodeSpec, LercError};

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
}
