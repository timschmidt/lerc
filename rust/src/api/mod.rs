//! Workflow-oriented safe Rust API facade.
//!
//! This module layers allocation-friendly encode, decode, metadata, and range
//! helpers over the lower-level format modules. The functions intentionally
//! expose the currently supported Rust codec subset while keeping the public C
//! ABI wrappers separate in [`crate::c_api`].

use crate::format::lerc1::{decode_lerc1_bands, DecodedLerc1Bands, CNT_Z_IMAGE_KEY};
use crate::format::lerc2::{
    decode_lerc2_bands_supported, decode_lerc_supported_into, decode_lerc_supported_to_f64,
    encode_lerc2_auto, encode_lerc2_auto_with_no_data, get_lerc2_blob_info_arrays,
    get_lerc2_data_ranges, get_lerc2_no_data_info, get_lerc_info, DataRanges, DecodeIntoResult,
    DecodeIntoSpec, DecodeToF64Result, LercInfo, NoDataInfo,
};
use crate::support::{prepare_active_no_data_nan_encode_inputs, prepare_nan_encode_inputs};
use crate::{convert_typed_bytes_to_f64, DataType, EncodeSpec, LercError, Result};

/// Latest Lerc2 codec version supported by the Rust encoder facade.
pub const DEFAULT_CODEC_VERSION: i32 = 6;

/// Decoded LERC data returned by [`decode`].
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedLerc {
    /// Legacy Lerc1 `CntZImage` bands.
    Lerc1(DecodedLerc1Bands),
    /// Lerc2 bands decoded by the supported Rust subset.
    Lerc2(crate::DecodedLerc2Bands),
}

impl DecodedLerc {
    /// Returns the number of decoded bands.
    pub fn n_bands(&self) -> usize {
        match self {
            Self::Lerc1(decoded) => decoded.n_bands,
            Self::Lerc2(decoded) => decoded.bands.len(),
        }
    }

    /// Returns the number of bytes consumed from the input blob.
    pub fn bytes_consumed(&self) -> usize {
        match self {
            Self::Lerc1(decoded) => decoded.bytes_consumed,
            Self::Lerc2(decoded) => decoded.bytes_consumed,
        }
    }
}

/// Allocated output returned by [`decode_into`] and [`decode_to_f64`].
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedBuffer<T> {
    /// Decoded sample values in band-major order.
    pub data: Vec<T>,
    /// Decoded byte masks in row-major order, when requested by the spec.
    pub masks: Option<Vec<u8>>,
    /// Number of bytes consumed from the input blob.
    pub bytes_consumed: usize,
    /// Number of mask bytes written.
    pub mask_bytes_written: usize,
}

/// Allocated output returned by [`decode_4d_into`] and [`decode_4d_to_f64`].
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded4DBuffer<T> {
    /// Decoded sample values in band-major order.
    pub data: Vec<T>,
    /// Decoded byte masks in row-major order, when requested by the spec.
    pub masks: Option<Vec<u8>>,
    /// Per-band no-data metadata.
    pub no_data: NoDataInfo,
    /// Number of bytes consumed from the input blob.
    pub bytes_consumed: usize,
    /// Number of mask bytes written.
    pub mask_bytes_written: usize,
}

/// Computes the compressed byte count for [`encode`].
pub fn compute_compressed_size(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
) -> Result<usize> {
    encode(spec, data, max_z_error, masks).map(|blob| blob.len())
}

/// Computes the compressed byte count for [`encode_for_version`].
pub fn compute_compressed_size_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<usize> {
    encode_for_version(spec, data, max_z_error, masks, version).map(|blob| blob.len())
}

/// Computes the compressed byte count for [`encode_4d`].
pub fn compute_compressed_size_4d(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
) -> Result<usize> {
    encode_4d(spec, data, max_z_error, masks, uses_no_data, no_data_values).map(|blob| blob.len())
}

/// Computes the compressed byte count for [`encode_4d_for_version`].
pub fn compute_compressed_size_4d_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<usize> {
    encode_4d_for_version(
        spec,
        data,
        max_z_error,
        masks,
        uses_no_data,
        no_data_values,
        version,
    )
    .map(|blob| blob.len())
}

/// Encodes a Lerc2 blob using the default supported codec version.
pub fn encode(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
) -> Result<Vec<u8>> {
    encode_for_version(spec, data, max_z_error, masks, DEFAULT_CODEC_VERSION)
}

/// Encodes a Lerc2 blob into a caller-provided output buffer.
///
/// Returns the number of bytes written. This mirrors the public C++ `Lerc::Encode`
/// buffer contract while using the default supported codec version.
pub fn encode_into(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    output: &mut [u8],
) -> Result<usize> {
    encode_into_for_version(
        spec,
        data,
        max_z_error,
        masks,
        DEFAULT_CODEC_VERSION,
        output,
    )
}

/// Encodes a Lerc2 blob using a specific codec version.
pub fn encode_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_version(version)?;
    let prepared_nan = prepare_nan_encode_inputs(spec, data, masks)?;
    let (spec, data, masks) = if let Some(prepared) = prepared_nan.as_ref() {
        (
            prepared.spec,
            prepared.data.as_slice(),
            Some(prepared.masks.as_slice()),
        )
    } else {
        (spec, data, masks)
    };
    encode_lerc2_auto(spec, data, max_z_error, masks, version)
}

