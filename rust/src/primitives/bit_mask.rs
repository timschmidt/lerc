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

    /// Returns true when every represented pixel is valid.
    pub fn is_all_valid(&self) -> bool {
        self.count_valid_bits() == self.pixel_count()
    }

    /// Returns true when every represented pixel is invalid.
    pub fn is_all_invalid(&self) -> bool {
        self.count_valid_bits() == 0
    }

    /// Compares two optional masks using the C++ `Lerc::MasksDiffer` semantics.
    ///
    /// A missing mask means every pixel is valid. Therefore `None` and an
    /// explicit all-valid mask are considered equivalent.
    pub fn differs_from_optional(&self, other: Option<&Self>) -> bool {
        optional_masks_differ(Some(self), other)
    }

    /// Returns true when both masks represent the same pixel validity pattern.
    ///
    /// Padding bits in the final packed byte are ignored.
    pub fn same_valid_pixels(&self, other: &Self) -> bool {
        if self.cols != other.cols || self.rows != other.rows {
            return false;
        }

        let pixels = self.pixel_count();
        let full_bytes = pixels / 8;
        if self.bits[..full_bytes] != other.bits[..full_bytes] {
            return false;
        }

        let tail_bits = pixels & 7;
        if tail_bits == 0 {
            return true;
        }

        let tail_mask = 0xff << (8 - tail_bits);
        (self.bits[full_bytes] & tail_mask) == (other.bits[full_bytes] & tail_mask)
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

/// Compares two optional packed masks using the C++ `Lerc::MasksDiffer` semantics.
///
/// A missing mask represents an all-valid mask. Explicit all-valid masks are
/// therefore equivalent to `None`; otherwise packed mask bytes are compared
/// directly.
pub fn optional_masks_differ(left: Option<&BitMask>, right: Option<&BitMask>) -> bool {
    match (left, right) {
        (None, None) => false,
        (None, Some(mask)) | (Some(mask), None) => !mask.is_all_valid(),
        (Some(left), Some(right)) => !left.same_valid_pixels(right),
    }
}

#[cfg(test)]
mod tests {
    use super::{optional_masks_differ, BitMask};

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
    fn compares_valid_pixels_while_ignoring_padding_bits() {
        let mut left = BitMask::from_byte_mask(&[1, 0, 1, 1, 0, 1, 1, 0, 1, 1], 5, 2).unwrap();
        let mut right = BitMask::from_byte_mask(&[1, 0, 1, 1, 0, 1, 1, 0, 1, 1], 5, 2).unwrap();
        right.bits_mut()[1] ^= 0b0011_1111;

        assert!(left.same_valid_pixels(&right));
        assert!(!optional_masks_differ(Some(&left), Some(&right)));

        left.set_invalid(9).unwrap();
        assert!(!left.same_valid_pixels(&right));
        assert!(optional_masks_differ(Some(&left), Some(&right)));

        let wrong_shape = BitMask::from_byte_mask(&[1, 0, 1, 1, 0, 1, 1, 0, 1, 1], 2, 5).unwrap();
        assert!(!left.same_valid_pixels(&wrong_shape));
    }

    #[test]
    fn reports_all_valid_and_all_invalid_masks() {
        let mut mask = BitMask::new(5, 2).unwrap();
        assert!(mask.is_all_invalid());
        assert!(!mask.is_all_valid());

        mask.set_all_valid();
        assert!(mask.is_all_valid());
        assert!(!mask.is_all_invalid());

        mask.bits_mut()[1] |= 0b0011_1111;
        assert!(mask.is_all_valid());

        mask.set_invalid(9).unwrap();
        assert!(!mask.is_all_valid());
        assert!(!mask.is_all_invalid());
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
    fn matches_cpp_larger_bit_mask_conversions_with_padding() {
        let bytes = [
            1, 0, 2, 0, 3, 4, 0, 5, 0, 0, 6, 7, 0, 8, 0, 9, 10, 0, 11, 0, 12,
        ];
        let mask = BitMask::from_byte_mask(&bytes, 7, 3).unwrap();

        assert_eq!(mask.bits(), &[173, 53, 175]);
        assert_eq!(mask.count_valid_bits(), 12);
        assert_eq!(
            mask.to_byte_mask(),
            [1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1]
        );

        let mut raw = BitMask::new(7, 3).unwrap();
        raw.bits_mut().copy_from_slice(&[170, 85, 227]);
        assert_eq!(raw.count_valid_bits(), 11);
        assert_eq!(
            raw.to_byte_mask(),
            [1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0]
        );
    }

    #[test]
    fn optional_mask_difference_matches_cpp_null_means_all_valid_semantics() {
        let mut all_valid = BitMask::new(3, 2).unwrap();
        all_valid.set_all_valid();
        let partial = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        let same_partial = BitMask::from_byte_mask(&[1, 0, 1, 1, 1, 0], 3, 2).unwrap();
        let other_partial = BitMask::from_byte_mask(&[1, 1, 1, 1, 1, 0], 3, 2).unwrap();

        assert!(!optional_masks_differ(None, None));
        assert!(!optional_masks_differ(None, Some(&all_valid)));
        assert!(!optional_masks_differ(Some(&all_valid), None));
        assert!(optional_masks_differ(None, Some(&partial)));
        assert!(partial.differs_from_optional(None));
        assert!(!partial.differs_from_optional(Some(&same_partial)));
        assert!(partial.differs_from_optional(Some(&other_partial)));
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
