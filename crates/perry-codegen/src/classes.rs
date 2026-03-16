//! Class compilation for the codegen module.
//!
//! Contains methods for processing, declaring, and compiling class methods,
//! constructors, getters, setters, static methods, and static fields.

use anyhow::{anyhow, Result};
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, Init, Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::BTreeMap;

use perry_hir::{
    CallArg, Class, ClassField, CompareOp, Decorator, Expr, Function, Stmt,
};
use perry_types::LocalId;
use cranelift_codegen::ir::{StackSlotData, StackSlotKind};

use crate::stmt::compile_stmt_with_this;
use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext, LoopContext};
use crate::util::*;
use crate::stmt::compile_stmt;
use crate::expr::compile_expr;

impl crate::codegen::Compiler {
    pub(crate) fn process_class(&mut self, class: &Class, all_classes: &[Class]) -> Result<()> {

        // Find parent class name if this class extends another
        // First try to resolve by ClassId, then fall back to extends_name for imported classes
        let parent_class = class.extends.and_then(|parent_id| {
            all_classes.iter()
                .find(|c| c.id == parent_id)
                .map(|c| c.name.clone())
        }).or_else(|| class.extends_name.clone());

        // Native parent class (e.g., EventEmitter from 'events')
        let native_parent = class.native_extends.clone();

        // If extending a native class, add a hidden field to store the native handle
        let native_handle_field_count = if native_parent.is_some() { 1 } else { 0 };
        let own_field_count = class.fields.len() as u32 + native_handle_field_count;

        // Start with own fields only - inheritance will be resolved later
        // If there's a native parent, field 0 is reserved for the native handle
        let mut field_indices = BTreeMap::new();
        let mut field_types = BTreeMap::new();
        if native_parent.is_some() {
            field_indices.insert("__native_handle__".to_string(), 0);
        }
        for (i, field) in class.fields.iter().enumerate() {
            field_indices.insert(field.name.clone(), (i as u32) + native_handle_field_count);
            field_types.insert(field.name.clone(), field.ty.clone());
        }

        // Collect method return types for type-aware console.log handling
        let mut method_return_types = BTreeMap::new();
        for method in &class.methods {
            method_return_types.insert(method.name.clone(), method.return_type.clone());
        }

        // Collect static method return types for singleton pattern (getInstance() etc.)
        let mut static_method_return_types = BTreeMap::new();
        for method in &class.static_methods {
            static_method_return_types.insert(method.name.clone(), method.return_type.clone());
        }

        // Extract type parameter names
        let type_params: Vec<String> = class.type_params.iter().map(|tp| tp.name.clone()).collect();

        // Collect field default initializer expressions
        let mut field_inits = BTreeMap::new();
        for field in &class.fields {
            if let Some(ref init) = field.init {
                field_inits.insert(field.name.clone(), init.clone());
            }
        }

        eprintln!("[CLASS_REG] class='{}' fields={} field_names={:?} parent={:?}",
            class.name, own_field_count, field_indices.keys().collect::<Vec<_>>(), parent_class);
        self.classes.insert(class.name.clone(), ClassMeta {
            id: class.id,
            parent_class,
            native_parent,
            own_field_count,
            field_count: own_field_count,
            field_indices,
            field_types,
            constructor_id: None,
            method_ids: BTreeMap::new(),
            method_param_counts: BTreeMap::new(),
            getter_ids: BTreeMap::new(),
            setter_ids: BTreeMap::new(),
            static_method_ids: BTreeMap::new(),
            static_field_ids: BTreeMap::new(),
            method_return_types,
            static_method_return_types,
            type_params,
            field_inits,
        });

        Ok(())
    }

    pub(crate) fn resolve_class_inheritance(&mut self) {
        // Get list of class names to process — MUST be sorted for deterministic compilation
        let mut class_names: Vec<String> = self.classes.keys().cloned().collect();
        class_names.sort();

        for class_name in &class_names {
            self.resolve_class_fields(class_name);
        }
    }

    pub(crate) fn resolve_class_fields(&mut self, class_name: &str) -> u32 {
        // Get a clone of the class meta to avoid borrow issues
        let class_meta = match self.classes.get(class_name) {
            Some(meta) => meta.clone(),
            None => return 0,
        };

        // If already resolved (no parent, or field_count > own_field_count meaning parent fields
        // were already merged), return early to avoid double-resolution corruption
        if class_meta.parent_class.is_none() {
            return class_meta.field_count;
        }
        if class_meta.field_count > class_meta.own_field_count {
            // Already resolved by a recursive call — don't re-shift indices
            return class_meta.field_count;
        }

        let parent_name = class_meta.parent_class.clone().unwrap();

        // Recursively resolve parent first
        let parent_field_count = self.resolve_class_fields(&parent_name);

        // Get parent's field indices
        let parent_field_indices = self.classes.get(&parent_name)
            .map(|m| m.field_indices.clone())
            .unwrap_or_default();

        // Update current class: shift own field indices by parent's field count
        if let Some(meta) = self.classes.get_mut(class_name) {
            // Merge parent fields (they come first)
            let mut new_field_indices = parent_field_indices;

            // Add own fields with offset
            for (field_name, idx) in &class_meta.field_indices {
                new_field_indices.insert(field_name.clone(), idx + parent_field_count);
            }

            meta.field_indices = new_field_indices;
            meta.field_count = parent_field_count + class_meta.own_field_count;
        }

        self.classes.get(class_name).map(|m| m.field_count).unwrap_or(0)
    }

