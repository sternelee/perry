//! Cranelift code generation
//!
//! Translates HIR to Cranelift IR and generates native machine code.

use anyhow::{anyhow, Result};
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, Init, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use perry_hir::{
    ArrayElement, BinaryOp, CallArg, CatchClause, Class, ClassField, CompareOp, Decorator, Expr, Function, LogicalOp, Module as HirModule, Stmt, UnaryOp, UpdateOp,
};
use perry_types::LocalId;
use cranelift_codegen::ir::{Block, StackSlot, StackSlotData, StackSlotKind, TrapCode};

use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext, LoopContext};
use crate::util::*;

/// The main compiler that generates native code from HIR
pub struct Compiler {
    /// Cranelift module for the object file
    pub(crate) module: ObjectModule,
    /// Cranelift context for function compilation
    pub(crate) ctx: Context,
    /// Function builder context (reused across functions)
    pub(crate) func_ctx: FunctionBuilderContext,
    /// Mapping from HIR function IDs to Cranelift function IDs
    pub(crate) func_ids: HashMap<u32, cranelift_module::FuncId>,
    /// Mapping from external function names to their IDs
    pub(crate) extern_funcs: HashMap<Cow<'static, str>, cranelift_module::FuncId>,
    /// Class metadata: class name -> metadata
    pub(crate) classes: HashMap<String, ClassMeta>,
    /// Enum member values: (enum_name, member_name) -> value
    pub(crate) enums: HashMap<(String, String), EnumMemberValue>,
    /// String literal data: string content -> data ID
    pub(crate) string_data: HashMap<String, cranelift_module::DataId>,
    /// Closure function IDs: closure HIR func_id -> Cranelift func_id
    pub(crate) closure_func_ids: HashMap<u32, cranelift_module::FuncId>,
    /// Set of async function IDs (for proper return type handling)
    pub(crate) async_func_ids: HashSet<u32>,
    /// Set of function IDs that return closures
    pub(crate) closure_returning_funcs: HashSet<u32>,
    /// Wrapper functions for named functions used as callbacks: HIR func_id -> wrapper Cranelift func_id
    pub(crate) func_wrapper_ids: HashMap<u32, cranelift_module::FuncId>,
    /// HIR functions (needed for wrapper generation)
    pub(crate) hir_functions: Vec<Function>,
    /// Function parameter ABI types: func_id -> Vec<abi_type>
    pub(crate) func_param_types: HashMap<u32, Vec<types::Type>>,
    /// Function return ABI types: func_id -> abi_type
    pub(crate) func_return_types: HashMap<u32, types::Type>,
    /// Function HIR return types: func_id -> full HirType (for detecting Map, Set, etc.)
    pub(crate) func_hir_return_types: HashMap<u32, perry_types::Type>,
    /// Rest parameter info: func_id -> index of rest parameter (if any)
    /// The rest parameter collects all arguments from this index onwards into an array
    pub(crate) func_rest_param_index: HashMap<u32, usize>,
    /// Union parameter info: func_id -> Vec<bool> (true if parameter is union type)
    pub(crate) func_union_params: HashMap<u32, Vec<bool>>,
    /// Whether the JS runtime is needed for this module
    pub(crate) needs_js_runtime: bool,
    /// Whether perry-stdlib is needed (controls stdlib function declarations)
    pub(crate) needs_stdlib: bool,
    /// Whether perry/ui is needed (controls UI/system/plugin function declarations)
    pub(crate) needs_ui: bool,
    /// Whether dotenv/config was imported (needs auto-init call)
    pub(crate) needs_dotenv_init: bool,
    /// Whether this is the entry module (should generate main)
    pub(crate) is_entry_module: bool,
    /// Native module init function names to call from main (for entry module)
    pub(crate) native_module_inits: Vec<String>,
    /// JavaScript module specifiers that need to be loaded at runtime
    pub(crate) js_modules: Vec<String>,
    /// Exported native instance data IDs: variable name -> data ID
    pub(crate) exported_native_instance_ids: HashMap<String, cranelift_module::DataId>,
    /// Exported object literal data IDs: variable name -> data ID
    pub(crate) exported_object_ids: HashMap<String, cranelift_module::DataId>,
    /// Exported function data IDs: function name -> (data ID, FuncId)
    /// These are functions that need globals so they can be passed as values to other modules
    pub(crate) exported_function_ids: HashMap<String, (cranelift_module::DataId, u32)>,
    /// Module-level variable data IDs: LocalId -> data ID
    /// These are variables defined at module scope that need to be accessible from functions
    pub(crate) module_var_data_ids: HashMap<LocalId, cranelift_module::DataId>,
    /// Module-level variable info: LocalId -> LocalInfo
    /// Populated during compile_init, used by compile_function for GlobalGet
    pub(crate) module_level_locals: HashMap<LocalId, LocalInfo>,
    /// Imported function parameter counts: function name -> param count
    /// Used to ensure consistent wrapper signatures for functions with optional params
    pub(crate) imported_func_param_counts: HashMap<String, usize>,
    /// Imported function return types: function name -> HIR return type
    /// Used to resolve types for await expressions on cross-module async function calls
    pub(crate) imported_func_return_types: HashMap<String, perry_types::Type>,
    /// Module symbol prefix for scoping cross-module symbols (sanitized module path)
    pub(crate) module_symbol_prefix: String,
    /// Mapping from imported function name -> source module's symbol prefix
    /// Used to construct the correct scoped wrapper name when calling cross-module functions
    pub(crate) import_module_prefixes: HashMap<String, String>,
    /// Maps local import name -> full scoped export name for imports where local != export name.
    /// E.g., `import bs58 from 'bs58'` maps "bs58" -> "__export_{bs58_prefix}__default".
    pub(crate) import_local_to_scoped: HashMap<String, String>,
    /// Pre-declared import wrapper function IDs: unscoped func_name -> (scoped FuncId, param_count)
    /// Populated by pre_declare_import_wrapper before compile_module
    pub(crate) pre_declared_import_wrappers: HashMap<String, (cranelift_module::FuncId, usize)>,
    /// Set of import names that are namespace imports (import * as X from './module')
    /// Used to intercept PropertyGet(ExternFuncRef { name: X }, prop) and resolve prop directly
    pub(crate) namespace_imports: HashSet<String>,
    /// Static fields that need runtime initialization (strings, expressions)
    /// Collected during compile_static_field, processed in compile_init
    pub(crate) static_field_runtime_inits: Vec<(cranelift_module::DataId, Expr)>,
    /// Output type: "executable" (default) or "dylib" (shared library plugin)
    pub(crate) output_type: String,
    /// Bundled extensions: (canonical_source_path, module_prefix) for static plugin registration
    pub(crate) bundled_extensions: Vec<(String, String)>,
    /// External native library FFI function declarations: function_name -> (params, returns)
    pub(crate) native_library_functions: Vec<(String, Vec<String>, String)>,
    /// Compile-time platform ID injected as `__platform__` constant:
    /// 0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux
    pub(crate) compile_target: i64,
    /// Compile-time feature flags. Each entry becomes a `__feature_NAME__` constant (1).
    /// Missing features default to 0. Used for dead-code elimination via `if (__plugins__)`.
    pub(crate) enabled_features: HashSet<String>,
    /// Whether geisterhand is enabled (starts HTTP server on startup)
    pub(crate) needs_geisterhand: bool,
    /// Port for geisterhand HTTP server (default 7676)
    pub(crate) geisterhand_port: u16,
    /// Type alias map: alias name -> resolved type.
    /// Collected from all modules' type_aliases during compile_module,
    /// used in type_to_abi to resolve Named("BlockTag") to its underlying type.
    pub(crate) type_alias_map: HashMap<String, perry_types::Type>,
}

