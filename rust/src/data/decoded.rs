/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Typed decoded data containers.

use crate::types::{DataType, LercError, Result};

/// Decoded pixel values in a concrete native LERC type.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedData {
    /// Signed 8-bit integer values.
    Char(Vec<i8>),
    /// Unsigned 8-bit integer values.
    UChar(Vec<u8>),
    /// Signed 16-bit integer values.
    Short(Vec<i16>),
    /// Unsigned 16-bit integer values.
    UShort(Vec<u16>),
    /// Signed 32-bit integer values.
    Int(Vec<i32>),
    /// Unsigned 32-bit integer values.
    UInt(Vec<u32>),
    /// 32-bit floating point values.
    Float(Vec<f32>),
    /// 64-bit floating point values.
    Double(Vec<f64>),
}

impl DecodedData {
    /// Returns the LERC scalar type held by this value.
    pub fn data_type(&self) -> DataType {
        match self {
            Self::Char(_) => DataType::Char,
            Self::UChar(_) => DataType::UChar,
            Self::Short(_) => DataType::Short,
            Self::UShort(_) => DataType::UShort,
            Self::Int(_) => DataType::Int,
            Self::UInt(_) => DataType::UInt,
            Self::Float(_) => DataType::Float,
            Self::Double(_) => DataType::Double,
        }
    }

    /// Returns the number of scalar values.
    pub fn len(&self) -> usize {
        match self {
            Self::Char(values) => values.len(),
            Self::UChar(values) => values.len(),
            Self::Short(values) => values.len(),
            Self::UShort(values) => values.len(),
            Self::Int(values) => values.len(),
            Self::UInt(values) => values.len(),
            Self::Float(values) => values.len(),
            Self::Double(values) => values.len(),
        }
    }

    /// Returns true when there are no scalar values.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the little-endian byte length of the scalar values.
    pub fn byte_len(&self) -> usize {
        self.len() * self.data_type().size_in_bytes()
    }

    /// Writes the scalar values to `output` as little-endian bytes.
    pub fn write_le_bytes(&self, output: &mut [u8]) -> Result<usize> {
        let byte_len = self.byte_len();
        if output.len() < byte_len {
            return Err(LercError::BufferTooSmall);
        }

        match self {
            Self::Char(values) => {
                for (dst, &value) in output.iter_mut().zip(values.iter()) {
                    *dst = value as u8;
                }
            }
            Self::UChar(values) => output[..byte_len].copy_from_slice(values),
            Self::Short(values) => write_native_values(output, values, i16::to_le_bytes),
            Self::UShort(values) => write_native_values(output, values, u16::to_le_bytes),
            Self::Int(values) => write_native_values(output, values, i32::to_le_bytes),
            Self::UInt(values) => write_native_values(output, values, u32::to_le_bytes),
            Self::Float(values) => write_native_values(output, values, f32::to_le_bytes),
            Self::Double(values) => write_native_values(output, values, f64::to_le_bytes),
        }

        Ok(byte_len)
    }

    /// Writes the scalar values converted to `data_type` as little-endian bytes.
    ///
    /// This mirrors the templated C++ decode behavior where callers choose the
    /// output scalar type independently from the blob's native scalar type.
    pub fn write_as_type_le_bytes(&self, data_type: DataType, output: &mut [u8]) -> Result<usize> {
        let byte_len = self.len() * data_type.size_in_bytes();
        if output.len() < byte_len {
            return Err(LercError::BufferTooSmall);
        }
        if data_type == self.data_type() {
            return self.write_le_bytes(output);
        }

        match data_type {
            DataType::Char => write_converted_values(output, self, |value| (value as i8) as u8),
            DataType::UChar => write_converted_values(output, self, |value| value as u8),
            DataType::Short => {
                write_converted_native_values(output, self, |value| (value as i16).to_le_bytes())
            }
            DataType::UShort => {
                write_converted_native_values(output, self, |value| (value as u16).to_le_bytes())
            }
            DataType::Int => {
                write_converted_native_values(output, self, |value| (value as i32).to_le_bytes())
            }
            DataType::UInt => {
                write_converted_native_values(output, self, |value| (value as u32).to_le_bytes())
            }
            DataType::Float => {
                write_converted_native_values(output, self, |value| (value as f32).to_le_bytes())
            }
            DataType::Double => {
                write_converted_native_values(output, self, |value| value.to_le_bytes())
            }
        }

        Ok(byte_len)
    }

    /// Writes the scalar values to `output` as 64-bit floating point values.
    pub fn write_f64_values(&self, output: &mut [f64]) -> Result<usize> {
        let len = self.len();
        if output.len() < len {
            return Err(LercError::BufferTooSmall);
        }

        match self {
            Self::Char(values) => write_as_f64(output, values, |value| value as f64),
            Self::UChar(values) => write_as_f64(output, values, |value| value as f64),
            Self::Short(values) => write_as_f64(output, values, |value| value as f64),
            Self::UShort(values) => write_as_f64(output, values, |value| value as f64),
            Self::Int(values) => write_as_f64(output, values, |value| value as f64),
            Self::UInt(values) => write_as_f64(output, values, |value| value as f64),
            Self::Float(values) => write_as_f64(output, values, |value| value as f64),
            Self::Double(values) => output[..len].copy_from_slice(values),
        }

        Ok(len)
    }
}

