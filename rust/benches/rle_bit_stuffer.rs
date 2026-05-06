use std::hint::black_box;
use std::time::{Duration, Instant};

use lerc::{
    compute_checksum_fletcher32, decode_lerc2_bands_supported, decode_lerc2_supported,
    decode_lerc2_supported_into, decode_typed_values, get_lerc1_header_info,
    get_lerc2_blob_info_arrays, get_lerc2_data_ranges, get_lerc2_header_info, get_lerc_info,
    read_lerc1_count_mask, read_lerc1_z_stats, read_lerc2_data_one_sweep, read_lerc2_mask,
    read_lerc2_min_max_ranges, read_lerc2_tiled_payload, read_lerc2_tiled_raw,
    validate_lerc2_checksum, BitMask, BitStuffer2, DataType, DecodeIntoSpec, Rle,
};

fn bench<F: FnMut()>(name: &str, iterations: u32, mut f: F) {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations;
    println!(
        "{name}: {iterations} iterations in {:?} ({:?}/iter)",
        elapsed, per_iter
    );
}

fn main() {
    let byte_data: Vec<u8> = (0..1_000_000)
        .map(|i| {
            if i % 97 < 55 {
                (i / 97 % 251) as u8
            } else {
                (i % 251) as u8
            }
        })
        .collect();
    let mask_data: Vec<u8> = (0..1_000_000)
        .map(|i| if i % 17 == 0 || i % 31 == 0 { 0 } else { 1 })
        .collect();
    let uint_data: Vec<u32> = (0..250_000).map(|i| ((i * 37) & 0x3fff) as u32).collect();
    let lerc2_blob = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testData/bluemarble_256_256_3_byte.lerc2"
    ))
    .unwrap();
    let float_fixture_blob = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testData/california_400_400_1_float.lerc2"
    ))
    .unwrap();
    let lerc1_blob = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testData/world.lerc1"
    ))
    .unwrap();
    let mut blob_info_array = [0u32; 11];
    let mut blob_range_array = [0.0f64; 3];
    let mut data_range_mins = [0.0f64; 3];
    let mut data_range_maxs = [0.0f64; 3];
    let min_max_blob = synthetic_v4_min_max_blob();
    let one_sweep_blob = synthetic_v4_one_sweep_blob();
    let mut one_sweep_bands_blob = one_sweep_blob.clone();
    one_sweep_bands_blob.extend_from_slice(&one_sweep_blob);
    let one_sweep_bands_decoded = decode_lerc2_bands_supported(&one_sweep_bands_blob).unwrap();
    let mut decoded_data_bytes = vec![0; one_sweep_bands_decoded.data_byte_len()];
    let mut decoded_mask_bytes = vec![0; one_sweep_bands_decoded.mask_byte_len()];
    let decode_into_spec = DecodeIntoSpec {
        data_type: DataType::UChar,
        n_depth: 2,
        n_cols: 3,
        n_rows: 2,
        n_bands: 2,
        n_masks: 1,
    };
    let mut decode_into_data = vec![0; one_sweep_bands_decoded.data_byte_len()];
    let mut decode_into_mask = vec![0; 6];
    let mut ffi_decode_data = vec![0; one_sweep_bands_decoded.data_byte_len()];
    let mut ffi_decode_mask = vec![0; 6];
    let mut ffi_decode_double_data = vec![0.0f64; 24];
    let mut ffi_decode_double_mask = vec![0; 6];
    let one_sweep_no_data_blob = synthetic_v6_uchar_one_sweep_no_data_blob();
    let mut ffi_decode_4d_data = vec![0u8; 12];
    let mut ffi_decode_4d_mask = vec![0; 6];
    let mut ffi_decode_4d_uses_no_data = vec![0; 1];
    let mut ffi_decode_4d_no_data_values = vec![0.0f64; 1];
    let mut ffi_decode_double_4d_data = vec![0.0f64; 12];
    let mut ffi_decode_double_4d_mask = vec![0; 6];
    let mut ffi_decode_double_4d_uses_no_data = vec![0; 1];
    let mut ffi_decode_double_4d_no_data_values = vec![0.0f64; 1];
    let mut ffi_float_fixture_data = vec![0.0f32; 160_000];
    let mut ffi_float_fixture_mask = vec![0; 160_000];
    let mut ffi_float_fixture_double_data = vec![0.0f64; 160_000];
    let mut ffi_float_fixture_double_mask = vec![0; 160_000];
    let tiled_raw_blob = synthetic_v4_tiled_raw_blob();
    let tiled_bitstuff_blob = synthetic_v4_tiled_bitstuff_blob();
    let tiled_lut_blob = synthetic_v4_tiled_lut_blob();
    let tiled_diff_blob = synthetic_v5_tiled_diff_blob();
    let tiled_diff_lut_blob = synthetic_v5_tiled_diff_lut_blob();
    let tiled_float_diff_blob = synthetic_v5_tiled_float_diff_blob();
    let tiled_float_diff_lut_blob = synthetic_v5_tiled_float_diff_lut_blob();
    let encoded_rle = Rle::compress(&byte_data).unwrap();
    let encoded_bits = BitStuffer2::encode_simple(&uint_data, 3).unwrap();
    let encoded_bits_pre_v3 = BitStuffer2::encode_simple(&uint_data, 2).unwrap();
    let bit_mask = BitMask::from_byte_mask(&mask_data, 1000, 1000).unwrap();
    let float_bytes: Vec<u8> = (0..250_000)
        .flat_map(|idx| ((idx as f32) * 0.25).to_le_bytes())
        .collect();

    bench("rle-compress-1mb", 50, || {
        black_box(Rle::compress(black_box(&byte_data)).unwrap());
    });
    bench("rle-decompress-1mb", 50, || {
        black_box(Rle::decompress(black_box(&encoded_rle)).unwrap());
    });
    bench("bit-stuffer-encode-250k", 100, || {
        black_box(BitStuffer2::encode_simple(black_box(&uint_data), 3).unwrap());
    });
    bench("bit-stuffer-decode-250k", 100, || {
        black_box(BitStuffer2::decode(black_box(&encoded_bits), uint_data.len(), 3).unwrap());
    });
    bench("bit-stuffer-encode-pre-v3-250k", 100, || {
        black_box(BitStuffer2::encode_simple(black_box(&uint_data), 2).unwrap());
    });
    bench("bit-stuffer-decode-pre-v3-250k", 100, || {
        black_box(
            BitStuffer2::decode(black_box(&encoded_bits_pre_v3), uint_data.len(), 2).unwrap(),
        );
    });
    bench("bit-mask-from-byte-mask-1mp", 100, || {
        black_box(BitMask::from_byte_mask(black_box(&mask_data), 1000, 1000).unwrap());
    });
    bench("bit-mask-to-byte-mask-1mp", 100, || {
        black_box(black_box(&bit_mask).to_byte_mask());
    });
    bench("bit-mask-count-valid-1mp", 1000, || {
        black_box(black_box(&bit_mask).count_valid_bits());
    });
    bench("typed-float-decode-250k", 1000, || {
        black_box(decode_typed_values(DataType::Float, black_box(&float_bytes)).unwrap());
    });
    bench("lerc1-header-parse", 100_000, || {
        black_box(get_lerc1_header_info(black_box(&lerc1_blob)).unwrap());
    });
    bench("lerc1-count-mask-read", 10_000, || {
        black_box(read_lerc1_count_mask(black_box(&lerc1_blob)).unwrap());
    });
    bench("lerc1-z-stats-read", 1_000, || {
        black_box(read_lerc1_z_stats(black_box(&lerc1_blob)).unwrap());
    });
    bench("lerc2-header-parse", 100_000, || {
        black_box(get_lerc2_header_info(black_box(&lerc2_blob)).unwrap());
    });
    bench("lerc2-info-aggregate-3-band", 50_000, || {
        black_box(get_lerc_info(black_box(&lerc2_blob)).unwrap());
    });
    bench("lerc2-blob-info-arrays-3-band", 50_000, || {
        black_box(
            get_lerc2_blob_info_arrays(
                black_box(&lerc2_blob),
                Some(black_box(&mut blob_info_array)),
                Some(black_box(&mut blob_range_array)),
            )
            .unwrap(),
        );
    });
    bench("lerc2-data-ranges-3-band", 50_000, || {
        black_box(get_lerc2_data_ranges(black_box(&lerc2_blob)).unwrap());
    });
    bench("ffi-get-data-ranges-3-band", 50_000, || {
        black_box(unsafe {
            lerc::ffi::lerc_getDataRanges(
                black_box(lerc2_blob.as_ptr()),
                lerc2_blob.len() as u32,
                1,
                3,
                black_box(data_range_mins.as_mut_ptr()),
                black_box(data_range_maxs.as_mut_ptr()),
            )
        });
    });
    bench("lerc2-mask-read-first-band", 10_000, || {
        black_box(read_lerc2_mask(black_box(&lerc2_blob)).unwrap());
    });
    bench("lerc2-checksum-first-band", 10_000, || {
        black_box(validate_lerc2_checksum(black_box(&lerc2_blob)).unwrap());
    });
    bench("lerc2-min-max-ranges-v4-synthetic", 100_000, || {
        black_box(read_lerc2_min_max_ranges(black_box(&min_max_blob)).unwrap());
    });
    bench("lerc2-data-one-sweep-v4-synthetic", 100_000, || {
        black_box(read_lerc2_data_one_sweep(black_box(&one_sweep_blob)).unwrap());
    });
    bench("lerc2-tiled-raw-v4-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_raw(black_box(&tiled_raw_blob)).unwrap());
    });
    bench("lerc2-tiled-bitstuff-v4-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_payload(black_box(&tiled_bitstuff_blob)).unwrap());
    });
    bench("lerc2-tiled-lut-v4-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_payload(black_box(&tiled_lut_blob)).unwrap());
    });
    bench("lerc2-tiled-diff-v5-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_payload(black_box(&tiled_diff_blob)).unwrap());
    });
    bench("lerc2-tiled-diff-lut-v5-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_payload(black_box(&tiled_diff_lut_blob)).unwrap());
    });
    bench("lerc2-tiled-float-diff-v5-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_payload(black_box(&tiled_float_diff_blob)).unwrap());
    });
    bench("lerc2-tiled-float-diff-lut-v5-synthetic", 100_000, || {
        black_box(read_lerc2_tiled_payload(black_box(&tiled_float_diff_lut_blob)).unwrap());
    });
    bench("lerc2-supported-decode-v4-synthetic", 100_000, || {
        black_box(decode_lerc2_supported(black_box(&one_sweep_blob)).unwrap());
    });
    bench("lerc2-supported-bands-decode-v4-synthetic", 100_000, || {
        black_box(decode_lerc2_bands_supported(black_box(&one_sweep_bands_blob)).unwrap());
    });
    bench(
        "lerc2-supported-no-data-decode-v6-synthetic",
        100_000,
        || {
            black_box(decode_lerc2_supported(black_box(&one_sweep_no_data_blob)).unwrap());
        },
    );
    bench("lerc2-supported-decode-float-fixture", 100, || {
        black_box(decode_lerc2_supported(black_box(&float_fixture_blob)).unwrap());
    });
    bench("lerc2-supported-write-data-bytes", 100_000, || {
        black_box(
            one_sweep_bands_decoded
                .write_data_le_bytes(black_box(&mut decoded_data_bytes))
                .unwrap(),
        );
    });
    bench("lerc2-supported-write-mask-bytes", 100_000, || {
        black_box(
            one_sweep_bands_decoded
                .write_mask_bytes(black_box(&mut decoded_mask_bytes))
                .unwrap(),
        );
    });
    bench("lerc2-supported-decode-into-v4-synthetic", 100_000, || {
        black_box(
            decode_lerc2_supported_into(
                black_box(&one_sweep_bands_blob),
                decode_into_spec,
                black_box(&mut decode_into_data),
                Some(black_box(&mut decode_into_mask)),
            )
            .unwrap(),
        );
    });
    bench("ffi-decode-v4-synthetic", 100_000, || {
        black_box(unsafe {
            lerc::ffi::lerc_decode(
                black_box(one_sweep_bands_blob.as_ptr()),
                one_sweep_bands_blob.len() as u32,
                1,
                black_box(ffi_decode_mask.as_mut_ptr()),
                2,
                3,
                2,
                2,
                DataType::UChar as u32,
                black_box(ffi_decode_data.as_mut_ptr().cast()),
            )
        });
    });
    bench("ffi-decode-to-double-v4-synthetic", 100_000, || {
        black_box(unsafe {
            lerc::ffi::lerc_decodeToDouble(
                black_box(one_sweep_bands_blob.as_ptr()),
                one_sweep_bands_blob.len() as u32,
                1,
                black_box(ffi_decode_double_mask.as_mut_ptr()),
                2,
                3,
                2,
                2,
                black_box(ffi_decode_double_data.as_mut_ptr()),
            )
        });
    });
    bench("ffi-decode-float-fixture", 100, || {
        black_box(unsafe {
            lerc::ffi::lerc_decode(
                black_box(float_fixture_blob.as_ptr()),
                float_fixture_blob.len() as u32,
                1,
                black_box(ffi_float_fixture_mask.as_mut_ptr()),
                1,
                400,
                400,
                1,
                DataType::Float as u32,
                black_box(ffi_float_fixture_data.as_mut_ptr().cast()),
            )
        });
    });
    bench("ffi-decode-to-double-float-fixture", 100, || {
        black_box(unsafe {
            lerc::ffi::lerc_decodeToDouble(
                black_box(float_fixture_blob.as_ptr()),
                float_fixture_blob.len() as u32,
                1,
                black_box(ffi_float_fixture_double_mask.as_mut_ptr()),
                1,
                400,
                400,
                1,
                black_box(ffi_float_fixture_double_data.as_mut_ptr()),
            )
        });
    });
    bench("ffi-decode-4d-v6-no-data-synthetic", 100_000, || {
        black_box(unsafe {
            lerc::ffi::lerc_decode_4D(
                black_box(one_sweep_no_data_blob.as_ptr()),
                one_sweep_no_data_blob.len() as u32,
                1,
                black_box(ffi_decode_4d_mask.as_mut_ptr()),
                2,
                3,
                2,
                1,
                DataType::UChar as u32,
                black_box(ffi_decode_4d_data.as_mut_ptr().cast()),
                black_box(ffi_decode_4d_uses_no_data.as_mut_ptr()),
                black_box(ffi_decode_4d_no_data_values.as_mut_ptr()),
            )
        });
    });
    bench(
        "ffi-decode-to-double-4d-v6-no-data-synthetic",
        100_000,
        || {
            black_box(unsafe {
                lerc::ffi::lerc_decodeToDouble_4D(
                    black_box(one_sweep_no_data_blob.as_ptr()),
                    one_sweep_no_data_blob.len() as u32,
                    1,
                    black_box(ffi_decode_double_4d_mask.as_mut_ptr()),
                    2,
                    3,
                    2,
                    1,
                    black_box(ffi_decode_double_4d_data.as_mut_ptr()),
                    black_box(ffi_decode_double_4d_uses_no_data.as_mut_ptr()),
                    black_box(ffi_decode_double_4d_no_data_values.as_mut_ptr()),
                )
            });
        },
    );

    std::thread::sleep(Duration::from_millis(1));
}

