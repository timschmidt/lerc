/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

use crate::types::{LercError, Result};

#[derive(Debug, Default, Clone, Copy)]
pub struct BitStuffer2;

impl BitStuffer2 {
    pub fn compute_num_bytes_needed_simple(num_elem: u32, max_elem: u32) -> u32 {
        let num_bits = num_bits(max_elem);
        1 + num_bytes_uint(num_elem) as u32
            + (((num_elem as u64 * num_bits as u64 + 7) >> 3) as u32)
    }

    pub fn compute_num_bytes_needed_lut(sorted_data: &[(u32, u32)]) -> Result<(u32, bool)> {
        if sorted_data.is_empty() {
            return Err(LercError::WrongParam("sorted_data must not be empty"));
        }
        let max_elem = sorted_data[sorted_data.len() - 1].0;
        let elem_count = sorted_data.len() as u32;
        let bits = num_bits(max_elem);
        let simple_bytes = 1
            + num_bytes_uint(elem_count) as u32
            + (((elem_count as u64 * bits as u64 + 7) >> 3) as u32);

        let mut lut_len = 0u32;
        for pair in sorted_data.windows(2) {
            if pair[0].0 != pair[1].0 {
                lut_len += 1;
            }
        }

        let lut_bits = num_bits(lut_len);
        let lut_bytes = 1
            + num_bytes_uint(elem_count) as u32
            + 1
            + (((lut_len as u64 * bits as u64 + 7) >> 3) as u32)
            + (((elem_count as u64 * lut_bits as u64 + 7) >> 3) as u32);

        Ok((simple_bytes.min(lut_bytes), lut_bytes < simple_bytes))
    }

    pub fn encode_simple(data: &[u32], lerc2_version: i32) -> Result<Vec<u8>> {
        if data.is_empty() {
            return Err(LercError::WrongParam("data must not be empty"));
        }
        if lerc2_version < 3 {
            return Err(LercError::Unsupported(
                "pre-v2.3 bit stuffing is not ported yet",
            ));
        }

        let max_elem = data.iter().copied().max().unwrap();
        let bits = num_bits(max_elem);
        if bits >= 32 {
            return Err(LercError::WrongParam(
                "values needing 32 bits are not supported",
            ));
        }

        let elem_count = data.len() as u32;
        let elem_count_bytes = num_bytes_uint(elem_count);
        let bits67 = if elem_count_bytes == 4 {
            0
        } else {
            3 - elem_count_bytes
        };
        let header = bits | ((bits67 as u8) << 6);

        let mut out = Vec::with_capacity(
            Self::compute_num_bytes_needed_simple(elem_count, max_elem) as usize,
        );
        out.push(header);
        encode_uint(elem_count, elem_count_bytes, &mut out)?;
        if bits > 0 {
            bit_stuff(data, bits, &mut out);
        }
        Ok(out)
    }

    pub fn encode_lut(sorted_data: &[(u32, u32)], lerc2_version: i32) -> Result<Vec<u8>> {
        if sorted_data.is_empty() {
            return Err(LercError::WrongParam("sorted_data must not be empty"));
        }
        if sorted_data[0].0 != 0 {
            return Err(LercError::WrongParam("first sorted value must be zero"));
        }
        if lerc2_version < 3 {
            return Err(LercError::Unsupported(
                "pre-v2.3 bit stuffing is not ported yet",
            ));
        }

        let elem_count = sorted_data.len();
        let mut lut = Vec::new();
        let mut indexes = vec![0u32; elem_count];
        let mut index_lut = 0u32;

        for i in 1..elem_count {
            let prev = sorted_data[i - 1].0;
            indexes[sorted_data[i - 1].1 as usize] = index_lut;
            if sorted_data[i].0 != prev {
                lut.push(sorted_data[i].0);
                index_lut += 1;
            }
        }
        indexes[sorted_data[elem_count - 1].1 as usize] = index_lut;

        let max_elem = *lut
            .last()
            .ok_or(LercError::WrongParam("LUT must contain non-zero values"))?;
        let bits = num_bits(max_elem);
        if bits == 0 || bits >= 32 || lut.len() >= 255 {
            return Err(LercError::WrongParam("invalid LUT bit width or length"));
        }

        let elem_count_u32 = elem_count as u32;
        let elem_count_bytes = num_bytes_uint(elem_count_u32);
        let bits67 = if elem_count_bytes == 4 {
            0
        } else {
            3 - elem_count_bytes
        };
        let header = bits | ((bits67 as u8) << 6) | (1 << 5);

        let mut out = Vec::new();
        out.push(header);
        encode_uint(elem_count_u32, elem_count_bytes, &mut out)?;
        out.push((lut.len() + 1) as u8);
        bit_stuff(&lut, bits, &mut out);

        let lut_index_bits = num_bits(lut.len() as u32);
        if lut_index_bits == 0 {
            return Err(LercError::WrongParam("invalid LUT index bit width"));
        }
        bit_stuff(&indexes, lut_index_bits, &mut out);
        Ok(out)
    }

