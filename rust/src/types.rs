/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrCode {
    Ok = 0,
    Failed = 1,
    WrongParam = 2,
    BufferTooSmall = 3,
    NaN = 4,
    HasNoData = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DataType {
    Char = 0,
    UChar = 1,
    Short = 2,
    UShort = 3,
    Int = 4,
    UInt = 5,
    Float = 6,
    Double = 7,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LercError {
    WrongParam(&'static str),
    BufferTooSmall,
    CorruptInput(&'static str),
    Unsupported(&'static str),
}

impl LercError {
    pub fn err_code(&self) -> ErrCode {
        match self {
            Self::WrongParam(_) => ErrCode::WrongParam,
            Self::BufferTooSmall => ErrCode::BufferTooSmall,
            Self::CorruptInput(_) | Self::Unsupported(_) => ErrCode::Failed,
        }
    }
}

impl fmt::Display for LercError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongParam(msg) => write!(f, "wrong parameter: {msg}"),
            Self::BufferTooSmall => f.write_str("buffer too small"),
            Self::CorruptInput(msg) => write!(f, "corrupt input: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

impl std::error::Error for LercError {}

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
