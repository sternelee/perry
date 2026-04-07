//! JSON handling — JSON.parse(), JSON.stringify(), and specialized variants
//!
//! Provides all core JSON functions used by compiled TypeScript programs.
//! These live in perry-runtime (not perry-stdlib) so that programs that
//! only use JSON don't need to link the full stdlib.

use crate::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_set_field,
    js_object_set_keys, js_string_from_bytes, JSValue, StringHeader,
};
use std::fmt::Write as FmtWrite;

// ─── Zero-copy string access ──────────────────────────────────────────────────

#[inline]
unsafe fn str_from_header<'a>(ptr: *const StringHeader) -> Option<&'a str> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).length as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(std::str::from_utf8_unchecked(bytes))
}

// ─── Direct JSON parser ────────────────────────────────────────────────────────

struct DirectParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> DirectParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    #[inline]
    fn advance(&mut self) {
        self.pos += 1;
    }

    #[inline]
    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    #[inline]
    fn expect(&mut self, ch: u8) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(ch) {
            self.advance();
            true
        } else {
            false
        }
    }

    unsafe fn parse_value(&mut self) -> JSValue {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string_value(),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_true(),
            Some(b'f') => self.parse_false(),
            Some(b'n') => self.parse_null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => JSValue::null(),
        }
    }

    unsafe fn parse_string_value(&mut self) -> JSValue {
        if let Some(s) = self.parse_string_bytes() {
            let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            JSValue::string_ptr(ptr)
        } else {
            JSValue::null()
        }
    }

    fn parse_string_bytes(&mut self) -> Option<Vec<u8>> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.advance();

        let mut result = Vec::new();
        loop {
            if self.pos >= self.input.len() {
                return None;
            }
            let ch = self.input[self.pos];
            self.pos += 1;
            match ch {
                b'"' => return Some(result),
                b'\\' => {
                    if self.pos >= self.input.len() {
                        return None;
                    }
                    let esc = self.input[self.pos];
                    self.pos += 1;
                    match esc {
                        b'"' => result.push(b'"'),
                        b'\\' => result.push(b'\\'),
                        b'/' => result.push(b'/'),
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'b' => result.push(0x08),
                        b'f' => result.push(0x0C),
                        b'u' => {
                            if self.pos + 4 > self.input.len() {
                                return None;
                            }
                            let hex = std::str::from_utf8(&self.input[self.pos..self.pos + 4]).ok()?;
                            let code = u16::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            if (0xD800..=0xDBFF).contains(&code) {
                                if self.pos + 6 <= self.input.len()
                                    && self.input[self.pos] == b'\\'
                                    && self.input[self.pos + 1] == b'u'
                                {
                                    let hex2 = std::str::from_utf8(&self.input[self.pos + 2..self.pos + 6]).ok()?;
                                    let low = u16::from_str_radix(hex2, 16).ok()?;
                                    self.pos += 6;
                                    let codepoint = 0x10000 + ((code as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
                                    if let Some(c) = char::from_u32(codepoint) {
                                        let mut buf = [0u8; 4];
                                        let s = c.encode_utf8(&mut buf);
                                        result.extend_from_slice(s.as_bytes());
                                    }
                                }
                            } else {
                                if let Some(c) = char::from_u32(code as u32) {
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                }
                            }
                        }
                        _ => result.push(esc),
                    }
                }
                _ => result.push(ch),
            }
        }
    }

    unsafe fn parse_object(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let mut pairs: Vec<(Vec<u8>, JSValue)> = Vec::new();

        if self.peek() == Some(b'}') {
            self.advance();
            let js_obj = js_object_alloc(0, 0);
            let keys_arr = js_array_alloc(0);
            js_object_set_keys(js_obj, keys_arr);
            return JSValue::object_ptr(js_obj as *mut u8);
        }

        loop {
            self.skip_whitespace();
            let key = match self.parse_string_bytes() {
                Some(k) => k,
                None => break,
            };

            if !self.expect(b':') {
                break;
            }

            let value = self.parse_value();
            pairs.push((key, value));

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b'}');

        let count = pairs.len();
        let js_obj = js_object_alloc(0, count as u32);
        let keys_arr = js_array_alloc(count as u32);

        for (idx, (key, value)) in pairs.into_iter().enumerate() {
            let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
            js_array_push(keys_arr, JSValue::string_ptr(key_ptr));
            js_object_set_field(js_obj, idx as u32, value);
        }
        js_object_set_keys(js_obj, keys_arr);
        JSValue::object_ptr(js_obj as *mut u8)
    }

    unsafe fn parse_array(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let mut js_arr = js_array_alloc(16);

        if self.peek() == Some(b']') {
            self.advance();
            return JSValue::object_ptr(js_arr as *mut u8);
        }

        loop {
            let value = self.parse_value();
            js_arr = js_array_push(js_arr, value);

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b']');
        JSValue::object_ptr(js_arr as *mut u8)
    }

    unsafe fn parse_number(&mut self) -> JSValue {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance();
        }
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.input.len() && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E') {
            self.pos += 1;
            if self.pos < self.input.len() && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-') {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        let num_str = std::str::from_utf8_unchecked(&self.input[start..self.pos]);
        let value: f64 = num_str.parse().unwrap_or(0.0);
        JSValue::number(value)
    }

    unsafe fn parse_true(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"true" {
            self.pos += 4;
            JSValue::bool(true)
        } else {
            JSValue::null()
        }
    }

    unsafe fn parse_false(&mut self) -> JSValue {
        if self.pos + 5 <= self.input.len() && &self.input[self.pos..self.pos + 5] == b"false" {
            self.pos += 5;
            JSValue::bool(false)
        } else {
            JSValue::null()
        }
    }

    unsafe fn parse_null(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"null" {
            self.pos += 4;
        }
        JSValue::null()
    }
}

