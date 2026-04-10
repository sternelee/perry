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

    // In-place append for the `x = x + y` pattern. When `x` has
    // refcount=1 (unique owner), the runtime mutates in-place and
    // returns the same pointer; otherwise it allocates a new string.
    // Either way the caller must use the returned pointer.
    // (`crates/perry-runtime/src/string.rs:88`)
    module.declare_function("js_string_append", I64, &[I64, I64]);

    // String methods (Phase B.12).
    // All take/return raw i64 string handles. Length args are i32.
    // - js_string_index_of(haystack, needle) -> i32
    // - js_string_index_of_from(haystack, needle, from) -> i32
    // - js_string_slice(s, start, end) -> *mut StringHeader (i64)
    // - js_string_substring(s, start, end) -> *mut StringHeader (i64)
    // - js_string_starts_with(s, prefix) -> i32 (boolean as 0/1)
    // - js_string_ends_with(s, suffix) -> i32
    module.declare_function("js_string_index_of", I32, &[I64, I64]);
    module.declare_function("js_string_index_of_from", I32, &[I64, I64, I32]);
    module.declare_function("js_string_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_string_substring", I64, &[I64, I32, I32]);
    module.declare_function("js_string_split", I64, &[I64, I64]);
    module.declare_function("js_math_pow", DOUBLE, &[DOUBLE, DOUBLE]);

    // Math.* unary functions: use LLVM intrinsics directly so we
    // get hardware instructions / libm calls instead of depending
    // on `js_math_*` runtime symbols (which the auto-optimize
    // dead-strip removes from libperry_runtime.a).
    module.declare_function("llvm.sqrt.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.floor.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.ceil.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.fabs.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.copysign.f64", DOUBLE, &[DOUBLE, DOUBLE]);
    // Keep js_math_pow for now — Math.pow has overflow / NaN
    // semantics that the libm pow doesn't quite match.

    // JSON.stringify (Phase B.15). The 2-arg form is JsonStringifyFull
    // in the HIR (value, type_hint, indent — actually 3 args; we use the
    // simple 2-arg js_json_stringify for now).
    module.declare_function("js_json_stringify", I64, &[DOUBLE, I32]);

    // Map (Phase B.15). The runtime stores keys/values as NaN-boxed doubles.
    // js_map_alloc returns a *mut MapHeader (i64 pointer).
    module.declare_function("js_map_alloc", I64, &[I32]);
    // typeof: returns a string handle ("number"/"string"/"boolean"/"undefined"/"object"/"function")
    module.declare_function("js_value_typeof", I64, &[DOUBLE]);
    module.declare_function("js_string_starts_with", I32, &[I64, I64]);
    module.declare_function("js_string_ends_with", I32, &[I64, I64]);

    // Closure / function-as-value primitives (Phase D).
    //
    // - js_closure_alloc(func_ptr, capture_count) -> *mut ClosureHeader
    //     Allocates a closure object pointing at the given function with
    //     space for `capture_count` captured-value slots.
    // - js_closure_set/get_capture_f64(closure, idx, value)
    //     Read/write a captured value (NaN-boxed double) at slot `idx`.
    // - js_closure_call0..call5(closure, args…) -> double
    //     Invoke the closure with N args. The runtime extracts the
    //     function pointer from the closure header and calls it with
    //     the closure as the first argument followed by the user args.
    module.declare_function("js_closure_alloc", I64, &[PTR, I32]);
    module.declare_function("js_closure_set_capture_f64", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_closure_get_capture_f64", DOUBLE, &[I64, I32]);
    module.declare_function("js_closure_call0", DOUBLE, &[I64]);
    module.declare_function("js_closure_call1", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_closure_call2", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call3", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call4", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call5", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);

    // Phase B.16 / D follow-ups: more runtime functions discovered
    // by the test-files sweep histogram.
    module.declare_function("js_array_map", I64, &[I64, I64]);
    module.declare_function("js_array_filter", I64, &[I64, I64]);
    module.declare_function("js_array_concat", I64, &[I64, I64]);
    module.declare_function("js_error_new", I64, &[]);
    module.declare_function("js_error_new_with_message", I64, &[I64]);
    module.declare_function("js_map_set", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_map_get", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_map_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_keys", I64, &[I64]);
    module.declare_function("js_is_finite", DOUBLE, &[DOUBLE]);
    module.declare_function("js_is_undefined_or_bare_nan", I32, &[DOUBLE]);
    module.declare_function("js_math_min_array", DOUBLE, &[I64]);
    module.declare_function("js_math_max_array", DOUBLE, &[I64]);
    module.declare_function("js_string_coerce", I64, &[DOUBLE]);
    module.declare_function("js_array_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_array_shift_f64", DOUBLE, &[I64]);
    module.declare_function("js_set_alloc", I64, &[I32]);
    module.declare_function("js_set_from_array", I64, &[I64]);
    module.declare_function("js_map_from_array", I64, &[I64]);
    module.declare_function("js_object_has_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_write_file_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_exists_sync", I32, &[DOUBLE]);
    module.declare_function("js_number_coerce", DOUBLE, &[DOUBLE]);
    module.declare_function("js_set_add", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_size", I32, &[I64]);
    module.declare_function("js_string_to_lower_case", I64, &[I64]);
    module.declare_function("js_string_to_upper_case", I64, &[I64]);
    module.declare_function("js_string_trim", I64, &[I64]);
    module.declare_function("js_string_trim_start", I64, &[I64]);
    module.declare_function("js_string_trim_end", I64, &[I64]);
    module.declare_function("js_string_char_at", I64, &[I64, I32]);
    module.declare_function("js_string_repeat", I64, &[I64, I32]);
    module.declare_function("js_string_replace_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_all_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_equals", I32, &[I64, I64]);
    module.declare_function("js_string_compare", I32, &[I64, I64]);
    module.declare_function("js_jsvalue_to_string_radix", I64, &[DOUBLE, I32]);
    module.declare_function("js_math_random", DOUBLE, &[]);
    module.declare_function("js_console_log_spread", VOID, &[I64]);
    module.declare_function("js_console_error_spread", VOID, &[I64]);
    module.declare_function("js_console_warn_spread", VOID, &[I64]);
    module.declare_function("js_getenv", I64, &[I64]);
    module.declare_function("js_console_table", VOID, &[DOUBLE]);
    // Heap-allocated mutable capture boxes.
    // See crates/perry-runtime/src/box.rs. These let multiple
    // closures share mutable state (e.g. a counter captured by
    // both inc() and get() in a returned object literal).
    module.declare_function("js_box_alloc", I64, &[DOUBLE]);
    module.declare_function("js_box_get", DOUBLE, &[I64]);
    module.declare_function("js_box_set", VOID, &[I64, DOUBLE]);
    module.declare_function("js_object_get_class_id", I32, &[I64]);
    module.declare_function("js_object_alloc_with_parent", I64, &[I32, I32, I32]);
    module.declare_function("js_object_delete_field", I32, &[I64, I64]);
    // js_eq takes JSValue (#[repr(transparent)] u64) for both
    // params + return — i64 in the ABI, not double.
    module.declare_function("js_eq", I64, &[I64, I64]);
    module.declare_function("js_loose_eq", I64, &[I64, I64]);
    module.declare_function("js_number_to_fixed", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_string_replace_regex", I64, &[I64, I64, I64]);
    module.declare_function("js_array_at", DOUBLE, &[I64, DOUBLE]);
    // Date getters: all take a timestamp double, return a double.
    module.declare_function("js_date_get_time", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_full_year", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_month", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_hours", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_minutes", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_seconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_milliseconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_full_year", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_month", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_hours", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_minutes", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_seconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_milliseconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_value_of", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_timezone_offset", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_to_iso_string", I64, &[DOUBLE]);
    module.declare_function("js_date_new_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_new_from_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_array_indexOf_f64", I32, &[I64, DOUBLE]);
    module.declare_function("js_array_includes_f64", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_size", I32, &[I64]);
    module.declare_function("js_map_clear", VOID, &[I64]);
    // Map iteration: entries/keys/values all take a map pointer and return an array pointer.
    module.declare_function("js_map_entries", I64, &[I64]);
    module.declare_function("js_map_keys", I64, &[I64]);
    module.declare_function("js_map_values", I64, &[I64]);
    // Map/Set forEach: (collection_ptr, callback_nanboxed_f64) -> void
    module.declare_function("js_map_foreach", VOID, &[I64, DOUBLE]);
    module.declare_function("js_set_foreach", VOID, &[I64, DOUBLE]);
    // Set to array conversion (for Set iteration via for...of)
    module.declare_function("js_set_to_array", I64, &[I64]);
    // Splice is unusual: takes an out-pointer for the deleted array
    // and returns the modified-in-place input (the splice point may
    // realloc). Param order is (arr, start, delete_count, items_ptr,
    // items_count, out_arr_ptr).
    module.declare_function("js_array_splice", I64, &[I64, I32, I32, PTR, I32, PTR]);
    module.declare_function("js_parse_int", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_parse_float", DOUBLE, &[I64]);
    module.declare_function("js_array_reduce", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_reduce_right", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_sort_default", I64, &[I64]);
    module.declare_function("js_array_reverse", I64, &[I64]);
    module.declare_function("js_array_flat", I64, &[I64]);
    module.declare_function("js_array_flatMap", I64, &[I64, I64]);
    module.declare_function("js_array_sort_with_comparator", I64, &[I64, I64]);
    module.declare_function("js_regexp_new", I64, &[I64, I64]);
    module.declare_function("js_regexp_test", I32, &[I64, I64]);
    module.declare_function("js_get_string_pointer_unified", I64, &[DOUBLE]);
    module.declare_function("js_bigint_from_string", I64, &[PTR, I32]);
    module.declare_function("js_instanceof", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    module.declare_function("js_object_values", I64, &[I64]);
    module.declare_function("js_object_entries", I64, &[I64]);
    module.declare_function("js_path_join", I64, &[I64, I64]);
    module.declare_function("js_path_dirname", I64, &[I64]);
    module.declare_function("js_path_relative", I64, &[I64, I64]);
    module.declare_function("js_object_from_entries", DOUBLE, &[DOUBLE]);
    module.declare_function("js_string_match", I64, &[I64, I64]);
    module.declare_function("llvm.log.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.log2.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.log10.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.exp.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.sin.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.cos.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("js_path_basename", I64, &[I64]);
    module.declare_function("js_path_extname", I64, &[I64]);
    module.declare_function("js_path_parse", I64, &[I64]);
    // JSON.parse returns JSValue (u64) via integer register on ARM64,
    // not f64. Use I64 return + bitcast to avoid ABI mismatch crash.
    module.declare_function("js_json_parse", I64, &[I64]);
    // Date string formatters
    module.declare_function("js_date_to_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_json", I64, &[DOUBLE]);
    // RegExp exec
    module.declare_function("js_regexp_exec", I64, &[I64, I64]);
    module.declare_function("js_number_to_precision", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_number_to_exponential", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_new", DOUBLE, &[]);
    module.declare_function("js_number_is_integer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_number_is_nan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_is", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_array_find", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_findIndex", I32, &[I64, I64]);
    module.declare_function("js_array_find_last", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_find_last_index", I32, &[I64, I64]);
    module.declare_function("js_array_some", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_every", DOUBLE, &[I64, I64]);

    // Phase E: async/await runtime support.
    // Promise polling: state is 0=pending, 1=fulfilled, 2=rejected.
    // The await busy-wait loop polls js_promise_state, calls
    // js_promise_run_microtasks + js_sleep_ms while pending, then
    // pulls the value via js_promise_value (or reason via
    // js_promise_reason on rejection).
    module.declare_function("js_promise_state", I32, &[I64]);
    module.declare_function("js_promise_value", DOUBLE, &[I64]);
    module.declare_function("js_promise_reason", DOUBLE, &[I64]);
    module.declare_function("js_promise_run_microtasks", I32, &[]);
    // js_stdlib_process_pending intentionally not declared — see
    // the await-loop comment in expr.rs for the dead-strip rationale.
    module.declare_function("js_sleep_ms", VOID, &[DOUBLE]);
    module.declare_function("js_throw", VOID, &[DOUBLE]);

    // Exception handling (Phase G): setjmp/longjmp-based try/catch.
    // js_try_push() returns a ptr to a jmp_buf.
    // setjmp(ptr) returns i32 (0 on first call, non-0 after longjmp).
    // js_try_end() pops the try depth (no return value).
    // js_get_exception() returns the thrown NaN-boxed value.
    // js_clear_exception() resets the exception state.
    // js_has_exception() returns i32 (1 if exception is active, 0 otherwise).
    // js_enter_finally() / js_leave_finally() bracket finally blocks.
    module.declare_function("js_try_push", PTR, &[]);
    module.declare_function("setjmp", I32, &[PTR]);
    module.declare_function("js_try_end", VOID, &[]);
    module.declare_function("js_get_exception", DOUBLE, &[]);
    module.declare_function("js_clear_exception", VOID, &[]);
    module.declare_function("js_has_exception", I32, &[]);
    module.declare_function("js_enter_finally", VOID, &[]);
    module.declare_function("js_leave_finally", VOID, &[]);
    module.declare_function("js_await_any_promise", DOUBLE, &[DOUBLE]);
    module.declare_function("js_promise_new", I64, &[]);
    module.declare_function("js_promise_resolve", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_reject", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_resolved", I64, &[DOUBLE]);
    module.declare_function("js_promise_rejected", I64, &[DOUBLE]);
    module.declare_function("js_promise_then", I64, &[I64, I64, I64]);
    module.declare_function("js_promise_all", I64, &[I64]);
    module.declare_function("js_promise_race", I64, &[I64]);
    module.declare_function("js_promise_all_settled", I64, &[I64]);
    module.declare_function("js_array_unshift_f64", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_entries", I64, &[I64]);
    module.declare_function("js_array_keys", I64, &[I64]);
    module.declare_function("js_array_values", I64, &[I64]);

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

    // Array methods (Phase B.12).
    // - js_array_pop_f64(arr) -> f64    (last element, NaN if empty)
    // - js_array_join(arr, sep) -> *mut StringHeader (i64)
    module.declare_function("js_array_pop_f64", DOUBLE, &[I64]);
    module.declare_function("js_array_join", I64, &[I64, I64]);

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
