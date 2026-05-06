/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! Packed bit-mask representation used by Lerc2.

use crate::types::{LercError, Result};

/// Valid-pixel mask using the same MSB-first packed bit order as C++ LERC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitMask {
    cols: usize,
    rows: usize,
    bits: Vec<u8>,
}

impl BitMask {
    /// Creates an all-invalid mask with the given dimensions.
    pub fn new(cols: usize, rows: usize) -> Result<Self> {
        if cols == 0 || rows == 0 {
            return Err(LercError::WrongParam(
                "bit mask dimensions must be non-zero",
            ));
        }

        let pixels = cols
            .checked_mul(rows)
            .ok_or(LercError::WrongParam("bit mask dimensions overflow"))?;
        Ok(Self {
            cols,
            rows,
            bits: vec![0; pixels.div_ceil(8)],
        })
    }

    /// Converts a byte mask to a packed bit mask.
    ///
    /// Nonzero input bytes are treated as valid pixels.
    pub fn from_byte_mask(bytes: &[u8], cols: usize, rows: usize) -> Result<Self> {
        let pixels = cols
            .checked_mul(rows)
            .ok_or(LercError::WrongParam("bit mask dimensions overflow"))?;
        if bytes.len() != pixels {
            return Err(LercError::WrongParam(
                "byte mask length does not match dimensions",
            ));
        }

        let mut mask = Self::new(cols, rows)?;
        mask.set_all_valid();
        for (idx, &value) in bytes.iter().enumerate() {
            if value == 0 {
                mask.set_invalid(idx)?;
            }
        }
        Ok(mask)
    }

    /// Returns the number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Returns the number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Returns the number of pixels represented by this mask.
    pub fn pixel_count(&self) -> usize {
        self.cols * self.rows
    }

    /// Returns the length of the packed bit buffer in bytes.
    pub fn byte_len(&self) -> usize {
        self.bits.len()
    }

    /// Returns the packed mask bytes.
    pub fn bits(&self) -> &[u8] {
        &self.bits
    }

    /// Returns the packed mask bytes mutably.
    pub fn bits_mut(&mut self) -> &mut [u8] {
        &mut self.bits
    }

    /// Marks every pixel valid.
    pub fn set_all_valid(&mut self) {
        self.bits.fill(0xff);
    }

    /// Marks every pixel invalid.
    pub fn set_all_invalid(&mut self) {
        self.bits.fill(0);
    }

    /// Returns whether the linear pixel index is valid.
    pub fn is_valid(&self, idx: usize) -> Result<bool> {
        self.check_idx(idx)?;
        Ok((self.bits[idx >> 3] & Self::bit(idx)) != 0)
    }

    /// Returns whether the pixel at `(row, col)` is valid.
    pub fn is_valid_at(&self, row: usize, col: usize) -> Result<bool> {
        self.index(row, col).and_then(|idx| self.is_valid(idx))
    }

    /// Marks the linear pixel index valid.
    pub fn set_valid(&mut self, idx: usize) -> Result<()> {
        self.check_idx(idx)?;
        self.bits[idx >> 3] |= Self::bit(idx);
        Ok(())
    }

    /// Marks the pixel at `(row, col)` valid.
    pub fn set_valid_at(&mut self, row: usize, col: usize) -> Result<()> {
        let idx = self.index(row, col)?;
        self.set_valid(idx)
    }

    /// Marks the linear pixel index invalid.
    pub fn set_invalid(&mut self, idx: usize) -> Result<()> {
        self.check_idx(idx)?;
        self.bits[idx >> 3] &= !Self::bit(idx);
        Ok(())
    }

    /// Marks the pixel at `(row, col)` invalid.
    pub fn set_invalid_at(&mut self, row: usize, col: usize) -> Result<()> {
        let idx = self.index(row, col)?;
        self.set_invalid(idx)
    }

