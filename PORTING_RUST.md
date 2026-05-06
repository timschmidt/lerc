# Rust Porting Plan

This branch is the start of a native Rust port of the LERC C++ library. The goal is to keep the C++ implementation as the behavioral reference while moving codec pieces into safe, testable Rust modules.

## Current Scope

- Added a Cargo workspace at the repository root with the Rust crate in `rust/`.
- Ported low-level `RLE` compression/decompression into `rust/src/rle.rs`, including pinned literal and repeated-run byte-stream parity tests.
- Ported `BitStuffer2` simple and LUT modes into `rust/src/bit_stuffer.rs`, including both the Lerc2 v2.3+ little-endian packing and the pre-v2.3 legacy word layout.
- Ported `BitMask` into `rust/src/bit_mask.rs`, including byte-mask conversion helpers and pinned packed-byte parity tests equivalent to the private C++ `Lerc::Convert` methods.
- Added Lerc2 header parsing and multi-band metadata aggregation into `rust/src/lerc2.rs`.
- Added the first legacy Lerc1 reader in `rust/src/lerc1.rs`, covering `CntZImage` header and part-header parsing, non-tiled count/mask decoding, z-tile min/max statistics, row-major float payload decode, safe `get_lerc_info` metadata fallback, and safe/C ABI data-range reporting for the checked-in `world.lerc1` fixture.
- Added Lerc2 mask-section reading, including RLE mask decompression and previous-mask reuse for concatenated bands.
- Added Lerc2 Fletcher32 checksum calculation and validation for version 3+ blobs.
- Added Lerc2 v4+ min/max range-section parsing for all native Lerc data types.
- Added Lerc2 one-sweep raw payload decoding, matching `Lerc2::ReadDataOneSweep`.
- Added Lerc2 tiled raw-binary payload decoding for non-diff raw and constant-zero tile blocks.
- Added Lerc2 tiled simple bit-stuffed payload decoding and constant-zMin tile handling.
- Added Lerc2 tiled LUT payload decoding via the shared `BitStuffer2` stream decoder.
- Added Lerc2 v5 diff-encoded tiled payload decoding for integer and floating-point simple, LUT, and constant tile modes.
- Added typed decoded-data conversion helpers for all Lerc native data types, plus typed accessors for one-sweep and tiled decode outputs.
- Added a high-level supported-subset Lerc2 decode dispatcher that validates checksums, reads headers/masks/min-max ranges, dispatches const/one-sweep/tiled payloads, and returns typed decoded data.
- Added a high-level supported-subset multi-band Lerc2 decode dispatcher for concatenated blobs, including previous-mask reuse between bands.
- Added v6 no-data remapping in the supported decoder so temporary encoded no-data values are restored to the original no-data sentinel for valid pixels in multi-depth blobs.
- Added safe per-band Lerc2 no-data metadata reporting for 4D decode wrappers.
- Added data-range aggregation matching the C++ `lerc_getDataRanges` path for Lerc2 blobs and the checked-in Lerc1 fixture, including `HasNoData` reporting for multi-depth no-data ranges.
- Added checked output-copy helpers for decoded typed values, supported single-band decode results, and supported multi-band decode results. These write little-endian data bytes and byte masks into caller-provided buffers for later C ABI use.
- Added decoded-data conversion into caller-provided `f64` buffers for `lerc_decodeToDouble`-style APIs.
- Added a safe format-agnostic `decode_lerc_supported_to_f64` helper that mirrors `lerc_decodeToDouble` conversion behavior.
- Added safe `decode_lerc2_supported_into` and format-agnostic `decode_lerc_supported_into` layers that validate requested output type, dimensions, band count, and mask count before writing decoded data and byte masks into caller-provided buffers.
- Added documented `DecodeIntoSpec` buffer-sizing helpers for decoded data bytes, scalar counts, and mask bytes.
- Added a documented `EncodeSpec` shape helper for encode and compute-size validation while the encoder is still pending.
- Added safe Lerc2 header byte-size and serialization helpers for encode groundwork, with v3/v6 parser round-trip tests.
- Added safe Lerc2 mask byte-size and serialization helpers for encode groundwork, including partial-mask RLE writing, omitted previous-mask sections, and all-valid/all-invalid zero sections.
- Added safe Lerc2 checksum finalization for encode groundwork, matching the C++ post-write checksum fill step for version 3+ blobs.
- Added rustdoc comments across the public Rust API and enabled crate-level `#![deny(missing_docs)]` so documentation completeness is enforced at compile time.
- Added a safe `get_lerc2_blob_info_arrays` helper that fills C API-style blob info and quick data-range arrays from Lerc2 metadata, with fixture coverage for the byte and float Lerc2 sample blobs.
- Added the first Rust C ABI entry point, `lerc_getBlobInfo`, backed by the safe metadata array helper and guarded with a panic boundary.
- Added the Rust C ABI entry point `lerc_getDataRanges`, backed by the safe data-range aggregator, cross-checked against checked-in Lerc2 fixtures and `world.lerc1`, and guarded with a panic boundary.
- Added the Rust C ABI entry point `lerc_decode` for the supported Lerc2 decode subset and the checked-in Lerc1 fixture path, backed by safe decode helpers and guarded with a panic boundary.
- Added the Rust C ABI entry point `lerc_decodeToDouble` for the supported Lerc2 decode subset and the checked-in Lerc1 fixture path, backed by native decode plus typed conversion into caller-provided `double` output.
- Added the Rust C ABI entry points `lerc_decode_4D` and `lerc_decodeToDouble_4D` for the supported Lerc2 decode subset and the checked-in Lerc1 fixture path, including per-band no-data reporting arrays for v6 multi-depth Lerc2 blobs.
- Added encode-side C ABI placeholders for `lerc_computeCompressedSize`, `lerc_computeCompressedSizeForVersion`, `lerc_computeCompressedSize_4D`, `lerc_encode`, `lerc_encodeForVersion`, and `lerc_encode_4D`; these validate arguments, zero output counters, and return `Failed` until the Rust encoder is ported.
- Promoted the real `california_400_400_1_float.lerc2` fixture into supported-subset decode coverage, including safe Rust decode plus `lerc_decode` and `lerc_decodeToDouble` FFI output checks.
- Configured the Rust crate to build `rlib`, `cdylib`, and `staticlib` outputs.
- Added unit tests for RLE round trips, RLE boundary counters, pinned RLE literal/repeated byte streams, bit widths, LUT streams, bit-mask packing, pinned BitMask byte conversion, typed decoded values, checked decoded output copying, decoded `f64` conversion, C API-style decode-into validation, `DecodeIntoSpec` buffer sizing and overflow checks, `EncodeSpec` validation and overflow checks, format-agnostic Lerc1/Lerc2 decode-into dispatch, format-agnostic Lerc1/Lerc2 decode-to-`f64` dispatch, C API-style blob info arrays across Lerc2 fixtures and the Lerc1 fixture, encode-side C ABI placeholder validation, the `lerc_getBlobInfo`, `lerc_getDataRanges`, `lerc_decode`, `lerc_decodeToDouble`, `lerc_decode_4D`, and `lerc_decodeToDouble_4D` C ABI wrappers, direct legacy Lerc1 C ABI blob-info, data-range, native decode, double decode, and 4D decode array checks, Lerc1 fixture header parsing, Lerc1 count-mask decoding, Lerc1 z-stat decoding, Lerc1 float value decoding, Lerc1 data-range reporting, Lerc2 fixture metadata, Lerc2 header byte sizing and writing, Lerc2 mask byte sizing and writing, Lerc2 masks, checksum validation and finalization, checksum mismatch detection, synthetic v4 min/max ranges, fixture data ranges, one-sweep payload expansion, raw tiled payload expansion, simple bit-stuffed tiled expansion, LUT tiled expansion, integer and floating-point diff tiled simple/LUT/constant expansion, high-level supported decode dispatch, real fixture supported decode dispatch, multi-band supported decode dispatch with previous-mask reuse, v6 no-data remapping, v6 no-data metadata reporting, data-range aggregation, `HasNoData` range reporting, unsupported Huffman dispatch, and malformed input.
- Added a dependency-free benchmark harness at `rust/benches/rle_bit_stuffer.rs`.

