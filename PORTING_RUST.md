# Rust Porting Plan

This branch is the start of a native Rust port of the LERC C++ library. The goal is to keep the C++ implementation as the behavioral reference while moving codec pieces into safe, testable Rust modules.

## Current Scope

- Added a Cargo workspace at the repository root with the Rust crate in `rust/`.
- Ported low-level `RLE` compression/decompression into `rust/src/rle.rs`.
- Ported the Lerc2 v2.3+ `BitStuffer2` simple and LUT modes into `rust/src/bit_stuffer.rs`.
- Ported `BitMask` into `rust/src/bit_mask.rs`, including byte-mask conversion helpers equivalent to the private C++ `Lerc::Convert` methods.
- Added unit tests for round trips, boundary counters, bit widths, LUT streams, bit-mask packing, byte-mask conversion, and malformed input.
- Added a dependency-free benchmark harness at `rust/benches/rle_bit_stuffer.rs`.

## Compatibility Notes

- The Rust `Rle` byte stream layout is intended to match `src/LercLib/RLE.cpp`: little-endian signed counters, negative repeated runs, positive literal runs, and `i16::MIN` end-of-stream marker.
- The Rust `BitStuffer2` implementation currently supports Lerc2 version 3 and newer bit packing. The older pre-v2.3 packing path in `BitStuff_Before_Lerc2v3` and `BitUnStuff_Before_Lerc2v3` is explicitly rejected for now.
- The Rust `BitMask` preserves the C++ MSB-first bit order from `BitMask::Bit`; padding bits may remain set after `set_all_valid`, but `count_valid_bits` excludes them.
- Public Rust APIs return `Result<T, LercError>` instead of bool/status pairs. C-compatible FFI wrappers should be added after the safe Rust codec surface is stable.

## Next Porting Steps

1. Add C++ parity fixtures for `RLE` and `BitStuffer2`.
   - Generate known byte streams from the C++ implementation.
   - Assert Rust can decode C++ bytes and that Rust encoded bytes match for deterministic cases.
2. Add C++ parity fixtures for `BitMask`.
   - Assert Rust bit-packed bytes match C++ conversion from byte masks.
   - Assert Rust byte-mask output matches C++ `Lerc::Convert(const BitMask&, Byte*)`.
3. Port Lerc2 header parsing.
   - Start with metadata-only reads for `lerc_getBlobInfo` parity.
   - Use existing files in `testData/` as decode fixtures.
4. Port decode-only Lerc2 block paths.
   - Decode uncompressed, bit-stuffed, RLE, and Huffman-backed blocks separately.
   - Keep each block codec independently fuzzable and benchmarkable.
5. Add encode paths after decode parity is established.
   - Start with lossless integer and byte tiles.
   - Add floating point quantization and max error checks.
6. Add FFI surface.
   - Mirror `src/LercLib/include/Lerc_c_api.h` status codes and data type integers.
   - Keep panic boundaries explicit with C ABI tests.

## Verification Commands

```sh
cargo test
cargo bench --bench rle_bit_stuffer
```

Last run in this branch:

- `cargo test`: passed, 12 unit tests.
- `cargo bench --bench rle_bit_stuffer`: passed and printed timings for RLE compress/decompress, BitStuffer encode/decode, and BitMask conversion/count operations.

## Open Risks

- Some C++ code relies on native integer memory behavior but serializes little-endian streams. Rust code should continue using explicit little-endian reads/writes.
- Full codec parity will need fixture generation against the C++ branch so byte-for-byte expectations are not inferred only from the source.
- Huffman, predictor, and Lerc2 block orchestration have a larger state surface and should be ported only after the low-level primitives are locked down with cross-language fixtures.
