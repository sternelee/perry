//! JSON handling — JSON.parse(), JSON.stringify(), and specialized variants
//!
//! Provides all core JSON functions used by compiled TypeScript programs.
//! These live in perry-runtime (not perry-stdlib) so that programs that
//! only use JSON don't need to link the full stdlib.

use crate::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_set_field,
    js_object_set_keys, js_string_from_bytes, JSValue, StringHeader,
};
use std::cell::RefCell;
use std::fmt::Write as FmtWrite;

// ─── Circular reference detection ────────────────────────────────────────────
thread_local! {
    /// Stack of object pointers currently being stringified (for circular detection).
    static STRINGIFY_STACK: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

// ─── Zero-copy string access ──────────────────────────────────────────────────

#[inline]
unsafe fn str_from_header<'a>(ptr: *const StringHeader) -> Option<&'a str> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
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
    let len = (*text_ptr).byte_len as usize;
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

/// Read the GC header's object type tag for a user-space heap pointer.
/// The GcHeader sits 8 bytes before `ptr`; its first byte is `obj_type`.
/// Returns 0 when `ptr` is null or in the low-memory guard range.
#[inline]
unsafe fn gc_obj_type(ptr: *const u8) -> u8 {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return 0;
    }
    // GcHeader.obj_type is at offset 0 (see crate::gc::GcHeader layout).
    *(ptr.sub(crate::gc::GC_HEADER_SIZE))
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

/// Check if a NaN-boxed value is a closure (function).
#[inline]
unsafe fn is_closure_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        // Check for ClosureHeader magic at offset 8 (type_tag field)
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        type_tag == crate::closure::CLOSURE_MAGIC
    } else {
        false
    }
}

/// Check if an object has a toJSON method. If so, call it and return the result as f64.
/// Returns None if no toJSON method exists.
#[inline]
unsafe fn object_get_to_json(ptr: *const u8) -> Option<f64> {
    let obj = ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }
    let keys_len = (*keys_arr).length;
    let keys_elements = (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;

    for f in 0..keys_len {
        let key_f64 = *keys_elements.add(f as usize);
        let key_bits = key_f64.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };
        if let Some(key_str) = str_from_header(key_ptr) {
            if key_str == "toJSON" {
                let field_val = *fields_ptr.add(f as usize);
                let field_bits = field_val.to_bits();
                // Check if this field is a closure
                if is_closure_value(field_bits) {
                    let closure_ptr = if (field_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
                        (field_bits & POINTER_MASK) as *const crate::closure::ClosureHeader
                    } else {
                        field_bits as *const crate::closure::ClosureHeader
                    };
                    // Call toJSON() with no arguments (pass empty string key per spec)
                    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
                    let key_f64_arg = f64::from_bits(STRING_TAG | (empty_str as u64 & POINTER_MASK));
                    let result = crate::js_closure_call1(closure_ptr, key_f64_arg);
                    return Some(result);
                }
            }
        }
    }
    None
}

unsafe fn stringify_value(value: f64, type_hint: u32, buf: &mut String) {
    let bits: u64 = value.to_bits();

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

        // Prefer the GC header's obj_type tag for dispatch — the old
        // capacity heuristic (`cap < 10000`) misidentified legitimate
        // arrays that had grown past 10k as strings, panicking on
        // `JSON.stringify(arr)` where `arr.length >= 10000` (issue #43).
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_ARRAY => stringify_array(ptr, buf),
            crate::gc::GC_TYPE_OBJECT => {
                if is_object_pointer(ptr) {
                    stringify_object(ptr, buf);
                } else {
                    buf.push_str("null");
                }
            }
            crate::gc::GC_TYPE_STRING => {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
            _ => {
                // Unknown/untagged pointer: fall back to the structural
                // heuristics for safety (e.g. pointers to non-GC-tracked
                // memory). Arrays up to 10k cap are dispatched here;
                // above that we defensively emit "null" rather than
                // trying to treat them as strings.
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
            }
        }
        return;
    }

    write_number(buf, value);
}

