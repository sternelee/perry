//! Module initialization compilation for the codegen module.
//!
//! Contains the `compile_init` method which generates the module initialization
//! function that runs module-level statements, sets up exports, and registers
//! native instances.

use anyhow::{anyhow, Result};
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, Init, Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{HashMap, HashSet};

use perry_hir::{
    Expr, Stmt,
};
use perry_types::LocalId;

use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext, LoopContext};
use crate::util::*;
use crate::stmt::compile_stmt;
use crate::expr::compile_expr;

impl crate::codegen::Compiler {
    pub(crate) fn compile_init(&mut self, module_name: &str, stmts: &[Stmt], exported_native_instances: &[(String, String, String)], exported_objects: &[String], exported_functions: &[(String, u32)]) -> Result<()> {
        let is_dylib = self.output_type == "dylib";

        // Create main function for init statements (entry module) or module init function (non-entry)
        let mut sig = self.module.make_signature();
        if is_dylib && self.is_entry_module {
            // plugin_activate(api_handle: i64) -> i64
            sig.params.push(AbiParam::new(types::I64)); // api_handle
            sig.returns.push(AbiParam::new(types::I64)); // success (1) or failure (0)
        } else {
            sig.returns.push(AbiParam::new(types::I32)); // returns i32
        }

        // iOS game loop mode: when targeting iOS with the "ios-game-loop" feature,
        // generate "_perry_user_main" instead of "main". The runtime provides the
        // actual "main" which calls UIApplicationMain on the main thread and spawns
        // _perry_user_main on a background game thread.
        let ios_game_loop = (self.compile_target == 1 || self.compile_target == 6) && self.enabled_features.contains("ios-game-loop");

        let func_id = if self.is_entry_module && is_dylib {
            // Dylib: generate "plugin_activate" as the entry point
            self.module.declare_function("plugin_activate", Linkage::Export, &sig)?
        } else if self.is_entry_module && ios_game_loop {
            // iOS game loop: generate "_perry_user_main" (actual main provided by runtime)
            self.module.declare_function("_perry_user_main", Linkage::Export, &sig)?
        } else if self.is_entry_module {
            // Entry module: generate "main"
            // On Android, -Bsymbolic-functions prevents the process's main() from
            // being called instead of ours via ELF symbol interposition.
            match self.module.declare_function("main", Linkage::Export, &sig) {
                Ok(id) => id,
                Err(_) => {
                    // "main" already exists (likely user function with different signature)
                    // Use alternative name for the entry point
                    self.module.declare_function("_perry_main", Linkage::Export, &sig)?
                }
            }
        } else {
            // Non-entry module: generate "_perry_init_<module_name>" with export linkage
            // This function will be called by the entry module's main
            let sanitized_name = module_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
            let func_name = format!("_perry_init_{}", sanitized_name);
            self.module.declare_function(&func_name, Linkage::Export, &sig)?
        };

        self.ctx.func.signature = sig;

        // Collect all variables that will be mutably captured by closures (before borrowing self.ctx)
        let all_boxed_vars = self.collect_mutable_captures_from_stmts(stmts);
        // Debug: all_boxed and modvar_keys
        // Module-level variables use global slots as their box pointer (handled after
        // data_id assignment below). Only pass non-module-level vars for heap-boxing in Stmt::Let.
        let boxed_vars: std::collections::HashSet<LocalId> = all_boxed_vars.iter()
            .filter(|id| !self.module_var_data_ids.contains_key(id))
            .copied()
            .collect();

        // Check if we need to call js_runtime_init
        let needs_js_runtime = self.needs_js_runtime;
        let js_runtime_init_id = if needs_js_runtime {
            self.extern_funcs.get("js_runtime_init").copied()
        } else {
            None
        };

        // Collect exported native instance names for post-processing
        let exported_native_names: HashSet<String> = exported_native_instances.iter()
            .map(|(name, _, _)| name.clone())
            .collect();
        let exported_object_names: HashSet<String> = exported_objects.iter().cloned().collect();
        // Combine all exported names
        let exported_names: HashSet<String> = exported_native_names.iter()
            .chain(exported_object_names.iter())
            .cloned()
            .collect();
        // Collect exported function info for initializing their globals
        // Each entry is (func_name, data_id, wrapper_or_func_id)
        let exported_func_info: Vec<(String, cranelift_module::DataId, cranelift_module::FuncId)> = exported_functions
            .iter()
            .filter_map(|(func_name, hir_func_id)| {
                // Get the data ID for this exported function
                let (data_id, _) = self.exported_function_ids.get(func_name)?;
                // Get the wrapper function ID if it exists, otherwise the direct function ID
                let func_id = self.func_wrapper_ids.get(hir_func_id)
                    .copied()
                    .or_else(|| self.func_ids.get(hir_func_id).copied())?;
                Some((func_name.clone(), *data_id, func_id))
            })
            .collect();

        // Get js_closure_alloc function ID if we have exported functions
        let closure_alloc_id = if !exported_func_info.is_empty() {
            self.extern_funcs.get("js_closure_alloc").copied()
        } else {
            None
        };

        // For non-entry modules, use a runtime init guard to prevent re-entrant
        // initialization from circular module dependencies (stack overflow).
        // We call perry_init_guard_check_and_set(module_id) which atomically checks
        // and sets a bit in a runtime bitset. This is an external function call that
        // Cranelift cannot optimize away (unlike a local data flag).
        let init_guard_module_id = if !self.is_entry_module {
            Some(INIT_MODULE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
        } else {
            None
        };

        // Pre-collect vtable registration info for all classes (methods + getters).
        // Must be done before the FunctionBuilder block since self.classes can't be
        // borrowed while self.ctx.func is mutably borrowed.
        struct VTableRegInfo {
            class_id: u32,
            methods: Vec<(String, cranelift_module::FuncId, usize)>, // (name, func_id, param_count)
            getters: Vec<(String, cranelift_module::FuncId)>,        // (name, func_id)
        }
        let vtable_reg_info: Vec<VTableRegInfo> = self.classes.iter()
            .map(|(_, meta)| VTableRegInfo {
                class_id: meta.id,
                methods: meta.method_ids.iter()
                    .map(|(n, &fid)| (n.clone(), fid, *meta.method_param_counts.get(n).unwrap_or(&0)))
                    .collect(),
                getters: meta.getter_ids.iter()
                    .map(|(n, &fid)| (n.clone(), fid))
                    .collect(),
            })
            .collect();

        // Pre-collect class IDs for classes whose parent is a built-in Error class.
        // These will be registered in the runtime so `instanceof Error` works on user
        // subclasses like `class HttpError extends Error`.
        const ERROR_PARENT_NAMES: &[&str] = &[
            "Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError", "AggregateError",
        ];
        let error_subclass_ids: Vec<u32> = self.classes.iter()
            .filter_map(|(_, meta)| {
                if let Some(ref parent) = meta.parent_class {
                    if ERROR_PARENT_NAMES.contains(&parent.as_str()) {
                        return Some(meta.id);
                    }
                }
                None
            })
            .collect();

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        // The shared self.func_ctx accumulates variable declarations across functions
        let mut init_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut init_func_ctx);

            let entry_block = builder.create_block();
            if is_dylib && self.is_entry_module {
                // plugin_activate receives api_handle parameter
                builder.append_block_params_for_function_params(entry_block);
            }
            builder.switch_to_block(entry_block);

            // Re-entrancy guard for non-entry modules: call perry_init_guard_check_and_set()
            // in the runtime. Returns 1 if already initializing (skip), 0 to proceed.
            // Using an external function call prevents Cranelift from optimizing the guard away.
            if let Some(module_id) = init_guard_module_id {
                let mut guard_sig = self.module.make_signature();
                guard_sig.params.push(AbiParam::new(types::I64));
                guard_sig.returns.push(AbiParam::new(types::I32));
                let guard_func_id = self.module.declare_function(
                    "perry_init_guard_check_and_set", Linkage::Import, &guard_sig
                )?;
                let guard_func_ref = self.module.declare_func_in_func(guard_func_id, builder.func);
                let id_val = builder.ins().iconst(types::I64, module_id as i64);
                let call = builder.ins().call(guard_func_ref, &[id_val]);
                let already_init = builder.inst_results(call)[0];

                let init_block = builder.create_block();
                let early_ret_block = builder.create_block();
                let zero = builder.ins().iconst(types::I32, 0);
                let skip = builder.ins().icmp(IntCC::NotEqual, already_init, zero);
                builder.ins().brif(skip, early_ret_block, &[], init_block, &[]);

                // Early return block: return 0 (success) without re-initializing
                builder.switch_to_block(early_ret_block);
                builder.seal_block(early_ret_block);
                let ret_val = builder.ins().iconst(types::I32, 0);
                builder.ins().return_(&[ret_val]);

                // Continue with actual initialization
                builder.switch_to_block(init_block);
                builder.seal_block(init_block);
            }

            builder.seal_block(entry_block);

            // For dylib plugins, skip GC/dispatch init — the host handles those.
            // For executables, initialize handle method dispatch and GC.
            if self.is_entry_module && !is_dylib {
                if let Some(init_dispatch_id) = self.extern_funcs.get("js_stdlib_init_dispatch") {
                    let init_dispatch_ref = self.module.declare_func_in_func(*init_dispatch_id, builder.func);
                    builder.ins().call(init_dispatch_ref, &[]);
                }
                // Initialize GC (registers root scanners for promises, timers, exceptions)
                if let Some(gc_init_id) = self.extern_funcs.get("js_gc_init") {
                    let gc_init_ref = self.module.declare_func_in_func(*gc_init_id, builder.func);
                    builder.ins().call(gc_init_ref, &[]);
                }

                // Start geisterhand HTTP server if enabled
                if self.needs_geisterhand {
                    let mut gh_sig = self.module.make_signature();
                    gh_sig.params.push(AbiParam::new(types::I32)); // port
                    if let Ok(gh_func_id) = self.module.declare_function("perry_geisterhand_start", Linkage::Import, &gh_sig) {
                        let gh_func_ref = self.module.declare_func_in_func(gh_func_id, builder.func);
                        let port_val = builder.ins().iconst(types::I32, self.geisterhand_port as i64);
                        builder.ins().call(gh_func_ref, &[port_val]);
                    }
                }
            }

            // Initialize JS runtime at the start of main() if needed
            if let Some(init_func_id) = js_runtime_init_id {
                let init_func_ref = self.module.declare_func_in_func(init_func_id, builder.func);
                builder.ins().call(init_func_ref, &[]);
            }

            // Call imported native module init functions (for entry module only)
            // This ensures exports from other modules are initialized before we use them
            if self.is_entry_module {
                // Declare debug trace function for init order tracing
                let trace_func_id = {
                    let mut trace_sig = self.module.make_signature();
                    trace_sig.params.push(AbiParam::new(types::I64)); // index
                    trace_sig.params.push(AbiParam::new(types::I64)); // name_ptr
                    trace_sig.params.push(AbiParam::new(types::I64)); // name_len
                    self.module.declare_function("perry_debug_trace_init", Linkage::Import, &trace_sig).ok()
                };

                // Declare done trace function
                let trace_done_func_id = {
                    let mut done_sig = self.module.make_signature();
                    done_sig.params.push(AbiParam::new(types::I64)); // index
                    self.module.declare_function("perry_debug_trace_init_done", Linkage::Import, &done_sig).ok()
                };

                // i18n: call perry_i18n_init() with locale codes if i18n is configured
                {
                    let locale_codes = crate::util::I18N_TABLE.with(|t| {
                        let t = t.borrow();
                        if t.locale_count > 0 {
                            // Extract locale codes from compile.rs's i18n config
                            // The I18nCodegenTable doesn't store locale codes, so we use
                            // a separate thread-local for them.
                            crate::util::I18N_LOCALE_CODES.with(|c| c.borrow().clone())
                        } else {
                            Vec::new()
                        }
                    });
                    if !locale_codes.is_empty() {
                        let init_func_id = *self.extern_funcs.get("perry_i18n_init")
                            .expect("perry_i18n_init not declared");
                        let init_func_ref = self.module.declare_func_in_func(init_func_id, builder.func);

                        let count = locale_codes.len();
                        // Create stack slots for locale code pointers and lengths
                        let ptrs_slot = builder.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot, (count * 8) as u32, 0,
                        ));
                        let lens_slot = builder.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot, (count * 4) as u32, 0,
                        ));

