//! String runtime support for Perry
//!
//! Strings are heap-allocated UTF-8 sequences with capacity for efficient appending.
//! Layout:
//!   - StringHeader at the start
//!   - Followed by `capacity` bytes of data (only `length` bytes are valid)

use std::ptr;
use std::slice;
use std::str;

/// A static empty string that can be used as a safe fallback for null pointers.
/// Has length=0, capacity=0, refcount=0 (shared). The address is valid and .length returns 0.
#[no_mangle]
pub static PERRY_EMPTY_STRING: StringHeader = StringHeader { length: 0, capacity: 0, refcount: 0 };

/// Get a pointer to the static empty string (for codegen null guards).
#[no_mangle]
pub extern "C" fn js_get_empty_string() -> *const StringHeader {
    &PERRY_EMPTY_STRING as *const StringHeader
}

/// Check if a pointer is valid (not null and not a small invalid value from bad NaN-unboxing).
/// When codegen extracts a "pointer" from TAG_UNDEFINED (0x7FFC_0000_0000_0001), the lower
/// 48-bit AND yields 1, which passes is_null() but crashes on dereference.
#[inline]
pub fn is_valid_string_ptr(p: *const StringHeader) -> bool {
    !p.is_null() && (p as usize) >= 0x1000
}

/// Header for heap-allocated strings
///
/// The `refcount` field enables in-place append optimization in `js_string_append`:
/// - refcount=0: shared/unknown ownership — never mutated in-place (safe default)
/// - refcount=1: unique owner — `js_string_append` can append in-place if capacity allows
/// Only strings created by `js_string_append` get refcount=1. When a string pointer is
/// copied to another variable, codegen calls `js_string_addref` to set refcount=0 (shared).
#[repr(C)]
pub struct StringHeader {
    /// Length in bytes (not chars - we store UTF-8)
    pub length: u32,
    /// Capacity (allocated space for data)
    pub capacity: u32,
    /// Reference hint: 0=shared (never mutate in-place), 1=unique (in-place append OK)
    pub refcount: u32,
}

/// Create a string from raw bytes
/// Returns a pointer to StringHeader
#[no_mangle]
pub extern "C" fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    js_string_from_bytes_with_capacity(data, len, len)
}

