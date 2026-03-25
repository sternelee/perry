//! Utility functions and thread-local state for codegen.

use cranelift::prelude::*;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use cranelift_object::ObjectModule;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UnaryOp, LogicalOp};
use perry_types::LocalId;

use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext};
use crate::expr::compile_expr;

/// Global counter for assigning unique init guard IDs to non-entry modules.
/// Each non-entry module gets a sequential ID used with perry_init_guard_check_and_set().
pub(crate) static INIT_MODULE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Thread-local tracking of the current function being compiled (for self-recursive call optimization)
thread_local! {
    pub(crate) static CURRENT_FUNC_HIR_ID: Cell<Option<u32>> = Cell::new(None);
    /// Import module prefixes: maps imported name -> source module's scoped prefix.
    /// Set at the start of compile_module, used by compile_expr for scoped symbol lookup.
    pub(crate) static IMPORT_MODULE_PREFIXES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// Namespace import names: set of local names that are namespace imports (import * as X).
    /// Used by compile_expr to intercept PropertyGet(ExternFuncRef { name: X }, prop).
    pub(crate) static NAMESPACE_IMPORTS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Imported function return types: maps function name -> HIR return type.
    /// Used by compile_stmt to resolve types for await expressions on cross-module async calls.
    pub(crate) static IMPORTED_FUNC_RETURN_TYPES: RefCell<HashMap<String, perry_types::Type>> = RefCell::new(HashMap::new());
    /// Maps local import name -> full scoped export name for imports where local != export name.
    /// E.g., `import bs58 from 'bs58'` maps "bs58" -> "__export_{bs58_prefix}__default".
    pub(crate) static IMPORT_LOCAL_TO_SCOPED: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// Track try-catch nesting depth during codegen, so return/break/continue
    /// can emit the right number of js_try_end() calls before jumping out.
    pub(crate) static TRY_CATCH_DEPTH: Cell<usize> = Cell::new(0);
    /// Compile-time platform target for the `__platform__` built-in constant.
    /// 0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux.
    pub(crate) static COMPILE_TARGET: Cell<i64> = Cell::new(0);
    /// Compile-time feature flags. Set from --features CLI flag.
    /// Used to inject `__plugins__` and `__feature_NAME__` constants.
    pub(crate) static ENABLED_FEATURES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Current static method's class name, so `this.method()` inside static methods
    /// can be resolved to direct static method calls on the same class.
    pub(crate) static CURRENT_STATIC_CLASS_NAME: RefCell<Option<String>> = RefCell::new(None);
    /// i18n string table set from compile.rs before compiling each module.
    /// Contains all translations for lookup during I18nString codegen.
    pub(crate) static I18N_TABLE: RefCell<I18nCodegenTable> = RefCell::new(I18nCodegenTable::empty());
    /// i18n locale codes (e.g., ["en", "de", "fr"]) — set from compile.rs, read by module_init.rs.
    pub(crate) static I18N_LOCALE_CODES: RefCell<Vec<String>> = RefCell::new(Vec::new());
    /// Module-scoped variables whose global write-back is deferred inside a simple loop
    /// (no function calls that could observe the global). The write-back is flushed after loop exit.
    pub(crate) static DEFERRED_MODULE_WRITEBACK_VARS: RefCell<HashSet<LocalId>> = RefCell::new(HashSet::new());
}

/// Lightweight i18n table data for codegen thread-local access.
#[derive(Clone)]
pub(crate) struct I18nCodegenTable {
    pub locale_count: usize,
    pub key_count: usize,
    /// Flat array: translations[locale_idx * key_count + string_idx]
    pub translations: Vec<String>,
}

impl I18nCodegenTable {
    pub const fn empty() -> Self {
        Self { locale_count: 0, key_count: 0, translations: Vec::new() }
    }
}

/// Global counter for generating unique temporary variable IDs
pub(crate) static TEMP_VAR_COUNTER: AtomicUsize = AtomicUsize::new(10000);

