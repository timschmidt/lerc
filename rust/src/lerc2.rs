/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Lerc2 metadata readers and supported-subset decoders.

use crate::types::{DataType, LercError, Result};
use crate::{decode_lerc1, decode_typed_values, read_lerc1_z_stats, DecodedData, CNT_Z_IMAGE_KEY};
use crate::{BitMask, BitStuffer2, Rle};

/// Highest Lerc2 codec version recognized by this crate.
pub const CURRENT_VERSION: i32 = 6;
/// ASCII file key that starts every Lerc2 blob.
pub const FILE_KEY: &[u8; 6] = b"Lerc2 ";
/// Number of integers currently produced by the C API blob-info array.
pub const BLOB_INFO_ARRAY_LEN: usize = 11;
/// Number of doubles currently produced by the C API data-range summary array.
pub const BLOB_DATA_RANGE_ARRAY_LEN: usize = 3;
const CHECKSUM_START_OFFSET: usize = FILE_KEY.len() + 4 + 4;

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
    let (_, _, stats) = read_lerc1_z_stats(blob)?;
    Ok(DataRanges {
        mins: vec![stats.z_min as f64],
        maxs: vec![stats.z_max as f64],
        n_bands: 1,
        n_depth: 1,
        bytes_consumed: stats.bytes_consumed,
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

fn decode_lerc1_supported_into(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data_output: &mut [u8],
    mask_output: Option<&mut [u8]>,
) -> Result<DecodeIntoResult> {
    let decoded = decode_lerc1(blob)?;
    if spec.data_type != DataType::Float
        || spec.n_depth != 1
        || spec.n_bands != 1
        || spec.n_cols != decoded.header.n_cols as usize
        || spec.n_rows != decoded.header.n_rows as usize
        || !(spec.n_masks == 0 || spec.n_masks == 1)
    {
        return Err(LercError::WrongParam("Lerc1 decode shape/type mismatch"));
    }

    let data_bytes_written = DecodedData::Float(decoded.values).write_le_bytes(data_output)?;
    let mask_bytes_written = if let Some(mask_output) = mask_output {
        let byte_mask = decoded.mask_info.mask.to_byte_mask();
        if mask_output.len() < byte_mask.len() {
            return Err(LercError::BufferTooSmall);
        }
        mask_output[..byte_mask.len()].copy_from_slice(&byte_mask);
        byte_mask.len()
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
                return Err(LercError::Unsupported(
                    "Lerc2 Huffman-backed image modes are not ported yet",
                ));
            }
        }
        read_tiled_payload(&mut reader, &header, &mask_info.mask, ranges.as_ref())?.data
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
    let (header, mask, stats) = read_lerc1_z_stats(blob)?;
    Ok(LercInfo {
        version: 0,
        n_depth: 1,
        n_cols: header.n_cols,
        n_rows: header.n_rows,
        num_valid_pixel: stats.num_valid_pixels as i32,
        n_bands: 1,
        blob_size: stats.bytes_consumed as i32,
        n_masks: if mask.all_valid { 0 } else { 1 },
        n_uses_no_data_value: 0,
        data_type: DataType::Float,
        z_min: stats.z_min as f64,
        z_max: stats.z_max as f64,
        max_z_error: header.max_z_error,
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

#[cfg(test)]
mod tests {
    use super::{
        compute_checksum_fletcher32, decode_lerc2_bands_supported, decode_lerc2_supported,
        decode_lerc2_supported_into, decode_lerc_supported_into, get_lerc2_blob_info_arrays,
        get_lerc2_data_ranges, get_lerc2_header_info, get_lerc2_no_data_info, get_lerc_info,
        read_lerc2_data_one_sweep, read_lerc2_mask, read_lerc2_mask_with_previous,
        read_lerc2_min_max_ranges, read_lerc2_tiled_payload, read_lerc2_tiled_raw,
        validate_lerc2_checksum, DecodeIntoSpec, BLOB_DATA_RANGE_ARRAY_LEN, BLOB_INFO_ARRAY_LEN,
        FILE_KEY,
    };
    use crate::{BitStuffer2, DataType, DecodedData, LercError, Rle};
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
    fn reports_unsupported_huffman_fixture_in_dispatcher() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        assert!(decode_lerc2_supported(&blob).is_err());
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
