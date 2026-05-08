//! Private support utilities for byte IO and numeric conversions.
//!
//! This module is reserved for helpers shared across format implementations as
//! the larger Lerc1 and Lerc2 modules are split.

use crate::{DataType, EncodeSpec, LercError, Result};

#[derive(Debug, Clone)]
pub(crate) struct PreparedNanEncode {
    pub(crate) spec: EncodeSpec,
    pub(crate) data: Vec<u8>,
    pub(crate) masks: Vec<u8>,
}

pub(crate) fn prepare_nan_encode_inputs(
    spec: EncodeSpec,
    data: &[u8],
    mask_bytes: Option<&[u8]>,
) -> Result<Option<PreparedNanEncode>> {
    if spec.data_type != DataType::Float && spec.data_type != DataType::Double {
        return Ok(None);
    }

    let n_pixels = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam("encode pixel count overflow"))?;
    let value_size = spec.data_type.size_in_bytes();
    let band_value_bytes = n_pixels
        .checked_mul(spec.n_depth)
        .and_then(|count| count.checked_mul(value_size))
        .ok_or(LercError::WrongParam("encode band byte count overflow"))?;
    validate_encode_preprocess_inputs(spec, data, mask_bytes)?;

    let mut prepared_data = data.to_vec();
    let mut prepared_masks = vec![1u8; n_pixels * spec.n_bands];
    let mut found_nan = false;

    for i_band in 0..spec.n_bands {
        let data_band_offset = i_band * band_value_bytes;
        let mask_band_offset = i_band * n_pixels;
        let input_mask = input_band_mask(spec, mask_bytes, i_band, n_pixels);

        for i_pixel in 0..n_pixels {
            let input_valid = input_mask.is_none_or(|mask| mask[i_pixel] != 0);
            if !input_valid {
                prepared_masks[mask_band_offset + i_pixel] = 0;
                continue;
            }

            let pixel_offset = data_band_offset + i_pixel * spec.n_depth * value_size;
            let mut nan_count = 0usize;
            for i_depth in 0..spec.n_depth {
                let value_offset = pixel_offset + i_depth * value_size;
                let value = read_float_value_for_nan(spec.data_type, &data[value_offset..]);
                if value.is_nan() {
                    nan_count += 1;
                    write_zero_float_value(spec.data_type, &mut prepared_data[value_offset..]);
                }
            }

            if nan_count == 0 {
                continue;
            }
            found_nan = true;
            if nan_count == spec.n_depth {
                prepared_masks[mask_band_offset + i_pixel] = 0;
            } else if spec.n_depth > 1 {
                return Err(LercError::NaN);
            }
        }
    }

    if !found_nan {
        return Ok(None);
    }

    Ok(Some(PreparedNanEncode {
        spec: EncodeSpec {
            n_masks: if spec.n_bands == 1 { 1 } else { spec.n_bands },
            ..spec
        },
        data: prepared_data,
        masks: prepared_masks,
    }))
}

