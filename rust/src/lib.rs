/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

#![deny(missing_docs)]

//! Native Rust port of LERC codec components.
//!
//! This crate contains reusable codec primitives, supported Lerc1/Lerc2
//! decoders and encoders, allocation-friendly safe workflow helpers, and C ABI
//! entry points for the ported public surface.

/// Major LERC library version mirrored from `Lerc_c_api.h`.
pub const LERC_VERSION_MAJOR: u32 = 4;

/// Minor LERC library version mirrored from `Lerc_c_api.h`.
pub const LERC_VERSION_MINOR: u32 = 1;

/// Patch LERC library version mirrored from `Lerc_c_api.h`.
pub const LERC_VERSION_PATCH: u32 = 0;

/// Computes a packed LERC version number from major, minor, and patch parts.
///
/// This mirrors the `LERC_COMPUTE_VERSION(maj, min, patch)` macro from the C
/// API header.
pub const fn lerc_compute_version(major: u32, minor: u32, patch: u32) -> u32 {
    major * 10000 + minor * 100 + patch
}

/// Packed LERC library version mirrored from `Lerc_c_api.h`.
pub const LERC_VERSION_NUMBER: u32 =
    lerc_compute_version(LERC_VERSION_MAJOR, LERC_VERSION_MINOR, LERC_VERSION_PATCH);

/// Returns true when this crate mirrors at least the requested LERC version.
///
/// This mirrors the `LERC_AT_LEAST_VERSION(maj, min, patch)` macro from the C
/// API header.
pub const fn lerc_at_least_version(major: u32, minor: u32, patch: u32) -> bool {
    LERC_VERSION_NUMBER >= lerc_compute_version(major, minor, patch)
}

/// Safe Rust API facade modules.
pub mod api;
/// C ABI entry points.
pub mod c_api;
/// Shared data model, error, and decoded-value types.
pub mod data;
/// Format-specific LERC implementations.
pub mod format;
/// Reusable codec primitives.
pub mod primitives;
mod support;

