/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! C ABI entry points backed by the safe Rust implementation.

use crate::{
    decode_lerc2_supported_into, decode_typed_values, get_lerc2_blob_info_arrays,
    get_lerc2_data_ranges, get_lerc2_header_info, get_lerc_info, DataType, DecodeIntoSpec, ErrCode,
    LercError,
};
use core::ffi::c_void;
use core::slice;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// C ABI equivalent of `lerc_computeCompressedSize`.
///
/// Encoding is not ported yet. Valid calls currently zero `num_bytes` and
/// return [`ErrCode::Failed`].
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
) -> u32 {
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
/// Encoding is not ported yet. Valid calls currently zero `num_bytes` and
/// return [`ErrCode::Failed`].
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
) -> u32 {
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
/// Encoding is not ported yet. Valid calls currently zero `n_bytes_written`
/// and return [`ErrCode::Failed`].
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
) -> u32 {
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
/// Encoding is not ported yet. Valid calls currently zero `n_bytes_written`
/// and return [`ErrCode::Failed`].
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
) -> u32 {
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
/// Encoding is not ported yet. Valid calls currently zero `num_bytes` and
/// return [`ErrCode::Failed`].
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
) -> u32 {
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
/// Encoding is not ported yet. Valid calls currently zero `n_bytes_written`
/// and return [`ErrCode::Failed`].
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
) -> u32 {
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
/// This function currently supports Lerc2 blobs handled by the Rust metadata
/// parser and the checked-in legacy Lerc1 metadata path. It fills the
/// caller-provided arrays using the same order and partial-fill behavior as
/// the C++ API.
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
) -> u32 {
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
/// This function currently supports Lerc2 blobs handled by the Rust metadata
/// parser and the checked-in legacy Lerc1 metadata path. It writes minima and
/// maxima in band-major, depth-minor order.
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
) -> u32 {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        lerc_get_data_ranges_impl(p_lerc_blob, blob_size, n_depth, n_bands, p_mins, p_maxs)
    }))
    .unwrap_or(ErrCode::Failed as u32)
}

/// C ABI equivalent of `lerc_decode` for the currently supported Lerc2 subset.
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
) -> u32 {
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

/// C ABI equivalent of `lerc_decodeToDouble` for the supported Lerc2 subset.
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
) -> u32 {
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

/// C ABI equivalent of `lerc_decode_4D` for the supported Lerc2 subset.
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
) -> u32 {
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

/// C ABI equivalent of `lerc_decodeToDouble_4D` for the supported Lerc2 subset.
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
) -> u32 {
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
    _codec_version: i32,
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

    match validate_encode_shape(
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
        Ok(()) => {}
        Err(err) => return err as u32,
    }
    if !validate_no_data_inputs(p_uses_no_data, no_data_values) {
        return ErrCode::WrongParam as u32;
    }

    ErrCode::Failed as u32
}

unsafe fn lerc_encode_impl(
    p_data: *const c_void,
    _codec_version: i32,
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

    match validate_encode_shape(
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
        Ok(()) => {}
        Err(err) => return err as u32,
    }
    if p_out_buffer.is_null() || out_buffer_size == 0 {
        return ErrCode::WrongParam as u32;
    }
    if !validate_no_data_inputs(p_uses_no_data, no_data_values) {
        return ErrCode::WrongParam as u32;
    }

    ErrCode::Failed as u32
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
) -> core::result::Result<(), ErrCode> {
    if p_data.is_null()
        || DataType::try_from(data_type as i32)
            .map(|parsed| parsed as u32 != data_type)
            .unwrap_or(true)
        || n_depth <= 0
        || n_cols <= 0
        || n_rows <= 0
        || n_bands <= 0
        || max_z_err < 0.0
        || !(n_masks == 0 || n_masks == 1 || n_masks == n_bands)
        || (n_masks > 0 && p_valid_bytes.is_null())
    {
        return Err(ErrCode::WrongParam);
    }

    Ok(())
}