## Compatibility Notes

- The Rust `Rle` byte stream layout is intended to match `src/LercLib/RLE.cpp`: little-endian signed counters, negative repeated runs, positive literal runs, and `i16::MIN` end-of-stream marker.
- The Rust `BitStuffer2` implementation supports Lerc2 version 3 and newer bit packing plus the older pre-v2.3 packing path in `BitStuff_Before_Lerc2v3` and `BitUnStuff_Before_Lerc2v3`.
- The Rust `BitMask` preserves the C++ MSB-first bit order from `BitMask::Bit`; padding bits may remain set after `set_all_valid`, but `count_valid_bits` excludes them.
- The Rust Lerc2 parser supports versions 0 through 6 and currently covers the fast header path used by `Lerc2::GetHeaderInfo`, checksum validation used by `Lerc2::Decode`, mask reading used by `Lerc2::ReadMask`, min/max range parsing used by `Lerc2::ReadMinMaxRanges`, one-sweep raw payload expansion used by `Lerc2::ReadDataOneSweep`, raw-binary, simple bit-stuffed, LUT, and integer diff-encoded tiled payload expansion for `Lerc2::ReadTile`, single-band and multi-band supported-subset decode orchestration, pre-v2.3 bit-stuffed tile streams, the Lerc2 branch of `Lerc::GetLercInfo`, and the legacy Lerc1 metadata fallback in the public metadata helpers.
- `compute_lerc2_header_byte_len` and `write_lerc2_header` mirror the C++ header layout, including v3+ checksum storage and v6 no-data metadata fields. Encoders can write a zero checksum placeholder and fill it after payload assembly.
- `compute_lerc2_mask_byte_len` and `write_lerc2_mask` mirror the C++ mask-section layout: a 4-byte encoded-mask length followed by RLE-compressed packed mask bytes only when a partial mask is explicitly encoded.
- `finalize_lerc2_checksum` mirrors the C++ `DoChecksOnEncode` checksum fill step by hashing bytes after the checksum field through the declared blob size and writing the result back into the header.
- The Rust Lerc1 reader currently validates the legacy `CntZImage` file key, version/type fields, dimensions, max-z-error, and count/z part headers. It also decodes non-tiled count parts stored as constant counts or RLE-compressed packed masks, computes z min/max statistics from tiled z payloads, decodes tiled z payloads into row-major `f32` values, and is integrated into the safe `get_lerc_info`, blob-info array, and data-range helper paths. `lerc_getBlobInfo` and `lerc_getDataRanges` inherit that fallback through the shared helpers.
- `decode_lerc2_supported` is intentionally decode-only and subset-scoped. It validates v3+ checksums and supports constant images, one-sweep raw payloads, and non-Huffman tiled payloads already covered by the lower-level readers. Huffman-backed image modes currently return `Unsupported`.
- `decode_lerc2_bands_supported` decodes concatenated Lerc2 blobs into separate band results and passes the previous band mask into later bands, matching the C++ previous-mask reuse mechanism for omitted partial masks.
- For v6 blobs with `bPassNoDataValues` and `nDepth > 1`, supported decode remaps decoded values equal to `noDataVal` back to `noDataValOrig` on valid pixels, matching the C++ `Lerc::RemapNoData` wrapper behavior.
- `get_lerc2_no_data_info` reports the per-band `bPassNoDataValues` flags and original no-data sentinels used by the 4D C API wrappers without decoding pixel values.
- `get_lerc2_data_ranges` reports header ranges for single-depth Lerc2 bands, v4+ min/max range sections for multi-depth Lerc2 bands, and decoded z stats for the checked-in Lerc1 fixture. Like the C++ public range API, it returns `HasNoData` instead of min/max values for multi-depth blobs that carry no-data sentinels.
- Decoded output-copy helpers use little-endian serialization explicitly and return `BufferTooSmall` before writing past caller-provided buffers. Multi-band output is band-major, matching the public C API data ordering. `DecodeIntoSpec` provides the same checked byte-count calculations used by FFI wrappers.
- `decode_lerc_supported_into` is the safe Rust equivalent of the output-buffer validation needed by `lerc_decode`: it dispatches Lerc1 and Lerc2 supported blobs, requires `n_masks` to be 0, 1, or `n_bands`, rejects insufficient mask requests for blobs with masks, and returns `WrongParam` for shape/type mismatches. The Lerc2-only branch remains available as `decode_lerc2_supported_into`.
- `decode_lerc_supported_to_f64` and `lerc_decodeToDouble` decode through the native typed path first, then convert the decoded scalar values to `f64`, matching the C++ API's output type while keeping the Rust decode core typed. Legacy Lerc1 blobs use the dedicated float decoder before conversion.
- `get_lerc2_blob_info_arrays` mirrors the truncation-tolerant output-array behavior of `lerc_getBlobInfo`, including zero-filling caller arrays and reporting `-1` quick ranges for multi-depth no-data blobs.
- The Rust `lerc_getBlobInfo` C ABI currently supports Lerc2 metadata and the checked-in Lerc1 fixture path. It validates null pointers and sizes like the C++ API, maps Rust errors to C status codes, and catches panics before returning `ErrCode::Failed`.
- The Rust `lerc_getDataRanges` C ABI currently supports Lerc2 metadata and the checked-in Lerc1 fixture path. It validates null pointers and caller capacity like the C++ API, maps Rust errors to C status codes, and catches panics before returning `ErrCode::Failed`.
- The Rust `lerc_decode` C ABI currently supports the same non-Huffman Lerc2 subset as `decode_lerc2_supported_into` plus the checked-in Lerc1 float fixture path. It validates shape, data type, mask count, and raw output pointers like the C++ API, and returns `HasNoData` for regular non-4D decode when multi-depth no-data metadata is present.
- The Rust `lerc_decodeToDouble` C ABI currently supports the same non-Huffman Lerc2 subset and checked-in Lerc1 fixture path as `lerc_decode`, including the same regular non-4D `HasNoData` behavior for Lerc2 no-data blobs.
- The Rust `lerc_decode_4D` and `lerc_decodeToDouble_4D` C ABI wrappers currently support the same non-Huffman Lerc2 subset and checked-in Lerc1 fixture path as the regular decode wrappers. They require no-data output arrays only when multi-depth no-data metadata is present, then report each decoded band's `bPassNoDataValues` flag and original no-data sentinel for Lerc2 blobs.
- Encode-side C ABI functions are exported but intentionally fail with `ErrCode::Failed` after C++-style argument validation and output-counter zeroing. This makes the missing encoder behavior explicit while preserving a stable link surface for FFI consumers.
- `EncodeSpec` centralizes the checked shape and byte-count validation currently shared by encode-side C ABI placeholders.
- Lerc2 header serialization is now isolated behind a safe helper so the encoder can reuse one layout implementation for compute-size, encode, and fixture construction.
- Lerc2 mask serialization is now isolated behind a safe helper so compute-size and encode paths can share validation for explicit masks and omitted previous-mask sections.
- Lerc2 checksum finalization is now isolated behind a safe helper so encoders can assemble blobs with a zero checksum placeholder and finalize them after payload writing.
- Lerc2 previous-mask reuse is explicit in Rust via `read_lerc2_mask_with_previous`; calling `read_lerc2_mask` on a blob that omits a partial mask returns an unsupported error.
- Public Rust APIs return `Result<T, LercError>` instead of bool/status pairs. C-compatible FFI wrappers should be added after the safe Rust codec surface is stable.
- Public Rust documentation is docs.rs-ready under `cargo doc --no-deps`; missing public documentation is a compile error.