/// Get a unique temporary variable ID
pub(crate) fn next_temp_var_id() -> usize {
    TEMP_VAR_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Construct a scoped export name for an imported symbol, using the thread-local mapping.
pub(crate) fn tl_scoped_export_name(name: &str) -> String {
    // First check the local-to-scoped map (for imports where local name != export name,
    // e.g., `import bs58 from 'bs58'` where local "bs58" maps to "__export_{prefix}__default")
    let local_scoped = IMPORT_LOCAL_TO_SCOPED.with(|p| {
        p.borrow().get(name).cloned()
    });
    if let Some(scoped) = local_scoped {
        return scoped;
    }
    IMPORT_MODULE_PREFIXES.with(|p| {
        let map = p.borrow();
        if let Some(prefix) = map.get(name) {
            format!("__export_{}__{}", prefix, name)
        } else {
            format!("__export_{}", name)
        }
    })
}

/// Check if a name is a namespace import (import * as X from './module')
pub(crate) fn tl_is_namespace_import(name: &str) -> bool {
    NAMESPACE_IMPORTS.with(|p| p.borrow().contains(name))
}

/// Global counter for generating unique regex data IDs
pub(crate) static REGEX_DATA_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Get a unique regex data ID
pub(crate) fn next_regex_data_id() -> usize {
    REGEX_DATA_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Global counter for generating unique JS interop data IDs
pub(crate) static JS_INTEROP_DATA_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Get a unique JS interop data ID
pub(crate) fn next_js_data_id() -> usize {
    JS_INTEROP_DATA_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Resolve class_name from a type annotation by checking if the type refers to a known class
pub(crate) fn resolve_class_name_from_type(ty: &perry_types::Type, classes: &HashMap<String, ClassMeta>) -> Option<String> {
    match ty {
        perry_types::Type::Named(name) => {
            if classes.contains_key(name) {
                Some(name.clone())
            } else {
                None
            }
        }
        perry_types::Type::Union(types) => {
            types.iter().find_map(|t| {
                if let perry_types::Type::Named(name) = t {
                    if classes.contains_key(name) {
                        Some(name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        }
        _ => None,
    }
}

/// Check if a block has been filled with a terminating instruction
pub(crate) fn is_block_filled(builder: &FunctionBuilder, block: cranelift_codegen::ir::Block) -> bool {
    if let Some(inst) = builder.func.layout.last_inst(block) {
        builder.func.dfg.insts[inst].opcode().is_terminator()
    } else {
        false
    }
}

/// Convert a HIR Type to a Cranelift ABI type (standalone version)
pub(crate) fn type_to_cranelift_abi(ty: &perry_types::Type) -> types::Type {
    use perry_types::Type;
    match ty {
        // Numbers use f64
        Type::Number | Type::Int32 | Type::BigInt => types::F64,
        // Booleans can be f64 (0.0 or 1.0) for simplicity
        Type::Boolean => types::F64,
        // Strings, arrays, objects, promises are pointers (i64)
        Type::String | Type::Array(_) | Type::Object(_) |
        Type::Promise(_) | Type::Named(_) | Type::Generic { .. } => types::I64,
        // Void/Null/Undefined return f64 (will be 0)
        Type::Void | Type::Null => types::F64,
        // Any/Unknown use i64 (tagged values or pointers)
        Type::Any | Type::Unknown => types::I64,
        // Functions are pointers
        Type::Function(_) => types::I64,
        // Tuples use i64 (could be more complex)
        Type::Tuple(_) => types::I64,
        // Union types use f64 (NaN-boxed values can be numbers or pointers)
        Type::Union(_) => types::F64,
        // Never type - use f64 as fallback (never actually returned)
        Type::Never => types::F64,
        // TypeVar should be substituted before codegen; default to f64
        Type::TypeVar(_) => types::F64,
        // Symbol is an i64 id
        Type::Symbol => types::I64,
    }
}

/// Convert a Cranelift value to i64, bitcasting from f64 only if needed.
/// This is used when values that are logically pointers (strings, arrays, objects)
/// need to be passed to runtime functions that expect i64.
pub(crate) fn ensure_i64(builder: &mut FunctionBuilder, val: Value) -> Value {
    let val_type = builder.func.dfg.value_type(val);
    if val_type == types::I64 {
        val
    } else if val_type == types::I32 {
        // Extend i32 to i64
        builder.ins().uextend(types::I64, val)
    } else {
        // F64 NaN-boxed pointer - strip the tag bits (top 16 bits)
        // to get the raw pointer address. Preserve JS_HANDLE_TAG (0x7FFB)
        // for V8 handles. Return 0 for null/undefined/boolean values
        // (TAG_NULL=0x7FFC...0002 → masked=0x2, which is not a valid pointer).
        let val_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), val);
        let mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFi64);
        let masked = builder.ins().band(val_i64, mask);
        // Guard: small values (< 0x1000) are TAG_NULL, TAG_UNDEFINED, booleans.
        // Return 0 (null) for these — callers must null-check before dereferencing.
        // BUT: NaN-boxed pointers (POINTER_TAG 0x7FFD, STRING_TAG 0x7FFF,
        // JS_HANDLE_TAG 0x7FFB, BIGINT_TAG 0x7FFE) with small payloads must
        // NOT be zeroed — extract their lower 48 bits.
        let top16 = builder.ins().ushr_imm(val_i64, 48);
        // Check if top16 is in the NaN-box tag range (0x7FF0..0xFFFF)
        let nan_threshold = builder.ins().iconst(types::I64, 0x7FF0i64);
        let is_nanboxed = builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, top16, nan_threshold);
        // For NaN-boxed values: return lower 48 bits (the payload/pointer)
        // For non-NaN-boxed values: return the masked value with small-value guard
        let threshold = builder.ins().iconst(types::I64, 0x1000i64);
        let zero = builder.ins().iconst(types::I64, 0i64);
        let is_small = builder.ins().icmp(IntCC::UnsignedLessThan, masked, threshold);
        let safe_masked = builder.ins().select(is_small, zero, masked);
        builder.ins().select(is_nanboxed, masked, safe_masked)
    }
}

/// Convert a Cranelift value to f64, bitcasting from i64 only if needed.
/// This is used when values need to be stored uniformly as f64 or passed to
/// JS interop functions that expect NaN-boxed values.
pub(crate) fn ensure_f64(builder: &mut FunctionBuilder, val: Value) -> Value {
    let val_type = builder.func.dfg.value_type(val);
    if val_type == types::F64 {
        val
    } else if val_type == types::I32 {
        // Convert i32 to f64 (as a number, not bitcast)
        builder.ins().fcvt_from_sint(types::F64, val)
    } else {
        builder.ins().bitcast(types::F64, MemFlags::new(), val)
    }
}

/// Convert NaN-boxed booleans to their numeric equivalents for arithmetic.
/// TAG_TRUE (0x7FFC_0000_0000_0004) -> 1.0, TAG_FALSE (0x7FFC_0000_0000_0003) -> 0.0.
/// Non-boolean values pass through unchanged. This is branchless (bitcast + 2 icmp + 2 select).
pub(crate) fn unbox_bool_to_number(builder: &mut FunctionBuilder, val: Value) -> Value {
    let bits = builder.ins().bitcast(types::I64, MemFlags::new(), val);
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let is_tag_true = builder.ins().icmp_imm(IntCC::Equal, bits, TAG_TRUE as i64);
    let is_tag_false = builder.ins().icmp_imm(IntCC::Equal, bits, TAG_FALSE as i64);
    let is_bool = builder.ins().bor(is_tag_true, is_tag_false);
    let one = builder.ins().f64const(1.0);
    let zero = builder.ins().f64const(0.0);
    let numeric = builder.ins().select(is_tag_true, one, zero);
    builder.ins().select(is_bool, numeric, val)
}

/// Inline truthiness check: returns I8 bool (1=truthy, 0=falsy) without FFI.
/// Covers falsy values: undefined, null, false (NaN-box tags within 2 of TAG_UNDEFINED),
/// ±0.0 (bit pattern with all-zero mantissa+exponent after shifting out sign),
/// NaN (quiet NaN bit pattern), "" (empty string via STRING_TAG + js_string_length),
/// and BigInt 0n (BIGINT_TAG with all-zero limbs, checked via js_bigint_is_zero).
pub(crate) fn inline_truthiness_check(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    extern_funcs: &HashMap<Cow<'static, str>, cranelift_module::FuncId>,
    val: Value,
) -> Value {
    let val_f64 = ensure_f64(builder, val);
    let val_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), val_f64);
    // Check falsy NaN-box tags: TAG_UNDEFINED(01), TAG_NULL(02), TAG_FALSE(03)
    // (val - TAG_UNDEFINED) <=u 2 covers all three
    let tag_base = builder.ins().iconst(types::I64, 0x7FFC_0000_0000_0001u64 as i64);
    let sub = builder.ins().isub(val_i64, tag_base);
    let two = builder.ins().iconst(types::I64, 2);
    let is_falsy_tag = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, sub, two);
    // Check ±0.0: (val << 1) == 0
    let shifted = builder.ins().ishl_imm(val_i64, 1);
    let zero_i64 = builder.ins().iconst(types::I64, 0);
    let is_zero_float = builder.ins().icmp(IntCC::Equal, shifted, zero_i64);

    // Check BigInt 0n: extract tag bits, if BIGINT_TAG call js_bigint_is_zero
    let tag = builder.ins().ushr_imm(val_i64, 48);
    let bigint_tag_val = builder.ins().iconst(types::I64, 0x7FFA);
    let is_bigint = builder.ins().icmp(IntCC::Equal, tag, bigint_tag_val);

    let bigint_block = builder.create_block();
    let non_bigint_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I8); // is_falsy result

    builder.ins().brif(is_bigint, bigint_block, &[], non_bigint_block, &[]);

    // BigInt block: extract pointer and call js_bigint_is_zero
    builder.switch_to_block(bigint_block);
    builder.seal_block(bigint_block);
    let pointer_mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFu64 as i64);
    let ptr = builder.ins().band(val_i64, pointer_mask);
    let is_zero_func = extern_funcs.get("js_bigint_is_zero")
        .expect("js_bigint_is_zero not declared");
    let is_zero_ref = module.declare_func_in_func(*is_zero_func, builder.func);
    let call = builder.ins().call(is_zero_ref, &[ptr]);
    let is_zero_result = builder.inst_results(call)[0]; // i32: 1=zero, 0=non-zero
    let is_bigint_falsy = builder.ins().ireduce(types::I8, is_zero_result);
    builder.ins().jump(merge_block, &[is_bigint_falsy.into()]);

    // Non-BigInt block: use existing tag/float checks + NaN + empty string
    builder.switch_to_block(non_bigint_block);
    builder.seal_block(non_bigint_block);
    let is_falsy_non_bigint = builder.ins().bor(is_falsy_tag, is_zero_float);

    // Check NaN: quiet NaN bit pattern 0x7FF8_0000_0000_0000
    let nan_bits = builder.ins().iconst(types::I64, 0x7FF8_0000_0000_0000u64 as i64);
    let is_nan = builder.ins().icmp(IntCC::Equal, val_i64, nan_bits);
    let is_falsy_with_nan = builder.ins().bor(is_falsy_non_bigint, is_nan);

    // Check empty string: STRING_TAG (0x7FFF) with length == 0
    let string_tag_val = builder.ins().iconst(types::I64, 0x7FFF);
    let is_string_tag = builder.ins().icmp(IntCC::Equal, tag, string_tag_val);

    let string_check_block = builder.create_block();
    let non_string_block = builder.create_block();
    let string_merge_block = builder.create_block();
    builder.append_block_param(string_merge_block, types::I8);

    builder.ins().brif(is_string_tag, string_check_block, &[], non_string_block, &[]);

    // String block: extract pointer and check length
    builder.switch_to_block(string_check_block);
    builder.seal_block(string_check_block);
    let pointer_mask2 = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFu64 as i64);
    let str_ptr = builder.ins().band(val_i64, pointer_mask2);
    let strlen_func = extern_funcs.get("js_string_length")
        .expect("js_string_length not declared");
    let strlen_ref = module.declare_func_in_func(*strlen_func, builder.func);
    let call = builder.ins().call(strlen_ref, &[str_ptr]);
    let str_len = builder.inst_results(call)[0]; // u32
    let zero_u32 = builder.ins().iconst(types::I32, 0);
    let is_empty_string = builder.ins().icmp(IntCC::Equal, str_len, zero_u32); // i8
    builder.ins().jump(string_merge_block, &[is_empty_string.into()]);

    // Non-string block: use NaN + tag + float checks
    builder.switch_to_block(non_string_block);
    builder.seal_block(non_string_block);
    builder.ins().jump(string_merge_block, &[is_falsy_with_nan.into()]);

    // String merge block
    builder.switch_to_block(string_merge_block);
    builder.seal_block(string_merge_block);
    let is_falsy_final = builder.block_params(string_merge_block)[0];
    builder.ins().jump(merge_block, &[is_falsy_final.into()]);

    // Merge block
    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    let is_falsy = builder.block_params(merge_block)[0];

    // Invert to get truthy
    let one_i8 = builder.ins().iconst(types::I8, 1);
    builder.ins().bxor(is_falsy, one_i8)
}