fn validate_no_data_inputs(
    p_uses_no_data: Option<*const u8>,
    no_data_values: Option<*const f64>,
) -> bool {
    match (p_uses_no_data, no_data_values) {
        (Some(ptr), Some(values)) => ptr.is_null() || !values.is_null(),
        _ => true,
    }
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

    let data_type_id = data_type;
    let data_type = match DataType::try_from(data_type_id as i32) {
        Ok(data_type) if data_type as u32 == data_type_id => data_type,
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

    let data_len = match decoded_data_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match decoded_mask_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };

    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    match get_lerc_info(blob) {
        Ok(info) if info.n_uses_no_data_value > 0 && n_depth > 1 => {
            return ErrCode::HasNoData as u32;
        }
        Ok(_) => {}
        Err(err) => return err.err_code() as u32,
    }

    let data_output = unsafe { slice::from_raw_parts_mut(p_data.cast::<u8>(), data_len) };
    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };

    match decode_lerc2_supported_into(blob, spec, data_output, mask_output.as_deref_mut()) {
        Ok(_) => ErrCode::Ok as u32,
        Err(err) => err.err_code() as u32,
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

    let data_type_id = data_type;
    let data_type = match DataType::try_from(data_type_id as i32) {
        Ok(data_type) if data_type as u32 == data_type_id => data_type,
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
    let data_len = match decoded_data_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match decoded_mask_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let blob = unsafe { slice::from_raw_parts(p_lerc_blob, blob_size as usize) };
    let info = match get_lerc_info(blob) {
        Ok(info) => info,
        Err(err) => return err.err_code() as u32,
    };
    if info.n_uses_no_data_value > 0
        && n_depth > 1
        && (p_uses_no_data.is_null() || no_data_values.is_null())
    {
        return ErrCode::HasNoData as u32;
    }

    let data_output = unsafe { slice::from_raw_parts_mut(p_data.cast::<u8>(), data_len) };
    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };
    let status =
        match decode_lerc2_supported_into(blob, spec, data_output, mask_output.as_deref_mut()) {
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
        Ok(info) if info.n_uses_no_data_value > 0 && n_depth > 1 => {
            return ErrCode::HasNoData as u32;
        }
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

    let data_len = match decoded_data_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let value_count = match decoded_value_count(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match decoded_mask_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };

    let mut native_data = vec![0u8; data_len];
    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };
    let decoded =
        match decode_lerc2_supported_into(blob, spec, &mut native_data, mask_output.as_deref_mut())
        {
            Ok(_) => match decode_typed_values(info.data_type, &native_data) {
                Ok(decoded) => decoded,
                Err(err) => return err.err_code() as u32,
            },
            Err(err) => return err.err_code() as u32,
        };

    let output = unsafe { slice::from_raw_parts_mut(p_data, value_count) };
    match decoded.write_f64_values(output) {
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
    if info.n_uses_no_data_value > 0
        && n_depth > 1
        && (p_uses_no_data.is_null() || no_data_values.is_null())
    {
        return ErrCode::HasNoData as u32;
    }
    let spec = DecodeIntoSpec {
        data_type: info.data_type,
        n_depth: n_depth as usize,
        n_cols: n_cols as usize,
        n_rows: n_rows as usize,
        n_bands: n_bands as usize,
        n_masks: n_masks as usize,
    };

    let data_len = match decoded_data_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let value_count = match decoded_value_count(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };
    let mask_len = match decoded_mask_byte_len(spec) {
        Ok(len) => len,
        Err(err) => return err.err_code() as u32,
    };

    let mut native_data = vec![0u8; data_len];
    let mut mask_output = if n_masks > 0 {
        Some(unsafe { slice::from_raw_parts_mut(p_valid_bytes, mask_len) })
    } else {
        None
    };
    let decoded =
        match decode_lerc2_supported_into(blob, spec, &mut native_data, mask_output.as_deref_mut())
        {
            Ok(_) => match decode_typed_values(info.data_type, &native_data) {
                Ok(decoded) => decoded,
                Err(err) => return err.err_code() as u32,
            },
            Err(err) => return err.err_code() as u32,
        };
    let output = unsafe { slice::from_raw_parts_mut(p_data, value_count) };
    if let Err(err) = decoded.write_f64_values(output) {
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

fn decoded_data_byte_len(spec: DecodeIntoSpec) -> Result<usize, LercError> {
    spec.n_bands
        .checked_mul(spec.n_rows)
        .and_then(|count| count.checked_mul(spec.n_cols))
        .and_then(|count| count.checked_mul(spec.n_depth))
        .and_then(|count| count.checked_mul(spec.data_type.size_in_bytes()))
        .ok_or(LercError::WrongParam("decode output byte count overflow"))
}

fn decoded_value_count(spec: DecodeIntoSpec) -> Result<usize, LercError> {
    spec.n_bands
        .checked_mul(spec.n_rows)
        .and_then(|count| count.checked_mul(spec.n_cols))
        .and_then(|count| count.checked_mul(spec.n_depth))
        .ok_or(LercError::WrongParam("decode output value count overflow"))
}

fn decoded_mask_byte_len(spec: DecodeIntoSpec) -> Result<usize, LercError> {
    spec.n_masks
        .checked_mul(spec.n_rows)
        .and_then(|count| count.checked_mul(spec.n_cols))
        .ok_or(LercError::WrongParam("decode mask byte count overflow"))
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

    let mut offset = 0usize;
    for i_band in 0..n_bands {
        let header = get_lerc2_header_info(&blob[offset..])?.header;
        uses_no_data[i_band] = u8::from(header.has_no_data_values());
        no_data_values[i_band] = header.no_data_val_orig;
        let blob_size = header.blob_size as usize;
        if blob_size == 0 || blob_size > blob.len().saturating_sub(offset) {
            return Err(LercError::BufferTooSmall);
        }
        offset += blob_size;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        lerc_computeCompressedSize, lerc_computeCompressedSizeForVersion,
        lerc_computeCompressedSize_4D, lerc_decode, lerc_decodeToDouble, lerc_decodeToDouble_4D,
        lerc_decode_4D, lerc_encode, lerc_encodeForVersion, lerc_encode_4D, lerc_getBlobInfo,
        lerc_getDataRanges,
    };
    use crate::{
        compute_checksum_fletcher32, get_lerc2_blob_info_arrays, get_lerc2_data_ranges, DataType,
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
    fn c_abi_encode_stubs_zero_output_counters_and_fail() {
        let data = [1u8, 2, 3, 4, 5, 6];
        let mut num_bytes = 123u32;
        let mut out = [0u8; 64];
        let mut written = 123u32;

        let status = unsafe {
            lerc_computeCompressedSize(
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
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(num_bytes, 0);

        num_bytes = 123;
        let status = unsafe {
            lerc_computeCompressedSizeForVersion(
                data.as_ptr().cast(),
                6,
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
        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(num_bytes, 0);

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
                0.0,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut written,
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(written, 0);

        written = 123;
        let status = unsafe {
            lerc_encodeForVersion(
                data.as_ptr().cast(),
                6,
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
        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(written, 0);
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
    fn c_abi_4d_encode_stubs_validate_no_data_pointers() {
        let data = [1u8, 2, 3, 4, 5, 6];
        let uses_no_data = [1u8];
        let no_data_values = [255.0f64];
        let mut num_bytes = 123u32;
        let mut out = [0u8; 64];
        let mut written = 123u32;

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
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(num_bytes, 0);

        num_bytes = 123;
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
                uses_no_data.as_ptr(),
                no_data_values.as_ptr(),
            )
        };
        assert_eq!(status, ErrCode::Failed as u32);
        assert_eq!(written, 0);
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
}