// ─── JSON.parse ───────────────────────────────────────────────────────────────

/// JSON.parse(text) -> any
///
/// Uses a direct recursive-descent parser that constructs Perry JSValues
/// without any intermediate representation.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse(text_ptr: *const StringHeader) -> JSValue {
    if text_ptr.is_null() {
        let msg = "Unexpected end of JSON input";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_val = JSValue::string_ptr(msg_ptr);
        crate::exception::js_throw(f64::from_bits(err_val.bits()));
    }
    let len = (*text_ptr).length as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    if len == 0 {
        let msg = "Unexpected end of JSON input";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_val = JSValue::string_ptr(msg_ptr);
        crate::exception::js_throw(f64::from_bits(err_val.bits()));
    }

    let mut parser = DirectParser::new(bytes);
    let result = parser.parse_value();

    // If parser didn't consume meaningful input (result is null and input wasn't "null"),
    // the input was invalid JSON — throw SyntaxError
    if result.is_null() {
        let is_literal_null = len >= 4 && bytes.starts_with(b"null");
        if !is_literal_null {
            let preview_len = len.min(50);
            let preview = std::str::from_utf8(&bytes[..preview_len]).unwrap_or("???");
            let msg = format!("JSON parse error: Unexpected token: {}", preview);
            let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err_val = JSValue::string_ptr(msg_ptr);
            crate::exception::js_throw(f64::from_bits(err_val.bits()));
        }
    }

    result
}

// ─── JSON.stringify ───────────────────────────────────────────────────────────

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

const TYPE_UNKNOWN: u32 = 0;
const TYPE_OBJECT: u32 = 1;
const TYPE_ARRAY: u32 = 2;

#[inline]
fn is_raw_pointer(bits: u64) -> bool {
    let exponent = (bits >> 52) & 0x7FF;
    let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
    let sign = bits >> 63;
    exponent == 0 && mantissa != 0 && sign == 0
}