fn synthetic_v4_tiled_raw_blob() -> Vec<u8> {
    let valid = [1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1];
    let mask = BitMask::from_byte_mask(&valid, 5, 3).unwrap();
    let encoded_mask = Rle::compress(mask.bits()).unwrap();
    let range_bytes = [10u8, 20];
    let tile_payloads: [&[u8]; 6] = [&[10, 11], &[12, 13, 14], &[15, 16], &[17, 18], &[19], &[20]];
    let tile_bytes_len: usize = tile_payloads.iter().map(|payload| 1 + payload.len()).sum();
    let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
    let blob_size = header_size + 4 + encoded_mask.len() + range_bytes.len() + 1 + tile_bytes_len;
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&4i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [
        3,
        5,
        1,
        valid.iter().filter(|&&value| value != 0).count() as i32,
        2,
        blob_size as i32,
        DataType::UChar as i32,
    ] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&0.5f64.to_le_bytes());
    blob.extend_from_slice(&10.0f64.to_le_bytes());
    blob.extend_from_slice(&20.0f64.to_le_bytes());
    blob.extend_from_slice(&(encoded_mask.len() as i32).to_le_bytes());
    blob.extend_from_slice(&encoded_mask);
    blob.extend_from_slice(&range_bytes);
    blob.push(0);
    for payload in tile_payloads {
        blob.push(0);
        blob.extend_from_slice(payload);
    }
    set_lerc2_checksum(&mut blob);
    blob
}