/// Encodes a Lerc2 blob into a caller-provided output buffer using a specific version.
///
/// Returns the number of bytes written.
pub fn encode_into_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
    output: &mut [u8],
) -> Result<usize> {
    let blob = encode_for_version(spec, data, max_z_error, masks, version)?;
    if output.len() < blob.len() {
        return Err(LercError::BufferTooSmall);
    }
    output[..blob.len()].copy_from_slice(&blob);
    Ok(blob.len())
}

/// Encodes a Lerc2 blob with optional 4D no-data metadata.
///
/// When `uses_no_data` is absent or all zeros, this follows [`encode`]. When
/// any band is active, `no_data_values` must contain one sentinel per band.
pub fn encode_4d(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
) -> Result<Vec<u8>> {
    encode_4d_for_version(
        spec,
        data,
        max_z_error,
        masks,
        uses_no_data,
        no_data_values,
        DEFAULT_CODEC_VERSION,
    )
}

/// Encodes a Lerc2 blob with optional 4D no-data metadata into `output`.
///
/// Returns the number of bytes written. This mirrors the public C++ `Lerc::Encode`
/// buffer contract while using the default supported codec version.
pub fn encode_4d_into(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    output: &mut [u8],
) -> Result<usize> {
    encode_4d_into_for_version(
        spec,
        data,
        max_z_error,
        masks,
        uses_no_data,
        no_data_values,
        DEFAULT_CODEC_VERSION,
        output,
    )
}

/// Encodes a Lerc2 blob with optional 4D no-data metadata and an explicit version.
pub fn encode_4d_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_version(version)?;
    let active_no_data = uses_no_data.is_some_and(|uses| uses.iter().any(|&value| value != 0));
    if active_no_data {
        validate_active_no_data_inputs(spec, uses_no_data, no_data_values, version)?;
        let prepared_nan = prepare_active_no_data_nan_encode_inputs(
            spec,
            data,
            masks,
            uses_no_data.unwrap_or(&[]),
            no_data_values,
        )?;
        let (spec, data, masks) = if let Some(prepared) = prepared_nan.as_ref() {
            (
                prepared.spec,
                prepared.data.as_slice(),
                Some(prepared.masks.as_slice()),
            )
        } else {
            (spec, data, masks)
        };
        encode_lerc2_auto_with_no_data(
            spec,
            data,
            max_z_error,
            masks,
            uses_no_data,
            no_data_values,
            version,
        )
    } else {
        encode_for_version(spec, data, max_z_error, masks, version)
    }
}

/// Encodes a Lerc2 blob with optional 4D no-data metadata into `output`.
///
/// Returns the number of bytes written.
pub fn encode_4d_into_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
    output: &mut [u8],
) -> Result<usize> {
    let blob = encode_4d_for_version(
        spec,
        data,
        max_z_error,
        masks,
        uses_no_data,
        no_data_values,
        version,
    )?;
    if output.len() < blob.len() {
        return Err(LercError::BufferTooSmall);
    }
    output[..blob.len()].copy_from_slice(&blob);
    Ok(blob.len())
}

/// Converts raw typed scalar bytes into allocated `f64` values.
///
/// This is the allocation-friendly equivalent of the C++ `Lerc::ConvertToDouble`
/// helper. `data` must contain little-endian scalar values of `data_type`.
/// Like the C++ helper, `Double` input is rejected because it is already double.
pub fn convert_to_double(data_type: DataType, data: &[u8]) -> Result<Vec<f64>> {
    let value_count = data.len() / data_type.size_in_bytes();
    let mut output = vec![0.0; value_count];
    let written = convert_to_double_into(data_type, data, &mut output)?;
    output.truncate(written);
    Ok(output)
}

/// Converts raw typed scalar bytes into caller-provided `f64` output.
///
/// Returns the number of values written. This mirrors the C++ `Lerc::ConvertToDouble`
/// helper for non-empty non-`Double` input while avoiding allocation.
pub fn convert_to_double_into(
    data_type: DataType,
    data: &[u8],
    output: &mut [f64],
) -> Result<usize> {
    convert_typed_bytes_to_f64(data_type, data, output)
}

/// Decodes a supported LERC blob into native typed band data.
pub fn decode(blob: &[u8]) -> Result<DecodedLerc> {
    if blob.starts_with(CNT_Z_IMAGE_KEY) {
        decode_lerc1_bands(blob).map(DecodedLerc::Lerc1)
    } else {
        decode_lerc2_bands_supported(blob).map(DecodedLerc::Lerc2)
    }
}

/// Decodes a supported LERC blob into allocated little-endian scalar bytes.
pub fn decode_into(blob: &[u8], spec: DecodeIntoSpec) -> Result<DecodedBuffer<u8>> {
    let mut data = vec![0u8; spec.data_byte_len()?];
    let mut masks = if spec.n_masks > 0 {
        Some(vec![0u8; spec.mask_byte_len()?])
    } else {
        None
    };
    let result = decode_lerc_supported_into(blob, spec, &mut data, masks.as_deref_mut())?;
    data.truncate(result.data_bytes_written);
    if let Some(mask_bytes) = masks.as_mut() {
        mask_bytes.truncate(result.mask_bytes_written);
    }
    Ok(DecodedBuffer {
        data,
        masks,
        bytes_consumed: result.bytes_consumed,
        mask_bytes_written: result.mask_bytes_written,
    })
}

