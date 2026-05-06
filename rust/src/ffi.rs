/*
Copyright 2015 - 2026 Esri

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
*/

//! C ABI entry points backed by the safe Rust implementation.

use crate::{get_lerc2_blob_info_arrays, ErrCode};
use core::slice;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// C ABI equivalent of `lerc_getBlobInfo`.
///
/// This function currently supports Lerc2 blobs handled by the Rust metadata
/// parser. It fills the caller-provided arrays using the same order and
/// partial-fill behavior as the C++ API.
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

#[cfg(test)]
mod tests {
    use super::lerc_getBlobInfo;
    use crate::{DataType, ErrCode, BLOB_DATA_RANGE_ARRAY_LEN, BLOB_INFO_ARRAY_LEN};
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
}