## Next Porting Steps

1. Add C++ parity fixtures for `RLE` and `BitStuffer2`.
   - Deterministic RLE literal and repeated-run stream bytes are now pinned in Rust tests.
   - Generate known byte streams from the C++ implementation.
   - Assert Rust can decode C++ bytes and that Rust encoded bytes match for deterministic cases.
2. Add C++ parity fixtures for `BitMask`.
   - Deterministic byte-mask-to-packed and packed-to-byte-mask conversions are now pinned in Rust tests.
   - Assert Rust bit-packed bytes match C++ conversion from byte masks.
   - Assert Rust byte-mask output matches C++ `Lerc::Convert(const BitMask&, Byte*)`.
3. Add C++ parity tests for `lerc_getBlobInfo`.
   - Rust safe and C ABI blob-info array paths are now cross-checked across the checked-in Lerc2 fixtures and `world.lerc1`.
   - Compare Rust `get_lerc_info` with C API output for all `testData/*.lerc2` fixtures.
   - Compare Rust `get_lerc2_blob_info_arrays` with C API `lerc_getBlobInfo` output arrays.
   - Rust safe and C ABI data-range paths are now cross-checked across the checked-in no-data-free Lerc2 fixtures and `world.lerc1`.
   - Compare Rust `get_lerc2_data_ranges` and Rust `lerc_getDataRanges` with the C++ C API for no-data-free fixtures.
   - Broaden legacy coverage with generated C++ expectations for more Lerc1 tile and mask variants.