fn for_each_as_f64(values: &DecodedData, mut f: impl FnMut(f64)) {
    match values {
        DecodedData::Char(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::UChar(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::Short(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::UShort(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::Int(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::UInt(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::Float(values) => values.iter().for_each(|&value| f(value as f64)),
        DecodedData::Double(values) => values.iter().for_each(|&value| f(value)),
    }
}

fn write_converted_values(output: &mut [u8], values: &DecodedData, convert: fn(f64) -> u8) {
    let mut idx = 0usize;
    for_each_as_f64(values, |value| {
        output[idx] = convert(value);
        idx += 1;
    });
}

fn write_converted_native_values<const N: usize>(
    output: &mut [u8],
    values: &DecodedData,
    convert: fn(f64) -> [u8; N],
) {
    let mut idx = 0usize;
    for_each_as_f64(values, |value| {
        output[idx..idx + N].copy_from_slice(&convert(value));
        idx += N;
    });
}

fn write_native_values<T, const N: usize>(
    output: &mut [u8],
    values: &[T],
    to_le_bytes: fn(T) -> [u8; N],
) where
    T: Copy,
{
    for (chunk, &value) in output.chunks_exact_mut(N).zip(values.iter()) {
        chunk.copy_from_slice(&to_le_bytes(value));
    }
}

fn write_as_f64<T>(output: &mut [f64], values: &[T], convert: fn(T) -> f64)
where
    T: Copy,
{
    for (dst, &value) in output.iter_mut().zip(values.iter()) {
        *dst = convert(value);
    }
}

/// Converts little-endian bytes into typed decoded values.
pub fn decode_typed_values(data_type: DataType, bytes: &[u8]) -> Result<DecodedData> {
    let value_size = data_type.size_in_bytes();
    if bytes.len() % value_size != 0 {
        return Err(LercError::WrongParam(
            "decoded byte length is not a multiple of the data type size",
        ));
    }

    Ok(match data_type {
        DataType::Char => DecodedData::Char(bytes.iter().map(|&value| value as i8).collect()),
        DataType::UChar => DecodedData::UChar(bytes.to_vec()),
        DataType::Short => DecodedData::Short(
            bytes
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        ),
        DataType::UShort => DecodedData::UShort(
            bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        ),
        DataType::Int => DecodedData::Int(
            bytes
                .chunks_exact(4)
                .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        ),
        DataType::UInt => DecodedData::UInt(
            bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        ),
        DataType::Float => DecodedData::Float(
            bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        ),
        DataType::Double => DecodedData::Double(
            bytes
                .chunks_exact(8)
                .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        ),
    })
}

/// Converts little-endian typed LERC scalar bytes into 64-bit floating point values.
///
/// This is the safe Rust equivalent of the C++ `Lerc::ConvertToDouble` helper:
/// `bytes` contains contiguous scalar values of `data_type`, and converted
/// values are written into `output`. Like the C++ helper, this converts scalar
/// types smaller than `Double`; callers that already have double values can use
/// the decoded data directly.
pub fn convert_typed_bytes_to_f64(
    data_type: DataType,
    bytes: &[u8],
    output: &mut [f64],
) -> Result<usize> {
    if bytes.is_empty() || data_type == DataType::Double {
        return Err(LercError::WrongParam(
            "ConvertToDouble requires non-empty non-double input",
        ));
    }
    let decoded = decode_typed_values(data_type, bytes)?;
    decoded.write_f64_values(output)
}

#[cfg(test)]
mod tests {
    use super::{convert_typed_bytes_to_f64, decode_typed_values, DecodedData};
    use crate::DataType;

    #[test]
    fn decodes_all_lerc_data_types() {
        assert_eq!(
            decode_typed_values(DataType::Char, &[0xff, 0, 1]).unwrap(),
            DecodedData::Char(vec![-1, 0, 1])
        );
        assert_eq!(
            decode_typed_values(DataType::UChar, &[0, 255]).unwrap(),
            DecodedData::UChar(vec![0, 255])
        );

        let mut short_bytes = Vec::new();
        for value in [-2i16, 300] {
            short_bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_typed_values(DataType::Short, &short_bytes).unwrap(),
            DecodedData::Short(vec![-2, 300])
        );

        let mut ushort_bytes = Vec::new();
        for value in [2u16, 65_000] {
            ushort_bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_typed_values(DataType::UShort, &ushort_bytes).unwrap(),
            DecodedData::UShort(vec![2, 65_000])
        );

        let mut int_bytes = Vec::new();
        for value in [-1_000i32, 2_000] {
            int_bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_typed_values(DataType::Int, &int_bytes).unwrap(),
            DecodedData::Int(vec![-1_000, 2_000])
        );

        let mut uint_bytes = Vec::new();
        for value in [1u32, 4_000_000] {
            uint_bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_typed_values(DataType::UInt, &uint_bytes).unwrap(),
            DecodedData::UInt(vec![1, 4_000_000])
        );

        let mut float_bytes = Vec::new();
        for value in [-1.25f32, 2.5] {
            float_bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_typed_values(DataType::Float, &float_bytes).unwrap(),
            DecodedData::Float(vec![-1.25, 2.5])
        );

        let mut double_bytes = Vec::new();
        for value in [-1.25f64, 2.5] {
            double_bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_typed_values(DataType::Double, &double_bytes).unwrap(),
            DecodedData::Double(vec![-1.25, 2.5])
        );
    }

    #[test]
    fn rejects_misaligned_input() {
        assert!(decode_typed_values(DataType::Short, &[1]).is_err());
        assert!(decode_typed_values(DataType::Double, &[0; 7]).is_err());
    }

    #[test]
    fn writes_decoded_values_as_little_endian_bytes() {
        let data = DecodedData::Int(vec![-1, 2_000]);
        let mut output = [0u8; 8];
        assert_eq!(data.data_type(), DataType::Int);
        assert_eq!(data.byte_len(), 8);
        assert_eq!(data.write_le_bytes(&mut output).unwrap(), 8);
        assert_eq!(output, [0xff, 0xff, 0xff, 0xff, 0xd0, 0x07, 0x00, 0x00,]);

        let data = DecodedData::Float(vec![-1.25, 2.5]);
        let mut output = [0u8; 8];
        assert_eq!(data.write_le_bytes(&mut output).unwrap(), 8);
        assert_eq!(&output[..4], &(-1.25f32).to_le_bytes());
        assert_eq!(&output[4..], &2.5f32.to_le_bytes());
    }

    #[test]
    fn rejects_too_small_decoded_output_buffer() {
        let data = DecodedData::UShort(vec![1, 2]);
        let mut output = [0u8; 3];
        assert!(data.write_le_bytes(&mut output).is_err());
    }

    #[test]
    fn writes_decoded_values_as_requested_lerc_type() {
        let data = DecodedData::UChar(vec![1, 2, 255]);
        let mut output = [0u8; 6];
        let written = data
            .write_as_type_le_bytes(DataType::UShort, &mut output)
            .unwrap();
        assert_eq!(written, 6);
        assert_eq!(output, [1, 0, 2, 0, 255, 0]);

        let data = DecodedData::Float(vec![1.25, -2.5]);
        let mut output = [0u8; 16];
        data.write_as_type_le_bytes(DataType::Double, &mut output)
            .unwrap();
        assert_eq!(f64::from_le_bytes(output[..8].try_into().unwrap()), 1.25);
        assert_eq!(f64::from_le_bytes(output[8..].try_into().unwrap()), -2.5);

        let mut too_small = [0u8; 1];
        assert!(DecodedData::Int(vec![1])
            .write_as_type_le_bytes(DataType::Double, &mut too_small)
            .is_err());
    }

    #[test]
    fn writes_decoded_values_as_f64() {
        let mut output = [0.0; 3];
        let written = DecodedData::Short(vec![-2, 0, 300])
            .write_f64_values(&mut output)
            .unwrap();
        assert_eq!(written, 3);
        assert_eq!(output, [-2.0, 0.0, 300.0]);

        let mut output = [0.0; 2];
        DecodedData::Float(vec![1.25, -2.5])
            .write_f64_values(&mut output)
            .unwrap();
        assert_eq!(output, [1.25, -2.5]);

        assert!(DecodedData::Double(vec![1.0])
            .write_f64_values(&mut [])
            .is_err());
    }

    #[test]
    fn converts_typed_bytes_to_f64_like_cpp_convert_to_double() {
        let mut short_bytes = Vec::new();
        for value in [-2i16, 0, 300] {
            short_bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut output = [0.0; 3];
        let written =
            convert_typed_bytes_to_f64(DataType::Short, &short_bytes, &mut output).unwrap();
        assert_eq!(written, 3);
        assert_eq!(output, [-2.0, 0.0, 300.0]);

        let mut uint_bytes = Vec::new();
        for value in [0u32, u32::MAX] {
            uint_bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut output = [0.0; 2];
        convert_typed_bytes_to_f64(DataType::UInt, &uint_bytes, &mut output).unwrap();
        assert_eq!(output, [0.0, u32::MAX as f64]);

        assert!(convert_typed_bytes_to_f64(DataType::UChar, &[], &mut output).is_err());
        assert!(convert_typed_bytes_to_f64(DataType::Double, &[0; 8], &mut output).is_err());
        assert!(convert_typed_bytes_to_f64(DataType::Short, &[1], &mut output).is_err());
        assert!(convert_typed_bytes_to_f64(DataType::UShort, &[0, 1], &mut []).is_err());
    }
}
