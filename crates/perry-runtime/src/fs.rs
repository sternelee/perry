//! File system module - provides file operations

use std::fs;
use std::path::Path;

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::POINTER_MASK;

/// Extract a string pointer from a NaN-boxed f64 value
/// Handles both NaN-boxed strings (with STRING_TAG) and raw pointers.
/// Returns null for invalid/small pointers (e.g. from TAG_UNDEFINED extraction).
#[inline]
fn extract_string_ptr(value: f64) -> *const StringHeader {
    let bits = value.to_bits();
    // Mask off the tag bits to get the raw pointer
    let ptr = (bits & POINTER_MASK) as usize;
    if ptr < 0x1000 { std::ptr::null() } else { ptr as *const StringHeader }
}

/// Read a file synchronously and return its contents as a string
/// Returns null pointer on error
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_read_file_sync(path_value: f64) -> *mut StringHeader {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return std::ptr::null_mut();
        }

        let len = (*path_ptr).length as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match fs::read_to_string(path_str) {
            Ok(content) => {
                let bytes = content.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Write content to a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string values
#[no_mangle]
pub extern "C" fn js_fs_write_file_sync(
    path_value: f64,
    content_value: f64,
) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        let content_ptr = extract_string_ptr(content_value);
        if path_ptr.is_null() || content_ptr.is_null() {
            return 0;
        }

        // Get path string
        let path_len = (*path_ptr).length as usize;
        let path_data = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(path_data, path_len);
        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        // Get content string
        let content_len = (*content_ptr).length as usize;
        let content_data = (content_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let content_bytes = std::slice::from_raw_parts(content_data, content_len);

        match fs::write(path_str, content_bytes) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

/// Append content to a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string values
#[no_mangle]
pub extern "C" fn js_fs_append_file_sync(
    path_value: f64,
    content_value: f64,
) -> i32 {
    use std::io::Write;
    use std::fs::OpenOptions;

    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        let content_ptr = extract_string_ptr(content_value);
        if path_ptr.is_null() || content_ptr.is_null() {
            return 0;
        }

        // Get path string
        let path_len = (*path_ptr).length as usize;
        let path_data = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(path_data, path_len);
        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        // Get content string
        let content_len = (*content_ptr).length as usize;
        let content_data = (content_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let content_bytes = std::slice::from_raw_parts(content_data, content_len);

        // Open file in append mode, creating if it doesn't exist
        match OpenOptions::new().create(true).append(true).open(path_str) {
            Ok(mut file) => {
                match file.write_all(content_bytes) {
                    Ok(_) => 1,
                    Err(_) => 0,
                }
            }
            Err(_) => 0,
        }
    }
}

/// Check if a file or directory exists
/// Returns 1 if exists, 0 if not
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_exists_sync(path_value: f64) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).length as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        if Path::new(path_str).exists() { 1 } else { 0 }
    }
}

/// Create a directory synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_mkdir_sync(path_value: f64) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).length as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        match fs::create_dir_all(path_str) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

/// Read directory entries synchronously and return as a JS array of strings.
/// Returns an empty array on error.
/// Accepts NaN-boxed string path.
#[no_mangle]
pub extern "C" fn js_fs_readdir_sync(path_value: f64) -> f64 {
    use crate::array::{js_array_alloc, js_array_push_f64};
    use crate::string::js_string_from_bytes;
    use crate::value::js_nanbox_string;

    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            let arr = js_array_alloc(0);
            return std::mem::transmute::<i64, f64>(arr as i64);
        }

        let len = (*path_ptr).length as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => {
                let arr = js_array_alloc(0);
                return std::mem::transmute::<i64, f64>(arr as i64);
            }
        };

        match fs::read_dir(path_str) {
            Ok(entries) => {
                let mut names: Vec<String> = Vec::new();
                for entry in entries {
                    if let Ok(e) = entry {
                        if let Some(name) = e.file_name().to_str() {
                            names.push(name.to_string());
                        }
                    }
                }
                names.sort();

                let mut arr = js_array_alloc(names.len() as u32);
                for name in &names {
                    let bytes = name.as_bytes();
                    let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                    let str_f64 = js_nanbox_string(str_ptr as i64);
                    arr = js_array_push_f64(arr, str_f64);
                }
                std::mem::transmute::<i64, f64>(arr as i64)
            }
            Err(_) => {
                let arr = js_array_alloc(0);
                std::mem::transmute::<i64, f64>(arr as i64)
            }
        }
    }
}

/// Check if a path is a directory.
/// Returns 1 if directory, 0 if not (or error).
/// Accepts NaN-boxed string path.
#[no_mangle]
pub extern "C" fn js_fs_is_directory(path_value: f64) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).length as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        if Path::new(path_str).is_dir() { 1 } else { 0 }
    }
}

/// Remove a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_unlink_sync(path_value: f64) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).length as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        match fs::remove_file(path_str) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}
