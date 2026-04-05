//! RegExp runtime support for Perry
//!
//! Provides JavaScript-compatible regular expression operations using the Rust regex crate.
//! RegExp objects are heap-allocated and store the compiled pattern and flags.

use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr;
use std::sync::Arc;

use crate::array::ArrayHeader;
use crate::string::StringHeader;
use crate::value::js_nanbox_string;

thread_local! {
    /// Cache of compiled regex objects, keyed by (pattern, flags).
    /// Without this cache, every call like `str.match(/^(\w+)/)` compiles a
    /// fresh Regex (tens to hundreds of KB of DFA/NFA state) and leaks it
    /// since RegExpHeader is never freed. Long-running services with
    /// frequent regex literals exhaust RSS quickly.
    static REGEX_CACHE: RefCell<HashMap<(String, String), Arc<Regex>>> = RefCell::new(HashMap::new());
}

fn get_or_compile_regex(pattern: &str, flags: &str) -> Arc<Regex> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(re) = cache.get(&(pattern.to_string(), flags.to_string())) {
            return re.clone();
        }
        // Translate JS regex to Rust-compatible pattern
        let translated = js_regex_to_rust(pattern);
        let case_insensitive = flags.contains('i');
        let multiline = flags.contains('m');
        let regex_pattern = if case_insensitive || multiline {
            let mut prefix = String::from("(?");
            if case_insensitive { prefix.push('i'); }
            if multiline { prefix.push('m'); }
            prefix.push(')');
            format!("{}{}", prefix, translated)
        } else {
            translated
        };
        let regex = Regex::new(&regex_pattern)
            .unwrap_or_else(|_| Regex::new(r"[^\s\S]").unwrap());
        let arc = Arc::new(regex);
        cache.insert((pattern.to_string(), flags.to_string()), arc.clone());
        arc
    })
}

/// Header for heap-allocated RegExp objects
#[repr(C)]
pub struct RegExpHeader {
    /// Pointer to the compiled Regex object (boxed)
    regex_ptr: *mut Regex,
    /// Original pattern string (for debugging/serialization)
    pattern_ptr: *const StringHeader,
    /// Flags string (e.g., "gi" for global+ignoreCase)
    flags_ptr: *const StringHeader,
    /// Cached flags for quick access
    pub case_insensitive: bool,
    pub global: bool,
    pub multiline: bool,
}

/// Check if a pointer is valid (not null and not a small invalid value from bad NaN-unboxing)
#[inline]
fn is_valid_ptr<T>(p: *const T) -> bool {
    !p.is_null() && (p as usize) >= 0x1000
}

