//! Built-in functions and objects
//!
//! Provides runtime implementations of JavaScript built-ins like console.log

use crate::JSValue;
use crate::string::{StringHeader, js_string_from_bytes};

/// Returns true if the f64 value is negative zero (-0.0).
/// Uses bit pattern comparison so +0.0 and -0.0 are distinguished
/// (they compare equal with normal `==`).
#[inline]
fn is_negative_zero(n: f64) -> bool {
    n.to_bits() == 0x8000_0000_0000_0000u64
}

/// Print a value to stdout (console.log implementation)
#[no_mangle]
pub extern "C" fn js_console_log(value: JSValue) {
    if value.is_undefined() {
        println!("undefined");
    } else if value.is_null() {
        println!("null");
    } else if value.is_bool() {
        println!("{}", value.as_bool());
    } else if value.is_number() {
        let n = value.as_number();
        // Match Node/V8 console.log semantics: distinguish -0 from 0
        if is_negative_zero(n) {
            println!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            // Print integers without decimal point
            println!("{}", n as i64);
        } else {
            println!("{}", n);
        }
    } else if value.is_int32() {
        println!("{}", value.as_int32());
    } else {
        println!("{:?}", value);
    }
}

/// Print a dynamic value to stdout (for union types, etc.)
/// Takes an f64 that uses proper NaN-boxing to distinguish types.
/// - Numbers are stored as regular f64 values
/// - Strings are stored as NaN-boxed pointers (tag 0x7FFF)
/// - Objects are stored as NaN-boxed pointers (tag 0x7FFD)
#[no_mangle]
pub extern "C" fn js_console_log_dynamic(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        println!("undefined");
    } else if jsval.is_null() {
        println!("null");
    } else if jsval.is_bool() {
        println!("{}", jsval.as_bool());
    } else if jsval.is_string() {
        // String pointer (uses STRING_TAG 0x7FFF)
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            println!("null");
        } else {
            unsafe {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    println!("{}", s);
                } else {
                    println!("[invalid utf8]");
                }
            }
        }
    } else if jsval.is_pointer() {
        // Object/array pointer - format as JSON
        println!("{}", format_jsvalue(value, 0));
    } else if jsval.is_int32() {
        println!("{}", jsval.as_int32());
    } else {
        // Must be a regular number
        let n = value;
        if n.is_nan() {
            println!("NaN");
        } else if n.is_infinite() {
            if n > 0.0 { println!("Infinity"); } else { println!("-Infinity"); }
        } else if is_negative_zero(n) {
            println!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            println!("{}", n as i64);
        } else {
            println!("{}", n);
        }
    }
}

/// Print a number to stdout (optimized path for known numbers)
#[no_mangle]
pub extern "C" fn js_console_log_number(value: f64) {
    if is_negative_zero(value) {
        println!("-0");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        println!("{}", value as i64);
    } else {
        println!("{}", value);
    }
}

/// Print an i32 to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_console_error_i32(value: i32) {
    eprintln!("{}", value);
}

/// Print a dynamic value to stderr (console.error for union types)
#[no_mangle]
pub extern "C" fn js_console_error_dynamic(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        eprintln!("undefined");
    } else if jsval.is_null() {
        eprintln!("null");
    } else if jsval.is_bool() {
        eprintln!("{}", jsval.as_bool());
    } else if jsval.is_string() {
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            eprintln!("null");
        } else {
            unsafe {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    eprintln!("{}", s);
                } else {
                    eprintln!("[invalid utf8]");
                }
            }
        }
    } else if jsval.is_pointer() {
        // Object/array pointer - format as JSON
        eprintln!("{}", format_jsvalue(value, 0));
    } else if jsval.is_int32() {
        eprintln!("{}", jsval.as_int32());
    } else {
        let n = value;
        if n.is_nan() {
            eprintln!("NaN");
        } else if n.is_infinite() {
            if n > 0.0 { eprintln!("Infinity"); } else { eprintln!("-Infinity"); }
        } else if is_negative_zero(n) {
            eprintln!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            eprintln!("{}", n as i64);
        } else {
            eprintln!("{}", n);
        }
    }
}

/// Print a number to stderr (console.error for numbers)
#[no_mangle]
pub extern "C" fn js_console_error_number(value: f64) {
    if is_negative_zero(value) {
        eprintln!("-0");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        eprintln!("{}", value as i64);
    } else {
        eprintln!("{}", value);
    }
}

/// Print an i32 to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_console_warn_i32(value: i32) {
    eprintln!("{}", value);
}