/// Inline NaN-box a string pointer with STRING_TAG.
/// Equivalent to `js_nanbox_string(ptr)` but avoids FFI overhead.
/// ptr must be I64, returns F64.
pub(crate) fn inline_nanbox_string(builder: &mut FunctionBuilder, ptr: Value) -> Value {
    let mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFu64 as i64);
    let masked = builder.ins().band(ptr, mask);
    let tag = builder.ins().iconst(types::I64, 0x7FFF_0000_0000_0000u64 as i64);
    let tagged = builder.ins().bor(masked, tag);
    builder.ins().bitcast(types::F64, MemFlags::new(), tagged)
}

/// Inline extract raw pointer from NaN-boxed string.
/// Equivalent to `js_get_string_pointer_unified(val)` for values known to be NaN-boxed strings.
/// val must be F64, returns I64.
pub(crate) fn inline_get_string_pointer(builder: &mut FunctionBuilder, val: Value) -> Value {
    // For reliability, just mask the lower 48 bits. Don't guard against
    // small values — callers should check for null before dereferencing.
    let val_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), val);
    let mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFi64);
    builder.ins().band(val_i64, mask)
}

/// Get a raw string pointer from a value that may be either:
/// - I64: already a raw pointer, use directly
/// - F64: NaN-boxed string, strip the tag to get the raw pointer
pub(crate) fn get_raw_string_ptr(builder: &mut FunctionBuilder, val: Value) -> Value {
    let val_type = builder.func.dfg.value_type(val);
    if val_type == types::I64 {
        val
    } else {
        // F64 NaN-boxed string — strip the tag bits
        inline_get_string_pointer(builder, val)
    }
}

