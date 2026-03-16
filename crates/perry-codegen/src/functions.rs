//! Function compilation for the codegen module.
//!
//! Contains methods for declaring and compiling top-level functions,
//! integer-specialized functions, and function wrapper generation.

use anyhow::{anyhow, Result};
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap, HashSet};

use perry_hir::{
    CompareOp,
    UnaryOp,
    BinaryOp, Expr, Function, Stmt,
};
use perry_types::LocalId;

use crate::stmt::compile_async_stmt;
use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext, LoopContext};
use crate::util::*;
use crate::stmt::compile_stmt;
use crate::expr::compile_expr;

impl crate::codegen::Compiler {
    pub(crate) fn declare_function(&mut self, func: &Function) -> Result<()> {
        let mut sig = self.module.make_signature();

        // Add parameters based on their types
        // Track rest parameter index if any
        for (i, param) in func.params.iter().enumerate() {
            let abi_type = self.type_to_abi(&param.ty);
            sig.params.push(AbiParam::new(abi_type));
            if param.is_rest {
                self.func_rest_param_index.insert(func.id, i);
            }
        }

        // Add return type based on the declared return type
        if func.is_async {
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            self.async_func_ids.insert(func.id);
        } else {
            let return_abi = self.type_to_abi(&func.return_type);
            sig.returns.push(AbiParam::new(return_abi));
        }

        // Rename user "main" to "_user_main" to avoid conflict with C entry point
        let symbol_name = if func.name == "main" {
            "_user_main"
        } else {
            &func.name
        };

        // Use Local linkage for all functions - cross-module calls go through
        // scoped __wrapper_ functions, so the raw name doesn't need to be exported.
        // This prevents duplicate symbol errors when two modules define functions
        // with the same name (e.g., formatTokenAmount in lib/generic.ts and lib/risk-assessment.ts).
        let linkage = if false && func.is_exported {
            Linkage::Export
        } else {
            Linkage::Local
        };

        let func_id = match self.module.declare_function(symbol_name, linkage, &sig) {
            Ok(id) => id,
            Err(e) => {
                // Check if this is an incompatible declaration error
                // If so, try to find the existing function by name and use its ID
                // This handles optional parameters where functions may have different param counts
                let err_str = format!("{:?}", e);
                let err_msg = e.to_string();
                let is_incompatible = err_str.to_lowercase().contains("incompatible") ||
                                       err_msg.to_lowercase().contains("incompatible") ||
                                       matches!(e, cranelift_module::ModuleError::IncompatibleDeclaration(_));
                if is_incompatible {
                    // Try to find existing function by iterating all declarations
                    for (id, decl) in self.module.declarations().get_functions() {
                        if decl.name.as_deref() == Some(symbol_name) {
                            // Already have this function declared, map our func.id to the existing ID
                            self.func_ids.insert(func.id, id);
                            return Ok(());
                        }
                    }
                    // If not found, this is a real error
                    return Err(anyhow!("Failed to declare function {} (symbol: {}): incompatible signature and no existing declaration found", func.name, symbol_name));
                } else {
                    return Err(anyhow!("Failed to declare function {} (symbol: {}): {}", func.name, symbol_name, e));
                }
            }
        };
        self.func_ids.insert(func.id, func_id);

        Ok(())
    }