unsafe fn stringify_object(ptr: *const u8, buf: &mut String) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(POINTER_TAG | (err_ptr as u64 & POINTER_MASK)));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    // Check for toJSON method
    if let Some(to_json_val) = object_get_to_json(ptr) {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        stringify_value(to_json_val, TYPE_UNKNOWN, buf);
        return;
    }

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
        // Skip undefined and function/closure values per JSON spec
        if field_bits == TAG_UNDEFINED {
            continue;
        }
        if is_closure_value(field_bits) {
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
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
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

        if elem_bits == TAG_UNDEFINED {
            buf.push_str("null"); // undefined in arrays becomes null per JSON spec
        } else if elem_tag == STRING_TAG {
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
            match gc_obj_type(elem_ptr) {
                crate::gc::GC_TYPE_ARRAY => stringify_array(elem_ptr, buf),
                crate::gc::GC_TYPE_OBJECT => {
                    if is_object_pointer(elem_ptr) {
                        stringify_object(elem_ptr, buf);
                    } else {
                        buf.push_str("null");
                    }
                }
                crate::gc::GC_TYPE_STRING => {
                    let str_ptr = elem_ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
                _ => {
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
/// Returns a string pointer
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader {
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
    let len = (*text_ptr).byte_len as usize;
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

// ─── Pretty-print stringify ─────────────────────────────────────────────────

unsafe fn stringify_value_pretty(value: f64, type_hint: u32, buf: &mut String, indent: &str, depth: usize) {
    let bits: u64 = value.to_bits();

    if bits == TAG_NULL || bits == TAG_UNDEFINED {
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
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_pretty(ptr, buf, indent, depth);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_pretty(ptr, buf, indent, depth);
        } else {
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 && !is_object_pointer(ptr) {
                    stringify_array_pretty(ptr, buf, indent, depth);
                    return;
                }
            }
            if is_object_pointer(ptr) {
                stringify_object_pretty(ptr, buf, indent, depth);
            } else {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
        }
        return;
    }

    write_number(buf, value);
}

unsafe fn stringify_object_pretty(ptr: *const u8, buf: &mut String, indent: &str, depth: usize) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(POINTER_TAG | (err_ptr as u64 & POINTER_MASK)));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    // Check for toJSON method
    if let Some(to_json_val) = object_get_to_json(ptr) {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        stringify_value_pretty(to_json_val, TYPE_UNKNOWN, buf, indent, depth);
        return;
    }

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements = (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let actual_fields = std::cmp::min(num_fields, keys_len);

    // Collect non-undefined, non-closure fields
    let mut entries: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let field_bits = field_val.to_bits();
        if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
            continue;
        }
        let key_name = if (f as u32) < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr).map(|s| s.to_string()).unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        entries.push((key_name, field_val));
    }

    if entries.is_empty() {
        buf.push_str("{}");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }

    buf.push_str("{\n");
    let inner_indent_count = depth + 1;
    for (i, (key_name, field_val)) in entries.iter().enumerate() {
        for _ in 0..inner_indent_count {
            buf.push_str(indent);
        }
        buf.push('"');
        buf.push_str(&key_name);
        buf.push_str("\": ");
        stringify_value_pretty(*field_val, TYPE_UNKNOWN, buf, indent, inner_indent_count);
        if i + 1 < entries.len() {
            buf.push(',');
        }
        buf.push('\n');
    }
    for _ in 0..depth {
        buf.push_str(indent);
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

unsafe fn stringify_array_pretty(ptr: *const u8, buf: &mut String, indent: &str, depth: usize) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    if len == 0 {
        buf.push_str("[]");
        return;
    }

    buf.push_str("[\n");
    let inner_indent_count = depth + 1;
    for i in 0..len {
        for _ in 0..inner_indent_count {
            buf.push_str(indent);
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        if elem_bits == TAG_UNDEFINED {
            buf.push_str("null");
        } else {
            stringify_value_pretty(elem, TYPE_UNKNOWN, buf, indent, inner_indent_count);
        }
        if i + 1 < len {
            buf.push(',');
        }
        buf.push('\n');
    }
    for _ in 0..depth {
        buf.push_str(indent);
    }
    buf.push(']');
}

// ─── Array replacer (key whitelist) stringify ────────────────────────────────

unsafe fn stringify_object_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(POINTER_TAG | (err_ptr as u64 & POINTER_MASK)));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements = (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let actual_fields = std::cmp::min(num_fields, keys_len);

    // Build a map of key_name -> field_value for the object
    let mut field_map: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let key_name = if (f as u32) < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr).map(|s| s.to_string()).unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        field_map.push((key_name, field_val));
    }

    buf.push('{');
    let mut first = true;
    for allowed_key in allowed_keys {
        if let Some((_, field_val)) = field_map.iter().find(|(k, _)| k == allowed_key) {
            let field_bits = field_val.to_bits();
            if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
                continue;
            }
            if !first {
                buf.push(',');
            }
            first = false;
            if use_pretty {
                buf.push('\n');
                let inner_indent_count = depth + 1;
                for _ in 0..inner_indent_count {
                    buf.push_str(indent);
                }
                buf.push('"');
                buf.push_str(allowed_key);
                buf.push_str("\": ");
                stringify_value_pretty(*field_val, TYPE_UNKNOWN, buf, indent, inner_indent_count);
            } else {
                buf.push('"');
                buf.push_str(allowed_key);
                buf.push_str("\":");
                stringify_value(*field_val, TYPE_UNKNOWN, buf);
            }
        }
    }
    if use_pretty && !first {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

// ─── Extract array of strings from a JSValue array ──────────────────────────

unsafe fn extract_string_array(ptr: *const u8) -> Vec<String> {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut result = Vec::new();
    for i in 0..len {
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;
        if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                result.push(s.to_string());
            }
        } else if is_raw_pointer(elem_bits) {
            let str_ptr = elem_bits as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                result.push(s.to_string());
            }
        }
    }
    result
}