#[inline]
unsafe fn extract_pointer(bits: u64) -> Option<*const u8> {
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == POINTER_TAG {
        Some((bits & POINTER_MASK) as *const u8)
    } else if is_raw_pointer(bits) {
        Some(bits as *const u8)
    } else {
        None
    }
}

#[inline]
unsafe fn is_object_pointer(ptr: *const u8) -> bool {
    let obj = ptr as *const crate::ObjectHeader;
    let potential_keys_ptr = (*obj).keys_array as u64;
    let top_16_bits = potential_keys_ptr >> 48;
    let is_likely_heap_pointer = top_16_bits == 0 || top_16_bits == 1;
    let looks_like_valid_pointer = is_likely_heap_pointer
        && potential_keys_ptr > 0x10000
        && (potential_keys_ptr & 0x7) == 0;

    if looks_like_valid_pointer {
        let keys_arr = (*obj).keys_array;
        let keys_len = (*keys_arr).length;
        let keys_cap = (*keys_arr).capacity;
        let field_count = (*obj).field_count;
        // field_count may be larger than keys_len due to pre-allocation (e.g., alloc(0, 8) for 2 keys).
        // Use keys_len as the authoritative count of actual properties.
        keys_len <= keys_cap && keys_len > 0 && keys_cap < 1000 && keys_len <= field_count && field_count < 1000
    } else {
        false
    }
}

#[inline]
unsafe fn write_number(buf: &mut String, value: f64) {
    if value.is_nan() || value.is_infinite() {
        buf.push_str("null");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        use std::fmt::Write;
        let _ = write!(buf, "{}", value as i64);
    } else {
        let s = format!("{}", value);
        buf.push_str(&s);
    }
}

#[inline]
unsafe fn write_escaped_string(buf: &mut String, s: &str) {
    buf.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let escape = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0..=0x1f => {
                if start < i {
                    buf.push_str(&s[start..i]);
                }
                let _ = write!(buf, "\\u{:04x}", b);
                start = i + 1;
                continue;
            }
            _ => None,
        };
        if let Some(esc) = escape {
            if start < i {
                buf.push_str(&s[start..i]);
            }
            buf.push_str(esc);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        buf.push_str(&s[start..]);
    }
    buf.push('"');
}

unsafe fn stringify_value(value: f64, type_hint: u32, buf: &mut String) {
    let bits: u64 = value.to_bits();

    if bits == TAG_UNDEFINED {
        // In arrays, undefined becomes null. In objects, the field is skipped
        // (handled by stringify_object). At root level, undefined is not valid JSON.
        buf.push_str("null");
        return;
    }
    if bits == TAG_NULL {
        buf.push_str("null");
        return;
    }
    if bits == TAG_TRUE {
        buf.push_str("true");
        return;
    }
    if bits == TAG_FALSE {
        buf.push_str("false");
        return;
    }

    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // BigInt: serialize as quoted string (matching JSON.stringify with BigInt replacer behavior)
    if tag == BIGINT_TAG {
        let bigint_ptr = (bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_OBJECT {
            stringify_object(ptr, buf);
            return;
        }
        if type_hint == TYPE_ARRAY {
            stringify_array(ptr, buf);
            return;
        }

        if is_object_pointer(ptr) {
            stringify_object(ptr, buf);
        } else {
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 {
                    stringify_array(ptr, buf);
                    return;
                }
            }
            let str_ptr = ptr as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        }
        return;
    }

    write_number(buf, value);
}