    /// Counts valid pixels, excluding padding bits in the final byte.
    pub fn count_valid_bits(&self) -> usize {
        let mut sum: usize = self
            .bits
            .iter()
            .map(|byte| byte.count_ones() as usize)
            .sum();
        for idx in self.pixel_count()..self.bits.len() * 8 {
            if (self.bits[idx >> 3] & Self::bit(idx)) != 0 {
                sum -= 1;
            }
        }
        sum
    }

    /// Converts this mask to one byte per pixel, using `1` for valid and `0` for invalid.
    pub fn to_byte_mask(&self) -> Vec<u8> {
        let mut bytes = vec![0; self.pixel_count()];
        for (idx, out) in bytes.iter_mut().enumerate() {
            if (self.bits[idx >> 3] & Self::bit(idx)) != 0 {
                *out = 1;
            }
        }
        bytes
    }

    /// Returns the MSB-first bit mask for a linear pixel index.
    pub fn bit(idx: usize) -> u8 {
        0x80 >> (idx & 7)
    }

    fn index(&self, row: usize, col: usize) -> Result<usize> {
        if row >= self.rows || col >= self.cols {
            return Err(LercError::WrongParam("bit mask index out of bounds"));
        }
        Ok(row * self.cols + col)
    }

    fn check_idx(&self, idx: usize) -> Result<()> {
        if idx >= self.pixel_count() {
            return Err(LercError::WrongParam("bit mask index out of bounds"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::BitMask;

    #[test]
    fn uses_msb_first_bit_order() {
        assert_eq!(BitMask::bit(0), 0b1000_0000);
        assert_eq!(BitMask::bit(1), 0b0100_0000);
        assert_eq!(BitMask::bit(7), 0b0000_0001);
        assert_eq!(BitMask::bit(8), 0b1000_0000);
    }

    #[test]
    fn sets_and_counts_valid_bits_ignoring_padding() {
        let mut mask = BitMask::new(5, 2).unwrap();
        mask.set_all_valid();
        assert_eq!(mask.byte_len(), 2);
        assert_eq!(mask.count_valid_bits(), 10);

        mask.set_invalid(0).unwrap();
        mask.set_invalid(9).unwrap();
        assert_eq!(mask.count_valid_bits(), 8);
        assert!(!mask.is_valid(0).unwrap());
        assert!(!mask.is_valid_at(1, 4).unwrap());

        mask.bits_mut()[1] |= 0b0011_1111;
        assert_eq!(mask.count_valid_bits(), 8);
    }

    #[test]
    fn converts_to_and_from_byte_masks() {
        let bytes = [1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0];
        let mask = BitMask::from_byte_mask(&bytes, 4, 3).unwrap();
        assert_eq!(mask.bits(), &[0b1011_0011, 0b1010_1111]);
        assert_eq!(mask.count_valid_bits(), 7);
        assert_eq!(mask.to_byte_mask(), bytes);
    }

    #[test]
    fn matches_cpp_byte_mask_to_packed_bits_conversion() {
        let bytes = [255, 0, 2, 0, 1, 1, 0, 7, 0, 1];
        let mask = BitMask::from_byte_mask(&bytes, 5, 2).unwrap();

        assert_eq!(mask.bits(), &[0b1010_1101, 0b0111_1111]);
        assert_eq!(mask.count_valid_bits(), 6);
    }

    #[test]
    fn matches_cpp_packed_bits_to_byte_mask_conversion() {
        let mut mask = BitMask::new(5, 2).unwrap();
        mask.bits_mut().copy_from_slice(&[0b0101_0011, 0b1000_1111]);

        assert_eq!(mask.to_byte_mask(), [0, 1, 0, 1, 0, 0, 1, 1, 1, 0]);
        assert_eq!(mask.count_valid_bits(), 5);
    }

    #[test]
    fn rejects_invalid_dimensions_and_indexes() {
        assert!(BitMask::new(0, 3).is_err());
        assert!(BitMask::from_byte_mask(&[1, 0], 3, 1).is_err());

        let mut mask = BitMask::new(2, 2).unwrap();
        assert!(mask.set_valid(4).is_err());
        assert!(mask.set_invalid_at(2, 0).is_err());
        assert!(mask.is_valid_at(0, 2).is_err());
    }
}