    pub fn decode(
        encoded: &[u8],
        max_element_count: usize,
        lerc2_version: i32,
    ) -> Result<(Vec<u32>, usize)> {
        if encoded.is_empty() {
            return Err(LercError::BufferTooSmall);
        }
        if lerc2_version < 3 {
            return Err(LercError::Unsupported(
                "pre-v2.3 bit unstuffing is not ported yet",
            ));
        }

        let mut pos = 0usize;
        let header = encoded[pos];
        pos += 1;

        let bits67 = header >> 6;
        let elem_count_bytes = if bits67 == 0 { 4 } else { 3 - bits67 };
        let do_lut = (header & (1 << 5)) != 0;
        let bits = header & 31;

        let elem_count = decode_uint(encoded, &mut pos, elem_count_bytes as usize)? as usize;
        if elem_count > max_element_count {
            return Err(LercError::CorruptInput("element count exceeds limit"));
        }

        if !do_lut {
            if bits == 0 {
                return Ok((vec![0; elem_count], pos));
            }
            let data = bit_unstuff(encoded, &mut pos, elem_count, bits)?;
            return Ok((data, pos));
        }

        if bits == 0 || pos >= encoded.len() {
            return Err(LercError::CorruptInput("invalid LUT header"));
        }
        let lut_len = encoded[pos].wrapping_sub(1) as usize;
        pos += 1;

        let mut lut = bit_unstuff(encoded, &mut pos, lut_len, bits)?;
        lut.insert(0, 0);

        let lut_index_bits = num_bits(lut_len as u32);
        if lut_index_bits == 0 {
            return Err(LercError::CorruptInput("invalid LUT index bit width"));
        }
        let mut indexes = bit_unstuff(encoded, &mut pos, elem_count, lut_index_bits)?;
        for idx in &mut indexes {
            let lut_idx = *idx as usize;
            *idx = *lut
                .get(lut_idx)
                .ok_or(LercError::CorruptInput("LUT index out of bounds"))?;
        }

        Ok((indexes, pos))
    }
}

fn num_bits(max_elem: u32) -> u8 {
    if max_elem == 0 {
        0
    } else {
        (u32::BITS - max_elem.leading_zeros()) as u8
    }
}

fn num_bytes_uint(value: u32) -> usize {
    if value < 256 {
        1
    } else if value < (1 << 16) {
        2
    } else {
        4
    }
}

fn encode_uint(value: u32, num_bytes: usize, out: &mut Vec<u8>) -> Result<()> {
    match num_bytes {
        1 => out.push(value as u8),
        2 => out.extend_from_slice(&(value as u16).to_le_bytes()),
        4 => out.extend_from_slice(&value.to_le_bytes()),
        _ => {
            return Err(LercError::WrongParam(
                "integer width must be 1, 2, or 4 bytes",
            ))
        }
    }
    Ok(())
}

fn decode_uint(encoded: &[u8], pos: &mut usize, num_bytes: usize) -> Result<u32> {
    if encoded.len().saturating_sub(*pos) < num_bytes {
        return Err(LercError::BufferTooSmall);
    }

    let value = match num_bytes {
        1 => encoded[*pos] as u32,
        2 => u16::from_le_bytes([encoded[*pos], encoded[*pos + 1]]) as u32,
        4 => u32::from_le_bytes([
            encoded[*pos],
            encoded[*pos + 1],
            encoded[*pos + 2],
            encoded[*pos + 3],
        ]),
        _ => {
            return Err(LercError::CorruptInput(
                "integer width must be 1, 2, or 4 bytes",
            ))
        }
    };
    *pos += num_bytes;
    Ok(value)
}

fn num_tail_bytes_not_needed(num_elem: usize, bits: u8) -> usize {
    let tail_bits = (num_elem as u64 * bits as u64) & 31;
    let tail_bytes = (tail_bits + 7) >> 3;
    if tail_bytes > 0 {
        4 - tail_bytes as usize
    } else {
        0
    }
}