unsafe fn stringify_object(ptr: *const u8, buf: &mut String) {
    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    buf.push('{');

    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements = (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr = (ptr as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;

    // Use keys_len as the iteration count since field_count may include pre-allocated slots.
    // Only the first keys_len fields have corresponding key names.
    let actual_fields = std::cmp::min(num_fields, keys_len);
    let mut first = true;
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let field_bits = field_val.to_bits();

        // Skip undefined values in objects (JS JSON.stringify spec)
        if field_bits == TAG_UNDEFINED {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

        if (f as u32) < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            if let Some(key_str) = str_from_header(key_ptr) {
                buf.push('"');
                buf.push_str(key_str);
                buf.push_str("\":");
            } else {
                let _ = write!(buf, "\"field{}\":", f);
            }
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }

        stringify_value(field_val, TYPE_UNKNOWN, buf);
    }
    buf.push('}');
}

unsafe fn stringify_array(ptr: *const u8, buf: &mut String) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;

        if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_bits == TAG_NULL {
            buf.push_str("null");
        } else if elem_bits == TAG_TRUE {
            buf.push_str("true");
        } else if elem_bits == TAG_FALSE {
            buf.push_str("false");
        } else if elem_tag == BIGINT_TAG {
            let bigint_ptr = (elem_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_tag == POINTER_TAG || is_raw_pointer(elem_bits) {
            let elem_ptr = if elem_tag == POINTER_TAG {
                (elem_bits & POINTER_MASK) as *const u8
            } else {
                elem_bits as *const u8
            };
            if is_object_pointer(elem_ptr) {
                stringify_object(elem_ptr, buf);
            } else {
                let arr_elem = elem_ptr as *const crate::ArrayHeader;
                let arr_len = (*arr_elem).length;
                let arr_cap = (*arr_elem).capacity;
                if arr_len <= arr_cap && arr_cap > 0 && arr_cap < 10000 {
                    stringify_array(elem_ptr, buf);
                } else {
                    let str_ptr = elem_ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
            }
        } else {
            write_number(buf, elem);
        }
    }
    buf.push(']');
}

#[inline]
unsafe fn estimate_json_size(value: f64, type_hint: u32) -> usize {
    let bits = value.to_bits();
    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_ARRAY || (!is_object_pointer(ptr) && type_hint != TYPE_OBJECT) {
            let arr = ptr as *const crate::ArrayHeader;
            let len = (*arr).length as usize;
            return (len * 300).max(256);
        }
        if type_hint == TYPE_OBJECT || is_object_pointer(ptr) {
            let obj = ptr as *const crate::ObjectHeader;
            let fields = (*obj).field_count as usize;
            return (fields * 200).max(256);
        }
    }
    4096
}

/// Generic JSON.stringify that handles any JSValue
/// Takes a f64 (NaN-boxed JSValue) and a type_hint (0=unknown, 1=object, 2=array)
/// Returns a string pointer (null if value is undefined — per JSON spec)
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader {
    // JSON.stringify(undefined) returns undefined (not a string)
    if value.to_bits() == TAG_UNDEFINED {
        return std::ptr::null_mut();
    }
    let estimated = estimate_json_size(value, type_hint);
    let mut buf = String::with_capacity(estimated);
    stringify_value(value, type_hint, &mut buf);
    js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

// ─── Specialized stringify functions ──────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_string(
    str_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match str_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let mut buf = String::with_capacity(s.len() + 16);
    write_escaped_string(&mut buf, s);
    js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

/// Stringify a number
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_number(value: f64) -> *mut StringHeader {
    if value.is_nan() || value.is_infinite() {
        return js_string_from_bytes(b"null".as_ptr(), 4);
    }
    if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        let mut itoa_buf = itoa::Buffer::new();
        let s = itoa_buf.format(value as i64);
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }
    let mut ryu_buf = ryu::Buffer::new();
    let s = ryu_buf.format(value);
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Stringify a boolean
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_bool(value: bool) -> *mut StringHeader {
    let s = if value { "true" } else { "false" };
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Stringify null
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_null() -> *mut StringHeader {
    js_string_from_bytes(b"null".as_ptr(), 4)
}

/// Check if a string is valid JSON
#[no_mangle]
pub unsafe extern "C" fn js_json_is_valid(text_ptr: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if text_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let len = (*text_ptr).length as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    if serde_json::from_slice::<serde_json::Value>(bytes).is_ok() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

// ─── Utility functions ────────────────────────────────────────────────────────

/// Legacy wrapper that allocates a String from a StringHeader
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    str_from_header(ptr).map(|s| s.to_string())
}

/// Get a value from parsed JSON by key (for object access)
#[no_mangle]
pub unsafe extern "C" fn js_json_get_string(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> *mut StringHeader {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return std::ptr::null_mut(),
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };
    match serde_json::from_str::<serde_json::Value>(&json_str) {
        Ok(serde_json::Value::Object(obj)) => {
            if let Some(serde_json::Value::String(s)) = obj.get(&key) {
                return js_string_from_bytes(s.as_ptr(), s.len() as u32);
            }
        }
        _ => {}
    }
    std::ptr::null_mut()
}

/// Get a number from parsed JSON by key
#[no_mangle]
pub unsafe extern "C" fn js_json_get_number(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> f64 {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return f64::NAN,
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return f64::NAN,
    };
    match serde_json::from_str::<serde_json::Value>(&json_str) {
        Ok(serde_json::Value::Object(obj)) => {
            if let Some(serde_json::Value::Number(n)) = obj.get(&key) {
                return n.as_f64().unwrap_or(f64::NAN);
            }
        }
        _ => {}
    }
    f64::NAN
}

/// Get a boolean from parsed JSON by key
#[no_mangle]
pub unsafe extern "C" fn js_json_get_bool(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> bool {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return false,
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return false,
    };
    match serde_json::from_str::<serde_json::Value>(&json_str) {
        Ok(serde_json::Value::Object(obj)) => {
            if let Some(serde_json::Value::Bool(b)) = obj.get(&key) {
                return *b;
            }
        }
        _ => {}
    }
    false
}

// ─── JSON.stringify with replacer ────────────────────────────────────────────

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Call a replacer closure with (key, value) and return the result as f64
#[inline]
unsafe fn call_replacer(
    replacer: *const crate::ClosureHeader,
    key_f64: f64,
    value_f64: f64,
) -> f64 {
    crate::js_closure_call2(replacer, key_f64, value_f64)
}

/// NaN-box a string pointer as f64 (STRING_TAG)
#[inline]
fn nanbox_string_f64(ptr: *const StringHeader) -> f64 {
    f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK))
}

/// NaN-box an object/array pointer as f64 (POINTER_TAG)
#[inline]
fn nanbox_pointer_f64(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

/// Stringify a value with replacer support.
/// The replacer is called as replacer(key, value) for each property.
/// Returns the replaced value serialized into the buffer.
unsafe fn stringify_value_with_replacer(
    key_f64: f64,
    value: f64,
    type_hint: u32,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    // Call the replacer with (key, value)
    let replaced = call_replacer(replacer, key_f64, value);
    let replaced_bits = replaced.to_bits();

    // If replacer returns undefined, skip this value
    if replaced_bits == TAG_UNDEFINED {
        return;
    }

    // Check if the replaced value is the same as the original (common case)
    // If it is, and the original is an object/array, recurse into it with replacer
    let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;

    // If the replaced value is a string, serialize it as a JSON string
    if replaced_tag == STRING_TAG {
        let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // If it's null/bool/number, serialize directly
    if replaced_bits == TAG_NULL {
        buf.push_str("null");
        return;
    }
    if replaced_bits == TAG_TRUE {
        buf.push_str("true");
        return;
    }
    if replaced_bits == TAG_FALSE {
        buf.push_str("false");
        return;
    }

    // Check for BigInt tag - serialize as number (toString)
    if replaced_tag == BIGINT_TAG {
        let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            // BigInt toString gives a plain number string, write it directly (no quotes)
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // Check for pointer (object/array) - recurse with replacer
    if let Some(ptr) = extract_pointer(replaced_bits) {
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_with_replacer(ptr, replacer, buf);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_with_replacer(ptr, replacer, buf);
        } else {
            // Try to detect: object vs array
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 && !is_object_pointer(ptr) {
                    stringify_array_with_replacer(ptr, replacer, buf);
                    return;
                }
            }
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, replacer, buf);
            } else {
                // Fallback: serialize as plain value (without replacer)
                stringify_value(replaced, TYPE_UNKNOWN, buf);
            }
        }
        return;
    }

    // Plain number
    write_number(buf, replaced);
}

