/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Run-length encoder used by LERC mask sections.

use crate::types::{LercError, Result};

const MIN_NUM_EVEN: usize = 5;
const EOF_COUNT: i16 = i16::MIN;
const MAX_COUNT: usize = i16::MAX as usize;

/// C++-compatible LERC byte run-length codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct Rle;

impl Rle {
    /// Computes the compressed byte count for `input`.
    pub fn compute_num_bytes(input: &[u8]) -> usize {
        if input.is_empty() {
            return 0;
        }

        let mut sum = 0usize;
        let mut cnt_odd = 0usize;
        let mut cnt_even = 0usize;
        let mut cnt_total = 0usize;
        let mut odd = true;

        while cnt_total < input.len() - 1 {
            let b = input[cnt_total];
            let next = input[cnt_total + 1];
            if b != next {
                if odd {
                    cnt_odd += 1;
                } else {
                    sum += 3;
                    odd = true;
                    cnt_odd = 0;
                    cnt_even = 0;
                }
            } else if !odd {
                cnt_even += 1;
            } else {
                let found_enough = cnt_total + MIN_NUM_EVEN < input.len()
                    && input[cnt_total + 1..cnt_total + MIN_NUM_EVEN]
                        .iter()
                        .all(|&candidate| candidate == b);

                if found_enough {
                    if cnt_odd > 0 {
                        sum += 2 + cnt_odd;
                    }
                    odd = false;
                    cnt_odd = 0;
                    cnt_even = 1;
                } else {
                    cnt_odd += 1;
                }
            }

            cnt_total += 1;

            if cnt_odd == MAX_COUNT {
                sum += 2 + MAX_COUNT;
                cnt_odd = 0;
            }
            if cnt_even == MAX_COUNT {
                sum += 3;
                cnt_even = 0;
            }
        }

        if odd {
            cnt_odd += 1;
            sum += 2 + cnt_odd;
        } else {
            sum += 3;
        }

        sum + 2
    }

    /// Compresses bytes using the LERC RLE stream format.
    pub fn compress(input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Err(LercError::WrongParam("input must not be empty"));
        }

        let mut out = Vec::with_capacity(Self::compute_num_bytes(input));
        out.extend_from_slice(&[0, 0]);
        let mut cnt_pos = 0usize;
        let mut cnt_odd = 0usize;
        let mut cnt_even = 0usize;
        let mut cnt_total = 0usize;
        let mut odd = true;

        while cnt_total < input.len() - 1 {
            let b = input[cnt_total];
            let next = input[cnt_total + 1];

            if b != next {
                out.push(b);
                if odd {
                    cnt_odd += 1;
                } else {
                    cnt_even += 1;
                    write_count(-(cnt_even as i16), &mut out, &mut cnt_pos);
                    odd = true;
                    cnt_odd = 0;
                    cnt_even = 0;
                }
            } else if !odd {
                cnt_even += 1;
            } else {
                let found_enough = cnt_total + MIN_NUM_EVEN < input.len()
                    && input[cnt_total + 1..cnt_total + MIN_NUM_EVEN]
                        .iter()
                        .all(|&candidate| candidate == b);

                if found_enough {
                    if cnt_odd > 0 {
                        write_count(cnt_odd as i16, &mut out, &mut cnt_pos);
                    }
                    odd = false;
                    cnt_odd = 0;
                    cnt_even = 1;
                } else {
                    out.push(b);
                    cnt_odd += 1;
                }
            }

            if cnt_odd == MAX_COUNT {
                write_count(cnt_odd as i16, &mut out, &mut cnt_pos);
                cnt_odd = 0;
            }
            if cnt_even == MAX_COUNT {
                out.push(b);
                write_count(-(cnt_even as i16), &mut out, &mut cnt_pos);
                cnt_even = 0;
            }

            cnt_total += 1;
        }

        out.push(input[input.len() - 1]);
        if odd {
            cnt_odd += 1;
            write_count(cnt_odd as i16, &mut out, &mut cnt_pos);
        } else {
            cnt_even += 1;
            write_count(-(cnt_even as i16), &mut out, &mut cnt_pos);
        }

