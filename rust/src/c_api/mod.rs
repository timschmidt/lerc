/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! C ABI entry points backed by the safe Rust implementation.

use crate::{
    decode_lerc_supported_into, decode_lerc_supported_to_f64, encode_lerc2_auto,
    encode_lerc2_auto_with_no_data, get_lerc2_blob_info_arrays, get_lerc2_data_ranges,
    get_lerc2_no_data_info, get_lerc_info, DataType, DecodeIntoSpec, EncodeSpec, ErrCode,
    LercError, LercInfo, LercStatus,
};
use core::ffi::c_void;
use core::slice;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// C ABI equivalent of `lerc_computeCompressedSize`.
///
/// Inputs are encoded by the Rust Lerc2 auto selector, including constant,
/// one-sweep, tiled LUT, eligible byte-Huffman, and eligible floating-point
/// Huffman paths. No-data metadata is available through the 4D entry points.
///
/// # Safety
///
/// `p_data` must point to readable scalar data for the requested shape. When
/// `n_masks` is nonzero, `p_valid_bytes` must point to `n_cols * n_rows *
/// n_masks` readable bytes. `num_bytes` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lerc_computeCompressedSize(
    p_data: *const c_void,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    num_bytes: *mut u32,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_compute_compressed_size_impl(
            p_data,
            -1,
            data_type,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            n_masks,
            p_valid_bytes,
            max_z_err,
            num_bytes,
            None,
            None,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_computeCompressedSizeForVersion`.
///
/// Inputs are encoded by the Rust Lerc2 auto selector, including constant,
/// one-sweep, tiled LUT, eligible byte-Huffman, and eligible floating-point
/// Huffman paths. No-data metadata is available through the 4D entry points.
///
/// # Safety
///
/// The pointer requirements match [`lerc_computeCompressedSize`].
#[no_mangle]
pub unsafe extern "C" fn lerc_computeCompressedSizeForVersion(
    p_data: *const c_void,
    codec_version: i32,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    num_bytes: *mut u32,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_compute_compressed_size_impl(
            p_data,
            codec_version,
            data_type,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            n_masks,
            p_valid_bytes,
            max_z_err,
            num_bytes,
            None,
            None,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_encode`.
///
/// Inputs are encoded by the Rust Lerc2 auto selector, including constant,
/// one-sweep, tiled LUT, eligible byte-Huffman, and eligible floating-point
/// Huffman paths. No-data metadata is available through the 4D entry points.
///
/// # Safety
///
/// `p_data` and optional `p_valid_bytes` must be readable for the requested
/// shape. `p_out_buffer` must point to `out_buffer_size` writable bytes.
/// `n_bytes_written` must be writable.
#[no_mangle]
pub unsafe extern "C" fn lerc_encode(
    p_data: *const c_void,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    p_out_buffer: *mut u8,
    out_buffer_size: u32,
    n_bytes_written: *mut u32,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_encode_impl(
            p_data,
            -1,
            data_type,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            n_masks,
            p_valid_bytes,
            max_z_err,
            p_out_buffer,
            out_buffer_size,
            n_bytes_written,
            None,
            None,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_encodeForVersion`.
///
/// Inputs are encoded by the Rust Lerc2 auto selector, including constant,
/// one-sweep, tiled LUT, eligible byte-Huffman, and eligible floating-point
/// Huffman paths. No-data metadata is available through the 4D entry points.
///
/// # Safety
///
/// The pointer requirements match [`lerc_encode`].
#[no_mangle]
pub unsafe extern "C" fn lerc_encodeForVersion(
    p_data: *const c_void,
    codec_version: i32,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    p_out_buffer: *mut u8,
    out_buffer_size: u32,
    n_bytes_written: *mut u32,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_encode_impl(
            p_data,
            codec_version,
            data_type,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            n_masks,
            p_valid_bytes,
            max_z_err,
            p_out_buffer,
            out_buffer_size,
            n_bytes_written,
            None,
            None,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_computeCompressedSize_4D`.
///
/// Calls with or without active no-data bands are encoded by the Rust Lerc2
/// auto selector, including no-data-aware tiled LUT, eligible byte-Huffman, and
/// eligible floating-point Huffman paths.
///
/// # Safety
///
/// The pointer requirements match [`lerc_computeCompressedSize`]. When
/// `p_uses_no_data` is non-null, `no_data_values` must point to at least
/// `n_bands` readable values.
#[no_mangle]
pub unsafe extern "C" fn lerc_computeCompressedSize_4D(
    p_data: *const c_void,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    num_bytes: *mut u32,
    p_uses_no_data: *const u8,
    no_data_values: *const f64,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_compute_compressed_size_impl(
            p_data,
            -1,
            data_type,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            n_masks,
            p_valid_bytes,
            max_z_err,
            num_bytes,
            Some(p_uses_no_data),
            Some(no_data_values),
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_encode_4D`.
///
/// Calls with or without active no-data bands are encoded by the Rust Lerc2
/// auto selector, including no-data-aware tiled LUT, eligible byte-Huffman, and
/// eligible floating-point Huffman paths.
///
/// # Safety
///
/// The pointer requirements match [`lerc_encode`]. When `p_uses_no_data` is
/// non-null, `no_data_values` must point to at least `n_bands` readable values.
#[no_mangle]
pub unsafe extern "C" fn lerc_encode_4D(
    p_data: *const c_void,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    p_out_buffer: *mut u8,
    out_buffer_size: u32,
    n_bytes_written: *mut u32,
    p_uses_no_data: *const u8,
    no_data_values: *const f64,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_encode_impl(
            p_data,
            -1,
            data_type,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            n_masks,
            p_valid_bytes,
            max_z_err,
            p_out_buffer,
            out_buffer_size,
            n_bytes_written,
            Some(p_uses_no_data),
            Some(no_data_values),
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_getBlobInfo`.
///
/// This function supports Lerc2 blobs handled by the Rust metadata parser and
/// the checked-in legacy Lerc1 metadata path. It fills the caller-provided
/// arrays using the same order and partial-fill behavior as the C++ API.
///
/// # Safety
///
/// `p_lerc_blob` must point to `blob_size` readable bytes. When non-null and
/// paired with a positive size, `info_array` and `data_range_array` must point
/// to writable arrays with at least `info_array_size` or
/// `data_range_array_size` elements respectively.
#[no_mangle]
pub unsafe extern "C" fn lerc_getBlobInfo(
    p_lerc_blob: *const u8,
    blob_size: u32,
    info_array: *mut u32,
    data_range_array: *mut f64,
    info_array_size: i32,
    data_range_array_size: i32,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_get_blob_info_impl(
            p_lerc_blob,
            blob_size,
            info_array,
            data_range_array,
            info_array_size,
            data_range_array_size,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_getDataRanges`.
///
/// This function supports Lerc2 blobs handled by the Rust metadata parser and
/// the checked-in legacy Lerc1 metadata path. It writes minima and maxima in
/// band-major, depth-minor order.
///
/// # Safety
///
/// `p_lerc_blob` must point to `blob_size` readable bytes. `p_mins` and
/// `p_maxs` must point to writable arrays with at least `n_depth * n_bands`
/// elements.
#[no_mangle]
pub unsafe extern "C" fn lerc_getDataRanges(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_depth: i32,
    n_bands: i32,
    p_mins: *mut f64,
    p_maxs: *mut f64,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_get_data_ranges_impl(p_lerc_blob, blob_size, n_depth, n_bands, p_mins, p_maxs)
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_decode` for the currently supported decode subset.
///
/// Decoded data is written in the caller-requested native LERC scalar type and
/// band-major order. Valid-pixel bytes are written when `n_masks` is nonzero.
///
/// # Safety
///
/// `p_lerc_blob` must point to `blob_size` readable bytes. `p_data` must point
/// to writable storage for `n_depth * n_cols * n_rows * n_bands` values of
/// `data_type`. When `n_masks` is nonzero, `p_valid_bytes` must point to
/// writable storage for `n_cols * n_rows * n_masks` bytes.
#[no_mangle]
pub unsafe extern "C" fn lerc_decode(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    data_type: u32,
    p_data: *mut c_void,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_decode_impl(
            p_lerc_blob,
            blob_size,
            n_masks,
            p_valid_bytes,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            data_type,
            p_data,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_decodeToDouble` for the supported decode subset.
///
/// Decoded values are converted to 64-bit floating point values in band-major
/// order. Valid-pixel bytes are written when `n_masks` is nonzero.
///
/// # Safety
///
/// `p_lerc_blob` must point to `blob_size` readable bytes. `p_data` must point
/// to writable storage for `n_depth * n_cols * n_rows * n_bands` `double`
/// values. When `n_masks` is nonzero, `p_valid_bytes` must point to writable
/// storage for `n_cols * n_rows * n_masks` bytes.
#[no_mangle]
pub unsafe extern "C" fn lerc_decodeToDouble(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    p_data: *mut f64,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_decode_to_double_impl(
            p_lerc_blob,
            blob_size,
            n_masks,
            p_valid_bytes,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            p_data,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_decode_4D` for the supported decode subset.
///
/// This is equivalent to [`lerc_decode`] but can report version 6+ no-data
/// metadata through `p_uses_no_data` and `no_data_values`.
///
/// # Safety
///
/// The data and mask pointer requirements match [`lerc_decode`]. When the blob
/// uses multi-depth no-data metadata, `p_uses_no_data` and `no_data_values`
/// must point to writable arrays with at least `n_bands` elements.
#[no_mangle]
pub unsafe extern "C" fn lerc_decode_4D(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    data_type: u32,
    p_data: *mut c_void,
    p_uses_no_data: *mut u8,
    no_data_values: *mut f64,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_decode_4d_impl(
            p_lerc_blob,
            blob_size,
            n_masks,
            p_valid_bytes,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            data_type,
            p_data,
            p_uses_no_data,
            no_data_values,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_decodeToDouble_4D` for the supported decode subset.
///
/// This is equivalent to [`lerc_decodeToDouble`] but can report version 6+
/// no-data metadata through `p_uses_no_data` and `no_data_values`.
///
/// # Safety
///
/// The data and mask pointer requirements match [`lerc_decodeToDouble`]. When
/// the blob uses multi-depth no-data metadata, `p_uses_no_data` and
/// `no_data_values` must point to writable arrays with at least `n_bands`
/// elements.
#[no_mangle]
pub unsafe extern "C" fn lerc_decodeToDouble_4D(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    p_data: *mut f64,
    p_uses_no_data: *mut u8,
    no_data_values: *mut f64,
) -> LercStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_decode_to_double_4d_impl(
            p_lerc_blob,
            blob_size,
            n_masks,
            p_valid_bytes,
            n_depth,
            n_cols,
            n_rows,
            n_bands,
            p_data,
            p_uses_no_data,
            no_data_values,
        )
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

unsafe fn lerc_compute_compressed_size_impl(
    p_data: *const c_void,
    codec_version: i32,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    num_bytes: *mut u32,
    p_uses_no_data: Option<*const u8>,
    no_data_values: Option<*const f64>,
) -> u32 {
    if num_bytes.is_null() {
        return ErrCode::WrongParam as u32;
    }
    unsafe {
        *num_bytes = 0;
    }

    let spec = match validate_encode_shape(
        p_data,
        data_type,
        n_depth,
        n_cols,
        n_rows,
        n_bands,
        n_masks,
        p_valid_bytes,
        max_z_err,
    ) {
        Ok(spec) => spec,
        Err(err) => return err as u32,
    };
    if !validate_no_data_inputs(p_uses_no_data, no_data_values) {
        return ErrCode::WrongParam as u32;
    }
    let version = match normalize_encode_version(codec_version) {
        Ok(version) => version,
        Err(err) => return err as u32,
    };

    match try_encode_supported_blob(
        p_data,
        spec,
        p_valid_bytes,
        max_z_err,
        version,
        p_uses_no_data,
        no_data_values,
    ) {
        Ok(Some(blob)) => match u32::try_from(blob.len()) {
            Ok(len) => {
                unsafe {
                    *num_bytes = len;
                }
                ErrCode::Ok as u32
            }
            Err(_) => ErrCode::BufferTooSmall as u32,
        },
        Ok(None) => ErrCode::Failed as u32,
        Err(err) => err.err_code() as u32,
    }
}

unsafe fn lerc_encode_impl(
    p_data: *const c_void,
    codec_version: i32,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    p_out_buffer: *mut u8,
    out_buffer_size: u32,
    n_bytes_written: *mut u32,
    p_uses_no_data: Option<*const u8>,
    no_data_values: Option<*const f64>,
) -> u32 {
    if n_bytes_written.is_null() {
        return ErrCode::WrongParam as u32;
    }
    unsafe {
        *n_bytes_written = 0;
    }

    let spec = match validate_encode_shape(
        p_data,
        data_type,
        n_depth,
        n_cols,
        n_rows,
        n_bands,
        n_masks,
        p_valid_bytes,
        max_z_err,
    ) {
        Ok(spec) => spec,
        Err(err) => return err as u32,
    };
    if p_out_buffer.is_null() || out_buffer_size == 0 {
        return ErrCode::WrongParam as u32;
    }
    unsafe {
        slice::from_raw_parts_mut(p_out_buffer, out_buffer_size as usize).fill(0);
    }
    if !validate_no_data_inputs(p_uses_no_data, no_data_values) {
        return ErrCode::WrongParam as u32;
    }
    let version = match normalize_encode_version(codec_version) {
        Ok(version) => version,
        Err(err) => return err as u32,
    };

    match try_encode_supported_blob(
        p_data,
        spec,
        p_valid_bytes,
        max_z_err,
        version,
        p_uses_no_data,
        no_data_values,
    ) {
        Ok(Some(blob)) => {
            if blob.len() > out_buffer_size as usize {
                return ErrCode::BufferTooSmall as u32;
            }
            unsafe {
                slice::from_raw_parts_mut(p_out_buffer, blob.len()).copy_from_slice(&blob);
                *n_bytes_written = blob.len() as u32;
            }
            ErrCode::Ok as u32
        }
        Ok(None) => ErrCode::Failed as u32,
        Err(err) => err.err_code() as u32,
    }
}

fn validate_encode_shape(
    p_data: *const c_void,
    data_type: u32,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    n_masks: i32,
    p_valid_bytes: *const u8,
    max_z_err: f64,
) -> core::result::Result<EncodeSpec, ErrCode> {
    if p_data.is_null()
        || n_depth <= 0
        || n_cols <= 0
        || n_rows <= 0
        || n_bands <= 0
        || max_z_err < 0.0
        || (n_masks > 0 && p_valid_bytes.is_null())
    {
        return Err(ErrCode::WrongParam);
    }

    let parsed_type = DataType::try_from(data_type).map_err(|_| ErrCode::WrongParam)?;
    let spec = EncodeSpec {
        data_type: parsed_type,
        n_depth: n_depth as usize,
        n_cols: n_cols as usize,
        n_rows: n_rows as usize,
        n_bands: n_bands as usize,
        n_masks: n_masks as usize,
    };
    spec.validate().map_err(|err| err.err_code())?;
    spec.data_byte_len().map_err(|err| err.err_code())?;
    spec.mask_byte_len().map_err(|err| err.err_code())?;
    Ok(spec)
}

fn normalize_encode_version(codec_version: i32) -> core::result::Result<i32, ErrCode> {
    if codec_version < 0 {
        Ok(6)
    } else if (2..=6).contains(&codec_version) {
        Ok(codec_version)
    } else {
        Err(ErrCode::WrongParam)
    }
}

unsafe fn try_encode_supported_blob(
    p_data: *const c_void,
    spec: EncodeSpec,
    p_valid_bytes: *const u8,
    max_z_err: f64,
    version: i32,
    p_uses_no_data: Option<*const u8>,
    no_data_values: Option<*const f64>,
) -> crate::Result<Option<Vec<u8>>> {
    let data = unsafe { slice::from_raw_parts(p_data.cast::<u8>(), spec.data_byte_len()?) };
    let mask_bytes = if spec.n_masks > 0 {
        Some(unsafe { slice::from_raw_parts(p_valid_bytes, spec.mask_byte_len()?) })
    } else {
        None
    };
    let uses_no_data = unsafe { encode_uses_no_data_slice(spec.n_bands, p_uses_no_data) };
    let has_active_no_data = uses_no_data
        .as_ref()
        .is_some_and(|uses| uses.iter().any(|&v| v != 0));
    if has_active_no_data && max_z_err < 0.0 {
        return Err(LercError::WrongParam(
            "active no-data encode requires nonnegative max_z_error",
        ));
    }
    let no_data_values = if has_active_no_data {
        match no_data_values {
            Some(ptr) if !ptr.is_null() => {
                Some(unsafe { slice::from_raw_parts(ptr, spec.n_bands) })
            }
            _ => {
                return Err(LercError::WrongParam(
                    "active no-data encode requires values",
                ))
            }
        }
    } else {
        None
    };

    if let Some(uses_no_data) = uses_no_data.as_ref() {
        if has_active_no_data {
            if version < 6 {
                return Err(LercError::WrongParam(
                    "active no-data encode requires version 6 or newer",
                ));
            }
            let prepared_nan = prepare_active_no_data_nan_encode_inputs(
                spec,
                data,
                mask_bytes,
                uses_no_data,
                no_data_values,
            )?;
            let (spec, data, mask_bytes) = if let Some(prepared) = prepared_nan.as_ref() {
                (
                    prepared.spec,
                    prepared.data.as_slice(),
                    Some(prepared.masks.as_slice()),
                )
            } else {
                (spec, data, mask_bytes)
            };
            return encode_lerc2_auto_with_no_data(
                spec,
                data,
                max_z_err,
                mask_bytes,
                Some(uses_no_data),
                no_data_values,
                version,
            )
            .map(Some);
        }
    }

    let prepared_nan = prepare_nan_encode_inputs(spec, data, mask_bytes)?;
    let (spec, data, mask_bytes) = if let Some(prepared) = prepared_nan.as_ref() {
        (
            prepared.spec,
            prepared.data.as_slice(),
            Some(prepared.masks.as_slice()),
        )
    } else {
        (spec, data, mask_bytes)
    };

    match encode_lerc2_auto(spec, data, max_z_err, mask_bytes, version) {
        Ok(blob) => Ok(Some(blob)),
        Err(LercError::WrongParam("one-sweep Lerc2 encode requires version 2 or newer")) => {
            Ok(None)
        }
        Err(LercError::WrongParam("pre-v4 Lerc2 encode can only store depth 1")) => Ok(None),
        Err(err) => Err(err),
    }
}

struct PreparedNanEncode {
    spec: EncodeSpec,
    data: Vec<u8>,
    masks: Vec<u8>,
}

fn prepare_nan_encode_inputs(
    spec: EncodeSpec,
    data: &[u8],
    mask_bytes: Option<&[u8]>,
) -> crate::Result<Option<PreparedNanEncode>> {
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

    let mut prepared_data = data.to_vec();
    let mut prepared_masks = vec![1u8; n_pixels * spec.n_bands];
    let mut found_nan = false;

    for i_band in 0..spec.n_bands {
        let data_band_offset = i_band * band_value_bytes;
        let mask_band_offset = i_band * n_pixels;
        let input_mask = match (spec.n_masks, mask_bytes) {
            (0, _) => None,
            (1, Some(masks)) => Some(&masks[..n_pixels]),
            (_, Some(masks)) => {
                let offset = i_band * n_pixels;
                Some(&masks[offset..offset + n_pixels])
            }
            (_, None) => None,
        };

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

fn prepare_active_no_data_nan_encode_inputs(
    spec: EncodeSpec,
    data: &[u8],
    mask_bytes: Option<&[u8]>,
    uses_no_data: &[u8],
    no_data_values: Option<&[f64]>,
) -> crate::Result<Option<PreparedNanEncode>> {
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
        let input_mask = match (spec.n_masks, mask_bytes) {
            (0, _) => None,
            (1, Some(masks)) => Some(&masks[..n_pixels]),
            (_, Some(masks)) => {
                let offset = i_band * n_pixels;
                Some(&masks[offset..offset + n_pixels])
            }
            (_, None) => None,
        };

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

fn validate_float_no_data_value_for_encode(data_type: DataType, value: f64) -> crate::Result<()> {
    if data_type == DataType::Float && (value < f32::MIN as f64 || value > f32::MAX as f64) {
        return Err(LercError::WrongParam(
            "active no-data value is outside the data type range",
        ));
    }
    Ok(())
}

fn validate_no_data_inputs(
    p_uses_no_data: Option<*const u8>,
    no_data_values: Option<*const f64>,
) -> bool {
    if let Some(values) = no_data_values {
        !values.is_null() || p_uses_no_data.is_some()
    } else {
        true
    }
}

unsafe fn encode_uses_no_data_slice(
    n_bands: usize,
    p_uses_no_data: Option<*const u8>,
) -> Option<&'static [u8]> {
    let Some(ptr) = p_uses_no_data else {
        return None;
    };
    if ptr.is_null() {
        return None;
    }

    Some(unsafe { slice::from_raw_parts(ptr, n_bands) })
}

unsafe fn lerc_get_blob_info_impl(
    p_lerc_blob: *const u8,
    blob_size: u32,
    info_array: *mut u32,
    data_range_array: *mut f64,
    info_array_size: i32,
    data_range_array_size: i32,
) -> u32 {
    if p_lerc_blob.is_null()
        || blob_size == 0
        || (info_array.is_null() && data_range_array.is_null())
        || (info_array_size <= 0 && data_range_array_size <= 0)
    {
        return ErrCode::WrongParam as u32;
    }

    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    let mut info_slice = if !info_array.is_null() && info_array_size > 0 {
        Some(unsafe { slice::from_raw_parts_mut(info_array, info_array_size as usize) })
    } else {
        None
    };
    let mut range_slice = if !data_range_array.is_null() && data_range_array_size > 0 {
        Some(unsafe { slice::from_raw_parts_mut(data_range_array, data_range_array_size as usize) })
    } else {
        None
    };

    match get_lerc2_blob_info_arrays(blob, info_slice.as_deref_mut(), range_slice.as_deref_mut()) {
        Ok(_) => ErrCode::Ok as u32,
        Err(err) => err.err_code() as u32,
    }
}

unsafe fn lerc_get_data_ranges_impl(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_depth: i32,
    n_bands: i32,
    p_mins: *mut f64,
    p_maxs: *mut f64,
) -> u32 {
    if p_lerc_blob.is_null()
        || blob_size == 0
        || p_mins.is_null()
        || p_maxs.is_null()
        || n_depth <= 0
        || n_bands <= 0
    {
        return ErrCode::WrongParam as u32;
    }

    let capacity = match (n_depth as usize).checked_mul(n_bands as usize) {
        Some(capacity) => capacity,
        None => return ErrCode::BufferTooSmall as u32,
    };
    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    let info = match get_lerc_info(blob) {
        Ok(info) => info,
        Err(err) => return err.err_code() as u32,
    };
    let required_capacity = match (info.n_depth as usize).checked_mul(info.n_bands as usize) {
        Some(capacity) => capacity,
        None => return ErrCode::BufferTooSmall as u32,
    };
    if capacity < required_capacity {
        return ErrCode::BufferTooSmall as u32;
    }

    match get_lerc2_data_ranges(blob) {
        Ok(ranges) => {
            if ranges.mins.len() > capacity || ranges.maxs.len() > capacity {
                return ErrCode::BufferTooSmall as u32;
            }

            let mins = unsafe { slice::from_raw_parts_mut(p_mins, capacity) };
            let maxs = unsafe { slice::from_raw_parts_mut(p_maxs, capacity) };
            mins[..ranges.mins.len()].copy_from_slice(&ranges.mins);
            maxs[..ranges.maxs.len()].copy_from_slice(&ranges.maxs);
            ErrCode::Ok as u32
        }
        Err(err) => err.err_code() as u32,
    }
}

unsafe fn lerc_decode_impl(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    data_type: u32,
    p_data: *mut c_void,
) -> u32 {
    if p_lerc_blob.is_null()
        || blob_size == 0
        || p_data.is_null()
        || n_depth <= 0
        || n_cols <= 0
        || n_rows <= 0
        || n_bands <= 0
        || !(n_masks == 0 || n_masks == 1 || n_masks == n_bands)
        || (n_masks > 0 && p_valid_bytes.is_null())
    {
        return ErrCode::WrongParam as u32;
    }

    let data_type = match DataType::try_from(data_type) {
        Ok(data_type) => data_type,
        _ => return ErrCode::WrongParam as u32,
    };
    let spec = DecodeIntoSpec {
        data_type,
        n_depth: n_depth as usize,
        n_cols: n_cols as usize,
        n_rows: n_rows as usize,
        n_bands: n_bands as usize,
        n_masks: n_masks as usize,
    };

    let data_len = match spec.data_byte_len() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match spec.mask_byte_len() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };

    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    match get_lerc_info(blob) {
        Ok(info) => {
            if let Some(status) = validate_decode_request_capacity(&info, spec) {
                return status;
            }
            if info.n_uses_no_data_value > 0 && n_depth > 1 {
                return ErrCode::HasNoData as u32;
            }
            if let Some(status) = validate_decode_request_shape(&info, spec) {
                return status;
            }
        }
        Err(err) => return err.err_code() as u32,
    }

    let data_output = unsafe { slice::from_raw_parts_mut(p_data.cast::<u8>(), data_len) };
    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };

    match decode_lerc_supported_into(blob, spec, data_output, mask_output.as_deref_mut()) {
        Ok(_) => ErrCode::Ok as u32,
        Err(err) => err.err_code() as u32,
    }
}

fn validate_decode_request_capacity(info: &LercInfo, spec: DecodeIntoSpec) -> Option<u32> {
    if spec.n_masks < info.n_masks as usize || spec.n_bands > info.n_bands as usize {
        Some(ErrCode::WrongParam as u32)
    } else {
        None
    }
}

fn validate_decode_request_shape(info: &LercInfo, spec: DecodeIntoSpec) -> Option<u32> {
    if spec.n_depth != info.n_depth as usize
        || spec.n_cols != info.n_cols as usize
        || spec.n_rows != info.n_rows as usize
    {
        Some(ErrCode::Failed as u32)
    } else {
        None
    }
}

unsafe fn lerc_decode_4d_impl(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    data_type: u32,
    p_data: *mut c_void,
    p_uses_no_data: *mut u8,
    no_data_values: *mut f64,
) -> u32 {
    if p_lerc_blob.is_null()
        || blob_size == 0
        || p_data.is_null()
        || n_depth <= 0
        || n_cols <= 0
        || n_rows <= 0
        || n_bands <= 0
        || !(n_masks == 0 || n_masks == 1 || n_masks == n_bands)
        || (n_masks > 0 && p_valid_bytes.is_null())
    {
        return ErrCode::WrongParam as u32;
    }

    let data_type = match DataType::try_from(data_type) {
        Ok(data_type) => data_type,
        _ => return ErrCode::WrongParam as u32,
    };
    let spec = DecodeIntoSpec {
        data_type,
        n_depth: n_depth as usize,
        n_cols: n_cols as usize,
        n_rows: n_rows as usize,
        n_bands: n_bands as usize,
        n_masks: n_masks as usize,
    };
    let data_len = match spec.data_byte_len() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match spec.mask_byte_len() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    let info = match get_lerc_info(blob) {
        Ok(info) => info,
        Err(err) => return err.err_code() as u32,
    };
    if let Some(status) = validate_decode_request_capacity(&info, spec) {
        return status;
    }
    if info.n_uses_no_data_value > 0
        && n_depth > 1
        && (p_uses_no_data.is_null() || no_data_values.is_null())
    {
        return ErrCode::HasNoData as u32;
    }
    if info.n_uses_no_data_value > 0 && n_depth > 1 {
        unsafe {
            slice::from_raw_parts_mut(p_uses_no_data, n_bands as usize).fill(0);
            slice::from_raw_parts_mut(no_data_values, n_bands as usize).fill(0.0);
        }
    }
    if let Some(status) = validate_decode_request_shape(&info, spec) {
        return status;
    }

    let data_output = unsafe { slice::from_raw_parts_mut(p_data.cast::<u8>(), data_len) };
    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };
    let status =
        match decode_lerc_supported_into(blob, spec, data_output, mask_output.as_deref_mut()) {
            Ok(_) => ErrCode::Ok as u32,
            Err(err) => err.err_code() as u32,
        };
    if status != ErrCode::Ok as u32 {
        return status;
    }

    if info.n_uses_no_data_value > 0 && n_depth > 1 {
        write_no_data_info(
            blob,
            n_bands as usize,
            unsafe { slice::from_raw_parts_mut(p_uses_no_data, n_bands as usize) },
            unsafe { slice::from_raw_parts_mut(no_data_values, n_bands as usize) },
        )
        .map_or_else(|err| err.err_code() as u32, |_| ErrCode::Ok as u32)
    } else {
        ErrCode::Ok as u32
    }
}

unsafe fn lerc_decode_to_double_impl(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    p_data: *mut f64,
) -> u32 {
    if p_lerc_blob.is_null()
        || blob_size == 0
        || p_data.is_null()
        || n_depth <= 0
        || n_cols <= 0
        || n_rows <= 0
        || n_bands <= 0
        || !(n_masks == 0 || n_masks == 1 || n_masks == n_bands)
        || (n_masks > 0 && p_valid_bytes.is_null())
    {
        return ErrCode::WrongParam as u32;
    }

    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    let info = match get_lerc_info(blob) {
        Ok(info) => info,
        Err(err) => return err.err_code() as u32,
    };
    let spec = DecodeIntoSpec {
        data_type: info.data_type,
        n_depth: n_depth as usize,
        n_cols: n_cols as usize,
        n_rows: n_rows as usize,
        n_bands: n_bands as usize,
        n_masks: n_masks as usize,
    };
    if let Some(status) = validate_decode_request_capacity(&info, spec) {
        return status;
    }
    if info.n_uses_no_data_value > 0 && n_depth > 1 {
        return ErrCode::HasNoData as u32;
    }
    if let Some(status) = validate_decode_request_shape(&info, spec) {
        return status;
    }

    let value_count = match spec.value_count() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match spec.mask_byte_len() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };

    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };
    let output = unsafe { slice::from_raw_parts_mut(p_data, value_count) };
    match decode_lerc_supported_to_f64(blob, spec, output, mask_output.as_deref_mut()) {
        Ok(_) => ErrCode::Ok as u32,
        Err(err) => err.err_code() as u32,
    }
}

unsafe fn lerc_decode_to_double_4d_impl(
    p_lerc_blob: *const u8,
    blob_size: u32,
    n_masks: i32,
    p_valid_bytes: *mut u8,
    n_depth: i32,
    n_cols: i32,
    n_rows: i32,
    n_bands: i32,
    p_data: *mut f64,
    p_uses_no_data: *mut u8,
    no_data_values: *mut f64,
) -> u32 {
    if p_lerc_blob.is_null()
        || blob_size == 0
        || p_data.is_null()
        || n_depth <= 0
        || n_cols <= 0
        || n_rows <= 0
        || n_bands <= 0
        || !(n_masks == 0 || n_masks == 1 || n_masks == n_bands)
        || (n_masks > 0 && p_valid_bytes.is_null())
    {
        return ErrCode::WrongParam as u32;
    }

    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    let info = match get_lerc_info(blob) {
        Ok(info) => info,
        Err(err) => return err.err_code() as u32,
    };
    let spec = DecodeIntoSpec {
        data_type: info.data_type,
        n_depth: n_depth as usize,
        n_cols: n_cols as usize,
        n_rows: n_rows as usize,
        n_bands: n_bands as usize,
        n_masks: n_masks as usize,
    };
    if let Some(status) = validate_decode_request_capacity(&info, spec) {
        return status;
    }
    if info.n_uses_no_data_value > 0
        && n_depth > 1
        && (p_uses_no_data.is_null() || no_data_values.is_null())
    {
        return ErrCode::HasNoData as u32;
    }
    if info.n_uses_no_data_value > 0 && n_depth > 1 {
        unsafe {
            slice::from_raw_parts_mut(p_uses_no_data, n_bands as usize).fill(0);
            slice::from_raw_parts_mut(no_data_values, n_bands as usize).fill(0.0);
        }
    }
    if let Some(status) = validate_decode_request_shape(&info, spec) {
        return status;
    }

    let value_count = match spec.value_count() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match spec.mask_byte_len() {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };

    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };
    let output = unsafe { slice::from_raw_parts_mut(p_data, value_count) };
    if let Err(err) = decode_lerc_supported_to_f64(blob, spec, output, mask_output.as_deref_mut()) {
        return err.err_code() as u32;
    }

    if info.n_uses_no_data_value > 0 && n_depth > 1 {
        write_no_data_info(
            blob,
            n_bands as usize,
            unsafe { slice::from_raw_parts_mut(p_uses_no_data, n_bands as usize) },
            unsafe { slice::from_raw_parts_mut(no_data_values, n_bands as usize) },
        )
        .map_or_else(|err| err.err_code() as u32, |_| ErrCode::Ok as u32)
    } else {
        ErrCode::Ok as u32
    }
}

fn write_no_data_info(
    blob: &[u8],
    n_bands: usize,
    uses_no_data: &mut [u8],
    no_data_values: &mut [f64],
) -> crate::Result<()> {
    if uses_no_data.len() < n_bands || no_data_values.len() < n_bands {
        return Err(LercError::BufferTooSmall);
    }

    uses_no_data[..n_bands].fill(0);
    no_data_values[..n_bands].fill(0.0);

    let info = get_lerc2_no_data_info(blob, n_bands)?;
    uses_no_data[..n_bands].copy_from_slice(&info.uses_no_data);
    no_data_values[..n_bands].copy_from_slice(&info.no_data_values);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        lerc_computeCompressedSize, lerc_computeCompressedSizeForVersion,
        lerc_computeCompressedSize_4D, lerc_decode, lerc_decodeToDouble, lerc_decodeToDouble_4D,
        lerc_decode_4D, lerc_encode, lerc_encodeForVersion, lerc_encode_4D, lerc_getBlobInfo,
        lerc_getDataRanges, try_encode_supported_blob,
    };
    use crate::{
        compute_checksum_fletcher32, decode_lerc2_bands_supported, decode_lerc2_supported,
        encode_lerc2_auto, encode_lerc2_tiled_lut_bands, encode_lerc2_tiled_lut_bands_with_no_data,
        encode_lerc2_uncompressed, encode_lerc2_uncompressed_with_no_data,
        get_lerc2_blob_info_arrays, get_lerc2_data_ranges, get_lerc_info, DataType, DecodedData,
        ErrCode, BLOB_DATA_RANGE_ARRAY_LEN, BLOB_INFO_ARRAY_LEN,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::ptr;

    fn fixture(name: &str) -> Vec<u8> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("testData");
        path.push(name);
        fs::read(path).unwrap()
    }

    fn min_max_valid_f32(values: &[f32], mask: &[u8]) -> (f32, f32) {
        let mut z_min = f32::INFINITY;
        let mut z_max = f32::NEG_INFINITY;
        for (&value, &valid) in values.iter().zip(mask.iter()) {
            if valid != 0 {
                z_min = z_min.min(value);
                z_max = z_max.max(value);
            }
        }
        (z_min, z_max)
    }

    fn min_max_valid_f64(values: &[f64], mask: &[u8]) -> (f64, f64) {
        let mut z_min = f64::INFINITY;
        let mut z_max = f64::NEG_INFINITY;
        for (&value, &valid) in values.iter().zip(mask.iter()) {
            if valid != 0 {
                z_min = z_min.min(value);
                z_max = z_max.max(value);
            }
        }
        (z_min, z_max)
    }

    fn append_test_value(bytes: &mut Vec<u8>, data_type: DataType, value: f64) {
        match data_type {
            DataType::Char => bytes.push(value as i8 as u8),
            DataType::UChar => bytes.push(value as u8),
            DataType::Short => bytes.extend_from_slice(&(value as i16).to_le_bytes()),
            DataType::UShort => bytes.extend_from_slice(&(value as u16).to_le_bytes()),
            DataType::Int => bytes.extend_from_slice(&(value as i32).to_le_bytes()),
            DataType::UInt => bytes.extend_from_slice(&(value as u32).to_le_bytes()),
            DataType::Float => bytes.extend_from_slice(&(value as f32).to_le_bytes()),
            DataType::Double => bytes.extend_from_slice(&value.to_le_bytes()),
        }
    }

    #[test]
    fn c_abi_get_blob_info_fills_arrays() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut info = [123u32; BLOB_INFO_ARRAY_LEN];
        let mut ranges = [123.0f64; BLOB_DATA_RANGE_ARRAY_LEN];

        let status = unsafe {
            lerc_getBlobInfo(
                blob.as_ptr(),
                blob.len() as u32,
                info.as_mut_ptr(),
                ranges.as_mut_ptr(),
                info.len() as i32,
                ranges.len() as i32,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(
            info,
            [
                3,
                DataType::UChar as u32,
                1,
                256,
                256,
                3,
                43_008,
                blob.len() as u32,
                1,
                1,
                0,
            ]
        );
        assert_eq!(ranges, [0.0, 255.0, 0.5]);
    }

    #[test]
    fn c_abi_get_blob_info_fills_legacy_lerc1_arrays() {
        let blob = fixture("world.lerc1");
        let mut info = [123u32; BLOB_INFO_ARRAY_LEN];
        let mut ranges = [123.0f64; BLOB_DATA_RANGE_ARRAY_LEN];

        let status = unsafe {
            lerc_getBlobInfo(
                blob.as_ptr(),
                blob.len() as u32,
                info.as_mut_ptr(),
                ranges.as_mut_ptr(),
                info.len() as i32,
                ranges.len() as i32,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(
            info,
            [
                0,
                DataType::Float as u32,
                1,
                257,
                257,
                1,
                65_025,
                blob.len() as u32,
                1,
                1,
                0,
            ]
        );
        assert_eq!(ranges, [-27.458_635_330_200_195, 5474.172_851_562_5, 0.1]);
    }

    #[test]
    fn c_abi_get_blob_info_matches_safe_helper_for_metadata_fixtures() {
        for fixture_name in [
            "bluemarble_256_256_3_byte.lerc2",
            "california_400_400_1_float.lerc2",
            "world.lerc1",
        ] {
            let blob = fixture(fixture_name);
            let mut expected_info = [0u32; BLOB_INFO_ARRAY_LEN];
            let mut expected_ranges = [0.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
            get_lerc2_blob_info_arrays(&blob, Some(&mut expected_info), Some(&mut expected_ranges))
                .unwrap();

            let mut info = [123u32; BLOB_INFO_ARRAY_LEN];
            let mut ranges = [123.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
            let status = unsafe {
                lerc_getBlobInfo(
                    blob.as_ptr(),
                    blob.len() as u32,
                    info.as_mut_ptr(),
                    ranges.as_mut_ptr(),
                    info.len() as i32,
                    ranges.len() as i32,
                )
            };

            assert_eq!(status, ErrCode::Ok as u32, "{fixture_name}");
            assert_eq!(info, expected_info, "{fixture_name}");
            assert_eq!(ranges, expected_ranges, "{fixture_name}");
        }
    }

    #[test]
    fn c_abi_metadata_matches_cpp_fixture_expectations() {
        for (
            fixture_name,
            expected_info,
            expected_ranges,
            n_depth,
            n_bands,
            expected_mins,
            expected_maxs,
        ) in [
            (
                "california_400_400_1_float.lerc2",
                [3, 6, 1, 400, 400, 1, 58515, 176451, 1, 1, 0],
                [-82.972_091_674_804_69, 4080.613_769_531_25, 0.000_075],
                1,
                1,
                vec![-82.972_091_674_804_69],
                vec![4080.613_769_531_25],
            ),
            (
                "bluemarble_256_256_3_byte.lerc2",
                [3, 1, 1, 256, 256, 3, 43008, 56389, 1, 1, 0],
                [0.0, 255.0, 0.5],
                1,
                3,
                vec![0.0, 0.0, 0.0],
                vec![255.0, 255.0, 255.0],
            ),
            (
                "world.lerc1",
                [0, 6, 1, 257, 257, 1, 65025, 63518, 1, 1, 0],
                [-27.458_635_330_200_195, 5474.172_851_562_5, 0.1],
                1,
                1,
                vec![-27.458_635_330_200_195],
                vec![5474.172_851_562_5],
            ),
        ] {
            let blob = fixture(fixture_name);
            let mut info = [0u32; BLOB_INFO_ARRAY_LEN];
            let mut ranges = [0.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
            let status = unsafe {
                lerc_getBlobInfo(
                    blob.as_ptr(),
                    blob.len() as u32,
                    info.as_mut_ptr(),
                    ranges.as_mut_ptr(),
                    info.len() as i32,
                    ranges.len() as i32,
                )
            };
            assert_eq!(status, ErrCode::Ok as u32, "{fixture_name}");
            assert_eq!(info, expected_info, "{fixture_name}");
            assert_eq!(ranges, expected_ranges, "{fixture_name}");

            let mut mins = vec![123.0; expected_mins.len()];
            let mut maxs = vec![123.0; expected_maxs.len()];
            let status = unsafe {
                lerc_getDataRanges(
                    blob.as_ptr(),
                    blob.len() as u32,
                    n_depth,
                    n_bands,
                    mins.as_mut_ptr(),
                    maxs.as_mut_ptr(),
                )
            };
            assert_eq!(status, ErrCode::Ok as u32, "{fixture_name}");
            assert_eq!(mins, expected_mins, "{fixture_name}");
            assert_eq!(maxs, expected_maxs, "{fixture_name}");
        }
    }

    #[test]
    fn c_abi_get_blob_info_rejects_invalid_arguments() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut info = [0u32; BLOB_INFO_ARRAY_LEN];

        let status = unsafe {
            lerc_getBlobInfo(
                ptr::null(),
                blob.len() as u32,
                info.as_mut_ptr(),
                ptr::null_mut(),
                info.len() as i32,
                0,
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_getBlobInfo(
                blob.as_ptr(),
                blob.len() as u32,
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                0,
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
    }

    #[test]
    fn c_abi_get_data_ranges_fills_arrays() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut mins = [123.0f64; 3];
        let mut maxs = [123.0f64; 3];

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                3,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mins, [0.0, 0.0, 0.0]);
        assert_eq!(maxs, [255.0, 255.0, 255.0]);
    }

    #[test]
    fn c_abi_get_data_ranges_fills_legacy_lerc1_arrays() {
        let blob = fixture("world.lerc1");
        let mut mins = [123.0f64; 1];
        let mut maxs = [123.0f64; 1];

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                1,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mins, [-27.458_635_330_200_195]);
        assert_eq!(maxs, [5474.172_851_562_5]);
    }

    #[test]
    fn c_abi_get_data_ranges_matches_safe_helper_for_metadata_fixtures() {
        for (fixture_name, n_depth, n_bands) in [
            ("bluemarble_256_256_3_byte.lerc2", 1, 3),
            ("california_400_400_1_float.lerc2", 1, 1),
            ("world.lerc1", 1, 1),
        ] {
            let blob = fixture(fixture_name);
            let expected = get_lerc2_data_ranges(&blob).unwrap();
            let mut mins = vec![123.0f64; expected.mins.len()];
            let mut maxs = vec![123.0f64; expected.maxs.len()];

            let status = unsafe {
                lerc_getDataRanges(
                    blob.as_ptr(),
                    blob.len() as u32,
                    n_depth,
                    n_bands,
                    mins.as_mut_ptr(),
                    maxs.as_mut_ptr(),
                )
            };

            assert_eq!(status, ErrCode::Ok as u32, "{fixture_name}");
            assert_eq!(mins, expected.mins, "{fixture_name}");
            assert_eq!(maxs, expected.maxs, "{fixture_name}");
        }
    }

    #[test]
    fn c_abi_get_data_ranges_rejects_invalid_arguments() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let mut mins = [0.0f64; 3];
        let mut maxs = [0.0f64; 3];

        let status = unsafe {
            lerc_getDataRanges(
                ptr::null(),
                blob.len() as u32,
                1,
                3,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                3,
                ptr::null_mut(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                0,
                3,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                2,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::BufferTooSmall as u32);
    }

    #[test]
    fn c_abi_get_data_ranges_validates_capacity_before_no_data_status() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let mut mins = [123.0f64; 2];
        let mut maxs = [123.0f64; 2];

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                1,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::BufferTooSmall as u32);
        assert_eq!(mins, [123.0, 123.0]);
        assert_eq!(maxs, [123.0, 123.0]);

        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                2,
                1,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::HasNoData as u32);
    }

    #[test]
    fn c_abi_encode_supports_pre_v4_one_sweep_version() {
        let data = [1u8, 2, 3, 4, 5, 6];
        let mut num_bytes = 123u32;
        let mut out = [0u8; 128];
        let mut written = 123u32;

        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                3,
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert!(num_bytes > 0);
        let expected_num_bytes = num_bytes;

        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                3,
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(written, expected_num_bytes);

        let info = get_lerc_info(&out[..written as usize]).unwrap();
        assert_eq!(info.version, 3);
        assert_eq!(info.n_depth, 1);
        assert_eq!(info.n_cols, 3);
        assert_eq!(info.n_rows, 2);
    }

    #[test]
    fn c_abi_versioned_encode_rejects_invalid_codec_versions() {
        let data = [1u8, 2, 3, 4, 5, 6];
        let mut out = [99u8; 128];

        for version in [0, 1, 7] {
            out.fill(99);
            let mut num_bytes = 123u32;
            let status = unsafe {
                lerc_computeCompressedSizeForVersion(
                    data.as_ptr().cast(),
                    version,
                    DataType::UChar as u32,
                    1,
                    3,
                    2,
                    1,
                    0,
                    ptr::null(),
                    0.0,
                    &mut num_bytes,
                )
            };
            assert_eq!(status, ErrCode::WrongParam as u32);
            assert_eq!(num_bytes, 0);

            let mut written = 123u32;
            let status = unsafe {
                lerc_encodeForVersion(
                    data.as_ptr().cast(),
                    version,
                    DataType::UChar as u32,
                    1,
                    3,
                    2,
                    1,
                    0,
                    ptr::null(),
                    0.0,
                    out.as_mut_ptr(),
                    out.len() as u32,
                    &mut written,
                )
            };
            assert_eq!(status, ErrCode::WrongParam as u32);
            assert_eq!(written, 0);
            assert_eq!(out, [0; 128]);
        }
    }

    #[test]
    fn c_abi_compute_size_supports_constant_encode() {
        let data = [7u8; 12];
        let valid = [1, 0, 1, 1, 1, 1];
        let mut num_bytes = 0u32;

        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                1,
                valid.as_ptr(),
                0.5,
                &mut num_bytes,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert!(num_bytes > 0);
    }

    #[test]
    fn c_abi_compute_size_supports_one_sweep_encode() {
        let data = [1u8, 2, 99, 99, 3, 9, 5, 6, 7, 8, 0, 0];
        let valid = [1, 0, 1, 1, 1, 0];
        let mut num_bytes = 0u32;

        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                1,
                valid.as_ptr(),
                0.5,
                &mut num_bytes,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert!(num_bytes > 0);
    }

    #[test]
    fn c_abi_encode_supports_constant_input() {
        let data = [7u8; 12];
        let valid = [1, 0, 1, 1, 1, 1];
        let mut out = [0u8; 128];
        let mut written = 0u32;

        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                1,
                valid.as_ptr(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.header.num_valid_pixel, 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![7, 7, 0, 0, 7, 7, 7, 7, 7, 7, 7, 7])
        );
    }

    #[test]
    fn c_abi_encode_supports_one_sweep_input() {
        let data = [1u8, 2, 99, 99, 3, 9, 5, 6, 7, 8, 0, 0];
        let valid = [1, 0, 1, 1, 1, 0];
        let mut out = [0u8; 160];
        let mut written = 0u32;

        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                1,
                valid.as_ptr(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.header.num_valid_pixel, 4);
        assert_eq!(decoded.header.z_min, 1.0);
        assert_eq!(decoded.header.z_max, 9.0);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 9, 5, 6, 7, 8, 0, 0])
        );
    }

    #[test]
    fn c_abi_encode_matches_safe_auto_selector() {
        let data = [1u8, 2, 99, 99, 3, 9, 5, 6, 7, 8, 0, 0];
        let valid = [1, 0, 1, 1, 1, 0];
        let mut out = [0u8; 160];
        let mut written = 0u32;

        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                1,
                valid.as_ptr(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };
        let expected = encode_lerc2_auto(
            crate::EncodeSpec {
                data_type: DataType::UChar,
                n_depth: 2,
                n_cols: 3,
                n_rows: 2,
                n_bands: 1,
                n_masks: 1,
            },
            &data,
            0.5,
            Some(&valid),
            6,
        )
        .unwrap();

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(&out[..written as usize], expected.as_slice());
    }

    #[test]
    fn c_abi_encode_selects_byte_huffman_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 64,
            n_rows: 64,
            n_bands: 1,
            n_masks: 0,
        };
        let data: Vec<u8> = (0..(spec.n_cols * spec.n_rows))
            .map(|idx| (idx % 64) as u8)
            .collect();
        let uncompressed = crate::encode_lerc2_uncompressed(spec, &data, 0.5, None, 6).unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                1,
                64,
                64,
                1,
                0,
                ptr::null(),
                0.5,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                1,
                64,
                64,
                1,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size, written);
        assert!((written as usize) < uncompressed.len());
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.data, DecodedData::UChar(data));
    }

    #[test]
    fn c_abi_encode_selects_float_huffman_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::Float,
            n_depth: 1,
            n_cols: 128,
            n_rows: 64,
            n_bands: 1,
            n_masks: 0,
        };
        let mut values = Vec::with_capacity(spec.n_cols * spec.n_rows);
        let mut data = Vec::with_capacity(spec.n_cols * spec.n_rows * 4);
        for idx in 0..(spec.n_cols * spec.n_rows) {
            let value = ((idx % 32) as f32) * 0.25;
            values.push(value);
            data.extend_from_slice(&value.to_le_bytes());
        }
        let uncompressed = crate::encode_lerc2_uncompressed(spec, &data, 0.0, None, 6).unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                128,
                64,
                1,
                0,
                ptr::null(),
                0.0,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                128,
                64,
                1,
                0,
                ptr::null(),
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size, written);
        assert!((written as usize) < uncompressed.len());
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.data, DecodedData::Float(values));
    }

    #[test]
    fn c_abi_4d_encode_selects_float_huffman_with_no_data_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::Float,
            n_depth: 2,
            n_cols: 128,
            n_rows: 64,
            n_bands: 1,
            n_masks: 0,
        };
        let n_pixels = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(n_pixels * spec.n_depth * 4);
        let mut expected = Vec::with_capacity(n_pixels * spec.n_depth);
        for pixel in 0..n_pixels {
            if pixel % 97 == 0 {
                data.extend_from_slice(&(-9999.0f32).to_le_bytes());
                data.extend_from_slice(&(-9999.0f32).to_le_bytes());
                expected.extend_from_slice(&[-9999.0, -9999.0]);
            } else if pixel % 89 == 0 {
                data.extend_from_slice(&((pixel % 32) as f32).to_le_bytes());
                data.extend_from_slice(&(-9999.0f32).to_le_bytes());
                expected.extend_from_slice(&[(pixel % 32) as f32, -9999.0]);
            } else {
                let value = ((pixel % 32) as f32) * 0.25;
                data.extend_from_slice(&value.to_le_bytes());
                data.extend_from_slice(&(value + 1.0).to_le_bytes());
                expected.extend_from_slice(&[value, value + 1.0]);
            }
        }
        let uses_no_data = [1u8];
        let no_data_values = [-9999.0f64];
        let uncompressed = crate::encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.0,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
        )
        .unwrap();
        let selected = crate::encode_lerc2_auto_with_no_data(
            spec,
            &data,
            0.0,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
        )
        .unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                spec.n_bands as i32,
                0,
                ptr::null(),
                0.0,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        let encode_status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                spec.n_bands as i32,
                0,
                ptr::null(),
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size as usize, selected.len());
        assert_eq!(written as usize, selected.len());
        assert!((written as usize) < uncompressed.len());
        assert_eq!(&out[..written as usize], selected.as_slice());
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, -9999.0);
        assert!(!decoded.mask.is_valid(0).unwrap());
        assert_eq!(decoded.data, DecodedData::Float(expected));
    }

    #[test]
    fn c_abi_4d_encode_selects_multi_band_float_huffman_with_no_data_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::Float,
            n_depth: 2,
            n_cols: 128,
            n_rows: 64,
            n_bands: 2,
            n_masks: 0,
        };
        let n_pixels = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(n_pixels * spec.n_depth * spec.n_bands * 4);
        let mut expected_band0 = Vec::with_capacity(n_pixels * spec.n_depth);
        let mut expected_band1 = Vec::with_capacity(n_pixels * spec.n_depth);
        for pixel in 0..n_pixels {
            if pixel % 97 == 0 {
                data.extend_from_slice(&(-9999.0f32).to_le_bytes());
                data.extend_from_slice(&(-9999.0f32).to_le_bytes());
                expected_band0.extend_from_slice(&[-9999.0, -9999.0]);
            } else if pixel % 89 == 0 {
                data.extend_from_slice(&((pixel % 32) as f32).to_le_bytes());
                data.extend_from_slice(&(-9999.0f32).to_le_bytes());
                expected_band0.extend_from_slice(&[(pixel % 32) as f32, -9999.0]);
            } else {
                let value = ((pixel % 32) as f32) * 0.25;
                data.extend_from_slice(&value.to_le_bytes());
                data.extend_from_slice(&(value + 1.0).to_le_bytes());
                expected_band0.extend_from_slice(&[value, value + 1.0]);
            }
        }
        for pixel in 0..n_pixels {
            let value = 100.0 + ((pixel % 64) as f32) * 0.125;
            data.extend_from_slice(&value.to_le_bytes());
            data.extend_from_slice(&(value + 2.0).to_le_bytes());
            expected_band1.extend_from_slice(&[value, value + 2.0]);
        }
        let uses_no_data = [1u8, 0];
        let no_data_values = [-9999.0f64, 0.0];
        let uncompressed = crate::encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.0,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
        )
        .unwrap();
        let selected = crate::encode_lerc2_auto_with_no_data(
            spec,
            &data,
            0.0,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
        )
        .unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                spec.n_bands as i32,
                0,
                ptr::null(),
                0.0,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        let encode_status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                spec.n_bands as i32,
                0,
                ptr::null(),
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size as usize, selected.len());
        assert_eq!(written as usize, selected.len());
        assert!((written as usize) < uncompressed.len());
        assert_eq!(&out[..written as usize], selected.as_slice());
        let decoded = decode_lerc2_bands_supported(&out[..written as usize]).unwrap();
        assert!(decoded.bands[0].header.has_no_data_values());
        assert!(!decoded.bands[1].header.has_no_data_values());
        assert!(!decoded.bands[0].mask.is_valid(0).unwrap());
        assert_eq!(decoded.bands[0].data, DecodedData::Float(expected_band0));
        assert_eq!(decoded.bands[1].data, DecodedData::Float(expected_band1));
    }

    #[test]
    fn c_abi_encode_selects_byte_huffman_bands_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 64,
            n_rows: 64,
            n_bands: 2,
            n_masks: 0,
        };
        let band_len = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(band_len * 2);
        data.extend((0..band_len).map(|idx| (idx % 64) as u8));
        data.extend((0..band_len).map(|idx| (128 + idx % 64) as u8));
        let uncompressed = crate::encode_lerc2_uncompressed(spec, &data, 0.5, None, 6).unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                1,
                64,
                64,
                2,
                0,
                ptr::null(),
                0.5,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                1,
                64,
                64,
                2,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size, written);
        assert!((written as usize) < uncompressed.len());
        let decoded = decode_lerc2_bands_supported(&out[..written as usize]).unwrap();
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(data[..band_len].to_vec())
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(data[band_len..].to_vec())
        );
    }

    #[test]
    fn c_abi_encode_selects_lut_tiled_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 1,
            n_cols: 32,
            n_rows: 16,
            n_bands: 1,
            n_masks: 0,
        };
        let data: Vec<u16> = (0..(spec.n_cols * spec.n_rows))
            .map(|idx| (100 + idx / 8) as u16)
            .collect();
        let data_bytes = data
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let uncompressed = encode_lerc2_uncompressed(spec, &data_bytes, 0.5, None, 6).unwrap();
        let tiled = encode_lerc2_tiled_lut_bands(spec, &data_bytes, 0.5, None, 6, 8).unwrap();
        let expected = encode_lerc2_auto(spec, &data_bytes, 0.5, None, 6).unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UShort as u32,
                1,
                spec.n_cols as i32,
                spec.n_rows as i32,
                1,
                0,
                ptr::null(),
                0.5,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UShort as u32,
                1,
                spec.n_cols as i32,
                spec.n_rows as i32,
                1,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert!(tiled.len() < uncompressed.len());
        assert_eq!(expected, tiled);
        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size as usize, tiled.len());
        assert_eq!(written as usize, tiled.len());
        assert_eq!(&out[..written as usize], expected.as_slice());
        let decoded = decode_lerc2_bands_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.bands[0].data, DecodedData::UShort(data));
    }

    #[test]
    fn c_abi_4d_encode_selects_byte_huffman_with_no_data_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 64,
            n_rows: 32,
            n_bands: 1,
            n_masks: 0,
        };
        let n_pixels = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(n_pixels * spec.n_depth);
        let mut expected = Vec::with_capacity(n_pixels * spec.n_depth);
        for pixel in 0..n_pixels {
            if pixel % 17 == 0 {
                data.extend_from_slice(&[255, 255]);
                expected.extend_from_slice(&[0, 0]);
            } else {
                let value = (pixel % 16) as u8;
                data.extend_from_slice(&[value, value.wrapping_add(3)]);
                expected.extend_from_slice(&[value, value.wrapping_add(3)]);
            }
        }
        let uses_no_data = [1u8];
        let no_data_values = [255.0f64];
        let uncompressed = crate::encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.5,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
        )
        .unwrap();
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                spec.n_bands as i32,
                0,
                ptr::null(),
                0.5,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        let encode_status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                spec.n_bands as i32,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size, written);
        assert!((written as usize) < uncompressed.len());
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(decoded.data, DecodedData::UChar(expected));
    }

    #[test]
    fn c_abi_4d_encode_selects_lut_tiled_with_no_data_when_smaller() {
        let spec = crate::EncodeSpec {
            data_type: DataType::UShort,
            n_depth: 2,
            n_cols: 16,
            n_rows: 16,
            n_bands: 1,
            n_masks: 0,
        };
        let n_pixels = spec.n_cols * spec.n_rows;
        let mut data = Vec::with_capacity(n_pixels * spec.n_depth);
        let mut expected = Vec::with_capacity(n_pixels * spec.n_depth);
        for pixel in 0..n_pixels {
            if pixel % 29 == 0 {
                data.extend_from_slice(&[u16::MAX, u16::MAX]);
                expected.extend_from_slice(&[0, 0]);
            } else if pixel % 31 == 0 {
                data.extend_from_slice(&[u16::MAX, 25]);
                expected.extend_from_slice(&[u16::MAX, 25]);
            } else {
                let first = if pixel % 5 == 0 { 200u16 } else { 20u16 };
                data.extend_from_slice(&[first, first + 3]);
                expected.extend_from_slice(&[first, first + 3]);
            }
        }
        let data_bytes = data
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let uses_no_data = [1u8];
        let no_data_values = [u16::MAX as f64];
        let uncompressed = encode_lerc2_uncompressed_with_no_data(
            spec,
            &data_bytes,
            0.5,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
        )
        .unwrap();
        let tiled = encode_lerc2_tiled_lut_bands_with_no_data(
            spec,
            &data_bytes,
            0.5,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
            8,
        )
        .unwrap();
        let tiled16 = encode_lerc2_tiled_lut_bands_with_no_data(
            spec,
            &data_bytes,
            0.5,
            None,
            Some(&uses_no_data),
            Some(&no_data_values),
            6,
            16,
        )
        .unwrap();
        let tiled = if tiled16.len() < tiled.len() {
            tiled16
        } else {
            tiled
        };
        let mut out = vec![0u8; uncompressed.len()];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UShort as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                1,
                0,
                ptr::null(),
                0.5,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        let encode_status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UShort as u32,
                spec.n_depth as i32,
                spec.n_cols as i32,
                spec.n_rows as i32,
                1,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert!(tiled.len() < uncompressed.len());
        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size as usize, tiled.len());
        assert_eq!(written as usize, tiled.len());
        assert_eq!(&out[..written as usize], tiled.as_slice());
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, u16::MAX as f64);
        assert_eq!(decoded.data, DecodedData::UShort(expected));
    }

    #[test]
    fn c_abi_4d_encode_supports_single_depth_no_data_as_mask() {
        let data = [1u8, 255, 3, 4, 5, 255, 7, 8];
        let uses_no_data = [1u8];
        let no_data_values = [255.0f64];
        let mut computed_size = 0u32;
        let mut out = [0u8; 256];
        let mut written = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                4,
                2,
                1,
                0,
                ptr::null(),
                0.5,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        let encode_status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                4,
                2,
                1,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size, written);
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert!(!decoded.header.has_no_data_values());
        assert_eq!(decoded.mask.to_byte_mask(), [1, 0, 1, 1, 1, 0, 1, 1]);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 0, 3, 4, 5, 0, 7, 8])
        );
    }

    #[test]
    fn c_abi_encode_supports_one_sweep_multi_band_input() {
        let data = [1u8, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let valid = [1u8, 0, 1, 1, 1, 0];
        let mut out = [0u8; 256];
        let mut written = 0u32;

        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                1,
                3,
                2,
                2,
                1,
                valid.as_ptr(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.header.n_blobs_more, 1);
        let decoded_bands = crate::decode_lerc2_bands_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded_bands.bands.len(), 2);
        assert_eq!(
            decoded_bands.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded_bands.bands[1].data,
            DecodedData::UChar(vec![10, 0, 30, 50, 70, 0])
        );
    }

    #[test]
    fn c_abi_encode_supports_pre_v4_one_sweep_multi_band_input() {
        let values = [1u16, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let valid = [1u8, 0, 1, 1, 1, 0];
        let mut computed_size = 0u32;
        let mut out = [0u8; 512];
        let mut written = 0u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                3,
                DataType::UShort as u32,
                1,
                3,
                2,
                2,
                1,
                valid.as_ptr(),
                0.0,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                3,
                DataType::UShort as u32,
                1,
                3,
                2,
                2,
                1,
                valid.as_ptr(),
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(size_status, ErrCode::Ok as u32);
        assert_eq!(encode_status, ErrCode::Ok as u32);
        assert_eq!(computed_size, written);
        let info = get_lerc_info(&out[..written as usize]).unwrap();
        assert_eq!(info.version, 3);
        assert_eq!(info.n_depth, 1);
        assert_eq!(info.n_bands, 2);
        assert_eq!(info.data_type, DataType::UShort);

        let decoded_bands = crate::decode_lerc2_bands_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded_bands.bands.len(), 2);
        assert_eq!(
            decoded_bands.bands[0].data,
            DecodedData::UShort(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded_bands.bands[1].data,
            DecodedData::UShort(vec![10, 0, 30, 50, 70, 0])
        );
    }

    #[test]
    fn c_abi_encode_rejects_integer_negative_max_z_error() {
        let values = [100u16, 101, 103, 106, 110, 115, 121, 128];
        let data = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut computed_size = 123u32;
        let mut out = [0u8; 512];
        let mut written = 123u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UShort as u32,
                1,
                4,
                2,
                1,
                0,
                std::ptr::null(),
                -0.2,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UShort as u32,
                1,
                4,
                2,
                1,
                0,
                std::ptr::null(),
                -0.2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(size_status, ErrCode::WrongParam as u32);
        assert_eq!(encode_status, ErrCode::WrongParam as u32);
        assert_eq!(computed_size, 0);
        assert_eq!(written, 0);
    }

    #[test]
    fn c_abi_encode_rejects_float_negative_max_z_error() {
        let data = [1.0f32, 1.25, 1.5, 1.75]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let mut computed_size = 123u32;
        let mut out = [0u8; 128];
        let mut written = 123u32;

        let size_status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                2,
                2,
                1,
                0,
                std::ptr::null(),
                -0.2,
                &mut computed_size,
            )
        };
        let encode_status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                2,
                2,
                1,
                0,
                std::ptr::null(),
                -0.2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(size_status, ErrCode::WrongParam as u32);
        assert_eq!(encode_status, ErrCode::WrongParam as u32);
        assert_eq!(computed_size, 0);
        assert_eq!(written, 0);
    }

    #[test]
    fn c_abi_encode_constant_reports_buffer_too_small() {
        let data = [7u8; 6];
        let mut out = [99u8; 8];
        let mut written = 123u32;

        let status = unsafe {
            lerc_encode(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };

        assert_eq!(status, ErrCode::BufferTooSmall as u32);
        assert_eq!(written, 0);
        assert_eq!(out, [0; 8]);
    }

    #[test]
    fn c_abi_encode_stubs_reject_invalid_arguments() {
        let data = [1u8, 2, 3, 4, 5, 6];
        let valid = [1u8; 6];
        let mut num_bytes = 123u32;
        let out = [0u8; 64];
        let mut written = 123u32;

        let status = unsafe {
            lerc_computeCompressedSize(
                ptr::null(),
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
        assert_eq!(num_bytes, 0);

        num_bytes = 123;
        let status = unsafe {
            lerc_computeCompressedSize(
                data.as_ptr().cast(),
                99,
                1,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
        assert_eq!(num_bytes, 0);

        let status = unsafe {
            lerc_computeCompressedSize(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                1,
                ptr::null(),
                0.0,
                &mut num_bytes,
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_encode(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                1,
                valid.as_ptr(),
                0.0,
                ptr::null_mut(),
                out.len() as u32,
                &mut written,
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
        assert_eq!(written, 0);
    }

    #[test]
    fn c_abi_4d_encode_validates_no_data_pointers() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let uses_no_data = [1u8];
        let no_uses_no_data = [0u8];
        let no_data_values = [255.0f64];
        let mut num_bytes = 123u32;
        let mut out = [0u8; 256];
        let mut written = 123u32;

        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert!(num_bytes > 0);
        let expected_num_bytes = num_bytes;

        num_bytes = 123;
        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
                uses_no_data.as_ptr(),
                ptr::null(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
        assert_eq!(num_bytes, 0);

        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(written, expected_num_bytes);

        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
                no_uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert!(num_bytes > 0);
    }

    #[test]
    fn c_abi_4d_encode_rejects_active_no_data_negative_max_z_error() {
        let data = [1u8, 255, 3, 4, 5, 255, 7, 8, 9, 10, 11, 12];
        let uses_no_data = [1u8];
        let no_data_values = [255.0f64];
        let mut num_bytes = 123u32;
        let mut out = [0u8; 256];
        let mut written = 123u32;

        let size_status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                -0.2,
                &mut num_bytes,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        let encode_status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                -0.2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(size_status, ErrCode::WrongParam as u32);
        assert_eq!(encode_status, ErrCode::WrongParam as u32);
        assert_eq!(num_bytes, 0);
        assert_eq!(written, 0);
    }

    #[test]
    fn c_abi_4d_encode_rejects_active_no_data_pre_v6_version() {
        let data = [1u8, 255, 3, 4, 5, 255];
        let uses_no_data = [1u8];
        let no_data_values = [255.0f64];
        let spec = crate::EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 1,
            n_cols: 3,
            n_rows: 2,
            n_bands: 1,
            n_masks: 0,
        };

        let err = unsafe {
            try_encode_supported_blob(
                data.as_ptr().cast(),
                spec,
                ptr::null(),
                0.5,
                5,
                Some(uses_no_data.as_ptr()),
                Some(no_data_values.as_ptr()),
            )
        }
        .unwrap_err();
        assert_eq!(err.err_code(), ErrCode::WrongParam);

        let blob = unsafe {
            try_encode_supported_blob(
                data.as_ptr().cast(),
                spec,
                ptr::null(),
                0.5,
                6,
                Some(uses_no_data.as_ptr()),
                Some(no_data_values.as_ptr()),
            )
        }
        .unwrap()
        .unwrap();
        let decoded = decode_lerc2_supported(&blob).unwrap();
        assert_eq!(decoded.header.version, 6);
        assert_eq!(decoded.data, DecodedData::UChar(vec![1, 0, 3, 4, 5, 0]));
    }

    #[test]
    fn c_abi_4d_encode_supports_no_active_no_data() {
        let data = [1u8, 99, 3, 5, 7, 0, 10, 99, 30, 50, 70, 0];
        let valid = [1u8, 0, 1, 1, 1, 0];
        let uses_no_data = [0u8; 2];
        let no_data_values = [0.0f64; 2];
        let mut out = [0u8; 256];
        let mut written = 0u32;
        let mut computed_size = 0u32;

        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                2,
                1,
                valid.as_ptr(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        let decoded = crate::decode_lerc2_bands_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.bands.len(), 2);
        assert_eq!(
            decoded.bands[0].data,
            DecodedData::UChar(vec![1, 0, 3, 5, 7, 0])
        );
        assert_eq!(
            decoded.bands[1].data,
            DecodedData::UChar(vec![10, 0, 30, 50, 70, 0])
        );

        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                2,
                1,
                valid.as_ptr(),
                0.5,
                &mut computed_size,
                uses_no_data.as_ptr(),
                ptr::null(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert!(computed_size > 0);

        written = 0;
        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                1,
                3,
                2,
                2,
                1,
                valid.as_ptr(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                ptr::null(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(written, computed_size);
    }

    #[test]
    fn c_abi_4d_encode_supports_active_no_data_metadata() {
        let data = [1u8, 2, 255, 255, 3, 4, 5, 255, 7, 8, 9, 10];
        let uses_no_data = [1u8];
        let no_data_values = [255.0f64];
        let mut out = [0u8; 192];
        let mut written = 0u32;

        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                0.5,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        let info = get_lerc_info(&out[..written as usize]).unwrap();
        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val_orig, 255.0);
        assert_eq!(decoded.mask.count_valid_bits(), 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 255, 7, 8, 9, 10])
        );
        assert_eq!(info.n_uses_no_data_value, 1);
    }

    #[test]
    fn c_abi_4d_encode_keeps_near_range_integer_no_data_sentinel() {
        let data = [1u8, 2, 5, 5, 3, 4, 5, 6, 7, 8, 9, 10];
        let uses_no_data = [1u8];
        let no_data_values = [5.0f64];
        let mut out = [0u8; 192];
        let mut written = 0u32;

        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::UChar as u32,
                2,
                3,
                2,
                1,
                0,
                ptr::null(),
                3.7,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
        assert_eq!(decoded.header.max_z_error, 0.5);
        assert!(decoded.header.has_no_data_values());
        assert_eq!(decoded.header.no_data_val, 5.0);
        assert_eq!(decoded.header.no_data_val_orig, 5.0);
        assert_eq!(decoded.mask.count_valid_bits(), 5);
        assert_eq!(
            decoded.data,
            DecodedData::UChar(vec![1, 2, 0, 0, 3, 4, 5, 6, 7, 8, 9, 10])
        );
    }

    #[test]
    fn c_abi_4d_encode_restores_no_data_for_all_native_data_types() {
        let cases = [
            (DataType::Char, -128.0),
            (DataType::UChar, u8::MAX as f64),
            (DataType::Short, i16::MIN as f64),
            (DataType::UShort, u16::MAX as f64),
            (DataType::Int, i32::MIN as f64),
            (DataType::UInt, u32::MAX as f64),
            (DataType::Float, -9999.0),
            (DataType::Double, -9999.0),
        ];

        for (data_type, no_data_value) in cases {
            let mut data = Vec::new();
            for value in [1.0, 2.0, no_data_value, no_data_value, no_data_value, 3.0] {
                append_test_value(&mut data, data_type, value);
            }
            let uses_no_data = [1u8];
            let no_data_values = [no_data_value];
            let max_z_err = if matches!(data_type, DataType::Float | DataType::Double) {
                0.0
            } else {
                0.5
            };
            let mut computed_size = 0u32;
            let mut out = [0u8; 512];
            let mut written = 0u32;

            let status = unsafe {
                lerc_computeCompressedSize_4D(
                    data.as_ptr().cast(),
                    data_type as u32,
                    2,
                    3,
                    1,
                    1,
                    0,
                    ptr::null(),
                    max_z_err,
                    &mut computed_size,
                    uses_no_data.as_ptr(),
                    no_data_values.as_ptr(),
                )
            };

            assert_eq!(status, ErrCode::Ok as u32, "{data_type:?}");

            let status = unsafe {
                lerc_encode_4D(
                    data.as_ptr().cast(),
                    data_type as u32,
                    2,
                    3,
                    1,
                    1,
                    0,
                    ptr::null(),
                    max_z_err,
                    out.as_mut_ptr(),
                    out.len() as u32,
                    &mut written,
                    uses_no_data.as_ptr(),
                    no_data_values.as_ptr(),
                )
            };

            assert_eq!(status, ErrCode::Ok as u32, "{data_type:?}");
            assert_eq!(written, computed_size, "{data_type:?}");
            let decoded = decode_lerc2_supported(&out[..written as usize]).unwrap();
            assert!(decoded.header.has_no_data_values(), "{data_type:?}");
            assert_eq!(decoded.header.no_data_val_orig, no_data_value);
            assert_eq!(decoded.mask.count_valid_bits(), 2, "{data_type:?}");

            match (data_type, decoded.data) {
                (DataType::Char, DecodedData::Char(values)) => {
                    assert_eq!(values, vec![1, 2, 0, 0, no_data_value as i8, 3]);
                }
                (DataType::UChar, DecodedData::UChar(values)) => {
                    assert_eq!(values, vec![1, 2, 0, 0, no_data_value as u8, 3]);
                }
                (DataType::Short, DecodedData::Short(values)) => {
                    assert_eq!(values, vec![1, 2, 0, 0, no_data_value as i16, 3]);
                }
                (DataType::UShort, DecodedData::UShort(values)) => {
                    assert_eq!(values, vec![1, 2, 0, 0, no_data_value as u16, 3]);
                }
                (DataType::Int, DecodedData::Int(values)) => {
                    assert_eq!(values, vec![1, 2, 0, 0, no_data_value as i32, 3]);
                }
                (DataType::UInt, DecodedData::UInt(values)) => {
                    assert_eq!(values, vec![1, 2, 0, 0, no_data_value as u32, 3]);
                }
                (DataType::Float, DecodedData::Float(values)) => {
                    assert_eq!(values, vec![1.0, 2.0, 0.0, 0.0, no_data_value as f32, 3.0]);
                }
                (DataType::Double, DecodedData::Double(values)) => {
                    assert_eq!(values, vec![1.0, 2.0, 0.0, 0.0, no_data_value, 3.0]);
                }
                (_, data) => panic!("unexpected decoded data for {data_type:?}: {data:?}"),
            }
        }
    }

    #[test]
    fn c_abi_decode_writes_data_and_mask() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0u8; 6];
        let mut mask = [0u8; 6];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(data, [7; 6]);
        assert_eq!(mask, [1; 6]);
    }

    #[test]
    fn c_abi_decode_converts_lerc2_to_requested_output_type() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0u16; 6];
        let mut mask = [0u8; 6];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                DataType::UShort as u32,
                data.as_mut_ptr().cast(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(data, [7; 6]);
        assert_eq!(mask, [1; 6]);
    }

    #[test]
    fn c_abi_decode_rejects_invalid_arguments() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0u8; 6];
        let mut mask = [0u8; 6];

        let status = unsafe {
            lerc_decode(
                ptr::null(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                ptr::null_mut(),
                1,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                2,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                99,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
    }

    #[test]
    fn c_abi_decode_shape_mismatch_returns_failed_like_cpp() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0u8; 4];
        let mut mask = [0u8; 4];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                2,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);

        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                2,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);
    }

    #[test]
    fn c_abi_decode_to_double_writes_data_and_mask() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0.0f64; 6];
        let mut mask = [0u8; 6];

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                data.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(data, [7.0; 6]);
        assert_eq!(mask, [1; 6]);
    }

    #[test]
    fn c_abi_decode_to_double_rejects_invalid_arguments() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0.0f64; 6];
        let mut mask = [0u8; 6];

        let status = unsafe {
            lerc_decodeToDouble(
                ptr::null(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                data.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                3,
                2,
                1,
                ptr::null_mut(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                ptr::null_mut(),
                1,
                3,
                2,
                1,
                data.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
    }

    #[test]
    fn c_abi_decode_to_double_shape_mismatch_returns_failed_like_cpp() {
        let blob = synthetic_v4_const_blob();
        let mut data = [0.0f64; 4];
        let mut mask = [0u8; 4];

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                2,
                2,
                1,
                data.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);

        let status = unsafe {
            lerc_decodeToDouble_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                2,
                2,
                1,
                data.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);
    }

    #[test]
    fn c_abi_decode_float_fixture_writes_data_and_mask() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let mut data = vec![0.0f32; 160_000];
        let mut mask = vec![0u8; 160_000];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                400,
                400,
                1,
                DataType::Float as u32,
                data.as_mut_ptr().cast(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 58_515);
        assert_eq!(mask[0], 0);
        assert_eq!(mask[67], 1);
        assert!((data[67] - 1443.2926).abs() < 0.0001);
        assert!((data[68] - 1330.419).abs() < 0.0001);
        assert!((data[435] - 181.57863).abs() < 0.0001);
    }

    #[test]
    fn c_abi_decode_byte_huffman_fixture_writes_data_and_mask() {
        let blob = fixture("bluemarble_256_256_3_byte.lerc2");
        let expected = decode_lerc2_bands_supported(&blob).unwrap();
        let mut expected_data = vec![0u8; expected.data_byte_len()];
        expected.write_data_le_bytes(&mut expected_data).unwrap();

        let mut data = vec![0u8; 256 * 256 * 3];
        let mut mask = vec![0u8; 256 * 256];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                256,
                256,
                3,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 43_008);
        assert_eq!(data, expected_data);
        for (idx, expected) in [
            (0, [1, 4, 19]),
            (1, [1, 4, 19]),
            (255, [1, 4, 19]),
            (256, [2, 5, 20]),
            (32_768, [2, 5, 20]),
            (43_008, [0, 0, 0]),
            (65_535, [0, 0, 0]),
        ] {
            assert_eq!(mask[idx], u8::from(expected != [0, 0, 0]));
            assert_eq!(
                [data[idx], data[256 * 256 + idx], data[2 * 256 * 256 + idx]],
                expected
            );
        }
    }

    #[test]
    fn c_abi_decode_legacy_lerc1_writes_data_and_mask() {
        let blob = fixture("world.lerc1");
        let mut data = vec![123.0f32; 257 * 257];
        let mut mask = vec![123u8; 257 * 257];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                257,
                257,
                1,
                DataType::Float as u32,
                data.as_mut_ptr().cast(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);
        assert_eq!(data[0], 0.0);
        assert_eq!(data[257 * 257 - 1], 0.0);
        let (z_min, z_max) = min_max_valid_f32(&data, &mask);
        assert_eq!(z_min, -27.458_635);
        assert_eq!(z_max, 5474.173);
        for (idx, valid, expected) in [
            (0, 0, 0.0),
            (1, 0, 0.0),
            (257, 0, 0.0),
            (1024, 1, 0.0),
            (33_025, 1, 0.0),
            (40_000, 1, 0.0),
            (24_838, 1, -27.458_635),
            (26_400, 1, 5474.173),
            (66_048, 0, 0.0),
        ] {
            assert_eq!(mask[idx], valid);
            assert!((data[idx] - expected).abs() < 0.000_5);
        }
    }

    #[test]
    fn c_abi_decode_legacy_lerc1_writes_integer_data_with_cpp_rounding() {
        let blob = fixture("world.lerc1");
        let mut data = vec![123i32; 257 * 257];
        let mut mask = vec![123u8; 257 * 257];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                257,
                257,
                1,
                DataType::Int as u32,
                data.as_mut_ptr().cast(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);

        let mut z_min = i32::MAX;
        let mut z_max = i32::MIN;
        for (&value, &valid) in data.iter().zip(mask.iter()) {
            if valid != 0 {
                z_min = z_min.min(value);
                z_max = z_max.max(value);
            }
        }
        assert_eq!(z_min, -27);
        assert_eq!(z_max, 5474);
    }

    #[test]
    fn c_abi_decode_to_double_float_fixture_writes_data_and_mask() {
        let blob = fixture("california_400_400_1_float.lerc2");
        let mut data = vec![0.0f64; 160_000];
        let mut mask = vec![0u8; 160_000];

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                400,
                400,
                1,
                data.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 58_515);
        assert_eq!(mask[0], 0);
        assert_eq!(mask[67], 1);
        assert!((data[67] - 1443.2926).abs() < 0.0001);
        assert!((data[68] - 1330.419).abs() < 0.0001);
        assert!((data[435] - 181.57863).abs() < 0.0001);
    }

    #[test]
    fn c_abi_decode_to_double_legacy_lerc1_writes_data_and_mask() {
        let blob = fixture("world.lerc1");
        let mut data = vec![123.0f64; 257 * 257];
        let mut mask = vec![123u8; 257 * 257];

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                257,
                257,
                1,
                data.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);
        assert_eq!(data[0], 0.0);
        assert_eq!(data[257 * 257 - 1], 0.0);
        let (z_min, z_max) = min_max_valid_f64(&data, &mask);
        assert_eq!(z_min, -27.458_635_330_200_195);
        assert_eq!(z_max, 5474.172_851_562_5);
    }

    #[test]
    fn c_abi_decode_4d_legacy_lerc1_writes_data_and_mask() {
        let blob = fixture("world.lerc1");
        let mut data = vec![123.0f32; 257 * 257];
        let mut mask = vec![123u8; 257 * 257];
        let mut uses_no_data = [123u8; 1];
        let mut no_data_values = [123.0f64; 1];

        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                257,
                257,
                1,
                DataType::Float as u32,
                data.as_mut_ptr().cast(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);
        let (z_min, z_max) = min_max_valid_f32(&data, &mask);
        assert_eq!(z_min, -27.458_635);
        assert_eq!(z_max, 5474.173);
        assert_eq!(uses_no_data, [123]);
        assert_eq!(no_data_values, [123.0]);
    }

    #[test]
    fn c_abi_decode_4d_reports_no_data() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let mut data = [0u8; 12];
        let mut mask = [0u8; 6];
        let mut uses_no_data = [0u8; 1];
        let mut no_data_values = [0.0f64; 1];

        let regular_status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(regular_status, ErrCode::HasNoData as u32);

        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(uses_no_data, [1]);
        assert_eq!(no_data_values, [255.0]);
        assert_eq!(mask, [1; 6]);
        assert_eq!(data, [1, 255, 2, 3, 255, 4, 5, 6, 7, 255, 8, 9]);
    }

    #[test]
    fn c_abi_decode_to_double_4d_reports_no_data() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let mut data = [0.0f64; 12];
        let mut mask = [0u8; 6];
        let mut uses_no_data = [0u8; 1];
        let mut no_data_values = [0.0f64; 1];

        let regular_status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                data.as_mut_ptr(),
            )
        };
        assert_eq!(regular_status, ErrCode::HasNoData as u32);

        let status = unsafe {
            lerc_decodeToDouble_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                data.as_mut_ptr(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(uses_no_data, [1]);
        assert_eq!(no_data_values, [255.0]);
        assert_eq!(mask, [1; 6]);
        assert_eq!(
            data,
            [1.0, 255.0, 2.0, 3.0, 255.0, 4.0, 5.0, 6.0, 7.0, 255.0, 8.0, 9.0]
        );
    }

    #[test]
    fn c_abi_decode_4d_clears_no_data_outputs_before_decode_failure() {
        let mut blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let last = blob.len() - 1;
        blob[last] ^= 0x80;
        let mut data = [77u8; 12];
        let mut mask = [77u8; 6];
        let mut uses_no_data = [123u8; 1];
        let mut no_data_values = [123.0f64; 1];

        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(uses_no_data, [0]);
        assert_eq!(no_data_values, [0.0]);
    }

    #[test]
    fn c_abi_decode_to_double_4d_clears_no_data_outputs_before_decode_failure() {
        let mut blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let last = blob.len() - 1;
        blob[last] ^= 0x80;
        let mut data = [77.0f64; 12];
        let mut mask = [77u8; 6];
        let mut uses_no_data = [123u8; 1];
        let mut no_data_values = [123.0f64; 1];

        let status = unsafe {
            lerc_decodeToDouble_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                data.as_mut_ptr(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(uses_no_data, [0]);
        assert_eq!(no_data_values, [0.0]);
    }

    #[test]
    fn c_abi_decode_to_double_4d_legacy_lerc1_writes_data_and_mask() {
        let blob = fixture("world.lerc1");
        let mut data = vec![123.0f64; 257 * 257];
        let mut mask = vec![123u8; 257 * 257];
        let mut uses_no_data = [123u8; 1];
        let mut no_data_values = [123.0f64; 1];

        let status = unsafe {
            lerc_decodeToDouble_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                1,
                257,
                257,
                1,
                data.as_mut_ptr(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(mask.iter().filter(|&&value| value != 0).count(), 65_025);
        let (z_min, z_max) = min_max_valid_f64(&data, &mask);
        assert_eq!(z_min, -27.458_635_330_200_195);
        assert_eq!(z_max, 5474.172_851_562_5);
        assert_eq!(uses_no_data, [123]);
        assert_eq!(no_data_values, [123.0]);
    }

    #[test]
    fn c_abi_decode_4d_requires_no_data_outputs_when_needed() {
        let blob = synthetic_v6_uchar_one_sweep_no_data_blob();
        let mut data = [0u8; 12];
        let mut mask = [0u8; 6];
        let mut no_data_values = [0.0f64; 1];

        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                2,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
                ptr::null_mut(),
                no_data_values.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::HasNoData as u32);
    }

    #[test]
    fn c_abi_decode_validates_capacity_before_no_data_status() {
        let blob = synthetic_v6_uchar_no_data_with_mask_blob();
        let info = get_lerc_info(&blob).unwrap();
        assert_eq!(info.n_masks, 1);
        assert_eq!(info.n_uses_no_data_value, 1);

        let mut data = [0u8; 12];
        let mut doubles = [0.0f64; 12];
        let mut mask = [0u8; 3];
        let mut uses_no_data = [0u8; 1];
        let mut no_data_values = [0.0f64; 1];

        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                0,
                ptr::null_mut(),
                2,
                3,
                1,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decodeToDouble(
                blob.as_ptr(),
                blob.len() as u32,
                0,
                ptr::null_mut(),
                2,
                3,
                1,
                1,
                doubles.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                0,
                ptr::null_mut(),
                2,
                3,
                1,
                1,
                DataType::UChar as u32,
                data.as_mut_ptr().cast(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);

        let status = unsafe {
            lerc_decodeToDouble_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                mask.as_mut_ptr(),
                2,
                3,
                1,
                2,
                doubles.as_mut_ptr(),
                uses_no_data.as_mut_ptr(),
                no_data_values.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::WrongParam as u32);
    }

    #[test]
    fn c_abi_round_trips_cpp_sample_style_float_image_with_mask() {
        let n_cols = 17usize;
        let n_rows = 13usize;
        let mut data = Vec::with_capacity(n_cols * n_rows);
        let mut valid = Vec::with_capacity(n_cols * n_rows);
        for i in 0..n_rows {
            for j in 0..n_cols {
                let value = ((i * i + j * j) as f32).sqrt() + ((i * 7 + j * 11) % 20) as f32;
                data.push(value);
                valid.push(u8::from(j % 5 != 0 && i % 4 != 0));
            }
        }

        let mut num_bytes = 0u32;
        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                n_cols as i32,
                n_rows as i32,
                1,
                1,
                valid.as_ptr(),
                0.1,
                &mut num_bytes,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);

        let mut blob = vec![0u8; num_bytes as usize];
        let mut written = 0u32;
        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                n_cols as i32,
                n_rows as i32,
                1,
                1,
                valid.as_ptr(),
                0.1,
                blob.as_mut_ptr(),
                blob.len() as u32,
                &mut written,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(written, num_bytes);
        blob.truncate(written as usize);

        let mut info = [0u32; BLOB_INFO_ARRAY_LEN];
        let mut ranges = [0.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
        let status = unsafe {
            lerc_getBlobInfo(
                blob.as_ptr(),
                blob.len() as u32,
                info.as_mut_ptr(),
                ranges.as_mut_ptr(),
                info.len() as i32,
                ranges.len() as i32,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(info[1], DataType::Float as u32);
        assert_eq!(info[2], 1);
        assert_eq!(info[3], n_cols as u32);
        assert_eq!(info[4], n_rows as u32);
        assert_eq!(info[5], 1);
        assert_eq!(info[8], 1);
        assert_eq!(ranges[2], 0.1);

        let mut decoded = vec![0.0f32; data.len()];
        let mut decoded_mask = vec![0u8; valid.len()];
        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                decoded_mask.as_mut_ptr(),
                1,
                n_cols as i32,
                n_rows as i32,
                1,
                DataType::Float as u32,
                decoded.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(decoded_mask, valid);
        for ((&actual, &expected), &is_valid) in decoded.iter().zip(data.iter()).zip(valid.iter()) {
            if is_valid != 0 {
                assert!((actual - expected).abs() <= 0.1);
            } else {
                assert_eq!(actual, 0.0);
            }
        }
    }

    #[test]
    fn c_abi_round_trips_cpp_sample_style_uchar_depth_triplets() {
        let n_cols = 11usize;
        let n_rows = 7usize;
        let n_depth = 3usize;
        let mut data = Vec::with_capacity(n_cols * n_rows * n_depth);
        for i in 0..n_rows {
            for j in 0..n_cols {
                for m in 0..n_depth {
                    data.push(((i * 13 + j * 17 + m * 19) % 30) as u8);
                }
            }
        }

        let mut blob = vec![0u8; data.len() * 2];
        let mut written = 0u32;
        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::UChar as u32,
                n_depth as i32,
                n_cols as i32,
                n_rows as i32,
                1,
                0,
                ptr::null(),
                0.0,
                blob.as_mut_ptr(),
                blob.len() as u32,
                &mut written,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        blob.truncate(written as usize);

        let mut mins = [0.0f64; 3];
        let mut maxs = [0.0f64; 3];
        let status = unsafe {
            lerc_getDataRanges(
                blob.as_ptr(),
                blob.len() as u32,
                n_depth as i32,
                1,
                mins.as_mut_ptr(),
                maxs.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        let mut expected_mins = [u8::MAX; 3];
        let mut expected_maxs = [u8::MIN; 3];
        for pixel in data.chunks_exact(n_depth) {
            for (i_depth, &value) in pixel.iter().enumerate() {
                expected_mins[i_depth] = expected_mins[i_depth].min(value);
                expected_maxs[i_depth] = expected_maxs[i_depth].max(value);
            }
        }
        assert_eq!(mins, expected_mins.map(f64::from),);
        assert_eq!(maxs, expected_maxs.map(f64::from),);

        let mut decoded = vec![0u8; data.len()];
        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                0,
                ptr::null_mut(),
                n_depth as i32,
                n_cols as i32,
                n_rows as i32,
                1,
                DataType::UChar as u32,
                decoded.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(decoded, data);
    }

    #[test]
    fn c_abi_round_trips_cpp_sample_style_4d_no_data() {
        let n_cols = 5usize;
        let n_rows = 4usize;
        let n_depth = 2usize;
        let n_bands = 2usize;
        let no_data = f32::MAX;
        let mut data = vec![0.0f32; n_bands * n_rows * n_cols * n_depth];
        for band in 0..n_bands {
            let band_offset = band * n_rows * n_cols * n_depth;
            for i in 0..n_rows {
                for j in 0..n_cols {
                    let pixel = i * n_cols + j;
                    let value = ((i * i + j * j) as f32).sqrt() + (band * 10) as f32;
                    data[band_offset + pixel * n_depth] = value;
                    data[band_offset + pixel * n_depth + 1] = value + 0.25;
                    if band == 0 && (i + j) % 3 == 0 {
                        data[band_offset + pixel * n_depth] = no_data;
                    }
                }
            }
        }
        let uses_no_data = [1u8, 0];
        let no_data_values = [no_data as f64, 0.0];

        let mut num_bytes = 0u32;
        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                n_depth as i32,
                n_cols as i32,
                n_rows as i32,
                n_bands as i32,
                0,
                ptr::null(),
                0.001,
                &mut num_bytes,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);

        let mut blob = vec![0u8; num_bytes as usize];
        let mut written = 0u32;
        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                n_depth as i32,
                n_cols as i32,
                n_rows as i32,
                n_bands as i32,
                0,
                ptr::null(),
                0.001,
                blob.as_mut_ptr(),
                blob.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        blob.truncate(written as usize);

        let info = get_lerc_info(&blob).unwrap();
        assert_eq!(info.n_depth, n_depth as i32);
        assert_eq!(info.n_bands, n_bands as i32);
        assert_eq!(info.n_uses_no_data_value, n_bands as i32);

        let mut decoded = vec![0.0f32; data.len()];
        let mut decoded_mask = vec![0u8; n_cols * n_rows];
        let mut decoded_uses_no_data = [0u8; 2];
        let mut decoded_no_data = [0.0f64; 2];
        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                blob.len() as u32,
                1,
                decoded_mask.as_mut_ptr(),
                n_depth as i32,
                n_cols as i32,
                n_rows as i32,
                n_bands as i32,
                DataType::Float as u32,
                decoded.as_mut_ptr().cast(),
                decoded_uses_no_data.as_mut_ptr(),
                decoded_no_data.as_mut_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(decoded_mask, vec![1u8; n_cols * n_rows]);
        assert_eq!(decoded_uses_no_data, uses_no_data);
        assert_eq!(decoded_no_data, no_data_values);
        for (&actual, &expected) in decoded.iter().zip(data.iter()) {
            if expected == no_data {
                assert_eq!(actual, no_data);
            } else {
                assert!((actual - expected).abs() <= 0.0011);
            }
        }
    }

    #[test]
    fn c_abi_active_no_data_replaces_mixed_depth_nan_with_sentinel() {
        let data = [1.0f32, 2.0, f32::NAN, 3.0, f32::NAN, f32::NAN];
        let uses_no_data = [1u8];
        let no_data_values = [-9999.0f64];
        let mut computed_size = 0u32;
        let mut blob = [0u8; 256];
        let mut written = 0u32;

        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                2,
                3,
                1,
                1,
                0,
                ptr::null(),
                0.0,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);

        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                2,
                3,
                1,
                1,
                0,
                ptr::null(),
                0.0,
                blob.as_mut_ptr(),
                blob.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(written, computed_size);

        let mut decoded = [0.0f32; 6];
        let mut decoded_mask = [0u8; 3];
        let mut decoded_uses_no_data = [0u8; 1];
        let mut decoded_no_data = [0.0f64; 1];
        let status = unsafe {
            lerc_decode_4D(
                blob.as_ptr(),
                written,
                1,
                decoded_mask.as_mut_ptr(),
                2,
                3,
                1,
                1,
                DataType::Float as u32,
                decoded.as_mut_ptr().cast(),
                decoded_uses_no_data.as_mut_ptr(),
                decoded_no_data.as_mut_ptr(),
            )
        };

        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(decoded_mask, [1, 1, 0]);
        assert_eq!(decoded_uses_no_data, uses_no_data);
        assert_eq!(decoded_no_data, no_data_values);
        assert_eq!(decoded, [1.0, 2.0, -9999.0, 3.0, 0.0, 0.0]);
    }

    #[test]
    fn c_abi_active_no_data_still_rejects_inactive_band_mixed_depth_nan() {
        let data = [1.0f32, -9999.0, 2.0, 3.0, 10.0, 11.0, f32::NAN, 12.0];
        let uses_no_data = [1u8, 0];
        let no_data_values = [-9999.0f64, 0.0];
        let mut computed_size = 123u32;
        let mut blob = [0u8; 256];
        let mut written = 123u32;

        let status = unsafe {
            lerc_computeCompressedSize_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                2,
                2,
                1,
                2,
                0,
                ptr::null(),
                0.0,
                &mut computed_size,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(status, ErrCode::NaN as u32);
        assert_eq!(computed_size, 0);

        let status = unsafe {
            lerc_encode_4D(
                data.as_ptr().cast(),
                DataType::Float as u32,
                2,
                2,
                1,
                2,
                0,
                ptr::null(),
                0.0,
                blob.as_mut_ptr(),
                blob.len() as u32,
                &mut written,
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };

        assert_eq!(status, ErrCode::NaN as u32);
        assert_eq!(written, 0);
        assert_eq!(blob, [0; 256]);
    }

    #[test]
    fn c_abi_encode_filters_cpp_sample_style_float_nan_pixels() {
        let n_cols = 9usize;
        let n_rows = 5usize;
        let n_bands = 4usize;
        let mut data = vec![0.0f32; n_cols * n_rows * n_bands];
        let mut expected_masks = vec![1u8; n_cols * n_rows * n_bands];

        for band in 0..n_bands {
            let offset = band * n_cols * n_rows;
            for i in 0..n_rows {
                for j in 0..n_cols {
                    let pixel = i * n_cols + j;
                    data[offset + pixel] =
                        ((i * i + j * j) as f32).sqrt() + ((band * 13 + i + j) % 20) as f32;
                    if band != 2 && (i * 3 + j + band) % 4 == 0 {
                        data[offset + pixel] = f32::NAN;
                        expected_masks[offset + pixel] = 0;
                    }
                }
            }
        }

        let mut num_bytes = 0u32;
        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                n_cols as i32,
                n_rows as i32,
                n_bands as i32,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);

        let mut blob = vec![0u8; num_bytes as usize];
        let mut written = 0u32;
        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                1,
                n_cols as i32,
                n_rows as i32,
                n_bands as i32,
                0,
                ptr::null(),
                0.0,
                blob.as_mut_ptr(),
                blob.len() as u32,
                &mut written,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        blob.truncate(written as usize);

        let mut info = [0u32; BLOB_INFO_ARRAY_LEN];
        let mut ranges = [0.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
        let status = unsafe {
            lerc_getBlobInfo(
                blob.as_ptr(),
                blob.len() as u32,
                info.as_mut_ptr(),
                ranges.as_mut_ptr(),
                info.len() as i32,
                ranges.len() as i32,
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(info[8], n_bands as u32);

        let mut decoded = vec![123.0f32; data.len()];
        let mut decoded_masks = vec![123u8; expected_masks.len()];
        let status = unsafe {
            lerc_decode(
                blob.as_ptr(),
                blob.len() as u32,
                n_bands as i32,
                decoded_masks.as_mut_ptr(),
                1,
                n_cols as i32,
                n_rows as i32,
                n_bands as i32,
                DataType::Float as u32,
                decoded.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, ErrCode::Ok as u32);
        assert_eq!(decoded_masks, expected_masks);
        for ((&actual, &expected), &valid) in
            decoded.iter().zip(data.iter()).zip(expected_masks.iter())
        {
            if valid != 0 {
                assert_eq!(actual, expected);
            } else {
                assert_eq!(actual, 0.0);
            }
        }
    }

    #[test]
    fn c_abi_encode_rejects_mixed_depth_nan_without_no_data() {
        let data = [1.0f32, f32::NAN, 2.0, 3.0, 4.0, 5.0];
        let mut num_bytes = 123u32;

        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
                DataType::Float as u32,
                2,
                3,
                1,
                1,
                0,
                ptr::null(),
                0.0,
                &mut num_bytes,
            )
        };

        assert_eq!(status, ErrCode::NaN as u32);
        assert_eq!(num_bytes, 0);
    }

    #[test]
    fn c_abi_decode_and_info_wrappers_reject_adversarial_blobs_without_writes() {
        let mut malformed_blobs = vec![
            Vec::new(),
            b"Lerc2 ".to_vec(),
            synthetic_v4_const_blob()[..20].to_vec(),
            synthetic_v4_const_blob(),
        ];
        malformed_blobs[3][0] = b'X';
        let mut checksum_mismatch = fixture("california_400_400_1_float.lerc2");
        let last = checksum_mismatch.len() - 1;
        checksum_mismatch[last] ^= 0x01;

        for blob in &malformed_blobs {
            let ptr = if blob.is_empty() {
                ptr::null()
            } else {
                blob.as_ptr()
            };
            let len = blob.len() as u32;
            let mut info = [77u32; BLOB_INFO_ARRAY_LEN];
            let mut ranges = [77.0f64; BLOB_DATA_RANGE_ARRAY_LEN];
            let status = unsafe {
                lerc_getBlobInfo(
                    ptr,
                    len,
                    info.as_mut_ptr(),
                    ranges.as_mut_ptr(),
                    info.len() as i32,
                    ranges.len() as i32,
                )
            };
            assert_ne!(status, ErrCode::Ok as u32);

            let mut mins = [77.0f64; 1];
            let mut maxs = [77.0f64; 1];
            let status =
                unsafe { lerc_getDataRanges(ptr, len, 1, 1, mins.as_mut_ptr(), maxs.as_mut_ptr()) };
            assert_ne!(status, ErrCode::Ok as u32);
        }

        malformed_blobs.push(checksum_mismatch);

        for blob in malformed_blobs {
            let ptr = if blob.is_empty() {
                ptr::null()
            } else {
                blob.as_ptr()
            };
            let len = blob.len() as u32;
            let mut decoded = [77u8; 4];
            let mut mask = [77u8; 4];
            let status = unsafe {
                lerc_decode(
                    ptr,
                    len,
                    1,
                    mask.as_mut_ptr(),
                    1,
                    2,
                    2,
                    1,
                    DataType::UChar as u32,
                    decoded.as_mut_ptr().cast(),
                )
            };
            assert_ne!(status, ErrCode::Ok as u32);
            assert_eq!(decoded, [77u8; 4]);
            assert_eq!(mask, [77u8; 4]);

            let mut decoded_f64 = [77.0f64; 4];
            let status = unsafe {
                lerc_decodeToDouble(
                    ptr,
                    len,
                    1,
                    mask.as_mut_ptr(),
                    1,
                    2,
                    2,
                    1,
                    decoded_f64.as_mut_ptr(),
                )
            };
            assert_ne!(status, ErrCode::Ok as u32);
            assert_eq!(decoded_f64, [77.0f64; 4]);
        }
    }

    fn synthetic_v4_const_blob() -> Vec<u8> {
        let header_size = 6 + 4 + 4 + 7 * 4 + 3 * 8;
        let blob_size = header_size + 4;
        let mut blob = Vec::with_capacity(blob_size);
        blob.extend_from_slice(b"Lerc2 ");
        blob.extend_from_slice(&4i32.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        for value in [2, 3, 1, 6, 2, blob_size as i32, DataType::UChar as i32] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob.extend_from_slice(&0.0f64.to_le_bytes());
        blob.extend_from_slice(&7.0f64.to_le_bytes());
        blob.extend_from_slice(&7.0f64.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        let checksum = compute_checksum_fletcher32(&blob[14..]);
        blob[10..14].copy_from_slice(&checksum.to_le_bytes());
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
        let checksum = compute_checksum_fletcher32(&blob[14..]);
        blob[10..14].copy_from_slice(&checksum.to_le_bytes());
        blob
    }

    fn synthetic_v6_uchar_no_data_with_mask_blob() -> Vec<u8> {
        let spec = crate::EncodeSpec {
            data_type: DataType::UChar,
            n_depth: 2,
            n_cols: 3,
            n_rows: 1,
            n_bands: 1,
            n_masks: 1,
        };
        let data = [255u8, 2, 7, 8, 3, 4];
        let mask = [1u8, 0, 1];
        encode_lerc2_uncompressed_with_no_data(
            spec,
            &data,
            0.5,
            Some(&mask),
            Some(&[1]),
            Some(&[255.0]),
            6,
        )
        .unwrap()
    }
}