fn synthetic_v4_tiled_bitstuff_blob() -> Vec<u8> {
    let valid = [1; 15];
    let range_bytes = [10u8, 24];
    let mut const_block = vec![3, 24];
    let blocks = vec![
        bit_stuffed_tile_block(10, &[0, 1, 5, 6]),
        bit_stuffed_tile_block(10, &[2, 3, 7, 8]),
        bit_stuffed_tile_block(10, &[4, 9]),
        bit_stuffed_tile_block(10, &[10, 11]),
        bit_stuffed_tile_block(10, &[12, 13]),
        std::mem::take(&mut const_block),
    ];
    synthetic_v4_tiled_blocks(DataType::UChar, 1, &valid, &range_bytes, &blocks)
}

fn synthetic_v4_tiled_lut_blob() -> Vec<u8> {
    let valid = [1; 15];
    let range_bytes = [10u8, 24];
    let blocks = vec![
        lut_tile_block(10, &[0, 1, 5, 6]),
        lut_tile_block(12, &[0, 1, 5, 6]),
        lut_tile_block(14, &[0, 5]),
        lut_tile_block(20, &[0, 1]),
        lut_tile_block(22, &[0, 1]),
        vec![3, 24],
    ];
    synthetic_v4_tiled_blocks(DataType::UChar, 1, &valid, &range_bytes, &blocks)
}

