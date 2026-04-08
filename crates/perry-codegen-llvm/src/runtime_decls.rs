//! Runtime function signature registry.
//!
//! These declare the FFI ABI for functions exported by `libperry_runtime.a`.
//! Phase 1 only needs a tiny subset — enough to print a number — so we start
//! with six entries. Each later phase adds what it needs; the goal is to
//! avoid declaring unused runtime symbols, which would force the linker to
//! pull in the whole runtime even for a trivial test.
//!
//! Signatures MUST match `perry-runtime/src/value.rs` and friends byte-for-byte.
//! Mismatch is silent and deadly — the generated code calls the function and
//! gets garbage back (see anvil README §48 bug hunt).

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR, VOID};

/// Declare the minimum set of runtime functions needed by Phase 1
/// (`console.log(42)`):
/// - `js_console_log_dynamic(double)` — prints any NaN-boxed value
/// - `js_nanbox_string(i64) -> double` — wraps a raw string handle
/// - `js_nanbox_get_pointer(double) -> i64` — unwraps a NaN-boxed pointer
/// - `js_string_from_bytes(ptr, i32) -> i64` — interns a UTF-8 string
/// - `js_is_truthy(double) -> i32` — JS-ish truthiness test
/// - `js_gc_init()` — runtime bootstrap, called once at start of `main`
pub fn declare_phase1(module: &mut LlModule) {
    // GC / runtime bootstrap.
    module.declare_function("js_gc_init", VOID, &[]);

    // Console.
    module.declare_function("js_console_log_dynamic", VOID, &[DOUBLE]);
    module.declare_function("js_console_log_number", VOID, &[DOUBLE]);

    // NaN-boxing wrappers (bridge between raw handles and NaN-boxed doubles).
    module.declare_function("js_nanbox_string", DOUBLE, &[I64]);
    module.declare_function("js_nanbox_pointer", DOUBLE, &[I64]);
    module.declare_function("js_nanbox_get_pointer", I64, &[DOUBLE]);

    // Strings (enough to produce string literals for later phases).
    module.declare_function("js_string_from_bytes", I64, &[PTR, I32]);

    // Type checks.
    module.declare_function("js_is_truthy", I32, &[DOUBLE]);

    // Phase 2.1: timing primitives.
    declare_phase2_1(module);
}

/// Phase 2.1 additions: just `js_date_now()` for in-program timing harnesses.
pub fn declare_phase2_1(module: &mut LlModule) {
    module.declare_function("js_date_now", DOUBLE, &[]);

    // Phase A additions go here too — separate function once they grow.
    declare_phase_a_strings(module);
}

/// Phase A additions: string literal hoisting needs the GC to treat module
/// globals holding string handles as permanent roots. `js_gc_register_global_root`
/// pushes the address into `GLOBAL_ROOTS` (`crates/perry-runtime/src/gc.rs:233`)
/// which the mark phase scans alongside the stack.
pub fn declare_phase_a_strings(module: &mut LlModule) {
    module.declare_function("js_gc_register_global_root", VOID, &[I64]);

    // Phase B (core types) additions live here too — split into a separate
    // function once they grow.
    declare_phase_b_strings(module);
}

/// Phase B string operations.
///
/// `js_string_concat(*const StringHeader, *const StringHeader) -> *mut StringHeader`
/// — both arguments and the return value are raw i64 pointers in our ABI
/// (no NaN-tag). The codegen unboxes the operands by `bitcast double → i64`
/// and `and` with `POINTER_MASK` (0x0000_FFFF_FFFF_FFFF), then re-boxes the
/// result with `js_nanbox_string`.
pub fn declare_phase_b_strings(module: &mut LlModule) {
    module.declare_function("js_string_concat", I64, &[I64, I64]);
    // Dynamic string coercion: takes any NaN-boxed JSValue and returns a
    // raw string handle, formatting numbers via the same codegen path
    // Cranelift uses (`crates/perry-runtime/src/value.rs:813`).
    module.declare_function("js_jsvalue_to_string", I64, &[DOUBLE]);

    declare_phase_b_arrays(module);
}

/// Phase B array operations (number-typed arrays for the first slice).
///
/// All arrays are stored as raw i64 pointers at the runtime level. The
/// codegen NaN-boxes them with `POINTER_TAG` for storage in locals/params,
/// and unboxes back to raw i64 (`bitcast` + `and POINTER_MASK`) before
/// passing to runtime functions.
///
/// - `js_array_alloc(u32) -> *mut ArrayHeader` — allocate with capacity
/// - `js_array_push_f64(arr, value) -> arr*` — push element, may realloc
///   and return a NEW pointer that the caller must use going forward
/// - `js_array_get_f64(arr, index) -> f64` — read typed-number element
/// - `js_array_length(arr) -> u32` — length (u32, sitofp'd to double for
///   our number ABI)
pub fn declare_phase_b_arrays(module: &mut LlModule) {
    module.declare_function("js_array_alloc", I64, &[I32]);
    module.declare_function("js_array_push_f64", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_get_f64", DOUBLE, &[I64, I32]);
    module.declare_function("js_array_set_f64", VOID, &[I64, I32, DOUBLE]);
    // Extending variant: returns a possibly-realloc'd pointer that the
    // caller must write back to the local slot.
    module.declare_function("js_array_set_f64_extend", I64, &[I64, I32, DOUBLE]);
    module.declare_function("js_array_length", I32, &[I64]);

    declare_phase_b_objects(module);
}

/// Phase B object operations (basic object literals + property get/set).
///
/// - `js_object_alloc(class_id, field_count) -> *mut ObjectHeader` —
///   allocate with class_id=0 for anonymous object literals. The runtime
///   pre-allocates at least 8 inline slots regardless of field_count
///   (`crates/perry-runtime/src/object.rs:500`) to prevent buffer
///   overflow on later set_field calls.
/// - `js_object_set_field_by_name(obj, key, value)` — set field by string
///   key. Both `obj` and `key` are raw i64 pointers; `value` is a
///   NaN-boxed double.
/// - `js_object_get_field_by_name_f64(obj, key) -> f64` — read field by
///   string key, returning the raw f64 (or the NaN-boxed value for
///   non-number fields — same bit pattern, just interpreted differently).
///
/// Field name strings are sourced from the same StringPool the literal
/// strings use, so `obj.x` and `obj["x"]` and `let s = "x"; obj[s]` all
/// share one allocation per unique key.
///
/// Phase C will replace the bare `js_object_alloc(0, N)` path with the
/// shape-cached `js_object_alloc_with_shape` Cranelift uses
/// (`crates/perry-codegen/src/expr.rs:17942`) for repeated literals.
pub fn declare_phase_b_objects(module: &mut LlModule) {
    module.declare_function("js_object_alloc", I64, &[I32, I32]);
    module.declare_function("js_object_set_field_by_name", VOID, &[I64, I64, DOUBLE]);
    module.declare_function("js_object_get_field_by_name_f64", DOUBLE, &[I64, I64]);
}
