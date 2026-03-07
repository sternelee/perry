//! Closure runtime support for Perry
//!
//! A closure is a function pointer plus captured environment.
//! Layout:
//!   - ClosureHeader at the start
//!   - Followed by captured values (as f64 or i64 pointers)

/// Magic value stored in ClosureHeader._reserved to identify closures at runtime.
/// Used by js_value_typeof to return "function" instead of "object" for closures.
pub const CLOSURE_MAGIC: u32 = 0x434C_4F53; // "CLOS" in ASCII

/// Sentinel func_ptr value indicating this closure is a "bound method" on a native module.
/// When js_closure_callN detects this, it extracts captures and dispatches via js_native_call_method.
/// Captures layout: [0] = namespace_obj (f64), [1] = method_name_ptr (i64), [2] = method_name_len (i64)
pub const BOUND_METHOD_FUNC_PTR: *const u8 = 0xBADD_DEAD_u64 as *const u8;

/// Header for heap-allocated closures
#[repr(C)]
pub struct ClosureHeader {
    /// Function pointer (the actual compiled function)
    pub func_ptr: *const u8,
    /// Number of captured values
    pub capture_count: u32,
    /// Type tag: set to CLOSURE_MAGIC to identify closures at runtime
    pub type_tag: u32,
}

/// Allocate a closure with space for captured values
/// Returns pointer to ClosureHeader
#[no_mangle]
pub extern "C" fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut ClosureHeader {
    let captures_size = (capture_count as usize) * 8; // Each capture is 8 bytes (f64 or i64)
    let total_size = std::mem::size_of::<ClosureHeader>() + captures_size;

    let raw = crate::gc::gc_malloc(total_size, crate::gc::GC_TYPE_CLOSURE);
    let ptr = raw as *mut ClosureHeader;

    unsafe {
        (*ptr).func_ptr = func_ptr;
        (*ptr).capture_count = capture_count;
        (*ptr).type_tag = CLOSURE_MAGIC;
    }

    ptr
}

/// Get the function pointer from a closure
#[no_mangle]
pub extern "C" fn js_closure_get_func(closure: *const ClosureHeader) -> *const u8 {
    unsafe { (*closure).func_ptr }
}

/// Get a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64 {
    unsafe {
        let captures_ptr = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        *captures_ptr.add(index as usize)
    }
}

/// Set a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64) {
    unsafe {
        let captures_ptr = (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut f64;
        *captures_ptr.add(index as usize) = value;
    }
}

/// Get a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_ptr(closure: *const ClosureHeader, index: u32) -> i64 {
    unsafe {
        let captures_ptr = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const i64;
        *captures_ptr.add(index as usize)
    }
}

/// Set a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_ptr(closure: *mut ClosureHeader, index: u32, value: i64) {
    unsafe {
        let captures_ptr = (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut i64;
        *captures_ptr.add(index as usize) = value;
    }
}

/// Dispatch a bound method call with the given arguments.
/// Extracts the namespace object and method name from the closure captures,
/// then calls js_native_call_method with the packed arguments.
#[inline]
unsafe fn dispatch_bound_method(closure: *const ClosureHeader, args: &[f64]) -> f64 {
    let namespace_obj = js_closure_get_capture_f64(closure, 0);
    let method_name_ptr = js_closure_get_capture_ptr(closure, 1) as *const i8;
    let method_name_len = js_closure_get_capture_ptr(closure, 2) as usize;
    crate::object::js_native_call_method(
        namespace_obj,
        method_name_ptr,
        method_name_len,
        args.as_ptr(),
        args.len(),
    )
}