fn synthetic_v5_tiled_diff_blob() -> Vec<u8> {
    synthetic_v5_tiled_diff_blob_with_block({
        let mut block = vec![(2 << 6) | 4 | 1];
        block.extend_from_slice(&5i16.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_simple(&[0, 1, 2, 3], 5).unwrap());
        block
    })
}

fn synthetic_v5_tiled_diff_lut_blob() -> Vec<u8> {
    let mut sorted: Vec<(u32, u32)> = [0, 1, 2, 3]
        .iter()
        .enumerate()
        .map(|(idx, &value)| (value, idx as u32))
        .collect();
    sorted.sort_unstable();
    synthetic_v5_tiled_diff_blob_with_block({
        let mut block = vec![(2 << 6) | 4 | 1];
        block.extend_from_slice(&5i16.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_lut(&sorted, 5).unwrap());
        block
    })
}

fn synthetic_v5_tiled_diff_blob_with_block(depth1_block: Vec<u8>) -> Vec<u8> {
    let range_bytes = [10u8, 15, 40, 48];
    let depth0_block = {
        let mut block = vec![0];
        block.extend_from_slice(&[10, 20, 30, 40]);
        block
    };
    let blocks = [depth0_block, depth1_block];
    let tile_bytes_len: usize = blocks.iter().map(Vec::len).sum();
    let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
    let blob_size = header_size + 4 + range_bytes.len() + 1 + tile_bytes_len;
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&5i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [2, 2, 2, 4, 2, blob_size as i32, DataType::UChar as i32] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&0.5f64.to_le_bytes());
    blob.extend_from_slice(&10.0f64.to_le_bytes());
    blob.extend_from_slice(&48.0f64.to_le_bytes());
    blob.extend_from_slice(&0i32.to_le_bytes());
    blob.extend_from_slice(&range_bytes);
    blob.push(0);
    for block in blocks {
        blob.extend_from_slice(&block);
    }
    set_lerc2_checksum(&mut blob);
    blob
}