4. Port decode-only Lerc2 block paths.
   - Add C++ parity tests for `decode_lerc2_supported` on synthetic and fixture blobs that avoid Huffman.
   - Add C++ parity tests for `decode_lerc2_bands_supported` on concatenated non-Huffman fixtures.
   - Add C++ parity coverage for v6 no-data remapping across all supported native data types.
   - Add broader C++ parity fixtures for diff-encoded integer and floating-point tile modes.
   - Decode Huffman-backed blocks separately.
   - Keep each block codec independently fuzzable and benchmarkable.
5. Add encode paths after decode parity is established.
   - Header byte sizing and serialization are now available as safe Rust helpers.
   - Mask byte sizing and serialization are now available as safe Rust helpers.
   - Checksum finalization is now available as a safe Rust helper.
   - Start with lossless integer and byte tiles.
   - Add floating point quantization and max error checks.
6. Add FFI surface.
   - Continue mirroring `src/LercLib/include/Lerc_c_api.h` status codes and data type integers.
   - Replace encode-side placeholders with real compute-size and encode implementations after the Rust encoder is ported.
   - Keep panic boundaries explicit with C ABI tests.

## Verification Commands

```sh
cargo check
cargo test
cargo doc --no-deps
cargo bench --bench rle_bit_stuffer
```