unsafe fn stringify_object_with_replacer(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    buf.push('{');

    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements = (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr = (ptr as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;

    // Use keys_len as the iteration count since field_count may include pre-allocated slots.
    let actual_fields = std::cmp::min(num_fields, keys_len);
    let mut first = true;
    for f in 0..actual_fields {
        // Get the key as a string
        let (key_str_ptr, key_str_opt) = if (f as u32) < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let kp = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            (kp, str_from_header(kp))
        } else {
            (std::ptr::null(), None)
        };

        // Create NaN-boxed key for replacer
        let key_f64_for_replacer = if !key_str_ptr.is_null() {
            nanbox_string_f64(key_str_ptr)
        } else {
            // Fallback: create a "fieldN" string
            let fallback = format!("field{}", f);
            let fallback_ptr = js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32);
            nanbox_string_f64(fallback_ptr)
        };

        // Get the field value
        let field_val = *fields_ptr.add(f as usize);

        // Call replacer with (key, value)
        let replaced = call_replacer(replacer, key_f64_for_replacer, field_val);
        let replaced_bits = replaced.to_bits();

        // If replacer returns undefined, skip this property
        if replaced_bits == TAG_UNDEFINED {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

        // Write the key
        if let Some(key_str) = key_str_opt {
            buf.push('"');
            buf.push_str(key_str);
            buf.push_str("\":");
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }

        // Stringify the replaced value
        // For nested objects/arrays, we need to recurse with the replacer
        let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
        if replaced_tag == STRING_TAG {
            let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_bits == TAG_NULL {
            buf.push_str("null");
        } else if replaced_bits == TAG_TRUE {
            buf.push_str("true");
        } else if replaced_bits == TAG_FALSE {
            buf.push_str("false");
        } else if replaced_tag == BIGINT_TAG {
            let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                buf.push_str(s);
            } else {
                buf.push_str("null");
            }
        } else if let Some(inner_ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(inner_ptr) {
                stringify_object_with_replacer(inner_ptr, replacer, buf);
            } else {
                let arr = inner_ptr as *const crate::ArrayHeader;
                if !arr.is_null() {
                    let len = (*arr).length;
                    let cap = (*arr).capacity;
                    if len <= cap && cap > 0 && cap < 10000 {
                        stringify_array_with_replacer(inner_ptr, replacer, buf);
                    } else {
                        stringify_value(replaced, TYPE_UNKNOWN, buf);
                    }
                } else {
                    stringify_value(replaced, TYPE_UNKNOWN, buf);
                }
            }
        } else {
            write_number(buf, replaced);
        }
    }
    buf.push('}');
}

unsafe fn stringify_array_with_replacer(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let elem = *elements.add(i as usize);

        // Create key string for the index
        let idx_str = i.to_string();
        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let key_f64 = nanbox_string_f64(idx_ptr);

        // Call replacer with (index_string, value)
        let replaced = call_replacer(replacer, key_f64, elem);
        let replaced_bits = replaced.to_bits();

        // For arrays, undefined becomes null (per JSON spec)
        if replaced_bits == TAG_UNDEFINED {
            buf.push_str("null");
            continue;
        }

        let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
        if replaced_tag == STRING_TAG {
            let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_bits == TAG_NULL {
            buf.push_str("null");
        } else if replaced_bits == TAG_TRUE {
            buf.push_str("true");
        } else if replaced_bits == TAG_FALSE {
            buf.push_str("false");
        } else if replaced_tag == BIGINT_TAG {
            let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                buf.push_str(s);
            } else {
                buf.push_str("null");
            }
        } else if let Some(inner_ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(inner_ptr) {
                stringify_object_with_replacer(inner_ptr, replacer, buf);
            } else {
                let inner_arr = inner_ptr as *const crate::ArrayHeader;
                if !inner_arr.is_null() {
                    let inner_len = (*inner_arr).length;
                    let inner_cap = (*inner_arr).capacity;
                    if inner_len <= inner_cap && inner_cap > 0 && inner_cap < 10000 {
                        stringify_array_with_replacer(inner_ptr, replacer, buf);
                    } else {
                        stringify_value(replaced, TYPE_UNKNOWN, buf);
                    }
                } else {
                    stringify_value(replaced, TYPE_UNKNOWN, buf);
                }
            }
        } else {
            write_number(buf, replaced);
        }
    }
    buf.push(']');
}

