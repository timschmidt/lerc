# Rust Porting Plan

This branch is the start of a native Rust port of the LERC C++ library. The goal is to keep the C++ implementation as the behavioral reference while moving codec pieces into safe, testable Rust modules.

## Current Scope

- Added a Cargo workspace at the repository root with the Rust crate in `rust/`.
- Reorganized the Rust crate into semantic module directories: `rust/src/data/` for shared types and decoded values, `rust/src/primitives/` for reusable codec primitives, `rust/src/format/` for Lerc1/Lerc2 internals, `rust/src/c_api/` for C ABI wrappers, and reserved `rust/src/api/` plus `rust/src/support/` for workflow facades and shared private helpers as the port grows. The crate root keeps the existing public re-exports stable.
- Ported low-level `RLE` compression/decompression into `rust/src/primitives/rle.rs`, including pinned literal and repeated-run byte-stream parity tests.
- Ported `BitStuffer2` simple and LUT modes into `rust/src/primitives/bit_stuffer.rs`, including both the Lerc2 v2.3+ little-endian packing and the pre-v2.3 legacy word layout.
- Ported `BitMask` into `rust/src/primitives/bit_mask.rs`, including byte-mask conversion helpers and pinned packed-byte parity tests equivalent to the private C++ `Lerc::Convert` methods.
- Added Lerc2 header parsing and multi-band metadata aggregation into `rust/src/format/lerc2/mod.rs`.
- Added the first legacy Lerc1 reader in `rust/src/format/lerc1/mod.rs`, covering `CntZImage` header and part-header parsing, non-tiled count/mask decoding, tiled count/mask decoding, z-tile min/max statistics, row-major float payload decode, safe `get_lerc_info` metadata fallback, and safe/C ABI data-range reporting for the checked-in `world.lerc1` fixture.
- Added Lerc2 mask-section reading, including RLE mask decompression and previous-mask reuse for concatenated bands.
- Added Lerc2 Fletcher32 checksum calculation and validation for version 3+ blobs.
- Added Lerc2 v4+ min/max range-section parsing for all native Lerc data types.
- Added Lerc2 one-sweep raw payload decoding, matching `Lerc2::ReadDataOneSweep`.
- Added Lerc2 tiled raw-binary payload decoding for non-diff raw and constant-zero tile blocks.
- Added Lerc2 tiled simple bit-stuffed payload decoding and constant-zMin tile handling.
- Added Lerc2 tiled LUT payload decoding via the shared `BitStuffer2` stream decoder.
- Added Lerc2 v5 diff-encoded tiled payload decoding for integer and floating-point simple, LUT, and constant tile modes.
- Added Lerc2 integer Huffman code-table and payload decoding for byte image modes, covering both regular Huffman and delta Huffman layouts used by the C++ decoder.
- Added `encode_lerc2_byte_huffman`, `encode_lerc2_byte_huffman_with_no_data`, `encode_lerc2_byte_huffman_bands`, and `encode_lerc2_byte_huffman_bands_with_no_data`, safe version 2+ byte Huffman encoders for `UChar` and `Char` data with `max_z_error == 0.5`. They write the same Lerc2 Huffman payload envelope consumed by the supported decoder, select the smaller regular-vs-delta Huffman candidate per band for version 4+ blobs, use delta Huffman for pre-v4 blobs, support band-major shared/per-band mask handling, and reuse the version 6 no-data sentinel preprocessing helpers. `encode_lerc2_auto` and `encode_lerc2_auto_with_no_data` now compare byte-Huffman candidates against the existing uncompressed baseline for eligible single-band and multi-band inputs.
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
- Added safe Lerc2 min/max range byte-size and serialization helpers for encode groundwork, matching the C++ v4+ range-section layout.
- Added safe Lerc2 one-sweep payload byte-size and serialization helpers for encode groundwork, matching `Lerc2::WriteDataOneSweep`.
- Added safe Lerc2 raw tiled payload byte-size and serialization helpers for encode groundwork, matching the raw tile layout used by `Lerc2::ReadTile`.
- Added safe Lerc2 checksum finalization for encode groundwork, matching the C++ post-write checksum fill step for version 3+ blobs.
- Added the first narrow safe Lerc2 encoder path, `encode_lerc2_constant`, for single-band constant-value blobs with optional masks.
- Added safe Lerc2 encode range computation from typed little-endian source bytes, a version 2+ single-band one-sweep encoder, `encode_lerc2_one_sweep`, a version 4+ single-band raw tiled encoder, `encode_lerc2_tiled_raw`, multi-band constant, one-sweep, and raw tiled encoders, `encode_lerc2_constant_bands`, `encode_lerc2_one_sweep_bands`, and `encode_lerc2_tiled_raw_bands`, version 2+ single-band and band-major byte Huffman encoders with and without v6 no-data metadata, v6 no-data sentinel handling via single-band and band-major helpers, high-level `encode_lerc2_uncompressed` / `encode_lerc2_uncompressed_with_no_data` fallback selectors for uncompressed blobs, `encode_lerc2_auto` for no-data-free size-based encode choice, and `encode_lerc2_auto_with_no_data` for active no-data-aware size-based encode choice. Pre-v4 one-sweep and byte-Huffman output is limited to single-depth blobs, matching the older Lerc2 header layout; pre-v4 multi-band one-sweep output is stored as concatenated single-depth blobs with legacy next-header discovery. Pre-v4 byte Huffman uses delta Huffman because regular Huffman image mode is version 4+. Raw tiled encoding emits image mode 0 when the header enters the C++ Huffman-probe envelope. Integer constant, one-sweep, and raw tiled encoders, including single-band no-data wrappers, now normalize `max_z_error` like C++: values floor to at least `0.5`, and negative values are treated as bit-plane epsilons where applicable. The auto selectors use that normalized value when deciding whether byte Huffman is eligible. Float and double one-sweep/raw tiled encoders now port the C++ positive-error raise heuristic when exact rounded candidates preserve the requested tolerance.
- Added rustdoc comments across the public Rust API and enabled crate-level `#![deny(missing_docs)]` so documentation completeness is enforced at compile time.
- Added a safe `get_lerc2_blob_info_arrays` helper that fills C API-style blob info and quick data-range arrays from Lerc2 metadata, with fixture coverage for the byte and float Lerc2 sample blobs.
- Added the first Rust C ABI entry point, `lerc_getBlobInfo`, backed by the safe metadata array helper and guarded with a panic boundary.
- Added the Rust C ABI entry point `lerc_getDataRanges`, backed by the safe data-range aggregator, cross-checked against checked-in Lerc2 fixtures and `world.lerc1`, and guarded with a panic boundary.
- Added the Rust C ABI entry point `lerc_decode` for the supported Lerc2 decode subset and the checked-in Lerc1 fixture path, backed by safe decode helpers and guarded with a panic boundary.
- Added the Rust C ABI entry point `lerc_decodeToDouble` for the supported Lerc2 decode subset and the checked-in Lerc1 fixture path, backed by native decode plus typed conversion into caller-provided `double` output.
- Added the Rust C ABI entry points `lerc_decode_4D` and `lerc_decodeToDouble_4D` for the supported Lerc2 decode subset and the checked-in Lerc1 fixture path, including per-band no-data reporting arrays for v6 multi-depth Lerc2 blobs.
- Added encode-side C ABI support for constant, one-sweep single-band, byte-Huffman single-band and multi-band when smaller, one-sweep multi-band, and 4D calls with or without active no-data metadata through the same safe selectors used by the Rust API. Active no-data byte-Huffman support expects the source data to contain the declared sentinel, masks all-depth no-data pixels, and remaps conflicting mixed-depth sentinels to an internal value for storage.
- Added C++-style encode-side NaN preprocessing for float/double C ABI inputs without active no-data metadata: all-depth NaN pixels are masked out, single-depth NaNs are masked out, and mixed-depth NaNs without no-data metadata return the public `NaN` status.
- Promoted the real `california_400_400_1_float.lerc2` fixture into supported-subset decode coverage, including safe Rust decode plus `lerc_decode` and `lerc_decodeToDouble` FFI output checks.
- Configured the Rust crate to build `rlib`, `cdylib`, and `staticlib` outputs.
- Added unit tests for RLE round trips, RLE boundary counters, pinned RLE literal/repeated byte streams, adversarial RLE count streams, bit widths, LUT streams, adversarial BitStuffer2 headers and payloads, bit-mask packing, pinned BitMask byte conversion, typed decoded values, checked decoded output copying, decoded `f64` conversion, C API-style decode-into validation, `DecodeIntoSpec` buffer sizing and overflow checks, `EncodeSpec` validation and overflow checks, format-agnostic Lerc1/Lerc2 decode-into dispatch, format-agnostic Lerc1/Lerc2 decode-to-`f64` dispatch, C API-style blob info arrays across Lerc2 fixtures and the Lerc1 fixture, encode-side C ABI placeholder validation, C++ sample-style C ABI round trips for masked float images, `nDepth = 3` byte images, 4D no-data float images, and multi-band float images containing NaNs, C ABI NaN rejection for unsupported mixed-depth inputs, C ABI malformed-blob decode/info rejection, safe and C ABI constant Lerc2 encode/decode round trips, safe Lerc2 encode range computation, safe and C ABI one-sweep Lerc2 encode/decode round trips, safe raw tiled Lerc2 encode/decode round trips, safe and C ABI multi-band one-sweep Lerc2 encode/decode round trips, safe multi-band raw tiled Lerc2 encode/decode round trips, safe byte Huffman Lerc2 encode/decode round trips for signed and unsigned byte data, safe and C ABI byte-Huffman auto-selection when smaller for single-band, multi-band, and active no-data inputs, safe and C ABI v6 no-data metadata one-sweep encode/decode round trips, safe v6 no-data metadata raw tiled encode/decode round trips, safe uncompressed Lerc2 fallback encode selection with and without active no-data metadata, C ABI encode parity with the safe auto selector, the `lerc_getBlobInfo`, `lerc_getDataRanges`, `lerc_decode`, `lerc_decodeToDouble`, `lerc_decode_4D`, and `lerc_decodeToDouble_4D` C ABI wrappers, direct legacy Lerc1 C ABI blob-info, data-range, native decode, double decode, and 4D decode array checks, Lerc1 fixture header parsing, non-tiled and tiled Lerc1 count-mask decoding, Lerc1 z-stat decoding, Lerc1 float value decoding, Lerc1 data-range reporting, Lerc2 fixture metadata, Lerc2 header byte sizing and writing, Lerc2 mask byte sizing and writing, Lerc2 min/max range byte sizing and writing, Lerc2 one-sweep and raw tiled payload byte sizing and writing, Lerc2 masks, checksum validation and finalization, checksum mismatch detection, synthetic v4 min/max ranges, fixture data ranges, one-sweep payload expansion, raw tiled payload expansion, simple bit-stuffed tiled expansion, LUT tiled expansion, integer and floating-point diff tiled simple/LUT/constant expansion, integer byte Huffman fixture decoding through safe Rust and the C ABI, high-level supported decode dispatch, real fixture supported decode dispatch, multi-band supported decode dispatch with previous-mask reuse, v6 no-data remapping, v6 no-data metadata reporting, data-range aggregation, `HasNoData` range reporting, and malformed input.
- Added a dependency-free benchmark harness at `rust/benches/primitives.rs`.

