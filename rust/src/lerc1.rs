/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Legacy Lerc1 metadata readers.

use crate::types::{DataType, LercError, Result};
use crate::{BitMask, Rle};

/// ASCII type string that starts legacy Lerc1 `CntZImage` blobs.
pub const CNT_Z_IMAGE_KEY: &[u8; 10] = b"CntZImage ";
const CNT_Z_IMAGE_VERSION: i32 = 11;
const CNT_Z_IMAGE_TYPE: i32 = 8;
const MAX_LERC1_DIMENSION: i32 = 20_000;

/// Header metadata for one legacy Lerc1 compressed part.
#[derive(Debug, Clone, PartialEq)]
pub struct Lerc1PartInfo {
    /// Number of vertical tiles in the part.
    pub num_tiles_vert: i32,
    /// Number of horizontal tiles in the part.
    pub num_tiles_hori: i32,
    /// Number of payload bytes following this part header.
    pub num_bytes: i32,
    /// Maximum value stored for the part.
    pub max_value: f32,
    /// Byte offset where this part payload starts.
    pub payload_offset: usize,
}

/// Parsed legacy Lerc1 `CntZImage` header metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Lerc1HeaderInfo {
    /// Lerc1 `CntZImage` format version.
    pub version: i32,
    /// Number of rows.
    pub n_rows: i32,
    /// Number of columns.
    pub n_cols: i32,
    /// Maximum permitted z error stored in the blob.
    pub max_z_error: f64,
    /// Scalar data type exposed by the public LERC API for Lerc1 blobs.
    pub data_type: DataType,
    /// Metadata for the count/mask part.
    pub count_part: Lerc1PartInfo,
    /// Metadata for the z-value part.
    pub z_part: Lerc1PartInfo,
    /// Number of bytes covered by the parsed Lerc1 blob.
    pub blob_size: usize,
}

/// Decoded legacy Lerc1 count mask metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lerc1MaskInfo {
    /// Decoded valid-pixel mask.
    pub mask: BitMask,
    /// Number of bytes consumed through the count/mask payload.
    pub bytes_consumed: usize,
    /// True when every represented pixel is valid.
    pub all_valid: bool,
}

/// Reads and validates the legacy Lerc1 `CntZImage` header and part headers.
///
/// This is a decode-free metadata reader. Full Lerc1 tile decoding is still
/// required before the public C metadata APIs can report exact valid-pixel
/// counts and min/max ranges for arbitrary Lerc1 blobs.
pub fn get_lerc1_header_info(blob: &[u8]) -> Result<Lerc1HeaderInfo> {
    let mut reader = Reader::new(blob);
    if reader.read_bytes(CNT_Z_IMAGE_KEY.len())? != CNT_Z_IMAGE_KEY {
        return Err(LercError::CorruptInput("missing Lerc1 CntZImage key"));
    }

    let version = reader.read_i32_le()?;
    let image_type = reader.read_i32_le()?;
    let n_rows = reader.read_i32_le()?;
    let n_cols = reader.read_i32_le()?;
    let max_z_error = reader.read_f64_le()?;

    if version != CNT_Z_IMAGE_VERSION || image_type != CNT_Z_IMAGE_TYPE {
        return Err(LercError::CorruptInput("invalid Lerc1 CntZImage header"));
    }
    if n_rows <= 0 || n_cols <= 0 || n_rows > MAX_LERC1_DIMENSION || n_cols > MAX_LERC1_DIMENSION {
        return Err(LercError::CorruptInput("invalid Lerc1 dimensions"));
    }

    let count_part = read_part_info(&mut reader)?;
    reader.skip_payload(count_part.num_bytes as usize)?;
    let z_part = read_part_info(&mut reader)?;
    reader.skip_payload(z_part.num_bytes as usize)?;

    Ok(Lerc1HeaderInfo {
        version,
        n_rows,
        n_cols,
        max_z_error,
        data_type: DataType::Float,
        count_part,
        z_part,
        blob_size: reader.pos,
    })
}

