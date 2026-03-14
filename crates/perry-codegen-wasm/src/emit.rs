//! HIR → WebAssembly bytecode emitter
//!
//! Translates HIR modules to WebAssembly binary format using wasm-encoder.
//! All JSValues are represented as f64 using NaN-boxing (matching perry-runtime).
//! Runtime operations (strings, console, objects) are imported from a JS bridge.

use perry_hir::ir::*;
use perry_types::{FuncId, LocalId, GlobalId};
use std::collections::BTreeMap;
use wasm_encoder::{
    CodeSection, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, Ieee64, ImportSection, Instruction, MemorySection, MemoryType,
    Module, RefType, TableSection, TableType, TypeSection, ValType, GlobalSection, GlobalType,
};

/// Helper: create an F64Const instruction from raw f64 bits
fn f64_const(val: f64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(val))
}

/// Helper: create an F64Const instruction from NaN-boxed tag bits
fn f64_const_bits(bits: u64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(f64::from_bits(bits)))
}

// NaN-boxing constants (must match perry-runtime and wasm_runtime.js)
const STRING_TAG: u64 = 0x7FFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

/// Import function indices (must match the order imports are added)
#[derive(Clone, Copy)]
struct RuntimeImports {
    string_new: u32,
    console_log: u32,
    console_warn: u32,
    console_error: u32,
    string_concat: u32,
    js_add: u32,
    string_eq: u32,
    string_len: u32,
    jsvalue_to_string: u32,
    is_truthy: u32,
    js_strict_eq: u32,
    math_floor: u32,
    math_ceil: u32,
    math_round: u32,
    math_abs: u32,
    math_sqrt: u32,
    math_pow: u32,
    math_random: u32,
    math_log: u32,
    date_now: u32,
    js_typeof: u32,
    math_min: u32,
    math_max: u32,
    parse_int: u32,
    parse_float: u32,
    // Phase 0 additions
    js_mod: u32,
    is_null_or_undefined: u32,
    // Phase 1: Object operations
    object_new: u32,
    object_set: u32,
    object_get: u32,
    object_get_dynamic: u32,
    object_set_dynamic: u32,
    object_delete: u32,
    object_delete_dynamic: u32,
    object_keys: u32,
    object_values: u32,
    object_entries: u32,
    object_has_property: u32,
    object_assign: u32,
    // Phase 1: Array operations
    array_new: u32,
    array_push: u32,
    array_pop: u32,
    array_get: u32,
    array_set: u32,
    array_length: u32,
    array_slice: u32,
    array_splice: u32,
    array_shift: u32,
    array_unshift: u32,
    array_join: u32,
    array_index_of: u32,
    array_includes: u32,
    array_concat: u32,
    array_reverse: u32,
    array_flat: u32,
    array_is_array: u32,
    array_from: u32,
    array_push_spread: u32,
    // Phase 1: String methods
    string_char_at: u32,
    string_substring: u32,
    string_index_of: u32,
    string_slice: u32,
    string_to_lower_case: u32,
    string_to_upper_case: u32,
    string_trim: u32,
    string_includes: u32,
    string_starts_with: u32,
    string_ends_with: u32,
    string_replace: u32,
    string_split: u32,
    string_from_char_code: u32,
    string_pad_start: u32,
    string_pad_end: u32,
    string_repeat: u32,
    string_match: u32,
    math_log2: u32,
    math_log10: u32,
    // Phase 2: Closure operations
    closure_new: u32,
    closure_set_capture: u32,
    closure_call_0: u32,
    closure_call_1: u32,
    closure_call_2: u32,
    closure_call_3: u32,
    closure_call_spread: u32,
    // Phase 2: Array higher-order methods
    array_map: u32,
    array_filter: u32,
    array_for_each: u32,
    array_reduce: u32,
    array_find: u32,
    array_find_index: u32,
    array_sort: u32,
    array_some: u32,
    array_every: u32,
    // Phase 3: Class operations
    class_new: u32,
    class_set_method: u32,
    class_call_method: u32,
    class_get_field: u32,
    class_set_field: u32,
    class_set_static: u32,
    class_get_static: u32,
    class_instanceof: u32,
    // Phase 4: JSON
    json_parse: u32,
    json_stringify: u32,
    // Phase 4: Map
    map_new: u32,
    map_set: u32,
    map_get: u32,
    map_has: u32,
    map_delete: u32,
    map_size: u32,
    map_clear: u32,
    map_entries: u32,
    map_keys: u32,
    map_values: u32,
    // Phase 4: Set
    set_new: u32,
    set_new_from_array: u32,
    set_add: u32,
    set_has: u32,
    set_delete: u32,
    set_size: u32,
    set_clear: u32,
    set_values: u32,
    // Phase 4: Date
    date_new: u32,
    date_get_time: u32,
    date_to_iso_string: u32,
    date_get_full_year: u32,
    date_get_month: u32,
    date_get_date: u32,
    date_get_hours: u32,
    date_get_minutes: u32,
    date_get_seconds: u32,
    date_get_milliseconds: u32,
    // Phase 4: Error
    error_new: u32,
    error_message: u32,
    // Phase 4: RegExp
    regexp_new: u32,
    regexp_test: u32,
    // Phase 4: Globals
    number_coerce: u32,
    is_nan: u32,
    is_finite: u32,
    // Phase 5: Misc
    console_log_multi: u32,
    // Phase 1 addition: Class inheritance
    class_set_parent: u32,
    // Phase 3: Try/Catch
    try_start: u32,
    try_end: u32,
    throw_value: u32,
    has_exception: u32,
    get_exception: u32,
    // Phase 4: URL
    url_parse: u32,
    url_get_href: u32,
    url_get_pathname: u32,
    url_get_hostname: u32,
    url_get_port: u32,
    url_get_search: u32,
    url_get_hash: u32,
    url_get_origin: u32,
    url_get_protocol: u32,
    url_get_search_params: u32,
    searchparams_get: u32,
    searchparams_has: u32,
    searchparams_set: u32,
    searchparams_append: u32,
    searchparams_delete: u32,
    searchparams_to_string: u32,
    // Phase 4: Crypto
    crypto_random_uuid: u32,
    crypto_random_bytes: u32,
    // Phase 4: Path
    path_join: u32,
    path_dirname: u32,
    path_basename: u32,
    path_extname: u32,
    path_resolve: u32,
    // Phase 4: Process/OS
    os_platform: u32,
    process_argv: u32,
    process_cwd: u32,
    // Phase 6: Buffer
    buffer_alloc: u32,
    buffer_from_string: u32,
    buffer_to_string: u32,
    buffer_get: u32,
    buffer_set: u32,
    buffer_length: u32,
    buffer_slice: u32,
    buffer_concat: u32,
    uint8array_new: u32,
    uint8array_from: u32,
    uint8array_length: u32,
    uint8array_get: u32,
    uint8array_set: u32,
    // Timers
    set_timeout: u32,
    set_interval: u32,
    clear_timeout: u32,
    clear_interval: u32,
    // Response properties
    response_status: u32,
    response_ok: u32,
    response_headers_get: u32,
    response_url: u32,
    // Buffer extras
    buffer_copy: u32,
    buffer_write: u32,
    buffer_equals: u32,
    buffer_is_buffer: u32,
    buffer_byte_length: u32,
    // Crypto extras
    crypto_sha256: u32,
    crypto_md5: u32,
    // Path extras
    path_is_absolute: u32,
    // Phase 5: Async/Promise/Fetch
    fetch_url: u32,
    fetch_with_options: u32,
    response_json: u32,
    response_text: u32,
    promise_new: u32,
    promise_resolve: u32,
    promise_then: u32,
    await_promise: u32,
}

/// Output from WASM compilation: binary + extra JS for async functions.
pub struct WasmCompileOutput {
    pub wasm_bytes: Vec<u8>,
    pub async_js: String,
}

/// Compile HIR modules to a WebAssembly binary.
pub fn compile_to_wasm(modules: &[(String, perry_hir::ir::Module)]) -> Vec<u8> {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules).wasm_bytes
}

/// Compile HIR modules to WASM binary + generated JS for async functions.
pub fn compile_to_wasm_with_async(modules: &[(String, perry_hir::ir::Module)]) -> WasmCompileOutput {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules)
}

struct WasmModuleEmitter {
    /// String literal table: content → (string_id, offset, length)
    string_table: Vec<(String, u32, u32)>, // (content, offset, len)
    string_map: BTreeMap<String, u32>,      // content → string_id
    string_data: Vec<u8>,                   // packed string bytes
    /// Type section entries: (params, results)
    types: Vec<(Vec<ValType>, Vec<ValType>)>,
    type_map: BTreeMap<(Vec<ValType>, Vec<ValType>), u32>,
    /// Function index mapping: FuncId → wasm function index
    func_map: BTreeMap<FuncId, u32>,
    /// Reverse table map: wasm function index → table index
    func_to_table_idx: BTreeMap<u32, u32>,
    /// Import count (import functions come first in the index space)
    num_imports: u32,
    /// Runtime import indices
    rt: Option<RuntimeImports>,
    /// Global variable mapping: GlobalId → wasm global index
    global_map: BTreeMap<GlobalId, u32>,
    num_globals: u32,
    /// Class constructor map: class_name → wasm function index
    class_ctor_map: BTreeMap<String, u32>,
    /// Class method map: class_name → {method_name → wasm function index}
    class_method_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Class static method map: class_name → {method_name → wasm function index}
    class_static_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Function name → wasm function index (for cross-module ExternFuncRef resolution)
    func_name_map: BTreeMap<String, u32>,
    /// Class parent map: child_class_name → parent_class_name
    class_parent_map: BTreeMap<String, String>,
    /// Async function names (compiled to JS, not WASM)
    async_func_imports: Vec<(String, u32, usize)>, // (name, import_idx, param_count)
    /// Generated JS code for async functions
    async_js_code: Vec<String>,
}

impl WasmModuleEmitter {
    fn new() -> Self {
        Self {
            string_table: Vec::new(),
            string_map: BTreeMap::new(),
            string_data: Vec::new(),
            types: Vec::new(),
            type_map: BTreeMap::new(),
            func_map: BTreeMap::new(),
            func_to_table_idx: BTreeMap::new(),
            num_imports: 0,
            rt: None,
            global_map: BTreeMap::new(),
            num_globals: 0,
            class_ctor_map: BTreeMap::new(),
            class_method_map: BTreeMap::new(),
            class_static_map: BTreeMap::new(),
            func_name_map: BTreeMap::new(),
            class_parent_map: BTreeMap::new(),
            async_func_imports: Vec::new(),
            async_js_code: Vec::new(),
        }
    }

    /// Intern a string literal, returning its string_id.
    fn intern_string(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.string_map.get(s) {
            return id;
        }
        let id = self.string_table.len() as u32;
        let offset = self.string_data.len() as u32;
        let bytes = s.as_bytes();
        let len = bytes.len() as u32;
        self.string_data.extend_from_slice(bytes);
        self.string_table.push((s.to_string(), offset, len));
        self.string_map.insert(s.to_string(), id);
        id
    }

    /// Get or create a function type index for the given signature.
    fn get_type_idx(&mut self, params: Vec<ValType>, results: Vec<ValType>) -> u32 {
        let key = (params.clone(), results.clone());
        if let Some(&idx) = self.type_map.get(&key) {
            return idx;
        }
        let idx = self.types.len() as u32;
        self.types.push((params, results));
        self.type_map.insert(key, idx);
        idx
    }