/// Decodes a supported LERC blob into caller-provided little-endian scalar bytes.
///
/// `data` must have room for `spec.data_byte_len()` bytes. When `spec.n_masks`
/// is nonzero, `masks` must have room for `spec.mask_byte_len()` bytes. The
/// returned result reports how many data and mask bytes were written.
pub fn decode_into_buffers(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data: &mut [u8],
    masks: Option<&mut [u8]>,
) -> Result<DecodeIntoResult> {
    decode_lerc_supported_into(blob, spec, data, masks)
}

/// Decodes a supported LERC blob into allocated `f64` values.
pub fn decode_to_f64(blob: &[u8], spec: DecodeIntoSpec) -> Result<DecodedBuffer<f64>> {
    let mut data = vec![0.0f64; spec.value_count()?];
    let mut masks = if spec.n_masks > 0 {
        Some(vec![0u8; spec.mask_byte_len()?])
    } else {
        None
    };
    let result = decode_lerc_supported_to_f64(blob, spec, &mut data, masks.as_deref_mut())?;
    data.truncate(result.values_written);
    if let Some(mask_bytes) = masks.as_mut() {
        mask_bytes.truncate(result.mask_bytes_written);
    }
    Ok(DecodedBuffer {
        data,
        masks,
        bytes_consumed: result.bytes_consumed,
        mask_bytes_written: result.mask_bytes_written,
    })
}

/// Decodes a supported LERC blob into caller-provided `f64` values.
///
/// `data` must have room for `spec.value_count()` values. When `spec.n_masks`
/// is nonzero, `masks` must have room for `spec.mask_byte_len()` bytes.
pub fn decode_to_f64_into(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data: &mut [f64],
    masks: Option<&mut [u8]>,
) -> Result<DecodeToF64Result> {
    decode_lerc_supported_to_f64(blob, spec, data, masks)
}

/// Decodes a supported LERC blob into allocated little-endian bytes and no-data metadata.
pub fn decode_4d_into(blob: &[u8], spec: DecodeIntoSpec) -> Result<Decoded4DBuffer<u8>> {
    let decoded = decode_into(blob, spec)?;
    let no_data = no_data_info(blob, spec.n_bands)?;
    Ok(Decoded4DBuffer {
        data: decoded.data,
        masks: decoded.masks,
        no_data,
        bytes_consumed: decoded.bytes_consumed,
        mask_bytes_written: decoded.mask_bytes_written,
    })
}

/// Decodes a supported LERC blob into caller-provided bytes and no-data arrays.
///
/// The `uses_no_data` and `no_data_values` buffers must each have room for
/// `spec.n_bands` entries. They are filled with per-band 4D no-data metadata
/// using the same convention as the C++ public decode API.
pub fn decode_4d_into_buffers(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data: &mut [u8],
    masks: Option<&mut [u8]>,
    uses_no_data: &mut [u8],
    no_data_values: &mut [f64],
) -> Result<DecodeIntoResult> {
    let no_data = no_data_info(blob, spec.n_bands)?;
    if uses_no_data.len() < spec.n_bands || no_data_values.len() < spec.n_bands {
        return Err(LercError::BufferTooSmall);
    }

    let result = decode_lerc_supported_into(blob, spec, data, masks)?;
    uses_no_data[..spec.n_bands].copy_from_slice(&no_data.uses_no_data[..spec.n_bands]);
    no_data_values[..spec.n_bands].copy_from_slice(&no_data.no_data_values[..spec.n_bands]);
    Ok(result)
}

/// Decodes a supported LERC blob into allocated `f64` values and no-data metadata.
pub fn decode_4d_to_f64(blob: &[u8], spec: DecodeIntoSpec) -> Result<Decoded4DBuffer<f64>> {
    let decoded = decode_to_f64(blob, spec)?;
    let no_data = no_data_info(blob, spec.n_bands)?;
    Ok(Decoded4DBuffer {
        data: decoded.data,
        masks: decoded.masks,
        no_data,
        bytes_consumed: decoded.bytes_consumed,
        mask_bytes_written: decoded.mask_bytes_written,
    })
}

/// Decodes a supported LERC blob into caller-provided `f64` values and no-data arrays.
///
/// The no-data buffers must each have at least `spec.n_bands` entries.
pub fn decode_4d_to_f64_into(
    blob: &[u8],
    spec: DecodeIntoSpec,
    data: &mut [f64],
    masks: Option<&mut [u8]>,
    uses_no_data: &mut [u8],
    no_data_values: &mut [f64],
) -> Result<DecodeToF64Result> {
    let no_data = no_data_info(blob, spec.n_bands)?;
    if uses_no_data.len() < spec.n_bands || no_data_values.len() < spec.n_bands {
        return Err(LercError::BufferTooSmall);
    }

    let result = decode_lerc_supported_to_f64(blob, spec, data, masks)?;
    uses_no_data[..spec.n_bands].copy_from_slice(&no_data.uses_no_data[..spec.n_bands]);
    no_data_values[..spec.n_bands].copy_from_slice(&no_data.no_data_values[..spec.n_bands]);
    Ok(result)
}

/// Reads public blob metadata for supported LERC blobs.
pub fn blob_info(blob: &[u8]) -> Result<LercInfo> {
    get_lerc_info(blob)
}