/// Detect whether a NaN-boxed value is an array (not an object).
#[inline]
unsafe fn is_array_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        if is_object_pointer(ptr) {
            return false;
        }
        let arr = ptr as *const crate::ArrayHeader;
        let len = (*arr).length;
        let cap = (*arr).capacity;
        len <= cap && cap > 0 && cap < 10000
    } else {
        false
    }
}

// ─── Full JSON.stringify(value, replacer, spacer) ───────────────────────────

/// JSON.stringify(value, replacer, spacer) — the full 3-arg form.
///
/// - `value`: NaN-boxed JSValue to stringify
/// - `replacer_f64`: NaN-boxed — a closure (function replacer), array (key whitelist), or null
/// - `spacer_f64`: NaN-boxed — a number (indent count), string (indent string), or null
///
/// Returns i64 JSValue bits: a NaN-boxed string pointer, or TAG_UNDEFINED when
/// `JSON.stringify(undefined)` should return `undefined`.
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_full(
    value: f64,
    replacer_f64: f64,
    spacer_f64: f64,
) -> i64 {
    let value_bits = value.to_bits();

    // JSON.stringify(undefined) returns undefined per spec
    if value_bits == TAG_UNDEFINED {
        return TAG_UNDEFINED as i64;
    }

    // If the value is a closure/function, return undefined per spec
    if is_closure_value(value_bits) {
        return TAG_UNDEFINED as i64;
    }

    // Determine spacer/indent
    let indent_str: String;
    let spacer_bits = spacer_f64.to_bits();
    let spacer_tag = spacer_bits & 0xFFFF_0000_0000_0000;
    if spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE {
        indent_str = String::new();
    } else if spacer_tag == STRING_TAG {
        let sp_ptr = (spacer_bits & POINTER_MASK) as *const StringHeader;
        indent_str = str_from_header(sp_ptr).unwrap_or("").to_string();
    } else if spacer_bits == TAG_TRUE {
        indent_str = String::new();
    } else {
        // Number — use that many spaces (clamped to 10)
        let n = spacer_f64 as usize;
        let n = n.min(10);
        indent_str = " ".repeat(n);
    }
    let use_pretty = !indent_str.is_empty();

    // Determine replacer type
    let replacer_bits = replacer_f64.to_bits();
    let is_null_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;

    // Check if replacer is an array (key whitelist)
    let array_replacer = if !is_null_replacer && is_array_value(replacer_bits) {
        let arr_ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            (replacer_bits & POINTER_MASK) as *const u8
        } else {
            replacer_bits as *const u8
        };
        Some(extract_string_array(arr_ptr))
    } else {
        None
    };

    // Check if replacer is a closure (function)
    let closure_replacer = if !is_null_replacer && array_replacer.is_none() && is_closure_value(replacer_bits) {
        let ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            (replacer_bits & POINTER_MASK) as *const crate::closure::ClosureHeader
        } else {
            replacer_bits as *const crate::closure::ClosureHeader
        };
        Some(ptr)
    } else {
        None
    };

    // Clear the circular detection stack
    STRINGIFY_STACK.with(|s| s.borrow_mut().clear());

    let mut buf = String::with_capacity(4096);

    if let Some(ref allowed_keys) = array_replacer {
        // Array replacer: only applies to objects at the top level
        if let Some(ptr) = extract_pointer(value_bits) {
            if is_object_pointer(ptr) {
                stringify_object_with_array_replacer(ptr, allowed_keys, &mut buf, &indent_str, 0, use_pretty);
            } else if use_pretty {
                stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
            } else {
                stringify_value(value, TYPE_UNKNOWN, &mut buf);
            }
        } else if use_pretty {
            stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
        } else {
            stringify_value(value, TYPE_UNKNOWN, &mut buf);
        }
    } else if let Some(closure_ptr) = closure_replacer {
        // Function replacer — use existing with_replacer path
        // First call replacer with ("", root_value)
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_f64 = nanbox_string_f64(empty_str);
        let replaced_root = call_replacer(closure_ptr, empty_key_f64, value);
        let replaced_bits = replaced_root.to_bits();
        if replaced_bits == TAG_UNDEFINED {
            STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
            return TAG_UNDEFINED as i64;
        }
        // For simplicity: when function replacer is used with pretty, we don't
        // interleave pretty-printing (matches simple spec behavior). Serialize
        // normally with the replacer.
        if let Some(ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, closure_ptr, &mut buf);
            } else {
                let arr = ptr as *const crate::ArrayHeader;
                if !arr.is_null() && (*arr).length <= (*arr).capacity && (*arr).capacity > 0 && (*arr).capacity < 10000 {
                    stringify_array_with_replacer(ptr, closure_ptr, &mut buf);
                } else {
                    stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                }
            }
        } else {
            stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
        }
    } else if use_pretty {
        // No replacer, but has spacer — pretty-print
        stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
    } else {
        // Plain stringify
        stringify_value(value, TYPE_UNKNOWN, &mut buf);
    }

    STRINGIFY_STACK.with(|s| s.borrow_mut().clear());

    let result_ptr = js_string_from_bytes(buf.as_ptr(), buf.len() as u32);
    // Return as NaN-boxed string
    (STRING_TAG | (result_ptr as u64 & POINTER_MASK)) as i64
}