/// Create a string from raw bytes with extra capacity for future appending
#[no_mangle]
pub extern "C" fn js_string_from_bytes_with_capacity(data: *const u8, len: u32, capacity: u32) -> *mut StringHeader {
    let capacity = capacity.max(len); // Ensure capacity >= len
    let total_size = std::mem::size_of::<StringHeader>() + capacity as usize;

    let raw = crate::gc::gc_malloc(total_size, crate::gc::GC_TYPE_STRING);
    let ptr = raw as *mut StringHeader;

    unsafe {
        (*ptr).length = len;
        (*ptr).capacity = capacity;
        (*ptr).refcount = 0; // shared by default — caller can set to 1 if uniquely owned

        // Copy string data after header
        if len > 0 && !data.is_null() {
            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }

    ptr
}

/// Append a string to another string in-place if possible.
/// Returns the (possibly new) string pointer.
///
/// When capacity is exceeded, allocates a fresh string and copies both
/// dest and src content into it. This avoids gc_realloc entirely, which
/// prevents stale-pointer issues when the conservative GC scanner misses
/// pointers in caller-saved registers. The old string becomes garbage and
/// is collected in the next GC cycle.
#[no_mangle]
pub extern "C" fn js_string_append(dest: *mut StringHeader, src: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(dest as *const StringHeader) {
        // If dest is invalid, just duplicate src
        if !is_valid_string_ptr(src) {
            return js_string_from_bytes(ptr::null(), 0);
        }
        let src_len = unsafe { (*src).length };
        let src_data = string_data(src);
        return js_string_from_bytes(src_data, src_len);
    }

    if !is_valid_string_ptr(src) {
        return dest;
    }

    // Self-append (s += s): must allocate fresh to avoid reading from
    // memory that is being written to.
    if dest as *const StringHeader == src {
        return js_string_concat(dest as *const StringHeader, src);
    }

    unsafe {
        let dest_len = (*dest).length;
        let src_len = (*src).length;

        if src_len == 0 {
            return dest;
        }

        let new_len = dest_len + src_len;

        // In-place append optimization: if dest is uniquely owned (refcount==1)
        // and has enough capacity, append directly without allocation.
        // This turns O(n^2) string building loops into amortized O(n).
        if (*dest).refcount == 1 && new_len <= (*dest).capacity {
            let dest_data = (dest as *mut u8).add(std::mem::size_of::<StringHeader>());
            let src_data_ptr = string_data(src);
            ptr::copy_nonoverlapping(src_data_ptr, dest_data.add(dest_len as usize), src_len as usize);
            (*dest).length = new_len;
            return dest; // Same pointer, no allocation!
        }

        // Allocate fresh with 2x capacity for future in-place appends.
        // Perry aliases strings through `let x = y` (pointer copy), so in-place
        // mutation of shared strings would corrupt other references.
        // We do NOT use gc_realloc here because the conservative GC scanner
        // may have already swept the dest string (pointer in a caller-saved
        // register that setjmp/stack-walk didn't capture). Fresh allocation
        // is safe: old string becomes garbage for the next GC cycle.
        let new_cap = (new_len * 2).max(32);
        let new_ptr = js_string_from_bytes_with_capacity(ptr::null(), 0, new_cap);

        // Copy old dest content
        let new_data = (new_ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
        let dest_data = (dest as *const u8).add(std::mem::size_of::<StringHeader>());
        ptr::copy_nonoverlapping(dest_data, new_data, dest_len as usize);

        // Copy src content after dest content
        let src_data_ptr = string_data(src);
        ptr::copy_nonoverlapping(src_data_ptr, new_data.add(dest_len as usize), src_len as usize);
        (*new_ptr).length = new_len;

        // Mark as uniquely owned — the caller (codegen) is about to assign
        // this pointer to a single variable, so in-place append is safe next time.
        (*new_ptr).refcount = 1;

        new_ptr
    }
}

/// Create an empty string with initial capacity (for building strings)
#[no_mangle]
pub extern "C" fn js_string_builder_new(initial_capacity: u32) -> *mut StringHeader {
    js_string_from_bytes_with_capacity(ptr::null(), 0, initial_capacity.max(16))
}

/// Mark a string as shared (refcount=0) so `js_string_append` won't mutate it in-place.
/// Called by codegen when a string pointer is copied to another variable (`let y = x`),
/// passed as a function argument, or stored into an array/object.
/// This is a NaN-boxed f64 input — extract the raw pointer first.
#[no_mangle]
pub extern "C" fn js_string_addref(s: *mut StringHeader) {
    if is_valid_string_ptr(s as *const StringHeader) {
        unsafe {
            (*s).refcount = 0; // Mark as shared — prevent in-place mutation
        }
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
#[inline]
fn js_string_from_str(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Get string length (in bytes for now, chars would need UTF-8 counting)
#[no_mangle]
pub extern "C" fn js_string_length(s: *const StringHeader) -> u32 {
    if !is_valid_string_ptr(s) {
        return 0;
    }
    unsafe { (*s).length }
}

/// Get the data pointer for a string
fn string_data(s: *const StringHeader) -> *const u8 {
    unsafe {
        (s as *const u8).add(std::mem::size_of::<StringHeader>())
    }
}

/// Get string as a Rust &str (for internal use)
fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe {
        let len = (*s).length as usize;
        let cap = (*s).capacity as usize;
        debug_assert!(len <= cap, "StringHeader length {} > capacity {}", len, cap);
        let data = string_data(s);
        let bytes = slice::from_raw_parts(data, len);
        str::from_utf8_unchecked(bytes)
    }
}

/// Concatenate two strings
#[no_mangle]
pub extern "C" fn js_string_concat(a: *const StringHeader, b: *const StringHeader) -> *mut StringHeader {
    let len_a = if is_valid_string_ptr(a) { unsafe { (*a).length } } else { 0 };
    let len_b = if is_valid_string_ptr(b) { unsafe { (*b).length } } else { 0 };
    let total_len = len_a + len_b;

    let total_size = std::mem::size_of::<StringHeader>() + total_len as usize;

    let raw = crate::gc::gc_malloc(total_size, crate::gc::GC_TYPE_STRING);
    let ptr = raw as *mut StringHeader;

    unsafe {
        (*ptr).length = total_len;
        (*ptr).capacity = total_len;
        (*ptr).refcount = 0; // shared by default

        let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());

        if is_valid_string_ptr(a) && len_a > 0 {
            ptr::copy_nonoverlapping(string_data(a), data_ptr, len_a as usize);
        }
        if is_valid_string_ptr(b) && len_b > 0 {
            ptr::copy_nonoverlapping(string_data(b), data_ptr.add(len_a as usize), len_b as usize);
        }

        ptr
    }
}

/// Convert a number (f64) to a string
/// Returns a new string representing the number
#[no_mangle]
pub extern "C" fn js_number_to_string(value: f64) -> *mut StringHeader {
    // Format the number as a string per JS semantics.
    let s = if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else if value == 0.0 {
        // Cover both +0 and -0 as "0" (matches JS)
        "0".to_string()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        // Integer-like, format without decimal
        format!("{}", value as i64)
    } else {
        // Float, format with appropriate precision
        format!("{}", value)
    };

    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number with a fixed number of decimal places (Number.prototype.toFixed)
#[no_mangle]
pub extern "C" fn js_number_to_fixed(value: f64, decimals: f64) -> *mut StringHeader {
    let dp = decimals as usize;
    let s = format!("{:.prec$}", value, prec = dp);
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get a slice of a string (byte-based for now)
/// Returns a new string from start to end (exclusive)
#[no_mangle]
pub extern "C" fn js_string_slice(s: *const StringHeader, start: i32, end: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let len = unsafe { (*s).length } as i32;

    // Handle negative indices (from end)
    let start = if start < 0 { (len + start).max(0) } else { start.min(len) };
    let end = if end < 0 { (len + end).max(0) } else { end.min(len) };

    if start >= end {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let slice_len = (end - start) as u32;
    unsafe {
        let src = string_data(s).add(start as usize);
        js_string_from_bytes(src, slice_len)
    }
}

/// Get a substring (similar to slice but different behavior)
/// - Negative indices are treated as 0
/// - If start > end, arguments are swapped
/// Returns a new string from start to end (exclusive)
#[no_mangle]
pub extern "C" fn js_string_substring(s: *const StringHeader, start: i32, end: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let len = unsafe { (*s).length } as i32;

    // Treat negative indices as 0
    let mut start = start.max(0).min(len);
    let mut end = end.max(0).min(len);

    // Swap if start > end
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }

    if start >= end {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let slice_len = (end - start) as u32;
    unsafe {
        let src = string_data(s).add(start as usize);
        js_string_from_bytes(src, slice_len)
    }
}

/// Trim whitespace from both ends of a string
#[no_mangle]
pub extern "C" fn js_string_trim(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let trimmed = str_data.trim();
    js_string_from_str(trimmed)
}

/// Trim whitespace from start of a string (trimStart/trimLeft)
#[no_mangle]
pub extern "C" fn js_string_trim_start(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    js_string_from_str(str_data.trim_start())
}

/// Trim whitespace from end of a string (trimEnd/trimRight)
#[no_mangle]
pub extern "C" fn js_string_trim_end(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    js_string_from_str(str_data.trim_end())
}

/// Convert string to lowercase
#[no_mangle]
pub extern "C" fn js_string_to_lower_case(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let lower = str_data.to_lowercase();
    js_string_from_str(&lower)
}

/// Convert string to uppercase
#[no_mangle]
pub extern "C" fn js_string_to_upper_case(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let upper = str_data.to_uppercase();
    js_string_from_str(&upper)
}

/// Find index of substring (-1 if not found)
#[no_mangle]
pub extern "C" fn js_string_index_of(haystack: *const StringHeader, needle: *const StringHeader) -> i32 {
    js_string_index_of_from(haystack, needle, 0)
}

/// Find index of substring starting from a given position (-1 if not found)
#[no_mangle]
pub extern "C" fn js_string_index_of_from(haystack: *const StringHeader, needle: *const StringHeader, from_index: i32) -> i32 {
    if !is_valid_string_ptr(haystack) || !is_valid_string_ptr(needle) {
        return -1;
    }

    let h = string_as_str(haystack);
    let n = string_as_str(needle);

    // Handle negative or out-of-bounds start index
    let start = if from_index < 0 { 0 } else { from_index as usize };
    if start >= h.len() {
        return -1;
    }

    // Search from the start position
    match h[start..].find(n) {
        Some(pos) => (start + pos) as i32,
        None => -1,
    }
}

/// Find the last index of a substring (-1 if not found).
/// Matches JS `String.prototype.lastIndexOf(searchValue)` semantics: returns the
/// byte offset of the LAST occurrence of `needle` in `haystack`, or -1 if not found.
/// An empty needle returns `haystack.length`.
#[no_mangle]
pub extern "C" fn js_string_last_index_of(haystack: *const StringHeader, needle: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(haystack) {
        return -1;
    }
    // If needle is invalid (null), treat as empty string (JS coerces undefined to "undefined", but
    // an empty string matches at every position and the last match is at haystack.length).
    if !is_valid_string_ptr(needle) {
        let h = string_as_str(haystack);
        return h.len() as i32;
    }

    let h = string_as_str(haystack);
    let n = string_as_str(needle);

    // Empty needle: per JS spec, returns the haystack length.
    if n.is_empty() {
        return h.len() as i32;
    }

    match h.rfind(n) {
        Some(pos) => pos as i32,
        None => -1,
    }
}

/// Compare two strings lexicographically.
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
#[no_mangle]
pub extern "C" fn js_string_compare(a: *const StringHeader, b: *const StringHeader) -> i32 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0;
    }
    if !a_valid {
        return -1;
    }
    if !b_valid {
        return 1;
    }

    unsafe {
        let len_a = (*a).length as usize;
        let len_b = (*b).length as usize;
        let data_a = string_data(a);
        let data_b = string_data(b);
        let a_bytes = std::slice::from_raw_parts(data_a, len_a);
        let b_bytes = std::slice::from_raw_parts(data_b, len_b);
        match a_bytes.cmp(b_bytes) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Compare two strings for equality
#[no_mangle]
pub extern "C" fn js_string_equals(a: *const StringHeader, b: *const StringHeader) -> i32 {
    // Pointer identity fast path
    if std::ptr::eq(a, b) {
        return 1;
    }

    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 1;
    }
    if !a_valid || !b_valid {
        return 0;
    }

    let len_a = unsafe { (*a).length };
    let len_b = unsafe { (*b).length };

    if len_a != len_b {
        return 0;
    }

    unsafe {
        let data_a = string_data(a);
        let data_b = string_data(b);
        let slice_a = std::slice::from_raw_parts(data_a, len_a as usize);
        let slice_b = std::slice::from_raw_parts(data_b, len_b as usize);
        if slice_a == slice_b { 1 } else { 0 }
    }
}

/// Check if a string starts with a prefix
#[no_mangle]
pub extern "C" fn js_string_starts_with(s: *const StringHeader, prefix: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let len = unsafe { (*s).length };
    let prefix_len = unsafe { (*prefix).length };

    if prefix_len > len {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let prefix_data = string_data(prefix);

        for i in 0..prefix_len as usize {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with a suffix
#[no_mangle]
pub extern "C" fn js_string_ends_with(s: *const StringHeader, suffix: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let len = unsafe { (*s).length };
    let suffix_len = unsafe { (*suffix).length };

    if suffix_len > len {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let suffix_data = string_data(suffix);
        let start = len - suffix_len;

        for i in 0..suffix_len as usize {
            if *data.add(start as usize + i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Get character code at index (returns UTF-16 code unit, or NaN if out of bounds)
#[no_mangle]
pub extern "C" fn js_string_char_code_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::NAN;
    }

    let len = unsafe { (*s).length };
    if index as u32 >= len {
        return f64::NAN;
    }

    unsafe {
        let data = string_data(s);
        *data.add(index as usize) as f64
    }
}

/// Get character at index (returns single-character string, empty string if out of bounds)
#[no_mangle]
pub extern "C" fn js_string_char_at(s: *const StringHeader, index: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || index < 0 {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    let len = unsafe { (*s).length };
    if index as u32 >= len {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    unsafe {
        let data = string_data(s);
        let char_ptr = data.add(index as usize);
        js_string_from_bytes(char_ptr, 1)
    }
}

/// Create a string from a character code (String.fromCharCode)
/// Takes a single character code and returns a 1-character string
#[no_mangle]
pub extern "C" fn js_string_from_char_code(code: i32) -> *mut StringHeader {
    if code < 0 || code > 0xFFFF {
        // Invalid character code, return empty string
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // For ASCII characters, create a simple 1-byte string
    if code < 128 {
        let byte = code as u8;
        return js_string_from_bytes(&byte as *const u8, 1);
    }

    // For non-ASCII, encode as UTF-8
    let ch = char::from_u32(code as u32).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// Print a string to stdout
#[no_mangle]
pub extern "C" fn js_string_print(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        println!("");
        return;
    }

    let str_data = string_as_str(s);
    println!("{}", str_data);
}

/// Print a string to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_string_error(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        eprintln!("");
        return;
    }

    let str_data = string_as_str(s);
    eprintln!("{}", str_data);
}

/// Print a string to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_string_warn(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        eprintln!("");
        return;
    }

    let str_data = string_as_str(s);
    eprintln!("{}", str_data);
}

use crate::array::ArrayHeader;

/// Split a string by a delimiter
/// Returns an array of string pointers (stored as f64 bit patterns)
#[no_mangle]
pub extern "C" fn js_string_split(s: *const StringHeader, delimiter: *const StringHeader) -> *mut ArrayHeader {
    if !is_valid_string_ptr(s) {
        // Return empty array
        return crate::array::js_array_alloc(0);
    }

    let str_data = string_as_str(s);
    let delim = if !is_valid_string_ptr(delimiter) {
        ""
    } else {
        string_as_str(delimiter)
    };

    // Split into string parts
    let parts: Vec<*mut StringHeader> = if delim.is_empty() {
        // Empty delimiter: split into individual characters (single pass)
        str_data.chars().map(|c| {
            let mut buf = [0u8; 4];
            let char_str = c.encode_utf8(&mut buf);
            js_string_from_bytes(char_str.as_ptr(), char_str.len() as u32)
        }).collect()
    } else {
        str_data.split(delim).map(|part| {
            js_string_from_bytes(part.as_ptr(), part.len() as u32)
        }).collect()
    };

    // Allocate array to hold string pointers
    // We store NaN-boxed string pointers (with STRING_TAG) since arrays use f64 storage
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    let arr = crate::array::js_array_alloc(parts.len() as u32);
    unsafe {
        (*arr).length = parts.len() as u32;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for (i, ptr) in parts.iter().enumerate() {
            // NaN-box the string pointer with STRING_TAG
            let ptr_as_u64 = *ptr as u64;
            let nanboxed = STRING_TAG | (ptr_as_u64 & POINTER_MASK);
            let ptr_as_f64 = f64::from_bits(nanboxed);
            std::ptr::write(elements_ptr.add(i), ptr_as_f64);
        }
    }

    arr
}

/// Allocate a string containing a single space character " "
/// Used as default pad string for padStart/padEnd
#[no_mangle]
pub extern "C" fn js_string_alloc_space() -> *mut StringHeader {
    js_string_from_bytes(" ".as_ptr(), 1)
}

/// Pad the start of a string to reach target length
/// str.padStart(targetLength, padString)
#[no_mangle]
pub extern "C" fn js_string_pad_start(s: *const StringHeader, target_length: u32, pad_string: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) { string_as_str(pad_string) } else { " " };

    let current_len = str_data.chars().count();
    let target_len = target_length as usize;

    if current_len >= target_len || pad_data.is_empty() {
        // Return a copy of the original string
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    let pad_needed = target_len - current_len;
    let mut result = String::with_capacity(target_len * 4); // UTF-8 can be up to 4 bytes per char

    // Build padding
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    for _ in 0..pad_needed {
        result.push(pad_chars[pad_idx % pad_chars.len()]);
        pad_idx += 1;
    }

    // Append original string
    result.push_str(str_data);

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Pad the end of a string to reach target length
/// str.padEnd(targetLength, padString)
#[no_mangle]
pub extern "C" fn js_string_pad_end(s: *const StringHeader, target_length: u32, pad_string: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) { string_as_str(pad_string) } else { " " };

    let current_len = str_data.chars().count();
    let target_len = target_length as usize;

    if current_len >= target_len || pad_data.is_empty() {
        // Return a copy of the original string
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    let pad_needed = target_len - current_len;
    let mut result = String::with_capacity(target_len * 4); // UTF-8 can be up to 4 bytes per char

    // Start with original string
    result.push_str(str_data);

    // Build padding
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    for _ in 0..pad_needed {
        result.push(pad_chars[pad_idx % pad_chars.len()]);
        pad_idx += 1;
    }

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Repeat a string a specified number of times
/// str.repeat(count)
#[no_mangle]
pub extern "C" fn js_string_repeat(s: *const StringHeader, count: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || count <= 0 {
        // Return empty string
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let str_data = string_as_str(s);
    if str_data.is_empty() {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let count = count as usize;
    let result = str_data.repeat(count);
    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// atob(base64) — decode a base64-encoded string to a binary string.
/// Input is a NaN-boxed STRING_TAG f64. Output is a raw *const StringHeader (codegen NaN-boxes).
#[no_mangle]
pub extern "C" fn js_atob(value: f64) -> *const StringHeader {
    use base64::Engine as _;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != STRING_TAG {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let s = string_as_str(str_ptr);
    match base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
        Ok(decoded) => js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32),
        Err(_) => js_string_from_bytes(ptr::null(), 0),
    }
}

/// btoa(string) — base64-encode a binary string.
#[no_mangle]
pub extern "C" fn js_btoa(value: f64) -> *const StringHeader {
    use base64::Engine as _;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != STRING_TAG {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let s = string_as_str(str_ptr);
    let encoded = base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_create() {
        let data = b"hello";
        let s = js_string_from_bytes(data.as_ptr(), data.len() as u32);
        assert_eq!(js_string_length(s), 5);
    }

    #[test]
    fn test_string_concat() {
        let a = js_string_from_bytes(b"hello".as_ptr(), 5);
        let b = js_string_from_bytes(b" world".as_ptr(), 6);
        let c = js_string_concat(a, b);
        assert_eq!(js_string_length(c), 11);
        assert_eq!(string_as_str(c), "hello world");
    }

    #[test]
    fn test_string_slice() {
        let s = js_string_from_bytes(b"hello world".as_ptr(), 11);
        let slice = js_string_slice(s, 0, 5);
        assert_eq!(string_as_str(slice), "hello");

        let slice2 = js_string_slice(s, 6, 11);
        assert_eq!(string_as_str(slice2), "world");
    }

    #[test]
    fn test_string_index_of() {
        let s = js_string_from_bytes(b"hello world".as_ptr(), 11);
        let needle = js_string_from_bytes(b"world".as_ptr(), 5);
        assert_eq!(js_string_index_of(s, needle), 6);

        let not_found = js_string_from_bytes(b"xyz".as_ptr(), 3);
        assert_eq!(js_string_index_of(s, not_found), -1);
    }

    #[test]
    fn test_string_split() {
        use crate::array::{js_array_length, js_array_get_f64};

        let s = js_string_from_bytes(b"a,b,c".as_ptr(), 5);
        let delim = js_string_from_bytes(b",".as_ptr(), 1);
        let arr = js_string_split(s, delim);

        assert_eq!(js_array_length(arr), 3);

        // Get the string pointers from the array and verify their contents
        // Note: split() stores NaN-boxed string pointers with STRING_TAG
        const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
        const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

        unsafe {
            // Extract pointer from NaN-boxed value by masking off STRING_TAG
            let ptr0 = (js_array_get_f64(arr, 0).to_bits() & POINTER_MASK) as *const StringHeader;
            let ptr1 = (js_array_get_f64(arr, 1).to_bits() & POINTER_MASK) as *const StringHeader;
            let ptr2 = (js_array_get_f64(arr, 2).to_bits() & POINTER_MASK) as *const StringHeader;

            assert_eq!(string_as_str(ptr0), "a");
            assert_eq!(string_as_str(ptr1), "b");
            assert_eq!(string_as_str(ptr2), "c");
        }
    }

    #[test]
    fn test_string_append_inplace() {
        // First append: creates new string with 2x capacity and refcount=1
        let a = js_string_from_bytes(b"hello".as_ptr(), 5);
        let b = js_string_from_bytes(b" world".as_ptr(), 6);
        let result = js_string_append(a, b);
        assert_eq!(string_as_str(result), "hello world");
        assert_eq!(unsafe { (*result).refcount }, 1); // uniquely owned
        assert!(unsafe { (*result).capacity } >= 22); // 2x capacity

        // Second append: should reuse same allocation (in-place)
        let c = js_string_from_bytes(b"!".as_ptr(), 1);
        let result2 = js_string_append(result, c);
        assert_eq!(result2, result); // Same pointer — in-place append!
        assert_eq!(string_as_str(result2), "hello world!");
        assert_eq!(unsafe { (*result2).refcount }, 1); // still uniquely owned
    }

    #[test]
    fn test_string_append_shared_no_inplace() {
        // Create a string via append (refcount=1)
        let a = js_string_from_bytes(b"hello".as_ptr(), 5);
        let b = js_string_from_bytes(b" ".as_ptr(), 1);
        let result = js_string_append(a, b);
        assert_eq!(unsafe { (*result).refcount }, 1);

        // Mark as shared (simulates `let y = x` in codegen)
        js_string_addref(result);
        assert_eq!(unsafe { (*result).refcount }, 0); // shared

        // Append should NOT be in-place — must allocate fresh
        let c = js_string_from_bytes(b"world".as_ptr(), 5);
        let result2 = js_string_append(result, c);
        assert_ne!(result2, result); // Different pointer — allocated fresh
        assert_eq!(string_as_str(result2), "hello world");
        assert_eq!(string_as_str(result), "hello "); // Original unchanged
    }

    #[test]
    fn test_string_append_self() {
        // Self-append (s += s) must always allocate fresh
        let a = js_string_from_bytes(b"ab".as_ptr(), 2);
        let result = js_string_append(a, a);
        assert_eq!(string_as_str(result), "abab");
    }

    #[test]
    fn test_string_append_loop() {
        // Simulate the common loop pattern: result = result + "x" repeated
        let mut result = js_string_from_bytes(b"".as_ptr(), 0);
        let x = js_string_from_bytes(b"x".as_ptr(), 1);
        let mut inplace_count = 0u32;
        for _ in 0..1000 {
            let old_ptr = result;
            result = js_string_append(result, x);
            if result == old_ptr {
                inplace_count += 1;
            }
        }
        assert_eq!(js_string_length(result), 1000);
        // Most appends should be in-place (only ~10 re-allocations for 1000 appends)
        assert!(inplace_count > 980, "Expected >980 in-place appends, got {}", inplace_count);
    }
}
