/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Lerc2 metadata readers and supported-subset decoders.

use crate::types::{DataType, EncodeSpec, LercError, Result};
use crate::{decode_lerc1_bands, decode_typed_values, DecodedData, CNT_Z_IMAGE_KEY};
use crate::{BitMask, BitStuffer2, Rle};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::HashMap;

/// Highest Lerc2 codec version recognized by this crate.
pub const CURRENT_VERSION: i32 = 6;
/// ASCII file key that starts every Lerc2 blob.
pub const FILE_KEY: &[u8; 6] = b"Lerc2 ";
/// Number of integers currently produced by the C API blob-info array.
pub const BLOB_INFO_ARRAY_LEN: usize = 11;
/// Number of doubles currently produced by the C API data-range summary array.
pub const BLOB_DATA_RANGE_ARRAY_LEN: usize = 3;
const CHECKSUM_START_OFFSET: usize = FILE_KEY.len() + 4 + 4;
#[allow(dead_code)]
const FP_MAX_DELTA: u8 = 5;
#[allow(dead_code)]
const FPL_HUFFMAN_NORMAL: u8 = 0;
#[allow(dead_code)]
const FPL_HUFFMAN_RLE: u8 = 1;
#[allow(dead_code)]
const FPL_HUFFMAN_NO_ENCODING: u8 = 2;
#[allow(dead_code)]
const FPL_HUFFMAN_PACKBITS: u8 = 3;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FpPredictor {
    None,
    Delta1,
    RowsCols,
}

#[allow(dead_code)]
impl FpPredictor {
    fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::None),
            1 => Some(Self::Delta1),
            2 => Some(Self::RowsCols),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Delta1 => 1,
            Self::RowsCols => 2,
        }
    }

    fn int_delta(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Delta1 => 1,
            Self::RowsCols => 2,
        }
    }

    fn max_byte_delta(self) -> u8 {
        FP_MAX_DELTA - self.int_delta()
    }

    fn from_delta_and_cross(delta: i32, cross: bool) -> Option<Self> {
        match (delta, cross) {
            (0, _) => Some(Self::None),
            (1, false) => Some(Self::Delta1),
            (2, true) => Some(Self::RowsCols),
            _ => None,
        }
    }
}

/// Parsed Lerc2 header fields.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderInfo {
    /// Lerc2 format version.
    pub version: i32,
    /// Stored Fletcher32 checksum for version 3 and newer blobs.
    pub checksum: u32,
    /// Number of rows.
    pub n_rows: i32,
    /// Number of columns.
    pub n_cols: i32,
    /// Number of values per pixel.
    pub n_depth: i32,
    /// Number of valid pixels in the 2D mask.
    pub num_valid_pixel: i32,
    /// Micro block size used for tiled payloads.
    pub micro_block_size: i32,
    /// Size in bytes of this Lerc2 blob.
    pub blob_size: i32,
    /// Number of following blobs for version 6+ concatenated streams.
    pub n_blobs_more: i32,
    /// Nonzero when version 6+ no-data sentinels are carried.
    pub b_pass_no_data_values: u8,
    /// Version 6+ flag indicating all input values were integer-valued.
    pub b_is_int: u8,
    /// Reserved version 6+ header byte.
    pub b_reserved_3: u8,
    /// Reserved version 6+ header byte.
    pub b_reserved_4: u8,
    /// Scalar data type of encoded values.
    pub data_type: DataType,
    /// Maximum permitted z error stored in the blob.
    pub max_z_error: f64,
    /// Global minimum encoded value.
    pub z_min: f64,
    /// Global maximum encoded value.
    pub z_max: f64,
    /// Temporary no-data value used inside version 6+ blobs.
    pub no_data_val: f64,
    /// Original no-data value restored on decode for version 6+ blobs.
    pub no_data_val_orig: f64,
    /// Header size in bytes.
    pub header_size: usize,
}

impl HeaderInfo {
    /// Returns true if this header carries version 6+ no-data values.
    pub fn has_no_data_values(&self) -> bool {
        self.b_pass_no_data_values != 0
    }
}

/// Header probe result with mask-presence information.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderProbe {
    /// Parsed header.
    pub header: HeaderInfo,
    /// True when the blob has an explicit mask section.
    pub has_mask: bool,
}

/// Decoded mask section and read position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskInfo {
    /// Decoded valid-pixel mask.
    pub mask: BitMask,
    /// Encoded mask byte count from the blob.
    pub num_bytes_mask: i32,
    /// Number of bytes consumed through the mask section.
    pub bytes_consumed: usize,
}

/// Per-depth min/max ranges for version 4+ Lerc2 blobs.
#[derive(Debug, Clone, PartialEq)]
pub struct MinMaxRanges {
    /// Minimum value per depth slice.
    pub mins: Vec<f64>,
    /// Maximum value per depth slice.
    pub maxs: Vec<f64>,
    /// Number of bytes consumed through the range section.
    pub bytes_consumed: usize,
    /// True when every min value equals its corresponding max value.
    pub min_max_equal: bool,
}

/// Raw bytes decoded from a one-sweep Lerc2 payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataOneSweep {
    /// Full image data as little-endian bytes, including zeroed invalid pixels.
    pub data: Vec<u8>,
    /// Number of bytes consumed through the payload.
    pub bytes_consumed: usize,
}

impl DataOneSweep {
    /// Converts the raw bytes to typed decoded values.
    pub fn decode_typed(&self, data_type: DataType) -> Result<DecodedData> {
        decode_typed_values(data_type, &self.data)
    }
}

/// Raw bytes decoded from a tiled Lerc2 payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiledData {
    /// Full image data as little-endian bytes, including zeroed invalid pixels.
    pub data: Vec<u8>,
    /// Number of bytes consumed through the payload.
    pub bytes_consumed: usize,
}

impl TiledData {
    /// Converts the raw bytes to typed decoded values.
    pub fn decode_typed(&self, data_type: DataType) -> Result<DecodedData> {
        decode_typed_values(data_type, &self.data)
    }
}

/// Aggregated blob information matching the public C API shape.
#[derive(Debug, Clone, PartialEq)]
pub struct LercInfo {
    /// LERC format version of the first blob.
    ///
    /// Legacy Lerc1 blobs report version `0`, matching the C++ public API.
    pub version: i32,
    /// Number of values per pixel.
    pub n_depth: i32,
    /// Number of columns.
    pub n_cols: i32,
    /// Number of rows.
    pub n_rows: i32,
    /// Number of valid pixels in the first band.
    pub num_valid_pixel: i32,
    /// Number of concatenated bands.
    pub n_bands: i32,
    /// Combined blob size in bytes.
    pub blob_size: i32,
    /// Number of masks required by the public C API.
    pub n_masks: i32,
    /// Number of bands reported as using no-data values.
    pub n_uses_no_data_value: i32,
    /// Scalar data type.
    pub data_type: DataType,
    /// Global minimum across bands.
    pub z_min: f64,
    /// Global maximum across bands.
    pub z_max: f64,
    /// Maximum z error across bands.
    pub max_z_error: f64,
}

/// Supported-subset single-band Lerc2 decode result.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedLerc2 {
    /// Parsed header for the decoded blob.
    pub header: HeaderInfo,
    /// Decoded valid-pixel mask.
    pub mask: BitMask,
    /// Per-depth ranges when present in the blob.
    pub ranges: Option<MinMaxRanges>,
    /// Typed decoded values.
    pub data: DecodedData,
    /// Number of bytes consumed by this blob.
    pub bytes_consumed: usize,
}

impl DecodedLerc2 {
    /// Returns the number of bytes needed to write decoded data values.
    pub fn data_byte_len(&self) -> usize {
        self.data.byte_len()
    }

    /// Writes decoded data values as little-endian bytes into `output`.
    pub fn write_data_le_bytes(&self, output: &mut [u8]) -> Result<usize> {
        self.data.write_le_bytes(output)
    }

    /// Returns the number of bytes needed to write the byte mask.
    pub fn mask_byte_len(&self) -> usize {
        (self.header.n_cols as usize) * (self.header.n_rows as usize)
    }

    /// Writes the decoded mask as one byte per pixel.
    pub fn write_mask_bytes(&self, output: &mut [u8]) -> Result<usize> {
        let byte_len = self.mask_byte_len();
        if output.len() < byte_len {
            return Err(LercError::BufferTooSmall);
        }
        output[..byte_len].copy_from_slice(&self.mask.to_byte_mask());
        Ok(byte_len)
    }
}

/// Supported-subset multi-band Lerc2 decode result.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedLerc2Bands {
    /// Decoded bands in blob order.
    pub bands: Vec<DecodedLerc2>,
    /// Total number of bytes consumed by all decoded bands.
    pub bytes_consumed: usize,
}

impl DecodedLerc2Bands {
    /// Returns the total number of bytes needed to write all decoded bands.
    pub fn data_byte_len(&self) -> usize {
        self.bands.iter().map(DecodedLerc2::data_byte_len).sum()
    }

    /// Writes all decoded bands as band-major little-endian bytes.
    pub fn write_data_le_bytes(&self, output: &mut [u8]) -> Result<usize> {
        let byte_len = self.data_byte_len();
        if output.len() < byte_len {
            return Err(LercError::BufferTooSmall);
        }

        let mut offset = 0usize;
        for band in &self.bands {
            offset += band.write_data_le_bytes(&mut output[offset..])?;
        }
        Ok(offset)
    }

    /// Returns the total number of bytes needed to write all byte masks.
    pub fn mask_byte_len(&self) -> usize {
        self.bands.iter().map(DecodedLerc2::mask_byte_len).sum()
    }

    /// Writes all decoded masks as band-major byte masks.
    pub fn write_mask_bytes(&self, output: &mut [u8]) -> Result<usize> {
        let byte_len = self.mask_byte_len();
        if output.len() < byte_len {
            return Err(LercError::BufferTooSmall);
        }

        let mut offset = 0usize;
        for band in &self.bands {
            offset += band.write_mask_bytes(&mut output[offset..])?;
        }
        Ok(offset)
    }
}

/// Aggregated min/max ranges for Lerc2 bands.
#[derive(Debug, Clone, PartialEq)]
pub struct DataRanges {
    /// Minimum values in band-major, depth-minor order.
    pub mins: Vec<f64>,
    /// Maximum values in band-major, depth-minor order.
    pub maxs: Vec<f64>,
    /// Number of bands represented.
    pub n_bands: usize,
    /// Number of values per pixel.
    pub n_depth: usize,
    /// Total number of bytes consumed while reading ranges.
    pub bytes_consumed: usize,
}

/// Per-band Lerc2 no-data metadata for 4D decode wrappers.
#[derive(Debug, Clone, PartialEq)]
pub struct NoDataInfo {
    /// Per-band flags: `1` when the band carries no-data metadata, otherwise `0`.
    pub uses_no_data: Vec<u8>,
    /// Per-band original no-data sentinel values.
    pub no_data_values: Vec<f64>,
    /// Total number of bytes consumed while reading band headers.
    pub bytes_consumed: usize,
}

/// Caller-provided output shape for supported decode-into operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeIntoSpec {
    /// Expected decoded scalar data type.
    pub data_type: DataType,
    /// Expected values per pixel.
    pub n_depth: usize,
    /// Expected column count.
    pub n_cols: usize,
    /// Expected row count.
    pub n_rows: usize,
    /// Number of bands to decode.
    pub n_bands: usize,
    /// Number of byte masks requested: 0, 1, or `n_bands`.
    pub n_masks: usize,
}

impl DecodeIntoSpec {
    /// Returns the byte count needed for decoded scalar output.
    pub fn data_byte_len(self) -> Result<usize> {
        self.n_bands
            .checked_mul(self.n_rows)
            .and_then(|count| count.checked_mul(self.n_cols))
            .and_then(|count| count.checked_mul(self.n_depth))
            .and_then(|count| count.checked_mul(self.data_type.size_in_bytes()))
            .ok_or(LercError::WrongParam("decode output byte count overflow"))
    }

    /// Returns the scalar value count represented by the decoded output.
    pub fn value_count(self) -> Result<usize> {
        self.n_bands
            .checked_mul(self.n_rows)
            .and_then(|count| count.checked_mul(self.n_cols))
            .and_then(|count| count.checked_mul(self.n_depth))
            .ok_or(LercError::WrongParam("decode output value count overflow"))
    }

    /// Returns the byte count needed for decoded byte-mask output.
    pub fn mask_byte_len(self) -> Result<usize> {
        self.n_masks
            .checked_mul(self.n_rows)
            .and_then(|count| count.checked_mul(self.n_cols))
            .ok_or(LercError::WrongParam("decode mask byte count overflow"))
    }
}

/// Byte counts produced by a supported decode-into operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeIntoResult {
    /// Total bytes consumed from the input blob.
    pub bytes_consumed: usize,
    /// Bytes written to the decoded data output buffer.
    pub data_bytes_written: usize,
    /// Bytes written to the decoded mask output buffer.
    pub mask_bytes_written: usize,
}

/// Byte counts produced by a supported decode-to-`f64` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeToF64Result {
    /// Total bytes consumed from the input blob.
    pub bytes_consumed: usize,
    /// Number of `f64` values written to the decoded output buffer.
    pub values_written: usize,
    /// Bytes written to the decoded mask output buffer.
    pub mask_bytes_written: usize,
}

/// Reads the Lerc2 header and reports whether an explicit mask follows it.
pub fn get_lerc2_header_info(blob: &[u8]) -> Result<HeaderProbe> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask_bytes = reader.peek_i32_le()?;
    Ok(HeaderProbe {
        header,
        has_mask: mask_bytes > 0,
    })
}

/// Computes the serialized byte count for a Lerc2 header version.
pub fn compute_lerc2_header_byte_len(version: i32) -> Result<usize> {
    if !(0..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam("unsupported Lerc2 header version"));
    }

    Ok(FILE_KEY.len()
        + 4
        + if version >= 3 { 4 } else { 0 }
        + if version >= 4 { 7 * 4 } else { 6 * 4 }
        + if version >= 6 { 4 + 4 } else { 0 }
        + if version >= 6 { 5 * 8 } else { 3 * 8 })
}

/// Writes a Lerc2 header in little-endian byte order.
///
/// The checksum field is written from `header.checksum`. Encoders that need a
/// zero placeholder should set that field to zero before calling this helper.
pub fn write_lerc2_header(header: &HeaderInfo, output: &mut [u8]) -> Result<usize> {
    if header.version < 4 && header.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 headers can only store depth 1",
        ));
    }

    validate_header_dims(
        header.n_rows,
        header.n_cols,
        header.n_depth,
        header.num_valid_pixel,
        header.micro_block_size,
        header.blob_size,
    )
    .map_err(|_| LercError::WrongParam("invalid Lerc2 header dimensions"))?;

    let header_len = compute_lerc2_header_byte_len(header.version)?;
    if output.len() < header_len {
        return Err(LercError::BufferTooSmall);
    }

    let mut writer = Writer::new(output);
    writer.write_bytes(FILE_KEY)?;
    writer.write_i32_le(header.version)?;
    if header.version >= 3 {
        writer.write_u32_le(header.checksum)?;
    }
    writer.write_i32_le(header.n_rows)?;
    writer.write_i32_le(header.n_cols)?;
    if header.version >= 4 {
        writer.write_i32_le(header.n_depth)?;
    }
    writer.write_i32_le(header.num_valid_pixel)?;
    writer.write_i32_le(header.micro_block_size)?;
    writer.write_i32_le(header.blob_size)?;
    writer.write_i32_le(header.data_type as i32)?;
    if header.version >= 6 {
        writer.write_i32_le(header.n_blobs_more)?;
        writer.write_bytes(&[
            header.b_pass_no_data_values,
            header.b_is_int,
            header.b_reserved_3,
            header.b_reserved_4,
        ])?;
    }
    writer.write_f64_le(header.max_z_error)?;
    writer.write_f64_le(header.z_min)?;
    writer.write_f64_le(header.z_max)?;
    if header.version >= 6 {
        writer.write_f64_le(header.no_data_val)?;
        writer.write_f64_le(header.no_data_val_orig)?;
    }

    Ok(writer.pos)
}

/// Computes the serialized byte count for a Lerc2 mask section.
///
/// The returned size always includes the 4-byte encoded-mask length field. When
/// the image is partially valid and `encode_mask` is true, `mask` must contain
/// the packed valid-pixel mask to be RLE-compressed.
pub fn compute_lerc2_mask_byte_len(
    header: &HeaderInfo,
    mask: Option<&BitMask>,
    encode_mask: bool,
) -> Result<usize> {
    let need_mask = validate_lerc2_mask_for_write(header, mask, encode_mask)?;
    let encoded_len = if need_mask && encode_mask {
        Rle::compress(mask.expect("validated mask").bits())?.len()
    } else {
        0
    };
    4usize
        .checked_add(encoded_len)
        .ok_or(LercError::WrongParam("Lerc2 mask byte count overflow"))
}

/// Writes a Lerc2 mask section in little-endian byte order.
///
/// The section is a 4-byte encoded-mask length followed by RLE-compressed
/// packed mask bytes when a partial mask is present and `encode_mask` is true.
/// Passing `encode_mask = false` writes a zero-length section, matching the
/// previous-mask reuse layout used by concatenated Lerc2 bands.
pub fn write_lerc2_mask(
    header: &HeaderInfo,
    mask: Option<&BitMask>,
    encode_mask: bool,
    output: &mut [u8],
) -> Result<usize> {
    let need_mask = validate_lerc2_mask_for_write(header, mask, encode_mask)?;
    let encoded_mask = if need_mask && encode_mask {
        Rle::compress(mask.expect("validated mask").bits())?
    } else {
        Vec::new()
    };

    let mask_len = compute_lerc2_mask_byte_len(header, mask, encode_mask)?;
    if output.len() < mask_len {
        return Err(LercError::BufferTooSmall);
    }

    let mut writer = Writer::new(output);
    writer.write_i32_le(encoded_mask.len() as i32)?;
    writer.write_bytes(&encoded_mask)?;
    Ok(writer.pos)
}

/// Computes the serialized byte count for a Lerc2 v4+ min/max range section.
pub fn compute_lerc2_min_max_ranges_byte_len(header: &HeaderInfo) -> Result<usize> {
    if header.version < 4 {
        return Err(LercError::WrongParam(
            "Lerc2 min/max ranges require version 4 or newer",
        ));
    }
    if header.n_depth <= 0 {
        return Err(LercError::WrongParam("Lerc2 depth must be positive"));
    }

    (header.n_depth as usize)
        .checked_mul(header.data_type.size_in_bytes())
        .and_then(|len| len.checked_mul(2))
        .ok_or(LercError::WrongParam(
            "Lerc2 min/max range byte count overflow",
        ))
}

/// Writes a Lerc2 v4+ min/max range section in little-endian byte order.
///
/// Values are written as all per-depth minimums followed by all per-depth
/// maximums, cast to the scalar type declared by `header`.
pub fn write_lerc2_min_max_ranges(
    header: &HeaderInfo,
    ranges: &MinMaxRanges,
    output: &mut [u8],
) -> Result<usize> {
    validate_lerc2_min_max_ranges_for_write(header, ranges)?;
    let byte_len = compute_lerc2_min_max_ranges_byte_len(header)?;
    if output.len() < byte_len {
        return Err(LercError::BufferTooSmall);
    }

    let mut writer = Writer::new(output);
    for &value in &ranges.mins {
        writer.write_bytes(&encode_value_as_bytes(header.data_type, value))?;
    }
    for &value in &ranges.maxs {
        writer.write_bytes(&encode_value_as_bytes(header.data_type, value))?;
    }
    Ok(writer.pos)
}

/// Computes the serialized byte count for a one-sweep payload section.
///
/// The returned size includes the one-byte one-sweep flag followed by one
/// contiguous depth tuple for each valid pixel.
pub fn compute_lerc2_one_sweep_byte_len(header: &HeaderInfo) -> Result<usize> {
    let pixel_byte_width = (header.n_depth as usize)
        .checked_mul(header.data_type.size_in_bytes())
        .ok_or(LercError::WrongParam(
            "Lerc2 one-sweep pixel byte width overflow",
        ))?;
    (header.num_valid_pixel as usize)
        .checked_mul(pixel_byte_width)
        .and_then(|len| len.checked_add(1))
        .ok_or(LercError::WrongParam("Lerc2 one-sweep byte count overflow"))
}

/// Writes a Lerc2 one-sweep payload section in row-major order.
///
/// `data` must contain the full image in little-endian scalar bytes, including
/// values for invalid pixels. Only valid pixels are copied into the payload.
pub fn write_lerc2_one_sweep(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    output: &mut [u8],
) -> Result<usize> {
    validate_lerc2_one_sweep_for_write(header, mask, data)?;
    let byte_len = compute_lerc2_one_sweep_byte_len(header)?;
    if output.len() < byte_len {
        return Err(LercError::BufferTooSmall);
    }

    let n_cols = header.n_cols as usize;
    let n_rows = header.n_rows as usize;
    let n_depth = header.n_depth as usize;
    let value_size = header.data_type.size_in_bytes();
    let pixel_byte_width = n_depth * value_size;
    let mut writer = Writer::new(output);
    writer.write_bytes(&[1])?;
    for row in 0..n_rows {
        for col in 0..n_cols {
            let pixel_idx = row * n_cols + col;
            if mask.is_valid(pixel_idx)? {
                let offset = pixel_idx * pixel_byte_width;
                writer.write_bytes(&data[offset..offset + pixel_byte_width])?;
            }
        }
    }

    Ok(writer.pos)
}

/// Computes the serialized byte count for a raw tiled payload section.
///
/// The returned size includes the one-byte tiled-payload flag, one raw-tile
/// flag per tile and depth, and one scalar value for each valid pixel in each
/// depth plane.
pub fn compute_lerc2_tiled_raw_byte_len(header: &HeaderInfo, mask: &BitMask) -> Result<usize> {
    compute_lerc2_tiled_raw_byte_len_with_mode_prefix(header, mask, false)
}

fn compute_lerc2_tiled_raw_byte_len_with_mode_prefix(
    header: &HeaderInfo,
    mask: &BitMask,
    include_image_mode: bool,
) -> Result<usize> {
    validate_lerc2_tiled_raw_for_write(header, mask, None)?;

    let n_depth = header.n_depth as usize;
    let value_size = header.data_type.size_in_bytes();
    let mb_size = header.micro_block_size as usize;
    let tiles_vert = (header.n_rows as usize).div_ceil(mb_size);
    let tiles_hori = (header.n_cols as usize).div_ceil(mb_size);
    let tile_flag_count = tiles_vert
        .checked_mul(tiles_hori)
        .and_then(|count| count.checked_mul(n_depth))
        .ok_or(LercError::WrongParam(
            "Lerc2 raw tiled flag byte count overflow",
        ))?;
    let data_len = (header.num_valid_pixel as usize)
        .checked_mul(n_depth)
        .and_then(|count| count.checked_mul(value_size))
        .ok_or(LercError::WrongParam(
            "Lerc2 raw tiled payload byte count overflow",
        ))?;

    1usize
        .checked_add(usize::from(include_image_mode))
        .and_then(|len| len.checked_add(tile_flag_count))
        .and_then(|len| len.checked_add(data_len))
        .ok_or(LercError::WrongParam(
            "Lerc2 raw tiled payload byte count overflow",
        ))
}

/// Writes a raw tiled Lerc2 payload section.
///
/// `data` must contain the full image in row-major little-endian scalar bytes,
/// including values for invalid pixels. Only valid pixels are copied into each
/// tile payload.
pub fn write_lerc2_tiled_raw(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    output: &mut [u8],
) -> Result<usize> {
    write_lerc2_tiled_raw_with_mode_prefix(header, mask, data, false, output)
}

fn write_lerc2_tiled_raw_with_mode_prefix(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    include_image_mode: bool,
    output: &mut [u8],
) -> Result<usize> {
    validate_lerc2_tiled_raw_for_write(header, mask, Some(data))?;
    let byte_len =
        compute_lerc2_tiled_raw_byte_len_with_mode_prefix(header, mask, include_image_mode)?;
    if output.len() < byte_len {
        return Err(LercError::BufferTooSmall);
    }

    let n_cols = header.n_cols as usize;
    let n_rows = header.n_rows as usize;
    let n_depth = header.n_depth as usize;
    let value_size = header.data_type.size_in_bytes();
    let mb_size = header.micro_block_size as usize;
    let tiles_vert = n_rows.div_ceil(mb_size);
    let tiles_hori = n_cols.div_ceil(mb_size);
    let mut writer = Writer::new(output);

    writer.write_bytes(&[0])?;
    if include_image_mode {
        writer.write_bytes(&[0])?;
    }
    for i_tile in 0..tiles_vert {
        let i0 = i_tile * mb_size;
        let i1 = (i0 + mb_size).min(n_rows);
        for j_tile in 0..tiles_hori {
            let j0 = j_tile * mb_size;
            let j1 = (j0 + mb_size).min(n_cols);
            let raw_flag = raw_tile_flag(header.version, j0);
            for depth in 0..n_depth {
                writer.write_bytes(&[raw_flag])?;
                for row in i0..i1 {
                    for col in j0..j1 {
                        let pixel_idx = row * n_cols + col;
                        if mask.is_valid(pixel_idx)? {
                            let offset = (pixel_idx * n_depth + depth) * value_size;
                            writer.write_bytes(&data[offset..offset + value_size])?;
                        }
                    }
                }
            }
        }
    }

    Ok(writer.pos)
}

fn encode_lerc2_tiled_simple_payload(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    include_image_mode: bool,
    allow_lut: bool,
    allow_depth_diff: bool,
) -> Result<Vec<u8>> {
    validate_lerc2_tiled_raw_for_write(header, mask, Some(data))?;
    if header.max_z_error <= 0.0 {
        return Err(LercError::WrongParam(
            "simple tiled Lerc2 payload requires positive max_z_error",
        ));
    }

    let n_cols = header.n_cols as usize;
    let n_rows = header.n_rows as usize;
    let n_depth = header.n_depth as usize;
    let value_size = header.data_type.size_in_bytes();
    let mb_size = header.micro_block_size as usize;
    let tiles_vert = n_rows.div_ceil(mb_size);
    let tiles_hori = n_cols.div_ceil(mb_size);
    let mut payload = Vec::new();

    payload.push(0);
    if include_image_mode {
        payload.push(0);
    }
    for i_tile in 0..tiles_vert {
        let i0 = i_tile * mb_size;
        let i1 = (i0 + mb_size).min(n_rows);
        for j_tile in 0..tiles_hori {
            let j0 = j_tile * mb_size;
            let j1 = (j0 + mb_size).min(n_cols);
            let tile_flag_base = raw_tile_flag(header.version, j0);
            let mut previous_values: Vec<f64> = Vec::new();
            for depth in 0..n_depth {
                let values = collect_valid_tile_values(header, mask, data, i0, i1, j0, j1, depth)?;
                let absolute = encode_lerc2_quantized_tile(
                    header,
                    &values,
                    tile_flag_base,
                    header.data_type,
                    allow_lut,
                    false,
                )?;
                let tile = if allow_depth_diff && depth > 0 && !values.is_empty() {
                    encode_lerc2_diff_tile(header, &values, &previous_values, tile_flag_base)?
                        .filter(|diff| diff.len() < absolute.len())
                        .unwrap_or(absolute)
                } else {
                    absolute
                };
                payload.extend_from_slice(&tile);
                previous_values = values;
            }
        }
    }

    if value_size == 0 {
        return Err(LercError::WrongParam("invalid Lerc2 data type byte width"));
    }
    Ok(payload)
}

fn encode_lerc2_quantized_tile(
    header: &HeaderInfo,
    values: &[f64],
    tile_flag_base: u8,
    offset_data_type: DataType,
    allow_lut: bool,
    diff_encoded: bool,
) -> Result<Vec<u8>> {
    let diff_bit = if diff_encoded { 4 } else { 0 };
    if values.is_empty() {
        return Ok(vec![tile_flag_base | diff_bit | 2]);
    }

    let z_min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let max_elem = ((z_max - z_min) / (2.0 * header.max_z_error) + 0.5) as u32;
    if max_elem == 0 {
        let mut out = vec![tile_flag_base | diff_bit | 3];
        out.extend_from_slice(&encode_value_as_bytes(offset_data_type, z_min));
        return Ok(out);
    }

    let quantized = quantize_lerc2_tile_values(header, values, z_min)?;
    let mut out = vec![tile_flag_base | diff_bit | 1];
    out.extend_from_slice(&encode_value_as_bytes(offset_data_type, z_min));
    out.extend_from_slice(&encode_quantized_tile_values(
        &quantized,
        header.version,
        allow_lut,
    )?);
    Ok(out)
}

fn encode_lerc2_diff_tile(
    header: &HeaderInfo,
    values: &[f64],
    previous_values: &[f64],
    tile_flag_base: u8,
) -> Result<Option<Vec<u8>>> {
    if values.len() != previous_values.len() || header.version < 5 {
        return Ok(None);
    }
    let mut diff_values = Vec::with_capacity(values.len());
    let mut previous_value = 0i32;
    let mut same_value_count = 0usize;
    for (&value, &previous) in values.iter().zip(previous_values.iter()) {
        let diff = value - previous;
        if diff < i32::MIN as f64 || diff > i32::MAX as f64 {
            return Ok(None);
        }
        let diff = diff as i32;
        if diff as f64 != value - previous {
            return Ok(None);
        }
        if diff == previous_value {
            same_value_count += 1;
        }
        previous_value = diff;
        diff_values.push(diff as f64);
    }
    let try_lut = values.len() > 4 && 2 * same_value_count > values.len();
    encode_lerc2_quantized_tile(
        header,
        &diff_values,
        tile_flag_base,
        DataType::Int,
        try_lut,
        true,
    )
    .map(Some)
}

fn encode_quantized_tile_values(
    quantized: &[u32],
    version: i32,
    allow_lut: bool,
) -> Result<Vec<u8>> {
    if allow_lut {
        let mut sorted = quantized
            .iter()
            .copied()
            .enumerate()
            .map(|(idx, value)| (value, idx as u32))
            .collect::<Vec<_>>();
        sorted.sort_unstable();
        if matches!(
            BitStuffer2::compute_num_bytes_needed_lut(&sorted),
            Ok((_, true))
        ) {
            if let Ok(encoded) = BitStuffer2::encode_lut(&sorted, version) {
                return Ok(encoded);
            }
        }
    }

    BitStuffer2::encode_simple(quantized, version)
}

fn collect_valid_tile_values(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    depth: usize,
) -> Result<Vec<f64>> {
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let value_size = header.data_type.size_in_bytes();
    let mut values = Vec::new();

    for row in i0..i1 {
        for col in j0..j1 {
            let pixel_idx = row * n_cols + col;
            if mask.is_valid(pixel_idx)? {
                let offset = (pixel_idx * n_depth + depth) * value_size;
                values.push(read_value_from_bytes(
                    header.data_type,
                    &data[offset..offset + value_size],
                ));
            }
        }
    }

    Ok(values)
}

fn quantize_lerc2_tile_values(header: &HeaderInfo, values: &[f64], z_min: f64) -> Result<Vec<u32>> {
    let scale = 1.0 / (2.0 * header.max_z_error);
    values
        .iter()
        .map(|&value| {
            let quantized = if is_integer_data_type(header.data_type) && header.max_z_error == 0.5 {
                value - z_min
            } else {
                (value - z_min) * scale + 0.5
            };
            if !(0.0..=(u32::MAX as f64)).contains(&quantized) {
                return Err(LercError::WrongParam(
                    "Lerc2 simple tiled quantized value overflow",
                ));
            }
            Ok(quantized as u32)
        })
        .collect()
}

/// Computes per-depth min/max ranges from full image bytes for Lerc2 encoding.
///
/// `data` must contain one single-band image in row-major little-endian scalar
/// bytes. Invalid pixels in `mask` are ignored. When no pixel is valid, the
/// returned ranges are all zero and marked as equal.
pub fn compute_lerc2_data_ranges_for_encode(
    spec: EncodeSpec,
    data: &[u8],
    mask: Option<&BitMask>,
) -> Result<MinMaxRanges> {
    validate_single_band_encode_inputs(spec, data, mask, "Lerc2 range computation")?;
    let mask = effective_encode_mask(spec, mask)?;
    compute_lerc2_data_ranges_for_encode_with_mask(spec, data, &mask)
}

fn compute_lerc2_data_ranges_for_encode_with_mask(
    spec: EncodeSpec,
    data: &[u8],
    mask: &BitMask,
) -> Result<MinMaxRanges> {
    let value_size = spec.data_type.size_in_bytes();
    let mut mins = vec![f64::INFINITY; spec.n_depth];
    let mut maxs = vec![f64::NEG_INFINITY; spec.n_depth];
    let mut any_valid = false;

    for row in 0..spec.n_rows {
        for col in 0..spec.n_cols {
            let pixel_idx = row * spec.n_cols + col;
            if mask.is_valid(pixel_idx)? {
                any_valid = true;
                for depth in 0..spec.n_depth {
                    let offset = (pixel_idx * spec.n_depth + depth) * value_size;
                    let value =
                        read_value_from_bytes(spec.data_type, &data[offset..offset + value_size]);
                    if value.is_nan() {
                        return Err(LercError::WrongParam("Lerc2 encode input contains NaN"));
                    }
                    mins[depth] = mins[depth].min(value);
                    maxs[depth] = maxs[depth].max(value);
                }
            }
        }
    }

    if !any_valid {
        mins.fill(0.0);
        maxs.fill(0.0);
    }

    let min_max_equal = mins
        .iter()
        .zip(maxs.iter())
        .all(|(min, max)| min.to_bits() == max.to_bits());
    Ok(MinMaxRanges {
        mins,
        maxs,
        bytes_consumed: 0,
        min_max_equal,
    })
}

/// Encodes a single-band Lerc2 blob using the one-sweep payload layout.
///
/// This safe fallback encoder writes version 2 and newer blobs. Version 2 and
/// 3 blobs are limited by the Lerc2 header layout to single-depth data. Version
/// 4 and newer blobs also carry per-depth min/max ranges. The encoder computes
/// ranges from the input bytes, writes the header, mask, optional range section,
/// and uncompressed one-sweep payload, then finalizes the version 3+ checksum.
pub fn encode_lerc2_one_sweep(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_one_sweep_band(spec, data, max_z_error, mask, version, 0, true, None)
}

/// Encodes Lerc2 data using the current uncompressed fallback strategy.
///
/// This helper chooses the smallest safe uncompressed path currently ported by
/// the Rust encoder: single-band constant images use [`encode_lerc2_constant`],
/// single-band non-constant images use [`encode_lerc2_one_sweep`], and
/// multi-band images use [`encode_lerc2_one_sweep_bands`]. `masks` follows the
/// public C API convention: no masks means all pixels are valid, one mask is
/// shared by all bands, and `n_bands` masks provide one mask per band.
pub fn encode_lerc2_uncompressed(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "uncompressed Lerc2 encode")?;
    if spec.n_bands != 1 {
        if all_bands_constant_for_uncompressed_encode(spec, data)? {
            return encode_lerc2_constant_bands(spec, data, max_z_error, masks, version);
        }
        return encode_lerc2_one_sweep_bands(spec, data, max_z_error, masks, version);
    }

    let mask = match masks {
        Some(mask_bytes) => Some(BitMask::from_byte_mask(
            &mask_bytes[..spec.mask_byte_len()?],
            spec.n_cols,
            spec.n_rows,
        )?),
        None => None,
    };
    match constant_value_for_uncompressed_encode(spec.data_type, data)? {
        Some(value) => encode_lerc2_constant(spec, value, max_z_error, mask.as_ref(), version),
        None => encode_lerc2_one_sweep(spec, data, max_z_error, mask.as_ref(), version),
    }
}

/// Encodes Lerc2 data using the current uncompressed fallback strategy with optional no-data metadata.
///
/// When `uses_no_data` is absent or contains only zero values, this delegates to
/// [`encode_lerc2_uncompressed`] and may still choose the constant path. When
/// any band is flagged as using no-data, this applies the C++ no-data filtering
/// path through the one-sweep encoder. Single-depth inputs encode all-sentinel
/// pixels through the mask only; multi-depth mixed valid/sentinel pixels use
/// version 6+ no-data metadata. Source data is expected to already contain the
/// per-band no-data sentinel wherever no-data should be represented.
pub fn encode_lerc2_uncompressed_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<Vec<u8>> {
    let has_active_no_data = uses_no_data
        .as_ref()
        .is_some_and(|uses| uses.iter().any(|&uses| uses != 0));
    if has_active_no_data {
        encode_lerc2_one_sweep_bands_with_no_data(
            spec,
            data,
            max_z_error,
            masks,
            uses_no_data,
            no_data_values,
            version,
        )
    } else {
        encode_lerc2_uncompressed(spec, data, max_z_error, masks, version)
    }
}

/// Encodes Lerc2 data with optional no-data metadata using the ported size selector.
///
/// This is the no-data-aware variant of [`encode_lerc2_auto`]. It keeps
/// [`encode_lerc2_uncompressed_with_no_data`] as the baseline and, for version
/// 6+ `UChar`/`Char` data with `max_z_error == 0.5`, also tries byte Huffman
/// after applying the same no-data mask filtering and internal-sentinel remap
/// as the uncompressed no-data helpers. The smaller valid blob is returned.
pub fn encode_lerc2_auto_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<Vec<u8>> {
    let has_active_no_data = uses_no_data
        .as_ref()
        .is_some_and(|uses| uses.iter().any(|&uses| uses != 0));
    if !has_active_no_data {
        return encode_lerc2_auto(spec, data, max_z_error, masks, version);
    }

    let baseline = encode_lerc2_uncompressed_with_no_data(
        spec,
        data,
        max_z_error,
        masks,
        uses_no_data,
        no_data_values,
        version,
    )?;
    if !matches!(spec.data_type, DataType::UChar | DataType::Char)
        || version < 6
        || !auto_byte_huffman_max_z_error_is_lossless_byte(spec, data, max_z_error, masks)?
    {
        return Ok(baseline);
    }

    match encode_lerc2_byte_huffman_bands_with_no_data(
        spec,
        data,
        masks,
        uses_no_data,
        no_data_values,
        version,
    ) {
        Ok(huffman) if huffman.len() < baseline.len() => Ok(huffman),
        Ok(_) => Ok(baseline),
        Err(LercError::WrongParam("constant byte input should use Lerc2 constant encode")) => {
            Ok(baseline)
        }
        Err(LercError::WrongParam("constant byte ranges should use Lerc2 constant encode")) => {
            Ok(baseline)
        }
        Err(LercError::WrongParam("byte Huffman encode requires at least two symbols")) => {
            Ok(baseline)
        }
        Err(err) => Err(err),
    }
}

/// Encodes Lerc2 data using the currently ported size-based safe selector.
///
/// The selector keeps [`encode_lerc2_uncompressed`] as the baseline and, for
/// `UChar`/`Char` data with `max_z_error == 0.5`, also tries byte Huffman.
/// The smaller valid blob is returned.
pub fn encode_lerc2_auto(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    let baseline = encode_lerc2_uncompressed(spec, data, max_z_error, masks, version)?;
    if !matches!(spec.data_type, DataType::UChar | DataType::Char)
        || version < 2
        || !auto_byte_huffman_max_z_error_is_lossless_byte(spec, data, max_z_error, masks)?
    {
        return Ok(baseline);
    }

    if spec.n_bands != 1 {
        return match encode_lerc2_byte_huffman_bands(spec, data, masks, version) {
            Ok(huffman) if huffman.len() < baseline.len() => Ok(huffman),
            Ok(_) => Ok(baseline),
            Err(LercError::WrongParam("constant byte input should use Lerc2 constant encode")) => {
                Ok(baseline)
            }
            Err(LercError::WrongParam("constant byte ranges should use Lerc2 constant encode")) => {
                Ok(baseline)
            }
            Err(LercError::WrongParam("byte Huffman encode requires at least two symbols")) => {
                Ok(baseline)
            }
            Err(err) => Err(err),
        };
    }

    let mask = match masks {
        Some(mask_bytes) => Some(BitMask::from_byte_mask(
            &mask_bytes[..spec.mask_byte_len()?],
            spec.n_cols,
            spec.n_rows,
        )?),
        None => None,
    };
    match encode_lerc2_byte_huffman(spec, data, mask.as_ref(), version) {
        Ok(huffman) if huffman.len() < baseline.len() => Ok(huffman),
        Ok(_) => Ok(baseline),
        Err(LercError::WrongParam("constant byte input should use Lerc2 constant encode")) => {
            Ok(baseline)
        }
        Err(LercError::WrongParam("constant byte ranges should use Lerc2 constant encode")) => {
            Ok(baseline)
        }
        Err(LercError::WrongParam("byte Huffman encode requires at least two symbols")) => {
            Ok(baseline)
        }
        Err(err) => Err(err),
    }
}

fn auto_byte_huffman_max_z_error_is_lossless_byte(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
) -> Result<bool> {
    if !matches!(spec.data_type, DataType::UChar | DataType::Char) {
        return Ok(false);
    }

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec.mask_byte_len()?;
    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 band data offset overflow"))?;
        let data_end = data_start
            .checked_add(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 band data offset overflow"))?;
        let band_data = data
            .get(data_start..data_end)
            .ok_or(LercError::WrongParam("Lerc2 encode data length mismatch"))?;
        let band_mask = match (masks, spec.n_masks) {
            (None, _) | (_, 0) => None,
            (Some(mask_bytes), 1) => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            (Some(mask_bytes), _) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 mask offset overflow"))?;
                let mask_end = mask_start
                    .checked_add(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    mask_bytes
                        .get(mask_start..mask_end)
                        .ok_or(LercError::WrongParam("Lerc2 mask byte length mismatch"))?,
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
        };
        let effective_mask = effective_encode_mask(band_spec, band_mask.as_ref())?;
        let normalized = normalize_lerc2_max_z_error_for_encode(
            band_spec,
            band_data,
            &effective_mask,
            max_z_error,
        )?;
        if normalized != 0.5 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Encodes band-major byte data as concatenated byte-Huffman Lerc2 blobs.
///
/// This helper mirrors the existing band-major encode layout: no masks means
/// all pixels are valid, one mask is shared by all bands and may be omitted
/// after the first band, and `n_bands` masks provide per-band masks. Version 6
/// blobs carry `nBlobsMore`; version 4 and 5 concatenation relies on the next
/// blob header.
pub fn encode_lerc2_byte_huffman_bands(
    spec: EncodeSpec,
    data: &[u8],
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_byte_huffman_bands_impl(spec, data, masks, None, None, version)
}

/// Encodes band-major byte data as byte-Huffman Lerc2 blobs with no-data metadata.
///
/// This version 6+ helper carries no-data metadata for bands whose
/// `uses_no_data` entry is nonzero. The source data is expected to already
/// contain the per-band no-data sentinel value wherever no-data should be
/// represented. Pixels whose every depth equals the sentinel are marked invalid;
/// mixed-depth sentinel samples are remapped to an internal sentinel when needed
/// and restored to the original sentinel during decode.
pub fn encode_lerc2_byte_huffman_bands_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_byte_huffman_bands_impl(spec, data, masks, uses_no_data, no_data_values, version)
}

fn encode_lerc2_byte_huffman_bands_impl(
    spec: EncodeSpec,
    data: &[u8],
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "byte Huffman Lerc2 band encode")?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "byte Huffman Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !matches!(spec.data_type, DataType::UChar | DataType::Char) {
        return Err(LercError::WrongParam(
            "byte Huffman Lerc2 encode requires Char or UChar data",
        ));
    }
    validate_encode_no_data_inputs(spec, uses_no_data, no_data_values, version)?;

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let no_data = uses_no_data
            .and_then(|uses| uses.get(band).copied())
            .filter(|&uses| uses != 0)
            .and_then(|_| no_data_values.and_then(|values| values.get(band).copied()))
            .map(|value| (value, value));
        let band_data = &data[data_start..data_start + band_data_len];
        let prepared = if let Some((_, no_data_orig)) = no_data {
            Some(prepare_no_data_band_for_encode(
                band_spec,
                band_data,
                mask.as_ref(),
                no_data_orig,
                0.5,
            )?)
        } else {
            None
        };
        let prepared_data = prepared
            .as_ref()
            .map(|prepared| prepared.data.as_slice())
            .unwrap_or(band_data);
        let prepared_mask = prepared
            .as_ref()
            .and_then(|prepared| prepared.mask.as_ref())
            .or(mask.as_ref());
        let prepared_no_data = prepared
            .as_ref()
            .and_then(|prepared| prepared.no_data)
            .or_else(|| (spec.n_depth > 1).then_some(no_data).flatten());
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 && prepared.is_none() {
            false
        } else {
            match (prepared_mask, &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let band_blob = encode_lerc2_byte_huffman_band(
            EncodeSpec {
                n_masks: usize::from(prepared_mask.is_some()),
                ..band_spec
            },
            prepared_data,
            prepared_mask,
            version,
            n_blobs_more,
            encode_mask,
            prepared_no_data,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = prepared_mask.cloned();
    }

    Ok(blob)
}

/// Encodes a single-band Lerc2 blob using one-sweep payloads with no-data metadata.
///
/// This version 6+ helper expects `data` to already contain `no_data_value`
/// wherever no-data should be represented. Pixels whose every depth equals the
/// sentinel are marked invalid; mixed-depth sentinel samples are remapped to an
/// internal sentinel when needed and restored during decode. Single-depth inputs
/// use mask filtering only and do not write no-data metadata.
pub fn encode_lerc2_one_sweep_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    no_data_value: f64,
    version: i32,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "one-sweep Lerc2 no-data encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    let prepared = prepare_no_data_band_for_encode(spec, data, mask, no_data_value, max_z_error)?;
    let prepared_data = prepared.data.as_slice();
    let prepared_mask = prepared.mask.as_ref().or(mask);
    encode_lerc2_one_sweep_band(
        EncodeSpec {
            n_masks: usize::from(prepared_mask.is_some()),
            ..spec
        },
        prepared_data,
        max_z_error,
        prepared_mask,
        version,
        0,
        true,
        if spec.n_depth > 1 {
            prepared.no_data.or(Some((no_data_value, no_data_value)))
        } else {
            None
        },
    )
}

/// Encodes a single-band Lerc2 blob using raw tiled payloads.
///
/// This safe encode path writes version 2 and newer blobs. Version 2 and 3
/// blobs are limited by the Lerc2 header layout to single-depth data. Version
/// 4 and newer blobs also carry per-depth min/max ranges. Header configurations
/// that use the C++ Huffman-probe envelope are emitted with image mode 0,
/// keeping the tiled payload raw and uncompressed.
pub fn encode_lerc2_tiled_raw(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_tiled_raw_band(
        spec,
        data,
        max_z_error,
        mask,
        version,
        micro_block_size,
        0,
        true,
        None,
    )
}

/// Encodes a single-band Lerc2 blob using simple bit-stuffed tiled payloads.
///
/// This version 2+ helper ports the C++ quantized tile layout without the
/// later LUT or depth-difference tile choices. Each tile/depth plane is
/// quantized from its tile minimum using `2 * max_z_error` and written with
/// [`BitStuffer2::encode_simple`]. Version 2 and 3 blobs are limited to
/// `n_depth == 1` by the legacy header layout.
pub fn encode_lerc2_tiled_simple(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_tiled_simple_band(
        spec,
        data,
        max_z_error,
        mask,
        version,
        micro_block_size,
        0,
        true,
        false,
        None,
    )
}

/// Encodes a single-band Lerc2 blob using LUT-capable bit-stuffed tiled payloads.
///
/// This version 2+ helper uses the same tile quantization as
/// [`encode_lerc2_tiled_simple`], but may write a `BitStuffer2` LUT stream for
/// sparse quantized tile values when it is smaller than the simple stream.
/// Tiles where LUT does not win are written with the simple bit-stuffed stream.
/// Version 2 and 3 blobs are limited to `n_depth == 1` by the legacy header
/// layout.
pub fn encode_lerc2_tiled_lut(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_tiled_simple_band(
        spec,
        data,
        max_z_error,
        mask,
        version,
        micro_block_size,
        0,
        true,
        true,
        None,
    )
}

/// Encodes a single-band Lerc2 blob using simple bit-stuffed tiled payloads with no-data metadata.
///
/// This version 6+ helper expects `data` to already contain `no_data_value`
/// wherever no-data should be represented. Pixels whose every depth equals the
/// sentinel are marked invalid; mixed-depth sentinel samples are remapped to an
/// internal sentinel when needed and restored during decode. Single-depth inputs
/// use mask filtering only and do not write no-data metadata.
pub fn encode_lerc2_tiled_simple_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    no_data_value: f64,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "simple tiled Lerc2 no-data encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    let prepared = prepare_no_data_band_for_encode(spec, data, mask, no_data_value, max_z_error)?;
    let prepared_data = prepared.data.as_slice();
    let prepared_mask = prepared.mask.as_ref().or(mask);
    encode_lerc2_tiled_simple_band(
        EncodeSpec {
            n_masks: usize::from(prepared_mask.is_some()),
            ..spec
        },
        prepared_data,
        max_z_error,
        prepared_mask,
        version,
        micro_block_size,
        0,
        true,
        false,
        if spec.n_depth > 1 {
            prepared.no_data.or(Some((no_data_value, no_data_value)))
        } else {
            None
        },
    )
}

/// Encodes a single-band Lerc2 blob using raw tiled payloads with no-data metadata.
///
/// This version 6+ helper expects `data` to already contain `no_data_value`
/// wherever no-data should be represented. Pixels whose every depth equals the
/// sentinel are marked invalid; mixed-depth sentinel samples are remapped to an
/// internal sentinel when needed and restored during decode. Single-depth inputs
/// use mask filtering only and do not write no-data metadata.
pub fn encode_lerc2_tiled_raw_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    mask: Option<&BitMask>,
    no_data_value: f64,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "raw tiled Lerc2 no-data encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    let prepared = prepare_no_data_band_for_encode(spec, data, mask, no_data_value, max_z_error)?;
    let prepared_data = prepared.data.as_slice();
    let prepared_mask = prepared.mask.as_ref().or(mask);
    encode_lerc2_tiled_raw_band(
        EncodeSpec {
            n_masks: usize::from(prepared_mask.is_some()),
            ..spec
        },
        prepared_data,
        max_z_error,
        prepared_mask,
        version,
        micro_block_size,
        0,
        true,
        if spec.n_depth > 1 {
            prepared.no_data.or(Some((no_data_value, no_data_value)))
        } else {
            None
        },
    )
}

/// Encodes a single-band byte Lerc2 blob using integer Huffman payloads.
///
/// This ports the lossless byte Huffman image modes used by the C++ encoder for
/// `UChar` and `Char` data with `max_z_error == 0.5`. Version 4 and newer may
/// choose either regular Huffman or delta Huffman, matching the C++ mode
/// selection rule of using the smaller candidate. Constant inputs should use
/// [`encode_lerc2_constant`] instead.
pub fn encode_lerc2_byte_huffman(
    spec: EncodeSpec,
    data: &[u8],
    mask: Option<&BitMask>,
    version: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_byte_huffman_band(spec, data, mask, version, 0, true, None)
}

/// Encodes a single-band byte Lerc2 blob using Huffman payloads with no-data metadata.
///
/// This version 6+ helper expects `data` to already contain `no_data_value`
/// wherever no-data should be represented. Pixels whose every depth equals the
/// sentinel are marked invalid; mixed-depth sentinel samples are remapped to an
/// internal sentinel when needed and restored during decode. Single-depth inputs
/// use mask filtering only and do not write no-data metadata.
pub fn encode_lerc2_byte_huffman_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    mask: Option<&BitMask>,
    no_data_value: f64,
    version: i32,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "byte Huffman Lerc2 no-data encode")?;
    if version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    let prepared = prepare_no_data_band_for_encode(spec, data, mask, no_data_value, 0.5)?;
    let prepared_data = prepared.data.as_slice();
    let prepared_mask = prepared.mask.as_ref().or(mask);
    encode_lerc2_byte_huffman_band(
        EncodeSpec {
            n_masks: usize::from(prepared_mask.is_some()),
            ..spec
        },
        prepared_data,
        prepared_mask,
        version,
        0,
        true,
        if spec.n_depth > 1 {
            prepared.no_data.or(Some((no_data_value, no_data_value)))
        } else {
            None
        },
    )
}

fn encode_lerc2_byte_huffman_band(
    spec: EncodeSpec,
    data: &[u8],
    mask: Option<&BitMask>,
    version: i32,
    n_blobs_more: i32,
    encode_partial_mask: bool,
    no_data: Option<(f64, f64)>,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "byte Huffman Lerc2 encode")?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "byte Huffman Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !matches!(spec.data_type, DataType::UChar | DataType::Char) {
        return Err(LercError::WrongParam(
            "byte Huffman Lerc2 encode requires Char or UChar data",
        ));
    }
    if no_data.is_some() && version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    if no_data.is_some() && spec.n_depth <= 1 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires depth greater than 1",
        ));
    }

    let mask = effective_encode_mask(spec, mask)?;
    let num_valid_pixel = mask.count_valid_bits();
    if num_valid_pixel == 0 {
        return Err(LercError::WrongParam(
            "byte Huffman Lerc2 encode requires valid pixels",
        ));
    }
    if constant_value_for_uncompressed_encode(spec.data_type, data)?.is_some() {
        return Err(LercError::WrongParam(
            "constant byte input should use Lerc2 constant encode",
        ));
    }

    let ranges = compute_lerc2_data_ranges_for_encode_with_mask(spec, data, &mask)?;
    if ranges.min_max_equal {
        return Err(LercError::WrongParam(
            "constant byte ranges should use Lerc2 constant encode",
        ));
    }

    let z_min = ranges.mins.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = ranges
        .maxs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let header_size = compute_lerc2_header_byte_len(version)?;
    let mut header = HeaderInfo {
        version,
        checksum: 0,
        n_rows: spec.n_rows as i32,
        n_cols: spec.n_cols as i32,
        n_depth: spec.n_depth as i32,
        num_valid_pixel: num_valid_pixel as i32,
        micro_block_size: 8,
        blob_size: 1,
        n_blobs_more,
        b_pass_no_data_values: u8::from(no_data.is_some()),
        b_is_int: 1,
        b_reserved_3: 0,
        b_reserved_4: 0,
        data_type: spec.data_type,
        max_z_error: 0.5,
        z_min,
        z_max,
        no_data_val: no_data.map(|values| values.0).unwrap_or(0.0),
        no_data_val_orig: no_data.map(|values| values.1).unwrap_or(0.0),
        header_size,
    };

    let payload = encode_huffman_int_payload(&header, &mask, data)?;
    let encode_mask = encode_partial_mask && mask.count_valid_bits() < spec.n_cols * spec.n_rows;
    let mask_len = compute_lerc2_mask_byte_len(&header, Some(&mask), encode_mask)?;
    let ranges_len = if version >= 4 {
        compute_lerc2_min_max_ranges_byte_len(&header)?
    } else {
        0
    };
    header.blob_size = header
        .header_size
        .checked_add(mask_len)
        .and_then(|len| len.checked_add(ranges_len))
        .and_then(|len| len.checked_add(payload.len()))
        .and_then(|len| i32::try_from(len).ok())
        .ok_or(LercError::WrongParam(
            "Lerc2 byte Huffman blob size overflow",
        ))?;

    let mut blob = vec![0; header.blob_size as usize];
    let mut offset = write_lerc2_header(&header, &mut blob)?;
    offset += write_lerc2_mask(&header, Some(&mask), encode_mask, &mut blob[offset..])?;
    if ranges_len > 0 {
        offset += write_lerc2_min_max_ranges(&header, &ranges, &mut blob[offset..])?;
    }
    blob[offset..offset + payload.len()].copy_from_slice(&payload);
    offset += payload.len();
    debug_assert_eq!(offset, blob.len());
    finalize_lerc2_checksum(&mut blob)?;
    Ok(blob)
}

fn encode_lerc2_tiled_simple_band(
    spec: EncodeSpec,
    data: &[u8],
    mut max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    micro_block_size: i32,
    n_blobs_more: i32,
    encode_partial_mask: bool,
    allow_lut: bool,
    no_data: Option<(f64, f64)>,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "simple tiled Lerc2 encode")?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "simple tiled Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !(1..=32).contains(&micro_block_size) {
        return Err(LercError::WrongParam(
            "Lerc2 simple tiled micro block size must be 1 through 32",
        ));
    }
    if no_data.is_some() && version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    if no_data.is_some() && spec.n_depth <= 1 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires depth greater than 1",
        ));
    }

    let mask = effective_encode_mask(spec, mask)?;
    let num_valid_pixel = mask.count_valid_bits();
    max_z_error = normalize_lerc2_max_z_error_for_encode(spec, data, &mask, max_z_error)?;
    if max_z_error <= 0.0 {
        return Err(LercError::WrongParam(
            "simple tiled Lerc2 encode requires positive max_z_error",
        ));
    }
    let ranges = compute_lerc2_data_ranges_for_encode_with_mask(spec, data, &mask)?;
    let z_min = ranges.mins.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = ranges
        .maxs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let has_valid = num_valid_pixel > 0;
    let header_size = compute_lerc2_header_byte_len(version)?;
    let mut header = HeaderInfo {
        version,
        checksum: 0,
        n_rows: spec.n_rows as i32,
        n_cols: spec.n_cols as i32,
        n_depth: spec.n_depth as i32,
        num_valid_pixel: num_valid_pixel as i32,
        micro_block_size,
        blob_size: 1,
        n_blobs_more,
        b_pass_no_data_values: u8::from(no_data.is_some()),
        b_is_int: u8::from(data_values_are_integer(spec.data_type, data)?),
        b_reserved_3: 0,
        b_reserved_4: 0,
        data_type: spec.data_type,
        max_z_error,
        z_min: if has_valid { z_min } else { 0.0 },
        z_max: if has_valid { z_max } else { 0.0 },
        no_data_val: no_data.map(|values| values.0).unwrap_or(0.0),
        no_data_val_orig: no_data.map(|values| values.1).unwrap_or(0.0),
        header_size,
    };

    let include_image_mode = try_huffman_int(&header) || try_huffman_float(&header);
    let payload = if has_valid && header.z_min != header.z_max && !ranges.min_max_equal {
        let allow_depth_diff = allow_lut
            && header.version >= 5
            && header.n_depth > 1
            && is_integer_data_type(header.data_type)
            && header.max_z_error == 0.5;
        encode_lerc2_tiled_simple_payload(
            &header,
            &mask,
            data,
            include_image_mode,
            allow_lut,
            allow_depth_diff,
        )?
    } else {
        Vec::new()
    };
    let encode_mask = encode_partial_mask && mask.count_valid_bits() < spec.n_cols * spec.n_rows;
    let mask_len = compute_lerc2_mask_byte_len(&header, Some(&mask), encode_mask)?;
    let ranges_len = if version >= 4 && has_valid && header.z_min != header.z_max {
        compute_lerc2_min_max_ranges_byte_len(&header)?
    } else {
        0
    };
    header.blob_size = header
        .header_size
        .checked_add(mask_len)
        .and_then(|len| len.checked_add(ranges_len))
        .and_then(|len| len.checked_add(payload.len()))
        .and_then(|len| i32::try_from(len).ok())
        .ok_or(LercError::WrongParam(
            "Lerc2 simple tiled blob size overflow",
        ))?;

    let mut blob = vec![0; header.blob_size as usize];
    let mut offset = write_lerc2_header(&header, &mut blob)?;
    offset += write_lerc2_mask(&header, Some(&mask), encode_mask, &mut blob[offset..])?;
    if ranges_len > 0 {
        offset += write_lerc2_min_max_ranges(&header, &ranges, &mut blob[offset..])?;
    }
    if !payload.is_empty() {
        blob[offset..offset + payload.len()].copy_from_slice(&payload);
        offset += payload.len();
    }
    debug_assert_eq!(offset, blob.len());
    finalize_lerc2_checksum(&mut blob)?;
    Ok(blob)
}

fn encode_lerc2_tiled_raw_band(
    spec: EncodeSpec,
    data: &[u8],
    mut max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    micro_block_size: i32,
    n_blobs_more: i32,
    encode_partial_mask: bool,
    no_data: Option<(f64, f64)>,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "raw tiled Lerc2 encode")?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "raw tiled Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !(1..=32).contains(&micro_block_size) {
        return Err(LercError::WrongParam(
            "Lerc2 raw tiled micro block size must be 1 through 32",
        ));
    }
    if no_data.is_some() && version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    if no_data.is_some() && spec.n_depth <= 1 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires depth greater than 1",
        ));
    }

    let mask = effective_encode_mask(spec, mask)?;
    let num_valid_pixel = mask.count_valid_bits();
    max_z_error = normalize_lerc2_max_z_error_for_encode(spec, data, &mask, max_z_error)?;
    let ranges = compute_lerc2_data_ranges_for_encode_with_mask(spec, data, &mask)?;
    let z_min = ranges.mins.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = ranges
        .maxs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let has_valid = num_valid_pixel > 0;
    let header_size = compute_lerc2_header_byte_len(version)?;
    let mut header = HeaderInfo {
        version,
        checksum: 0,
        n_rows: spec.n_rows as i32,
        n_cols: spec.n_cols as i32,
        n_depth: spec.n_depth as i32,
        num_valid_pixel: num_valid_pixel as i32,
        micro_block_size,
        blob_size: 1,
        n_blobs_more,
        b_pass_no_data_values: u8::from(no_data.is_some()),
        b_is_int: u8::from(data_values_are_integer(spec.data_type, data)?),
        b_reserved_3: 0,
        b_reserved_4: 0,
        data_type: spec.data_type,
        max_z_error,
        z_min: if has_valid { z_min } else { 0.0 },
        z_max: if has_valid { z_max } else { 0.0 },
        no_data_val: no_data.map(|values| values.0).unwrap_or(0.0),
        no_data_val_orig: no_data.map(|values| values.1).unwrap_or(0.0),
        header_size,
    };
    let include_image_mode = try_huffman_int(&header) || try_huffman_float(&header);

    let encode_mask = encode_partial_mask && mask.count_valid_bits() < spec.n_cols * spec.n_rows;
    let mask_len = compute_lerc2_mask_byte_len(&header, Some(&mask), encode_mask)?;
    let ranges_len = if version >= 4 && has_valid && header.z_min != header.z_max {
        compute_lerc2_min_max_ranges_byte_len(&header)?
    } else {
        0
    };
    let payload_len = if has_valid && header.z_min != header.z_max && !ranges.min_max_equal {
        compute_lerc2_tiled_raw_byte_len_with_mode_prefix(&header, &mask, include_image_mode)?
    } else {
        0
    };
    header.blob_size = header
        .header_size
        .checked_add(mask_len)
        .and_then(|len| len.checked_add(ranges_len))
        .and_then(|len| len.checked_add(payload_len))
        .and_then(|len| i32::try_from(len).ok())
        .ok_or(LercError::WrongParam("Lerc2 raw tiled blob size overflow"))?;

    let mut blob = vec![0; header.blob_size as usize];
    let mut offset = write_lerc2_header(&header, &mut blob)?;
    offset += write_lerc2_mask(&header, Some(&mask), encode_mask, &mut blob[offset..])?;
    if ranges_len > 0 {
        offset += write_lerc2_min_max_ranges(&header, &ranges, &mut blob[offset..])?;
    }
    if payload_len > 0 {
        offset += write_lerc2_tiled_raw_with_mode_prefix(
            &header,
            &mask,
            data,
            include_image_mode,
            &mut blob[offset..],
        )?;
    }
    debug_assert_eq!(offset, blob.len());
    finalize_lerc2_checksum(&mut blob)?;
    Ok(blob)
}

/// Encodes band-major data as concatenated single-band raw tiled Lerc2 blobs.
///
/// `data` must contain `n_bands` complete bands in band-major order. Masks
/// follow the public C API convention: no masks means all pixels are valid, one
/// mask is shared by all bands, and `n_bands` masks provide one mask per band.
/// Version 6 blobs carry `nBlobsMore`; earlier concatenation relies on the
/// following blob header. Version 2 and 3 blobs are limited to `n_depth == 1`
/// because those headers do not carry a depth field.
pub fn encode_lerc2_tiled_raw_bands(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_tiled_raw_bands_with_no_data(
        spec,
        data,
        max_z_error,
        masks,
        None,
        None,
        version,
        micro_block_size,
    )
}

/// Encodes band-major data as concatenated simple bit-stuffed tiled Lerc2 blobs.
///
/// `data` must contain `n_bands` complete bands in band-major order. Masks
/// follow the public C API convention: no masks means all pixels are valid, one
/// mask is shared by all bands, and `n_bands` masks provide one mask per band.
/// Version 6 blobs carry `nBlobsMore`; earlier concatenation relies on the
/// following blob header. Version 2 and 3 blobs are limited to `n_depth == 1`
/// because those headers do not carry a depth field.
pub fn encode_lerc2_tiled_simple_bands(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "simple tiled Lerc2 band encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "simple tiled Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !(1..=32).contains(&micro_block_size) {
        return Err(LercError::WrongParam(
            "Lerc2 simple tiled micro block size must be 1 through 32",
        ));
    }

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 {
            false
        } else {
            match (mask.as_ref(), &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let band_blob = encode_lerc2_tiled_simple_band(
            EncodeSpec {
                n_masks: usize::from(mask.is_some()),
                ..band_spec
            },
            &data[data_start..data_start + band_data_len],
            max_z_error,
            mask.as_ref(),
            version,
            micro_block_size,
            n_blobs_more,
            encode_mask,
            false,
            None,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = mask;
    }

    Ok(blob)
}

/// Encodes band-major data as concatenated LUT-capable bit-stuffed tiled Lerc2 blobs.
///
/// `data` must contain `n_bands` complete bands in band-major order. Masks
/// follow the public C API convention: no masks means all pixels are valid, one
/// mask is shared by all bands, and `n_bands` masks provide one mask per band.
/// Each tile may use either simple bit stuffing or LUT bit stuffing, depending
/// on which stream is smaller.
pub fn encode_lerc2_tiled_lut_bands(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "LUT tiled Lerc2 band encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "LUT tiled Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !(1..=32).contains(&micro_block_size) {
        return Err(LercError::WrongParam(
            "Lerc2 LUT tiled micro block size must be 1 through 32",
        ));
    }

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 {
            false
        } else {
            match (mask.as_ref(), &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let band_blob = encode_lerc2_tiled_simple_band(
            EncodeSpec {
                n_masks: usize::from(mask.is_some()),
                ..band_spec
            },
            &data[data_start..data_start + band_data_len],
            max_z_error,
            mask.as_ref(),
            version,
            micro_block_size,
            n_blobs_more,
            encode_mask,
            true,
            None,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = mask;
    }

    Ok(blob)
}

/// Encodes band-major data as concatenated simple bit-stuffed tiled Lerc2 blobs with no-data metadata.
///
/// This helper carries version 6+ no-data metadata for bands whose
/// `uses_no_data` entry is nonzero. The source data is expected to already
/// contain the per-band no-data sentinel value wherever no-data should be
/// represented. Single-depth sentinel pixels and all-depth multi-depth
/// sentinel pixels are moved into the mask; mixed-depth sentinel samples are
/// remapped to an internal sentinel when needed and restored during decode.
pub fn encode_lerc2_tiled_simple_bands_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "simple tiled Lerc2 band encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "simple tiled Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !(1..=32).contains(&micro_block_size) {
        return Err(LercError::WrongParam(
            "Lerc2 simple tiled micro block size must be 1 through 32",
        ));
    }
    validate_encode_no_data_inputs(spec, uses_no_data, no_data_values, version)?;

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let no_data = uses_no_data
            .and_then(|uses| uses.get(band).copied())
            .filter(|&uses| uses != 0)
            .and_then(|_| no_data_values.and_then(|values| values.get(band).copied()))
            .map(|value| (value, value));
        let band_data = &data[data_start..data_start + band_data_len];
        let prepared = if let Some((_, no_data_orig)) = no_data {
            Some(prepare_no_data_band_for_encode(
                band_spec,
                band_data,
                mask.as_ref(),
                no_data_orig,
                max_z_error,
            )?)
        } else {
            None
        };
        let prepared_data = prepared
            .as_ref()
            .map(|prepared| prepared.data.as_slice())
            .unwrap_or(band_data);
        let prepared_mask = prepared
            .as_ref()
            .and_then(|prepared| prepared.mask.as_ref())
            .or(mask.as_ref());
        let prepared_no_data = prepared
            .as_ref()
            .and_then(|prepared| prepared.no_data)
            .or_else(|| (spec.n_depth > 1).then_some(no_data).flatten());
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 && prepared.is_none() {
            false
        } else {
            match (prepared_mask, &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let band_blob = encode_lerc2_tiled_simple_band(
            EncodeSpec {
                n_masks: usize::from(prepared_mask.is_some()),
                ..band_spec
            },
            prepared_data,
            max_z_error,
            prepared_mask,
            version,
            micro_block_size,
            n_blobs_more,
            encode_mask,
            false,
            prepared_no_data,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = prepared_mask.cloned();
    }

    Ok(blob)
}

/// Encodes band-major data as concatenated raw tiled Lerc2 blobs with no-data metadata.
///
/// This helper carries version 6+ no-data metadata for bands whose
/// `uses_no_data` entry is nonzero. The source data is expected to already
/// contain the per-band no-data sentinel value wherever no-data should be
/// represented. Pixels whose every depth equals the sentinel are marked invalid;
/// mixed-depth sentinel samples are remapped to an internal sentinel when needed
/// and restored to the original sentinel during decode.
pub fn encode_lerc2_tiled_raw_bands_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
    micro_block_size: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "raw tiled Lerc2 band encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "raw tiled Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if !(1..=32).contains(&micro_block_size) {
        return Err(LercError::WrongParam(
            "Lerc2 raw tiled micro block size must be 1 through 32",
        ));
    }
    validate_encode_no_data_inputs(spec, uses_no_data, no_data_values, version)?;

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let no_data = uses_no_data
            .and_then(|uses| uses.get(band).copied())
            .filter(|&uses| uses != 0)
            .and_then(|_| no_data_values.and_then(|values| values.get(band).copied()))
            .map(|value| (value, value));
        let band_data = &data[data_start..data_start + band_data_len];
        let prepared = if let Some((_, no_data_orig)) = no_data {
            Some(prepare_no_data_band_for_encode(
                band_spec,
                band_data,
                mask.as_ref(),
                no_data_orig,
                max_z_error,
            )?)
        } else {
            None
        };
        let prepared_data = prepared
            .as_ref()
            .map(|prepared| prepared.data.as_slice())
            .unwrap_or(band_data);
        let prepared_mask = prepared
            .as_ref()
            .and_then(|prepared| prepared.mask.as_ref())
            .or(mask.as_ref());
        let prepared_no_data = prepared
            .as_ref()
            .and_then(|prepared| prepared.no_data)
            .or_else(|| (spec.n_depth > 1).then_some(no_data).flatten());
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 && prepared.is_none() {
            false
        } else {
            match (prepared_mask, &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let band_blob = encode_lerc2_tiled_raw_band(
            EncodeSpec {
                n_masks: usize::from(prepared_mask.is_some()),
                ..band_spec
            },
            prepared_data,
            max_z_error,
            prepared_mask,
            version,
            micro_block_size,
            n_blobs_more,
            encode_mask,
            prepared_no_data,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = prepared_mask.cloned();
    }

    Ok(blob)
}

fn encode_lerc2_one_sweep_band(
    spec: EncodeSpec,
    data: &[u8],
    mut max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    n_blobs_more: i32,
    encode_partial_mask: bool,
    no_data: Option<(f64, f64)>,
) -> Result<Vec<u8>> {
    validate_single_band_encode_inputs(spec, data, mask, "one-sweep Lerc2 encode")?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "one-sweep Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if no_data.is_some() && version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    if no_data.is_some() && spec.n_depth <= 1 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires depth greater than 1",
        ));
    }

    let mask = effective_encode_mask(spec, mask)?;
    let num_valid_pixel = mask.count_valid_bits();
    max_z_error = normalize_lerc2_max_z_error_for_encode(spec, data, &mask, max_z_error)?;
    let ranges = compute_lerc2_data_ranges_for_encode_with_mask(spec, data, &mask)?;
    let z_min = ranges.mins.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = ranges
        .maxs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let has_valid = num_valid_pixel > 0;
    let header_size = compute_lerc2_header_byte_len(version)?;
    let mut header = HeaderInfo {
        version,
        checksum: 0,
        n_rows: spec.n_rows as i32,
        n_cols: spec.n_cols as i32,
        n_depth: spec.n_depth as i32,
        num_valid_pixel: num_valid_pixel as i32,
        micro_block_size: 8,
        blob_size: 1,
        n_blobs_more,
        b_pass_no_data_values: u8::from(no_data.is_some()),
        b_is_int: u8::from(data_values_are_integer(spec.data_type, data)?),
        b_reserved_3: 0,
        b_reserved_4: 0,
        data_type: spec.data_type,
        max_z_error,
        z_min: if has_valid { z_min } else { 0.0 },
        z_max: if has_valid { z_max } else { 0.0 },
        no_data_val: no_data.map(|values| values.0).unwrap_or(0.0),
        no_data_val_orig: no_data.map(|values| values.1).unwrap_or(0.0),
        header_size,
    };

    let encode_mask = encode_partial_mask && mask.count_valid_bits() < spec.n_cols * spec.n_rows;
    let mask_len = compute_lerc2_mask_byte_len(&header, Some(&mask), encode_mask)?;
    let ranges_len = if version >= 4 && has_valid && header.z_min != header.z_max {
        compute_lerc2_min_max_ranges_byte_len(&header)?
    } else {
        0
    };
    let payload_len = if has_valid && header.z_min != header.z_max && !ranges.min_max_equal {
        compute_lerc2_one_sweep_byte_len(&header)?
    } else {
        0
    };
    header.blob_size = header
        .header_size
        .checked_add(mask_len)
        .and_then(|len| len.checked_add(ranges_len))
        .and_then(|len| len.checked_add(payload_len))
        .and_then(|len| i32::try_from(len).ok())
        .ok_or(LercError::WrongParam("Lerc2 one-sweep blob size overflow"))?;

    let mut blob = vec![0; header.blob_size as usize];
    let mut offset = write_lerc2_header(&header, &mut blob)?;
    offset += write_lerc2_mask(&header, Some(&mask), encode_mask, &mut blob[offset..])?;
    if ranges_len > 0 {
        offset += write_lerc2_min_max_ranges(&header, &ranges, &mut blob[offset..])?;
    }
    if payload_len > 0 {
        offset += write_lerc2_one_sweep(&header, &mask, data, &mut blob[offset..])?;
    }
    debug_assert_eq!(offset, blob.len());
    finalize_lerc2_checksum(&mut blob)?;
    Ok(blob)
}

/// Encodes band-major data as concatenated single-band one-sweep Lerc2 blobs.
///
/// `data` must contain `n_bands` complete bands in band-major order. Masks
/// follow the public C API convention: no masks means all pixels are valid, one
/// mask is shared by all bands, and `n_bands` masks provide one mask per band.
/// Version 6 blobs carry `nBlobsMore`; earlier concatenation relies on the
/// following blob header, matching the legacy C++ behavior. Version 2 and 3
/// blobs are limited to `n_depth == 1` because those headers do not carry a
/// depth field.
pub fn encode_lerc2_one_sweep_bands(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_one_sweep_bands_with_no_data(spec, data, max_z_error, masks, None, None, version)
}

/// Encodes band-major data as concatenated one-sweep Lerc2 blobs with no-data metadata.
///
/// This helper carries version 6+ no-data metadata for bands whose
/// `uses_no_data` entry is nonzero. The source data is expected to already
/// contain the per-band no-data sentinel value wherever no-data should be
/// represented. Pixels whose every depth equals the sentinel are marked invalid;
/// mixed-depth sentinel samples are remapped to an internal sentinel when needed
/// and restored to the original sentinel during decode.
pub fn encode_lerc2_one_sweep_bands_with_no_data(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "one-sweep Lerc2 band encode")?;
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if !(2..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam(
            "one-sweep Lerc2 encode requires version 2 or newer",
        ));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    validate_encode_no_data_inputs(spec, uses_no_data, no_data_values, version)?;

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let no_data = uses_no_data
            .and_then(|uses| uses.get(band).copied())
            .filter(|&uses| uses != 0)
            .and_then(|_| no_data_values.and_then(|values| values.get(band).copied()))
            .map(|value| (value, value));
        let band_data = &data[data_start..data_start + band_data_len];
        let prepared = if let Some((_, no_data_orig)) = no_data {
            Some(prepare_no_data_band_for_encode(
                band_spec,
                band_data,
                mask.as_ref(),
                no_data_orig,
                max_z_error,
            )?)
        } else {
            None
        };
        let prepared_data = prepared
            .as_ref()
            .map(|prepared| prepared.data.as_slice())
            .unwrap_or(band_data);
        let prepared_mask = prepared
            .as_ref()
            .and_then(|prepared| prepared.mask.as_ref())
            .or(mask.as_ref());
        let prepared_no_data = prepared
            .as_ref()
            .and_then(|prepared| prepared.no_data)
            .or_else(|| (spec.n_depth > 1).then_some(no_data).flatten());
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 && prepared.is_none() {
            false
        } else {
            match (prepared_mask, &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let encode_band_spec = EncodeSpec {
            n_masks: usize::from(prepared_mask.is_some()),
            ..band_spec
        };

        let band_blob = encode_lerc2_one_sweep_band(
            encode_band_spec,
            prepared_data,
            max_z_error,
            prepared_mask,
            version,
            n_blobs_more,
            encode_mask,
            prepared_no_data,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = prepared_mask.cloned();
    }

    Ok(blob)
}

/// Encodes a single-band constant Lerc2 blob.
///
/// This is the first narrow encode path: all valid pixels and depths are
/// represented by the same scalar value, so the blob contains only a header and
/// mask section. The returned blob is finalized with a valid checksum for
/// version 3 and newer.
pub fn encode_lerc2_constant(
    spec: EncodeSpec,
    value: f64,
    max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
) -> Result<Vec<u8>> {
    encode_lerc2_constant_band(spec, value, max_z_error, mask, version, 0, true)
}

/// Encodes band-major data as concatenated constant Lerc2 blobs.
///
/// Each band must be constant across all pixels and depths. Masks follow the
/// public C API convention: no masks means all pixels are valid, one mask is
/// shared by all bands, and `n_bands` masks provide one mask per band. Version
/// 6 blobs carry `nBlobsMore`; version 4 and 5 concatenation relies on the
/// following blob header, matching the other band-major encoders.
pub fn encode_lerc2_constant_bands(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_bands_inputs(spec, data, masks, "constant Lerc2 band encode")?;

    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    let mask_len = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam(
            "Lerc2 encode mask byte count overflow",
        ))?;
    let mut blob = Vec::new();
    let mut previous_mask: Option<BitMask> = None;

    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let band_data = &data[data_start..data_start + band_data_len];
        let value = constant_value_for_uncompressed_encode(spec.data_type, band_data)?.ok_or(
            LercError::WrongParam("constant Lerc2 band encode requires constant data"),
        )?;
        let mask = match masks {
            Some(mask_bytes) if spec.n_masks == 1 => Some(BitMask::from_byte_mask(
                &mask_bytes[..mask_len],
                spec.n_cols,
                spec.n_rows,
            )?),
            Some(mask_bytes) => {
                let mask_start = band
                    .checked_mul(mask_len)
                    .ok_or(LercError::WrongParam("Lerc2 encode mask offset overflow"))?;
                Some(BitMask::from_byte_mask(
                    &mask_bytes[mask_start..mask_start + mask_len],
                    spec.n_cols,
                    spec.n_rows,
                )?)
            }
            None => None,
        };
        let encode_mask = if band == 0 {
            true
        } else if spec.n_masks == 1 {
            false
        } else {
            match (mask.as_ref(), &previous_mask) {
                (Some(mask), Some(previous)) => mask != previous,
                (Some(_), None) => true,
                _ => false,
            }
        };
        let n_blobs_more = if version >= 6 {
            i32::try_from(spec.n_bands - 1 - band)
                .map_err(|_| LercError::WrongParam("Lerc2 band count overflow"))?
        } else {
            0
        };
        let band_blob = encode_lerc2_constant_band(
            EncodeSpec {
                n_masks: usize::from(mask.is_some()),
                ..band_spec
            },
            value,
            max_z_error,
            mask.as_ref(),
            version,
            n_blobs_more,
            encode_mask,
        )?;
        blob.extend_from_slice(&band_blob);
        previous_mask = mask;
    }

    Ok(blob)
}

fn encode_lerc2_constant_band(
    spec: EncodeSpec,
    value: f64,
    mut max_z_error: f64,
    mask: Option<&BitMask>,
    version: i32,
    n_blobs_more: i32,
    encode_mask: bool,
) -> Result<Vec<u8>> {
    spec.validate()?;
    if spec.n_bands != 1 {
        return Err(LercError::WrongParam(
            "constant Lerc2 encode currently supports one band",
        ));
    }
    if spec.n_masks > 1 {
        return Err(LercError::WrongParam(
            "constant Lerc2 encode mask count must be 0 or 1",
        ));
    }
    max_z_error = normalize_constant_lerc2_max_z_error_for_encode(spec.data_type, max_z_error)?;
    if !(0..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::WrongParam("unsupported Lerc2 encode version"));
    }
    if version < 4 && spec.n_depth != 1 {
        return Err(LercError::WrongParam(
            "pre-v4 Lerc2 encode can only store depth 1",
        ));
    }
    if mask.is_some() && spec.n_masks == 0 {
        return Err(LercError::WrongParam(
            "constant Lerc2 encode mask count must be nonzero when a mask is supplied",
        ));
    }

    let total_pixels = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam("Lerc2 encode pixel count overflow"))?;
    let num_valid_pixel = match mask {
        Some(mask) => {
            if mask.cols() != spec.n_cols || mask.rows() != spec.n_rows {
                return Err(LercError::WrongParam(
                    "Lerc2 encode mask dimensions do not match spec",
                ));
            }
            mask.count_valid_bits()
        }
        None => total_pixels,
    };

    let mut header = HeaderInfo {
        version,
        checksum: 0,
        n_rows: spec.n_rows as i32,
        n_cols: spec.n_cols as i32,
        n_depth: spec.n_depth as i32,
        num_valid_pixel: num_valid_pixel as i32,
        micro_block_size: 8,
        blob_size: 1,
        n_blobs_more,
        b_pass_no_data_values: 0,
        b_is_int: u8::from(value.fract() == 0.0),
        b_reserved_3: 0,
        b_reserved_4: 0,
        data_type: spec.data_type,
        max_z_error,
        z_min: value,
        z_max: value,
        no_data_val: 0.0,
        no_data_val_orig: 0.0,
        header_size: compute_lerc2_header_byte_len(version)?,
    };

    let mask_len = compute_lerc2_mask_byte_len(&header, mask, encode_mask)?;
    header.blob_size = header
        .header_size
        .checked_add(mask_len)
        .and_then(|len| i32::try_from(len).ok())
        .ok_or(LercError::WrongParam("Lerc2 constant blob size overflow"))?;

    let mut blob = vec![0; header.blob_size as usize];
    let header_len = write_lerc2_header(&header, &mut blob)?;
    write_lerc2_mask(&header, mask, encode_mask, &mut blob[header_len..])?;
    finalize_lerc2_checksum(&mut blob)?;
    Ok(blob)
}

/// Reads and decodes the Lerc2 mask section.
pub fn read_lerc2_mask(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo)> {
    read_lerc2_mask_with_previous(blob, None)
}

/// Reads and decodes the Lerc2 mask section, allowing omitted masks to reuse `previous_mask`.
pub fn read_lerc2_mask_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo)> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask = read_mask(&mut reader, &header, previous_mask)?;
    Ok((header, mask))
}

/// Computes the Fletcher32 checksum used by Lerc2 version 3 and newer.
pub fn compute_checksum_fletcher32(bytes: &[u8]) -> u32 {
    let mut sum1 = 0xffffu32;
    let mut sum2 = 0xffffu32;
    let mut pos = 0usize;
    let mut words = bytes.len() / 2;

    while words > 0 {
        let tlen = words.min(359);
        words -= tlen;

        for _ in 0..tlen {
            sum1 += (bytes[pos] as u32) << 8;
            pos += 1;
            sum1 += bytes[pos] as u32;
            pos += 1;
            sum2 += sum1;
        }

        sum1 = (sum1 & 0xffff) + (sum1 >> 16);
        sum2 = (sum2 & 0xffff) + (sum2 >> 16);
    }

    if bytes.len() & 1 != 0 {
        sum1 += (bytes[pos] as u32) << 8;
        sum2 += sum1;
    }

    sum1 = (sum1 & 0xffff) + (sum1 >> 16);
    sum2 = (sum2 & 0xffff) + (sum2 >> 16);
    (sum2 << 16) | sum1
}

/// Validates the stored Lerc2 checksum and returns the parsed header.
pub fn validate_lerc2_checksum(blob: &[u8]) -> Result<HeaderInfo> {
    let probe = get_lerc2_header_info(blob)?;
    let header = probe.header;

    if header.blob_size as usize > blob.len() {
        return Err(LercError::BufferTooSmall);
    }
    if header.version < 3 {
        return Ok(header);
    }
    if header.blob_size as usize <= CHECKSUM_START_OFFSET {
        return Err(LercError::CorruptInput("Lerc2 blob too small for checksum"));
    }

    let checksum =
        compute_checksum_fletcher32(&blob[CHECKSUM_START_OFFSET..header.blob_size as usize]);
    if checksum != header.checksum {
        return Err(LercError::CorruptInput("Lerc2 checksum mismatch"));
    }

    Ok(header)
}

/// Computes and writes the Lerc2 checksum into a complete blob in place.
///
/// Version 0 through 2 blobs do not carry a checksum and are left unchanged.
/// For version 3 and newer blobs, the Fletcher32 checksum is computed over the
/// byte range after the checksum field through `header.blob_size`, matching the
/// C++ encoder finalization step.
pub fn finalize_lerc2_checksum(blob: &mut [u8]) -> Result<u32> {
    let header = get_lerc2_header_info(blob)?.header;
    let blob_size = header.blob_size as usize;
    if blob_size > blob.len() {
        return Err(LercError::BufferTooSmall);
    }
    if header.version < 3 {
        return Ok(0);
    }
    if blob_size <= CHECKSUM_START_OFFSET {
        return Err(LercError::CorruptInput("Lerc2 blob too small for checksum"));
    }

    let checksum = compute_checksum_fletcher32(&blob[CHECKSUM_START_OFFSET..blob_size]);
    blob[FILE_KEY.len() + 4..CHECKSUM_START_OFFSET].copy_from_slice(&checksum.to_le_bytes());
    Ok(checksum)
}

/// Reads the version 4+ min/max range section.
pub fn read_lerc2_min_max_ranges(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, MinMaxRanges)> {
    read_lerc2_min_max_ranges_with_previous(blob, None)
}

/// Reads the min/max range section, allowing omitted masks to reuse `previous_mask`.
pub fn read_lerc2_min_max_ranges_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo, MinMaxRanges)> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask = read_mask(&mut reader, &header, previous_mask)?;

    if header.version < 4 {
        return Err(LercError::Unsupported(
            "Lerc2 min/max range section is only present for version 4+ blobs",
        ));
    }

    let ranges = if header.num_valid_pixel == 0 {
        MinMaxRanges {
            mins: vec![0.0; header.n_depth as usize],
            maxs: vec![0.0; header.n_depth as usize],
            bytes_consumed: reader.pos,
            min_max_equal: true,
        }
    } else if header.z_min == header.z_max {
        MinMaxRanges {
            mins: vec![header.z_min; header.n_depth as usize],
            maxs: vec![header.z_max; header.n_depth as usize],
            bytes_consumed: reader.pos,
            min_max_equal: true,
        }
    } else {
        read_min_max_ranges(&mut reader, &header)?
    };

    Ok((header, mask, ranges))
}

/// Aggregates data ranges across one or more concatenated Lerc2 bands.
///
/// Legacy Lerc1 blobs fall back to the Lerc1 z-stat reader and report a
/// single-band, single-depth range.
pub fn get_lerc2_data_ranges(blob: &[u8]) -> Result<DataRanges> {
    if get_lerc2_header_info(blob).is_err() {
        return get_lerc1_data_ranges(blob);
    }

    let mut offset = 0usize;
    let mut previous_mask: Option<BitMask> = None;
    let mut first_header: Option<HeaderInfo> = None;
    let mut mins = Vec::new();
    let mut maxs = Vec::new();
    let mut n_bands = 0usize;

    loop {
        let (header, mask, ranges) = read_lerc2_data_ranges_blob(
            &blob[offset..],
            first_header.as_ref(),
            previous_mask.as_ref(),
        )?;
        let blob_size = header.blob_size as usize;
        if blob_size == 0 || blob_size > blob.len().saturating_sub(offset) {
            return Err(LercError::BufferTooSmall);
        }

        if first_header.is_none() {
            first_header = Some(header.clone());
        }

        mins.extend_from_slice(&ranges.mins);
        maxs.extend_from_slice(&ranges.maxs);
        n_bands += 1;

        let has_more = header.version <= 5 || header.n_blobs_more > 0;
        offset += blob_size;
        previous_mask = Some(mask.mask);

        if !has_more || offset >= blob.len() {
            break;
        }
    }

    let n_depth = first_header
        .as_ref()
        .map(|header| header.n_depth as usize)
        .unwrap_or(0);
    Ok(DataRanges {
        mins,
        maxs,
        n_bands,
        n_depth,
        bytes_consumed: offset,
    })
}

fn get_lerc1_data_ranges(blob: &[u8]) -> Result<DataRanges> {
    let decoded = decode_lerc1_bands(blob)?;
    Ok(DataRanges {
        mins: decoded
            .stats
            .iter()
            .map(|stats| stats.z_min as f64)
            .collect(),
        maxs: decoded
            .stats
            .iter()
            .map(|stats| stats.z_max as f64)
            .collect(),
        n_bands: decoded.n_bands,
        n_depth: 1,
        bytes_consumed: decoded.bytes_consumed,
    })
}

/// Reads per-band Lerc2 no-data metadata without decoding pixel values.
///
/// The returned arrays match the public 4D C API output shape: one flag and
/// one original no-data sentinel per requested band.
pub fn get_lerc2_no_data_info(blob: &[u8], n_bands: usize) -> Result<NoDataInfo> {
    if n_bands == 0 {
        return Err(LercError::WrongParam("band count must be positive"));
    }

    let mut uses_no_data = vec![0u8; n_bands];
    let mut no_data_values = vec![0.0f64; n_bands];
    let mut offset = 0usize;
    for i_band in 0..n_bands {
        let header = get_lerc2_header_info(&blob[offset..])?.header;
        uses_no_data[i_band] = u8::from(header.has_no_data_values());
        no_data_values[i_band] = header.no_data_val_orig;
        let blob_size = header.blob_size as usize;
        if blob_size == 0 || blob_size > blob.len().saturating_sub(offset) {
            return Err(LercError::BufferTooSmall);
        }
        offset += blob_size;
    }

    Ok(NoDataInfo {
        uses_no_data,
        no_data_values,
        bytes_consumed: offset,
    })
}

/// Reads a one-sweep raw Lerc2 payload.
pub fn read_lerc2_data_one_sweep(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, DataOneSweep)> {
    read_lerc2_data_one_sweep_with_previous(blob, None)
}

/// Reads a one-sweep payload, allowing omitted masks to reuse `previous_mask`.
pub fn read_lerc2_data_one_sweep_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo, DataOneSweep)> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask = read_mask(&mut reader, &header, previous_mask)?;

    if header.num_valid_pixel == 0 || header.z_min == header.z_max {
        return Err(LercError::Unsupported(
            "Lerc2 const or empty blobs do not carry one-sweep payloads",
        ));
    }

    if header.version >= 4 {
        let ranges = read_min_max_ranges(&mut reader, &header)?;
        if ranges.min_max_equal {
            return Err(LercError::Unsupported(
                "Lerc2 all-constant min/max ranges do not carry one-sweep payloads",
            ));
        }
    }

    let flag = reader.read_bytes(1)?[0];
    if flag != 1 {
        return Err(LercError::Unsupported(
            "Lerc2 blob is not encoded one-sweep",
        ));
    }

    let data = read_data_one_sweep(&mut reader, &header, &mask.mask)?;
    Ok((header, mask, data))
}

/// Reads a supported tiled Lerc2 payload.
///
/// This currently delegates to [`read_lerc2_tiled_payload`].
pub fn read_lerc2_tiled_raw(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    read_lerc2_tiled_raw_with_previous(blob, None)
}

/// Reads a supported tiled Lerc2 payload.
pub fn read_lerc2_tiled_payload(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    read_lerc2_tiled_payload_with_previous(blob, None)
}

/// Reads a supported tiled payload, allowing omitted masks to reuse `previous_mask`.
///
/// This currently delegates to [`read_lerc2_tiled_payload_with_previous`].
pub fn read_lerc2_tiled_raw_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    read_lerc2_tiled_payload_with_previous(blob, previous_mask)
}

/// Reads a supported tiled payload, allowing omitted masks to reuse `previous_mask`.
pub fn read_lerc2_tiled_payload_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask = read_mask(&mut reader, &header, previous_mask)?;

    if header.num_valid_pixel == 0 || header.z_min == header.z_max {
        return Err(LercError::Unsupported(
            "Lerc2 const or empty blobs do not carry tiled payloads",
        ));
    }

    let ranges = if header.version >= 4 {
        let ranges = read_min_max_ranges(&mut reader, &header)?;
        if ranges.min_max_equal {
            return Err(LercError::Unsupported(
                "Lerc2 all-constant min/max ranges do not carry tiled payloads",
            ));
        }
        Some(ranges)
    } else {
        None
    };

    let one_sweep_flag = reader.read_bytes(1)?[0];
    if one_sweep_flag != 0 {
        return Err(LercError::Unsupported(
            "Lerc2 blob is not encoded with tiled payloads",
        ));
    }

    if try_huffman_int(&header) || try_huffman_float(&header) {
        let payload_start = reader.pos;
        let image_mode = reader.read_bytes(1)?[0];
        let valid_image_mode = image_mode <= 3
            && (image_mode <= 2 || header.version >= 6)
            && (image_mode <= 1 || header.version >= 4);
        if valid_image_mode && image_mode != 0 {
            reader.pos = payload_start;
            if let Ok(data) = read_tiled_payload(&mut reader, &header, &mask.mask, ranges.as_ref())
            {
                return Ok((header, mask, data));
            }
            return Err(LercError::Unsupported(
                "Lerc2 blob is not encoded with tiled payloads",
            ));
        }
        if valid_image_mode {
            match read_tiled_payload(&mut reader, &header, &mask.mask, ranges.as_ref()) {
                Ok(data) => return Ok((header, mask, data)),
                Err(_) => reader.pos = payload_start,
            }
        } else {
            reader.pos = payload_start;
        }
    }

    let data = read_tiled_payload(&mut reader, &header, &mask.mask, ranges.as_ref())?;
    Ok((header, mask, data))
}

/// Decodes a single Lerc2 blob using the currently supported non-Huffman subset.
pub fn decode_lerc2_supported(blob: &[u8]) -> Result<DecodedLerc2> {
    decode_lerc2_supported_with_previous(blob, None)
}

/// Decodes concatenated Lerc2 blobs using the currently supported subset.
pub fn decode_lerc2_bands_supported(blob: &[u8]) -> Result<DecodedLerc2Bands> {
    let mut bands = Vec::new();
    let mut offset = 0usize;
    let mut previous_mask: Option<BitMask> = None;
    let mut first_header: Option<HeaderInfo> = None;

    loop {
        let decoded =
            decode_lerc2_supported_with_previous(&blob[offset..], previous_mask.as_ref())?;
        let blob_size = decoded.header.blob_size as usize;
        if blob_size == 0 || blob_size > blob.len().saturating_sub(offset) {
            return Err(LercError::BufferTooSmall);
        }

        if let Some(first) = &first_header {
            if decoded.header.n_depth != first.n_depth
                || decoded.header.n_cols != first.n_cols
                || decoded.header.n_rows != first.n_rows
                || decoded.header.data_type != first.data_type
            {
                return Err(LercError::CorruptInput(
                    "concatenated Lerc2 header mismatch",
                ));
            }
        } else {
            first_header = Some(decoded.header.clone());
        }

        let has_more = decoded.header.version <= 5 || decoded.header.n_blobs_more > 0;
        offset += blob_size;
        previous_mask = Some(decoded.mask.clone());
        bands.push(decoded);

        if !has_more || offset >= blob.len() {
            break;
        }
    }

    Ok(DecodedLerc2Bands {
        bands,
        bytes_consumed: offset,
    })
}

/// Decodes supported Lerc2 data directly into caller-provided byte buffers.
///
/// Data is written in band-major order as little-endian scalar bytes. Masks are
/// written as one byte per pixel when requested by [`DecodeIntoSpec::n_masks`].
pub fn decode_lerc2_supported_into(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data_output: &mut [u8],
    mask_output: Option<&mut [u8]>,
) -> Result<DecodeIntoResult> {
    validate_decode_into_spec(spec, mask_output.is_some())?;
    let decoded = decode_lerc2_bands_supported(blob)?;
    if spec.n_bands > decoded.bands.len() {
        return Err(LercError::WrongParam(
            "requested more bands than the Lerc2 blob contains",
        ));
    }

    let selected_bands = &decoded.bands[..spec.n_bands];
    for band in selected_bands {
        if band.header.data_type != spec.data_type
            || band.header.n_depth as usize != spec.n_depth
            || band.header.n_cols as usize != spec.n_cols
            || band.header.n_rows as usize != spec.n_rows
        {
            return Err(LercError::WrongParam(
                "decode output shape does not match the Lerc2 blob",
            ));
        }
    }

    let required_masks = required_mask_count(selected_bands);
    if spec.n_masks < required_masks {
        return Err(LercError::WrongParam(
            "caller did not provide enough mask buffers for the Lerc2 blob",
        ));
    }

    let data_bytes_needed = spec.data_byte_len()?;
    if data_output.len() < data_bytes_needed {
        return Err(LercError::BufferTooSmall);
    }

    let mut data_offset = 0usize;
    for band in selected_bands {
        data_offset += band.write_data_le_bytes(&mut data_output[data_offset..])?;
    }

    let mask_bytes_written = match (spec.n_masks, mask_output) {
        (0, _) => 0,
        (1, Some(output)) => selected_bands[0].write_mask_bytes(output)?,
        (_, Some(output)) => {
            let mask_bytes_needed = spec.mask_byte_len()?;
            if output.len() < mask_bytes_needed {
                return Err(LercError::BufferTooSmall);
            }

            let mut mask_offset = 0usize;
            for band in selected_bands {
                mask_offset += band.write_mask_bytes(&mut output[mask_offset..])?;
            }
            mask_offset
        }
        (_, None) => unreachable!("mask output presence is validated before decode"),
    };

    Ok(DecodeIntoResult {
        bytes_consumed: decoded.bytes_consumed,
        data_bytes_written: data_offset,
        mask_bytes_written,
    })
}

/// Decodes supported LERC data directly into caller-provided byte buffers.
///
/// This is the format-agnostic safe helper used by the C ABI decode wrappers.
/// It dispatches legacy Lerc1 `CntZImage` blobs to the Lerc1 float decoder and
/// all other blobs to [`decode_lerc2_supported_into`].
pub fn decode_lerc_supported_into(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data_output: &mut [u8],
    mask_output: Option<&mut [u8]>,
) -> Result<DecodeIntoResult> {
    validate_decode_into_spec(spec, mask_output.is_some())?;
    if blob.starts_with(CNT_Z_IMAGE_KEY) {
        decode_lerc1_supported_into(blob, spec, data_output, mask_output)
    } else {
        decode_lerc2_supported_into(blob, spec, data_output, mask_output)
    }
}

/// Decodes supported LERC data into caller-provided `f64` output.
///
/// The blob is decoded through its native scalar type first, then converted to
/// `f64`, matching the public C API `lerc_decodeToDouble` behavior. Masks are
/// written as one byte per pixel when requested by [`DecodeIntoSpec::n_masks`].
pub fn decode_lerc_supported_to_f64(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data_output: &mut [f64],
    mask_output: Option<&mut [u8]>,
) -> Result<DecodeToF64Result> {
    let mut native_data = vec![0u8; spec.data_byte_len()?];
    let decoded = decode_lerc_supported_into(blob, spec, &mut native_data, mask_output).and_then(
        |result| {
            decode_typed_values(spec.data_type, &native_data[..result.data_bytes_written])
                .map(|decoded| (result, decoded))
        },
    )?;

    let values_written = decoded.1.write_f64_values(data_output)?;
    Ok(DecodeToF64Result {
        bytes_consumed: decoded.0.bytes_consumed,
        values_written,
        mask_bytes_written: decoded.0.mask_bytes_written,
    })
}

fn decode_lerc1_supported_into(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data_output: &mut [u8],
    mask_output: Option<&mut [u8]>,
) -> Result<DecodeIntoResult> {
    let decoded = decode_lerc1_bands(blob)?;
    if spec.data_type != DataType::Float
        || spec.n_depth != 1
        || spec.n_bands != decoded.n_bands
        || spec.n_cols != decoded.header.n_cols as usize
        || spec.n_rows != decoded.header.n_rows as usize
        || !(spec.n_masks == 0 || spec.n_masks == 1 || spec.n_masks == decoded.n_bands)
    {
        return Err(LercError::WrongParam("Lerc1 decode shape/type mismatch"));
    }

    let data_bytes_written = DecodedData::Float(decoded.values).write_le_bytes(data_output)?;
    let mask_bytes_written = if let Some(mask_output) = mask_output {
        let byte_mask = decoded.mask_info.mask.to_byte_mask();
        let required_len = byte_mask
            .len()
            .checked_mul(spec.n_masks)
            .ok_or(LercError::WrongParam("Lerc1 mask output size overflow"))?;
        if mask_output.len() < required_len {
            return Err(LercError::BufferTooSmall);
        }
        for band in 0..spec.n_masks {
            let start = band * byte_mask.len();
            mask_output[start..start + byte_mask.len()].copy_from_slice(&byte_mask);
        }
        required_len
    } else {
        0
    };

    Ok(DecodeIntoResult {
        bytes_consumed: decoded.bytes_consumed,
        data_bytes_written,
        mask_bytes_written,
    })
}

/// Decodes one Lerc2 blob, allowing omitted masks to reuse `previous_mask`.
pub fn decode_lerc2_supported_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<DecodedLerc2> {
    validate_lerc2_checksum(blob)?;

    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask_info = read_mask(&mut reader, &header, previous_mask)?;

    let value_count = (header.n_cols as usize)
        .checked_mul(header.n_rows as usize)
        .and_then(|count| count.checked_mul(header.n_depth as usize))
        .ok_or(LercError::CorruptInput(
            "Lerc2 decoded value count overflow",
        ))?;

    if header.num_valid_pixel == 0 {
        let data = decode_lerc2_typed_values(
            &header,
            &mask_info.mask,
            &vec![0; value_count * header.data_type.size_in_bytes()],
        )?;
        return Ok(DecodedLerc2 {
            header: header.clone(),
            mask: mask_info.mask,
            ranges: None,
            data,
            bytes_consumed: reader.pos,
        });
    }

    if header.z_min == header.z_max {
        let raw = const_image_bytes(&header, &mask_info.mask, None)?;
        let data = decode_lerc2_typed_values(&header, &mask_info.mask, &raw)?;
        return Ok(DecodedLerc2 {
            header: header.clone(),
            mask: mask_info.mask,
            ranges: None,
            data,
            bytes_consumed: reader.pos,
        });
    }

    let ranges = if header.version >= 4 {
        let ranges = read_min_max_ranges(&mut reader, &header)?;
        if ranges.min_max_equal {
            let raw = const_image_bytes(&header, &mask_info.mask, Some(&ranges))?;
            let data = decode_lerc2_typed_values(&header, &mask_info.mask, &raw)?;
            return Ok(DecodedLerc2 {
                header: header.clone(),
                mask: mask_info.mask,
                ranges: Some(ranges),
                data,
                bytes_consumed: reader.pos,
            });
        }
        Some(ranges)
    } else {
        None
    };

    let one_sweep_flag = reader.read_bytes(1)?[0];
    let raw = if one_sweep_flag != 0 {
        read_data_one_sweep(&mut reader, &header, &mask_info.mask)?.data
    } else {
        if try_huffman_int(&header) || try_huffman_float(&header) {
            let image_mode = reader.read_bytes(1)?[0];
            if image_mode > 3
                || (image_mode > 2 && header.version < 6)
                || (image_mode > 1 && header.version < 4)
            {
                return Err(LercError::CorruptInput("invalid Lerc2 image encode mode"));
            }
            if image_mode != 0 {
                if try_huffman_int(&header) {
                    read_huffman_int_payload(&mut reader, &header, &mask_info.mask, image_mode)?
                } else if try_huffman_float(&header) && image_mode == 3 {
                    read_huffman_float_payload(&mut reader, &header)?
                } else {
                    return Err(LercError::Unsupported(
                        "Lerc2 floating-point Huffman image modes are not ported yet",
                    ));
                }
            } else {
                read_tiled_payload(&mut reader, &header, &mask_info.mask, ranges.as_ref())?.data
            }
        } else {
            read_tiled_payload(&mut reader, &header, &mask_info.mask, ranges.as_ref())?.data
        }
    };
    let data = decode_lerc2_typed_values(&header, &mask_info.mask, &raw)?;

    Ok(DecodedLerc2 {
        header: header.clone(),
        mask: mask_info.mask,
        ranges,
        data,
        bytes_consumed: reader.pos,
    })
}

/// Aggregates public LERC metadata.
///
/// Lerc2 blobs are aggregated across concatenated bands. Legacy Lerc1 blobs
/// currently report single-band metadata from the decoded count mask and z
/// statistics.
pub fn get_lerc_info(blob: &[u8]) -> Result<LercInfo> {
    let first = match get_lerc2_header_info(blob) {
        Ok(first) => first,
        Err(_) => return get_lerc1_info(blob),
    };
    let mut info = LercInfo {
        version: first.header.version,
        n_depth: first.header.n_depth,
        n_cols: first.header.n_cols,
        n_rows: first.header.n_rows,
        num_valid_pixel: first.header.num_valid_pixel,
        n_bands: 1,
        blob_size: first.header.blob_size,
        n_masks: if first.has_mask || first.header.num_valid_pixel == 0 {
            1
        } else {
            0
        },
        n_uses_no_data_value: if first.header.has_no_data_values() {
            1
        } else {
            0
        },
        data_type: first.header.data_type,
        z_min: first.header.z_min,
        z_max: first.header.z_max,
        max_z_error: first.header.max_z_error,
    };

    if info.blob_size < 0 || info.blob_size as usize > blob.len() {
        return Err(LercError::BufferTooSmall);
    }

    let mut try_next_blob = first.header.version <= 5 || first.header.n_blobs_more > 0;
    while try_next_blob {
        let offset = info.blob_size as usize;
        if offset >= blob.len() {
            break;
        }

        let probe = match get_lerc2_header_info(&blob[offset..]) {
            Ok(probe) => probe,
            Err(_) => break,
        };
        let hd = probe.header;

        if hd.n_depth != info.n_depth
            || hd.n_cols != info.n_cols
            || hd.n_rows != info.n_rows
            || hd.data_type != info.data_type
        {
            return Err(LercError::CorruptInput(
                "concatenated Lerc2 header mismatch",
            ));
        }

        try_next_blob = hd.version <= 5 || hd.n_blobs_more > 0;

        if hd.has_no_data_values() {
            info.n_uses_no_data_value += 1;
        }

        if probe.has_mask || hd.num_valid_pixel != info.num_valid_pixel {
            info.n_masks = 2;
        }

        let next_blob_size = info
            .blob_size
            .checked_add(hd.blob_size)
            .ok_or(LercError::CorruptInput("combined Lerc2 blob size overflow"))?;
        if next_blob_size as usize > blob.len() {
            return Err(LercError::BufferTooSmall);
        }

        info.z_min = info.z_min.min(hd.z_min);
        info.z_max = info.z_max.max(hd.z_max);
        info.max_z_error = info.max_z_error.max(hd.max_z_error);
        info.blob_size = next_blob_size;
        info.n_bands += 1;
    }

    if info.n_masks > 1 {
        info.n_masks = info.n_bands;
    }
    if info.n_uses_no_data_value > 0 {
        info.n_uses_no_data_value = info.n_bands;
    }

    Ok(info)
}

fn get_lerc1_info(blob: &[u8]) -> Result<LercInfo> {
    let decoded = decode_lerc1_bands(blob)?;
    let z_min = decoded
        .stats
        .iter()
        .map(|stats| stats.z_min)
        .fold(f32::INFINITY, f32::min);
    let z_max = decoded
        .stats
        .iter()
        .map(|stats| stats.z_max)
        .fold(f32::NEG_INFINITY, f32::max);
    Ok(LercInfo {
        version: 0,
        n_depth: 1,
        n_cols: decoded.header.n_cols,
        n_rows: decoded.header.n_rows,
        num_valid_pixel: decoded
            .stats
            .first()
            .map(|stats| stats.num_valid_pixels as i32)
            .unwrap_or(0),
        n_bands: decoded.n_bands as i32,
        blob_size: decoded.bytes_consumed as i32,
        n_masks: if decoded.mask_info.all_valid { 0 } else { 1 },
        n_uses_no_data_value: 0,
        data_type: DataType::Float,
        z_min: z_min as f64,
        z_max: z_max as f64,
        max_z_error: decoded.header.max_z_error,
    })
}

/// Fills C API-style blob info and data-range arrays.
///
/// `info_array`, when provided, is zeroed and then filled up to its length with
/// `{ version, dataType, nDepth, nCols, nRows, nBands, nValidPixels, blobSize,
/// nMasks, nDepth, nUsesNoDataValue }`.
///
/// `data_range_array`, when provided, is zeroed and then filled up to its length
/// with `{ zMin, zMax, maxZErrorUsed }`. For multi-depth blobs with no-data
/// sentinels, `zMin` and `zMax` are reported as `-1.0`, matching the public C
/// API behavior.
pub fn get_lerc2_blob_info_arrays(
    blob: &[u8],
    info_array: Option<&mut [u32]>,
    data_range_array: Option<&mut [f64]>,
) -> Result<LercInfo> {
    let info = get_lerc_info(blob)?;

    if let Some(info_array) = info_array {
        info_array.fill(0);
        let values = [
            info.version as u32,
            info.data_type as u32,
            info.n_depth as u32,
            info.n_cols as u32,
            info.n_rows as u32,
            info.n_bands as u32,
            info.num_valid_pixel as u32,
            info.blob_size as u32,
            info.n_masks as u32,
            info.n_depth as u32,
            info.n_uses_no_data_value as u32,
        ];
        let len = info_array.len().min(values.len());
        info_array[..len].copy_from_slice(&values[..len]);
    }

    if let Some(data_range_array) = data_range_array {
        data_range_array.fill(0.0);
        let uses_no_data = info.n_depth > 1 && info.n_uses_no_data_value > 0;
        let values = [
            if uses_no_data { -1.0 } else { info.z_min },
            if uses_no_data { -1.0 } else { info.z_max },
            info.max_z_error,
        ];
        let len = data_range_array.len().min(values.len());
        data_range_array[..len].copy_from_slice(&values[..len]);
    }

    Ok(info)
}

fn read_mask(
    reader: &mut Reader<'_>,
    header: &HeaderInfo,
    previous_mask: Option<&BitMask>,
) -> Result<MaskInfo> {
    let num_valid = header.num_valid_pixel;
    let width = header.n_cols;
    let height = header.n_rows;
    let total = width
        .checked_mul(height)
        .ok_or(LercError::CorruptInput("Lerc2 mask pixel count overflow"))?;

    let num_bytes_mask = reader.read_i32_le()?;
    if num_bytes_mask < 0 {
        return Err(LercError::CorruptInput("negative Lerc2 mask byte count"));
    }

    if (num_valid == 0 || num_valid == total) && num_bytes_mask != 0 {
        return Err(LercError::CorruptInput(
            "Lerc2 mask bytes present for all-valid or all-invalid image",
        ));
    }

    let mut mask = BitMask::new(width as usize, height as usize)?;
    if num_valid == 0 {
        mask.set_all_invalid();
    } else if num_valid == total {
        mask.set_all_valid();
    } else if num_bytes_mask > 0 {
        let mask_bytes = reader.read_bytes(num_bytes_mask as usize)?;
        Rle::decompress_into(mask_bytes, mask.bits_mut())?;
    } else if let Some(previous) = previous_mask {
        if previous.cols() != width as usize || previous.rows() != height as usize {
            return Err(LercError::WrongParam(
                "previous Lerc2 mask dimensions do not match header",
            ));
        }
        mask = previous.clone();
    } else {
        return Err(LercError::Unsupported(
            "Lerc2 partial mask omitted; previous-mask reuse is not supported yet",
        ));
    }

    Ok(MaskInfo {
        mask,
        num_bytes_mask,
        bytes_consumed: reader.pos,
    })
}

fn validate_lerc2_mask_for_write(
    header: &HeaderInfo,
    mask: Option<&BitMask>,
    encode_mask: bool,
) -> Result<bool> {
    let total = header
        .n_cols
        .checked_mul(header.n_rows)
        .ok_or(LercError::WrongParam("Lerc2 mask pixel count overflow"))?;
    if header.num_valid_pixel < 0 || header.num_valid_pixel > total {
        return Err(LercError::WrongParam("invalid Lerc2 valid pixel count"));
    }

    let need_mask = header.num_valid_pixel > 0 && header.num_valid_pixel < total;
    if !need_mask {
        return Ok(false);
    }

    if !encode_mask {
        return Ok(true);
    }

    let mask = mask.ok_or(LercError::WrongParam(
        "partial Lerc2 mask is required when encode_mask is true",
    ))?;
    if mask.cols() != header.n_cols as usize || mask.rows() != header.n_rows as usize {
        return Err(LercError::WrongParam(
            "Lerc2 mask dimensions do not match header",
        ));
    }
    if mask.count_valid_bits() != header.num_valid_pixel as usize {
        return Err(LercError::WrongParam(
            "Lerc2 mask valid count does not match header",
        ));
    }

    Ok(true)
}

fn validate_lerc2_min_max_ranges_for_write(
    header: &HeaderInfo,
    ranges: &MinMaxRanges,
) -> Result<()> {
    let n_depth = header.n_depth as usize;
    if ranges.mins.len() != n_depth || ranges.maxs.len() != n_depth {
        return Err(LercError::WrongParam(
            "Lerc2 min/max range count does not match depth",
        ));
    }

    Ok(())
}

fn validate_lerc2_one_sweep_for_write(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
) -> Result<()> {
    if header.num_valid_pixel < 0 {
        return Err(LercError::WrongParam("invalid Lerc2 valid pixel count"));
    }
    if mask.cols() != header.n_cols as usize || mask.rows() != header.n_rows as usize {
        return Err(LercError::WrongParam(
            "Lerc2 one-sweep mask dimensions do not match header",
        ));
    }
    if mask.count_valid_bits() != header.num_valid_pixel as usize {
        return Err(LercError::WrongParam(
            "Lerc2 one-sweep mask valid count does not match header",
        ));
    }

    let expected_len = (header.n_cols as usize)
        .checked_mul(header.n_rows as usize)
        .and_then(|count| count.checked_mul(header.n_depth as usize))
        .and_then(|count| count.checked_mul(header.data_type.size_in_bytes()))
        .ok_or(LercError::WrongParam(
            "Lerc2 one-sweep data byte count overflow",
        ))?;
    if data.len() != expected_len {
        return Err(LercError::WrongParam(
            "Lerc2 one-sweep data length does not match header",
        ));
    }

    Ok(())
}

fn validate_lerc2_tiled_raw_for_write(
    header: &HeaderInfo,
    mask: &BitMask,
    data: Option<&[u8]>,
) -> Result<()> {
    if header.num_valid_pixel < 0 {
        return Err(LercError::WrongParam("invalid Lerc2 valid pixel count"));
    }
    if header.n_depth <= 0 || header.n_cols <= 0 || header.n_rows <= 0 {
        return Err(LercError::WrongParam(
            "Lerc2 raw tiled dimensions must be positive",
        ));
    }
    if header.micro_block_size <= 0 || header.micro_block_size > 32 {
        return Err(LercError::WrongParam(
            "Lerc2 raw tiled micro block size must be 1 through 32",
        ));
    }
    if mask.cols() != header.n_cols as usize || mask.rows() != header.n_rows as usize {
        return Err(LercError::WrongParam(
            "Lerc2 raw tiled mask dimensions do not match header",
        ));
    }
    if mask.count_valid_bits() != header.num_valid_pixel as usize {
        return Err(LercError::WrongParam(
            "Lerc2 raw tiled mask valid count does not match header",
        ));
    }

    if let Some(data) = data {
        let expected_len = (header.n_cols as usize)
            .checked_mul(header.n_rows as usize)
            .and_then(|count| count.checked_mul(header.n_depth as usize))
            .and_then(|count| count.checked_mul(header.data_type.size_in_bytes()))
            .ok_or(LercError::WrongParam(
                "Lerc2 raw tiled data byte count overflow",
            ))?;
        if data.len() != expected_len {
            return Err(LercError::WrongParam(
                "Lerc2 raw tiled data length does not match header",
            ));
        }
    }

    Ok(())
}

fn raw_tile_flag(version: i32, j0: usize) -> u8 {
    let pattern = if version >= 5 { 14 } else { 15 };
    (((j0 >> 3) as u8) & pattern) << 2
}

fn validate_single_band_encode_inputs(
    spec: EncodeSpec,
    data: &[u8],
    mask: Option<&BitMask>,
    context: &'static str,
) -> Result<()> {
    spec.validate()?;
    if spec.n_bands != 1 {
        return Err(LercError::WrongParam(
            "Lerc2 encode currently supports one band",
        ));
    }
    if spec.n_masks > 1 {
        return Err(LercError::WrongParam(
            "Lerc2 encode mask count must be 0 or 1",
        ));
    }
    if spec.n_cols > i32::MAX as usize
        || spec.n_rows > i32::MAX as usize
        || spec.n_depth > i32::MAX as usize
    {
        return Err(LercError::WrongParam("Lerc2 encode dimensions overflow"));
    }
    if data.len() != spec.data_byte_len()? {
        return Err(LercError::WrongParam("Lerc2 encode data length mismatch"));
    }
    if mask.is_some() && spec.n_masks == 0 {
        return Err(LercError::WrongParam(
            "Lerc2 encode mask count must be nonzero when a mask is supplied",
        ));
    }
    if let Some(mask) = mask {
        if mask.cols() != spec.n_cols || mask.rows() != spec.n_rows {
            return Err(LercError::WrongParam(
                "Lerc2 encode mask dimensions do not match spec",
            ));
        }
    }

    let _ = context;
    Ok(())
}

fn validate_encode_bands_inputs(
    spec: EncodeSpec,
    data: &[u8],
    masks: Option<&[u8]>,
    _context: &'static str,
) -> Result<()> {
    spec.validate()?;
    if spec.n_cols > i32::MAX as usize
        || spec.n_rows > i32::MAX as usize
        || spec.n_depth > i32::MAX as usize
        || spec.n_bands > i32::MAX as usize
    {
        return Err(LercError::WrongParam("Lerc2 encode dimensions overflow"));
    }
    if data.len() != spec.data_byte_len()? {
        return Err(LercError::WrongParam("Lerc2 encode data length mismatch"));
    }
    match (spec.n_masks, masks) {
        (0, None) => {}
        (0, Some(_)) => {
            return Err(LercError::WrongParam(
                "Lerc2 encode mask count must be nonzero when masks are supplied",
            ));
        }
        (_, Some(mask_bytes)) if mask_bytes.len() == spec.mask_byte_len()? => {}
        (_, Some(_)) => {
            return Err(LercError::WrongParam(
                "Lerc2 encode mask byte length mismatch",
            ));
        }
        (_, None) => {
            return Err(LercError::WrongParam(
                "Lerc2 encode masks are required when n_masks is nonzero",
            ));
        }
    }

    Ok(())
}

fn validate_encode_no_data_inputs(
    spec: EncodeSpec,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<()> {
    let Some(uses_no_data) = uses_no_data else {
        return Ok(());
    };
    if uses_no_data.len() != spec.n_bands {
        return Err(LercError::WrongParam(
            "Lerc2 encode no-data use count does not match band count",
        ));
    }
    if uses_no_data.iter().all(|&uses| uses == 0) {
        return Ok(());
    }
    if version < 6 {
        return Err(LercError::WrongParam(
            "Lerc2 no-data encode requires version 6 or newer",
        ));
    }
    let no_data_values = no_data_values.ok_or(LercError::WrongParam(
        "Lerc2 encode no-data values are required",
    ))?;
    if no_data_values.len() != spec.n_bands {
        return Err(LercError::WrongParam(
            "Lerc2 encode no-data value count does not match band count",
        ));
    }

    Ok(())
}

fn effective_encode_mask(spec: EncodeSpec, mask: Option<&BitMask>) -> Result<BitMask> {
    if let Some(mask) = mask {
        return Ok(mask.clone());
    }

    let mut mask = BitMask::new(spec.n_cols, spec.n_rows)?;
    mask.set_all_valid();
    Ok(mask)
}

fn validate_negative_max_z_error_for_encode(data_type: DataType, max_z_error: f64) -> Result<()> {
    if max_z_error < 0.0 && !is_integer_data_type(data_type) {
        return Err(LercError::WrongParam(
            "negative max_z_error bit-plane encode requires integer data",
        ));
    }
    Ok(())
}

fn normalize_lerc2_max_z_error_for_encode(
    spec: EncodeSpec,
    data: &[u8],
    mask: &BitMask,
    max_z_error: f64,
) -> Result<f64> {
    if max_z_error >= 0.0 {
        if is_integer_data_type(spec.data_type) {
            return Ok(max_z_error.floor().max(0.5));
        }
        if max_z_error > 0.0 && is_floating_point_data_type(spec.data_type) {
            return Ok(
                try_raise_lerc2_float_max_z_error(spec, data, mask, max_z_error)?
                    .unwrap_or(max_z_error),
            );
        }
        return Ok(max_z_error);
    }
    validate_negative_max_z_error_for_encode(spec.data_type, max_z_error)?;
    let inferred = try_lerc2_bit_plane_max_z_error(spec, data, mask, -max_z_error)?.unwrap_or(0.0);
    Ok(inferred.floor().max(0.5))
}

fn normalize_constant_lerc2_max_z_error_for_encode(
    data_type: DataType,
    max_z_error: f64,
) -> Result<f64> {
    if is_integer_data_type(data_type) {
        return Ok(max_z_error.floor().max(0.5));
    }
    if max_z_error < 0.0 {
        return Err(LercError::WrongParam(
            "negative max_z_error bit-plane encode requires integer data",
        ));
    }
    Ok(max_z_error)
}

fn is_integer_data_type(data_type: DataType) -> bool {
    (data_type as i32) < (DataType::Float as i32)
}

fn is_floating_point_data_type(data_type: DataType) -> bool {
    matches!(data_type, DataType::Float | DataType::Double)
}

#[allow(dead_code)]
fn try_raise_lerc2_float_max_z_error(
    spec: EncodeSpec,
    data: &[u8],
    mask: &BitMask,
    max_z_error: f64,
) -> Result<Option<f64>> {
    if !is_floating_point_data_type(spec.data_type)
        || max_z_error <= 0.0
        || mask.count_valid_bits() == 0
    {
        return Ok(None);
    }

    let candidates = [
        (0.5, 1.0),
        (0.25, 2.0),
        (0.05, 10.0),
        (0.025, 20.0),
        (0.005, 100.0),
        (0.0025, 200.0),
        (0.0005, 1000.0),
        (0.00025, 2000.0),
        (0.00005, 10000.0),
    ];
    let mut z_err = Vec::new();
    let mut z_fac = Vec::new();
    let mut round_err = Vec::new();
    for (err, fac) in candidates {
        if err > max_z_error {
            z_err.push(err);
            z_fac.push(fac);
            round_err.push(0.0f64);
        }
    }
    if z_err.is_empty() {
        return Ok(None);
    }

    let value_size = spec.data_type.size_in_bytes();
    for row in 0..spec.n_rows {
        let candidate_count = z_err.len();
        for col in 0..spec.n_cols {
            let pixel_idx = row * spec.n_cols + col;
            if !mask.is_valid(pixel_idx)? {
                continue;
            }
            for depth in 0..spec.n_depth {
                let offset = (pixel_idx * spec.n_depth + depth) * value_size;
                let value =
                    read_value_from_bytes(spec.data_type, &data[offset..offset + value_size]);
                if value.is_nan() {
                    return Err(LercError::WrongParam("Lerc2 encode input contains NaN"));
                }
                for candidate in 0..candidate_count {
                    let scaled = value * z_fac[candidate];
                    if scaled.fract() == 0.0 {
                        break;
                    }
                    let delta = ((scaled + 0.5).floor() - scaled).abs();
                    round_err[candidate] = round_err[candidate].max(delta);
                }
            }
        }

        if !prune_lerc2_float_error_candidates(&mut round_err, &mut z_err, &mut z_fac, max_z_error)
        {
            return Ok(None);
        }
    }

    for idx in 0..z_err.len() {
        if round_err[idx] / z_fac[idx] <= max_z_error / 2.0 {
            return Ok(Some(z_err[idx]));
        }
    }
    Ok(None)
}

fn prune_lerc2_float_error_candidates(
    round_err: &mut Vec<f64>,
    z_err: &mut Vec<f64>,
    z_fac: &mut Vec<f64>,
    max_z_error: f64,
) -> bool {
    if z_err.is_empty()
        || round_err.len() != z_err.len()
        || z_fac.len() != z_err.len()
        || max_z_error <= 0.0
    {
        return false;
    }

    for idx in (0..z_err.len()).rev() {
        if round_err[idx] / z_fac[idx] > max_z_error / 2.0 {
            round_err.remove(idx);
            z_err.remove(idx);
            z_fac.remove(idx);
        }
    }
    !z_err.is_empty()
}

#[allow(dead_code)]
fn try_lerc2_bit_plane_max_z_error(
    spec: EncodeSpec,
    data: &[u8],
    mask: &BitMask,
    eps: f64,
) -> Result<Option<f64>> {
    if !is_integer_data_type(spec.data_type) || eps <= 0.0 {
        return Ok(None);
    }
    let num_valid_pixel = mask.count_valid_bits();
    const MIN_COUNT: usize = 5000;
    if num_valid_pixel < MIN_COUNT {
        return Ok(None);
    }

    let max_shift = spec.data_type.size_in_bytes() * 8;
    let mut counts = vec![0usize; spec.n_depth * max_shift];
    let mut pair_count = 0usize;

    if spec.n_depth == 1 && num_valid_pixel == spec.n_cols * spec.n_rows {
        for row in 0..spec.n_rows.saturating_sub(1) {
            for col in 0..spec.n_cols.saturating_sub(1) {
                let idx = row * spec.n_cols + col;
                add_lerc2_bit_plane_diff_counts(
                    spec.data_type,
                    max_shift,
                    data,
                    idx,
                    idx + 1,
                    0,
                    spec.n_depth,
                    &mut counts,
                );
                pair_count += 1;
                add_lerc2_bit_plane_diff_counts(
                    spec.data_type,
                    max_shift,
                    data,
                    idx,
                    idx + spec.n_cols,
                    0,
                    spec.n_depth,
                    &mut counts,
                );
                pair_count += 1;
            }
        }
    } else {
        for row in 0..spec.n_rows {
            for col in 0..spec.n_cols {
                let pixel_idx = row * spec.n_cols + col;
                if !mask.is_valid(pixel_idx)? {
                    continue;
                }
                if col + 1 < spec.n_cols && mask.is_valid(pixel_idx + 1)? {
                    for depth in 0..spec.n_depth {
                        add_lerc2_bit_plane_diff_counts(
                            spec.data_type,
                            max_shift,
                            data,
                            pixel_idx,
                            pixel_idx + 1,
                            depth,
                            spec.n_depth,
                            &mut counts,
                        );
                    }
                    pair_count += 1;
                }
                if row + 1 < spec.n_rows && mask.is_valid(pixel_idx + spec.n_cols)? {
                    for depth in 0..spec.n_depth {
                        add_lerc2_bit_plane_diff_counts(
                            spec.data_type,
                            max_shift,
                            data,
                            pixel_idx,
                            pixel_idx + spec.n_cols,
                            depth,
                            spec.n_depth,
                            &mut counts,
                        );
                    }
                    pair_count += 1;
                }
            }
        }
    }

    if pair_count < MIN_COUNT {
        return Ok(None);
    }

    let mut cuts_found = 0usize;
    let mut last_plane_kept = 0usize;
    for bit in (0..max_shift).rev() {
        let critical = (0..spec.n_depth).all(|depth| {
            let ones = counts[depth * max_shift + bit] as f64;
            let m = ones / pair_count as f64;
            (1.0 - 2.0 * m).abs() < eps
        });
        if critical && cuts_found < 2 {
            if cuts_found == 0 {
                last_plane_kept = bit;
            }
            if cuts_found == 1 && bit < last_plane_kept.saturating_sub(1) {
                last_plane_kept = bit;
                cuts_found = 0;
            }
            cuts_found += 1;
        }
    }

    let max_z_error = if last_plane_kept == 0 {
        0.0
    } else {
        ((1u64 << last_plane_kept) >> 1) as f64
    };
    Ok(Some(max_z_error))
}

fn add_lerc2_bit_plane_diff_counts(
    data_type: DataType,
    max_shift: usize,
    data: &[u8],
    pixel_idx: usize,
    other_pixel_idx: usize,
    depth: usize,
    n_depth: usize,
    counts: &mut [usize],
) {
    let diff = match data_type {
        DataType::Char | DataType::Short | DataType::Int => {
            let lhs =
                read_integer_value_from_bytes(data_type, data, pixel_idx, depth, n_depth) as i32;
            let rhs =
                read_integer_value_from_bytes(data_type, data, other_pixel_idx, depth, n_depth)
                    as i32;
            (lhs ^ rhs) as u32
        }
        DataType::UChar | DataType::UShort | DataType::UInt => {
            let lhs =
                read_unsigned_value_from_bytes(data_type, data, pixel_idx, depth, n_depth) as u32;
            let rhs =
                read_unsigned_value_from_bytes(data_type, data, other_pixel_idx, depth, n_depth)
                    as u32;
            lhs ^ rhs
        }
        _ => unreachable!("bit-plane heuristic is only called for integer data"),
    };
    let start = depth * max_shift;
    for bit in 0..max_shift {
        counts[start + bit] += ((diff >> bit) & 1) as usize;
    }
}

fn read_integer_value_from_bytes(
    data_type: DataType,
    data: &[u8],
    pixel_idx: usize,
    depth: usize,
    n_depth: usize,
) -> i64 {
    let value_size = data_type.size_in_bytes();
    let offset = (pixel_idx * n_depth + depth) * value_size;
    match data_type {
        DataType::Char => i8::from_le_bytes(data[offset..offset + 1].try_into().unwrap()) as i64,
        DataType::Short => i16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()) as i64,
        DataType::Int => i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as i64,
        _ => unreachable!("signed integer reader called for non-signed type"),
    }
}

fn read_unsigned_value_from_bytes(
    data_type: DataType,
    data: &[u8],
    pixel_idx: usize,
    depth: usize,
    n_depth: usize,
) -> u64 {
    let value_size = data_type.size_in_bytes();
    let offset = (pixel_idx * n_depth + depth) * value_size;
    match data_type {
        DataType::UChar => data[offset] as u64,
        DataType::UShort => u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()) as u64,
        DataType::UInt => u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as u64,
        _ => unreachable!("unsigned integer reader called for non-unsigned type"),
    }
}

fn data_values_are_integer(data_type: DataType, data: &[u8]) -> Result<bool> {
    if is_integer_data_type(data_type) {
        return Ok(true);
    }

    let value_size = data_type.size_in_bytes();
    let mut offset = 0usize;
    while offset < data.len() {
        let value = read_value_from_bytes(data_type, &data[offset..offset + value_size]);
        if value.is_nan() {
            return Err(LercError::WrongParam("Lerc2 encode input contains NaN"));
        }
        if value.fract() != 0.0 {
            return Ok(false);
        }
        offset += value_size;
    }

    Ok(true)
}

fn constant_value_for_uncompressed_encode(data_type: DataType, data: &[u8]) -> Result<Option<f64>> {
    let value_size = data_type.size_in_bytes();
    if data.len() < value_size || data.len() % value_size != 0 {
        return Err(LercError::WrongParam(
            "encode data byte length does not match type",
        ));
    }
    let first = &data[..value_size];
    let value = read_value_from_bytes(data_type, first);
    if value.is_nan() {
        return Err(LercError::WrongParam("Lerc2 encode input contains NaN"));
    }
    for chunk in data[value_size..].chunks_exact(value_size) {
        if chunk != first {
            return Ok(None);
        }
    }
    Ok(Some(value))
}

fn all_bands_constant_for_uncompressed_encode(spec: EncodeSpec, data: &[u8]) -> Result<bool> {
    let band_spec = EncodeSpec {
        n_bands: 1,
        n_masks: usize::from(spec.n_masks > 0),
        ..spec
    };
    let band_data_len = band_spec.data_byte_len()?;
    for band in 0..spec.n_bands {
        let data_start = band
            .checked_mul(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        let data_end = data_start
            .checked_add(band_data_len)
            .ok_or(LercError::WrongParam("Lerc2 encode band offset overflow"))?;
        if constant_value_for_uncompressed_encode(spec.data_type, &data[data_start..data_end])?
            .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

struct PreparedNoDataBand {
    data: Vec<u8>,
    mask: Option<BitMask>,
    no_data: Option<(f64, f64)>,
}

fn prepare_no_data_band_for_encode(
    spec: EncodeSpec,
    data: &[u8],
    mask: Option<&BitMask>,
    no_data_orig: f64,
    max_z_error: f64,
) -> Result<PreparedNoDataBand> {
    let mut data = data.to_vec();
    let mut mask = effective_encode_mask(spec, mask)?;
    let value_size = spec.data_type.size_in_bytes();
    let no_data_orig = cast_no_data_value(spec.data_type, no_data_orig)?;
    let mut min_valid = f64::INFINITY;
    let mut max_valid = f64::NEG_INFINITY;
    let mut need_no_data = false;
    let mut modified_mask = false;

    for row in 0..spec.n_rows {
        for col in 0..spec.n_cols {
            let pixel_idx = row * spec.n_cols + col;
            if !mask.is_valid(pixel_idx)? {
                continue;
            }

            let mut no_data_count = 0usize;
            for depth in 0..spec.n_depth {
                let offset = (pixel_idx * spec.n_depth + depth) * value_size;
                let value =
                    read_value_from_bytes(spec.data_type, &data[offset..offset + value_size]);
                if values_equal_for_no_data(spec.data_type, value, no_data_orig) {
                    no_data_count += 1;
                } else {
                    min_valid = min_valid.min(value);
                    max_valid = max_valid.max(value);
                }
            }

            if no_data_count == spec.n_depth {
                mask.set_invalid(pixel_idx)?;
                modified_mask = true;
            } else if no_data_count > 0 {
                need_no_data = true;
            }
        }
    }

    if !need_no_data {
        return Ok(PreparedNoDataBand {
            data,
            mask: Some(mask).filter(|_| modified_mask),
            no_data: None,
        });
    }

    let no_data_internal = choose_internal_no_data_value(
        spec.data_type,
        no_data_orig,
        min_valid,
        max_valid,
        max_z_error,
    )?;
    if !values_equal_for_no_data(spec.data_type, no_data_internal, no_data_orig) {
        replace_no_data_value(spec, &mut data, &mask, no_data_orig, no_data_internal)?;
    }

    Ok(PreparedNoDataBand {
        data,
        mask: Some(mask),
        no_data: Some((no_data_internal, no_data_orig)),
    })
}

fn replace_no_data_value(
    spec: EncodeSpec,
    data: &mut [u8],
    mask: &BitMask,
    old_value: f64,
    new_value: f64,
) -> Result<()> {
    let value_size = spec.data_type.size_in_bytes();
    let new_bytes = encode_value_as_bytes(spec.data_type, new_value);
    for row in 0..spec.n_rows {
        for col in 0..spec.n_cols {
            let pixel_idx = row * spec.n_cols + col;
            if !mask.is_valid(pixel_idx)? {
                continue;
            }
            for depth in 0..spec.n_depth {
                let offset = (pixel_idx * spec.n_depth + depth) * value_size;
                let value =
                    read_value_from_bytes(spec.data_type, &data[offset..offset + value_size]);
                if values_equal_for_no_data(spec.data_type, value, old_value) {
                    data[offset..offset + value_size].copy_from_slice(&new_bytes);
                }
            }
        }
    }
    Ok(())
}

fn choose_internal_no_data_value(
    data_type: DataType,
    no_data_orig: f64,
    min_valid: f64,
    max_valid: f64,
    max_z_error: f64,
) -> Result<f64> {
    if !min_valid.is_finite() || !max_valid.is_finite() {
        return Ok(no_data_orig);
    }

    let (type_min, type_max) = data_type_range(data_type);
    let int_type = (data_type as i32) < (DataType::Float as i32);
    let dist = if int_type {
        max_z_error.max(0.5).floor() + 1.0
    } else {
        (2.0 * max_z_error).max(0.0001)
    };
    if no_data_orig < min_valid - dist || no_data_orig > max_valid + dist {
        return Ok(no_data_orig);
    }

    let below = min_valid - dist;
    if below >= type_min {
        return cast_no_data_value(data_type, below);
    }

    let above = max_valid + dist;
    if above <= type_max {
        return cast_no_data_value(data_type, above);
    }

    Err(LercError::Unsupported(
        "Lerc2 no-data encode could not find an internal sentinel",
    ))
}

fn cast_no_data_value(data_type: DataType, value: f64) -> Result<f64> {
    let (min_value, max_value) = data_type_range(data_type);
    if value < min_value || value > max_value {
        return Err(LercError::WrongParam(
            "Lerc2 no-data value is outside the data type range",
        ));
    }
    Ok(read_value_from_bytes(
        data_type,
        &encode_value_as_bytes(data_type, value),
    ))
}

fn values_equal_for_no_data(data_type: DataType, lhs: f64, rhs: f64) -> bool {
    if matches!(data_type, DataType::Float | DataType::Double) {
        lhs.to_bits() == rhs.to_bits()
    } else {
        lhs == rhs
    }
}

fn data_type_range(data_type: DataType) -> (f64, f64) {
    match data_type {
        DataType::Char => (i8::MIN as f64, i8::MAX as f64),
        DataType::UChar => (u8::MIN as f64, u8::MAX as f64),
        DataType::Short => (i16::MIN as f64, i16::MAX as f64),
        DataType::UShort => (u16::MIN as f64, u16::MAX as f64),
        DataType::Int => (i32::MIN as f64, i32::MAX as f64),
        DataType::UInt => (u32::MIN as f64, u32::MAX as f64),
        DataType::Float => (f32::MIN as f64, f32::MAX as f64),
        DataType::Double => (f64::MIN, f64::MAX),
    }
}

fn validate_decode_into_spec(spec: DecodeIntoSpec, has_mask_output: bool) -> Result<()> {
    if spec.n_depth == 0 || spec.n_cols == 0 || spec.n_rows == 0 || spec.n_bands == 0 {
        return Err(LercError::WrongParam(
            "decode output dimensions must be positive",
        ));
    }
    if !(spec.n_masks == 0 || spec.n_masks == 1 || spec.n_masks == spec.n_bands) {
        return Err(LercError::WrongParam(
            "decode mask count must be 0, 1, or n_bands",
        ));
    }
    if spec.n_masks > 0 && !has_mask_output {
        return Err(LercError::WrongParam(
            "decode mask output buffer is required when n_masks is nonzero",
        ));
    }
    Ok(())
}

fn required_mask_count(bands: &[DecodedLerc2]) -> usize {
    if bands.iter().all(|band| {
        band.mask.count_valid_bits()
            == (band.header.n_cols as usize) * (band.header.n_rows as usize)
    }) {
        return 0;
    }

    if bands.windows(2).all(|pair| pair[0].mask == pair[1].mask) {
        1
    } else {
        bands.len()
    }
}

fn read_min_max_ranges(reader: &mut Reader<'_>, header: &HeaderInfo) -> Result<MinMaxRanges> {
    let n_depth = header.n_depth as usize;
    let mut mins = Vec::with_capacity(n_depth);
    let mut maxs = Vec::with_capacity(n_depth);

    for _ in 0..n_depth {
        mins.push(reader.read_value_as_f64(header.data_type)?);
    }
    for _ in 0..n_depth {
        maxs.push(reader.read_value_as_f64(header.data_type)?);
    }

    let min_max_equal = mins
        .iter()
        .zip(maxs.iter())
        .all(|(min, max)| min.to_bits() == max.to_bits());

    Ok(MinMaxRanges {
        mins,
        maxs,
        bytes_consumed: reader.pos,
        min_max_equal,
    })
}

fn read_lerc2_data_ranges_blob(
    blob: &[u8],
    first_header: Option<&HeaderInfo>,
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo, MinMaxRanges)> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    if let Some(first) = first_header {
        if header.n_depth != first.n_depth
            || header.n_cols != first.n_cols
            || header.n_rows != first.n_rows
            || header.data_type != first.data_type
        {
            return Err(LercError::CorruptInput(
                "concatenated Lerc2 header mismatch",
            ));
        }
    }

    if header.n_depth == 1 {
        let mask = read_mask(&mut reader, &header, previous_mask)?;
        return Ok((
            header.clone(),
            mask,
            MinMaxRanges {
                mins: vec![header.z_min],
                maxs: vec![header.z_max],
                bytes_consumed: reader.pos,
                min_max_equal: header.z_min.to_bits() == header.z_max.to_bits(),
            },
        ));
    }

    if header.has_no_data_values() {
        return Err(LercError::HasNoData);
    }

    if header.version < 4 {
        return Err(LercError::Unsupported(
            "multi-depth Lerc2 data ranges require version 4+ blobs",
        ));
    }

    let mask = read_mask(&mut reader, &header, previous_mask)?;
    let ranges = if header.num_valid_pixel == 0 {
        MinMaxRanges {
            mins: vec![0.0; header.n_depth as usize],
            maxs: vec![0.0; header.n_depth as usize],
            bytes_consumed: reader.pos,
            min_max_equal: true,
        }
    } else if header.z_min == header.z_max {
        MinMaxRanges {
            mins: vec![header.z_min; header.n_depth as usize],
            maxs: vec![header.z_max; header.n_depth as usize],
            bytes_consumed: reader.pos,
            min_max_equal: true,
        }
    } else {
        read_min_max_ranges(&mut reader, &header)?
    };

    Ok((header, mask, ranges))
}

fn read_data_one_sweep(
    reader: &mut Reader<'_>,
    header: &HeaderInfo,
    mask: &BitMask,
) -> Result<DataOneSweep> {
    let n_depth = header.n_depth as usize;
    let bytes_per_pixel = n_depth
        .checked_mul(header.data_type.size_in_bytes())
        .ok_or(LercError::CorruptInput(
            "Lerc2 one-sweep pixel byte width overflow",
        ))?;
    let pixel_count = (header.n_cols as usize)
        .checked_mul(header.n_rows as usize)
        .ok_or(LercError::CorruptInput(
            "Lerc2 one-sweep pixel count overflow",
        ))?;
    let output_len = pixel_count
        .checked_mul(bytes_per_pixel)
        .ok_or(LercError::CorruptInput(
            "Lerc2 one-sweep output size overflow",
        ))?;
    let payload_len = (header.num_valid_pixel as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or(LercError::CorruptInput(
            "Lerc2 one-sweep payload size overflow",
        ))?;

    if mask.count_valid_bits() != header.num_valid_pixel as usize {
        return Err(LercError::CorruptInput(
            "Lerc2 one-sweep mask valid count does not match header",
        ));
    }

    let payload = reader.read_bytes(payload_len)?;
    let mut data = vec![0; output_len];
    let mut src_offset = 0usize;

    for pixel_idx in 0..pixel_count {
        if mask.is_valid(pixel_idx)? {
            let dst_offset = pixel_idx * bytes_per_pixel;
            data[dst_offset..dst_offset + bytes_per_pixel]
                .copy_from_slice(&payload[src_offset..src_offset + bytes_per_pixel]);
            src_offset += bytes_per_pixel;
        }
    }

    Ok(DataOneSweep {
        data,
        bytes_consumed: reader.pos,
    })
}

fn read_tiled_payload(
    reader: &mut Reader<'_>,
    header: &HeaderInfo,
    mask: &BitMask,
    ranges: Option<&MinMaxRanges>,
) -> Result<TiledData> {
    if header.micro_block_size > 32 {
        return Err(LercError::CorruptInput(
            "Lerc2 micro block size is too large",
        ));
    }
    if mask.count_valid_bits() != header.num_valid_pixel as usize {
        return Err(LercError::CorruptInput(
            "Lerc2 tiled mask valid count does not match header",
        ));
    }

    let n_depth = header.n_depth as usize;
    let value_size = header.data_type.size_in_bytes();
    let pixel_count = (header.n_cols as usize)
        .checked_mul(header.n_rows as usize)
        .ok_or(LercError::CorruptInput("Lerc2 tiled pixel count overflow"))?;
    let output_len = pixel_count
        .checked_mul(n_depth)
        .and_then(|count| count.checked_mul(value_size))
        .ok_or(LercError::CorruptInput("Lerc2 tiled output size overflow"))?;
    let mut data = vec![0; output_len];

    let mb_size = header.micro_block_size as usize;
    let n_rows = header.n_rows as usize;
    let n_cols = header.n_cols as usize;
    let tiles_vert = n_rows.div_ceil(mb_size);
    let tiles_hori = n_cols.div_ceil(mb_size);

    for i_tile in 0..tiles_vert {
        let i0 = i_tile * mb_size;
        let i1 = (i0 + mb_size).min(n_rows);

        for j_tile in 0..tiles_hori {
            let j0 = j_tile * mb_size;
            let j1 = (j0 + mb_size).min(n_cols);

            for i_depth in 0..n_depth {
                read_tile_payload(
                    reader, header, mask, ranges, &mut data, i0, i1, j0, j1, i_depth,
                )?;
            }
        }
    }

    Ok(TiledData {
        data,
        bytes_consumed: reader.pos,
    })
}

fn const_image_bytes(
    header: &HeaderInfo,
    mask: &BitMask,
    ranges: Option<&MinMaxRanges>,
) -> Result<Vec<u8>> {
    let value_size = header.data_type.size_in_bytes();
    let n_cols = header.n_cols as usize;
    let n_rows = header.n_rows as usize;
    let n_depth = header.n_depth as usize;
    let mut data = vec![0; n_cols * n_rows * n_depth * value_size];
    let depth_values: Vec<f64> = if n_depth > 1 && header.z_min != header.z_max {
        ranges
            .ok_or(LercError::CorruptInput("missing Lerc2 const depth ranges"))?
            .mins
            .clone()
    } else {
        vec![header.z_min; n_depth]
    };

    for row in 0..n_rows {
        for col in 0..n_cols {
            let pixel_idx = row * n_cols + col;
            if mask.is_valid(pixel_idx)? {
                for (depth, &value) in depth_values.iter().enumerate() {
                    let offset = (pixel_idx * n_depth + depth) * value_size;
                    let value_bytes = encode_value_as_bytes(header.data_type, value);
                    data[offset..offset + value_size].copy_from_slice(&value_bytes);
                }
            }
        }
    }

    Ok(data)
}

fn read_tile_payload(
    reader: &mut Reader<'_>,
    header: &HeaderInfo,
    mask: &BitMask,
    ranges: Option<&MinMaxRanges>,
    data: &mut [u8],
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    i_depth: usize,
) -> Result<()> {
    let raw_flag = reader.read_bytes(1)?[0];
    let diff_encoded = header.version >= 5 && (raw_flag & 4) != 0;
    if diff_encoded && i_depth == 0 {
        return Err(LercError::CorruptInput(
            "Lerc2 diff-encoded first depth tile is invalid",
        ));
    }

    let pattern = if header.version >= 5 { 14 } else { 15 };
    if ((raw_flag >> 2) & pattern) != (((j0 >> 3) as u8) & pattern) {
        return Err(LercError::CorruptInput(
            "Lerc2 tile integrity bits mismatch",
        ));
    }

    let value_size = header.data_type.size_in_bytes();
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let bits67 = raw_flag >> 6;
    let tile_mode = raw_flag & 3;

    match tile_mode {
        0 => {
            if diff_encoded {
                return Err(LercError::Unsupported(
                    "Lerc2 raw binary diff tiles are invalid",
                ));
            }
            for row in i0..i1 {
                for col in j0..j1 {
                    let pixel_idx = row * n_cols + col;
                    if mask.is_valid(pixel_idx)? {
                        let src = reader.read_bytes(value_size)?;
                        let dst_offset = (pixel_idx * n_depth + i_depth) * value_size;
                        data[dst_offset..dst_offset + value_size].copy_from_slice(src);
                    }
                }
            }
            Ok(())
        }
        2 => {
            if diff_encoded {
                copy_previous_depth_tile(header, mask, data, i0, i1, j0, j1, i_depth)
            } else {
                Ok(())
            }
        }
        3 => {
            let dt_used = get_data_type_used(
                if diff_encoded && (header.data_type as i32) < (DataType::Float as i32) {
                    DataType::Int
                } else {
                    header.data_type
                },
                bits67,
            )?;
            let offset = reader.read_value_as_f64(dt_used)?;
            fill_tile_with_value(
                header,
                mask,
                ranges,
                data,
                i0,
                i1,
                j0,
                j1,
                i_depth,
                offset,
                diff_encoded,
            )
        }
        1 => {
            let dt_used = get_data_type_used(
                if diff_encoded && (header.data_type as i32) < (DataType::Float as i32) {
                    DataType::Int
                } else {
                    header.data_type
                },
                bits67,
            )?;
            let offset = reader.read_value_as_f64(dt_used)?;
            let max_element_count = (i1 - i0) * (j1 - j0);
            let remaining = &reader.bytes[reader.pos..];
            let (quantized, consumed) =
                BitStuffer2::decode(remaining, max_element_count, header.version)?;
            reader.pos += consumed;
            scale_quantized_tile(
                header,
                mask,
                ranges,
                data,
                i0,
                i1,
                j0,
                j1,
                i_depth,
                offset,
                &quantized,
                diff_encoded,
            )
        }
        _ => unreachable!(),
    }
}

fn fill_tile_with_value(
    header: &HeaderInfo,
    mask: &BitMask,
    ranges: Option<&MinMaxRanges>,
    data: &mut [u8],
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    i_depth: usize,
    value: f64,
    diff_encoded: bool,
) -> Result<()> {
    let value_size = header.data_type.size_in_bytes();
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let z_max = z_max_for_depth(header, ranges, i_depth);

    for row in i0..i1 {
        for col in j0..j1 {
            let pixel_idx = row * n_cols + col;
            if mask.is_valid(pixel_idx)? {
                let dst_offset = (pixel_idx * n_depth + i_depth) * value_size;
                let value = if diff_encoded {
                    let prev_offset = (pixel_idx * n_depth + i_depth - 1) * value_size;
                    (value
                        + read_value_from_bytes(
                            header.data_type,
                            &data[prev_offset..prev_offset + value_size],
                        ))
                    .min(z_max)
                } else {
                    value
                };
                let value_bytes = encode_value_as_bytes(header.data_type, value);
                data[dst_offset..dst_offset + value_size].copy_from_slice(&value_bytes);
            }
        }
    }

    Ok(())
}

fn scale_quantized_tile(
    header: &HeaderInfo,
    mask: &BitMask,
    ranges: Option<&MinMaxRanges>,
    data: &mut [u8],
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    i_depth: usize,
    offset: f64,
    quantized: &[u32],
    diff_encoded: bool,
) -> Result<()> {
    let max_element_count = (i1 - i0) * (j1 - j0);
    let all_valid_tile = quantized.len() == max_element_count;
    let value_size = header.data_type.size_in_bytes();
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let inv_scale = 2.0 * header.max_z_error;
    let z_max = z_max_for_depth(header, ranges, i_depth);
    let mut src_idx = 0usize;

    for row in i0..i1 {
        for col in j0..j1 {
            let pixel_idx = row * n_cols + col;
            if all_valid_tile || mask.is_valid(pixel_idx)? {
                if src_idx >= quantized.len() {
                    return Err(LercError::CorruptInput(
                        "Lerc2 tiled bit-stuffed payload is too short",
                    ));
                }

                let mut value = offset + quantized[src_idx] as f64 * inv_scale;
                if diff_encoded {
                    let prev_offset = (pixel_idx * n_depth + i_depth - 1) * value_size;
                    value += read_value_from_bytes(
                        header.data_type,
                        &data[prev_offset..prev_offset + value_size],
                    );
                }
                value = value.min(z_max);
                let value_bytes = encode_value_as_bytes(header.data_type, value);
                let dst_offset = (pixel_idx * n_depth + i_depth) * value_size;
                data[dst_offset..dst_offset + value_size].copy_from_slice(&value_bytes);
                src_idx += 1;
            }
        }
    }

    if src_idx != quantized.len() {
        return Err(LercError::CorruptInput(
            "Lerc2 tiled bit-stuffed payload has unused values",
        ));
    }

    Ok(())
}

fn copy_previous_depth_tile(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &mut [u8],
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    i_depth: usize,
) -> Result<()> {
    let value_size = header.data_type.size_in_bytes();
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;

    for row in i0..i1 {
        for col in j0..j1 {
            let pixel_idx = row * n_cols + col;
            if mask.is_valid(pixel_idx)? {
                let dst_offset = (pixel_idx * n_depth + i_depth) * value_size;
                let prev_offset = dst_offset - value_size;
                let value = data[prev_offset..prev_offset + value_size].to_vec();
                data[dst_offset..dst_offset + value_size].copy_from_slice(&value);
            }
        }
    }

    Ok(())
}

fn z_max_for_depth(header: &HeaderInfo, ranges: Option<&MinMaxRanges>, i_depth: usize) -> f64 {
    if header.version >= 4 && header.n_depth > 1 {
        ranges
            .and_then(|ranges| ranges.maxs.get(i_depth))
            .copied()
            .unwrap_or(header.z_max)
    } else {
        header.z_max
    }
}

fn try_huffman_int(header: &HeaderInfo) -> bool {
    header.version >= 2
        && matches!(header.data_type, DataType::UChar | DataType::Char)
        && header.max_z_error == 0.5
}

fn try_huffman_float(header: &HeaderInfo) -> bool {
    header.version >= 6
        && matches!(header.data_type, DataType::Float | DataType::Double)
        && header.max_z_error == 0.0
}

struct HuffmanCodeTable {
    symbols: HashMap<(u8, u32), i32>,
    max_len: u8,
}

#[derive(Debug, Clone)]
struct HuffmanEncodeTable {
    codes: Vec<(u8, u32)>,
}

#[derive(Debug)]
struct HuffmanCandidate {
    image_mode: u8,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct HuffmanHeapEntry {
    weight: usize,
    min_symbol: usize,
    node_idx: usize,
}

impl Ord for HuffmanHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .weight
            .cmp(&self.weight)
            .then_with(|| other.min_symbol.cmp(&self.min_symbol))
            .then_with(|| other.node_idx.cmp(&self.node_idx))
    }
}

impl PartialOrd for HuffmanHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
struct HuffmanLengthNode {
    left: Option<usize>,
    right: Option<usize>,
    symbol: Option<usize>,
}

fn encode_huffman_int_payload(header: &HeaderInfo, mask: &BitMask, data: &[u8]) -> Result<Vec<u8>> {
    let (histo, delta_histo) = compute_huffman_int_histograms(header, mask, data)?;
    let regular = if header.version >= 4 {
        huffman_candidate(header, mask, data, 2, &histo)?
    } else {
        None
    };
    let delta = huffman_candidate(header, mask, data, 1, &delta_histo)?;
    let selected = match (regular, delta) {
        (Some(regular), Some(delta)) => {
            if regular.payload.len() <= delta.payload.len() {
                regular
            } else {
                delta
            }
        }
        (Some(regular), None) => regular,
        (None, Some(delta)) => delta,
        (None, None) => {
            return Err(LercError::WrongParam(
                "byte Huffman encode requires at least two symbols",
            ));
        }
    };

    let mut out = Vec::with_capacity(2 + selected.payload.len());
    out.push(0);
    out.push(selected.image_mode);
    out.extend_from_slice(&selected.payload);
    Ok(out)
}

fn huffman_candidate(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    image_mode: u8,
    histo: &[usize; 256],
) -> Result<Option<HuffmanCandidate>> {
    let Some(table) = compute_huffman_encode_table(histo)? else {
        return Ok(None);
    };
    let mut payload = Vec::new();
    write_huffman_code_table(&table, header.version, &mut payload)?;
    write_huffman_int_data_bits(header, mask, data, image_mode, &table, &mut payload)?;
    Ok(Some(HuffmanCandidate {
        image_mode,
        payload,
    }))
}

fn compute_huffman_int_histograms(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
) -> Result<([usize; 256], [usize; 256])> {
    if !matches!(header.data_type, DataType::UChar | DataType::Char) {
        return Err(LercError::WrongParam(
            "integer Huffman histograms require byte data",
        ));
    }
    let n_rows = header.n_rows as usize;
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let mut histo = [0usize; 256];
    let mut delta_histo = [0usize; 256];

    match header.data_type {
        DataType::UChar => {
            for i_depth in 0..n_depth {
                let mut prev = 0u8;
                for row in 0..n_rows {
                    for col in 0..n_cols {
                        let pixel_idx = row * n_cols + col;
                        if !mask.is_valid(pixel_idx)? {
                            continue;
                        }
                        let src = pixel_idx * n_depth + i_depth;
                        let value = data[src];
                        let base = if col > 0 && mask.is_valid(pixel_idx - 1)? {
                            prev
                        } else if row > 0 && mask.is_valid(pixel_idx - n_cols)? {
                            data[(pixel_idx - n_cols) * n_depth + i_depth]
                        } else {
                            prev
                        };
                        histo[value as usize] += 1;
                        delta_histo[value.wrapping_sub(base) as usize] += 1;
                        prev = value;
                    }
                }
            }
        }
        DataType::Char => {
            for i_depth in 0..n_depth {
                let mut prev = 0i8;
                for row in 0..n_rows {
                    for col in 0..n_cols {
                        let pixel_idx = row * n_cols + col;
                        if !mask.is_valid(pixel_idx)? {
                            continue;
                        }
                        let src = pixel_idx * n_depth + i_depth;
                        let value = data[src] as i8;
                        let base = if col > 0 && mask.is_valid(pixel_idx - 1)? {
                            prev
                        } else if row > 0 && mask.is_valid(pixel_idx - n_cols)? {
                            data[(pixel_idx - n_cols) * n_depth + i_depth] as i8
                        } else {
                            prev
                        };
                        histo[(i16::from(value) + 128) as usize] += 1;
                        delta_histo[(i16::from(value.wrapping_sub(base)) + 128) as usize] += 1;
                        prev = value;
                    }
                }
            }
        }
        _ => unreachable!("checked byte data type above"),
    }

    Ok((histo, delta_histo))
}

fn compute_huffman_encode_table(histo: &[usize; 256]) -> Result<Option<HuffmanEncodeTable>> {
    let mut heap = BinaryHeap::new();
    let mut nodes = Vec::new();
    for (symbol, &count) in histo.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let node_idx = nodes.len();
        nodes.push(HuffmanLengthNode {
            left: None,
            right: None,
            symbol: Some(symbol),
        });
        heap.push(HuffmanHeapEntry {
            weight: count,
            min_symbol: symbol,
            node_idx,
        });
    }

    if heap.len() < 2 {
        return Ok(None);
    }

    while heap.len() > 1 {
        let first = heap.pop().unwrap();
        let second = heap.pop().unwrap();
        let node_idx = nodes.len();
        nodes.push(HuffmanLengthNode {
            left: Some(first.node_idx),
            right: Some(second.node_idx),
            symbol: None,
        });
        heap.push(HuffmanHeapEntry {
            weight: first
                .weight
                .checked_add(second.weight)
                .ok_or(LercError::WrongParam("Huffman histogram weight overflow"))?,
            min_symbol: first.min_symbol.min(second.min_symbol),
            node_idx,
        });
    }

    let root = heap.pop().unwrap().node_idx;
    let mut lengths = vec![0u8; 256];
    fill_huffman_lengths(&nodes, root, 0, &mut lengths)?;
    Ok(Some(canonical_huffman_table(&lengths)))
}

fn fill_huffman_lengths(
    nodes: &[HuffmanLengthNode],
    node_idx: usize,
    depth: u8,
    lengths: &mut [u8],
) -> Result<()> {
    let node = &nodes[node_idx];
    if let Some(symbol) = node.symbol {
        if depth == 0 || depth > 32 {
            return Err(LercError::WrongParam("invalid Huffman code length"));
        }
        lengths[symbol] = depth;
        return Ok(());
    }
    if depth == 32 {
        return Err(LercError::WrongParam("Huffman code length exceeds 32 bits"));
    }
    fill_huffman_lengths(nodes, node.left.unwrap(), depth + 1, lengths)?;
    fill_huffman_lengths(nodes, node.right.unwrap(), depth + 1, lengths)?;
    Ok(())
}

fn canonical_huffman_table(lengths: &[u8]) -> HuffmanEncodeTable {
    let table_size = lengths.len();
    let mut symbols: Vec<(u8, usize)> = lengths
        .iter()
        .enumerate()
        .filter_map(|(symbol, &len)| (len > 0).then_some((len, symbol)))
        .collect();
    symbols.sort_unstable();
    let mut codes = vec![(0u8, 0u32); table_size];
    let mut code = 0u32;
    let mut previous_len = 0u8;
    for (len, symbol) in symbols {
        code <<= len - previous_len;
        codes[symbol] = (len, code);
        previous_len = len;
        code += 1;
    }

    HuffmanEncodeTable { codes }
}

fn write_huffman_code_table(
    table: &HuffmanEncodeTable,
    lerc2_version: i32,
    out: &mut Vec<u8>,
) -> Result<()> {
    let (i0, i1, _max_len) = huffman_code_range(&table.codes)?;
    out.extend_from_slice(&4i32.to_le_bytes());
    out.extend_from_slice(&(table.codes.len() as i32).to_le_bytes());
    out.extend_from_slice(&i0.to_le_bytes());
    out.extend_from_slice(&i1.to_le_bytes());

    let lengths: Vec<u32> = (i0..i1)
        .map(|i| {
            let idx = huffman_index_wrap(i, table.codes.len() as i32).unwrap() as usize;
            table.codes[idx].0 as u32
        })
        .collect();
    out.extend_from_slice(&BitStuffer2::encode_simple(&lengths, lerc2_version)?);

    let mut bits = HuffmanBitWriter::new();
    for i in i0..i1 {
        let idx = huffman_index_wrap(i, table.codes.len() as i32)? as usize;
        let (len, code) = table.codes[idx];
        if len > 0 {
            bits.push_bits(code, len)?;
        }
    }
    out.extend_from_slice(&bits.finish(false));
    Ok(())
}

fn huffman_code_range(codes: &[(u8, u32)]) -> Result<(i32, i32, u8)> {
    let size = codes.len();
    let first = codes
        .iter()
        .position(|&(len, _)| len > 0)
        .ok_or(LercError::WrongParam("empty Huffman code table"))?;
    let last = codes
        .iter()
        .rposition(|&(len, _)| len > 0)
        .ok_or(LercError::WrongParam("empty Huffman code table"))?
        + 1;

    let mut i0 = first;
    let mut i1 = last;
    let mut best_zero_start = 0usize;
    let mut best_zero_len = 0usize;
    let mut pos = 0usize;
    while pos < size {
        while pos < size && codes[pos].0 > 0 {
            pos += 1;
        }
        let zero_start = pos;
        while pos < size && codes[pos].0 == 0 {
            pos += 1;
        }
        let zero_len = pos - zero_start;
        if zero_len > best_zero_len {
            best_zero_start = zero_start;
            best_zero_len = zero_len;
        }
    }
    if size - best_zero_len < i1 - i0 {
        i0 = best_zero_start + best_zero_len;
        i1 = best_zero_start + size;
    }

    let mut max_len = 0u8;
    for i in i0..i1 {
        let idx = huffman_index_wrap(i as i32, size as i32)? as usize;
        max_len = max_len.max(codes[idx].0);
    }
    if max_len == 0 || max_len > 32 {
        return Err(LercError::WrongParam("invalid Huffman code range"));
    }
    Ok((i0 as i32, i1 as i32, max_len))
}

fn write_huffman_int_data_bits(
    header: &HeaderInfo,
    mask: &BitMask,
    data: &[u8],
    image_mode: u8,
    table: &HuffmanEncodeTable,
    out: &mut Vec<u8>,
) -> Result<()> {
    let n_rows = header.n_rows as usize;
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let mut bits = HuffmanBitWriter::new();

    match (header.data_type, image_mode) {
        (DataType::UChar, 1) => {
            for i_depth in 0..n_depth {
                let mut prev = 0u8;
                for row in 0..n_rows {
                    for col in 0..n_cols {
                        let pixel_idx = row * n_cols + col;
                        if !mask.is_valid(pixel_idx)? {
                            continue;
                        }
                        let src = pixel_idx * n_depth + i_depth;
                        let value = data[src];
                        let base = if col > 0 && mask.is_valid(pixel_idx - 1)? {
                            prev
                        } else if row > 0 && mask.is_valid(pixel_idx - n_cols)? {
                            data[(pixel_idx - n_cols) * n_depth + i_depth]
                        } else {
                            prev
                        };
                        write_huffman_symbol(&mut bits, table, value.wrapping_sub(base) as usize)?;
                        prev = value;
                    }
                }
            }
        }
        (DataType::UChar, 2) => {
            for pixel_idx in 0..(n_rows * n_cols) {
                if !mask.is_valid(pixel_idx)? {
                    continue;
                }
                let src = pixel_idx * n_depth;
                for i_depth in 0..n_depth {
                    write_huffman_symbol(&mut bits, table, data[src + i_depth] as usize)?;
                }
            }
        }
        (DataType::Char, 1) => {
            for i_depth in 0..n_depth {
                let mut prev = 0i8;
                for row in 0..n_rows {
                    for col in 0..n_cols {
                        let pixel_idx = row * n_cols + col;
                        if !mask.is_valid(pixel_idx)? {
                            continue;
                        }
                        let src = pixel_idx * n_depth + i_depth;
                        let value = data[src] as i8;
                        let base = if col > 0 && mask.is_valid(pixel_idx - 1)? {
                            prev
                        } else if row > 0 && mask.is_valid(pixel_idx - n_cols)? {
                            data[(pixel_idx - n_cols) * n_depth + i_depth] as i8
                        } else {
                            prev
                        };
                        let symbol = (i16::from(value.wrapping_sub(base)) + 128) as usize;
                        write_huffman_symbol(&mut bits, table, symbol)?;
                        prev = value;
                    }
                }
            }
        }
        (DataType::Char, 2) => {
            for pixel_idx in 0..(n_rows * n_cols) {
                if !mask.is_valid(pixel_idx)? {
                    continue;
                }
                let src = pixel_idx * n_depth;
                for i_depth in 0..n_depth {
                    let symbol = (i16::from(data[src + i_depth] as i8) + 128) as usize;
                    write_huffman_symbol(&mut bits, table, symbol)?;
                }
            }
        }
        _ => {
            return Err(LercError::WrongParam(
                "unsupported integer Huffman encode mode",
            ));
        }
    }

    out.extend_from_slice(&bits.finish(true));
    Ok(())
}

fn write_huffman_symbol(
    bits: &mut HuffmanBitWriter,
    table: &HuffmanEncodeTable,
    symbol: usize,
) -> Result<()> {
    let (len, code) = table.codes[symbol];
    if len == 0 {
        return Err(LercError::WrongParam("missing Huffman symbol code"));
    }
    bits.push_bits(code, len)
}

#[allow(dead_code)]
fn extract_fpl_compressed_buffer(encoded: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    let (&mode, payload) = encoded.split_first().ok_or(LercError::BufferTooSmall)?;
    match mode {
        FPL_HUFFMAN_RLE => {
            if payload.len() != 5 {
                return Err(LercError::CorruptInput(
                    "floating-point Huffman RLE payload has invalid length",
                ));
            }
            let count = u32::from_le_bytes(payload[1..5].try_into().unwrap()) as usize;
            if count != expected_len {
                return Err(LercError::CorruptInput(
                    "floating-point Huffman RLE count mismatch",
                ));
            }
            Ok(vec![payload[0]; expected_len])
        }
        FPL_HUFFMAN_NO_ENCODING => {
            if payload.len() != expected_len {
                return Err(LercError::CorruptInput(
                    "floating-point Huffman raw payload length mismatch",
                ));
            }
            Ok(payload.to_vec())
        }
        FPL_HUFFMAN_PACKBITS => extract_fpl_packbits(payload, expected_len),
        FPL_HUFFMAN_NORMAL => extract_fpl_normal_huffman(payload, expected_len),
        _ => Err(LercError::CorruptInput(
            "floating-point Huffman payload mode is invalid",
        )),
    }
}

#[allow(dead_code)]
fn extract_fpl_packbits(payload: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(expected_len);
    let mut reader = Reader::new(payload);
    while reader.pos < payload.len() {
        let control = reader.read_bytes(1)?[0];
        if control <= 127 {
            let count = control as usize + 1;
            let literal = reader.read_bytes(count)?;
            if out.len().saturating_add(count) > expected_len {
                return Err(LercError::CorruptInput(
                    "floating-point Huffman PackBits output is too long",
                ));
            }
            out.extend_from_slice(literal);
        } else {
            let count = control as usize - 127;
            let value = reader.read_bytes(1)?[0];
            if out.len().saturating_add(count) > expected_len {
                return Err(LercError::CorruptInput(
                    "floating-point Huffman PackBits output is too long",
                ));
            }
            out.resize(out.len() + count, value);
        }
    }
    if out.len() != expected_len {
        return Err(LercError::CorruptInput(
            "floating-point Huffman PackBits output length mismatch",
        ));
    }
    Ok(out)
}

#[allow(dead_code)]
fn extract_fpl_normal_huffman(payload: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    let mut reader = Reader::new(payload);
    let table = read_huffman_code_table(&mut reader, 5)?;
    let mut bits = HuffmanBitReader::new(&reader.bytes[reader.pos..]);
    let mut out = Vec::with_capacity(expected_len);
    for _ in 0..expected_len {
        let value = table.decode_symbol(&mut bits)?;
        if !(0..=255).contains(&value) {
            return Err(LercError::CorruptInput(
                "floating-point Huffman symbol is outside byte range",
            ));
        }
        out.push(value as u8);
    }
    Ok(out)
}

#[allow(dead_code)]
fn read_fp_huffman_slice(
    reader: &mut Reader<'_>,
    data_type: DataType,
    cols: usize,
    rows: usize,
) -> Result<Vec<u8>> {
    let predictor_code = reader.read_bytes(1)?[0];
    let predictor = FpPredictor::from_code(predictor_code).ok_or(LercError::CorruptInput(
        "floating-point Huffman predictor code is invalid",
    ))?;
    let value_size = match data_type {
        DataType::Float => 4,
        DataType::Double => 8,
        _ => {
            return Err(LercError::Unsupported(
                "floating-point Huffman slice requires float or double data",
            ))
        }
    };
    let expected_len = cols.checked_mul(rows).ok_or(LercError::CorruptInput(
        "floating-point sample count overflow",
    ))?;

    let mut planes = Vec::with_capacity(value_size);
    for _ in 0..value_size {
        let byte_index = reader.read_bytes(1)?[0] as usize;
        let byte_delta = reader.read_bytes(1)?[0];
        if byte_delta > FP_MAX_DELTA {
            return Err(LercError::CorruptInput(
                "floating-point Huffman byte delta level is invalid",
            ));
        }
        let compressed_size = reader.read_u32_le()? as usize;
        let compressed = reader.read_bytes(compressed_size)?;
        let extracted = extract_fpl_compressed_buffer(compressed, expected_len)?;
        let restored = restore_fp_byte_delta_sequence(&extracted, byte_delta)?;
        planes.push((byte_index, restored));
    }

    restore_fp_bytes_from_planes(&planes, data_type, cols, rows, predictor)
}

#[allow(dead_code)]
fn restore_fp_byte_delta_sequence(data: &[u8], level: u8) -> Result<Vec<u8>> {
    if level > FP_MAX_DELTA {
        return Err(LercError::CorruptInput(
            "floating-point Huffman byte delta level is invalid",
        ));
    }

    let mut restored = data.to_vec();
    for delta in (1..=level as usize).rev() {
        for idx in delta..restored.len() {
            let previous = restored[idx - 1];
            restored[idx] = restored[idx].wrapping_add(previous);
        }
    }
    Ok(restored)
}

#[allow(dead_code)]
fn restore_fp_bytes_from_planes(
    byte_planes: &[(usize, Vec<u8>)],
    data_type: DataType,
    cols: usize,
    rows: usize,
    predictor: FpPredictor,
) -> Result<Vec<u8>> {
    let value_size = match data_type {
        DataType::Float => 4,
        DataType::Double => 8,
        _ => {
            return Err(LercError::Unsupported(
                "floating-point Huffman restore requires float or double data",
            ))
        }
    };
    let sample_count = cols.checked_mul(rows).ok_or(LercError::CorruptInput(
        "floating-point sample count overflow",
    ))?;
    if byte_planes.len() != value_size {
        return Err(LercError::CorruptInput(
            "floating-point Huffman byte-plane count mismatch",
        ));
    }

    let mut bytes = vec![0u8; sample_count * value_size];
    let mut seen = vec![false; value_size];
    for (byte_index, plane) in byte_planes {
        if *byte_index >= value_size || seen[*byte_index] {
            return Err(LercError::CorruptInput(
                "floating-point Huffman byte-plane index is invalid",
            ));
        }
        if plane.len() != sample_count {
            return Err(LercError::CorruptInput(
                "floating-point Huffman byte-plane length mismatch",
            ));
        }
        seen[*byte_index] = true;
        for (sample_idx, byte) in plane.iter().copied().enumerate() {
            bytes[sample_idx * value_size + *byte_index] = byte;
        }
    }

    match data_type {
        DataType::Float => {
            let mut values: Vec<u32> = bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            match predictor {
                FpPredictor::RowsCols => {
                    restore_fp_cross_sequence_u32(&mut values, cols, rows, predictor.int_delta())
                }
                _ => restore_fp_block_sequence_u32(&mut values, cols, rows, predictor.int_delta()),
            }
            for value in &mut values {
                *value = undo_float_transform_bits(*value);
            }
            bytes.clear();
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        DataType::Double => {
            let mut values: Vec<u64> = bytes
                .chunks_exact(8)
                .map(|chunk| {
                    u64::from_le_bytes([
                        chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6],
                        chunk[7],
                    ])
                })
                .collect();
            match predictor {
                FpPredictor::RowsCols => {
                    restore_fp_cross_sequence_u64(&mut values, cols, rows, predictor.int_delta())
                }
                _ => restore_fp_block_sequence_u64(&mut values, cols, rows, predictor.int_delta()),
            }
            bytes.clear();
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        _ => unreachable!("data type checked above"),
    }
    Ok(bytes)
}

#[allow(dead_code)]
fn undo_float_transform_bits(value: u32) -> u32 {
    let mantissa = value & 0x007f_ffff;
    let exponent = ((value & 0xff80_0000) >> 24) & 0xff;
    let sign = (value >> 23) & 0x01;
    mantissa | (exponent << 23) | (sign << 31)
}

fn add_fp32_bits(lhs: u32, rhs: u32) -> u32 {
    let mantissa = lhs.wrapping_add(rhs) & 0x007f_ffff;
    let lhs_exp = ((lhs & 0xff80_0000) >> 23) & 0x1ff;
    let rhs_exp = ((rhs & 0xff80_0000) >> 23) & 0x1ff;
    mantissa | (((lhs_exp + rhs_exp) & 0x1ff) << 23)
}

fn add_fp64_bits(lhs: u64, rhs: u64) -> u64 {
    let mantissa = lhs.wrapping_add(rhs) & 0x000f_ffff_ffff_ffff;
    let lhs_exp = ((lhs & 0xfff0_0000_0000_0000) >> 52) & 0xfff;
    let rhs_exp = ((rhs & 0xfff0_0000_0000_0000) >> 52) & 0xfff;
    mantissa | (((lhs_exp + rhs_exp) & 0xfff) << 52)
}

fn restore_fp_block_sequence_u32(values: &mut [u32], cols: usize, rows: usize, delta: u8) {
    if delta == 2 {
        for row in 0..rows {
            let row_start = row * cols;
            for col in 2..cols {
                let idx = row_start + col;
                values[idx] = add_fp32_bits(values[idx], values[idx - 1]);
            }
        }
    }
    if delta > 0 {
        for row in 0..rows {
            let row_start = row * cols;
            for col in 1..cols {
                let idx = row_start + col;
                values[idx] = add_fp32_bits(values[idx], values[idx - 1]);
            }
        }
    }
}

fn restore_fp_block_sequence_u64(values: &mut [u64], cols: usize, rows: usize, delta: u8) {
    if delta == 2 {
        for row in 0..rows {
            let row_start = row * cols;
            for col in 2..cols {
                let idx = row_start + col;
                values[idx] = add_fp64_bits(values[idx], values[idx - 1]);
            }
        }
    }
    if delta > 0 {
        for row in 0..rows {
            let row_start = row * cols;
            for col in 1..cols {
                let idx = row_start + col;
                values[idx] = add_fp64_bits(values[idx], values[idx - 1]);
            }
        }
    }
}

fn restore_fp_cross_sequence_u32(values: &mut [u32], cols: usize, rows: usize, delta: u8) {
    if delta == 2 {
        for col in 0..cols {
            for row in 1..rows {
                let idx = row * cols + col;
                values[idx] = add_fp32_bits(values[idx], values[idx - cols]);
            }
        }
    }
    for row in 0..rows {
        let row_start = row * cols;
        for col in 1..cols {
            let idx = row_start + col;
            values[idx] = add_fp32_bits(values[idx], values[idx - 1]);
        }
    }
}

fn restore_fp_cross_sequence_u64(values: &mut [u64], cols: usize, rows: usize, delta: u8) {
    if delta == 2 {
        for col in 0..cols {
            for row in 1..rows {
                let idx = row * cols + col;
                values[idx] = add_fp64_bits(values[idx], values[idx - cols]);
            }
        }
    }
    for row in 0..rows {
        let row_start = row * cols;
        for col in 1..cols {
            let idx = row_start + col;
            values[idx] = add_fp64_bits(values[idx], values[idx - 1]);
        }
    }
}

fn read_huffman_int_payload(
    reader: &mut Reader<'_>,
    header: &HeaderInfo,
    mask: &BitMask,
    image_mode: u8,
) -> Result<Vec<u8>> {
    if !matches!(image_mode, 1 | 2) {
        return Err(LercError::Unsupported(
            "unsupported integer Huffman image mode",
        ));
    }
    if !matches!(header.data_type, DataType::UChar | DataType::Char) {
        return Err(LercError::Unsupported(
            "integer Huffman decode requires byte data",
        ));
    }

    let table = read_huffman_code_table(reader, header.version)?;
    let payload_start = reader.pos;
    let mut bits = HuffmanBitReader::new(&reader.bytes[payload_start..]);
    let n_rows = header.n_rows as usize;
    let n_cols = header.n_cols as usize;
    let n_depth = header.n_depth as usize;
    let mut out = vec![0u8; n_rows * n_cols * n_depth];

    match (header.data_type, image_mode) {
        (DataType::UChar, 1) => {
            for i_depth in 0..n_depth {
                let mut prev = 0u8;
                for row in 0..n_rows {
                    for col in 0..n_cols {
                        let pixel_idx = row * n_cols + col;
                        let dst = pixel_idx * n_depth + i_depth;
                        if !mask.is_valid(pixel_idx)? {
                            continue;
                        }
                        let delta = table.decode_symbol(&mut bits)? as u8;
                        let value = if col > 0 && mask.is_valid(pixel_idx - 1)? {
                            delta.wrapping_add(prev)
                        } else if row > 0 && mask.is_valid(pixel_idx - n_cols)? {
                            delta.wrapping_add(out[dst - n_cols * n_depth])
                        } else {
                            delta.wrapping_add(prev)
                        };
                        out[dst] = value;
                        prev = value;
                    }
                }
            }
        }
        (DataType::UChar, 2) => {
            for pixel_idx in 0..(n_rows * n_cols) {
                if !mask.is_valid(pixel_idx)? {
                    continue;
                }
                let dst = pixel_idx * n_depth;
                for i_depth in 0..n_depth {
                    out[dst + i_depth] = table.decode_symbol(&mut bits)? as u8;
                }
            }
        }
        (DataType::Char, 1) => {
            for i_depth in 0..n_depth {
                let mut prev = 0i8;
                for row in 0..n_rows {
                    for col in 0..n_cols {
                        let pixel_idx = row * n_cols + col;
                        let dst = pixel_idx * n_depth + i_depth;
                        if !mask.is_valid(pixel_idx)? {
                            continue;
                        }
                        let delta = (table.decode_symbol(&mut bits)? - 128) as i8;
                        let value = if col > 0 && mask.is_valid(pixel_idx - 1)? {
                            delta.wrapping_add(prev)
                        } else if row > 0 && mask.is_valid(pixel_idx - n_cols)? {
                            delta.wrapping_add(out[dst - n_cols * n_depth] as i8)
                        } else {
                            delta.wrapping_add(prev)
                        };
                        out[dst] = value as u8;
                        prev = value;
                    }
                }
            }
        }
        (DataType::Char, 2) => {
            for pixel_idx in 0..(n_rows * n_cols) {
                if !mask.is_valid(pixel_idx)? {
                    continue;
                }
                let dst = pixel_idx * n_depth;
                for i_depth in 0..n_depth {
                    out[dst + i_depth] = (table.decode_symbol(&mut bits)? - 128) as i8 as u8;
                }
            }
        }
        _ => unreachable!("checked integer Huffman inputs above"),
    }

    let full_words = bits.bit_pos / 32;
    let partial_words = usize::from(bits.bit_pos % 32 != 0);
    let bytes_consumed =
        (full_words + partial_words + 1)
            .checked_mul(4)
            .ok_or(LercError::CorruptInput(
                "Huffman payload byte count overflow",
            ))?;
    if reader.bytes[payload_start..].len() < bytes_consumed {
        return Err(LercError::BufferTooSmall);
    }
    reader.pos = payload_start + bytes_consumed;
    Ok(out)
}

fn read_huffman_float_payload(reader: &mut Reader<'_>, header: &HeaderInfo) -> Result<Vec<u8>> {
    let n_cols = header.n_cols as usize;
    let n_rows = header.n_rows as usize;
    let n_depth = header.n_depth as usize;
    if n_depth == 1 {
        read_fp_huffman_slice(reader, header.data_type, n_cols, n_rows)
    } else {
        read_fp_huffman_slice(reader, header.data_type, n_depth, n_cols * n_rows)
    }
}

fn read_huffman_code_table(
    reader: &mut Reader<'_>,
    lerc2_version: i32,
) -> Result<HuffmanCodeTable> {
    let version = reader.read_i32_le()?;
    let size = reader.read_i32_le()?;
    let i0 = reader.read_i32_le()?;
    let i1 = reader.read_i32_le()?;
    if version < 2 || size <= 0 || size >= 65_536 || i0 >= i1 || i0 < 0 {
        return Err(LercError::CorruptInput("invalid Huffman code table header"));
    }
    if huffman_index_wrap(i0, size)? >= size || huffman_index_wrap(i1 - 1, size)? >= size {
        return Err(LercError::CorruptInput("invalid Huffman code range"));
    }

    let code_count = (i1 - i0) as usize;
    let (lengths, consumed) =
        BitStuffer2::decode(&reader.bytes[reader.pos..], code_count, lerc2_version)?;
    if lengths.len() != code_count {
        return Err(LercError::CorruptInput("Huffman code length mismatch"));
    }
    reader.pos += consumed;

    let mut code_lengths = vec![0u8; size as usize];
    for i in i0..i1 {
        let len = lengths[(i - i0) as usize];
        if len > 32 {
            return Err(LercError::CorruptInput("invalid Huffman code length"));
        }
        let idx = huffman_index_wrap(i, size)? as usize;
        code_lengths[idx] = len as u8;
    }

    let total_bits: usize = code_lengths.iter().map(|&len| len as usize).sum();
    let code_bytes = total_bits.div_ceil(32) * 4;
    if reader.bytes.len().saturating_sub(reader.pos) < code_bytes {
        return Err(LercError::BufferTooSmall);
    }
    let mut bit_reader = HuffmanBitReader::new(&reader.bytes[reader.pos..reader.pos + code_bytes]);
    let mut symbols = HashMap::new();
    let mut max_len = 0u8;
    for i in i0..i1 {
        let idx = huffman_index_wrap(i, size)? as usize;
        let len = code_lengths[idx];
        if len == 0 {
            continue;
        }
        let code = bit_reader.read_bits(len)?;
        max_len = max_len.max(len);
        if symbols.insert((len, code), idx as i32).is_some() {
            return Err(LercError::CorruptInput("duplicate Huffman code"));
        }
    }
    reader.pos += code_bytes;

    if symbols.is_empty() || max_len == 0 {
        return Err(LercError::CorruptInput("empty Huffman code table"));
    }
    Ok(HuffmanCodeTable { symbols, max_len })
}

fn huffman_index_wrap(i: i32, size: i32) -> Result<i32> {
    let idx = i - if i < size { 0 } else { size };
    if idx < 0 || idx >= size {
        return Err(LercError::CorruptInput("invalid wrapped Huffman index"));
    }
    Ok(idx)
}

impl HuffmanCodeTable {
    fn decode_symbol(&self, bits: &mut HuffmanBitReader<'_>) -> Result<i32> {
        let mut code = 0u32;
        for len in 1..=self.max_len {
            code = (code << 1) | u32::from(bits.read_bit()?);
            if let Some(&symbol) = self.symbols.get(&(len, code)) {
                return Ok(symbol);
            }
        }
        Err(LercError::CorruptInput("invalid Huffman payload code"))
    }
}

struct HuffmanBitReader<'a> {
    bytes: &'a [u8],
    bit_pos: usize,
}

struct HuffmanBitWriter {
    words: Vec<u32>,
    bit_pos: usize,
}

impl HuffmanBitWriter {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            bit_pos: 0,
        }
    }

    fn push_bits(&mut self, value: u32, len: u8) -> Result<()> {
        if len == 0 || len > 32 {
            return Err(LercError::WrongParam("invalid Huffman bit width"));
        }
        let value = if len == 32 {
            value
        } else {
            value & ((1u32 << len) - 1)
        };
        let word_idx = self.bit_pos / 32;
        let bit_in_word = self.bit_pos % 32;
        if self.words.len() <= word_idx {
            self.words.resize(word_idx + 1, 0);
        }

        if 32 - bit_in_word >= len as usize {
            self.words[word_idx] |= value << (32 - bit_in_word - len as usize);
            self.bit_pos += len as usize;
        } else {
            let first_len = 32 - bit_in_word;
            let second_len = len as usize - first_len;
            self.words[word_idx] |= value >> second_len;
            let next_idx = word_idx + 1;
            if self.words.len() <= next_idx {
                self.words.resize(next_idx + 1, 0);
            }
            self.words[next_idx] |= value << (32 - second_len);
            self.bit_pos += len as usize;
        }
        Ok(())
    }

    fn finish(mut self, add_lookahead_word: bool) -> Vec<u8> {
        let used_words = self.bit_pos.div_ceil(32);
        self.words.truncate(used_words);
        if add_lookahead_word {
            self.words.push(0);
        }

        let mut out = Vec::with_capacity(self.words.len() * 4);
        for word in self.words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }
}

impl<'a> HuffmanBitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    fn read_bit(&mut self) -> Result<u8> {
        Ok(self.read_bits(1)? as u8)
    }

    fn read_bits(&mut self, len: u8) -> Result<u32> {
        if len == 0 || len > 32 {
            return Err(LercError::CorruptInput("invalid Huffman bit width"));
        }
        let mut value = 0u32;
        for _ in 0..len {
            let word_offset = (self.bit_pos / 32)
                .checked_mul(4)
                .ok_or(LercError::CorruptInput("Huffman bit offset overflow"))?;
            let word = self
                .bytes
                .get(word_offset..word_offset + 4)
                .ok_or(LercError::BufferTooSmall)?;
            let word = u32::from_le_bytes(word.try_into().unwrap());
            let bit_in_word = self.bit_pos % 32;
            let bit = (word << bit_in_word) >> 31;
            value = (value << 1) | bit;
            self.bit_pos += 1;
        }
        Ok(value)
    }
}

fn decode_lerc2_typed_values(
    header: &HeaderInfo,
    mask: &BitMask,
    bytes: &[u8],
) -> Result<DecodedData> {
    let mut data = decode_typed_values(header.data_type, bytes)?;
    remap_no_data_values(&mut data, header, mask)?;
    Ok(data)
}

fn remap_no_data_values(data: &mut DecodedData, header: &HeaderInfo, mask: &BitMask) -> Result<()> {
    if !header.has_no_data_values()
        || header.n_depth <= 1
        || header.no_data_val.to_bits() == header.no_data_val_orig.to_bits()
    {
        return Ok(());
    }

    let n_depth = header.n_depth as usize;
    let n_cols = header.n_cols as usize;
    let n_rows = header.n_rows as usize;
    let expected_len = n_cols
        .checked_mul(n_rows)
        .and_then(|count| count.checked_mul(n_depth))
        .ok_or(LercError::CorruptInput(
            "Lerc2 no-data value count overflow",
        ))?;
    if data.len() != expected_len {
        return Err(LercError::CorruptInput(
            "Lerc2 decoded no-data remap length mismatch",
        ));
    }

    match data {
        DecodedData::Char(values) => remap_no_data_slice(
            values,
            header.no_data_val as i8,
            header.no_data_val_orig as i8,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::UChar(values) => remap_no_data_slice(
            values,
            header.no_data_val as u8,
            header.no_data_val_orig as u8,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::Short(values) => remap_no_data_slice(
            values,
            header.no_data_val as i16,
            header.no_data_val_orig as i16,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::UShort(values) => remap_no_data_slice(
            values,
            header.no_data_val as u16,
            header.no_data_val_orig as u16,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::Int(values) => remap_no_data_slice(
            values,
            header.no_data_val as i32,
            header.no_data_val_orig as i32,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::UInt(values) => remap_no_data_slice(
            values,
            header.no_data_val as u32,
            header.no_data_val_orig as u32,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::Float(values) => remap_no_data_slice(
            values,
            header.no_data_val as f32,
            header.no_data_val_orig as f32,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
        DecodedData::Double(values) => remap_no_data_slice(
            values,
            header.no_data_val,
            header.no_data_val_orig,
            n_cols,
            n_rows,
            n_depth,
            mask,
        ),
    }
}

fn remap_no_data_slice<T: Copy + PartialEq>(
    values: &mut [T],
    old_value: T,
    new_value: T,
    n_cols: usize,
    n_rows: usize,
    n_depth: usize,
    mask: &BitMask,
) -> Result<()> {
    for row in 0..n_rows {
        for col in 0..n_cols {
            let pixel_idx = row * n_cols + col;
            if mask.is_valid(pixel_idx)? {
                let base = pixel_idx * n_depth;
                for depth in 0..n_depth {
                    if values[base + depth] == old_value {
                        values[base + depth] = new_value;
                    }
                }
            }
        }
    }

    Ok(())
}

fn get_data_type_used(data_type: DataType, tc: u8) -> Result<DataType> {
    let dt = data_type as i32;
    let tc = tc as i32;
    let used = match data_type {
        DataType::Short | DataType::Int => dt - tc,
        DataType::UShort | DataType::UInt => dt - 2 * tc,
        DataType::Float => {
            if tc == 0 {
                dt
            } else if tc == 1 {
                DataType::Short as i32
            } else {
                DataType::UChar as i32
            }
        }
        DataType::Double => dt - 2 * tc + 1,
        _ => dt,
    };
    DataType::try_from(used)
}

fn encode_value_as_bytes(data_type: DataType, value: f64) -> Vec<u8> {
    match data_type {
        DataType::Char => vec![(value as i8) as u8],
        DataType::UChar => vec![value as u8],
        DataType::Short => (value as i16).to_le_bytes().to_vec(),
        DataType::UShort => (value as u16).to_le_bytes().to_vec(),
        DataType::Int => (value as i32).to_le_bytes().to_vec(),
        DataType::UInt => (value as u32).to_le_bytes().to_vec(),
        DataType::Float => (value as f32).to_le_bytes().to_vec(),
        DataType::Double => value.to_le_bytes().to_vec(),
    }
}

fn read_value_from_bytes(data_type: DataType, bytes: &[u8]) -> f64 {
    match data_type {
        DataType::Char => i8::from_le_bytes(bytes[..1].try_into().unwrap()) as f64,
        DataType::UChar => bytes[0] as f64,
        DataType::Short => i16::from_le_bytes(bytes[..2].try_into().unwrap()) as f64,
        DataType::UShort => u16::from_le_bytes(bytes[..2].try_into().unwrap()) as f64,
        DataType::Int => i32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        DataType::UInt => u32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        DataType::Float => f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        DataType::Double => f64::from_le_bytes(bytes[..8].try_into().unwrap()),
    }
}

fn read_header(reader: &mut Reader<'_>) -> Result<HeaderInfo> {
    let start = reader.pos;
    if reader.read_bytes(FILE_KEY.len())? != FILE_KEY {
        return Err(LercError::CorruptInput("missing Lerc2 file key"));
    }

    let version = reader.read_i32_le()?;
    if !(0..=CURRENT_VERSION).contains(&version) {
        return Err(LercError::CorruptInput("unsupported Lerc2 version"));
    }

    let checksum = if version >= 3 {
        reader.read_u32_le()?
    } else {
        0
    };

    let n_ints = 6 + i32::from(version >= 4) as usize + i32::from(version >= 6) as usize;
    let mut ints = Vec::with_capacity(n_ints);
    for _ in 0..n_ints {
        ints.push(reader.read_i32_le()?);
    }

    let mut bytes = [0u8; 4];
    if version >= 6 {
        bytes.copy_from_slice(reader.read_bytes(4)?);
    }

    let n_doubles = 3 + if version >= 6 { 2 } else { 0 };
    let mut doubles = Vec::with_capacity(n_doubles);
    for _ in 0..n_doubles {
        doubles.push(reader.read_f64_le()?);
    }

    let mut i = 0usize;
    let n_rows = ints[i];
    i += 1;
    let n_cols = ints[i];
    i += 1;
    let n_depth = if version >= 4 {
        let value = ints[i];
        i += 1;
        value
    } else {
        1
    };
    let num_valid_pixel = ints[i];
    i += 1;
    let micro_block_size = ints[i];
    i += 1;
    let blob_size = ints[i];
    i += 1;
    let data_type = DataType::try_from(ints[i])?;
    i += 1;
    let n_blobs_more = if version >= 6 { ints[i] } else { 0 };

    let mut d = 0usize;
    let max_z_error = doubles[d];
    d += 1;
    let z_min = doubles[d];
    d += 1;
    let z_max = doubles[d];
    d += 1;
    let no_data_val = if version >= 6 {
        let value = doubles[d];
        d += 1;
        value
    } else {
        0.0
    };
    let no_data_val_orig = if version >= 6 { doubles[d] } else { 0.0 };

    validate_header_dims(
        n_rows,
        n_cols,
        n_depth,
        num_valid_pixel,
        micro_block_size,
        blob_size,
    )?;

    Ok(HeaderInfo {
        version,
        checksum,
        n_rows,
        n_cols,
        n_depth,
        num_valid_pixel,
        micro_block_size,
        blob_size,
        n_blobs_more,
        b_pass_no_data_values: bytes[0],
        b_is_int: bytes[1],
        b_reserved_3: bytes[2],
        b_reserved_4: bytes[3],
        data_type,
        max_z_error,
        z_min,
        z_max,
        no_data_val,
        no_data_val_orig,
        header_size: reader.pos - start,
    })
}

fn validate_header_dims(
    n_rows: i32,
    n_cols: i32,
    n_depth: i32,
    num_valid_pixel: i32,
    micro_block_size: i32,
    blob_size: i32,
) -> Result<()> {
    if n_rows <= 0
        || n_cols <= 0
        || n_depth <= 0
        || num_valid_pixel < 0
        || micro_block_size <= 0
        || blob_size <= 0
    {
        return Err(LercError::CorruptInput("invalid Lerc2 header dimensions"));
    }

    let pixel_count = n_rows
        .checked_mul(n_cols)
        .ok_or(LercError::CorruptInput("Lerc2 pixel count overflow"))?;
    if num_valid_pixel > pixel_count {
        return Err(LercError::CorruptInput(
            "valid pixel count exceeds image dimensions",
        ));
    }

    Ok(())
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

    fn read_u32_le(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_f64_le(&mut self) -> Result<f64> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_value_as_f64(&mut self, data_type: DataType) -> Result<f64> {
        match data_type {
            DataType::Char => Ok(i8::from_le_bytes(self.read_bytes(1)?.try_into().unwrap()) as f64),
            DataType::UChar => Ok(self.read_bytes(1)?[0] as f64),
            DataType::Short => {
                Ok(i16::from_le_bytes(self.read_bytes(2)?.try_into().unwrap()) as f64)
            }
            DataType::UShort => {
                Ok(u16::from_le_bytes(self.read_bytes(2)?.try_into().unwrap()) as f64)
            }
            DataType::Int => Ok(i32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()) as f64),
            DataType::UInt => {
                Ok(u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()) as f64)
            }
            DataType::Float => {
                Ok(f32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()) as f64)
            }
            DataType::Double => self.read_f64_le(),
        }
    }

    fn peek_i32_le(&self) -> Result<i32> {
        if self.bytes.len().saturating_sub(self.pos) < 4 {
            return Err(LercError::BufferTooSmall);
        }
        Ok(i32::from_le_bytes(
            self.bytes[self.pos..self.pos + 4].try_into().unwrap(),
        ))
    }
}

struct Writer<'a> {
    bytes: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn write_bytes(&mut self, src: &[u8]) -> Result<()> {
        if self.bytes.len().saturating_sub(self.pos) < src.len() {
            return Err(LercError::BufferTooSmall);
        }
        self.bytes[self.pos..self.pos + src.len()].copy_from_slice(src);
        self.pos += src.len();
        Ok(())
    }

    fn write_i32_le(&mut self, value: i32) -> Result<()> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_u32_le(&mut self, value: u32) -> Result<()> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_f64_le(&mut self, value: f64) -> Result<()> {
        self.write_bytes(&value.to_le_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_checksum_fletcher32, compute_huffman_encode_table,
        compute_lerc2_data_ranges_for_encode, compute_lerc2_header_byte_len,
        compute_lerc2_mask_byte_len, compute_lerc2_min_max_ranges_byte_len,
        compute_lerc2_one_sweep_byte_len, compute_lerc2_tiled_raw_byte_len,
        decode_lerc2_bands_supported, decode_lerc2_supported, decode_lerc2_supported_into,
        decode_lerc_supported_into, decode_lerc_supported_to_f64, encode_lerc2_auto,
        encode_lerc2_auto_with_no_data, encode_lerc2_byte_huffman, encode_lerc2_byte_huffman_bands,
        encode_lerc2_byte_huffman_bands_with_no_data, encode_lerc2_byte_huffman_with_no_data,
        encode_lerc2_constant, encode_lerc2_constant_bands, encode_lerc2_one_sweep,
        encode_lerc2_one_sweep_bands, encode_lerc2_one_sweep_bands_with_no_data,
        encode_lerc2_one_sweep_with_no_data, encode_lerc2_tiled_lut, encode_lerc2_tiled_lut_bands,
        encode_lerc2_tiled_raw, encode_lerc2_tiled_raw_bands,
        encode_lerc2_tiled_raw_bands_with_no_data, encode_lerc2_tiled_raw_with_no_data,
        encode_lerc2_tiled_simple, encode_lerc2_tiled_simple_bands,
        encode_lerc2_tiled_simple_bands_with_no_data, encode_lerc2_tiled_simple_with_no_data,
        encode_lerc2_uncompressed, encode_lerc2_uncompressed_with_no_data,
        extract_fpl_compressed_buffer, finalize_lerc2_checksum, get_lerc2_blob_info_arrays,
        get_lerc2_data_ranges, get_lerc2_header_info, get_lerc2_no_data_info, get_lerc_info,
        read_fp_huffman_slice, read_lerc2_data_one_sweep, read_lerc2_mask,
        read_lerc2_mask_with_previous, read_lerc2_min_max_ranges,
        read_lerc2_min_max_ranges_with_previous, read_lerc2_tiled_payload, read_lerc2_tiled_raw,
        restore_fp_byte_delta_sequence, restore_fp_bytes_from_planes,
        try_lerc2_bit_plane_max_z_error, try_raise_lerc2_float_max_z_error,
        validate_lerc2_checksum, write_huffman_code_table, write_lerc2_header, write_lerc2_mask,
        write_lerc2_min_max_ranges, write_lerc2_one_sweep, write_lerc2_tiled_raw, DecodeIntoSpec,
        FpPredictor, HeaderInfo, HuffmanBitWriter, MinMaxRanges, Reader, BLOB_DATA_RANGE_ARRAY_LEN,
        BLOB_INFO_ARRAY_LEN, FILE_KEY,
    };
    use crate::{BitMask, BitStuffer2, DataType, DecodedData, EncodeSpec, LercError, Rle};
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("testData");
        path.push(name);
        fs::read(path).unwrap()
    }

    fn header_for_write(version: i32) -> HeaderInfo {
        let header_size = compute_lerc2_header_byte_len(version).unwrap();
        HeaderInfo {
            version,
            checksum: 0x1234_5678,
            n_rows: 2,
            n_cols: 3,
            n_depth: if version >= 4 { 2 } else { 1 },
            num_valid_pixel: 5,
            micro_block_size: 8,
            blob_size: (header_size + 4) as i32,
            n_blobs_more: if version >= 6 { 1 } else { 0 },
            b_pass_no_data_values: if version >= 6 { 1 } else { 0 },
            b_is_int: if version >= 6 { 1 } else { 0 },
            b_reserved_3: if version >= 6 { 2 } else { 0 },
            b_reserved_4: if version >= 6 { 3 } else { 0 },
            data_type: DataType::Float,
            max_z_error: 0.125,
            z_min: -3.5,
            z_max: 10.25,
            no_data_val: if version >= 6 { -9999.0 } else { 0.0 },
            no_data_val_orig: if version >= 6 { -32768.0 } else { 0.0 },
            header_size,
        }
    }

    fn blob_with_written_header_and_mask(
        header: &HeaderInfo,
        mask: Option<&BitMask>,
        encode_mask: bool,
    ) -> Vec<u8> {
        let header_len = compute_lerc2_header_byte_len(header.version).unwrap();
        let mask_len = compute_lerc2_mask_byte_len(header, mask, encode_mask).unwrap();
        let mut blob = vec![0; header_len + mask_len];
        let written_header = write_lerc2_header(header, &mut blob).unwrap();
        let written_mask =
            write_lerc2_mask(header, mask, encode_mask, &mut blob[written_header..]).unwrap();
        assert_eq!(written_header, header_len);
        assert_eq!(written_mask, mask_len);
        blob
    }

    fn blob_with_written_header_mask_and_ranges(
        header: &HeaderInfo,
        mask: Option<&BitMask>,
        encode_mask: bool,
        ranges: &MinMaxRanges,
    ) -> Vec<u8> {
        let header_len = compute_lerc2_header_byte_len(header.version).unwrap();
        let mask_len = compute_lerc2_mask_byte_len(header, mask, encode_mask).unwrap();
        let ranges_len = compute_lerc2_min_max_ranges_byte_len(header).unwrap();
        let mut blob = vec![0; header_len + mask_len + ranges_len];
        let written_header = write_lerc2_header(header, &mut blob).unwrap();
        let written_mask =
            write_lerc2_mask(header, mask, encode_mask, &mut blob[written_header..]).unwrap();
        let written_ranges =
            write_lerc2_min_max_ranges(header, ranges, &mut blob[written_header + written_mask..])
                .unwrap();
        assert_eq!(written_header, header_len);
        assert_eq!(written_mask, mask_len);
        assert_eq!(written_ranges, ranges_len);
        blob
    }

    fn blob_with_written_one_sweep(
        header: &HeaderInfo,
        mask: &BitMask,
        encode_mask: bool,
        ranges: &MinMaxRanges,
        data: &[u8],
    ) -> Vec<u8> {
        let header_len = compute_lerc2_header_byte_len(header.version).unwrap();
        let mask_len = compute_lerc2_mask_byte_len(header, Some(mask), encode_mask).unwrap();
        let ranges_len = compute_lerc2_min_max_ranges_byte_len(header).unwrap();
        let payload_len = compute_lerc2_one_sweep_byte_len(header).unwrap();
        let mut blob = vec![0; header_len + mask_len + ranges_len + payload_len];
        let written_header = write_lerc2_header(header, &mut blob).unwrap();
        let written_mask =
            write_lerc2_mask(header, Some(mask), encode_mask, &mut blob[written_header..]).unwrap();
        let written_ranges =
            write_lerc2_min_max_ranges(header, ranges, &mut blob[written_header + written_mask..])
                .unwrap();
        let written_payload = write_lerc2_one_sweep(
            header,
            mask,
            data,
            &mut blob[written_header + written_mask + written_ranges..],
        )
        .unwrap();
        assert_eq!(written_header, header_len);
        assert_eq!(written_mask, mask_len);
        assert_eq!(written_ranges, ranges_len);
        assert_eq!(written_payload, payload_len);
        blob
    }

    fn blob_with_written_tiled_raw(
        header: &mut HeaderInfo,
        mask: &BitMask,
        ranges: &MinMaxRanges,
        data: &[u8],
    ) -> Vec<u8> {
        let header_len = compute_lerc2_header_byte_len(header.version).unwrap();
        let mask_len = compute_lerc2_mask_byte_len(header, Some(mask), true).unwrap();
        let ranges_len = compute_lerc2_min_max_ranges_byte_len(header).unwrap();
        let payload_len = compute_lerc2_tiled_raw_byte_len(header, mask).unwrap();
        header.blob_size = (header_len + mask_len + ranges_len + payload_len) as i32;

        let mut blob = vec![0; header.blob_size as usize];
        let written_header = write_lerc2_header(header, &mut blob).unwrap();
        let written_mask =
            write_lerc2_mask(header, Some(mask), true, &mut blob[written_header..]).unwrap();
        let written_ranges =
            write_lerc2_min_max_ranges(header, ranges, &mut blob[written_header + written_mask..])
                .unwrap();
        let written_payload = write_lerc2_tiled_raw(
            header,
            mask,
            data,
            &mut blob[written_header + written_mask + written_ranges..],
        )
        .unwrap();
        assert_eq!(written_header, header_len);
        assert_eq!(written_mask, mask_len);
        assert_eq!(written_ranges, ranges_len);
        assert_eq!(written_payload, payload_len);
        finalize_lerc2_checksum(&mut blob).unwrap();
        blob
    }

    fn synthetic_v4_blob(data_type: DataType, n_depth: i32, range_bytes: &[u8]) -> Vec<u8> {
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4 + range_bytes.len();
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [2, 3, n_depth, 6, 8, blob_size as i32, data_type as i32] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&(-10.0f64).to_le_bytes());
        blob.extend_from_slice(&1000.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(range_bytes);
        blob
    }

    fn synthetic_v4_one_sweep_blob(
        data_type: DataType,
        n_depth: i32,
        valid: &[u8],
        range_bytes: &[u8],
        payload: &[u8],
    ) -> Vec<u8> {
        let num_valid = valid.iter().filter(|&&value| value != 0).count();
        let encoded_mask = if num_valid > 0 && num_valid < valid.len() {
            let mask = crate::BitMask::from_byte_mask(valid, 3, 2).unwrap();
            Rle::compress(mask.bits()).unwrap()
        } else {
            Vec::new()
        };
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size =
            header_size + 4 + encoded_mask.len() + range_bytes.len() + 1 + payload.len();
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [
            2,
            3,
            n_depth,
            num_valid as i32,
            8,
            blob_size as i32,
            data_type as i32,
        ] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&(-10.0f64).to_le_bytes());
        blob.extend_from_slice(&1000.0f64.to_le_bytes());
        blob.extend_from_slice(&(encoded_mask.len() as i32).to_le_bytes());
        blob.extend_from_slice(&encoded_mask);
        blob.extend_from_slice(range_bytes);
        blob.push(1);
        blob.extend_from_slice(payload);
        set_lerc2_checksum(&mut blob);
        blob
    }

    fn synthetic_v4_one_sweep_blob_reusing_previous_mask(
        data_type: DataType,
        n_depth: i32,
        num_valid: i32,
        range_bytes: &[u8],
        payload: &[u8],
    ) -> Vec<u8> {
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4 + range_bytes.len() + 1 + payload.len();
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [
            2,
            3,
            n_depth,
            num_valid,
            8,
            blob_size as i32,
            data_type as i32,
        ] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&(-10.0f64).to_le_bytes());
        blob.extend_from_slice(&1000.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(range_bytes);
        blob.push(1);
        blob.extend_from_slice(payload);
        set_lerc2_checksum(&mut blob);
        blob
    }

    fn synthetic_v4_tiled_raw_blob(
        data_type: DataType,
        n_depth: i32,
        valid: &[u8],
        range_bytes: &[u8],
        tile_payloads: &[&[u8]],
    ) -> Vec<u8> {
        let mask = crate::BitMask::from_byte_mask(valid, 5, 3).unwrap();
        let encoded_mask = Rle::compress(mask.bits()).unwrap();
        let tile_bytes_len: usize = tile_payloads.iter().map(|payload| 1 + payload.len()).sum();
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size =
            header_size + 4 + encoded_mask.len() + range_bytes.len() + 1 + tile_bytes_len;
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [
            3,
            5,
            n_depth,
            valid.iter().filter(|&&value| value != 0).count() as i32,
            2,
            blob_size as i32,
            data_type as i32,
        ] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&(-10.0f64).to_le_bytes());
        blob.extend_from_slice(&1000.0f64.to_le_bytes());
        blob.extend_from_slice(&(encoded_mask.len() as i32).to_le_bytes());
        blob.extend_from_slice(&encoded_mask);
        blob.extend_from_slice(range_bytes);
        blob.push(0);
        for payload in tile_payloads {
            blob.push(0);
            blob.extend_from_slice(payload);
        }
        blob
    }

    fn synthetic_v4_tiled_block_blob(
        data_type: DataType,
        n_depth: i32,
        valid: &[u8],
        range_bytes: &[u8],
        blocks: &[Vec<u8>],
    ) -> Vec<u8> {
        let num_valid = valid.iter().filter(|&&value| value != 0).count();
        let encoded_mask = if num_valid > 0 && num_valid < valid.len() {
            let mask = crate::BitMask::from_byte_mask(valid, 5, 3).unwrap();
            Rle::compress(mask.bits()).unwrap()
        } else {
            Vec::new()
        };
        let tile_bytes_len: usize = blocks.iter().map(Vec::len).sum();
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size =
            header_size + 4 + encoded_mask.len() + range_bytes.len() + 1 + tile_bytes_len;
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [
            3,
            5,
            n_depth,
            num_valid as i32,
            2,
            blob_size as i32,
            data_type as i32,
        ] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&10.0f64.to_le_bytes());
        blob.extend_from_slice(&24.0f64.to_le_bytes());
        blob.extend_from_slice(&(encoded_mask.len() as i32).to_le_bytes());
        blob.extend_from_slice(&encoded_mask);
        blob.extend_from_slice(range_bytes);
        blob.push(0);
        for block in blocks {
            blob.extend_from_slice(block);
        }
        blob
    }

    fn synthetic_v5_diff_tiled_blob(diff_block: Vec<u8>) -> Vec<u8> {
        let range_bytes = [10u8, 15, 40, 48];
        let depth0_block = {
            let mut block = vec![0];
            block.extend_from_slice(&[10, 20, 30, 40]);
            block
        };
        let blocks = [depth0_block, diff_block];
        let tile_bytes_len: usize = blocks.iter().map(Vec::len).sum();
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4 + range_bytes.len() + 1 + tile_bytes_len;
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&5i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [2, 2, 2, 4, 2, blob_size as i32, DataType::UChar as i32] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&10.0f64.to_le_bytes());
        blob.extend_from_slice(&48.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&range_bytes);
        blob.push(0);
        for block in blocks {
            blob.extend_from_slice(&block);
        }
        blob
    }

    fn synthetic_v5_float_diff_tiled_blob(diff_block: Vec<u8>) -> Vec<u8> {
        let mut range_bytes = Vec::new();
        for value in [10.0f32, 11.5, 40.0, 42.0] {
            range_bytes.extend_from_slice(&value.to_le_bytes());
        }
        let depth0_block = {
            let mut block = vec![0];
            for value in [10.0f32, 20.0, 30.0, 40.0] {
                block.extend_from_slice(&value.to_le_bytes());
            }
            block
        };
        let blocks = [depth0_block, diff_block];
        let tile_bytes_len: usize = blocks.iter().map(Vec::len).sum();
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4 + range_bytes.len() + 1 + tile_bytes_len;
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&5i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [2, 2, 2, 4, 2, blob_size as i32, DataType::Float as i32] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.25f64.to_le_bytes());
        blob.extend_from_slice(&10.0f64.to_le_bytes());
        blob.extend_from_slice(&42.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&range_bytes);
        blob.push(0);
        for block in blocks {
            blob.extend_from_slice(&block);
        }
        blob
    }

    fn synthetic_v4_ushort_tiled_raw_blob() -> Vec<u8> {
        let values = [100u16, 200, 300, 400];
        let mut payload = vec![0u8];
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }

        let mut range_bytes = Vec::new();
        range_bytes.extend_from_slice(&100u16.to_le_bytes());
        range_bytes.extend_from_slice(&400u16.to_le_bytes());

        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4 + range_bytes.len() + 1 + payload.len();
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [2, 2, 1, 4, 2, blob_size as i32, DataType::UShort as i32] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&100.0f64.to_le_bytes());
        blob.extend_from_slice(&400.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&range_bytes);
        blob.push(0);
        blob.extend_from_slice(&payload);
        set_lerc2_checksum(&mut blob);
        blob
    }

    fn synthetic_v4_const_blob() -> Vec<u8> {
        let header_size = FILE_KEY.len() + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4;
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [2, 2, 1, 4, 2, blob_size as i32, DataType::UChar as i32] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&7.0f64.to_le_bytes());
        blob.extend_from_slice(&7.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        set_lerc2_checksum(&mut blob);
        blob
    }

    fn synthetic_v6_uchar_one_sweep_no_data_blob() -> Vec<u8> {
        let n_rows = 2i32;
        let n_cols = 3i32;
        let n_depth = 2i32;
        let num_valid = n_rows * n_cols;
        let range_bytes = [1u8, 1, 99, 99];
        let payload = [1u8, 99, 2, 3, 99, 4, 5, 6, 7, 99, 8, 9];
        let header_size = FILE_KEY.len() + 4 + 4 + 8 * 4 + 4 + 5 * 8;
        let blob_size = header_size + 4 + range_bytes.len() + 1 + payload.len();
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(FILE_KEY);
        blob.extend_from_slice(&6i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [
            n_rows,
            n_cols,
            n_depth,
            num_valid,
            8,
            blob_size as i32,
            DataType::UChar as i32,
            0,
        ] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&[1, 1, 0, 0]);
        blob.extend_from_slice(&0.5f64.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&99.0f64.to_le_bytes());
        blob.extend_from_slice(&99.0f64.to_le_bytes());
        blob.extend_from_slice(&255.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&range_bytes);
        blob.push(1);
        blob.extend_from_slice(&payload);
        set_lerc2_checksum(&mut blob);
        blob
    }

    fn set_lerc2_checksum(blob: &mut [u8]) {
        let checksum = compute_checksum_fletcher32(&blob[14..]);
        blob[10..14].copy_from_slice(&checksum.to_le_bytes());
    }

    fn bit_stuffed_tile_block(offset: u8, quantized: &[u32]) -> Vec<u8> {
        let mut block = vec![1, offset];
        block.extend_from_slice(&BitStuffer2::encode_simple(quantized, 4).unwrap());
        block
    }

    fn lut_tile_block(offset: u8, quantized: &[u32]) -> Vec<u8> {
        let mut sorted: Vec<(u32, u32)> = quantized
            .iter()
            .enumerate()
            .map(|(idx, &value)| (value, idx as u32))
            .collect();
        sorted.sort_unstable();

        let mut block = vec![1, offset];
        block.extend_from_slice(&BitStuffer2::encode_lut(&sorted, 4).unwrap());
        block
    }

    fn diff_bit_stuffed_tile_block(offset: i16, quantized: &[u32]) -> Vec<u8> {
        let mut block = vec![(2 << 6) | 4 | 1];
        block.extend_from_slice(&offset.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_simple(quantized, 5).unwrap());
        block
    }

    fn diff_lut_tile_block(offset: i16, quantized: &[u32]) -> Vec<u8> {
        let mut sorted: Vec<(u32, u32)> = quantized
            .iter()
            .enumerate()
            .map(|(idx, &value)| (value, idx as u32))
            .collect();
        sorted.sort_unstable();

        let mut block = vec![(2 << 6) | 4 | 1];
        block.extend_from_slice(&offset.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_lut(&sorted, 5).unwrap());
        block
    }

    fn diff_constant_tile_block(offset: i16) -> Vec<u8> {
        let mut block = vec![(2 << 6) | 4 | 3];
        block.extend_from_slice(&offset.to_le_bytes());
        block
    }

    fn diff_float_constant_tile_block(offset: f32) -> Vec<u8> {
        let mut block = vec![4 | 3];
        block.extend_from_slice(&offset.to_le_bytes());
        block
    }

    fn diff_float_bit_stuffed_tile_block(offset: f32, quantized: &[u32]) -> Vec<u8> {
        let mut block = vec![4 | 1];
        block.extend_from_slice(&offset.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_simple(quantized, 5).unwrap());
        block
    }

    fn diff_float_lut_tile_block(offset: f32, quantized: &[u32]) -> Vec<u8> {
        let mut sorted: Vec<(u32, u32)> = quantized
            .iter()
            .enumerate()
            .map(|(idx, &value)| (value, idx as u32))
            .collect();
        sorted.sort_unstable();

        let mut block = vec![4 | 1];
        block.extend_from_slice(&offset.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_lut(&sorted, 5).unwrap());
        block
    }

    #[test]
    fn parses_single_band_float_fixture_header() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let probe = get_lerc2_header_info(&blob).unwrap();
        let header = probe.header;

        assert!(probe.has_mask);
        assert_eq!(header.version, 3);
        assert_eq!(header.checksum, 0x2efd_91bb);
        assert_eq!(header.n_rows, 400);
        assert_eq!(header.n_cols, 400);
        assert_eq!(header.n_depth, 1);
        assert_eq!(header.num_valid_pixel, 58_515);
        assert_eq!(header.micro_block_size, 8);
        assert_eq!(header.blob_size as usize, blob.len());
        assert_eq!(header.data_type, DataType::Float);
        assert_eq!(header.header_size, 62);
        assert_eq!(header.max_z_error, 0.000_075);
        assert_eq!(header.z_min, -82.972_091_674_804_69);
        assert_eq!(header.z_max, 4080.613_769_531_25);
    }

    #[test]
    fn computes_lerc2_header_byte_lengths() {
        assert_eq!(compute_lerc2_header_byte_len(2).unwrap(), 58);
        assert_eq!(compute_lerc2_header_byte_len(3).unwrap(), 62);
        assert_eq!(compute_lerc2_header_byte_len(4).unwrap(), 66);
        assert_eq!(compute_lerc2_header_byte_len(6).unwrap(), 90);
        assert_eq!(
            compute_lerc2_header_byte_len(7).unwrap_err(),
            LercError::WrongParam("unsupported Lerc2 header version")
        );
    }

    #[test]
    fn writes_lerc2_v3_header_for_parser_round_trip() {
        let header = header_for_write(3);
        let mut blob = vec![0; header.header_size + 4];
        let written = write_lerc2_header(&header, &mut blob).unwrap();
        blob[written..written + 4].copy_from_slice(&0i32.to_le_bytes());

        assert_eq!(written, header.header_size);
        let probe = get_lerc2_header_info(&blob).unwrap();
        assert!(!probe.has_mask);
        assert_eq!(probe.header, header);
    }

    #[test]
    fn writes_lerc2_v6_header_for_parser_round_trip() {
        let header = header_for_write(6);
        let mut blob = vec![0; header.header_size + 4];
        let written = write_lerc2_header(&header, &mut blob).unwrap();
        blob[written..written + 4].copy_from_slice(&0i32.to_le_bytes());

        assert_eq!(written, header.header_size);
        let probe = get_lerc2_header_info(&blob).unwrap();
        assert!(!probe.has_mask);
        assert_eq!(probe.header, header);
    }

    #[test]
    fn rejects_invalid_lerc2_header_write_inputs() {
        let header = header_for_write(4);
        assert_eq!(
            write_lerc2_header(&header, &mut [0; 8]).unwrap_err(),
            LercError::BufferTooSmall
        );

        let bad_depth = HeaderInfo {
            version: 3,
            n_depth: 2,
            ..header_for_write(3)
        };
        let mut output = [0; 86];
        assert_eq!(
            write_lerc2_header(&bad_depth, &mut output).unwrap_err(),
            LercError::WrongParam("pre-v4 Lerc2 headers can only store depth 1")
        );

        let bad_dims = HeaderInfo {
            n_rows: 0,
            ..header
        };
        assert_eq!(
            write_lerc2_header(&bad_dims, &mut output).unwrap_err(),
            LercError::WrongParam("invalid Lerc2 header dimensions")
        );
    }

    #[test]
    fn writes_lerc2_partial_mask_for_parser_round_trip() {
        let header = header_for_write(4);
        let byte_mask = [1, 0, 1, 1, 1, 1];
        let mask = BitMask::from_byte_mask(&byte_mask, 3, 2).unwrap();
        let blob = blob_with_written_header_and_mask(&header, Some(&mask), true);

        assert!(blob.len() > header.header_size + 4);
        let (_, mask_info) = read_lerc2_mask(&blob).unwrap();
        assert_eq!(mask_info.mask, mask);
        assert_eq!(
            mask_info.num_bytes_mask as usize,
            blob.len() - header.header_size - 4
        );
        assert_eq!(mask_info.bytes_consumed, blob.len());
    }

    #[test]
    fn writes_lerc2_zero_mask_sections_for_trivial_masks() {
        let mut all_valid = header_for_write(4);
        all_valid.num_valid_pixel = all_valid.n_cols * all_valid.n_rows;
        all_valid.blob_size = (all_valid.header_size + 4) as i32;
        let blob = blob_with_written_header_and_mask(&all_valid, None, true);
        let (_, mask_info) = read_lerc2_mask(&blob).unwrap();
        assert_eq!(blob.len(), all_valid.header_size + 4);
        assert_eq!(mask_info.num_bytes_mask, 0);
        assert_eq!(mask_info.mask.count_valid_bits(), 6);

        let mut all_invalid = all_valid;
        all_invalid.num_valid_pixel = 0;
        let blob = blob_with_written_header_and_mask(&all_invalid, None, true);
        let (_, mask_info) = read_lerc2_mask(&blob).unwrap();
        assert_eq!(blob.len(), all_invalid.header_size + 4);
        assert_eq!(mask_info.num_bytes_mask, 0);
        assert_eq!(mask_info.mask.count_valid_bits(), 0);
    }

    #[test]
    fn writes_lerc2_omitted_partial_mask_for_previous_reuse() {
        let header = header_for_write(4);
        let previous = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 1], 3, 2).unwrap();
        let blob = blob_with_written_header_and_mask(&header, Some(&previous), false);

        assert_eq!(blob.len(), header.header_size + 4);
        let (_, mask_info) = read_lerc2_mask_with_previous(&blob, Some(&previous)).unwrap();
        assert_eq!(mask_info.mask, previous);
        assert_eq!(mask_info.num_bytes_mask, 0);
    }

    #[test]
    fn rejects_invalid_lerc2_mask_write_inputs() {
        let header = header_for_write(4);
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 1], 3, 2).unwrap();
        assert_eq!(
            write_lerc2_mask(&header, Some(&mask), true, &mut [0; 3]).unwrap_err(),
            LercError::BufferTooSmall
        );
        assert_eq!(
            write_lerc2_mask(&header, None, true, &mut [0; 16]).unwrap_err(),
            LercError::WrongParam("partial Lerc2 mask is required when encode_mask is true")
        );

        let wrong_dims = BitMask::from_byte_mask(&[1, 1, 1, 1], 2, 2).unwrap();
        assert_eq!(
            write_lerc2_mask(&header, Some(&wrong_dims), true, &mut [0; 16]).unwrap_err(),
            LercError::WrongParam("Lerc2 mask dimensions do not match header")
        );

        let wrong_count = BitMask::from_byte_mask(&[1, 1, 1, 1, 1, 1], 3, 2).unwrap();
        assert_eq!(
            write_lerc2_mask(&header, Some(&wrong_count), true, &mut [0; 16]).unwrap_err(),
            LercError::WrongParam("Lerc2 mask valid count does not match header")
        );
    }

    #[test]
    fn reads_single_band_float_fixture_mask() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let (header, mask_info) = read_lerc2_mask(&blob).unwrap();

        assert_eq!(header.num_valid_pixel, 58_515);
        assert!(mask_info.num_bytes_mask > 0);
        assert_eq!(
            mask_info.bytes_consumed,
            header.header_size + 4 + mask_info.num_bytes_mask as usize
        );
        assert_eq!(mask_info.mask.cols(), 400);
        assert_eq!(mask_info.mask.rows(), 400);
        assert_eq!(
            mask_info.mask.count_valid_bits() as i32,
            header.num_valid_pixel
        );
    }

    #[test]
    fn aggregates_concatenated_byte_fixture_info() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let info = get_lerc_info(&blob).unwrap();

        assert_eq!(info.version, 3);
        assert_eq!(info.n_rows, 256);
        assert_eq!(info.n_cols, 256);
        assert_eq!(info.n_depth, 1);
        assert_eq!(info.n_bands, 3);
        assert_eq!(info.blob_size as usize, blob.len());
        assert_eq!(info.data_type, DataType::UChar);
        assert_eq!(info.n_masks, 1);
        assert_eq!(info.n_uses_no_data_value, 0);
        assert_eq!(info.num_valid_pixel, 43_008);
        assert_eq!(info.max_z_error, 0.5);
        assert_eq!(info.z_min, 0.0);
        assert_eq!(info.z_max, 255.0);
    }

    #[test]
    fn reports_legacy_lerc1_fixture_info() {
        let blob = fixture("world.lerc1");
        let info = get_lerc_info(&blob).unwrap();

        assert_eq!(info.version, 0);
        assert_eq!(info.n_rows, 257);
        assert_eq!(info.n_cols, 257);
        assert_eq!(info.n_depth, 1);
        assert_eq!(info.n_bands, 1);
        assert_eq!(info.blob_size as usize, blob.len());
        assert_eq!(info.data_type, DataType::Float);
        assert_eq!(info.n_masks, 1);
        assert_eq!(info.n_uses_no_data_value, 0);
        assert_eq!(info.num_valid_pixel, 65_025);
        assert_eq!(info.max_z_error, 0.1);
        assert_eq!(info.z_min, -27.458_635_330_200_195);
        assert_eq!(info.z_max, 5474.172_851_562_5);
    }

    #[test]
    fn fills_c_api_style_blob_info_arrays() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut info_array = [123u32; BLOB_INFO_ARRAY_LEN + 2];
        let mut range_array = [123.0f64; BLOB_DATA_RANGE_ARRAY_LEN + 2];
        let info = get_lerc2_blob_info_arrays(&blob, Some(&mut info_array), Some(&mut range_array))
            .unwrap();

        assert_eq!(info.n_bands, 3);
        assert_eq!(
            &info_array[..BLOB_INFO_ARRAY_LEN],
            &[3, 1, 1, 256, 256, 3, 43_008, blob.len() as u32, 1, 1, 0]
        );
        assert_eq!(&info_array[BLOB_INFO_ARRAY_LEN..], &[0, 0]);
        assert_eq!(
            &range_array[..BLOB_DATA_RANGE_ARRAY_LEN],
            &[0.0, 255.0, 0.5]
        );
        assert_eq!(&range_array[BLOB_DATA_RANGE_ARRAY_LEN..], &[0.0, 0.0]);
    }

    #[test]
    fn fills_legacy_lerc1_blob_info_arrays() {
        let blob = fixture("world.lerc1");
        let mut info_array = [123u32; BLOB_INFO_ARRAY_LEN];
        let mut range_array = [123.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
        let info = get_lerc2_blob_info_arrays(&blob, Some(&mut info_array), Some(&mut range_array))
            .unwrap();

        assert_eq!(info.version, 0);
        assert_eq!(
            info_array,
            [
                0,
                DataType::Float as u32,
                1,
                257,
                257,
                1,
                65_025,
                blob.len() as u32,
                1,
                1,
                0,
            ]
        );
        assert_eq!(
            range_array,
            [-27.458_635_330_200_195, 5474.172_851_562_5, 0.1]
        );
    }

    #[test]
    fn fills_float_fixture_blob_info_arrays() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let mut info_array = [123u32; BLOB_INFO_ARRAY_LEN];
        let mut range_array = [123.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
        let info = get_lerc2_blob_info_arrays(&blob, Some(&mut info_array), Some(&mut range_array))
            .unwrap();

        assert_eq!(info.n_bands, 1);
        assert_eq!(
            info_array,
            [
                3,
                DataType::Float as u32,
                1,
                400,
                400,
                1,
                58_515,
                blob.len() as u32,
                1,
                1,
                0,
            ]
        );
        assert_eq!(
            range_array,
            [-82.972_091_674_804_69, 4080.613_769_531_25, 0.000_075]
        );
    }

    #[test]
    fn fills_partial_blob_info_arrays_and_no_data_range_sentinels() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let mut info_array = [123u32; 4];
        let mut range_array = [123.0f64; 2];

        let info = get_lerc2_blob_info_arrays(&blob, Some(&mut info_array), Some(&mut range_array))
            .unwrap();

        assert_eq!(info.n_uses_no_data_value, 1);
        assert_eq!(info_array, [6, DataType::UChar as u32, 2, 3]);
        assert_eq!(range_array, [-1.0, -1.0]);
    }

    #[test]
    fn reads_each_concatenated_byte_fixture_mask() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut offset = 0usize;
        let mut valid_counts = Vec::new();
        let mut previous_mask = None;

        while offset < blob.len() {
            let (header, mask_info) =
                read_lerc2_mask_with_previous(&blob[offset..], previous_mask.as_ref()).unwrap();
            assert_eq!(mask_info.mask.cols(), 256);
            assert_eq!(mask_info.mask.rows(), 256);
            assert_eq!(
                mask_info.mask.count_valid_bits() as i32,
                header.num_valid_pixel
            );
            assert!(mask_info.bytes_consumed <= header.blob_size as usize);
            valid_counts.push(header.num_valid_pixel);
            offset += header.blob_size as usize;
            previous_mask = Some(mask_info.mask);
        }

        assert_eq!(offset, blob.len());
        assert_eq!(valid_counts, [43_008, 43_008, 43_008]);
    }

    #[test]
    fn validates_v3_fixture_checksums() {
        let california = fixture("california_400_400_1_float.lerc2");
        let header = validate_lerc2_checksum(&california).unwrap();
        assert_eq!(
            header.checksum,
            compute_checksum_fletcher32(&california[14..])
        );

        let bluemarble = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut offset = 0usize;
        let mut checksums = Vec::new();
        while offset < bluemarble.len() {
            let header = validate_lerc2_checksum(&bluemarble[offset..]).unwrap();
            checksums.push(header.checksum);
            offset += header.blob_size as usize;
        }

        assert_eq!(offset, bluemarble.len());
        assert_eq!(checksums.len(), 3);
        assert_eq!(checksums, [0x86ce_e665, 0x2419_e3dc, 0x5e2d_64e2]);
    }

    #[test]
    fn finalizes_lerc2_checksum_for_written_blob() {
        let mut header = header_for_write(4);
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 1], 3, 2).unwrap();
        let mask_len = compute_lerc2_mask_byte_len(&header, Some(&mask), true).unwrap();
        header.blob_size = (header.header_size + mask_len) as i32;
        header.checksum = 0;
        let mut blob = blob_with_written_header_and_mask(&header, Some(&mask), true);

        assert!(validate_lerc2_checksum(&blob).is_err());
        let checksum = finalize_lerc2_checksum(&mut blob).unwrap();
        assert_eq!(checksum, compute_checksum_fletcher32(&blob[14..]));
        assert_eq!(validate_lerc2_checksum(&blob).unwrap().checksum, checksum);
    }

    #[test]
    fn finalizing_pre_v3_lerc2_blob_leaves_checksum_absent() {
        let mut header = header_for_write(2);
        header.num_valid_pixel = header.n_cols * header.n_rows;
        let mut blob = blob_with_written_header_and_mask(&header, None, true);
        let before = blob.clone();

        assert_eq!(finalize_lerc2_checksum(&mut blob).unwrap(), 0);
        assert_eq!(blob, before);
        assert_eq!(validate_lerc2_checksum(&blob).unwrap().version, 2);
    }

    #[test]
    fn maps_floating_point_predictor_codes_like_cpp() {
        assert_eq!(FpPredictor::from_code(0), Some(FpPredictor::None));
        assert_eq!(FpPredictor::from_code(1), Some(FpPredictor::Delta1));
        assert_eq!(FpPredictor::from_code(2), Some(FpPredictor::RowsCols));
        assert_eq!(FpPredictor::from_code(3), None);

        assert_eq!(FpPredictor::None.code(), 0);
        assert_eq!(FpPredictor::Delta1.code(), 1);
        assert_eq!(FpPredictor::RowsCols.code(), 2);
        assert_eq!(FpPredictor::None.int_delta(), 0);
        assert_eq!(FpPredictor::Delta1.int_delta(), 1);
        assert_eq!(FpPredictor::RowsCols.int_delta(), 2);
        assert_eq!(FpPredictor::None.max_byte_delta(), 5);
        assert_eq!(FpPredictor::Delta1.max_byte_delta(), 4);
        assert_eq!(FpPredictor::RowsCols.max_byte_delta(), 3);

        assert_eq!(
            FpPredictor::from_delta_and_cross(0, false),
            Some(FpPredictor::None)
        );
        assert_eq!(
            FpPredictor::from_delta_and_cross(0, true),
            Some(FpPredictor::None)
        );
        assert_eq!(
            FpPredictor::from_delta_and_cross(1, false),
            Some(FpPredictor::Delta1)
        );
        assert_eq!(
            FpPredictor::from_delta_and_cross(2, true),
            Some(FpPredictor::RowsCols)
        );
        assert_eq!(FpPredictor::from_delta_and_cross(1, true), None);
        assert_eq!(FpPredictor::from_delta_and_cross(2, false), None);
        assert_eq!(FpPredictor::from_delta_and_cross(-1, false), None);
    }

    #[test]
    fn restores_floating_point_huffman_byte_delta_sequences() {
        assert_eq!(
            restore_fp_byte_delta_sequence(&[5, 2, 250, 4], 0).unwrap(),
            [5, 2, 250, 4]
        );
        assert_eq!(
            restore_fp_byte_delta_sequence(&[5, 2, 250, 4], 1).unwrap(),
            [5, 7, 1, 5]
        );
        assert_eq!(
            restore_fp_byte_delta_sequence(&[5, 2, 250, 4], 2).unwrap(),
            [5, 7, 3, 3]
        );
        assert_eq!(
            restore_fp_byte_delta_sequence(&[1, 2, 3, 4, 5], 5).unwrap(),
            [1, 3, 8, 20, 48]
        );
        assert_eq!(
            restore_fp_byte_delta_sequence(&[1, 2, 3], 6).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman byte delta level is invalid")
        );
    }

    fn encode_fpl_normal_huffman_payload(data: &[u8]) -> Vec<u8> {
        let mut histo = [0usize; 256];
        for &value in data {
            histo[value as usize] += 1;
        }
        let table = compute_huffman_encode_table(&histo).unwrap().unwrap();
        let mut out = vec![0];
        write_huffman_code_table(&table, 5, &mut out).unwrap();
        let mut bits = HuffmanBitWriter::new();
        for &value in data {
            let (len, code) = table.codes[value as usize];
            bits.push_bits(code, len).unwrap();
        }
        out.extend_from_slice(&bits.finish(true));
        out
    }

    #[test]
    fn extracts_floating_point_huffman_wrapped_rle_raw_and_packbits_payloads() {
        let mut rle = vec![1, 0xab];
        rle.extend_from_slice(&5u32.to_le_bytes());
        assert_eq!(extract_fpl_compressed_buffer(&rle, 5).unwrap(), [0xab; 5]);

        let raw = [2, 9, 8, 7, 6];
        assert_eq!(
            extract_fpl_compressed_buffer(&raw, 4).unwrap(),
            [9, 8, 7, 6]
        );

        let packbits = [3, 2, 1, 2, 3, 130, 4, 0, 5];
        assert_eq!(
            extract_fpl_compressed_buffer(&packbits, 7).unwrap(),
            [1, 2, 3, 4, 4, 4, 5]
        );
    }

    #[test]
    fn extracts_floating_point_huffman_wrapped_normal_payload() {
        let values = [4, 7, 4, 9, 9, 9, 7, 4, 12, 12, 9];
        let encoded = encode_fpl_normal_huffman_payload(&values);

        assert_eq!(
            extract_fpl_compressed_buffer(&encoded, values.len()).unwrap(),
            values
        );
    }

    #[test]
    fn rejects_malformed_floating_point_huffman_wrapped_payloads() {
        assert_eq!(
            extract_fpl_compressed_buffer(&[4], 0).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman payload mode is invalid")
        );
        assert_eq!(
            extract_fpl_compressed_buffer(&[1, 7, 2, 0, 0, 0], 3).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman RLE count mismatch")
        );
        assert_eq!(
            extract_fpl_compressed_buffer(&[2, 1, 2], 3).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman raw payload length mismatch")
        );
        assert_eq!(
            extract_fpl_compressed_buffer(&[3, 4, 1, 2], 5).unwrap_err(),
            LercError::BufferTooSmall
        );
        assert_eq!(
            extract_fpl_compressed_buffer(&[3, 130, 1], 2).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman PackBits output is too long")
        );
    }

    fn append_fpl_raw_plane(slice: &mut Vec<u8>, byte_index: u8, byte_delta: u8, bytes: &[u8]) {
        slice.push(byte_index);
        slice.push(byte_delta);
        slice.extend_from_slice(&((bytes.len() + 1) as u32).to_le_bytes());
        slice.push(2);
        slice.extend_from_slice(bytes);
    }

    #[test]
    fn reads_floating_point_huffman_slice_from_wrapped_byte_planes() {
        let transformed = [0x7f00_0000u32, 0x8020_0000];
        let mut slice = vec![FpPredictor::None.code()];
        for byte_index in [2u8, 0, 3, 1] {
            let plane = transformed
                .iter()
                .map(|value| value.to_le_bytes()[byte_index as usize])
                .collect::<Vec<_>>();
            append_fpl_raw_plane(&mut slice, byte_index, 0, &plane);
        }

        let mut reader = Reader::new(&slice);
        let restored = read_fp_huffman_slice(&mut reader, DataType::Float, 2, 1).unwrap();
        let values: Vec<f32> = restored
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        assert_eq!(values, [1.0, 2.5]);
        assert_eq!(reader.pos, slice.len());
    }

    #[test]
    fn decodes_supported_lerc2_floating_point_huffman_blob() {
        let transformed = [0x7f00_0000u32, 0x8020_0000];
        let mut slice = vec![FpPredictor::None.code()];
        for byte_index in 0..4u8 {
            let plane = transformed
                .iter()
                .map(|value| value.to_le_bytes()[byte_index as usize])
                .collect::<Vec<_>>();
            append_fpl_raw_plane(&mut slice, byte_index, 0, &plane);
        }
        let mut payload = vec![0, 3];
        payload.extend_from_slice(&slice);

        let header_size = compute_lerc2_header_byte_len(6).unwrap();
        let mut header = HeaderInfo {
            version: 6,
            checksum: 0,
            n_rows: 1,
            n_cols: 2,
            n_depth: 1,
            num_valid_pixel: 2,
            micro_block_size: 8,
            blob_size: 0,
            n_blobs_more: 0,
            b_pass_no_data_values: 0,
            b_is_int: 0,
            b_reserved_3: 0,
            b_reserved_4: 0,
            data_type: DataType::Float,
            max_z_error: 0.0,
            z_min: 1.0,
            z_max: 2.5,
            no_data_val: 0.0,
            no_data_val_orig: 0.0,
            header_size,
        };
        let mask_len = compute_lerc2_mask_byte_len(&header, None, true).unwrap();
        let ranges = MinMaxRanges {
            mins: vec![1.0],
            maxs: vec![2.5],
            bytes_consumed: 8,
            min_max_equal: false,
        };
        let ranges_len = compute_lerc2_min_max_ranges_byte_len(&header).unwrap();
        header.blob_size = (header_size + mask_len + ranges_len + payload.len()) as i32;
        let mut blob = blob_with_written_header_mask_and_ranges(&header, None, true, &ranges);
        blob.extend_from_slice(&payload);
        finalize_lerc2_checksum(&mut blob).unwrap();

        let decoded = decode_lerc2_supported(&blob).unwrap();
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.data, DecodedData::Float(vec![1.0, 2.5]));
    }

    #[test]
    fn rejects_malformed_floating_point_huffman_slice_headers() {
        let mut invalid_predictor = vec![3];
        for byte_index in 0..4 {
            append_fpl_raw_plane(&mut invalid_predictor, byte_index, 0, &[0]);
        }
        let mut reader = Reader::new(&invalid_predictor);
        assert_eq!(
            read_fp_huffman_slice(&mut reader, DataType::Float, 1, 1).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman predictor code is invalid")
        );

        let mut invalid_delta = vec![FpPredictor::None.code()];
        append_fpl_raw_plane(&mut invalid_delta, 0, 6, &[0]);
        let mut reader = Reader::new(&invalid_delta);
        assert_eq!(
            read_fp_huffman_slice(&mut reader, DataType::Float, 1, 1).unwrap_err(),
            LercError::CorruptInput("floating-point Huffman byte delta level is invalid")
        );

        let mut short_payload = vec![FpPredictor::None.code(), 0, 0];
        short_payload.extend_from_slice(&3u32.to_le_bytes());
        short_payload.extend_from_slice(&[2, 1]);
        let mut reader = Reader::new(&short_payload);
        assert_eq!(
            read_fp_huffman_slice(&mut reader, DataType::Float, 1, 1).unwrap_err(),
            LercError::BufferTooSmall
        );
    }

    #[test]
    fn restores_floating_point_huffman_byte_order_without_prediction() {
        let transformed = [0x7f00_0000u32, 0x8020_0000];
        let planes = vec![
            (
                2,
                transformed
                    .iter()
                    .map(|value| value.to_le_bytes()[2])
                    .collect(),
            ),
            (
                0,
                transformed
                    .iter()
                    .map(|value| value.to_le_bytes()[0])
                    .collect(),
            ),
            (
                3,
                transformed
                    .iter()
                    .map(|value| value.to_le_bytes()[3])
                    .collect(),
            ),
            (
                1,
                transformed
                    .iter()
                    .map(|value| value.to_le_bytes()[1])
                    .collect(),
            ),
        ];
        let restored =
            restore_fp_bytes_from_planes(&planes, DataType::Float, 2, 1, FpPredictor::None)
                .unwrap();
        let values: Vec<f32> = restored
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        assert_eq!(values, [1.0, 2.5]);
    }

    #[test]
    fn restores_floating_point_huffman_byte_order_with_row_delta() {
        let values = vec![
            0x3ff0_0000_0000_0000u64,
            0x0010_0000_0000_0001,
            0x0000_0000_0000_0002,
        ];
        let mut planes = Vec::new();
        for byte_index in 0..8 {
            planes.push((
                byte_index,
                values
                    .iter()
                    .map(|value| value.to_le_bytes()[byte_index])
                    .collect::<Vec<_>>(),
            ));
        }
        let restored =
            restore_fp_bytes_from_planes(&planes, DataType::Double, 3, 1, FpPredictor::Delta1)
                .unwrap();
        let restored_values: Vec<u64> = restored
            .chunks_exact(8)
            .map(|chunk| {
                u64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect();

        assert_eq!(
            restored_values,
            [
                0x3ff0_0000_0000_0000,
                0x4000_0000_0000_0001,
                0x4000_0000_0000_0003,
            ]
        );
    }

    #[test]
    fn restores_floating_point_huffman_byte_order_with_cross_delta() {
        let values = vec![
            0x3ff0_0000_0000_0000u64,
            0x0010_0000_0000_0001,
            0x0010_0000_0000_0002,
            0x0000_0000_0000_0003,
        ];
        let mut planes = Vec::new();
        for byte_index in 0..8 {
            planes.push((
                byte_index,
                values
                    .iter()
                    .map(|value| value.to_le_bytes()[byte_index])
                    .collect::<Vec<_>>(),
            ));
        }
        let restored =
            restore_fp_bytes_from_planes(&planes, DataType::Double, 2, 2, FpPredictor::RowsCols)
                .unwrap();
        let restored_values: Vec<u64> = restored
            .chunks_exact(8)
            .map(|chunk| {
                u64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect();

        assert_eq!(
            restored_values,
            [
                0x3ff0_0000_0000_0000,
                0x4000_0000_0000_0001,
                0x4000_0000_0000_0002,
                0x4010_0000_0000_0006,
            ]
        );
        assert_eq!(
            restore_fp_bytes_from_planes(&planes[..3], DataType::Double, 2, 2, FpPredictor::None)
                .unwrap_err(),
            LercError::CorruptInput("floating-point Huffman byte-plane count mismatch")
        );
    }

    #[test]
    fn encodes_constant_lerc2_blob_all_valid() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let blob = encode_lerc2_constant(spec, 7.0, 0.5, None, 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.header.n_depth, 2);
        assert_eq!(decoded.header.num_valid_pixel, 6);
        assert_eq!(decoded.header.z_min, 7.0);
        assert_eq!(decoded.header.z_max, 7.0);
        assert_eq!(decoded.mask.count_valid_bits(), 6);
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(
            validate_lerc2_checksum(&blob).unwrap().blob_size as usize,
            blob.len()
        );
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![7; spec.n_cols * spec.n_rows * spec.n_depth])
        );
    }

    #[test]
    fn encodes_constant_lerc2_blob_with_partial_mask() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        let blob = encode_lerc2_constant(spec, -2.5, 0.0, Some(&mask), 4).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 4);
        assert_eq!(decoded.header.num_valid_pixel, 4);
        assert_eq!(decoded.mask, mask);
        assert_eq!(
            decoded.data,
            DecodedData::Float(vec![-2.5, 0.0, -2.5, -2.5, -2.5, 0.0])
        );
        assert_eq!(
            validate_lerc2_checksum(&blob).unwrap().checksum,
            decoded.header.checksum
        );
    }

    #[test]
    fn rejects_invalid_constant_lerc2_encode_inputs() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let negative = encode_lerc2_constant(spec, 1.0, -1.0, None, 6).unwrap();
        assert_eq!(
            get_lerc2_header_info(&negative).unwrap().header.max_z_error,
            0.5
        );
        assert_eq!(
            encode_lerc2_constant(spec, 1.0, 0.0, None, 3).unwrap_err(),
            LercError::WrongParam("pre-v4 Lerc2 encode can only store depth 1")
        );

        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        assert_eq!(
            encode_lerc2_constant(spec, 1.0, 0.0, Some(&mask), 6).unwrap_err(),
            LercError::WrongParam(
                "constant Lerc2 encode mask count must be nonzero when a mask is supplied"
            )
        );

        let multi_band = EncodeSpec {
            n_bands: 2,
            n_masks: 1,
            ..spec
        };
        assert_eq!(
            encode_lerc2_constant(multi_band, 1.0, 0.0, None, 6).unwrap_err(),
            LercError::WrongParam("constant Lerc2 encode currently supports one band")
        );
    }

    #[test]
    fn computes_lerc2_encode_ranges_from_valid_pixels() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        let data = [1u8, 2, 99, 99, 3, 9, 5, 6, 7, 8, 0, 0];
        let ranges = compute_lerc2_data_ranges_for_encode(spec, &data, Some(&mask)).unwrap();

        assert_eq!(ranges.mins, [1.0, 2.0]);
        assert_eq!(ranges.maxs, [7.0, 9.0]);
        assert!(!ranges.min_max_equal);
    }

    #[test]
    fn encodes_byte_huffman_lerc2_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 8,
            n_rows: 4,
            n_bands: 1,
            n_masks: 1,
        };
        let mut data = Vec::new();
        for row in 0..spec.n_rows {
            for col in 0..spec.n_cols {
                data.push((row * 3 + col * 5) as u8);
                data.push((200usize.wrapping_sub(row * 7 + col * 2) & 0xff) as u8);
            }
        }
        let mask_bytes: Vec<u8> = (0..(spec.n_cols * spec.n_rows))
            .map(|idx| u8::from(idx % 7 != 0 && idx % 11 != 0))
            .collect();
        let mask = BitMask::from_byte_mask(&mask_bytes, spec.n_cols, spec.n_rows).unwrap();

        let blob = encode_lerc2_byte_huffman(spec, &data, Some(&mask), 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.mask, mask);
        let mut expected = data.clone();
        for (pixel_idx, valid) in mask_bytes.iter().enumerate() {
            if *valid == 0 {
                expected[pixel_idx * 2] = 0;
                expected[pixel_idx * 2 + 1] = 0;
            }
        }
        assert_eq!(decoded.data, DecodedData::UChar(expected));
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(
            validate_lerc2_checksum(&blob).unwrap().blob_size as usize,
            blob.len()
        );
    }

    #[test]
    fn encodes_pre_v4_byte_huffman_lerc2_as_delta_huffman() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 16,
            n_rows: 4,
            n_bands: 1,
            n_masks: 0,
        };
        let data: Vec<u8> = (0..spec.n_rows)
            .flat_map(|row| (0..spec.n_cols).map(move |col| (row * 3 + col * 2) as u8))
            .collect();
        let blob = encode_lerc2_byte_huffman(spec, &data, None, 3).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let header = get_lerc2_header_info(&blob).unwrap().header;
        let payload_offset =
            header.header_size + compute_lerc2_mask_byte_len(&header, None, false).unwrap();

        assert_eq!(decoded.header.version, 3);
        assert_eq!(decoded.header.n_depth, 1);
        assert!(decoded.ranges.is_none());
        assert_eq!(&blob[payload_offset..payload_offset + 2], &[0, 1]);
        assert_eq!(decoded.data, DecodedData::UChar(data));
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_signed_byte_huffman_lerc2_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::Char,
            n_depth: 1,
            n_cols: 8,
            n_rows: 4,
            n_bands: 1,
            n_masks: 0,
        };
        let data: Vec<u8> = (0..(spec.n_cols * spec.n_rows))
            .map(|idx| (((idx as i16 * 9) % 127) - 63) as i8 as u8)
            .collect();

        let blob = encode_lerc2_byte_huffman(spec, &data, None, 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        let expected: Vec<i8> = data.iter().map(|&value| value as i8).collect();
        assert_eq!(decoded.header.data_type, DataType::Char);
        assert_eq!(decoded.mask.count_valid_bits(), spec.n_cols * spec.n_rows);
        assert_eq!(decoded.data, DecodedData::Char(expected));
    }

    #[test]
    fn encodes_byte_huffman_lerc2_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 64,
            n_rows: 32,
            n_bands: 1,
            n_masks: 0,
        };
        let n_pixels = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(n_pixels * spec.n_depth);
        let mut expected = Vec::with_capacity(n_pixels * spec.n_depth);
        for pixel in 0..n_pixels {
            if pixel % 17 == 0 {
                data.extend_from_slice(&[255, 255]);
                expected.extend_from_slice(&[0, 0]);
            } else if pixel % 19 == 0 {
                data.extend_from_slice(&[7, 255]);
                expected.extend_from_slice(&[7, 255]);
            } else {
                let value = (pixel % 16) as u8;
                data.extend_from_slice(&[value, value.wrapping_add(3)]);
                expected.extend_from_slice(&[value, value.wrapping_add(3)]);
            }
        }

        let baseline = encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[255.0]),
            6,
        )
        .unwrap();
        let explicit = encode_lerc2_byte_huffman_with_no_data(spec, &data, None, 255.0, 6).unwrap();
        let auto =
            encode_lerc2_auto_with_no_data(spec, &data, 0.5, None, Some(&[1]), Some(&[255.0]), 6)
                .unwrap();
        let negative_auto =
            encode_lerc2_auto_with_no_data(spec, &data, -0.2, None, Some(&[1]), Some(&[255.0]), 6)
                .unwrap();
        let negative_baseline = encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            -0.2,
            None,
            Some(&[1]),
            Some(&[255.0]),
            6,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&auto).unwrap();

        assert!(explicit.len() < baseline.len());
        assert_eq!(auto, explicit);
        assert!(negative_auto.len() < negative_baseline.len());
        assert_eq!(negative_auto, explicit);
        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(
            decoded.header.num_valid_pixel as usize,
            n_pixels - (n_pixels + 16) / 17
        );
        assert_eq!(decoded.data, DecodedData::UChar(expected));
    }

    #[test]
    fn encodes_byte_huffman_lerc2_bands_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 64,
            n_rows: 16,
            n_bands: 2,
            n_masks: 0,
        };
        let band_len = spec.n_cols * spec.n_rows * spec.n_depth;
        let mut data = Vec::with_capacity(band_len * spec.n_bands);
        let mut expected = Vec::with_capacity(band_len * spec.n_bands);
        for band in 0..spec.n_bands {
            for pixel in 0..(spec.n_cols * spec.n_rows) {
                if band == 0 && pixel % 23 == 0 {
                    data.extend_from_slice(&[255, 255]);
                    expected.extend_from_slice(&[0, 0]);
                } else {
                    let value = ((band * 31 + pixel) % 32) as u8;
                    data.extend_from_slice(&[value, value.wrapping_add(5)]);
                    expected.extend_from_slice(&[value, value.wrapping_add(5)]);
                }
            }
        }

        let baseline = encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1, 0]),
            Some(&[255.0, 0.0]),
            6,
        )
        .unwrap();
        let blob = encode_lerc2_byte_huffman_bands_with_no_data(
            spec,
            &data,
            None,
            Some(&[1, 0]),
            Some(&[255.0, 0.0]),
            6,
        )
        .unwrap();
        let auto = encode_lerc2_auto_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1, 0]),
            Some(&[255.0, 0.0]),
            6,
        )
        .unwrap();
        let decoded = decode_lerc2_bands_supported(&auto).unwrap();
        let no_data = get_lerc2_no_data_info(&auto, 2).unwrap();

        assert!(blob.len() < baseline.len());
        assert_eq!(auto, blob);
        assert_eq!(no_data.uses_no_data, [1, 0]);
        assert_eq!(no_data.no_data_values, [255.0, 0.0]);
        assert!(decoded.bands[0].header.has_no_data_values());
        assert!(!decoded.bands[1].header.has_no_data_values());
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(expected[..band_len].to_vec())
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(expected[band_len..].to_vec())
        );
    }

    #[test]
    fn encodes_single_depth_no_data_as_mask_without_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 4,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1u16, 999, 3, 4, 5, 999, 7, 8];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.0,
            None,
            Some(&[1]),
            Some(&[999.0]),
            6,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(!decoded.header.has_no_data_values());
        assert_eq!(decoded.header.num_valid_pixel, 6);
        assert_eq!(decoded.mask.to_byte_mask(), [1, 0, 1, 1, 1, 0, 1, 1]);
        assert_eq!(
            decoded.data,
            DecodedData::UShort(vec![1, 0, 3, 4, 5, 0, 7, 8])
        );
    }

    #[test]
    fn rejects_invalid_byte_huffman_encode_inputs() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0];
        assert_eq!(
            encode_lerc2_byte_huffman(spec, &data, None, 6).unwrap_err(),
            LercError::WrongParam("byte Huffman Lerc2 encode requires Char or UChar data")
        );

        let byte_spec = EncodeSpec {
            data_type: DataType::UChar,
            ..spec
        };
        assert_eq!(
            encode_lerc2_byte_huffman(byte_spec, &[7; 6], None, 6).unwrap_err(),
            LercError::WrongParam("constant byte input should use Lerc2 constant encode")
        );
        assert_eq!(
            encode_lerc2_byte_huffman(byte_spec, &[1, 2, 3, 4, 5, 6], None, 1).unwrap_err(),
            LercError::WrongParam("byte Huffman Lerc2 encode requires version 2 or newer")
        );
        assert_eq!(
            encode_lerc2_byte_huffman(
                EncodeSpec {
                    n_depth: 2,
                    ..byte_spec
                },
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
                None,
                3
            )
            .unwrap_err(),
            LercError::WrongParam("pre-v4 Lerc2 encode can only store depth 1")
        );
    }

    #[test]
    fn auto_encode_selects_byte_huffman_when_smaller() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 64,
            n_rows: 64,
            n_bands: 1,
            n_masks: 0,
        };
        let data: Vec<u8> = (0..(spec.n_cols * spec.n_rows))
            .map(|idx| (idx % 64) as u8)
            .collect();

        let auto = encode_lerc2_auto(spec, &data, 0.5, None, 6).unwrap();
        let uncompressed = encode_lerc2_uncompressed(spec, &data, 0.5, None, 6).unwrap();
        let decoded = decode_lerc2_supported(&auto).unwrap();

        assert!(auto.len() < uncompressed.len());
        assert_eq!(decoded.data, DecodedData::UChar(data.clone()));
        assert_eq!(decoded.bytes_consumed, auto.len());

        let zero_auto = encode_lerc2_auto(spec, &data, 0.0, None, 6).unwrap();
        let zero_uncompressed = encode_lerc2_uncompressed(spec, &data, 0.0, None, 6).unwrap();
        let zero_decoded = decode_lerc2_supported(&zero_auto).unwrap();
        assert!(zero_auto.len() < zero_uncompressed.len());
        assert_eq!(zero_decoded.header.max_z_error, 0.5);
        assert_eq!(zero_decoded.data, DecodedData::UChar(data.clone()));

        let negative_auto = encode_lerc2_auto(spec, &data, -0.2, None, 6).unwrap();
        let negative_uncompressed = encode_lerc2_uncompressed(spec, &data, -0.2, None, 6).unwrap();
        let negative_decoded = decode_lerc2_supported(&negative_auto).unwrap();
        assert!(negative_auto.len() < negative_uncompressed.len());
        assert_eq!(negative_decoded.header.max_z_error, 0.5);
        assert_eq!(negative_decoded.data, DecodedData::UChar(data));
    }

    #[test]
    fn encodes_byte_huffman_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 64,
            n_rows: 16,
            n_bands: 2,
            n_masks: 1,
        };
        let band_len = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(band_len * 2);
        data.extend((0..band_len).map(|idx| (idx % 64) as u8));
        data.extend((0..band_len).map(|idx| (255 - (idx % 64)) as u8));
        let mask: Vec<u8> = (0..band_len)
            .map(|idx| u8::from(idx % 13 != 0 && idx % 17 != 0))
            .collect();

        let blob = encode_lerc2_byte_huffman_bands(spec, &data, Some(&mask), 6).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 1);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        for band in 0..2 {
            let mut expected = data[band * band_len..(band + 1) * band_len].to_vec();
            for (idx, valid) in mask.iter().enumerate() {
                if *valid == 0 {
                    expected[idx] = 0;
                }
            }
            assert_eq!(decoded.bands[band].data, DecodedData::UChar(expected));
        }
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn auto_encode_selects_byte_huffman_bands_when_smaller() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 64,
            n_rows: 64,
            n_bands: 2,
            n_masks: 0,
        };
        let band_len = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(band_len * 2);
        data.extend((0..band_len).map(|idx| (idx % 64) as u8));
        data.extend((0..band_len).map(|idx| (128 + idx % 64) as u8));

        let auto = encode_lerc2_auto(spec, &data, 0.5, None, 6).unwrap();
        let uncompressed = encode_lerc2_uncompressed(spec, &data, 0.5, None, 6).unwrap();
        let decoded = decode_lerc2_bands_supported(&auto).unwrap();

        assert!(auto.len() < uncompressed.len());
        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(data[..band_len].to_vec())
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(data[band_len..].to_vec())
        );
        assert_eq!(decoded.bytes_consumed, auto.len());
    }

    #[test]
    fn auto_encode_keeps_uncompressed_for_constant_or_nonbyte_data() {
        let byte_spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 4,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let constant = [7u8; 8];
        assert_eq!(
            encode_lerc2_auto(byte_spec, &constant, 0.5, None, 6).unwrap(),
            encode_lerc2_uncompressed(byte_spec, &constant, 0.5, None, 6).unwrap()
        );

        let float_spec = EncodeSpec {
            data_type: DataType::Float,
            ..byte_spec
        };
        let mut float_data = Vec::new();
        for value in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] {
            float_data.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            encode_lerc2_auto(float_spec, &float_data, 0.0, None, 6).unwrap(),
            encode_lerc2_uncompressed(float_spec, &float_data, 0.0, None, 6).unwrap()
        );
    }

    #[test]
    fn encodes_uncompressed_lerc2_constant_via_constant_path() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [7u8; 6];
        let blob = encode_lerc2_uncompressed(spec, &data, 0.5, None, 6).unwrap();
        let expected = encode_lerc2_constant(spec, 7.0, 0.5, None, 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(blob, expected);
        assert_eq!(decoded.data, DecodedData::UChar(data.to_vec()));
    }

    #[test]
    fn encodes_uncompressed_lerc2_nonconstant_via_one_sweep_path() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let data = [1u8, 2, 99, 99, 3, 4, 5, 6, 7, 8, 11, 12];
        let mask = [1u8, 0, 1, 1, 1, 1];
        let blob = encode_lerc2_uncompressed(spec, &data, 0.5, Some(&mask), 6).unwrap();
        let expected_mask = BitMask::from_byte_mask(&mask, 3, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.mask, expected_mask);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 6, 7, 8, 11, 12])
        );
    }

    #[test]
    fn encodes_uncompressed_lerc2_multi_band_via_one_sweep_bands_path() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let data = [1u8, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_uncompressed(spec, &data, 0.5, Some(&mask), 6).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 0, 30, 50, 70, 0])
        );
    }

    #[test]
    fn encodes_uncompressed_lerc2_multi_band_constants_via_constant_bands_path() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let data = [7u8; 12];
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_uncompressed(spec, &data, 0.0, Some(&mask), 6).unwrap();
        let expected = encode_lerc2_constant_bands(spec, &data, 0.0, Some(&mask), 6).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(blob, expected);
        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 1);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[0].header.z_min, 7.0);
        assert_eq!(decoded.bands[1].header.z_min, 7.0);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![7, 0, 7, 7, 7, 0])
        );
        assert_eq!(decoded.bands[1].data, decoded.bands[0].data);
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_uncompressed_lerc2_with_inactive_no_data_via_selector_path() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [7u8; 6];
        let blob =
            encode_lerc2_uncompressed_with_no_data(spec, &data, 0.5, None, Some(&[0]), None, 6)
                .unwrap();
        let expected = encode_lerc2_uncompressed(spec, &data, 0.5, None, 6).unwrap();

        assert_eq!(blob, expected);
    }

    #[test]
    fn encodes_uncompressed_lerc2_with_active_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob = encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[255.0]),
            6,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
    }

    #[test]
    fn encodes_one_sweep_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        let data = [1u8, 2, 99, 99, 3, 9, 5, 6, 7, 8, 0, 0];
        let blob = encode_lerc2_one_sweep(spec, &data, 0.5, Some(&mask), 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.header.n_depth, 2);
        assert_eq!(decoded.header.num_valid_pixel, 4);
        assert_eq!(decoded.header.z_min, 1.0);
        assert_eq!(decoded.header.z_max, 9.0);
        assert_eq!(decoded.mask, mask);
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(
            validate_lerc2_checksum(&blob).unwrap().blob_size as usize,
            blob.len()
        );
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 9, 5, 6, 7, 8, 0, 0])
        );
    }

    #[test]
    fn encodes_pre_v4_one_sweep_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 4,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 1, 0, 1], 4, 2).unwrap();
        let blob = encode_lerc2_one_sweep(spec, &data, 0.0, Some(&mask), 3).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 3);
        assert_eq!(decoded.header.n_depth, 1);
        assert!(decoded.ranges.is_none());
        assert_eq!(decoded.header.max_z_error, 0.5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 0, 3, 4, 5, 6, 0, 8])
        );
        assert_eq!(decoded.mask, mask);
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_raw_tiled_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 5,
            n_rows: 3,
            n_bands: 1,
            n_masks: 1,
        };
        let mask =
            BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1], 5, 3).unwrap();
        let data: Vec<u8> = (0..30).collect();
        let blob = encode_lerc2_tiled_raw(spec, &data, 0.0, Some(&mask), 6, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.header.micro_block_size, 2);
        assert_eq!(decoded.mask, mask);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![
                0, 1, 0, 0, 4, 5, 6, 7, 8, 9, 0, 0, 12, 13, 14, 15, 16, 17, 18, 19, 0, 0, 22, 23,
                24, 25, 26, 27, 28, 29
            ])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_simple_bit_stuffed_tiled_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 5,
            n_rows: 3,
            n_bands: 1,
            n_masks: 1,
        };
        let mask =
            BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1], 5, 3).unwrap();
        let data: Vec<u8> = (10..25).collect();
        let blob = encode_lerc2_tiled_simple(spec, &data, 0.5, Some(&mask), 6, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.header.micro_block_size, 2);
        assert_eq!(decoded.mask, mask);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![
                10, 0, 12, 13, 14, 0, 16, 17, 18, 19, 0, 21, 22, 23, 24
            ])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_pre_v4_simple_bit_stuffed_tiled_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 5,
            n_rows: 3,
            n_bands: 1,
            n_masks: 1,
        };
        let mask =
            BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1], 5, 3).unwrap();
        let values = [1u16, 99, 3, 4, 5, 99, 7, 8, 9, 10, 99, 12, 13, 14, 15];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_lerc2_tiled_simple(spec, &data, 0.5, Some(&mask), 3, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 3);
        assert_eq!(decoded.header.n_depth, 1);
        assert_eq!(decoded.header.micro_block_size, 2);
        assert!(decoded.ranges.is_none());
        assert_eq!(decoded.mask, mask);
        assert_eq!(
            decoded.data,
            DecodedData::UShort(vec![1, 0, 3, 4, 5, 0, 7, 8, 9, 10, 0, 12, 13, 14, 15])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_lut_tiled_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 8,
            n_rows: 8,
            n_bands: 1,
            n_masks: 0,
        };
        let values = (0..spec.n_cols * spec.n_rows)
            .map(|idx| if idx % 5 == 0 { 100u16 } else { 10u16 })
            .collect::<Vec<_>>();
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_lerc2_tiled_lut(spec, &data, 0.5, None, 4, 8).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let header = get_lerc2_header_info(&blob).unwrap().header;
        let payload_offset = header.header_size
            + compute_lerc2_mask_byte_len(&header, None, false).unwrap()
            + compute_lerc2_min_max_ranges_byte_len(&header).unwrap();
        let bit_stuffer_header_offset = payload_offset + 1 + 1 + DataType::UShort.size_in_bytes();

        assert_eq!(decoded.header.micro_block_size, 8);
        assert_eq!(decoded.data, DecodedData::UShort(values));
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(blob[payload_offset], 0);
        assert_eq!(blob[payload_offset + 1] & 3, 1);
        assert_ne!(blob[bit_stuffer_header_offset] & (1 << 5), 0);
    }

    #[test]
    fn encodes_depth_diff_lut_tiled_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 8,
            n_rows: 8,
            n_bands: 1,
            n_masks: 0,
        };
        let mut values = Vec::with_capacity(spec.n_cols * spec.n_rows * spec.n_depth);
        for idx in 0..spec.n_cols * spec.n_rows {
            let first = if idx % 5 == 0 { 100u16 } else { 10u16 };
            values.push(first);
            values.push(first + 5);
        }
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_lerc2_tiled_lut(spec, &data, 0.5, None, 5, 8).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let header = get_lerc2_header_info(&blob).unwrap().header;
        let payload_offset = header.header_size
            + compute_lerc2_mask_byte_len(&header, None, false).unwrap()
            + compute_lerc2_min_max_ranges_byte_len(&header).unwrap();
        let mut pos = payload_offset + 1;
        assert_eq!(blob[pos] & 3, 1);
        pos += 1 + DataType::UShort.size_in_bytes();
        let (_, consumed) =
            BitStuffer2::decode(&blob[pos..], spec.n_cols * spec.n_rows, 5).unwrap();
        pos += consumed;

        assert_eq!(decoded.header.version, 5);
        assert_eq!(decoded.data, DecodedData::UShort(values));
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_ne!(blob[pos] & 4, 0);
        assert_eq!(blob[pos] & 3, 3);
    }

    #[test]
    fn encodes_pre_v4_raw_tiled_lerc2_blob_for_supported_decode_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 5,
            n_rows: 3,
            n_bands: 1,
            n_masks: 1,
        };
        let mask =
            BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1], 5, 3).unwrap();
        let values = [1u16, 99, 3, 4, 5, 99, 7, 8, 9, 10, 99, 12, 13, 14, 15];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_lerc2_tiled_raw(spec, &data, 0.0, Some(&mask), 3, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 3);
        assert_eq!(decoded.header.n_depth, 1);
        assert_eq!(decoded.header.micro_block_size, 2);
        assert!(decoded.ranges.is_none());
        assert_eq!(decoded.mask, mask);
        assert_eq!(
            decoded.data,
            DecodedData::UShort(vec![1, 0, 3, 4, 5, 0, 7, 8, 9, 10, 0, 12, 13, 14, 15])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_raw_tiled_lerc2_huffman_probe_mode_zero_prefix() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 2,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 3, 4];
        let blob = encode_lerc2_tiled_raw(spec, &data, 0.5, None, 6, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let (_, _, tiled) = read_lerc2_tiled_raw(&blob).unwrap();
        let (header, _, _) = read_lerc2_min_max_ranges(&blob).unwrap();
        let payload_offset = header.header_size
            + compute_lerc2_mask_byte_len(&header, None, false).unwrap()
            + compute_lerc2_min_max_ranges_byte_len(&header).unwrap();
        let mut huffman_mode_blob = blob.clone();
        huffman_mode_blob[payload_offset + 1] = 1;

        assert_eq!(decoded.data, DecodedData::UChar(data.to_vec()));
        assert_eq!(tiled.data, data);
        assert_eq!(tiled.bytes_consumed, blob.len());
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(&blob[payload_offset..payload_offset + 2], &[0, 0]);
        assert_eq!(
            read_lerc2_tiled_raw(&huffman_mode_blob).unwrap_err(),
            LercError::Unsupported("Lerc2 blob is not encoded with tiled payloads")
        );
    }

    #[test]
    fn encodes_one_sweep_lerc2_all_depths_constant_as_range_only_blob() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2];
        let blob = encode_lerc2_one_sweep(spec, &data, 0.5, None, 4).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 4);
        assert_eq!(decoded.header.z_min, 1.0);
        assert_eq!(decoded.header.z_max, 2.0);
        assert!(decoded.ranges.as_ref().unwrap().min_max_equal);
        assert_eq!(decoded.data, DecodedData::UChar(data.to_vec()));
    }

    #[test]
    fn encodes_raw_tiled_lerc2_all_depths_constant_as_range_only_blob() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 2,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 1, 2, 1, 2, 1, 2];
        let blob = encode_lerc2_tiled_raw(spec, &data, 0.0, None, 4, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.version, 4);
        assert_eq!(decoded.header.micro_block_size, 2);
        assert!(decoded.ranges.as_ref().unwrap().min_max_equal);
        assert_eq!(decoded.data, DecodedData::UChar(data.to_vec()));
    }

    #[test]
    fn encodes_raw_tiled_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let data = [1u8, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_tiled_raw_bands(spec, &data, 0.0, Some(&mask), 6, 2).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 1);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[0].header.micro_block_size, 2);
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 0, 30, 50, 70, 0])
        );
    }

    #[test]
    fn encodes_pre_v4_raw_tiled_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let values = [1u16, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_tiled_raw_bands(spec, &data, 0.0, Some(&mask), 3, 2).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.version, 3);
        assert_eq!(decoded.bands[1].header.version, 3);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert!(decoded.bands[0].ranges.is_none());
        assert!(decoded.bands[1].ranges.is_none());
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UShort(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UShort(vec![10, 0, 30, 50, 70, 0])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_simple_bit_stuffed_tiled_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let data = [1u8, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_tiled_simple_bands(spec, &data, 0.5, Some(&mask), 6, 2).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 1);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[0].header.micro_block_size, 2);
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 0, 30, 50, 70, 0])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_lut_tiled_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 4,
            n_rows: 4,
            n_bands: 2,
            n_masks: 1,
        };
        let band0 =
            (0..spec.n_cols * spec.n_rows).map(|idx| if idx % 3 == 0 { 100u16 } else { 10u16 });
        let band1 =
            (0..spec.n_cols * spec.n_rows).map(|idx| if idx % 4 == 0 { 250u16 } else { 25u16 });
        let values = band0.chain(band1).collect::<Vec<_>>();
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mask = [1u8; 16];
        let blob = encode_lerc2_tiled_lut_bands(spec, &data, 0.5, Some(&mask), 6, 4).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 1);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UShort(values[..16].to_vec())
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UShort(values[16..].to_vec())
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_pre_v4_simple_bit_stuffed_tiled_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let values = [1u16, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_tiled_simple_bands(spec, &data, 0.5, Some(&mask), 3, 2).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.version, 3);
        assert_eq!(decoded.bands[1].header.version, 3);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert!(decoded.bands[0].ranges.is_none());
        assert!(decoded.bands[1].ranges.is_none());
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UShort(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UShort(vec![10, 0, 30, 50, 70, 0])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_raw_tiled_lerc2_bands_with_per_band_masks() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 2,
        };
        let data = [1u8, 2, 3, 4, 5, 6, 10, 20, 30, 40, 50, 60];
        let masks = [1u8, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1];
        let blob = encode_lerc2_tiled_raw_bands(spec, &data, 0.0, Some(&masks), 6, 2).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_ne!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 4, 5, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 20, 0, 0, 50, 60])
        );
    }

    #[test]
    fn encodes_raw_tiled_lerc2_bands_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob = encode_lerc2_tiled_raw_bands_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[255.0]),
            6,
            2,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let no_data = get_lerc2_no_data_info(&blob, 1).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.micro_block_size, 2);
        assert_eq!(decoded.header.no_data_val, 255.0);
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(decoded.mask.count_valid_bits(), 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
        assert_eq!(no_data.uses_no_data, [1]);
        assert_eq!(no_data.no_data_values, [255.0]);
    }

    #[test]
    fn encodes_raw_tiled_lerc2_single_band_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob =
            encode_lerc2_tiled_raw_with_no_data(spec, &data, 0.5, None, 255.0, 6, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.micro_block_size, 2);
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
    }

    #[test]
    fn encodes_simple_tiled_lerc2_bands_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob = encode_lerc2_tiled_simple_bands_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[255.0]),
            6,
            2,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let no_data = get_lerc2_no_data_info(&blob, 1).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.micro_block_size, 2);
        assert_eq!(decoded.header.no_data_val, 255.0);
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(decoded.mask.count_valid_bits(), 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
        assert_eq!(no_data.uses_no_data, [1]);
        assert_eq!(no_data.no_data_values, [255.0]);
    }

    #[test]
    fn encodes_simple_tiled_lerc2_single_band_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob =
            encode_lerc2_tiled_simple_with_no_data(spec, &data, 0.5, None, 255.0, 6, 2).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.micro_block_size, 2);
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
    }

    #[test]
    fn single_band_no_data_encoders_accept_integer_negative_max_z_error() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];

        let one_sweep =
            encode_lerc2_one_sweep_with_no_data(spec, &data, -0.2, None, 255.0, 6).unwrap();
        let one_sweep_decoded = decode_lerc2_supported(&one_sweep).unwrap();
        assert_eq!(one_sweep_decoded.header.max_z_error, 0.5);
        assert!(one_sweep_decoded.header.has_no_data_values());
        assert_eq!(
            one_sweep_decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );

        let raw_tiled =
            encode_lerc2_tiled_raw_with_no_data(spec, &data, -0.2, None, 255.0, 6, 2).unwrap();
        let raw_tiled_decoded = decode_lerc2_supported(&raw_tiled).unwrap();
        assert_eq!(raw_tiled_decoded.header.max_z_error, 0.5);
        assert!(raw_tiled_decoded.header.has_no_data_values());
        assert_eq!(raw_tiled_decoded.data, one_sweep_decoded.data);
    }

    #[test]
    fn encodes_one_sweep_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let data = [1u8, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_one_sweep_bands(spec, &data, 0.5, Some(&mask), 6).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 1);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 0, 30, 50, 70, 0])
        );
    }

    #[test]
    fn encodes_pre_v4_one_sweep_lerc2_bands_with_shared_mask() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let values = [1u16, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mask = [1u8, 0, 1, 1, 1, 0];
        let blob = encode_lerc2_one_sweep_bands(spec, &data, 0.0, Some(&mask), 3).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].header.version, 3);
        assert_eq!(decoded.bands[1].header.version, 3);
        assert_eq!(decoded.bands[0].header.n_blobs_more, 0);
        assert_eq!(decoded.bands[1].header.n_blobs_more, 0);
        assert!(decoded.bands[0].ranges.is_none());
        assert!(decoded.bands[1].ranges.is_none());
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UShort(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UShort(vec![10, 0, 30, 50, 70, 0])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn encodes_one_sweep_lerc2_bands_with_per_band_masks() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 2,
        };
        let data = [1u8, 2, 3, 4, 5, 6, 10, 20, 30, 40, 50, 60];
        let masks = [1u8, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1];
        let blob = encode_lerc2_one_sweep_bands(spec, &data, 0.5, Some(&masks), 6).unwrap();
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bands.len(), 2);
        assert_ne!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 4, 5, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 20, 0, 0, 50, 60])
        );
    }

    #[test]
    fn encodes_one_sweep_lerc2_bands_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob = encode_lerc2_one_sweep_bands_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[255.0]),
            6,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        let no_data = get_lerc2_no_data_info(&blob, 1).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val, 255.0);
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(decoded.mask.count_valid_bits(), 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
        assert_eq!(no_data.uses_no_data, [1]);
        assert_eq!(no_data.no_data_values, [255.0]);
        assert_eq!(
            get_lerc2_data_ranges(&blob).unwrap_err(),
            LercError::HasNoData
        );
    }

    #[test]
    fn encodes_one_sweep_lerc2_single_band_with_no_data_metadata() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let blob = encode_lerc2_one_sweep_with_no_data(spec, &data, 0.5, None, 255.0, 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
    }

    #[test]
    fn encodes_one_sweep_lerc2_no_data_with_internal_sentinel_remap() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 5, 5, 3, 4, 5, 6, 7, 8, 9, 10];
        let blob = encode_lerc2_one_sweep_bands_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[5.0]),
            6,
        )
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val, 0.0);
        assert_eq!(decoded.header.no_data_val_orig, 5.0);
        assert_eq!(decoded.mask.count_valid_bits(), 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 6, 7, 8, 9, 10])
        );
    }

    #[test]
    fn rejects_invalid_one_sweep_lerc2_encode_inputs() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 2,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1.0f32, 2.0, f32::NAN, 4.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            encode_lerc2_one_sweep(spec, &data, 0.0, None, 6).unwrap_err(),
            LercError::WrongParam("Lerc2 encode input contains NaN")
        );
        assert_eq!(
            encode_lerc2_one_sweep(spec, &[1, 2, 3], 0.0, None, 6).unwrap_err(),
            LercError::WrongParam("Lerc2 encode data length mismatch")
        );
        assert_eq!(
            encode_lerc2_one_sweep(spec, &data, 0.0, None, 1).unwrap_err(),
            LercError::WrongParam("one-sweep Lerc2 encode requires version 2 or newer")
        );
        let non_nan_data = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            encode_lerc2_one_sweep(
                EncodeSpec { n_depth: 2, ..spec },
                &non_nan_data,
                0.0,
                None,
                3
            )
            .unwrap_err(),
            LercError::WrongParam("pre-v4 Lerc2 encode can only store depth 1")
        );
        assert_eq!(
            encode_lerc2_one_sweep(spec, &data, -0.01, None, 6).unwrap_err(),
            LercError::WrongParam("negative max_z_error bit-plane encode requires integer data")
        );

        let no_data_spec = EncodeSpec { n_depth: 2, ..spec };
        let no_data = [1.0f32, -9999.0, -9999.0, -9999.0, 2.0, 3.0, 4.0, 5.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            encode_lerc2_one_sweep_with_no_data(no_data_spec, &no_data, -0.01, None, -9999.0, 6)
                .unwrap_err(),
            LercError::WrongParam("negative max_z_error bit-plane encode requires integer data")
        );
    }

    #[test]
    fn rejects_invalid_raw_tiled_lerc2_encode_inputs() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 2,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 3, 4];
        assert_eq!(
            encode_lerc2_tiled_raw(spec, &data, 0.0, None, 1, 2).unwrap_err(),
            LercError::WrongParam("raw tiled Lerc2 encode requires version 2 or newer")
        );
        assert_eq!(
            encode_lerc2_tiled_raw(
                EncodeSpec { n_depth: 2, ..spec },
                &[1, 2, 3, 4, 5, 6, 7, 8],
                0.0,
                None,
                3,
                2
            )
            .unwrap_err(),
            LercError::WrongParam("pre-v4 Lerc2 encode can only store depth 1")
        );
        assert_eq!(
            encode_lerc2_tiled_raw(spec, &data, 0.0, None, 6, 0).unwrap_err(),
            LercError::WrongParam("Lerc2 raw tiled micro block size must be 1 through 32")
        );
        assert_eq!(
            encode_lerc2_tiled_raw(spec, &[1, 2, 3], 0.0, None, 6, 2).unwrap_err(),
            LercError::WrongParam("Lerc2 encode data length mismatch")
        );
        let band_spec = EncodeSpec {
            n_bands: 2,
            n_masks: 0,
            ..spec
        };
        let band_data = [1u8, 2, 3, 4, 10, 20, 30, 40];
        assert_eq!(
            encode_lerc2_tiled_raw_bands(band_spec, &band_data, 0.0, None, 1, 2).unwrap_err(),
            LercError::WrongParam("raw tiled Lerc2 encode requires version 2 or newer")
        );
        assert_eq!(
            encode_lerc2_tiled_raw_bands(
                EncodeSpec {
                    n_depth: 2,
                    ..band_spec
                },
                &[1, 2, 3, 4, 5, 6, 7, 8, 10, 20, 30, 40, 50, 60, 70, 80],
                0.0,
                None,
                3,
                2
            )
            .unwrap_err(),
            LercError::WrongParam("pre-v4 Lerc2 encode can only store depth 1")
        );
        assert_eq!(
            encode_lerc2_tiled_raw_bands(band_spec, &band_data, 0.0, None, 6, 0).unwrap_err(),
            LercError::WrongParam("Lerc2 raw tiled micro block size must be 1 through 32")
        );
    }

    #[test]
    fn integer_negative_max_z_error_uses_bit_plane_heuristic() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 80,
            n_rows: 80,
            n_bands: 1,
            n_masks: 0,
        };
        let mut state = 0x1234_5678u32;
        let data = (0..spec.n_cols * spec.n_rows)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state & 3) as u8
            })
            .collect::<Vec<_>>();
        let mut mask = BitMask::new(spec.n_cols, spec.n_rows).unwrap();
        mask.set_all_valid();
        let inferred = try_lerc2_bit_plane_max_z_error(spec, &data, &mask, 0.2).unwrap();
        assert_eq!(inferred, Some(1.0));

        let blob = encode_lerc2_one_sweep(spec, &data, -0.2, None, 6).unwrap();
        let header = get_lerc2_header_info(&blob).unwrap().header;
        assert_eq!(header.max_z_error, 1.0);

        let small_spec = EncodeSpec {
            n_cols: 2,
            n_rows: 2,
            ..spec
        };
        let small_blob =
            encode_lerc2_tiled_raw(small_spec, &[1, 2, 3, 4], -0.2, None, 6, 2).unwrap();
        let small_header = get_lerc2_header_info(&small_blob).unwrap().header;
        assert_eq!(small_header.max_z_error, 0.5);
    }

    #[test]
    fn float_positive_max_z_error_uses_cpp_raise_heuristic() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 4,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1.0f32, 1.5, 2.0, 2.5, -3.0, -2.5, 0.0, 4.5]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let mut mask = BitMask::new(spec.n_cols, spec.n_rows).unwrap();
        mask.set_all_valid();

        assert_eq!(
            try_raise_lerc2_float_max_z_error(spec, &data, &mask, 0.1).unwrap(),
            Some(0.25)
        );

        let blob = encode_lerc2_one_sweep(spec, &data, 0.1, None, 6).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        assert_eq!(decoded.header.max_z_error, 0.25);
        assert_eq!(
            decoded.data,
            DecodedData::Float(vec![1.0, 1.5, 2.0, 2.5, -3.0, -2.5, 0.0, 4.5])
        );
    }

    #[test]
    fn reads_v4_byte_min_max_ranges() {
        let range_bytes = [1u8, 2, 3, 10, 20, 30];
        let blob = synthetic_v4_blob(DataType::UChar, 3, &range_bytes);
        let (_, mask, ranges) = read_lerc2_min_max_ranges(&blob).unwrap();

        assert_eq!(mask.mask.count_valid_bits(), 6);
        assert_eq!(ranges.mins, [1.0, 2.0, 3.0]);
        assert_eq!(ranges.maxs, [10.0, 20.0, 30.0]);
        assert_eq!(ranges.bytes_consumed, blob.len());
        assert!(!ranges.min_max_equal);
    }

    #[test]
    fn reads_v4_float_and_double_min_max_ranges() {
        let mut float_ranges = Vec::new();
        for value in [-1.25f32, 2.5, 9.75, 10.5] {
            float_ranges.extend_from_slice(&value.to_le_bytes());
        }
        let float_blob = synthetic_v4_blob(DataType::Float, 2, &float_ranges);
        let (_, _, ranges) = read_lerc2_min_max_ranges(&float_blob).unwrap();
        assert_eq!(ranges.mins, [-1.25, 2.5]);
        assert_eq!(ranges.maxs, [9.75, 10.5]);

        let mut double_ranges = Vec::new();
        for value in [-100.0f64, 7.0, -100.0, 7.0] {
            double_ranges.extend_from_slice(&value.to_le_bytes());
        }
        let double_blob = synthetic_v4_blob(DataType::Double, 2, &double_ranges);
        let (_, _, ranges) = read_lerc2_min_max_ranges(&double_blob).unwrap();
        assert_eq!(ranges.mins, [-100.0, 7.0]);
        assert_eq!(ranges.maxs, [-100.0, 7.0]);
        assert!(ranges.min_max_equal);
    }

    #[test]
    fn writes_lerc2_byte_min_max_ranges_for_parser_round_trip() {
        let mut header = header_for_write(4);
        header.data_type = DataType::UChar;
        header.n_depth = 3;
        let ranges = MinMaxRanges {
            mins: vec![1.0, 2.0, 3.0],
            maxs: vec![10.0, 20.0, 30.0],
            bytes_consumed: 0,
            min_max_equal: false,
        };
        header.blob_size = (header.header_size
            + compute_lerc2_mask_byte_len(&header, None, false).unwrap()
            + compute_lerc2_min_max_ranges_byte_len(&header).unwrap())
            as i32;

        let blob = blob_with_written_header_mask_and_ranges(&header, None, false, &ranges);
        let (_, _, decoded_ranges) = read_lerc2_min_max_ranges_with_previous(
            &blob,
            Some(&BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 1], 3, 2).unwrap()),
        )
        .unwrap();

        assert_eq!(decoded_ranges.mins, ranges.mins);
        assert_eq!(decoded_ranges.maxs, ranges.maxs);
        assert_eq!(decoded_ranges.bytes_consumed, blob.len());
        assert!(!decoded_ranges.min_max_equal);
    }

    #[test]
    fn writes_lerc2_float_min_max_ranges_for_parser_round_trip() {
        let mut header = header_for_write(4);
        header.data_type = DataType::Float;
        header.n_depth = 2;
        let ranges = MinMaxRanges {
            mins: vec![-1.25, 2.5],
            maxs: vec![9.75, 10.5],
            bytes_consumed: 0,
            min_max_equal: false,
        };
        header.blob_size = (header.header_size
            + compute_lerc2_mask_byte_len(&header, None, false).unwrap()
            + compute_lerc2_min_max_ranges_byte_len(&header).unwrap())
            as i32;

        let blob = blob_with_written_header_mask_and_ranges(&header, None, false, &ranges);
        let (_, _, decoded_ranges) = read_lerc2_min_max_ranges_with_previous(
            &blob,
            Some(&BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 1], 3, 2).unwrap()),
        )
        .unwrap();

        assert_eq!(decoded_ranges.mins, ranges.mins);
        assert_eq!(decoded_ranges.maxs, ranges.maxs);
        assert_eq!(decoded_ranges.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_invalid_lerc2_min_max_range_write_inputs() {
        let mut header = header_for_write(4);
        header.data_type = DataType::UShort;
        header.n_depth = 2;
        let ranges = MinMaxRanges {
            mins: vec![1.0, 2.0],
            maxs: vec![10.0, 20.0],
            bytes_consumed: 0,
            min_max_equal: false,
        };

        assert_eq!(compute_lerc2_min_max_ranges_byte_len(&header).unwrap(), 8);
        assert_eq!(
            write_lerc2_min_max_ranges(&header, &ranges, &mut [0; 7]).unwrap_err(),
            LercError::BufferTooSmall
        );

        let bad_ranges = MinMaxRanges {
            mins: vec![1.0],
            ..ranges.clone()
        };
        assert_eq!(
            write_lerc2_min_max_ranges(&header, &bad_ranges, &mut [0; 8]).unwrap_err(),
            LercError::WrongParam("Lerc2 min/max range count does not match depth")
        );

        let old_header = HeaderInfo {
            version: 3,
            ..header
        };
        assert_eq!(
            compute_lerc2_min_max_ranges_byte_len(&old_header).unwrap_err(),
            LercError::WrongParam("Lerc2 min/max ranges require version 4 or newer")
        );
    }

    #[test]
    fn writes_lerc2_one_sweep_payload_for_supported_decode_round_trip() {
        let mut header = header_for_write(4);
        header.data_type = DataType::UChar;
        header.n_depth = 2;
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        header.num_valid_pixel = mask.count_valid_bits() as i32;
        header.z_min = 1.0;
        header.z_max = 8.0;
        let ranges = MinMaxRanges {
            mins: vec![1.0, 2.0],
            maxs: vec![7.0, 8.0],
            bytes_consumed: 0,
            min_max_equal: false,
        };
        let data = [1, 2, 9, 9, 3, 4, 5, 6, 7, 8, 9, 9];
        header.blob_size = (header.header_size
            + compute_lerc2_mask_byte_len(&header, Some(&mask), true).unwrap()
            + compute_lerc2_min_max_ranges_byte_len(&header).unwrap()
            + compute_lerc2_one_sweep_byte_len(&header).unwrap()) as i32;
        header.checksum = 0;

        let mut blob = blob_with_written_one_sweep(&header, &mask, true, &ranges, &data);
        finalize_lerc2_checksum(&mut blob).unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 6, 7, 8, 0, 0])
        );
        assert_eq!(decoded.mask, mask);
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_invalid_lerc2_one_sweep_write_inputs() {
        let mut header = header_for_write(4);
        header.data_type = DataType::UChar;
        header.n_depth = 2;
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        header.num_valid_pixel = mask.count_valid_bits() as i32;
        let data = [1, 2, 9, 9, 3, 4, 5, 6, 7, 8, 9, 9];

        assert_eq!(compute_lerc2_one_sweep_byte_len(&header).unwrap(), 9);
        assert_eq!(
            write_lerc2_one_sweep(&header, &mask, &data, &mut [0; 8]).unwrap_err(),
            LercError::BufferTooSmall
        );

        let wrong_dims = BitMask::from_byte_mask(&[1, 1, 1, 1], 2, 2).unwrap();
        assert_eq!(
            write_lerc2_one_sweep(&header, &wrong_dims, &data, &mut [0; 16]).unwrap_err(),
            LercError::WrongParam("Lerc2 one-sweep mask dimensions do not match header")
        );
        assert_eq!(
            write_lerc2_one_sweep(&header, &mask, &data[..10], &mut [0; 16]).unwrap_err(),
            LercError::WrongParam("Lerc2 one-sweep data length does not match header")
        );
    }

    #[test]
    fn writes_lerc2_tiled_raw_payload_for_supported_decode_round_trip() {
        let valid = [1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1];
        let mask = BitMask::from_byte_mask(&valid, 5, 3).unwrap();
        let mut header = HeaderInfo {
            version: 4,
            checksum: 0,
            n_rows: 3,
            n_cols: 5,
            n_depth: 2,
            num_valid_pixel: mask.count_valid_bits() as i32,
            micro_block_size: 2,
            blob_size: 0,
            n_blobs_more: 0,
            b_pass_no_data_values: 0,
            b_is_int: 1,
            b_reserved_3: 0,
            b_reserved_4: 0,
            data_type: DataType::UChar,
            max_z_error: 0.0,
            z_min: 0.0,
            z_max: 29.0,
            no_data_val: 0.0,
            no_data_val_orig: 0.0,
            header_size: compute_lerc2_header_byte_len(4).unwrap(),
        };
        let ranges = MinMaxRanges {
            mins: vec![0.0, 1.0],
            maxs: vec![28.0, 29.0],
            bytes_consumed: 0,
            min_max_equal: false,
        };
        let data: Vec<u8> = (0..30).collect();

        assert_eq!(
            compute_lerc2_tiled_raw_byte_len(&header, &mask).unwrap(),
            37
        );
        let blob = blob_with_written_tiled_raw(&mut header, &mask, &ranges, &data);
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.mask, mask);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![
                0, 1, 0, 0, 4, 5, 6, 7, 8, 9, 0, 0, 12, 13, 14, 15, 16, 17, 18, 19, 0, 0, 22, 23,
                24, 25, 26, 27, 28, 29
            ])
        );
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_invalid_lerc2_tiled_raw_write_inputs() {
        let mut header = header_for_write(4);
        header.data_type = DataType::UChar;
        header.n_depth = 2;
        header.micro_block_size = 2;
        let mask = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        header.num_valid_pixel = mask.count_valid_bits() as i32;
        let data = [1, 2, 9, 9, 3, 4, 5, 6, 7, 8, 9, 9];

        assert_eq!(
            write_lerc2_tiled_raw(&header, &mask, &data, &mut [0; 12]).unwrap_err(),
            LercError::BufferTooSmall
        );

        let wrong_dims = BitMask::from_byte_mask(&[1, 1, 1, 1], 2, 2).unwrap();
        assert_eq!(
            write_lerc2_tiled_raw(&header, &wrong_dims, &data, &mut [0; 32]).unwrap_err(),
            LercError::WrongParam("Lerc2 raw tiled mask dimensions do not match header")
        );
        assert_eq!(
            write_lerc2_tiled_raw(&header, &mask, &data[..10], &mut [0; 32]).unwrap_err(),
            LercError::WrongParam("Lerc2 raw tiled data length does not match header")
        );

        header.micro_block_size = 0;
        assert_eq!(
            compute_lerc2_tiled_raw_byte_len(&header, &mask).unwrap_err(),
            LercError::WrongParam("Lerc2 raw tiled micro block size must be 1 through 32")
        );
    }

    #[test]
    fn reports_data_ranges_for_single_and_multi_depth_lerc2() {
        let single = synthetic_v4_ushort_tiled_raw_blob();
        let ranges = get_lerc2_data_ranges(&single).unwrap();
        assert_eq!(ranges.n_bands, 1);
        assert_eq!(ranges.n_depth, 1);
        assert_eq!(ranges.bytes_consumed, single.len());
        assert_eq!(ranges.mins, [100.0]);
        assert_eq!(ranges.maxs, [400.0]);

        let mut multi_ranges = Vec::new();
        for value in [-1.25f32, 2.5, 9.75, 10.5] {
            multi_ranges.extend_from_slice(&value.to_le_bytes());
        }
        let multi = synthetic_v4_blob(DataType::Float, 2, &multi_ranges);
        let ranges = get_lerc2_data_ranges(&multi).unwrap();
        assert_eq!(ranges.n_bands, 1);
        assert_eq!(ranges.n_depth, 2);
        assert_eq!(ranges.mins, [-1.25, 2.5]);
        assert_eq!(ranges.maxs, [9.75, 10.5]);
    }

    #[test]
    fn reports_data_ranges_for_float_fixture() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let ranges = get_lerc2_data_ranges(&blob).unwrap();

        assert_eq!(ranges.n_bands, 1);
        assert_eq!(ranges.n_depth, 1);
        assert_eq!(ranges.bytes_consumed, blob.len());
        assert_eq!(ranges.mins, [-82.972_091_674_804_69]);
        assert_eq!(ranges.maxs, [4080.613_769_531_25]);
    }

    #[test]
    fn reports_data_ranges_for_legacy_lerc1_fixture() {
        let blob = fixture("world.lerc1");
        let ranges = get_lerc2_data_ranges(&blob).unwrap();

        assert_eq!(ranges.n_bands, 1);
        assert_eq!(ranges.n_depth, 1);
        assert_eq!(ranges.bytes_consumed, blob.len());
        assert_eq!(ranges.mins, [-27.458_635_330_200_195]);
        assert_eq!(ranges.maxs, [5474.172_851_562_5]);
    }

    #[test]
    fn reports_data_ranges_for_concatenated_lerc2_bands() {
        let first = synthetic_v4_ushort_tiled_raw_blob();
        let mut second = synthetic_v4_ushort_tiled_raw_blob();
        let z_min_offset = FILE_KEY.len() + 4 + 4 + 7 * 4 + 8;
        second[z_min_offset..z_min_offset + 8].copy_from_slice(&500.0f64.to_le_bytes());
        second[z_min_offset + 8..z_min_offset + 16].copy_from_slice(&800.0f64.to_le_bytes());
        set_lerc2_checksum(&mut second);

        let mut concatenated = first;
        concatenated.extend_from_slice(&second);
        let ranges = get_lerc2_data_ranges(&concatenated).unwrap();

        assert_eq!(ranges.n_bands, 2);
        assert_eq!(ranges.n_depth, 1);
        assert_eq!(ranges.bytes_consumed, concatenated.len());
        assert_eq!(ranges.mins, [100.0, 500.0]);
        assert_eq!(ranges.maxs, [400.0, 800.0]);
    }

    #[test]
    fn rejects_truncated_v4_min_max_ranges() {
        let range_bytes = [1u8, 2, 3, 10, 20];
        let blob = synthetic_v4_blob(DataType::UChar, 3, &range_bytes);
        assert!(read_lerc2_min_max_ranges(&blob).is_err());
    }

    #[test]
    fn reads_v4_byte_one_sweep_payload_with_mask() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 2, 10, 20];
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let blob = synthetic_v4_one_sweep_blob(DataType::UChar, 2, &valid, &ranges, &payload);
        let (_, _, one_sweep) = read_lerc2_data_one_sweep(&blob).unwrap();

        assert_eq!(one_sweep.data, [1, 2, 0, 0, 3, 4, 5, 6, 0, 0, 7, 8]);
        assert_eq!(
            one_sweep.decode_typed(DataType::UChar).unwrap(),
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 6, 0, 0, 7, 8])
        );
        assert_eq!(one_sweep.bytes_consumed, blob.len());
    }

    #[test]
    fn reads_v4_float_one_sweep_payload_all_valid() {
        let valid = [1, 1, 1, 1, 1, 1];
        let mut ranges = Vec::new();
        for value in [-1.0f32, 10.0] {
            ranges.extend_from_slice(&value.to_le_bytes());
        }
        let values = [1.25f32, 2.5, 3.75, 4.0, 5.5, 6.25];
        let mut payload = Vec::new();
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }

        let blob = synthetic_v4_one_sweep_blob(DataType::Float, 1, &valid, &ranges, &payload);
        let (_, _, one_sweep) = read_lerc2_data_one_sweep(&blob).unwrap();
        assert_eq!(one_sweep.data, payload);
        assert_eq!(
            one_sweep.decode_typed(DataType::Float).unwrap(),
            DecodedData::Float(values.to_vec())
        );
    }

    #[test]
    fn rejects_non_one_sweep_and_truncated_payloads() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 2, 10, 20];
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut blob = synthetic_v4_one_sweep_blob(DataType::UChar, 2, &valid, &ranges, &payload);

        let flag_offset = blob.len() - payload.len() - 1;
        blob[flag_offset] = 0;
        assert!(read_lerc2_data_one_sweep(&blob).is_err());

        blob[flag_offset] = 1;
        blob.pop();
        assert!(read_lerc2_data_one_sweep(&blob).is_err());
    }

    #[test]
    fn reads_v4_raw_tiled_payload_with_mask() {
        let valid = [1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1];
        let ranges = [10u8, 20];
        let tile_payloads: [&[u8]; 6] =
            [&[10, 11], &[12, 13, 14], &[15, 16], &[17, 18], &[19], &[20]];
        let blob = synthetic_v4_tiled_raw_blob(DataType::UChar, 1, &valid, &ranges, &tile_payloads);
        let (_, _, tiled) = read_lerc2_tiled_raw(&blob).unwrap();

        assert_eq!(
            tiled.data,
            [10, 0, 12, 13, 15, 0, 11, 14, 0, 16, 17, 18, 0, 19, 20]
        );
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_raw_tile_integrity_mismatch_and_truncation() {
        let valid = [1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1];
        let ranges = [10u8, 20];
        let tile_payloads: [&[u8]; 6] =
            [&[10, 11], &[12, 13, 14], &[15, 16], &[17, 18], &[19], &[20]];
        let mut blob =
            synthetic_v4_tiled_raw_blob(DataType::UChar, 1, &valid, &ranges, &tile_payloads);
        let tile_start = blob.len()
            - tile_payloads
                .iter()
                .map(|payload| 1 + payload.len())
                .sum::<usize>();

        blob[tile_start] = 4;
        assert!(read_lerc2_tiled_raw(&blob).is_err());

        blob[tile_start] = 0;
        blob.pop();
        assert!(read_lerc2_tiled_raw(&blob).is_err());
    }

    #[test]
    fn reads_v4_simple_bit_stuffed_tiled_payload() {
        let valid = [1; 15];
        let ranges = [10u8, 24];
        let mut const_block = vec![3, 24];
        let blocks = vec![
            bit_stuffed_tile_block(10, &[0, 1, 5, 6]),
            bit_stuffed_tile_block(10, &[2, 3, 7, 8]),
            bit_stuffed_tile_block(10, &[4, 9]),
            bit_stuffed_tile_block(10, &[10, 11]),
            bit_stuffed_tile_block(10, &[12, 13]),
            std::mem::take(&mut const_block),
        ];
        let blob = synthetic_v4_tiled_block_blob(DataType::UChar, 1, &valid, &ranges, &blocks);
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();

        assert_eq!(tiled.data, (10u8..=24).collect::<Vec<_>>());
        assert_eq!(
            tiled.decode_typed(DataType::UChar).unwrap(),
            DecodedData::UChar((10u8..=24).collect())
        );
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_truncated_bit_stuffed_tiled_payload() {
        let valid = [1; 15];
        let ranges = [10u8, 24];
        let blocks = vec![
            bit_stuffed_tile_block(10, &[0, 1, 5, 6]),
            bit_stuffed_tile_block(10, &[2, 3, 7, 8]),
            bit_stuffed_tile_block(10, &[4, 9]),
            bit_stuffed_tile_block(10, &[10, 11]),
            bit_stuffed_tile_block(10, &[12, 13]),
            vec![3, 24],
        ];
        let mut blob = synthetic_v4_tiled_block_blob(DataType::UChar, 1, &valid, &ranges, &blocks);
        blob.pop();

        assert!(read_lerc2_tiled_payload(&blob).is_err());
    }

    #[test]
    fn reads_v4_lut_tiled_payload() {
        let valid = [1; 15];
        let ranges = [10u8, 24];
        let blocks = vec![
            lut_tile_block(10, &[0, 1, 5, 6]),
            lut_tile_block(12, &[0, 1, 5, 6]),
            lut_tile_block(14, &[0, 5]),
            lut_tile_block(20, &[0, 1]),
            lut_tile_block(22, &[0, 1]),
            vec![3, 24],
        ];
        let blob = synthetic_v4_tiled_block_blob(DataType::UChar, 1, &valid, &ranges, &blocks);
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();

        assert_eq!(tiled.data, (10u8..=24).collect::<Vec<_>>());
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_truncated_lut_tiled_payload() {
        let valid = [1; 15];
        let ranges = [10u8, 24];
        let blocks = vec![
            lut_tile_block(10, &[0, 1, 5, 6]),
            lut_tile_block(12, &[0, 1, 5, 6]),
            lut_tile_block(14, &[0, 5]),
            lut_tile_block(20, &[0, 1]),
            lut_tile_block(22, &[0, 1]),
            vec![3, 24],
        ];
        let mut blob = synthetic_v4_tiled_block_blob(DataType::UChar, 1, &valid, &ranges, &blocks);
        blob.pop();

        assert!(read_lerc2_tiled_payload(&blob).is_err());
    }

    #[test]
    fn decodes_supported_tiled_payload_to_typed_values() {
        let blob = synthetic_v4_ushort_tiled_raw_blob();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.header.data_type, DataType::UShort);
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.mask.count_valid_bits(), 4);
        assert_eq!(decoded.data, DecodedData::UShort(vec![100, 200, 300, 400]));
    }

    #[test]
    fn writes_supported_decode_data_and_mask_to_c_api_style_buffers() {
        let blob = synthetic_v4_ushort_tiled_raw_blob();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        let mut data = vec![0; decoded.data_byte_len()];
        assert_eq!(decoded.write_data_le_bytes(&mut data).unwrap(), data.len());
        assert_eq!(
            data,
            [100u16, 200, 300, 400]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );

        let mut mask = vec![0; decoded.mask_byte_len()];
        assert_eq!(decoded.write_mask_bytes(&mut mask).unwrap(), mask.len());
        assert_eq!(mask, [1, 1, 1, 1]);

        assert!(decoded.write_data_le_bytes(&mut [0; 7]).is_err());
        assert!(decoded.write_mask_bytes(&mut [0; 3]).is_err());
    }

    #[test]
    fn decodes_supported_one_sweep_and_const_payloads() {
        let valid = [1, 1, 1, 1, 1, 1];
        let mut ranges = Vec::new();
        for value in [-1.0f32, 10.0] {
            ranges.extend_from_slice(&value.to_le_bytes());
        }
        let values = [1.25f32, 2.5, 3.75, 4.0, 5.5, 6.25];
        let mut payload = Vec::new();
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }

        let one_sweep = synthetic_v4_one_sweep_blob(DataType::Float, 1, &valid, &ranges, &payload);
        let decoded = decode_lerc2_supported(&one_sweep).unwrap();
        assert_eq!(decoded.data, DecodedData::Float(values.to_vec()));

        let const_blob = synthetic_v4_const_blob();
        let decoded = decode_lerc2_supported(&const_blob).unwrap();
        assert_eq!(decoded.data, DecodedData::UChar(vec![7, 7, 7, 7]));
    }

    #[test]
    fn decodes_supported_concatenated_bands_with_previous_mask() {
        let valid = [1, 0, 1, 1, 0, 1];
        let mut ranges = Vec::new();
        for value in [1u8, 20] {
            ranges.push(value);
        }

        let first_payload = [1u8, 2, 3, 4];
        let second_payload = [10u8, 20, 30, 40];
        let first =
            synthetic_v4_one_sweep_blob(DataType::UChar, 1, &valid, &ranges, &first_payload);
        let second = synthetic_v4_one_sweep_blob_reusing_previous_mask(
            DataType::UChar,
            1,
            4,
            &ranges,
            &second_payload,
        );

        let mut concatenated = first.clone();
        concatenated.extend_from_slice(&second);

        let decoded = decode_lerc2_bands_supported(&concatenated).unwrap();
        assert_eq!(decoded.bytes_consumed, concatenated.len());
        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(decoded.bands[0].mask, decoded.bands[1].mask);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 2, 3, 0, 4])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 0, 20, 30, 0, 40])
        );
    }

    #[test]
    fn writes_supported_band_decode_data_and_masks_in_band_order() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 20];
        let first_payload = [1u8, 2, 3, 4];
        let second_payload = [10u8, 20, 30, 40];
        let first =
            synthetic_v4_one_sweep_blob(DataType::UChar, 1, &valid, &ranges, &first_payload);
        let second = synthetic_v4_one_sweep_blob_reusing_previous_mask(
            DataType::UChar,
            1,
            4,
            &ranges,
            &second_payload,
        );
        let mut concatenated = first;
        concatenated.extend_from_slice(&second);
        let decoded = decode_lerc2_bands_supported(&concatenated).unwrap();

        let mut data = vec![0; decoded.data_byte_len()];
        assert_eq!(decoded.write_data_le_bytes(&mut data).unwrap(), data.len());
        assert_eq!(data, [1, 0, 2, 3, 0, 4, 10, 0, 20, 30, 0, 40]);

        let mut masks = vec![0; decoded.mask_byte_len()];
        assert_eq!(decoded.write_mask_bytes(&mut masks).unwrap(), masks.len());
        assert_eq!(masks, [1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1]);
        assert!(decoded.write_data_le_bytes(&mut [0; 11]).is_err());
        assert!(decoded.write_mask_bytes(&mut [0; 11]).is_err());
    }

    #[test]
    fn decodes_supported_lerc2_into_c_api_style_buffers() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 20];
        let first_payload = [1u8, 2, 3, 4];
        let second_payload = [10u8, 20, 30, 40];
        let first =
            synthetic_v4_one_sweep_blob(DataType::UChar, 1, &valid, &ranges, &first_payload);
        let second = synthetic_v4_one_sweep_blob_reusing_previous_mask(
            DataType::UChar,
            1,
            4,
            &ranges,
            &second_payload,
        );
        let mut concatenated = first;
        concatenated.extend_from_slice(&second);

        let spec = DecodeIntoSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let mut data = [0u8; 12];
        let mut mask = [0u8; 6];
        let result =
            decode_lerc2_supported_into(&concatenated, spec, &mut data, Some(&mut mask)).unwrap();

        assert_eq!(result.bytes_consumed, concatenated.len());
        assert_eq!(result.data_bytes_written, 12);
        assert_eq!(result.mask_bytes_written, 6);
        assert_eq!(data, [1, 0, 2, 3, 0, 4, 10, 0, 20, 30, 0, 40]);
        assert_eq!(mask, [1, 0, 1, 1, 0, 1]);
    }

    #[test]
    fn decode_into_spec_reports_buffer_lengths_and_overflow() {
        let spec = DecodeIntoSpec {
            data_type: DataType::Float,
            n_depth: 2,
            n_cols: 3,
            n_rows: 5,
            n_bands: 7,
            n_masks: 1,
        };

        assert_eq!(spec.value_count().unwrap(), 210);
        assert_eq!(spec.data_byte_len().unwrap(), 840);
        assert_eq!(spec.mask_byte_len().unwrap(), 15);

        let overflow = DecodeIntoSpec {
            data_type: DataType::Double,
            n_depth: usize::MAX,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        assert_eq!(
            overflow.value_count().unwrap_err(),
            LercError::WrongParam("decode output value count overflow")
        );
        assert_eq!(
            overflow.data_byte_len().unwrap_err(),
            LercError::WrongParam("decode output byte count overflow")
        );

        let mask_overflow = DecodeIntoSpec {
            n_masks: usize::MAX,
            ..spec
        };
        assert_eq!(
            mask_overflow.mask_byte_len().unwrap_err(),
            LercError::WrongParam("decode mask byte count overflow")
        );
    }

    #[test]
    fn decodes_supported_lerc_dispatches_lerc2_into_c_api_style_buffers() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 20];
        let payload = [1u8, 2, 3, 4];
        let blob = synthetic_v4_one_sweep_blob(DataType::UChar, 1, &valid, &ranges, &payload);
        let spec = DecodeIntoSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let mut data = [0u8; 6];
        let mut mask = [0u8; 6];

        let result = decode_lerc_supported_into(&blob, spec, &mut data, Some(&mut mask)).unwrap();

        assert_eq!(result.bytes_consumed, blob.len());
        assert_eq!(result.data_bytes_written, 6);
        assert_eq!(result.mask_bytes_written, 6);
        assert_eq!(data, [1, 0, 2, 3, 0, 4]);
        assert_eq!(mask, valid);
    }

    #[test]
    fn decodes_supported_lerc_dispatches_lerc1_into_c_api_style_buffers() {
        let blob = fixture("world.lerc1");
        let spec = DecodeIntoSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 257,
            n_rows: 257,
            n_bands: 1,
            n_masks: 1,
        };
        let mut data = vec![0u8; 257 * 257 * 4];
        let mut mask = vec![0u8; 257 * 257];

        let result = decode_lerc_supported_into(&blob, spec, &mut data, Some(&mut mask)).unwrap();

        assert_eq!(result.bytes_consumed, blob.len());
        assert_eq!(result.data_bytes_written, data.len());
        assert_eq!(result.mask_bytes_written, mask.len());
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);

        let values = match crate::decode_typed_values(DataType::Float, &data).unwrap() {
            DecodedData::Float(values) => values,
            _ => unreachable!("requested float output"),
        };
        assert_eq!(values[0], 0.0);
        assert_eq!(values[257 * 257 - 1], 0.0);

        let mut z_min = f32::INFINITY;
        let mut z_max = f32::NEG_INFINITY;
        for (&value, &valid) in values.iter().zip(mask.iter()) {
            if valid != 0 {
                z_min = z_min.min(value);
                z_max = z_max.max(value);
            }
        }
        assert_eq!(z_min, -27.458_635);
        assert_eq!(z_max, 5474.173);
    }

    #[test]
    fn decodes_supported_lerc_to_f64_for_lerc2_and_lerc1() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 20];
        let payload = [1u8, 2, 3, 4];
        let lerc2 = synthetic_v4_one_sweep_blob(DataType::UChar, 1, &valid, &ranges, &payload);
        let lerc2_spec = DecodeIntoSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let mut data = [0.0f64; 6];
        let mut mask = [0u8; 6];
        let result =
            decode_lerc_supported_to_f64(&lerc2, lerc2_spec, &mut data, Some(&mut mask)).unwrap();
        assert_eq!(result.bytes_consumed, lerc2.len());
        assert_eq!(result.values_written, 6);
        assert_eq!(result.mask_bytes_written, 6);
        assert_eq!(data, [1.0, 0.0, 2.0, 3.0, 0.0, 4.0]);
        assert_eq!(mask, valid);

        let lerc1 = fixture("world.lerc1");
        let lerc1_spec = DecodeIntoSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 257,
            n_rows: 257,
            n_bands: 1,
            n_masks: 1,
        };
        let mut data = vec![0.0f64; lerc1_spec.value_count().unwrap()];
        let mut mask = vec![0u8; lerc1_spec.mask_byte_len().unwrap()];
        let result =
            decode_lerc_supported_to_f64(&lerc1, lerc1_spec, &mut data, Some(&mut mask)).unwrap();
        assert_eq!(result.bytes_consumed, lerc1.len());
        assert_eq!(result.values_written, data.len());
        assert_eq!(result.mask_bytes_written, mask.len());
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);

        let mut z_min = f64::INFINITY;
        let mut z_max = f64::NEG_INFINITY;
        for (&value, &valid) in data.iter().zip(mask.iter()) {
            if valid != 0 {
                z_min = z_min.min(value);
                z_max = z_max.max(value);
            }
        }
        assert_eq!(z_min, -27.458_635_330_200_195);
        assert_eq!(z_max, 5474.172_851_562_5);
    }

    #[test]
    fn decode_into_validates_shape_type_masks_and_buffer_sizes() {
        let valid = [1, 0, 1, 1, 0, 1];
        let ranges = [1u8, 20];
        let payload = [1u8, 2, 3, 4];
        let blob = synthetic_v4_one_sweep_blob(DataType::UChar, 1, &valid, &ranges, &payload);
        let spec = DecodeIntoSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };
        let mut data = [0u8; 6];
        let err = decode_lerc2_supported_into(&blob, spec, &mut data, None).unwrap_err();
        assert_eq!(
            err,
            LercError::WrongParam("caller did not provide enough mask buffers for the Lerc2 blob")
        );

        let mut mask = [0u8; 6];
        let mut spec_with_mask = spec;
        spec_with_mask.n_masks = 1;
        let mut bad_type = spec;
        bad_type.data_type = DataType::UInt;
        let err =
            decode_lerc2_supported_into(&blob, bad_type, &mut data, Some(&mut mask)).unwrap_err();
        assert_eq!(
            err,
            LercError::WrongParam("decode output shape does not match the Lerc2 blob")
        );

        let mut bad_shape = spec;
        bad_shape.n_cols = 2;
        let err =
            decode_lerc2_supported_into(&blob, bad_shape, &mut data, Some(&mut mask)).unwrap_err();
        assert_eq!(
            err,
            LercError::WrongParam("decode output shape does not match the Lerc2 blob")
        );

        let mut short_data = [0u8; 5];
        let err =
            decode_lerc2_supported_into(&blob, spec_with_mask, &mut short_data, Some(&mut mask))
                .unwrap_err();
        assert_eq!(err, LercError::BufferTooSmall);

        let mut short_mask = [0u8; 5];
        let err =
            decode_lerc2_supported_into(&blob, spec_with_mask, &mut data, Some(&mut short_mask))
                .unwrap_err();
        assert_eq!(err, LercError::BufferTooSmall);
    }

    #[test]
    fn decodes_v6_no_data_values_to_original_sentinel() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val, 99.0);
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 255, 2, 3, 255, 4, 5, 6, 7, 255, 8, 9])
        );
    }

    #[test]
    fn data_ranges_report_has_no_data_for_v6_multi_depth_no_data() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let err = get_lerc2_data_ranges(&blob).unwrap_err();
        assert_eq!(err, LercError::HasNoData);
        assert_eq!(err.err_code(), crate::ErrCode::HasNoData);
    }

    #[test]
    fn reads_v6_no_data_info_for_single_and_concatenated_bands() {
        let no_data_blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let plain_blob = synthetic_v4_one_sweep_blob(
            DataType::UChar,
            1,
            &[1, 1, 1, 1, 1, 1],
            &[1, 9],
            &[1, 2, 3, 4, 5, 6],
        );

        let single = get_lerc2_no_data_info(&no_data_blob, 1).unwrap();
        assert_eq!(single.uses_no_data, [1]);
        assert_eq!(single.no_data_values, [255.0]);
        assert_eq!(single.bytes_consumed, no_data_blob.len());

        let mut concatenated = no_data_blob.clone();
        concatenated.extend_from_slice(&plain_blob);
        let multi = get_lerc2_no_data_info(&concatenated, 2).unwrap();
        assert_eq!(multi.uses_no_data, [1, 0]);
        assert_eq!(multi.no_data_values, [255.0, 0.0]);
        assert_eq!(multi.bytes_consumed, concatenated.len());

        assert_eq!(
            get_lerc2_no_data_info(&concatenated, 0).unwrap_err(),
            LercError::WrongParam("band count must be positive")
        );
        assert_eq!(
            get_lerc2_no_data_info(&no_data_blob, 2).unwrap_err(),
            LercError::BufferTooSmall
        );
    }

    #[test]
    fn decodes_byte_huffman_fixture_supported_subset() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let decoded = decode_lerc2_bands_supported(&blob).unwrap();

        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.bands.len(), 3);
        for band in &decoded.bands {
            assert_eq!(band.header.data_type, DataType::UChar);
            assert_eq!(band.header.n_cols, 256);
            assert_eq!(band.header.n_rows, 256);
            assert_eq!(band.mask.count_valid_bits(), 43_008);
            match &band.data {
                DecodedData::UChar(values) => {
                    assert_eq!(values.len(), 256 * 256);
                    assert!(values.iter().any(|&value| value == 0));
                    assert!(values.iter().any(|&value| value == 255));
                }
                other => panic!("expected byte decoded data, got {other:?}"),
            }
        }
    }

    #[test]
    fn decodes_single_band_float_fixture_supported_subset() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let decoded = decode_lerc2_supported(&blob).unwrap();

        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.header.data_type, DataType::Float);
        assert_eq!(decoded.mask.count_valid_bits(), 58_515);
        match decoded.data {
            DecodedData::Float(values) => {
                assert_eq!(values.len(), 160_000);
                assert_eq!(values[0], 0.0);
                assert!((values[67] - 1443.2926).abs() < 0.0001);
                assert!((values[68] - 1330.419).abs() < 0.0001);
                assert!((values[435] - 181.57863).abs() < 0.0001);
            }
            other => panic!("expected float decoded data, got {other:?}"),
        }
    }

    #[test]
    fn reads_v5_diff_bit_stuffed_tiled_payload() {
        let blob = synthetic_v5_diff_tiled_blob(diff_bit_stuffed_tile_block(5, &[0, 1, 2, 3]));
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();

        assert_eq!(tiled.data, [10, 15, 20, 26, 30, 37, 40, 48]);
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn reads_v5_diff_lut_tiled_payload() {
        let blob = synthetic_v5_diff_tiled_blob(diff_lut_tile_block(5, &[0, 1, 2, 3]));
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();

        assert_eq!(tiled.data, [10, 15, 20, 26, 30, 37, 40, 48]);
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn reads_v5_diff_constant_tiled_payload() {
        let blob = synthetic_v5_diff_tiled_blob(diff_constant_tile_block(5));
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();

        assert_eq!(tiled.data, [10, 15, 20, 25, 30, 35, 40, 45]);
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn reads_v5_float_diff_bit_stuffed_tiled_payload() {
        let blob = synthetic_v5_float_diff_tiled_blob(diff_float_bit_stuffed_tile_block(
            1.5,
            &[0, 1, 2, 3],
        ));
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();
        let decoded = tiled.decode_typed(DataType::Float).unwrap();

        assert_eq!(
            decoded,
            DecodedData::Float(vec![10.0, 11.5, 20.0, 22.0, 30.0, 32.5, 40.0, 42.0])
        );
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn reads_v5_float_diff_lut_tiled_payload() {
        let blob =
            synthetic_v5_float_diff_tiled_blob(diff_float_lut_tile_block(1.5, &[0, 1, 2, 3]));
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();
        let decoded = tiled.decode_typed(DataType::Float).unwrap();

        assert_eq!(
            decoded,
            DecodedData::Float(vec![10.0, 11.5, 20.0, 22.0, 30.0, 32.5, 40.0, 42.0])
        );
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn reads_v5_float_diff_constant_tiled_payload() {
        let blob = synthetic_v5_float_diff_tiled_blob(diff_float_constant_tile_block(1.5));
        let (_, _, tiled) = read_lerc2_tiled_payload(&blob).unwrap();
        let decoded = tiled.decode_typed(DataType::Float).unwrap();

        assert_eq!(
            decoded,
            DecodedData::Float(vec![10.0, 11.5, 20.0, 21.5, 30.0, 31.5, 40.0, 41.5])
        );
        assert_eq!(tiled.bytes_consumed, blob.len());
    }

    #[test]
    fn rejects_v5_diff_first_depth_tile() {
        let mut blob = synthetic_v5_diff_tiled_blob(diff_constant_tile_block(5));
        let tile_start = blob.len() - (1 + 4) - (1 + 2);
        blob[tile_start] = 6;

        assert!(read_lerc2_tiled_payload(&blob).is_err());
    }

    #[test]
    fn rejects_checksum_mismatch() {
        let mut blob = fixture("california_400_400_1_float.lerc2");
        assert!(validate_lerc2_checksum(&blob).is_ok());

        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert!(validate_lerc2_checksum(&blob).is_err());
    }

    #[test]
    fn detects_truncated_header_and_blob() {
        let blob = fixture("california_400_400_1_float.lerc2");
        assert!(get_lerc2_header_info(&blob[..12]).is_err());
        assert!(get_lerc_info(&blob[..blob.len() - 1]).is_err());
        assert!(read_lerc2_mask(&blob[..80]).is_err());
        assert!(validate_lerc2_checksum(&blob[..blob.len() - 1]).is_err());
    }

    #[test]
    fn rejects_non_lerc2_blob() {
        let blob = fixture("world.lerc1");
        assert!(get_lerc2_header_info(&blob).is_err());
    }
}