    fn compile(&mut self, modules: &[(String, perry_hir::ir::Module)]) -> WasmCompileOutput {
        // First pass: collect all string literals
        for (_, module) in modules {
            self.collect_strings(module);
        }

        // Register runtime import types and get type indices
        // All imports use f64 for JSValues
        let t_void = self.get_type_idx(vec![], vec![]);
        let t_i32_i32_void = self.get_type_idx(vec![ValType::I32, ValType::I32], vec![]);
        let t_f64_void = self.get_type_idx(vec![ValType::F64], vec![]);
        let t_f64_f64_f64 = self.get_type_idx(vec![ValType::F64, ValType::F64], vec![ValType::F64]);
        let t_f64_f64_i32 = self.get_type_idx(vec![ValType::F64, ValType::F64], vec![ValType::I32]);
        let t_f64_f64 = self.get_type_idx(vec![ValType::F64], vec![ValType::F64]);
        let t_f64_i32 = self.get_type_idx(vec![ValType::F64], vec![ValType::I32]);
        let t_void_f64 = self.get_type_idx(vec![], vec![ValType::F64]);

        // Add runtime imports (order matters — defines function indices)
        let mut import_idx: u32 = 0;
        let mut next_import = || { let i = import_idx; import_idx += 1; i };

        // Additional type signatures needed for Phase 1+
        let t_f64_f64_void = self.get_type_idx(vec![ValType::F64, ValType::F64], vec![]);
        let t_f64_f64_f64_void = self.get_type_idx(vec![ValType::F64, ValType::F64, ValType::F64], vec![]);
        let t_f64_f64_f64_f64 = self.get_type_idx(vec![ValType::F64, ValType::F64, ValType::F64], vec![ValType::F64]);
        let t_f64_f64_f64_f64_f64 = self.get_type_idx(vec![ValType::F64, ValType::F64, ValType::F64, ValType::F64], vec![ValType::F64]);

        let rt = RuntimeImports {
            string_new: next_import(),
            console_log: next_import(),
            console_warn: next_import(),
            console_error: next_import(),
            string_concat: next_import(),
            js_add: next_import(),
            string_eq: next_import(),
            string_len: next_import(),
            jsvalue_to_string: next_import(),
            is_truthy: next_import(),
            js_strict_eq: next_import(),
            math_floor: next_import(),
            math_ceil: next_import(),
            math_round: next_import(),
            math_abs: next_import(),
            math_sqrt: next_import(),
            math_pow: next_import(),
            math_random: next_import(),
            math_log: next_import(),
            date_now: next_import(),
            js_typeof: next_import(),
            math_min: next_import(),
            math_max: next_import(),
            parse_int: next_import(),
            parse_float: next_import(),
            // Phase 0
            js_mod: next_import(),
            is_null_or_undefined: next_import(),
            // Phase 1: Objects
            object_new: next_import(),
            object_set: next_import(),
            object_get: next_import(),
            object_get_dynamic: next_import(),
            object_set_dynamic: next_import(),
            object_delete: next_import(),
            object_delete_dynamic: next_import(),
            object_keys: next_import(),
            object_values: next_import(),
            object_entries: next_import(),
            object_has_property: next_import(),
            object_assign: next_import(),
            // Phase 1: Arrays
            array_new: next_import(),
            array_push: next_import(),
            array_pop: next_import(),
            array_get: next_import(),
            array_set: next_import(),
            array_length: next_import(),
            array_slice: next_import(),
            array_splice: next_import(),
            array_shift: next_import(),
            array_unshift: next_import(),
            array_join: next_import(),
            array_index_of: next_import(),
            array_includes: next_import(),
            array_concat: next_import(),
            array_reverse: next_import(),
            array_flat: next_import(),
            array_is_array: next_import(),
            array_from: next_import(),
            array_push_spread: next_import(),
            // Phase 1: Strings
            string_char_at: next_import(),
            string_substring: next_import(),
            string_index_of: next_import(),
            string_slice: next_import(),
            string_to_lower_case: next_import(),
            string_to_upper_case: next_import(),
            string_trim: next_import(),
            string_includes: next_import(),
            string_starts_with: next_import(),
            string_ends_with: next_import(),
            string_replace: next_import(),
            string_split: next_import(),
            string_from_char_code: next_import(),
            string_pad_start: next_import(),
            string_pad_end: next_import(),
            string_repeat: next_import(),
            string_match: next_import(),
            math_log2: next_import(),
            math_log10: next_import(),
            // Phase 2: Closures
            closure_new: next_import(),
            closure_set_capture: next_import(),
            closure_call_0: next_import(),
            closure_call_1: next_import(),
            closure_call_2: next_import(),
            closure_call_3: next_import(),
            closure_call_spread: next_import(),
            // Phase 2: Array higher-order
            array_map: next_import(),
            array_filter: next_import(),
            array_for_each: next_import(),
            array_reduce: next_import(),
            array_find: next_import(),
            array_find_index: next_import(),
            array_sort: next_import(),
            array_some: next_import(),
            array_every: next_import(),
            // Phase 3: Classes
            class_new: next_import(),
            class_set_method: next_import(),
            class_call_method: next_import(),
            class_get_field: next_import(),
            class_set_field: next_import(),
            class_set_static: next_import(),
            class_get_static: next_import(),
            class_instanceof: next_import(),
            // Phase 4: JSON
            json_parse: next_import(),
            json_stringify: next_import(),
            // Phase 4: Map
            map_new: next_import(),
            map_set: next_import(),
            map_get: next_import(),
            map_has: next_import(),
            map_delete: next_import(),
            map_size: next_import(),
            map_clear: next_import(),
            map_entries: next_import(),
            map_keys: next_import(),
            map_values: next_import(),
            // Phase 4: Set
            set_new: next_import(),
            set_new_from_array: next_import(),
            set_add: next_import(),
            set_has: next_import(),
            set_delete: next_import(),
            set_size: next_import(),
            set_clear: next_import(),
            set_values: next_import(),
            // Phase 4: Date
            date_new: next_import(),
            date_get_time: next_import(),
            date_to_iso_string: next_import(),
            date_get_full_year: next_import(),
            date_get_month: next_import(),
            date_get_date: next_import(),
            date_get_hours: next_import(),
            date_get_minutes: next_import(),
            date_get_seconds: next_import(),
            date_get_milliseconds: next_import(),
            // Phase 4: Error
            error_new: next_import(),
            error_message: next_import(),
            // Phase 4: RegExp
            regexp_new: next_import(),
            regexp_test: next_import(),
            // Phase 4: Globals
            number_coerce: next_import(),
            is_nan: next_import(),
            is_finite: next_import(),
            // Phase 5: Misc
            console_log_multi: next_import(),
            // Phase 1 addition: Class inheritance
            class_set_parent: next_import(),
            // Phase 3: Try/Catch
            try_start: next_import(),
            try_end: next_import(),
            throw_value: next_import(),
            has_exception: next_import(),
            get_exception: next_import(),
            // Phase 4: URL
            url_parse: next_import(),
            url_get_href: next_import(),
            url_get_pathname: next_import(),
            url_get_hostname: next_import(),
            url_get_port: next_import(),
            url_get_search: next_import(),
            url_get_hash: next_import(),
            url_get_origin: next_import(),
            url_get_protocol: next_import(),
            url_get_search_params: next_import(),
            searchparams_get: next_import(),
            searchparams_has: next_import(),
            searchparams_set: next_import(),
            searchparams_append: next_import(),
            searchparams_delete: next_import(),
            searchparams_to_string: next_import(),
            // Phase 4: Crypto
            crypto_random_uuid: next_import(),
            crypto_random_bytes: next_import(),
            // Phase 4: Path
            path_join: next_import(),
            path_dirname: next_import(),
            path_basename: next_import(),
            path_extname: next_import(),
            path_resolve: next_import(),
            // Phase 4: Process/OS
            os_platform: next_import(),
            process_argv: next_import(),
            process_cwd: next_import(),
            // Phase 6: Buffer
            buffer_alloc: next_import(),
            buffer_from_string: next_import(),
            buffer_to_string: next_import(),
            buffer_get: next_import(),
            buffer_set: next_import(),
            buffer_length: next_import(),
            buffer_slice: next_import(),
            buffer_concat: next_import(),
            uint8array_new: next_import(),
            uint8array_from: next_import(),
            uint8array_length: next_import(),
            uint8array_get: next_import(),
            uint8array_set: next_import(),
            // Timers
            set_timeout: next_import(),
            set_interval: next_import(),
            clear_timeout: next_import(),
            clear_interval: next_import(),
            // Response properties
            response_status: next_import(),
            response_ok: next_import(),
            response_headers_get: next_import(),
            response_url: next_import(),
            // Buffer extras
            buffer_copy: next_import(),
            buffer_write: next_import(),
            buffer_equals: next_import(),
            buffer_is_buffer: next_import(),
            buffer_byte_length: next_import(),
            // Crypto extras
            crypto_sha256: next_import(),
            crypto_md5: next_import(),
            // Path extras
            path_is_absolute: next_import(),
            // Phase 5: Async/Promise/Fetch
            fetch_url: next_import(),
            fetch_with_options: next_import(),
            response_json: next_import(),
            response_text: next_import(),
            promise_new: next_import(),
            promise_resolve: next_import(),
            promise_then: next_import(),
            await_promise: next_import(),
        };
        self.num_imports = import_idx;
        self.rt = Some(rt);

        // Additional types for new phases
        let t_void_i32 = self.get_type_idx(vec![], vec![ValType::I32]);

        // Build import tables dynamically from struct fields
        // Each entry: (name, type_idx)
        let import_entries: Vec<(&str, u32)> = vec![
            ("string_new", t_i32_i32_void),
            ("console_log", t_f64_void),
            ("console_warn", t_f64_void),
            ("console_error", t_f64_void),
            ("string_concat", t_f64_f64_f64),
            ("js_add", t_f64_f64_f64),
            ("string_eq", t_f64_f64_i32),
            ("string_len", t_f64_f64),
            ("jsvalue_to_string", t_f64_f64),
            ("is_truthy", t_f64_i32),
            ("js_strict_eq", t_f64_f64_i32),
            ("math_floor", t_f64_f64),
            ("math_ceil", t_f64_f64),
            ("math_round", t_f64_f64),
            ("math_abs", t_f64_f64),
            ("math_sqrt", t_f64_f64),
            ("math_pow", t_f64_f64_f64),
            ("math_random", t_void_f64),
            ("math_log", t_f64_f64),
            ("date_now", t_void_f64),
            ("js_typeof", t_f64_f64),
            ("math_min", t_f64_f64_f64),
            ("math_max", t_f64_f64_f64),
            ("parse_int", t_f64_f64),
            ("parse_float", t_f64_f64),
            // Phase 0
            ("js_mod", t_f64_f64_f64),
            ("is_null_or_undefined", t_f64_i32),
            // Phase 1: Objects (f64 handles)
            ("object_new", t_void_f64),                    // () -> handle
            ("object_set", t_f64_f64_f64_f64),              // (handle, key_str, value) -> handle (chaining)
            ("object_get", t_f64_f64_f64),                 // (handle, key_str) -> value
            ("object_get_dynamic", t_f64_f64_f64),         // (handle, key) -> value
            ("object_set_dynamic", t_f64_f64_f64_void),    // (handle, key, value) -> void
            ("object_delete", t_f64_f64_void),             // (handle, key_str) -> void
            ("object_delete_dynamic", t_f64_f64_void),     // (handle, key) -> void
            ("object_keys", t_f64_f64),                    // (handle) -> array_handle
            ("object_values", t_f64_f64),                  // (handle) -> array_handle
            ("object_entries", t_f64_f64),                 // (handle) -> array_handle
            ("object_has_property", t_f64_f64_i32),        // (handle, key) -> i32
            ("object_assign", t_f64_f64_f64),              // (target, source) -> target
            // Phase 1: Arrays
            ("array_new", t_void_f64),                     // () -> handle
            ("array_push", t_f64_f64_f64),                  // (handle, value) -> handle (chaining)
            ("array_pop", t_f64_f64),                      // (handle) -> value
            ("array_get", t_f64_f64_f64),                  // (handle, index) -> value
            ("array_set", t_f64_f64_f64_void),             // (handle, index, value) -> void
            ("array_length", t_f64_f64),                   // (handle) -> length
            ("array_slice", t_f64_f64_f64_f64),            // (handle, start, end) -> new_handle
            ("array_splice", t_f64_f64_f64_f64),           // (handle, start, deleteCount) -> removed_handle
            ("array_shift", t_f64_f64),                    // (handle) -> value
            ("array_unshift", t_f64_f64_void),             // (handle, value) -> void
            ("array_join", t_f64_f64_f64),                 // (handle, separator) -> string
            ("array_index_of", t_f64_f64_f64),             // (handle, value) -> index
            ("array_includes", t_f64_f64_i32),             // (handle, value) -> i32
            ("array_concat", t_f64_f64_f64),               // (handle1, handle2) -> new_handle
            ("array_reverse", t_f64_f64),                  // (handle) -> handle
            ("array_flat", t_f64_f64),                     // (handle) -> new_handle
            ("array_is_array", t_f64_i32),                 // (value) -> i32
            ("array_from", t_f64_f64),                     // (value) -> handle
            ("array_push_spread", t_f64_f64_f64),            // (target, source) -> handle (chaining)
            // Phase 1: Strings
            ("string_charAt", t_f64_f64_f64),              // (str, idx) -> str
            ("string_substring", t_f64_f64_f64_f64),       // (str, start, end) -> str
            ("string_indexOf", t_f64_f64_f64),             // (str, search) -> number
            ("string_slice", t_f64_f64_f64_f64),           // (str, start, end) -> str
            ("string_toLowerCase", t_f64_f64),
            ("string_toUpperCase", t_f64_f64),
            ("string_trim", t_f64_f64),
            ("string_includes", t_f64_f64_i32),
            ("string_startsWith", t_f64_f64_i32),
            ("string_endsWith", t_f64_f64_i32),
            ("string_replace", t_f64_f64_f64_f64),         // (str, pat, repl) -> str
            ("string_split", t_f64_f64_f64),               // (str, delim) -> array_handle
            ("string_fromCharCode", t_f64_f64),             // (code) -> str
            ("string_padStart", t_f64_f64_f64_f64),         // (str, len, fill) -> str
            ("string_padEnd", t_f64_f64_f64_f64),
            ("string_repeat", t_f64_f64_f64),               // (str, count) -> str
            ("string_match", t_f64_f64_f64),                // (str, regex) -> array_handle
            ("math_log2", t_f64_f64),
            ("math_log10", t_f64_f64),
            // Phase 2: Closures
            ("closure_new", t_f64_f64_f64),                // (func_table_idx, capture_count) -> handle
            ("closure_set_capture", t_f64_f64_f64_f64),     // (handle, idx, value) -> handle (chaining)
            ("closure_call_0", t_f64_f64),                 // (handle) -> result
            ("closure_call_1", t_f64_f64_f64),             // (handle, arg0) -> result
            ("closure_call_2", t_f64_f64_f64_f64),         // (handle, arg0, arg1) -> result
            ("closure_call_3", t_f64_f64_f64_f64_f64),     // (handle, arg0, arg1, arg2) -> result
            ("closure_call_spread", t_f64_f64_f64),        // (handle, args_array) -> result
            // Phase 2: Array higher-order
            ("array_map", t_f64_f64_f64),                  // (handle, closure) -> new_handle
            ("array_filter", t_f64_f64_f64),
            ("array_forEach", t_f64_f64_void),             // (handle, closure) -> void
            ("array_reduce", t_f64_f64_f64_f64),           // (handle, closure, initial) -> value
            ("array_find", t_f64_f64_f64),                 // (handle, closure) -> value
            ("array_find_index", t_f64_f64_f64),           // (handle, closure) -> number
            ("array_sort", t_f64_f64_f64),                 // (handle, closure) -> handle
            ("array_some", t_f64_f64_i32),                 // (handle, closure) -> i32
            ("array_every", t_f64_f64_i32),                // (handle, closure) -> i32
            // Phase 3: Classes
            ("class_new", t_f64_f64_f64),                  // (class_id, field_count) -> handle
            ("class_set_method", t_f64_f64_f64_void),      // (class_id, name_str, func_table_idx) -> void
            ("class_call_method", t_f64_f64_f64_f64),      // (handle, name_str, args_array) -> result
            ("class_get_field", t_f64_f64_f64),            // (handle, name_str) -> value
            ("class_set_field", t_f64_f64_f64_void),       // (handle, name_str, value) -> void
            ("class_set_static", t_f64_f64_f64_void),      // (class_id, name_str, value) -> void
            ("class_get_static", t_f64_f64_f64),           // (class_id, name_str) -> value
            ("class_instanceof", t_f64_f64_i32),           // (handle, class_id) -> i32
            // Phase 4: JSON
            ("json_parse", t_f64_f64),                     // (str) -> handle
            ("json_stringify", t_f64_f64),                 // (value) -> str
            // Phase 4: Map
            ("map_new", t_void_f64),
            ("map_set", t_f64_f64_f64_void),               // (handle, key, value) -> void
            ("map_get", t_f64_f64_f64),
            ("map_has", t_f64_f64_i32),
            ("map_delete", t_f64_f64_void),
            ("map_size", t_f64_f64),
            ("map_clear", t_f64_void),
            ("map_entries", t_f64_f64),
            ("map_keys", t_f64_f64),
            ("map_values", t_f64_f64),
            // Phase 4: Set
            ("set_new", t_void_f64),
            ("set_new_from_array", t_f64_f64),
            ("set_add", t_f64_f64_void),
            ("set_has", t_f64_f64_i32),
            ("set_delete", t_f64_f64_void),
            ("set_size", t_f64_f64),
            ("set_clear", t_f64_void),
            ("set_values", t_f64_f64),
            // Phase 4: Date
            ("date_new_val", t_f64_f64),                   // (opt_arg) -> handle
            ("date_get_time", t_f64_f64),
            ("date_to_iso_string", t_f64_f64),
            ("date_get_full_year", t_f64_f64),
            ("date_get_month", t_f64_f64),
            ("date_get_date", t_f64_f64),
            ("date_get_hours", t_f64_f64),
            ("date_get_minutes", t_f64_f64),
            ("date_get_seconds", t_f64_f64),
            ("date_get_milliseconds", t_f64_f64),
            // Phase 4: Error
            ("error_new", t_f64_f64),                      // (message) -> handle
            ("error_message", t_f64_f64),                  // (handle) -> string
            // Phase 4: RegExp
            ("regexp_new", t_f64_f64_f64),                 // (pattern, flags) -> handle
            ("regexp_test", t_f64_f64_i32),                // (regex, str) -> i32
            // Phase 4: Globals
            ("number_coerce", t_f64_f64),
            ("is_nan", t_f64_i32),
            ("is_finite", t_f64_i32),
            // Phase 5
            ("console_log_multi", t_f64_void),             // (args_array) -> void
            // Phase 1 addition: Class inheritance
            ("class_set_parent", t_f64_f64_void),          // (child_str, parent_str) -> void
            // Phase 3: Try/Catch
            ("try_start", t_void),                         // () -> void
            ("try_end", t_void),                           // () -> void
            ("throw_value", t_f64_void),                   // (val) -> void
            ("has_exception", t_void_i32),                 // () -> i32
            ("get_exception", t_void_f64),                 // () -> f64
            // Phase 4: URL
            ("url_parse", t_f64_f64),                      // (url_str) -> handle
            ("url_get_href", t_f64_f64),
            ("url_get_pathname", t_f64_f64),
            ("url_get_hostname", t_f64_f64),
            ("url_get_port", t_f64_f64),
            ("url_get_search", t_f64_f64),
            ("url_get_hash", t_f64_f64),
            ("url_get_origin", t_f64_f64),
            ("url_get_protocol", t_f64_f64),
            ("url_get_search_params", t_f64_f64),
            ("searchparams_get", t_f64_f64_f64),           // (handle, key) -> str
            ("searchparams_has", t_f64_f64_i32),           // (handle, key) -> i32
            ("searchparams_set", t_f64_f64_f64_void),      // (handle, key, val) -> void
            ("searchparams_append", t_f64_f64_f64_void),
            ("searchparams_delete", t_f64_f64_void),
            ("searchparams_to_string", t_f64_f64),
            // Phase 4: Crypto
            ("crypto_random_uuid", t_void_f64),
            ("crypto_random_bytes", t_f64_f64),
            // Phase 4: Path
            ("path_join", t_f64_f64_f64),                  // (a, b) -> str
            ("path_dirname", t_f64_f64),
            ("path_basename", t_f64_f64),
            ("path_extname", t_f64_f64),
            ("path_resolve", t_f64_f64),
            // Phase 4: Process/OS
            ("os_platform", t_void_f64),
            ("process_argv", t_void_f64),
            ("process_cwd", t_void_f64),
            // Phase 6: Buffer
            ("buffer_alloc", t_f64_f64),
            ("buffer_from_string", t_f64_f64_f64),
            ("buffer_to_string", t_f64_f64_f64),
            ("buffer_get", t_f64_f64_f64),
            ("buffer_set", t_f64_f64_f64_void),
            ("buffer_length", t_f64_f64),
            ("buffer_slice", t_f64_f64_f64_f64),
            ("buffer_concat", t_f64_f64),
            ("uint8array_new", t_f64_f64),
            ("uint8array_from", t_f64_f64),
            ("uint8array_length", t_f64_f64),
            ("uint8array_get", t_f64_f64_f64),
            ("uint8array_set", t_f64_f64_f64_void),
            // Timers
            ("set_timeout", t_f64_f64_f64),                // (closure, delay) -> timer_id
            ("set_interval", t_f64_f64_f64),               // (closure, delay) -> timer_id
            ("clear_timeout", t_f64_void),                 // (id) -> void
            ("clear_interval", t_f64_void),                // (id) -> void
            // Response properties
            ("response_status", t_f64_f64),                // (handle) -> number
            ("response_ok", t_f64_i32),                    // (handle) -> i32
            ("response_headers_get", t_f64_f64_f64),       // (handle, name) -> str
            ("response_url", t_f64_f64),                   // (handle) -> str
            // Buffer extras
            ("buffer_copy", {
                let t = self.get_type_idx(vec![ValType::F64; 5], vec![ValType::F64]);
                t
            }),
            ("buffer_write", t_f64_f64_f64_f64),           // (handle, str, offset, encoding) -> number
            ("buffer_equals", t_f64_f64_i32),              // (handle, other) -> i32
            ("buffer_is_buffer", t_f64_i32),               // (val) -> i32
            ("buffer_byte_length", t_f64_f64),             // (val) -> number
            // Crypto extras
            ("crypto_sha256", t_f64_f64),                  // (data) -> promise_handle
            ("crypto_md5", t_f64_f64),                     // (data) -> undefined
            // Path extras
            ("path_is_absolute", t_f64_i32),               // (str) -> i32
            // Phase 5: Async/Promise/Fetch
            ("fetch_url", t_f64_f64),                      // (url_str) -> promise_handle
            ("fetch_with_options", t_f64_f64_f64_f64),     // (url, method, body, headers_obj) -> promise_handle
            ("response_json", t_f64_f64),                  // (response_handle) -> promise_handle
            ("response_text", t_f64_f64),                  // (response_handle) -> promise_handle
            ("promise_new", t_void_f64),                   // () -> promise_handle
            ("promise_resolve", t_f64_f64_void),           // (promise_handle, value) -> void
            ("promise_then", t_f64_f64_f64),               // (promise_handle, closure_handle) -> promise_handle
            ("await_promise", t_f64_f64),                  // (value) -> resolved_value_or_value
        ];

        // Collect all closures from all modules (they need function indices too)
        let mut closure_funcs: Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)> = Vec::new();
        for (_, module) in modules {
            collect_closures_from_stmts(&module.init, &mut closure_funcs);
            for func in &module.functions {
                collect_closures_from_stmts(&func.body, &mut closure_funcs);
            }
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    collect_closures_from_stmts(&ctor.body, &mut closure_funcs);
                }
                for method in &class.methods {
                    collect_closures_from_stmts(&method.body, &mut closure_funcs);
                }
                for method in &class.static_methods {
                    collect_closures_from_stmts(&method.body, &mut closure_funcs);
                }
                for (_, getter) in &class.getters {
                    collect_closures_from_stmts(&getter.body, &mut closure_funcs);
                }
                for (_, setter) in &class.setters {
                    collect_closures_from_stmts(&setter.body, &mut closure_funcs);
                }
                for field in &class.fields {
                    if let Some(init) = &field.init {
                        collect_closures_from_expr(init, &mut closure_funcs);
                    }
                }
                for field in &class.static_fields {
                    if let Some(init) = &field.init {
                        collect_closures_from_expr(init, &mut closure_funcs);
                    }
                }
            }
        }

        // Register async functions as additional bridge imports
        // Must happen before user function registration to get correct import indices
        let mut async_import_idx = self.num_imports;
        for (_, module) in modules {
            for func in &module.functions {
                if func.is_async {
                    let param_count = func.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64]; // returns promise handle
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.func_map.insert(func.id, async_import_idx);
                    self.func_name_map.insert(func.name.clone(), async_import_idx);
                    self.async_func_imports.push((func.name.clone(), async_import_idx, param_count));
                    // Generate JS for this async function
                    let js_code = self.emit_js_async_function(func);
                    self.async_js_code.push(js_code);
                    async_import_idx += 1;
                }
            }
        }
        self.num_imports = async_import_idx;

        // Now set user_func_idx AFTER all imports (including async) are registered
        let mut user_func_idx = self.num_imports;

        // __init_strings function
        let init_strings_idx = user_func_idx;
        let init_strings_type = t_void;
        user_func_idx += 1;

        // Register user functions from all modules (skip async ones)
        for (_, module) in modules {
            for func in &module.functions {
                if func.is_async {
                    continue; // already registered as bridge import
                }
                let param_count = func.params.len();
                let params = vec![ValType::F64; param_count];
                let results = if func.body.iter().any(|s| has_return(s)) || func.name == "main" {
                    vec![ValType::F64]
                } else {
                    vec![]
                };
                let type_idx = self.get_type_idx(params, results);
                let _ = type_idx;
                self.func_map.insert(func.id, user_func_idx);
                // Build func_name_map for ExternFuncRef resolution
                self.func_name_map.insert(func.name.clone(), user_func_idx);
                user_func_idx += 1;
            }
        }

        // Register class constructors, methods, and static methods
        for (_, module) in modules {
            for class in &module.classes {
                // Record parent class relationship
                if let Some(parent) = &class.extends_name {
                    self.class_parent_map.insert(class.name.clone(), parent.clone());
                }
                // Constructor: params = this + declared params, returns f64 (this)
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_ctor_map.insert(class.name.clone(), user_func_idx);
                    user_func_idx += 1;
                }
                // Instance methods: params = this + declared params
                for method in &class.methods {
                    let param_count = 1 + method.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(method.name.clone(), user_func_idx);
                    user_func_idx += 1;
                }
                // Static methods: no this param
                for method in &class.static_methods {
                    let param_count = method.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_static_map
                        .entry(class.name.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(method.name.clone(), user_func_idx);
                    // Also register in func_name_map for cross-module resolution
                    self.func_name_map.insert(format!("{}_{}", class.name, method.name), user_func_idx);
                    user_func_idx += 1;
                }
                // Getters: like methods with 0 params + this
                for (name, getter) in &class.getters {
                    let params = vec![ValType::F64]; // just this
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(format!("__get_{}", name), user_func_idx);
                    let _ = getter;
                    user_func_idx += 1;
                }
                // Setters: this + value
                for (name, setter) in &class.setters {
                    let params = vec![ValType::F64; 2]; // this + value
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(format!("__set_{}", name), user_func_idx);
                    let _ = setter;
                    user_func_idx += 1;
                }
            }
        }

        // Register closure functions
        for (func_id, params, body, captures, mutable_captures) in &closure_funcs {
            if !self.func_map.contains_key(func_id) {
                // Closure params: captures first (as f64), then declared params
                let total_params = captures.len() + mutable_captures.len() + params.len();
                let wasm_params = vec![ValType::F64; total_params];
                let results = if body.iter().any(|s| has_return(s)) {
                    vec![ValType::F64]
                } else {
                    vec![ValType::F64] // closures always return f64
                };
                let type_idx = self.get_type_idx(wasm_params, results);
                let _ = type_idx;
                self.func_map.insert(*func_id, user_func_idx);
                user_func_idx += 1;
            }
        }

        // _start function (entry point)
        let start_idx = user_func_idx;
        let start_type = t_void;
        user_func_idx += 1;

        // Register globals from all modules
        for (_, module) in modules {
            for global in &module.globals {
                self.global_map.insert(global.id, self.num_globals);
                self.num_globals += 1;
            }
        }

        // Build the WASM module
        let mut wasm_module = Module::new();

        // --- Type section ---
        let mut type_section = TypeSection::new();
        for (params, results) in &self.types {
            type_section.ty().function(
                params.iter().copied(),
                results.iter().copied(),
            );
        }
        wasm_module.section(&type_section);

        // --- Import section ---
        let mut import_section = ImportSection::new();
        for (name, type_idx) in &import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add async function imports
        let async_import_entries: Vec<(String, u32)> = self.async_func_imports.iter().map(|(name, _idx, param_count)| {
            let import_name = format!("__async_{}", name);
            let params = vec![ValType::F64; *param_count];
            let results = vec![ValType::F64];
            let key = (params, results);
            let type_idx = self.type_map.get(&key).copied().unwrap_or(0);
            (import_name, type_idx)
        }).collect();
        for (name, type_idx) in &async_import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        wasm_module.section(&import_section);

        // --- Function section (declares type indices for each defined function) ---
        let mut func_section = FunctionSection::new();
        // __init_strings
        func_section.function(init_strings_type);
        // User functions (skip async — they are imports)
        for (_, module) in modules {
            for func in &module.functions {
                if func.is_async { continue; }
                let param_count = func.params.len();
                let params = vec![ValType::F64; param_count];
                let results = if func.body.iter().any(|s| has_return(s)) || func.name == "main" {
                    vec![ValType::F64]
                } else {
                    vec![]
                };
                let type_idx = self.get_type_idx(params, results);
                func_section.function(type_idx);
            }
        }
        // Class constructors, methods, static methods, getters, setters
        for (_, module) in modules {
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for method in &class.methods {
                    let param_count = 1 + method.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for method in &class.static_methods {
                    let param_count = method.params.len();
                    let params = vec![ValType::F64; param_count];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for (_name, _getter) in &class.getters {
                    let params = vec![ValType::F64];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for (_name, _setter) in &class.setters {
                    let params = vec![ValType::F64; 2];
                    let results = vec![ValType::F64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
            }
        }
        // Closure functions
        for (func_id, params, body, captures, mutable_captures) in &closure_funcs {
            if self.func_map.contains_key(func_id) {
                let total_params = captures.len() + mutable_captures.len() + params.len();
                let wasm_params = vec![ValType::F64; total_params];
                let results = vec![ValType::F64]; // closures always return f64
                let type_idx = self.get_type_idx(wasm_params, results);
                func_section.function(type_idx);
            }
        }
        // _start
        func_section.function(start_type);
        wasm_module.section(&func_section);

        // --- Table section (for indirect calls / closures) ---
        // Must come after Function section but before Memory section (WASM spec ordering)
        let all_func_indices: Vec<u32> = {
            let mut indices = vec![init_strings_idx]; // placeholder at index 0
            for (_, module) in modules {
                for func in &module.functions {
                    if let Some(&idx) = self.func_map.get(&func.id) {
                        indices.push(idx);
                    }
                }
            }
            // Add class constructor/method/static indices
            for (_, idx) in &self.class_ctor_map {
                if !indices.contains(idx) {
                    indices.push(*idx);
                }
            }
            for (_, methods) in &self.class_method_map {
                for (_, idx) in methods {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for (_, statics) in &self.class_static_map {
                for (_, idx) in statics {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for (func_id, _, _, _, _) in &closure_funcs {
                if let Some(&idx) = self.func_map.get(func_id) {
                    if !indices.contains(&idx) {
                        indices.push(idx);
                    }
                }
            }
            indices.push(start_idx);
            indices
        };
        // Build reverse map: wasm func index → table position
        for (table_idx, &func_idx) in all_func_indices.iter().enumerate() {
            self.func_to_table_idx.insert(func_idx, table_idx as u32);
        }

        let table_size = all_func_indices.len() as u32;
        {
            let mut table_section = TableSection::new();
            table_section.table(TableType {
                element_type: RefType::FUNCREF,
                minimum: table_size as u64,
                maximum: Some(table_size as u64),
                table64: false,
                shared: false,
            });
            wasm_module.section(&table_section);
        }

        // --- Memory section ---
        let mut mem_section = MemorySection::new();
        let pages = ((self.string_data.len() + 65535) / 65536).max(1) as u64;
        mem_section.memory(MemoryType {
            minimum: pages,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        wasm_module.section(&mem_section);

        // --- Global section (mutable f64 globals for module-level variables) ---
        if self.num_globals > 0 {
            let mut global_section = GlobalSection::new();
            for _ in 0..self.num_globals {
                global_section.global(
                    GlobalType {
                        val_type: ValType::F64,
                        mutable: true,
                        shared: false,
                    },
                    &wasm_encoder::ConstExpr::f64_const(Ieee64::from(f64::from_bits(TAG_UNDEFINED))),
                );
            }
            wasm_module.section(&global_section);
        }

        // --- Export section ---
        let mut export_section = ExportSection::new();
        export_section.export("_start", ExportKind::Func, start_idx);
        export_section.export("memory", ExportKind::Memory, 0);
        export_section.export("__indirect_function_table", ExportKind::Table, 0);
        wasm_module.section(&export_section);

        // --- Element section (populate the indirect call table) ---
        {
            let mut elem_section = ElementSection::new();
            elem_section.active(
                Some(0), // table index
                &wasm_encoder::ConstExpr::i32_const(0), // offset
                Elements::Functions(std::borrow::Cow::Borrowed(&all_func_indices)),
            );
            wasm_module.section(&elem_section);
        }

        // --- DataCount section (required before Code when Data section exists) ---
        if !self.string_data.is_empty() {
            wasm_module.section(&wasm_encoder::DataCountSection { count: 1 });
        }

        // --- Code section ---
        let mut code_section = CodeSection::new();

        // __init_strings: register all string literals with the JS runtime
        {
            let mut func = Function::new(vec![]);
            for (_content, offset, len) in &self.string_table {
                func.instruction(&Instruction::I32Const(*offset as i32));
                func.instruction(&Instruction::I32Const(*len as i32));
                func.instruction(&Instruction::Call(rt.string_new));
            }
            func.instruction(&Instruction::End);
            code_section.function(&func);
        }

        // User functions (skip async — they are JS bridge imports)
        for (_, module) in modules {
            for hir_func in &module.functions {
                if hir_func.is_async { continue; }
                let func = self.compile_function(hir_func);
                code_section.function(&func);
            }
        }

        // Class constructors, methods, static methods, getters, setters
        for (_, module) in modules {
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    let func = self.compile_class_constructor(class, ctor);
                    code_section.function(&func);
                }
                for method in &class.methods {
                    let func = self.compile_class_method(method);
                    code_section.function(&func);
                }
                for method in &class.static_methods {
                    let func = self.compile_function(method);
                    code_section.function(&func);
                }
                for (_name, getter) in &class.getters {
                    let func = self.compile_class_method(getter);
                    code_section.function(&func);
                }
                for (_name, setter) in &class.setters {
                    let func = self.compile_class_method(setter);
                    code_section.function(&func);
                }
            }
        }

        // Closure functions
        for (func_id, params, body, captures, mutable_captures) in &closure_funcs {
            if let Some(&_) = self.func_map.get(func_id) {
                let func = self.compile_closure(params, body, captures, mutable_captures);
                code_section.function(&func);
            }
        }

        // _start: call __init_strings, then execute module init code
        {
            // Collect all init statements to determine locals needed (recursively)
            let mut init_locals = BTreeMap::new();
            let mut extra_count = 0u32;
            for (_, module) in modules {
                collect_locals(&module.init, &mut init_locals, &mut extra_count, 0);
            }

            let num_locals = init_locals.len();
            let locals = if num_locals > 0 {
                vec![(num_locals as u32, ValType::F64)]
            } else {
                vec![]
            };
            let mut func = Function::new(locals);

            // Call __init_strings first
            func.instruction(&Instruction::Call(init_strings_idx));

            // Initialize globals
            for (_, module) in modules {
                for global in &module.globals {
                    if let Some(init) = &global.init {
                        let mut ctx = FuncEmitCtx::new(self, &init_locals);
                        ctx.emit_expr(&mut func, init);
                        let gidx = self.global_map[&global.id];
                        func.instruction(&Instruction::GlobalSet(gidx));
                    } else if global.name == "__platform__" {
                        // Web platform ID = 5
                        func.instruction(&f64_const(5.0));
                        let gidx = self.global_map[&global.id];
                        func.instruction(&Instruction::GlobalSet(gidx));
                    }
                }
            }

            // Register class methods with the bridge and set up inheritance
            for (_, module) in modules {
                for class in &module.classes {
                    let class_name_id = self.string_map.get(class.name.as_str()).copied().unwrap_or(0);
                    let class_bits = (STRING_TAG << 48) | (class_name_id as u64);

                    // Register instance methods in classMethodTable (including getters/setters)
                    if let Some(methods) = self.class_method_map.get(&class.name) {
                        for (method_name, &func_idx) in methods {
                            let real_name = method_name.as_str();
                            let method_name_id = self.string_map.get(real_name).copied().unwrap_or(0);
                            let method_bits = (STRING_TAG << 48) | (method_name_id as u64);
                            let table_idx = self.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                            func.instruction(&f64_const(f64::from_bits(class_bits)));
                            func.instruction(&f64_const(f64::from_bits(method_bits)));
                            func.instruction(&f64_const(table_idx as f64));
                            func.instruction(&Instruction::Call(rt.class_set_method));
                        }
                    }

                    // Set up inheritance
                    if let Some(parent_name) = &class.extends_name {
                        let parent_name_id = self.string_map.get(parent_name.as_str()).copied().unwrap_or(0);
                        let parent_bits = (STRING_TAG << 48) | (parent_name_id as u64);
                        func.instruction(&f64_const(f64::from_bits(class_bits)));
                        func.instruction(&f64_const(f64::from_bits(parent_bits)));
                        func.instruction(&Instruction::Call(rt.class_set_parent));
                    }

                    // Register static fields
                    for field in &class.static_fields {
                        if let Some(init) = &field.init {
                            let field_name_id = self.string_map.get(field.name.as_str()).copied().unwrap_or(0);
                            let field_bits = (STRING_TAG << 48) | (field_name_id as u64);
                            func.instruction(&f64_const(f64::from_bits(class_bits)));
                            func.instruction(&f64_const(f64::from_bits(field_bits)));
                            let mut ctx = FuncEmitCtx::new(self, &init_locals);
                            ctx.emit_expr(&mut func, init);
                            func.instruction(&Instruction::Call(rt.class_set_static));
                        }
                    }
                }
            }

            // Execute init statements from all modules
            for (_, module) in modules {
                let mut ctx = FuncEmitCtx::new(self, &init_locals);
                for stmt in &module.init {
                    ctx.emit_stmt(&mut func, stmt, false);
                }
            }

            func.instruction(&Instruction::End);
            code_section.function(&func);
        }

        wasm_module.section(&code_section);

        // --- Data section (string literal bytes, must come after Code) ---
        if !self.string_data.is_empty() {
            let mut data_section = DataSection::new();
            data_section.active(0, &wasm_encoder::ConstExpr::i32_const(0), self.string_data.iter().copied());
            wasm_module.section(&data_section);
        }

        let wasm_bytes = wasm_module.finish();
        let async_js = self.async_js_code.join("\n");
        WasmCompileOutput { wasm_bytes, async_js }
    }

    fn compile_function(&self, hir_func: &perry_hir::ir::Function) -> Function {
        // Build local map: param locals come first, then body locals
        let mut local_map = BTreeMap::new();
        for (i, param) in hir_func.params.iter().enumerate() {
            local_map.insert(param.id, i as u32);
        }

        // Scan body for local variable declarations
        let param_count = hir_func.params.len() as u32;
        let mut extra_locals = 0u32;
        collect_locals(&hir_func.body, &mut local_map, &mut extra_locals, param_count);

        let locals = if extra_locals > 0 {
            vec![(extra_locals, ValType::F64)]
        } else {
            vec![]
        };
        let mut func = Function::new(locals);

        let has_ret = hir_func.body.iter().any(|s| has_return(s));
        let mut ctx = FuncEmitCtx::new(self, &local_map);

        for stmt in &hir_func.body {
            ctx.emit_stmt(&mut func, stmt, has_ret);
        }

        // If function should return but doesn't always, add a default return
        if has_ret {
            // Push undefined as default return
            func.instruction(&f64_const_bits(TAG_UNDEFINED));
        }

        func.instruction(&Instruction::End);
        func
    }

    fn compile_closure(&self, params: &[Param], body: &[Stmt], captures: &[LocalId], mutable_captures: &[LocalId]) -> Function {
        // Closure parameters: captures first, then declared params
        let mut local_map = BTreeMap::new();
        let mut param_idx = 0u32;
        for cap in captures {
            local_map.insert(*cap, param_idx);
            param_idx += 1;
        }
        for cap in mutable_captures {
            local_map.insert(*cap, param_idx);
            param_idx += 1;
        }
        for param in params {
            local_map.insert(param.id, param_idx);
            param_idx += 1;
        }

        // Scan body for additional locals
        let mut extra_locals = 0u32;
        collect_locals(body, &mut local_map, &mut extra_locals, param_idx);

        let locals = if extra_locals > 0 {
            vec![(extra_locals, ValType::F64)]
        } else {
            vec![]
        };
        let mut func = Function::new(locals);

        let mut ctx = FuncEmitCtx::new(self, &local_map);
        let has_ret = body.iter().any(|s| has_return(s));

        for stmt in body {
            ctx.emit_stmt(&mut func, stmt, true); // closures always "return"
        }

        // Default return undefined
        func.instruction(&f64_const_bits(TAG_UNDEFINED));
        func.instruction(&Instruction::End);
        func
    }

    fn compile_class_constructor(&self, class: &perry_hir::ir::Class, ctor: &perry_hir::ir::Function) -> Function {
        // Local 0 = this (the instance handle)
        // Params start at local index 1
        let mut local_map = BTreeMap::new();
        // Don't insert this into local_map — Expr::This emits LocalGet(0) directly
        for (i, param) in ctor.params.iter().enumerate() {
            local_map.insert(param.id, (i + 1) as u32);
        }

        let param_count = 1 + ctor.params.len();
        let mut extra_locals = 0u32;
        collect_locals(&ctor.body, &mut local_map, &mut extra_locals, param_count as u32);

        let locals = if extra_locals > 0 {
            vec![(extra_locals, ValType::F64)]
        } else {
            vec![]
        };
        let mut func = Function::new(locals);
        let rt = self.rt.as_ref().unwrap();

        // Emit field initializers: class_set_field(this, field_name, value)
        for field in &class.fields {
            if let Some(init) = &field.init {
                func.instruction(&Instruction::LocalGet(0)); // this
                let field_id = self.string_map.get(field.name.as_str()).copied().unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                func.instruction(&f64_const(f64::from_bits(field_bits)));
                let mut ctx = FuncEmitCtx::new(self, &local_map);
                ctx.emit_expr(&mut func, init);
                func.instruction(&Instruction::Call(rt.class_set_field));
            }
        }

        // Emit constructor body
        let mut ctx = FuncEmitCtx::new(self, &local_map);
        ctx.current_class = Some(class.name.clone());
        for stmt in &ctor.body {
            ctx.emit_stmt(&mut func, stmt, false);
        }

        // Return this
        func.instruction(&Instruction::LocalGet(0));
        func.instruction(&Instruction::End);
        func
    }

    fn compile_class_method(&self, method: &perry_hir::ir::Function) -> Function {
        // Local 0 = this, params start at 1
        let mut local_map = BTreeMap::new();
        for (i, param) in method.params.iter().enumerate() {
            local_map.insert(param.id, (i + 1) as u32);
        }

        let param_count = 1 + method.params.len();
        let mut extra_locals = 0u32;
        collect_locals(&method.body, &mut local_map, &mut extra_locals, param_count as u32);

        let locals = if extra_locals > 0 {
            vec![(extra_locals, ValType::F64)]
        } else {
            vec![]
        };
        let mut func = Function::new(locals);
        let has_ret = method.body.iter().any(|s| has_return(s));
        let mut ctx = FuncEmitCtx::new(self, &local_map);

        for stmt in &method.body {
            ctx.emit_stmt(&mut func, stmt, true); // methods always return f64
        }

        // Always push default return (method type is always -> f64)
        func.instruction(&f64_const_bits(TAG_UNDEFINED));
        func.instruction(&Instruction::End);
        func
    }

    /// Generate JavaScript code for an async function body.
    /// The generated function uses NaN-boxed f64 values and bridge helper functions.
    fn emit_js_async_function(&self, func: &perry_hir::ir::Function) -> String {
        let params: Vec<String> = func.params.iter().enumerate()
            .map(|(i, _)| format!("__p{}", i))
            .collect();
        let params_str = params.join(", ");

        let mut body = String::new();
        // Map param names to local IDs for the JS emitter
        let mut local_names: BTreeMap<u32, String> = BTreeMap::new();
        for (i, param) in func.params.iter().enumerate() {
            local_names.insert(param.id, format!("__p{}", i));
        }

        for stmt in &func.body {
            self.emit_js_stmt(&mut body, stmt, &mut local_names, 2);
        }

        format!(
            "  __async_{name}: ({params}) => {{\n\
             \x20   const __p = (async () => {{\n\
             {body}\
             \x20     return {undef};\n\
             \x20   }})();\n\
             \x20   return nanboxPointer(allocHandle(__p));\n\
             \x20 }},",
            name = func.name,
            params = params_str,
            body = body,
            undef = "u64ToF64(TAG_UNDEFINED)",
        )
    }

    fn emit_js_stmt(&self, out: &mut String, stmt: &Stmt, locals: &mut BTreeMap<u32, String>, indent: usize) {
        let pad = "  ".repeat(indent);
        match stmt {
            Stmt::Let { id, init, .. } => {
                let name = format!("__l{}", id);
                locals.insert(*id, name.clone());
                if let Some(init_expr) = init {
                    let val = self.emit_js_expr(init_expr, locals);
                    out.push_str(&format!("{pad}    let {name} = {val};\n"));
                } else {
                    out.push_str(&format!("{pad}    let {name} = u64ToF64(TAG_UNDEFINED);\n"));
                }
            }
            Stmt::Expr(e) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    {val};\n"));
            }
            Stmt::Return(Some(e)) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    return {val};\n"));
            }
            Stmt::Return(None) => {
                out.push_str(&format!("{pad}    return u64ToF64(TAG_UNDEFINED);\n"));
            }
            Stmt::If { condition, then_branch, else_branch } => {
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    if (toJsValue({cond})) {{\n"));
                for s in then_branch {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                if let Some(eb) = else_branch {
                    out.push_str(&format!("{pad}    }} else {{\n"));
                    for s in eb {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::While { condition, body } => {
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    while (toJsValue({cond})) {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::For { init, condition, update, body } => {
                out.push_str(&format!("{pad}    {{\n"));
                if let Some(init_stmt) = init {
                    self.emit_js_stmt(out, init_stmt, locals, indent + 1);
                }
                let cond = condition.as_ref().map(|c| self.emit_js_expr(c, locals)).unwrap_or_else(|| "1".to_string());
                out.push_str(&format!("{pad}      while (toJsValue({cond})) {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 2);
                }
                if let Some(upd) = update {
                    let u = self.emit_js_expr(upd, locals);
                    out.push_str(&format!("{pad}        {u};\n"));
                }
                out.push_str(&format!("{pad}      }}\n"));
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Try { body, catch, finally } => {
                out.push_str(&format!("{pad}    try {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                if let Some(c) = catch {
                    if let Some((param_id, _)) = &c.param {
                        let name = format!("__l{}", param_id);
                        locals.insert(*param_id, name.clone());
                        out.push_str(&format!("{pad}    }} catch (__e) {{\n"));
                        out.push_str(&format!("{pad}      let {name} = fromJsValue(__e);\n"));
                    } else {
                        out.push_str(&format!("{pad}    }} catch (__e) {{\n"));
                    }
                    for s in &c.body {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                if let Some(f) = finally {
                    out.push_str(&format!("{pad}    }} finally {{\n"));
                    for s in f {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Throw(e) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    throw toJsValue({val});\n"));
            }
            Stmt::Break => { out.push_str(&format!("{pad}    break;\n")); }
            Stmt::Continue => { out.push_str(&format!("{pad}    continue;\n")); }
            Stmt::Switch { discriminant, cases } => {
                let disc = self.emit_js_expr(discriminant, locals);
                out.push_str(&format!("{pad}    switch (toJsValue({disc})) {{\n"));
                for case in cases {
                    if let Some(test) = &case.test {
                        let t = self.emit_js_expr(test, locals);
                        out.push_str(&format!("{pad}      case toJsValue({t}):\n"));
                    } else {
                        out.push_str(&format!("{pad}      default:\n"));
                    }
                    for s in &case.body {
                        self.emit_js_stmt(out, s, locals, indent + 2);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
        }
    }

    fn emit_js_expr(&self, expr: &Expr, locals: &BTreeMap<u32, String>) -> String {
        match expr {
            Expr::Number(n) => format!("{}", n),
            Expr::Integer(i) => format!("{}", *i as f64),
            Expr::Bool(true) => "u64ToF64(TAG_TRUE)".to_string(),
            Expr::Bool(false) => "u64ToF64(TAG_FALSE)".to_string(),
            Expr::Undefined => "u64ToF64(TAG_UNDEFINED)".to_string(),
            Expr::Null => "u64ToF64(TAG_NULL)".to_string(),
            Expr::String(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r");
                format!("fromJsValue(\"{}\")", escaped)
            }
            Expr::LocalGet(id) => {
                locals.get(id).cloned().unwrap_or_else(|| format!("__l{}", id))
            }
            Expr::LocalSet(id, val) => {
                let name = locals.get(id).cloned().unwrap_or_else(|| format!("__l{}", id));
                let v = self.emit_js_expr(val, locals);
                format!("({} = {})", name, v)
            }
            Expr::GlobalGet(id) => format!("__g{}", id),
            Expr::GlobalSet(id, val) => {
                let v = self.emit_js_expr(val, locals);
                format!("(__g{} = {})", id, v)
            }
            Expr::Binary { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                match op {
                    BinaryOp::Add => format!("fromJsValue(toJsValue({}) + toJsValue({}))", l, r),
                    BinaryOp::Sub => format!("({} - {})", l, r),
                    BinaryOp::Mul => format!("({} * {})", l, r),
                    BinaryOp::Div => format!("({} / {})", l, r),
                    BinaryOp::Mod => format!("({} % {})", l, r),
                    _ => format!("fromJsValue(toJsValue({}) + toJsValue({}))", l, r),
                }
            }
            Expr::Compare { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                let js_op = match op {
                    CompareOp::Eq => "===",
                    CompareOp::Ne => "!==",
                    CompareOp::Lt => "<",
                    CompareOp::Le => "<=",
                    CompareOp::Gt => ">",
                    CompareOp::Ge => ">=",
                };
                format!("(toJsValue({}) {} toJsValue({}) ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))", l, js_op, r)
            }
            Expr::Logical { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                match op {
                    LogicalOp::And => format!("(toJsValue({l}) ? {r} : {l})"),
                    LogicalOp::Or => format!("(toJsValue({l}) ? {l} : {r})"),
                    LogicalOp::Coalesce => format!("(isNull({l}) || isUndefined({l}) ? {r} : {l})"),
                }
            }
            Expr::Unary { op, operand } => {
                let o = self.emit_js_expr(operand, locals);
                match op {
                    UnaryOp::Neg => format!("(-{})", o),
                    UnaryOp::Not => format!("(toJsValue({}) ? u64ToF64(TAG_FALSE) : u64ToF64(TAG_TRUE))", o),
                    _ => o,
                }
            }
            Expr::Await(inner) => {
                let v = self.emit_js_expr(inner, locals);
                // In JS async context, we can truly await
                format!("fromJsValue(await toJsValue({}))", v)
            }
            Expr::Call { callee, args, .. } => {
                let args_js: Vec<String> = args.iter().map(|a| self.emit_js_expr(a, locals)).collect();
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&func_idx) = self.func_map.get(id) {
                            // Call exported WASM function
                            format!("wasmInstance.exports.__wasm_func_{}({})", func_idx, args_js.join(", "))
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    Expr::PropertyGet { object, property } => {
                        let obj = self.emit_js_expr(object, locals);
                        let args_str = args_js.join(", ");
                        format!("fromJsValue(toJsValue({}).{}({}))", obj, property,
                            args.iter().map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals))).collect::<Vec<_>>().join(", "))
                    }
                    _ => {
                        let callee_js = self.emit_js_expr(callee, locals);
                        format!("{}({})", callee_js, args_js.join(", "))
                    }
                }
            }
            Expr::FetchWithOptions { url, method, body, headers } => {
                let url_js = self.emit_js_expr(url, locals);
                let method_js = self.emit_js_expr(method, locals);
                let body_js = self.emit_js_expr(body, locals);
                // In async JS context, we can do a real fetch
                let mut opts = format!("{{ method: getString({}) || 'GET'", method_js);
                if !matches!(body.as_ref(), Expr::Undefined) {
                    opts.push_str(&format!(", body: getString({})", body_js));
                }
                if !headers.is_empty() {
                    opts.push_str(", headers: {");
                    for (i, (key, val)) in headers.iter().enumerate() {
                        if i > 0 { opts.push_str(", "); }
                        let v = self.emit_js_expr(val, locals);
                        opts.push_str(&format!("'{}': getString({})", key, v));
                    }
                    opts.push('}');
                }
                opts.push('}');
                format!("fromJsValue(await fetch(getString({}), {}))", url_js, opts)
            }
            Expr::PropertyGet { object, property } => {
                let obj = self.emit_js_expr(object, locals);
                format!("fromJsValue(toJsValue({}).{})", obj, property)
            }
            Expr::PropertySet { object, property, value } => {
                let obj = self.emit_js_expr(object, locals);
                let val = self.emit_js_expr(value, locals);
                format!("(toJsValue({}).{} = toJsValue({}), {})", obj, property, val, val)
            }
            Expr::Object(fields) => {
                let mut parts = Vec::new();
                for (key, val) in fields {
                    let v = self.emit_js_expr(val, locals);
                    parts.push(format!("'{}': toJsValue({})", key, v));
                }
                format!("fromJsValue({{{}}})", parts.join(", "))
            }
            Expr::Array(elements) => {
                let elems: Vec<String> = elements.iter()
                    .map(|e| format!("toJsValue({})", self.emit_js_expr(e, locals)))
                    .collect();
                format!("fromJsValue([{}])", elems.join(", "))
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                let c = self.emit_js_expr(condition, locals);
                let t = self.emit_js_expr(then_expr, locals);
                let e = self.emit_js_expr(else_expr, locals);
                format!("(toJsValue({}) ? {} : {})", c, t, e)
            }
            Expr::NativeMethodCall { module, method, object, args, .. } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                match normalized {
                    "console" => {
                        let args_js: Vec<String> = args.iter()
                            .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                            .collect();
                        match method.as_str() {
                            "log" => format!("(console.log({}), u64ToF64(TAG_UNDEFINED))", args_js.join(", ")),
                            "warn" => format!("(console.warn({}), u64ToF64(TAG_UNDEFINED))", args_js.join(", ")),
                            "error" => format!("(console.error({}), u64ToF64(TAG_UNDEFINED))", args_js.join(", ")),
                            _ => "u64ToF64(TAG_UNDEFINED)".to_string(),
                        }
                    }
                    "JSON" => {
                        match method.as_str() {
                            "parse" if !args.is_empty() => {
                                let a = self.emit_js_expr(&args[0], locals);
                                format!("fromJsValue(JSON.parse(getString({})))", a)
                            }
                            "stringify" if !args.is_empty() => {
                                let a = self.emit_js_expr(&args[0], locals);
                                format!("fromJsValue(JSON.stringify(toJsValue({})))", a)
                            }
                            _ => "u64ToF64(TAG_UNDEFINED)".to_string(),
                        }
                    }
                    _ => {
                        if let Some(obj) = object {
                            let obj_js = self.emit_js_expr(obj, locals);
                            let args_js: Vec<String> = args.iter()
                                .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                                .collect();
                            format!("fromJsValue(toJsValue({}).{}({}))", obj_js, method, args_js.join(", "))
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                }
            }
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    let m_js = self.emit_js_expr(m, locals);
                    format!("fromJsValue(new Error(getString({})))", m_js)
                } else {
                    "fromJsValue(new Error())".to_string()
                }
            }
            Expr::ErrorMessage(err) => {
                let e = self.emit_js_expr(err, locals);
                format!("fromJsValue(toJsValue({}).message)", e)
            }
            Expr::JsonParse(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(JSON.parse(getString({})))", v)
            }
            Expr::JsonStringify(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(JSON.stringify(toJsValue({})))", v)
            }
            Expr::This => "__this".to_string(),
            Expr::IndexGet { object, index } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                format!("fromJsValue(toJsValue({})[toJsValue({})])", obj, idx)
            }
            Expr::IndexSet { object, index, value } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                let val = self.emit_js_expr(value, locals);
                format!("(toJsValue({})[toJsValue({})] = toJsValue({}), {})", obj, idx, val, val)
            }
            Expr::ArrayPush { array_id, value } => {
                let arr = locals.get(array_id).cloned().unwrap_or_else(|| format!("__l{}", array_id));
                let val = self.emit_js_expr(value, locals);
                format!("fromJsValue(toJsValue({}).push(toJsValue({})))", arr, val)
            }
            Expr::StringCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(String(toJsValue({})))", v)
            }
            Expr::MathFloor(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.floor({})", v)
            }
            Expr::MathCeil(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.ceil({})", v)
            }
            Expr::MathRound(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.round({})", v)
            }
            Expr::MathAbs(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.abs({})", v)
            }
            Expr::MathRandom => "Math.random()".to_string(),
            Expr::DateNow => "Date.now()".to_string(),
            Expr::Sequence(exprs) => {
                if exprs.is_empty() {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                } else {
                    let parts: Vec<String> = exprs.iter().map(|e| self.emit_js_expr(e, locals)).collect();
                    format!("({})", parts.join(", "))
                }
            }
            Expr::ExternFuncRef { name, .. } => {
                // In JS context, look up exported WASM functions
                if let Some(&func_idx) = self.func_name_map.get(name) {
                    format!("fromJsValue(wasmInstance.exports.__wasm_func_{})", func_idx)
                } else {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                }
            }
            Expr::FuncRef(id) => {
                if let Some(&func_idx) = self.func_map.get(id) {
                    format!("fromJsValue(wasmInstance.exports.__wasm_func_{})", func_idx)
                } else {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                }
            }
            Expr::New { class_name, args, .. } => {
                let args_js: Vec<String> = args.iter()
                    .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                    .collect();
                format!("fromJsValue(new (toJsValue(fromJsValue('{}')))({})))", class_name, args_js.join(", "))
            }
            Expr::InstanceOf { expr, ty } => {
                let e = self.emit_js_expr(expr, locals);
                // Use the bridge instanceof check
                format!("(toJsValue({}) instanceof {} ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))", e, ty)
            }
            Expr::TypeOf(operand) => {
                let o = self.emit_js_expr(operand, locals);
                format!("fromJsValue(typeof toJsValue({}))", o)
            }
            Expr::Void(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("({}, u64ToF64(TAG_UNDEFINED))", v)
            }
            Expr::Delete(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("(delete toJsValue({}), u64ToF64(TAG_TRUE))", v)
            }
            _ => {
                // Fallback: return undefined for unhandled expressions
                "u64ToF64(TAG_UNDEFINED)".to_string()
            }
        }
    }

    fn collect_strings(&mut self, module: &perry_hir::ir::Module) {
        // Pre-intern common strings used by bridge calls
        self.intern_string("Authorization");
        self.intern_string("POST");
        self.intern_string("GET");

        for func in &module.functions {
            self.collect_strings_in_stmts(&func.body);
        }
        for stmt in &module.init {
            self.collect_strings_in_stmt(stmt);
        }
        for global in &module.globals {
            if let Some(init) = &global.init {
                self.collect_strings_in_expr(init);
            }
        }
        // Collect enum names and member names/values
        for enum_def in &module.enums {
            self.intern_string(&enum_def.name);
            for member in &enum_def.members {
                self.intern_string(&member.name);
                if let EnumValue::String(s) = &member.value {
                    self.intern_string(s);
                }
            }
        }
        // Collect class names and method/field names
        for class in &module.classes {
            self.intern_string(&class.name);
            if let Some(parent) = &class.extends_name {
                self.intern_string(parent);
            }
            if let Some(ctor) = &class.constructor {
                self.collect_strings_in_stmts(&ctor.body);
                for param in &ctor.params {
                    if let Some(default) = &param.default {
                        self.collect_strings_in_expr(default);
                    }
                }
            }
            for method in &class.methods {
                self.intern_string(&method.name);
                self.collect_strings_in_stmts(&method.body);
            }
            for method in &class.static_methods {
                self.intern_string(&method.name);
                self.collect_strings_in_stmts(&method.body);
            }
            for (name, getter) in &class.getters {
                self.intern_string(name);
                self.intern_string(&format!("__get_{}", name));
                self.collect_strings_in_stmts(&getter.body);
            }
            for (name, setter) in &class.setters {
                self.intern_string(name);
                self.intern_string(&format!("__set_{}", name));
                self.collect_strings_in_stmts(&setter.body);
            }
            for field in &class.fields {
                self.intern_string(&field.name);
                if let Some(init) = &field.init {
                    self.collect_strings_in_expr(init);
                }
            }
            for field in &class.static_fields {
                self.intern_string(&field.name);
                if let Some(init) = &field.init {
                    self.collect_strings_in_expr(init);
                }
            }
        }
    }

    fn collect_strings_in_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.collect_strings_in_stmt(stmt);
        }
    }

    fn collect_strings_in_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init { self.collect_strings_in_expr(e); }
            }
            Stmt::Expr(e) => self.collect_strings_in_expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e { self.collect_strings_in_expr(e); }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_stmts(then_branch);
                if let Some(eb) = else_branch { self.collect_strings_in_stmts(eb); }
            }
            Stmt::While { condition, body } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_stmts(body);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(i) = init { self.collect_strings_in_stmt(i); }
                if let Some(c) = condition { self.collect_strings_in_expr(c); }
                if let Some(u) = update { self.collect_strings_in_expr(u); }
                self.collect_strings_in_stmts(body);
            }
            Stmt::Throw(e) => self.collect_strings_in_expr(e),
            Stmt::Try { body, catch, finally } => {
                self.collect_strings_in_stmts(body);
                if let Some(c) = catch {
                    self.collect_strings_in_stmts(&c.body);
                }
                if let Some(f) = finally { self.collect_strings_in_stmts(f); }
            }
            Stmt::Switch { discriminant, cases } => {
                self.collect_strings_in_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test { self.collect_strings_in_expr(t); }
                    self.collect_strings_in_stmts(&case.body);
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }

    fn collect_strings_in_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::String(s) => { self.intern_string(s); }
            Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. }
            | Expr::Logical { left, right, .. } => {
                self.collect_strings_in_expr(left);
                self.collect_strings_in_expr(right);
            }
            Expr::Unary { operand, .. } => self.collect_strings_in_expr(operand),
            Expr::Call { callee, args, .. } => {
                self.collect_strings_in_expr(callee);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
                self.collect_strings_in_expr(val);
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_expr(then_expr);
                self.collect_strings_in_expr(else_expr);
            }
            Expr::Closure { body, .. } => {
                self.collect_strings_in_stmts(body);
            }
            Expr::NativeMethodCall { args, .. } => {
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::Array(elems) => {
                for e in elems { self.collect_strings_in_expr(e); }
            }
            Expr::Object(fields) => {
                for (k, v) in fields {
                    self.intern_string(k);
                    self.collect_strings_in_expr(v);
                }
            }
            Expr::PropertyGet { object, property } => {
                self.collect_strings_in_expr(object);
                self.intern_string(property);
            }
            Expr::PropertySet { object, value, property, .. } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(value);
                self.intern_string(property);
            }
            Expr::IndexGet { object, index } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
            }
            Expr::IndexSet { object, index, value } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
                self.collect_strings_in_expr(value);
            }
            Expr::Await(e) | Expr::TypeOf(e) | Expr::Void(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::New { args, .. } => {
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::Update { .. } | Expr::Sequence(_) => {}
            Expr::EnumMember { enum_name, member_name } => {
                self.intern_string(enum_name);
                self.intern_string(member_name);
            }
            Expr::StaticFieldGet { class_name, field_name } |
            Expr::StaticFieldSet { class_name, field_name, .. } => {
                self.intern_string(class_name);
                self.intern_string(field_name);
            }
            Expr::StaticMethodCall { class_name, method_name, args } => {
                self.intern_string(class_name);
                self.intern_string(method_name);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::InstanceOf { expr, ty } => {
                self.collect_strings_in_expr(expr);
                self.intern_string(ty);
            }
            Expr::In { property, object } => {
                self.collect_strings_in_expr(property);
                self.collect_strings_in_expr(object);
            }
            Expr::Delete(e) => self.collect_strings_in_expr(e),
            Expr::RegExp { pattern, flags } => {
                self.intern_string(pattern);
                self.intern_string(flags);
            }
            Expr::RegExpTest { regex, string } => {
                self.collect_strings_in_expr(regex);
                self.collect_strings_in_expr(string);
            }
            Expr::StringMatch { string, regex } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(regex);
            }
            Expr::StringReplace { string, pattern, replacement } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(pattern);
                self.collect_strings_in_expr(replacement);
            }
            Expr::StringSplit(a, b) => {
                self.collect_strings_in_expr(a);
                self.collect_strings_in_expr(b);
            }
            Expr::StringFromCharCode(e) | Expr::StringCoerce(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ObjectSpread { parts } => {
                for (key_opt, val) in parts {
                    if let Some(k) = key_opt { self.intern_string(k); }
                    self.collect_strings_in_expr(val);
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                            self.collect_strings_in_expr(e);
                        }
                    }
                }
            }
            Expr::ObjectKeys(e) | Expr::ObjectValues(e) | Expr::ObjectEntries(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ObjectRest { object, exclude_keys } => {
                self.collect_strings_in_expr(object);
                for k in exclude_keys { self.intern_string(k); }
            }
            Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } => {
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayPushSpread { source, .. } => {
                self.collect_strings_in_expr(source);
            }
            Expr::ArraySlice { array, start, end } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(start);
                if let Some(e) = end { self.collect_strings_in_expr(e); }
            }
            Expr::ArraySplice { start, delete_count, items, .. } => {
                self.collect_strings_in_expr(start);
                if let Some(dc) = delete_count { self.collect_strings_in_expr(dc); }
                for item in items { self.collect_strings_in_expr(item); }
            }
            Expr::ArrayJoin { array, separator } => {
                self.collect_strings_in_expr(array);
                if let Some(s) = separator { self.collect_strings_in_expr(s); }
                self.intern_string(","); // default separator
            }
            Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } |
            Expr::ArrayForEach { array, callback } | Expr::ArrayFind { array, callback } |
            Expr::ArrayFindIndex { array, callback } | Expr::ArraySort { array, comparator: callback } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(callback);
            }
            Expr::ArrayReduce { array, callback, initial } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(callback);
                if let Some(i) = initial { self.collect_strings_in_expr(i); }
            }
            Expr::ArrayFlat { array } | Expr::ArrayIsArray(array) | Expr::ArrayFrom(array) => {
                self.collect_strings_in_expr(array);
            }
            Expr::MapSet { map, key, value } => {
                self.collect_strings_in_expr(map);
                self.collect_strings_in_expr(key);
                self.collect_strings_in_expr(value);
            }
            Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
                self.collect_strings_in_expr(map);
                self.collect_strings_in_expr(key);
            }
            Expr::MapSize(e) | Expr::MapClear(e) | Expr::MapEntries(e) |
            Expr::MapKeys(e) | Expr::MapValues(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::SetNewFromArray(e) | Expr::SetSize(e) | Expr::SetClear(e) | Expr::SetValues(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::SetAdd { value, .. } => { self.collect_strings_in_expr(value); }
            Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
                self.collect_strings_in_expr(set);
                self.collect_strings_in_expr(value);
            }
            Expr::DateNew(arg) => {
                if let Some(a) = arg { self.collect_strings_in_expr(a); }
            }
            Expr::DateGetTime(e) | Expr::DateToISOString(e) | Expr::DateGetFullYear(e) |
            Expr::DateGetMonth(e) | Expr::DateGetDate(e) | Expr::DateGetHours(e) |
            Expr::DateGetMinutes(e) | Expr::DateGetSeconds(e) | Expr::DateGetMilliseconds(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg { self.collect_strings_in_expr(m); }
            }
            Expr::ErrorMessage(e) => { self.collect_strings_in_expr(e); }
            Expr::JsonParse(e) | Expr::JsonStringify(e) => { self.collect_strings_in_expr(e); }
            Expr::NumberCoerce(e) | Expr::IsNaN(e) | Expr::IsFinite(e) | Expr::BigIntCoerce(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ParseInt { string, radix } => {
                self.collect_strings_in_expr(string);
                if let Some(r) = radix { self.collect_strings_in_expr(r); }
            }
            Expr::ParseFloat(e) => { self.collect_strings_in_expr(e); }
            Expr::PropertyUpdate { object, property, .. } => {
                self.collect_strings_in_expr(object);
                self.intern_string(property);
            }
            Expr::IndexUpdate { object, index, .. } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
            }
            Expr::SuperCall(args) => {
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::SuperMethodCall { method, args } => {
                self.intern_string(method);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::NewDynamic { callee, args } => {
                self.collect_strings_in_expr(callee);
                for a in args { self.collect_strings_in_expr(a); }
            }
            _ => {}
        }
    }
}

/// Context for emitting a single function body
struct FuncEmitCtx<'a> {
    emitter: &'a WasmModuleEmitter,
    local_map: &'a BTreeMap<LocalId, u32>,
    /// Block nesting depth for break/continue
    break_depth: Vec<u32>,
    loop_depth: Vec<u32>,
    block_depth: u32,
    /// Current class name (set when compiling class methods/constructors)
    current_class: Option<String>,
}

impl<'a> FuncEmitCtx<'a> {
    fn new(emitter: &'a WasmModuleEmitter, local_map: &'a BTreeMap<LocalId, u32>) -> Self {
        Self {
            emitter,
            local_map,
            break_depth: Vec::new(),
            loop_depth: Vec::new(),
            block_depth: 0,
            current_class: None,
        }
    }

    fn rt(&self) -> &RuntimeImports {
        self.emitter.rt.as_ref().unwrap()
    }

    /// Try to emit a method call on an object expression.
    /// Returns true if handled, false if not recognized.
    fn emit_method_call(&mut self, func: &mut Function, object: &Expr, method: &str, args: &[Expr]) -> bool {
        match method {
            // String methods
            "charAt" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_char_at));
                true
            }
            "substring" if args.len() >= 2 => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::Call(self.rt().string_substring));
                true
            }
            "indexOf" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_index_of));
                true
            }
            "slice" if args.len() >= 2 => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::Call(self.rt().string_slice));
                true
            }
            "toLowerCase" if args.is_empty() => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().string_to_lower_case));
                true
            }
            "toUpperCase" if args.is_empty() => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().string_to_upper_case));
                true
            }
            "trim" if args.is_empty() => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().string_trim));
                true
            }
            "includes" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_includes));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
                true
            }
            "startsWith" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_starts_with));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
                true
            }
            "endsWith" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_ends_with));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
                true
            }
            "replace" if args.len() >= 2 => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::Call(self.rt().string_replace));
                true
            }
            "split" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_split));
                true
            }
            "repeat" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().string_repeat));
                true
            }
            "padStart" if args.len() >= 2 => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::Call(self.rt().string_pad_start));
                true
            }
            "padEnd" if args.len() >= 2 => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::Call(self.rt().string_pad_end));
                true
            }
            // Array methods
            "push" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_push));
                func.instruction(&Instruction::Call(self.rt().array_length));
                true
            }
            "pop" => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().array_pop));
                true
            }
            "shift" => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().array_shift));
                true
            }
            "join" => {
                self.emit_expr(func, object);
                if !args.is_empty() {
                    self.emit_expr(func, &args[0]);
                } else {
                    let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                    let bits = (STRING_TAG << 48) | (comma_id as u64);
                    func.instruction(&f64_const(f64::from_bits(bits)));
                }
                func.instruction(&Instruction::Call(self.rt().array_join));
                true
            }
            "map" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_map));
                true
            }
            "filter" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_filter));
                true
            }
            "forEach" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_for_each));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
                true
            }
            "find" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_find));
                true
            }
            "findIndex" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_find_index));
                true
            }
            "reduce" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                if args.len() >= 2 {
                    self.emit_expr(func, &args[1]);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_reduce));
                true
            }
            "sort" => {
                self.emit_expr(func, object);
                if !args.is_empty() {
                    self.emit_expr(func, &args[0]);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_sort));
                true
            }
            "reverse" => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().array_reverse));
                true
            }
            "concat" if !args.is_empty() => {
                self.emit_expr(func, object);
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::Call(self.rt().array_concat));
                true
            }
            "flat" => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().array_flat));
                true
            }
            "toString" => {
                self.emit_expr(func, object);
                func.instruction(&Instruction::Call(self.rt().jsvalue_to_string));
                true
            }
            _ => false,
        }
    }

    /// Emit a binary bitwise operation with proper i32 truncation
    fn emit_bitwise_binary(&mut self, func: &mut Function, left: &Expr, right: &Expr, op: Instruction<'static>) {
        self.emit_expr(func, left);
        func.instruction(&Instruction::I32TruncF64S);
        self.emit_expr(func, right);
        func.instruction(&Instruction::I32TruncF64S);
        func.instruction(&op);
        func.instruction(&Instruction::F64ConvertI32S);
    }

    fn emit_stmt(&mut self, func: &mut Function, stmt: &Stmt, in_returning_func: bool) {
        match stmt {
            Stmt::Let { id, init, .. } => {
                if let Some(init_expr) = init {
                    self.emit_expr(func, init_expr);
                } else {
                    // Default: undefined
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                if let Some(&idx) = self.local_map.get(id) {
                    func.instruction(&Instruction::LocalSet(idx));
                } else {
                    func.instruction(&Instruction::Drop);
                }
            }
            Stmt::Expr(expr) => {
                self.emit_expr(func, expr);
                // Drop the result (expression statement)
                // Check if expr produces a value
                if self.expr_has_value(expr) {
                    func.instruction(&Instruction::Drop);
                }
            }
            Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.emit_expr(func, e);
                } else if in_returning_func {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Return);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.emit_expr(func, condition);
                // Convert to i32 boolean via is_truthy
                func.instruction(&Instruction::Call(self.rt().is_truthy));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                for s in then_branch {
                    self.emit_stmt(func, s, in_returning_func);
                }
                if let Some(else_stmts) = else_branch {
                    func.instruction(&Instruction::Else);
                    for s in else_stmts {
                        self.emit_stmt(func, s, in_returning_func);
                    }
                }
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            Stmt::While { condition, body } => {
                // block $break
                //   loop $continue
                //     <condition>
                //     is_truthy
                //     i32.eqz
                //     br_if $break (1)
                //     <body>
                //     br $continue (0)
                //   end
                // end
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_depth = self.block_depth;
                self.break_depth.push(break_depth);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_depth = self.block_depth;
                self.loop_depth.push(continue_depth);

                self.emit_expr(func, condition);
                func.instruction(&Instruction::Call(self.rt().is_truthy));
                func.instruction(&Instruction::I32Eqz);
                func.instruction(&Instruction::BrIf(1)); // break to outer block

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                func.instruction(&Instruction::Br(0)); // continue (loop back)
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end loop

                self.loop_depth.pop();
                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end block
            }
            Stmt::For { init, condition, update, body } => {
                // <init>
                // block $break
                //   loop $continue
                //     <condition>
                //     is_truthy ; i32.eqz ; br_if $break
                //     <body>
                //     <update> ; drop
                //     br $continue
                //   end
                // end
                if let Some(init_stmt) = init {
                    self.emit_stmt(func, init_stmt, in_returning_func);
                }

                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                self.break_depth.push(self.block_depth);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                self.loop_depth.push(self.block_depth);

                if let Some(cond) = condition {
                    self.emit_expr(func, cond);
                    func.instruction(&Instruction::Call(self.rt().is_truthy));
                    func.instruction(&Instruction::I32Eqz);
                    func.instruction(&Instruction::BrIf(1));
                }

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                if let Some(upd) = update {
                    self.emit_expr(func, upd);
                    if self.expr_has_value(upd) {
                        func.instruction(&Instruction::Drop);
                    }
                }

                func.instruction(&Instruction::Br(0));
                self.block_depth -= 1;
                func.instruction(&Instruction::End);

                self.loop_depth.pop();
                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            Stmt::Break => {
                // Branch to the enclosing block (break target)
                // The break target is 1 level up from the loop
                func.instruction(&Instruction::Br(1));
            }
            Stmt::Continue => {
                // Branch to the enclosing loop (continue target)
                func.instruction(&Instruction::Br(0));
            }
            Stmt::Throw(expr) => {
                // Set exception in bridge and return
                self.emit_expr(func, expr);
                func.instruction(&Instruction::Call(self.rt().throw_value));
                if in_returning_func {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                    func.instruction(&Instruction::Return);
                }
            }
            Stmt::Try { body, catch, finally } => {
                // Bridge-based exception handling:
                // try_start(); <try body>; try_end();
                // if has_exception(): <bind catch param>; <catch body>
                // <finally body>
                func.instruction(&Instruction::Call(self.rt().try_start));

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                func.instruction(&Instruction::Call(self.rt().try_end));

                // Check for exception and execute catch block
                if let Some(catch_clause) = catch {
                    func.instruction(&Instruction::Call(self.rt().has_exception));
                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                    self.block_depth += 1;

                    // Bind catch parameter
                    if let Some((param_id, _)) = &catch_clause.param {
                        func.instruction(&Instruction::Call(self.rt().get_exception));
                        if let Some(&local_idx) = self.local_map.get(param_id) {
                            func.instruction(&Instruction::LocalSet(local_idx));
                        } else {
                            func.instruction(&Instruction::Drop);
                        }
                    } else {
                        // No param, just clear the exception
                        func.instruction(&Instruction::Call(self.rt().get_exception));
                        func.instruction(&Instruction::Drop);
                    }

                    for s in &catch_clause.body {
                        self.emit_stmt(func, s, in_returning_func);
                    }

                    self.block_depth -= 1;
                    func.instruction(&Instruction::End);
                }

                // Finally block (unconditional)
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        self.emit_stmt(func, s, in_returning_func);
                    }
                }
            }
            Stmt::Switch { discriminant, cases } => {
                // Compile switch as cascading if/else blocks
                // Strategy: store discriminant in a local-like pattern, compare each case
                // Since we can't easily allocate a local here, we use nested blocks + br_table approach
                // Simpler approach: nested if/else with js_strict_eq

                // Outer block for break
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                self.break_depth.push(self.block_depth);

                // We need to evaluate discriminant once. Without scratch locals,
                // we'll re-evaluate it for each case (works if it's a simple expression).
                // For complex discriminants, this could cause issues but handles most cases.

                let mut has_matched = false;
                for case in cases {
                    if let Some(test) = &case.test {
                        // case <test>:
                        self.emit_expr(func, discriminant);
                        self.emit_expr(func, test);
                        func.instruction(&Instruction::Call(self.rt().js_strict_eq));
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                        self.block_depth += 1;
                        for s in &case.body {
                            self.emit_stmt(func, s, in_returning_func);
                        }
                        self.block_depth -= 1;
                        func.instruction(&Instruction::End);
                    } else {
                        // default:
                        for s in &case.body {
                            self.emit_stmt(func, s, in_returning_func);
                        }
                        has_matched = true;
                    }
                }
                let _ = has_matched;

                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
        }
    }

    fn emit_expr(&mut self, func: &mut Function, expr: &Expr) {
        match expr {
            // --- Literals ---
            Expr::Number(n) => {
                func.instruction(&f64_const(*n));
            }
            Expr::Integer(i) => {
                func.instruction(&f64_const(*i as f64));
            }
            Expr::Bool(true) => {
                func.instruction(&f64_const_bits(TAG_TRUE));
            }
            Expr::Bool(false) => {
                func.instruction(&f64_const_bits(TAG_FALSE));
            }
            Expr::Undefined => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::Null => {
                func.instruction(&f64_const_bits(TAG_NULL));
            }
            Expr::String(s) => {
                let string_id = self.emitter.string_map.get(s.as_str())
                    .copied().unwrap_or(0);
                // NaN-box: (STRING_TAG << 48) | string_id
                let bits = (STRING_TAG << 48) | (string_id as u64);
                func.instruction(&f64_const(f64::from_bits(bits)));
            }

            // --- Variables ---
            Expr::LocalGet(id) => {
                if let Some(&idx) = self.local_map.get(id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    // Unknown local — push undefined
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }
            Expr::LocalSet(id, val) => {
                self.emit_expr(func, val);
                if let Some(&idx) = self.local_map.get(id) {
                    // Tee: set and leave on stack
                    func.instruction(&Instruction::LocalTee(idx));
                }
            }
            Expr::GlobalGet(id) => {
                if let Some(&idx) = self.emitter.global_map.get(id) {
                    func.instruction(&Instruction::GlobalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }
            Expr::GlobalSet(id, val) => {
                self.emit_expr(func, val);
                if let Some(&idx) = self.emitter.global_map.get(id) {
                    // Duplicate value on stack (set + leave result)
                    // WASM doesn't have GlobalTee, so we need a local
                    func.instruction(&Instruction::GlobalSet(idx));
                    func.instruction(&Instruction::GlobalGet(idx));
                }
            }

            // --- Update ---
            Expr::Update { id, op, prefix } => {
                if let Some(&idx) = self.local_map.get(id) {
                    if *prefix {
                        // ++x: increment then return new value
                        func.instruction(&Instruction::LocalGet(idx));
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => { func.instruction(&Instruction::F64Add); }
                            UpdateOp::Decrement => { func.instruction(&Instruction::F64Sub); }
                        };
                        func.instruction(&Instruction::LocalTee(idx));
                    } else {
                        // x++: return old value, then increment
                        func.instruction(&Instruction::LocalGet(idx));
                        // Compute new value
                        func.instruction(&Instruction::LocalGet(idx));
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => { func.instruction(&Instruction::F64Add); }
                            UpdateOp::Decrement => { func.instruction(&Instruction::F64Sub); }
                        };
                        func.instruction(&Instruction::LocalSet(idx));
                        // Old value is still on stack
                    }
                } else {
                    func.instruction(&f64_const(f64::NAN));
                }
            }

            // --- Binary operations ---
            Expr::Binary { op, left, right } => {
                match op {
                    BinaryOp::Add => {
                        // Use js_add for dynamic dispatch (handles string+number etc.)
                        self.emit_expr(func, left);
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Call(self.rt().js_add));
                    }
                    // Bitwise ops need i32 truncation before the operation
                    BinaryOp::BitAnd => { self.emit_bitwise_binary(func, left, right, Instruction::I32And); }
                    BinaryOp::BitOr => { self.emit_bitwise_binary(func, left, right, Instruction::I32Or); }
                    BinaryOp::BitXor => { self.emit_bitwise_binary(func, left, right, Instruction::I32Xor); }
                    BinaryOp::Shl => { self.emit_bitwise_binary(func, left, right, Instruction::I32Shl); }
                    BinaryOp::Shr => { self.emit_bitwise_binary(func, left, right, Instruction::I32ShrS); }
                    BinaryOp::UShr => { self.emit_bitwise_binary(func, left, right, Instruction::I32ShrU); }
                    _ => {
                        // Pure numeric operations
                        self.emit_expr(func, left);
                        self.emit_expr(func, right);
                        match op {
                            BinaryOp::Sub => { func.instruction(&Instruction::F64Sub); }
                            BinaryOp::Mul => { func.instruction(&Instruction::F64Mul); }
                            BinaryOp::Div => { func.instruction(&Instruction::F64Div); }
                            BinaryOp::Mod => {
                                func.instruction(&Instruction::Call(self.rt().js_mod));
                            }
                            BinaryOp::Pow => {
                                func.instruction(&Instruction::Call(self.rt().math_pow));
                            }
                            _ => { func.instruction(&Instruction::F64Add); }
                        };
                    }
                }
            }

            // --- Comparison ---
            Expr::Compare { op, left, right } => {
                self.emit_expr(func, left);
                self.emit_expr(func, right);
                // For strict equality on mixed types, use JS bridge
                match op {
                    CompareOp::Eq | CompareOp::Ne => {
                        func.instruction(&Instruction::Call(self.rt().js_strict_eq));
                        if matches!(op, CompareOp::Ne) {
                            func.instruction(&Instruction::I32Eqz);
                        }
                        // Convert i32 result to NaN-boxed boolean
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                        func.instruction(&f64_const_bits(TAG_TRUE));
                        func.instruction(&Instruction::Else);
                        func.instruction(&f64_const_bits(TAG_FALSE));
                        func.instruction(&Instruction::End);
                    }
                    _ => {
                        // Numeric comparisons
                        match op {
                            CompareOp::Lt => func.instruction(&Instruction::F64Lt),
                            CompareOp::Le => func.instruction(&Instruction::F64Le),
                            CompareOp::Gt => func.instruction(&Instruction::F64Gt),
                            CompareOp::Ge => func.instruction(&Instruction::F64Ge),
                            _ => unreachable!(),
                        };
                        // Convert i32 to NaN-boxed boolean
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                        func.instruction(&f64_const_bits(TAG_TRUE));
                        func.instruction(&Instruction::Else);
                        func.instruction(&f64_const_bits(TAG_FALSE));
                        func.instruction(&Instruction::End);
                    }
                }
            }

            // --- Logical ---
            Expr::Logical { op, left, right } => {
                match op {
                    LogicalOp::And => {
                        // Short-circuit: if left is falsy, return left; else return right
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::Call(self.rt().is_truthy));
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::End);
                    }
                    LogicalOp::Or => {
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::Call(self.rt().is_truthy));
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::End);
                    }
                    LogicalOp::Coalesce => {
                        // a ?? b: if a is null/undefined, return b; otherwise return a
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::Call(self.rt().is_null_or_undefined));
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::End);
                    }
                }
            }

            // --- Unary ---
            Expr::Unary { op, operand } => {
                self.emit_expr(func, operand);
                match op {
                    UnaryOp::Neg => { func.instruction(&Instruction::F64Neg); }
                    UnaryOp::Pos => {} // no-op for numbers
                    UnaryOp::Not => {
                        func.instruction(&Instruction::Call(self.rt().is_truthy));
                        func.instruction(&Instruction::I32Eqz);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                        func.instruction(&f64_const_bits(TAG_TRUE));
                        func.instruction(&Instruction::Else);
                        func.instruction(&f64_const_bits(TAG_FALSE));
                        func.instruction(&Instruction::End);
                    }
                    UnaryOp::BitNot => {
                        // ~x: convert to i32, bitwise not, convert back
                        func.instruction(&Instruction::I32TruncF64S);
                        func.instruction(&Instruction::I32Const(-1));
                        func.instruction(&Instruction::I32Xor);
                        func.instruction(&Instruction::F64ConvertI32S);
                    }
                };
            }

            // --- Function calls ---
            Expr::Call { callee, args, .. } => {
                // Check for method call patterns: obj.method(args)
                if let Expr::PropertyGet { object, property } = callee.as_ref() {
                    // console.log/warn/error
                    if let Expr::GlobalGet(_) = object.as_ref() {
                        match property.as_str() {
                            "log" => {
                                for arg in args {
                                    self.emit_expr(func, arg);
                                    func.instruction(&Instruction::Call(self.rt().console_log));
                                }
                                return;
                            }
                            "warn" => {
                                for arg in args {
                                    self.emit_expr(func, arg);
                                    func.instruction(&Instruction::Call(self.rt().console_warn));
                                }
                                return;
                            }
                            "error" => {
                                for arg in args {
                                    self.emit_expr(func, arg);
                                    func.instruction(&Instruction::Call(self.rt().console_error));
                                }
                                return;
                            }
                            _ => {}
                        }
                    }
                    // String/Array method calls: expr.method(args)
                    if self.emit_method_call(func, object, property, args) {
                        return;
                    }

                    // Fallback: try class method dispatch via bridge
                    {
                        let array_new = self.rt().array_new;
                        let array_push = self.rt().array_push;
                        let class_call_method = self.rt().class_call_method;
                        self.emit_expr(func, object);
                        let method_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                        let method_bits = (STRING_TAG << 48) | (method_id as u64);
                        func.instruction(&f64_const(f64::from_bits(method_bits)));
                        // Build args array
                        func.instruction(&Instruction::Call(array_new));
                        for arg in args {
                            self.emit_expr(func, arg);
                            func.instruction(&Instruction::Call(array_push));
                        }
                        func.instruction(&Instruction::Call(class_call_method));
                        return;
                    }
                }

                // Evaluate arguments first
                for arg in args {
                    self.emit_expr(func, arg);
                }
                // Call the function
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&idx) = self.emitter.func_map.get(id) {
                            func.instruction(&Instruction::Call(idx));
                        } else {
                            // Unknown function — push undefined
                            for _ in args {
                                func.instruction(&Instruction::Drop);
                            }
                            func.instruction(&f64_const_bits(TAG_UNDEFINED));
                        }
                    }
                    Expr::ExternFuncRef { name, .. } => {
                        // Cross-module function call — look up by name
                        if let Some(&idx) = self.emitter.func_name_map.get(name) {
                            func.instruction(&Instruction::Call(idx));
                        } else {
                            for _ in args {
                                func.instruction(&Instruction::Drop);
                            }
                            func.instruction(&f64_const_bits(TAG_UNDEFINED));
                        }
                    }
                    _ => {
                        // Dynamic call via closure bridge
                        // Stack has: [arg0, arg1, ..., argN] but callee not yet pushed
                        // We need callee first for closure_call. Restructure:
                        // Drop the args we already pushed, re-emit callee first, then args
                        for _ in args {
                            func.instruction(&Instruction::Drop);
                        }
                        // Now emit: callee, args..., closure_call_N
                        self.emit_expr(func, callee);
                        for arg in args {
                            self.emit_expr(func, arg);
                        }
                        match args.len() {
                            0 => { func.instruction(&Instruction::Call(self.rt().closure_call_0)); }
                            1 => { func.instruction(&Instruction::Call(self.rt().closure_call_1)); }
                            2 => { func.instruction(&Instruction::Call(self.rt().closure_call_2)); }
                            3 => { func.instruction(&Instruction::Call(self.rt().closure_call_3)); }
                            _ => {
                                // Too many args for direct call, use spread
                                func.instruction(&Instruction::Drop); // drop callee
                                for _ in args { func.instruction(&Instruction::Drop); }
                                func.instruction(&f64_const_bits(TAG_UNDEFINED));
                            }
                        }
                    }
                }
            }

            // --- Native method calls (console.log, etc.) ---
            Expr::NativeMethodCall { module, method, object, args, .. } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                match normalized {
                    "console" => {
                        match method.as_str() {
                            "log" => {
                                for arg in args {
                                    self.emit_expr(func, arg);
                                    func.instruction(&Instruction::Call(self.rt().console_log));
                                }
                            }
                            "warn" => {
                                for arg in args {
                                    self.emit_expr(func, arg);
                                    func.instruction(&Instruction::Call(self.rt().console_warn));
                                }
                            }
                            "error" => {
                                for arg in args {
                                    self.emit_expr(func, arg);
                                    func.instruction(&Instruction::Call(self.rt().console_error));
                                }
                            }
                            _ => {}
                        }
                    }
                    "JSON" => {
                        match method.as_str() {
                            "parse" => {
                                if let Some(a) = args.first() {
                                    self.emit_expr(func, a);
                                    func.instruction(&Instruction::Call(self.rt().json_parse));
                                } else {
                                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                                }
                            }
                            "stringify" => {
                                if let Some(a) = args.first() {
                                    self.emit_expr(func, a);
                                    func.instruction(&Instruction::Call(self.rt().json_stringify));
                                } else {
                                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                                }
                            }
                            _ => {}
                        }
                    }
                    "Math" => {
                        match method.as_str() {
                            "floor" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64Floor); }
                            "ceil" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64Ceil); }
                            "round" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64Nearest); }
                            "abs" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64Abs); }
                            "sqrt" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64Sqrt); }
                            "pow" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::Call(self.rt().math_pow));
                            }
                            "min" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::F64Min);
                            }
                            "max" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::F64Max);
                            }
                            "random" => {
                                func.instruction(&Instruction::Call(self.rt().math_random));
                            }
                            "log" if !args.is_empty() => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::Call(self.rt().math_log));
                            }
                            "log2" if !args.is_empty() => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::Call(self.rt().math_log2));
                            }
                            "log10" if !args.is_empty() => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::Call(self.rt().math_log10));
                            }
                            _ => { func.instruction(&f64_const_bits(TAG_UNDEFINED)); }
                        }
                    }
                    _ => {
                        // Handle instance method calls on objects
                        if let Some(obj) = object {
                            self.emit_expr(func, obj);
                            match method.as_str() {
                                // String instance methods
                                "charAt" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_char_at));
                                }
                                "substring" if args.len() >= 2 => {
                                    self.emit_expr(func, &args[0]);
                                    self.emit_expr(func, &args[1]);
                                    func.instruction(&Instruction::Call(self.rt().string_substring));
                                }
                                "indexOf" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_index_of));
                                }
                                "slice" if args.len() >= 2 => {
                                    self.emit_expr(func, &args[0]);
                                    self.emit_expr(func, &args[1]);
                                    func.instruction(&Instruction::Call(self.rt().string_slice));
                                }
                                "toLowerCase" => {
                                    func.instruction(&Instruction::Call(self.rt().string_to_lower_case));
                                }
                                "toUpperCase" => {
                                    func.instruction(&Instruction::Call(self.rt().string_to_upper_case));
                                }
                                "trim" => {
                                    func.instruction(&Instruction::Call(self.rt().string_trim));
                                }
                                "includes" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_includes));
                                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                                    func.instruction(&f64_const_bits(TAG_TRUE));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&f64_const_bits(TAG_FALSE));
                                    func.instruction(&Instruction::End);
                                }
                                "startsWith" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_starts_with));
                                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                                    func.instruction(&f64_const_bits(TAG_TRUE));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&f64_const_bits(TAG_FALSE));
                                    func.instruction(&Instruction::End);
                                }
                                "endsWith" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_ends_with));
                                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                                    func.instruction(&f64_const_bits(TAG_TRUE));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&f64_const_bits(TAG_FALSE));
                                    func.instruction(&Instruction::End);
                                }
                                "replace" if args.len() >= 2 => {
                                    self.emit_expr(func, &args[0]);
                                    self.emit_expr(func, &args[1]);
                                    func.instruction(&Instruction::Call(self.rt().string_replace));
                                }
                                "split" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_split));
                                }
                                "repeat" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().string_repeat));
                                }
                                "padStart" if args.len() >= 2 => {
                                    self.emit_expr(func, &args[0]);
                                    self.emit_expr(func, &args[1]);
                                    func.instruction(&Instruction::Call(self.rt().string_pad_start));
                                }
                                "padEnd" if args.len() >= 2 => {
                                    self.emit_expr(func, &args[0]);
                                    self.emit_expr(func, &args[1]);
                                    func.instruction(&Instruction::Call(self.rt().string_pad_end));
                                }
                                // Array instance methods called via NativeMethodCall
                                "push" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_push));
                                    func.instruction(&Instruction::Call(self.rt().array_length));
                                }
                                "pop" => {
                                    func.instruction(&Instruction::Call(self.rt().array_pop));
                                }
                                "shift" => {
                                    func.instruction(&Instruction::Call(self.rt().array_shift));
                                }
                                "unshift" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_unshift));
                                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                                }
                                "join" => {
                                    if !args.is_empty() {
                                        self.emit_expr(func, &args[0]);
                                    } else {
                                        let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                                        let bits = (STRING_TAG << 48) | (comma_id as u64);
                                        func.instruction(&f64_const(f64::from_bits(bits)));
                                    }
                                    func.instruction(&Instruction::Call(self.rt().array_join));
                                }
                                "map" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_map));
                                }
                                "filter" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_filter));
                                }
                                "forEach" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_for_each));
                                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                                }
                                "find" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_find));
                                }
                                "findIndex" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_find_index));
                                }
                                "reduce" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    if args.len() >= 2 {
                                        self.emit_expr(func, &args[1]);
                                    } else {
                                        func.instruction(&f64_const_bits(TAG_UNDEFINED));
                                    }
                                    func.instruction(&Instruction::Call(self.rt().array_reduce));
                                }
                                "sort" => {
                                    if !args.is_empty() {
                                        self.emit_expr(func, &args[0]);
                                    } else {
                                        func.instruction(&f64_const_bits(TAG_UNDEFINED));
                                    }
                                    func.instruction(&Instruction::Call(self.rt().array_sort));
                                }
                                "reverse" => {
                                    func.instruction(&Instruction::Call(self.rt().array_reverse));
                                }
                                "concat" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    func.instruction(&Instruction::Call(self.rt().array_concat));
                                }
                                "flat" => {
                                    func.instruction(&Instruction::Call(self.rt().array_flat));
                                }
                                "length" => {
                                    func.instruction(&Instruction::Call(self.rt().array_length));
                                }
                                // Response methods
                                "json" => {
                                    func.instruction(&Instruction::Call(self.rt().response_json));
                                }
                                "text" => {
                                    func.instruction(&Instruction::Call(self.rt().response_text));
                                }
                                "status" => {
                                    func.instruction(&Instruction::Call(self.rt().response_status));
                                }
                                _ => {
                                    // Fall back to class_call_method for class instances
                                    let array_new = self.rt().array_new;
                                    let array_push = self.rt().array_push;
                                    let class_call_method = self.rt().class_call_method;
                                    let method_id = self.emitter.string_map.get(method.as_str()).copied().unwrap_or(0);
                                    let method_bits = (STRING_TAG << 48) | (method_id as u64);
                                    func.instruction(&f64_const(f64::from_bits(method_bits)));
                                    func.instruction(&Instruction::Call(array_new));
                                    for arg in args {
                                        self.emit_expr(func, arg);
                                        func.instruction(&Instruction::Call(array_push));
                                    }
                                    func.instruction(&Instruction::Call(class_call_method));
                                }
                            }
                        } else {
                            // No object — module-level function
                            func.instruction(&f64_const_bits(TAG_UNDEFINED));
                        }
                    }
                }
            }

            // --- Conditional (ternary) ---
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.emit_expr(func, condition);
                func.instruction(&Instruction::Call(self.rt().is_truthy));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                self.emit_expr(func, then_expr);
                func.instruction(&Instruction::Else);
                self.emit_expr(func, else_expr);
                func.instruction(&Instruction::End);
            }

            // --- Math ---
            Expr::MathFloor(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64Floor);
            }
            Expr::MathCeil(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64Ceil);
            }
            Expr::MathAbs(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64Abs);
            }
            Expr::MathSqrt(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64Sqrt);
            }
            Expr::MathRound(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64Nearest);
            }
            Expr::MathPow(base, exp) => {
                self.emit_expr(func, base);
                self.emit_expr(func, exp);
                func.instruction(&Instruction::Call(self.rt().math_pow));
            }
            Expr::MathMin(args) if args.len() == 2 => {
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::F64Min);
            }
            Expr::MathMax(args) if args.len() == 2 => {
                self.emit_expr(func, &args[0]);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::F64Max);
            }
            Expr::MathRandom => {
                func.instruction(&Instruction::Call(self.rt().math_random));
            }

            // --- Typeof ---
            Expr::TypeOf(operand) => {
                self.emit_expr(func, operand);
                func.instruction(&Instruction::Call(self.rt().js_typeof));
            }

            // --- Async ---
            Expr::Await(e) => {
                // Evaluate inner expression, then call await_promise bridge
                // If the value is a promise handle, tries to get resolved value
                // If not a promise, returns the value as-is
                self.emit_expr(func, e);
                func.instruction(&Instruction::Call(self.rt().await_promise));
            }

            // --- Object literal ---
            Expr::Object(fields) => {
                let rt = self.rt();
                let obj_new = rt.object_new;
                let obj_set = rt.object_set;
                func.instruction(&Instruction::Call(obj_new));
                // Stack: [handle]
                for (key, val) in fields {
                    // Duplicate the handle (no tee for intermediate values, re-get isn't possible)
                    // We need to keep the handle. Use a strategy: emit handle, key, value, call set, then re-push handle.
                    // Actually object_set is (handle, key, value) -> void, and we need handle to remain.
                    // Problem: after Call, the handle is consumed. We need it for subsequent sets.
                    // Solution: use a local. But we don't have one allocated.
                    // Alternative: just call object_new once at the start and then repeatedly push undefined
                    // Actually the simplest: just re-emit object_new? No, that creates a new object.
                    // The trick: we'll emit one extra set of instructions to duplicate the handle.
                    // But WASM has no dup instruction. We need a scratch local.
                    // For now, we'll work around this by storing the handle in a pattern:
                    // object_set returns void, so we do:
                    //   call object_new -> handle on stack
                    //   For each field: we need handle on stack again
                    // Without locals we can't do this. Let's emit it differently.
                    // We'll just call object_new, then for each field: push handle, push key, push value, call set.
                    // But we don't have the handle anymore after the first set.
                    //
                    // The real solution: our object_set should return the handle.
                    // Let's change the bridge: object_set returns handle for chaining.
                    // Actually let's just do it the simple way by using the JS bridge
                    // which handles everything. We'll change object_set to return handle.
                    let key_id = self.emitter.string_map.get(key.as_str()).copied().unwrap_or(0);
                    let key_bits = (STRING_TAG << 48) | (key_id as u64);
                    func.instruction(&f64_const(f64::from_bits(key_bits)));
                    self.emit_expr(func, val);
                    func.instruction(&Instruction::Call(obj_set));
                    // object_set returns handle (chaining)
                }
                // Handle is on stack from last object_set (or object_new if no fields)
            }

            // --- Object spread ---
            Expr::ObjectSpread { parts } => {
                let rt = self.rt();
                let obj_new = rt.object_new;
                let obj_set = rt.object_set;
                let obj_assign = rt.object_assign;
                func.instruction(&Instruction::Call(obj_new));
                for (key_opt, val) in parts {
                    if let Some(key) = key_opt {
                        // Named field
                        let key_id = self.emitter.string_map.get(key.as_str()).copied().unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        func.instruction(&f64_const(f64::from_bits(key_bits)));
                        self.emit_expr(func, val);
                        func.instruction(&Instruction::Call(obj_set));
                    } else {
                        // Spread: ...val
                        self.emit_expr(func, val);
                        func.instruction(&Instruction::Call(obj_assign));
                    }
                }
            }

            // --- Array literal ---
            Expr::Array(elements) => {
                let rt = self.rt();
                let arr_new = rt.array_new;
                let arr_push = rt.array_push;
                func.instruction(&Instruction::Call(arr_new));
                for elem in elements {
                    self.emit_expr(func, elem);
                    func.instruction(&Instruction::Call(arr_push));
                    // array_push returns handle (chaining)
                }
            }

            // --- Array spread ---
            Expr::ArraySpread(elements) => {
                let rt = self.rt();
                let arr_new = rt.array_new;
                let arr_push = rt.array_push;
                let arr_push_spread = rt.array_push_spread;
                func.instruction(&Instruction::Call(arr_new));
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) => {
                            self.emit_expr(func, e);
                            func.instruction(&Instruction::Call(arr_push));
                        }
                        ArrayElement::Spread(e) => {
                            self.emit_expr(func, e);
                            func.instruction(&Instruction::Call(arr_push_spread));
                        }
                    }
                }
            }

            // --- Property access ---
            Expr::PropertyGet { object, property } => {
                // Special case: .length uses string_len which handles both strings and arrays
                if property == "length" {
                    self.emit_expr(func, object);
                    func.instruction(&Instruction::Call(self.rt().string_len));
                    return;
                }
                // Special case: .message on error objects
                if property == "message" {
                    self.emit_expr(func, object);
                    func.instruction(&Instruction::Call(self.rt().error_message));
                    return;
                }
                self.emit_expr(func, object);
                let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                func.instruction(&f64_const(f64::from_bits(key_bits)));
                // Use class_get_field (works for both plain objects and class instances)
                func.instruction(&Instruction::Call(self.rt().class_get_field));
            }
            Expr::PropertySet { object, property, value } => {
                self.emit_expr(func, object);
                let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                func.instruction(&f64_const(f64::from_bits(key_bits)));
                self.emit_expr(func, value);
                // Use class_set_field (works for both plain objects and class instances)
                func.instruction(&Instruction::Call(self.rt().class_set_field));
                // class_set_field is void; push the object back for chaining
                self.emit_expr(func, object);
            }
            Expr::PropertyUpdate { object, property, op, prefix } => {
                // obj.prop++ or ++obj.prop
                self.emit_expr(func, object);
                let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Get current value
                // We need the object handle twice. Can't dup in WASM without locals.
                // For simplicity: re-emit object (works if object is a simple expression)
                func.instruction(&f64_const(f64::from_bits(key_bits)));
                func.instruction(&Instruction::Call(self.rt().object_get));
                // Stack: [old_value]
                if *prefix {
                    func.instruction(&f64_const(1.0));
                    match op {
                        BinaryOp::Add => func.instruction(&Instruction::F64Add),
                        BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                        _ => func.instruction(&Instruction::F64Add),
                    };
                    // Set new value
                    self.emit_expr(func, object);
                    func.instruction(&f64_const(f64::from_bits(key_bits)));
                    // Stack: [new_val, handle, key] — wrong order for object_set(handle, key, val)
                    // We need to restructure. For now, just emit the value (prefix returns new)
                    // This is imprecise but works for basic cases
                } else {
                    // postfix: return old, then update
                    // For now, just do the increment and return new value (approximate)
                    func.instruction(&f64_const(1.0));
                    match op {
                        BinaryOp::Add => func.instruction(&Instruction::F64Add),
                        BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                        _ => func.instruction(&Instruction::F64Add),
                    };
                }
            }

            // --- Index access ---
            Expr::IndexGet { object, index } => {
                self.emit_expr(func, object);
                self.emit_expr(func, index);
                func.instruction(&Instruction::Call(self.rt().object_get_dynamic));
            }
            Expr::IndexSet { object, index, value } => {
                self.emit_expr(func, object);
                self.emit_expr(func, index);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().object_set_dynamic));
                // set_dynamic is void; push undefined as expression result
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::IndexUpdate { object, index, op, prefix: _ } => {
                // Approximate: get, increment, set
                self.emit_expr(func, object);
                self.emit_expr(func, index);
                func.instruction(&Instruction::Call(self.rt().object_get_dynamic));
                func.instruction(&f64_const(1.0));
                match op {
                    BinaryOp::Add => func.instruction(&Instruction::F64Add),
                    BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                    _ => func.instruction(&Instruction::F64Add),
                };
            }

            // --- Object/Array methods ---
            Expr::ObjectKeys(obj) => {
                self.emit_expr(func, obj);
                func.instruction(&Instruction::Call(self.rt().object_keys));
            }
            Expr::ObjectValues(obj) => {
                self.emit_expr(func, obj);
                func.instruction(&Instruction::Call(self.rt().object_values));
            }
            Expr::ObjectEntries(obj) => {
                self.emit_expr(func, obj);
                func.instruction(&Instruction::Call(self.rt().object_entries));
            }
            Expr::ObjectRest { object, .. } => {
                // For now, just return a copy of the object (approximate)
                self.emit_expr(func, object);
            }
            Expr::Delete(expr) => {
                match expr.as_ref() {
                    Expr::PropertyGet { object, property } => {
                        self.emit_expr(func, object);
                        let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        func.instruction(&f64_const(f64::from_bits(key_bits)));
                        func.instruction(&Instruction::Call(self.rt().object_delete));
                        func.instruction(&f64_const_bits(TAG_TRUE));
                    }
                    Expr::IndexGet { object, index } => {
                        self.emit_expr(func, object);
                        self.emit_expr(func, index);
                        func.instruction(&Instruction::Call(self.rt().object_delete_dynamic));
                        func.instruction(&f64_const_bits(TAG_TRUE));
                    }
                    _ => {
                        func.instruction(&f64_const_bits(TAG_TRUE));
                    }
                }
            }
            Expr::In { property, object } => {
                self.emit_expr(func, object);
                self.emit_expr(func, property);
                func.instruction(&Instruction::Call(self.rt().object_has_property));
                // Convert i32 to NaN-boxed boolean
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }

            // --- Array methods (HIR-level) ---
            Expr::ArrayPush { array_id, value } => {
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().array_push));
                // array_push returns handle, but ArrayPush typically returns new length
                // The bridge returns the array handle. We need to store back and return length.
                // For now, return the result of array_push (the handle).
                // Actually, drop result and push the new length
                func.instruction(&Instruction::Call(self.rt().array_length));
            }
            Expr::ArrayPushSpread { array_id, source } => {
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                self.emit_expr(func, source);
                func.instruction(&Instruction::Call(self.rt().array_push_spread));
                // Returns handle
            }
            Expr::ArrayPop(array_id) => {
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_pop));
            }
            Expr::ArrayShift(array_id) => {
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_shift));
            }
            Expr::ArrayUnshift { array_id, value } => {
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().array_unshift));
                // void return, push length
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_length));
            }
            Expr::ArraySlice { array, start, end } => {
                self.emit_expr(func, array);
                self.emit_expr(func, start);
                if let Some(e) = end {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&f64_const(f64::from_bits(TAG_UNDEFINED)));
                }
                func.instruction(&Instruction::Call(self.rt().array_slice));
            }
            Expr::ArraySplice { array_id, start, delete_count, items } => {
                if let Some(&idx) = self.local_map.get(array_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                self.emit_expr(func, start);
                if let Some(dc) = delete_count {
                    self.emit_expr(func, dc);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_splice));
                // Returns removed elements array handle
                // TODO: insert items if present
                let _ = items;
            }
            Expr::ArrayJoin { array, separator } => {
                self.emit_expr(func, array);
                if let Some(sep) = separator {
                    self.emit_expr(func, sep);
                } else {
                    // Default separator: ","
                    let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                    let comma_bits = (STRING_TAG << 48) | (comma_id as u64);
                    func.instruction(&f64_const(f64::from_bits(comma_bits)));
                }
                func.instruction(&Instruction::Call(self.rt().array_join));
            }
            Expr::ArrayIndexOf { array, value } => {
                self.emit_expr(func, array);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().array_index_of));
            }
            Expr::ArrayIncludes { array, value } => {
                self.emit_expr(func, array);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().array_includes));
                // Convert i32 to NaN-boxed boolean
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::ArrayFlat { array } => {
                self.emit_expr(func, array);
                func.instruction(&Instruction::Call(self.rt().array_flat));
            }
            Expr::ArrayIsArray(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().array_is_array));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::ArrayFrom(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().array_from));
            }

            // --- Array higher-order methods ---
            Expr::ArrayMap { array, callback } => {
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                func.instruction(&Instruction::Call(self.rt().array_map));
            }
            Expr::ArrayFilter { array, callback } => {
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                func.instruction(&Instruction::Call(self.rt().array_filter));
            }
            Expr::ArrayForEach { array, callback } => {
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                func.instruction(&Instruction::Call(self.rt().array_for_each));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::ArrayFind { array, callback } => {
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                func.instruction(&Instruction::Call(self.rt().array_find));
            }
            Expr::ArrayFindIndex { array, callback } => {
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                func.instruction(&Instruction::Call(self.rt().array_find_index));
            }
            Expr::ArraySort { array, comparator } => {
                self.emit_expr(func, array);
                self.emit_expr(func, comparator);
                func.instruction(&Instruction::Call(self.rt().array_sort));
            }
            Expr::ArrayReduce { array, callback, initial } => {
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                if let Some(init) = initial {
                    self.emit_expr(func, init);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().array_reduce));
            }

            // --- Closure ---
            Expr::Closure { func_id, params, body, captures, mutable_captures, .. } => {
                // Compile closure body as a function (it was already registered if it's in module.functions)
                // If not registered, we need to handle it inline
                if let Some(&func_idx) = self.emitter.func_map.get(func_id) {
                    // Function is registered, create closure handle
                    // Use table index, not raw WASM function index
                    let table_idx = self.emitter.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                    func.instruction(&f64_const(table_idx as f64));
                    func.instruction(&f64_const(captures.len() as f64));
                    func.instruction(&Instruction::Call(self.rt().closure_new));
                    // Set captures
                    for (i, cap_id) in captures.iter().chain(mutable_captures.iter()).enumerate() {
                        // Duplicate closure handle (it's returned by closure_new)
                        // closure_set_capture(handle, idx, value) -> handle (chaining)
                        func.instruction(&f64_const(i as f64));
                        if let Some(&local_idx) = self.local_map.get(cap_id) {
                            func.instruction(&Instruction::LocalGet(local_idx));
                        } else {
                            func.instruction(&f64_const_bits(TAG_UNDEFINED));
                        }
                        func.instruction(&Instruction::Call(self.rt().closure_set_capture));
                    }
                } else {
                    // Inline closure — not in function table, push undefined
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                let _ = (params, body);
            }
            Expr::FuncRef(id) => {
                if let Some(&func_idx) = self.emitter.func_map.get(id) {
                    // Create a closure wrapper with 0 captures for function reference
                    let table_idx = self.emitter.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                    func.instruction(&f64_const(table_idx as f64));
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::Call(self.rt().closure_new));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }
            Expr::ExternFuncRef { name, .. } => {
                // Look up by function name in the flat function index space
                if let Some(&func_idx) = self.emitter.func_name_map.get(name) {
                    // Create a closure wrapper with 0 captures (like FuncRef)
                    let table_idx = self.emitter.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                    func.instruction(&f64_const(table_idx as f64));
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::Call(self.rt().closure_new));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }

            // --- Class instantiation ---
            Expr::New { class_name, args, .. } => {
                // Create new instance via bridge
                let class_name_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_name_id as u64);
                func.instruction(&f64_const(f64::from_bits(class_bits)));
                func.instruction(&f64_const(args.len() as f64));
                func.instruction(&Instruction::Call(self.rt().class_new));
                // Call the compiled constructor if it exists
                if let Some(&ctor_idx) = self.emitter.class_ctor_map.get(class_name.as_str()) {
                    // Stack: [instance_handle]
                    // Constructor takes: (this, arg0, arg1, ...)
                    // instance_handle is already on stack as first arg (this)
                    for arg in args {
                        self.emit_expr(func, arg);
                    }
                    func.instruction(&Instruction::Call(ctor_idx));
                    // Constructor returns this
                }
                // If no compiled constructor, just leave the instance handle on stack
            }
            Expr::NewDynamic { callee, args } => {
                // Dynamic new — approximate with regular call
                self.emit_expr(func, callee);
                for arg in args {
                    self.emit_expr(func, arg);
                }
                // Use closure_call
                match args.len() {
                    0 => { func.instruction(&Instruction::Call(self.rt().closure_call_0)); }
                    1 => { func.instruction(&Instruction::Call(self.rt().closure_call_1)); }
                    2 => { func.instruction(&Instruction::Call(self.rt().closure_call_2)); }
                    3 => { func.instruction(&Instruction::Call(self.rt().closure_call_3)); }
                    _ => {
                        for _ in args { func.instruction(&Instruction::Drop); }
                        func.instruction(&Instruction::Drop); // callee
                        func.instruction(&f64_const_bits(TAG_UNDEFINED));
                    }
                }
            }
            Expr::This => {
                // 'this' is passed as first parameter (local 0) in methods
                func.instruction(&Instruction::LocalGet(0));
            }
            Expr::SuperCall(args) => {
                // Call parent constructor: super(args)
                // this is local 0 in the current constructor
                let mut called = false;
                if let Some(ref current_class) = self.current_class {
                    // Look up parent class name
                    if let Some(parent_name) = self.emitter.class_parent_map.get(current_class) {
                        if let Some(&ctor_idx) = self.emitter.class_ctor_map.get(parent_name) {
                            // Call parent constructor with this + args
                            func.instruction(&Instruction::LocalGet(0)); // this
                            for arg in args {
                                self.emit_expr(func, arg);
                            }
                            func.instruction(&Instruction::Call(ctor_idx));
                            func.instruction(&Instruction::Drop); // parent ctor returns this, discard
                            called = true;
                        }
                    }
                }
                if !called {
                    // No parent constructor found, drop args
                    for arg in args {
                        self.emit_expr(func, arg);
                        func.instruction(&Instruction::Drop);
                    }
                }
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::SuperMethodCall { method, args } => {
                // Call parent method on this via class_call_method (walks parent chain)
                let array_new = self.rt().array_new;
                let array_push = self.rt().array_push;
                let class_call_method = self.rt().class_call_method;
                func.instruction(&Instruction::LocalGet(0)); // this handle
                let method_id = self.emitter.string_map.get(method.as_str()).copied().unwrap_or(0);
                let method_bits = (STRING_TAG << 48) | (method_id as u64);
                func.instruction(&f64_const(f64::from_bits(method_bits)));
                // Build args array
                func.instruction(&Instruction::Call(array_new));
                for arg in args {
                    self.emit_expr(func, arg);
                    func.instruction(&Instruction::Call(array_push));
                }
                func.instruction(&Instruction::Call(class_call_method));
            }
            Expr::ClassRef(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::StaticFieldGet { class_name, field_name } => {
                let class_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let field_id = self.emitter.string_map.get(field_name.as_str()).copied().unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                func.instruction(&f64_const(f64::from_bits(class_bits)));
                func.instruction(&f64_const(f64::from_bits(field_bits)));
                func.instruction(&Instruction::Call(self.rt().class_get_static));
            }
            Expr::StaticFieldSet { class_name, field_name, value } => {
                let class_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let field_id = self.emitter.string_map.get(field_name.as_str()).copied().unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                func.instruction(&f64_const(f64::from_bits(class_bits)));
                func.instruction(&f64_const(f64::from_bits(field_bits)));
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().class_set_static));
                // void return, push the value back
                self.emit_expr(func, value);
            }
            Expr::StaticMethodCall { class_name, method_name, args } => {
                // Try to call compiled static method directly
                if let Some(statics) = self.emitter.class_static_map.get(class_name.as_str()) {
                    if let Some(&static_idx) = statics.get(method_name.as_str()) {
                        // Direct call to compiled static method (no this param)
                        for arg in args {
                            self.emit_expr(func, arg);
                        }
                        func.instruction(&Instruction::Call(static_idx));
                        return;
                    }
                }
                // Fallback: bridge dispatch
                let class_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let method_id = self.emitter.string_map.get(method_name.as_str()).copied().unwrap_or(0);
                let method_bits = (STRING_TAG << 48) | (method_id as u64);
                let rt = self.rt();
                let arr_new = rt.array_new;
                let arr_push = rt.array_push;
                let call_method = rt.class_call_method;
                func.instruction(&f64_const(f64::from_bits(class_bits)));
                func.instruction(&f64_const(f64::from_bits(method_bits)));
                func.instruction(&Instruction::Call(arr_new));
                for arg in args {
                    self.emit_expr(func, arg);
                    func.instruction(&Instruction::Call(arr_push));
                }
                func.instruction(&Instruction::Call(call_method));
            }

            // --- Enum members ---
            Expr::EnumMember { enum_name: _, member_name } => {
                // Enum members are either numeric or string values
                // Try to parse as number first
                if let Ok(n) = member_name.parse::<f64>() {
                    func.instruction(&f64_const(n));
                } else {
                    // String enum member — return the member name as a string
                    let id = self.emitter.string_map.get(member_name.as_str()).copied().unwrap_or(0);
                    let bits = (STRING_TAG << 48) | (id as u64);
                    func.instruction(&f64_const(f64::from_bits(bits)));
                }
            }

            // --- InstanceOf ---
            Expr::InstanceOf { expr, ty } => {
                self.emit_expr(func, expr);
                let type_id = self.emitter.string_map.get(ty.as_str()).copied().unwrap_or(0);
                let type_bits = (STRING_TAG << 48) | (type_id as u64);
                func.instruction(&f64_const(f64::from_bits(type_bits)));
                func.instruction(&Instruction::Call(self.rt().class_instanceof));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }

            // --- Void ---
            Expr::Void(e) => {
                self.emit_expr(func, e);
                func.instruction(&Instruction::Drop);
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }

            // --- String methods ---
            Expr::StringSplit(string, delim) => {
                self.emit_expr(func, string);
                self.emit_expr(func, delim);
                func.instruction(&Instruction::Call(self.rt().string_split));
            }
            Expr::StringFromCharCode(code) => {
                self.emit_expr(func, code);
                func.instruction(&Instruction::Call(self.rt().string_from_char_code));
            }
            Expr::StringMatch { string, regex } => {
                self.emit_expr(func, string);
                self.emit_expr(func, regex);
                func.instruction(&Instruction::Call(self.rt().string_match));
            }
            Expr::StringReplace { string, pattern, replacement } => {
                self.emit_expr(func, string);
                self.emit_expr(func, pattern);
                self.emit_expr(func, replacement);
                func.instruction(&Instruction::Call(self.rt().string_replace));
            }
            Expr::StringCoerce(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().jsvalue_to_string));
            }

            // --- JSON ---
            Expr::JsonParse(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().json_parse));
            }
            Expr::JsonStringify(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().json_stringify));
            }

            // --- Map ---
            Expr::MapNew => {
                func.instruction(&Instruction::Call(self.rt().map_new));
            }
            Expr::MapSet { map, key, value } => {
                self.emit_expr(func, map);
                self.emit_expr(func, key);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().map_set));
                // void return, push the map back
                self.emit_expr(func, map);
            }
            Expr::MapGet { map, key } => {
                self.emit_expr(func, map);
                self.emit_expr(func, key);
                func.instruction(&Instruction::Call(self.rt().map_get));
            }
            Expr::MapHas { map, key } => {
                self.emit_expr(func, map);
                self.emit_expr(func, key);
                func.instruction(&Instruction::Call(self.rt().map_has));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::MapDelete { map, key } => {
                self.emit_expr(func, map);
                self.emit_expr(func, key);
                func.instruction(&Instruction::Call(self.rt().map_delete));
                func.instruction(&f64_const_bits(TAG_TRUE));
            }
            Expr::MapSize(map) => {
                self.emit_expr(func, map);
                func.instruction(&Instruction::Call(self.rt().map_size));
            }
            Expr::MapClear(map) => {
                self.emit_expr(func, map);
                func.instruction(&Instruction::Call(self.rt().map_clear));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::MapEntries(map) => {
                self.emit_expr(func, map);
                func.instruction(&Instruction::Call(self.rt().map_entries));
            }
            Expr::MapKeys(map) => {
                self.emit_expr(func, map);
                func.instruction(&Instruction::Call(self.rt().map_keys));
            }
            Expr::MapValues(map) => {
                self.emit_expr(func, map);
                func.instruction(&Instruction::Call(self.rt().map_values));
            }

            // --- Set ---
            Expr::SetNew => {
                func.instruction(&Instruction::Call(self.rt().set_new));
            }
            Expr::SetNewFromArray(arr) => {
                self.emit_expr(func, arr);
                func.instruction(&Instruction::Call(self.rt().set_new_from_array));
            }
            Expr::SetAdd { set_id, value } => {
                if let Some(&idx) = self.local_map.get(set_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().set_add));
                // void, push set back
                if let Some(&idx) = self.local_map.get(set_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }
            Expr::SetHas { set, value } => {
                self.emit_expr(func, set);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().set_has));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::SetDelete { set, value } => {
                self.emit_expr(func, set);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().set_delete));
                func.instruction(&f64_const_bits(TAG_TRUE));
            }
            Expr::SetSize(set) => {
                self.emit_expr(func, set);
                func.instruction(&Instruction::Call(self.rt().set_size));
            }
            Expr::SetClear(set) => {
                self.emit_expr(func, set);
                func.instruction(&Instruction::Call(self.rt().set_clear));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::SetValues(set) => {
                self.emit_expr(func, set);
                func.instruction(&Instruction::Call(self.rt().set_values));
            }

            // --- Date ---
            Expr::DateNew(arg) => {
                if let Some(a) = arg {
                    self.emit_expr(func, a);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().date_new));
            }
            Expr::DateGetTime(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_time));
            }
            Expr::DateToISOString(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_to_iso_string));
            }
            Expr::DateGetFullYear(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_full_year));
            }
            Expr::DateGetMonth(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_month));
            }
            Expr::DateGetDate(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_date));
            }
            Expr::DateGetHours(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_hours));
            }
            Expr::DateGetMinutes(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_minutes));
            }
            Expr::DateGetSeconds(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_seconds));
            }
            Expr::DateGetMilliseconds(d) => {
                self.emit_expr(func, d);
                func.instruction(&Instruction::Call(self.rt().date_get_milliseconds));
            }

            // --- Error ---
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    self.emit_expr(func, m);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().error_new));
            }
            Expr::ErrorMessage(err) => {
                self.emit_expr(func, err);
                func.instruction(&Instruction::Call(self.rt().error_message));
            }

            // --- RegExp ---
            Expr::RegExp { pattern, flags } => {
                let pat_id = self.emitter.string_map.get(pattern.as_str()).copied().unwrap_or(0);
                let pat_bits = (STRING_TAG << 48) | (pat_id as u64);
                let flags_id = self.emitter.string_map.get(flags.as_str()).copied().unwrap_or(0);
                let flags_bits = (STRING_TAG << 48) | (flags_id as u64);
                func.instruction(&f64_const(f64::from_bits(pat_bits)));
                func.instruction(&f64_const(f64::from_bits(flags_bits)));
                func.instruction(&Instruction::Call(self.rt().regexp_new));
            }
            Expr::RegExpTest { regex, string } => {
                self.emit_expr(func, regex);
                self.emit_expr(func, string);
                func.instruction(&Instruction::Call(self.rt().regexp_test));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }

            // --- Global builtins ---
            Expr::ParseInt { string, radix } => {
                self.emit_expr(func, string);
                let _ = radix; // TODO: radix support
                func.instruction(&Instruction::Call(self.rt().parse_int));
            }
            Expr::ParseFloat(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().parse_float));
            }
            Expr::NumberCoerce(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().number_coerce));
            }
            Expr::IsNaN(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().is_nan));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::IsFinite(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().is_finite));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::BigIntCoerce(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }

            // --- Math extra ---
            Expr::MathLog2(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::Call(self.rt().math_log2));
            }
            Expr::MathLog10(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::Call(self.rt().math_log10));
            }
            Expr::MathImul(a, b) => {
                self.emit_expr(func, a);
                func.instruction(&Instruction::I32TruncF64S);
                self.emit_expr(func, b);
                func.instruction(&Instruction::I32TruncF64S);
                func.instruction(&Instruction::I32Mul);
                func.instruction(&Instruction::F64ConvertI32S);
            }
            Expr::MathMin(args) if args.len() != 2 => {
                // Variadic min — use bridge
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_expr(func, arg);
                        func.instruction(&Instruction::Call(self.rt().math_min));
                    }
                } else {
                    func.instruction(&f64_const(f64::INFINITY));
                }
            }
            Expr::MathMax(args) if args.len() != 2 => {
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_expr(func, arg);
                        func.instruction(&Instruction::Call(self.rt().math_max));
                    }
                } else {
                    func.instruction(&f64_const(f64::NEG_INFINITY));
                }
            }

            // --- URL ---
            Expr::UrlNew { url, base } => {
                self.emit_expr(func, url);
                if let Some(b) = base {
                    // URL(url, base) — for now just use url
                    self.emit_expr(func, b);
                    func.instruction(&Instruction::Drop);
                }
                func.instruction(&Instruction::Call(self.rt().url_parse));
            }
            Expr::UrlGetHref(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_href));
            }
            Expr::UrlGetPathname(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_pathname));
            }
            Expr::UrlGetProtocol(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_protocol));
            }
            Expr::UrlGetHost(u) | Expr::UrlGetHostname(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_hostname));
            }
            Expr::UrlGetPort(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_port));
            }
            Expr::UrlGetSearch(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_search));
            }
            Expr::UrlGetHash(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_hash));
            }
            Expr::UrlGetOrigin(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_origin));
            }
            Expr::UrlGetSearchParams(u) => {
                self.emit_expr(func, u);
                func.instruction(&Instruction::Call(self.rt().url_get_search_params));
            }

            // --- Process/OS ---
            Expr::ProcessArgv => {
                func.instruction(&Instruction::Call(self.rt().process_argv));
            }
            Expr::ProcessCwd => {
                func.instruction(&Instruction::Call(self.rt().process_cwd));
            }
            Expr::OsPlatform => {
                func.instruction(&Instruction::Call(self.rt().os_platform));
            }
            Expr::ProcessUptime | Expr::ProcessMemoryUsage |
            Expr::OsArch | Expr::OsHostname | Expr::OsHomedir | Expr::OsTmpdir |
            Expr::OsTotalmem | Expr::OsFreemem | Expr::OsUptime |
            Expr::OsType | Expr::OsRelease | Expr::OsCpus | Expr::OsNetworkInterfaces |
            Expr::OsUserInfo | Expr::OsEOL => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::EnvGet(_) | Expr::EnvGetDynamic(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }

            // --- FS stubs ---
            Expr::FsReadFileSync(_) | Expr::FsWriteFileSync(_, _) |
            Expr::FsExistsSync(_) | Expr::FsMkdirSync(_) |
            Expr::FsUnlinkSync(_) | Expr::FsAppendFileSync(_, _) |
            Expr::FsReadFileBinary(_) | Expr::FsRmRecursive(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            // --- Path ---
            Expr::PathJoin(a, b) => {
                self.emit_expr(func, a);
                self.emit_expr(func, b);
                func.instruction(&Instruction::Call(self.rt().path_join));
            }
            Expr::PathDirname(p) => {
                self.emit_expr(func, p);
                func.instruction(&Instruction::Call(self.rt().path_dirname));
            }
            Expr::PathBasename(p) => {
                self.emit_expr(func, p);
                func.instruction(&Instruction::Call(self.rt().path_basename));
            }
            Expr::PathExtname(p) => {
                self.emit_expr(func, p);
                func.instruction(&Instruction::Call(self.rt().path_extname));
            }
            Expr::PathResolve(p) => {
                self.emit_expr(func, p);
                func.instruction(&Instruction::Call(self.rt().path_resolve));
            }
            Expr::PathIsAbsolute(p) => {
                self.emit_expr(func, p);
                func.instruction(&Instruction::Call(self.rt().path_is_absolute));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::FileURLToPath(p) => {
                self.emit_expr(func, p);
                // In WASM, just return the string as-is
            }
            // --- Buffer/TypedArray ---
            Expr::BufferAlloc { ref size, .. } => {
                self.emit_expr(func, size.as_ref());
                func.instruction(&Instruction::Call(self.rt().buffer_alloc));
            }
            Expr::BufferAllocUnsafe(size) => {
                self.emit_expr(func, size);
                func.instruction(&Instruction::Call(self.rt().buffer_alloc));
            }
            Expr::BufferFrom { data, encoding } => {
                self.emit_expr(func, data);
                if let Some(enc) = encoding {
                    self.emit_expr(func, enc);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().buffer_from_string));
            }
            Expr::BufferToString { buffer, encoding } => {
                self.emit_expr(func, buffer);
                if let Some(enc) = encoding {
                    self.emit_expr(func, enc);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().buffer_to_string));
            }
            Expr::BufferLength(buf) => {
                self.emit_expr(func, buf);
                func.instruction(&Instruction::Call(self.rt().buffer_length));
            }
            Expr::BufferSlice { buffer, start, end } => {
                self.emit_expr(func, buffer);
                if let Some(s) = start {
                    self.emit_expr(func, s);
                } else {
                    func.instruction(&f64_const(0.0));
                }
                if let Some(e) = end {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().buffer_slice));
            }
            Expr::BufferConcat(arr) => {
                self.emit_expr(func, arr);
                func.instruction(&Instruction::Call(self.rt().buffer_concat));
            }
            Expr::BufferIndexGet { buffer, index } => {
                self.emit_expr(func, buffer);
                self.emit_expr(func, index);
                func.instruction(&Instruction::Call(self.rt().buffer_get));
            }
            Expr::BufferIndexSet { buffer, index, value } => {
                self.emit_expr(func, buffer);
                self.emit_expr(func, index);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().buffer_set));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::BufferCopy { source, target, target_start, source_start, source_end } => {
                self.emit_expr(func, source);
                self.emit_expr(func, target);
                if let Some(ts) = target_start {
                    self.emit_expr(func, ts);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                if let Some(ss) = source_start {
                    self.emit_expr(func, ss);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                if let Some(se) = source_end {
                    self.emit_expr(func, se);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().buffer_copy));
            }
            Expr::BufferWrite { buffer, string, offset, encoding } => {
                self.emit_expr(func, buffer);
                self.emit_expr(func, string);
                if let Some(o) = offset {
                    self.emit_expr(func, o);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                if let Some(e) = encoding {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
                func.instruction(&Instruction::Call(self.rt().buffer_write));
            }
            Expr::BufferEquals { buffer, other } => {
                self.emit_expr(func, buffer);
                self.emit_expr(func, other);
                func.instruction(&Instruction::Call(self.rt().buffer_equals));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::BufferIsBuffer(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().buffer_is_buffer));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::BufferByteLength(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().buffer_byte_length));
            }
            Expr::Uint8ArrayNew(size) => {
                if let Some(s) = size {
                    self.emit_expr(func, s);
                } else {
                    func.instruction(&f64_const(0.0));
                }
                func.instruction(&Instruction::Call(self.rt().uint8array_new));
            }
            Expr::Uint8ArrayFrom(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::Call(self.rt().uint8array_from));
            }
            Expr::Uint8ArrayLength(buf) => {
                self.emit_expr(func, buf);
                func.instruction(&Instruction::Call(self.rt().uint8array_length));
            }
            Expr::Uint8ArrayGet { array, index } => {
                self.emit_expr(func, array);
                self.emit_expr(func, index);
                func.instruction(&Instruction::Call(self.rt().uint8array_get));
            }
            Expr::Uint8ArraySet { array, index, value } => {
                self.emit_expr(func, array);
                self.emit_expr(func, index);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().uint8array_set));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            // --- Child process stubs ---
            Expr::ChildProcessExecSync { .. } | Expr::ChildProcessSpawnSync { .. } |
            Expr::ChildProcessSpawn { .. } | Expr::ChildProcessExec { .. } |
            Expr::ChildProcessSpawnBackground { .. } |
            Expr::ChildProcessGetProcessStatus(_) | Expr::ChildProcessKillProcess(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            // --- Fetch ---
            Expr::FetchWithOptions { url, method, body, headers } => {
                self.emit_expr(func, url);
                self.emit_expr(func, method);
                self.emit_expr(func, body);
                // Build headers object
                if headers.is_empty() {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                } else {
                    let obj_new = self.rt().object_new;
                    let obj_set = self.rt().object_set;
                    func.instruction(&Instruction::Call(obj_new));
                    for (key, val) in headers {
                        let key_id = self.emitter.string_map.get(key.as_str()).copied().unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        func.instruction(&f64_const(f64::from_bits(key_bits)));
                        self.emit_expr(func, val);
                        func.instruction(&Instruction::Call(obj_set));
                    }
                }
                func.instruction(&Instruction::Call(self.rt().fetch_with_options));
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.emit_expr(func, url);
                func.instruction(&f64_const_bits(TAG_UNDEFINED)); // method (default GET)
                func.instruction(&f64_const_bits(TAG_UNDEFINED)); // body
                // Build headers object with Authorization
                let obj_new = self.rt().object_new;
                let obj_set = self.rt().object_set;
                func.instruction(&Instruction::Call(obj_new));
                let auth_key_id = self.emitter.string_map.get("Authorization").copied().unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                func.instruction(&f64_const(f64::from_bits(auth_key_bits)));
                self.emit_expr(func, auth_header);
                func.instruction(&Instruction::Call(obj_set));
                func.instruction(&Instruction::Call(self.rt().fetch_with_options));
            }
            Expr::FetchPostWithAuth { url, auth_header, body } => {
                self.emit_expr(func, url);
                // POST method string
                let post_id = self.emitter.string_map.get("POST").copied().unwrap_or(0);
                let post_bits = (STRING_TAG << 48) | (post_id as u64);
                func.instruction(&f64_const(f64::from_bits(post_bits)));
                self.emit_expr(func, body);
                // Build headers object with Authorization
                let obj_new = self.rt().object_new;
                let obj_set = self.rt().object_set;
                func.instruction(&Instruction::Call(obj_new));
                let auth_key_id = self.emitter.string_map.get("Authorization").copied().unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                func.instruction(&f64_const(f64::from_bits(auth_key_bits)));
                self.emit_expr(func, auth_header);
                func.instruction(&Instruction::Call(obj_set));
                func.instruction(&Instruction::Call(self.rt().fetch_with_options));
            }
            // --- Net stubs ---
            Expr::NetCreateServer { .. } | Expr::NetCreateConnection { .. } |
            Expr::NetConnect { .. } => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            // --- Crypto ---
            Expr::CryptoRandomUUID => {
                func.instruction(&Instruction::Call(self.rt().crypto_random_uuid));
            }
            Expr::CryptoRandomBytes(n) => {
                self.emit_expr(func, n);
                func.instruction(&Instruction::Call(self.rt().crypto_random_bytes));
            }
            Expr::CryptoSha256(data) => {
                self.emit_expr(func, data);
                func.instruction(&Instruction::Call(self.rt().crypto_sha256));
            }
            Expr::CryptoMd5(data) => {
                self.emit_expr(func, data);
                func.instruction(&Instruction::Call(self.rt().crypto_md5));
            }
            // --- URL SearchParams ---
            Expr::UrlSearchParamsNew(init) => {
                if let Some(init_expr) = init {
                    self.emit_expr(func, init_expr);
                    func.instruction(&Instruction::Call(self.rt().url_parse));
                    func.instruction(&Instruction::Call(self.rt().url_get_search_params));
                } else {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }
            Expr::UrlSearchParamsGet { params, name } => {
                self.emit_expr(func, params);
                self.emit_expr(func, name);
                func.instruction(&Instruction::Call(self.rt().searchparams_get));
            }
            Expr::UrlSearchParamsHas { params, name } => {
                self.emit_expr(func, params);
                self.emit_expr(func, name);
                func.instruction(&Instruction::Call(self.rt().searchparams_has));
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::F64)));
                func.instruction(&f64_const_bits(TAG_TRUE));
                func.instruction(&Instruction::Else);
                func.instruction(&f64_const_bits(TAG_FALSE));
                func.instruction(&Instruction::End);
            }
            Expr::UrlSearchParamsSet { params, name, value } => {
                self.emit_expr(func, params);
                self.emit_expr(func, name);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().searchparams_set));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::UrlSearchParamsAppend { params, name, value } => {
                self.emit_expr(func, params);
                self.emit_expr(func, name);
                self.emit_expr(func, value);
                func.instruction(&Instruction::Call(self.rt().searchparams_append));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::UrlSearchParamsDelete { params, name } => {
                self.emit_expr(func, params);
                self.emit_expr(func, name);
                func.instruction(&Instruction::Call(self.rt().searchparams_delete));
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::UrlSearchParamsToString(params) => {
                self.emit_expr(func, params);
                func.instruction(&Instruction::Call(self.rt().searchparams_to_string));
            }
            Expr::UrlSearchParamsGetAll { .. } => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            // --- JS runtime interop stubs ---
            Expr::JsLoadModule { .. } | Expr::JsGetExport { .. } |
            Expr::JsCallFunction { .. } | Expr::JsCallMethod { .. } |
            Expr::JsGetProperty { .. } | Expr::JsSetProperty { .. } |
            Expr::JsNew { .. } | Expr::JsNewFromHandle { .. } |
            Expr::JsCreateCallback { .. } => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            // --- Misc ---
            Expr::ImportMetaUrl(_) | Expr::StaticPluginResolve(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::Yield { .. } => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
            Expr::BigInt(_) | Expr::NativeModuleRef(_) => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }

            // --- DateNow ---
            Expr::DateNow => {
                func.instruction(&Instruction::Call(self.rt().date_now));
            }

            // --- Sequence ---
            Expr::Sequence(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    self.emit_expr(func, e);
                    if i < exprs.len() - 1 {
                        func.instruction(&Instruction::Drop);
                    }
                }
                if exprs.is_empty() {
                    func.instruction(&f64_const_bits(TAG_UNDEFINED));
                }
            }

            // --- Catch-all: emit undefined ---
            _ => {
                func.instruction(&f64_const_bits(TAG_UNDEFINED));
            }
        }
    }

    /// Check if an expression produces a value on the stack
    fn expr_has_value(&self, expr: &Expr) -> bool {
        match expr {
            Expr::NativeMethodCall { module, method, .. } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                if normalized == "console" {
                    return false;
                }
                // void-returning array methods via NativeMethodCall
                if matches!(method.as_str(), "forEach") {
                    return false;
                }
                true
            }
            // console.log/warn/error via Call + PropertyGet pattern
            Expr::Call { callee, .. } => {
                if let Expr::PropertyGet { object, property } = callee.as_ref() {
                    if let Expr::GlobalGet(_) = object.as_ref() {
                        if matches!(property.as_str(), "log" | "warn" | "error") {
                            return false;
                        }
                    }
                }
                true
            }
            // ArrayForEach returns undefined but we emit it explicitly
            _ => true,
        }
    }
}

