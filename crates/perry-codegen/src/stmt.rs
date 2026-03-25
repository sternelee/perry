//! Statement compilation for the codegen module.
//!
//! Contains the `compile_stmt` free function and related helpers:
//! `compile_async_stmt`, `emit_try_end_cleanup`, `contains_loop_control`,
//! and `compile_stmt_with_this`.

use anyhow::{anyhow, Result};
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{DataDescription, Init, Linkage, Module};
use cranelift_object::ObjectModule;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use perry_hir::{
    UpdateOp,
    BinaryOp, CallArg, CatchClause, CompareOp, Expr, LogicalOp, Stmt, UnaryOp,
};
use perry_types::LocalId;
use cranelift_codegen::ir::{StackSlot, StackSlotData, StackSlotKind, TrapCode};

use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext, LoopContext};
use crate::util::*;
use crate::expr::compile_expr;



/// Check if a list of statements contains break, continue, return, or throw (for loop unrolling safety)
pub(crate) fn contains_loop_control(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Break | Stmt::Continue | Stmt::Return(_) | Stmt::Throw(_) => return true,
            Stmt::If { then_branch, else_branch, .. } => {
                if contains_loop_control(then_branch) { return true; }
                if let Some(else_b) = else_branch {
                    if contains_loop_control(else_b) { return true; }
                }
            }
            // Nested loops have their own break/continue scope, so we don't recurse
            Stmt::For { .. } | Stmt::While { .. } => {}
            Stmt::Try { body, catch, finally } => {
                if contains_loop_control(body) { return true; }
                if let Some(c) = catch {
                    if contains_loop_control(&c.body) { return true; }
                }
                if let Some(f) = finally {
                    if contains_loop_control(f) { return true; }
                }
            }
            _ => {}
        }
    }
    false
}

/// Compile a statement inside an async function (handles return specially)
pub(crate) fn compile_async_stmt(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    func_ids: &HashMap<u32, cranelift_module::FuncId>,
    closure_func_ids: &HashMap<u32, cranelift_module::FuncId>,
    func_wrapper_ids: &HashMap<u32, cranelift_module::FuncId>,
    extern_funcs: &HashMap<Cow<'static, str>, cranelift_module::FuncId>,
    async_func_ids: &HashSet<u32>,
    closure_returning_funcs: &HashSet<u32>,
    classes: &HashMap<String, ClassMeta>,
    enums: &HashMap<(String, String), EnumMemberValue>,
    func_param_types: &HashMap<u32, Vec<types::Type>>, func_union_params: &HashMap<u32, Vec<bool>>,
    func_return_types: &HashMap<u32, types::Type>,
    func_hir_return_types: &HashMap<u32, perry_types::Type>,
    func_rest_param_index: &HashMap<u32, usize>,
    imported_func_param_counts: &HashMap<String, usize>,
    locals: &mut HashMap<LocalId, LocalInfo>,
    next_var: &mut usize,
    stmt: &Stmt,
    promise_var: Variable,
    boxed_vars: &std::collections::HashSet<LocalId>,
    return_as_f64: bool,  // If true, bitcast Promise pointer to F64 before returning (for closures)
) -> Result<()> {
    match stmt {
        Stmt::Return(Some(expr)) => {
            // In async function, return resolves the promise and returns it
            let value = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, expr, None)?;
            let promise_ptr = builder.use_var(promise_var);

            // Helper to detect if an expression returns a Promise
            // This is needed for Promise unwrapping - when returning a Promise from async function
            fn is_promise_expr(expr: &Expr, async_func_ids: &HashSet<u32>) -> bool {
                match expr {
                    // new Promise(...) returns a Promise
                    Expr::New { class_name, .. } if class_name == "Promise" => true,
                    // Calling an async function returns a Promise
                    Expr::Call { callee, .. } => {
                        if let Expr::FuncRef(func_id) = callee.as_ref() {
                            async_func_ids.contains(func_id)
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }

            // Helper to detect if an expression is an object/array (pointer type)
            fn is_object_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>, async_func_ids: &HashSet<u32>) -> bool {
                // Promises are handled separately - don't treat them as generic objects
                if is_promise_expr(expr, async_func_ids) {
                    return false;
                }
                match expr {
                    Expr::Object(_) | Expr::ObjectSpread { .. } | Expr::Array(_) | Expr::ArraySpread(_) => true,
                    Expr::New { .. } => true,
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_pointer && !i.is_string).unwrap_or(false),
                    Expr::JsonParse(_) => true,
                    _ => false,
                }
            }

            // Check if a Call expression invokes a function that returns a string
            fn is_string_returning_call(expr: &Expr, func_hir_return_types: &HashMap<u32, perry_types::Type>) -> bool {
                match expr {
                    Expr::Call { callee, .. } => {
                        match callee.as_ref() {
                            Expr::FuncRef(id) => {
                                matches!(func_hir_return_types.get(id), Some(perry_types::Type::String))
                            }
                            Expr::ExternFuncRef { return_type, name, .. } => {
                                if matches!(return_type, perry_types::Type::String) {
                                    return true;
                                }
                                // Cross-module: check IMPORTED_FUNC_RETURN_TYPES
                                IMPORTED_FUNC_RETURN_TYPES.with(|p| {
                                    matches!(p.borrow().get(name), Some(perry_types::Type::String))
                                })
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }

            // Helper to detect if an expression is a string
            fn is_string_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
                match expr {
                    Expr::String(_) => true,
                    Expr::StringFromCharCode(_) => true,  // String.fromCharCode() returns a string
                    Expr::ArrayJoin { .. } => true,  // array.join() returns a string
                    Expr::EnvGet(_) | Expr::EnvGetDynamic(_) | Expr::FsReadFileSync(_) => true,
                    Expr::PathJoin(_, _) | Expr::PathDirname(_) | Expr::PathBasename(_) |
                    Expr::PathExtname(_) | Expr::PathResolve(_) | Expr::FileURLToPath(_) => true,
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_string).unwrap_or(false),
                    Expr::NativeMethodCall { module, method, .. } => {
                        (module == "path" && matches!(method.as_str(), "dirname" | "basename" | "extname" | "join" | "resolve"))
                        || (module == "fs" && method == "readFileSync")
                        || (module == "uuid" && matches!(method.as_str(), "v4" | "v1" | "v7"))
                        || (module == "crypto" && matches!(method.as_str(), "sha256" | "md5" | "randomUUID" | "hmacSha256"))
                    }
                    // String concatenation (+ with string operand) returns string
                    Expr::Binary { op: BinaryOp::Add, left, right } => {
                        is_string_expr(left, locals) || is_string_expr(right, locals)
                    }
                    // Property access on strings (like str.substring()) returns string
                    Expr::PropertyGet { object, property } => {
                        if is_string_expr(object, locals) {
                            matches!(property.as_str(), "substring" | "slice" | "toLowerCase" | "toUpperCase"
                                | "trim" | "trimStart" | "trimEnd" | "charAt" | "padStart" | "padEnd"
                                | "repeat" | "replace" | "replaceAll" | "concat")
                        } else {
                            false
                        }
                    }
                    Expr::Call { callee, .. } => {
                        // Check if it's a string method call
                        if let Expr::PropertyGet { object, property } = callee.as_ref() {
                            if is_string_expr(object, locals) {
                                return matches!(property.as_str(), "substring" | "slice" | "toLowerCase" | "toUpperCase"
                                    | "trim" | "trimStart" | "trimEnd" | "charAt" | "padStart" | "padEnd"
                                    | "repeat" | "replace" | "replaceAll" | "concat" | "split" | "join");
                            }
                        }
                        false
                    }
                    _ => false,
                }
            }

            // Check if we're returning a Promise - need special handling for Promise unwrapping
            if is_promise_expr(expr, async_func_ids) {
                // Returning a Promise from async function - chain the promises
                // The outer promise should adopt the inner promise's eventual value
                let inner_promise_ptr = ensure_i64(builder, value);
                let resolve_with_promise_func = extern_funcs.get("js_promise_resolve_with_promise")
                    .ok_or_else(|| anyhow!("js_promise_resolve_with_promise not declared"))?;
                let resolve_ref = module.declare_func_in_func(*resolve_with_promise_func, builder.func);
                builder.ins().call(resolve_ref, &[promise_ptr, inner_promise_ptr]);
            } else {
                // Resolve the promise with the value
                // NaN-box object/array pointers for proper typeof support
                let value_f64 = if is_object_expr(expr, locals, async_func_ids) {
                    // Object pointer needs NaN-boxing with POINTER_TAG
                    let ptr = ensure_i64(builder, value);
                    inline_nanbox_pointer(builder, ptr)
                } else if is_string_expr(expr, locals) || is_string_returning_call(expr, func_hir_return_types) {
                    // String pointer needs NaN-boxing with STRING_TAG
                    let ptr = ensure_i64(builder, value);
                    let nanbox_func = extern_funcs.get("js_nanbox_string")
                        .ok_or_else(|| anyhow!("js_nanbox_string not declared"))?;
                    let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                    let call = builder.ins().call(nanbox_ref, &[ptr]);
                    builder.inst_results(call)[0]
                } else {
                    ensure_f64(builder, value)
                };

                let resolve_func = extern_funcs.get("js_promise_resolve")
                    .ok_or_else(|| anyhow!("js_promise_resolve not declared"))?;
                let resolve_ref = module.declare_func_in_func(*resolve_func, builder.func);
                builder.ins().call(resolve_ref, &[promise_ptr, value_f64]);
            }

            // Pop any enclosing try frames (including the implicit async try/catch) before returning
            let try_depth = TRY_CATCH_DEPTH.with(|d| d.get());
            emit_try_end_cleanup(builder, module, extern_funcs, try_depth)?;

            // Return the promise (NaN-boxed for closures so caller recognizes it as a pointer)
            let ret_val = if return_as_f64 {
                // NaN-box the Promise pointer with POINTER_TAG so caller can detect it
                let nanbox_func = extern_funcs.get("js_nanbox_pointer")
                    .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                let call = builder.ins().call(nanbox_ref, &[promise_ptr]);
                builder.inst_results(call)[0]
            } else {
                promise_ptr
            };
            builder.ins().return_(&[ret_val]);
            Ok(())
        }
        Stmt::Return(None) => {
            // Return undefined
            let promise_ptr = builder.use_var(promise_var);

            let resolve_func = extern_funcs.get("js_promise_resolve")
                .ok_or_else(|| anyhow!("js_promise_resolve not declared"))?;
            let resolve_ref = module.declare_func_in_func(*resolve_func, builder.func);
            const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
            let undef_val = builder.ins().f64const(f64::from_bits(TAG_UNDEFINED));
            builder.ins().call(resolve_ref, &[promise_ptr, undef_val]);

            // Pop any enclosing try frames (including the implicit async try/catch) before returning
            let try_depth = TRY_CATCH_DEPTH.with(|d| d.get());
            emit_try_end_cleanup(builder, module, extern_funcs, try_depth)?;

            // Return the promise (NaN-boxed for closures so caller recognizes it as a pointer)
            let ret_val = if return_as_f64 {
                // NaN-box the Promise pointer with POINTER_TAG so caller can detect it
                let nanbox_func = extern_funcs.get("js_nanbox_pointer")
                    .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                let call = builder.ins().call(nanbox_ref, &[promise_ptr]);
                builder.inst_results(call)[0]
            } else {
                promise_ptr
            };
            builder.ins().return_(&[ret_val]);
            Ok(())
        }
        // Handle If statements specially to ensure nested returns are compiled correctly
        Stmt::If { condition, then_branch, else_branch } => {
            let cond_bool = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, None)?;

            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();

            builder.ins().brif(cond_bool, then_block, &[], else_block, &[]);

            // Then branch - use compile_async_stmt for nested statements
            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            for s in then_branch {
                compile_async_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, promise_var, boxed_vars, return_as_f64)?;
            }
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                builder.ins().jump(merge_block, &[]);
            }

            // Else branch - use compile_async_stmt for nested statements
            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    compile_async_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, promise_var, boxed_vars, return_as_f64)?;
                }
            }
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                builder.ins().jump(merge_block, &[]);
            }

            // Merge
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            Ok(())
        }
        // For other statements, use the regular compile_stmt with async context
        _ => {
            compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, stmt, None, None, boxed_vars, Some(promise_var))
        }
    }
}

/// Emit `count` calls to js_try_end() for try-catch cleanup before return/break/continue.
/// When a return/break/continue exits one or more try blocks, the runtime try depth must
/// be decremented for each try block being exited to avoid leaking try depth.
pub(crate) fn emit_try_end_cleanup(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    extern_funcs: &HashMap<Cow<'static, str>, cranelift_module::FuncId>,
    count: usize,
) -> Result<()> {
    if count > 0 {
        let try_end_func = extern_funcs.get("js_try_end")
            .ok_or_else(|| anyhow!("js_try_end not declared"))?;
        let try_end_ref = module.declare_func_in_func(*try_end_func, builder.func);
        for _ in 0..count {
            builder.ins().call(try_end_ref, &[]);
        }
    }
    Ok(())
}