/// Print a dynamic value to stderr (console.warn for union types)
#[no_mangle]
pub extern "C" fn js_console_warn_dynamic(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        eprintln!("undefined");
    } else if jsval.is_null() {
        eprintln!("null");
    } else if jsval.is_bool() {
        eprintln!("{}", jsval.as_bool());
    } else if jsval.is_string() {
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            eprintln!("null");
        } else {
            unsafe {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    eprintln!("{}", s);
                } else {
                    eprintln!("[invalid utf8]");
                }
            }
        }
    } else if jsval.is_pointer() {
        // Object/array pointer - format as JSON
        eprintln!("{}", format_jsvalue(value, 0));
    } else if jsval.is_int32() {
        eprintln!("{}", jsval.as_int32());
    } else {
        let n = value;
        if n.is_nan() {
            eprintln!("NaN");
        } else if n.is_infinite() {
            if n > 0.0 { eprintln!("Infinity"); } else { eprintln!("-Infinity"); }
        } else if is_negative_zero(n) {
            eprintln!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            eprintln!("{}", n as i64);
        } else {
            eprintln!("{}", n);
        }
    }
}

/// Print a number to stderr (console.warn for numbers)
#[no_mangle]
pub extern "C" fn js_console_warn_number(value: f64) {
    if is_negative_zero(value) {
        eprintln!("-0");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        eprintln!("{}", value as i64);
    } else {
        eprintln!("{}", value);
    }
}

/// Print an i32 to stdout
#[no_mangle]
pub extern "C" fn js_console_log_i32(value: i32) {
    println!("{}", value);
}

/// Print an i64 to stdout
#[no_mangle]
pub extern "C" fn js_console_log_i64(value: i64) {
    println!("{}", value);
}

/// Print multiple values from an array (console.log with spread support)
/// Takes a pointer to an ArrayHeader containing f64 values
/// Helper function to format a JSValue as a string (for spread arrays)
fn format_jsvalue(value: f64, depth: usize) -> String {
    // Prevent stack overflow with deeply nested structures
    if depth > 10 {
        return "[...]".to_string();
    }

    let jsval = JSValue::from_bits(value.to_bits());

    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if jsval.is_string() {
            let ptr = jsval.as_string_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(bytes).unwrap_or("[invalid utf8]").to_string()
            }
        } else if jsval.is_bigint() {
            // Format BigInt by converting to string
            let ptr = jsval.as_bigint_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                if str_ptr.is_null() {
                    "0n".to_string()
                } else {
                    let len = (*str_ptr).length as usize;
                    let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    let num_str = std::str::from_utf8(bytes).unwrap_or("0");
                    format!("{}n", num_str)
                }
            }
        } else if jsval.is_pointer() {
            let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
            if ptr.is_null() {
                "null".to_string()
            } else {
                // Use GC header to determine the actual type of the object.
                // The GC header is located GC_HEADER_SIZE bytes before the user pointer.
                let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let gc_type = (*gc_header).obj_type;

                if gc_type == crate::gc::GC_TYPE_ERROR {
                    // Error object
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    let name_ptr = (*error_ptr).name;
                    let message_ptr = (*error_ptr).message;

                    let name_str = if name_ptr.is_null() {
                        "Error".to_string()
                    } else {
                        let len = (*name_ptr).length as usize;
                        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                    };

                    let message_str = if message_ptr.is_null() {
                        "".to_string()
                    } else {
                        let len = (*message_ptr).length as usize;
                        let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("").to_string()
                    };

                    if message_str.is_empty() {
                        name_str
                    } else {
                        format!("{}: {}", name_str, message_str)
                    }
                } else if gc_type == crate::gc::GC_TYPE_ARRAY {
                    // Array — format as [ elem1, elem2, ... ] matching Node.js util.inspect
                    let maybe_arr = ptr;
                    let length = (*maybe_arr).length as usize;
                    let data_ptr = (maybe_arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
                    let mut parts: Vec<String> = Vec::with_capacity(length);
                    for i in 0..length {
                        let elem_value = *data_ptr.add(i);
                        let elem_jsval = JSValue::from_bits(elem_value.to_bits());
                        // Quote string elements like Node's util.inspect: 'hello'
                        if elem_jsval.is_string() {
                            let s = format_jsvalue(elem_value, depth + 1);
                            parts.push(format!("'{}'", s));
                        } else {
                            parts.push(format_jsvalue(elem_value, depth + 1));
                        }
                    }
                    let inner = parts.join(", ");
                    // Node uses multi-line for arrays with >5 elements or >72 chars
                    if length > 5 || inner.len() > 72 {
                        let indent = "  ";
                        let lines: Vec<String> = parts.chunks(4).map(|chunk| {
                            format!("{}{}", indent, chunk.join(", "))
                        }).collect();
                        format!("[\n{}\n]", lines.join(",\n"))
                    } else if length == 0 {
                        "[]".to_string()
                    } else {
                        format!("[ {} ]", inner)
                    }
                } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                    // Object — check for keys_array
                    let obj_ptr = ptr as *const crate::object::ObjectHeader;
                    let keys_array = (*obj_ptr).keys_array;

                    if !keys_array.is_null() && (keys_array as usize) > 0x10000 && ((keys_array as usize) >> 48) == 0 {
                        format_object_as_json(obj_ptr, depth)
                    } else {
                        "[object Object]".to_string()
                    }
                } else {
                    // Fallback: use heuristics for non-GC-tracked pointers (e.g., static objects)
                    let object_type = *(ptr as *const u32);
                    if object_type == crate::error::OBJECT_TYPE_ERROR {
                        let error_ptr = ptr as *const crate::error::ErrorHeader;
                        let name_ptr = (*error_ptr).name;
                        let message_ptr = (*error_ptr).message;
                        let name_str = if name_ptr.is_null() {
                            "Error".to_string()
                        } else {
                            let len = (*name_ptr).length as usize;
                            let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                            let bytes = std::slice::from_raw_parts(data, len);
                            std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                        };
                        let message_str = if message_ptr.is_null() {
                            "".to_string()
                        } else {
                            let len = (*message_ptr).length as usize;
                            let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                            let bytes = std::slice::from_raw_parts(data, len);
                            std::str::from_utf8(bytes).unwrap_or("").to_string()
                        };
                        if message_str.is_empty() { name_str } else { format!("{}: {}", name_str, message_str) }
                    } else {
                        // Heuristic array check for non-GC pointers
                        let maybe_arr = ptr;
                        let length = (*maybe_arr).length as usize;
                        let capacity = (*maybe_arr).capacity as usize;
                        if capacity >= length && length < 1_000_000 && capacity < 10_000_000 && capacity > 0 {
                            let data_ptr = (maybe_arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
                            let mut parts: Vec<String> = Vec::with_capacity(length);
                            for i in 0..length {
                                let elem_value = *data_ptr.add(i);
                                parts.push(format_jsvalue(elem_value, depth + 1));
                            }
                            format!("[{}]", parts.join(", "))
                        } else {
                            let obj_ptr = ptr as *const crate::object::ObjectHeader;
                            let keys_array = (*obj_ptr).keys_array;
                            if !keys_array.is_null() && (keys_array as usize) > 0x10000 && ((keys_array as usize) >> 48) == 0 {
                                format_object_as_json(obj_ptr, depth)
                            } else {
                                "[object Object]".to_string()
                            }
                        }
                    }
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
            // Regular number
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                (n as i64).to_string()
            } else {
                n.to_string()
            }
        }
    }
}