impl Compiler {
    /// Create a new compiler for the host target
    pub fn new(target: Option<&str>) -> Result<Self> {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        // Enable PIC for macOS/iOS compatibility and Windows COFF cross-module references
        flag_builder.set("is_pic", "true").unwrap();
        // Enable maximum optimization
        flag_builder.set("opt_level", "speed").unwrap();
        // Enable register allocation checker to detect regalloc bugs

        let isa = match target {
            Some("ios-simulator") | Some("ios") => {
                // Cross-compile for aarch64-apple-ios (Mach-O)
                let triple = target_lexicon::Triple::from_str("aarch64-apple-ios")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create iOS ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            Some("watchos") | Some("watchos-simulator") => {
                // Cross-compile for aarch64-apple-watchos (Mach-O)
                let triple = target_lexicon::Triple::from_str("aarch64-apple-watchos")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create watchOS ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            Some("tvos") | Some("tvos-simulator") => {
                // Cross-compile for aarch64-apple-tvos (Mach-O)
                let triple = target_lexicon::Triple::from_str("aarch64-apple-tvos")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create tvOS ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            Some("android") => {
                // Cross-compile for aarch64-linux-android (ELF)
                let triple = target_lexicon::Triple::from_str("aarch64-unknown-linux-android")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create Android ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            Some("linux") => {
                // Cross-compile for x86_64-linux (ELF)
                let triple = target_lexicon::Triple::from_str("x86_64-unknown-linux-gnu")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create Linux ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            Some("macos") => {
                // Cross-compile for aarch64-apple-darwin (Mach-O)
                let triple = target_lexicon::Triple::from_str("aarch64-apple-darwin")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create macOS ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            Some("windows") => {
                // Cross-compile for x86_64-windows (PE/COFF)
                let triple = target_lexicon::Triple::from_str("x86_64-pc-windows-msvc")
                    .map_err(|e| anyhow!("Bad triple: {}", e))?;
                let isa_builder = cranelift::codegen::isa::lookup(triple)
                    .map_err(|e| anyhow!("Failed to create Windows ISA: {}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
            _ => {
                // Native host target
                let isa_builder = cranelift_native::builder().map_err(|e| anyhow!("{}", e))?;
                isa_builder
                    .finish(settings::Flags::new(flag_builder))
                    .map_err(|e| anyhow!("{}", e))?
            }
        };

        let builder = ObjectBuilder::new(
            isa,
            "perry_output",
            cranelift_module::default_libcall_names(),
        )?;
        let module = ObjectModule::new(builder);
        let ctx = module.make_context();

        // Determine the compile-time platform constant for __platform__:
        // 0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux, 5 = watchOS, 6 = tvOS
        let compile_target: i64 = match target {
            Some("ios") | Some("ios-simulator") => 1,
            Some("android") => 2,
            Some("watchos") | Some("watchos-simulator") => 5,
            Some("tvos") | Some("tvos-simulator") => 6,
            Some("windows") => 3,
            Some("linux") => 4,
            _ => {
                // Native host target: detect the host OS at Rust compile time via cfg!()
                if cfg!(target_os = "ios")     { 1 }
                else if cfg!(target_os = "linux")   { 4 }
                else if cfg!(target_os = "windows") { 3 }
                else                                { 0 }  // macOS or other → treat as macOS
            }
        };
        // Publish to thread-local so free functions (compile_stmt) can read it
        COMPILE_TARGET.with(|c| c.set(compile_target));

        Ok(Self {
            module,
            ctx,
            func_ctx: FunctionBuilderContext::new(),
            func_ids: HashMap::new(),
            extern_funcs: HashMap::new(),
            classes: HashMap::new(),
            enums: HashMap::new(),
            string_data: HashMap::new(),
            closure_func_ids: HashMap::new(),
            async_func_ids: HashSet::new(),
            closure_returning_funcs: HashSet::new(),
            func_wrapper_ids: HashMap::new(),
            hir_functions: Vec::new(),
            func_param_types: HashMap::new(),
            func_return_types: HashMap::new(),
            func_hir_return_types: HashMap::new(),
            func_rest_param_index: HashMap::new(),
            func_union_params: HashMap::new(),
            needs_js_runtime: false,
            needs_stdlib: true,  // Default to true for backwards compatibility
            needs_ui: false,
            needs_dotenv_init: false,
            is_entry_module: true,  // Default to true for single-module compilation
            native_module_inits: Vec::new(),
            js_modules: Vec::new(),
            exported_native_instance_ids: HashMap::new(),
            exported_object_ids: HashMap::new(),
            exported_function_ids: HashMap::new(),
            module_var_data_ids: HashMap::new(),
            module_level_locals: HashMap::new(),
            imported_func_param_counts: HashMap::new(),
            imported_func_return_types: HashMap::new(),
            module_symbol_prefix: String::new(),
            import_module_prefixes: HashMap::new(),
            import_local_to_scoped: HashMap::new(),
            pre_declared_import_wrappers: HashMap::new(),
            namespace_imports: HashSet::new(),
            static_field_runtime_inits: Vec::new(),
            output_type: "executable".to_string(),
            bundled_extensions: Vec::new(),
            native_library_functions: Vec::new(),
            compile_target,
            enabled_features: HashSet::new(),
            needs_geisterhand: false,
            geisterhand_port: 7676,
            type_alias_map: HashMap::new(),
        })
    }

    /// Set whether the JS runtime is needed for this module
    pub fn set_needs_js_runtime(&mut self, needs: bool) {
        self.needs_js_runtime = needs;
    }

    /// Set whether perry-stdlib is needed
    pub fn set_needs_stdlib(&mut self, needs: bool) {
        self.needs_stdlib = needs;
    }

    /// Set whether perry/ui is needed (controls UI/system/plugin/screen function declarations)
    pub fn set_needs_ui(&mut self, needs: bool) {
        self.needs_ui = needs;
    }

    /// Set whether this is the entry module (generates main function)
    pub fn set_is_entry_module(&mut self, is_entry: bool) {
        self.is_entry_module = is_entry;
    }

    /// Set the output type ("executable" or "dylib")
    pub fn set_output_type(&mut self, output_type: String) {
        self.output_type = output_type;
    }

    /// Add a native module init function to call from main (for entry module)
    pub fn add_native_module_init(&mut self, module_name: String) {
        // Sanitize the module name the same way as in compile_init
        let sanitized_name = module_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        let func_name = format!("_perry_init_{}", sanitized_name);
        self.native_module_inits.push(func_name);
    }

    /// Add a JavaScript module that should be loaded at runtime
    pub fn add_js_module(&mut self, specifier: String) {
        self.js_modules.push(specifier);
    }

    /// Set enabled compile-time feature flags.
    /// Each feature becomes a `__feature_NAME__` constant (1) in TypeScript scope.
    /// The special `__plugins__` constant is derived from the "plugins" feature.
    pub fn set_enabled_features(&mut self, features: Vec<String>) {
        self.enabled_features = features.into_iter().collect();
        // Publish to thread-local so free functions (compile_stmt) can read it
        ENABLED_FEATURES.with(|f| {
            *f.borrow_mut() = self.enabled_features.clone();
        });
    }

    /// Set whether geisterhand (in-process input fuzzer) is enabled
    pub fn set_needs_geisterhand(&mut self, needs: bool) {
        self.needs_geisterhand = needs;
    }

    /// Set the port for geisterhand HTTP server
    pub fn set_geisterhand_port(&mut self, port: u16) {
        self.geisterhand_port = port;
    }

    /// Register a bundled extension for static plugin registration in the entry module init.
    /// `source_path` is the canonical absolute path to the extension entry file.
    /// `module_prefix` is the sanitized module prefix for resolving the extension's default export.
    pub fn add_bundled_extension(&mut self, source_path: String, module_prefix: String) {
        self.bundled_extensions.push((source_path, module_prefix));
    }

    /// Register a type alias from another module.
    /// This allows `type_to_abi` to resolve cross-module type aliases like
    /// `type BlockTag = 'latest' | number | string` when used in function parameters.
    pub fn register_type_alias(&mut self, name: String, ty: perry_types::Type) {
        self.type_alias_map.insert(name, ty);
    }

    /// Register an imported function's parameter count.
    /// This ensures wrapper functions use the correct full signature even when
    /// the function has optional parameters and is called with different arities.
    pub fn register_imported_func_param_count(&mut self, func_name: String, param_count: usize) {
        self.imported_func_param_counts.insert(func_name, param_count);
    }

    /// Register an imported function's return type.
    /// Used to resolve types for await expressions on cross-module async function calls.
    pub fn register_imported_func_return_type(&mut self, func_name: String, return_type: perry_types::Type) {
        self.imported_func_return_types.insert(func_name, return_type);
    }

    /// Register external native library FFI functions from package manifests.
    /// These will be declared as extern imports during codegen initialization.
    pub fn set_native_library_functions(&mut self, functions: Vec<(String, Vec<String>, String)>) {
        self.native_library_functions = functions;
    }

    /// Set the module symbol prefix for scoping cross-module symbols.
    /// This should be the sanitized module path (e.g., "lib_generic_ts").
    pub fn set_module_symbol_prefix(&mut self, prefix: String) {
        self.module_symbol_prefix = prefix;
    }

    /// Register that an imported function name comes from a specific source module.
    /// This allows the compiler to construct the correct scoped wrapper name.
    pub fn register_import_source(&mut self, func_name: String, source_prefix: String) {
        self.import_module_prefixes.insert(func_name, source_prefix);
    }

    /// Construct a module-scoped export global name.
    /// For the exporting module: uses self.module_symbol_prefix.
    pub(crate) fn scoped_export_name(&self, name: &str) -> String {
        if self.module_symbol_prefix.is_empty() {
            format!("__export_{}", name)
        } else {
            format!("__export_{}__{}", self.module_symbol_prefix, name)
        }
    }

    /// Construct a module-scoped wrapper function name.
    /// For the exporting module: uses self.module_symbol_prefix.
    pub(crate) fn scoped_wrapper_name(&self, name: &str) -> String {
        if self.module_symbol_prefix.is_empty() {
            format!("__wrapper_{}", name)
        } else {
            format!("__wrapper_{}__{}", self.module_symbol_prefix, name)
        }
    }

    /// Construct a scoped export name for an imported symbol from another module.
    /// Looks up the source module's prefix from import_module_prefixes.
    pub(crate) fn import_scoped_export_name(&self, name: &str) -> String {
        if let Some(prefix) = self.import_module_prefixes.get(name) {
            format!("__export_{}__{}", prefix, name)
        } else {
            format!("__export_{}", name)
        }
    }

    /// Construct a scoped wrapper name for an imported function from another module.
    pub(crate) fn import_scoped_wrapper_name(&self, name: &str) -> String {
        if let Some(prefix) = self.import_module_prefixes.get(name) {
            format!("__wrapper_{}__{}", prefix, name)
        } else {
            format!("__wrapper_{}", name)
        }
    }

    /// Pre-declare an imported wrapper function with its scoped name.
    /// This is called from compile.rs before compile_module() to set up the import references
    /// with the correct module-scoped symbol names. When compile_expr encounters an ExternFuncRef
    /// call, it checks pre_declared_import_wrappers first before constructing a wrapper name.
    /// Pre-declare an imported wrapper function with its scoped name.
    /// Stores the FuncId in extern_funcs with a `__scoped_wrapper__` prefix key
    /// so compile_expr can find it without needing a new parameter.
    pub fn pre_declare_import_wrapper(&mut self, func_name: &str, source_module_prefix: &str, param_count: usize) -> Result<()> {
        let scoped_name = format!("__wrapper_{}__{}", source_module_prefix, func_name);
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // closure_ptr (ignored)
        for _ in 0..param_count {
            sig.params.push(AbiParam::new(types::F64));
        }
        sig.returns.push(AbiParam::new(types::F64));
        let key: Cow<'static, str> = format!("__scoped_wrapper__{}", func_name).into();
        match self.module.declare_function(&scoped_name, Linkage::Import, &sig) {
            Ok(func_id) => {
                self.extern_funcs.insert(key, func_id);
                // Also store param count for proper argument padding
                self.imported_func_param_counts.entry(func_name.to_string()).or_insert(param_count);
            }
            Err(_) => {
                // Already declared or incompatible - try to find existing
                for (id, decl) in self.module.declarations().get_functions() {
                    if decl.name.as_deref() == Some(&scoped_name) {
                        self.extern_funcs.insert(key, id);
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Register an alias for an already pre-declared import wrapper.
    /// Used when local name differs from export name (e.g., default imports).
    pub fn register_import_wrapper_alias(&mut self, local_name: &str, export_name: &str, param_count: usize) {
        let export_key = format!("__scoped_wrapper__{}", export_name);
        if let Some(&func_id) = self.extern_funcs.get(export_key.as_str()) {
            let local_key: Cow<'static, str> = format!("__scoped_wrapper__{}", local_name).into();
            self.extern_funcs.insert(local_key, func_id);
            self.imported_func_param_counts.entry(local_name.to_string()).or_insert(param_count);
        }
    }

    /// Pre-declare an imported export global with its scoped name.
    pub fn pre_declare_import_export(&mut self, export_name: &str, local_name: &str, source_module_prefix: &str) -> Result<()> {
        let scoped_name = format!("__export_{}__{}", source_module_prefix, export_name);
        let _ = self.module.declare_data(&scoped_name, Linkage::Import, true, false);
        self.import_module_prefixes.insert(export_name.to_string(), source_module_prefix.to_string());
        // When local name differs from export name (e.g., default imports),
        // map the local name directly to the full scoped export symbol name
        if local_name != export_name {
            self.import_module_prefixes.insert(local_name.to_string(), source_module_prefix.to_string());
            self.import_local_to_scoped.insert(local_name.to_string(), scoped_name);
        }
        Ok(())
    }

    /// Register a namespace import name so codegen can intercept PropertyGet on it.
    pub fn register_namespace_import(&mut self, local_name: String) {
        self.namespace_imports.insert(local_name);
    }

    /// Register an imported class from another module.
    /// This declares the class's constructor and methods as imports so they can be called.
    /// The class definition must have been exported from the source module.
    /// If `local_alias` is provided and differs from the class name, the class is also
    /// registered under the alias so it can be found when used with that name in the code.
    /// `source_prefix` is the module symbol prefix of the source module (used to qualify symbols).
    pub fn register_imported_class(&mut self, class: &Class, local_alias: Option<&str>, source_prefix: &str) -> Result<()> {
        // Skip if already registered (e.g., if class is defined locally)
        if self.classes.contains_key(&class.name) {
            // If there's an alias that differs from the class name, also register under the alias
            if let Some(alias) = local_alias {
                if alias != class.name && !self.classes.contains_key(alias) {
                    // Clone the existing metadata for the alias
                    if let Some(existing_meta) = self.classes.get(&class.name).cloned() {
                        self.classes.insert(alias.to_string(), existing_meta);
                    }
                }
            }
            return Ok(());
        }

        // Build field indices and types
        let mut field_indices = HashMap::new();
        let mut field_types = HashMap::new();
        for (i, field) in class.fields.iter().enumerate() {
            field_indices.insert(field.name.clone(), i as u32);
            field_types.insert(field.name.clone(), field.ty.clone());
        }

        // Collect method return types
        let mut method_return_types = HashMap::new();
        for method in &class.methods {
            method_return_types.insert(method.name.clone(), method.return_type.clone());
        }

        // Collect static method return types
        let mut static_method_return_types = HashMap::new();
        for method in &class.static_methods {
            static_method_return_types.insert(method.name.clone(), method.return_type.clone());
        }

        // Extract type parameter names
        let type_params: Vec<String> = class.type_params.iter().map(|tp| tp.name.clone()).collect();

        // Collect field default initializer expressions
        let mut field_inits = HashMap::new();
        for field in &class.fields {
            if let Some(ref init) = field.init {
                field_inits.insert(field.name.clone(), init.clone());
            }
        }

        // Create the class metadata (constructor_id and method_ids will be filled below)
        // Use extends_name for imported classes where extends ClassId may not resolve locally
        self.classes.insert(class.name.clone(), ClassMeta {
            id: class.id,
            parent_class: class.extends_name.clone(),
            native_parent: class.native_extends.clone(),
            own_field_count: class.fields.len() as u32,
            field_count: class.fields.len() as u32,
            field_indices,
            field_types,
            constructor_id: None,
            method_ids: HashMap::new(),
            method_param_counts: HashMap::new(),
            getter_ids: HashMap::new(),
            setter_ids: HashMap::new(),
            static_method_ids: HashMap::new(),
            static_field_ids: HashMap::new(),
            method_return_types,
            static_method_return_types,
            type_params,
            field_inits,
        });

        // Declare the constructor as an import (if present)
        if let Some(ref ctor) = class.constructor {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // 'this' pointer
            for _ in &ctor.params {
                sig.params.push(AbiParam::new(types::F64));
            }
            // Constructor returns void

            let func_name = format!("{}__{}_constructor", source_prefix, class.name);
            let func_id = self.module.declare_function(&func_name, Linkage::Import, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.constructor_id = Some(func_id);
            }
        }

        // Declare methods as imports
        for method in &class.methods {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // 'this' pointer
            for _ in &method.params {
                sig.params.push(AbiParam::new(types::F64));
            }
            sig.returns.push(AbiParam::new(types::F64));

            let func_name = format!("{}__{}_{}",source_prefix, class.name, method.name);
            let func_id = self.module.declare_function(&func_name, Linkage::Import, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.method_ids.insert(method.name.clone(), func_id);
                meta.method_param_counts.insert(method.name.clone(), method.params.len());
            }
        }

        // Declare static methods as imports
        for method in &class.static_methods {
            let mut sig = self.module.make_signature();
            // Static methods do NOT have 'this' pointer
            for _ in &method.params {
                sig.params.push(AbiParam::new(types::F64));
            }
            sig.returns.push(AbiParam::new(types::F64));

            let func_name = format!("{}__{}_{}__static", source_prefix, class.name, method.name);
            let func_id = self.module.declare_function(&func_name, Linkage::Import, &sig)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.static_method_ids.insert(method.name.clone(), func_id);
            }
        }

        // Declare static fields as imports (so cross-module static field access works)
        for field in &class.static_fields {
            let data_name = format!("{}__{}_{}__static_field", source_prefix, class.name, field.name);
            let data_id = self.module.declare_data(&data_name, Linkage::Import, true, false)?;

            if let Some(meta) = self.classes.get_mut(&class.name) {
                meta.static_field_ids.insert(field.name.clone(), data_id);
            }
        }

        // If there's a local alias that differs from the class name, also register under the alias
        // This allows code to use the alias name (e.g., `new Alias()` when import was `{ MyClass as Alias }`)
        if let Some(alias) = local_alias {
            if alias != class.name && !self.classes.contains_key(alias) {
                if let Some(existing_meta) = self.classes.get(&class.name).cloned() {
                    self.classes.insert(alias.to_string(), existing_meta);
                }
            }
        }

        Ok(())
    }

    /// Convert a HIR type to a Cranelift ABI type
    pub(crate) fn type_to_abi(&self, ty: &perry_types::Type) -> types::Type {
        use perry_types::Type;
        match ty {
            // Numbers use f64
            Type::Number | Type::Int32 => types::F64,
            // BigInt uses f64 (NaN-boxed pointer with BIGINT_TAG)
            Type::BigInt => types::F64,
            // Booleans can be f64 (0.0 or 1.0) for simplicity
            Type::Boolean => types::F64,
            // Strings, arrays, objects are pointers (i64)
            Type::String | Type::Array(_) | Type::Object(_) => types::I64,
            // Promises are pointers (i64)
            Type::Promise(_) => types::I64,
            // Named types: most are pointers, but some are stored as f64
            Type::Named(name) => {
                // First check if this is a type alias — resolve to the underlying type.
                // E.g., `type BlockTag = 'latest' | number | string` should resolve to
                // Union([String, Number, String]) -> F64 (NaN-boxed), not I64 (object pointer).
                if let Some(resolved) = self.type_alias_map.get(name) {
                    return self.type_to_abi(resolved);
                }
                match name.as_str() {
                    "Date" => types::F64,  // Date is stored as f64 timestamp in runtime
                    // Fastify handles are NaN-boxed f64 values (not raw I64 pointers).
                    // When passed through closures (js_closure_call2 uses f64 for all args),
                    // they must be declared as f64 in function signatures to avoid ABI mismatch.
                    "FastifyInstance" | "FastifyRequest" | "FastifyReply" => types::F64,
                    _ => {
                        // Check if this is a numeric const enum — its values are inlined
                        // as f64 constants, so the ABI type must be f64, not i64.
                        // String enums are i64 (string pointers).
                        let is_numeric_enum = self.enums.iter().any(|((enum_name, _), _)| enum_name == name)
                            && !self.enums.iter().any(|((enum_name, _), v)| enum_name == name && matches!(v, EnumMemberValue::String(_)));
                        if is_numeric_enum {
                            types::F64
                        } else {
                            types::I64  // Other named types are object pointers
                        }
                    }
                }
            }
            // Generic types are pointers
            Type::Generic { .. } => types::I64,
            // Void/Null/Undefined return f64 (will be 0)
            Type::Void | Type::Null => types::F64,
            // Any/Unknown use f64 (NaN-boxed values for JS interop)
            Type::Any | Type::Unknown => types::F64,
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

    /// Create global data slots for module-level variables
    /// These allow functions to access variables defined in init statements
    pub(crate) fn create_module_var_globals(&mut self, init_stmts: &[Stmt]) -> Result<()> {
        self.create_module_var_globals_recursive(init_stmts)
    }

    /// Recursively create globals for variables in nested statements (for loops, if blocks, etc.)
    pub(crate) fn create_module_var_globals_recursive(&mut self, stmts: &[Stmt]) -> Result<()> {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, name, .. } => {
                    // Create a global data slot for this variable
                    // Each slot holds an f64 (8 bytes)
                    let global_name = format!("__modvar_{}_{}", name, id);
                    let data_id = self.module.declare_data(&global_name, Linkage::Local, true, false)?;
                    let mut data_desc = DataDescription::new();
                    data_desc.define_zeroinit(8); // 8 bytes for f64
                    self.module.define_data(data_id, &data_desc)?;
                    self.module_var_data_ids.insert(*id, data_id);
                }
                Stmt::For { init, body, .. } => {
                    // Walk init statement if present
                    if let Some(init_stmt) = init {
                        self.create_module_var_globals_recursive(&[*init_stmt.clone()])?;
                    }
                    // Walk body statements
                    self.create_module_var_globals_recursive(body)?;
                }
                Stmt::While { body, .. } => {
                    self.create_module_var_globals_recursive(body)?;
                }
                Stmt::If { then_branch, else_branch, .. } => {
                    self.create_module_var_globals_recursive(then_branch)?;
                    if let Some(else_stmts) = else_branch {
                        self.create_module_var_globals_recursive(else_stmts)?;
                    }
                }
                Stmt::Try { body, catch, finally } => {
                    self.create_module_var_globals_recursive(body)?;
                    if let Some(c) = catch {
                        self.create_module_var_globals_recursive(&c.body)?;
                    }
                    if let Some(f) = finally {
                        self.create_module_var_globals_recursive(f)?;
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        self.create_module_var_globals_recursive(&case.body)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Analyze module-level variable types from init statements
    /// This is needed before compile_function to know the types for loading from global slots
    pub(crate) fn analyze_module_var_types(&mut self, init_stmts: &[Stmt]) {
        self.analyze_module_var_types_recursive(init_stmts);
    }

    /// Check if an init expression produces a bigint value (for type analysis)
    fn is_buffer_method_call(init: Option<&Expr>, locals: &HashMap<LocalId, LocalInfo>) -> bool {
        if let Some(Expr::Call { callee, .. }) = init {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if matches!(property.as_str(), "slice" | "subarray") {
                    if let Expr::LocalGet(obj_id) = object.as_ref() {
                        return locals.get(obj_id).map(|i| i.is_buffer).unwrap_or(false);
                    }
                }
            }
        }
        false
    }

    pub(crate) fn is_bigint_init_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::BigInt(_) | Expr::BigIntCoerce(_) => true,
            // new BN(...) produces a BigInt value
            Expr::New { class_name, .. } if class_name == "BN" => true,
            Expr::LocalGet(id) => self.module_level_locals.get(id).map(|i| i.is_bigint).unwrap_or(false),
            Expr::Binary { left, right, .. } => {
                self.is_bigint_init_expr(left) || self.is_bigint_init_expr(right)
            }
            Expr::Unary { operand, .. } => self.is_bigint_init_expr(operand),
            _ => false,
        }
    }

    /// Recursively analyze variable types in nested statements
    pub(crate) fn analyze_module_var_types_recursive(&mut self, stmts: &[Stmt]) {
        use perry_types::Type as HirType;

        for stmt in stmts {
            match stmt {
                Stmt::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        self.analyze_module_var_types_recursive(&[*init_stmt.clone()]);
                    }
                    self.analyze_module_var_types_recursive(body);
                }
                Stmt::While { body, .. } => {
                    self.analyze_module_var_types_recursive(body);
                }
                Stmt::If { then_branch, else_branch, .. } => {
                    self.analyze_module_var_types_recursive(then_branch);
                    if let Some(else_stmts) = else_branch {
                        self.analyze_module_var_types_recursive(else_stmts);
                    }
                }
                Stmt::Try { body, catch, finally } => {
                    self.analyze_module_var_types_recursive(body);
                    if let Some(c) = catch {
                        self.analyze_module_var_types_recursive(&c.body);
                    }
                    if let Some(f) = finally {
                        self.analyze_module_var_types_recursive(f);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        self.analyze_module_var_types_recursive(&case.body);
                    }
                }
                Stmt::Let { id, name, ty, init, mutable, .. } => {
                // Determine if this variable is a pointer type
                // Note: String is NOT included because strings are now NaN-boxed (f64 values)
                let is_pointer = matches!(ty, HirType::Array(_) |
                    HirType::Object(_) | HirType::Named(_) | HirType::Generic { .. } |
                    HirType::Function(_));

                // Also check the init expression type for better inference
                // Note: Expr::String is NOT included because strings are now NaN-boxed (f64 values)
                // Note: Native handle classes (EventEmitter, Decimal, etc.) use f64 (NaN-boxed), not i64 pointers
                let is_pointer_from_init = if let Some(init_expr) = init {
                    match init_expr {
                        Expr::New { class_name, .. } => {
                            // Native handle classes and BN (BigInt) use f64, not i64
                            !matches!(class_name.as_str(),
                                "BN" | "EventEmitter" | "Decimal" | "Big" | "BigNumber" | "LRUCache" | "Command" | "Redis")
                        }
                        Expr::Array(_) | Expr::Object(_) | Expr::ObjectSpread { .. } | Expr::ArraySpread(_) |
                        Expr::Closure { .. } | Expr::MapNew | Expr::MapNewFromArray(_) | Expr::SetNew | Expr::SetNewFromArray(_) |
                        // JS interop expressions return pointers
                        Expr::JsCallFunction { .. } | Expr::JsCallMethod { .. } |
                        Expr::JsGetExport { .. } | Expr::JsNew { .. } |
                        Expr::JsNewFromHandle { .. } | Expr::JsGetProperty { .. } |
                        // Call expressions might return objects/arrays, but only if the
                        // type annotation doesn't indicate a primitive type.
                        // Without this check, `const val: number = someFunc()` would be
                        // incorrectly treated as a pointer, breaking numeric closure returns.
                        Expr::Call { .. } | Expr::NativeMethodCall { .. } => {
                            !matches!(ty, HirType::Number | HirType::Boolean | HirType::String | HirType::BigInt)
                        }
                        _ => false,
                    }
                } else {
                    false
                };

                // Check if NativeMethodCall returns a string
                let is_string_from_native = matches!(init, Some(Expr::NativeMethodCall { module, method, .. })
                    if (module == "path" && matches!(method.as_str(), "dirname" | "basename" | "extname" | "join" | "resolve"))
                       || (module == "fs" && method == "readFileSync")
                       || (module == "uuid" && matches!(method.as_str(), "v4" | "v1" | "v7"))
                       || (module == "crypto" && matches!(method.as_str(), "sha256" | "md5" | "randomUUID" | "hmacSha256"))
                );
                // Check if NativeMethodCall returns a buffer
                let is_buffer_from_native = matches!(init, Some(Expr::NativeMethodCall { module, method, .. })
                    if module == "crypto" && method == "randomBytes"
                ) || matches!(init, Some(Expr::CryptoRandomBytes(_)));
                // Check if Call expression returns a string (e.g., buffer.toString())
                let is_string_from_call = if let Some(Expr::Call { callee, .. }) = init {
                    if let Expr::PropertyGet { object, property } = callee.as_ref() {
                        // buffer.toString() returns string
                        if property == "toString" {
                            if let Expr::LocalGet(obj_id) = object.as_ref() {
                                self.module_level_locals.get(obj_id).map(|i| i.is_buffer).unwrap_or(false)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                let is_string = matches!(ty, HirType::String) || matches!(init, Some(Expr::String(_))) || is_string_from_native || is_string_from_call;
                let is_array_from_call = if let Some(Expr::Call { callee, .. }) = init {
                    if let Expr::PropertyGet { object, property } = callee.as_ref() {
                        // process.argv.slice() returns an array
                        property == "slice" && matches!(object.as_ref(), Expr::ProcessArgv)
                    } else { false }
                } else { false };
                let is_array = matches!(ty, HirType::Array(_)) || is_array_from_call || matches!(init, Some(Expr::Array(_))) || matches!(init, Some(Expr::ArraySpread(_))) || matches!(init, Some(Expr::ProcessArgv));
                let is_closure = matches!(ty, HirType::Function(_)) || matches!(init, Some(Expr::Closure { .. }));
                // Check for buffer expressions
                let is_buffer = matches!(init, Some(Expr::BufferFrom { .. }) | Some(Expr::BufferAlloc { .. }) |
                    Some(Expr::BufferAllocUnsafe(_)) | Some(Expr::BufferConcat(_)) |
                    Some(Expr::BufferSlice { .. }) | Some(Expr::BufferFill { .. }) |
                    Some(Expr::Uint8ArrayNew(_)) | Some(Expr::Uint8ArrayFrom(_)) |
                    Some(Expr::ChildProcessExecSync { .. }))
                    || is_buffer_from_native
                    || matches!(ty, HirType::Named(name) if name == "Uint8Array" || name == "Buffer")
                    // Detect chained buffer methods: new Uint8Array(n).fill(v), Buffer.alloc(n).fill(v)
                    || matches!(init, Some(Expr::Call { callee, .. }) if matches!(callee.as_ref(),
                        Expr::PropertyGet { object, property } if property == "fill" && matches!(object.as_ref(),
                            Expr::Uint8ArrayNew(_) | Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) |
                            Expr::BufferFrom { .. } | Expr::BufferSlice { .. } | Expr::BufferConcat(_)
                        )
                    ))
                    // Detect buffer.slice() / buffer.subarray() returning a buffer
                    || Self::is_buffer_method_call(init.as_ref(), &self.module_level_locals);

                // Track compile-time constant values for const module-level variables.
                // Special case: `declare const __platform__: number` is injected with
                // the compile-time platform ID (0=macOS,1=iOS,2=Android,3=Windows,4=Linux).
                // Special case: `declare const __plugins__: number` gets 1 if "plugins" feature.
                // Special case: `declare const __feature_NAME__: number` gets 1 if feature "NAME".
                let const_value = if !mutable && !is_pointer && !is_string {
                    if name == "__platform__" {
                        Some(self.compile_target as f64)
                    } else if name == "__plugins__" {
                        if self.enabled_features.contains("plugins") { Some(1.0) } else { Some(0.0) }
                    } else if name.starts_with("__feature_") && name.ends_with("__") {
                        let feature_name = &name[10..name.len()-2];
                        if self.enabled_features.contains(feature_name) { Some(1.0) } else { Some(0.0) }
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

                // Extract type arguments from HirType::Generic (e.g., Map<string, PoolData>)
                // or from Expr::New { type_args } init expressions
                let type_args = if let Some(Expr::New { type_args, .. }) = init {
                    type_args.clone()
                } else if let HirType::Generic { type_args, .. } = ty {
                    type_args.clone()
                } else {
                    Vec::new()
                };

                // Also detect Map/Set from type annotation (not just init expression)
                let is_map_from_type = matches!(ty, HirType::Generic { base, .. } if base == "Map");
                let is_set_from_type = matches!(ty, HirType::Generic { base, .. } if base == "Set");

                // Store the type info
                let class_name = resolve_class_name_from_type(ty, &self.classes).or_else(|| {
                    if let Some(Expr::New { class_name, .. }) = init {
                        Some(class_name.clone())
                    } else {
                        None
                    }
                });
                let info = LocalInfo {
                    var: Variable::new(0), // Will be overwritten in compile_function
                    name: Some(name.clone()),
                    class_name,
                    type_args,
                    is_pointer: is_pointer || is_pointer_from_init,
                    is_array,
                    is_string,
                    is_bigint: matches!(ty, HirType::BigInt) || matches!(init, Some(Expr::BigInt(_))) || matches!(init, Some(Expr::BigIntCoerce(_))) || {
                        // Check if init is a LocalGet of a known bigint variable,
                        // or a binary/unary expression involving bigint operands
                        if let Some(init_expr) = init {
                            self.is_bigint_init_expr(init_expr)
                        } else {
                            false
                        }
                    },
                    is_closure, closure_func_id: None,
                    is_boxed: false,
                    is_map: matches!(init, Some(Expr::MapNew) | Some(Expr::MapNewFromArray(_))) || is_map_from_type,
                    is_set: matches!(init, Some(Expr::SetNew) | Some(Expr::SetNewFromArray(_))) || is_set_from_type,
                    is_buffer,
                    is_event_emitter: matches!(init, Some(Expr::New { class_name, .. }) if class_name == "EventEmitter"),
                    // Mark as union only when the concrete type is unknown.
                    // Matches stmt.rs logic: Named/Object/Any/Unknown set is_union UNLESS
                    // expression inference determined a concrete pointer type (array, string,
                    // closure, map, set, buffer, bigint). Without this exclusion, untyped
                    // arrays (ty=Unknown) would get is_union=true, causing cranelift_var_type()
                    // to select F64 while stmt.rs stores them as I64 — a mismatch that
                    // corrupts pointers on ARM FP flush-to-zero platforms (Android).
                    is_union: matches!(ty, HirType::Union(_)) ||
                        (matches!(ty, HirType::Named(_) | HirType::Object(_) | HirType::Any | HirType::Unknown)
                         && !is_array && !is_string && !is_closure
                         && !matches!(init, Some(Expr::MapNew) | Some(Expr::MapNewFromArray(_))) && !is_map_from_type
                         && !matches!(init, Some(Expr::SetNew) | Some(Expr::SetNewFromArray(_))) && !is_set_from_type
                         && !is_buffer
                         && !matches!(ty, HirType::BigInt) && !matches!(init, Some(Expr::BigInt(_))) && !matches!(init, Some(Expr::BigIntCoerce(_)))),
                    is_mixed_array: if let HirType::Array(elem_ty) = ty {
                        // String/union/any arrays need mixed-array access (NaN-boxed elements)
                        matches!(elem_ty.as_ref(), HirType::Union(_) | HirType::Any | HirType::String)
                    } else {
                        false
                    },
                    is_integer: false,
                    is_integer_array: false,
                    is_i32: false,
                    is_boolean: matches!(ty, HirType::Boolean)
                        || matches!(init, Some(Expr::Bool(_)))
                        || matches!(init, Some(Expr::Compare { .. }))
                        || matches!(init, Some(Expr::Unary { op: perry_hir::UnaryOp::Not, .. })),
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None,
                    product_cache: None,
                    cached_array_ptr: None,
                    const_value,
                    hoisted_element_loads: None,
                    hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None, object_field_indices: None,
                };
                if info.is_bigint {
                }
                self.module_level_locals.insert(*id, info);
                }
                _ => {}
            }
        }
    }

    /// Compile a HIR module to an object file
    pub fn compile_module(mut self, hir: &HirModule) -> Result<Vec<u8>> {
        // Set the module symbol prefix from the module name
        self.module_symbol_prefix = hir.name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");

        // Populate thread-local import module prefixes for use by compile_expr
        IMPORT_MODULE_PREFIXES.with(|p| {
            let mut map = p.borrow_mut();
            map.clear();
            for (k, v) in &self.import_module_prefixes {
                map.insert(k.clone(), v.clone());
            }
        });

        // Populate thread-local local-to-scoped name mapping for default imports
        IMPORT_LOCAL_TO_SCOPED.with(|p| {
            let mut map = p.borrow_mut();
            map.clear();
            for (k, v) in &self.import_local_to_scoped {
                map.insert(k.clone(), v.clone());
            }
        });

        // Populate thread-local namespace imports set for use by compile_expr
        NAMESPACE_IMPORTS.with(|p| {
            let mut set = p.borrow_mut();
            set.clear();
            for name in &self.namespace_imports {
                set.insert(name.clone());
            }
        });

        // Populate thread-local imported function return types for use by compile_stmt
        IMPORTED_FUNC_RETURN_TYPES.with(|p| {
            let mut map = p.borrow_mut();
            map.clear();
            for (k, v) in &self.imported_func_return_types {
                map.insert(k.clone(), v.clone());
            }
        });

        // Store HIR functions for wrapper generation
        self.hir_functions = hir.functions.clone();

        // Process classes first to build metadata
        for class in &hir.classes {
            self.process_class(class, &hir.classes)?;
        }

        // Resolve class inheritance (merge parent fields into child classes)
        self.resolve_class_inheritance();

        // Process enums to store their member values
        // Must happen before func_param_types so type_to_abi can detect numeric enums
        for en in &hir.enums {
            self.process_enum(en)?;
        }

        // Collect type aliases from this module into the type alias map.
        // This allows type_to_abi to resolve Named("BlockTag") -> Union([...])
        // so the correct ABI type (F64 for unions) is used for parameters.
        for ta in &hir.type_aliases {
            if ta.type_params.is_empty() {
                self.type_alias_map.insert(ta.name.clone(), ta.ty.clone());
            }
        }

        // Build function parameter and return types map for proper call-site type conversion
        for func in &hir.functions {
            let param_types: Vec<types::Type> = func.params.iter()
                .map(|p| self.type_to_abi(&p.ty))
                .collect();
            self.func_param_types.insert(func.id, param_types);

            // Track return type for correct variable typing when storing call results
            let return_type = if func.is_async {
                types::I64 // Async functions return Promise (i64 pointer)
            } else {
                self.type_to_abi(&func.return_type)
            };
            self.func_return_types.insert(func.id, return_type);

            // Store full HIR return type for detecting Map, Set, etc. at call sites
            self.func_hir_return_types.insert(func.id, func.return_type.clone());

            // Track which parameters are union types (for proper NaN-boxing at call sites).
            // Also resolve type aliases: Named("BlockTag") -> Union([...]) is a union param.
            let union_params: Vec<bool> = func.params.iter()
                .map(|p| {
                    if matches!(p.ty, perry_types::Type::Union(_)) {
                        return true;
                    }
                    // Check if Named type resolves to a union via type alias
                    if let perry_types::Type::Named(name) = &p.ty {
                        if let Some(resolved) = self.type_alias_map.get(name) {
                            return matches!(resolved, perry_types::Type::Union(_));
                        }
                    }
                    false
                })
                .collect();
            self.func_union_params.insert(func.id, union_params);
        }

        // Infer BigInt return types for functions that return new BN(...) in all paths
        // This enables is_bigint detection for variables assigned from these functions
        for func in &hir.functions {
            if matches!(func.return_type, perry_types::Type::Any | perry_types::Type::Unknown) {
                if all_returns_are_bigint(&func.body) {
                    self.func_hir_return_types.insert(func.id, perry_types::Type::BigInt);
                }
            }
        }

        // Check for dotenv/config side-effect import (auto-calls dotenv.config())
        for import in &hir.imports {
            if import.source == "dotenv/config" && import.is_native {
                self.needs_dotenv_init = true;
                break;
            }
        }

        // Identify functions that return closures by scanning their body for return statements
        for func in &hir.functions {
            if self.function_returns_closure(&func.body) {
                self.closure_returning_funcs.insert(func.id);
            }
        }

        // First pass: declare all functions
        for func in &hir.functions {
            self.declare_function(func)?;
        }

        // Declare class constructors and methods
        for class in &hir.classes {
            self.declare_class_constructor(class)?;
            self.declare_class_methods(class)?;
            self.declare_class_getters(class)?;
            self.declare_class_setters(class)?;
            self.declare_static_methods(class)?;
            self.declare_static_fields(class)?;
        }

        // Now that all methods are declared, resolve method inheritance
        self.resolve_method_inheritance();

        // Declare external runtime functions
        self.declare_runtime_functions()?;

        // Map empty-body functions (from `declare function`) to extern FFI functions.
        // When a TypeScript file has `declare function hone_editor_create(...)`, the lowering
        // produces a Function with empty body. If the function name matches an extern FFI
        // function from a native library manifest, remap the func_id to the extern function.
        // Also override func_param_types with the manifest's declared types so that
        // call-site argument coercion (f64→i64 for string pointers) works correctly.
        let native_lib_param_types: HashMap<String, Vec<types::Type>> = self.native_library_functions.iter()
            .map(|(name, params, _)| {
                let types: Vec<types::Type> = params.iter().map(|p| Self::parse_cranelift_type(p)).collect();
                (name.clone(), types)
            })
            .collect();
        for func in &hir.functions {
            if func.body.is_empty() {
                if let Some(extern_id) = self.extern_funcs.get(func.name.as_str()) {
                    self.func_ids.insert(func.id, *extern_id);
                    // Override param types from the native library manifest
                    if let Some(manifest_types) = native_lib_param_types.get(&func.name) {
                        self.func_param_types.insert(func.id, manifest_types.clone());
                    }
                }
            }
        }

        // Collect closures from ALL sources: functions, classes, and init statements
        // This MUST happen BEFORE compiling class methods that may contain closures
        // Tuple: (func_id, params, body, captures, mutable_captures, captures_this, enclosing_class)
        let mut all_closures: Vec<(u32, Vec<perry_hir::Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>, bool, Option<String>, bool)> = Vec::new();

        // Collect from function bodies (no enclosing class)
        for func in &hir.functions {
            self.collect_closures_from_stmts_into(&func.body, &mut all_closures, None);
        }

        // Collect from class methods and constructors (pass class name for this capture)
        for class in &hir.classes {
            let class_name = class.name.as_str();
            for method in &class.methods {
                self.collect_closures_from_stmts_into(&method.body, &mut all_closures, Some(class_name));
            }
            for (_, getter) in &class.getters {
                self.collect_closures_from_stmts_into(&getter.body, &mut all_closures, Some(class_name));
            }
            for (_, setter) in &class.setters {
                self.collect_closures_from_stmts_into(&setter.body, &mut all_closures, Some(class_name));
            }
            for method in &class.static_methods {
                // Static methods don't have `this`, so pass None
                self.collect_closures_from_stmts_into(&method.body, &mut all_closures, None);
            }
            if let Some(ctor) = &class.constructor {
                self.collect_closures_from_stmts_into(&ctor.body, &mut all_closures, Some(class_name));
            }

            // Collect from class field initializers
            for field in &class.fields {
                if let Some(init) = &field.init {
                    self.collect_closures_from_expr(init, &mut all_closures, Some(class_name));
                }
            }

            // Collect from static field initializers (no enclosing class for static context)
            for field in &class.static_fields {
                if let Some(init) = &field.init {
                    self.collect_closures_from_expr(init, &mut all_closures, None);
                }
            }
        }

        // Collect from init statements (no enclosing class)
        self.collect_closures_from_stmts_into(&hir.init, &mut all_closures, None);

        // Collect from global variable initializers
        for global in &hir.globals {
            if let Some(init) = &global.init {
                self.collect_closures_from_expr(init, &mut all_closures, None);
            }
        }

        // Deduplicate closures by func_id (same closure may appear in class method and init statements)
        // Prefer entries with enclosing_class set (from class methods) over those without
        let mut seen_func_ids: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        let mut deduped_closures: Vec<(u32, Vec<perry_hir::Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>, bool, Option<String>, bool)> = Vec::new();
        for closure in all_closures {
            let func_id = closure.0;
            if let Some(&existing_idx) = seen_func_ids.get(&func_id) {
                // If existing has no enclosing_class but this one does, replace it
                if deduped_closures[existing_idx].6.is_none() && closure.6.is_some() {
                    deduped_closures[existing_idx] = closure;
                }
                // Otherwise keep the existing one
            } else {
                seen_func_ids.insert(func_id, deduped_closures.len());
                deduped_closures.push(closure);
            }
        }

        // Declare all closures first, then compile them
        // If captures_this, we need an extra slot for the `this` pointer
        for (func_id, params, _body, captures, _mutable_captures, captures_this, _enclosing_class, is_async) in &deduped_closures {
            let capture_count = if *captures_this { captures.len() + 1 } else { captures.len() };
            self.declare_closure(*func_id, params.len(), capture_count, *is_async)?;
            // Track rest parameter index for closures (before code generation so callers can see it)
            for (i, param) in params.iter().enumerate() {
                if param.is_rest {
                    self.func_rest_param_index.insert(*func_id, i);
                }
            }
        }

        // Collect FuncRef expressions that need closure-compatible wrappers
        // NOTE: This must be done BEFORE compiling closures, as closures may use FuncRefs
        let mut func_refs_needing_wrappers: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        for func in &hir.functions {
            self.collect_func_refs_needing_wrappers_from_stmts(&func.body, &mut func_refs_needing_wrappers);
        }
        for class in &hir.classes {
            for method in &class.methods {
                self.collect_func_refs_needing_wrappers_from_stmts(&method.body, &mut func_refs_needing_wrappers);
            }
            for method in &class.static_methods {
                self.collect_func_refs_needing_wrappers_from_stmts(&method.body, &mut func_refs_needing_wrappers);
            }
            if let Some(ctor) = &class.constructor {
                self.collect_func_refs_needing_wrappers_from_stmts(&ctor.body, &mut func_refs_needing_wrappers);
            }
        }
        self.collect_func_refs_needing_wrappers_from_stmts(&hir.init, &mut func_refs_needing_wrappers);

        // Also collect from closure bodies (closures may contain FuncRefs)
        for (_, _, body, _, _, _, _, _) in &deduped_closures {
            self.collect_func_refs_needing_wrappers_from_stmts(body, &mut func_refs_needing_wrappers);
        }

        // Exported functions also need wrappers so they can be passed as values to other modules
        for (_, func_id) in &hir.exported_functions {
            func_refs_needing_wrappers.insert(*func_id);
        }

        // ALL functions should have wrappers generated so they can be used as values
        // This is necessary because collect_func_refs_from_expr doesn't traverse all expression types
        // and some functions may be passed as values in ways that aren't detected
        for func in &hir.functions {
            func_refs_needing_wrappers.insert(func.id);
        }

        // Generate wrappers for all FuncRefs that need them
        for func_id in &func_refs_needing_wrappers {
            self.get_or_create_func_wrapper(*func_id)?;
        }

        // Create global data slots for module-level variables that may be accessed from functions/methods/closures
        // IMPORTANT: This must be done BEFORE compiling closures/methods so the data IDs are available
        self.create_module_var_globals(&hir.init)?;

        // Pre-compute which module-level variables are pointers
        self.analyze_module_var_types(&hir.init);

        // Also analyze function body variables for closure capture type propagation
        // This populates module_level_locals with type info for function-local variables
        // that may be captured by closures (e.g., bigint variables captured from enclosing functions)
        for func in &hir.functions {
            // Analyze function parameter types FIRST (body may reference params via LocalGet)
            for param in &func.params {
                let is_bigint = matches!(param.ty, perry_types::Type::BigInt);
                let is_string = matches!(param.ty, perry_types::Type::String) || {
                    if let perry_types::Type::Named(name) = &param.ty {
                        self.enums.iter().any(|((enum_name, _), v)| enum_name == name && matches!(v, EnumMemberValue::String(_)))
                    } else if let perry_types::Type::Union(types) = &param.ty {
                        // Union of all strings (e.g. string | string) should be treated as strings
                        !types.is_empty() && types.iter().all(|t| matches!(t, perry_types::Type::String))
                    } else {
                        false
                    }
                };
                let is_array = matches!(param.ty, perry_types::Type::Array(_));
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                // Check if this Named type is a numeric enum (values are f64, not pointers)
                let is_numeric_enum = if let perry_types::Type::Named(name) = &param.ty {
                    self.enums.iter().any(|((en, _), _)| en == name)
                        && !self.enums.iter().any(|((en, _), v)| en == name && matches!(v, EnumMemberValue::String(_)))
                } else { false };
                let is_pointer = !is_numeric_enum && matches!(param.ty, perry_types::Type::String | perry_types::Type::Array(_) |
                    perry_types::Type::Object(_) | perry_types::Type::Named(_) | perry_types::Type::Generic { .. } |
                    perry_types::Type::Function(_));
                let is_map = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Map");
                let is_set = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Set");
                let is_union = !is_numeric_enum && matches!(param.ty, perry_types::Type::Union(_) | perry_types::Type::Named(_) |
                    perry_types::Type::Object(_) | perry_types::Type::Any | perry_types::Type::Unknown);
                // Only insert if not already present (module-level takes precedence)
                if !self.module_level_locals.contains_key(&param.id) {
                    self.module_level_locals.insert(param.id, LocalInfo {
                        var: Variable::new(0),
                        name: Some(param.name.clone()),
                        class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                        type_args: Vec::new(),
                        is_pointer,
                        is_array,
                        is_string,
                        is_bigint,
                        is_closure, closure_func_id: None,
                        is_boxed: false,
                        is_map, is_set,
                        is_buffer: false, is_event_emitter: false, is_union,
                        is_mixed_array: false,
                        is_integer: false, is_integer_array: false,
                        is_i32: false, is_boolean: false, i32_shadow: None,
                        bounded_by_array: None, bounded_by_constant: None,
                        scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None,
                        const_value: None, hoisted_element_loads: None, hoisted_i32_products: None,
                        module_var_data_id: None, class_ref_name: None, object_field_indices: None,
                    });
                }
            }
            // Now analyze function body variables (after params are registered)
            self.analyze_module_var_types_recursive(&func.body);
        }

        // Detect function-local variables captured by inner class methods.
        // When a class is defined inside a function body (e.g., noble-curves' Point class inside edwards()),
        // class method bodies may reference variables from the enclosing function scope via LocalGet(id).
        // These variables don't exist in the class method's locals map. We promote them to module-level
        // data globals so that:
        // 1. The function writes to the data global when defining/setting the variable
        // 2. The class method loads from the data global (via the existing module_var_data_ids loop)
        {
            // Collect all LocalIds referenced in class method/constructor/getter/setter bodies
            let mut class_refs: Vec<LocalId> = Vec::new();
            let mut visited_closures = std::collections::HashSet::new();
            for class in &hir.classes {
                for method in &class.methods {
                    for stmt in &method.body {
                        perry_hir::collect_local_refs_stmt(stmt, &mut class_refs, &mut visited_closures);
                    }
                }
                if let Some(ctor) = &class.constructor {
                    for stmt in &ctor.body {
                        perry_hir::collect_local_refs_stmt(stmt, &mut class_refs, &mut visited_closures);
                    }
                }
                for (_, getter) in &class.getters {
                    for stmt in &getter.body {
                        perry_hir::collect_local_refs_stmt(stmt, &mut class_refs, &mut visited_closures);
                    }
                }
                for (_, setter) in &class.setters {
                    for stmt in &setter.body {
                        perry_hir::collect_local_refs_stmt(stmt, &mut class_refs, &mut visited_closures);
                    }
                }
                for method in &class.static_methods {
                    for stmt in &method.body {
                        perry_hir::collect_local_refs_stmt(stmt, &mut class_refs, &mut visited_closures);
                    }
                }
            }

            // Collect all LocalIds that are class-internal parameters (constructor params,
            // method params, getter/setter params). These should NOT be promoted — they
            // are local to the class method, not outer-scope captures.
            // This is needed because function inlining (e.g., assertEq calls inlined at
            // HIR level) can create LocalIds in function bodies that collide with class
            // constructor param IDs, causing incorrect promotion of unrelated variables.
            let mut class_own_param_ids: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
            for class in &hir.classes {
                if let Some(ctor) = &class.constructor {
                    for p in &ctor.params {
                        class_own_param_ids.insert(p.id);
                    }
                }
                for method in &class.methods {
                    for p in &method.params {
                        class_own_param_ids.insert(p.id);
                    }
                }
                for (_, getter) in &class.getters {
                    for p in &getter.params {
                        class_own_param_ids.insert(p.id);
                    }
                }
                for (_, setter) in &class.setters {
                    for p in &setter.params {
                        class_own_param_ids.insert(p.id);
                    }
                }
                for method in &class.static_methods {
                    for p in &method.params {
                        class_own_param_ids.insert(p.id);
                    }
                }
            }

            // Find which of these references are NOT already module-level variables
            // (i.e., they're function-local variables that need to be promoted)
            let class_ref_set: std::collections::HashSet<LocalId> = class_refs.into_iter().collect();
            let mut promoted_count = 0;
            for id in &class_ref_set {
                if self.module_var_data_ids.contains_key(id) {
                    continue; // Already a module-level variable
                }
                // Skip class-internal parameters — they are NOT outer-scope captures
                if class_own_param_ids.contains(id) {
                    continue;
                }
                // Check if this ID exists in module_level_locals (function params/body vars)
                if !self.module_level_locals.contains_key(id) {
                    continue; // Unknown variable — skip
                }

                // This is a function-local variable referenced by a class method.
                // Promote it to a module-level data global.
                let var_name = self.module_level_locals.get(id)
                    .and_then(|info| info.name.clone())
                    .unwrap_or_else(|| format!("cap_{}", id));
                let global_name = format!("__class_cap_{}_{}", self.module_symbol_prefix, var_name);
                match self.module.declare_data(&global_name, Linkage::Local, true, false) {
                    Ok(data_id) => {
                        let mut data_desc = DataDescription::new();
                        data_desc.define_zeroinit(8);
                        if let Ok(()) = self.module.define_data(data_id, &data_desc) {
                            self.module_var_data_ids.insert(*id, data_id);
                            // Update the module_level_locals entry with the data_id
                            if let Some(info) = self.module_level_locals.get_mut(id) {
                                info.module_var_data_id = Some(data_id);
                            }
                            promoted_count += 1;
                        }
                    }
                    Err(_) => {} // Skip on error
                }
            }
            if promoted_count > 0 {
                eprintln!("[CLASS_CAPTURE] Promoted {} function-local variables to module-level for class method access", promoted_count);
            }
        }

        // Publish module-var data IDs to a thread-local for compile_expr's
        // closure-construction path. This must happen AFTER class capture
        // promotion (which adds entries) and BEFORE any compile_closure /
        // compile_function / compile_class_method / compile_init call (which
        // can build closures whose `captures` list references module-level vars
        // not yet bound as `locals` at the construction site). Without this,
        // the construction silently sets the capture slot to 0.0 and the
        // closure crashes with NULL box pointer the first time it reads it.
        crate::util::MODULE_VAR_DATA_IDS.with(|p| {
            let mut map = p.borrow_mut();
            map.clear();
            for (k, v) in &self.module_var_data_ids {
                map.insert(*k, *v);
            }
        });

        // Now compile closures (after wrappers are created and module vars are registered)
        for (func_id, params, body, captures, mutable_captures, captures_this, enclosing_class, is_async) in deduped_closures {
            self.compile_closure(func_id, &params, &body, &captures, &mutable_captures, captures_this, enclosing_class.as_deref(), is_async)?;
        }

        // Compile class constructors and methods
        for class in &hir.classes {
            if let Some(ref ctor) = class.constructor {
                self.compile_class_constructor(class, ctor)?;
            }
            for method in &class.methods {
                self.compile_class_method(class, method)?;
            }
            // Compile getters
            for (prop_name, getter) in &class.getters {
                self.compile_class_getter(class, prop_name, getter)?;
            }
            // Compile setters
            for (prop_name, setter) in &class.setters {
                self.compile_class_setter(class, prop_name, setter)?;
            }
            // Compile static methods
            for method in &class.static_methods {
                self.compile_static_method(class, method)?;
            }
            // Compile static fields
            for field in &class.static_fields {
                self.compile_static_field(class, field)?;
            }
        }

        // Create exported globals for native instances (e.g., `export const pool = new Pool(...)`)
        // These will be filled in during compile_init and accessed by other modules
        for (export_name, _module_name, _class_name) in &hir.exported_native_instances {
            let global_name = self.scoped_export_name(export_name);
            let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
            // Create a data description with space for one f64 (8 bytes), initialized to 0
            let mut data_desc = DataDescription::new();
            data_desc.define_zeroinit(8);
            self.module.define_data(data_id, &data_desc)?;
            self.exported_native_instance_ids.insert(export_name.clone(), data_id);
        }

        // Create exported globals for object literals (e.g., `export const config = { ... }`)
        // These will be filled in during compile_init and accessed by other modules
        // Skip exports that are already defined as native instances to avoid duplicate definitions
        let native_instance_names: std::collections::HashSet<&String> = hir.exported_native_instances
            .iter()
            .map(|(name, _, _)| name)
            .collect();
        for export_name in &hir.exported_objects {
            // Skip if already defined as a native instance export
            if native_instance_names.contains(export_name) {
                continue;
            }
            let global_name = self.scoped_export_name(export_name);
            let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
            // Create a data description with space for one f64 (8 bytes), initialized to 0
            let mut data_desc = DataDescription::new();
            data_desc.define_zeroinit(8);
            self.module.define_data(data_id, &data_desc)?;
            self.exported_object_ids.insert(export_name.clone(), data_id);
        }

        // Create exported globals for functions (e.g., `export function foo() { ... }`)
        // These allow functions to be passed as values to other modules
        // Deduplicate by name to handle TypeScript function overloads (multiple declarations, one implementation)
        let mut seen_export_func_names = std::collections::HashSet::new();
        for (func_name, func_id) in &hir.exported_functions {
            if !seen_export_func_names.insert(func_name.clone()) {
                continue; // Skip duplicate overload declarations
            }
            let global_name = self.scoped_export_name(func_name);
            let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
            // Create a data description with space for one f64 (8 bytes), initialized to 0
            let mut data_desc = DataDescription::new();
            data_desc.define_zeroinit(8);
            self.module.define_data(data_id, &data_desc)?;
            self.exported_function_ids.insert(func_name.clone(), (data_id, *func_id));
        }

        // Create exported globals for exported classes, interfaces, and type aliases.
        // Importing modules always pre-declare `__export_<module>__<Name>` for every imported symbol.
        // Without a matching definition in the exporting module, MSVC's linker reports LNK2001.
        // These globals are zero-initialized data slots — they exist only to satisfy the linker.
        {
            let mut already_exported: std::collections::HashSet<String> = std::collections::HashSet::new();
            for (name, _, _) in &hir.exported_native_instances {
                already_exported.insert(name.clone());
            }
            for name in &hir.exported_objects {
                already_exported.insert(name.clone());
            }
            for (name, _) in &hir.exported_functions {
                already_exported.insert(name.clone());
            }

            // Exported classes
            for class in &hir.classes {
                if class.is_exported && !already_exported.contains(&class.name) {
                    let global_name = self.scoped_export_name(&class.name);
                    let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
                    let mut data_desc = DataDescription::new();
                    data_desc.define_zeroinit(8);
                    self.module.define_data(data_id, &data_desc)?;
                    already_exported.insert(class.name.clone());
                }
            }

            // Exported interfaces
            for iface in &hir.interfaces {
                if iface.is_exported && !already_exported.contains(&iface.name) {
                    let global_name = self.scoped_export_name(&iface.name);
                    let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
                    let mut data_desc = DataDescription::new();
                    data_desc.define_zeroinit(8);
                    self.module.define_data(data_id, &data_desc)?;
                    already_exported.insert(iface.name.clone());
                }
            }

            // Exported type aliases
            for ta in &hir.type_aliases {
                if ta.is_exported && !already_exported.contains(&ta.name) {
                    let global_name = self.scoped_export_name(&ta.name);
                    let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
                    let mut data_desc = DataDescription::new();
                    data_desc.define_zeroinit(8);
                    self.module.define_data(data_id, &data_desc)?;
                    already_exported.insert(ta.name.clone());
                }
            }

            // Exported enums
            for en in &hir.enums {
                if en.is_exported && !already_exported.contains(&en.name) {
                    let global_name = self.scoped_export_name(&en.name);
                    let data_id = self.module.declare_data(&global_name, Linkage::Export, true, false)?;
                    let mut data_desc = DataDescription::new();
                    data_desc.define_zeroinit(8);
                    self.module.define_data(data_id, &data_desc)?;
                    already_exported.insert(en.name.clone());
                }
            }
        }

        // Second pass: compile all functions
        // Note: create_module_var_globals and analyze_module_var_types were already called
        // before compiling closures/methods above
        // Deduplicate by FuncId (not name) so same-name functions at different scopes each get compiled
        let mut compiled_func_ids = std::collections::HashSet::new();
        for func in &hir.functions {
            if !compiled_func_ids.insert(func.id) {
                continue; // Skip duplicate declarations with the same FuncId
            }
            self.compile_function(func)?;
        }

        // Generate wrapper functions for all exported functions
        // This is necessary because cross-module calls use __wrapper_functionName for uniform ABI
        // Track which wrappers we've generated and their func_ids for aliasing
        let mut wrapper_aliases_needed: Vec<(String, String, cranelift_module::FuncId)> = Vec::new();
        let mut seen_wrapper_names = std::collections::HashSet::new();

        for (export_name, func_id) in &hir.exported_functions {
            if !seen_wrapper_names.insert(export_name.clone()) {
                continue; // Skip duplicate overload declarations
            }
            // This will create the wrapper if it doesn't exist
            match self.get_or_create_func_wrapper(*func_id) {
                Ok(wrapper_id) => {
                    // Check if the exported name differs from the function's actual name
                    // This handles cases like: export const foo = bar;
                    // where bar has __wrapper_bar but we also need __wrapper_foo
                    if let Some(func) = self.hir_functions.iter().find(|f| f.id == *func_id) {
                        if export_name != &func.name {
                            wrapper_aliases_needed.push((
                                export_name.clone(),
                                func.name.clone(),
                                wrapper_id
                            ));
                        }
                    }
                }
                Err(e) => {
                    // This is a real error - wrapper generation failed, which means
                    // cross-module calls to this function will get null/undefined.
                    // Make it visible so users can diagnose issues.
                    eprintln!("[WRAPPER ERROR] Could not create wrapper for exported function '{}': {}", export_name, e);
                    return Err(anyhow!("Failed to create wrapper for exported function '{}': {}", export_name, e));
                }
            }
        }

        // Generate wrapper aliases (trampolines that just call the original wrapper)
        for (alias_name, _original_name, original_wrapper_id) in wrapper_aliases_needed {
            let alias_wrapper_name = if self.module_symbol_prefix.is_empty() {
                format!("__wrapper_{}", alias_name)
            } else {
                format!("__wrapper_{}__{}", self.module_symbol_prefix, alias_name)
            };

            // Get the signature from the original wrapper
            let sig = self.module.declarations().get_function_decl(original_wrapper_id).signature.clone();

            // Declare the alias function
            if let Ok(alias_id) = self.module.declare_function(&alias_wrapper_name, Linkage::Export, &sig) {
                // Build a simple trampoline that tail-calls the original wrapper
                self.ctx.func.signature = sig.clone();
                let mut alias_func_ctx = FunctionBuilderContext::new();

                {
                    let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut alias_func_ctx);
                    let entry_block = builder.create_block();
                    builder.append_block_params_for_function_params(entry_block);
                    builder.switch_to_block(entry_block);
                    builder.seal_block(entry_block);

                    // Get all block params
                    let params: Vec<Value> = builder.block_params(entry_block).to_vec();

                    // Declare the original wrapper function
                    let original_ref = self.module.declare_func_in_func(original_wrapper_id, builder.func);

                    // Call the original wrapper with all args
                    let call = builder.ins().call(original_ref, &params);

                    // Return the result
                    if sig.returns.is_empty() {
                        builder.ins().return_(&[]);
                    } else {
                        let results: Vec<Value> = builder.inst_results(call).to_vec();
                        builder.ins().return_(&results);
                    }

                    builder.finalize();
                }

                if let Err(e) = self.module.define_function(alias_id, &mut self.ctx) {
                    eprintln!("[WRAPPER ALIAS] Failed to define {}: {}", alias_wrapper_name, e);
                    return Err(anyhow!("Failed to define wrapper alias '{}': {}", alias_wrapper_name, e));
                }
                self.module.clear_context(&mut self.ctx);
            }
        }

        // Generate wrapper functions for exported closures (export const fn = () => {})
        // These are stored in exported_objects, not exported_functions, but callers expect
        // __wrapper_functionName to exist for uniform cross-module calling
        self.generate_exported_closure_wrappers(&hir.init, &hir.exported_objects)?;

        // Compile init statements as main (entry) or module init function (non-entry)
        // Non-entry modules always need __perry_init_ generated because the entry module calls it.
        // Entry module always needs main.
        let should_compile_init = true;

        if should_compile_init {
            self.compile_init(&hir.name, &hir.init, &hir.exported_native_instances, &hir.exported_objects, &hir.exported_functions)?;
        }

        // Emit object file
        let product = self.module.finish();
        Ok(product.emit()?)
    }

    pub(crate) fn parse_cranelift_type(type_str: &str) -> types::Type {
        match type_str {
            "i32" => types::I32,
            "i64" => types::I64,
            "f32" => types::F32,
            "f64" => types::F64,
            _ => types::F64, // Default to f64 (NaN-boxed values)
        }
    }

}