    /// Resolve method inheritance - copy parent methods to child classes
    /// This must be called AFTER all methods have been declared
    pub(crate) fn resolve_method_inheritance(&mut self) {
        // MUST be sorted for deterministic compilation
        let mut class_names: Vec<String> = self.classes.keys().cloned().collect();
        class_names.sort();

        for class_name in &class_names {
            self.resolve_methods_for_class(class_name);
        }
    }

    pub(crate) fn resolve_methods_for_class(&mut self, class_name: &str) {
        let class_meta = match self.classes.get(class_name) {
            Some(meta) => meta.clone(),
            None => return,
        };

        // If no parent, nothing to inherit
        let parent_name = match &class_meta.parent_class {
            Some(name) => name.clone(),
            None => return,
        };

        // Recursively resolve parent first
        self.resolve_methods_for_class(&parent_name);

        // Get parent's method IDs
        let parent_method_ids = self.classes.get(&parent_name)
            .map(|m| m.method_ids.clone())
            .unwrap_or_default();

        // Inherit parent methods (child methods override parent)
        if let Some(meta) = self.classes.get_mut(class_name) {
            for (method_name, method_id) in parent_method_ids {
                if !meta.method_ids.contains_key(&method_name) {
                    meta.method_ids.insert(method_name, method_id);
                }
            }
        }
    }

    pub(crate) fn process_enum(&mut self, en: &perry_hir::Enum) -> Result<()> {
        for member in &en.members {
            let value = match &member.value {
                perry_hir::EnumValue::Number(n) => EnumMemberValue::Number(*n),
                perry_hir::EnumValue::String(s) => EnumMemberValue::String(s.clone()),
            };
            self.enums.insert((en.name.clone(), member.name.clone()), value);
        }
        Ok(())
    }

    /// Register an imported enum from another module.
    /// This allows EnumMember expressions to be resolved at codegen time.
    pub fn register_imported_enum(&mut self, enum_name: &str, members: &[(String, perry_hir::EnumValue)]) {
        for (member_name, value) in members {
            let v = match value {
                perry_hir::EnumValue::Number(n) => EnumMemberValue::Number(*n),
                perry_hir::EnumValue::String(s) => EnumMemberValue::String(s.clone()),
            };
            self.enums.insert((enum_name.to_string(), member_name.clone()), v);
        }
    }

    pub(crate) fn declare_class_methods(&mut self, class: &Class) -> Result<()> {
        // Export methods for exported classes so other modules can call them
        let linkage = if class.is_exported { Linkage::Export } else { Linkage::Local };

        for method in &class.methods {
            let mut sig = self.module.make_signature();

            // Methods take 'this' as first parameter (i64 pointer)
            sig.params.push(AbiParam::new(types::I64));

            // Then regular parameters (all f64 for now)
            for _ in &method.params {
                sig.params.push(AbiParam::new(types::F64));
            }

            // Return type (f64 for now)
            sig.returns.push(AbiParam::new(types::F64));

            let func_name = format!("{}__{}_{}",self.module_symbol_prefix, class.name, method.name);
            let func_id = self.module.declare_function(&func_name, linkage, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.method_ids.insert(method.name.clone(), func_id);
                meta.method_param_counts.insert(method.name.clone(), method.params.len());
            }
        }
        Ok(())
    }