pub use api::{
    blob_info, compute_compressed_size, compute_compressed_size_4d,
    compute_compressed_size_4d_for_version, compute_compressed_size_for_version, convert_to_double,
    convert_to_double_into, data_ranges, decode, decode_4d_into, decode_4d_into_buffers,
    decode_4d_to_f64, decode_4d_to_f64_into, decode_into, decode_into_buffers, decode_to_f64,
    decode_to_f64_into, encode, encode_4d, encode_4d_for_version, encode_4d_into,
    encode_4d_into_for_version, encode_for_version, encode_into, encode_into_for_version,
    no_data_info, Decoded4DBuffer, DecodedBuffer, DecodedLerc, DEFAULT_CODEC_VERSION,
};
pub use c_api as ffi;
pub use data::decoded;
pub use data::decoded::{convert_typed_bytes_to_f64, decode_typed_values, DecodedData};
pub use data::types;
pub use format::lerc1;
pub use format::lerc2;
pub use lerc1::{
    decode_lerc1, decode_lerc1_bands, get_lerc1_header_info, read_lerc1_count_mask,
    read_lerc1_z_stats, DecodedLerc1, DecodedLerc1Bands, Lerc1HeaderInfo, Lerc1MaskInfo,
    Lerc1PartInfo, Lerc1ZStats, CNT_Z_IMAGE_KEY,
};
pub use lerc2::{
    compute_checksum_fletcher32, compute_lerc2_data_ranges_for_encode,
    compute_lerc2_header_byte_len, compute_lerc2_mask_byte_len,
    compute_lerc2_min_max_ranges_byte_len, compute_lerc2_one_sweep_byte_len,
    compute_lerc2_tiled_raw_byte_len, decode_lerc2_bands_supported, decode_lerc2_supported,
    decode_lerc2_supported_into, decode_lerc2_supported_with_previous, decode_lerc_supported_into,
    decode_lerc_supported_to_f64, encode_lerc2_auto, encode_lerc2_auto_with_no_data,
    encode_lerc2_byte_huffman, encode_lerc2_byte_huffman_bands,
    encode_lerc2_byte_huffman_bands_with_no_data, encode_lerc2_byte_huffman_with_no_data,
    encode_lerc2_constant, encode_lerc2_float_huffman, encode_lerc2_float_huffman_bands,
    encode_lerc2_float_huffman_bands_with_no_data, encode_lerc2_float_huffman_with_no_data,
    encode_lerc2_one_sweep, encode_lerc2_one_sweep_bands,
    encode_lerc2_one_sweep_bands_with_no_data, encode_lerc2_one_sweep_with_no_data,
    encode_lerc2_tiled_lut, encode_lerc2_tiled_lut_bands,
    encode_lerc2_tiled_lut_bands_with_no_data, encode_lerc2_tiled_lut_with_no_data,
    encode_lerc2_tiled_raw, encode_lerc2_tiled_raw_bands,
    encode_lerc2_tiled_raw_bands_with_no_data, encode_lerc2_tiled_raw_with_no_data,
    encode_lerc2_tiled_simple, encode_lerc2_tiled_simple_bands,
    encode_lerc2_tiled_simple_bands_with_no_data, encode_lerc2_tiled_simple_with_no_data,
    encode_lerc2_uncompressed, encode_lerc2_uncompressed_with_no_data, finalize_lerc2_checksum,
    get_lerc2_blob_info_arrays, get_lerc2_data_ranges, get_lerc2_header_info,
    get_lerc2_no_data_info, get_lerc_info, read_lerc2_data_one_sweep,
    read_lerc2_data_one_sweep_with_previous, read_lerc2_mask, read_lerc2_mask_with_previous,
    read_lerc2_min_max_ranges, read_lerc2_min_max_ranges_with_previous, read_lerc2_tiled_payload,
    read_lerc2_tiled_payload_with_previous, read_lerc2_tiled_raw,
    read_lerc2_tiled_raw_with_previous, validate_lerc2_checksum, write_lerc2_header,
    write_lerc2_mask, write_lerc2_min_max_ranges, write_lerc2_one_sweep, write_lerc2_tiled_raw,
    DataOneSweep, DataRanges, DecodeIntoResult, DecodeIntoSpec, DecodeToF64Result, DecodedLerc2,
    DecodedLerc2Bands, HeaderInfo, LercInfo, MaskInfo, MinMaxRanges, NoDataInfo, TiledData,
    BLOB_DATA_RANGE_ARRAY_LEN, BLOB_INFO_ARRAY_LEN,
};
pub use primitives::bit_mask;
pub use primitives::bit_mask::{optional_masks_differ, BitMask};
pub use primitives::bit_stuffer;
pub use primitives::bit_stuffer::BitStuffer2;
pub use primitives::rle;
pub use primitives::rle::Rle;
pub use types::{
    DataRangeArrOrder, DataType, EncodeSpec, ErrCode, InfoArrOrder, LercError, LercStatus, Result,
};

#[cfg(test)]
mod tests {
    use super::{
        lerc_at_least_version, lerc_compute_version, LERC_VERSION_MAJOR, LERC_VERSION_MINOR,
        LERC_VERSION_NUMBER, LERC_VERSION_PATCH,
    };

    #[test]
    fn public_version_constants_match_c_api_header() {
        assert_eq!(LERC_VERSION_MAJOR, 4);
        assert_eq!(LERC_VERSION_MINOR, 1);
        assert_eq!(LERC_VERSION_PATCH, 0);
        assert_eq!(LERC_VERSION_NUMBER, 40100);
    }

    #[test]
    fn version_helpers_match_c_api_macros() {
        assert_eq!(lerc_compute_version(4, 1, 0), LERC_VERSION_NUMBER);
        assert_eq!(lerc_compute_version(3, 0, 12), 30012);
        assert!(lerc_at_least_version(4, 0, 0));
        assert!(lerc_at_least_version(4, 1, 0));
        assert!(!lerc_at_least_version(4, 1, 1));
        assert!(!lerc_at_least_version(5, 0, 0));
    }
}