/// Validate a closure pointer and return its func_ptr if the closure is valid.
///
/// Uses `read_volatile` for type_tag + `compiler_fence` to GUARANTEE that:
/// 1. CLOSURE_MAGIC is checked BEFORE func_ptr is ever read
/// 2. The optimizer cannot hoist the func_ptr read before the type_tag check
///
/// Background: `#[inline(never)]` on `is_valid_closure_ptr` is insufficient — LLVM
/// still speculatively hoists the non-volatile func_ptr load before the CLOSURE_MAGIC
/// check in the caller. This produces code that only checks CLOSURE_MAGIC when func_ptr==0,
/// allowing non-closure heap objects (Box<JSValue>, BigInt structs) to bypass validation
/// and execute their data as code via `br x1` → SIGBUS.
///
/// Returns null pointer if invalid (address out of range, wrong CLOSURE_MAGIC, bad func_ptr).
#[inline(always)]
fn get_valid_func_ptr(closure: *const ClosureHeader) -> *const u8 {
    let addr = closure as usize;
    if addr < 0x1000 || addr >= 0x0001_0000_0000_0000 {
        return std::ptr::null();
    }
    let type_tag = unsafe {
        std::ptr::read_volatile((closure as *const u8).add(12) as *const u32)
    };
    if type_tag != CLOSURE_MAGIC {
        return std::ptr::null();
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    let func_ptr = unsafe {
        std::ptr::read_volatile(closure as *const *const u8)
    };
    let func_ptr_addr = func_ptr as usize;
    if func_ptr_addr == 0 {
        return std::ptr::null();
    }
    // Validate func_ptr is in a reasonable code address range.
    // macOS ARM64: .text starts at 0x100000000, typically < 0x400000000
    // Windows x86_64: typically 0x7FF7_xxxx_xxxx (ASLR), so we allow up to 0x8000_0000_0000
    // Linux x86_64 PIE: .text is typically in 0x55xxxxxxxxxx range
    // Skip this check on Linux since PIE addresses vary widely and CLOSURE_MAGIC
    // already provides strong validation.
    #[cfg(target_os = "macos")]
    if func_ptr_addr < 0x100000000 || func_ptr_addr > 0x400000000 {
        return std::ptr::null();
    }
    #[cfg(target_os = "windows")]
    if func_ptr_addr < 0x10000 || func_ptr_addr > 0x800000000000 {
        return std::ptr::null();
    }
    func_ptr
}

/// Call a closure with 0 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[]) };
    }
    let func: extern "C" fn(*const ClosureHeader) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure)
}

/// Call a closure with 1 argument, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call1(closure: *const ClosureHeader, arg0: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0)
}

/// Call a closure with 2 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call2(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1)
}

/// Call a closure with 3 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call3(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2)
}

/// Call a closure with 4 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call4(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3)
}

/// Call a closure with 5 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call5(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4)
}

/// Call a closure with 6 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call6(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5)
}

/// Call a closure with 7 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call7(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6)
}

/// Call a closure with 8 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call8(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
}

/// Call a closure with 9 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call9(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)
}

/// Call a closure with 10 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call10(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9)
}

/// Call a closure with 11 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call11(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10)
}

/// Call a closure with 12 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call12(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11)
}

/// Call a closure with 13 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call13(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12)
}

/// Call a closure with 14 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call14(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64, arg13: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13)
}

/// Call a closure with 15 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call15(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64, arg13: f64, arg14: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14)
}

/// Call a closure with 16 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call16(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64, arg13: f64, arg14: f64, arg15: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14, arg15]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14, arg15)
}