/// Format an object as JSON-like string
/// Reads keys from the keys_array and values from the fields
unsafe fn format_object_as_json(obj_ptr: *const crate::object::ObjectHeader, depth: usize) -> String {
    if depth > 10 {
        return "{...}".to_string();
    }

    let keys_array = (*obj_ptr).keys_array;
    if keys_array.is_null() {
        return "{}".to_string();
    }

    let key_count = crate::array::js_array_length(keys_array) as usize;
    if key_count == 0 {
        return "{}".to_string();
    }

    let mut parts: Vec<String> = Vec::with_capacity(key_count);

    for i in 0..key_count {
        // Get the key (NaN-boxed string pointer)
        let key_val = crate::array::js_array_get(keys_array, i as u32);
        let key_str = if key_val.is_string() {
            let key_ptr = key_val.as_string_ptr();
            if key_ptr.is_null() {
                continue;
            }
            let len = (*key_ptr).length as usize;
            let data = (key_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        } else {
            continue;
        };

        // Get the value
        let value = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
        let value_str = format_jsvalue_for_json(value, depth + 1);

        parts.push(format!("{}: {}", key_str, value_str));
    }

    format!("{{ {} }}", parts.join(", "))
}

/// Format a JSValue for JSON output (strings get quotes)
fn format_jsvalue_for_json(value: f64, depth: usize) -> String {
    if depth > 10 {
        return "\"...\"".to_string();
    }

    let jsval = JSValue::from_bits(value.to_bits());

    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if jsval.is_string() {
            let ptr = jsval.as_string_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                let s = std::str::from_utf8(bytes).unwrap_or("[invalid utf8]");
                // Escape and quote strings for JSON-like output
                format!("'{}'", escape_string(s))
            }
        } else if jsval.is_bigint() {
            let ptr = jsval.as_bigint_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                if str_ptr.is_null() {
                    "0n".to_string()
                } else {
                    let len = (*str_ptr).length as usize;
                    let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    let num_str = std::str::from_utf8(bytes).unwrap_or("0");
                    format!("{}n", num_str)
                }
            }
        } else if jsval.is_pointer() {
            let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
            if ptr.is_null() {
                "null".to_string()
            } else {
                // First check if this is an Error object
                let object_type = *(ptr as *const u32);
                if object_type == crate::error::OBJECT_TYPE_ERROR {
                    // Format Error as "Error: <message>"
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    let name_ptr = (*error_ptr).name;
                    let message_ptr = (*error_ptr).message;

                    let name_str = if name_ptr.is_null() {
                        "Error".to_string()
                    } else {
                        let len = (*name_ptr).length as usize;
                        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                    };

                    let message_str = if message_ptr.is_null() {
                        "".to_string()
                    } else {
                        let len = (*message_ptr).length as usize;
                        let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("").to_string()
                    };

                    if message_str.is_empty() {
                        name_str
                    } else {
                        format!("{}: {}", name_str, message_str)
                    }
                } else {
                    // Check if it's an object with keys
                    let obj_ptr = ptr as *const crate::object::ObjectHeader;
                    let keys_array = (*obj_ptr).keys_array;

                    if !keys_array.is_null() {
                        format_object_as_json(obj_ptr, depth)
                    } else {
                        // Check if array
                        let maybe_arr = ptr;
                        let length = (*maybe_arr).length as usize;
                        let capacity = (*maybe_arr).capacity as usize;

                        if capacity >= length && length < 1_000_000 && capacity < 10_000_000 {
                            let data_ptr = (maybe_arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
                            let mut parts: Vec<String> = Vec::with_capacity(length);
                            for i in 0..length {
                                let elem_value = *data_ptr.add(i);
                                parts.push(format_jsvalue_for_json(elem_value, depth + 1));
                            }
                            format!("[{}]", parts.join(", "))
                        } else {
                            "[object Object]".to_string()
                        }
                    }
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                (n as i64).to_string()
            } else {
                n.to_string()
            }
        }
    }
}