/// Reads public blob metadata into C API-style summary arrays.
///
/// This is the buffer-oriented equivalent of [`blob_info`]. When provided,
/// `info_array` is zeroed and filled in [`crate::InfoArrOrder`] order, and
/// `data_range_array` is zeroed and filled in [`crate::DataRangeArrOrder`]
/// order. Short arrays are accepted and filled up to their length, matching the
/// public C API truncation behavior.
pub fn blob_info_arrays_into(
    blob: &[u8],
    info_array: Option<&mut [u32]>,
    data_range_array: Option<&mut [f64]>,
) -> Result<LercInfo> {
    get_lerc2_blob_info_arrays(blob, info_array, data_range_array)
}

/// Reads public blob metadata and per-band/per-depth ranges in one call.
///
/// This mirrors the C++ `Lerc::GetLercInfo` overload that accepts optional
/// `pMins` and `pMaxs` buffers. The requested `n_depth * n_bands` capacity may
/// be larger than the blob requires. Capacity is checked before range decoding,
/// matching the public C++/C API status ordering for no-data blobs.
pub fn blob_info_with_ranges_into(
    blob: &[u8],
    n_depth: usize,
    n_bands: usize,
    mins: &mut [f64],
    maxs: &mut [f64],
) -> Result<LercInfo> {
    if n_depth == 0 || n_bands == 0 {
        return Err(LercError::WrongParam(
            "data range dimensions must be positive",
        ));
    }
    let capacity = n_depth
        .checked_mul(n_bands)
        .ok_or(LercError::BufferTooSmall)?;
    if mins.len() < capacity || maxs.len() < capacity {
        return Err(LercError::BufferTooSmall);
    }

    let info = blob_info(blob)?;
    let required_capacity = (info.n_depth as usize)
        .checked_mul(info.n_bands as usize)
        .ok_or(LercError::BufferTooSmall)?;
    if capacity < required_capacity {
        return Err(LercError::BufferTooSmall);
    }

    let ranges = data_ranges(blob)?;
    if ranges.mins.len() > capacity || ranges.maxs.len() > capacity {
        return Err(LercError::BufferTooSmall);
    }
    mins[..ranges.mins.len()].copy_from_slice(&ranges.mins);
    maxs[..ranges.maxs.len()].copy_from_slice(&ranges.maxs);
    Ok(info)
}

/// Reads per-band 4D no-data metadata for supported LERC blobs.
pub fn no_data_info(blob: &[u8], n_bands: usize) -> Result<NoDataInfo> {
    if n_bands == 0 {
        return Err(LercError::WrongParam("band count must be positive"));
    }
    let info = blob_info(blob)?;
    if n_bands > info.n_bands as usize {
        return Err(LercError::BufferTooSmall);
    }
    if info.n_uses_no_data_value > 0 && info.version != 0 {
        get_lerc2_no_data_info(blob, n_bands)
    } else {
        Ok(NoDataInfo {
            uses_no_data: vec![0; n_bands],
            no_data_values: vec![0.0; n_bands],
            bytes_consumed: info.blob_size as usize,
        })
    }
}

/// Reads per-band/per-depth data ranges for supported LERC blobs.
pub fn data_ranges(blob: &[u8]) -> Result<DataRanges> {
    get_lerc2_data_ranges(blob)
}

/// Reads per-band/per-depth data ranges into caller-provided buffers.
///
/// This is the buffer-oriented equivalent of [`data_ranges`], matching the
/// public C API layout: `n_depth` values per band, with bands laid out
/// consecutively. The requested `n_depth * n_bands` capacity may be larger
/// than the blob requires.
pub fn data_ranges_into(
    blob: &[u8],
    n_depth: usize,
    n_bands: usize,
    mins: &mut [f64],
    maxs: &mut [f64],
) -> Result<usize> {
    if n_depth == 0 || n_bands == 0 {
        return Err(LercError::WrongParam(
            "data range dimensions must be positive",
        ));
    }
    let capacity = n_depth
        .checked_mul(n_bands)
        .ok_or(LercError::BufferTooSmall)?;
    if mins.len() < capacity || maxs.len() < capacity {
        return Err(LercError::BufferTooSmall);
    }

    let info = blob_info(blob)?;
    let required_capacity = (info.n_depth as usize)
        .checked_mul(info.n_bands as usize)
        .ok_or(LercError::BufferTooSmall)?;
    if capacity < required_capacity {
        return Err(LercError::BufferTooSmall);
    }

    let ranges = data_ranges(blob)?;
    if ranges.mins.len() > capacity || ranges.maxs.len() > capacity {
        return Err(LercError::BufferTooSmall);
    }
    mins[..ranges.mins.len()].copy_from_slice(&ranges.mins);
    maxs[..ranges.maxs.len()].copy_from_slice(&ranges.maxs);
    Ok(ranges.mins.len())
}

fn validate_encode_version(version: i32) -> Result<()> {
    if (2..=DEFAULT_CODEC_VERSION).contains(&version) {
        Ok(())
    } else {
        Err(LercError::WrongParam(
            "codec version must be between 2 and 6",
        ))
    }
}