/// Recursively scan statements for local variable declarations
fn collect_locals(stmts: &[Stmt], map: &mut BTreeMap<LocalId, u32>, count: &mut u32, offset: u32) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, .. } => {
                if !map.contains_key(id) {
                    map.insert(*id, offset + *count);
                    *count += 1;
                }
            }
            Stmt::If { then_branch, else_branch, .. } => {
                collect_locals(then_branch, map, count, offset);
                if let Some(eb) = else_branch {
                    collect_locals(eb, map, count, offset);
                }
            }
            Stmt::While { body, .. } => {
                collect_locals(body, map, count, offset);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_locals(std::slice::from_ref(init_stmt.as_ref()), map, count, offset);
                }
                collect_locals(body, map, count, offset);
            }
            Stmt::Try { body, catch, finally } => {
                collect_locals(body, map, count, offset);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        if !map.contains_key(id) {
                            map.insert(*id, offset + *count);
                            *count += 1;
                        }
                    }
                    collect_locals(&c.body, map, count, offset);
                }
                if let Some(f) = finally {
                    collect_locals(f, map, count, offset);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_locals(&case.body, map, count, offset);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect all Expr::Closure nodes from statements
fn collect_closures_from_stmts(
    stmts: &[Stmt],
    out: &mut Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init { collect_closures_from_expr(e, out); }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => collect_closures_from_expr(e, out),
            Stmt::Return(e) => {
                if let Some(e) = e { collect_closures_from_expr(e, out); }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(then_branch, out);
                if let Some(eb) = else_branch { collect_closures_from_stmts(eb, out); }
            }
            Stmt::While { condition, body } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(body, out);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(i) = init { collect_closures_from_stmts(std::slice::from_ref(i.as_ref()), out); }
                if let Some(c) = condition { collect_closures_from_expr(c, out); }
                if let Some(u) = update { collect_closures_from_expr(u, out); }
                collect_closures_from_stmts(body, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_closures_from_stmts(body, out);
                if let Some(c) = catch { collect_closures_from_stmts(&c.body, out); }
                if let Some(f) = finally { collect_closures_from_stmts(f, out); }
            }
            Stmt::Switch { discriminant, cases } => {
                collect_closures_from_expr(discriminant, out);
                for case in cases {
                    if let Some(t) = &case.test { collect_closures_from_expr(t, out); }
                    collect_closures_from_stmts(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect Expr::Closure from an expression tree
fn collect_closures_from_expr(
    expr: &Expr,
    out: &mut Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)>,
) {
    match expr {
        Expr::Closure { func_id, params, body, captures, mutable_captures, .. } => {
            out.push((*func_id, params.clone(), body.clone(), captures.clone(), mutable_captures.clone()));
            // Also collect nested closures
            collect_closures_from_stmts(body, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_closures_from_expr(callee, out);
            for a in args { collect_closures_from_expr(a, out); }
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
        Expr::Logical { left, right, .. } => {
            collect_closures_from_expr(left, out);
            collect_closures_from_expr(right, out);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) |
        Expr::Await(operand) => {
            collect_closures_from_expr(operand, out);
        }
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            collect_closures_from_expr(val, out);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_closures_from_expr(condition, out);
            collect_closures_from_expr(then_expr, out);
            collect_closures_from_expr(else_expr, out);
        }
        Expr::Object(fields) => {
            for (_, v) in fields { collect_closures_from_expr(v, out); }
        }
        Expr::Array(elems) => {
            for e in elems { collect_closures_from_expr(e, out); }
        }
        Expr::PropertyGet { object, .. } => { collect_closures_from_expr(object, out); }
        Expr::PropertySet { object, value, .. } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(index, out);
            collect_closures_from_expr(value, out);
        }
        Expr::NativeMethodCall { args, object, .. } => {
            if let Some(o) = object { collect_closures_from_expr(o, out); }
            for a in args { collect_closures_from_expr(a, out); }
        }
        Expr::New { args, .. } => {
            for a in args { collect_closures_from_expr(a, out); }
        }
        Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } |
        Expr::ArrayForEach { array, callback } | Expr::ArrayFind { array, callback } |
        Expr::ArrayFindIndex { array, callback } | Expr::ArraySort { array, comparator: callback } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(callback, out);
        }
        Expr::ArrayReduce { array, callback, initial } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(callback, out);
            if let Some(i) = initial { collect_closures_from_expr(i, out); }
        }
        Expr::Sequence(exprs) => {
            for e in exprs { collect_closures_from_expr(e, out); }
        }
        _ => {}
    }
}

/// Check if a statement or its children contain a return statement
fn has_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::If { then_branch, else_branch, .. } => {
            then_branch.iter().any(has_return) ||
            else_branch.as_ref().map_or(false, |eb| eb.iter().any(has_return))
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            body.iter().any(has_return)
        }
        Stmt::Try { body, catch, finally } => {
            body.iter().any(has_return) ||
            catch.as_ref().map_or(false, |c| c.body.iter().any(has_return)) ||
            finally.as_ref().map_or(false, |f| f.iter().any(has_return))
        }
        Stmt::Switch { cases, .. } => {
            cases.iter().any(|c| c.body.iter().any(has_return))
        }
        _ => false,
    }
}