/// Reads the legacy Lerc1 count part as a valid-pixel mask.
///
/// This covers the non-tiled count-part form used by the checked-in Lerc1
/// fixture: either a constant count value or an RLE-compressed packed bit mask.
/// Tiled Lerc1 count payloads are left for the full legacy tile decoder.
pub fn read_lerc1_count_mask(blob: &[u8]) -> Result<(Lerc1HeaderInfo, Lerc1MaskInfo)> {
    let info = get_lerc1_header_info(blob)?;
    if info.count_part.num_tiles_vert != 0 || info.count_part.num_tiles_hori != 0 {
        return Err(LercError::Unsupported("tiled Lerc1 count parts"));
    }

    let mut mask = BitMask::new(info.n_cols as usize, info.n_rows as usize)?;
    if info.count_part.num_bytes == 0 {
        if info.count_part.max_value > 0.0 {
            mask.set_all_valid();
        }
    } else {
        let payload_start = info.count_part.payload_offset;
        let payload_end = payload_start
            .checked_add(info.count_part.num_bytes as usize)
            .ok_or(LercError::CorruptInput("Lerc1 count payload overflow"))?;
        let payload = blob
            .get(payload_start..payload_end)
            .ok_or(LercError::BufferTooSmall)?;
        Rle::decompress_into(payload, mask.bits_mut())?;
    }

    let all_valid = mask.count_valid_bits() == mask.pixel_count();
    let bytes_consumed = info.count_part.payload_offset + info.count_part.num_bytes as usize;
    Ok((
        info,
        Lerc1MaskInfo {
            mask,
            bytes_consumed,
            all_valid,
        },
    ))
}

fn read_part_info(reader: &mut Reader<'_>) -> Result<Lerc1PartInfo> {
    let num_tiles_vert = reader.read_i32_le()?;
    let num_tiles_hori = reader.read_i32_le()?;
    let num_bytes = reader.read_i32_le()?;
    let max_value = reader.read_f32_le()?;
    if num_bytes < 0 {
        return Err(LercError::CorruptInput("negative Lerc1 part byte count"));
    }

    Ok(Lerc1PartInfo {
        num_tiles_vert,
        num_tiles_hori,
        num_bytes,
        max_value,
        payload_offset: reader.pos,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.bytes.len().saturating_sub(self.pos) < len {
            return Err(LercError::BufferTooSmall);
        }
        let out = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok(out)
    }

    fn read_i32_le(&mut self) -> Result<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_f32_le(&mut self) -> Result<f32> {
        let bytes = self.read_bytes(4)?;
        Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_f64_le(&mut self) -> Result<f64> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn skip_payload(&mut self, len: usize) -> Result<()> {
        self.read_bytes(len)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{get_lerc1_header_info, read_lerc1_count_mask};
    use crate::DataType;
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("testData");
        path.push(name);
        fs::read(path).unwrap()
    }

    #[test]
    fn reads_world_lerc1_header() {
        let blob = fixture("world.lerc1");
        let info = get_lerc1_header_info(&blob).unwrap();

        assert_eq!(info.version, 11);
        assert_eq!(info.n_rows, 257);
        assert_eq!(info.n_cols, 257);
        assert_eq!(info.max_z_error, 0.1);
        assert_eq!(info.data_type, DataType::Float);
        assert_eq!(info.blob_size, blob.len());
        assert_eq!(info.count_part.num_tiles_vert, 0);
        assert_eq!(info.count_part.num_tiles_hori, 0);
        assert_eq!(info.count_part.num_bytes, 1572);
        assert_eq!(info.count_part.max_value, 1.0);
        assert_eq!(info.count_part.payload_offset, 50);
        assert_eq!(info.z_part.num_tiles_vert, 32);
        assert_eq!(info.z_part.num_tiles_hori, 32);
        assert_eq!(info.z_part.num_bytes, 61_880);
        assert_eq!(info.z_part.max_value, 5474.173);
        assert_eq!(info.z_part.payload_offset, 1638);
    }

    #[test]
    fn reads_world_lerc1_count_mask() {
        let blob = fixture("world.lerc1");
        let (header, mask_info) = read_lerc1_count_mask(&blob).unwrap();

        assert_eq!(header.n_cols, 257);
        assert_eq!(header.n_rows, 257);
        assert_eq!(mask_info.bytes_consumed, 1622);
        assert_eq!(mask_info.mask.byte_len(), 8257);
        assert_eq!(mask_info.mask.count_valid_bits(), 65_025);
        assert!(!mask_info.all_valid);
    }

    #[test]
    fn rejects_non_lerc1_and_truncated_headers() {
        let blob = fixture("california_400_400_1_float.lerc2");
        assert!(get_lerc1_header_info(&blob).is_err());

        let lerc1 = fixture("world.lerc1");
        assert!(get_lerc1_header_info(&lerc1[..20]).is_err());
        assert!(get_lerc1_header_info(&lerc1[..lerc1.len() - 1]).is_err());
    }
}