fn synthetic_v5_tiled_float_diff_blob() -> Vec<u8> {
    synthetic_v5_tiled_float_diff_blob_with_block({
        let mut block = vec![4 | 1];
        block.extend_from_slice(&1.5f32.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_simple(&[0, 1, 2, 3], 5).unwrap());
        block
    })
}

fn synthetic_v5_tiled_float_diff_lut_blob() -> Vec<u8> {
    let mut sorted: Vec<(u32, u32)> = [0, 1, 2, 3]
        .iter()
        .enumerate()
        .map(|(idx, &value)| (value, idx as u32))
        .collect();
    sorted.sort_unstable();
    synthetic_v5_tiled_float_diff_blob_with_block({
        let mut block = vec![4 | 1];
        block.extend_from_slice(&1.5f32.to_le_bytes());
        block.extend_from_slice(&BitStuffer2::encode_lut(&sorted, 5).unwrap());
        block
    })
}

fn synthetic_v5_tiled_float_diff_blob_with_block(depth1_block: Vec<u8>) -> Vec<u8> {
    let mut range_bytes = Vec::new();
    for value in [10.0f32, 11.5, 40.0, 42.0] {
        range_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let depth0_block = {
        let mut block = vec![0];
        for value in [10.0f32, 20.0, 30.0, 40.0] {
            block.extend_from_slice(&value.to_le_bytes());
        }
        block
    };
    let blocks = [depth0_block, depth1_block];
    let tile_bytes_len: usize = blocks.iter().map(Vec::len).sum();
    let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
    let blob_size = header_size + 4 + range_bytes.len() + 1 + tile_bytes_len;
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&5i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [2, 2, 2, 4, 2, blob_size as i32, DataType::Float as i32] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&0.25f64.to_le_bytes());
    blob.extend_from_slice(&10.0f64.to_le_bytes());
    blob.extend_from_slice(&42.0f64.to_le_bytes());
    blob.extend_from_slice(&0i32.to_le_bytes());
    blob.extend_from_slice(&range_bytes);
    blob.push(0);
    for block in blocks {
        blob.extend_from_slice(&block);
    }
    set_lerc2_checksum(&mut blob);
    blob
}