    pub(crate) fn compile_class_method(&mut self, class: &Class, method: &Function) -> Result<()> {
        let func_name = format!("{}__{}_{}",self.module_symbol_prefix, class.name, method.name);
        let func_id = self.classes.get(&class.name)
            .and_then(|m| m.method_ids.get(&method.name).copied())
            .ok_or_else(|| anyhow!("Method not declared: {}::{}", class.name, method.name))?;

        // Set up the function signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // 'this' as first parameter
        self.ctx.func.signature.params.push(AbiParam::new(types::I64));

        for _ in &method.params {
            self.ctx.func.signature.params.push(AbiParam::new(types::F64));
        }
        self.ctx.func.signature.returns.push(AbiParam::new(types::F64));

        let class_meta = self.classes.get(&class.name).cloned()
            .ok_or_else(|| anyhow!("Class metadata not found: {}", class.name))?;


        // Collect mutable captures before FunctionBuilder block
        let boxed_vars = self.collect_mutable_captures_from_stmts(&method.body);

        // Prepare decorator data before creating the FunctionBuilder
        let decorator_data = self.prepare_decorators(&method.decorators, &method.name)?;
        let print_func_id = self.extern_funcs.get("js_string_print").copied();

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts across functions
        let mut method_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut method_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // 'this' is the first parameter (i64 pointer)
            let this_var = Variable::new(0);
            builder.declare_var(this_var, types::I64);
            let this_val = builder.block_params(entry_block)[0];
            builder.def_var(this_var, this_val);

            // Create variables for other parameters
            let mut locals: BTreeMap<LocalId, LocalInfo> = BTreeMap::new();
            let mut next_var = 1usize;
            for (i, param) in method.params.iter().enumerate() {
                let var = Variable::new(next_var);
                next_var += 1;
                // Check parameter types for correct handling of string methods, array methods, etc.
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                let is_string = matches!(param.ty, perry_types::Type::String);
                let is_array = matches!(param.ty, perry_types::Type::Array(_));
                let is_map = matches!(param.ty, perry_types::Type::Named(ref n) if n == "Map");
                let is_set = matches!(param.ty, perry_types::Type::Named(ref n) if n == "Set");
                let is_pointer = is_closure || is_string || is_array || is_map || is_set ||
                    matches!(param.ty, perry_types::Type::Object(_) | perry_types::Type::Named(_));
                let is_union_type = matches!(param.ty, perry_types::Type::Any | perry_types::Type::Unknown);
                let is_pointer = is_pointer && !is_union_type;
                let var_type = if is_pointer { types::I64 } else { types::F64 };
                builder.declare_var(var, var_type);
                let val = builder.block_params(entry_block)[i + 1]; // +1 to skip 'this'
                // Parameters arrive as f64, extract pointer if needed
                let final_val = if is_pointer {
                    let get_ptr_func = self.extern_funcs.get("js_nanbox_get_pointer")
                        .expect("js_nanbox_get_pointer not declared");
                    let get_ptr_ref = self.module.declare_func_in_func(*get_ptr_func, builder.func);
                    let call = builder.ins().call(get_ptr_ref, &[val]);
                    builder.inst_results(call)[0]
                } else {
                    val
                };
                builder.def_var(var, final_val);
                locals.insert(param.id, LocalInfo {
                    var,
                    name: Some(param.name.clone()),
                    class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                    type_args: Vec::new(),
                    is_pointer,
                    is_array,
                    is_string,
                    is_bigint: matches!(param.ty, perry_types::Type::BigInt),
                    is_closure, closure_func_id: None,
                    is_boxed: false,
                    is_map, is_set, is_buffer: false, is_event_emitter: false, is_union: is_union_type,
                    is_mixed_array: false,
                    is_integer: false,
                    is_integer_array: false,
                    is_i32: false,
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                });
            }

            // Load module-level variables from their global slots
            // These are variables defined at module scope that the method may reference
            for (local_id, data_id) in &self.module_var_data_ids {
                // Get the type info from module_level_locals (populated during compile_init)
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    // Variable is stored as i64 only if is_pointer && !is_union
                    let vt = if info.is_pointer && !info.is_union { types::I64 } else { types::F64 };
                    (vt, info.clone())
                } else {
                    // Fallback to f64 if type info not available
                    (types::F64, LocalInfo {
                        var: Variable::new(0), // Will be overwritten
                        name: None,
                        class_name: None,
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
                        is_i32: false,
                        i32_shadow: None,
                        bounded_by_array: None,
                        bounded_by_constant: None,
                        scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    })
                };

                // Skip if this LocalId is already a method parameter
                if locals.contains_key(local_id) {
                    continue;
                }

                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, var_type);