/// Returns a short string describing the HIR Expr variant for diagnostic purposes.
pub(crate) fn expr_type_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Closure { .. } => "Closure",
        Expr::LocalGet(_) => "LocalGet",
        Expr::Call { .. } => "Call",
        Expr::Binary { .. } => "Binary",
        Expr::Logical { .. } => "Logical",
        Expr::Conditional { .. } => "Conditional",
        Expr::PropertyGet { .. } => "PropertyGet",
        Expr::Object(_) => "Object",
        Expr::ObjectSpread { .. } => "ObjectSpread",
        Expr::Array(_) => "Array",
        Expr::MapNew => "MapNew",
        Expr::SetNew | Expr::SetNewFromArray(_) => "SetNew",
        Expr::New { .. } => "New",
        Expr::Await(_) => "Await",
        Expr::FuncRef(_) => "FuncRef",
        Expr::ExternFuncRef { .. } => "ExternFuncRef",
        Expr::NativeMethodCall { .. } => "NativeMethodCall",
        Expr::BigInt(_) => "BigInt",
        Expr::Integer(_) => "Integer",
        Expr::String(_) => "String",
        Expr::Undefined => "Undefined",
        Expr::Null => "Null",
        Expr::This => "This",
        _ => "Other",
    }
}

/// Inline NaN-box a pointer with POINTER_TAG (0x7FFD).
/// ptr must be I64, returns F64.
pub(crate) fn inline_nanbox_pointer(builder: &mut FunctionBuilder, ptr: Value) -> Value {
    // Guard: null pointer (ptr == 0) → TAG_NULL (JS null) instead of null POINTER_TAG.
    let zero_i64 = builder.ins().iconst(types::I64, 0);
    let is_null = builder.ins().icmp(IntCC::Equal, ptr, zero_i64);

    // Guard: if value already has a NaN-box tag (top 16 bits >= 0x7FF8),
    // preserve it via bitcast — e.g., JS_HANDLE_TAG (0x7FFB), STRING_TAG (0x7FFF).
    let top16 = builder.ins().ushr_imm(ptr, 48);
    let threshold = builder.ins().iconst(types::I64, 0x7FF8);
    let already_tagged = builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, top16, threshold);
    let already_nanboxed = builder.ins().bitcast(types::F64, MemFlags::new(), ptr);

    // Normal path: mask lower 48 bits and add POINTER_TAG
    let mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFu64 as i64);
    let masked = builder.ins().band(ptr, mask);
    let tag = builder.ins().iconst(types::I64, 0x7FFD_0000_0000_0000u64 as i64);
    let tagged = builder.ins().bor(masked, tag);
    let ptr_nanboxed = builder.ins().bitcast(types::F64, MemFlags::new(), tagged);

    // TAG_NULL = 0x7FFC_0000_0000_0002
    let tag_null_bits = builder.ins().iconst(types::I64, 0x7FFC_0000_0000_0002u64 as i64);
    let tag_null_f64 = builder.ins().bitcast(types::F64, MemFlags::new(), tag_null_bits);

    // Select: null → TAG_NULL, already_tagged → preserve, else → POINTER_TAG
    let non_null_result = builder.ins().select(already_tagged, already_nanboxed, ptr_nanboxed);
    builder.ins().select(is_null, tag_null_f64, non_null_result)
}