pub(crate) fn prepare_active_no_data_nan_encode_inputs(
    spec: EncodeSpec,
    data: &[u8],
    mask_bytes: Option<&[u8]>,
    uses_no_data: &[u8],
    no_data_values: Option<&[f64]>,
) -> Result<Option<PreparedNanEncode>> {
    if spec.data_type != DataType::Float && spec.data_type != DataType::Double {
        return Ok(None);
    }
    let Some(no_data_values) = no_data_values else {
        return Ok(None);
    };

    let n_pixels = spec
        .n_cols
        .checked_mul(spec.n_rows)
        .ok_or(LercError::WrongParam("encode pixel count overflow"))?;
    let value_size = spec.data_type.size_in_bytes();
    let band_value_bytes = n_pixels
        .checked_mul(spec.n_depth)
        .and_then(|count| count.checked_mul(value_size))
        .ok_or(LercError::WrongParam("encode band byte count overflow"))?;
    validate_encode_preprocess_inputs(spec, data, mask_bytes)?;

    let mut prepared_data = data.to_vec();
    let mut prepared_masks = vec![1u8; n_pixels * spec.n_bands];
    let mut found_nan = false;

    for i_band in 0..spec.n_bands {
        let data_band_offset = i_band * band_value_bytes;
        let mask_band_offset = i_band * n_pixels;
        let band_uses_no_data = uses_no_data.get(i_band).copied().unwrap_or(0) != 0;
        let no_data_value = no_data_values.get(i_band).copied().unwrap_or(0.0);
        if band_uses_no_data {
            validate_float_no_data_value_for_encode(spec.data_type, no_data_value)?;
        }
        let input_mask = input_band_mask(spec, mask_bytes, i_band, n_pixels);

        for i_pixel in 0..n_pixels {
            let input_valid = input_mask.is_none_or(|mask| mask[i_pixel] != 0);
            if !input_valid {
                prepared_masks[mask_band_offset + i_pixel] = 0;
                continue;
            }

            let pixel_offset = data_band_offset + i_pixel * spec.n_depth * value_size;
            let mut nan_count = 0usize;
            for i_depth in 0..spec.n_depth {
                let value_offset = pixel_offset + i_depth * value_size;
                let value = read_float_value_for_nan(spec.data_type, &data[value_offset..]);
                if value.is_nan() {
                    nan_count += 1;
                    found_nan = true;
                    if band_uses_no_data && spec.n_depth > 1 {
                        write_float_value_for_encode(
                            spec.data_type,
                            no_data_value,
                            &mut prepared_data[value_offset..],
                        );
                    } else {
                        write_zero_float_value(spec.data_type, &mut prepared_data[value_offset..]);
                    }
                }
            }

            if nan_count == 0 {
                continue;
            }
            if nan_count == spec.n_depth {
                prepared_masks[mask_band_offset + i_pixel] = 0;
            } else if spec.n_depth > 1 && !band_uses_no_data {
                return Err(LercError::NaN);
            }
        }
    }

    if !found_nan {
        return Ok(None);
    }

    Ok(Some(PreparedNanEncode {
        spec: EncodeSpec {
            n_masks: if spec.n_bands == 1 { 1 } else { spec.n_bands },
            ..spec
        },
        data: prepared_data,
        masks: prepared_masks,
    }))
}

fn validate_encode_preprocess_inputs(
    spec: EncodeSpec,
    data: &[u8],
    mask_bytes: Option<&[u8]>,
) -> Result<()> {
    spec.validate()?;
    if data.len() != spec.data_byte_len()? {
        return Err(LercError::WrongParam("Lerc2 encode data length mismatch"));
    }
    if let Some(mask_bytes) = mask_bytes {
        if spec.n_masks > 0 && mask_bytes.len() != spec.mask_byte_len()? {
            return Err(LercError::WrongParam("Lerc2 encode mask length mismatch"));
        }
    }
    Ok(())
}

fn input_band_mask(
    spec: EncodeSpec,
    mask_bytes: Option<&[u8]>,
    i_band: usize,
    n_pixels: usize,
) -> Option<&[u8]> {
    match (spec.n_masks, mask_bytes) {
        (0, _) => None,
        (1, Some(masks)) => Some(&masks[..n_pixels]),
        (_, Some(masks)) => {
            let offset = i_band * n_pixels;
            Some(&masks[offset..offset + n_pixels])
        }
        (_, None) => None,
    }
}

fn read_float_value_for_nan(data_type: DataType, bytes: &[u8]) -> f64 {
    match data_type {
        DataType::Float => f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        DataType::Double => f64::from_le_bytes(bytes[..8].try_into().unwrap()),
        _ => unreachable!("NaN filtering only handles float and double"),
    }
}

fn write_zero_float_value(data_type: DataType, bytes: &mut [u8]) {
    match data_type {
        DataType::Float => bytes[..4].copy_from_slice(&0.0f32.to_le_bytes()),
        DataType::Double => bytes[..8].copy_from_slice(&0.0f64.to_le_bytes()),
        _ => unreachable!("NaN filtering only handles float and double"),
    }
}

fn write_float_value_for_encode(data_type: DataType, value: f64, bytes: &mut [u8]) {
    match data_type {
        DataType::Float => bytes[..4].copy_from_slice(&(value as f32).to_le_bytes()),
        DataType::Double => bytes[..8].copy_from_slice(&value.to_le_bytes()),
        _ => unreachable!("float writer only handles float and double"),
    }
}

fn validate_float_no_data_value_for_encode(data_type: DataType, value: f64) -> Result<()> {
    if data_type == DataType::Float && (value < f32::MIN as f64 || value > f32::MAX as f64) {
        return Err(LercError::WrongParam(
            "active no-data value is outside the data type range",
        ));
    }
    Ok(())
}