pub(crate) fn compile_stmt(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    func_ids: &HashMap<u32, cranelift_module::FuncId>,
    closure_func_ids: &HashMap<u32, cranelift_module::FuncId>,
    func_wrapper_ids: &HashMap<u32, cranelift_module::FuncId>,
    extern_funcs: &HashMap<Cow<'static, str>, cranelift_module::FuncId>,
    async_func_ids: &HashSet<u32>,
    closure_returning_funcs: &HashSet<u32>,
    classes: &HashMap<String, ClassMeta>,
    enums: &HashMap<(String, String), EnumMemberValue>,
    func_param_types: &HashMap<u32, Vec<types::Type>>, func_union_params: &HashMap<u32, Vec<bool>>,
    func_return_types: &HashMap<u32, types::Type>,
    func_hir_return_types: &HashMap<u32, perry_types::Type>,
    func_rest_param_index: &HashMap<u32, usize>,
    imported_func_param_counts: &HashMap<String, usize>,
    locals: &mut HashMap<LocalId, LocalInfo>,
    next_var: &mut usize,
    stmt: &Stmt,
    this_ctx: Option<&ThisContext>,
    loop_ctx: Option<&LoopContext>,
    boxed_vars: &std::collections::HashSet<LocalId>,
    async_promise_var: Option<Variable>,
) -> Result<()> {
    match stmt {
        Stmt::Let { id, name: var_name, mutable, ty, init, .. } => {
            // Use the declared type to determine the ABI type
            // Note: Type::Any and Type::Unknown use expression inference, not pointer type
            use perry_types::Type as HirType;
            let is_typed_pointer = matches!(ty, HirType::String | HirType::Array(_) |
                HirType::Object(_) | HirType::Named(_) | HirType::Generic { .. } |
                HirType::Function(_));
            // Check if parameter is a string type (including string enums like ChainName)
            let is_typed_string = matches!(ty, HirType::String) || {
                if let HirType::Named(name) = ty {
                    enums.iter().any(|((enum_name, _), val)| {
                        enum_name == name && matches!(val, EnumMemberValue::String(_))
                    })
                } else {
                    false
                }
            };
            let is_typed_bigint_check = matches!(ty, HirType::BigInt);

            // Helper to detect if an expression produces a string (fallback for untyped cases)
            // Note: EnvGet is NOT included here because it can return undefined if the env var doesn't exist
            fn is_string_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
                match expr {
                    Expr::String(_) => true,
                    Expr::StringFromCharCode(_) => true,
                    // EnvGet is NOT included - it returns string OR undefined (handled as union)
                    Expr::FsReadFileSync(_) => true, // fs.readFileSync returns a string
                    // All path operations return strings
                    Expr::PathJoin(_, _) | Expr::PathDirname(_) | Expr::PathBasename(_) |
                    Expr::PathExtname(_) | Expr::PathResolve(_) | Expr::FileURLToPath(_) | Expr::JsonStringify(_) => true,
                    // All crypto operations return strings (hex or UUID format)
                    Expr::CryptoRandomBytes(_) | Expr::CryptoRandomUUID |
                    Expr::CryptoSha256(_) | Expr::CryptoMd5(_) => true,
                    // Date.toISOString() returns a string
                    Expr::DateToISOString(_) => true,
                    // OS operations that return strings
                    Expr::OsPlatform | Expr::OsArch | Expr::OsHostname | Expr::OsHomedir |
                    Expr::OsTmpdir | Expr::OsType | Expr::OsRelease | Expr::OsEOL => true,
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_string).unwrap_or(false),
                    Expr::Binary { op: BinaryOp::Add, left, right } => {
                        is_string_expr(left, locals) || is_string_expr(right, locals)
                    }
                    Expr::Call { callee, .. } => {
                        // Check if it's a string method call (slice/substring/trim/toLowerCase/toUpperCase returns string)
                        if let Expr::PropertyGet { object, property } = callee.as_ref() {
                            if property == "slice" || property == "substring" || property == "trim"
                               || property == "toLowerCase" || property == "toUpperCase" || property == "replace"
                               || property == "padStart" || property == "padEnd" || property == "repeat" || property == "charAt" {
                                if let Expr::LocalGet(id) = object.as_ref() {
                                    // If we can find the local, check if it's a string
                                    // Otherwise, assume it is (since we know it's a string method)
                                    return locals.get(id).map(|i| i.is_string).unwrap_or(true);
                                }
                                // ProcessArgv is an array, not a string — .slice() returns array
                                if matches!(object.as_ref(), Expr::ProcessArgv) {
                                    return false;
                                }
                                // For non-LocalGet (like property chains), assume it's a string method
                                return true;
                            }
                            // Check if it's buffer.toString() which returns a string
                            if property == "toString" {
                                if let Expr::LocalGet(id) = object.as_ref() {
                                    if locals.get(id).map(|i| i.is_buffer).unwrap_or(false) {
                                        return true;
                                    }
                                }
                            }
                        }
                        // Check if it's an external function that returns a string
                        if let Expr::ExternFuncRef { name: func_name, return_type, .. } = callee.as_ref() {
                            // Use the return type if available
                            if matches!(return_type, perry_types::Type::String) {
                                return true;
                            }
                            // Cross-module imports may have Type::Any in ExternFuncRef
                            // but actual return type stored in IMPORTED_FUNC_RETURN_TYPES
                            if matches!(return_type, perry_types::Type::Any) {
                                let is_str = IMPORTED_FUNC_RETURN_TYPES.with(|p| {
                                    p.borrow().get(func_name).map(|t| matches!(t, perry_types::Type::String)).unwrap_or(false)
                                });
                                if is_str {
                                    return true;
                                }
                            }
                            // Fallback: HTTP request methods that return strings
                            if func_name.starts_with("js_http_request_method")
                                || func_name.starts_with("js_http_request_path")
                                || func_name.starts_with("js_http_request_query")
                                || func_name.starts_with("js_http_request_body")
                                || func_name.starts_with("js_http_request_content_type")
                                || func_name.starts_with("js_http_request_header")
                            {
                                return true;
                            }
                        }
                        false
                    }
                    // NativeMethodCall methods that return strings
                    Expr::NativeMethodCall { module, method, .. } => {
                        // Decimal/Big.js toString and toFixed return strings
                        ((module == "decimal.js" || module == "big.js" || module == "bignumber.js") &&
                         (method == "toString" || method == "toFixed"))
                        // Path module functions return strings
                        || (module == "path" && matches!(method.as_str(), "dirname" | "basename" | "extname" | "join" | "resolve"))
                        // fs.readFileSync returns string
                        || (module == "fs" && method == "readFileSync")
                        // uuid functions return strings
                        || (module == "uuid" && matches!(method.as_str(), "v4" | "v1" | "v7"))
                        // crypto functions return strings
                        || (module == "crypto" && matches!(method.as_str(), "sha256" | "md5" | "randomUUID" | "hmacSha256"))
                        // ethers formatUnits/formatEther/getAddress return strings
                        || (module == "ethers" && matches!(method.as_str(), "formatUnits" | "formatEther" | "getAddress"))
                    }
                    _ => false,
                }
            }

            // Helper to detect if an expression produces a BigInt
            fn is_bigint_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>, func_hir_return_types: &HashMap<u32, perry_types::Type>, classes: &HashMap<String, ClassMeta>) -> bool {
                match expr {
                    Expr::BigInt(_) => true,
                    Expr::BigIntCoerce(_) => true,
                    // new BN(...) produces a BigInt value
                    Expr::New { class_name, .. } if class_name == "BN" => true,
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_bigint).unwrap_or(false),
                    Expr::Binary { left, right, .. } => {
                        is_bigint_expr(left, locals, func_hir_return_types, classes) || is_bigint_expr(right, locals, func_hir_return_types, classes)
                    }
                    Expr::Unary { operand, .. } => is_bigint_expr(operand, locals, func_hir_return_types, classes),
                    // Function calls that return BigInt
                    Expr::Call { callee, .. } => {
                        match callee.as_ref() {
                            Expr::FuncRef(id) => {
                                func_hir_return_types.get(id).map(|t| matches!(t, perry_types::Type::BigInt)).unwrap_or(false)
                            }
                            Expr::ExternFuncRef { return_type, name, .. } => {
                                if matches!(return_type, perry_types::Type::BigInt) {
                                    return true;
                                }
                                // Cross-module imports may have Type::Any in ExternFuncRef
                                // but actual return type stored in IMPORTED_FUNC_RETURN_TYPES
                                if matches!(return_type, perry_types::Type::Any) {
                                    return IMPORTED_FUNC_RETURN_TYPES.with(|p| {
                                        p.borrow().get(name).map(|t| matches!(t, perry_types::Type::BigInt)).unwrap_or(false)
                                    });
                                }
                                false
                            }
                            _ => false,
                        }
                    }
                    // Static method calls that return BigInt
                    Expr::StaticMethodCall { class_name, method_name, .. } => {
                        classes.get(class_name)
                            .and_then(|meta| meta.static_method_return_types.get(method_name))
                            .map(|t| matches!(t, perry_types::Type::BigInt))
                            .unwrap_or(false)
                    }
                    // NativeMethodCall methods that return BigInt
                    Expr::NativeMethodCall { module, method, .. } => {
                        // ethers parseUnits/parseEther return BigInt
                        module == "ethers" && (method == "parseUnits" || method == "parseEther")
                    }
                    _ => false,
                }
            }

            // Helper to detect if an expression produces a Closure
            fn is_closure_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>, closure_returning_funcs: &HashSet<u32>) -> bool {
                match expr {
                    Expr::Closure { .. } => true,
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_closure).unwrap_or(false),
                    // Check for function calls that return closures
                    Expr::Call { callee, .. } => {
                        match callee.as_ref() {
                            Expr::FuncRef(func_id) => closure_returning_funcs.contains(func_id),
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }

            // Helper to detect if an expression produces an integer value (for native i64 optimization)
            fn is_integer_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
                match expr {
                    // Integer literals
                    Expr::Integer(_) => true,
                    // Variable that was tracked as integer
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_integer).unwrap_or(false),
                    // Bitwise operations always produce integers
                    Expr::Binary { op: BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor |
                                       BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr, .. } => true,
                    // Arithmetic operations produce integers if both operands are integers
                    Expr::Binary { op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Mod, left, right } => {
                        is_integer_expr(left, locals) && is_integer_expr(right, locals)
                    }
                    // Unary minus on integer is still integer
                    Expr::Unary { op: UnaryOp::Neg, operand } => is_integer_expr(operand, locals),
                    // Bitwise NOT always produces integer
                    Expr::Unary { op: UnaryOp::BitNot, .. } => true,
                    // Array/string .length is always an integer
                    Expr::PropertyGet { property, object } if property == "length" => {
                        if let Expr::LocalGet(id) = object.as_ref() {
                            locals.get(id).map(|i| i.is_array || i.is_string).unwrap_or(false)
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }

            // Determine variable type - prefer declared type, fall back to init expression inference
            let is_typed_array = matches!(ty, HirType::Array(_));
            let is_typed_bigint = matches!(ty, HirType::BigInt);
            let is_typed_closure = matches!(ty, HirType::Function(_));
            let is_typed_map = matches!(ty, HirType::Generic { base, .. } if base == "Map");
            let is_typed_set = matches!(ty, HirType::Generic { base, .. } if base == "Set");
            let is_typed_union = matches!(ty, HirType::Union(_));
            // Named/Object types may contain NaN-boxed values when fields are accessed
            let is_typed_generic_object = matches!(ty, HirType::Named(_) | HirType::Object(_) | HirType::Any);

            // Helper to detect mixed-type array from expression
            fn is_mixed_array_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
                match expr {
                    Expr::Array(elements) => {
                        // Check if array contains both strings and numbers
                        let has_string = elements.iter().any(|e| matches!(e, Expr::String(_)));
                        let has_number = elements.iter().any(|e| matches!(e, Expr::Number(_) | Expr::Integer(_) | Expr::Bool(_)));
                        has_string && has_number
                    }
                    // ProcessArgv returns an array of NaN-boxed strings
                    Expr::ProcessArgv => true,
                    // ArraySlice/ArrayMap/ArrayFilter/ArraySort inherit mixed-ness from source
                    Expr::ArraySlice { array, .. } | Expr::ArrayMap { array, .. } | Expr::ArrayFilter { array, .. } | Expr::ArraySort { array, .. } => {
                        is_mixed_array_expr(array, locals)
                        || if let Expr::LocalGet(id) = array.as_ref() {
                            locals.get(id).map(|i| i.is_mixed_array).unwrap_or(false)
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }

            // Check if array has mixed element types (union or any) - from type or expression
            // Also check source variables for LocalGet to propagate is_mixed_array
            let is_mixed_array_from_type = if let HirType::Array(elem_ty) = ty {
                // String arrays need mixed-array access because strings are NaN-boxed
                matches!(elem_ty.as_ref(), HirType::Union(_) | HirType::Any | HirType::String)
            } else if let Some(expr) = init {
                is_mixed_array_expr(expr, locals)
            } else {
                false
            };
            let is_mixed_array_from_source = if let Some(Expr::LocalGet(src_id)) = init {
                locals.get(src_id).map(|i| i.is_mixed_array || i.is_union).unwrap_or(false)
            } else {
                false
            };
            let is_mixed_array = is_mixed_array_from_type || is_mixed_array_from_source;

            // Extract class name from Named type (also check union types and generics)
            let typed_class_name = if let HirType::Named(name) = ty {
                Some(name.clone())
            } else if let HirType::Generic { base, type_args, .. } = ty {
                // Unwrap Partial<T>, Readonly<T>, Required<T> to inner class name
                if (base == "Partial" || base == "Readonly" || base == "Required")
                    && !type_args.is_empty()
                {
                    if let HirType::Named(inner) = &type_args[0] {
                        Some(inner.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if let HirType::Union(types) = ty {
                // For union types like `Person | null`, extract the class name from Named types
                types.iter().find_map(|t| {
                    if let HirType::Named(name) = t {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // Helper to detect if an expression produces a buffer
            fn is_buffer_expr(expr: &Expr) -> bool {
                match expr {
                    Expr::BufferFrom { .. } | Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) |
                    Expr::BufferConcat(_) | Expr::BufferSlice { .. } | Expr::BufferFill { .. } |
                    Expr::Uint8ArrayNew(_) | Expr::Uint8ArrayFrom(_) |
                    Expr::ChildProcessExecSync { .. } | Expr::CryptoRandomBytes(_) => true,
                    Expr::NativeMethodCall { module, method, .. }
                        if module == "crypto" && method == "randomBytes" => true,
                    // Detect chained buffer methods: new Uint8Array(n).fill(v)
                    Expr::Call { callee, .. } => {
                        if let Expr::PropertyGet { object, property } = callee.as_ref() {
                            property == "fill" && is_buffer_expr(object.as_ref())
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }

            // Use declared type information first, fall back to expression inference
            // For union types, we should NOT infer is_string etc. based on initialization
            let (class_name, is_pointer, is_array, is_string, is_bigint, is_closure, is_map, is_set, is_buffer, is_event_emitter) = if is_typed_union {
                // Union types use f64 (NaN-boxed), but track class_name if available for property access
                (typed_class_name.clone(), false, false, false, false, false, false, false, false, false)
            } else if is_typed_string {
                // String type uses i64 (raw pointer) with is_string = true for console.log
                (None, true, false, true, false, false, false, false, false, false)
            } else if is_typed_bigint_check {
                // BigInt type uses f64 (NaN-boxed) but is_bigint = true for method calls
                (None, false, false, false, true, false, false, false, false, false)
            } else if is_typed_pointer {
                // Type annotation tells us the type (non-string pointer types use i64)
                let class_name = typed_class_name.or_else(|| {
                    if let Some(Expr::New { class_name, .. }) = init {
                        Some(class_name.clone())
                    } else {
                        None
                    }
                });
                let is_typed_buffer = matches!(ty, HirType::Named(name) if name == "Uint8Array" || name == "Buffer")
                    || init.as_ref().map(|e| is_buffer_expr(e)).unwrap_or(false);
                (class_name, true, is_typed_array, false, is_typed_bigint, is_typed_closure, is_typed_map, is_typed_set, is_typed_buffer, false)
            } else {
                // Fall back to expression inference for untyped cases
                match init {
                    Some(Expr::New { class_name, .. }) => {
                        // BN.js mapped to BigInt - stored as NaN-boxed F64 with BIGINT_TAG
                        if class_name == "BN" {
                            (None, false, false, false, true, false, false, false, false, false)
                        } else {
                            // Native handle-based classes use f64 (handles bitcast to f64), not i64 pointers
                            let is_native_handle_class = matches!(class_name.as_str(),
                                "Decimal" | "Big" | "BigNumber" | "LRUCache" | "Command" | "EventEmitter" | "Redis");
                            let is_event_emitter = class_name == "EventEmitter";
                            (Some(class_name.clone()), !is_native_handle_class, false, false, false, false, false, false, false, is_event_emitter)
                        }
                    }
                    Some(Expr::StaticMethodCall { class_name: static_class, method_name, .. }) => {
                        // Check the static method's return type
                        let ret_type = classes.get(static_class)
                            .and_then(|meta| meta.static_method_return_types.get(method_name));
                        if let Some(perry_types::Type::BigInt) = ret_type {
                            // Static method returns BigInt
                            (None, false, false, false, true, false, false, false, false, false)
                        } else if let Some(perry_types::Type::String) = ret_type {
                            // Static method returns String (i64 pointer)
                            (None, true, false, true, false, false, false, false, false, false)
                        } else if let Some(perry_types::Type::Named(name)) = ret_type {
                            // Static method returns a class instance (singleton pattern etc.)
                            let name = name.clone();
                            let is_native_handle_class = matches!(name.as_str(),
                                "Decimal" | "Big" | "BigNumber" | "LRUCache" | "Command" | "EventEmitter" | "Redis");
                            let is_event_emitter = name == "EventEmitter";
                            (Some(name), !is_native_handle_class, false, false, false, false, false, false, false, is_event_emitter)
                        } else {
                            // Static method doesn't return a class instance, or class/method not found
                            (None, false, false, false, false, false, false, false, false, false)
                        }
                    }
                    Some(Expr::Array(_)) | Some(Expr::ArraySpread(_)) | Some(Expr::ProcessArgv) => (None, true, true, false, false, false, false, false, false, false),
                    // Object literals return object pointers
                    Some(Expr::Object(_)) | Some(Expr::ObjectSpread { .. }) => (None, true, false, false, false, false, false, false, false, false),
                    // ArrayMap, ArrayFilter, ArraySort, ArraySlice, and ArraySplice return arrays
                    Some(Expr::ArrayMap { .. }) | Some(Expr::ArrayFilter { .. }) |
                    Some(Expr::ArraySort { .. }) | Some(Expr::ArraySlice { .. }) |
                    Some(Expr::ArraySplice { .. }) => (None, true, true, false, false, false, false, false, false, false),
                    // MapNew returns a Map pointer
                    Some(Expr::MapNew) => (None, true, false, false, false, false, true, false, false, false),
                    // SetNew/SetNewFromArray returns a Set pointer
                    Some(Expr::SetNew) | Some(Expr::SetNewFromArray(_)) => (None, true, false, false, false, false, false, true, false, false),
                    // Buffer expressions return buffer pointers
                    Some(expr) if is_buffer_expr(expr) => (None, true, false, false, false, false, false, false, true, false),
                    Some(Expr::Closure { .. }) => (None, true, false, false, false, true, false, false, false, false),
                    // BigInt literals - stored as NaN-boxed F64 (is_pointer = false)
                    Some(Expr::BigInt(_)) => (None, false, false, false, true, false, false, false, false, false),
                    Some(expr) if is_bigint_expr(expr, locals, func_hir_return_types, classes) => (None, false, false, false, true, false, false, false, false, false),
                    Some(expr) if is_string_expr(expr, locals) => (None, true, false, true, false, false, false, false, false, false),
                    Some(expr) if is_closure_expr(expr, locals, closure_returning_funcs) => (None, true, false, false, false, true, false, false, false, false),
                    // JsonParse returns any type - mark as union for dynamic typeof
                    Some(Expr::JsonParse(_)) => (None, false, false, false, false, false, false, false, false, false),
                    // Await expression - resolve the inner async function's return type
                    // to propagate Map/Set/Array flags from Promise<T> -> T
                    Some(Expr::Await(inner)) => {
                        let mut resolved = (None, false, false, false, false, false, false, false, false, false);
                        if let Expr::Call { callee, .. } = inner.as_ref() {
                            // Get the return type from the callee
                            // For ExternFuncRef with Type::Any, look up cross-module return type
                            let owned_ret_type: Option<perry_types::Type> = match callee.as_ref() {
                                Expr::FuncRef(id) => func_hir_return_types.get(id).cloned(),
                                Expr::ExternFuncRef { return_type, name, .. } => {
                                    match return_type {
                                        perry_types::Type::Any => {
                                            // Cross-module import: look up actual return type from thread-local
                                            IMPORTED_FUNC_RETURN_TYPES.with(|p| {
                                                p.borrow().get(name).cloned()
                                            })
                                        }
                                        other => Some(other.clone()),
                                    }
                                }
                                _ => None,
                            };
                            if let Some(hir_ret_type) = owned_ret_type.as_ref() {
                                // Strip Promise<T> to get the resolved type T
                                let inner_type = match hir_ret_type {
                                    perry_types::Type::Promise(inner) => inner.as_ref(),
                                    other => other,
                                };
                                match inner_type {
                                    perry_types::Type::Generic { base, .. } if base == "Map" => {
                                        resolved = (None, true, false, false, false, false, true, false, false, false);
                                    }
                                    perry_types::Type::Generic { base, .. } if base == "Set" => {
                                        resolved = (None, true, false, false, false, false, false, true, false, false);
                                    }
                                    perry_types::Type::Array(_) => {
                                        resolved = (None, true, true, false, false, false, false, false, false, false);
                                    }
                                    perry_types::Type::Named(name) => {
                                        resolved = (Some(name.clone()), true, false, false, false, false, false, false, false, false);
                                    }
                                    perry_types::Type::String => {
                                        // Promise<string> resolves to string (i64 pointer)
                                        resolved = (None, true, false, true, false, false, false, false, false, false);
                                    }
                                    perry_types::Type::BigInt => {
                                        // Promise<bigint> resolves to bigint
                                        resolved = (None, false, false, false, true, false, false, false, false, false);
                                    }
                                    _ => {} // Keep default all-false for Number, Any, etc.
                                }
                            }
                        } else if let Expr::NativeMethodCall { module, method, .. } = inner.as_ref() {
                            // Native method calls that return arrays/objects via Promise
                            match (module.as_str(), method.as_str()) {
                                // mysql2 query/execute returns [rows, fields] array
                                ("mysql2" | "mysql2/promise", "query" | "execute") => {
                                    resolved = (None, true, true, false, false, false, false, false, false, false);
                                }
                                // ioredis get returns a string (NaN-boxed), but we treat as plain value
                                // since the value comes back as F64 from promise resolution
                                _ => {}
                            }
                        }
                        resolved
                    }
                    // Function call - check if the function returns a pointer type (i64)
                    // Also check for array method calls like .map(), .filter(), .slice() which return arrays
                    Some(Expr::Call { callee, .. }) => {
                        if let Expr::FuncRef(func_id) = callee.as_ref() {
                            // Check the HIR return type to detect Map, Set, Array, etc.
                            if let Some(hir_ret_type) = func_hir_return_types.get(func_id) {
                                match hir_ret_type {
                                    perry_types::Type::Generic { base, .. } if base == "Map" => {
                                        // Function returns Map<K, V>
                                        (None, true, false, false, false, false, true, false, false, false)
                                    }
                                    perry_types::Type::Generic { base, .. } if base == "Set" => {
                                        // Function returns Set<T>
                                        (None, true, false, false, false, false, false, true, false, false)
                                    }
                                    perry_types::Type::Array(_) => {
                                        // Function returns Array<T>
                                        (None, true, true, false, false, false, false, false, false, false)
                                    }
                                    perry_types::Type::String => {
                                        // Function returns string (i64 pointer)
                                        (None, true, false, true, false, false, false, false, false, false)
                                    }
                                    _ => {
                                        // Check ABI type for other pointer types
                                        if let Some(&ret_type) = func_return_types.get(func_id) {
                                            if ret_type == types::I64 {
                                                (None, true, false, false, false, false, false, false, false, false)
                                            } else {
                                                (None, false, false, false, false, false, false, false, false, false)
                                            }
                                        } else {
                                            (None, false, false, false, false, false, false, false, false, false)
                                        }
                                    }
                                }
                            } else if let Some(&ret_type) = func_return_types.get(func_id) {
                                if ret_type == types::I64 {
                                    // Function returns i64 (string/array/object pointer)
                                    (None, true, false, false, false, false, false, false, false, false)
                                } else {
                                    (None, false, false, false, false, false, false, false, false, false)
                                }
                            } else {
                                (None, false, false, false, false, false, false, false, false, false)
                            }
                        } else if let Expr::PropertyGet { object, property } = callee.as_ref() {
                            // Check if this is an instance method call on a known class
                            let class_method_result = if let Expr::LocalGet(obj_id) = object.as_ref() {
                                if let Some(obj_info) = locals.get(obj_id) {
                                    if let Some(ref obj_class) = obj_info.class_name {
                                        if let Some(class_meta) = classes.get(obj_class) {
                                            if let Some(ret_type) = class_meta.method_return_types.get(property.as_str()) {
                                                match ret_type {
                                                    perry_types::Type::Named(name) => {
                                                        Some((Some(name.clone()), true, false, false, false, false, false, false, false, false))
                                                    }
                                                    perry_types::Type::Generic { base, .. } => {
                                                        Some((Some(base.clone()), true, false, false, false, false, false, false, false, false))
                                                    }
                                                    perry_types::Type::String => {
                                                        Some((None, true, false, true, false, false, false, false, false, false))
                                                    }
                                                    perry_types::Type::Array(_) => {
                                                        Some((None, true, true, false, false, false, false, false, false, false))
                                                    }
                                                    _ => None,
                                                }
                                            } else { None }
                                        } else { None }
                                    } else { None }
                                } else { None }
                            } else { None };

                            if let Some(result) = class_method_result {
                                result
                            } else
                            // Check for array methods that return arrays
                            if property == "slice" {
                                // slice could be array or string method - check the object
                                if matches!(object.as_ref(), Expr::ProcessArgv) {
                                    // process.argv.slice() always returns array
                                    (None, true, true, false, false, false, false, false, false, false)
                                } else if let Expr::LocalGet(id) = object.as_ref() {
                                    if let Some(src_info) = locals.get(id) {
                                        if src_info.is_string {
                                            // String.slice returns string (i64 pointer)
                                            (None, true, false, true, false, false, false, false, false, false)
                                        } else if src_info.is_array {
                                            // Array.slice returns array
                                            (None, true, true, false, false, false, false, false, false, false)
                                        } else {
                                            (None, false, false, false, false, false, false, false, false, false)
                                        }
                                    } else {
                                        (None, false, false, false, false, false, false, false, false, false)
                                    }
                                } else {
                                    (None, false, false, false, false, false, false, false, false, false)
                                }
                            } else if property == "substring" || property == "trim" || property == "toLowerCase"
                                || property == "toUpperCase" || property == "charAt" || property == "padStart"
                                || property == "padEnd" || property == "repeat" || property == "replace" {
                                // String methods return strings (i64 pointers)
                                (None, true, false, true, false, false, false, false, false, false)
                            } else if property == "map" || property == "filter" ||
                               property == "concat" || property == "flat" || property == "flatMap" ||
                               property == "reverse" || property == "sort" || property == "toSorted" ||
                               property == "toReversed" || property == "with" {
                                (None, true, true, false, false, false, false, false, false, false)
                            } else if property == "split" {
                                // split() on strings returns a NaN-boxed array pointer (f64)
                                // is_pointer must be false so the variable uses f64 type
                                (None, false, true, false, false, false, false, false, false, false)
                            } else {
                                (None, false, false, false, false, false, false, false, false, false)
                            }
                        } else {
                            (None, false, false, false, false, false, false, false, false, false)
                        }
                    }
                    // LocalGet inherits pointer-ness from source variable
                    Some(Expr::LocalGet(src_id)) => {
                        if let Some(src_info) = locals.get(src_id) {
                            (src_info.class_name.clone(), src_info.is_pointer, src_info.is_array, src_info.is_string, src_info.is_bigint, src_info.is_closure, src_info.is_map, src_info.is_set, src_info.is_buffer, src_info.is_event_emitter)
                        } else {
                            (None, false, false, false, false, false, false, false, false, false)
                        }
                    }
                    // MapGet - infer value type from the map's generic type args
                    Some(Expr::MapGet { map, .. }) => {
                        // Try to determine the map's value type from LocalInfo.type_args
                        let value_type = if let Expr::LocalGet(map_id) = map.as_ref() {
                            locals.get(map_id).and_then(|info| {
                                if info.is_map && info.type_args.len() >= 2 {
                                    Some(info.type_args[1].clone())
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        };
                        match value_type {
                            Some(perry_types::Type::Number) | Some(perry_types::Type::Boolean) => {
                                // Primitive number/boolean - not a pointer
                                (None, false, false, false, false, false, false, false, false, false)
                            }
                            Some(perry_types::Type::BigInt) => {
                                (None, false, false, false, true, false, false, false, false, false)
                            }
                            Some(perry_types::Type::String) => {
                                (None, true, false, true, false, false, false, false, false, false)
                            }
                            Some(perry_types::Type::Array(_)) => {
                                (None, true, true, false, false, false, false, false, false, false)
                            }
                            Some(perry_types::Type::Generic { ref base, .. }) if base == "Map" => {
                                (None, true, false, false, false, false, true, false, false, false)
                            }
                            Some(perry_types::Type::Generic { ref base, .. }) if base == "Set" => {
                                (None, true, false, false, false, false, false, true, false, false)
                            }
                            Some(perry_types::Type::Named(ref name)) => {
                                (Some(name.clone()), true, false, false, false, false, false, false, false, false)
                            }
                            Some(_) => {
                                // Unknown/Any/other types - conservatively treat as pointer
                                // so NaN-boxing extraction runs (safe for non-pointers too)
                                (None, true, false, false, false, false, false, false, false, false)
                            }
                            None => {
                                // Can't determine map's value type - conservatively treat as pointer
                                (None, true, false, false, false, false, false, false, false, false)
                            }
                        }
                    }
                    _ => (None, false, false, false, false, false, false, false, false, false),
                }
            };

            // Check if initialized with JsonParse - need is_union for proper typeof
            let is_json_parse_init = matches!(init, Some(Expr::JsonParse(_)));

            // Check if initialized from PropertyGet on a non-class object (e.g., object destructuring)
            // These values are NaN-boxed and need dynamic handling
            let is_property_from_generic_object = matches!(init, Some(Expr::PropertyGet { object, property })
                if property != "length" && matches!(object.as_ref(), Expr::LocalGet(id)
                    if locals.get(id).map(|info| !info.is_array && !info.is_string && !info.is_map && !info.is_set && info.class_name.is_none()).unwrap_or(true)));

            // Check if initialized from JS interop expressions (JsCallFunction, JsGetExport, JsCallMethod)
            // These return NaN-boxed values that could be any type
            let is_js_interop_init = matches!(init, Some(Expr::JsCallFunction { .. }) | Some(Expr::JsGetExport { .. }) | Some(Expr::JsCallMethod { .. }));

            // NOTE: String method calls (substring, slice, trim, etc.) return NaN-boxed f64 strings.
            // is_string is set to true, and is_pointer is false, so they use f64 variable type.
            // js_get_string_pointer_unified handles both NaN-boxed and raw pointer strings.

            // Check if initialized from a conditional (ternary) expression
            // These can return different types from each branch
            let is_conditional_init = matches!(init, Some(Expr::Conditional { .. }));

            // Check if initialized from await expression
            // Await results are NaN-boxed and need dynamic typeof handling
            let is_await_init = matches!(init, Some(Expr::Await(_)));

            // Check if initialized from EnvGet/EnvGetDynamic (process.env.VAR or process.env[key])
            // EnvGet/EnvGetDynamic returns a NaN-boxed value: string if the env var exists, undefined if not
            let is_envget_init = matches!(init, Some(Expr::EnvGet(_)) | Some(Expr::EnvGetDynamic(_)));

            // Check if initialized from IndexGet (array element access)
            // Array elements from mixed/union arrays are NaN-boxed and need dynamic handling
            let is_indexget_init = if let Some(Expr::IndexGet { object, .. }) = init {
                // If the array is marked as mixed or union, elements need union handling
                if let Expr::LocalGet(arr_id) = object.as_ref() {
                    locals.get(arr_id).map(|i| i.is_mixed_array || i.is_union || i.is_array).unwrap_or(true)
                } else {
                    true // Conservative: unknown array sources need union handling
                }
            } else {
                false
            };

            // Check if initialized from a LocalGet to a variable that has is_union or is_mixed_array
            // This propagates the union/mixed flags for variables like for...of internal arrays
            let (is_localget_union, is_localget_mixed_array) = if let Some(Expr::LocalGet(src_id)) = init {
                if let Some(src_info) = locals.get(src_id) {
                    (src_info.is_union || src_info.is_mixed_array, src_info.is_mixed_array)
                } else {
                    (false, false)
                }
            } else {
                (false, false)
            };

            // Don't mark as union if we know the concrete type from expression inference
            // This prevents strings/arrays/etc with ty=Any from being treated as union
            let is_typed_generic_object_union = is_typed_generic_object && !is_string && !is_array && !is_bigint && !is_closure && !is_map && !is_set && !is_buffer;

            // For await: only mark as union if we don't know the concrete type
            let is_await_union = is_await_init && !is_pointer;
            let is_union = is_typed_union || is_typed_generic_object_union || is_json_parse_init || is_property_from_generic_object || is_js_interop_init || is_conditional_init || is_await_union || is_localget_union || is_indexget_init || is_envget_init;

            // Extract type arguments from Expr::New or from the HIR type annotation for generic types
            let type_args = if let Some(Expr::New { type_args, .. }) = init {
                type_args.clone()
            } else if let HirType::Generic { type_args, .. } = ty {
                type_args.clone()
            } else {
                Vec::new()
            };

            // Detect if the variable should be stored as native i32
            // IMPORTANT: We do NOT automatically use i32 for integer-initialized variables
            // because they can overflow (e.g., `let sum = 0` can accumulate to huge values).
            // The loop counter i32 optimization is handled separately during BCE detection,
            // which properly bounds the loop counter by the array length or constant limit.
            let should_use_i32 = false;

            let var = Variable::new(*next_var);
            *next_var += 1;

            // Use i64 for pointers, i32 for integer accumulators, f64 for other numbers
            // Union types use f64 because they contain NaN-boxed values that need the type tag
            let var_type = if is_pointer && !is_union { types::I64 } else if should_use_i32 { types::I32 } else { types::F64 };
            builder.declare_var(var, var_type);

            if let Some(init_expr) = init {
                if should_use_i32 {
                    // Initialize i32 variable directly
                    let init_val = match init_expr {
                        Expr::Integer(n) => builder.ins().iconst(types::I32, *n),
                        Expr::Number(f) => builder.ins().iconst(types::I32, *f as i64),
                        _ => {
                            // Fallback: compile expression and convert
                            let val = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, init_expr, this_ctx)?;
                            // Safe conversion: f64 -> i64 -> i32 (avoids ARM64 SIGILL on large values)
                            let val_f64 = ensure_f64(builder, val);
                            let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, val_f64);
                            builder.ins().ireduce(types::I32, i64_val)
                        }
                    };
                    builder.def_var(var, init_val);
                } else {
                    // Compile the expression and assign directly - typed expressions now return correct types
                    let val = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, init_expr, this_ctx)?;

                    // String aliasing: when `let y = x` copies a string pointer, mark the
                    // source string as shared so js_string_append won't mutate it in-place.
                    // This prevents `let y = x; x = x + "z"` from corrupting y.
                    if let Expr::LocalGet(src_id) = init_expr {
                        if let Some(src_info) = locals.get(src_id) {
                            if src_info.is_string {
                                let val_type = builder.func.dfg.value_type(val);
                                let raw_ptr = if val_type == types::I64 {
                                    val
                                } else {
                                    inline_get_string_pointer(builder, val)
                                };
                                if let Some(addref_func) = extern_funcs.get("js_string_addref") {
                                    let addref_ref = module.declare_func_in_func(*addref_func, builder.func);
                                    builder.ins().call(addref_ref, &[raw_ptr]);
                                }
                            }
                        }
                    }

                    // Check if this is a string from an array IndexGet (NaN-boxed string needs un-boxing)
                    let is_string_from_array = is_string && matches!(init_expr, Expr::IndexGet { object, .. }
                        if matches!(object.as_ref(), Expr::LocalGet(id) if locals.get(id).map(|i| i.is_array).unwrap_or(false)));

                    // For union types, we need to NaN-box pointer values (strings, objects, etc.)
                    // so they can be distinguished from regular numbers at runtime
                    let val = if is_typed_union && is_string_expr(init_expr, locals) {
                        // String expression returns f64 (bitcast from i64 pointer)
                        // Convert back to i64 and wrap with NaN-boxing using STRING_TAG
                        let ptr = ensure_i64(builder, val);
                        let nanbox_func = extern_funcs.get("js_nanbox_string")
                            .ok_or_else(|| anyhow!("js_nanbox_string not declared"))?;
                        let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                        let call = builder.ins().call(nanbox_ref, &[ptr]);
                        builder.inst_results(call)[0]
                    } else if is_string_from_array && !is_pointer {
                        // String from array is NaN-boxed - extract the raw pointer
                        // (When is_pointer=true, the general is_pointer path below handles this)
                        let get_str_ptr_func = extern_funcs.get("js_nanbox_get_string_pointer")
                            .ok_or_else(|| anyhow!("js_nanbox_get_string_pointer not declared"))?;
                        let get_str_ptr_ref = module.declare_func_in_func(*get_str_ptr_func, builder.func);
                        let call = builder.ins().call(get_str_ptr_ref, &[val]);
                        let str_ptr = builder.inst_results(call)[0];
                        // Bitcast i64 to f64 for storage (string variables use f64 representation)
                        builder.ins().bitcast(types::F64, MemFlags::new(), str_ptr)
                    } else if is_pointer && !is_union {
                        // If variable is i64 (pointer) but expression returns f64, we need to handle
                        // NaN-boxed values properly. IndexGet and PropertyGet return NaN-boxed F64
                        // values that need to have the pointer extracted, not just bitcast.
                        let val_type = builder.func.dfg.value_type(val);
                        if val_type == types::F64 {
                            if is_string || is_typed_string {
                                // FAST PATH: String values are always NaN-boxed with STRING_TAG.
                                // Use inline_get_string_pointer (3 instructions: bitcast + mask + band)
                                // instead of ensure_i64 (7 instructions) or js_nanbox_get_pointer (FFI call).
                                // Also handle is_typed_string for cross-module calls where the
                                // expression type inference (is_string_expr) can't detect the return
                                // type but the variable's declared type is String.
                                inline_get_string_pointer(builder, val)
                            } else {
                            // Check if this is a NaN-boxed expression (IndexGet, PropertyGet on generic objects, etc.)
                            let is_nanboxed_expr = match init_expr {
                                Expr::IndexGet { .. } => true,
                                // Object literals return NaN-boxed POINTER_TAG F64
                                Expr::Object(_) | Expr::ObjectSpread { .. } => true,
                                // Array/Map/Set constructors may return NaN-boxed
                                Expr::Array(_) | Expr::ArraySpread(_) | Expr::ArrayMap { .. } |
                                Expr::ArrayFilter { .. } | Expr::ArraySort { .. } | Expr::ArraySlice { .. } |
                                Expr::MapNew | Expr::SetNew | Expr::SetNewFromArray(_) |
                                Expr::MapEntries(_) | Expr::MapKeys(_) | Expr::MapValues(_) => true,
                                // Map.get() returns NaN-boxed value (may be POINTER_TAG object)
                                Expr::MapGet { .. } => true,
                                Expr::New { .. } | Expr::NewDynamic { .. } => true,
                                // Await returns NaN-boxed value from js_promise_value (F64 with POINTER_TAG)
                                Expr::Await(_) => true,
                                Expr::PropertyGet { object, property } => {
                                    // PropertyGet on generic objects (not arrays/maps/etc.) returns NaN-boxed
                                    if let Expr::LocalGet(id) = object.as_ref() {
                                        locals.get(id).map(|info| {
                                            !info.is_array && !info.is_string && !info.is_map && !info.is_set && info.class_name.is_none()
                                        }).unwrap_or(true) && property != "length"
                                    } else {
                                        true
                                    }
                                }
                                // Function calls returning F64 when stored as pointer must be NaN-boxed
                                // (e.g., closure-returning functions with Type::Any return type)
                                Expr::Call { .. } => true,
                                _ => false,
                            };

                            if is_nanboxed_expr {
                                // Extract the raw pointer from NaN-boxed value
                                let get_ptr_func = extern_funcs.get("js_nanbox_get_pointer")
                                    .ok_or_else(|| anyhow!("js_nanbox_get_pointer not declared"))?;
                                let get_ptr_ref = module.declare_func_in_func(*get_ptr_func, builder.func);
                                let call = builder.ins().call(get_ptr_ref, &[val]);
                                builder.inst_results(call)[0]
                            } else {
                                // Regular bitcast for values that are already raw pointers
                                ensure_i64(builder, val)
                            }
                            }
                        } else {
                            // Value is not F64 - need to convert to I64
                            let val_type = builder.func.dfg.value_type(val);
                            if val_type == types::I32 {
                                builder.ins().sextend(types::I64, val)
                            } else {
                                val
                            }
                        }
                    } else if is_union {
                        // Union types use NaN-boxed f64 values - keep as f64
                        let val_type = builder.func.dfg.value_type(val);
                        if val_type == types::I64 {
                            // I64 pointer (Named type, object, closure, etc.) being stored into
                            // an any/union-typed F64 variable.  Must NaN-box with POINTER_TAG so
                            // that runtime typeof checks (js_value_typeof) return "object"/"function"
                            // instead of "number".  Raw bitcast would produce a subnormal float.
                            inline_nanbox_pointer(builder, val)
                        } else if val_type == types::I32 {
                            builder.ins().fcvt_from_sint(types::F64, val)
                        } else {
                            val
                        }
                    } else {
                        // Variable is f64, but expression might return i64 (e.g., function call returning pointer)
                        // Check and convert if needed
                        let val_type = builder.func.dfg.value_type(val);
                        if val_type == types::I64 && var_type == types::F64 {
                            builder.ins().bitcast(types::F64, MemFlags::new(), val)
                        } else if val_type == types::I32 && var_type == types::F64 {
                            builder.ins().fcvt_from_sint(types::F64, val)
                        } else if val_type == types::I32 && var_type == types::I64 {
                            builder.ins().sextend(types::I64, val)
                        } else if val_type == types::F64 && var_type == types::I64 {
                            // F64 (NaN-boxed) to I64 (raw pointer) - extract pointer from NaN-boxed value
                            ensure_i64(builder, val)
                        } else {
                            val
                        }
                    };
                    builder.def_var(var, val);
                }
            } else {
                // Initialize to undefined/null/0 depending on the type.
                // Special: `declare const __platform__: number` gets the compile-time platform ID.
                // Special: `declare const __plugins__: number` gets 1 if "plugins" feature is enabled, else 0.
                if var_name == "__platform__" {
                    let platform_val = COMPILE_TARGET.with(|c| c.get()) as f64;
                    let platform_const = builder.ins().f64const(platform_val);
                    builder.def_var(var, platform_const);
                } else if var_name == "__plugins__" {
                    let plugins_val = ENABLED_FEATURES.with(|f| {
                        if f.borrow().contains("plugins") { 1.0_f64 } else { 0.0_f64 }
                    });
                    let plugins_const = builder.ins().f64const(plugins_val);
                    builder.def_var(var, plugins_const);
                } else if var_name.starts_with("__feature_") && var_name.ends_with("__") {
                    let feature_name = &var_name[10..var_name.len()-2];
                    let feature_val = ENABLED_FEATURES.with(|f| {
                        if f.borrow().contains(feature_name) { 1.0_f64 } else { 0.0_f64 }
                    });
                    let feature_const = builder.ins().f64const(feature_val);
                    builder.def_var(var, feature_const);
                } else if is_pointer && !is_union {
                    // Raw pointer type - use null pointer (0)
                    let zero = builder.ins().iconst(types::I64, 0);
                    builder.def_var(var, zero);
                } else if should_use_i32 {
                    let zero = builder.ins().iconst(types::I32, 0);
                    builder.def_var(var, zero);
                } else {
                    // f64 type - use TAG_UNDEFINED for proper JavaScript semantics
                    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
                    let undef = builder.ins().f64const(f64::from_bits(TAG_UNDEFINED));
                    builder.def_var(var, undef);
                }
            }

            // Detect if the variable contains an integer value (for native i64 optimization)
            // Only track as integer if not a pointer type and initialized with an integer expression
            let is_integer = !is_pointer && init.as_ref().map(|e| is_integer_expr(e, locals)).unwrap_or(false);

            let i32_shadow: Option<Variable> = None;

            // Track compile-time constant values for const variables with literal initializers.
            // Special case: `declare const __platform__: number` gets the compile-time platform ID
            // (0=macOS, 1=iOS, 2=Android, 3=Windows, 4=Linux) injected via COMPILE_TARGET thread-local.
            // Special case: `declare const __plugins__: number` gets 1 if "plugins" feature enabled.
            // Special case: `declare const __feature_NAME__: number` gets 1 if feature "NAME" enabled.
            let const_value = if !mutable && !is_pointer && !is_string && !is_bigint {
                if var_name == "__platform__" {
                    Some(COMPILE_TARGET.with(|c| c.get()) as f64)
                } else if var_name == "__plugins__" {
                    Some(ENABLED_FEATURES.with(|f| {
                        if f.borrow().contains("plugins") { 1.0_f64 } else { 0.0_f64 }
                    }))
                } else if var_name.starts_with("__feature_") && var_name.ends_with("__") {
                    let feature_name = &var_name[10..var_name.len()-2];
                    Some(ENABLED_FEATURES.with(|f| {
                        if f.borrow().contains(feature_name) { 1.0_f64 } else { 0.0_f64 }
                    }))
                } else {
                    match init {
                        Some(Expr::Integer(n)) => Some(*n as f64),
                        Some(Expr::Number(f)) => Some(*f),
                        _ => None,
                    }
                }
            } else {
                None
            };

            // If this variable will be captured mutably by a closure, wrap it in a heap box
            // so both the outer scope and the closure share the same storage location.
            // Reads go through js_box_get, writes through js_box_set (handled by is_boxed flag).
            let (final_var, is_boxed_var) = if boxed_vars.contains(id) {
                let orig_val = builder.use_var(var);
                // For pointer/closure variables declared as I64, the raw pointer must be NaN-boxed
                // with POINTER_TAG before storing in the box. Otherwise js_box_get returns raw bits
                // and js_nanbox_get_pointer returns null when the variable is later called/read.
                let val_f64 = if is_pointer && !is_string && !is_union {
                    let orig_type = builder.func.dfg.value_type(orig_val);
                    if orig_type == types::I64 {
                        // Raw I64 pointer — NaN-box it before boxing
                        let nanbox_func = extern_funcs.get("js_nanbox_pointer")
                            .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                        let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                        let call = builder.ins().call(nanbox_ref, &[orig_val]);
                        builder.inst_results(call)[0]
                    } else {
                        orig_val  // Already F64 NaN-boxed (e.g., from Expr::Closure)
                    }
                } else {
                    ensure_f64(builder, orig_val)
                };

                let box_alloc_func = extern_funcs.get("js_box_alloc")
                    .ok_or_else(|| anyhow!("js_box_alloc not declared"))?;
                let box_alloc_ref = module.declare_func_in_func(*box_alloc_func, builder.func);
                let box_call = builder.ins().call(box_alloc_ref, &[val_f64]);
                let box_ptr = builder.inst_results(box_call)[0]; // I64

                // Create a new variable for the box pointer (I64)
                let box_var = Variable::new(*next_var);
                *next_var += 1;
                builder.declare_var(box_var, types::I64);
                builder.def_var(box_var, box_ptr);

                (box_var, true)
            } else {
                (var, false)
            };

            // Preserve module_var_data_id from any pre-existing entry.
            // This happens when a function-local variable was promoted to module-level
            // for class method access (class capture promotion). Without this, the
            // data global association is lost and writes won't propagate to class methods.
            let existing_data_id = locals.get(id).and_then(|info| info.module_var_data_id);
            // Track class references (e.g., `const cls = MyClass`) so `new cls()` resolves correctly
            let class_ref_name = if let Some(Expr::ClassRef(name)) = init.as_ref() {
                Some(name.clone())
            } else {
                None
            };
            // Extract closure func_id if init is a Closure expression (for rest param lookup)
            let closure_func_id = if let Some(Expr::Closure { func_id: cfid, .. }) = init {
                Some(*cfid)
            } else {
                None
            };
            // Detect if the variable holds a boolean value (from comparison or bool literal)
            let is_boolean = match init {
                Some(Expr::Compare { .. }) => true,
                Some(Expr::Bool(_)) => true,
                Some(Expr::Unary { op: UnaryOp::Not, .. }) => true,
                _ => matches!(ty, HirType::Boolean),
            };
            locals.insert(*id, LocalInfo { var: final_var, name: Some(var_name.clone()), class_name, type_args, is_pointer, is_array, is_string, is_bigint, is_closure, closure_func_id, is_boxed: is_boxed_var, is_map, is_set, is_buffer, is_event_emitter, is_union, is_mixed_array, is_integer, is_integer_array: false, is_i32: should_use_i32, is_boolean, i32_shadow, bounded_by_array: None, bounded_by_constant: None, scalar_fields: None, squared_cache: None, product_cache: None, cached_array_ptr: None, const_value, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: existing_data_id, class_ref_name });

            // If this variable has a module-level data global (promoted for class capture),
            // write the initial value to the data global so class methods can read it.
            if let Some(data_id) = existing_data_id {
                let current = builder.use_var(final_var);
                let val_type = builder.func.dfg.value_type(current);
                let store_val = if val_type == types::I32 {
                    builder.ins().fcvt_from_sint(types::F64, current)
                } else {
                    current
                };
                let global_val = module.declare_data_in_func(data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                builder.ins().store(MemFlags::new(), store_val, ptr, 0);
            }
        }
        Stmt::Return(expr) => {
            // Helper to detect if a return expression is a string value
            fn is_string_return_expr(expr: &Expr, locals: &HashMap<LocalId, LocalInfo>) -> bool {
                match expr {
                    Expr::String(_) => true,
                    Expr::StringFromCharCode(_) => true,
                    Expr::ArrayJoin { .. } => true,
                    Expr::LocalGet(id) => locals.get(id).map(|i| i.is_string).unwrap_or(false),
                    Expr::Binary { op: BinaryOp::Add, left, right } => {
                        is_string_return_expr(left, locals) || is_string_return_expr(right, locals)
                    }
                    Expr::Call { callee, .. } => {
                        if let Expr::PropertyGet { object, property } = callee.as_ref() {
                            if is_string_return_expr(object, locals) {
                                return matches!(property.as_str(),
                                    "substring" | "slice" | "toLowerCase" | "toUpperCase" |
                                    "trim" | "trimStart" | "trimEnd" | "charAt" | "padStart" |
                                    "padEnd" | "repeat" | "replace" | "replaceAll" | "concat" |
                                    "toString" | "join"
                                );
                            }
                        }
                        false
                    }
                    Expr::EnvGet(_) | Expr::EnvGetDynamic(_) | Expr::FsReadFileSync(_) => true,
                    Expr::PathJoin(_, _) | Expr::PathDirname(_) | Expr::PathBasename(_) |
                    Expr::PathExtname(_) | Expr::PathResolve(_) | Expr::FileURLToPath(_) => true,
                    _ => false,
                }
            }

            // Check if a Call expression invokes a function that returns a string
            fn is_string_returning_call_r(expr: &Expr, func_hir_return_types: &HashMap<u32, perry_types::Type>) -> bool {
                match expr {
                    Expr::Call { callee, .. } => {
                        match callee.as_ref() {
                            Expr::FuncRef(id) => {
                                matches!(func_hir_return_types.get(id), Some(perry_types::Type::String))
                            }
                            Expr::ExternFuncRef { return_type, name, .. } => {
                                if matches!(return_type, perry_types::Type::String) {
                                    return true;
                                }
                                IMPORTED_FUNC_RETURN_TYPES.with(|p| {
                                    matches!(p.borrow().get(name), Some(perry_types::Type::String))
                                })
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }

            // Emit js_try_end() for each enclosing try block before returning
            let try_depth = TRY_CATCH_DEPTH.with(|d| d.get());
            emit_try_end_cleanup(builder, module, extern_funcs, try_depth)?;
            // If we're inside an async function (via try/catch), resolve the promise and return it
            if let Some(promise_var) = async_promise_var {
                let promise_ptr = builder.use_var(promise_var);
                if let Some(e) = expr {
                    let value = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, e, this_ctx)?;
                    // Resolve the promise with the value, NaN-boxing strings/objects properly
                    let value_f64 = if is_string_return_expr(e, locals) || is_string_returning_call_r(e, func_hir_return_types) {
                        // String pointer needs NaN-boxing with STRING_TAG
                        let ptr = ensure_i64(builder, value);
                        let nanbox_func = extern_funcs.get("js_nanbox_string")
                            .ok_or_else(|| anyhow!("js_nanbox_string not declared"))?;
                        let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                        let call = builder.ins().call(nanbox_ref, &[ptr]);
                        builder.inst_results(call)[0]
                    } else {
                        ensure_f64(builder, value)
                    };
                    let resolve_func = extern_funcs.get("js_promise_resolve")
                        .ok_or_else(|| anyhow!("js_promise_resolve not declared"))?;
                    let resolve_ref = module.declare_func_in_func(*resolve_func, builder.func);
                    builder.ins().call(resolve_ref, &[promise_ptr, value_f64]);
                } else {
                    // Resolve with undefined
                    let resolve_func = extern_funcs.get("js_promise_resolve")
                        .ok_or_else(|| anyhow!("js_promise_resolve not declared"))?;
                    let resolve_ref = module.declare_func_in_func(*resolve_func, builder.func);
                    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
                    let undef_val = builder.ins().f64const(f64::from_bits(TAG_UNDEFINED));
                    builder.ins().call(resolve_ref, &[promise_ptr, undef_val]);
                }
                // Return the promise pointer, NaN-boxed if function signature expects F64
                let ret_type = builder.func.signature.returns.first().map(|p| p.value_type).unwrap_or(types::I64);
                let ret_val = if ret_type == types::F64 {
                    // Closure or function returning F64 - NaN-box the promise pointer
                    let nanbox_func = extern_funcs.get("js_nanbox_pointer")
                        .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                    let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                    let call = builder.ins().call(nanbox_ref, &[promise_ptr]);
                    builder.inst_results(call)[0]
                } else {
                    promise_ptr
                };
                builder.ins().return_(&[ret_val]);
            } else {
                // Check if this is a void function (no return type) - e.g., constructors
                let is_void = builder.func.signature.returns.is_empty();

                if is_void {
                    // Void function - return without a value
                    // If there's an expression, compile it for side effects but don't return it
                    if let Some(e) = expr {
                        compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, e, this_ctx)?;
                    }
                    builder.ins().return_(&[]);
                } else {
                    // Function has return type
                    let ret_type = builder.func.signature.returns.first().map(|p| p.value_type).unwrap_or(types::F64);

                    let val = if let Some(e) = expr {
                        compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, e, this_ctx)?
                    } else {
                        // Return 0 in the appropriate type for the function signature
                        match ret_type {
                            types::I32 => builder.ins().iconst(types::I32, 0),
                            types::I64 => builder.ins().iconst(types::I64, 0),
                            _ => builder.ins().f64const(0.0),
                        }
                    };
                    // Check if return type expects i64 but expression returns f64 (or vice versa)
                    // This handles cases like returning array literals from functions with array return type
                    let val_type = builder.func.dfg.value_type(val);
                    let val = if ret_type == types::I64 && val_type == types::F64 {
                        // Expression returned f64, need i64 - bitcast
                        ensure_i64(builder, val)
                    } else if ret_type == types::F64 && val_type == types::I64 {
                        // Expression returned i64 (pointer), NaN-box it for function returning f64
                        // Use STRING_TAG for string values, POINTER_TAG for objects
                        let is_string_return = if let Some(e) = expr {
                            is_string_return_expr(e, locals)
                        } else {
                            false
                        };
                        if is_string_return {
                            inline_nanbox_string(builder, val)
                        } else {
                            inline_nanbox_pointer(builder, val)
                        }
                    } else if ret_type == types::I32 && val_type == types::F64 {
                        // Expression returned f64, need i32 - safe truncate via i64
                        let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, val);
                        builder.ins().ireduce(types::I32, i64_val)
                    } else if ret_type == types::I32 && val_type == types::I64 {
                        // Expression returned i64, need i32 - truncate
                        builder.ins().ireduce(types::I32, val)
                    } else {
                        val
                    };
                    builder.ins().return_(&[val]);
                }
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            let cond_bool = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;

            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();

            builder.ins().brif(cond_bool, then_block, &[], else_block, &[]);

            // Then branch
            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            for s in then_branch {
                compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, loop_ctx, boxed_vars, async_promise_var)?;
            }
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                builder.ins().jump(merge_block, &[]);
            }

            // Else branch
            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, loop_ctx, boxed_vars, async_promise_var)?;
                }
            }
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                builder.ins().jump(merge_block, &[]);
            }

            // Merge
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
        }
        Stmt::While { condition, body } => {
            // WHILE LOOP I32 COUNTER OPTIMIZATION
            // Detect patterns like: while (... && iter < limit) { ...; iter = iter + 1; }
            // If found, keep iter as native i32 for faster arithmetic and comparison

            // CSE: Find var * var expressions (squared variables)
            fn find_squared_vars_in_expr(expr: &Expr, vars: &mut HashSet<LocalId>) {
                match expr {
                    Expr::Binary { op: BinaryOp::Mul, left, right } => {
                        if let (Expr::LocalGet(l_id), Expr::LocalGet(r_id)) = (left.as_ref(), right.as_ref()) {
                            if l_id == r_id {
                                vars.insert(*l_id);
                            }
                        }
                        find_squared_vars_in_expr(left, vars);
                        find_squared_vars_in_expr(right, vars);
                    }
                    Expr::Binary { left, right, .. } |
                    Expr::Compare { left, right, .. } |
                    Expr::Logical { left, right, .. } => {
                        find_squared_vars_in_expr(left, vars);
                        find_squared_vars_in_expr(right, vars);
                    }
                    Expr::Unary { operand, .. } => find_squared_vars_in_expr(operand, vars),
                    Expr::LocalSet(_, val) => find_squared_vars_in_expr(val, vars),
                    _ => {}
                }
            }

            fn find_squared_vars_in_stmt(stmt: &Stmt, vars: &mut HashSet<LocalId>) {
                match stmt {
                    Stmt::Expr(e) => find_squared_vars_in_expr(e, vars),
                    Stmt::Let { init: Some(e), .. } => find_squared_vars_in_expr(e, vars),
                    _ => {}
                }
            }

            // Helper to find counter pattern in condition: returns (counter_id, limit_expr)
            fn find_counter_in_condition(cond: &Expr) -> Option<(LocalId, &Expr)> {
                match cond {
                    // Direct: iter < limit
                    Expr::Compare { op: CompareOp::Lt, left, right } => {
                        if let Expr::LocalGet(id) = left.as_ref() {
                            Some((*id, right.as_ref()))
                        } else { None }
                    }
                    // AND condition: ... && iter < limit
                    Expr::Logical { op: LogicalOp::And, left, right } => {
                        // Check right side first (more common pattern)
                        find_counter_in_condition(right)
                            .or_else(|| find_counter_in_condition(left))
                    }
                    _ => None,
                }
            }

            // Helper to check if body increments the counter: iter = iter + 1
            fn body_increments_counter(body: &[Stmt], counter_id: LocalId) -> bool {
                for stmt in body {
                    if let Stmt::Expr(expr) = stmt {
                        match expr {
                            Expr::LocalSet(id, value) if *id == counter_id => {
                                if let Expr::Binary { op: BinaryOp::Add, left, right } = value.as_ref() {
                                    if let Expr::LocalGet(left_id) = left.as_ref() {
                                        if *left_id == counter_id {
                                            if matches!(right.as_ref(), Expr::Integer(1)) {
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                            Expr::Update { id, op: UpdateOp::Increment, .. } if *id == counter_id => {
                                return true;
                            }
                            _ => {}
                        }
                    }
                }
                false
            }

            // Try to detect and optimize integer counter
            let mut counter_opt: Option<(LocalId, Variable, Variable)> = None;  // (counter_id, i32_var, limit_var)
            let mut original_f64_var: Option<Variable> = None;

            if let Some((counter_id, limit_expr)) = find_counter_in_condition(condition) {
                if body_increments_counter(body, counter_id) {
                    // Get limit as i32
                    let limit_i32 = match limit_expr {
                        Expr::Integer(n) if *n >= 0 && *n <= i32::MAX as i64 => {
                            Some(builder.ins().iconst(types::I32, *n))
                        }
                        Expr::Number(n) if *n >= 0.0 && *n <= i32::MAX as f64 && n.fract() == 0.0 => {
                            Some(builder.ins().iconst(types::I32, *n as i64))
                        }
                        Expr::LocalGet(limit_id) => {
                            if let Some(limit_info) = locals.get(limit_id) {
                                if limit_info.is_i32 {
                                    Some(builder.use_var(limit_info.var))
                                } else {
                                    let limit_val = builder.use_var(limit_info.var);
                                    // Safe conversion: f64 -> i64 -> i32 (avoids ARM64 SIGILL)
                                    let limit_f64 = ensure_f64(builder, limit_val);
                                    let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, limit_f64);
                                    Some(builder.ins().ireduce(types::I32, i64_val))
                                }
                            } else { None }
                        }
                        // GlobalGet (like MAX_ITER) - skip for now, complex to handle
                        _ => None,
                    };

                    if let Some(limit_val) = limit_i32 {
                        if let Some(counter_info) = locals.get(&counter_id) {
                            if !counter_info.is_i32 {
                                // Cache limit in a variable
                                let limit_var = Variable::new(*next_var);
                                *next_var += 1;
                                builder.declare_var(limit_var, types::I32);
                                builder.def_var(limit_var, limit_val);

                                // Create i32 variable for counter
                                let i32_var = Variable::new(*next_var);
                                *next_var += 1;
                                builder.declare_var(i32_var, types::I32);

                                // Initialize from current value (may be i64 if NaN-boxed)
                                // Safe conversion: f64 -> i64 -> i32 (avoids ARM64 SIGILL)
                                let counter_val = builder.use_var(counter_info.var);
                                let counter_f64 = ensure_f64(builder, counter_val);
                                let counter_i64 = builder.ins().fcvt_to_sint_sat(types::I64, counter_f64);
                                let counter_i32 = builder.ins().ireduce(types::I32, counter_i64);
                                builder.def_var(i32_var, counter_i32);

                                // Store original f64 var for restoration
                                original_f64_var = Some(counter_info.var);

                                // Update LocalInfo to use i32 variable
                                if let Some(info) = locals.get_mut(&counter_id) {
                                    info.var = i32_var;
                                    info.is_i32 = true;
                                }

                                counter_opt = Some((counter_id, i32_var, limit_var));
                            }
                        }
                    }
                }
            }

            // CSE OPTIMIZATION: Detect var * var and var * other_var patterns
            // If found in both condition and body, cache at the start of each iteration

            // Helper to find product pairs (x * y where x != y)
            // Also detects patterns like (const * x) * y -> record (x, y)
            fn find_product_pairs_in_expr(expr: &Expr, pairs: &mut HashSet<(LocalId, LocalId)>) {
                match expr {
                    Expr::Binary { op: BinaryOp::Mul, left, right } => {
                        // Direct pattern: x * y
                        if let (Expr::LocalGet(l_id), Expr::LocalGet(r_id)) = (left.as_ref(), right.as_ref()) {
                            if l_id != r_id {
                                let pair = if l_id < r_id { (*l_id, *r_id) } else { (*r_id, *l_id) };
                                pairs.insert(pair);
                            }
                        }
                        // Pattern: (const * x) * y or (x * const) * y
                        if let Expr::Binary { op: BinaryOp::Mul, left: inner_left, right: inner_right } = left.as_ref() {
                            // (const * x) * y
                            if let (Expr::Number(_) | Expr::Integer(_), Expr::LocalGet(x_id)) = (inner_left.as_ref(), inner_right.as_ref()) {
                                if let Expr::LocalGet(y_id) = right.as_ref() {
                                    if x_id != y_id {
                                        let pair = if x_id < y_id { (*x_id, *y_id) } else { (*y_id, *x_id) };
                                        pairs.insert(pair);
                                    }
                                }
                            }
                            // (x * const) * y
                            if let (Expr::LocalGet(x_id), Expr::Number(_) | Expr::Integer(_)) = (inner_left.as_ref(), inner_right.as_ref()) {
                                if let Expr::LocalGet(y_id) = right.as_ref() {
                                    if x_id != y_id {
                                        let pair = if x_id < y_id { (*x_id, *y_id) } else { (*y_id, *x_id) };
                                        pairs.insert(pair);
                                    }
                                }
                            }
                        }
                        // Pattern: x * (const * y) or x * (y * const)
                        if let Expr::Binary { op: BinaryOp::Mul, left: inner_left, right: inner_right } = right.as_ref() {
                            // x * (const * y)
                            if let (Expr::Number(_) | Expr::Integer(_), Expr::LocalGet(y_id)) = (inner_left.as_ref(), inner_right.as_ref()) {
                                if let Expr::LocalGet(x_id) = left.as_ref() {
                                    if x_id != y_id {
                                        let pair = if x_id < y_id { (*x_id, *y_id) } else { (*y_id, *x_id) };
                                        pairs.insert(pair);
                                    }
                                }
                            }
                            // x * (y * const)
                            if let (Expr::LocalGet(y_id), Expr::Number(_) | Expr::Integer(_)) = (inner_left.as_ref(), inner_right.as_ref()) {
                                if let Expr::LocalGet(x_id) = left.as_ref() {
                                    if x_id != y_id {
                                        let pair = if x_id < y_id { (*x_id, *y_id) } else { (*y_id, *x_id) };
                                        pairs.insert(pair);
                                    }
                                }
                            }
                        }
                        find_product_pairs_in_expr(left, pairs);
                        find_product_pairs_in_expr(right, pairs);
                    }
                    Expr::Binary { left, right, .. } |
                    Expr::Compare { left, right, .. } |
                    Expr::Logical { left, right, .. } => {
                        find_product_pairs_in_expr(left, pairs);
                        find_product_pairs_in_expr(right, pairs);
                    }
                    Expr::Unary { operand, .. } => find_product_pairs_in_expr(operand, pairs),
                    Expr::LocalSet(_, val) => find_product_pairs_in_expr(val, pairs),
                    _ => {}
                }
            }

            fn find_product_pairs_in_stmt(stmt: &Stmt, pairs: &mut HashSet<(LocalId, LocalId)>) {
                match stmt {
                    Stmt::Expr(e) => find_product_pairs_in_expr(e, pairs),
                    Stmt::Let { init: Some(e), .. } => find_product_pairs_in_expr(e, pairs),
                    _ => {}
                }
            }

            let mut cse_squared_vars: Vec<LocalId> = Vec::new();
            let mut cse_product_pairs: Vec<(LocalId, LocalId)> = Vec::new();
            {
                let mut cond_squared: HashSet<LocalId> = HashSet::new();
                let mut body_squared: HashSet<LocalId> = HashSet::new();
                let mut cond_products: HashSet<(LocalId, LocalId)> = HashSet::new();
                let mut body_products: HashSet<(LocalId, LocalId)> = HashSet::new();

                find_squared_vars_in_expr(condition, &mut cond_squared);
                find_product_pairs_in_expr(condition, &mut cond_products);
                for stmt in body.iter() {
                    find_squared_vars_in_stmt(stmt, &mut body_squared);
                    find_product_pairs_in_stmt(stmt, &mut body_products);
                }

                // Find intersection - vars that are squared in both condition and body
                for var_id in cond_squared.intersection(&body_squared) {
                    cse_squared_vars.push(*var_id);
                }

                // Find product pairs used in body (for mandelbrot, x*y is only in body, not condition)
                // But we still benefit from caching it since it's recomputed after x and y change
                for pair in &body_products {
                    cse_product_pairs.push(*pair);
                }
            }

            // Create cache variables for CSE and set up squared_cache in LocalInfo
            for var_id in &cse_squared_vars {
                let cache_var = Variable::new(*next_var);
                *next_var += 1;
                builder.declare_var(cache_var, types::F64);
                // Initialize to 0 (will be computed at loop header)
                let zero = builder.ins().f64const(0.0);
                builder.def_var(cache_var, zero);

                if let Some(info) = locals.get_mut(var_id) {
                    info.squared_cache = Some(cache_var);
                }
            }

            // Create cache variables for product pairs and set up product_cache in LocalInfo
            let mut product_cache_vars: HashMap<(LocalId, LocalId), Variable> = HashMap::new();
            for (id1, id2) in &cse_product_pairs {
                let cache_var = Variable::new(*next_var);
                *next_var += 1;
                builder.declare_var(cache_var, types::F64);
                let zero = builder.ins().f64const(0.0);
                builder.def_var(cache_var, zero);

                product_cache_vars.insert((*id1, *id2), cache_var);

                // Set up product_cache in both variables' LocalInfo
                if let Some(info) = locals.get_mut(id1) {
                    if info.product_cache.is_none() {
                        info.product_cache = Some(HashMap::new());
                    }
                    if let Some(ref mut pc) = info.product_cache {
                        pc.insert(*id2, cache_var);
                    }
                }
                if let Some(info) = locals.get_mut(id2) {
                    if info.product_cache.is_none() {
                        info.product_cache = Some(HashMap::new());
                    }
                    if let Some(ref mut pc) = info.product_cache {
                        pc.insert(*id1, cache_var);
                    }
                }
            }

            let header_block = builder.create_block();
            let body_block = builder.create_block();
            let exit_block = builder.create_block();

            builder.ins().jump(header_block, &[]);

            // Header (condition check)
            builder.switch_to_block(header_block);

            // CSE: Compute squared values at start of each iteration
            for var_id in &cse_squared_vars {
                if let Some(info) = locals.get(var_id) {
                    if let Some(cache_var) = info.squared_cache {
                        let val = builder.use_var(info.var);
                        let squared = builder.ins().fmul(val, val);
                        builder.def_var(cache_var, squared);
                    }
                }
            }

            // CSE: Compute product pairs at start of each iteration
            for ((id1, id2), cache_var) in &product_cache_vars {
                if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                    let val1 = builder.use_var(info1.var);
                    let val2 = builder.use_var(info2.var);
                    let product = builder.ins().fmul(val1, val2);
                    builder.def_var(*cache_var, product);
                }
            }

            // Compile condition - if we have i32 counter, optimize the counter < limit part
            if let Some((counter_id, i32_var, limit_var)) = counter_opt {
                // Compile condition with optimized i32 comparison for the counter
                let cond_bool = match condition {
                    // Direct: iter < limit - use icmp
                    Expr::Compare { op: CompareOp::Lt, left, right: cmp_right } => {
                        if let Expr::LocalGet(id) = left.as_ref() {
                            if *id == counter_id {
                                let counter_i32 = builder.use_var(i32_var);
                                let limit_i32 = builder.use_var(limit_var);
                                builder.ins().icmp(IntCC::SignedLessThan, counter_i32, limit_i32)
                            } else {
                                // Direct Compare::Lt - compile as fcmp (no js_is_truthy needed)
                                let lhs = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, left, this_ctx)?;
                                let rhs = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, cmp_right, this_ctx)?;
                                let lhs_f64 = ensure_f64(builder, lhs);
                                let rhs_f64 = ensure_f64(builder, rhs);
                                builder.ins().fcmp(FloatCC::LessThan, lhs_f64, rhs_f64)
                            }
                        } else {
                            // Direct Compare::Lt - compile as fcmp (no js_is_truthy needed)
                            let lhs = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, left, this_ctx)?;
                            let rhs = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, cmp_right, this_ctx)?;
                            let lhs_f64 = ensure_f64(builder, lhs);
                            let rhs_f64 = ensure_f64(builder, rhs);
                            builder.ins().fcmp(FloatCC::LessThan, lhs_f64, rhs_f64)
                        }
                    }
                    // AND condition: ... && iter < limit
                    Expr::Logical { op: LogicalOp::And, left, right } => {
                        if let Expr::Compare { op: CompareOp::Lt, left: cmp_left, .. } = right.as_ref() {
                            if let Expr::LocalGet(id) = cmp_left.as_ref() {
                                if *id == counter_id {
                                    // Compile left side - use compile_condition_to_bool which correctly
                                    // handles BigInt comparisons (via js_bigint_cmp), string comparisons,
                                    // and boolean comparisons. Using fcmp directly would break BigInt !== 0n
                                    // (different pointers = always "not equal" via fcmp, causing infinite loops).
                                    let left_bool = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, left, this_ctx)?;

                                    // Use icmp for counter comparison
                                    let counter_i32 = builder.use_var(i32_var);
                                    let limit_i32 = builder.use_var(limit_var);
                                    let right_bool = builder.ins().icmp(IntCC::SignedLessThan, counter_i32, limit_i32);

                                    // AND them together
                                    builder.ins().band(left_bool, right_bool)
                                } else {
                                    compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?
                                }
                            } else {
                                compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?
                            }
                        } else {
                            compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?
                        }
                    }
                    // Other conditions - use compile_condition_to_bool for all patterns
                    _ => {
                        compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?
                    }
                };
                builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);
            } else {
                // Non-optimized path - use compile_condition_to_bool for all patterns
                let cond_bool = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);
            }

            // Create loop context for break/continue
            let while_loop_ctx = LoopContext { exit_block, header_block, bounded_indices: HashMap::new(), try_depth: TRY_CATCH_DEPTH.with(|d| d.get()) };

            // Body - with optional unrolling for CSE loops
            builder.switch_to_block(body_block);
            builder.seal_block(body_block);

            // WHILE LOOP UNROLLING: For loops with CSE, unroll by 8 to reduce branch overhead
            let should_unroll = false; // Disabled: while-loop unrolling bloats code beyond i-cache/LSD limits, hurting mandelbrot-style tight loops

            // OPTIMIZATION: Defer module-level variable write-backs for simple while loops
            let mut deferred_while_vars: HashSet<LocalId> = HashSet::new();

            if should_unroll {
                // First iteration
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                // Create blocks for iterations 2-8
                let check2_block = builder.create_block();
                let body2_block = builder.create_block();
                let check3_block = builder.create_block();
                let body3_block = builder.create_block();
                let check4_block = builder.create_block();
                let body4_block = builder.create_block();
                let check5_block = builder.create_block();
                let body5_block = builder.create_block();
                let check6_block = builder.create_block();
                let body6_block = builder.create_block();
                let check7_block = builder.create_block();
                let body7_block = builder.create_block();
                let check8_block = builder.create_block();
                let body8_block = builder.create_block();

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check2_block, &[]);
                }

                // Check and iteration 2
                builder.switch_to_block(check2_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_bool2 = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                builder.ins().brif(cond_bool2, body2_block, &[], exit_block, &[]);

                builder.switch_to_block(body2_block);
                builder.seal_block(body2_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check3_block, &[]);
                }

                // Check and iteration 3
                builder.switch_to_block(check3_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_bool3 = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                builder.ins().brif(cond_bool3, body3_block, &[], exit_block, &[]);

                builder.switch_to_block(body3_block);
                builder.seal_block(body3_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check4_block, &[]);
                }

                // Check and iteration 4
                builder.switch_to_block(check4_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_bool4 = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                builder.ins().brif(cond_bool4, body4_block, &[], exit_block, &[]);

                builder.switch_to_block(body4_block);
                builder.seal_block(body4_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check5_block, &[]);
                }

                // Check and iteration 5
                builder.switch_to_block(check5_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_val5_raw = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                let cond_val5 = ensure_f64(builder, cond_val5_raw);
                let zero5 = builder.ins().f64const(0.0);
                let cond_bool5 = builder.ins().fcmp(FloatCC::NotEqual, cond_val5, zero5);
                builder.ins().brif(cond_bool5, body5_block, &[], exit_block, &[]);

                builder.switch_to_block(body5_block);
                builder.seal_block(body5_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check6_block, &[]);
                }

                // Check and iteration 6
                builder.switch_to_block(check6_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_val6_raw = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                let cond_val6 = ensure_f64(builder, cond_val6_raw);
                let zero6 = builder.ins().f64const(0.0);
                let cond_bool6 = builder.ins().fcmp(FloatCC::NotEqual, cond_val6, zero6);
                builder.ins().brif(cond_bool6, body6_block, &[], exit_block, &[]);

                builder.switch_to_block(body6_block);
                builder.seal_block(body6_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check7_block, &[]);
                }

                // Check and iteration 7
                builder.switch_to_block(check7_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_val7_raw = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                let cond_val7 = ensure_f64(builder, cond_val7_raw);
                let zero7 = builder.ins().f64const(0.0);
                let cond_bool7 = builder.ins().fcmp(FloatCC::NotEqual, cond_val7, zero7);
                builder.ins().brif(cond_bool7, body7_block, &[], exit_block, &[]);

                builder.switch_to_block(body7_block);
                builder.seal_block(body7_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(check8_block, &[]);
                }

                // Check and iteration 8
                builder.switch_to_block(check8_block);
                for var_id in &cse_squared_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(cache_var) = info.squared_cache {
                            let val = builder.use_var(info.var);
                            let squared = builder.ins().fmul(val, val);
                            builder.def_var(cache_var, squared);
                        }
                    }
                }
                for ((id1, id2), cache_var) in &product_cache_vars {
                    if let (Some(info1), Some(info2)) = (locals.get(id1), locals.get(id2)) {
                        let val1 = builder.use_var(info1.var);
                        let val2 = builder.use_var(info2.var);
                        let product = builder.ins().fmul(val1, val2);
                        builder.def_var(*cache_var, product);
                    }
                }
                let cond_val8_raw = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, condition, this_ctx)?;
                let cond_val8 = ensure_f64(builder, cond_val8_raw);
                let zero8 = builder.ins().f64const(0.0);
                let cond_bool8 = builder.ins().fcmp(FloatCC::NotEqual, cond_val8, zero8);
                builder.ins().brif(cond_bool8, body8_block, &[], exit_block, &[]);

                builder.switch_to_block(body8_block);
                builder.seal_block(body8_block);
                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }

                // Seal check blocks (now that all predecessors are known)
                builder.seal_block(check2_block);
                builder.seal_block(check3_block);
                builder.seal_block(check4_block);
                builder.seal_block(check5_block);
                builder.seal_block(check6_block);
                builder.seal_block(check7_block);
                builder.seal_block(check8_block);

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(header_block, &[]);
                }
            } else {
                // Normal (non-unrolled) body

                // Populate deferred write-back set for simple while loops
                if !loop_body_has_calls(body) {
                    deferred_while_vars = collect_module_var_writes_in_loop(body, locals);
                    if !deferred_while_vars.is_empty() {
                        DEFERRED_MODULE_WRITEBACK_VARS.with(|d| {
                            d.borrow_mut().extend(deferred_while_vars.iter().copied());
                        });
                    }
                }

                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&while_loop_ctx), boxed_vars, async_promise_var)?;
                }
                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(header_block, &[]);
                }

                // Clear deferred set before flushing
                if !deferred_while_vars.is_empty() {
                    DEFERRED_MODULE_WRITEBACK_VARS.with(|d| {
                        let mut set = d.borrow_mut();
                        for id in &deferred_while_vars {
                            set.remove(id);
                        }
                    });
                }
            }

            // Now seal header - all predecessors (entry jump + back-edge) are known
            builder.seal_block(header_block);

            // Exit
            builder.switch_to_block(exit_block);
            builder.seal_block(exit_block);

            // Flush deferred module-level variable write-backs after while loop exit
            for var_id in &deferred_while_vars {
                if let Some(info) = locals.get(var_id) {
                    if let Some(data_id) = info.module_var_data_id {
                        let current_val = builder.use_var(info.var);
                        let val_type = builder.func.dfg.value_type(current_val);
                        let store_val = if val_type == types::I32 {
                            builder.ins().fcvt_from_sint(types::F64, current_val)
                        } else {
                            current_val
                        };
                        let global_val = module.declare_data_in_func(data_id, builder.func);
                        let ptr = builder.ins().global_value(types::I64, global_val);
                        builder.ins().store(MemFlags::new(), store_val, ptr, 0);
                    }
                }
            }

            // Restore f64 variable after loop
            if let Some((counter_id, i32_var, _)) = counter_opt {
                if let Some(orig_f64_var) = original_f64_var {
                    // Convert final i32 value back to f64
                    let final_i32 = builder.use_var(i32_var);
                    let final_f64 = builder.ins().fcvt_from_sint(types::F64, final_i32);
                    builder.def_var(orig_f64_var, final_f64);

                    // Restore LocalInfo
                    if let Some(info) = locals.get_mut(&counter_id) {
                        info.var = orig_f64_var;
                        info.is_i32 = false;
                    }
                }
            }

            // Clear CSE squared_cache after loop
            for var_id in &cse_squared_vars {
                if let Some(info) = locals.get_mut(var_id) {
                    info.squared_cache = None;
                }
            }
        }
        Stmt::For { init, condition, update, body } => {
            // Execute init statement (if any) in current block (outside loop context)
            if let Some(init_stmt) = init {
                compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, init_stmt, this_ctx, loop_ctx, boxed_vars, async_promise_var)?;
            }

            // LOOP COUNTER OPTIMIZATION: Detect patterns `i < arr.length` or `i < CONSTANT`
            // If found, use native i32 for loop counter to eliminate f64 conversion overhead
            let mut bounded_indices: HashMap<LocalId, (LocalId, Value)> = HashMap::new();
            let mut cached_length_var: Option<Variable> = None;  // Stores i32 limit
            let mut bce_index_var: Option<LocalId> = None;
            let mut bce_array_var: Option<LocalId> = None;
            let mut bce_constant_limit: Option<i64> = None;  // For constant bound BCE
            let mut original_f64_var: Option<Variable> = None;  // Store original f64 var for restoration

            if let Some(cond) = condition {
                // Check for pattern: Compare { op: Lt, left: LocalGet(i), right: ... }
                if let Expr::Compare { left, op: CompareOp::Lt, right } = cond {
                    if let Expr::LocalGet(index_id) = left.as_ref() {
                        let is_boxed_var = locals.get(index_id).map(|i| i.is_boxed).unwrap_or(false);
                        let idx_var = locals.get(index_id).map(|i| i.var);
                        let limit_i32: Option<Value> = if is_boxed_var { None } else { match right.as_ref() {
                            // Pattern 1: i < arr.length
                            Expr::PropertyGet { object, property } if property == "length" => {
                                if let Expr::LocalGet(array_id) = object.as_ref() {
                                    if let Some(arr_info) = locals.get(array_id) {
                                        if arr_info.is_array {
                                            let arr_val = builder.use_var(arr_info.var);
                                            // Always use js_nanbox_get_pointer to safely extract the array pointer.
                                            // Array values may come from various sources (Expr::Array, ArrayMap,
                                            // await results, etc.) with different pointer representations.
                                            // js_nanbox_get_pointer handles all cases by masking to 48 bits.
                                            let arr_f64 = ensure_f64(builder, arr_val);
                                            let get_ptr_func = extern_funcs.get("js_nanbox_get_pointer")
                                                .ok_or_else(|| anyhow!("js_nanbox_get_pointer not declared"))?;
                                            let get_ptr_ref = module.declare_func_in_func(*get_ptr_func, builder.func);
                                            let call = builder.ins().call(get_ptr_ref, &[arr_f64]);
                                            let arr_ptr = builder.inst_results(call)[0];
                                            let length_i32 = builder.ins().load(types::I32, MemFlags::new(), arr_ptr, 0);
                                            bce_array_var = Some(*array_id);
                                            Some(length_i32)
                                        } else { None }
                                    } else { None }
                                } else { None }
                            }
                            // Pattern 2: i < INTEGER_CONSTANT
                            Expr::Integer(n) if *n >= 0 && *n <= i32::MAX as i64 => {
                                bce_constant_limit = Some(*n);
                                Some(builder.ins().iconst(types::I32, *n))
                            }
                            // Pattern 3: i < FLOAT_CONSTANT (if it's a whole number)
                            Expr::Number(f) if *f >= 0.0 && *f <= i32::MAX as f64 && f.fract() == 0.0 => {
                                bce_constant_limit = Some(*f as i64);
                                Some(builder.ins().iconst(types::I32, *f as i64))
                            }
                            // Pattern 4: i < other_local (if it's an integer variable)
                            Expr::LocalGet(limit_id) => {
                                if let Some(limit_info) = locals.get(limit_id) {
                                    if limit_info.is_i32 {
                                        Some(builder.use_var(limit_info.var))
                                    } else if limit_info.is_integer {
                                        // Safe conversion: f64 -> i64 -> i32 (avoids ARM64 SIGILL)
                                        let limit_val = builder.use_var(limit_info.var);
                                        let limit_f64 = ensure_f64(builder, limit_val);
                                        let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, limit_f64);
                                        Some(builder.ins().ireduce(types::I32, i64_val))
                                    } else { None }
                                } else { None }
                            }
                            _ => None,
                        } };

                        if let (Some(idx_v), Some(limit_val)) = (idx_var, limit_i32) {
                            // Check if the index variable is already i32
                            let idx_already_i32 = locals.get(index_id).map(|i| i.is_i32).unwrap_or(false);

                            if idx_already_i32 {
                                // Variable is already i32, just cache the limit
                                let len_var = Variable::new(*next_var);
                                *next_var += 1;
                                builder.declare_var(len_var, types::I32);
                                builder.def_var(len_var, limit_val);

                                cached_length_var = Some(len_var);
                                bce_index_var = Some(*index_id);
                                if let Some(arr_id) = bce_array_var {
                                    bounded_indices.insert(*index_id, (arr_id, limit_val));
                                }
                                // No need to store original_f64_var since it's already i32
                            } else {
                                // Found optimizable pattern! Use native i32 for loop counter

                                // Cache limit as i32 variable
                                let len_var = Variable::new(*next_var);
                                *next_var += 1;
                                builder.declare_var(len_var, types::I32);
                                builder.def_var(len_var, limit_val);

                                // Create new i32 variable to replace the f64 one
                                let i32_var = Variable::new(*next_var);
                                *next_var += 1;
                                builder.declare_var(i32_var, types::I32);

                                // Initialize from current value (may be i64 if NaN-boxed)
                                // Safe conversion: f64 -> i64 -> i32 (avoids ARM64 SIGILL)
                                let idx_val = builder.use_var(idx_v);
                                let idx_f64 = ensure_f64(builder, idx_val);
                                let idx_i64 = builder.ins().fcvt_to_sint_sat(types::I64, idx_f64);
                                let idx_i32 = builder.ins().ireduce(types::I32, idx_i64);
                                builder.def_var(i32_var, idx_i32);

                                cached_length_var = Some(len_var);
                                bce_index_var = Some(*index_id);
                                if let Some(arr_id) = bce_array_var {
                                    bounded_indices.insert(*index_id, (arr_id, limit_val));
                                }

                                // Store original f64 var and update LocalInfo to use i32 variable
                                original_f64_var = Some(idx_v);
                                if let Some(idx_info) = locals.get_mut(index_id) {
                                    idx_info.var = i32_var;      // Replace variable
                                    idx_info.is_i32 = true;      // Mark as i32
                                    idx_info.i32_shadow = None;  // No longer need shadow
                                }
                            }
                        }
                    }
                }
            }

            // LOOP UNROLLING: Unroll by factor of 8 when profitable
            // Conditions: BCE optimization active, no break/continue in body, update is simple i++ or i = i + constant
            const UNROLL_FACTOR: i64 = 8;

            // Detect loop stride: 1 for i++, N for i = i + N
            let loop_stride: Option<i64> = update.as_ref().and_then(|u| {
                match u {
                    // i++ has stride 1
                    Expr::Update { op: UpdateOp::Increment, .. } => Some(1),
                    // i = i + constant has stride = constant
                    Expr::LocalSet(set_id, value) => {
                        if let Expr::Binary { op: BinaryOp::Add, left, right } = value.as_ref() {
                            if let Expr::LocalGet(get_id) = left.as_ref() {
                                if get_id == set_id {
                                    match right.as_ref() {
                                        Expr::Integer(n) if *n > 0 && *n <= 1000 => Some(*n),
                                        _ => None,
                                    }
                                } else { None }
                            } else { None }
                        } else { None }
                    }
                    _ => None,
                }
            });

            let can_unroll = cached_length_var.is_some()
                && bce_index_var.is_some()
                && !contains_loop_control(body)
                && loop_stride.is_some();

            // LICM helper functions (shared between unrolled and non-unrolled paths)
            // Hoist invariant array element loads out of inner loops
            fn collect_invariant_array_loads_expr(
                expr: &Expr,
                counter_id: LocalId,
                pairs: &mut Vec<(LocalId, LocalId)>,
                assigned_in_loop: &HashSet<LocalId>,
            ) {
                match expr {
                    Expr::IndexGet { object, index } => {
                        if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) {
                            if *idx_id != counter_id && !assigned_in_loop.contains(idx_id) {
                                if !pairs.iter().any(|(a, i)| a == arr_id && i == idx_id) {
                                    pairs.push((*arr_id, *idx_id));
                                }
                            }
                        }
                        collect_invariant_array_loads_expr(object, counter_id, pairs, assigned_in_loop);
                        collect_invariant_array_loads_expr(index, counter_id, pairs, assigned_in_loop);
                    }
                    Expr::IndexSet { object, index, value } => {
                        collect_invariant_array_loads_expr(object, counter_id, pairs, assigned_in_loop);
                        collect_invariant_array_loads_expr(index, counter_id, pairs, assigned_in_loop);
                        collect_invariant_array_loads_expr(value, counter_id, pairs, assigned_in_loop);
                    }
                    Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
                    Expr::Logical { left, right, .. } => {
                        collect_invariant_array_loads_expr(left, counter_id, pairs, assigned_in_loop);
                        collect_invariant_array_loads_expr(right, counter_id, pairs, assigned_in_loop);
                    }
                    Expr::LocalSet(_, val) => collect_invariant_array_loads_expr(val, counter_id, pairs, assigned_in_loop),
                    Expr::Unary { operand, .. } => collect_invariant_array_loads_expr(operand, counter_id, pairs, assigned_in_loop),
                    Expr::Call { callee, args, .. } => {
                        collect_invariant_array_loads_expr(callee, counter_id, pairs, assigned_in_loop);
                        for a in args { collect_invariant_array_loads_expr(a, counter_id, pairs, assigned_in_loop); }
                    }
                    _ => {}
                }
            }
            fn collect_invariant_array_loads_stmts(
                stmts: &[Stmt],
                counter_id: LocalId,
                pairs: &mut Vec<(LocalId, LocalId)>,
                assigned_in_loop: &HashSet<LocalId>,
            ) {
                for s in stmts {
                    match s {
                        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                            collect_invariant_array_loads_expr(e, counter_id, pairs, assigned_in_loop);
                        }
                        Stmt::Let { init: Some(e), .. } => collect_invariant_array_loads_expr(e, counter_id, pairs, assigned_in_loop),
                        Stmt::If { condition, then_branch, else_branch, .. } => {
                            collect_invariant_array_loads_expr(condition, counter_id, pairs, assigned_in_loop);
                            collect_invariant_array_loads_stmts(then_branch, counter_id, pairs, assigned_in_loop);
                            if let Some(eb) = else_branch { collect_invariant_array_loads_stmts(eb, counter_id, pairs, assigned_in_loop); }
                        }
                        Stmt::For { init, condition, update, body } => {
                            if let Some(init_s) = init { collect_invariant_array_loads_stmts(&[init_s.as_ref().clone()], counter_id, pairs, assigned_in_loop); }
                            if let Some(c) = condition { collect_invariant_array_loads_expr(c, counter_id, pairs, assigned_in_loop); }
                            if let Some(u) = update { collect_invariant_array_loads_expr(u, counter_id, pairs, assigned_in_loop); }
                            collect_invariant_array_loads_stmts(body, counter_id, pairs, assigned_in_loop);
                        }
                        Stmt::While { condition, body } => {
                            collect_invariant_array_loads_expr(condition, counter_id, pairs, assigned_in_loop);
                            collect_invariant_array_loads_stmts(body, counter_id, pairs, assigned_in_loop);
                        }
                        _ => {}
                    }
                }
            }

            // Collect variables assigned in the loop body (to exclude from invariant detection)
            fn collect_assigned_ids_expr(expr: &Expr, assigned: &mut HashSet<LocalId>) {
                match expr {
                    Expr::LocalSet(id, val) => {
                        assigned.insert(*id);
                        collect_assigned_ids_expr(val, assigned);
                    }
                    Expr::Update { id, .. } => { assigned.insert(*id); }
                    Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
                    Expr::Logical { left, right, .. } => {
                        collect_assigned_ids_expr(left, assigned);
                        collect_assigned_ids_expr(right, assigned);
                    }
                    Expr::Unary { operand, .. } => collect_assigned_ids_expr(operand, assigned),
                    Expr::Call { callee, args, .. } => {
                        collect_assigned_ids_expr(callee, assigned);
                        for a in args { collect_assigned_ids_expr(a, assigned); }
                    }
                    Expr::IndexGet { object, index } => {
                        collect_assigned_ids_expr(object, assigned);
                        collect_assigned_ids_expr(index, assigned);
                    }
                    Expr::IndexSet { object, index, value } => {
                        collect_assigned_ids_expr(object, assigned);
                        collect_assigned_ids_expr(index, assigned);
                        collect_assigned_ids_expr(value, assigned);
                    }
                    Expr::Conditional { condition, then_expr, else_expr } => {
                        collect_assigned_ids_expr(condition, assigned);
                        collect_assigned_ids_expr(then_expr, assigned);
                        collect_assigned_ids_expr(else_expr, assigned);
                    }
                    _ => {}
                }
            }
            fn collect_assigned_ids_stmts(stmts: &[Stmt], assigned: &mut HashSet<LocalId>) {
                for s in stmts {
                    match s {
                        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => collect_assigned_ids_expr(e, assigned),
                        Stmt::Let { id, init, .. } => {
                            assigned.insert(*id);
                            if let Some(e) = init { collect_assigned_ids_expr(e, assigned); }
                        }
                        Stmt::If { condition, then_branch, else_branch, .. } => {
                            collect_assigned_ids_expr(condition, assigned);
                            collect_assigned_ids_stmts(then_branch, assigned);
                            if let Some(eb) = else_branch { collect_assigned_ids_stmts(eb, assigned); }
                        }
                        Stmt::For { init, condition, update, body } => {
                            if let Some(init_s) = init { collect_assigned_ids_stmts(&[init_s.as_ref().clone()], assigned); }
                            if let Some(c) = condition { collect_assigned_ids_expr(c, assigned); }
                            if let Some(u) = update { collect_assigned_ids_expr(u, assigned); }
                            collect_assigned_ids_stmts(body, assigned);
                        }
                        Stmt::While { condition, body } => {
                            collect_assigned_ids_expr(condition, assigned);
                            collect_assigned_ids_stmts(body, assigned);
                        }
                        _ => {}
                    }
                }
            }

            // Detect invariant index products (e.g., i*size in inner k-loop)
            fn collect_invariant_products_expr(
                expr: &Expr,
                counter_id: LocalId,
                assigned_in_loop: &HashSet<LocalId>,
                products: &mut HashSet<(LocalId, LocalId)>,
            ) {
                match expr {
                    Expr::IndexGet { object, index } | Expr::IndexSet { object, index, .. } => {
                        collect_invariant_products_from_index(index, counter_id, assigned_in_loop, products);
                        collect_invariant_products_expr(object, counter_id, assigned_in_loop, products);
                        collect_invariant_products_expr(index, counter_id, assigned_in_loop, products);
                        if let Expr::IndexSet { value, .. } = expr {
                            collect_invariant_products_expr(value, counter_id, assigned_in_loop, products);
                        }
                    }
                    Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
                    Expr::Logical { left, right, .. } => {
                        collect_invariant_products_expr(left, counter_id, assigned_in_loop, products);
                        collect_invariant_products_expr(right, counter_id, assigned_in_loop, products);
                    }
                    Expr::LocalSet(_, val) => collect_invariant_products_expr(val, counter_id, assigned_in_loop, products),
                    Expr::Unary { operand, .. } => collect_invariant_products_expr(operand, counter_id, assigned_in_loop, products),
                    Expr::Call { callee, args, .. } => {
                        collect_invariant_products_expr(callee, counter_id, assigned_in_loop, products);
                        for a in args { collect_invariant_products_expr(a, counter_id, assigned_in_loop, products); }
                    }
                    _ => {}
                }
            }
            fn collect_invariant_products_stmts(
                stmts: &[Stmt],
                counter_id: LocalId,
                assigned_in_loop: &HashSet<LocalId>,
                products: &mut HashSet<(LocalId, LocalId)>,
            ) {
                for s in stmts {
                    match s {
                        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                            collect_invariant_products_expr(e, counter_id, assigned_in_loop, products);
                        }
                        Stmt::Let { init: Some(e), .. } => collect_invariant_products_expr(e, counter_id, assigned_in_loop, products),
                        Stmt::If { condition, then_branch, else_branch, .. } => {
                            collect_invariant_products_expr(condition, counter_id, assigned_in_loop, products);
                            collect_invariant_products_stmts(then_branch, counter_id, assigned_in_loop, products);
                            if let Some(eb) = else_branch { collect_invariant_products_stmts(eb, counter_id, assigned_in_loop, products); }
                        }
                        Stmt::For { init, condition, update, body } => {
                            if let Some(init_s) = init { collect_invariant_products_stmts(&[init_s.as_ref().clone()], counter_id, assigned_in_loop, products); }
                            if let Some(c) = condition { collect_invariant_products_expr(c, counter_id, assigned_in_loop, products); }
                            if let Some(u) = update { collect_invariant_products_expr(u, counter_id, assigned_in_loop, products); }
                            collect_invariant_products_stmts(body, counter_id, assigned_in_loop, products);
                        }
                        Stmt::While { condition, body } => {
                            collect_invariant_products_expr(condition, counter_id, assigned_in_loop, products);
                            collect_invariant_products_stmts(body, counter_id, assigned_in_loop, products);
                        }
                        _ => {}
                    }
                }
            }
            // Helper: scan an index expression for Mul(a, b) where both a, b are loop-invariant
            fn collect_invariant_products_from_index(
                expr: &Expr,
                counter_id: LocalId,
                assigned_in_loop: &HashSet<LocalId>,
                products: &mut HashSet<(LocalId, LocalId)>,
            ) {
                match expr {
                    Expr::Binary { op: BinaryOp::Mul, left, right } => {
                        if let (Expr::LocalGet(a_id), Expr::LocalGet(b_id)) = (left.as_ref(), right.as_ref()) {
                            // Both operands must be invariant (not the counter, not assigned in loop)
                            if *a_id != counter_id && *b_id != counter_id
                                && !assigned_in_loop.contains(a_id) && !assigned_in_loop.contains(b_id)
                            {
                                let key = if *a_id <= *b_id { (*a_id, *b_id) } else { (*b_id, *a_id) };
                                products.insert(key);
                            }
                        }
                        collect_invariant_products_from_index(left, counter_id, assigned_in_loop, products);
                        collect_invariant_products_from_index(right, counter_id, assigned_in_loop, products);
                    }
                    Expr::Binary { left, right, .. } => {
                        collect_invariant_products_from_index(left, counter_id, assigned_in_loop, products);
                        collect_invariant_products_from_index(right, counter_id, assigned_in_loop, products);
                    }
                    _ => {}
                }
            }

            if can_unroll {
                // UNROLLED LOOP: Main loop (8 iterations at a time) + Remainder loop
                let len_var = cached_length_var.unwrap();
                let idx_id = bce_index_var.unwrap();
                let stride = loop_stride.unwrap();

                // PATTERN DETECTION: Check what optimizations we can apply
                // Pattern 1: x = x + constant -> strength reduction
                // Pattern 2: x = x + arr[i] -> SIMD multiple accumulators
                // Pattern 3: x = x + f(i) -> scalar multiple accumulators (generic)
                // Pattern 4: obj.field = obj.field + constant -> field strength reduction
                // Pattern 5: const p = new Point(i, j); sum = sum + p.x + p.y; -> escape analysis/scalar replacement
                let mut use_strength_reduction: Option<(LocalId, f64)> = None;  // (sum_var, constant)
                let mut use_multi_accum: Option<(LocalId, LocalId)> = None;  // (sum_var, arr_var)
                let mut use_generic_accum: Option<LocalId> = None;  // sum_var for generic pattern
                let mut use_field_strength_reduction: Option<(LocalId, String, f64)> = None;  // (obj_id, property, constant)
                // Pattern 5: escape analysis - (obj_id, class_name, field_args: Vec<(field_name, arg_expr)>)
                let mut use_scalar_replacement: Option<(LocalId, String, Vec<(String, Expr)>)> = None;

                // Helper to check if expression references a specific local
                fn expr_references_local(expr: &Expr, target: LocalId) -> bool {
                    match expr {
                        Expr::LocalGet(id) | Expr::LocalSet(id, _) => *id == target,
                        Expr::Update { id, .. } => *id == target,
                        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
                        Expr::Compare { left, right, .. } => {
                            expr_references_local(left, target) || expr_references_local(right, target)
                        }
                        Expr::Unary { operand, .. } => expr_references_local(operand, target),
                        Expr::Conditional { condition, then_expr, else_expr } => {
                            expr_references_local(condition, target) ||
                            expr_references_local(then_expr, target) ||
                            expr_references_local(else_expr, target)
                        }
                        Expr::Call { callee, args, .. } => {
                            expr_references_local(callee, target) ||
                            args.iter().any(|a| expr_references_local(a, target))
                        }
                        Expr::IndexGet { object, index } => {
                            expr_references_local(object, target) || expr_references_local(index, target)
                        }
                        Expr::PropertyGet { object, .. } => expr_references_local(object, target),
                        _ => false,
                    }
                }

                if body.len() == 1 {
                    if let Stmt::Expr(expr) = &body[0] {
                        if let Expr::LocalSet(set_id, value) = expr {
                            if let Expr::Binary { op: BinaryOp::Add, left, right } = value.as_ref() {
                                if let Expr::LocalGet(get_id) = left.as_ref() {
                                    if get_id == set_id {
                                        // Pattern 1: x = x + constant
                                        if let Some(c) = match right.as_ref() {
                                            Expr::Integer(n) => Some(*n as f64),
                                            Expr::Number(f) => Some(*f),
                                            _ => None,
                                        } {
                                            use_strength_reduction = Some((*set_id, c));
                                        }

                                        // Pattern 2: x = x + arr[i]
                                        if use_strength_reduction.is_none() {
                                            if let Expr::IndexGet { object, index } = right.as_ref() {
                                                if let (Expr::LocalGet(arr_id), Expr::LocalGet(index_id)) = (object.as_ref(), index.as_ref()) {
                                                    if index_id == &idx_id {
                                                        if let Some(arr_info) = locals.get(arr_id) {
                                                            if arr_info.is_array {
                                                                use_multi_accum = Some((*set_id, *arr_id));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // Pattern 3: x = x + f(i) where f(i) doesn't reference x
                                        // This is a generic accumulation pattern for NUMERIC values only
                                        // Skip for string variables - string concat has different semantics
                                        if use_strength_reduction.is_none() && use_multi_accum.is_none() {
                                            let is_string_var = locals.get(set_id).map(|i| i.is_string).unwrap_or(false);
                                            if !is_string_var && !expr_references_local(right, *set_id) {
                                                use_generic_accum = Some(*set_id);
                                            }
                                        }
                                    }
                                }
                            }
                        // Pattern 4: obj.field = obj.field + constant
                        } else if let Expr::PropertySet { object, property, value } = expr {
                            if let Expr::LocalGet(obj_id) = object.as_ref() {
                                if let Expr::Binary { op: BinaryOp::Add, left, right } = value.as_ref() {
                                    if let Expr::PropertyGet { object: get_obj, property: get_prop } = left.as_ref() {
                                        if let Expr::LocalGet(get_obj_id) = get_obj.as_ref() {
                                            if get_obj_id == obj_id && get_prop == property {
                                                // Pattern: obj.field = obj.field + constant
                                                if let Some(c) = match right.as_ref() {
                                                    Expr::Integer(n) => Some(*n as f64),
                                                    Expr::Number(f) => Some(*f),
                                                    _ => None,
                                                } {
                                                    use_field_strength_reduction = Some((*obj_id, property.clone(), c));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Pattern 5: Escape analysis for non-escaping objects
                // Pattern: const p = new Point(i, i+1); sum = sum + p.x + p.y;
                // Detect: 2 statements, first is Let with New, second only uses the object via PropertyGet
                if body.len() == 2 {
                    if let Stmt::Let { id: obj_id, init: Some(init_expr), .. } = &body[0] {
                        if let Expr::New { class_name, args, .. } = init_expr {
                            // Check if this class exists and we can map args to fields
                            if let Some(class_meta) = classes.get(class_name) {
                                // Check if constructor just assigns args to fields in order
                                // Most simple Point-like classes do: constructor(x, y) { this.x = x; this.y = y; }
                                // For now, assume args map to fields in field order
                                let field_names: Vec<String> = class_meta.field_indices.iter()
                                    .map(|(name, idx)| (name.clone(), *idx))
                                    .collect::<Vec<_>>()
                                    .into_iter()
                                    .filter(|(_, idx)| (*idx as usize) < args.len())
                                    .map(|(name, _)| name)
                                    .collect();

                                if field_names.len() == args.len() {
                                    // Helper to check if object only used via PropertyGet
                                    fn expr_only_propertyget(expr: &Expr, obj_id: LocalId) -> bool {
                                        match expr {
                                            // Object is used via PropertyGet - OK
                                            Expr::PropertyGet { object, .. } => {
                                                if let Expr::LocalGet(id) = object.as_ref() {
                                                    if *id == obj_id { return true; }
                                                }
                                                expr_only_propertyget(object, obj_id)
                                            }
                                            // Direct use of object - NOT OK (escapes)
                                            Expr::LocalGet(id) => *id != obj_id,
                                            // Recurse through expressions
                                            Expr::Binary { left, right, .. } |
                                            Expr::Compare { left, right, .. } |
                                            Expr::Logical { left, right, .. } => {
                                                expr_only_propertyget(left, obj_id) && expr_only_propertyget(right, obj_id)
                                            }
                                            Expr::LocalSet(_, val) => expr_only_propertyget(val, obj_id),
                                            Expr::Unary { operand, .. } => expr_only_propertyget(operand, obj_id),
                                            Expr::Call { args, .. } => {
                                                // If object is passed to a function, it escapes
                                                args.iter().all(|a| expr_only_propertyget(a, obj_id))
                                            }
                                            _ => true,
                                        }
                                    }

                                    fn stmt_only_propertyget(stmt: &Stmt, obj_id: LocalId) -> bool {
                                        match stmt {
                                            Stmt::Expr(e) => expr_only_propertyget(e, obj_id),
                                            _ => false,
                                        }
                                    }

                                    // Check if second statement only uses object via PropertyGet
                                    if stmt_only_propertyget(&body[1], *obj_id) {
                                        // Map field names to their argument expressions
                                        let mut field_args: Vec<(String, Expr)> = Vec::new();
                                        for (name, idx) in &class_meta.field_indices {
                                            if (*idx as usize) < args.len() {
                                                field_args.push((name.clone(), args[*idx as usize].clone()));
                                            }
                                        }

                                        use_scalar_replacement = Some((*obj_id, class_name.clone(), field_args));
                                    }
                                }
                            }
                        }
                    }
                }

                // Create blocks for main loop
                let main_header = builder.create_block();
                let main_body = builder.create_block();
                let main_update = builder.create_block();

                // Create blocks for remainder loop
                let rem_combine = builder.create_block();  // Combine accumulators (runs once)
                let rem_header = builder.create_block();   // Check condition (loop header)
                let rem_body = builder.create_block();
                let rem_update = builder.create_block();

                let exit_block = builder.create_block();

                // SIMD VECTORIZATION: Create F64X2 vector accumulators (4 vectors = 8 f64 values)
                // Using 128-bit vectors (F64X2) for portable SIMD across x86-64 and ARM64
                let f64x2_type = types::F64X2;
                let mut simd_accumulators: Option<([Variable; 4], LocalId)> = None;
                if let Some((sum_id, arr_id)) = use_multi_accum {
                    if let Some(_sum_info) = locals.get(&sum_id) {
                        // Create 4 vector accumulator variables (4 x F64X2 = 8 f64 values)
                        let mut vec_accs = [Variable::new(0); 4];
                        for i in 0..4 {
                            vec_accs[i] = Variable::new(*next_var);
                            *next_var += 1;
                            builder.declare_var(vec_accs[i], f64x2_type);
                        }

                        // Initialize all vector accumulators to [0.0, 0.0]
                        let zero = builder.ins().f64const(0.0);
                        let zero_vec = builder.ins().splat(f64x2_type, zero);
                        for acc in &vec_accs {
                            builder.def_var(*acc, zero_vec);
                        }

                        simd_accumulators = Some((vec_accs, arr_id));
                    }
                }

                // GENERIC ACCUMULATORS: Create 8 scalar f64 accumulators for Pattern 3
                // This breaks the dependency chain for x = x + f(i) patterns
                let mut generic_accumulators: Option<([Variable; 8], LocalId, Variable)> = None;  // (accs, sum_id, original_var)
                if let Some(sum_id) = use_generic_accum {
                    if let Some(sum_info) = locals.get(&sum_id) {
                        // Create 8 scalar accumulator variables
                        let mut accs = [Variable::new(0); 8];
                        for i in 0..8 {
                            accs[i] = Variable::new(*next_var);
                            *next_var += 1;
                            builder.declare_var(accs[i], types::F64);
                        }

                        // Initialize all accumulators to 0
                        let zero = builder.ins().f64const(0.0);
                        for acc in &accs {
                            builder.def_var(*acc, zero);
                        }

                        generic_accumulators = Some((accs, sum_id, sum_info.var));
                    }
                }

                // SCALAR REPLACEMENT: Create scalar variables for each field (Pattern 5)
                // This eliminates heap allocation by keeping field values in registers
                let mut scalar_replacement_vars: Option<(LocalId, HashMap<String, Variable>)> = None;
                if let Some((obj_id, ref _class_name, ref field_args)) = use_scalar_replacement {
                    let mut field_vars: HashMap<String, Variable> = HashMap::new();

                    for (field_name, _arg_expr) in field_args {
                        let var = Variable::new(*next_var);
                        *next_var += 1;
                        builder.declare_var(var, types::F64);
                        // Initialize to 0 (will be set in loop body)
                        let zero = builder.ins().f64const(0.0);
                        builder.def_var(var, zero);
                        field_vars.insert(field_name.clone(), var);
                    }

                    // Create a "fake" LocalInfo for the object that uses scalar fields
                    // This allows PropertyGet to use the scalar variables instead of heap access
                    let dummy_var = Variable::new(*next_var);
                    *next_var += 1;
                    builder.declare_var(dummy_var, types::F64);
                    let zero = builder.ins().f64const(0.0);
                    builder.def_var(dummy_var, zero);

                    locals.insert(obj_id, LocalInfo {
                        var: dummy_var,
                        name: None, // Scalar replacement uses internal variables
                        class_name: Some(_class_name.clone()),
                        type_args: Vec::new(),
                        is_pointer: false,
                        is_array: false,
                        is_string: false,
                        is_bigint: false,
                        is_closure: false, closure_func_id: None,
                        is_boxed: false,
                        is_map: false, is_set: false, is_buffer: false, is_event_emitter: false, is_union: false,
                        is_mixed_array: false,
                        is_integer: false,
                        is_integer_array: false,
                        is_i32: false, is_boolean: false,
                        i32_shadow: None,
                        bounded_by_array: None,
                        bounded_by_constant: None,
                        scalar_fields: Some(field_vars.clone()),
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    });

                    scalar_replacement_vars = Some((obj_id, field_vars));
                }

                // LICM for unrolled path: Hoist invariant loads and products before the loop
                let mut unrolled_assigned_in_loop: HashSet<LocalId> = HashSet::new();
                collect_assigned_ids_stmts(body, &mut unrolled_assigned_in_loop);
                unrolled_assigned_in_loop.insert(idx_id);

                // Hoist invariant array element loads
                let mut unrolled_invariant_loads: Vec<(LocalId, LocalId)> = Vec::new();
                collect_invariant_array_loads_stmts(body, idx_id, &mut unrolled_invariant_loads, &unrolled_assigned_in_loop);

                let mut unrolled_hoisted_load_arr_ids: Vec<LocalId> = Vec::new();
                for (arr_id, idx_load_id) in &unrolled_invariant_loads {
                    if let Some(arr_info) = locals.get(arr_id) {
                        if arr_info.is_array && !arr_info.is_mixed_array && !arr_info.is_union {
                            if let Some(cache_ptr_var) = arr_info.cached_array_ptr {
                                if locals.contains_key(idx_load_id) {
                                    let arr_ptr = builder.use_var(cache_ptr_var);
                                    let idx_val = {
                                        let idx_info = locals.get(idx_load_id).unwrap();
                                        if idx_info.is_i32 {
                                            builder.use_var(idx_info.var)
                                        } else if let Some(shadow) = idx_info.i32_shadow {
                                            builder.use_var(shadow)
                                        } else {
                                            // Safe conversion: f64 -> i64 -> i32
                                            let f64_val = builder.use_var(idx_info.var);
                                            let i64_tmp = builder.ins().fcvt_to_sint_sat(types::I64, f64_val);
                                            builder.ins().ireduce(types::I32, i64_tmp)
                                        }
                                    };
                                    let idx_i64 = builder.ins().uextend(types::I64, idx_val);
                                    let byte_offset = builder.ins().ishl_imm(idx_i64, 3);
                                    let data_ptr = builder.ins().iadd_imm(arr_ptr, 8);
                                    let element_ptr = builder.ins().iadd(data_ptr, byte_offset);
                                    let element_val = builder.ins().load(types::F64, MemFlags::new(), element_ptr, 0);

                                    let cache_var = Variable::new(*next_var);
                                    *next_var += 1;
                                    builder.declare_var(cache_var, types::F64);
                                    builder.def_var(cache_var, element_val);

                                    if !unrolled_hoisted_load_arr_ids.contains(arr_id) {
                                        unrolled_hoisted_load_arr_ids.push(*arr_id);
                                    }
                                    drop(arr_info);
                                    locals.get_mut(arr_id).unwrap()
                                        .hoisted_element_loads.get_or_insert_with(HashMap::new)
                                        .insert(*idx_load_id, cache_var);
                                }
                            }
                        }
                    }
                }

                // Hoist invariant i32 index products
                let mut unrolled_invariant_products: HashSet<(LocalId, LocalId)> = HashSet::new();
                collect_invariant_products_stmts(body, idx_id, &unrolled_assigned_in_loop, &mut unrolled_invariant_products);

                let mut unrolled_hoisted_product_ids: Vec<(LocalId, LocalId)> = Vec::new();
                for (a_id, b_id) in &unrolled_invariant_products {
                    let a_val = try_compile_index_as_i32(builder, &Expr::LocalGet(*a_id), locals);
                    let b_val = try_compile_index_as_i32(builder, &Expr::LocalGet(*b_id), locals);
                    if let (Some(a_i32), Some(b_i32)) = (a_val, b_val) {
                        let product = builder.ins().imul(a_i32, b_i32);
                        let cache_var = Variable::new(*next_var);
                        *next_var += 1;
                        builder.declare_var(cache_var, types::I32);
                        builder.def_var(cache_var, product);
                        unrolled_hoisted_product_ids.push((*a_id, *b_id));

                        locals.get_mut(a_id).unwrap()
                            .hoisted_i32_products.get_or_insert_with(HashMap::new)
                            .insert(*b_id, cache_var);
                    }
                }

                // Jump to main loop
                builder.ins().jump(main_header, &[]);

                // === MAIN LOOP (processes UNROLL_FACTOR iterations at a time) ===
                builder.switch_to_block(main_header);

                // Check: i + UNROLL_FACTOR * stride <= limit
                let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;
                let idx_i32 = builder.use_var(idx_info.var);
                let length_i32 = builder.use_var(len_var);
                let idx_plus_unroll = builder.ins().iadd_imm(idx_i32, UNROLL_FACTOR * stride);
                let can_do_unroll = builder.ins().icmp(IntCC::SignedLessThanOrEqual, idx_plus_unroll, length_i32);
                builder.ins().brif(can_do_unroll, main_body, &[], rem_combine, &[]);

                // Main loop context
                let main_loop_ctx = LoopContext { exit_block, header_block: main_update, bounded_indices: bounded_indices.clone(), try_depth: TRY_CATCH_DEPTH.with(|d| d.get()) };

                // Set BCE for loop counter
                if let Some(arr_id) = bce_array_var {
                    if let Some(idx_info) = locals.get_mut(&idx_id) {
                        idx_info.bounded_by_array = Some(arr_id);
                    }
                }

                // Main loop body
                builder.switch_to_block(main_body);
                builder.seal_block(main_body);

                // OPTIMIZATION: Defer module-level variable write-backs in unrolled loops
                let mut deferred_unrolled_vars: HashSet<LocalId> = HashSet::new();
                let unrolled_body_has_calls = loop_body_has_calls(body)
                    || update.as_ref().map_or(false, |u| loop_expr_has_calls(u));
                if !unrolled_body_has_calls {
                    deferred_unrolled_vars = collect_module_var_writes_in_loop(body, locals);
                    if let Some(upd) = update {
                        let update_stmts = vec![Stmt::Expr(upd.clone())];
                        let update_writes = collect_module_var_writes_in_loop(&update_stmts, locals);
                        deferred_unrolled_vars.extend(update_writes);
                    }
                    if !deferred_unrolled_vars.is_empty() {

                        DEFERRED_MODULE_WRITEBACK_VARS.with(|d| {
                            d.borrow_mut().extend(deferred_unrolled_vars.iter().copied());
                        });
                    }
                } else {

                }

                let mut optimized = false;

                // OPTIMIZATION 1: Strength reduction for x = x + constant
                if let Some((sum_id, constant)) = use_strength_reduction {
                    let combined_const = constant * (UNROLL_FACTOR as f64);
                    if let Some(info) = locals.get(&sum_id) {
                        let current = builder.use_var(info.var);
                        let add_val = builder.ins().f64const(combined_const);
                        let new_val = builder.ins().fadd(current, add_val);
                        builder.def_var(info.var, new_val);
                        optimized = true;
                    }
                }

                // OPTIMIZATION 2: SIMD vector operations for x = x + arr[i]
                if let Some((vec_accs, arr_id)) = simd_accumulators.as_ref() {
                    if let Some(arr_info) = locals.get(arr_id) {
                        let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;

                        // Get array data pointer (skip 8-byte header)
                        let arr_val = builder.use_var(arr_info.var);
                        let arr_ptr = ensure_i64(builder, arr_val);
                        let data_ptr = builder.ins().iadd_imm(arr_ptr, 8);

                        // Get current index and compute base byte offset
                        let idx_i32 = builder.use_var(idx_info.var);
                        let base_byte_offset = builder.ins().ishl_imm(idx_i32, 3); // idx * 8 bytes
                        let base_byte_offset_i64 = builder.ins().sextend(types::I64, base_byte_offset);
                        let base_ptr = builder.ins().iadd(data_ptr, base_byte_offset_i64);

                        // Load 4 pairs of elements (8 total) using SIMD vector loads
                        // Each vector load gets 2 consecutive f64 values (16 bytes)
                        for k in 0..4 {
                            // Calculate pointer for this vector (k * 16 bytes)
                            let vec_ptr = if k == 0 {
                                base_ptr
                            } else {
                                builder.ins().iadd_imm(base_ptr, (k * 16) as i64)
                            };

                            // Vector load: load 2 f64 values at once
                            let vec_val = builder.ins().load(f64x2_type, MemFlags::new(), vec_ptr, 0);

                            // Vector add to accumulator
                            let acc_var = vec_accs[k as usize];
                            let acc_val = builder.use_var(acc_var);
                            let new_acc = builder.ins().fadd(acc_val, vec_val);
                            builder.def_var(acc_var, new_acc);
                        }
                        optimized = true;
                    }
                }

                // OPTIMIZATION 4: Field strength reduction for obj.field = obj.field + constant
                // DISABLED: Index-based field access is unsafe for non-this variables because
                // plain objects (MySQL rows, JSON.parse results) may have different field ordering
                // than the class definition. The non-optimized path uses name-based access.
                // if let Some((obj_id, ref property, constant)) = use_field_strength_reduction { ... }

                // OPTIMIZATION 5: Scalar replacement for non-escaping objects
                // Skip allocation, compile constructor args directly to scalar variables
                if let Some((obj_id, ref field_vars)) = scalar_replacement_vars.as_ref() {
                    if let Some((_obj_id, _class_name, ref field_args)) = use_scalar_replacement.as_ref() {
                        for unroll_iter in 0..UNROLL_FACTOR {
                            // Initialize scalar fields from constructor args
                            for (field_name, arg_expr) in field_args {
                                if let Some(var) = field_vars.get(field_name) {
                                    let val = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, arg_expr, this_ctx)?;
                                    builder.def_var(*var, val);
                                }
                            }

                            // Compile only the second statement (the one that uses the object)
                            // The first statement (Let with New) is replaced by scalar initialization above
                            if body.len() >= 2 {
                                compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, &body[1], this_ctx, Some(&main_loop_ctx), boxed_vars, async_promise_var)?;
                            }

                            // Increment loop counter by stride (except for last iteration)
                            if unroll_iter < UNROLL_FACTOR - 1 {
                                let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;
                                let current = builder.use_var(idx_info.var);
                                let incremented = builder.ins().iadd_imm(current, stride);
                                builder.def_var(idx_info.var, incremented);
                            }
                        }
                        optimized = true;
                    }
                }

                // OPTIMIZATION 3: Generic accumulator pattern for x = x + f(i)
                // Compile body 8 times, but redirect sum writes to separate accumulators
                if let Some((ref accs, sum_id, original_sum_var)) = generic_accumulators {
                    for unroll_iter in 0..UNROLL_FACTOR {
                        // Temporarily redirect sum variable to accumulator[k]
                        let acc_var = accs[unroll_iter as usize];
                        if let Some(sum_info) = locals.get_mut(&sum_id) {
                            sum_info.var = acc_var;
                        }

                        // Compile the body (writes to accumulator instead of sum)
                        for s in body {
                            compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&main_loop_ctx), boxed_vars, async_promise_var)?;
                        }

                        // Increment loop counter by stride (except for last iteration - update block handles it)
                        if unroll_iter < UNROLL_FACTOR - 1 {
                            let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;
                            let current = builder.use_var(idx_info.var);
                            let incremented = builder.ins().iadd_imm(current, stride);
                            builder.def_var(idx_info.var, incremented);
                        }
                    }

                    // Restore original sum variable
                    if let Some(sum_info) = locals.get_mut(&sum_id) {
                        sum_info.var = original_sum_var;
                    }

                    optimized = true;
                }

                if optimized && generic_accumulators.is_none() && scalar_replacement_vars.is_none() {
                    // Increment loop counter by (UNROLL_FACTOR - 1) * stride (update block does +stride)
                    // Only for strength reduction and SIMD which don't have per-iteration increments
                    let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;
                    let current = builder.use_var(idx_info.var);
                    let incremented = builder.ins().iadd_imm(current, (UNROLL_FACTOR - 1) * stride);
                    builder.def_var(idx_info.var, incremented);
                } else if !optimized {
                    // Normal unrolling: compile body UNROLL_FACTOR times
                    for unroll_iter in 0..UNROLL_FACTOR {
                        // Compile all body statements
                        for s in body {
                            compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&main_loop_ctx), boxed_vars, async_promise_var)?;
                        }

                        // Stop unrolling if body terminated the block (e.g., return/throw)
                        let current_blk = builder.current_block().unwrap();
                        if is_block_filled(builder, current_blk) {
                            break;
                        }

                        // Increment loop counter by stride (except for last iteration - update block handles it)
                        if unroll_iter < UNROLL_FACTOR - 1 {
                            let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;
                            let current = builder.use_var(idx_info.var);
                            let incremented = builder.ins().iadd_imm(current, stride);
                            builder.def_var(idx_info.var, incremented);
                        }
                    }
                }

                // Jump to main update
                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(main_update, &[]);
                }

                // Main loop update - execute original update expression (i++)
                builder.switch_to_block(main_update);
                builder.seal_block(main_update);
                if let Some(upd) = update {
                    compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, upd, this_ctx)?;
                }
                builder.ins().jump(main_header, &[]);
                builder.seal_block(main_header);

                // === REMAINDER SECTION (combine accumulators, then process remaining iterations) ===

                // rem_combine block: Combine accumulators (runs once when exiting main loop)
                builder.switch_to_block(rem_combine);
                builder.seal_block(rem_combine);

                // COMBINE SIMD ACCUMULATORS: sum = original_sum + horizontal_sum(vec_acc0..vec_acc3)
                if let Some((vec_accs, _arr_id)) = simd_accumulators.as_ref() {
                    if let Some((sum_id, _)) = use_multi_accum {
                        if let Some(sum_info) = locals.get(&sum_id) {
                            let original_sum = builder.use_var(sum_info.var);

                            // Load all 4 vector accumulators
                            let v0 = builder.use_var(vec_accs[0]); // [a0, a1]
                            let v1 = builder.use_var(vec_accs[1]); // [a2, a3]
                            let v2 = builder.use_var(vec_accs[2]); // [a4, a5]
                            let v3 = builder.use_var(vec_accs[3]); // [a6, a7]

                            // Combine vectors pairwise: v01 = v0 + v1, v23 = v2 + v3
                            let v01 = builder.ins().fadd(v0, v1); // [(a0+a2), (a1+a3)]
                            let v23 = builder.ins().fadd(v2, v3); // [(a4+a6), (a5+a7)]

                            // Combine to single vector
                            let v_all = builder.ins().fadd(v01, v23); // [sum_even, sum_odd]

                            // Extract lanes and sum horizontally
                            let lane0 = builder.ins().extractlane(v_all, 0); // sum of even indices
                            let lane1 = builder.ins().extractlane(v_all, 1); // sum of odd indices
                            let accum_total = builder.ins().fadd(lane0, lane1);

                            let total = builder.ins().fadd(original_sum, accum_total);
                            builder.def_var(sum_info.var, total);
                        }
                    }
                }

                // COMBINE GENERIC ACCUMULATORS: sum = original_sum + acc0 + acc1 + ... + acc7
                if let Some((ref accs, sum_id, _original_sum_var)) = generic_accumulators {
                    if let Some(sum_info) = locals.get(&sum_id) {
                        let original_sum = builder.use_var(sum_info.var);

                        // Load all 8 accumulators
                        let a: Vec<_> = accs.iter().map(|acc| builder.use_var(*acc)).collect();

                        // Combine in tree fashion: ((a0+a1)+(a2+a3)) + ((a4+a5)+(a6+a7))
                        let sum01 = builder.ins().fadd(a[0], a[1]);
                        let sum23 = builder.ins().fadd(a[2], a[3]);
                        let sum45 = builder.ins().fadd(a[4], a[5]);
                        let sum67 = builder.ins().fadd(a[6], a[7]);
                        let sum0123 = builder.ins().fadd(sum01, sum23);
                        let sum4567 = builder.ins().fadd(sum45, sum67);
                        let accum_total = builder.ins().fadd(sum0123, sum4567);

                        let total = builder.ins().fadd(original_sum, accum_total);
                        builder.def_var(sum_info.var, total);
                    }
                }

                // Jump to remainder loop header
                builder.ins().jump(rem_header, &[]);

                // rem_header block: Check condition (loop header)
                builder.switch_to_block(rem_header);

                // Check: i < limit
                let idx_info = locals.get(&idx_id).ok_or_else(|| anyhow!("Index variable not found"))?;
                let idx_i32 = builder.use_var(idx_info.var);
                let length_i32 = builder.use_var(len_var);
                let in_bounds = builder.ins().icmp(IntCC::SignedLessThan, idx_i32, length_i32);
                builder.ins().brif(in_bounds, rem_body, &[], exit_block, &[]);

                // Remainder loop context
                let rem_loop_ctx = LoopContext { exit_block, header_block: rem_update, bounded_indices, try_depth: TRY_CATCH_DEPTH.with(|d| d.get()) };

                // Remainder loop body
                builder.switch_to_block(rem_body);
                builder.seal_block(rem_body);

                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&rem_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(rem_update, &[]);
                }

                // Remainder loop update
                builder.switch_to_block(rem_update);
                builder.seal_block(rem_update);
                if let Some(upd) = update {
                    compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, upd, this_ctx)?;
                }
                builder.ins().jump(rem_header, &[]);
                builder.seal_block(rem_header);

                // Clear deferred set before exit flush (unrolled path)
                if !deferred_unrolled_vars.is_empty() {
                    DEFERRED_MODULE_WRITEBACK_VARS.with(|d| {
                        let mut set = d.borrow_mut();
                        for id in &deferred_unrolled_vars {
                            set.remove(id);
                        }
                    });
                }

                // Exit
                builder.switch_to_block(exit_block);
                builder.seal_block(exit_block);

                // Flush deferred module-level variable write-backs (unrolled path)
                for var_id in &deferred_unrolled_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(data_id) = info.module_var_data_id {
                            let current_val = builder.use_var(info.var);
                            let val_type = builder.func.dfg.value_type(current_val);
                            let store_val = if val_type == types::I32 {
                                builder.ins().fcvt_from_sint(types::F64, current_val)
                            } else {
                                current_val
                            };
                            let global_val = module.declare_data_in_func(data_id, builder.func);
                            let ptr = builder.ins().global_value(types::I64, global_val);
                            builder.ins().store(MemFlags::new(), store_val, ptr, 0);
                        }
                    }
                }

                // Clean up after loop
                if let Some(orig_f64_var) = original_f64_var {
                    if let Some(idx_info) = locals.get_mut(&idx_id) {
                        let final_i32 = builder.use_var(idx_info.var);
                        let final_f64 = builder.ins().fcvt_from_sint(types::F64, final_i32);
                        builder.def_var(orig_f64_var, final_f64);
                        idx_info.var = orig_f64_var;
                        idx_info.is_i32 = false;
                        idx_info.bounded_by_array = None;
                    }
                }

                // Clear LICM hoisted element loads (unrolled path)
                for arr_id in &unrolled_hoisted_load_arr_ids {
                    if let Some(info) = locals.get_mut(arr_id) {
                        info.hoisted_element_loads = None;
                    }
                }

                // Clear LICM hoisted i32 products (unrolled path)
                for (a_id, _) in &unrolled_hoisted_product_ids {
                    if let Some(info) = locals.get_mut(a_id) {
                        info.hoisted_i32_products = None;
                    }
                }
            } else {
                // NON-UNROLLED LOOP: Original implementation

                // OPTIMIZATION: Cache array raw pointers before the loop to avoid
                // redundant js_nanbox_get_pointer calls on every iteration
                fn collect_array_ids_from_expr(expr: &Expr, ids: &mut HashSet<LocalId>) {
                    match expr {
                        Expr::IndexGet { object, index } | Expr::IndexSet { object, index, .. } => {
                            if let Expr::LocalGet(id) = object.as_ref() {
                                ids.insert(*id);
                            }
                            collect_array_ids_from_expr(object, ids);
                            collect_array_ids_from_expr(index, ids);
                            if let Expr::IndexSet { value, .. } = expr {
                                collect_array_ids_from_expr(value, ids);
                            }
                        }
                        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
                        Expr::Logical { left, right, .. } => {
                            collect_array_ids_from_expr(left, ids);
                            collect_array_ids_from_expr(right, ids);
                        }
                        Expr::LocalSet(_, val) => collect_array_ids_from_expr(val, ids),
                        Expr::Unary { operand, .. } => collect_array_ids_from_expr(operand, ids),
                        Expr::Call { callee, args, .. } => {
                            collect_array_ids_from_expr(callee, ids);
                            for a in args { collect_array_ids_from_expr(a, ids); }
                        }
                        Expr::PropertyGet { object, .. } => collect_array_ids_from_expr(object, ids),
                        Expr::PropertySet { object, value, .. } => {
                            collect_array_ids_from_expr(object, ids);
                            collect_array_ids_from_expr(value, ids);
                        }
                        Expr::Conditional { condition, then_expr, else_expr } => {
                            collect_array_ids_from_expr(condition, ids);
                            collect_array_ids_from_expr(then_expr, ids);
                            collect_array_ids_from_expr(else_expr, ids);
                        }
                        _ => {}
                    }
                }
                fn collect_array_ids_from_stmts(stmts: &[Stmt], ids: &mut HashSet<LocalId>) {
                    for s in stmts {
                        match s {
                            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                                collect_array_ids_from_expr(e, ids);
                            }
                            Stmt::Let { init: Some(e), .. } => collect_array_ids_from_expr(e, ids),
                            Stmt::If { condition, then_branch, else_branch, .. } => {
                                collect_array_ids_from_expr(condition, ids);
                                collect_array_ids_from_stmts(then_branch, ids);
                                if let Some(eb) = else_branch { collect_array_ids_from_stmts(eb, ids); }
                            }
                            Stmt::For { init, condition, update, body } => {
                                if let Some(init_s) = init { collect_array_ids_from_stmts(&[init_s.as_ref().clone()], ids); }
                                if let Some(c) = condition { collect_array_ids_from_expr(c, ids); }
                                if let Some(u) = update { collect_array_ids_from_expr(u, ids); }
                                collect_array_ids_from_stmts(body, ids);
                            }
                            Stmt::While { condition, body } => {
                                collect_array_ids_from_expr(condition, ids);
                                collect_array_ids_from_stmts(body, ids);
                            }
                            _ => {}
                        }
                    }
                }

                let mut array_ids_in_body: HashSet<LocalId> = HashSet::new();
                collect_array_ids_from_stmts(body, &mut array_ids_in_body);

                // Cache raw pointers for arrays used in this loop
                let mut cached_array_ids: Vec<LocalId> = Vec::new();
                for arr_id in &array_ids_in_body {
                    if let Some(info) = locals.get(arr_id) {
                        if info.is_array && !info.is_mixed_array && !info.is_union && info.cached_array_ptr.is_none() {
                            // Extract raw pointer once and cache it
                            let arr_val = builder.use_var(info.var);
                            let val_type = builder.func.dfg.value_type(arr_val);
                            let raw_ptr = if val_type == types::I64 {
                                // Already a raw I64 pointer - use directly, no FFI needed
                                arr_val
                            } else {
                                // F64 (NaN-boxed) - extract via runtime function
                                let arr_f64 = ensure_f64(builder, arr_val);
                                let get_ptr_func = extern_funcs.get("js_nanbox_get_pointer")
                                    .ok_or_else(|| anyhow!("js_nanbox_get_pointer not declared"))?;
                                let get_ptr_ref = module.declare_func_in_func(*get_ptr_func, builder.func);
                                let call = builder.ins().call(get_ptr_ref, &[arr_f64]);
                                builder.inst_results(call)[0]
                            };

                            // Create a cached variable for this pointer
                            let cache_var = Variable::new(*next_var);
                            *next_var += 1;
                            builder.declare_var(cache_var, types::I64);
                            builder.def_var(cache_var, raw_ptr);

                            cached_array_ids.push(*arr_id);
                            // We need to set it after collecting all, to avoid borrow conflict
                            drop(info);
                            if let Some(info_mut) = locals.get_mut(arr_id) {
                                info_mut.cached_array_ptr = Some(cache_var);
                            }
                        }
                    }
                }

                let counter_id = bce_index_var.unwrap_or(u32::MAX);

                // Collect all assigned variables in loop body
                let mut assigned_in_loop: HashSet<LocalId> = HashSet::new();
                collect_assigned_ids_stmts(body, &mut assigned_in_loop);
                // The counter itself is always assigned in the loop
                assigned_in_loop.insert(counter_id);

                // LICM: Hoist invariant array element loads
                let mut invariant_loads: Vec<(LocalId, LocalId)> = Vec::new();
                collect_invariant_array_loads_stmts(body, counter_id, &mut invariant_loads, &assigned_in_loop);

                let mut hoisted_load_arr_ids: Vec<LocalId> = Vec::new();
                for (arr_id, idx_id) in &invariant_loads {
                    if let Some(arr_info) = locals.get(arr_id) {
                        if arr_info.is_array && !arr_info.is_mixed_array && !arr_info.is_union {
                            if let Some(cache_ptr_var) = arr_info.cached_array_ptr {
                                if locals.contains_key(idx_id) {
                                    let arr_ptr = builder.use_var(cache_ptr_var);
                                    let idx_val = {
                                        let idx_info = locals.get(idx_id).unwrap();
                                        if idx_info.is_i32 {
                                            builder.use_var(idx_info.var)
                                        } else if let Some(shadow) = idx_info.i32_shadow {
                                            builder.use_var(shadow)
                                        } else {
                                            // Safe conversion: f64 -> i64 -> i32
                                            let f64_val = builder.use_var(idx_info.var);
                                            let i64_tmp = builder.ins().fcvt_to_sint_sat(types::I64, f64_val);
                                            builder.ins().ireduce(types::I32, i64_tmp)
                                        }
                                    };
                                    let idx_i64 = builder.ins().uextend(types::I64, idx_val);
                                    let byte_offset = builder.ins().ishl_imm(idx_i64, 3);
                                    let data_ptr = builder.ins().iadd_imm(arr_ptr, 8);
                                    let element_ptr = builder.ins().iadd(data_ptr, byte_offset);
                                    let element_val = builder.ins().load(types::F64, MemFlags::new(), element_ptr, 0);

                                    let cache_var = Variable::new(*next_var);
                                    *next_var += 1;
                                    builder.declare_var(cache_var, types::F64);
                                    builder.def_var(cache_var, element_val);

                                    if !hoisted_load_arr_ids.contains(arr_id) {
                                        hoisted_load_arr_ids.push(*arr_id);
                                    }
                                    drop(arr_info);
                                    locals.get_mut(arr_id).unwrap()
                                        .hoisted_element_loads.get_or_insert_with(HashMap::new)
                                        .insert(*idx_id, cache_var);
                                }
                            }
                        }
                    }
                }

                // LICM: Hoist invariant i32 index products
                let mut invariant_products: HashSet<(LocalId, LocalId)> = HashSet::new();
                collect_invariant_products_stmts(body, counter_id, &assigned_in_loop, &mut invariant_products);

                let mut hoisted_product_ids: Vec<(LocalId, LocalId)> = Vec::new();
                for (a_id, b_id) in &invariant_products {
                    let a_val = try_compile_index_as_i32(builder, &Expr::LocalGet(*a_id), locals);
                    let b_val = try_compile_index_as_i32(builder, &Expr::LocalGet(*b_id), locals);
                    if let (Some(a_i32), Some(b_i32)) = (a_val, b_val) {
                        let product = builder.ins().imul(a_i32, b_i32);
                        let cache_var = Variable::new(*next_var);
                        *next_var += 1;
                        builder.declare_var(cache_var, types::I32);
                        builder.def_var(cache_var, product);
                        hoisted_product_ids.push((*a_id, *b_id));

                        locals.get_mut(a_id).unwrap()
                            .hoisted_i32_products.get_or_insert_with(HashMap::new)
                            .insert(*b_id, cache_var);
                    }
                }

                let header_block = builder.create_block();
                let body_block = builder.create_block();
                let update_block = builder.create_block();
                let exit_block = builder.create_block();

                builder.ins().jump(header_block, &[]);

                // Header (condition check)
                builder.switch_to_block(header_block);

                if let Some(cond) = condition {
                    // Use optimized condition if BCE pattern was detected
                    if let (Some(len_var), Some(idx_id)) = (cached_length_var, bce_index_var) {
                        if let Some(idx_info) = locals.get(&idx_id) {
                            let idx_i32 = builder.use_var(idx_info.var);
                            let length_i32 = builder.use_var(len_var);
                            let in_bounds = builder.ins().icmp(IntCC::SignedLessThan, idx_i32, length_i32);
                            builder.ins().brif(in_bounds, body_block, &[], exit_block, &[]);
                        } else {
                            let cond_bool = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, cond, this_ctx)?;
                            builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);
                        }
                    } else {
                        let cond_bool = compile_condition_to_bool(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, cond, this_ctx)?;
                        builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);
                    }
                } else {
                    builder.ins().jump(body_block, &[]);
                }

                let for_loop_ctx = LoopContext { exit_block, header_block: update_block, bounded_indices, try_depth: TRY_CATCH_DEPTH.with(|d| d.get()) };

                // Set BCE information on index variable
                if let Some(idx_id) = bce_index_var {
                    if let Some(idx_info) = locals.get_mut(&idx_id) {
                        if let Some(arr_id) = bce_array_var {
                            idx_info.bounded_by_array = Some(arr_id);
                        }
                        if let Some(limit) = bce_constant_limit {
                            idx_info.bounded_by_constant = Some(limit);
                        }
                    }
                }

                builder.switch_to_block(body_block);
                builder.seal_block(body_block);

                // OPTIMIZATION: Defer module-level variable write-backs for simple loops
                // (no function calls in body+update that could observe the global value).
                // Collect the set of module vars assigned in the loop, and skip their
                // global stores during body/update compilation. Flush after loop exit.
                let mut deferred_for_vars: HashSet<LocalId> = HashSet::new();
                let body_has_calls = loop_body_has_calls(body)
                    || update.as_ref().map_or(false, |u| loop_expr_has_calls(u));
                if !body_has_calls {
                    deferred_for_vars = collect_module_var_writes_in_loop(body, locals);
                    if let Some(upd) = update {
                        // Also check the update expression for module var writes
                        let mut update_stmts = vec![Stmt::Expr(upd.clone())];
                        let update_writes = collect_module_var_writes_in_loop(&update_stmts, locals);
                        deferred_for_vars.extend(update_writes);
                        drop(update_stmts);
                    }
                    if !deferred_for_vars.is_empty() {
                        DEFERRED_MODULE_WRITEBACK_VARS.with(|d| {
                            d.borrow_mut().extend(deferred_for_vars.iter().copied());
                        });
                    }
                }

                for s in body {
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, s, this_ctx, Some(&for_loop_ctx), boxed_vars, async_promise_var)?;
                }

                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    builder.ins().jump(update_block, &[]);
                }

                builder.switch_to_block(update_block);
                builder.seal_block(update_block);
                if let Some(upd) = update {
                    compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, upd, this_ctx)?;
                }

                builder.ins().jump(header_block, &[]);
                builder.seal_block(header_block);

                // Clear deferred set before flushing (so the flush stores aren't skipped)
                if !deferred_for_vars.is_empty() {
                    DEFERRED_MODULE_WRITEBACK_VARS.with(|d| {
                        let mut set = d.borrow_mut();
                        for id in &deferred_for_vars {
                            set.remove(id);
                        }
                    });
                }

                builder.switch_to_block(exit_block);
                builder.seal_block(exit_block);

                // Flush deferred module-level variable write-backs after loop exit
                for var_id in &deferred_for_vars {
                    if let Some(info) = locals.get(var_id) {
                        if let Some(data_id) = info.module_var_data_id {
                            let current_val = builder.use_var(info.var);
                            let val_type = builder.func.dfg.value_type(current_val);
                            let store_val = if val_type == types::I32 {
                                builder.ins().fcvt_from_sint(types::F64, current_val)
                            } else {
                                current_val
                            };
                            let global_val = module.declare_data_in_func(data_id, builder.func);
                            let ptr = builder.ins().global_value(types::I64, global_val);
                            builder.ins().store(MemFlags::new(), store_val, ptr, 0);
                        }
                    }
                }

                if let (Some(idx_id), Some(orig_f64_var)) = (bce_index_var, original_f64_var) {
                    if let Some(idx_info) = locals.get_mut(&idx_id) {
                        let final_i32 = builder.use_var(idx_info.var);
                        let final_f64 = builder.ins().fcvt_from_sint(types::F64, final_i32);
                        builder.def_var(orig_f64_var, final_f64);
                        idx_info.var = orig_f64_var;
                        idx_info.is_i32 = false;
                        idx_info.bounded_by_array = None;
                    }
                }

                // Clear cached array pointers after the loop
                for arr_id in &cached_array_ids {
                    if let Some(info) = locals.get_mut(arr_id) {
                        info.cached_array_ptr = None;
                    }
                }

                // Clear LICM hoisted element loads
                for arr_id in &hoisted_load_arr_ids {
                    if let Some(info) = locals.get_mut(arr_id) {
                        info.hoisted_element_loads = None;
                    }
                }

                // Clear LICM hoisted i32 products
                for (a_id, _) in &hoisted_product_ids {
                    if let Some(info) = locals.get_mut(a_id) {
                        info.hoisted_i32_products = None;
                    }
                }
            }
        }
        Stmt::Expr(expr) => {
            // Check if this is an array mutation method call that returns a new pointer
            // e.g., this.paths.push(x) or arr.push(x) - these need to update the property/variable
            let needs_update = matches!(expr, Expr::Call { callee, .. } if {
                if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                    matches!(property.as_str(), "push" | "unshift" | "splice" | "concat")
                } else {
                    false
                }
            });

            let result = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, expr, this_ctx)?;

            // If this is an array mutation method, update the property/variable with the returned pointer
            // EXCEPTION: push/unshift are handled by specialized code that stores the new array pointer
            // and returns the length, so we must NOT store the return value back to the property
            if needs_update {
                if let Expr::Call { callee, .. } = expr {
                    if let Expr::PropertyGet { object, property: method_name } = callee.as_ref() {
                        // Skip update for push/unshift - they're handled by specialized code
                        if !matches!(method_name.as_str(), "push" | "unshift") {
                        // Check if the object is a PropertyGet (this.paths) or LocalGet (arr)
                        match object.as_ref() {
                            Expr::PropertyGet { object: nested_obj, property: nested_prop } => {
                                match nested_obj.as_ref() {
                                    Expr::This => {
                                        // this.property.push() - use inline field storage
                                        if let Some(this) = this_ctx {
                                            if let Some(&field_idx) = this.class_meta.field_indices.get(nested_prop) {
                                                let this_ptr = builder.use_var(this.this_var);
                                                let result_f64 = ensure_f64(builder, result);
                                                let field_offset = 24 + (field_idx as i32) * 8;
                                                builder.ins().store(MemFlags::new(), result_f64, this_ptr, field_offset);
                                            }
                                        }
                                    }
                                    Expr::LocalGet(local_id) => {
                                        // localVar.property.push() - get the object and update its property
                                        if let Some(info) = locals.get(local_id) {
                                            let obj_val = builder.use_var(info.var);
                                            let obj_ptr = ensure_i64(builder, obj_val);
                                            let result_f64 = ensure_f64(builder, result);
                                            if let Some(ref class_name) = info.class_name {
                                                if let Some(class_meta) = classes.get(class_name) {
                                                    if let Some(&field_idx) = class_meta.field_indices.get(nested_prop) {
                                                        let field_offset = 24 + (field_idx as i32) * 8;
                                                        builder.ins().store(MemFlags::new(), result_f64, obj_ptr, field_offset);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Expr::LocalGet(id) => {
                                let var = Variable::from_u32(*id);
                                builder.def_var(var, result);
                            }
                            _ => {}
                        }
                        }  // end of !push/!unshift block
                    }
                }
            }
        }
        Stmt::Break => {
            if let Some(ctx) = loop_ctx {
                // Emit js_try_end() for each try block between here and the loop
                let try_depth = TRY_CATCH_DEPTH.with(|d| d.get());
                let cleanup_count = try_depth.saturating_sub(ctx.try_depth);
                emit_try_end_cleanup(builder, module, extern_funcs, cleanup_count)?;
                builder.ins().jump(ctx.exit_block, &[]);
            }
            // If no loop context, break is invalid but we silently ignore for now
        }
        Stmt::Continue => {
            if let Some(ctx) = loop_ctx {
                // Emit js_try_end() for each try block between here and the loop
                let try_depth = TRY_CATCH_DEPTH.with(|d| d.get());
                let cleanup_count = try_depth.saturating_sub(ctx.try_depth);
                emit_try_end_cleanup(builder, module, extern_funcs, cleanup_count)?;
                builder.ins().jump(ctx.header_block, &[]);
            }
            // If no loop context, continue is invalid but we silently ignore for now
        }
        Stmt::Throw(expr) => {
            // Compile the expression to throw
            let val = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, expr, this_ctx)?;

            // Call js_throw(value) - this function never returns (uses longjmp)
            // js_throw expects f64. If the value is an i64 (pointer), we need to NaN-box it
            // so that the catch block can correctly identify it as an object.
            let val_type = builder.func.dfg.value_type(val);
            let val_f64 = if val_type == types::I64 {
                // i64 pointer - NaN-box it as an object
                let nanbox_func = extern_funcs.get("js_nanbox_pointer")
                    .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                let nanbox_ref = module.declare_func_in_func(*nanbox_func, builder.func);
                let call = builder.ins().call(nanbox_ref, &[val]);
                builder.inst_results(call)[0]
            } else {
                // Already f64 (number, NaN-boxed value, etc.)
                val
            };
            let throw_func = extern_funcs.get("js_throw")
                .ok_or_else(|| anyhow!("js_throw not declared"))?;
            let throw_ref = module.declare_func_in_func(*throw_func, builder.func);
            builder.ins().call(throw_ref, &[val_f64]);

            // js_throw never returns, but Cranelift needs a terminator.
            // Create a new unreachable block and jump to it, then trap there.
            // This prevents the trap from being placed inline after the call.
            let unreachable_block = builder.create_block();
            builder.ins().jump(unreachable_block, &[]);
            builder.switch_to_block(unreachable_block);
            builder.seal_block(unreachable_block);
            builder.ins().trap(TrapCode::user(1).unwrap());
        }
        Stmt::Try { body, catch, finally } => {
            // Collect LocalIds that are assigned in the try body
            // These need stack slots to preserve values across longjmp
            fn collect_assigned_in_stmts(stmts: &[Stmt], assigned: &mut std::collections::HashSet<LocalId>) {
                for stmt in stmts {
                    collect_assigned_in_stmt(stmt, assigned);
                }
            }
            fn collect_assigned_in_stmt(stmt: &Stmt, assigned: &mut std::collections::HashSet<LocalId>) {
                match stmt {
                    Stmt::Expr(expr) => collect_assigned_in_expr(expr, assigned),
                    Stmt::If { condition, then_branch, else_branch } => {
                        collect_assigned_in_expr(condition, assigned);
                        collect_assigned_in_stmts(then_branch, assigned);
                        if let Some(else_stmts) = else_branch {
                            collect_assigned_in_stmts(else_stmts, assigned);
                        }
                    }
                    Stmt::While { condition, body } => {
                        collect_assigned_in_expr(condition, assigned);
                        collect_assigned_in_stmts(body, assigned);
                    }
                    Stmt::For { init, condition, update, body } => {
                        if let Some(init_stmt) = init {
                            collect_assigned_in_stmt(init_stmt, assigned);
                        }
                        if let Some(cond) = condition {
                            collect_assigned_in_expr(cond, assigned);
                        }
                        if let Some(upd) = update {
                            collect_assigned_in_expr(upd, assigned);
                        }
                        collect_assigned_in_stmts(body, assigned);
                    }
                    Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                        collect_assigned_in_expr(expr, assigned);
                    }
                    _ => {}
                }
            }
            fn collect_assigned_in_expr(expr: &Expr, assigned: &mut std::collections::HashSet<LocalId>) {
                match expr {
                    Expr::LocalSet(id, val) => {
                        assigned.insert(*id);
                        collect_assigned_in_expr(val, assigned);
                    }
                    Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
                        collect_assigned_in_expr(left, assigned);
                        collect_assigned_in_expr(right, assigned);
                    }
                    Expr::Unary { operand, .. } => collect_assigned_in_expr(operand, assigned),
                    Expr::Call { callee, args, .. } => {
                        collect_assigned_in_expr(callee, assigned);
                        for arg in args {
                            collect_assigned_in_expr(arg, assigned);
                        }
                    }
                    _ => {}
                }
            }

            let mut try_assigned: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
            collect_assigned_in_stmts(body, &mut try_assigned);

            // Create stack slots for ALL local variables that exist before the try block.
            // setjmp/longjmp only preserves callee-saved registers — after longjmp, any
            // variable Cranelift placed in a caller-saved register will have a stale value.
            // When the try body has complex control flow (if/else branches), the register
            // allocator may move variables around in ways that don't survive longjmp.
            // Saving ALL pre-existing locals ensures correctness in the catch block.
            // Map: LocalId -> (StackSlot, actual_var_type, original_var, was_i32)
            let mut try_var_slots: HashMap<LocalId, (StackSlot, types::Type, Variable, bool)> = HashMap::new();
            for (local_id, info) in locals.iter() {
                    let val = builder.use_var(info.var);
                    let var_type = builder.func.dfg.value_type(val);
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        8, // f64 or i64 = 8 bytes
                        8, // alignment
                    ));
                    // Store current value to slot before setjmp
                    builder.ins().stack_store(val, slot, 0);
                    // Store original var and is_i32 state for proper restoration after longjmp
                    try_var_slots.insert(*local_id, (slot, var_type, info.var, info.is_i32));
            }

            // Generate control flow blocks
            let try_body_block = builder.create_block();
            let catch_block = builder.create_block();
            let finally_block = builder.create_block();
            let merge_block = builder.create_block();

            // Track whether we entered the finally block via exception or normal path.
            // This is needed to:
            // 1. Only call js_try_end() once (in catch or in finally, not both)
            // 2. Re-throw exceptions after finally when there's no catch clause
            let had_exception_var = Variable::new(*next_var);
            *next_var += 1;
            builder.declare_var(had_exception_var, types::I32);
            let zero_val = builder.ins().iconst(types::I32, 0);
            builder.def_var(had_exception_var, zero_val);

            // Call js_try_push() to get a pointer to the jmp_buf
            let try_push_func = extern_funcs.get("js_try_push")
                .ok_or_else(|| anyhow!("js_try_push not declared"))?;
            let try_push_ref = module.declare_func_in_func(*try_push_func, builder.func);
            let call = builder.ins().call(try_push_ref, &[]);
            let jmp_buf_ptr = builder.inst_results(call)[0];

            // Call setjmp directly with the jmp_buf pointer
            // This is critical: setjmp must be called from this stack frame, not from inside a helper function
            let setjmp_func = extern_funcs.get("setjmp")
                .ok_or_else(|| anyhow!("setjmp not declared"))?;
            let setjmp_ref = module.declare_func_in_func(*setjmp_func, builder.func);
            let call = builder.ins().call(setjmp_ref, &[jmp_buf_ptr]);
            let setjmp_result = builder.inst_results(call)[0];

            // Branch: if setjmp returned 0, go to try body; otherwise go to catch
            let zero = builder.ins().iconst(types::I32, 0);
            let is_normal = builder.ins().icmp(IntCC::Equal, setjmp_result, zero);
            builder.ins().brif(is_normal, try_body_block, &[], catch_block, &[]);

            // Try body
            builder.switch_to_block(try_body_block);
            builder.seal_block(try_body_block);
            // Track try nesting so return/break/continue can emit js_try_end() cleanup
            TRY_CATCH_DEPTH.with(|d| d.set(d.get() + 1));
            for stmt in body {
                // Check if block is already filled (e.g., by throw/return)
                let current = builder.current_block().unwrap();
                if is_block_filled(builder, current) {
                    break;
                }
                compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, stmt, this_ctx, loop_ctx, boxed_vars, async_promise_var)?;

                // After compiling each statement, store modified variables to stack slots
                // This ensures the value survives longjmp
                // Only do this if the block isn't already terminated (e.g., by throw)
                let current_after = builder.current_block().unwrap();
                if !is_block_filled(builder, current_after) {
                    for (local_id, (slot, slot_type, _orig_var, _was_i32)) in &try_var_slots {
                        if let Some(info) = locals.get(local_id) {
                            let val = builder.use_var(info.var);
                            let val_type = builder.func.dfg.value_type(val);
                            // Convert to the expected slot type if needed
                            // Loop optimization might have changed the variable to i32
                            let store_val = if info.is_i32 {
                                // Variable is now i32, convert to slot type for storage
                                if *slot_type == types::I64 {
                                    builder.ins().sextend(types::I64, val)
                                } else {
                                    builder.ins().fcvt_from_sint(types::F64, val)
                                }
                            } else if *slot_type == types::I64 && val_type == types::F64 {
                                ensure_i64(builder, val)
                            } else if *slot_type == types::F64 && val_type == types::I64 {
                                builder.ins().bitcast(types::F64, MemFlags::new(), val)
                            } else {
                                val
                            };
                            builder.ins().stack_store(store_val, *slot, 0);
                        }
                    }
                }
            }
            // Restore codegen-time try depth (must happen regardless of block state)
            TRY_CATCH_DEPTH.with(|d| d.set(d.get() - 1));
            // Jump to finally (or merge if no finally)
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                if finally.is_some() {
                    builder.ins().jump(finally_block, &[]);
                } else {
                    // Call js_try_end() before leaving
                    let try_end_func = extern_funcs.get("js_try_end")
                        .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                    let try_end_ref = module.declare_func_in_func(*try_end_func, builder.func);
                    builder.ins().call(try_end_ref, &[]);
                    builder.ins().jump(merge_block, &[]);
                }
            }

            // Catch block
            builder.switch_to_block(catch_block);
            builder.seal_block(catch_block);

            // Restore variables from stack slots (longjmp may have clobbered SSA values)
            // CRITICAL: We must restore to the ORIGINAL variable, not the current info.var
            // Loop optimization inside the try block might have changed info.var to an i32 variable
            for (local_id, (slot, slot_type, orig_var, was_i32)) in &try_var_slots {
                let val = builder.ins().stack_load(*slot_type, *slot, 0);
                // Always restore to the original variable (which has the correct declared type)
                builder.def_var(*orig_var, val);
                // Restore the LocalInfo to its original state
                if let Some(info) = locals.get_mut(local_id) {
                    info.var = *orig_var;
                    info.is_i32 = *was_i32;
                }
            }

            // Pop this try level FIRST, before executing catch body
            // This ensures any throw inside catch propagates to outer try
            {
                let try_end_func_catch = extern_funcs.get("js_try_end")
                    .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                let try_end_ref_catch = module.declare_func_in_func(*try_end_func_catch, builder.func);
                builder.ins().call(try_end_ref_catch, &[]);
            }

            // Mark that we entered via exception path
            let one_val = builder.ins().iconst(types::I32, 1);
            builder.def_var(had_exception_var, one_val);

            if let Some(catch_clause) = catch {
                // Get the exception value
                let get_exc_func = extern_funcs.get("js_get_exception")
                    .ok_or_else(|| anyhow!("js_get_exception not declared"))?;
                let get_exc_ref = module.declare_func_in_func(*get_exc_func, builder.func);
                let call = builder.ins().call(get_exc_ref, &[]);
                let exc_val = builder.inst_results(call)[0];

                // If catch has a parameter, bind it
                if let Some((param_id, param_name)) = &catch_clause.param {
                    let var = Variable::new(*next_var);
                    *next_var += 1;
                    builder.declare_var(var, types::F64);
                    builder.def_var(var, exc_val);
                    locals.insert(*param_id, LocalInfo {
                        var,
                        name: Some(param_name.clone()),
                        class_name: None,
                        type_args: Vec::new(),
                        is_pointer: false,
                        is_array: false,
                        is_string: false,
                        is_bigint: false,
                        is_closure: false, closure_func_id: None,
                        is_boxed: false,
                        // Caught exceptions can be any type, so mark as union for runtime type checking
                        is_map: false, is_set: false, is_buffer: false, is_event_emitter: false, is_union: true,
                        is_mixed_array: false,
                        is_integer: false,
                        is_integer_array: false,
                        is_i32: false, is_boolean: false,
                        i32_shadow: None,
                        bounded_by_array: None,
                        bounded_by_constant: None,
                        scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    });
                }

                // Clear the exception since we're handling it
                let clear_exc_func = extern_funcs.get("js_clear_exception")
                    .ok_or_else(|| anyhow!("js_clear_exception not declared"))?;
                let clear_exc_ref = module.declare_func_in_func(*clear_exc_func, builder.func);
                builder.ins().call(clear_exc_ref, &[]);

                // Compile catch body
                for stmt in &catch_clause.body {
                    let current = builder.current_block().unwrap();
                    if is_block_filled(builder, current) {
                        break;
                    }
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, stmt, this_ctx, loop_ctx, boxed_vars, async_promise_var)?;
                }
            }

            // After catch, jump to finally (or merge)
            // Note: js_try_end() was already called at the start of catch block
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                if finally.is_some() {
                    builder.ins().jump(finally_block, &[]);
                } else {
                    builder.ins().jump(merge_block, &[]);
                }
            }

            // Finally block (if present)
            builder.switch_to_block(finally_block);
            builder.seal_block(finally_block);

            if let Some(finally_stmts) = finally {
                // Mark entering finally
                let enter_finally_func = extern_funcs.get("js_enter_finally")
                    .ok_or_else(|| anyhow!("js_enter_finally not declared"))?;
                let enter_finally_ref = module.declare_func_in_func(*enter_finally_func, builder.func);
                builder.ins().call(enter_finally_ref, &[]);

                // Compile finally body
                for stmt in finally_stmts {
                    let current = builder.current_block().unwrap();
                    if is_block_filled(builder, current) {
                        break;
                    }
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, stmt, this_ctx, loop_ctx, boxed_vars, async_promise_var)?;
                }

                // Mark leaving finally
                let leave_finally_func = extern_funcs.get("js_leave_finally")
                    .ok_or_else(|| anyhow!("js_leave_finally not declared"))?;
                let leave_finally_ref = module.declare_func_in_func(*leave_finally_func, builder.func);
                builder.ins().call(leave_finally_ref, &[]);
            }

            // Only call js_try_end() if we came from the normal path (no exception).
            // In the exception path, js_try_end() was already called in the catch block.
            let current = builder.current_block().unwrap();
            if !is_block_filled(builder, current) {
                let had_exc = builder.use_var(had_exception_var);
                let zero_i32 = builder.ins().iconst(types::I32, 0);
                let was_normal = builder.ins().icmp(IntCC::Equal, had_exc, zero_i32);

                let try_end_block = builder.create_block();
                let after_try_end_block = builder.create_block();
                builder.ins().brif(was_normal, try_end_block, &[], after_try_end_block, &[]);

                builder.switch_to_block(try_end_block);
                builder.seal_block(try_end_block);
                let try_end_func = extern_funcs.get("js_try_end")
                    .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                let try_end_ref = module.declare_func_in_func(*try_end_func, builder.func);
                builder.ins().call(try_end_ref, &[]);
                builder.ins().jump(after_try_end_block, &[]);

                builder.switch_to_block(after_try_end_block);
                builder.seal_block(after_try_end_block);

                // If there's no catch clause and we had an exception, re-throw it
                // This implements JavaScript's try {} finally {} semantics:
                // the finally block runs, then the exception propagates.
                if catch.is_none() {
                    let had_exc2 = builder.use_var(had_exception_var);
                    let one_i32 = builder.ins().iconst(types::I32, 1);
                    let should_rethrow = builder.ins().icmp(IntCC::Equal, had_exc2, one_i32);

                    let rethrow_block = builder.create_block();
                    builder.ins().brif(should_rethrow, rethrow_block, &[], merge_block, &[]);

                    builder.switch_to_block(rethrow_block);
                    builder.seal_block(rethrow_block);

                    // Get the stored exception and re-throw it
                    let get_exc_func = extern_funcs.get("js_get_exception")
                        .ok_or_else(|| anyhow!("js_get_exception not declared"))?;
                    let get_exc_ref = module.declare_func_in_func(*get_exc_func, builder.func);
                    let call = builder.ins().call(get_exc_ref, &[]);
                    let exc_val = builder.inst_results(call)[0];

                    let throw_func = extern_funcs.get("js_throw")
                        .ok_or_else(|| anyhow!("js_throw not declared"))?;
                    let throw_ref = module.declare_func_in_func(*throw_func, builder.func);
                    builder.ins().call(throw_ref, &[exc_val]);
                    // js_throw is divergent (longjmp), but Cranelift may need a terminator
                    builder.ins().trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
                } else {
                    builder.ins().jump(merge_block, &[]);
                }
            }

            // Merge block - continue after try-catch-finally
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
        }
        Stmt::Switch { discriminant, cases } => {
            // Check if this is a string switch (any case has a string literal or string enum member test)
            let is_string_switch = cases.iter().any(|c| {
                match c.test.as_ref() {
                    Some(Expr::String(_)) => true,
                    Some(Expr::EnumMember { enum_name, member_name }) => {
                        let key = (enum_name.clone(), member_name.clone());
                        matches!(enums.get(&key), Some(EnumMemberValue::String(_)))
                    }
                    _ => false,
                }
            });

            // Evaluate the discriminant once
            let disc_val_raw = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, discriminant, this_ctx)?;
            let disc_val = ensure_f64(builder, disc_val_raw);

            // For string switches, extract the discriminant string pointer once
            let disc_str_ptr = if is_string_switch {
                let get_str_ptr_func = extern_funcs.get("js_get_string_pointer_unified")
                    .ok_or_else(|| anyhow!("js_get_string_pointer_unified not declared"))?;
                let get_str_ptr_ref = module.declare_func_in_func(*get_str_ptr_func, builder.func);
                let disc_call = builder.ins().call(get_str_ptr_ref, &[disc_val]);
                Some(builder.inst_results(disc_call)[0])
            } else {
                None
            };

            // Create blocks for each case body and a merge block
            let merge_block = builder.create_block();
            let case_blocks: Vec<_> = cases.iter().map(|_| builder.create_block()).collect();

            // Find the default case index (if any)
            let default_idx = cases.iter().position(|c| c.test.is_none());

            // Create a block for each case's test (for non-default cases)
            let mut test_blocks: Vec<_> = (0..cases.len()).map(|_| builder.create_block()).collect();

            // Start by jumping to the first test block (or default/merge if no cases)
            if cases.is_empty() {
                builder.ins().jump(merge_block, &[]);
            } else {
                builder.ins().jump(test_blocks[0], &[]);
            }

            // Generate test blocks - each tests its case value and jumps accordingly
            for (i, case) in cases.iter().enumerate() {
                builder.switch_to_block(test_blocks[i]);
                builder.seal_block(test_blocks[i]);

                if let Some(ref test_expr) = case.test {
                    // Compare discriminant with case value
                    let test_val_raw = compile_expr(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, test_expr, this_ctx)?;
                    let test_val = ensure_f64(builder, test_val_raw);

                    let eq = if is_string_switch {
                        // String comparison: use js_string_equals
                        let get_str_ptr_func = extern_funcs.get("js_get_string_pointer_unified")
                            .ok_or_else(|| anyhow!("js_get_string_pointer_unified not declared"))?;
                        let get_str_ptr_ref = module.declare_func_in_func(*get_str_ptr_func, builder.func);
                        let test_call = builder.ins().call(get_str_ptr_ref, &[test_val]);
                        let test_str_ptr = builder.inst_results(test_call)[0];

                        let equals_func = extern_funcs.get("js_string_equals")
                            .ok_or_else(|| anyhow!("js_string_equals not declared"))?;
                        let equals_ref = module.declare_func_in_func(*equals_func, builder.func);
                        let cmp_call = builder.ins().call(equals_ref, &[disc_str_ptr.unwrap(), test_str_ptr]);
                        let result = builder.inst_results(cmp_call)[0]; // i32 bool
                        builder.ins().icmp_imm(IntCC::NotEqual, result, 0)
                    } else {
                        // Numeric comparison
                        builder.ins().fcmp(FloatCC::Equal, disc_val, test_val)
                    };

                    // If equal, jump to case body; otherwise, try next case
                    let next_test = if i + 1 < cases.len() {
                        test_blocks[i + 1]
                    } else if let Some(def_idx) = default_idx {
                        case_blocks[def_idx]
                    } else {
                        merge_block
                    };
                    builder.ins().brif(eq, case_blocks[i], &[], next_test, &[]);
                } else {
                    // Default case - will be reached via fallthrough from last non-matching test
                    // Just jump to the case body
                    builder.ins().jump(case_blocks[i], &[]);
                }
            }

            // Generate case body blocks with fall-through semantics
            // Set up loop context for break statements to target merge_block
            let switch_loop_ctx = Some(LoopContext {
                exit_block: merge_block,
                header_block: merge_block, // continue in switch goes to merge (unusual but safe)
                bounded_indices: HashMap::new(),
                try_depth: TRY_CATCH_DEPTH.with(|d| d.get()),
            });

            for (i, case) in cases.iter().enumerate() {
                builder.switch_to_block(case_blocks[i]);
                builder.seal_block(case_blocks[i]);

                // Compile case body
                for stmt in &case.body {
                    let current = builder.current_block().unwrap();
                    if is_block_filled(builder, current) {
                        break;
                    }
                    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, stmt, this_ctx, switch_loop_ctx.as_ref(), boxed_vars, async_promise_var)?;
                }

                // Fall through to next case body (if not already terminated by break/return)
                let current = builder.current_block().unwrap();
                if !is_block_filled(builder, current) {
                    let next_block = if i + 1 < cases.len() {
                        case_blocks[i + 1]
                    } else {
                        merge_block
                    };
                    builder.ins().jump(next_block, &[]);
                }
            }

            // Merge block
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
        }
    }
    Ok(())
}