// ─── JSON.parse with reviver ────────────────────────────────────────────────

/// Apply reviver to a parsed JSON value. The reviver is called as reviver(key, value).
/// For objects, it's called for each property; for the root, key is "".
unsafe fn apply_reviver(value: JSValue, key_f64: f64, reviver: *const crate::closure::ClosureHeader) -> JSValue {
    let bits = value.bits();

    // If value is an object, recurse into its properties first
    if let Some(ptr) = extract_pointer(bits) {
        if is_object_pointer(ptr) {
            let obj = ptr as *const crate::ObjectHeader;
            let num_fields = (*obj).field_count;
            let keys_arr = (*obj).keys_array;
            let keys_len = (*keys_arr).length;
            let keys_elements = (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
            let fields_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut f64;
            let actual_fields = std::cmp::min(num_fields, keys_len);

            for f in 0..actual_fields {
                let field_key_f64 = *keys_elements.add(f as usize);
                let field_val_f64 = *fields_ptr.add(f as usize);
                let child_val = JSValue::from_bits(field_val_f64.to_bits());
                let revived_child = apply_reviver(child_val, field_key_f64, reviver);
                // Write back the revived value
                *fields_ptr.add(f as usize) = f64::from_bits(revived_child.bits());
            }
        } else {
            // Check if it's an array
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 {
                    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
                    for i in 0..len {
                        let idx_str = i.to_string();
                        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
                        let idx_key_f64 = nanbox_string_f64(idx_ptr);
                        let elem_f64 = *elements.add(i as usize);
                        let child_val = JSValue::from_bits(elem_f64.to_bits());
                        let revived_child = apply_reviver(child_val, idx_key_f64, reviver);
                        *elements.add(i as usize) = f64::from_bits(revived_child.bits());
                    }
                }
            }
        }
    }

    // Now call reviver on this value
    let value_f64 = f64::from_bits(value.bits());
    let result = crate::js_closure_call2(reviver, key_f64, value_f64);
    JSValue::from_bits(result.to_bits())
}

/// JSON.parse(text, reviver) — parse JSON with a reviver function.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_with_reviver(
    text_ptr: *const StringHeader,
    reviver_ptr: i64,
) -> JSValue {
    // First, parse normally
    let parsed = js_json_parse(text_ptr);

    let reviver = reviver_ptr as *const crate::closure::ClosureHeader;
    if reviver.is_null() || (reviver_ptr as u64) < 0x1000 {
        return parsed;
    }

    // Apply reviver starting from root
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);
    apply_reviver(parsed, empty_key_f64, reviver)
}