fn bit_stuffed_tile_block(offset: u8, quantized: &[u32]) -> Vec<u8> {
    let mut block = vec![1, offset];
    block.extend_from_slice(&BitStuffer2::encode_simple(quantized, 4).unwrap());
    block
}

fn lut_tile_block(offset: u8, quantized: &[u32]) -> Vec<u8> {
    let mut sorted: Vec<(u32, u32)> = quantized
        .iter()
        .enumerate()
        .map(|(idx, &value)| (value, idx as u32))
        .collect();
    sorted.sort_unstable();

    let mut block = vec![1, offset];
    block.extend_from_slice(&BitStuffer2::encode_lut(&sorted, 4).unwrap());
    block
}

fn synthetic_v4_tiled_blocks(
    data_type: DataType,
    n_depth: i32,
    valid: &[u8],
    range_bytes: &[u8],
    blocks: &[Vec<u8>],
) -> Vec<u8> {
    let tile_bytes_len: usize = blocks.iter().map(Vec::len).sum();
    let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
    let blob_size = header_size + 4 + range_bytes.len() + 1 + tile_bytes_len;
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&4i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [
        3,
        5,
        n_depth,
        valid.iter().filter(|&&value| value != 0).count() as i32,
        2,
        blob_size as i32,
        data_type as i32,
    ] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&0.5f64.to_le_bytes());
    blob.extend_from_slice(&10.0f64.to_le_bytes());
    blob.extend_from_slice(&24.0f64.to_le_bytes());
    blob.extend_from_slice(&0i32.to_le_bytes());
    blob.extend_from_slice(range_bytes);
    blob.push(0);
    for block in blocks {
        blob.extend_from_slice(block);
    }
    set_lerc2_checksum(&mut blob);
    blob
}