/// Escape special characters in a string for display
fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\'' => result.push_str("\\'"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
    }
    result
}

#[no_mangle]
pub extern "C" fn js_console_log_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!();
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue(value, 0));
        }
        println!("{}", parts.join(" "));
    }
}

/// Print multiple values to stderr (console.error with spread support)
#[no_mangle]
pub extern "C" fn js_console_error_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        eprintln!();
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue(value, 0));
        }
        eprintln!("{}", parts.join(" "));
    }
}

/// Print multiple values to stderr (console.warn with spread support)
#[no_mangle]
pub extern "C" fn js_console_warn_spread(arr_ptr: *const crate::array::ArrayHeader) {
    // console.warn is essentially the same as console.error in Node.js
    js_console_error_spread(arr_ptr);
}

/// Print an array in the format [element1, element2, ...]
#[no_mangle]
pub extern "C" fn js_array_print(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!("null");
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue_for_json(value, 0));
        }
        println!("[{}]", parts.join(", "));
    }
}

// Arithmetic operations on JSValue (with type coercion)

#[no_mangle]
pub extern "C" fn js_add(a: JSValue, b: JSValue) -> JSValue {
    // For MVP, just handle number + number
    JSValue::number(a.to_number() + b.to_number())
}

#[no_mangle]
pub extern "C" fn js_sub(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() - b.to_number())
}

#[no_mangle]
pub extern "C" fn js_mul(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() * b.to_number())
}

#[no_mangle]
pub extern "C" fn js_div(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() / b.to_number())
}

#[no_mangle]
pub extern "C" fn js_mod(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() % b.to_number())
}

// Comparison operations

#[no_mangle]
pub extern "C" fn js_eq(a: JSValue, b: JSValue) -> JSValue {
    // Strict equality for numbers
    if a.is_number() && b.is_number() {
        JSValue::bool(a.as_number() == b.as_number())
    } else if a.bits() == b.bits() {
        JSValue::bool(true)
    } else {
        JSValue::bool(false)
    }
}

#[no_mangle]
pub extern "C" fn js_lt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() < b.to_number())
}

#[no_mangle]
pub extern "C" fn js_le(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() <= b.to_number())
}

#[no_mangle]
pub extern "C" fn js_gt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() > b.to_number())
}

#[no_mangle]
pub extern "C" fn js_ge(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() >= b.to_number())
}

