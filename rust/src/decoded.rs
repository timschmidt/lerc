/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

use crate::types::{DataType, LercError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedData {
    Char(Vec<i8>),
    UChar(Vec<u8>),
    Short(Vec<i16>),
    UShort(Vec<u16>),
    Int(Vec<i32>),
    UInt(Vec<u32>),
    Float(Vec<f32>),
    Double(Vec<f64>),
}

impl DecodedData {
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

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn byte_len(&self) -> usize {
        self.len() * self.data_type().size_in_bytes()
    }

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

#[cfg(test)]
mod tests {
    use super::{decode_typed_values, DecodedData};
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
}
