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
}
