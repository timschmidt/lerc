# Rust Porting Plan

This branch is the start of a native Rust port of the LERC C++ library. The goal is to keep the C++ implementation as the behavioral reference while moving codec pieces into safe, testable Rust modules.

## Current Scope

- Added a Cargo workspace at the repository root with the Rust crate in `rust/`.
- Ported low-level `RLE` compression/decompression into `rust/src/rle.rs`.
- Ported the Lerc2 v2.3+ `BitStuffer2` simple and LUT modes into `rust/src/bit_stuffer.rs`.
- Ported `BitMask` into `rust/src/bit_mask.rs`, including byte-mask conversion helpers equivalent to the private C++ `Lerc::Convert` methods.
- Added Lerc2 header parsing and multi-band metadata aggregation into `rust/src/lerc2.rs`.
- Added Lerc2 mask-section reading, including RLE mask decompression and previous-mask reuse for concatenated bands.
- Added Lerc2 Fletcher32 checksum calculation and validation for version 3+ blobs.
- Added Lerc2 v4+ min/max range-section parsing for all native Lerc data types.
- Added Lerc2 one-sweep raw payload decoding, matching `Lerc2::ReadDataOneSweep`.
- Added Lerc2 tiled raw-binary payload decoding for non-diff raw and constant-zero tile blocks.
- Added Lerc2 tiled simple bit-stuffed payload decoding and constant-zMin tile handling.
- Added Lerc2 tiled LUT payload decoding via the shared `BitStuffer2` stream decoder.
- Added Lerc2 v5 diff-encoded tiled payload decoding for integer lossless simple/constant tile modes.
- Added typed decoded-data conversion helpers for all Lerc native data types, plus typed accessors for one-sweep and tiled decode outputs.
- Added a high-level supported-subset Lerc2 decode dispatcher that validates checksums, reads headers/masks/min-max ranges, dispatches const/one-sweep/tiled payloads, and returns typed decoded data.
- Added a high-level supported-subset multi-band Lerc2 decode dispatcher for concatenated blobs, including previous-mask reuse between bands.
- Added v6 no-data remapping in the supported decoder so temporary encoded no-data values are restored to the original no-data sentinel for valid pixels in multi-depth blobs.
- Added Lerc2 data-range aggregation matching the C++ `lerc_getDataRanges` path for Lerc2 blobs, including `HasNoData` reporting for multi-depth no-data ranges.
- Added checked output-copy helpers for decoded typed values, supported single-band decode results, and supported multi-band decode results. These write little-endian data bytes and byte masks into caller-provided buffers for later C ABI use.
- Added a safe `decode_lerc2_supported_into` layer that validates requested output type, dimensions, band count, and mask count before writing decoded data and byte masks into caller-provided buffers.
- Added unit tests for round trips, boundary counters, bit widths, LUT streams, bit-mask packing, byte-mask conversion, typed decoded values, checked decoded output copying, C API-style decode-into validation, Lerc2 fixture metadata, Lerc2 masks, checksum validation, checksum mismatch detection, synthetic v4 min/max ranges, one-sweep payload expansion, raw tiled payload expansion, simple bit-stuffed tiled expansion, LUT tiled expansion, diff tiled expansion, high-level supported decode dispatch, multi-band supported decode dispatch with previous-mask reuse, v6 no-data remapping, data-range aggregation, `HasNoData` range reporting, unsupported Huffman dispatch, and malformed input.
- Added a dependency-free benchmark harness at `rust/benches/rle_bit_stuffer.rs`.

## Compatibility Notes

