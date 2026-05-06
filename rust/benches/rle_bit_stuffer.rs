use std::hint::black_box;
use std::time::{Duration, Instant};

use lerc::{
    get_lerc2_header_info, get_lerc_info, read_lerc2_data_one_sweep, read_lerc2_mask,
    read_lerc2_min_max_ranges, read_lerc2_tiled_payload, read_lerc2_tiled_raw,
    validate_lerc2_checksum, BitMask, BitStuffer2, DataType, Rle,
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
    let min_max_blob = synthetic_v4_min_max_blob();
    let one_sweep_blob = synthetic_v4_one_sweep_blob();
    let tiled_raw_blob = synthetic_v4_tiled_raw_blob();
    let tiled_bitstuff_blob = synthetic_v4_tiled_bitstuff_blob();
    let encoded_rle = Rle::compress(&byte_data).unwrap();
    let encoded_bits = BitStuffer2::encode_simple(&uint_data, 3).unwrap();
    let bit_mask = BitMask::from_byte_mask(&mask_data, 1000, 1000).unwrap();

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
    bench("bit-mask-from-byte-mask-1mp", 100, || {
        black_box(BitMask::from_byte_mask(black_box(&mask_data), 1000, 1000).unwrap());
    });
    bench("bit-mask-to-byte-mask-1mp", 100, || {
        black_box(black_box(&bit_mask).to_byte_mask());
    });
    bench("bit-mask-count-valid-1mp", 1000, || {
        black_box(black_box(&bit_mask).count_valid_bits());
    });
    bench("lerc2-header-parse", 100_000, || {
        black_box(get_lerc2_header_info(black_box(&lerc2_blob)).unwrap());
    });
    bench("lerc2-info-aggregate-3-band", 50_000, || {
        black_box(get_lerc_info(black_box(&lerc2_blob)).unwrap());
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

fn bit_stuffed_tile_block(offset: u8, quantized: &[u32]) -> Vec<u8> {
    let mut block = vec![1, offset];
    block.extend_from_slice(&BitStuffer2::encode_simple(quantized, 4).unwrap());
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
    blob
}