/// Detect if an expression contains a `State.value` reference with surrounding string parts.
/// Returns (prefix, suffix, state_object_expr) if found.
/// Handles: bare `state.value`, `"pre" + state.value`, `state.value + "suf"`,
/// and template literals like `\`pre${state.value}suf\`` (desugared to nested Add).

pub(crate) fn compile_stmt_with_this(

    builder: &mut FunctionBuilder,

    module: &mut ObjectModule,

    func_ids: &HashMap<u32, cranelift_module::FuncId>,

    closure_func_ids: &HashMap<u32, cranelift_module::FuncId>,

    func_wrapper_ids: &HashMap<u32, cranelift_module::FuncId>,

    extern_funcs: &HashMap<Cow<'static, str>, cranelift_module::FuncId>,

    async_func_ids: &HashSet<u32>,

    closure_returning_funcs: &HashSet<u32>,

    classes: &HashMap<String, ClassMeta>,

    enums: &HashMap<(String, String), EnumMemberValue>,

    func_param_types: &HashMap<u32, Vec<types::Type>>, func_union_params: &HashMap<u32, Vec<bool>>,

    func_return_types: &HashMap<u32, types::Type>,

    func_hir_return_types: &HashMap<u32, perry_types::Type>,

    func_rest_param_index: &HashMap<u32, usize>,

    imported_func_param_counts: &HashMap<String, usize>,

    locals: &mut HashMap<LocalId, LocalInfo>,

    next_var: &mut usize,

    stmt: &Stmt,

    this_var: Variable,

    class_meta: &ClassMeta,

    loop_ctx: Option<&LoopContext>,

    boxed_vars: &std::collections::HashSet<LocalId>,

) -> Result<()> {

    let this_ctx = ThisContext {

        this_var,

        class_meta: class_meta.clone(),

    };

    compile_stmt(builder, module, func_ids, closure_func_ids, func_wrapper_ids, extern_funcs, async_func_ids, closure_returning_funcs, classes, enums, func_param_types, func_union_params, func_return_types, func_hir_return_types, func_rest_param_index, imported_func_param_counts, locals, next_var, stmt, Some(&this_ctx), loop_ctx, boxed_vars, None)

}
