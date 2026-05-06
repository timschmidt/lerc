use std::hint::black_box;
use std::time::{Duration, Instant};

use lerc::{BitMask, BitStuffer2, Rle};

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

    std::thread::sleep(Duration::from_millis(1));
}