        write_count(EOF_COUNT, &mut out, &mut cnt_pos);
        out.truncate(cnt_pos);
        debug_assert_eq!(out.len(), Self::compute_num_bytes(input));
        Ok(out)
    }

    /// Decompresses a LERC RLE stream into a newly allocated buffer.
    pub fn decompress(encoded: &[u8]) -> Result<Vec<u8>> {
        let output_len = Self::decompressed_len(encoded)?;
        let mut out = vec![0; output_len];
        Self::decompress_into(encoded, &mut out)?;
        Ok(out)
    }

    /// Computes the decompressed byte count without producing output.
    pub fn decompressed_len(encoded: &[u8]) -> Result<usize> {
        if encoded.len() < 2 {
            return Err(LercError::BufferTooSmall);
        }

        let mut pos = 0usize;
        let mut remaining = encoded.len() - 2;
        let mut sum = 0usize;
        let mut cnt = read_count(encoded, &mut pos)?;

        while cnt != EOF_COUNT {
            let count_abs = cnt.unsigned_abs() as usize;
            let stored_bytes = if cnt > 0 { count_abs } else { 1 };
            if remaining < stored_bytes + 2 {
                return Err(LercError::CorruptInput("RLE count exceeds input"));
            }
            sum = sum
                .checked_add(count_abs)
                .ok_or(LercError::CorruptInput("RLE decoded length overflow"))?;
            pos += stored_bytes;
            remaining -= stored_bytes + 2;
            cnt = read_count(encoded, &mut pos)?;
        }

        if sum == 0 {
            return Err(LercError::CorruptInput("RLE decoded length is zero"));
        }
        Ok(sum)
    }

    /// Decompresses a LERC RLE stream into `out`.
    pub fn decompress_into(encoded: &[u8], out: &mut [u8]) -> Result<()> {
        if encoded.len() < 2 {
            return Err(LercError::BufferTooSmall);
        }

        let mut pos = 0usize;
        let mut remaining = encoded.len() - 2;
        let mut out_pos = 0usize;
        let mut cnt = read_count(encoded, &mut pos)?;

        while cnt != EOF_COUNT {
            let count_abs = cnt.unsigned_abs() as usize;
            let stored_bytes = if cnt > 0 { count_abs } else { 1 };
            if remaining < stored_bytes + 2 || out_pos + count_abs > out.len() {
                return Err(LercError::CorruptInput("RLE stream exceeds bounds"));
            }

            if cnt > 0 {
                out[out_pos..out_pos + count_abs].copy_from_slice(&encoded[pos..pos + count_abs]);
                pos += count_abs;
            } else {
                out[out_pos..out_pos + count_abs].fill(encoded[pos]);
                pos += 1;
            }

            out_pos += count_abs;
            remaining -= stored_bytes + 2;
            cnt = read_count(encoded, &mut pos)?;
        }

        if out_pos != out.len() {
            return Err(LercError::CorruptInput("RLE decoded length mismatch"));
        }
        Ok(())
    }
}

fn write_count(cnt: i16, out: &mut Vec<u8>, cnt_pos: &mut usize) {
    let bytes = cnt.to_le_bytes();
    out[*cnt_pos..*cnt_pos + 2].copy_from_slice(&bytes);
    *cnt_pos = out.len();
    out.extend_from_slice(&[0, 0]);
}

fn read_count(encoded: &[u8], pos: &mut usize) -> Result<i16> {
    if encoded.len().saturating_sub(*pos) < 2 {
        return Err(LercError::BufferTooSmall);
    }
    let value = i16::from_le_bytes([encoded[*pos], encoded[*pos + 1]]);
    *pos += 2;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::Rle;

    fn round_trip(input: &[u8]) {
        let compressed = Rle::compress(input).unwrap();
        assert_eq!(compressed.len(), Rle::compute_num_bytes(input));
        assert_eq!(Rle::decompress(&compressed).unwrap(), input);
    }

    #[test]
    fn rejects_empty_input() {
        assert!(Rle::compress(&[]).is_err());
        assert_eq!(Rle::compute_num_bytes(&[]), 0);
    }

    #[test]
    fn round_trips_literal_runs_and_repeated_runs() {
        round_trip(&[1]);
        round_trip(&[1, 2, 3, 4, 5, 6]);
        round_trip(&[7, 7, 7, 7, 7, 7, 7, 8, 9]);
        round_trip(&[1, 2, 2, 2, 2, 2, 3, 4, 4, 4, 4, 4, 5]);
    }

    #[test]
    fn matches_known_literal_run_stream() {
        let input = [1, 2, 3, 4];
        let encoded = [
            4, 0, // literal count
            1, 2, 3, 4, // literal bytes
            0, 128, // EOF count
        ];

        assert_eq!(Rle::compute_num_bytes(&input), encoded.len());
        assert_eq!(Rle::compress(&input).unwrap(), encoded);
        assert_eq!(Rle::decompress(&encoded).unwrap(), input);
    }

    #[test]
    fn matches_known_repeated_run_stream() {
        let input = [9, 9, 9, 9, 9, 9];
        let encoded = [
            250, 255, // repeated count: -6i16
            9,   // repeated byte
            0, 128, // EOF count
        ];

        assert_eq!(Rle::compute_num_bytes(&input), encoded.len());
        assert_eq!(Rle::compress(&input).unwrap(), encoded);
        assert_eq!(Rle::decompress(&encoded).unwrap(), input);
    }

    #[test]
    fn round_trips_counter_boundaries() {
        let mut input = vec![42; 32770];
        input.extend((0..512).map(|i| (i & 0xff) as u8));
        round_trip(&input);
    }

    #[test]
    fn detects_truncated_stream() {
        let compressed = Rle::compress(&[3, 3, 3, 3, 3, 4]).unwrap();
        assert!(Rle::decompress(&compressed[..compressed.len() - 1]).is_err());
    }

    #[test]
    fn rejects_adversarial_count_streams() {
        let missing_eof = [
            2, 0, // literal count
            1, 2, // literal bytes, but no EOF marker
        ];
        assert!(Rle::decompressed_len(&missing_eof).is_err());
        assert!(Rle::decompress(&missing_eof).is_err());

        let count_exceeds_input = [
            5, 0, // claims five literal bytes
            1, 2, // only two literal bytes before EOF-shaped bytes
            0, 128,
        ];
        assert!(Rle::decompressed_len(&count_exceeds_input).is_err());
        assert!(Rle::decompress(&count_exceeds_input).is_err());

        let zero_length_stream = [0, 128];
        assert!(Rle::decompressed_len(&zero_length_stream).is_err());
        assert!(Rle::decompress(&zero_length_stream).is_err());

        let encoded = Rle::compress(&[1, 2, 3, 4]).unwrap();
        let mut too_short = [0u8; 3];
        assert!(Rle::decompress_into(&encoded, &mut too_short).is_err());
    }
}