                // Load the value from the global data slot
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);

                // Insert into locals so LocalGet can find it
                let mut info = local_info_template;
                info.var = var;
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // Compile method body with 'this' context
            let this_ctx = ThisContext {
                this_var,
                class_meta: class_meta.clone(),
            };

            // Handle decorators: inject decorator behavior before method body
            for data_id in &decorator_data {
                let local_data = self.module.declare_data_in_func(*data_id, builder.func);
                let str_ptr = builder.ins().symbol_value(types::I64, local_data);

                if let Some(print_id) = print_func_id {
                    let print_func_ref = self.module.declare_func_in_func(print_id, builder.func);
                    builder.ins().call(print_func_ref, &[str_ptr]);
                }
            }

            for stmt in &method.body {
                compile_stmt(
                    &mut builder,
                    &mut self.module,
                    &self.func_ids,
                    &self.closure_func_ids,
                    &self.func_wrapper_ids,
                    &self.extern_funcs,
                    &self.async_func_ids,
                    &self.closure_returning_funcs,
                    &self.classes,
                    &self.enums,
                    &self.func_param_types,
                    &self.func_union_params,
                    &self.func_return_types,
                    &self.func_hir_return_types,
                    &self.func_rest_param_index,
                    &self.imported_func_param_counts,
                    &mut locals,
                    &mut next_var,
                    stmt,
                    Some(&this_ctx),
                    None,
                    &boxed_vars,
                    None,
                )?;
            }

            // If no explicit return, return 0 with correct type
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

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in instance method '{}' ===", method.name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling instance method '{}': {}", method.name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    /// Prepare decorator data (string constants, etc.) before code generation
    /// Returns a list of data IDs for strings to print
    pub(crate) fn prepare_decorators(
        &mut self,
        decorators: &[Decorator],
        method_name: &str,
    ) -> Result<Vec<cranelift_module::DataId>> {
        let mut data_ids = Vec::new();

        for decorator in decorators {
            match decorator.name.as_str() {
                "log" => {
                    // @log decorator: print "Calling <method_name>" before method execution
                    let msg = format!("Calling {}", method_name);

                    // Create or get the string data for the message
                    let data_id = if let Some(&existing) = self.string_data.get(&msg) {
                        existing
                    } else {
                        let mut data_desc = DataDescription::new();
                        let msg_bytes = msg.as_bytes();

                        // Build the string structure: [len: u32, cap: u32, data...]
                        // StringHeader is only 8 bytes (length + capacity), then data follows immediately
                        let total_len = 8 + msg_bytes.len();
                        let mut buffer = vec![0u8; total_len];

                        // Length (first 4 bytes)
                        let len_bytes = (msg_bytes.len() as u32).to_le_bytes();
                        buffer[0..4].copy_from_slice(&len_bytes);

                        // Capacity (next 4 bytes, same as length for static strings)
                        buffer[4..8].copy_from_slice(&len_bytes);

                        // String data (rest of the buffer, immediately after header)
                        buffer[8..].copy_from_slice(msg_bytes);

                        data_desc.define(buffer.into_boxed_slice());

                        let data_name = format!("decorator_str_{}", self.string_data.len());
                        let new_data_id = self.module.declare_data(&data_name, Linkage::Local, false, false)?;
                        self.module.define_data(new_data_id, &data_desc)?;
                        self.string_data.insert(msg.clone(), new_data_id);
                        new_data_id
                    };

                    data_ids.push(data_id);
                }
                _ => {
                    // Unknown decorator - ignore for now
                    // In a full implementation, we would call the decorator function
                }
            }
        }
        Ok(data_ids)
    }

    pub(crate) fn declare_class_getters(&mut self, class: &Class) -> Result<()> {
        for (prop_name, getter) in &class.getters {
            let mut sig = self.module.make_signature();

            // Getters take 'this' as first parameter (i64 pointer)
            sig.params.push(AbiParam::new(types::I64));

            // No other parameters for getters

            // Return type (f64 for now)
            sig.returns.push(AbiParam::new(types::F64));

            let func_name = format!("{}__{}__get_{}", self.module_symbol_prefix, class.name, prop_name);
            let func_id = self.module.declare_function(&func_name, Linkage::Local, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.getter_ids.insert(prop_name.clone(), func_id);
            }
        }
        Ok(())
    }

    pub(crate) fn declare_class_setters(&mut self, class: &Class) -> Result<()> {
        for (prop_name, setter) in &class.setters {
            let mut sig = self.module.make_signature();

            // Setters take 'this' as first parameter (i64 pointer)
            sig.params.push(AbiParam::new(types::I64));

            // Then one value parameter
            for _ in &setter.params {
                sig.params.push(AbiParam::new(types::F64));
            }

            // Return type void (f64 for consistency)
            sig.returns.push(AbiParam::new(types::F64));

            let func_name = format!("{}__{}__set_{}", self.module_symbol_prefix, class.name, prop_name);
            let func_id = self.module.declare_function(&func_name, Linkage::Local, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.setter_ids.insert(prop_name.clone(), func_id);
            }
        }
        Ok(())
    }

    pub(crate) fn compile_class_getter(&mut self, class: &Class, prop_name: &str, getter: &Function) -> Result<()> {
        let func_id = self.classes.get(&class.name)
            .and_then(|m| m.getter_ids.get(prop_name).copied())
            .ok_or_else(|| anyhow!("Getter not declared: {}::get_{}", class.name, prop_name))?;

        // Set up the function signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // 'this' as first parameter
        self.ctx.func.signature.params.push(AbiParam::new(types::I64));

        // No other parameters for getters
        self.ctx.func.signature.returns.push(AbiParam::new(types::F64));

        let class_meta = self.classes.get(&class.name).cloned()
            .ok_or_else(|| anyhow!("Class metadata not found: {}", class.name))?;

        // Collect mutable captures before FunctionBuilder block
        let boxed_vars = self.collect_mutable_captures_from_stmts(&getter.body);

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut getter_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut getter_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // 'this' is the first parameter (i64 pointer)
            let this_var = Variable::new(0);
            builder.declare_var(this_var, types::I64);
            let this_val = builder.block_params(entry_block)[0];
            builder.def_var(this_var, this_val);

            // No other parameters for getters
            let mut locals: BTreeMap<LocalId, LocalInfo> = BTreeMap::new();
            let mut next_var = 1usize;

            // Load module-level variables from their global slots
            for (local_id, data_id) in &self.module_var_data_ids {
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    // Variable is stored as i64 only if is_pointer && !is_union
                    let vt = if info.is_pointer && !info.is_union { types::I64 } else { types::F64 };
                    (vt, info.clone())
                } else {
                    (types::F64, LocalInfo {
                        var: Variable::new(0),
                        name: None,
                        class_name: None,
                        type_args: Vec::new(),
                        is_pointer: false, is_array: false, is_string: false, is_bigint: false,
                        is_closure: false, closure_func_id: None, is_boxed: false, is_map: false, is_set: false,
                        is_buffer: false, is_event_emitter: false, is_union: false, is_mixed_array: false, is_integer: false,
                        is_integer_array: false, is_i32: false, i32_shadow: None,
                        bounded_by_array: None, bounded_by_constant: None, scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    })
                };
                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, var_type);
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);
                let mut info = local_info_template;
                info.var = var;
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // Compile getter body with 'this' context
            let this_ctx = ThisContext {
                this_var,
                class_meta: class_meta.clone(),
            };

            for stmt in &getter.body {
                compile_stmt(
                    &mut builder,
                    &mut self.module,
                    &self.func_ids,
                    &self.closure_func_ids,
                    &self.func_wrapper_ids,
                    &self.extern_funcs,
                    &self.async_func_ids,
                    &self.closure_returning_funcs,
                    &self.classes,
                    &self.enums,
                    &self.func_param_types,
                    &self.func_union_params,
                    &self.func_return_types,
                    &self.func_hir_return_types,
                    &self.func_rest_param_index,
                    &self.imported_func_param_counts,
                    &mut locals,
                    &mut next_var,
                    stmt,
                    Some(&this_ctx),
                    None,
                    &boxed_vars,
                    None,
                )?;
            }

            // If no explicit return, return 0 with correct type
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

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in getter '{}::get_{}' ===", class.name, prop_name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling getter '{}::get_{}': {}", class.name, prop_name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    pub(crate) fn compile_class_setter(&mut self, class: &Class, prop_name: &str, setter: &Function) -> Result<()> {
        let func_id = self.classes.get(&class.name)
            .and_then(|m| m.setter_ids.get(prop_name).copied())
            .ok_or_else(|| anyhow!("Setter not declared: {}::set_{}", class.name, prop_name))?;

        // Set up the function signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // 'this' as first parameter
        self.ctx.func.signature.params.push(AbiParam::new(types::I64));

        // Value parameter
        for _ in &setter.params {
            self.ctx.func.signature.params.push(AbiParam::new(types::F64));
        }
        self.ctx.func.signature.returns.push(AbiParam::new(types::F64));

        let class_meta = self.classes.get(&class.name).cloned()
            .ok_or_else(|| anyhow!("Class metadata not found: {}", class.name))?;

        // Collect mutable captures before FunctionBuilder block
        let boxed_vars = self.collect_mutable_captures_from_stmts(&setter.body);

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut setter_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut setter_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // 'this' is the first parameter (i64 pointer)
            let this_var = Variable::new(0);
            builder.declare_var(this_var, types::I64);
            let this_val = builder.block_params(entry_block)[0];
            builder.def_var(this_var, this_val);

            // Create variables for value parameters
            let mut locals: BTreeMap<LocalId, LocalInfo> = BTreeMap::new();
            let mut next_var = 1usize;
            for (i, param) in setter.params.iter().enumerate() {
                let var = Variable::new(next_var);
                next_var += 1;
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                let is_string = matches!(param.ty, perry_types::Type::String);
                let is_array = matches!(param.ty, perry_types::Type::Array(_));
                let is_map = matches!(param.ty, perry_types::Type::Named(ref n) if n == "Map");
                let is_set = matches!(param.ty, perry_types::Type::Named(ref n) if n == "Set");
                let is_pointer = is_closure || is_string || is_array || is_map || is_set ||
                    matches!(param.ty, perry_types::Type::Object(_) | perry_types::Type::Named(_));
                let is_union_type = matches!(param.ty, perry_types::Type::Any | perry_types::Type::Unknown);
                let is_pointer = is_pointer && !is_union_type;
                let var_type = if is_pointer { types::I64 } else { types::F64 };
                builder.declare_var(var, var_type);
                let val = builder.block_params(entry_block)[i + 1]; // +1 to skip 'this'
                let final_val = if is_pointer {
                    let get_ptr_func = self.extern_funcs.get("js_nanbox_get_pointer")
                        .expect("js_nanbox_get_pointer not declared");
                    let get_ptr_ref = self.module.declare_func_in_func(*get_ptr_func, builder.func);
                    let call = builder.ins().call(get_ptr_ref, &[val]);
                    builder.inst_results(call)[0]
                } else {
                    val
                };
                builder.def_var(var, final_val);
                locals.insert(param.id, LocalInfo {
                    var,
                    name: Some(param.name.clone()),
                    class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                    type_args: Vec::new(),
                    is_pointer,
                    is_array,
                    is_string,
                    is_bigint: matches!(param.ty, perry_types::Type::BigInt),
                    is_closure, closure_func_id: None,
                    is_boxed: false,
                    is_map, is_set, is_buffer: false, is_event_emitter: false, is_union: is_union_type,
                    is_mixed_array: false,
                    is_integer: false,
                    is_integer_array: false,
                    is_i32: false,
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                });
            }

            // Load module-level variables from their global slots
            for (local_id, data_id) in &self.module_var_data_ids {
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    // Variable is stored as i64 only if is_pointer && !is_union
                    let vt = if info.is_pointer && !info.is_union { types::I64 } else { types::F64 };
                    (vt, info.clone())
                } else {
                    (types::F64, LocalInfo {
                        var: Variable::new(0),
                        name: None,
                        class_name: None,
                        type_args: Vec::new(),
                        is_pointer: false, is_array: false, is_string: false, is_bigint: false,
                        is_closure: false, closure_func_id: None, is_boxed: false, is_map: false, is_set: false,
                        is_buffer: false, is_event_emitter: false, is_union: false, is_mixed_array: false, is_integer: false,
                        is_integer_array: false, is_i32: false, i32_shadow: None,
                        bounded_by_array: None, bounded_by_constant: None, scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    })
                };
                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, var_type);
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);
                let mut info = local_info_template;
                info.var = var;
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // Compile setter body with 'this' context
            let this_ctx = ThisContext {
                this_var,
                class_meta: class_meta.clone(),
            };

            for stmt in &setter.body {
                compile_stmt(
                    &mut builder,
                    &mut self.module,
                    &self.func_ids,
                    &self.closure_func_ids,
                    &self.func_wrapper_ids,
                    &self.extern_funcs,
                    &self.async_func_ids,
                    &self.closure_returning_funcs,
                    &self.classes,
                    &self.enums,
                    &self.func_param_types,
                    &self.func_union_params,
                    &self.func_return_types,
                    &self.func_hir_return_types,
                    &self.func_rest_param_index,
                    &self.imported_func_param_counts,
                    &mut locals,
                    &mut next_var,
                    stmt,
                    Some(&this_ctx),
                    None,
                    &boxed_vars,
                    None,
                )?;
            }

            // If no explicit return, return 0 with correct type
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

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in setter '{}::set_{}' ===", class.name, prop_name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling setter '{}::set_{}': {}", class.name, prop_name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    pub(crate) fn declare_static_methods(&mut self, class: &Class) -> Result<()> {
        for method in &class.static_methods {
            let mut sig = self.module.make_signature();

            // Static methods do NOT take 'this' - they're regular functions
            for _ in &method.params {
                sig.params.push(AbiParam::new(types::F64));
            }

            // Return type (f64 for now)
            sig.returns.push(AbiParam::new(types::F64));

            let func_name = format!("{}__{}_{}__static", self.module_symbol_prefix, class.name, method.name);
            let func_id = self.module.declare_function(&func_name, Linkage::Export, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.static_method_ids.insert(method.name.clone(), func_id);
            }
        }
        Ok(())
    }

    pub(crate) fn declare_static_fields(&mut self, class: &Class) -> Result<()> {
        let linkage = if class.is_exported { Linkage::Export } else { Linkage::Local };
        for field in &class.static_fields {
            // Static fields are global variables stored as 8 bytes (f64)
            let data_name = format!("{}__{}_{}__static_field", self.module_symbol_prefix, class.name, field.name);
            let data_id = self.module.declare_data(&data_name, linkage, true, false)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.static_field_ids.insert(field.name.clone(), data_id);
            }
        }
        Ok(())
    }

    pub(crate) fn compile_static_method(&mut self, class: &Class, method: &Function) -> Result<()> {
        let func_name = format!("{}__{}_{}__static", self.module_symbol_prefix, class.name, method.name);
        let func_id = self.classes.get(&class.name)
            .and_then(|m| m.static_method_ids.get(&method.name).copied())
            .ok_or_else(|| anyhow!("Static method not declared: {}::{}", class.name, method.name))?;

        // Set current static class name so `this.method()` resolves to static method calls
        CURRENT_STATIC_CLASS_NAME.with(|c| *c.borrow_mut() = Some(class.name.clone()));

        // Set up the function signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // Static methods do NOT have 'this'
        for _ in &method.params {
            self.ctx.func.signature.params.push(AbiParam::new(types::F64));
        }
        self.ctx.func.signature.returns.push(AbiParam::new(types::F64));

        // Collect mutable captures before FunctionBuilder block
        let boxed_vars = self.collect_mutable_captures_from_stmts(&method.body);

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut static_method_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut static_method_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Create variables for parameters (no 'this')
            let mut locals: BTreeMap<LocalId, LocalInfo> = BTreeMap::new();
            let mut next_var = 0usize;
            for (i, param) in method.params.iter().enumerate() {
                let var = Variable::new(next_var);
                next_var += 1;
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                let is_string = matches!(param.ty, perry_types::Type::String);
                let is_array = matches!(param.ty, perry_types::Type::Array(_));
                let is_map = matches!(param.ty, perry_types::Type::Named(ref n) if n == "Map");
                let is_set = matches!(param.ty, perry_types::Type::Named(ref n) if n == "Set");
                let is_pointer = is_closure || is_string || is_array || is_map || is_set ||
                    matches!(param.ty, perry_types::Type::Object(_) | perry_types::Type::Named(_));
                let is_union_type = matches!(param.ty, perry_types::Type::Any | perry_types::Type::Unknown);
                let is_pointer = is_pointer && !is_union_type;
                let var_type = if is_pointer { types::I64 } else { types::F64 };
                builder.declare_var(var, var_type);
                let val = builder.block_params(entry_block)[i];
                let final_val = if is_pointer {
                    let get_ptr_func = self.extern_funcs.get("js_nanbox_get_pointer")
                        .expect("js_nanbox_get_pointer not declared");
                    let get_ptr_ref = self.module.declare_func_in_func(*get_ptr_func, builder.func);
                    let call = builder.ins().call(get_ptr_ref, &[val]);
                    builder.inst_results(call)[0]
                } else {
                    val
                };
                builder.def_var(var, final_val);
                locals.insert(param.id, LocalInfo {
                    var,
                    name: Some(param.name.clone()),
                    class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                    type_args: Vec::new(),
                    is_pointer,
                    is_array,
                    is_string,
                    is_bigint: matches!(param.ty, perry_types::Type::BigInt),
                    is_closure, closure_func_id: None,
                    is_boxed: false,
                    is_map, is_set, is_buffer: false, is_event_emitter: false, is_union: is_union_type,
                    is_mixed_array: false,
                    is_integer: false,
                    is_integer_array: false,
                    is_i32: false,
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                });
            }

            // Load module-level variables from their global slots
            for (local_id, data_id) in &self.module_var_data_ids {
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    // Variable is stored as i64 only if is_pointer && !is_union
                    let vt = if info.is_pointer && !info.is_union { types::I64 } else { types::F64 };
                    (vt, info.clone())
                } else {
                    (types::F64, LocalInfo {
                        var: Variable::new(0),
                        name: None,
                        class_name: None,
                        type_args: Vec::new(),
                        is_pointer: false, is_array: false, is_string: false, is_bigint: false,
                        is_closure: false, closure_func_id: None, is_boxed: false, is_map: false, is_set: false,
                        is_buffer: false, is_event_emitter: false, is_union: false, is_mixed_array: false, is_integer: false,
                        is_integer_array: false, is_i32: false, i32_shadow: None,
                        bounded_by_array: None, bounded_by_constant: None, scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    })
                };
                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, var_type);
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);
                let mut info = local_info_template;
                info.var = var;
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // Compile method body WITHOUT 'this' context
            for stmt in &method.body {
                compile_stmt(
                    &mut builder,
                    &mut self.module,
                    &self.func_ids,
                    &self.closure_func_ids,
                    &self.func_wrapper_ids,
                    &self.extern_funcs,
                    &self.async_func_ids,
                    &self.closure_returning_funcs,
                    &self.classes,
                    &self.enums,
                    &self.func_param_types,
                    &self.func_union_params,
                    &self.func_return_types,
                    &self.func_hir_return_types,
                    &self.func_rest_param_index,
                    &self.imported_func_param_counts,
                    &mut locals,
                    &mut next_var,
                    stmt,
                    None, // No 'this' context for static methods
                    None,
                    &boxed_vars,
                    None,
                )?;
            }

            // If no explicit return, return 0 with correct type
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

            builder.finalize();
        }

        // Clear current static class name
        CURRENT_STATIC_CLASS_NAME.with(|c| *c.borrow_mut() = None);

        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in static method '{}::{}' ===", class.name, method.name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling static method '{}::{}': {}", class.name, method.name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    pub(crate) fn compile_static_field(&mut self, class: &Class, field: &ClassField) -> Result<()> {
        let data_id = self.classes.get(&class.name)
            .and_then(|m| m.static_field_ids.get(&field.name).copied())
            .ok_or_else(|| anyhow!("Static field not declared: {}::{}", class.name, field.name))?;

        // Create data description and define the static field
        let mut data_desc = DataDescription::new();

        // Initialize with the field's init value or 0.0
        // For booleans, use NaN-boxed tags
        const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
        const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
        let init_value: f64 = match &field.init {
            Some(Expr::Number(n)) => *n,
            Some(Expr::Integer(n)) => *n as f64,
            Some(Expr::Bool(b)) => f64::from_bits(if *b { TAG_TRUE } else { TAG_FALSE }),
            Some(expr @ Expr::String(_)) |
            Some(expr @ Expr::BigInt(_)) |
            Some(expr @ Expr::Unary { .. }) |
            Some(expr @ Expr::Binary { .. }) |
            Some(expr @ Expr::Call { .. }) => {
                // Complex expressions need runtime evaluation — defer to compile_init
                self.static_field_runtime_inits.push((data_id, expr.clone()));
                0.0 // Placeholder, will be overwritten at runtime
            }
            _ => 0.0
        };

        data_desc.init = Init::Bytes {
            contents: init_value.to_le_bytes().to_vec().into_boxed_slice(),
        };
        self.module.define_data(data_id, &data_desc)?;

        Ok(())
    }

    pub(crate) fn declare_class_constructor(&mut self, class: &Class) -> Result<()> {
        if let Some(ref ctor) = class.constructor {
            let mut sig = self.module.make_signature();

            // Constructor takes 'this' pointer as first parameter, then user params
            sig.params.push(AbiParam::new(types::I64)); // 'this' pointer
            for _ in &ctor.params {
                sig.params.push(AbiParam::new(types::F64));
            }
            // Constructor returns void - the object is passed in

            let func_name = format!("{}__{}_constructor", self.module_symbol_prefix, class.name);
            // Export constructors for exported classes so other modules can call them
            let linkage = if class.is_exported { Linkage::Export } else { Linkage::Local };
            let func_id = self.module.declare_function(&func_name, linkage, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.constructor_id = Some(func_id);
            }
        }
        Ok(())
    }

    pub(crate) fn compile_class_constructor(&mut self, class: &Class, ctor: &Function) -> Result<()> {
        let func_name = format!("{}__{}_constructor", self.module_symbol_prefix, class.name);
        let func_id = self.classes.get(&class.name)
            .and_then(|m| m.constructor_id)
            .ok_or_else(|| anyhow!("Constructor not declared for class {}", class.name))?;

        // Set up the function signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // First parameter is 'this' pointer
        self.ctx.func.signature.params.push(AbiParam::new(types::I64));
        for _ in &ctor.params {
            self.ctx.func.signature.params.push(AbiParam::new(types::F64));
        }
        // Constructor returns void - the object is passed in

        let class_meta = self.classes.get(&class.name).cloned()
            .ok_or_else(|| anyhow!("Class metadata not found: {}", class.name))?;

        // Collect mutable captures before FunctionBuilder block
        let boxed_vars = self.collect_mutable_captures_from_stmts(&ctor.body);

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut ctor_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut ctor_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // 'this' is passed as the first parameter
            let obj_ptr = builder.block_params(entry_block)[0];

            // 'this' is the object pointer
            let mut locals: BTreeMap<LocalId, LocalInfo> = BTreeMap::new();
            let this_var = Variable::new(0);
            builder.declare_var(this_var, types::I64);
            builder.def_var(this_var, obj_ptr);

            // Create variables for user parameters (starting from index 1 in block params)
            let mut next_var = 1usize;
            for (i, param) in ctor.params.iter().enumerate() {
                let var = Variable::new(next_var);
                next_var += 1;
                // Check parameter types for correct handling of string methods, array methods, etc.
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                let is_string = matches!(param.ty, perry_types::Type::String);
                let is_array = matches!(param.ty, perry_types::Type::Array(_));
                let is_map = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Map")
                    || matches!(&param.ty, perry_types::Type::Named(name) if name == "Map");
                let is_set = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Set")
                    || matches!(&param.ty, perry_types::Type::Named(name) if name == "Set");
                let is_union = matches!(param.ty, perry_types::Type::Any | perry_types::Type::Union(_) | perry_types::Type::Unknown);
                let is_pointer = is_closure || is_string || is_array ||
                    matches!(param.ty, perry_types::Type::Object(_) | perry_types::Type::Named(_) | perry_types::Type::Promise(_));
                // Constructor params come in as NaN-boxed F64 values (from the signature)
                // Always use F64 for variable type - is_pointer flag is for extraction, not storage
                builder.declare_var(var, types::F64);
                let val = builder.block_params(entry_block)[i + 1]; // +1 to skip 'this'
                builder.def_var(var, val);
                // Constructor params are NaN-boxed F64, so is_pointer is false (not raw I64)
                // The is_array/is_string/is_closure flags indicate the type for proper extraction
                locals.insert(param.id, LocalInfo {
                    var,
                    name: Some(param.name.clone()),
                    class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                    type_args: Vec::new(),
                    is_pointer: false,  // NaN-boxed F64, not raw I64 pointer
                    is_array,
                    is_string,
                    is_bigint: matches!(param.ty, perry_types::Type::BigInt),
                    is_closure, closure_func_id: None,
                    is_boxed: false,
                    is_map, is_set, is_buffer: false, is_event_emitter: false, is_union,
                    is_mixed_array: false,
                    is_integer: false,
                    is_integer_array: false,
                    is_i32: false,
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                });
            }

            // Load module-level variables from their global slots
            for (local_id, data_id) in &self.module_var_data_ids {
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    // Variable is stored as i64 only if is_pointer && !is_union
                    let vt = if info.is_pointer && !info.is_union { types::I64 } else { types::F64 };
                    (vt, info.clone())
                } else {
                    (types::F64, LocalInfo {
                        var: Variable::new(0),
                        name: None,
                        class_name: None,
                        type_args: Vec::new(),
                        is_pointer: false, is_array: false, is_string: false, is_bigint: false,
                        is_closure: false, closure_func_id: None, is_boxed: false, is_map: false, is_set: false,
                        is_buffer: false, is_event_emitter: false, is_union: false, is_mixed_array: false, is_integer: false,
                        is_integer_array: false, is_i32: false, i32_shadow: None,
                        bounded_by_array: None, bounded_by_constant: None, scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None,
                    })
                };
                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, var_type);
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);
                let mut info = local_info_template;
                info.var = var;
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // Initialize field defaults at the START of the constructor.
            // Field initializers run before the constructor body in TypeScript semantics.
            // We compile them here (inside the constructor) rather than at the `new` call site
            // because the constructor has access to the class's module-level variables.
            // At `new` call sites in other modules, module-local `LocalGet` references would
            // not resolve since they refer to the defining module's scope.
            let obj_ptr_for_fields = builder.use_var(this_var);
            for (field_name, init_expr) in &class_meta.field_inits {
                if let Some(&field_idx) = class_meta.field_indices.get(field_name) {
                    let field_offset = 24 + (field_idx as i32) * 8;

                    let this_ctx_for_init = ThisContext { this_var, class_meta: class_meta.clone() };
                    let init_val = compile_expr(
                        &mut builder, &mut self.module,
                        &self.func_ids, &self.closure_func_ids, &self.func_wrapper_ids,
                        &self.extern_funcs, &self.async_func_ids,
                        &self.classes, &self.enums,
                        &self.func_param_types, &self.func_union_params,
                        &self.func_return_types, &self.func_hir_return_types,
                        &self.func_rest_param_index, &self.imported_func_param_counts,
                        &locals, init_expr, Some(&this_ctx_for_init),
                    )?;

                    // Determine if the value needs NaN-boxing for storage
                    let store_val = match init_expr {
                        Expr::Array(_) | Expr::ArraySpread(_) | Expr::Object(_) | Expr::ObjectSpread { .. } => {
                            let ptr_i64 = ensure_i64(&mut builder, init_val);
                            let nanbox_func = self.extern_funcs.get("js_nanbox_pointer")
                                .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                            let nanbox_ref = self.module.declare_func_in_func(*nanbox_func, builder.func);
                            let nanbox_call = builder.ins().call(nanbox_ref, &[ptr_i64]);
                            builder.inst_results(nanbox_call)[0]
                        }
                        _ => {
                            ensure_f64(&mut builder, init_val)
                        }
                    };

                    builder.ins().store(MemFlags::new(), store_val, obj_ptr_for_fields, field_offset);
                }
            }

            // Compile constructor body with special handling for 'this'
            for stmt in &ctor.body {
                compile_stmt_with_this(
                    &mut builder,
                    &mut self.module,
                    &self.func_ids,
                    &self.closure_func_ids,
                    &self.func_wrapper_ids,
                    &self.extern_funcs,
                    &self.async_func_ids,
                    &self.closure_returning_funcs,
                    &self.classes,
                    &self.enums,
                    &self.func_param_types,
                    &self.func_union_params,
                    &self.func_return_types,
                    &self.func_hir_return_types,
                    &self.func_rest_param_index,
                    &self.imported_func_param_counts,
                    &mut locals,
                    &mut next_var,
                    stmt,
                    this_var,
                    &class_meta,
                    None,
                    &boxed_vars,
                )?;
            }

            // Return void
            builder.ins().return_(&[]);

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in constructor '{}' ===", class.name);
            eprintln!("Error: {}", e);
            eprintln!("Debug: {:?}", e);
            eprintln!("=== CLIF IR ===");
            eprintln!("{}", self.ctx.func.display());
            return Err(anyhow!("Error compiling constructor '{}': {}", class.name, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }



}