fn validate_active_no_data_inputs(
    spec: EncodeSpec,
    uses_no_data: Option<&[u8]>,
    no_data_values: Option<&[f64]>,
    version: i32,
) -> Result<()> {
    let uses_no_data = uses_no_data.ok_or(LercError::WrongParam(
        "Lerc2 encode no-data use count does not match band count",
    ))?;
    if uses_no_data.len() != spec.n_bands {
        return Err(LercError::WrongParam(
            "Lerc2 encode no-data use count does not match band count",
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DataType, DecodeIntoSpec, DecodedData, EncodeSpec};
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
    fn api_encode_decode_and_info_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 4,
            n_rows: 2,
            n_bands: 1,
            n_masks: 1,
        };
        let data = [1u8, 2, 99, 4, 5, 99, 7, 8];
        let mask = [1u8, 1, 0, 1, 1, 0, 1, 1];

        let size = compute_compressed_size(spec, &data, 0.5, Some(&mask)).unwrap();
        let blob = encode(spec, &data, 0.5, Some(&mask)).unwrap();
        assert_eq!(size, blob.len());
        let mut output = vec![0u8; size];
        let written = encode_into(spec, &data, 0.5, Some(&mask), &mut output).unwrap();
        assert_eq!(written, blob.len());
        assert_eq!(output, blob);
        let mut too_small = vec![0u8; size.saturating_sub(1)];
        assert_eq!(
            encode_into(spec, &data, 0.5, Some(&mask), &mut too_small).unwrap_err(),
            LercError::BufferTooSmall
        );

        let info = blob_info(&blob).unwrap();
        assert_eq!(info.n_cols, 4);
        assert_eq!(info.num_valid_pixel, 6);

        let decoded = decode(&blob).unwrap();
        assert_eq!(decoded.n_bands(), 1);
        assert_eq!(decoded.bytes_consumed(), blob.len());
        match decoded {
            DecodedLerc::Lerc2(decoded) => {
                assert_eq!(
                    decoded.bands[0].data,
                    DecodedData::UChar(vec![1, 2, 0, 4, 5, 0, 7, 8])
                );
            }
            DecodedLerc::Lerc1(_) => panic!("expected Lerc2 decode"),
        }

        let output = decode_into(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UChar,
                n_depth: 1,
                n_cols: 4,
                n_rows: 2,
                n_bands: 1,
                n_masks: 1,
            },
        )
        .unwrap();
        assert_eq!(output.data, vec![1, 2, 0, 4, 5, 0, 7, 8]);
        assert_eq!(output.masks, Some(mask.to_vec()));

        let mut direct_data = [0u8; 8];
        let mut direct_mask = [0u8; 8];
        let direct = decode_into_buffers(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UChar,
                n_depth: 1,
                n_cols: 4,
                n_rows: 2,
                n_bands: 1,
                n_masks: 1,
            },
            &mut direct_data,
            Some(&mut direct_mask),
        )
        .unwrap();
        assert_eq!(direct.data_bytes_written, 8);
        assert_eq!(direct.mask_bytes_written, 8);
        assert_eq!(direct_data, [1, 2, 0, 4, 5, 0, 7, 8]);
        assert_eq!(direct_mask, mask);

        let doubles = decode_to_f64(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UChar,
                n_depth: 1,
                n_cols: 4,
                n_rows: 2,
                n_bands: 1,
                n_masks: 1,
            },
        )
        .unwrap();
        assert_eq!(doubles.data, vec![1.0, 2.0, 0.0, 4.0, 5.0, 0.0, 7.0, 8.0]);

        let mut direct_doubles = [0.0f64; 8];
        let mut direct_double_mask = [0u8; 8];
        let direct = decode_to_f64_into(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UChar,
                n_depth: 1,
                n_cols: 4,
                n_rows: 2,
                n_bands: 1,
                n_masks: 1,
            },
            &mut direct_doubles,
            Some(&mut direct_double_mask),
        )
        .unwrap();
        assert_eq!(direct.values_written, 8);
        assert_eq!(direct.mask_bytes_written, 8);
        assert_eq!(direct_doubles, [1.0, 2.0, 0.0, 4.0, 5.0, 0.0, 7.0, 8.0]);
        assert_eq!(direct_double_mask, mask);
    }

    #[test]
    fn api_encode_4d_with_no_data_round_trip() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 3,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1u16, 2, u16::MAX, u16::MAX, u16::MAX, 3];
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        let blob = encode_4d(spec, &data, 0.5, None, Some(&[1]), Some(&[u16::MAX as f64])).unwrap();
        assert_eq!(
            compute_compressed_size_4d(
                spec,
                &data,
                0.5,
                None,
                Some(&[1]),
                Some(&[u16::MAX as f64])
            )
            .unwrap(),
            blob.len()
        );
        let mut output = vec![0u8; blob.len()];
        let written = encode_4d_into(
            spec,
            &data,
            0.5,
            None,
            Some(&[1]),
            Some(&[u16::MAX as f64]),
            &mut output,
        )
        .unwrap();
        assert_eq!(written, blob.len());
        assert_eq!(output, blob);
        let mut too_small = vec![0u8; blob.len() - 1];
        assert_eq!(
            encode_4d_into(
                spec,
                &data,
                0.5,
                None,
                Some(&[1]),
                Some(&[u16::MAX as f64]),
                &mut too_small,
            )
            .unwrap_err(),
            LercError::BufferTooSmall
        );
        let no_data = no_data_info(&blob, 1).unwrap();
        assert_eq!(no_data.uses_no_data, [1]);
        assert_eq!(no_data.no_data_values, [u16::MAX as f64]);
        assert_eq!(no_data.bytes_consumed, blob.len());

        let decoded_bytes = decode_4d_into(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UShort,
                n_depth: 2,
                n_cols: 3,
                n_rows: 1,
                n_bands: 1,
                n_masks: 1,
            },
        )
        .unwrap();
        assert_eq!(decoded_bytes.no_data, no_data);
        assert_eq!(decoded_bytes.masks, Some(vec![1, 0, 1]));

        let mut direct_data = [0u8; 12];
        let mut direct_mask = [0u8; 3];
        let mut direct_uses_no_data = [0u8; 1];
        let mut direct_no_data_values = [0.0f64; 1];
        let direct = decode_4d_into_buffers(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UShort,
                n_depth: 2,
                n_cols: 3,
                n_rows: 1,
                n_bands: 1,
                n_masks: 1,
            },
            &mut direct_data,
            Some(&mut direct_mask),
            &mut direct_uses_no_data,
            &mut direct_no_data_values,
        )
        .unwrap();
        assert_eq!(direct.data_bytes_written, 12);
        assert_eq!(direct.mask_bytes_written, 3);
        assert_eq!(direct_mask, [1, 0, 1]);
        assert_eq!(direct_uses_no_data, [1]);
        assert_eq!(direct_no_data_values, [u16::MAX as f64]);
        assert_eq!(direct_data, [1, 0, 2, 0, 0, 0, 0, 0, 255, 255, 3, 0]);

        let decoded_doubles = decode_4d_to_f64(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UShort,
                n_depth: 2,
                n_cols: 3,
                n_rows: 1,
                n_bands: 1,
                n_masks: 1,
            },
        )
        .unwrap();
        assert_eq!(
            decoded_doubles.data,
            vec![1.0, 2.0, 0.0, 0.0, u16::MAX as f64, 3.0]
        );
        assert_eq!(decoded_doubles.no_data, no_data);
        assert_eq!(decoded_doubles.masks, Some(vec![1, 0, 1]));

        let mut direct_doubles = [0.0f64; 6];
        let mut direct_double_mask = [0u8; 3];
        let mut direct_double_uses_no_data = [0u8; 1];
        let mut direct_double_no_data_values = [0.0f64; 1];
        let direct = decode_4d_to_f64_into(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::UShort,
                n_depth: 2,
                n_cols: 3,
                n_rows: 1,
                n_bands: 1,
                n_masks: 1,
            },
            &mut direct_doubles,
            Some(&mut direct_double_mask),
            &mut direct_double_uses_no_data,
            &mut direct_double_no_data_values,
        )
        .unwrap();
        assert_eq!(direct.values_written, 6);
        assert_eq!(direct.mask_bytes_written, 3);
        assert_eq!(direct_doubles, [1.0, 2.0, 0.0, 0.0, u16::MAX as f64, 3.0]);
        assert_eq!(direct_double_mask, [1, 0, 1]);
        assert_eq!(direct_double_uses_no_data, [1]);
        assert_eq!(direct_double_no_data_values, [u16::MAX as f64]);

        let decoded = decode(&blob).unwrap();
        match decoded {
            DecodedLerc::Lerc2(decoded) => {
                assert_eq!(decoded.bands[0].mask.count_valid_bits(), 2);
                assert_eq!(
                    decoded.bands[0].data,
                    DecodedData::UShort(vec![1, 2, 0, 0, u16::MAX, 3])
                );
            }
            DecodedLerc::Lerc1(_) => panic!("expected Lerc2 decode"),
        }
    }

    #[test]
    fn api_buffer_decode_validates_output_capacity_before_writing_no_data() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1u16, 2, u16::MAX, 4];
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_4d(spec, &data, 0.5, None, Some(&[1]), Some(&[u16::MAX as f64])).unwrap();
        let decode_spec = DecodeIntoSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 1,
        };

        let mut output = [99u8; 8];
        let mut mask = [99u8; 2];
        let mut no_data_values = [99.0f64; 1];
        assert_eq!(
            decode_4d_into_buffers(
                &blob,
                decode_spec,
                &mut output,
                Some(&mut mask),
                &mut [],
                &mut no_data_values,
            )
            .unwrap_err(),
            LercError::BufferTooSmall
        );
        assert_eq!(output, [99; 8]);
        assert_eq!(mask, [99; 2]);
        assert_eq!(no_data_values, [99.0]);
    }

    #[test]
    fn api_encode_filters_nan_pixels_like_cpp_public_api() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 3,
            n_rows: 1,
            n_bands: 2,
            n_masks: 0,
        };
        let values = [1.0f32, f32::NAN, 3.0, f32::NAN, 5.0, f32::NAN];
        let data = values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();

        let blob = encode(spec, &data, 0.0, None).unwrap();
        let info = blob_info(&blob).unwrap();
        assert_eq!(info.n_masks, 2);
        assert_eq!(
            compute_compressed_size(spec, &data, 0.0, None).unwrap(),
            blob.len()
        );

        let decoded = decode_to_f64(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::Float,
                n_depth: 1,
                n_cols: 3,
                n_rows: 1,
                n_bands: 2,
                n_masks: 2,
            },
        )
        .unwrap();
        assert_eq!(decoded.masks, Some(vec![1, 0, 1, 0, 1, 0]));
        assert_eq!(decoded.data, vec![1.0, 0.0, 3.0, 0.0, 5.0, 0.0]);
    }

    #[test]
    fn api_encode_rejects_mixed_depth_nan_without_no_data() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 2,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1.0f32, f32::NAN, 2.0, 2.0];
        let data = values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();

        assert_eq!(encode(spec, &data, 0.0, None).unwrap_err(), LercError::NaN);
        assert_eq!(
            compute_compressed_size(spec, &data, 0.0, None).unwrap_err(),
            LercError::NaN
        );
    }

    #[test]
    fn api_encode_4d_replaces_active_no_data_mixed_depth_nan_with_sentinel() {
        let spec = EncodeSpec {
            data_type: DataType::Float,
            n_depth: 2,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1.0f32, f32::NAN, 2.0, 2.0];
        let data = values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();

        let blob = encode_4d(spec, &data, 0.0, None, Some(&[1]), Some(&[-9999.0])).unwrap();
        assert_eq!(
            compute_compressed_size_4d(spec, &data, 0.0, None, Some(&[1]), Some(&[-9999.0]))
                .unwrap(),
            blob.len()
        );

        let decoded = decode_4d_to_f64(
            &blob,
            DecodeIntoSpec {
                data_type: DataType::Float,
                n_depth: 2,
                n_cols: 2,
                n_rows: 1,
                n_bands: 1,
                n_masks: 1,
            },
        )
        .unwrap();
        assert_eq!(decoded.masks, Some(vec![1, 1]));
        assert_eq!(decoded.no_data.uses_no_data, [1]);
        assert_eq!(decoded.no_data.no_data_values, [-9999.0]);
        assert_eq!(decoded.data, vec![1.0, -9999.0, 2.0, 2.0]);
    }

    #[test]
    fn api_blob_info_with_ranges_into_fills_metadata_and_ranges() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut mins = [99.0f64; 4];
        let mut maxs = [99.0f64; 4];

        let info = blob_info_with_ranges_into(&blob, 1, 4, &mut mins, &mut maxs).unwrap();

        assert_eq!(info.n_bands, 3);
        assert_eq!(info.n_depth, 1);
        assert_eq!(mins, [0.0, 0.0, 0.0, 99.0]);
        assert_eq!(maxs, [255.0, 255.0, 255.0, 99.0]);
    }

    #[test]
    fn api_blob_info_with_ranges_into_validates_capacity_before_no_data_status() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1u16, u16::MAX, 2, 3];
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_4d(spec, &data, 0.5, None, Some(&[1]), Some(&[u16::MAX as f64])).unwrap();
        let mut short_mins = [0.0f64; 1];
        let mut short_maxs = [0.0f64; 1];
        let mut mins = [0.0f64; 2];
        let mut maxs = [0.0f64; 2];

        assert_eq!(
            blob_info_with_ranges_into(&blob, 1, 1, &mut short_mins, &mut short_maxs).unwrap_err(),
            LercError::BufferTooSmall
        );
        assert_eq!(
            blob_info_with_ranges_into(&blob, 2, 1, &mut mins, &mut maxs).unwrap_err(),
            LercError::HasNoData
        );
    }

    #[test]
    fn api_blob_info_with_ranges_into_supports_legacy_lerc1() {
        let blob = fixture("world.lerc1");
        let mut mins = [99.0f64; 2];
        let mut maxs = [99.0f64; 2];

        let info = blob_info_with_ranges_into(&blob, 2, 1, &mut mins, &mut maxs).unwrap();

        assert_eq!(info.version, 0);
        assert_eq!(info.n_bands, 1);
        assert_eq!(mins, [-27.458_635_330_200_195, 99.0]);
        assert_eq!(maxs, [5474.172_851_562_5, 99.0]);
    }

    #[test]
    fn api_blob_info_arrays_into_fills_c_api_style_buffers() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut info_array = [123u32; 13];
        let mut range_array = [123.0f64; 5];

        let info =
            blob_info_arrays_into(&blob, Some(&mut info_array), Some(&mut range_array)).unwrap();

        assert_eq!(info.n_bands, 3);
        assert_eq!(
            &info_array[..11],
            &[3, 1, 1, 256, 256, 3, 43_008, blob.len() as u32, 1, 1, 0]
        );
        assert_eq!(&info_array[11..], &[0, 0]);
        assert_eq!(&range_array[..3], &[0.0, 255.0, 0.5]);
        assert_eq!(&range_array[3..], &[0.0, 0.0]);
    }

    #[test]
    fn api_blob_info_arrays_into_accepts_short_and_absent_buffers() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let mut info_array = [123u32; 4];

        let info = blob_info_arrays_into(&blob, Some(&mut info_array), None).unwrap();

        assert_eq!(info.n_bands, 1);
        assert_eq!(info_array, [3, DataType::Float as u32, 1, 400]);

        let info = blob_info_arrays_into(&blob, None, None).unwrap();
        assert_eq!(info.n_cols, 400);
        assert_eq!(info.n_rows, 400);
    }

    #[test]
    fn api_blob_info_arrays_into_supports_legacy_lerc1() {
        let blob = fixture("world.lerc1");
        let mut info_array = [123u32; 11];
        let mut range_array = [123.0f64; 3];

        let info =
            blob_info_arrays_into(&blob, Some(&mut info_array), Some(&mut range_array)).unwrap();

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
    fn api_blob_info_arrays_into_reports_no_data_quick_range_sentinels() {
        let spec = EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let values = [1u16, u16::MAX, 2, 3];
        let data = values
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let blob = encode_4d(spec, &data, 0.5, None, Some(&[1]), Some(&[u16::MAX as f64])).unwrap();
        let mut range_array = [123.0f64; 3];

        let info = blob_info_arrays_into(&blob, None, Some(&mut range_array)).unwrap();

        assert_eq!(info.n_uses_no_data_value, 1);
        assert_eq!(range_array, [-1.0, -1.0, 0.5]);
    }

    #[test]
    fn api_no_data_info_validates_requested_band_count() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let blob = encode(spec, &[3, 4], 0.0, None).unwrap();

        assert_eq!(
            no_data_info(&blob, 0).unwrap_err(),
            LercError::WrongParam("band count must be positive")
        );
        assert_eq!(
            no_data_info(&blob, 2).unwrap_err(),
            LercError::BufferTooSmall
        );
    }

    #[test]
    fn api_data_ranges_into_fills_c_api_style_buffers() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 3,
            n_cols: 2,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        let data = [1u8, 2, 3, 4, 5, 6];
        let blob = encode(spec, &data, 0.0, None).unwrap();
        let mut mins = [99.0f64; 4];
        let mut maxs = [99.0f64; 4];

        let written = data_ranges_into(&blob, 4, 1, &mut mins, &mut maxs).unwrap();

        assert_eq!(written, 3);
        assert_eq!(mins, [1.0, 2.0, 3.0, 99.0]);
        assert_eq!(maxs, [4.0, 5.0, 6.0, 99.0]);

        let mut short_mins = [0.0f64; 2];
        let mut short_maxs = [0.0f64; 2];
        assert_eq!(
            data_ranges_into(&blob, 2, 1, &mut short_mins, &mut short_maxs).unwrap_err(),
            LercError::BufferTooSmall
        );
        assert_eq!(
            data_ranges_into(&blob, 0, 1, &mut mins, &mut maxs).unwrap_err(),
            LercError::WrongParam("data range dimensions must be positive")
        );
    }

    #[test]
    fn api_data_ranges_into_supports_legacy_lerc1() {
        let blob = fixture("world.lerc1");
        let mut mins = [99.0f64; 2];
        let mut maxs = [99.0f64; 2];

        let written = data_ranges_into(&blob, 2, 1, &mut mins, &mut maxs).unwrap();

        assert_eq!(written, 1);
        assert_eq!(mins, [-27.458_635_330_200_195, 99.0]);
        assert_eq!(maxs, [5474.172_851_562_5, 99.0]);
    }

    #[test]
    fn api_convert_to_double_matches_cpp_helper_contract() {
        let short_bytes = [-2i16, 0, 17]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            convert_to_double(DataType::Short, &short_bytes).unwrap(),
            vec![-2.0, 0.0, 17.0]
        );
        let mut direct = [0.0f64; 3];
        assert_eq!(
            convert_to_double_into(DataType::Short, &short_bytes, &mut direct).unwrap(),
            3
        );
        assert_eq!(direct, [-2.0, 0.0, 17.0]);

        let float_bytes = [1.25f32, -3.5]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            convert_to_double(DataType::Float, &float_bytes).unwrap(),
            vec![1.25, -3.5]
        );
        let mut short_output = [0.0f64; 1];
        assert_eq!(
            convert_to_double_into(DataType::Float, &float_bytes, &mut short_output).unwrap_err(),
            LercError::BufferTooSmall
        );

        assert_eq!(
            convert_to_double(DataType::Double, &1.0f64.to_le_bytes()).unwrap_err(),
            LercError::WrongParam("ConvertToDouble requires non-empty non-double input")
        );
        assert_eq!(
            convert_to_double(DataType::UShort, &[1]).unwrap_err(),
            LercError::WrongParam("decoded byte length is not a multiple of the data type size")
        );
        assert_eq!(
            convert_to_double(DataType::UChar, &[]).unwrap_err(),
            LercError::WrongParam("ConvertToDouble requires non-empty non-double input")
        );
    }

    #[test]
    fn api_decodes_lerc1_fixture() {
        let blob = fixture("world.lerc1");
        let info = blob_info(&blob).unwrap();
        assert_eq!(info.version, 0);
        assert_eq!(info.n_cols, 257);
        assert_eq!(info.n_rows, 257);
        assert_eq!(info.n_bands, 1);

        let ranges = data_ranges(&blob).unwrap();
        assert_eq!(ranges.n_bands, 1);
        assert_eq!(ranges.n_depth, 1);
        assert_eq!(ranges.mins, [-27.458_635_330_200_195]);
        assert_eq!(ranges.maxs, [5474.172_851_562_5]);

        let decoded = decode(&blob).unwrap();
        match decoded {
            DecodedLerc::Lerc1(decoded) => {
                assert_eq!(decoded.n_bands, 1);
                assert_eq!(decoded.header.n_cols, 257);
                assert_eq!(decoded.header.n_rows, 257);
            }
            DecodedLerc::Lerc2(_) => panic!("expected Lerc1 decode"),
        }

        let no_data = no_data_info(&blob, 1).unwrap();
        assert_eq!(no_data.uses_no_data, [0]);
        assert_eq!(no_data.no_data_values, [0.0]);
        assert_eq!(no_data.bytes_consumed, blob.len());
    }

    #[test]
    fn api_rejects_unsupported_encode_versions() {
        let spec = EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 1,
            n_rows: 1,
            n_bands: 1,
            n_masks: 0,
        };
        assert_eq!(
            encode_for_version(spec, &[1], 0.5, None, 7).unwrap_err(),
            LercError::WrongParam("codec version must be between 2 and 6")
        );
    }
}