fn bit_stuff(data: &[u32], bits: u8, out: &mut Vec<u8>) {
    let num_uints = (data.len() * bits as usize + 31) / 32;
    let num_bytes = num_uints * 4;
    let mut words = vec![0u32; num_uints];
    let mut word_idx = 0usize;
    let mut bit_pos = 0i32;

    for &value in data {
        let bits_i32 = bits as i32;
        if 32 - bit_pos >= bits_i32 {
            words[word_idx] |= value << bit_pos;
            bit_pos += bits_i32;
            if bit_pos == 32 {
                word_idx += 1;
                bit_pos = 0;
            }
        } else {
            words[word_idx] |= value << bit_pos;
            word_idx += 1;
            words[word_idx] |= value >> (32 - bit_pos);
            bit_pos += bits_i32 - 32;
        }
    }

    let bytes_used = num_bytes - num_tail_bytes_not_needed(data.len(), bits);
    let start = out.len();
    out.resize(start + bytes_used, 0);
    for (i, word) in words.iter().enumerate() {
        let byte_pos = start + i * 4;
        if byte_pos >= start + bytes_used {
            break;
        }
        let word_bytes = word.to_le_bytes();
        let copy_len = ((start + bytes_used) - byte_pos).min(4);
        out[byte_pos..byte_pos + copy_len].copy_from_slice(&word_bytes[..copy_len]);
    }
}

fn bit_unstuff(encoded: &[u8], pos: &mut usize, elem_count: usize, bits: u8) -> Result<Vec<u32>> {
    if elem_count == 0 || bits >= 32 {
        return Err(LercError::CorruptInput(
            "invalid bit-stuffed element count or width",
        ));
    }

    let num_uints = (elem_count * bits as usize + 31) / 32;
    let num_bytes = num_uints * 4;
    let bytes_used = num_bytes - num_tail_bytes_not_needed(elem_count, bits);
    if encoded.len().saturating_sub(*pos) < bytes_used {
        return Err(LercError::BufferTooSmall);
    }

    let mut words = vec![0u32; num_uints];
    for (i, chunk) in encoded[*pos..*pos + bytes_used].chunks(4).enumerate() {
        let mut word_bytes = [0u8; 4];
        word_bytes[..chunk.len()].copy_from_slice(chunk);
        words[i] = u32::from_le_bytes(word_bytes);
    }

    let mut out = vec![0u32; elem_count];
    let mut word_idx = 0usize;
    let mut bit_pos = 0i32;
    let bits_i32 = bits as i32;
    let nb = 32 - bits_i32;

    for value in &mut out {
        if nb >= bit_pos {
            *value = (words[word_idx] << (nb - bit_pos)) >> nb;
            bit_pos += bits_i32;
            if bit_pos == 32 {
                word_idx += 1;
                bit_pos = 0;
            }
        } else {
            *value = words[word_idx] >> bit_pos;
            word_idx += 1;
            *value |= (words[word_idx] << (64 - bits_i32 - bit_pos)) >> nb;
            bit_pos -= nb;
        }
    }

    *pos += bytes_used;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::BitStuffer2;

    #[test]
    fn simple_round_trips_zero_width_data() {
        let data = vec![0u32; 17];
        let encoded = BitStuffer2::encode_simple(&data, 3).unwrap();
        assert_eq!(
            encoded.len(),
            BitStuffer2::compute_num_bytes_needed_simple(17, 0) as usize
        );
        let (decoded, consumed) = BitStuffer2::decode(&encoded, 17, 3).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, data);
    }

    #[test]
    fn simple_round_trips_various_bit_widths() {
        for bits in 1..31 {
            let max = (1u32 << bits) - 1;
            let data: Vec<u32> = (0..257).map(|i| ((i * 37) as u32) & max).collect();
            let encoded = BitStuffer2::encode_simple(&data, 3).unwrap();
            let (decoded, consumed) = BitStuffer2::decode(&encoded, data.len(), 3).unwrap();
            assert_eq!(consumed, encoded.len());
            assert_eq!(decoded, data);
        }
    }

    #[test]
    fn lut_round_trips_sparse_values() {
        let data = [0, 0, 9, 42, 9, 0, 42, 1000, 9, 1000];
        let mut sorted: Vec<(u32, u32)> = data
            .iter()
            .enumerate()
            .map(|(idx, &value)| (value, idx as u32))
            .collect();
        sorted.sort_unstable();

        let encoded = BitStuffer2::encode_lut(&sorted, 3).unwrap();
        let (decoded, consumed) = BitStuffer2::decode(&encoded, data.len(), 3).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, data);
    }

    #[test]
    fn rejects_too_many_decoded_elements() {
        let data = vec![1, 2, 3, 4];
        let encoded = BitStuffer2::encode_simple(&data, 3).unwrap();
        assert!(BitStuffer2::decode(&encoded, 3, 3).is_err());
    }
}