/// JSON.stringify with replacer function
/// value: the JSValue to stringify (NaN-boxed f64)
/// type_hint: 0=unknown, 1=object, 2=array
/// replacer_ptr: pointer to a ClosureHeader (the replacer function)
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_with_replacer(
    value: f64,
    type_hint: u32,
    replacer_ptr: i64,
) -> *mut StringHeader {
    let replacer = replacer_ptr as *const crate::ClosureHeader;
    if replacer.is_null() {
        // Fall back to normal stringify if replacer is null
        return js_json_stringify(value, type_hint);
    }

    // Per JSON spec, the initial call to the replacer is with key="" and the root value
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);

    // Call replacer with ("", root_value)
    let replaced_root = call_replacer(replacer, empty_key_f64, value);
    let replaced_bits = replaced_root.to_bits();

    // If replacer returns undefined for root, return undefined (represented as "undefined" string? No, just return null)
    if replaced_bits == TAG_UNDEFINED {
        return std::ptr::null_mut();
    }

    let estimated = estimate_json_size(value, type_hint);
    let mut buf = String::with_capacity(estimated);

    // Check what the replacer returned
    let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
    if replaced_tag == STRING_TAG {
        let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(&mut buf, s);
        } else {
            buf.push_str("null");
        }
    } else if replaced_bits == TAG_NULL {
        buf.push_str("null");
    } else if replaced_bits == TAG_TRUE {
        buf.push_str("true");
    } else if replaced_bits == TAG_FALSE {
        buf.push_str("false");
    } else if replaced_tag == BIGINT_TAG {
        let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
    } else if let Some(ptr) = extract_pointer(replaced_bits) {
        // Object or array - recurse with replacer
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_with_replacer(ptr, replacer, &mut buf);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_with_replacer(ptr, replacer, &mut buf);
        } else {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, replacer, &mut buf);
            } else {
                let arr = ptr as *const crate::ArrayHeader;
                if !arr.is_null() {
                    let len = (*arr).length;
                    let cap = (*arr).capacity;
                    if len <= cap && cap > 0 && cap < 10000 {
                        stringify_array_with_replacer(ptr, replacer, &mut buf);
                    } else {
                        stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                    }
                } else {
                    stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                }
            }
        }
    } else {
        // Number
        write_number(&mut buf, replaced_root);
    }

    js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

