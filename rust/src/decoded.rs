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
}