fn synthetic_v4_one_sweep_blob() -> Vec<u8> {
    let n_depth = 2i32;
    let valid = [1, 0, 1, 1, 0, 1];
    let mask = BitMask::from_byte_mask(&valid, 3, 2).unwrap();
    let encoded_mask = Rle::compress(mask.bits()).unwrap();
    let num_valid = valid.iter().filter(|&&value| value != 0).count();
    let range_bytes = [1u8, 2, 10, 20];
    let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
    let blob_size = header_size + 4 + encoded_mask.len() + range_bytes.len() + 1 + payload.len();
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&4i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [
        2,
        3,
        n_depth,
        num_valid as i32,
        8,
        blob_size as i32,
        DataType::UChar as i32,
    ] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&0.5f64.to_le_bytes());
    blob.extend_from_slice(&1.0f64.to_le_bytes());
    blob.extend_from_slice(&20.0f64.to_le_bytes());
    blob.extend_from_slice(&(encoded_mask.len() as i32).to_le_bytes());
    blob.extend_from_slice(&encoded_mask);
    blob.extend_from_slice(&range_bytes);
    blob.push(1);
    blob.extend_from_slice(&payload);
    set_lerc2_checksum(&mut blob);
    blob
}

fn synthetic_v4_min_max_blob() -> Vec<u8> {
    let n_depth = 4i32;
    let mut range_bytes = Vec::new();
    for value in [-1.0f32, 0.0, 2.5, 9.0, 10.0, 20.0, 30.0, 40.0] {
        range_bytes.extend_from_slice(&value.to_le_bytes());
    }

    let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
    let blob_size = header_size + 4 + range_bytes.len();
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&4i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [
        2,
        3,
        n_depth,
        6,
        8,
        blob_size as i32,
        DataType::Float as i32,
    ] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&0.5f64.to_le_bytes());
    blob.extend_from_slice(&(-1.0f64).to_le_bytes());
    blob.extend_from_slice(&40.0f64.to_le_bytes());
    blob.extend_from_slice(&0i32.to_le_bytes());
    blob.extend_from_slice(&range_bytes);
    set_lerc2_checksum(&mut blob);
    blob
}

fn synthetic_v6_uchar_one_sweep_no_data_blob() -> Vec<u8> {
    let n_rows = 2i32;
    let n_cols = 3i32;
    let n_depth = 2i32;
    let num_valid = n_rows * n_cols;
    let range_bytes = [1u8, 1, 99, 99];
    let payload = [1u8, 99, 2, 3, 99, 4, 5, 6, 7, 99, 8, 9];
    let header_size = 6 + 4 + 4 + 8 * 4 + 4 + 5 * 8;
    let blob_size = header_size + 4 + range_bytes.len() + 1 + payload.len();
    let mut blob = Vec::with_capacity(blob_size);
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&6i32.to_le_bytes());
    blob.extend_from_slice(&0u32.to_le_bytes());
    for value in [
        n_rows,
        n_cols,
        n_depth,
        num_valid,
        8,
        blob_size as i32,
        DataType::UChar as i32,
        0,
    ] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&[1, 1, 0, 0]);
    blob.extend_from_slice(&0.5f64.to_le_bytes());
    blob.extend_from_slice(&1.0f64.to_le_bytes());
    blob.extend_from_slice(&99.0f64.to_le_bytes());
    blob.extend_from_slice(&99.0f64.to_le_bytes());
    blob.extend_from_slice(&255.0f64.to_le_bytes());
    blob.extend_from_slice(&0i32.to_le_bytes());
    blob.extend_from_slice(&range_bytes);
    blob.push(1);
    blob.extend_from_slice(&payload);
    set_lerc2_checksum(&mut blob);
    blob
}

fn set_lerc2_checksum(blob: &mut [u8]) {
    let checksum = compute_checksum_fletcher32(&blob[14..]);
    blob[10..14].copy_from_slice(&checksum.to_le_bytes());
}
