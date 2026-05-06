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
