/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Legacy Lerc1 metadata and float payload readers.

use crate::types::{DataType, LercError, Result};
use crate::{BitMask, BitStuffer2, Rle};

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

/// Decoded legacy Lerc1 z-value statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct Lerc1ZStats {
    /// Minimum z value over valid pixels.
    pub z_min: f32,
    /// Maximum z value over valid pixels.
    pub z_max: f32,
    /// Number of valid z values included in the range.
    pub num_valid_pixels: usize,
    /// Number of bytes consumed through the z-value payload.
    pub bytes_consumed: usize,
}

/// Decoded legacy Lerc1 float image.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedLerc1 {
    /// Parsed Lerc1 header metadata.
    pub header: Lerc1HeaderInfo,
    /// Decoded valid-pixel mask metadata.
    pub mask_info: Lerc1MaskInfo,
    /// Decoded float values in row-major order.
    ///
    /// Invalid pixels are left as `0.0`; callers should consult
    /// [`mask_info`](Self::mask_info) before using a pixel value.
    pub values: Vec<f32>,
    /// Number of bytes consumed through the z-value payload.
    pub bytes_consumed: usize,
}

/// Decoded legacy Lerc1 bands.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedLerc1Bands {
    /// Parsed first-band Lerc1 header metadata.
    pub header: Lerc1HeaderInfo,
    /// Decoded shared valid-pixel mask metadata.
    pub mask_info: Lerc1MaskInfo,
    /// Decoded band-major float values.
    ///
    /// Invalid pixels are left as `0.0`; callers should consult
    /// [`mask_info`](Self::mask_info) before using a pixel value.
    pub values: Vec<f32>,
    /// Per-band z-value statistics.
    pub stats: Vec<Lerc1ZStats>,
    /// Number of bands decoded.
    pub n_bands: usize,
    /// Number of bytes consumed through all decoded bands.
    pub bytes_consumed: usize,
}