/// Call a JavaScript function value with variable arguments
/// This is the native implementation for dynamic function dispatch.
/// func_value: NaN-boxed f64 containing a closure pointer
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_value to avoid symbol collision
/// with js_call_value in perry-jsruntime which handles V8 JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_value(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use crate::value::JSValue;

    let jsval = JSValue::from_bits(func_value.to_bits());

    // Get the closure pointer from the value
    // For native compilation, function values are stored as NaN-boxed pointers
    let closure: *const ClosureHeader = if jsval.is_pointer() {
        jsval.as_pointer()
    } else if jsval.is_undefined() || jsval.is_null() || func_value.is_nan() {
        // TAG_UNDEFINED, TAG_NULL, or other NaN values are not callable
        return f64::from_bits(JSValue::undefined().bits());
    } else {
        // Try treating the value directly as a pointer (for i64 representation)
        func_value.to_bits() as *const ClosureHeader
    };

    if closure.is_null() {
        // Return undefined for null/invalid closures
        return f64::from_bits(JSValue::undefined().bits());
    }

    // Call with the appropriate arity
    match args_len {
        0 => js_closure_call0(closure),
        1 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            js_closure_call1(closure, arg0)
        }
        2 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            js_closure_call2(closure, arg0, arg1)
        }
        3 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            js_closure_call3(closure, arg0, arg1, arg2)
        }
        4 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            js_closure_call4(closure, arg0, arg1, arg2, arg3)
        }
        5 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            js_closure_call5(closure, arg0, arg1, arg2, arg3, arg4)
        }
        6 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            let arg5 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(5) };
            js_closure_call6(closure, arg0, arg1, arg2, arg3, arg4, arg5)
        }
        7 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            let arg5 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(5) };
            let arg6 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(6) };
            js_closure_call7(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6)
        }
        8 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            let arg5 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(5) };
            let arg6 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(6) };
            let arg7 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(7) };
            js_closure_call8(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
        }
        9 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call9(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8))
        }
        10 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call10(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9))
        }
        11 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call11(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10))
        }
        12 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call12(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11))
        }
        13 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call13(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12))
        }
        14 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call14(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13))
        }
        15 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call15(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14))
        }
        16 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call16(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14), a(15))
        }
        _ => {
            eprintln!("Warning: js_native_call_value called with {} args, only supporting up to 16", args_len);
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call16(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14), a(15))
        }
    }
}

use std::collections::HashMap;
use std::sync::{OnceLock, Mutex};

static CLOSURE_PROPS: OnceLock<Mutex<HashMap<usize, HashMap<String, f64>>>> = OnceLock::new();

fn get_closure_props() -> &'static Mutex<HashMap<usize, HashMap<String, f64>>> {
    CLOSURE_PROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Check if a raw pointer points to a ClosureHeader by checking CLOSURE_MAGIC at offset 12.
/// Safe to call with any non-null, sufficiently aligned pointer >= 0x10000.
pub fn is_closure_ptr(ptr: usize) -> bool {
    if ptr < 0x10000 {
        return false;
    }
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        type_tag == CLOSURE_MAGIC
    }
}

/// Get a dynamic property stored on a closure.
/// Returns TAG_UNDEFINED if not found.
pub fn closure_get_dynamic_prop(ptr: usize, prop: &str) -> f64 {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(closure_props) = props.get(&ptr) {
            if let Some(&val) = closure_props.get(prop) {
                return val;
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Set a dynamic property on a closure.
pub fn closure_set_dynamic_prop(ptr: usize, prop: &str, value: f64) {
    if let Ok(mut props) = get_closure_props().lock() {
        props.entry(ptr).or_insert_with(HashMap::new).insert(prop.to_string(), value);
    }
}

/// Create a new instance of a class by name from a module handle.
// All V8 interop stubs removed — real implementations are in perry-jsruntime/src/interop.rs
// (V8 builds). For V8-free builds, compile.rs generates Cranelift-level stubs.
// Removed: js_new_from_handle, js_call_function, js_load_module, js_new_instance,
// js_create_callback, js_runtime_init, js_set_property, js_get_export, js_await_js_promise

// =============================================================================
// AOT stubs for unconditionally-declared extern functions
// =============================================================================

#[no_mangle] pub extern "C" fn js_ratelimit_create() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_lodash_ends_with() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_escape() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_includes() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_lower_first() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_replace() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_split() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_start_case() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_starts_with() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_unescape() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_upper_first() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_axios_create() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_axios_request() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_argon2_hash_options() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sharp_negate() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sharp_quality() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sharp_to_format() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sqlite_transaction() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sqlite_transaction_commit() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sqlite_transaction_rollback() -> i64 { 0 }
#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn test_closure_func(closure: *const ClosureHeader) -> f64 {
        unsafe {
            let captured = js_closure_get_capture_f64(closure, 0);
            captured * 2.0
        }
    }

    #[test]
    fn test_closure_basic() {
        let closure = js_closure_alloc(test_closure_func as *const u8, 1);
        js_closure_set_capture_f64(closure, 0, 21.0);
        let result = js_closure_call0(closure);
        assert_eq!(result, 42.0);
    }
}