Last run in this branch:

- `cargo check`: passed with `#![deny(missing_docs)]` enabled.
- `cargo test`: passed, 116 unit tests.
- `cargo doc --no-deps`: passed and generated crate documentation.
- `cargo bench --bench rle_bit_stuffer`: passed and printed timings for RLE compress/decompress, BitStuffer encode/decode, pre-v2.3 BitStuffer encode/decode, BitMask conversion/count operations, typed decoded value conversion, checked decoded output copying, decoded `f64` conversion through C ABI, DecodeIntoSpec byte-count helpers, EncodeSpec validation and byte-count helpers, Lerc1 header parsing, Lerc1 count-mask reading, Lerc1 z-stat reading, Lerc1 float value decoding, Lerc1 safe decode-into, Lerc1 safe decode-to-`f64`, Lerc1 C ABI decode, Lerc1 C ABI decode-to-double, Lerc1 info aggregation, Lerc1 data-range aggregation, Lerc2 metadata parsing, C API-style blob info array filling, Lerc2 data-range aggregation, C ABI data-range retrieval, Lerc2 mask reading, Lerc2 checksum validation and finalization, Lerc2 header writing, Lerc2 mask writing, Lerc2 min/max range parsing, Lerc2 no-data metadata reporting, Lerc2 one-sweep decode, Lerc2 raw tiled decode, Lerc2 simple bit-stuffed tiled decode, Lerc2 LUT tiled decode, Lerc2 integer and floating-point diff tiled simple/LUT decode, high-level supported-subset Lerc2 decode, high-level supported-subset multi-band Lerc2 decode, Lerc2 safe decode-to-`f64`, real float fixture decode, v6 supported-subset no-data decode, C API-style supported decode-into-buffer validation, C ABI decode, C ABI decode-to-double, C ABI real float fixture decode, and C ABI 4D no-data decode variants.

## Open Risks

- Some C++ code relies on native integer memory behavior but serializes little-endian streams. Rust code should continue using explicit little-endian reads/writes.
- Full codec parity will need fixture generation against the C++ branch so byte-for-byte expectations are not inferred only from the source.
- Lerc1 metadata parity is currently covered by the checked-in `world.lerc1` fixture; broader Lerc1 tile-mode coverage still needs generated fixtures.
- Huffman and predictor paths have a larger state surface and should be ported only after the non-Huffman decode dispatcher is locked down with cross-language fixtures.