- The Rust `Rle` byte stream layout is intended to match `src/LercLib/RLE.cpp`: little-endian signed counters, negative repeated runs, positive literal runs, and `i16::MIN` end-of-stream marker.
- The Rust `BitStuffer2` implementation currently supports Lerc2 version 3 and newer bit packing. The older pre-v2.3 packing path in `BitStuff_Before_Lerc2v3` and `BitUnStuff_Before_Lerc2v3` is explicitly rejected for now.
- The Rust `BitMask` preserves the C++ MSB-first bit order from `BitMask::Bit`; padding bits may remain set after `set_all_valid`, but `count_valid_bits` excludes them.
- The Rust Lerc2 parser supports versions 0 through 6 and currently covers the fast header path used by `Lerc2::GetHeaderInfo`, checksum validation used by `Lerc2::Decode`, mask reading used by `Lerc2::ReadMask`, min/max range parsing used by `Lerc2::ReadMinMaxRanges`, one-sweep raw payload expansion used by `Lerc2::ReadDataOneSweep`, raw-binary, simple bit-stuffed, LUT, and integer diff-encoded tiled payload expansion for `Lerc2::ReadTile`, single-band and multi-band supported-subset decode orchestration, and the Lerc2 branch of `Lerc::GetLercInfo`. It does not yet fall back to legacy Lerc1 metadata parsing.
- `decode_lerc2_supported` is intentionally decode-only and subset-scoped. It validates v3+ checksums and supports constant images, one-sweep raw payloads, and non-Huffman tiled payloads already covered by the lower-level readers. Huffman-backed image modes currently return `Unsupported`.
- `decode_lerc2_bands_supported` decodes concatenated Lerc2 blobs into separate band results and passes the previous band mask into later bands, matching the C++ previous-mask reuse mechanism for omitted partial masks.
- For v6 blobs with `bPassNoDataValues` and `nDepth > 1`, supported decode remaps decoded values equal to `noDataVal` back to `noDataValOrig` on valid pixels, matching the C++ `Lerc::RemapNoData` wrapper behavior.
- `get_lerc2_data_ranges` reports header ranges for single-depth bands and v4+ min/max range sections for multi-depth bands. Like the C++ public range API, it returns `HasNoData` instead of min/max values for multi-depth blobs that carry no-data sentinels.
- Decoded output-copy helpers use little-endian serialization explicitly and return `BufferTooSmall` before writing past caller-provided buffers. Multi-band output is band-major, matching the public C API data ordering.
- `decode_lerc2_supported_into` is a safe Rust internal equivalent of the output-buffer validation needed by `lerc_decode`: it requires `n_masks` to be 0, 1, or `n_bands`, rejects insufficient mask requests for blobs with masks, and returns `WrongParam` for shape/type mismatches.
- Lerc2 previous-mask reuse is explicit in Rust via `read_lerc2_mask_with_previous`; calling `read_lerc2_mask` on a blob that omits a partial mask returns an unsupported error.
- Public Rust APIs return `Result<T, LercError>` instead of bool/status pairs. C-compatible FFI wrappers should be added after the safe Rust codec surface is stable.

## Next Porting Steps

1. Add C++ parity fixtures for `RLE` and `BitStuffer2`.
   - Generate known byte streams from the C++ implementation.
   - Assert Rust can decode C++ bytes and that Rust encoded bytes match for deterministic cases.
2. Add C++ parity fixtures for `BitMask`.
   - Assert Rust bit-packed bytes match C++ conversion from byte masks.
   - Assert Rust byte-mask output matches C++ `Lerc::Convert(const BitMask&, Byte*)`.
3. Add C++ parity tests for `lerc_getBlobInfo`.
   - Compare Rust `get_lerc_info` with C API output for all `testData/*.lerc2` fixtures.
   - Compare Rust `get_lerc2_data_ranges` with C API `lerc_getDataRanges` for no-data-free fixtures.
   - Decide whether the Rust crate should implement Lerc1 metadata fallback or expose Lerc2-only behavior explicitly.
4. Port decode-only Lerc2 block paths.
   - Add C++ parity tests for `decode_lerc2_supported` on synthetic and fixture blobs that avoid Huffman.
   - Add C++ parity tests for `decode_lerc2_bands_supported` on concatenated non-Huffman fixtures.
   - Add C++ parity coverage for v6 no-data remapping across all supported native data types.
   - Extend diff-encoded coverage to LUT and floating-point cases where C++ supports them.
   - Decode Huffman-backed blocks separately.
   - Keep each block codec independently fuzzable and benchmarkable.
5. Add encode paths after decode parity is established.
   - Start with lossless integer and byte tiles.
   - Add floating point quantization and max error checks.
6. Add FFI surface.
   - Mirror `src/LercLib/include/Lerc_c_api.h` status codes and data type integers.
   - Use `decode_lerc2_supported_into` as the internal boundary for `lerc_decode`-style functions.
   - Keep panic boundaries explicit with C ABI tests.

## Verification Commands

```sh
cargo test
cargo bench --bench rle_bit_stuffer
```

Last run in this branch:

- `cargo test`: passed, 51 unit tests.
- `cargo bench --bench rle_bit_stuffer`: passed and printed timings for RLE compress/decompress, BitStuffer encode/decode, BitMask conversion/count operations, typed decoded value conversion, checked decoded output copying, Lerc2 metadata parsing, Lerc2 data-range aggregation, Lerc2 mask reading, Lerc2 checksum validation, Lerc2 min/max range parsing, Lerc2 one-sweep decode, Lerc2 raw tiled decode, Lerc2 simple bit-stuffed tiled decode, Lerc2 LUT tiled decode, Lerc2 diff tiled decode, high-level supported-subset Lerc2 decode, high-level supported-subset multi-band Lerc2 decode, v6 supported-subset no-data decode, and C API-style supported decode-into-buffer validation.

## Open Risks

- Some C++ code relies on native integer memory behavior but serializes little-endian streams. Rust code should continue using explicit little-endian reads/writes.
- Full codec parity will need fixture generation against the C++ branch so byte-for-byte expectations are not inferred only from the source.
- Huffman and predictor paths have a larger state surface and should be ported only after the non-Huffman decode dispatcher is locked down with cross-language fixtures.
