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

//! Rust port of LERC.
//!
//! This crate currently contains safe Rust ports of low-level codec primitives.
//! The full LERC image encoder/decoder will be layered on these modules as the
//! port progresses.

pub mod bit_mask;
pub mod bit_stuffer;
pub mod decoded;
pub mod lerc2;
pub mod rle;
pub mod types;

pub use bit_mask::BitMask;
pub use bit_stuffer::BitStuffer2;
pub use decoded::{decode_typed_values, DecodedData};
pub use lerc2::{
    compute_checksum_fletcher32, decode_lerc2_bands_supported, decode_lerc2_supported,
    decode_lerc2_supported_with_previous, get_lerc2_header_info, get_lerc_info,
    read_lerc2_data_one_sweep, read_lerc2_data_one_sweep_with_previous, read_lerc2_mask,
    read_lerc2_mask_with_previous, read_lerc2_min_max_ranges,
    read_lerc2_min_max_ranges_with_previous, read_lerc2_tiled_payload,
    read_lerc2_tiled_payload_with_previous, read_lerc2_tiled_raw,
    read_lerc2_tiled_raw_with_previous, validate_lerc2_checksum, DataOneSweep, DecodedLerc2,
    DecodedLerc2Bands, HeaderInfo, LercInfo, MaskInfo, MinMaxRanges, TiledData,
};
pub use rle::Rle;
pub use types::{DataType, ErrCode, LercError, Result};
