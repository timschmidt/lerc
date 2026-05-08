//! Workflow-oriented safe Rust API facade.
//!
//! This module layers allocation-friendly encode, decode, metadata, and range
//! helpers over the lower-level format modules. The functions intentionally
//! expose the currently supported Rust codec subset while keeping the public C
//! ABI wrappers separate in [`crate::c_api`].

use crate::format::lerc1::{decode_lerc1_bands, DecodedLerc1Bands, CNT_Z_IMAGE_KEY};
use crate::format::lerc2::{
    decode_lerc2_bands_supported, decode_lerc_supported_into, decode_lerc_supported_to_f64,
    encode_lerc2_auto, encode_lerc2_auto_with_no_data, get_lerc2_data_ranges,
    get_lerc2_no_data_info, get_lerc_info, DataRanges, DecodeIntoSpec, LercInfo, NoDataInfo,
};
use crate::{EncodeSpec, LercError, Result};

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

/// Encodes a Lerc2 blob using a specific codec version.
pub fn encode_for_version(
    spec: EncodeSpec,
    data: &[u8],
    max_z_error: f64,
    masks: Option<&[u8]>,
    version: i32,
) -> Result<Vec<u8>> {
    validate_encode_version(version)?;
    encode_lerc2_auto(spec, data, max_z_error, masks, version)
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
        encode_lerc2_auto(spec, data, max_z_error, masks, version)
    }
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

/// Reads public blob metadata for supported LERC blobs.
pub fn blob_info(blob: &[u8]) -> Result<LercInfo> {
    get_lerc_info(blob)
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

fn validate_encode_version(version: i32) -> Result<()> {
    if (2..=DEFAULT_CODEC_VERSION).contains(&version) {
        Ok(())
    } else {
        Err(LercError::WrongParam(
            "codec version must be between 2 and 6",
        ))
    }
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