pub(crate) fn inline_nanbox_bigint(builder: &mut FunctionBuilder, ptr: Value) -> Value {
    let zero_i64 = builder.ins().iconst(types::I64, 0);
    let is_null = builder.ins().icmp(IntCC::Equal, ptr, zero_i64);
    let mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFu64 as i64);
    let masked = builder.ins().band(ptr, mask);
    let tag = builder.ins().iconst(types::I64, 0x7FFA_0000_0000_0000u64 as i64);
    let tagged = builder.ins().bor(masked, tag);
    let ptr_nanboxed = builder.ins().bitcast(types::F64, MemFlags::new(), tagged);
    let tag_null_bits = builder.ins().iconst(types::I64, 0x7FFC_0000_0000_0002u64 as i64);
    let tag_null_f64 = builder.ins().bitcast(types::F64, MemFlags::new(), tag_null_bits);
    builder.ins().select(is_null, tag_null_f64, ptr_nanboxed)
}

/// Check if all return statements in a function body return BigInt values (new BN(...) or BigInt literals).
/// Used to infer BigInt return types for untyped functions.
pub(crate) fn all_returns_are_bigint(stmts: &[Stmt]) -> bool {
    let mut found_return = false;
    fn check_stmts(stmts: &[Stmt], found: &mut bool) -> bool {
        for stmt in stmts {
            match stmt {
                Stmt::Return(Some(expr)) => {
                    *found = true;
                    if !is_bigint_return_expr(expr) {
                        return false;
                    }
                }
                Stmt::Return(None) => {
                    return false;
                }
                Stmt::If { then_branch, else_branch, .. } => {
                    if !check_stmts(then_branch, found) { return false; }
                    if let Some(body) = else_branch {
                        if !check_stmts(body, found) { return false; }
                    }
                }
                _ => {}
            }
        }
        true
    }
    fn is_bigint_return_expr(expr: &Expr) -> bool {
        match expr {
            Expr::New { class_name, .. } if class_name == "BN" => true,
            Expr::BigInt(_) | Expr::BigIntCoerce(_) => true,
            _ => false,
        }
    }
    check_stmts(stmts, &mut found_return) && found_return
}