/// Internal helper: Get string data from StringHeader
fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe {
        let len = (*s).length as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8_unchecked(bytes)
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
fn js_string_from_str(s: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Translate a JavaScript regex pattern to a Rust regex-crate compatible pattern.
/// Handles JS-specific escape sequences not supported by the Rust regex crate.
fn js_regex_to_rust(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                // JS allows \/ to escape forward slash — Rust regex doesn't need it
                '/' => {
                    result.push('/');
                    i += 2;
                }
                // Pass through all other backslash sequences as-is
                _ => {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Create a new RegExp from pattern and flags strings
/// Returns a pointer to RegExpHeader
///
/// Uses the thread-local REGEX_CACHE so repeated regex literals (e.g. in a
/// loop) reuse the same compiled Regex instead of leaking a fresh one each
/// time. The raw pointer stored in RegExpHeader is kept alive by the cache.
#[no_mangle]
pub extern "C" fn js_regexp_new(pattern: *const StringHeader, flags: *const StringHeader) -> *mut RegExpHeader {
    let pattern_str = if is_valid_ptr(pattern) { string_as_str(pattern) } else { "" };
    let flags_str = if is_valid_ptr(flags) { string_as_str(flags) } else { "" };

    let case_insensitive = flags_str.contains('i');
    let global = flags_str.contains('g');
    let multiline = flags_str.contains('m');

    // Get or compile the regex from the cache. The returned Arc is stored
    // in the cache indefinitely, so the raw pointer we extract stays valid
    // for the lifetime of the process.
    let arc = get_or_compile_regex(pattern_str, flags_str);
    let regex_ptr = Arc::as_ptr(&arc) as *mut Regex;

    // Allocate the header via gc_malloc so it's tracked by the GC and gets
    // freed when no longer referenced. Previously this used raw alloc() and
    // leaked every header, which was a 64-byte-per-call leak on top of the
    // (now-fixed) regex object leak.
    let header_size = std::mem::size_of::<RegExpHeader>();
    unsafe {
        let raw = crate::gc::gc_malloc(header_size, crate::gc::GC_TYPE_OBJECT);
        if raw.is_null() {
            panic!("Failed to allocate RegExp");
        }
        let ptr = raw as *mut RegExpHeader;

        (*ptr).regex_ptr = regex_ptr;
        (*ptr).pattern_ptr = pattern;
        (*ptr).flags_ptr = flags;
        (*ptr).case_insensitive = case_insensitive;
        (*ptr).global = global;
        (*ptr).multiline = multiline;

        ptr
    }
}

/// Test if a string matches the regex pattern
/// regex.test(string) -> boolean
#[no_mangle]
pub extern "C" fn js_regexp_test(re: *const RegExpHeader, s: *const StringHeader) -> i32 {
    if !is_valid_ptr(re) || !is_valid_ptr(s) {
        return 0;
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;
        if regex.is_match(str_data) { 1 } else { 0 }
    }
}

/// Find matches in a string
/// string.match(regex) -> string[] | null (returns array pointer, null if no match)
#[no_mangle]
pub extern "C" fn js_string_match(s: *const StringHeader, re: *const RegExpHeader) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_ptr(re) {
        return ptr::null_mut();
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;

        if global {
            // Global flag: return all matches
            let matches: Vec<&str> = regex.find_iter(str_data).map(|m| m.as_str()).collect();

            if matches.is_empty() {
                return ptr::null_mut();
            }

            // Create array of string pointers
            let arr = crate::array::js_array_alloc(matches.len() as u32);
            (*arr).length = matches.len() as u32;
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

            for (i, m) in matches.iter().enumerate() {
                let str_ptr = js_string_from_str(m);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                std::ptr::write(elements_ptr.add(i), nanboxed);
            }

            arr
        } else {
            // Non-global: return first match only (or with capture groups)
            match regex.captures(str_data) {
                Some(caps) => {
                    // Return array with full match and capture groups
                    let arr = crate::array::js_array_alloc(caps.len() as u32);
                    (*arr).length = caps.len() as u32;
                    let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

                    for (i, cap) in caps.iter().enumerate() {
                        if let Some(m) = cap {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            std::ptr::write(elements_ptr.add(i), nanboxed);
                        } else {
                            // Undefined capture group - store as undefined (TAG_UNDEFINED = 0x7FFC_0000_0000_0001)
                            std::ptr::write(elements_ptr.add(i), f64::from_bits(0x7FFC_0000_0000_0001));
                        }
                    }

                    arr
                }
                None => ptr::null_mut(),
            }
        }
    }
}

/// Find all matches in a string, each with capture groups
/// string.matchAll(regex) -> Array<Array<string>> (array of match arrays)
#[no_mangle]
pub extern "C" fn js_string_match_all(s: *const StringHeader, re: *const RegExpHeader) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_ptr(re) {
        // Return empty array, not null (matchAll never returns null)
        return crate::array::js_array_alloc(0);
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;

        // Collect all captures
        let all_caps: Vec<regex::Captures> = regex.captures_iter(str_data).collect();

        if all_caps.is_empty() {
            return crate::array::js_array_alloc(0);
        }

        // Create outer array (one entry per match)
        let outer = crate::array::js_array_alloc(all_caps.len() as u32);
        (*outer).length = all_caps.len() as u32;
        let outer_elements = (outer as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for (i, caps) in all_caps.iter().enumerate() {
            // Create inner array for this match (full match + capture groups)
            let inner = crate::array::js_array_alloc(caps.len() as u32);
            (*inner).length = caps.len() as u32;
            let inner_elements = (inner as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

            for (j, cap) in caps.iter().enumerate() {
                if let Some(m) = cap {
                    let str_ptr = js_string_from_str(m.as_str());
                    let nanboxed = js_nanbox_string(str_ptr as i64);
                    std::ptr::write(inner_elements.add(j), nanboxed);
                } else {
                    // Undefined capture group
                    std::ptr::write(inner_elements.add(j), f64::from_bits(0x7FFC_0000_0000_0001));
                }
            }

            // Store inner array as NaN-boxed pointer in outer array
            let inner_ptr = inner as i64;
            std::ptr::write(outer_elements.add(i), f64::from_bits(inner_ptr as u64));
        }

        outer
    }
}

/// Replace matches in a string
/// string.replace(regex, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let repl_str = if is_valid_ptr(replacement) { string_as_str(replacement) } else { "undefined" };

    if !is_valid_ptr(re) {
        // If regex is null, return original string
        return js_string_from_str(str_data);
    }

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;

        let result = if global {
            // Global flag: replace all occurrences
            regex.replace_all(str_data, repl_str).to_string()
        } else {
            // Non-global: replace first occurrence only
            regex.replace(str_data, repl_str).to_string()
        };

        js_string_from_str(&result)
    }
}

/// Replace with a simple string pattern (not regex)
/// string.replace(pattern, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_string(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) { string_as_str(pattern) } else { "" };
    let repl_str = if is_valid_ptr(replacement) { string_as_str(replacement) } else { "undefined" };

    // String.replace with a string pattern only replaces the first occurrence
    let result = str_data.replacen(pattern_str, repl_str, 1);
    js_string_from_str(&result)
}

/// Replace ALL occurrences with a simple string pattern (not regex)
/// string.replaceAll(pattern, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_all_string(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) { string_as_str(pattern) } else { "" };
    let repl_str = if is_valid_ptr(replacement) { string_as_str(replacement) } else { "undefined" };

    let result = str_data.replace(pattern_str, repl_str);
    js_string_from_str(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::js_string_from_bytes;

    fn make_string(s: &str) -> *mut StringHeader {
        js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }

    #[test]
    fn test_regexp_test_basic() {
        let pattern = make_string("hello");
        let flags = make_string("");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        assert!(js_regexp_test(re, test_str) != 0);

        let test_str2 = make_string("goodbye world");
        assert!(js_regexp_test(re, test_str2) == 0);
    }

    #[test]
    fn test_regexp_test_case_insensitive() {
        let pattern = make_string("hello");
        let flags = make_string("i");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("HELLO World");
        assert!(js_regexp_test(re, test_str) != 0);
    }

    #[test]
    fn test_string_match() {
        let pattern = make_string(r"\w+");
        let flags = make_string("");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let result = js_string_match(test_str, re);
        assert!(!result.is_null());

        unsafe {
            assert_eq!((*result).length, 1); // One match (first word)
        }
    }

    #[test]
    fn test_string_match_global() {
        let pattern = make_string(r"\w+");
        let flags = make_string("g");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let result = js_string_match(test_str, re);
        assert!(!result.is_null());

        unsafe {
            assert_eq!((*result).length, 2); // Two matches (hello, world)
        }
    }

    #[test]
    fn test_string_replace() {
        let pattern = make_string("world");
        let flags = make_string("");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let replacement = make_string("universe");
        let result = js_string_replace_regex(test_str, re, replacement);

        assert_eq!(string_as_str(result), "hello universe");
    }

    #[test]
    fn test_string_replace_global() {
        let pattern = make_string("o");
        let flags = make_string("g");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let replacement = make_string("0");
        let result = js_string_replace_regex(test_str, re, replacement);

        assert_eq!(string_as_str(result), "hell0 w0rld");
    }
}