## Compatibility Notes

- The Rust `Rle` byte stream layout is intended to match `src/LercLib/RLE.cpp`: little-endian signed counters, negative repeated runs, positive literal runs, and `i16::MIN` end-of-stream marker.
- The Rust `BitStuffer2` implementation supports Lerc2 version 3 and newer bit packing plus the older pre-v2.3 packing path in `BitStuff_Before_Lerc2v3` and `BitUnStuff_Before_Lerc2v3`.
- The Rust `BitMask` preserves the C++ MSB-first bit order from `BitMask::Bit`; padding bits may remain set after `set_all_valid`, but `count_valid_bits` excludes them.
- The Rust Lerc2 parser supports versions 0 through 6 and currently covers the fast header path used by `Lerc2::GetHeaderInfo`, checksum validation used by `Lerc2::Decode`, mask reading used by `Lerc2::ReadMask`, min/max range parsing used by `Lerc2::ReadMinMaxRanges`, one-sweep raw payload expansion used by `Lerc2::ReadDataOneSweep`, raw-binary, simple bit-stuffed, LUT, and integer diff-encoded tiled payload expansion for `Lerc2::ReadTile`, direct tiled-payload reads for both legacy raw tiled streams and C++ Huffman-probe image-mode-0 envelopes, single-band and multi-band supported-subset decode orchestration, pre-v2.3 bit-stuffed tile streams, the Lerc2 branch of `Lerc::GetLercInfo`, and the legacy Lerc1 metadata fallback in the public metadata helpers.
- `compute_lerc2_header_byte_len` and `write_lerc2_header` mirror the C++ header layout, including v3+ checksum storage and v6 no-data metadata fields. Encoders can write a zero checksum placeholder and fill it after payload assembly.
- `compute_lerc2_mask_byte_len` and `write_lerc2_mask` mirror the C++ mask-section layout: a 4-byte encoded-mask length followed by RLE-compressed packed mask bytes only when a partial mask is explicitly encoded.
- `compute_lerc2_min_max_ranges_byte_len` and `write_lerc2_min_max_ranges` mirror the C++ v4+ range-section layout by writing all per-depth minimums followed by all per-depth maximums in the blob scalar type.
- `compute_lerc2_one_sweep_byte_len` and `write_lerc2_one_sweep` mirror the C++ one-sweep layout: a one-byte mode flag followed by contiguous depth tuples for valid pixels only.
- `compute_lerc2_tiled_raw_byte_len` and `write_lerc2_tiled_raw` mirror the raw tiled payload layout: a zero one-sweep flag followed by one tile mode byte and raw valid-pixel scalar bytes for each tile and depth. The higher-level raw tiled encoders additionally emit image mode 0 for header configurations that use the C++ Huffman-probe envelope, and the direct tiled reader now consumes that mode-0 prefix while preserving compatibility with existing unprefixed synthetic tiled streams.
- `finalize_lerc2_checksum` mirrors the C++ `DoChecksOnEncode` checksum fill step by hashing bytes after the checksum field through the declared blob size and writing the result back into the header.
- `encode_lerc2_constant` mirrors the C++ early-return encode path for constant images: it writes header and mask sections only, then finalizes the checksum. It is currently scoped to one band and one constant value shared by all depths.
- `compute_lerc2_data_ranges_for_encode` mirrors the C++ valid-pixel min/max scan for encoder setup, ignoring invalid pixels and rejecting NaN input.
- `encode_lerc2_one_sweep` assembles version 2+ single-band blobs from the safe header, mask, optional v4+ min/max range, one-sweep payload, and checksum helpers. It covers uncompressed fallback blobs and the v4+ all-depths-constant range-only early return.
- `encode_lerc2_uncompressed` chooses among the ported uncompressed safe encoders: constant for single-band constant input, concatenated constant blobs for multi-band constant input, one-sweep for single-band varying input, and concatenated one-sweep for other multi-band input.
- `encode_lerc2_uncompressed_with_no_data` extends the same selector with optional v6 no-data metadata, using the plain selector when no bands are active and the no-data one-sweep path when any band is active.
- `encode_lerc2_tiled_raw` assembles version 4+ single-band blobs from the safe header, mask, min/max range, raw tiled payload, and checksum helpers. For header settings that require the C++ Huffman-probe envelope, it writes image mode 0 and keeps the payload raw.
- `encode_lerc2_one_sweep_bands` mirrors the C++ band loop for concatenated Lerc2 output: data is band-major, one shared mask can be stored once and reused by later bands, per-band masks are stored when they differ, pre-v4 output is limited to single-depth blobs, and v6 `nBlobsMore` counts remaining blobs.
- `encode_lerc2_tiled_raw_bands` mirrors the same C++ band loop for concatenated raw tiled output, including shared-mask omission after the first band and v6 `nBlobsMore` counts.
- `encode_lerc2_one_sweep_bands_with_no_data` writes v6 no-data metadata for bands flagged by the caller, marks pixels invalid when every depth equals the supplied sentinel, and remaps mixed-depth sentinel samples to an internal no-data value when the original sentinel is too close to the valid data range.
- `encode_lerc2_tiled_raw_bands_with_no_data` applies the same v6 no-data metadata, all-depth mask filtering, and mixed-depth internal sentinel remapping to raw tiled output.
- `encode_lerc2_one_sweep_with_no_data` and `encode_lerc2_tiled_raw_with_no_data` provide single-band convenience wrappers over the same v6 no-data encode preparation.
- The Rust Lerc1 reader currently validates the legacy `CntZImage` file key, version/type fields, dimensions, max-z-error, and count/z part headers. It also decodes non-tiled count parts stored as constant counts or RLE-compressed packed masks, decodes tiled count parts stored as constants, raw float counts, or bit-stuffed integer counts, computes z min/max statistics from tiled z payloads, decodes tiled z payloads into row-major `f32` values, and is integrated into the safe `get_lerc_info`, blob-info array, and data-range helper paths. `lerc_getBlobInfo` and `lerc_getDataRanges` inherit that fallback through the shared helpers.
- `decode_lerc2_supported` is intentionally decode-only and subset-scoped. It validates v3+ checksums and supports constant images, one-sweep raw payloads, non-Huffman tiled payloads already covered by the lower-level readers, integer byte Huffman image modes, and version 6 floating-point Huffman image mode 3 for `Float`/`Double` payloads.
- Floating-point Huffman decode groundwork is now wired into the supported Lerc2 path: the Rust port mirrors the C++ `Predictor` code/type/delta mapping, `fpl_Compression::extract_buffer` wrapper modes, byte-plane delta restoration, byte-plane reassembly, float transform undo, and row/cross predictor restoration used by `LosslessFPCompression::DecodeHuffmanFltSlice`. Real C++-generated FP Huffman fixtures are still pending.
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
- The Rust `lerc_decode` C ABI currently supports the same Lerc2 subset as `decode_lerc2_supported_into`, including integer byte Huffman decode, plus the checked-in Lerc1 float fixture path. It validates shape, data type, mask count, and raw output pointers like the C++ API, and returns `HasNoData` for regular non-4D decode when multi-depth no-data metadata is present.
- The Rust `lerc_decodeToDouble` C ABI currently supports the same Lerc2 subset and checked-in Lerc1 fixture path as `lerc_decode`, including the same regular non-4D `HasNoData` behavior for Lerc2 no-data blobs.
- The Rust `lerc_decode_4D` and `lerc_decodeToDouble_4D` C ABI wrappers currently support the same Lerc2 subset and checked-in Lerc1 fixture path as the regular decode wrappers. They require no-data output arrays only when multi-depth no-data metadata is present, then report each decoded band's `bPassNoDataValues` flag and original no-data sentinel for Lerc2 blobs.
- Encode-side C ABI functions now route no-data-free inputs through `encode_lerc2_auto` and active no-data inputs through `encode_lerc2_auto_with_no_data`, enabling byte-Huffman single-band, multi-band, and v6 no-data output when it beats the uncompressed baseline. Active no-data encode still requires the caller's data to already contain the declared no-data sentinel and applies the same all-depth mask filtering and mixed-depth sentinel remapping as the safe no-data helper.
- Encode-side C ABI float/double inputs without active no-data metadata now mirror C++ NaN preprocessing for the supported uncompressed path. NaNs at fully invalid pixels are ignored, all-depth NaN pixels become mask-invalid, single-depth NaNs become mask-invalid, and mixed-depth NaNs require explicit no-data metadata.
- `EncodeSpec` centralizes the checked shape and byte-count validation currently shared by encode-side C ABI placeholders.
- Lerc2 header serialization is now isolated behind a safe helper so the encoder can reuse one layout implementation for compute-size, encode, and fixture construction.
- Lerc2 mask serialization is now isolated behind a safe helper so compute-size and encode paths can share validation for explicit masks and omitted previous-mask sections.
- Lerc2 min/max range serialization is now isolated behind a safe helper so v4+ compute-size and encode paths can share range layout validation.
- Lerc2 one-sweep payload serialization is now isolated behind a safe helper so uncompressed fallback encode paths can share payload layout validation.
- Lerc2 raw tiled payload serialization is now isolated behind a safe helper so tiled encode paths can reuse the same tile order, integrity bits, and valid-pixel filtering as the decoder.
- Lerc2 checksum finalization is now isolated behind a safe helper so encoders can assemble blobs with a zero checksum placeholder and finalize them after payload writing.
- Constant-value Lerc2 encoding is now available as the first safe encode path and round-trips through the supported decoder.
- Multi-band constant Lerc2 encoding is now available as a safe concatenated encode path, including version 6 `nBlobsMore` counts and previous-mask reuse for shared masks.
- Version 2+ one-sweep Lerc2 encoding is now available as the first safe non-constant encode path and round-trips through the supported single-band and multi-band decoders. Pre-v4 output is constrained to single-depth blobs and omits the v4 min/max range section.
- High-level safe uncompressed Lerc2 encode selection is now available for callers that want the current Rust fallback behavior without manually choosing constant versus one-sweep helpers, with or without active v6 no-data metadata.
- Version 4+ raw tiled Lerc2 encoding is now available as a safe single-band and multi-band uncompressed encode path and round-trips through the supported decoders, including image-mode-0 blobs inside the C++ Huffman-probe envelope and v6 no-data metadata.
- Version 2+ byte Huffman Lerc2 encoding is now available as safe single-band and band-major encode paths for `UChar` and `Char` data with `max_z_error == 0.5`, including version 6 active no-data metadata. It round-trips through the supported decoder, writes explicit Huffman code tables, supports delta-only pre-v4 blobs plus regular-vs-delta selection for version 4+, and supports no mask, shared mask, per-band mask, and previous-mask reuse sections; byte-for-byte C++ parity still needs generated fixtures.
- Integer constant, one-sweep, and raw tiled encoders, including version 6 single-band no-data helpers, port the C++ integer `maxZError` normalization: nonnegative values floor to at least `0.5`, negative values are interpreted as bit-plane epsilons for non-constant data, noisy low bit planes may raise the stored max error, and the fallback stored error is `0.5` when the heuristic has too little data. Float and double inputs still reject negative values.
- Float and double one-sweep/raw tiled encoders port the C++ `TryRaiseMaxZError` heuristic for positive error values, pruning candidate decimal error factors row by row and storing the coarser candidate when it remains within the caller's original tolerance.
- `encode_lerc2_auto` currently chooses between the uncompressed baseline and single-band or multi-band byte Huffman for eligible no-data-free byte data. `encode_lerc2_auto_with_no_data` applies the same choice after v6 no-data preprocessing. Both selectors treat negative integer `max_z_error` as byte-Huffman eligible when the normalized stored error is `0.5`. C ABI compute-size and encode wrappers use these selectors.
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
   - Add C++ parity tests for `decode_lerc2_supported` on synthetic and fixture blobs.
   - Add C++ parity tests for `decode_lerc2_bands_supported` on concatenated fixtures.
   - Add C++ parity coverage for v6 no-data remapping across all supported native data types.
   - Add broader C++ parity fixtures for diff-encoded integer and floating-point tile modes.
   - Floating-point Huffman predictor code mapping, wrapped byte-plane extraction, byte-plane delta restoration, byte-plane reassembly, row/cross predictor restoration, and supported-subset image-mode-3 dispatch are now pinned in Rust tests.
   - Add C++ parity fixtures for floating-point Huffman and predictor-backed blocks.
   - Keep each block codec independently fuzzable and benchmarkable.