/// JSON.stringify(value, replacer?, space) — pretty-printing and/or replacer support.
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_pretty(
    value: f64,
    replacer_ptr: i64,
    space_f64: f64,
) -> *mut StringHeader {
    // First, produce the compact JSON (with or without replacer)
    let compact = if replacer_ptr != 0 {
        js_json_stringify_with_replacer(value, 0, replacer_ptr)
    } else {
        js_json_stringify(value, 0)
    };
    if compact.is_null() {
        return std::ptr::null_mut();
    }

    // Determine the indent string from the space parameter
    let space_bits = space_f64.to_bits();
    let space_tag = space_bits & 0xFFFF_0000_0000_0000;
    let indent: String = if space_tag == STRING_TAG {
        // String indent (e.g. "\t")
        let ptr = (space_bits & POINTER_MASK) as *const StringHeader;
        let len = (*ptr).length as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("").to_string()
    } else {
        // Numeric indent
        let n = if space_f64.is_nan() || space_f64 <= 0.0 { 0 } else { space_f64 as usize };
        if n == 0 { return compact; }
        " ".repeat(n.min(10))
    };

    if indent.is_empty() {
        return compact;
    }

    // Re-format the compact JSON with indentation using serde_json
    let compact_len = (*compact).length as usize;
    let compact_data = (compact as *const u8).add(std::mem::size_of::<StringHeader>());
    let compact_bytes = std::slice::from_raw_parts(compact_data, compact_len);
    let compact_str = std::str::from_utf8(compact_bytes).unwrap_or("");
    match serde_json::from_str::<serde_json::Value>(compact_str) {
        Ok(parsed) => {
            let mut writer = Vec::new();
            let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
            let mut ser = serde_json::Serializer::with_formatter(&mut writer, formatter);
            if serde_json::to_writer(&mut writer, &parsed).is_err() {
                // Fallback: try with the formatter
                writer.clear();
            }
            // Re-do with the custom formatter
            writer.clear();
            {
                use std::io::Write;
                // Manual pretty-print by re-serializing
                let pretty_str = serde_json::to_string_pretty(&parsed).unwrap_or_default();
                // serde_json::to_string_pretty uses 2-space indent. If indent != "  ", we need to replace.
                let default_indent = "  ";
                let result = if indent == default_indent {
                    pretty_str
                } else {
                    // Replace each indent level
                    let mut output = String::new();
                    for line in pretty_str.lines() {
                        let stripped = line.trim_start_matches(' ');
                        let spaces = line.len() - stripped.len();
                        let level = spaces / 2;
                        for _ in 0..level { output.push_str(&indent); }
                        output.push_str(stripped);
                        output.push('\n');
                    }
                    if output.ends_with('\n') { output.pop(); }
                    output
                };
                let _ = write!(writer, "{}", result);
            }
            if !writer.is_empty() {
                return js_string_from_bytes(writer.as_ptr(), writer.len() as u32);
            }
            compact
        }
        Err(_) => compact,
    }
}