    /// Check if a function body is fully integer-compatible (no strings, objects, floats, etc.)
    /// This enables generating an i64 specialization for better performance
    pub(crate) fn is_integer_only_function(func: &Function) -> bool {
        // All params must be Number type and return type must be Number
        // Type::Any must NOT be accepted - it could be BigInt, string, etc.
        // Integer specialization uses native sdiv/mul which crashes on NaN-boxed BigInt values.
        if !func.params.iter().all(|p| matches!(p.ty, perry_types::Type::Number)) {
            return false;
        }
        if !matches!(func.return_type, perry_types::Type::Number | perry_types::Type::Any) {
            return false;
        }
        // Must not be async
        if func.is_async { return false; }
        // Body must only contain integer-compatible operations
        fn is_integer_expr(expr: &Expr, func_id: u32) -> bool {
            match expr {
                Expr::Integer(_) => true,
                Expr::Number(f) => f.fract() == 0.0,
                Expr::LocalGet(_) => true,
                Expr::Binary { op, left, right } => {
                    matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Mod |
                             BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor |
                             BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr) &&
                    is_integer_expr(left, func_id) && is_integer_expr(right, func_id)
                }
                Expr::Compare { left, right, .. } => {
                    is_integer_expr(left, func_id) && is_integer_expr(right, func_id)
                }
                Expr::Unary { op, operand } => {
                    matches!(op, UnaryOp::Neg | UnaryOp::BitNot) && is_integer_expr(operand, func_id)
                }
                Expr::Conditional { condition, then_expr, else_expr } => {
                    is_integer_expr(condition, func_id) && is_integer_expr(then_expr, func_id) && is_integer_expr(else_expr, func_id)
                }
                Expr::Call { callee, args, .. } => {
                    // Only allow self-recursive calls
                    if let Expr::FuncRef(id) = callee.as_ref() {
                        *id == func_id && args.iter().all(|a| is_integer_expr(a, func_id))
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }
        fn is_integer_stmt(stmt: &Stmt, func_id: u32) -> bool {
            match stmt {
                Stmt::Return(Some(expr)) => is_integer_expr(expr, func_id),
                Stmt::Return(None) => true,
                Stmt::Expr(expr) => is_integer_expr(expr, func_id),
                Stmt::If { condition, then_branch, else_branch } => {
                    is_integer_expr(condition, func_id) &&
                    then_branch.iter().all(|s| is_integer_stmt(s, func_id)) &&
                    else_branch.as_ref().map(|eb| eb.iter().all(|s| is_integer_stmt(s, func_id))).unwrap_or(true)
                }
                Stmt::Let { init: Some(expr), .. } => is_integer_expr(expr, func_id),
                Stmt::Let { init: None, .. } => true,
                _ => false,
            }
        }
        func.body.iter().all(|s| is_integer_stmt(s, func.id))
    }

    pub(crate) fn compile_function(&mut self, func: &Function) -> Result<()> {
        // Track current function for self-recursive call optimization
        CURRENT_FUNC_HIR_ID.with(|c| c.set(Some(func.id)));

        if self.module_symbol_prefix.contains("modular") || self.module_symbol_prefix.contains("_u64") {
            let param_types: Vec<_> = func.params.iter().map(|p| format!("{}: {:?}", p.name, p.ty)).collect();
            eprintln!("[COMPILE_FUNC] module={} func={} id={} params=[{}] return={:?}",
                self.module_symbol_prefix, func.name, func.id, param_types.join(", "), func.return_type);
        }

        let result = if Self::is_integer_only_function(func) && func.params.len() <= 4 {
            self.compile_integer_specialized_function(func)
        } else {
            self.compile_function_inner(func)
        };

        CURRENT_FUNC_HIR_ID.with(|c| c.set(None));
        result
    }

    pub(crate) fn compile_function_inner(&mut self, func: &Function) -> Result<()> {
        let func_id = *self.func_ids.get(&func.id)
            .ok_or_else(|| anyhow!("Function not declared: {}", func.name))?;

        // Set up the function signature with actual types
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // Collect param types for use in local info
        let param_abi_types: Vec<types::Type> = func.params.iter()
            .map(|p| self.type_to_abi(&p.ty))
            .collect();

        for abi_type in &param_abi_types {
            self.ctx.func.signature.params.push(AbiParam::new(*abi_type));
        }

        // Async functions return a Promise (i64 pointer)
        if func.is_async {
            self.ctx.func.signature.returns.push(AbiParam::new(types::I64));
        } else {
            let return_abi = self.type_to_abi(&func.return_type);
            self.ctx.func.signature.returns.push(AbiParam::new(return_abi));
        }

        // Collect all variables that will be mutably captured by closures (before borrowing self.ctx)
        let boxed_vars = self.collect_mutable_captures_from_stmts(&func.body);

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut func_build_ctx = FunctionBuilderContext::new();
        {
            // Build the function
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut func_build_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Create variables for parameters using sequential indices (0, 1, 2, ...)
            let mut locals: BTreeMap<LocalId, LocalInfo> = BTreeMap::new();
            for (i, param) in func.params.iter().enumerate() {
                let var = Variable::new(i);  // Use sequential index, not param.id
                let abi_type = param_abi_types[i];
                builder.declare_var(var, abi_type);
                let val = builder.block_params(entry_block)[i];
                builder.def_var(var, val);
                // Determine local info flags based on type
                // String enums (e.g., ChainName) are strings at runtime
                let is_string = matches!(param.ty, perry_types::Type::String) || {
                    if let perry_types::Type::Named(name) = &param.ty {
                        self.enums.iter().any(|((enum_name, _), v)| enum_name == name && matches!(v, EnumMemberValue::String(_)))
                    } else {
                        false
                    }
                };
                let is_array = matches!(&param.ty, perry_types::Type::Array(_));
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                let is_bigint = matches!(param.ty, perry_types::Type::BigInt);
                let is_numeric_enum = if let perry_types::Type::Named(name) = &param.ty {
                    self.enums.iter().any(|((en, _), _)| en == name)
                        && !self.enums.iter().any(|((en, _), v)| en == name && matches!(v, EnumMemberValue::String(_)))
                } else { false };
                let is_pointer = !is_numeric_enum && abi_type == types::I64;
                // Detect Map/Set parameter types for proper property dispatch
                let is_map = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Map")
                    || matches!(&param.ty, perry_types::Type::Named(name) if name == "Map");
                let is_set = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Set")
                    || matches!(&param.ty, perry_types::Type::Named(name) if name == "Set");
                // Named types (interfaces) and Object types may contain NaN-boxed values
                // when accessed via PropertyGet, so treat them as potentially union
                let is_union = !is_numeric_enum && matches!(param.ty,
                    perry_types::Type::Union(_) |
                    perry_types::Type::Named(_) |
                    perry_types::Type::Object(_) |
                    perry_types::Type::Any);
                // Check if array has mixed element types (union or any)
                let is_mixed_array = if let perry_types::Type::Array(elem_ty) = &param.ty {
                    matches!(elem_ty.as_ref(), perry_types::Type::Union(_) | perry_types::Type::Any)
                } else {
                    false
                };
                locals.insert(param.id, LocalInfo {
                    var,
                    name: Some(param.name.clone()),
                    class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                    type_args: Vec::new(),
                    is_pointer,
                    is_array,
                    is_string,
                    is_bigint,
                    is_closure,
                    is_boxed: false,
                    is_map, is_set, is_buffer: false, is_event_emitter: false, is_union,
                    is_mixed_array,
                    is_integer: matches!(param.ty, perry_types::Type::Number),
                    is_integer_array: false,
                    is_i32: false,
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                });
            }

            // Add i32 shadow variables for Number function parameters that aren't reassigned
            // This avoids repeated fcvt_to_sint conversions when params are used in array indices
            {
                fn collect_assigned_param_ids(stmts: &[Stmt], assigned: &mut HashSet<LocalId>) {
                    for s in stmts {
                        match s {
                            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => collect_assigned_param_ids_expr(e, assigned),
                            Stmt::Let { init: Some(e), .. } => collect_assigned_param_ids_expr(e, assigned),
                            Stmt::If { condition, then_branch, else_branch, .. } => {
                                collect_assigned_param_ids_expr(condition, assigned);
                                collect_assigned_param_ids(then_branch, assigned);
                                if let Some(eb) = else_branch { collect_assigned_param_ids(eb, assigned); }
                            }
                            Stmt::For { init, condition, update, body } => {
                                if let Some(i) = init { collect_assigned_param_ids(&[i.as_ref().clone()], assigned); }
                                if let Some(c) = condition { collect_assigned_param_ids_expr(c, assigned); }
                                if let Some(u) = update { collect_assigned_param_ids_expr(u, assigned); }
                                collect_assigned_param_ids(body, assigned);
                            }
                            Stmt::While { condition, body } => {
                                collect_assigned_param_ids_expr(condition, assigned);
                                collect_assigned_param_ids(body, assigned);
                            }
                            _ => {}
                        }
                    }
                }
                fn collect_assigned_param_ids_expr(expr: &Expr, assigned: &mut HashSet<LocalId>) {
                    match expr {
                        Expr::LocalSet(id, val) => { assigned.insert(*id); collect_assigned_param_ids_expr(val, assigned); }
                        Expr::Update { id, .. } => { assigned.insert(*id); }
                        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
                        Expr::Logical { left, right, .. } => {
                            collect_assigned_param_ids_expr(left, assigned);
                            collect_assigned_param_ids_expr(right, assigned);
                        }
                        Expr::Unary { operand, .. } => collect_assigned_param_ids_expr(operand, assigned),
                        Expr::Call { callee, args, .. } => {
                            collect_assigned_param_ids_expr(callee, assigned);
                            for a in args { collect_assigned_param_ids_expr(a, assigned); }
                        }
                        Expr::IndexGet { object, index } => {
                            collect_assigned_param_ids_expr(object, assigned);
                            collect_assigned_param_ids_expr(index, assigned);
                        }
                        Expr::IndexSet { object, index, value } => {
                            collect_assigned_param_ids_expr(object, assigned);
                            collect_assigned_param_ids_expr(index, assigned);
                            collect_assigned_param_ids_expr(value, assigned);
                        }
                        _ => {}
                    }
                }
                let mut assigned_params: HashSet<LocalId> = HashSet::new();
                collect_assigned_param_ids(&func.body, &mut assigned_params);

                for param in &func.params {
                    if matches!(param.ty, perry_types::Type::Number) && !assigned_params.contains(&param.id) {
                        if let Some(info) = locals.get_mut(&param.id) {
                            let shadow = Variable::new(next_temp_var_id());
                            builder.declare_var(shadow, types::I32);
                            let f64_val = builder.use_var(info.var);
                            // Safe conversion: f64 -> i64 -> i32 (ireduce won't trap)
                            // Direct fcvt_to_sint_sat(I32) traps on ARM64 if value > i32::MAX
                            let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, f64_val);
                            let i32_val = builder.ins().ireduce(types::I32, i64_val);
                            builder.def_var(shadow, i32_val);
                            info.i32_shadow = Some(shadow);
                        }
                    }
                }
            }

            // Load module-level variables from their global slots
            for (local_id, data_id) in &self.module_var_data_ids {
                // Skip if this LocalId is already a function parameter
                if locals.contains_key(local_id) {
                    continue;
                }
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    let vt = if info.is_pointer && !info.is_union { types::I64 } else { types::F64 };
                    (vt, info.clone())
                } else {
                    (types::F64, LocalInfo {
                        var: Variable::new(0),
                        name: None, class_name: None, type_args: Vec::new(),
                        is_pointer: false, is_array: false, is_string: false, is_bigint: false,
                        is_closure: false, is_boxed: false,
                        is_map: false, is_set: false, is_buffer: false, is_event_emitter: false, is_union: false,
                        is_mixed_array: false, is_integer: false, is_integer_array: false, is_i32: false,
                        i32_shadow: None, bounded_by_array: None, bounded_by_constant: None,
                        scalar_fields: None, squared_cache: None, product_cache: None,
                        cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    })
                };
                // Use next_temp_var_id() for guaranteed unique variable IDs
                let var = Variable::new(next_temp_var_id());
                builder.declare_var(var, var_type);
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);
                let mut info = local_info_template;
                info.var = var;
                // Propagate the global slot DataId so that closures inside this
                // named function can also use the global slot as their box pointer,
                // keeping module-level variable writes in sync everywhere.
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // For async functions, create a Promise variable to track
            let promise_var = if func.is_async {
                // Use next_temp_var_id() to get a guaranteed unique ID (avoids conflicts with other temp vars)
                let var = Variable::new(next_temp_var_id());
                builder.declare_var(var, types::I64);

                // Allocate the promise: js_promise_new()
                let promise_new = self.extern_funcs.get("js_promise_new")
                    .ok_or_else(|| anyhow!("js_promise_new not declared"))?;
                let func_ref = self.module.declare_func_in_func(*promise_new, builder.func);
                let call = builder.ins().call(func_ref, &[]);
                let promise_ptr = builder.inst_results(call)[0];
                builder.def_var(var, promise_ptr);

                Some(var)
            } else {
                None
            };

            // Compile the function body
            // next_var continues from where params left off (params use 0..params.len())
            let mut next_var = func.params.len();

            // Generate default parameter checks for cross-module calls.
            // When functions are called cross-module through wrappers, missing optional params
            // arrive as 0 (null) for I64 types or NaN for F64 types (from TAG_UNDEFINED conversion).
            // We handle simple default values (literals, enum members) inline.
            for (i, param) in func.params.iter().enumerate() {
                if let Some(default_expr) = &param.default {
                    let var = Variable::new(i);
                    let abi_type = param_abi_types[i];

                    // Helper closure to create a string constant from bytes
                    let make_string_default = |builder: &mut FunctionBuilder, module: &mut ObjectModule, s: &str| -> Option<Value> {
                        if let Some(func_id) = self.extern_funcs.get("js_string_from_bytes") {
                            let func_ref = module.declare_func_in_func(*func_id, builder.func);
                            let bytes = s.as_bytes();
                            let data_id = module.declare_anonymous_data(false, false).ok()?;
                            let mut data_desc = cranelift_module::DataDescription::new();
                            data_desc.define(bytes.to_vec().into_boxed_slice());
                            module.define_data(data_id, &data_desc).ok()?;
                            let data_val = module.declare_data_in_func(data_id, builder.func);
                            let ptr = builder.ins().global_value(types::I64, data_val);
                            let len = builder.ins().iconst(types::I32, bytes.len() as i64);
                            let call = builder.ins().call(func_ref, &[ptr, len]);
                            Some(builder.inst_results(call)[0])
                        } else {
                            None
                        }
                    };

                    // Resolve enum value from various representations
                    let resolve_enum_string = |enum_name: &str, member_name: &str| -> Option<String> {
                        if let Some(value) = self.enums.get(&(enum_name.to_string(), member_name.to_string())) {
                            match value {
                                EnumMemberValue::String(s) => Some(s.clone()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    };
                    let resolve_enum_number = |enum_name: &str, member_name: &str| -> Option<f64> {
                        if let Some(value) = self.enums.get(&(enum_name.to_string(), member_name.to_string())) {
                            match value {
                                EnumMemberValue::Number(n) => Some(*n as f64),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    };

                    // Only handle simple default values that don't require full expression compilation
                    let default_val_opt: Option<Value> = match default_expr {
                        Expr::String(s) => {
                            if abi_type == types::I64 {
                                make_string_default(&mut builder, &mut self.module, s)
                            } else {
                                None
                            }
                        }
                        Expr::Number(n) => {
                            if abi_type == types::F64 {
                                Some(builder.ins().f64const(*n))
                            } else if abi_type == types::I64 {
                                Some(builder.ins().iconst(types::I64, *n as i64))
                            } else {
                                None
                            }
                        }
                        Expr::Integer(n) => {
                            if abi_type == types::F64 {
                                Some(builder.ins().f64const(*n as f64))
                            } else if abi_type == types::I64 {
                                Some(builder.ins().iconst(types::I64, *n))
                            } else {
                                None
                            }
                        }
                        Expr::Bool(b) => {
                            if abi_type == types::F64 {
                                Some(builder.ins().f64const(if *b { 1.0 } else { 0.0 }))
                            } else {
                                None
                            }
                        }
                        Expr::Undefined => {
                            // Default is explicitly undefined - no need to check/set
                            None
                        }
                        Expr::EnumMember { enum_name, member_name } => {
                            if abi_type == types::I64 {
                                if let Some(s) = resolve_enum_string(enum_name, member_name) {
                                    make_string_default(&mut builder, &mut self.module, &s)
                                } else {
                                    None
                                }
                            } else if abi_type == types::F64 {
                                resolve_enum_number(enum_name, member_name)
                                    .map(|n| builder.ins().f64const(n))
                            } else {
                                None
                            }
                        }
                        // PropertyGet on ExternFuncRef represents imported enum member access
                        // e.g., ChainName.ETHEREUM where ChainName is imported
                        Expr::PropertyGet { object, property } => {
                            if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                                if abi_type == types::I64 {
                                    if let Some(s) = resolve_enum_string(name, property) {
                                        make_string_default(&mut builder, &mut self.module, &s)
                                    } else {
                                        None
                                    }
                                } else if abi_type == types::F64 {
                                    resolve_enum_number(name, property)
                                        .map(|n| builder.ins().f64const(n))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None, // Complex defaults handled by intra-module call expansion
                    };

                    if let Some(default_val) = default_val_opt {
                        let param_val = builder.use_var(var);
                        let is_undefined = if abi_type == types::I64 {
                            let zero = builder.ins().iconst(types::I64, 0);
                            builder.ins().icmp(IntCC::Equal, param_val, zero)
                        } else {
                            // Compare against TAG_UNDEFINED (0x7FFC_0000_0000_0001) specifically.
                            // NaN check (fcmp Unordered) is too broad — it catches NaN-boxed booleans
                            // (TAG_TRUE = 0x7FFC_0000_0000_0004, TAG_FALSE = 0x7FFC_0000_0000_0003)
                            // which are valid parameter values, not missing args.
                            let raw_bits = builder.ins().bitcast(types::I64, MemFlags::new(), param_val);
                            let tag_undefined = builder.ins().iconst(types::I64, 0x7FFC_0000_0000_0001u64 as i64);
                            builder.ins().icmp(IntCC::Equal, raw_bits, tag_undefined)
                        };

                        let default_block = builder.create_block();
                        let continue_block = builder.create_block();
                        builder.ins().brif(is_undefined, default_block, &[], continue_block, &[]);

                        builder.switch_to_block(default_block);
                        builder.seal_block(default_block);
                        builder.def_var(var, default_val);
                        builder.ins().jump(continue_block, &[]);

                        builder.switch_to_block(continue_block);
                        builder.seal_block(continue_block);
                    }
                }
            }

            // For async functions, we need to handle returns specially.
            // Wrap the entire body in an implicit try/catch so that any throw
            // inside the async function rejects the returned Promise (matching JS semantics).
            if func.is_async {
                let promise_var_unwrapped = promise_var.unwrap();

                // === Implicit try/catch for async function body ===
                let try_body_block = builder.create_block();
                let implicit_catch_block = builder.create_block();

                // Push try frame
                let try_push_func = self.extern_funcs.get("js_try_push")
                    .ok_or_else(|| anyhow!("js_try_push not declared"))?;
                let try_push_ref = self.module.declare_func_in_func(*try_push_func, builder.func);
                let call = builder.ins().call(try_push_ref, &[]);
                let jmp_buf_ptr = builder.inst_results(call)[0];

                // Call setjmp directly (must be in this stack frame)
                let setjmp_func = self.extern_funcs.get("setjmp")
                    .ok_or_else(|| anyhow!("setjmp not declared"))?;
                let setjmp_ref = self.module.declare_func_in_func(*setjmp_func, builder.func);
                let call = builder.ins().call(setjmp_ref, &[jmp_buf_ptr]);
                let setjmp_result = builder.inst_results(call)[0];

                // Branch: 0 = normal execution, non-0 = exception caught
                let zero = builder.ins().iconst(types::I32, 0);
                let is_normal = builder.ins().icmp(IntCC::Equal, setjmp_result, zero);
                builder.ins().brif(is_normal, try_body_block, &[], implicit_catch_block, &[]);

                // === Try body: execute async function body ===
                builder.switch_to_block(try_body_block);
                builder.seal_block(try_body_block);

                TRY_CATCH_DEPTH.with(|d| d.set(d.get() + 1));
                for stmt in &func.body {
                    compile_async_stmt(&mut builder, &mut self.module, &self.func_ids, &self.closure_func_ids, &self.func_wrapper_ids, &self.extern_funcs, &self.async_func_ids, &self.closure_returning_funcs, &self.classes, &self.enums, &self.func_param_types, &self.func_union_params, &self.func_return_types, &self.func_hir_return_types, &self.func_rest_param_index, &self.imported_func_param_counts, &mut locals, &mut next_var, stmt, promise_var_unwrapped, &boxed_vars, false)
                        .map_err(|e| anyhow!("In async function '{}': {}", func.name, e))?;
                }
                TRY_CATCH_DEPTH.with(|d| d.set(d.get() - 1));

                // If no explicit return, resolve with undefined and return the promise
                let current_block = builder.current_block().unwrap();
                if !is_block_filled(&builder, current_block) {
                    let promise_ptr = builder.use_var(promise_var_unwrapped);

                    // Resolve with undefined
                    let resolve_func = self.extern_funcs.get("js_promise_resolve")
                        .ok_or_else(|| anyhow!("js_promise_resolve not declared"))?;
                    let resolve_ref = self.module.declare_func_in_func(*resolve_func, builder.func);
                    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
                    let undef_val = builder.ins().f64const(f64::from_bits(TAG_UNDEFINED));
                    builder.ins().call(resolve_ref, &[promise_ptr, undef_val]);

                    // Pop implicit try frame before returning
                    let try_end_func = self.extern_funcs.get("js_try_end")
                        .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                    let try_end_ref = self.module.declare_func_in_func(*try_end_func, builder.func);
                    builder.ins().call(try_end_ref, &[]);

                    builder.ins().return_(&[promise_ptr]);
                }

                // === Implicit catch: reject Promise with exception ===
                builder.switch_to_block(implicit_catch_block);
                builder.seal_block(implicit_catch_block);

                // Pop try frame
                {
                    let try_end_func = self.extern_funcs.get("js_try_end")
                        .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                    let try_end_ref = self.module.declare_func_in_func(*try_end_func, builder.func);
                    builder.ins().call(try_end_ref, &[]);
                }

                // Get the exception value
                let get_exc_func = self.extern_funcs.get("js_get_exception")
                    .ok_or_else(|| anyhow!("js_get_exception not declared"))?;
                let get_exc_ref = self.module.declare_func_in_func(*get_exc_func, builder.func);
                let call = builder.ins().call(get_exc_ref, &[]);
                let exc_val = builder.inst_results(call)[0];

                // Clear exception
                let clear_exc_func = self.extern_funcs.get("js_clear_exception")
                    .ok_or_else(|| anyhow!("js_clear_exception not declared"))?;
                let clear_exc_ref = self.module.declare_func_in_func(*clear_exc_func, builder.func);
                builder.ins().call(clear_exc_ref, &[]);

                // Reject promise with exception
                let promise_ptr = builder.use_var(promise_var_unwrapped);
                let reject_func = self.extern_funcs.get("js_promise_reject")
                    .ok_or_else(|| anyhow!("js_promise_reject not declared"))?;
                let reject_ref = self.module.declare_func_in_func(*reject_func, builder.func);
                builder.ins().call(reject_ref, &[promise_ptr, exc_val]);

                // Return the (now rejected) promise
                builder.ins().return_(&[promise_ptr]);
            } else {
                for stmt in &func.body {
                    compile_stmt(&mut builder, &mut self.module, &self.func_ids, &self.closure_func_ids, &self.func_wrapper_ids, &self.extern_funcs, &self.async_func_ids, &self.closure_returning_funcs, &self.classes, &self.enums, &self.func_param_types, &self.func_union_params, &self.func_return_types, &self.func_hir_return_types, &self.func_rest_param_index, &self.imported_func_param_counts, &mut locals, &mut next_var, stmt, None, None, &boxed_vars, None)
                        .map_err(|e| anyhow!("In function '{}': {}", func.name, e))?;
                }

                // If no explicit return, return 0 with the correct type
                let current_block = builder.current_block().unwrap();
                if !is_block_filled(&builder, current_block) {
                    let ret_type = builder.func.signature.returns.first().map(|p| p.value_type).unwrap_or(types::F64);
                    let zero = match ret_type {
                        types::I64 => builder.ins().iconst(types::I64, 0),
                        types::I32 => builder.ins().iconst(types::I32, 0),
                        _ => builder.ins().f64const(0.0),
                    };
                    builder.ins().return_(&[zero]);
                }
            }

            builder.finalize();
        }

        // Compile and define the function
        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            // Print detailed error info
            eprintln!("=== VERIFIER ERROR in function '{}' ===", func.name);
            eprintln!("Error: {}", e);
            eprintln!("Debug: {:?}", e);
            return Err(anyhow!("Error compiling function '{}': {}", func.name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    /// Compile an integer-only function with i64 specialization for better performance.
    /// Creates a `{name}_i64` inner function using integer instructions (icmp/iadd/isub)
    /// and makes the original function a thin wrapper that converts f64 <-> i64.
    pub(crate) fn compile_integer_specialized_function(&mut self, func: &Function) -> Result<()> {
        let orig_func_id = *self.func_ids.get(&func.id)
            .ok_or_else(|| anyhow!("Function not declared: {}", func.name))?;

        // Step 1: Declare the i64 specialized function
        let i64_name = format!("{}_i64", func.name);
        let mut i64_sig = self.module.make_signature();
        for _ in &func.params {
            i64_sig.params.push(AbiParam::new(types::I64));
        }
        i64_sig.returns.push(AbiParam::new(types::I64));
        let i64_func_id = self.module.declare_function(&i64_name, Linkage::Local, &i64_sig)?;

        // Step 2: Compile the i64 specialized function body
        self.ctx.func.signature = i64_sig.clone();
        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut i64_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut i64_func_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Create i64 variables for parameters
            let mut param_vars: HashMap<LocalId, Variable> = HashMap::new();
            for (i, param) in func.params.iter().enumerate() {
                let var = Variable::new(i);
                builder.declare_var(var, types::I64);
                let val = builder.block_params(entry_block)[i];
                builder.def_var(var, val);
                param_vars.insert(param.id, var);
            }

            // Load module-level variables from their global slots
            // This ensures functions can access module-level constants and variables
            for (local_id, data_id) in &self.module_var_data_ids {
                if param_vars.contains_key(local_id) {
                    continue;
                }
                if let Some(info) = self.module_level_locals.get(local_id) {
                    if let Some(cv) = info.const_value {
                        // For compile-time constants, inline the value directly as i64
                        let var = Variable::new(func.params.len() + param_vars.len());
                        builder.declare_var(var, types::I64);
                        let val = builder.ins().iconst(types::I64, cv as i64);
                        builder.def_var(var, val);
                        param_vars.insert(*local_id, var);
                    } else if info.is_pointer || info.is_string || info.is_array {
                        // For pointer/array/string module vars, load raw I64 from global slot
                        let var = Variable::new(func.params.len() + param_vars.len());
                        builder.declare_var(var, types::I64);
                        let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                        let ptr = builder.ins().global_value(types::I64, global_val);
                        let val = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
                        builder.def_var(var, val);
                        param_vars.insert(*local_id, var);
                    } else {
                        // For non-const number module vars, load from global slot and convert f64 -> i64
                        let var = Variable::new(func.params.len() + param_vars.len());
                        builder.declare_var(var, types::I64);
                        let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                        let ptr = builder.ins().global_value(types::I64, global_val);
                        let f64_val = builder.ins().load(types::F64, MemFlags::new(), ptr, 0);
                        let i64_val = builder.ins().fcvt_to_sint_sat(types::I64, f64_val);
                        builder.def_var(var, i64_val);
                        param_vars.insert(*local_id, var);
                    }
                }
            }

            let i64_func_ref = self.module.declare_func_in_func(i64_func_id, builder.func);
            let mut next_var = func.params.len() + param_vars.len();

            // Compile function body with integer operations
            Self::compile_i64_body(&mut builder, &func.body, &mut param_vars, &mut next_var, i64_func_ref, func.id);

            // Fallback return 0 if body doesn't explicitly return
            let current_block = builder.current_block().unwrap();
            if !is_block_filled(&builder, current_block) {
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().return_(&[zero]);
            }

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(i64_func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in i64-specialized function '{}' ===", i64_name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling i64-specialized function '{}': {}", i64_name, e));
        }
        self.module.clear_context(&mut self.ctx);

        // Step 3: Compile the original function as a thin wrapper: f64 -> i64 -> call -> i64 -> f64
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();
        for _ in &func.params {
            self.ctx.func.signature.params.push(AbiParam::new(types::F64));
        }
        self.ctx.func.signature.returns.push(AbiParam::new(types::F64));

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut wrapper_i64_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut wrapper_i64_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let i64_func_ref = self.module.declare_func_in_func(i64_func_id, builder.func);

            // Convert f64 params to i64 via bitcast (NOT fcvt_to_sint which destroys NaN-boxed values)
            // NaN-boxed strings/pointers/bigints are IEEE NaN values — fcvt_to_sint_sat converts
            // NaN to 0, destroying the pointer. Bitcast preserves the raw bits.
            let mut i64_args = Vec::new();
            for i in 0..func.params.len() {
                let f64_val = builder.block_params(entry_block)[i];
                let i64_val = builder.ins().bitcast(types::I64, cranelift_codegen::ir::MemFlags::new(), f64_val);
                i64_args.push(i64_val);
            }

            // Call the i64 specialized function
            let call = builder.ins().call(i64_func_ref, &i64_args);
            let i64_result = builder.inst_results(call)[0];

            // Convert i64 result back to f64 via bitcast (preserve NaN-boxing)
            let f64_result = builder.ins().bitcast(types::F64, cranelift_codegen::ir::MemFlags::new(), i64_result);
            builder.ins().return_(&[f64_result]);

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(orig_func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in wrapper for '{}' ===", func.name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling wrapper for '{}': {}", func.name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    /// Compile a statement list for the i64-specialized function body
    pub(crate) fn compile_i64_body(
        builder: &mut FunctionBuilder,
        stmts: &[Stmt],
        vars: &mut HashMap<LocalId, Variable>,
        next_var: &mut usize,
        self_func_ref: cranelift_codegen::ir::FuncRef,
        func_hir_id: u32,
    ) {
        for stmt in stmts {
            Self::compile_i64_stmt(builder, stmt, vars, next_var, self_func_ref, func_hir_id);
            // Stop if block is terminated (e.g. by a return)
            if let Some(block) = builder.current_block() {
                if is_block_filled(builder, block) {
                    break;
                }
            }
        }
    }

    /// Compile a single statement in the i64-specialized function
    pub(crate) fn compile_i64_stmt(
        builder: &mut FunctionBuilder,
        stmt: &Stmt,
        vars: &mut HashMap<LocalId, Variable>,
        next_var: &mut usize,
        self_func_ref: cranelift_codegen::ir::FuncRef,
        func_hir_id: u32,
    ) {
        match stmt {
            Stmt::Return(Some(expr)) => {
                let val = Self::compile_i64_expr(builder, expr, vars, next_var, self_func_ref, func_hir_id);
                builder.ins().return_(&[val]);
            }
            Stmt::Return(None) => {
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().return_(&[zero]);
            }
            Stmt::Expr(expr) => {
                Self::compile_i64_expr(builder, expr, vars, next_var, self_func_ref, func_hir_id);
            }
            Stmt::Let { id, init, .. } => {
                let var = Variable::new(*next_var);
                *next_var += 1;
                builder.declare_var(var, types::I64);
                let val = if let Some(init_expr) = init {
                    Self::compile_i64_expr(builder, init_expr, vars, next_var, self_func_ref, func_hir_id)
                } else {
                    builder.ins().iconst(types::I64, 0)
                };
                builder.def_var(var, val);
                vars.insert(*id, var);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                let cond_val = Self::compile_i64_expr(builder, condition, vars, next_var, self_func_ref, func_hir_id);
                let then_block = builder.create_block();
                let else_block = builder.create_block();
                let merge_block = builder.create_block();

                builder.ins().brif(cond_val, then_block, &[], else_block, &[]);

                // Then branch
                builder.switch_to_block(then_block);
                builder.seal_block(then_block);
                Self::compile_i64_body(builder, then_branch, vars, next_var, self_func_ref, func_hir_id);
                if let Some(block) = builder.current_block() {
                    if !is_block_filled(builder, block) {
                        builder.ins().jump(merge_block, &[]);
                    }
                }

                // Else branch
                builder.switch_to_block(else_block);
                builder.seal_block(else_block);
                if let Some(else_stmts) = else_branch {
                    Self::compile_i64_body(builder, else_stmts, vars, next_var, self_func_ref, func_hir_id);
                }
                if let Some(block) = builder.current_block() {
                    if !is_block_filled(builder, block) {
                        builder.ins().jump(merge_block, &[]);
                    }
                }

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
            }
            _ => {} // Other statements not supported in integer-only functions
        }
    }

    /// Compile an expression in the i64-specialized function, returning an i64 Value
    pub(crate) fn compile_i64_expr(
        builder: &mut FunctionBuilder,
        expr: &Expr,
        vars: &mut HashMap<LocalId, Variable>,
        next_var: &mut usize,
        self_func_ref: cranelift_codegen::ir::FuncRef,
        func_hir_id: u32,
    ) -> Value {
        match expr {
            Expr::Integer(n) => {
                builder.ins().iconst(types::I64, *n as i64)
            }
            Expr::Number(f) => {
                builder.ins().iconst(types::I64, *f as i64)
            }
            Expr::LocalGet(id) => {
                if let Some(var) = vars.get(id) {
                    builder.use_var(*var)
                } else {
                    builder.ins().iconst(types::I64, 0)
                }
            }
            Expr::Binary { op, left, right } => {
                let lhs = Self::compile_i64_expr(builder, left, vars, next_var, self_func_ref, func_hir_id);
                let rhs = Self::compile_i64_expr(builder, right, vars, next_var, self_func_ref, func_hir_id);
                match op {
                    BinaryOp::Add => builder.ins().iadd(lhs, rhs),
                    BinaryOp::Sub => builder.ins().isub(lhs, rhs),
                    BinaryOp::Mul => builder.ins().imul(lhs, rhs),
                    BinaryOp::Mod => builder.ins().srem(lhs, rhs),
                    BinaryOp::BitAnd => builder.ins().band(lhs, rhs),
                    BinaryOp::BitOr => builder.ins().bor(lhs, rhs),
                    BinaryOp::BitXor => builder.ins().bxor(lhs, rhs),
                    BinaryOp::Shl => builder.ins().ishl(lhs, rhs),
                    BinaryOp::Shr => builder.ins().sshr(lhs, rhs),
                    BinaryOp::UShr => builder.ins().ushr(lhs, rhs),
                    _ => builder.ins().iconst(types::I64, 0), // Unsupported
                }
            }
            Expr::Compare { op, left, right } => {
                let lhs = Self::compile_i64_expr(builder, left, vars, next_var, self_func_ref, func_hir_id);
                let rhs = Self::compile_i64_expr(builder, right, vars, next_var, self_func_ref, func_hir_id);
                let cc = match op {
                    CompareOp::Lt => IntCC::SignedLessThan,
                    CompareOp::Le => IntCC::SignedLessThanOrEqual,
                    CompareOp::Gt => IntCC::SignedGreaterThan,
                    CompareOp::Ge => IntCC::SignedGreaterThanOrEqual,
                    CompareOp::Eq => IntCC::Equal,
                    CompareOp::Ne => IntCC::NotEqual,
                };
                builder.ins().icmp(cc, lhs, rhs)
            }
            Expr::Unary { op, operand } => {
                let val = Self::compile_i64_expr(builder, operand, vars, next_var, self_func_ref, func_hir_id);
                match op {
                    UnaryOp::Neg => builder.ins().ineg(val),
                    UnaryOp::BitNot => builder.ins().bnot(val),
                    _ => val,
                }
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                let cond = Self::compile_i64_expr(builder, condition, vars, next_var, self_func_ref, func_hir_id);
                let then_val = Self::compile_i64_expr(builder, then_expr, vars, next_var, self_func_ref, func_hir_id);
                let else_val = Self::compile_i64_expr(builder, else_expr, vars, next_var, self_func_ref, func_hir_id);
                builder.ins().select(cond, then_val, else_val)
            }
            Expr::Call { callee, args, .. } => {
                // Only self-recursive calls are allowed
                if let Expr::FuncRef(id) = callee.as_ref() {
                    if *id == func_hir_id {
                        let arg_vals: Vec<Value> = args.iter()
                            .map(|a| Self::compile_i64_expr(builder, a, vars, next_var, self_func_ref, func_hir_id))
                            .collect();
                        let call = builder.ins().call(self_func_ref, &arg_vals);
                        return builder.inst_results(call)[0];
                    }
                }
                builder.ins().iconst(types::I64, 0)
            }
            _ => builder.ins().iconst(types::I64, 0), // Unsupported expression
        }
    }

    /// Check if a function body returns a closure (by scanning return statements)
    pub(crate) fn function_returns_closure(&self, body: &[Stmt]) -> bool {
        for stmt in body {
            if self.stmt_returns_closure(stmt) {
                return true;
            }
        }
        false
    }

    /// Check if a statement contains a return that returns a closure
    pub(crate) fn stmt_returns_closure(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Return(Some(expr)) => self.expr_is_closure(expr),
            Stmt::If { then_branch, else_branch, .. } => {
                for s in then_branch {
                    if self.stmt_returns_closure(s) {
                        return true;
                    }
                }
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        if self.stmt_returns_closure(s) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Check if an expression is a closure
    pub(crate) fn expr_is_closure(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Closure { .. })
    }

}