5. Add encode paths after decode parity is established.
   - Header byte sizing and serialization are now available as safe Rust helpers.
   - Mask byte sizing and serialization are now available as safe Rust helpers.
   - Min/max range byte sizing and serialization are now available as safe Rust helpers.
   - One-sweep payload byte sizing and serialization are now available as safe Rust helpers.
   - Raw tiled payload byte sizing and serialization are now available as safe Rust helpers.
   - Checksum finalization is now available as a safe Rust helper.
   - Constant-value single-band Lerc2 encoding is now available as a safe Rust helper.
   - Multi-band constant Lerc2 encoding is now available as a safe Rust helper and is selected by the uncompressed fallback when every band is constant.
   - Version 2+ single-band one-sweep Lerc2 encoding is now available as a safe Rust helper, with pre-v4 blobs limited to single-depth data.
   - Version 4+ single-band raw tiled Lerc2 encoding is now available as a safe Rust helper, including image mode 0 for Huffman-probe envelope header configurations.
   - Version 2+ single-band and band-major byte Huffman Lerc2 encoding is now available as safe Rust helpers for `UChar` and `Char` data, including version 6 no-data metadata helpers.
   - Version 2+ multi-band one-sweep Lerc2 encoding is now available as a safe Rust helper, with pre-v4 blobs limited to single-depth data.
   - Version 4+ multi-band raw tiled Lerc2 encoding is now available as a safe Rust helper, including image mode 0 for Huffman-probe envelope header configurations.
   - C++-style negative `maxZError` bit-plane heuristic is now ported for integer one-sweep and raw tiled encode paths.
   - C++-style positive `maxZError` raise heuristic is now ported for float/double one-sweep and raw tiled encode paths.
   - Version 6 no-data one-sweep and raw tiled encoding are now available as safe Rust single-band and band-major helpers when data already contains the sentinel value, including all-depth no-data mask filtering and mixed-depth internal sentinel remapping.
   - Safe uncompressed encode selection now chooses constant, single-band one-sweep, or multi-band one-sweep paths from one helper, with an optional no-data-aware selector for v6 metadata.
   - Safe auto encode selection now chooses byte Huffman when it beats the uncompressed baseline for eligible single-band, multi-band, and active no-data byte input.
   - Start with lossless integer and byte tiles.
   - Add floating point quantization and max error checks.