/// Compile a condition expression to an I8 bool without FFI calls.
/// Handles Compare (fcmp), Logical And/Or (band/bor), Unary Not, and falls back
/// to inline_truthiness_check for general expressions.
pub(crate) fn compile_condition_to_bool(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    func_ids: &HashMap<u32, cranelift_module::FuncId>,
    closure_func_ids: &HashMap<u32, cranelift_module::FuncId>,
    func_wrapper_ids: &HashMap<u32, cranelift_module::FuncId>,
    extern_funcs: &HashMap<Cow<'static, str>, cranelift_module::FuncId>,
    async_func_ids: &HashSet<u32>,
    classes: &HashMap<String, ClassMeta>,
    enums: &HashMap<(String, String), EnumMemberValue>,
    func_param_types: &HashMap<u32, Vec<types::Type>>,
    func_union_params: &HashMap<u32, Vec<bool>>,
    func_return_types: &HashMap<u32, types::Type>,
    func_hir_return_types: &HashMap<u32, perry_types::Type>,
    func_rest_param_index: &HashMap<u32, usize>,
    imported_func_param_counts: &HashMap<String, usize>,
    locals: &HashMap<LocalId, LocalInfo>,
    expr: &Expr,
    this_ctx: Option<&ThisContext>,
) -> anyhow::Result<Value> {
    match expr {
        // Expr::Compare is handled by the _ fallback below, which calls compile_expr
        // (which has full string/bigint/bool comparison logic) + inline_truthiness_check.
        // Using simple fcmp here would break NaN-boxed string comparisons (NaN != NaN).
        Expr::Logical { op: LogicalOp::And, left, right } => {
            // Short-circuit AND: if left is false, skip right evaluation
            let l = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, left, this_ctx)?;
            let right_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, types::I8);
            let false_val = builder.ins().iconst(types::I8, 0);
            builder.ins().brif(l, right_block, &[], merge_block, &[false_val.into()]);
            builder.switch_to_block(right_block);
            builder.seal_block(right_block);
            let r = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, right, this_ctx)?;
            builder.ins().jump(merge_block, &[r.into()]);
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            Ok(builder.block_params(merge_block)[0])
        }
        Expr::Logical { op: LogicalOp::Or, left, right } => {
            // Short-circuit OR: if left is true, skip right evaluation
            let l = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, left, this_ctx)?;
            let right_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, types::I8);
            let true_val = builder.ins().iconst(types::I8, 1);
            builder.ins().brif(l, merge_block, &[true_val.into()], right_block, &[]);
            builder.switch_to_block(right_block);
            builder.seal_block(right_block);
            let r = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, right, this_ctx)?;
            builder.ins().jump(merge_block, &[r.into()]);
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            Ok(builder.block_params(merge_block)[0])
        }
        Expr::Unary { op: UnaryOp::Not, operand } => {
            let inner = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, operand, this_ctx)?;
            let one_i8 = builder.ins().iconst(types::I8, 1);
            Ok(builder.ins().bxor(inner, one_i8))
        }
        // Fast path: numeric comparisons directly to I8 boolean (no NaN-box round-trip)
        Expr::Compare { op, left, right } => {
            fn is_known_numeric(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
                match expr {
                    Expr::Number(_) | Expr::Integer(_) => true,
                    Expr::Update { .. } => true,
                    Expr::LocalGet(id) => {
                        locals.get(id).map(|info| {
                            !info.is_string && !info.is_bigint && !info.is_pointer
                            && !info.is_union && !info.is_boolean && !info.is_array
                            && !info.is_map && !info.is_set && !info.is_buffer
                            && !info.is_event_emitter && !info.is_closure
                            && info.class_name.is_none()
                        }).unwrap_or(false)
                    }
                    Expr::Binary { op, left, right } => {
                        matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul
                            | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow
                            | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
                            | BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr)
                        && is_known_numeric(left, locals)
                        && is_known_numeric(right, locals)
                    }
                    Expr::Unary { op, operand } => {
                        matches!(op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot)
                        && is_known_numeric(operand, locals)
                    }
                    _ => false,
                }
            }
            if is_known_numeric(left, locals) && is_known_numeric(right, locals) {
                let lhs = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, left, this_ctx)?;
                let rhs = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, right, this_ctx)?;
                let lhs_f64 = ensure_f64(builder, lhs);
                let rhs_f64 = ensure_f64(builder, rhs);
                let float_cc = match op {
                    CompareOp::Eq => FloatCC::Equal,
                    CompareOp::Ne => FloatCC::NotEqual,
                    CompareOp::Lt => FloatCC::LessThan,
                    CompareOp::Le => FloatCC::LessThanOrEqual,
                    CompareOp::Gt => FloatCC::GreaterThan,
                    CompareOp::Ge => FloatCC::GreaterThanOrEqual,
                };
                Ok(builder.ins().fcmp(float_cc, lhs_f64, rhs_f64))
            } else {
                // Fall through to generic path for non-numeric comparisons
                let val = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, expr, this_ctx)?;
                Ok(inline_truthiness_check(builder, module, extern_funcs, val))
            }
        }
        _ => {
            // For string locals, check string length directly (raw i64 pointers lack STRING_TAG)
            if let Expr::LocalGet(id) = expr {
                if let Some(info) = locals.get(id) {
                    if info.is_string && !info.is_union {
                        let str_val = builder.use_var(info.var);
                        let str_ptr = if info.is_pointer {
                            str_val // already i64
                        } else {
                            ensure_i64(builder, str_val)
                        };
                        // Check non-null AND non-empty: ptr != 0 && string_length(ptr) > 0
                        let zero_i64 = builder.ins().iconst(types::I64, 0);
                        let is_non_null = builder.ins().icmp(IntCC::NotEqual, str_ptr, zero_i64);
                        let strlen_func = extern_funcs.get("js_string_length")
                            .expect("js_string_length not declared");
                        let strlen_ref = module.declare_func_in_func(*strlen_func, builder.func);
                        let call = builder.ins().call(strlen_ref, &[str_ptr]);
                        let str_len = builder.inst_results(call)[0]; // i32
                        let is_non_empty = builder.ins().icmp_imm(IntCC::SignedGreaterThan, str_len, 0);
                        let is_truthy = builder.ins().band(is_non_null, is_non_empty);
                        return Ok(is_truthy);
                    }
                }
            }
            let val = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, expr, this_ctx)?;
            Ok(inline_truthiness_check(builder, module, extern_funcs, val))
        }
    }
}

/// Try to compile an index expression entirely in i32 arithmetic.
/// Returns Some(i32_value) if the expression can be computed in i32, None otherwise.
/// This avoids f64 round-trips for array index computations like `i * size + k`.
/// The i32 result is immediately consumed as an array index in IndexGet/IndexSet.
pub(crate) fn try_compile_index_as_i32(
    builder: &mut FunctionBuilder,
    expr: &Expr,
    locals: &HashMap<LocalId, LocalInfo>,
) -> Option<Value> {
    match expr {
        Expr::Integer(n) if *n >= 0 && *n <= i32::MAX as i64 => {
            Some(builder.ins().iconst(types::I32, *n))
        }
        Expr::LocalGet(id) => {
            let info = locals.get(id)?;
            if info.is_i32 {
                Some(builder.use_var(info.var))
            } else if let Some(shadow) = info.i32_shadow {
                Some(builder.use_var(shadow))
            } else if info.is_integer {
                // Safe conversion: f64 -> i64 -> i32 (avoids ARM64 SIGILL on large values)
                let f64_val = builder.use_var(info.var);
                let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, f64_val);
                Some(builder.ins().ireduce(types::I32, i64_val))
            } else {
                None
            }
        }
        Expr::Binary { op, left, right }
            if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) =>
        {
            // LICM: Check for hoisted i32 product (Mul only)
            if *op == BinaryOp::Mul {
                if let (Expr::LocalGet(a_id), Expr::LocalGet(b_id)) = (left.as_ref(), right.as_ref()) {
                    // Check a's hoisted products for b
                    if let Some(info) = locals.get(a_id) {
                        if let Some(ref products) = info.hoisted_i32_products {
                            if let Some(&cached) = products.get(b_id) {
                                return Some(builder.use_var(cached));
                            }
                        }
                    }
                    // Check b's hoisted products for a (commutative)
                    if let Some(info) = locals.get(b_id) {
                        if let Some(ref products) = info.hoisted_i32_products {
                            if let Some(&cached) = products.get(a_id) {
                                return Some(builder.use_var(cached));
                            }
                        }
                    }
                }
            }
            let l = try_compile_index_as_i32(builder, left, locals)?;
            let r = try_compile_index_as_i32(builder, right, locals)?;
            Some(match op {
                BinaryOp::Add => builder.ins().iadd(l, r),
                BinaryOp::Sub => builder.ins().isub(l, r),
                BinaryOp::Mul => builder.ins().imul(l, r),
                _ => unreachable!(),
            })
        }
        _ => None,
    }
}

