# Rust Porting Plan

This branch is the start of a native Rust port of the LERC C++ library. The goal is to keep the C++ implementation as the behavioral reference while moving codec pieces into safe, testable Rust modules.

## Current Scope

- Added a Cargo workspace at the repository root with the Rust crate in `rust/`.
- Ported low-level `RLE` compression/decompression into `rust/src/rle.rs`.
- Ported the Lerc2 v2.3+ `BitStuffer2` simple and LUT modes into `rust/src/bit_stuffer.rs`.
- Added unit tests for round trips, boundary counters, bit widths, LUT streams, and malformed input.
- Added a dependency-free benchmark harness at `rust/benches/rle_bit_stuffer.rs`.

## Compatibility Notes

- The Rust `Rle` byte stream layout is intended to match `src/LercLib/RLE.cpp`: little-endian signed counters, negative repeated runs, positive literal runs, and `i16::MIN` end-of-stream marker.
- The Rust `BitStuffer2` implementation currently supports Lerc2 version 3 and newer bit packing. The older pre-v2.3 packing path in `BitStuff_Before_Lerc2v3` and `BitUnStuff_Before_Lerc2v3` is explicitly rejected for now.
- Public Rust APIs return `Result<T, LercError>` instead of bool/status pairs. C-compatible FFI wrappers should be added after the safe Rust codec surface is stable.

## Next Porting Steps

1. Add C++ parity fixtures for `RLE` and `BitStuffer2`.
   - Generate known byte streams from the C++ implementation.
   - Assert Rust can decode C++ bytes and that Rust encoded bytes match for deterministic cases.
2. Port `BitMask`.
   - Preserve MSB-first bit order from `BitMask::Bit`.
   - Add byte-mask conversion tests matching `Lerc::Convert`.
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

- `cargo test`: passed, 8 unit tests.
- `cargo bench --bench rle_bit_stuffer`: passed and printed timings for RLE compress/decompress and BitStuffer encode/decode.

## Open Risks

- Some C++ code relies on native integer memory behavior but serializes little-endian streams. Rust code should continue using explicit little-endian reads/writes.
- Full codec parity will need fixture generation against the C++ branch so byte-for-byte expectations are not inferred only from the source.
- Huffman, predictor, and Lerc2 block orchestration have a larger state surface and should be ported only after the low-level primitives are locked down with cross-language fixtures.