/// Return the typeof a value as a string
/// Takes an f64 that uses NaN-boxing to distinguish types.
/// Returns a pointer to a string: "undefined", "boolean", "number", "string", "object", "function"
///
/// Optimization: typeof only returns 7 possible strings, so we cache them as
/// pre-allocated StringHeader pointers to avoid heap allocation on every call.
#[no_mangle]
pub extern "C" fn js_value_typeof(value: f64) -> *mut StringHeader {
    use std::cell::Cell;

    thread_local! {
        static TYPEOF_UNDEFINED: Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_OBJECT:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_BOOLEAN:   Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_NUMBER:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_STRING:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_FUNCTION:  Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_BIGINT:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
    }

    /// Get or initialize a cached typeof string.
    fn get_cached(
        cache: &'static std::thread::LocalKey<Cell<*mut StringHeader>>,
        s: &str,
    ) -> *mut StringHeader {
        cache.with(|cell| {
            let ptr = cell.get();
            if !ptr.is_null() {
                return ptr;
            }
            let new_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            cell.set(new_ptr);
            new_ptr
        })
    }

    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        get_cached(&TYPEOF_UNDEFINED, "undefined")
    } else if jsval.is_null() {
        // typeof null === "object" in JavaScript
        get_cached(&TYPEOF_OBJECT, "object")
    } else if jsval.is_bool() {
        get_cached(&TYPEOF_BOOLEAN, "boolean")
    } else if jsval.is_string() {
        // String pointer (uses STRING_TAG)
        get_cached(&TYPEOF_STRING, "string")
    } else if crate::value::is_js_handle(value) {
        // JS handle from V8 runtime - always an object
        get_cached(&TYPEOF_OBJECT, "object")
    } else if jsval.is_pointer() {
        // Object/array/closure pointer - check if it's a closure
        let ptr = jsval.as_pointer::<u8>();
        if !ptr.is_null() && (ptr as usize) > 0x10000 {
            // ClosureHeader has type_tag at offset 12 (after func_ptr:8 + capture_count:4)
            let type_tag = unsafe { *(ptr.add(12) as *const u32) };
            if type_tag == crate::closure::CLOSURE_MAGIC {
                get_cached(&TYPEOF_FUNCTION, "function")
            } else {
                get_cached(&TYPEOF_OBJECT, "object")
            }
        } else {
            get_cached(&TYPEOF_OBJECT, "object")
        }
    } else if jsval.is_bigint() {
        get_cached(&TYPEOF_BIGINT, "bigint")
    } else if jsval.is_int32() {
        get_cached(&TYPEOF_NUMBER, "number")
    } else {
        // Regular f64 number
        get_cached(&TYPEOF_NUMBER, "number")
    }
}

/// parseInt(string, radix?) -> number
/// Parses a string and returns an integer.
/// If the string cannot be parsed, returns NaN.
#[no_mangle]
pub extern "C" fn js_parse_int(str_ptr: *const StringHeader, radix: f64) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }

    unsafe {
        let len = (*str_ptr).length as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        if let Ok(s) = std::str::from_utf8(bytes) {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return f64::NAN;
            }

            // Determine radix
            let radix = if radix.is_nan() || radix == 0.0 {
                10
            } else {
                radix as u32
            };

            // Handle sign
            let (is_negative, trimmed) = if trimmed.starts_with('-') {
                (true, &trimmed[1..])
            } else if trimmed.starts_with('+') {
                (false, &trimmed[1..])
            } else {
                (false, trimmed)
            };

            // Handle hex prefix (only if radix is 16 or auto)
            let (actual_radix, trimmed) = if (radix == 16 || radix == 10) &&
                (trimmed.starts_with("0x") || trimmed.starts_with("0X")) {
                (16, &trimmed[2..])
            } else {
                (radix, trimmed)
            };

            // Parse characters until we hit a non-digit
            let valid_chars: String = trimmed.chars()
                .take_while(|c| c.is_digit(actual_radix))
                .collect();

            if valid_chars.is_empty() {
                return f64::NAN;
            }

            match i64::from_str_radix(&valid_chars, actual_radix) {
                Ok(n) => {
                    let result = if is_negative { -n } else { n };
                    result as f64
                }
                Err(_) => f64::NAN,
            }
        } else {
            f64::NAN
        }
    }
}

/// parseFloat(string) -> number
/// Parses a string and returns a floating-point number.
#[no_mangle]
pub extern "C" fn js_parse_float(str_ptr: *const StringHeader) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }

    unsafe {
        let len = (*str_ptr).length as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        if let Ok(s) = std::str::from_utf8(bytes) {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return f64::NAN;
            }

            // Parse as much of the string as is a valid float
            // JavaScript parseFloat stops at first invalid character
            let valid_chars: String = trimmed.chars()
                .scan(false, |seen_dot, c| {
                    if c.is_ascii_digit() {
                        Some(c)
                    } else if c == '.' && !*seen_dot {
                        *seen_dot = true;
                        Some(c)
                    } else if c == '-' || c == '+' {
                        Some(c)
                    } else if c == 'e' || c == 'E' {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();

            match valid_chars.parse::<f64>() {
                Ok(n) => n,
                Err(_) => f64::NAN,
            }
        } else {
            f64::NAN
        }
    }
}

/// Number(value) -> number
/// Converts a value to a number.
#[no_mangle]
pub extern "C" fn js_number_coerce(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() { 1.0 } else { 0.0 }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            return f64::NAN;
        }
        unsafe {
            let len = (*ptr).length as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            if let Ok(s) = std::str::from_utf8(bytes) {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return 0.0;
                }
                // Handle hex strings (0x/0X prefix) — JavaScript Number() supports these
                if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                    return match u64::from_str_radix(&trimmed[2..], 16) {
                        Ok(n) => n as f64,
                        Err(_) => f64::NAN,
                    };
                }
                // Handle negative hex
                if trimmed.starts_with("-0x") || trimmed.starts_with("-0X") {
                    return match u64::from_str_radix(&trimmed[3..], 16) {
                        Ok(n) => -(n as f64),
                        Err(_) => f64::NAN,
                    };
                }
                match trimmed.parse::<f64>() {
                    Ok(n) => n,
                    Err(_) => f64::NAN,
                }
            } else {
                f64::NAN
            }
        }
    } else if jsval.is_int32() {
        // INT32 NaN-boxed value → convert to f64
        jsval.as_int32() as f64
    } else if jsval.is_bigint() {
        // BigInt → number conversion
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_f64(ptr)
    } else if jsval.is_pointer() {
        // Object → NaN (can't convert object to number directly)
        f64::NAN
    } else {
        // Already a number
        value
    }
}