/// Check whether an expression is statically known to produce a plain Array
/// (not Buffer, Set, Map, or a union type). When true, the codegen can safely
/// emit `js_array_get_f64_unchecked` / `js_array_set_f64_unchecked` instead
/// of the checked variants that probe 3 thread-local registries per access.
pub(crate) fn is_known_plain_array_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
    match expr {
        // Local variable with type info from HIR lowering
        Expr::LocalGet(id) => {
            locals.get(id).map(|i| {
                i.is_array && !i.is_union && !i.is_buffer && !i.is_set && !i.is_map
            }).unwrap_or(false)
        }
        // These expressions always return a freshly-allocated plain Array
        Expr::ArrayFrom(_) |
        Expr::ArraySlice { .. } |
        Expr::ObjectKeys(_) |
        Expr::ObjectValues(_) |
        Expr::ObjectEntries(_) |
        Expr::ProcessArgv => true,
        _ => false,
    }
}

/// Check if a list of HIR statements contains any function calls (Call, MethodCall,
/// NativeMethodCall, CallSpread, New, NewDynamic, Await, Yield).
/// Used to determine if module-level variable write-backs can be safely deferred in loops.
pub(crate) fn loop_body_has_calls(stmts: &[Stmt]) -> bool {
    for s in stmts {
        if loop_stmt_has_calls(s) {
            return true;
        }
    }
    false
}

fn loop_stmt_has_calls(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => loop_expr_has_calls(e),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => false,
        Stmt::Let { init: Some(e), .. } => loop_expr_has_calls(e),
        Stmt::Let { init: None, .. } => false,
        Stmt::If { condition, then_branch, else_branch } => {
            loop_expr_has_calls(condition)
                || loop_body_has_calls(then_branch)
                || else_branch.as_ref().map_or(false, |eb| loop_body_has_calls(eb))
        }
        Stmt::For { init, condition, update, body } => {
            init.as_ref().map_or(false, |s| loop_stmt_has_calls(s))
                || condition.as_ref().map_or(false, |e| loop_expr_has_calls(e))
                || update.as_ref().map_or(false, |e| loop_expr_has_calls(e))
                || loop_body_has_calls(body)
        }
        Stmt::While { condition, body } => {
            loop_expr_has_calls(condition) || loop_body_has_calls(body)
        }
        Stmt::Try { body, catch, finally } => {
            loop_body_has_calls(body)
                || catch.as_ref().map_or(false, |c| loop_body_has_calls(&c.body))
                || finally.as_ref().map_or(false, |f| loop_body_has_calls(f))
        }
        Stmt::Switch { discriminant, cases } => {
            loop_expr_has_calls(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().map_or(false, |t| loop_expr_has_calls(t))
                        || loop_body_has_calls(&c.body)
                })
        }
    }
}

pub(crate) fn loop_expr_has_calls(expr: &Expr) -> bool {
    match expr {
        // These are function calls — any of these means we can't defer write-backs
        Expr::Call { .. } | Expr::CallSpread { .. } | Expr::NativeMethodCall { .. }
        | Expr::New { .. } | Expr::NewDynamic { .. } | Expr::Await(..) | Expr::Yield { .. } => true,

        // Recurse through sub-expressions
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            loop_expr_has_calls(left) || loop_expr_has_calls(right)
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
            loop_expr_has_calls(operand)
        }
        Expr::LocalSet(_, val) => loop_expr_has_calls(val),
        Expr::Conditional { condition, then_expr, else_expr } => {
            loop_expr_has_calls(condition)
                || loop_expr_has_calls(then_expr)
                || loop_expr_has_calls(else_expr)
        }
        Expr::PropertyGet { object, .. } => loop_expr_has_calls(object),
        Expr::PropertySet { object, value, .. } => {
            loop_expr_has_calls(object) || loop_expr_has_calls(value)
        }
        Expr::PropertyUpdate { object, .. } => loop_expr_has_calls(object),
        Expr::IndexGet { object, index } => {
            loop_expr_has_calls(object) || loop_expr_has_calls(index)
        }
        Expr::IndexSet { object, index, value } => {
            loop_expr_has_calls(object) || loop_expr_has_calls(index) || loop_expr_has_calls(value)
        }
        Expr::IndexUpdate { object, index, .. } => {
            loop_expr_has_calls(object) || loop_expr_has_calls(index)
        }
        Expr::Object(fields) => fields.iter().any(|(_, e)| loop_expr_has_calls(e)),
        Expr::ObjectSpread { parts } => parts.iter().any(|(_, e)| loop_expr_has_calls(e)),
        Expr::Array(elems) => elems.iter().any(|e| loop_expr_has_calls(e)),
        Expr::ArraySpread(elems) => elems.iter().any(|e| match e {
            perry_hir::ArrayElement::Expr(ex) | perry_hir::ArrayElement::Spread(ex) => loop_expr_has_calls(ex),
        }),
        Expr::InstanceOf { expr, .. } => loop_expr_has_calls(expr),
        Expr::In { property, object } => {
            loop_expr_has_calls(property) || loop_expr_has_calls(object)
        }
        Expr::Update { .. } => false,
        Expr::StaticFieldSet { value, .. } => loop_expr_has_calls(value),
        Expr::GlobalSet(_, val) => loop_expr_has_calls(val),
        // Leaves — no sub-expressions, no calls
        _ => false,
    }
}

