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

//! Native Rust port of selected LERC codec components.
//!
//! This crate currently contains safe Rust ports of low-level codec primitives.
//! The full LERC image encoder/decoder will be layered on these modules as the
//! port progresses.

/// Packed valid-pixel mask helpers.
pub mod bit_mask;
/// Lerc2 bit-stuffing helpers.
pub mod bit_stuffer;
/// Typed decoded data containers and conversion helpers.
pub mod decoded;
/// C ABI entry points.
pub mod ffi;
/// Legacy Lerc1 metadata helpers.
pub mod lerc1;
/// Lerc2 metadata and supported-subset decode helpers.
pub mod lerc2;
/// LERC run-length encoding helpers.
pub mod rle;
/// Shared data types and errors.
pub mod types;

pub use bit_mask::BitMask;
pub use bit_stuffer::BitStuffer2;
pub use decoded::{decode_typed_values, DecodedData};
pub use lerc1::{
    decode_lerc1, get_lerc1_header_info, read_lerc1_count_mask, read_lerc1_z_stats, DecodedLerc1,
    Lerc1HeaderInfo, Lerc1MaskInfo, Lerc1PartInfo, Lerc1ZStats, CNT_Z_IMAGE_KEY,
};
pub use lerc2::{
    compute_checksum_fletcher32, compute_lerc2_data_ranges_for_encode,
    compute_lerc2_header_byte_len, compute_lerc2_mask_byte_len,
    compute_lerc2_min_max_ranges_byte_len, compute_lerc2_one_sweep_byte_len,
    compute_lerc2_tiled_raw_byte_len, decode_lerc2_bands_supported, decode_lerc2_supported,
    decode_lerc2_supported_into, decode_lerc2_supported_with_previous, decode_lerc_supported_into,
    decode_lerc_supported_to_f64, encode_lerc2_constant, encode_lerc2_one_sweep,
    encode_lerc2_one_sweep_bands, encode_lerc2_one_sweep_bands_with_no_data,
    encode_lerc2_tiled_raw, encode_lerc2_tiled_raw_bands, finalize_lerc2_checksum,
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
pub use rle::Rle;
pub use types::{DataType, EncodeSpec, ErrCode, LercError, Result};