/// String(value) -> string
/// Converts a value to a string.
#[no_mangle]
pub extern "C" fn js_string_coerce(value: f64) -> *mut StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    let result = if jsval.is_undefined() {
        "undefined".to_string()
    } else if jsval.is_null() {
        "null".to_string()
    } else if jsval.is_bool() {
        if jsval.as_bool() { "true".to_string() } else { "false".to_string() }
    } else if jsval.is_string() {
        // Already a string, return as-is
        return jsval.as_string_ptr() as *mut StringHeader;
    } else if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        if ptr.is_null() {
            "0".to_string()
        } else {
            let str_ptr = crate::bigint::js_bigint_to_string(ptr);
            return str_ptr as *mut StringHeader;
        }
    } else if jsval.is_pointer() {
        // Pointer type — could be array or object.
        // Delegate to js_jsvalue_to_string which handles arrays via join(",") and objects.
        return crate::value::js_jsvalue_to_string(value);
    } else if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else {
        // Regular number
        let n = value;
        if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
        } else if n == 0.0 {
            "0".to_string()
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            (n as i64).to_string()
        } else {
            n.to_string()
        }
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// isNaN(value) -> boolean
/// Returns true if value is NaN.
#[no_mangle]
pub extern "C" fn js_is_nan(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    // isNaN first coerces to number, then checks for NaN
    let num = if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() { 1.0 } else { 0.0 }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        0.0
                    } else {
                        match trimmed.parse::<f64>() {
                            Ok(n) => n,
                            Err(_) => f64::NAN,
                        }
                    }
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        value
    };

    // Return NaN-boxed boolean (TAG_TRUE / TAG_FALSE)
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if num.is_nan() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// isFinite(value) -> boolean
/// Returns true if value is a finite number.
#[no_mangle]
pub extern "C" fn js_is_finite(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    // isFinite first coerces to number, then checks for finite
    let num = if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() { 1.0 } else { 0.0 }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).length as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        0.0
                    } else {
                        match trimmed.parse::<f64>() {
                            Ok(n) => n,
                            Err(_) => f64::NAN,
                        }
                    }
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        value
    };

    // Return NaN-boxed boolean (TAG_TRUE / TAG_FALSE)
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if num.is_finite() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

const NB_TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const NB_TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