/// JSON.parse(text, reviver) — parse JSON then apply reviver to each key/value pair.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_reviver(
    text_ptr: *const StringHeader,
    reviver_ptr: i64,
) -> i64 {
    // First parse normally — result is JSValue bits as i64
    let result = js_json_parse(text_ptr);
    let result_bits = {
        // JSValue is repr(transparent) u64, so transmute is safe
        let v: u64 = std::mem::transmute(result);
        v
    };

    if result.is_undefined() || reviver_ptr == 0 {
        return result_bits as i64;
    }

    let reviver = reviver_ptr as *const crate::ClosureHeader;
    if reviver.is_null() {
        return result_bits as i64;
    }

    let result_f64 = f64::from_bits(result_bits);

    // Walk the parsed value and apply reviver to each property
    let tag = result_bits & 0xFFFF_0000_0000_0000;
    if tag == POINTER_TAG {
        let obj_ptr = (result_bits & POINTER_MASK) as *mut crate::object::ObjectHeader;
        if !obj_ptr.is_null() && (obj_ptr as usize) > 0x10000 {
            let field_count = (*obj_ptr).field_count;
            let keys_arr = (*obj_ptr).keys_array;
            if !keys_arr.is_null() {
                let keys_data = (keys_arr as *const u8).add(8) as *const f64;
                let fields_base = (obj_ptr as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
                for i in 0..field_count as usize {
                    let key_f64 = *keys_data.add(i);
                    let old_val = f64::from_bits(*fields_base.add(i));
                    let new_val = crate::closure::js_closure_call2(reviver, key_f64, old_val);
                    *fields_base.add(i) = new_val.to_bits();
                }
            }
        }
    }

    // Call reviver with ("", root) for the root value
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key = crate::value::js_nanbox_string(empty_str as i64);
    let final_val = crate::closure::js_closure_call2(reviver, empty_key, result_f64);
    final_val.to_bits() as i64
}

