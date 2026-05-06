/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

use crate::types::{DataType, LercError, Result};
use crate::{decode_typed_values, DecodedData};
use crate::{BitMask, BitStuffer2, Rle};

pub const CURRENT_VERSION: i32 = 6;
pub const FILE_KEY: &[u8; 6] = b"Lerc2 ";
const CHECKSUM_START_OFFSET: usize = FILE_KEY.len() + 4 + 4;

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderInfo {
    pub version: i32,
    pub checksum: u32,
    pub n_rows: i32,
    pub n_cols: i32,
    pub n_depth: i32,
    pub num_valid_pixel: i32,
    pub micro_block_size: i32,
    pub blob_size: i32,
    pub n_blobs_more: i32,
    pub b_pass_no_data_values: u8,
    pub b_is_int: u8,
    pub b_reserved_3: u8,
    pub b_reserved_4: u8,
    pub data_type: DataType,
    pub max_z_error: f64,
    pub z_min: f64,
    pub z_max: f64,
    pub no_data_val: f64,
    pub no_data_val_orig: f64,
    pub header_size: usize,
}

impl HeaderInfo {
    pub fn has_no_data_values(&self) -> bool {
        self.b_pass_no_data_values != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderProbe {
    pub header: HeaderInfo,
    pub has_mask: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskInfo {
    pub mask: BitMask,
    pub num_bytes_mask: i32,
    pub bytes_consumed: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MinMaxRanges {
    pub mins: Vec<f64>,
    pub maxs: Vec<f64>,
    pub bytes_consumed: usize,
    pub min_max_equal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataOneSweep {
    pub data: Vec<u8>,
    pub bytes_consumed: usize,
}

impl DataOneSweep {
    pub fn decode_typed(&self, data_type: DataType) -> Result<DecodedData> {
        decode_typed_values(data_type, &self.data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiledData {
    pub data: Vec<u8>,
    pub bytes_consumed: usize,
}

impl TiledData {
    pub fn decode_typed(&self, data_type: DataType) -> Result<DecodedData> {
        decode_typed_values(data_type, &self.data)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LercInfo {
    pub version: i32,
    pub n_depth: i32,
    pub n_cols: i32,
    pub n_rows: i32,
    pub num_valid_pixel: i32,
    pub n_bands: i32,
    pub blob_size: i32,
    pub n_masks: i32,
    pub n_uses_no_data_value: i32,
    pub data_type: DataType,
    pub z_min: f64,
    pub z_max: f64,
    pub max_z_error: f64,
}

pub fn get_lerc2_header_info(blob: &[u8]) -> Result<HeaderProbe> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask_bytes = reader.peek_i32_le()?;
    Ok(HeaderProbe {
        header,
        has_mask: mask_bytes > 0,
    })
}

pub fn read_lerc2_mask(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo)> {
    read_lerc2_mask_with_previous(blob, None)
}

pub fn read_lerc2_mask_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo)> {
    let mut reader = Reader::new(blob);
    let header = read_header(&mut reader)?;
    let mask = read_mask(&mut reader, &header, previous_mask)?;
    Ok((header, mask))
}

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

pub fn read_lerc2_min_max_ranges(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, MinMaxRanges)> {
    read_lerc2_min_max_ranges_with_previous(blob, None)
}

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

pub fn read_lerc2_data_one_sweep(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, DataOneSweep)> {
    read_lerc2_data_one_sweep_with_previous(blob, None)
}

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

pub fn read_lerc2_tiled_raw(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    read_lerc2_tiled_raw_with_previous(blob, None)
}

pub fn read_lerc2_tiled_payload(blob: &[u8]) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    read_lerc2_tiled_payload_with_previous(blob, None)
}

pub fn read_lerc2_tiled_raw_with_previous(
    blob: &[u8],
    previous_mask: Option<&BitMask>,
) -> Result<(HeaderInfo, MaskInfo, TiledData)> {
    read_lerc2_tiled_payload_with_previous(blob, previous_mask)
}

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

    let data = read_tiled_payload(&mut reader, &header, &mask.mask, ranges.as_ref())?;
    Ok((header, mask, data))
}

pub fn get_lerc_info(blob: &[u8]) -> Result<LercInfo> {
    let first = get_lerc2_header_info(blob)?;
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

#[cfg(test)]
mod tests {
    use super::{
        compute_checksum_fletcher32, get_lerc2_header_info, get_lerc_info,
        read_lerc2_data_one_sweep, read_lerc2_mask, read_lerc2_mask_with_previous,
        read_lerc2_min_max_ranges, read_lerc2_tiled_payload, read_lerc2_tiled_raw,
        validate_lerc2_checksum, FILE_KEY,
    };
    use crate::{BitStuffer2, DataType, DecodedData, Rle};
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("testData");
        path.push(name);
        fs::read(path).unwrap()
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

    fn diff_constant_tile_block(offset: i16) -> Vec<u8> {
        let mut block = vec![(2 << 6) | 4 | 3];
        block.extend_from_slice(&offset.to_le_bytes());
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
    fn reads_v5_diff_bit_stuffed_tiled_payload() {
        let blob = synthetic_v5_diff_tiled_blob(diff_bit_stuffed_tile_block(5, &[0, 1, 2, 3]));
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