/// Reads and validates the legacy Lerc1 `CntZImage` header and part headers.
///
/// This is a decode-free structural reader. Use [`read_lerc1_count_mask`],
/// [`read_lerc1_z_stats`], or [`decode_lerc1`] when mask counts, data ranges,
/// or decoded pixel values are needed.
pub fn get_lerc1_header_info(blob: &[u8]) -> Result<Lerc1HeaderInfo> {
    let mut reader = Reader::new(blob);
    let (version, n_rows, n_cols, max_z_error) = read_lerc1_common_header(&mut reader)?;
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

fn read_lerc1_common_header(reader: &mut Reader<'_>) -> Result<(i32, i32, i32, f64)> {
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

    Ok((version, n_rows, n_cols, max_z_error))
}

/// Reads the legacy Lerc1 count part as a valid-pixel mask.
///
/// This covers both the non-tiled count-part form used by the checked-in Lerc1
/// fixture and tiled count parts from older legacy blobs. Count values greater
/// than zero are mapped to valid pixels.
pub fn read_lerc1_count_mask(blob: &[u8]) -> Result<(Lerc1HeaderInfo, Lerc1MaskInfo)> {
    let info = get_lerc1_header_info(blob)?;
    let mut mask = BitMask::new(info.n_cols as usize, info.n_rows as usize)?;

    if info.count_part.num_tiles_vert != 0 || info.count_part.num_tiles_hori != 0 {
        read_lerc1_count_tiles(&info, blob, &mut mask)?;
    } else if info.count_part.num_bytes == 0 {
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

fn read_lerc1_count_tiles(info: &Lerc1HeaderInfo, blob: &[u8], mask: &mut BitMask) -> Result<()> {
    if info.count_part.num_tiles_vert <= 0 || info.count_part.num_tiles_hori <= 0 {
        return Err(LercError::CorruptInput("invalid Lerc1 count tile grid"));
    }

    let payload_start = info.count_part.payload_offset;
    let payload_end = payload_start
        .checked_add(info.count_part.num_bytes as usize)
        .ok_or(LercError::CorruptInput("Lerc1 count payload overflow"))?;
    let payload = blob
        .get(payload_start..payload_end)
        .ok_or(LercError::BufferTooSmall)?;
    let mut reader = Reader::new(payload);

    for (i0, i1) in tile_ranges(
        info.n_rows as usize,
        info.count_part.num_tiles_vert as usize,
    )? {
        for (j0, j1) in tile_ranges(
            info.n_cols as usize,
            info.count_part.num_tiles_hori as usize,
        )? {
            read_lerc1_count_tile(&mut reader, info.n_cols as usize, i0, i1, j0, j1, mask)?;
        }
    }

    if reader.pos != payload.len() {
        return Err(LercError::CorruptInput("unused Lerc1 count payload bytes"));
    }
    Ok(())
}

fn read_lerc1_count_tile(
    reader: &mut Reader<'_>,
    width: usize,
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    mask: &mut BitMask,
) -> Result<()> {
    let raw_flag = reader.read_u8()?;
    if raw_flag == 2 || raw_flag == 3 {
        return Ok(());
    }
    if raw_flag == 4 {
        set_lerc1_count_tile_mask(mask, width, i0, i1, j0, j1, true)?;
        return Ok(());
    }
    if (raw_flag & 63) > 4 {
        return Err(LercError::CorruptInput("invalid Lerc1 count tile flag"));
    }

    if raw_flag == 0 {
        for row in i0..i1 {
            for col in j0..j1 {
                let valid = reader.read_f32_le()? > 0.0;
                if valid {
                    mask.set_valid(row * width + col)?;
                }
            }
        }
        return Ok(());
    }

    let bits67 = raw_flag >> 6;
    let num_bytes = if bits67 == 0 { 4 } else { 3 - bits67 as usize };
    let offset = reader.read_lerc1_float(num_bytes)?;
    let tile_pixel_count = (i1 - i0) * (j1 - j0);
    let (values, consumed) = BitStuffer2::decode(&reader.bytes[reader.pos..], tile_pixel_count, 2)?;
    if values.len() < tile_pixel_count {
        return Err(LercError::CorruptInput("Lerc1 count tile value underrun"));
    }
    reader.pos += consumed;

    let mut src_idx = 0usize;
    for row in i0..i1 {
        for col in j0..j1 {
            if offset + values[src_idx] as f32 > 0.0 {
                mask.set_valid(row * width + col)?;
            }
            src_idx += 1;
        }
    }
    Ok(())
}

fn set_lerc1_count_tile_mask(
    mask: &mut BitMask,
    width: usize,
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    valid: bool,
) -> Result<()> {
    for row in i0..i1 {
        for col in j0..j1 {
            let idx = row * width + col;
            if valid {
                mask.set_valid(idx)?;
            } else {
                mask.set_invalid(idx)?;
            }
        }
    }
    Ok(())
}

/// Reads legacy Lerc1 z tiles and returns min/max statistics for valid pixels.
///
/// This is a stats-only decoder for the Lerc1 z part; it does not expose the
/// full pixel array. Count/mask decoding is performed first so invalid pixels
/// can be skipped the same way as the C++ `CntZImage` reader.
pub fn read_lerc1_z_stats(blob: &[u8]) -> Result<(Lerc1HeaderInfo, Lerc1MaskInfo, Lerc1ZStats)> {
    let (info, mask_info) = read_lerc1_count_mask(blob)?;
    let mut stats = ZStatsBuilder::default();
    let bytes_consumed = read_lerc1_z_tiles(&info, &mask_info.mask, blob, &mut stats)?;
    let stats = stats.finish(bytes_consumed)?;
    Ok((info, mask_info, stats))
}

/// Decodes a legacy Lerc1 blob into row-major `f32` pixels and a valid mask.
///
/// This covers the tiled z-part and non-tiled count/mask layout used by the
/// checked-in Lerc1 fixture. Invalid pixels are represented in the mask and
/// left as `0.0` in the returned value buffer.
pub fn decode_lerc1(blob: &[u8]) -> Result<DecodedLerc1> {
    let (info, mask_info) = read_lerc1_count_mask(blob)?;
    let mut values = vec![0.0f32; (info.n_cols as usize) * (info.n_rows as usize)];
    let bytes_consumed = read_lerc1_z_tiles(
        &info,
        &mask_info.mask,
        blob,
        &mut ZValueWriter {
            values: &mut values,
        },
    )?;

    Ok(DecodedLerc1 {
        header: info,
        mask_info,
        values,
        bytes_consumed,
    })
}

/// Decodes one or more legacy Lerc1 bands into band-major `f32` pixels.
///
/// Legacy multi-band Lerc1 stores the first band as a full `CntZImage` with a
/// count/mask part and stores following bands as z-only `CntZImage` blobs that
/// reuse the first band mask. Invalid pixels are represented in the shared mask
/// and left as `0.0` in each returned band.
pub fn decode_lerc1_bands(blob: &[u8]) -> Result<DecodedLerc1Bands> {
    let first = decode_lerc1(blob)?;
    decode_lerc1_continuation_bands(blob, first, None, false)
}

/// Decodes the requested prefix of legacy Lerc1 bands.
///
/// This mirrors the C++ decode loop, which only reads `nBands` bands from a
/// legacy blob and ignores any later bytes.
pub(crate) fn decode_lerc1_bands_prefix(blob: &[u8], n_bands: usize) -> Result<DecodedLerc1Bands> {
    if n_bands == 0 {
        return Err(LercError::WrongParam("Lerc1 band count must be positive"));
    }
    let first = decode_lerc1(blob)?;
    decode_lerc1_continuation_bands(blob, first, Some(n_bands), false)
}

/// Decodes Lerc1 metadata while tolerating failed continuation bands.
///
/// The C++ `GetLercInfo` path returns metadata for the bands decoded so far
/// when a later legacy z-only continuation cannot be read. Full data decode
/// remains strict through [`decode_lerc1_bands`].
pub(crate) fn decode_lerc1_bands_for_metadata(blob: &[u8]) -> Result<DecodedLerc1Bands> {
    let first = decode_lerc1(blob)?;
    decode_lerc1_continuation_bands(blob, first, None, true)
}

fn decode_lerc1_continuation_bands(
    blob: &[u8],
    first: DecodedLerc1,
    max_bands: Option<usize>,
    tolerate_failed_continuation: bool,
) -> Result<DecodedLerc1Bands> {
    let pixel_count = (first.header.n_cols as usize)
        .checked_mul(first.header.n_rows as usize)
        .ok_or(LercError::CorruptInput("Lerc1 pixel count overflow"))?;
    let first_stats =
        lerc1_z_stats_from_values(&first.values, &first.mask_info.mask, first.bytes_consumed)?;
    let mut values = first.values;
    let mut stats = vec![first_stats];
    let mut offset = first.bytes_consumed;

    while offset < blob.len() && max_bands.is_none_or(|max_bands| stats.len() < max_bands) {
        let (header, band_values, band_stats, consumed) =
            match decode_lerc1_z_only_band(&blob[offset..], &first.mask_info.mask) {
                Ok(decoded) => decoded,
                Err(_err) if tolerate_failed_continuation => break,
                Err(err) => return Err(err),
            };
        if header.n_cols != first.header.n_cols
            || header.n_rows != first.header.n_rows
            || header.max_z_error != first.header.max_z_error
        {
            if tolerate_failed_continuation {
                break;
            }
            return Err(LercError::CorruptInput(
                "concatenated Lerc1 header mismatch",
            ));
        }
        if band_values.len() != pixel_count {
            if tolerate_failed_continuation {
                break;
            }
            return Err(LercError::CorruptInput("Lerc1 band size mismatch"));
        }
        values.extend_from_slice(&band_values);
        stats.push(band_stats);
        offset += consumed;
    }

    if max_bands.is_some_and(|max_bands| stats.len() < max_bands) {
        return Err(LercError::BufferTooSmall);
    }

    Ok(DecodedLerc1Bands {
        header: first.header,
        mask_info: first.mask_info,
        values,
        n_bands: stats.len(),
        stats,
        bytes_consumed: offset,
    })
}

fn decode_lerc1_z_only_band(
    blob: &[u8],
    mask: &BitMask,
) -> Result<(Lerc1HeaderInfo, Vec<f32>, Lerc1ZStats, usize)> {
    let info = get_lerc1_z_only_header_info(blob)?;
    let mut values = vec![0.0f32; (info.n_cols as usize) * (info.n_rows as usize)];
    let mut writer = ZValueWriter {
        values: &mut values,
    };
    let bytes_consumed = read_lerc1_z_tiles(&info, mask, blob, &mut writer)?;
    let stats = lerc1_z_stats_from_values(&values, mask, bytes_consumed)?;
    Ok((info, values, stats, bytes_consumed))
}

fn lerc1_z_stats_from_values(
    values: &[f32],
    mask: &BitMask,
    bytes_consumed: usize,
) -> Result<Lerc1ZStats> {
    if values.len() != mask.pixel_count() {
        return Err(LercError::CorruptInput("Lerc1 value/mask size mismatch"));
    }
    let mut stats = ZStatsBuilder::default();
    for (idx, value) in values.iter().copied().enumerate() {
        if mask.is_valid(idx)? {
            stats.add_value(value);
        }
    }
    stats.finish(bytes_consumed)
}

fn get_lerc1_z_only_header_info(blob: &[u8]) -> Result<Lerc1HeaderInfo> {
    let mut reader = Reader::new(blob);
    let (version, n_rows, n_cols, max_z_error) = read_lerc1_common_header(&mut reader)?;
    let z_part = read_part_info(&mut reader)?;
    reader.skip_payload(z_part.num_bytes as usize)?;

    Ok(Lerc1HeaderInfo {
        version,
        n_rows,
        n_cols,
        max_z_error,
        data_type: DataType::Float,
        count_part: Lerc1PartInfo {
            num_tiles_vert: 0,
            num_tiles_hori: 0,
            num_bytes: 0,
            max_value: 0.0,
            payload_offset: reader.pos,
        },
        z_part,
        blob_size: reader.pos,
    })
}

fn read_lerc1_z_tiles(
    info: &Lerc1HeaderInfo,
    mask: &BitMask,
    blob: &[u8],
    sink: &mut impl ZValueSink,
) -> Result<usize> {
    if info.z_part.num_tiles_vert <= 0 || info.z_part.num_tiles_hori <= 0 {
        return Err(LercError::Unsupported("non-tiled Lerc1 z parts"));
    }

    let payload_start = info.z_part.payload_offset;
    let payload_end = payload_start
        .checked_add(info.z_part.num_bytes as usize)
        .ok_or(LercError::CorruptInput("Lerc1 z payload overflow"))?;
    let payload = blob
        .get(payload_start..payload_end)
        .ok_or(LercError::BufferTooSmall)?;
    let mut reader = Reader::new(payload);

    for (i0, i1) in tile_ranges(info.n_rows as usize, info.z_part.num_tiles_vert as usize)? {
        for (j0, j1) in tile_ranges(info.n_cols as usize, info.z_part.num_tiles_hori as usize)? {
            read_z_tile(&mut reader, info, mask, i0, i1, j0, j1, sink)?;
        }
    }

    if reader.pos != payload.len() {
        return Err(LercError::CorruptInput("unused Lerc1 z payload bytes"));
    }

    Ok(payload_start + reader.pos)
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

fn tile_ranges(size: usize, num_tiles: usize) -> Result<Vec<(usize, usize)>> {
    if size == 0 || num_tiles == 0 || num_tiles > size {
        return Err(LercError::CorruptInput("invalid Lerc1 tile grid"));
    }

    let base = size / num_tiles;
    let remainder = size % num_tiles;
    let mut ranges = Vec::with_capacity(num_tiles + usize::from(remainder > 0));
    for tile in 0..=num_tiles {
        let start = tile * base;
        let len = if tile == num_tiles { remainder } else { base };
        if len > 0 {
            ranges.push((start, start + len));
        }
    }
    Ok(ranges)
}

fn read_z_tile(
    reader: &mut Reader<'_>,
    info: &Lerc1HeaderInfo,
    mask: &BitMask,
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    sink: &mut impl ZValueSink,
) -> Result<()> {
    let raw_flag = reader.read_u8()?;
    let bits67 = raw_flag >> 6;
    let flag = raw_flag & 63;

    if flag == 2 {
        for idx in valid_indexes(mask, info.n_cols as usize, i0, i1, j0, j1)? {
            sink.add(idx, 0.0);
        }
        return Ok(());
    }

    if flag > 3 {
        return Err(LercError::CorruptInput("invalid Lerc1 z tile flag"));
    }

    if flag == 0 {
        for idx in valid_indexes(mask, info.n_cols as usize, i0, i1, j0, j1)? {
            sink.add(idx, reader.read_f32_le()?);
        }
        return Ok(());
    }

    let num_bytes = if bits67 == 0 { 4 } else { 3 - bits67 as usize };
    let offset = reader.read_lerc1_float(num_bytes)?;
    if flag == 3 {
        for idx in valid_indexes(mask, info.n_cols as usize, i0, i1, j0, j1)? {
            sink.add(idx, offset);
        }
        return Ok(());
    }

    let valid_count = valid_indexes(mask, info.n_cols as usize, i0, i1, j0, j1)?.len();
    let (values, consumed) = BitStuffer2::decode(
        &reader.bytes[reader.pos..],
        ((i1 - i0) * (j1 - j0)).max(valid_count),
        2,
    )?;
    if values.len() < valid_count {
        return Err(LercError::CorruptInput("Lerc1 z tile value underrun"));
    }
    reader.pos += consumed;

    let inv_scale = (2.0 * info.max_z_error) as f32;
    for (idx, value) in valid_indexes(mask, info.n_cols as usize, i0, i1, j0, j1)?
        .into_iter()
        .zip(values.iter().take(valid_count))
    {
        sink.add(
            idx,
            (offset + *value as f32 * inv_scale).min(info.z_part.max_value),
        );
    }
    Ok(())
}

fn valid_indexes(
    mask: &BitMask,
    width: usize,
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
) -> Result<Vec<usize>> {
    let mut indexes = Vec::new();
    for row in i0..i1 {
        for col in j0..j1 {
            let idx = row * width + col;
            if mask.is_valid(idx)? {
                indexes.push(idx);
            }
        }
    }
    Ok(indexes)
}

#[derive(Default)]
struct ZStatsBuilder {
    z_min: f32,
    z_max: f32,
    count: usize,
}

trait ZValueSink {
    fn add(&mut self, idx: usize, value: f32);
}

impl ZValueSink for ZStatsBuilder {
    fn add(&mut self, _idx: usize, value: f32) {
        self.add_value(value);
    }
}

impl ZStatsBuilder {
    fn add_value(&mut self, value: f32) {
        if self.count == 0 {
            self.z_min = value;
            self.z_max = value;
        } else {
            self.z_min = self.z_min.min(value);
            self.z_max = self.z_max.max(value);
        }
        self.count += 1;
    }

    fn finish(self, bytes_consumed: usize) -> Result<Lerc1ZStats> {
        if self.count == 0 {
            return Err(LercError::CorruptInput(
                "Lerc1 z stats contain no valid pixels",
            ));
        }
        Ok(Lerc1ZStats {
            z_min: self.z_min,
            z_max: self.z_max,
            num_valid_pixels: self.count,
            bytes_consumed,
        })
    }
}

struct ZValueWriter<'a> {
    values: &'a mut [f32],
}

impl ZValueSink for ZValueWriter<'_> {
    fn add(&mut self, idx: usize, value: f32) {
        self.values[idx] = value;
    }
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

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_bytes(1)?[0])
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

    fn read_lerc1_float(&mut self, len: usize) -> Result<f32> {
        match len {
            1 => Ok(i8::from_le_bytes([self.read_u8()?]) as f32),
            2 => {
                let bytes = self.read_bytes(2)?;
                Ok(i16::from_le_bytes(bytes.try_into().unwrap()) as f32)
            }
            4 => self.read_f32_le(),
            _ => Err(LercError::CorruptInput("invalid Lerc1 float byte width")),
        }
    }

    fn skip_payload(&mut self, len: usize) -> Result<()> {
        self.read_bytes(len)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_lerc1, decode_lerc1_bands, get_lerc1_header_info, read_lerc1_count_mask,
        read_lerc1_z_stats,
    };
    use crate::{
        decode_lerc_supported_into, get_lerc2_data_ranges, get_lerc_info, BitStuffer2, DataType,
        DecodeIntoSpec,
    };
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("testData");
        path.push(name);
        fs::read(path).unwrap()
    }

    fn synthetic_lerc1_with_count_part(
        n_cols: i32,
        n_rows: i32,
        num_tiles_vert: i32,
        num_tiles_hori: i32,
        count_payload: &[u8],
        count_max: f32,
    ) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend_from_slice(super::CNT_Z_IMAGE_KEY);
        blob.extend_from_slice(&11i32.to_le_bytes());
        blob.extend_from_slice(&8i32.to_le_bytes());
        blob.extend_from_slice(&n_rows.to_le_bytes());
        blob.extend_from_slice(&n_cols.to_le_bytes());
        blob.extend_from_slice(&0.1f64.to_le_bytes());
        blob.extend_from_slice(&num_tiles_vert.to_le_bytes());
        blob.extend_from_slice(&num_tiles_hori.to_le_bytes());
        blob.extend_from_slice(&(count_payload.len() as i32).to_le_bytes());
        blob.extend_from_slice(&count_max.to_le_bytes());
        blob.extend_from_slice(count_payload);
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob
    }

    fn raw_z_tile(values: &[f32]) -> Vec<u8> {
        let mut payload = vec![0u8];
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload
    }

    fn append_lerc1_common_header(blob: &mut Vec<u8>, n_cols: i32, n_rows: i32) {
        blob.extend_from_slice(super::CNT_Z_IMAGE_KEY);
        blob.extend_from_slice(&11i32.to_le_bytes());
        blob.extend_from_slice(&8i32.to_le_bytes());
        blob.extend_from_slice(&n_rows.to_le_bytes());
        blob.extend_from_slice(&n_cols.to_le_bytes());
        blob.extend_from_slice(&0.1f64.to_le_bytes());
    }

    fn append_lerc1_z_part(blob: &mut Vec<u8>, z_payload: &[u8], z_max: f32) {
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.extend_from_slice(&(z_payload.len() as i32).to_le_bytes());
        blob.extend_from_slice(&z_max.to_le_bytes());
        blob.extend_from_slice(z_payload);
    }

    fn synthetic_lerc1_two_band_blob() -> Vec<u8> {
        let first_z = raw_z_tile(&[1.0, 2.0, 3.0, 4.0]);
        let second_z = raw_z_tile(&[10.0, 20.0, 30.0, 40.0]);
        let mut blob = Vec::new();

        append_lerc1_common_header(&mut blob, 2, 2);
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        blob.extend_from_slice(&1.0f32.to_le_bytes());
        append_lerc1_z_part(&mut blob, &first_z, 4.0);

        append_lerc1_common_header(&mut blob, 2, 2);
        append_lerc1_z_part(&mut blob, &second_z, 40.0);
        blob
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
    fn reads_tiled_lerc1_count_mask_constants_and_raw_counts() {
        let mut count_payload = Vec::new();
        count_payload.push(4); // first 2x2 tile is constant valid
        count_payload.push(0); // second 2x2 tile stores raw float counts
        for value in [0.0f32, 1.0, -1.0, 2.0] {
            count_payload.extend_from_slice(&value.to_le_bytes());
        }
        let blob = synthetic_lerc1_with_count_part(4, 2, 1, 2, &count_payload, 2.0);
        let (header, mask_info) = read_lerc1_count_mask(&blob).unwrap();

        assert_eq!(header.count_part.num_tiles_vert, 1);
        assert_eq!(header.count_part.num_tiles_hori, 2);
        assert_eq!(mask_info.mask.to_byte_mask(), [1, 1, 0, 1, 1, 1, 0, 1]);
        assert_eq!(mask_info.mask.count_valid_bits(), 6);
        assert!(!mask_info.all_valid);
    }

    #[test]
    fn reads_tiled_lerc1_count_mask_bit_stuffed_counts() {
        let mut count_payload = Vec::new();
        count_payload.push(0b1000_0001); // one-byte offset plus bit-stuffed counts
        count_payload.push(0); // offset
        count_payload.extend_from_slice(&BitStuffer2::encode_simple(&[0, 1, 2, 0], 2).unwrap());
        let blob = synthetic_lerc1_with_count_part(4, 1, 1, 1, &count_payload, 2.0);
        let (_, mask_info) = read_lerc1_count_mask(&blob).unwrap();

        assert_eq!(mask_info.mask.to_byte_mask(), [0, 1, 1, 0]);
        assert_eq!(mask_info.mask.count_valid_bits(), 2);
        assert!(!mask_info.all_valid);
    }

    #[test]
    fn reads_world_lerc1_z_stats() {
        let blob = fixture("world.lerc1");
        let (_, mask_info, stats) = read_lerc1_z_stats(&blob).unwrap();

        assert_eq!(stats.num_valid_pixels, mask_info.mask.count_valid_bits());
        assert_eq!(stats.bytes_consumed, blob.len());
        assert_eq!(stats.z_min, -27.458_635);
        assert_eq!(stats.z_max, 5474.173);
    }

    #[test]
    fn decodes_world_lerc1_float_values() {
        let blob = fixture("world.lerc1");
        let decoded = decode_lerc1(&blob).unwrap();

        assert_eq!(decoded.header.n_cols, 257);
        assert_eq!(decoded.header.n_rows, 257);
        assert_eq!(decoded.values.len(), 257 * 257);
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.mask_info.mask.count_valid_bits(), 65_025);
        assert_eq!(decoded.values[0], 0.0);
        assert_eq!(decoded.values[257 * 257 - 1], 0.0);

        let mut z_min = f32::INFINITY;
        let mut z_max = f32::NEG_INFINITY;
        let mut invalid_non_zero = 0usize;
        for (idx, value) in decoded.values.iter().copied().enumerate() {
            if decoded.mask_info.mask.is_valid(idx).unwrap() {
                z_min = z_min.min(value);
                z_max = z_max.max(value);
            } else if value != 0.0 {
                invalid_non_zero += 1;
            }
        }

        assert_eq!(invalid_non_zero, 0);
        assert_eq!(z_min, -27.458_635);
        assert_eq!(z_max, 5474.173);
    }

    #[test]
    fn decodes_concatenated_lerc1_z_only_bands() {
        let blob = synthetic_lerc1_two_band_blob();
        let decoded = decode_lerc1_bands(&blob).unwrap();

        assert_eq!(decoded.n_bands, 2);
        assert_eq!(decoded.header.n_cols, 2);
        assert_eq!(decoded.header.n_rows, 2);
        assert_eq!(decoded.mask_info.mask.count_valid_bits(), 4);
        assert_eq!(decoded.values, [1.0, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 40.0]);
        assert_eq!(decoded.stats[0].z_min, 1.0);
        assert_eq!(decoded.stats[0].z_max, 4.0);
        assert_eq!(decoded.stats[1].z_min, 10.0);
        assert_eq!(decoded.stats[1].z_max, 40.0);
        assert_eq!(decoded.bytes_consumed, blob.len());
    }

    #[test]
    fn reports_and_decodes_concatenated_lerc1_bands_through_supported_dispatch() {
        let blob = synthetic_lerc1_two_band_blob();
        let info = get_lerc_info(&blob).unwrap();
        let ranges = get_lerc2_data_ranges(&blob).unwrap();
        let spec = DecodeIntoSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 2,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let mut data = vec![0u8; spec.data_byte_len().unwrap()];
        let mut mask = vec![0u8; spec.mask_byte_len().unwrap()];
        let decoded = decode_lerc_supported_into(&blob, spec, &mut data, Some(&mut mask)).unwrap();

        assert_eq!(info.version, 0);
        assert_eq!(info.n_bands, 2);
        assert_eq!(info.blob_size as usize, blob.len());
        assert_eq!(info.z_min, 1.0);
        assert_eq!(info.z_max, 40.0);
        assert_eq!(ranges.n_bands, 2);
        assert_eq!(ranges.mins, [1.0, 10.0]);
        assert_eq!(ranges.maxs, [4.0, 40.0]);
        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.mask_bytes_written, 4);
        assert_eq!(mask, [1, 1, 1, 1]);
        let expected = [1.0f32, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 40.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(data, expected);
    }

    #[test]
    fn decodes_prefix_of_concatenated_lerc1_bands_through_supported_dispatch() {
        let blob = synthetic_lerc1_two_band_blob();
        let spec = DecodeIntoSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 2,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let mut data = vec![0u8; spec.data_byte_len().unwrap()];
        let mut mask = vec![0u8; spec.mask_byte_len().unwrap()];
        let decoded = decode_lerc_supported_into(&blob, spec, &mut data, Some(&mut mask)).unwrap();

        assert_eq!(decoded.bytes_consumed, blob.len());
        assert_eq!(decoded.data_bytes_written, 4 * std::mem::size_of::<f32>());
        assert_eq!(decoded.mask_bytes_written, 4);
        assert_eq!(mask, [1, 1, 1, 1]);
        let expected = [1.0f32, 2.0, 3.0, 4.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(data, expected);
    }

    #[test]
    fn lerc1_metadata_tolerates_failed_continuation_after_valid_bands() {
        let mut blob = synthetic_lerc1_two_band_blob();
        let valid_len = blob.len();
        blob.extend_from_slice(&[0u8; 48]);

        let info = get_lerc_info(&blob).unwrap();
        let ranges = get_lerc2_data_ranges(&blob).unwrap();

        assert_eq!(info.n_bands, 2);
        assert_eq!(info.blob_size as usize, valid_len);
        assert_eq!(info.z_min, 1.0);
        assert_eq!(info.z_max, 40.0);
        assert_eq!(ranges.n_bands, 2);
        assert_eq!(ranges.bytes_consumed, valid_len);
        assert_eq!(ranges.mins, [1.0, 10.0]);
        assert_eq!(ranges.maxs, [4.0, 40.0]);
        assert!(decode_lerc1_bands(&blob).is_err());
    }

    #[test]
    fn lerc1_decode_reads_only_requested_prefix_bands() {
        let mut blob = synthetic_lerc1_two_band_blob();
        let valid_len = blob.len();
        blob.extend_from_slice(&[0u8; 48]);
        let spec = DecodeIntoSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 2,
            n_rows: 2,
            n_bands: 2,
            n_masks: 1,
        };
        let mut data = vec![0u8; spec.data_byte_len().unwrap()];
        let mut mask = vec![0u8; spec.mask_byte_len().unwrap()];

        let decoded = decode_lerc_supported_into(&blob, spec, &mut data, Some(&mut mask)).unwrap();

        assert_eq!(decoded.bytes_consumed, valid_len);
        assert_eq!(decoded.data_bytes_written, 8 * std::mem::size_of::<f32>());
        assert_eq!(decoded.mask_bytes_written, 4);
        assert_eq!(mask, [1, 1, 1, 1]);
        let expected = [1.0f32, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 40.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(data, expected);

        let mut too_many = spec;
        too_many.n_bands = 3;
        too_many.n_masks = 1;
        let mut data = vec![0u8; too_many.data_byte_len().unwrap()];
        assert!(decode_lerc_supported_into(&blob, too_many, &mut data, Some(&mut mask)).is_err());
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