/// Number.isNaN(value) -> boolean (strict, no coercion)
/// Returns true only if value is a plain number AND that number is NaN.
#[no_mangle]
pub extern "C" fn js_number_is_nan(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    // Strict: only plain numbers can be NaN. Any NaN-boxed tag type => false.
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_nan() {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isFinite(value) -> boolean (strict, no coercion)
/// Returns true only if value is a plain finite number.
#[no_mangle]
pub extern "C" fn js_number_is_finite(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_finite() {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isInteger(value) -> boolean
/// Returns true if value is a finite number with no fractional part.
#[no_mangle]
pub extern "C" fn js_number_is_integer(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_finite() && n.floor() == n {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isSafeInteger(value) -> boolean
/// Returns true if value is an integer within ±(2^53 - 1).
#[no_mangle]
pub extern "C" fn js_number_is_safe_integer(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    const MAX_SAFE: f64 = 9007199254740991.0;
    if n.is_finite() && n.floor() == n && n.abs() <= MAX_SAFE {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Debug trace for module initialization order.
/// Called before each _perry_init_* call to identify which module crashes.
/// No-op in release builds; re-enable eprintln for debugging.
#[no_mangle]
pub extern "C" fn perry_debug_trace_init(_index: i64, _name_ptr: *const u8, _name_len: i64) {
}

#[no_mangle]
pub extern "C" fn perry_debug_trace_init_done(_index: i64) {
}

// === console.time / timeEnd / timeLog ===
//
// Per-thread map from label string to start Instant. Matches Node's
// behavior of warning on duplicate labels and on missing labels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

thread_local! {
    static CONSOLE_TIMERS: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    static CONSOLE_COUNTERS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
}

unsafe fn label_from_str_ptr(ptr: *const StringHeader) -> String {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return "default".to_string();
    }
    let len = (*ptr).length as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("default").to_string()
}

fn format_elapsed(dur: std::time::Duration) -> String {
    let ms = dur.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{:.3}ms", ms)
    } else if ms < 1000.0 {
        format!("{:.3}ms", ms)
    } else {
        format!("{:.3}s", dur.as_secs_f64())
    }
}

#[no_mangle]
pub extern "C" fn js_console_time(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let mut map = t.borrow_mut();
        if map.contains_key(&label) {
            eprintln!("Warning: Label '{}' already exists for console.time()", label);
        }
        map.insert(label, Instant::now());
    });
}

#[no_mangle]
pub extern "C" fn js_console_time_end(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let mut map = t.borrow_mut();
        match map.remove(&label) {
            Some(start) => println!("{}: {}", label, format_elapsed(start.elapsed())),
            None => eprintln!("Warning: No such label '{}' for console.timeEnd()", label),
        }
    });
}

#[no_mangle]
pub extern "C" fn js_console_time_log(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let map = t.borrow();
        match map.get(&label) {
            Some(start) => println!("{}: {}", label, format_elapsed(start.elapsed())),
            None => eprintln!("Warning: No such label '{}' for console.timeLog()", label),
        }
    });
}

// === console.count / countReset ===

#[no_mangle]
pub extern "C" fn js_console_count(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_COUNTERS.with(|c| {
        let mut map = c.borrow_mut();
        let entry = map.entry(label.clone()).or_insert(0);
        *entry += 1;
        println!("{}: {}", label, *entry);
    });
}

#[no_mangle]
pub extern "C" fn js_console_count_reset(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_COUNTERS.with(|c| {
        let mut map = c.borrow_mut();
        if map.remove(&label).is_none() {
            eprintln!("Warning: Count for '{}' does not exist", label);
        }
    });
}

// === console.group / groupEnd / groupCollapsed ===
//
// Just print the label like console.log; we don't track indent yet.

#[no_mangle]
pub extern "C" fn js_console_group(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    println!("{}", label);
}

#[no_mangle]
pub extern "C" fn js_console_group_end() {
    // No-op until we add indent tracking.
}

// === console.assert ===
//
// Prints "Assertion failed" + the message string when the condition is false.

#[no_mangle]
pub extern "C" fn js_console_assert(cond: f64, msg_ptr: *const StringHeader) {
    use crate::value::js_is_truthy;
    if js_is_truthy(cond) != 0 { return; }
    let msg = unsafe {
        if msg_ptr.is_null() || (msg_ptr as usize) < 0x1000 {
            String::new()
        } else {
            let len = (*msg_ptr).length as usize;
            let data = (msg_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        }
    };
    if msg.is_empty() {
        eprintln!("Assertion failed");
    } else {
        eprintln!("Assertion failed: {}", msg);
    }
}

// === console.clear ===
//
// Best-effort: emit ANSI clear sequence on stdout.

#[no_mangle]
pub extern "C" fn js_console_clear() {
    print!("\x1b[2J\x1b[H");
}

// ============================================================
// TextEncoder / TextDecoder
// ============================================================

/// TextEncoder.encode(string) -> Buffer (Uint8Array of UTF-8 bytes)
/// Takes a NaN-boxed string value and returns a raw buffer pointer.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode(value: f64) -> i64 {
    use crate::buffer::js_buffer_from_string;
    let str_ptr = crate::value::js_get_string_pointer_unified(value);
    let buf = js_buffer_from_string(str_ptr as *const StringHeader, 0); // 0 = UTF-8
    buf as i64
}

/// TextDecoder.decode(buffer_ptr) -> string pointer (i64)
/// Takes a raw buffer/Uint8Array pointer (i64) and returns a StringHeader pointer.
#[no_mangle]
pub extern "C" fn js_text_decoder_decode(buf_ptr: i64) -> i64 {
    use crate::buffer::{BufferHeader, js_buffer_to_string};
    if buf_ptr == 0 || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(std::ptr::null(), 0) as i64;
    }
    let ptr = buf_ptr as *const BufferHeader;
    let str_ptr = js_buffer_to_string(ptr, 0); // 0 = UTF-8
    str_ptr as i64
}

// ============================================================
// encodeURI / decodeURI / encodeURIComponent / decodeURIComponent
// ============================================================

/// Characters that encodeURI does NOT encode (RFC 2396 unreserved + reserved)
const URI_UNESCAPED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
const URI_RESERVED: &[u8] = b";/?:@&=+$,#";