6. Add FFI surface.
   - Continue mirroring `src/LercLib/include/Lerc_c_api.h` status codes and data type integers.
   - Replace encode-side placeholders with real compute-size and encode implementations after the Rust encoder is ported.
   - Constant, one-sweep single-band, byte-Huffman single-band and multi-band with or without active no-data metadata, one-sweep multi-band, and 4D C ABI encode paths now use the safe Rust encoder.
   - Keep panic boundaries explicit with C ABI tests.

## Verification Commands

```sh
cargo check
cargo test
cargo doc --no-deps
cargo bench --bench primitives
```

Last run in this branch:

- `cargo check`: passed with `#![deny(missing_docs)]` enabled.
- `cargo test`: passed, 199 unit tests.
- `cargo doc --no-deps`: passed and generated crate documentation.
- `cargo bench --bench primitives`: passed and printed timings for RLE compress/decompress, BitStuffer encode/decode, pre-v2.3 BitStuffer encode/decode, BitMask conversion/count operations, typed decoded value conversion, checked decoded output copying, decoded `f64` conversion through C ABI, DecodeIntoSpec byte-count helpers, EncodeSpec validation and byte-count helpers, Lerc1 header parsing, Lerc1 count-mask reading, Lerc1 z-stat reading, Lerc1 float value decoding, Lerc1 safe decode-into, Lerc1 safe decode-to-`f64`, Lerc1 C ABI decode, Lerc1 C ABI decode-to-double, Lerc1 info aggregation, Lerc1 data-range aggregation, Lerc2 metadata parsing, C API-style blob info array filling, Lerc2 data-range aggregation, C ABI data-range retrieval, Lerc2 mask reading, Lerc2 checksum validation and finalization, Lerc2 header writing, Lerc2 mask writing, Lerc2 min/max range parsing and writing, Lerc2 one-sweep and raw tiled payload writing, Lerc2 constant encoding, high-level uncompressed encode selection with and without no-data metadata, Lerc2 auto encode selection, Lerc2 one-sweep single-band, raw tiled single-band including image-mode-0 envelope and direct image-mode-0 tiled readback, byte Huffman single-band, multi-band, and active no-data, one-sweep and raw tiled multi-band, v6 one-sweep and raw tiled no-data metadata encoding, and v6 no-data metadata encoding, C ABI constant and one-sweep single-band, multi-band, no-active-no-data 4D, and active-no-data metadata 4D encode compute-size and encode, Lerc2 no-data metadata reporting, Lerc2 one-sweep decode, Lerc2 raw tiled decode, Lerc2 simple bit-stuffed tiled decode, Lerc2 LUT tiled decode, Lerc2 integer and floating-point diff tiled simple/LUT decode, floating-point Huffman predictor and wrapped byte-plane extraction tests, high-level supported-subset Lerc2 decode, high-level supported-subset multi-band Lerc2 decode, Lerc2 safe decode-to-`f64`, real float fixture decode, byte Huffman fixture decode, v6 supported-subset no-data decode, C API-style supported decode-into-buffer validation, C ABI decode, C ABI decode-to-double, C ABI real float fixture decode, C ABI byte Huffman fixture decode, and C ABI 4D no-data decode variants.

## Open Risks

- Some C++ code relies on native integer memory behavior but serializes little-endian streams. Rust code should continue using explicit little-endian reads/writes.
- Full codec parity will need fixture generation against the C++ branch so byte-for-byte expectations are not inferred only from the source.
- Lerc1 metadata parity is currently covered by the checked-in `world.lerc1` fixture; tiled count modes have synthetic Rust coverage, while broader Lerc1 z-tile and cross-language tile-mode coverage still needs generated fixtures.
- Floating-point Huffman decode now has native Rust synthetic coverage, but still needs separate C++-generated cross-language fixtures before being treated as parity-complete. Broader C API compressed encode parity also needs generated fixtures.