/// Collect LocalIds of module-scoped variables that are assigned (LocalSet or Update) in the given stmts.
/// Only includes variables that have a `module_var_data_id` set.
pub(crate) fn collect_module_var_writes_in_loop(
    stmts: &[Stmt],
    locals: &HashMap<LocalId, LocalInfo>,
) -> HashSet<LocalId> {
    let mut result = HashSet::new();
    collect_module_var_writes_stmts(stmts, locals, &mut result);
    result
}

fn collect_module_var_writes_stmts(stmts: &[Stmt], locals: &HashMap<LocalId, LocalInfo>, out: &mut HashSet<LocalId>) {
    for s in stmts {
        collect_module_var_writes_stmt(s, locals, out);
    }
}

fn collect_module_var_writes_stmt(stmt: &Stmt, locals: &HashMap<LocalId, LocalInfo>, out: &mut HashSet<LocalId>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => collect_module_var_writes_expr(e, locals, out),
        Stmt::Let { init: Some(e), .. } => collect_module_var_writes_expr(e, locals, out),
        Stmt::If { condition, then_branch, else_branch } => {
            collect_module_var_writes_expr(condition, locals, out);
            collect_module_var_writes_stmts(then_branch, locals, out);
            if let Some(eb) = else_branch { collect_module_var_writes_stmts(eb, locals, out); }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(s) = init { collect_module_var_writes_stmt(s, locals, out); }
            if let Some(e) = condition { collect_module_var_writes_expr(e, locals, out); }
            if let Some(e) = update { collect_module_var_writes_expr(e, locals, out); }
            collect_module_var_writes_stmts(body, locals, out);
        }
        Stmt::While { condition, body } => {
            collect_module_var_writes_expr(condition, locals, out);
            collect_module_var_writes_stmts(body, locals, out);
        }
        Stmt::Try { body, catch, finally } => {
            collect_module_var_writes_stmts(body, locals, out);
            if let Some(c) = catch { collect_module_var_writes_stmts(&c.body, locals, out); }
            if let Some(f) = finally { collect_module_var_writes_stmts(f, locals, out); }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_module_var_writes_expr(discriminant, locals, out);
            for c in cases {
                if let Some(t) = &c.test { collect_module_var_writes_expr(t, locals, out); }
                collect_module_var_writes_stmts(&c.body, locals, out);
            }
        }
        _ => {}
    }
}

fn collect_module_var_writes_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>, out: &mut HashSet<LocalId>) {
    match expr {
        Expr::LocalSet(id, val) => {
            if let Some(info) = locals.get(id) {
                if !info.is_boxed && info.module_var_data_id.is_some() {
                    out.insert(*id);
                }
            }
            collect_module_var_writes_expr(val, locals, out);
        }
        Expr::Update { id, .. } => {
            if let Some(info) = locals.get(id) {
                if info.module_var_data_id.is_some() {
                    out.insert(*id);
                }
            }
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_module_var_writes_expr(left, locals, out);
            collect_module_var_writes_expr(right, locals, out);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
            collect_module_var_writes_expr(operand, locals, out);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_module_var_writes_expr(condition, locals, out);
            collect_module_var_writes_expr(then_expr, locals, out);
            collect_module_var_writes_expr(else_expr, locals, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_module_var_writes_expr(callee, locals, out);
            for a in args { collect_module_var_writes_expr(a, locals, out); }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_module_var_writes_expr(callee, locals, out);
            for a in args {
                match a {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        collect_module_var_writes_expr(e, locals, out);
                    }
                }
            }
        }
        Expr::PropertyGet { object, .. } => collect_module_var_writes_expr(object, locals, out),
        Expr::PropertySet { object, value, .. } => {
            collect_module_var_writes_expr(object, locals, out);
            collect_module_var_writes_expr(value, locals, out);
        }
        Expr::IndexGet { object, index } => {
            collect_module_var_writes_expr(object, locals, out);
            collect_module_var_writes_expr(index, locals, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_module_var_writes_expr(object, locals, out);
            collect_module_var_writes_expr(index, locals, out);
            collect_module_var_writes_expr(value, locals, out);
        }
        Expr::Object(fields) => {
            for (_, e) in fields { collect_module_var_writes_expr(e, locals, out); }
        }
        Expr::Array(elems) => {
            for e in elems { collect_module_var_writes_expr(e, locals, out); }
        }
        _ => {}
    }
}