/// Characters that encodeURIComponent does NOT encode (RFC 2396 unreserved only)
const URI_COMPONENT_UNESCAPED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";

fn percent_encode(input: &str, safe_chars: &[u8]) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input.as_bytes() {
        if safe_chars.contains(byte) {
            result.push(*byte as char);
        } else {
            result.push('%');
            result.push_str(&format!("{:02X}", byte));
        }
    }
    result
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_digit(bytes[i + 1]);
            let lo = hex_digit(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                result.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn extract_str_from_nanbox(value: f64) -> String {
    let str_ptr = crate::value::js_get_string_pointer_unified(value);
    if (str_ptr as usize) < 0x1000 {
        return String::new();
    }
    unsafe {
        let header = str_ptr as *const StringHeader;
        let len = (*header).length as usize;
        let data = (header as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("").to_string()
    }
}

/// encodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let mut safe = Vec::with_capacity(URI_UNESCAPED.len() + URI_RESERVED.len());
    safe.extend_from_slice(URI_UNESCAPED);
    safe.extend_from_slice(URI_RESERVED);
    let encoded = percent_encode(&input, &safe);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let decoded = percent_decode(&input);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

/// encodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri_component(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let encoded = percent_encode(&input, URI_COMPONENT_UNESCAPED);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri_component(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let decoded = percent_decode(&input);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

// ============================================================
// structuredClone
// ============================================================

/// structuredClone(value) -> deep-cloned value
/// Handles numbers (pass-through), strings (copy), arrays/objects (shallow for now)
#[no_mangle]
pub extern "C" fn js_structured_clone(value: f64) -> f64 {
    let bits = value.to_bits();
    // Pass through primitives (undefined, null, true, false)
    if bits == 0x7FFC_0000_0000_0001 || bits == 0x7FFC_0000_0000_0002
        || bits == 0x7FFC_0000_0000_0003 || bits == 0x7FFC_0000_0000_0004 {
        return value;
    }
    // Regular f64 numbers pass through
    let tag = (bits >> 48) as u16;
    if tag < 0x7FF8 {
        return value;
    }

    match tag {
        0x7FFF => {
            // STRING_TAG — copy the string
            let str_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if (str_ptr as usize) < 0x1000 {
                return value;
            }
            unsafe {
                let len = (*str_ptr).length as usize;
                let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let new_str = js_string_from_bytes(data, len as u32);
                let new_bits = 0x7FFF_0000_0000_0000u64 | (new_str as u64 & 0x0000_FFFF_FFFF_FFFF);
                f64::from_bits(new_bits)
            }
        }
        0x7FFE => {
            // INT32_TAG — pass through
            value
        }
        0x7FFD => {
            // POINTER_TAG — could be array or object. Deep clone recursively.
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
            if (ptr as usize) < 0x10000 { return value; }
            unsafe {
                // GcHeader is stored BEFORE the user pointer (at ptr - GC_HEADER_SIZE)
                let gc_header_ptr = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE);
                let gc_type = *gc_header_ptr;
                if gc_type == crate::gc::GC_TYPE_ARRAY {
                    // Clone array using existing clone, then recursively clone elements
                    let arr = ptr as *const crate::array::ArrayHeader;
                    let new_arr = crate::array::js_array_clone(arr);
                    let len = (*new_arr).length;
                    let elements = (new_arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut f64;
                    for i in 0..len as usize {
                        let elem = *elements.add(i);
                        *elements.add(i) = js_structured_clone(elem);
                    }
                    let new_bits = 0x7FFD_0000_0000_0000u64 | (new_arr as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                    // Clone object using clone_with_extra (0 extra fields, no static keys)
                    let cloned_obj = crate::object::js_object_clone_with_extra(value, 0, std::ptr::null(), 0);
                    if !cloned_obj.is_null() && (cloned_obj as usize) > 0x10000 {
                        let field_count = (*cloned_obj).field_count;
                        let fields = (cloned_obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut f64;
                        for i in 0..field_count as usize {
                            let field = *fields.add(i);
                            *fields.add(i) = js_structured_clone(field);
                        }
                    }
                    // NaN-box with POINTER_TAG
                    let new_bits = 0x7FFD_0000_0000_0000u64 | (cloned_obj as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else {
                    // Unknown pointer type — pass through
                    value
                }
            }
        }
        _ => value,
    }
}

// ============================================================
// queueMicrotask
// ============================================================

/// queueMicrotask(callback) — calls the closure immediately (simplified)
/// In a full implementation this would schedule on the microtask queue,
/// but Perry's current event loop processes microtasks synchronously.
#[no_mangle]
pub extern "C" fn js_queue_microtask(callback: i64) {
    use crate::closure::js_closure_call0;
    unsafe {
        js_closure_call0(callback as *const crate::closure::ClosureHeader);
    }
}
