//! Runtime Library for Perry
//!
//! Provides the runtime support needed by compiled TypeScript programs:
//! - JSValue representation (NaN-boxing)
//! - Object representation and allocation
//! - Array representation and operations
//! - Garbage collection integration
//! - Built-in object implementations
//! - Console and other global functions

pub mod value;
pub mod gc;
pub mod arena;
pub mod object;
pub mod array;
pub mod map;
pub mod set;
pub mod string;
pub mod bigint;
pub mod closure;
pub mod exception;
pub mod error;
pub mod promise;
pub mod timer;
pub mod builtins;
pub mod r#box;
pub mod process;
pub mod fs;
pub mod path;
pub mod math;
pub mod date;
pub mod url;
pub mod regex;
pub mod os;
pub mod buffer;
pub mod child_process;
pub mod net;
pub mod json;
pub mod static_plugins;
#[cfg(not(feature = "stdlib"))]
pub mod stdlib_stubs;
#[cfg(feature = "full")]
pub mod redis_client;
#[cfg(feature = "full")]
pub mod plugin;

pub use value::JSValue;
pub use promise::Promise;
pub use object::ObjectHeader;
pub use array::ArrayHeader;
pub use map::MapHeader;
pub use set::SetHeader;
pub use string::StringHeader;
pub use bigint::BigIntHeader;
pub use closure::ClosureHeader;
pub use regex::RegExpHeader;
pub use buffer::BufferHeader;

// Re-export closure module for stdlib to use js_closure_call* functions
pub use closure::{js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3};

// Re-export commonly used FFI functions for stdlib
pub use array::{js_array_alloc, js_array_set, js_array_get, js_array_push, js_array_length, js_array_is_array, js_array_get_jsvalue};
pub use object::{js_object_alloc, js_object_set_field, js_object_set_field_f64, js_object_get_field, js_object_set_keys, js_object_keys, js_object_values, js_object_entries, js_object_get_field_by_name, js_object_get_field_by_name_f64};
pub use string::js_string_from_bytes;
pub use promise::{js_promise_new, js_promise_resolve, js_promise_reject};
pub use bigint::js_bigint_from_string;
pub use value::{js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, js_get_string_pointer_unified, js_jsvalue_to_string};
pub use value::{js_set_handle_array_get, js_set_handle_array_length, js_set_handle_object_get_property, js_set_handle_to_string, js_set_handle_call_method, js_set_native_module_js_loader, js_set_new_from_handle_v8};
pub use array::{js_array_push_f64};
pub use object::js_object_set_field_by_name;
pub use promise::{js_promise_run_microtasks, js_promise_state, js_is_promise, js_promise_value};

// Module init guard for preventing circular dependency stack overflow.
// Uses a simple bitset in the runtime so Cranelift cannot optimize it away.
mod init_guard {
    use std::sync::atomic::{AtomicU8, Ordering};

    // Support up to 2048 modules (256 bytes). Each bit = one module.
    const GUARD_BYTES: usize = 256;
    static INIT_GUARD: [AtomicU8; GUARD_BYTES] = {
        const ZERO: AtomicU8 = AtomicU8::new(0);
        [ZERO; GUARD_BYTES]
    };

    /// Check and set the init guard for a module. Returns 1 if already set (skip init),
    /// 0 if not set (proceed with init). The guard is set atomically.
    #[no_mangle]
    pub extern "C" fn perry_init_guard_check_and_set(module_id: u64) -> i32 {
        let byte_idx = (module_id as usize) / 8;
        let bit_idx = (module_id as usize) % 8;
        if byte_idx >= GUARD_BYTES {
            return 0; // Out of range, don't guard
        }
        let mask = 1u8 << bit_idx;
        let prev = INIT_GUARD[byte_idx].fetch_or(mask, Ordering::SeqCst);
        if prev & mask != 0 { 1 } else { 0 }
    }
}