                        for (i, code) in locale_codes.iter().enumerate() {
                            let bytes = code.as_bytes();
                            let data_name = format!("_perry_i18n_locale_{}", i);
                            let data_id = self.module.declare_data(&data_name, Linkage::Local, false, false)?;
                            let mut data_desc = cranelift_module::DataDescription::new();
                            data_desc.define(bytes.to_vec().into_boxed_slice());
                            self.module.define_data(data_id, &data_desc)?;
                            let gv = self.module.declare_data_in_func(data_id, builder.func);
                            let ptr = builder.ins().global_value(types::I64, gv);
                            builder.ins().stack_store(ptr, ptrs_slot, (i * 8) as i32);
                            let len = builder.ins().iconst(types::I32, bytes.len() as i64);
                            builder.ins().stack_store(len, lens_slot, (i * 4) as i32);
                        }

                        let ptrs_addr = builder.ins().stack_addr(types::I64, ptrs_slot, 0);
                        let lens_addr = builder.ins().stack_addr(types::I64, lens_slot, 0);
                        let count_val = builder.ins().iconst(types::I32, count as i64);
                        builder.ins().call(init_func_ref, &[ptrs_addr, lens_addr, count_val]);
                    }
                }

                for (i, init_func_name) in self.native_module_inits.clone().iter().enumerate() {
                    // Emit debug trace call before each init
                    if let Some(trace_id) = trace_func_id {
                        let trace_ref = self.module.declare_func_in_func(trace_id, builder.func);
                        let idx_val = builder.ins().iconst(types::I64, i as i64);
                        // Store init function name as data and pass pointer
                        let name_bytes = init_func_name.as_bytes();
                        let data_name = format!("__init_trace_name_{}", i);
                        if let Ok(data_id) = self.module.declare_data(&data_name, Linkage::Local, false, false) {
                            let mut data_desc = DataDescription::new();
                            data_desc.define(name_bytes.to_vec().into_boxed_slice());
                            if self.module.define_data(data_id, &data_desc).is_ok() {
                                let gv = self.module.declare_data_in_func(data_id, builder.func);
                                let name_ptr = builder.ins().global_value(types::I64, gv);
                                let name_len = builder.ins().iconst(types::I64, name_bytes.len() as i64);
                                builder.ins().call(trace_ref, &[idx_val, name_ptr, name_len]);
                            }
                        }
                    }

                    // Declare the external init function
                    let mut init_sig = self.module.make_signature();
                    init_sig.returns.push(AbiParam::new(types::I32));
                    if let Ok(init_func_id) = self.module.declare_function(init_func_name, Linkage::Import, &init_sig) {
                        let init_func_ref = self.module.declare_func_in_func(init_func_id, builder.func);
                        builder.ins().call(init_func_ref, &[]);
                    }

                    // Emit done trace call after each init
                    if let Some(done_id) = trace_done_func_id {
                        let done_ref = self.module.declare_func_in_func(done_id, builder.func);
                        let idx_val = builder.ins().iconst(types::I64, i as i64);
                        builder.ins().call(done_ref, &[idx_val]);
                    }
                }

                // Register bundled extensions as static plugins.
                // After all module inits have run, each extension's default export global
                // is populated. We read it and register it in the runtime lookup table
                // so perryResolveStaticPlugin() can find it by source path.
                let bundled_extensions = std::mem::take(&mut self.bundled_extensions);
                if !bundled_extensions.is_empty() {
                    let register_func_id = *self.extern_funcs.get("perry_register_static_plugin")
                        .expect("perry_register_static_plugin not declared");
                    let register_func_ref = self.module.declare_func_in_func(register_func_id, builder.func);

                    let string_alloc_id = *self.extern_funcs.get("js_string_from_bytes")
                        .expect("js_string_from_bytes not declared");
                    let string_alloc_ref = self.module.declare_func_in_func(string_alloc_id, builder.func);

                    for (ext_source_path, ext_module_prefix) in &bundled_extensions {
                        // Load the extension's default export from __export_<prefix>__default
                        let export_global_name = format!("__export_{}__default", ext_module_prefix);
                        let data_id = match self.module.declare_data(&export_global_name, Linkage::Import, true, false) {
                            Ok(id) => id,
                            Err(_) => continue,
                        };
                        let gv = self.module.declare_data_in_func(data_id, builder.func);
                        let addr = builder.ins().global_value(types::I64, gv);
                        let export_val = builder.ins().load(types::F64, MemFlags::new(), addr, 0);

                        // Create string from the source path bytes
                        let path_bytes = ext_source_path.as_bytes();
                        let path_data_id = self.module.declare_anonymous_data(false, false)?;
                        let mut path_data_desc = cranelift_module::DataDescription::new();
                        path_data_desc.define(path_bytes.to_vec().into_boxed_slice());
                        self.module.define_data(path_data_id, &path_data_desc)?;
                        let path_data_val = self.module.declare_data_in_func(path_data_id, builder.func);
                        let path_ptr = builder.ins().global_value(types::I64, path_data_val);
                        let path_len = builder.ins().iconst(types::I32, path_bytes.len() as i64);
                        let call_inst = builder.ins().call(string_alloc_ref, &[path_ptr, path_len]);
                        let string_ptr = builder.inst_results(call_inst)[0];

                        // Call perry_register_static_plugin(string_ptr, export_val)
                        builder.ins().call(register_func_ref, &[string_ptr, export_val]);
                    }
                }
                self.bundled_extensions = bundled_extensions;
            }

            // Auto-call dotenv.config() if dotenv/config was imported (side-effect import)
            if self.needs_dotenv_init {
                if let Some(dotenv_func_id) = self.extern_funcs.get("js_dotenv_config") {
                    let dotenv_func_ref = self.module.declare_func_in_func(*dotenv_func_id, builder.func);
                    builder.ins().call(dotenv_func_ref, &[]);
                }
            }

            // Runtime-initialize static fields that need heap allocation (strings, etc.)
            for (data_id, init_expr) in std::mem::take(&mut self.static_field_runtime_inits) {
                let empty_locals: HashMap<LocalId, LocalInfo> = HashMap::new();
                let val = compile_expr(
                    &mut builder, &mut self.module,
                    &self.func_ids, &self.closure_func_ids, &self.func_wrapper_ids,
                    &self.extern_funcs, &self.async_func_ids,
                    &self.classes, &self.enums,
                    &self.func_param_types, &self.func_union_params,
                    &self.func_return_types, &self.func_hir_return_types,
                    &self.func_rest_param_index, &self.imported_func_param_counts,
                    &empty_locals, &init_expr, None,
                )?;
                let global_val = self.module.declare_data_in_func(data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                builder.ins().store(MemFlags::new(), val, ptr, 0);
            }

            // Runtime-initialize exported enum objects so Object.values(EnumName) works
            {
                // Group enum members by enum name
                let mut enum_groups: HashMap<String, Vec<(String, EnumMemberValue)>> = HashMap::new();
                for ((enum_name, member_name), value) in &self.enums {
                    enum_groups.entry(enum_name.clone())
                        .or_default()
                        .push((member_name.clone(), value.clone()));
                }

                for (enum_name, members) in &enum_groups {
                    // Only initialize enums that have an export data slot
                    let data_id = match self.exported_object_ids.get(enum_name) {
                        Some(id) => *id,
                        None => continue,
                    };

                    // Allocate object: js_object_alloc(class_id=0, field_count)
                    let alloc_func_id = *self.extern_funcs.get("js_object_alloc").unwrap();
                    let alloc_ref = self.module.declare_func_in_func(alloc_func_id, builder.func);
                    let class_id = builder.ins().iconst(types::I32, 0);
                    let field_count = builder.ins().iconst(types::I32, members.len() as i64);
                    let alloc_call = builder.ins().call(alloc_ref, &[class_id, field_count]);
                    let obj_ptr = builder.inst_results(alloc_call)[0]; // I64

                    // Set each member: js_object_set_field_by_name(obj, key_str, value)
                    let set_func_id = *self.extern_funcs.get("js_object_set_field_by_name").unwrap();
                    let set_ref = self.module.declare_func_in_func(set_func_id, builder.func);
                    let string_from_bytes_id = *self.extern_funcs.get("js_string_from_bytes").unwrap();
                    let string_from_bytes_ref = self.module.declare_func_in_func(string_from_bytes_id, builder.func);
                    let nanbox_string_id = *self.extern_funcs.get("js_nanbox_string").unwrap();
                    let nanbox_string_ref = self.module.declare_func_in_func(nanbox_string_id, builder.func);

                    for (member_name, member_value) in members {
                        // Create key string from member name bytes
                        let key_bytes = member_name.as_bytes();
                        let key_data_id = self.module.declare_anonymous_data(false, false)?;
                        let mut key_desc = cranelift_module::DataDescription::new();
                        key_desc.define(key_bytes.to_vec().into_boxed_slice());
                        self.module.define_data(key_data_id, &key_desc)?;
                        let key_gv = self.module.declare_data_in_func(key_data_id, builder.func);
                        let key_ptr = builder.ins().global_value(types::I64, key_gv);
                        let key_len = builder.ins().iconst(types::I32, key_bytes.len() as i64);
                        let key_call = builder.ins().call(string_from_bytes_ref, &[key_ptr, key_len]);
                        let key_str_ptr = builder.inst_results(key_call)[0]; // I64

                        // Create the value (F64)
                        let value_f64 = match member_value {
                            EnumMemberValue::Number(n) => {
                                builder.ins().f64const(*n as f64)
                            }
                            EnumMemberValue::String(s) => {
                                // Create string value
                                let val_bytes = s.as_bytes();
                                let val_data_id = self.module.declare_anonymous_data(false, false)?;
                                let mut val_desc = cranelift_module::DataDescription::new();
                                val_desc.define(val_bytes.to_vec().into_boxed_slice());
                                self.module.define_data(val_data_id, &val_desc)?;
                                let val_gv = self.module.declare_data_in_func(val_data_id, builder.func);
                                let val_ptr = builder.ins().global_value(types::I64, val_gv);
                                let val_len = builder.ins().iconst(types::I32, val_bytes.len() as i64);
                                let val_call = builder.ins().call(string_from_bytes_ref, &[val_ptr, val_len]);
                                let val_str_ptr = builder.inst_results(val_call)[0];
                                // NaN-box the string value
                                let nanbox_call = builder.ins().call(nanbox_string_ref, &[val_str_ptr]);
                                builder.inst_results(nanbox_call)[0]
                            }
                        };

                        // Set field: js_object_set_field_by_name(obj_ptr, key_str_ptr, value_f64)
                        builder.ins().call(set_ref, &[obj_ptr, key_str_ptr, value_f64]);
                    }

                    // NaN-box the object pointer and store to export global
                    let nanbox_ptr_id = *self.extern_funcs.get("js_nanbox_pointer").unwrap();
                    let nanbox_ptr_ref = self.module.declare_func_in_func(nanbox_ptr_id, builder.func);
                    let nanbox_call = builder.ins().call(nanbox_ptr_ref, &[obj_ptr]);
                    let obj_val = builder.inst_results(nanbox_call)[0]; // F64

                    let global_val = self.module.declare_data_in_func(data_id, builder.func);
                    let ptr = builder.ins().global_value(types::I64, global_val);
                    builder.ins().store(MemFlags::new(), obj_val, ptr, 0);
                }
            }

            // Emit vtable registration calls for every class method and getter.
            // Using func_addr creates linker roots that prevent dead_strip from
            // removing methods that are only reached via dynamic dispatch.
            {
                let register_method_id = *self.extern_funcs.get("js_register_class_method").unwrap();
                let register_method_ref = self.module.declare_func_in_func(register_method_id, builder.func);

                for info in &vtable_reg_info {
                    let class_id_val = builder.ins().iconst(types::I64, info.class_id as i64);

                    for (method_name, method_func_id, param_count) in &info.methods {
                        // Embed method name as static data
                        let name_bytes = method_name.as_bytes();
                        let data_id = self.module.declare_anonymous_data(false, false)?;
                        let mut desc = cranelift_module::DataDescription::new();
                        desc.define(name_bytes.to_vec().into_boxed_slice());
                        self.module.define_data(data_id, &desc)?;
                        let gv = self.module.declare_data_in_func(data_id, builder.func);
                        let name_ptr = builder.ins().global_value(types::I64, gv);
                        let name_len = builder.ins().iconst(types::I64, name_bytes.len() as i64);

                        // Get function address — this also creates a linker root
                        let func_ref = self.module.declare_func_in_func(*method_func_id, builder.func);
                        let func_ptr = builder.ins().func_addr(types::I64, func_ref);

                        let pc_val = builder.ins().iconst(types::I64, *param_count as i64);

                        builder.ins().call(register_method_ref, &[
                            class_id_val, name_ptr, name_len, func_ptr, pc_val,
                        ]);
                    }
                }
            }

            {
                let register_getter_id = *self.extern_funcs.get("js_register_class_getter").unwrap();
                let register_getter_ref = self.module.declare_func_in_func(register_getter_id, builder.func);

                for info in &vtable_reg_info {
                    let class_id_val = builder.ins().iconst(types::I64, info.class_id as i64);

                    for (getter_name, getter_func_id) in &info.getters {
                        // Embed getter name as static data
                        let name_bytes = getter_name.as_bytes();
                        let data_id = self.module.declare_anonymous_data(false, false)?;
                        let mut desc = cranelift_module::DataDescription::new();
                        desc.define(name_bytes.to_vec().into_boxed_slice());
                        self.module.define_data(data_id, &desc)?;
                        let gv = self.module.declare_data_in_func(data_id, builder.func);
                        let name_ptr = builder.ins().global_value(types::I64, gv);
                        let name_len = builder.ins().iconst(types::I64, name_bytes.len() as i64);

                        // Get function address
                        let func_ref = self.module.declare_func_in_func(*getter_func_id, builder.func);
                        let func_ptr = builder.ins().func_addr(types::I64, func_ref);

                        builder.ins().call(register_getter_ref, &[
                            class_id_val, name_ptr, name_len, func_ptr,
                        ]);
                    }
                }
            }

            // Register classes that extend Error so `instanceof Error` works on user subclasses
            if !error_subclass_ids.is_empty() {
                let register_extends_id = *self.extern_funcs.get("js_register_class_extends_error").unwrap();
                let register_extends_ref = self.module.declare_func_in_func(register_extends_id, builder.func);
                for cid in &error_subclass_ids {
                    let cid_val = builder.ins().iconst(types::I32, *cid as i64);
                    builder.ins().call(register_extends_ref, &[cid_val]);
                }
            }

            let mut locals: HashMap<LocalId, LocalInfo> = HashMap::new();
            let mut next_var = 0;

            for stmt in stmts {
                // Check if this statement is a Let for a module-level variable
                if let Stmt::Let { name, init: Some(_init_expr), id, .. } = stmt {
                    // Compile the statement to get the value (this creates the local variable)
                    compile_stmt(&mut builder, &mut self.module, &self.func_ids, &self.closure_func_ids, &self.func_wrapper_ids, &self.extern_funcs, &self.async_func_ids, &self.closure_returning_funcs, &self.classes, &self.enums, &self.func_param_types, &self.func_union_params, &self.func_return_types, &self.func_hir_return_types, &self.func_rest_param_index, &self.imported_func_param_counts, &mut locals, &mut next_var, stmt, None, None, &boxed_vars, None)?;

                    // Get the value from the local variable
                    if let Some(local_info) = locals.get(id).cloned() {
                        let val = builder.use_var(local_info.var);

                        // Store to exported global if this is an exported variable
                        if exported_names.contains(name) {
                            let data_id = self.exported_native_instance_ids.get(name)
                                .or_else(|| self.exported_object_ids.get(name))
                                .copied();
                            if let Some(data_id) = data_id {
                                let global_val = self.module.declare_data_in_func(data_id, builder.func);
                                let ptr = builder.ins().global_value(types::I64, global_val);

                                // NaN-box pointer types before storing to export globals,
                                // so importing modules can load them uniformly as f64.
                                let val_to_store = {
                                    let val_type = builder.func.dfg.value_type(val);
                                    if local_info.is_string && val_type == types::I64 {
                                        // String pointer: NaN-box with STRING_TAG
                                        inline_nanbox_string(&mut builder, val)
                                    } else if local_info.is_pointer && !local_info.is_string && val_type == types::I64 {
                                        // Object/array pointer: NaN-box with POINTER_TAG
                                        let nanbox_func_id = self.extern_funcs.get("js_nanbox_pointer")
                                            .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                                        let nanbox_ref = self.module.declare_func_in_func(*nanbox_func_id, builder.func);
                                        let call = builder.ins().call(nanbox_ref, &[val]);
                                        builder.inst_results(call)[0]
                                    } else if local_info.is_union && val_type == types::F64 {
                                        // Union-typed F64 that might be a pointer
                                        let i64_val = ensure_i64(&mut builder, val);
                                        let nanbox_func_id = self.extern_funcs.get("js_nanbox_pointer")
                                            .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                                        let nanbox_ref = self.module.declare_func_in_func(*nanbox_func_id, builder.func);
                                        let call = builder.ins().call(nanbox_ref, &[i64_val]);
                                        builder.inst_results(call)[0]
                                    } else {
                                        val
                                    }
                                };
                                builder.ins().store(MemFlags::new(), val_to_store, ptr, 0);
                            }
                        }

                        // Also store to module variable global for function access
                        if let Some(data_id) = self.module_var_data_ids.get(id).copied() {
                            let global_val = self.module.declare_data_in_func(data_id, builder.func);
                            let ptr = builder.ins().global_value(types::I64, global_val);
                            builder.ins().store(MemFlags::new(), val, ptr, 0);
                            // Store the LocalInfo so compile_function knows the type
                            self.module_level_locals.insert(*id, local_info);
                            // Tag the local with its global slot DataId so that closures
                            // capturing this variable mutably can use the global slot as
                            // the box, keeping named-function reads in sync.
                            if let Some(local_info_mut) = locals.get_mut(id) {
                                local_info_mut.module_var_data_id = Some(data_id);
                            }

                            // For module-level variables that are mutably captured by closures,
                            // convert to boxed access using the global slot address as the box pointer.
                            // This ensures the outer scope always reads the latest value after
                            // closures modify the variable via js_box_set on the global slot.
                            if all_boxed_vars.contains(id) {
                                let global_val = self.module.declare_data_in_func(data_id, builder.func);
                                let slot_addr = builder.ins().global_value(types::I64, global_val);

                                let box_var = Variable::new(next_var);
                                next_var += 1;
                                builder.declare_var(box_var, types::I64);
                                builder.def_var(box_var, slot_addr);

                                if let Some(local_info_mut) = locals.get_mut(id) {
                                    local_info_mut.var = box_var;
                                    local_info_mut.is_boxed = true;
                                }
                            }
                        }
                    }
                    // Fall through to reload module vars below
                } else {
                    compile_stmt(&mut builder, &mut self.module, &self.func_ids, &self.closure_func_ids, &self.func_wrapper_ids, &self.extern_funcs, &self.async_func_ids, &self.closure_returning_funcs, &self.classes, &self.enums, &self.func_param_types, &self.func_union_params, &self.func_return_types, &self.func_hir_return_types, &self.func_rest_param_index, &self.imported_func_param_counts, &mut locals, &mut next_var, stmt, None, None, &boxed_vars, None)?;
                }

                // Reload all module-level variables from their global slots.
                // Function calls inside the statement may have modified module variables
                // via LocalSet write-back. The init function's Cranelift locals are stale
                // unless we reload them from the global data slots.
                // Collect vars to reload first to avoid borrow conflicts
                let vars_to_reload: Vec<(Variable, cranelift::prelude::types::Type, cranelift_module::DataId)> = locals.iter()
                    .filter(|(_, info)| !info.is_boxed && info.module_var_data_id.is_some())
                    .map(|(_, info)| {
                        let val = builder.use_var(info.var);
                        let var_type = builder.func.dfg.value_type(val);
                        (info.var, var_type, info.module_var_data_id.unwrap())
                    })
                    .collect();
                for (var, var_type, data_id) in vars_to_reload {
                    let global_val = self.module.declare_data_in_func(data_id, builder.func);
                    let ptr = builder.ins().global_value(types::I64, global_val);
                    let loaded = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                    builder.def_var(var, loaded);
                }
            }

            // NOTE: The old "write back all module-level variables" loop was removed.
            // With the LocalSet write-back fix (Bug #17), every assignment to a module
            // variable immediately writes to the global slot. The old blanket write-back
            // was harmful: it stored STALE init-local values back to globals, overwriting
            // values that called functions had written via their own LocalSet write-backs.

            // Initialize exported function globals with closure values
            // This allows functions to be passed as values to other modules
            if let Some(alloc_func_id) = closure_alloc_id {
                let alloc_ref = self.module.declare_func_in_func(alloc_func_id, builder.func);
                // Get js_nanbox_pointer function for proper NaN-boxing
                let nanbox_func_id = self.extern_funcs.get("js_nanbox_pointer")
                    .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                let nanbox_ref = self.module.declare_func_in_func(*nanbox_func_id, builder.func);

                for (_func_name, data_id, wrapper_func_id) in &exported_func_info {
                    // Get the function address
                    let func_ref = self.module.declare_func_in_func(*wrapper_func_id, builder.func);
                    let func_ptr = builder.ins().func_addr(types::I64, func_ref);

                    // Allocate a closure with 0 captures
                    let capture_count = builder.ins().iconst(types::I32, 0);
                    let call = builder.ins().call(alloc_ref, &[func_ptr, capture_count]);
                    let closure_ptr = builder.inst_results(call)[0];

                    // Properly NaN-box the closure pointer using js_nanbox_pointer
                    // This ensures typeof returns "object" (closures are objects) and
                    // the value can be properly recognized by runtime functions
                    let nanbox_call = builder.ins().call(nanbox_ref, &[closure_ptr]);
                    let closure_val = builder.inst_results(nanbox_call)[0];

                    // Store to the exported global
                    let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                    let ptr = builder.ins().global_value(types::I64, global_val);
                    builder.ins().store(MemFlags::new(), closure_val, ptr, 0);
                }
            }

            // Register module-level variable addresses as GC roots
            // This ensures the GC can find references stored in module globals
            if let Some(gc_root_id) = self.extern_funcs.get("js_gc_register_global_root").copied() {
                let gc_root_ref = self.module.declare_func_in_func(gc_root_id, builder.func);
                for (_local_id, data_id) in &self.module_var_data_ids {
                    let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                    let ptr = builder.ins().global_value(types::I64, global_val);
                    builder.ins().call(gc_root_ref, &[ptr]);
                }

                // Also register static field data slots as GC roots.
                // Static fields (e.g., TickMath.MIN_SQRT_RATIO = 4295128739n) store
                // gc-allocated values (BigInts, strings, arrays) in global data slots.
                // Without GC root registration, these get collected → dangling pointers.
                for (_class_name, class_meta) in &self.classes {
                    for (_field_name, data_id) in &class_meta.static_field_ids {
                        let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                        let ptr = builder.ins().global_value(types::I64, global_val);
                        builder.ins().call(gc_root_ref, &[ptr]);
                    }
                }
            }

            // For dylib plugins, call the user's exported activate(api) function
            // Use direct func_ids (not wrappers, which have an extra closure_ptr param)
            if is_dylib && self.is_entry_module {
                let current_block = builder.current_block().unwrap();
                if !is_block_filled(&builder, current_block) {
                    // Find the "activate" function's direct (non-wrapper) Cranelift func ID
                    let activate_func_id = exported_functions.iter()
                        .find(|(name, _)| name == "activate")
                        .and_then(|(_, hir_id)| self.func_ids.get(hir_id).copied());
                    if let Some(func_id) = activate_func_id {
                        let api_handle = builder.block_params(entry_block)[0]; // i64
                        // NaN-box api_handle with POINTER_TAG: 0x7FFD << 48 | (handle & 0x0000_FFFF_FFFF_FFFF)
                        let tag = builder.ins().iconst(types::I64, 0x7FFD_0000_0000_0000u64 as i64);
                        let mask = builder.ins().iconst(types::I64, 0x0000_FFFF_FFFF_FFFFu64 as i64);
                        let masked = builder.ins().band(api_handle, mask);
                        let nanboxed = builder.ins().bor(tag, masked);
                        // Check the activate function's parameter type and pass accordingly
                        let activate_ref = self.module.declare_func_in_func(func_id, builder.func);
                        let sig = builder.func.dfg.ext_funcs[activate_ref].signature;
                        let param_type = builder.func.dfg.signatures[sig].params[0].value_type;
                        let arg = if param_type == types::F64 {
                            builder.ins().bitcast(types::F64, MemFlags::new(), nanboxed)
                        } else {
                            nanboxed // i64
                        };
                        builder.ins().call(activate_ref, &[arg]);
                    }
                }
            }

            // For CLI entry modules, emit an event loop that keeps the process alive
            // while there are pending setInterval/setTimeout callback timers OR
            // pending node-cron jobs. Without this, the process exits as soon as the
            // init statements finish, causing setInterval/setTimeout/cron-based
            // services to die after the first tick.
            // Skipped for dylibs (no main() exit) and iOS game loop (runtime manages).
            if self.is_entry_module && !is_dylib && !ios_game_loop {
                let current_block = builder.current_block().unwrap();
                if !is_block_filled(&builder, current_block) {
                    let interval_tick_id = self.extern_funcs.get("js_interval_timer_tick").copied();
                    let callback_tick_id = self.extern_funcs.get("js_callback_timer_tick").copied();
                    let interval_pending_id = self.extern_funcs.get("js_interval_timer_has_pending").copied();
                    let callback_pending_id = self.extern_funcs.get("js_callback_timer_has_pending").copied();
                    let sleep_id = self.extern_funcs.get("js_sleep_ms").copied();
                    // Cron timer ticks are pumped from the same loop. The decls are
                    // unconditionally added by runtime_decls.rs so the .map() below
                    // is just defensive — the symbols are always materialised.
                    let cron_tick_id = self.extern_funcs.get("js_cron_timer_tick").copied();
                    let cron_pending_id = self.extern_funcs.get("js_cron_timer_has_pending").copied();

                    if let (Some(int_tick), Some(cb_tick), Some(int_pend), Some(cb_pend), Some(sleep_fn)) =
                        (interval_tick_id, callback_tick_id, interval_pending_id, callback_pending_id, sleep_id)
                    {
                        let int_tick_ref = self.module.declare_func_in_func(int_tick, builder.func);
                        let cb_tick_ref = self.module.declare_func_in_func(cb_tick, builder.func);
                        let int_pend_ref = self.module.declare_func_in_func(int_pend, builder.func);
                        let cb_pend_ref = self.module.declare_func_in_func(cb_pend, builder.func);
                        let sleep_ref = self.module.declare_func_in_func(sleep_fn, builder.func);
                        let cron_tick_ref = cron_tick_id.map(|id| self.module.declare_func_in_func(id, builder.func));
                        let cron_pend_ref = cron_pending_id.map(|id| self.module.declare_func_in_func(id, builder.func));

                        let loop_header = builder.create_block();
                        let loop_body = builder.create_block();
                        let loop_exit = builder.create_block();

                        // Jump to the loop header to start checking for pending timers
                        builder.ins().jump(loop_header, &[]);

                        // loop_header: if any pending timers, go to body; else exit
                        builder.switch_to_block(loop_header);
                        let int_has = {
                            let call = builder.ins().call(int_pend_ref, &[]);
                            builder.inst_results(call)[0]
                        };
                        let cb_has = {
                            let call = builder.ins().call(cb_pend_ref, &[]);
                            builder.inst_results(call)[0]
                        };
                        let mut any_pending = builder.ins().bor(int_has, cb_has);
                        if let Some(pend_ref) = cron_pend_ref {
                            let cron_has = {
                                let call = builder.ins().call(pend_ref, &[]);
                                builder.inst_results(call)[0]
                            };
                            any_pending = builder.ins().bor(any_pending, cron_has);
                        }
                        let zero_i32 = builder.ins().iconst(types::I32, 0);
                        let has_work = builder.ins().icmp(IntCC::NotEqual, any_pending, zero_i32);
                        builder.ins().brif(has_work, loop_body, &[], loop_exit, &[]);

                        // loop_body: tick all timer queues, then GC, sleep 10ms, jump back
                        builder.switch_to_block(loop_body);
                        builder.seal_block(loop_body);
                        builder.ins().call(int_tick_ref, &[]);
                        builder.ins().call(cb_tick_ref, &[]);
                        if let Some(tick_ref) = cron_tick_ref {
                            builder.ins().call(tick_ref, &[]);
                        }

                        // GC safe point: timer callbacks have returned, so all live JS values
                        // are stored in module globals, closure boxes, or timer root lists —
                        // NOT in registers. Uses threshold-based check (not unconditional
                        // collection) to avoid the overhead of running GC on every loop tick.
                        if let Some(&gc_trigger_func) = self.extern_funcs.get("gc_check_trigger_export") {
                            let gc_ref = self.module.declare_func_in_func(gc_trigger_func, builder.func);
                            builder.ins().call(gc_ref, &[]);
                        }

                        let ten_ms = builder.ins().f64const(10.0);
                        builder.ins().call(sleep_ref, &[ten_ms]);
                        builder.ins().jump(loop_header, &[]);

                        builder.seal_block(loop_header);
                        builder.switch_to_block(loop_exit);
                        builder.seal_block(loop_exit);
                    }
                }
            }

            // Return from init function (if not already terminated)
            let current_block = builder.current_block().unwrap();
            if !is_block_filled(&builder, current_block) {
                if is_dylib && self.is_entry_module {
                    // plugin_activate returns 1 (success) as i64
                    let one = builder.ins().iconst(types::I64, 1);
                    builder.ins().return_(&[one]);
                } else {
                    let zero = builder.ins().iconst(types::I32, 0);
                    builder.ins().return_(&[zero]);
                }
            }

            let fn_name = if self.is_entry_module && is_dylib {
                "plugin_activate"
            } else if self.is_entry_module {
                "main"
            } else {
                module_name
            };
            builder.finalize();
        }

        let func_name = if self.is_entry_module && is_dylib {
            "plugin_activate"
        } else if self.is_entry_module {
            "main"
        } else {
            module_name
        };
        if let Err(e) = self.module.define_function(func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in init/main '{}' ===", func_name);
            eprintln!("Error: {}", e);
            eprintln!("Debug: {:?}", e);
            // Print the CLIF IR for debugging
            eprintln!("=== CLIF IR ===");
            eprintln!("{}", self.ctx.func.display());
            return Err(anyhow!("Error compiling init/main '{}': {}", func_name, e));
        }
        self.module.clear_context(&mut self.ctx);

        // For dylib entry module, also generate plugin_deactivate and perry_plugin_abi_version
        if is_dylib && self.is_entry_module {
            // Generate plugin_deactivate() -> void
            // Calls the user's deactivate() function if exported, then returns
            {
                let sig = self.module.make_signature();
                let deactivate_id = self.module.declare_function("plugin_deactivate", Linkage::Export, &sig)?;
                self.ctx.func.signature = sig;
                let mut deactivate_ctx = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut deactivate_ctx);
                let block = builder.create_block();
                builder.switch_to_block(block);
                builder.seal_block(block);
                // Call user's deactivate() if exported (use direct func, not wrapper)
                let deactivate_func_id = exported_functions.iter()
                    .find(|(name, _)| name == "deactivate")
                    .and_then(|(_, hir_id)| self.func_ids.get(hir_id).copied());
                if let Some(func_id) = deactivate_func_id {
                    let deactivate_ref = self.module.declare_func_in_func(func_id, builder.func);
                    builder.ins().call(deactivate_ref, &[]);
                }
                builder.ins().return_(&[]);
                builder.finalize();
                self.module.define_function(deactivate_id, &mut self.ctx)?;
                self.module.clear_context(&mut self.ctx);
            }

            // Generate perry_plugin_abi_version() -> u64
            {
                let mut sig = self.module.make_signature();
                sig.returns.push(AbiParam::new(types::I64));
                let version_id = self.module.declare_function("perry_plugin_abi_version", Linkage::Export, &sig)?;
                self.ctx.func.signature = sig;
                let mut version_ctx = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut version_ctx);
                let block = builder.create_block();
                builder.switch_to_block(block);
                builder.seal_block(block);
                let version = builder.ins().iconst(types::I64, 2); // ABI version 2
                builder.ins().return_(&[version]);
                builder.finalize();
                self.module.define_function(version_id, &mut self.ctx)?;
                self.module.clear_context(&mut self.ctx);
            }
        }

        Ok(())
    }

}
