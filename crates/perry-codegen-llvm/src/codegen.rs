//! HIR → LLVM IR compilation entry point.
//!
//! Public contract:
//!
//! ```ignore
//! let opts = CompileOptions { target: None, is_entry_module: true };
//! let object_bytes: Vec<u8> = perry_codegen_llvm::compile_module(&hir, opts)?;
//! ```
//!
//! The returned bytes are a regular object file produced by `clang -c`.
//! Perry's existing linking stage in `crates/perry/src/commands/compile.rs`
//! picks them up identically to the Cranelift output.
//!
//! ## Phase A scope (in progress — primary-backend migration)
//!
//! Building toward feature parity with the Cranelift backend so LLVM can
//! become Perry's primary build platform. See
//! `/Users/amlug/.claude/plans/sorted-noodling-quilt.md` for the full
//! migration plan.
//!
//! Currently supported (Phases 1, 2, 2.1, A-strings):
//!
//! - User functions with typed `double` ABI
//! - Recursive and forward calls via `FuncRef`
//! - If/else, for loops, let, return
//! - Binary arithmetic (add/sub/mul/div/mod) and compare
//! - Update (++/--) and LocalSet
//! - `Date.now()` via `js_date_now`
//! - **String literals** via the hoisted `StringPool` (one allocation per
//!   literal at module init time, registered as a permanent GC root via
//!   `js_gc_register_global_root`; use sites are a single `load`)
//! - `console.log(<expr>)` — uses `js_console_log_number` for static number
//!   literals (optimized path) and `js_console_log_dynamic` for everything
//!   else (NaN-tag dispatch at runtime)
//!
//! Anything else (objects, arrays, classes, closures, async, imports, …)
//! errors with an actionable "Phase X not yet supported" message.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use perry_hir::{Function, Module as HirModule};

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::runtime_decls;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{DOUBLE, I32, I64, LlvmType, PTR, VOID};

/// Options mirrored from the Cranelift backend's setter API.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Target triple override. `None` uses the host default.
    pub target: Option<String>,
    /// Whether this module is the program entry point. When true, codegen
    /// emits a `main` function that calls `js_gc_init`, the string pool
    /// init, every non-entry module's `<prefix>__init`, then the entry
    /// module's own top-level statements.
    pub is_entry_module: bool,
    /// Prefixes of every non-entry module in the program. Only consulted
    /// when `is_entry_module = true` — `main` calls `<prefix>__init` for
    /// each one in order before running its own init statements. The
    /// order matches Perry's existing topological sort (set up by the
    /// CLI driver in `crates/perry/src/commands/compile.rs`).
    pub non_entry_module_prefixes: Vec<String>,
    /// For each imported function name in this module, the prefix of the
    /// source module that exports it. Used by `ExternFuncRef` lowering
    /// in `lower_call` to generate the correct cross-module call to
    /// `perry_fn_<source_prefix>__<funcname>`. Built by the CLI driver
    /// from each module's `hir.imports` table.
    pub import_function_prefixes: std::collections::HashMap<String, String>,
    /// When true, `compile_module` returns the textual LLVM IR (`.ll`)
    /// as bytes instead of invoking `clang -c` to produce an object file.
    /// Used by the bitcode-link path (`PERRY_LLVM_BITCODE_LINK=1`).
    pub emit_ir_only: bool,

    // ── Cross-module import plumbing (mirrors Cranelift setter chain) ──

    /// Locals that are namespace imports (`import * as X from "./mod"`).
    /// Codegen uses this to know that `X.foo()` should be dispatched as
    /// a cross-module call rather than an object method call.
    pub namespace_imports: Vec<String>,
    /// Imported class definitions from other native modules, keyed by
    /// the local alias (or original name when no alias). Each entry
    /// carries the class HIR, the module prefix of its origin, and an
    /// optional local alias.
    pub imported_classes: Vec<ImportedClass>,
    /// Imported enum member lists, keyed by the local name under which
    /// the enum is visible in this module.
    pub imported_enums: Vec<(String, Vec<(String, perry_hir::EnumValue)>)>,
    /// Names of imported functions that are async. Codegen needs this to
    /// wrap calls in the promise machinery.
    pub imported_async_funcs: std::collections::HashSet<String>,
    /// Type alias map (name → Type) aggregated from all modules. Codegen
    /// uses this to resolve `Named` types in function signatures.
    pub type_aliases: std::collections::HashMap<String, perry_types::Type>,
    /// Imported function parameter counts, keyed by function name.
    pub imported_func_param_counts: std::collections::HashMap<String, usize>,
    /// Imported function return types, keyed by local function name.
    pub imported_func_return_types: std::collections::HashMap<String, perry_types::Type>,
}

/// A class imported from another native module.
#[derive(Debug, Clone)]
pub struct ImportedClass {
    /// The class name as exported from its origin module.
    pub name: String,
    /// Optional local alias (`import { Foo as Bar }`).
    pub local_alias: Option<String>,
    /// Symbol prefix of the origin module (for cross-module method calls).
    pub source_prefix: String,
    /// Number of constructor parameters (needed for dispatch).
    pub constructor_param_count: usize,
    /// Method names defined on this class.
    pub method_names: Vec<String>,
    /// Parent class name, if any.
    pub parent_name: Option<String>,
}

/// Cross-module import context, bundled into a single struct to avoid
/// adding five more individual parameters to every compile_* function.
/// Built once in `compile_module` from `CompileOptions`.
pub(crate) struct CrossModuleCtx {
    pub namespace_imports: std::collections::HashSet<String>,
    pub imported_async_funcs: std::collections::HashSet<String>,
    pub type_aliases: std::collections::HashMap<String, perry_types::Type>,
    pub imported_func_param_counts: std::collections::HashMap<String, usize>,
    pub imported_func_return_types: std::collections::HashMap<String, perry_types::Type>,
}

/// Compile a Perry HIR module to an object file via LLVM IR.
pub fn compile_module(hir: &HirModule, opts: CompileOptions) -> Result<Vec<u8>> {
    let triple = opts.target.clone().unwrap_or_else(default_target_triple);

    let mut llmod = LlModule::new(&triple);
    runtime_decls::declare_phase1(&mut llmod);

    // Phase F.1: derive a per-module symbol prefix from the HIR module
    // name. Mirrors `perry-codegen` (Cranelift):
    //
    //     self.module_symbol_prefix = hir.name.replace(|c: char|
    //         !c.is_alphanumeric() && c != '_', "_");
    //
    // Every emitted symbol that could collide across modules
    // (user functions, class methods, string pool globals, handle slots,
    // module-level globals) gets prefixed with this. The entry module's
    // `main` is the only globally-named symbol — non-entry modules emit
    // `<prefix>__init` instead.
    let module_prefix = sanitize(&hir.name);

    // Imports are no longer a hard error — Phase F.1 supports multi-
    // module compilation. Cross-module function CALLS via ExternFuncRef
    // still land in Phase F.2; for now they'll error at the use site
    // with a specific message.

    // Phase C.2: classes (and inheritance!) are supported. Perry's HIR
    // lowering aggressively pre-resolves both methods and super calls
    // into inline statements at the constructor/method body, so the
    // LLVM codegen mostly sees a flat object-allocation + field-set
    // pattern. We let everything through and let the expression-level
    // codegen error at any specific construct it doesn't know how to
    // handle.

    // Module-wide string literal pool. Owned by the codegen so that
    // `compile_function` and `compile_main` can take split borrows of
    // (&mut LlFunction, &mut StringPool) without confusing the borrow
    // checker — the pool lives outside LlModule. The module prefix
    // becomes part of every emitted global so multi-module programs
    // don't collide on `.str.0.handle`.
    let mut strings = StringPool::with_prefix(module_prefix.clone());

    // Class lookup table for `Expr::New`. Indexed by class name —
    // the HIR has unique names per module.
    let mut class_table: HashMap<String, &perry_hir::Class> = hir
        .classes
        .iter()
        .map(|c| (c.name.clone(), c))
        .collect();

    // Class id assignment: each user class gets an integer id
    // starting at 1 (0 is reserved for anonymous object literals).
    // Used by lower_new to tag the object header so virtual
    // dispatch and instanceof can read the actual class at runtime.
    let mut class_ids: HashMap<String, u32> = hir
        .classes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), (i as u32) + 1))
        .collect();

    // Enum lookup table for `Expr::EnumMember`. Each (enum_name,
    // member_name) maps to its EnumValue, which the codegen lowers
    // to either a numeric or string constant. Built once here.
    let mut enum_table: HashMap<(String, String), perry_hir::EnumValue> = hir
        .enums
        .iter()
        .flat_map(|e| {
            e.members
                .iter()
                .map(move |m| ((e.name.clone(), m.name.clone()), m.value.clone()))
        })
        .collect();

    // ── Phase F: merge imported cross-module definitions ──────────
    //
    // Imported enums: add their members to the enum_table so
    // `Expr::EnumMember` can resolve them in this module.
    for (enum_name, members) in &opts.imported_enums {
        for (member_name, value) in members {
            enum_table
                .entry((enum_name.clone(), member_name.clone()))
                .or_insert_with(|| value.clone());
        }
    }

    // Imported classes: build lightweight stub `Class` objects so the
    // codegen dispatch tables (class_table, method_names, class_ids)
    // can resolve cross-module class method calls. The actual method
    // bodies live in the other module's .o — here we only need the
    // metadata for dispatch and the extern LLVM declarations for the
    // linker.
    let mut imported_class_stubs: Vec<perry_hir::Class> = Vec::new();
    let next_class_id = (hir.classes.len() as u32) + 1;
    for (idx, ic) in opts.imported_classes.iter().enumerate() {
        let class_id = next_class_id + (idx as u32);
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);

        // Skip if already defined locally (local definition takes precedence).
        if class_table.contains_key(effective_name) {
            continue;
        }

        // Assign a class id for dispatch / instanceof.
        class_ids.insert(effective_name.to_string(), class_id);
        // Also register the canonical name if aliased.
        if ic.local_alias.is_some() && !class_ids.contains_key(&ic.name) {
            class_ids.insert(ic.name.clone(), class_id);
        }

        // Build a stub Class with the minimum fields the codegen needs.
        // Most fields are empty — only name, extends_name, and methods
        // are consulted by dispatch.
        let stub = perry_hir::Class {
            id: 0, // imported — no local ClassId
            name: effective_name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: ic.parent_name.clone(),
            native_extends: None,
            fields: Vec::new(),
            constructor: None,
            methods: ic.method_names.iter().map(|m| perry_hir::Function {
                id: 0,
                name: m.clone(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: perry_types::Type::Any,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
            }).collect(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            is_exported: false,
        };
        imported_class_stubs.push(stub);
    }
    // Add imported class stubs to the class_table (references into the
    // Vec we just built — the Vec lives for the remainder of compile_module).
    for stub in &imported_class_stubs {
        class_table.entry(stub.name.clone()).or_insert(stub);
    }

    // Build the cross-module context bundle from CompileOptions.
    let cross_module = CrossModuleCtx {
        namespace_imports: opts.namespace_imports.iter().cloned().collect(),
        imported_async_funcs: opts.imported_async_funcs,
        type_aliases: opts.type_aliases,
        imported_func_param_counts: opts.imported_func_param_counts,
        imported_func_return_types: opts.imported_func_return_types,
    };

    // Module-level globals registry. Pre-walk:
    //   1. Collect every LocalId referenced from any function or method
    //      body (LocalGet / LocalSet / Update). Those that aren't a
    //      function/method's own param or Let must be module-level.
    //   2. Walk hir.init's top-level Lets and globalize ONLY the ones in
    //      that set. Lets that are only referenced from main itself stay
    //      as cheap stack alloca (preserves perf for the bench
    //      benchmarks that don't share state with helper functions).
    let mut referenced_from_fn: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // Helper that handles "params + lets define a scope, refs minus
    // defines flow out". Used for every function/method/closure body.
    let scan_body = |params: &[perry_hir::Param],
                     body: &[perry_hir::Stmt],
                     out: &mut std::collections::HashSet<u32>| {
        let mut local_defs: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        collect_let_ids(body, &mut local_defs);
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut refs);
        for r in refs {
            if !local_defs.contains(&r) {
                out.insert(r);
            }
        }
    };
    for f in &hir.functions {
        scan_body(&f.params, &f.body, &mut referenced_from_fn);
    }
    for c in &hir.classes {
        for m in &c.methods {
            scan_body(&m.params, &m.body, &mut referenced_from_fn);
        }
        if let Some(ctor) = &c.constructor {
            scan_body(&ctor.params, &ctor.body, &mut referenced_from_fn);
        }
    }
    // Also walk every closure body. A self-referencing recursive
    // closure (`let f = (n) => f(n-1)`) needs `f` to be globalized
    // so the closure body can see the live storage instead of a
    // stale snapshot. Without this, the closure auto-capture sees
    // `f` is not yet declared and bails with "local not in scope".
    {
        let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
        let mut seen: std::collections::HashSet<perry_types::FuncId> = std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
        for (_, closure_expr) in &closures {
            if let perry_hir::Expr::Closure { params, body, .. } = closure_expr {
                scan_body(params, body, &mut referenced_from_fn);
            }
        }
    }

    let mut module_globals: HashMap<u32, String> = HashMap::new();
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, .. } = s {
            if referenced_from_fn.contains(id) {
                let name = format!("perry_global_{}__{}", module_prefix, id);
                llmod.add_internal_global(&name, DOUBLE, "0.0");
                module_globals.insert(*id, name);
            }
        }
    }

    // Phase E: register and emit static class fields as module globals.
    // Each `static foo: T = init` becomes `@perry_static_<modprefix>__
    // <class>__<field>` initialized to 0.0. The init expression runs
    // in compile_module_entry's main/init function before user code.
    let mut static_field_globals: HashMap<(String, String), String> = HashMap::new();
    for c in &hir.classes {
        for sf in &c.static_fields {
            let name = format!(
                "perry_static_{}__{}__{}",
                module_prefix,
                sanitize(&c.name),
                sanitize(&sf.name),
            );
            llmod.add_internal_global(&name, DOUBLE, "0.0");
            static_field_globals.insert((c.name.clone(), sf.name.clone()), name);
        }
    }

    // Method registry: (class_name, method_name) → LLVM function name.
    // Built from `class.methods` so the dispatch in `lower_call` knows
    // which mangled function name to call for `obj.method(args)`. Method
    // names are also scoped by module prefix.
    let mut method_names: HashMap<(String, String), String> = HashMap::new();
    for c in &hir.classes {
        for m in &c.methods {
            method_names.insert(
                (c.name.clone(), m.name.clone()),
                scoped_method_name(&module_prefix, &c.name, &m.name),
            );
        }
        // Getters: register under the property name with a `__get_`
        // prefix to avoid colliding with a regular method of the same
        // name. The dispatch site for `obj.prop` checks the getter
        // map first, then falls back to the regular method registry.
        for (prop, f) in &c.getters {
            method_names.insert(
                (c.name.clone(), format!("__get_{}", prop)),
                scoped_method_name(&module_prefix, &c.name, &format!("__get_{}", f.name)),
            );
        }
        for (prop, f) in &c.setters {
            method_names.insert(
                (c.name.clone(), format!("__set_{}", prop)),
                scoped_method_name(&module_prefix, &c.name, &format!("__set_{}", f.name)),
            );
        }
        // Static methods. Registered under their plain method name
        // so `Counter.increment()` (StaticMethodCall) can look them
        // up the same way as instance methods, but emitted as
        // `perry_static_<modprefix>__<class>__<method>` (no `this`).
        // The class/method names are sanitized so private methods
        // (`#helper`) produce a valid LLVM identifier.
        for sm in &c.static_methods {
            method_names.insert(
                (c.name.clone(), sm.name.clone()),
                format!(
                    "perry_static_{}__{}__{}",
                    module_prefix,
                    sanitize(&c.name),
                    sanitize(&sm.name),
                ),
            );
        }
    }

    // Phase F: register imported class methods in the method_names
    // registry and pre-declare them as extern LLVM functions so the
    // linker can resolve cross-module method calls.
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip if locally defined — local methods take precedence.
        if hir.classes.iter().any(|c| c.name == *effective_name) {
            continue;
        }
        let src = &ic.source_prefix;

        for method_name in &ic.method_names {
            // The source module emitted its methods as
            // `perry_method_<source_prefix>__<class>__<method>`.
            // Use the canonical class name (ic.name) for the symbol
            // since that's how the source module mangled it.
            let llvm_fn = format!(
                "perry_method_{}__{}__{}",
                sanitize(src),
                sanitize(&ic.name),
                sanitize(method_name),
            );
            method_names
                .entry((effective_name.to_string(), method_name.clone()))
                .or_insert_with(|| llvm_fn.clone());

            // Declare extern: double method(double this, double arg0, …).
            // We don't know the exact param count of each method from the
            // ImportedClass metadata (only method_names), so declare with
            // a variadic-safe 6-arg signature. The LLVM IR `declare` is
            // just a prototype — it only matters that the symbol exists
            // and the return type is correct. At the call site, only the
            // actual args are passed.
            // For methods, signature is: double(double_this, ..args)
            // We declare conservatively with just (double) → double;
            // extra args at call sites are fine because LLVM validates
            // the callsite against the number of args it actually passes.
            // Actually, LLVM requires exact match. We don't know the
            // arity, so declare with 6 params as a safe upper bound.
            // Call sites with fewer args work because LLVM only checks
            // at the indirect call site. For direct calls, the linker
            // resolves regardless.
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat(DOUBLE).take(6).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }

        // Constructor: declared as
        // `<source_prefix>__<class>_constructor(i64 this, double arg0, …) → void`
        // matching the Cranelift convention.
        let ctor_fn = format!(
            "{}__{}_constructor",
            sanitize(src),
            sanitize(&ic.name),
        );
        let mut ctor_params: Vec<crate::types::LlvmType> = vec![I64];
        for _ in 0..ic.constructor_param_count {
            ctor_params.push(DOUBLE);
        }
        llmod.declare_function(&ctor_fn, VOID, &ctor_params);
    }

    // Resolve user function names up-front so body lowering can emit
    // forward/recursive calls without worrying about emission order.
    // Names are scoped by module prefix to avoid cross-module collisions.
    let mut func_names: HashMap<u32, String> = HashMap::new();
    let mut func_signatures: HashMap<u32, (usize, bool)> = HashMap::new();
    for f in &hir.functions {
        func_names.insert(f.id, scoped_fn_name(&module_prefix, &f.name));
        let has_rest = f.params.iter().any(|p| p.is_rest);
        func_signatures.insert(f.id, (f.params.len(), has_rest));
    }

    // Module-level boxed_vars: union of every per-function/method/
    // closure/module-init boxed set. We compute this once here because
    // closures emitted in `compile_closure` need to know whether their
    // transitively-captured ids from an enclosing function were boxed
    // at the creation site. Since HIR LocalIds are globally unique
    // across the module, a single union set is enough: each id either
    // lives in a box or it doesn't, irrespective of which function
    // owns it.
    let mut module_boxed_vars: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    for f in &hir.functions {
        module_boxed_vars.extend(collect_boxed_vars(&f.body));
    }
    for c in &hir.classes {
        for m in &c.methods {
            module_boxed_vars.extend(collect_boxed_vars(&m.body));
        }
        if let Some(ctor) = &c.constructor {
            module_boxed_vars.extend(collect_boxed_vars(&ctor.body));
        }
    }
    module_boxed_vars.extend(collect_boxed_vars(&hir.init));

    // Module-wide LocalId → Type map. Used by closure bodies to
    // learn the types of captured vars from the enclosing scope.
    // HIR LocalIds are globally unique within the module, so a
    // single flat map works.
    let mut module_local_types: HashMap<u32, perry_types::Type> = HashMap::new();
    collect_let_types_in_stmts(&hir.init, &mut module_local_types);
    for f in &hir.functions {
        for p in &f.params {
            module_local_types.insert(p.id, p.ty.clone());
        }
        collect_let_types_in_stmts(&f.body, &mut module_local_types);
    }
    for c in &hir.classes {
        for m in &c.methods {
            for p in &m.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&m.body, &mut module_local_types);
        }
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&ctor.body, &mut module_local_types);
        }
        for sm in &c.static_methods {
            for p in &sm.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&sm.body, &mut module_local_types);
        }
    }

    // Pre-declare each imported function as an extern. Cross-module
    // calls in lower_call need a `declare` line at the top of the IR
    // for the symbol to be referenceable; without this, clang errors
    // with "use of undefined value @perry_fn_<src>__<name>".
    //
    // We walk hir.functions/methods/init for `Expr::ExternFuncRef` and
    // for each unique (name, source_prefix) emit a declare with the
    // right number of double parameters from the carried param_types.
    {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut collected: Vec<(String, usize)> = Vec::new();
        for f in &hir.functions {
            collect_extern_func_refs_in_stmts(&f.body, &mut seen, &mut collected);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_extern_func_refs_in_stmts(&m.body, &mut seen, &mut collected);
            }
            if let Some(ctor) = &c.constructor {
                collect_extern_func_refs_in_stmts(&ctor.body, &mut seen, &mut collected);
            }
        }
        collect_extern_func_refs_in_stmts(&hir.init, &mut seen, &mut collected);

        for (name, param_count) in collected {
            if let Some(source_prefix) = opts.import_function_prefixes.get(&name) {
                let llvm_name = format!("perry_fn_{}__{}", source_prefix, name);
                let param_types: Vec<crate::types::LlvmType> =
                    std::iter::repeat(DOUBLE).take(param_count).collect();
                llmod.declare_function(&llvm_name, DOUBLE, &param_types);
            }
        }
    }

    // Pre-walk for closures: every `Expr::Closure` in the program needs
    // its body emitted as a top-level LLVM function so the closure
    // creation site can take its address. Collect them all first, then
    // emit each via `compile_closure` (Phase D.1).
    let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
    {
        let mut seen: std::collections::HashSet<perry_types::FuncId> = std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
    }

    // Build closure rest param index: for each closure that has a rest
    // parameter, record its func_id → rest param position. Used by
    // the closure call site in `lower_call` to bundle trailing args.
    let closure_rest_params: HashMap<u32, usize> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                params.iter().position(|p| p.is_rest).map(|idx| (*fid, idx))
            } else {
                None
            }
        })
        .collect();

    // Lower each user function into the module.
    for f in &hir.functions {
        compile_function(&mut llmod, f, &func_names, &mut strings, &class_table, &method_names, &module_globals, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
            .with_context(|| format!("lowering function '{}'", f.name))?;
    }

    // Lower each closure body as a top-level LLVM function.
    for (func_id, closure_expr) in &closures {
        compile_closure(
            &mut llmod,
            *func_id,
            closure_expr,
            &func_names,
            &mut strings,
            &class_table,
            &method_names,
            &module_globals,
            &opts.import_function_prefixes,
            &enum_table,
            &static_field_globals,
            &class_ids,
            &func_signatures,
            &module_prefix,
            &module_boxed_vars,
            &module_local_types,
            &closure_rest_params,
            &cross_module,
        )
        .with_context(|| format!("lowering closure func_id={}", func_id))?;
    }

    // Lower each class method as `perry_method_<modprefix>__<class>__<name>(
    // this_box, arg0, arg1, ...) -> double`. Methods are emitted as
    // standalone LLVM functions; the dispatch in `lower_call` calls
    // them directly.
    for class in &hir.classes {
        for method in &class.methods {
            compile_method(&mut llmod, class, method, &func_names, &mut strings, &class_table, &method_names, &module_globals, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
                .with_context(|| format!("lowering method '{}::{}'", class.name, method.name))?;
        }
        // Getters and setters are also methods, just registered under
        // a __get_/__set_ prefix in the registry. Emit their bodies
        // with the same prefix as the LLVM function name.
        for (prop, getter_fn) in &class.getters {
            let mut renamed = getter_fn.clone();
            renamed.name = format!("__get_{}", prop);
            compile_method(&mut llmod, class, &renamed, &func_names, &mut strings, &class_table, &method_names, &module_globals, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
                .with_context(|| format!("lowering getter '{}::{}'", class.name, prop))?;
        }
        for (prop, setter_fn) in &class.setters {
            let mut renamed = setter_fn.clone();
            renamed.name = format!("__set_{}", prop);
            compile_method(&mut llmod, class, &renamed, &func_names, &mut strings, &class_table, &method_names, &module_globals, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
                .with_context(|| format!("lowering setter '{}::{}'", class.name, prop))?;
        }
        // Static methods compile as plain functions named
        // `perry_static_<modprefix>__<class>__<method>` — no `this`
        // parameter, no class_stack push.
        for sm in &class.static_methods {
            compile_static_method(
                &mut llmod,
                &class.name,
                sm,
                &func_names,
                &mut strings,
                &class_table,
                &method_names,
                &module_globals,
                &opts.import_function_prefixes,
                &enum_table,
                &static_field_globals,
                &class_ids,
                &func_signatures,
                &module_prefix,
                &module_boxed_vars,
                &closure_rest_params,
                &cross_module,
            )
            .with_context(|| format!("lowering static method '{}::{}'", class.name, sm.name))?;
        }
    }

    // Emit FuncRef-as-value wrappers. For each user function, generate
    // a thin wrapper `__perry_wrap_<name>` whose signature matches the
    // closure-call ABI: `double(i64 this_closure, double arg0, double
    // arg1, ...)`. The wrapper discards the closure pointer and forwards
    // the args to the underlying function.
    //
    // The wrapper exists so that `apply(add, 3, 4)` can pass `add` as
    // a value and have `apply` call it via `js_closure_call2`. Without
    // a wrapper, the closure call would invoke `add(closure, 3, 4)`
    // (wrong calling convention) instead of `add(3, 4)`.
    //
    // Wrappers are emitted unconditionally for every user function;
    // dead-code elimination at link time will remove unused ones.
    for f in &hir.functions {
        let original_name = func_names.get(&f.id).cloned().unwrap();
        // Wrapper signature: i64 closure_ptr + N doubles for args.
        // Cap at 5 since js_closure_call only goes up to 5 args.
        let arity = f.params.len().min(5);
        let mut wrap_params: Vec<(LlvmType, String)> =
            vec![(I64, "%this_closure".to_string())];
        for i in 0..arity {
            wrap_params.push((DOUBLE, format!("%a{}", i)));
        }
        let wrap_name = format!("__perry_wrap_{}", original_name);
        let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // Call the underlying function with just the arg doubles.
        let call_args: Vec<(LlvmType, &str)> = (0..arity)
            .map(|i| (DOUBLE, if i == 0 { "%a0" } else if i == 1 { "%a1" }
                else if i == 2 { "%a2" } else if i == 3 { "%a3" } else { "%a4" }))
            .collect();
        let result = blk.call(DOUBLE, &original_name, &call_args);
        blk.ret(DOUBLE, &result);
    }

    // Emit either `int main()` (entry module) or `void <prefix>__init()`
    // (non-entry module). The entry main calls each non-entry init in
    // order before running its own statements.
    compile_module_entry(
        &mut llmod,
        hir,
        &func_names,
        &mut strings,
        &class_table,
        &method_names,
        &module_globals,
        &opts.import_function_prefixes,
        &enum_table,
        &static_field_globals,
        &class_ids,
        &func_signatures,
        &module_prefix,
        opts.is_entry_module,
        &opts.non_entry_module_prefixes,
        &module_boxed_vars,
        &closure_rest_params,
        &cross_module,
    )
    .with_context(|| format!("lowering entry of module '{}'", hir.name))?;

    // After all user code is lowered, the string pool's contents are final.
    // Emit the bytes globals, handle globals, and the
    // `__perry_init_strings_<prefix>` function that runs once at startup.
    // The function name is scoped by module prefix so multiple modules
    // can each have their own string-pool init without colliding.
    emit_string_pool(&mut llmod, &strings, &module_prefix);

    let ll_text = llmod.to_ir();
    log::debug!(
        "perry-codegen-llvm: emitted {} bytes of LLVM IR for '{}' ({} interned strings)",
        ll_text.len(),
        hir.name,
        strings.len()
    );
    if opts.emit_ir_only {
        Ok(ll_text.into_bytes())
    } else {
        crate::linker::compile_ll_to_object(&ll_text, opts.target.as_deref())
    }
}

/// Compile a single user function into the module.
fn compile_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool)>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;

    // Phase A assumes all user-function params are `double`. Parameter
    // registers are named `%arg{LocalId}` so the body can store them into
    // alloca slots keyed by the same HIR LocalId.
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    // Store each param into an alloca slot, collecting LocalId → slot
    // mappings. We release the &mut LlBlock at scope end before handing
    // the function over to the FnCtx lowering pass.
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in &f.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    // Param types feed local_types so type-aware dispatch (e.g. string
    // concat detection on a `: string` parameter) works inside the body.
    let local_types: HashMap<u32, perry_types::Type> = f
        .params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .collect();

    // Pre-walk: which locals need to be boxed? A local is boxed when
    // it's captured by a closure AND written by someone (either the
    // enclosing function or inside a closure). Box-backing lets multiple
    // closures share the same mutable cell — critical for the common
    // `let x = 0; return { get: () => x, set: (n) => x = n }` pattern.
    let boxed_vars = module_boxed_vars.clone();

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack: Vec::new(),
        class_stack: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        static_field_globals,
        class_ids,
        func_signatures,
        boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
    };
    stmt::lower_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of '{}'", f.name))?;

    // Defensive: a well-typed numeric function always returns via an
    // explicit `return`, but we emit `ret double 0.0` as a fallback so
    // the LLVM verifier doesn't reject a missing terminator. For
    // async functions, the fallback also wraps in a resolved promise
    // so callers can await the result.
    if !ctx.block().is_terminated() {
        if f.is_async {
            let zero = "0.0".to_string();
            let handle = ctx.block().call(I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, "0.0");
        }
    }
    Ok(())
}

/// Compile a closure body as a top-level LLVM function.
///
/// Signature: `double perry_closure_<modprefix>__<func_id>(i64 this_closure,
/// double arg0, double arg1, …)`. The first parameter is the closure
/// pointer (raw i64); the remaining params are the closure's own
/// declared parameters.
///
/// Inside the body, captured variables (`closure.captures`) are mapped
/// to capture indices and accessed via the runtime
/// `js_closure_get/set_capture_f64(this_closure, idx)` calls. The
/// `closure_captures` field on `FnCtx` carries the LocalId → capture
/// index map; `current_closure_ptr` carries the closure pointer SSA
/// value name.
#[allow(clippy::too_many_arguments)]
fn compile_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool)>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    module_local_types: &HashMap<u32, perry_types::Type>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    // Destructure the closure expression. We trust that the caller
    // passes only `Expr::Closure` here (from `collect_closures_*`).
    let (params, body, captures, captures_this, enclosing_class) = match closure_expr {
        perry_hir::Expr::Closure {
            params,
            body,
            captures,
            captures_this,
            enclosing_class,
            ..
        } => (params, body, captures, *captures_this, enclosing_class.clone()),
        _ => return Err(anyhow!("compile_closure: expected Expr::Closure")),
    };

    let llvm_name = format!("perry_closure_{}__{}", module_prefix, func_id);

    // Param list: i64 this_closure, then each param as double.
    let mut llvm_params: Vec<(LlvmType, String)> =
        Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    for p in params {
        llvm_params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let lf = llmod.define_function(&llvm_name, DOUBLE, llvm_params);
    let _ = lf.create_block("entry");

    // Allocate slots for the closure's own params (captures don't get
    // alloca slots — they're accessed via the runtime).
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    // Start with the closure's own params as local_types, then
    // merge in the module-wide map so captured-from-outer ids have
    // their types available inside the body. Without this, closures
    // that capture an array `items` and do `items.length` miss the
    // typed fast path and return undefined.
    let mut local_types: HashMap<u32, perry_types::Type> = params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .collect();
    for (id, ty) in module_local_types.iter() {
        local_types.entry(*id).or_insert_with(|| ty.clone());
    }

    // Build the capture map: each captured LocalId gets the index it
    // occupies in the closure's capture array. Identical logic to the
    // `compute_auto_captures` helper used by the closure creation site
    // — they MUST agree on the slot indices, otherwise the body reads
    // captures from the wrong slots. Sorting the auto-detected ids
    // gives deterministic indexing across both call sites.
    let mut auto_captures: Vec<u32> = captures.clone();
    {
        let mut referenced: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut referenced);
        let mut inner_lets: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_let_ids(body, &mut inner_lets);
        let param_ids: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        let already: std::collections::HashSet<u32> = auto_captures.iter().copied().collect();
        let mut sorted: Vec<u32> = referenced.into_iter().collect();
        sorted.sort();
        for id in sorted {
            if !param_ids.contains(&id)
                && !inner_lets.contains(&id)
                && !already.contains(&id)
                && !module_globals.contains_key(&id)
            {
                auto_captures.push(id);
            }
        }
    }
    let closure_captures: HashMap<u32, u32> = auto_captures
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();

    // `this` capture. Object-literal methods get `captures_this=true`
    // AND the creation site (lower_object_literal) patches a reserved
    // capture slot at index `auto_captures.len()` with the containing
    // object pointer. At function entry we read that slot and store it
    // into the `this` alloca so `Expr::This` loads the real receiver.
    //
    // Arrow-in-class leftover path (`enclosing_class.is_some()` without
    // the object-literal patch) keeps the old 0.0 sentinel — reads
    // return a bogus value but don't crash.
    let this_stack = if captures_this || enclosing_class.is_some() {
        let this_cap_idx = auto_captures.len() as u32;
        let blk = lf.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        if captures_this {
            let idx_str = this_cap_idx.to_string();
            let v = blk.call(
                DOUBLE,
                "js_closure_get_capture_f64",
                &[(I64, "%this_closure"), (I32, &idx_str)],
            );
            blk.store(DOUBLE, &v, &slot);
        } else {
            blk.store(DOUBLE, "0.0", &slot);
        }
        vec![slot]
    } else {
        Vec::new()
    };
    let class_stack = match enclosing_class.clone() {
        Some(c) => vec![c],
        None => Vec::new(),
    };

    // Boxed vars inside the closure body: mutable captures from the
    // closure's own let-bindings. We don't add the captured-from-outer
    // ids here because those are already boxed in the outer function;
    // the closure body just sees them via the capture mechanism.
    let closure_boxed_vars = module_boxed_vars.clone();

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack,
        class_stack,
        methods,
        module_globals,
        import_function_prefixes,
        closure_captures,
        current_closure_ptr: Some("%this_closure".to_string()),
        enums,
        // Closures don't surface their is_async on the body in the
        // same way functions do. The closure-creation site emits
        // them as plain double-returning functions; we set false
        // here to skip the wrap-in-promise behaviour.
        is_async_fn: false,
        static_field_globals,
        class_ids,
        func_signatures,
        boxed_vars: closure_boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
    };

    stmt::lower_stmts(&mut ctx, body)
        .with_context(|| format!("lowering closure body func_id={}", func_id))?;

    if !ctx.block().is_terminated() {
        ctx.block().ret(DOUBLE, "0.0");
    }
    Ok(())
}

/// Compile a class instance method as a top-level LLVM function with the
/// signature `perry_method_<class>_<name>(this_box: double, args: double…)
/// -> double`. The first parameter (`this`) is stored in a slot whose
/// pointer is pushed onto `this_stack`, then `class_stack` is set so
/// inner `Expr::This` and `super` work correctly.
fn compile_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool)>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;

    // Build the param list: (this, arg0, arg1, ...). All are doubles.
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    // Allocate slots for `this` and each parameter; pre-populate with
    // the incoming values.
    let (this_slot, locals): (String, HashMap<u32, String>) = {
        let blk = lf.block_mut(0).unwrap();
        let this_slot = blk.alloca(DOUBLE);
        blk.store(DOUBLE, "%this_arg", &this_slot);
        let mut map = HashMap::new();
        for p in &method.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        (this_slot, map)
    };

    let local_types: HashMap<u32, perry_types::Type> = method
        .params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .collect();

    let method_boxed_vars = module_boxed_vars.clone();

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack: vec![this_slot],
        class_stack: vec![class.name.clone()],
        methods,
        module_globals,
        import_function_prefixes,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: method.is_async,
        static_field_globals,
        class_ids,
        func_signatures,
        boxed_vars: method_boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
    };

    stmt::lower_stmts(&mut ctx, &method.body)
        .with_context(|| format!("lowering body of method '{}::{}'", class.name, method.name))?;

    if !ctx.block().is_terminated() {
        ctx.block().ret(DOUBLE, "0.0");
    }
    Ok(())
}

/// Emit the module's entry function.
///
/// For the **entry module**: emits `int main()` that bootstraps GC, runs
/// the entry module's own string pool init, then calls every non-entry
/// module's `<prefix>__init` function in order, then runs the entry
/// module's top-level statements, then `return 0`.
///
/// For **non-entry modules**: emits `void <prefix>__init()` that runs the
/// non-entry module's string pool init followed by its top-level
/// statements. The entry module's main calls these via the
/// `non_entry_module_prefixes` list.
///
/// Each module gets its OWN string pool init function
/// (`__perry_init_strings_<prefix>`) so multiple modules in the same
/// program don't collide on the symbol name.
#[allow(clippy::too_many_arguments)]
fn compile_module_entry(
    llmod: &mut LlModule,
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool)>,
    module_prefix: &str,
    is_entry: bool,
    non_entry_module_prefixes: &[String],
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let strings_init_name = format!("__perry_init_strings_{}", module_prefix);

    if is_entry {
        // Pre-declare each non-entry module's init function as an
        // extern so the entry main can call them. The actual definition
        // lives in the OTHER module's compiled .o file; the linker
        // resolves the symbols at link time.
        for prefix in non_entry_module_prefixes {
            llmod.declare_function(&format!("{}__init", prefix), VOID, &[]);
        }

        let main = llmod.define_function("main", I32, vec![]);
        let _ = main.create_block("entry");
        {
            let blk = main.block_mut(0).unwrap();
            blk.call_void("js_gc_init", &[]);
            // Entry module's own string pool first.
            blk.call_void(&strings_init_name, &[]);
            // Then every non-entry module's init in order. Each
            // non-entry module's `<prefix>__init` runs its own string
            // pool init internally before its top-level statements.
            for prefix in non_entry_module_prefixes {
                blk.call_void(&format!("{}__init", prefix), &[]);
            }
        }

        let main_boxed_vars = module_boxed_vars.clone();
        let mut ctx = FnCtx {
            func: main,
            locals: HashMap::new(),
            local_types: HashMap::new(),
            current_block: 0,
            func_names,
            strings,
            loop_targets: Vec::new(),
            label_targets: HashMap::new(),
            pending_label: None,
            classes,
            this_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            static_field_globals,
            class_ids,
            func_signatures,
            boxed_vars: main_boxed_vars,
            closure_rest_params: &closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            namespace_imports: &cross_module.namespace_imports,
            imported_async_funcs: &cross_module.imported_async_funcs,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
            imported_func_return_types: &cross_module.imported_func_return_types,
        };
        // Initialize static class fields with their declared init
        // expressions. Runs once at the top of main, before user code.
        init_static_fields(&mut ctx, hir)?;
        stmt::lower_stmts(&mut ctx, &hir.init)
            .with_context(|| format!("lowering init statements of module '{}'", hir.name))?;

        if !ctx.block().is_terminated() {
            ctx.block().ret(I32, "0");
        }
    } else {
        let init_name = format!("{}__init", module_prefix);
        let init_fn = llmod.define_function(&init_name, VOID, vec![]);
        let _ = init_fn.create_block("entry");
        {
            let blk = init_fn.block_mut(0).unwrap();
            // Each non-entry module runs its own string pool init at
            // the start of its module init function. The entry main
            // calls each module init in order (after running its own
            // strings init), so by the time user code in any module
            // executes, every module's strings are alive.
            blk.call_void(&strings_init_name, &[]);
        }

        let init_boxed_vars = module_boxed_vars.clone();
        let mut ctx = FnCtx {
            func: init_fn,
            locals: HashMap::new(),
            local_types: HashMap::new(),
            current_block: 0,
            func_names,
            strings,
            loop_targets: Vec::new(),
            label_targets: HashMap::new(),
            pending_label: None,
            classes,
            this_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            static_field_globals,
            class_ids,
            func_signatures,
            boxed_vars: init_boxed_vars,
            closure_rest_params: &closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            namespace_imports: &cross_module.namespace_imports,
            imported_async_funcs: &cross_module.imported_async_funcs,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
            imported_func_return_types: &cross_module.imported_func_return_types,
        };
        init_static_fields(&mut ctx, hir)?;
        stmt::lower_stmts(&mut ctx, &hir.init)
            .with_context(|| format!("lowering init statements of non-entry module '{}'", hir.name))?;

        if !ctx.block().is_terminated() {
            ctx.block().ret_void();
        }
    }
    Ok(())
}

/// Emit the string pool into the module: byte-array constants, handle
/// globals, and the `__perry_init_strings_<prefix>` function that
/// allocates + NaN-boxes + GC-roots each handle exactly once at startup.
///
/// The string pool was constructed with a `module_prefix`, so every
/// `entry.bytes_global` / `entry.handle_global` is already prefixed.
/// Emission uses those names directly — no extra prefixing here.
fn emit_string_pool(llmod: &mut LlModule, strings: &StringPool, module_prefix: &str) {
    for entry in strings.iter() {
        // .rodata bytes — `[N+1 x i8]` because we include the null terminator.
        llmod.add_named_string_constant(&entry.bytes_global, entry.byte_len + 1, &entry.escaped_ir);
        // Mutable handle global initialized to 0.0; populated by
        // __perry_init_strings_<prefix>.
        llmod.add_internal_global(&entry.handle_global, DOUBLE, "0.0");
    }

    let init_name = format!("__perry_init_strings_{}", module_prefix);
    let init_fn = llmod.define_function(&init_name, VOID, vec![]);
    let _ = init_fn.create_block("entry");
    let blk = init_fn.block_mut(0).unwrap();

    for entry in strings.iter() {
        let bytes_ref = format!("@{}", entry.bytes_global);
        let handle_ref = format!("@{}", entry.handle_global);
        let len_str = entry.byte_len.to_string();

        let handle = blk.call(
            I64,
            "js_string_from_bytes",
            &[(PTR, &bytes_ref), (I32, &len_str)],
        );
        let nanboxed = blk.call(DOUBLE, "js_nanbox_string", &[(I64, &handle)]);
        blk.store(DOUBLE, &nanboxed, &handle_ref);
        let addr_i64 = blk.ptrtoint(&handle_ref, I64);
        blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);
    }

    blk.ret_void();
}

/// Compile a static class method as a top-level LLVM function with
/// no `this` parameter. Mostly identical to `compile_function` but
/// the LLVM symbol name is `perry_static_<modprefix>__<class>__<method>`
/// instead of `perry_fn_<modprefix>__<name>`.
#[allow(clippy::too_many_arguments)]
fn compile_static_method(
    llmod: &mut LlModule,
    class_name: &str,
    f: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool)>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = format!(
        "perry_static_{}__{}__{}",
        module_prefix,
        sanitize(class_name),
        sanitize(&f.name),
    );

    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in &f.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    let local_types: HashMap<u32, perry_types::Type> = f
        .params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .collect();

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack: Vec::new(),
        // Static methods have no `this` but they CAN reference
        // sibling static methods/fields via the class name (which
        // they handle via StaticFieldGet/StaticMethodCall, not via
        // `this`). The class_stack is empty here.
        class_stack: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        static_field_globals,
        class_ids,
        func_signatures,
        boxed_vars: module_boxed_vars.clone(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
    };
    stmt::lower_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of static '{}::{}'", class_name, f.name))?;

    if !ctx.block().is_terminated() {
        if f.is_async {
            let zero = "0.0".to_string();
            let handle = ctx.block().call(I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, "0.0");
        }
    }
    Ok(())
}

/// Initialize each class's static fields with their declared init
/// expressions. Called at the top of compile_module_entry's main /
/// __init function. The static field globals were registered in
/// compile_module — this just emits the per-field "store init value
/// to global" sequence.
fn init_static_fields(
    ctx: &mut crate::expr::FnCtx<'_>,
    hir: &HirModule,
) -> Result<()> {
    for c in &hir.classes {
        for sf in &c.static_fields {
            let key = (c.name.clone(), sf.name.clone());
            let Some(global_name) = ctx.static_field_globals.get(&key).cloned() else {
                continue;
            };
            if let Some(init_expr) = &sf.init {
                let v = crate::expr::lower_expr(ctx, init_expr)?;
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &v, &g_ref);
            }
        }
    }
    // Static blocks — emitted as synthetic static methods with the
    // name prefix `__perry_static_init_`. Call them in registration
    // order for each class, after that class's static fields are
    // initialized, so they can reference those fields.
    for c in &hir.classes {
        for sm in &c.static_methods {
            if !sm.name.starts_with("__perry_static_init_") {
                continue;
            }
            let key = (c.name.clone(), sm.name.clone());
            if let Some(llvm_name) = ctx.methods.get(&key).cloned() {
                ctx.block().call(DOUBLE, &llvm_name, &[]);
            }
        }
    }
    Ok(())
}

// Collector and boxing-analysis walkers live in dedicated modules.
use crate::collectors::{
    collect_closures_in_stmts, collect_extern_func_refs_in_stmts,
    collect_let_ids, collect_ref_ids_in_stmts,
};
use crate::boxed_vars::{collect_boxed_vars, collect_let_types_in_stmts};

/// Mangle a HIR function name into an LLVM symbol, scoped by module prefix.
///
/// `perry_fn_<modprefix>__<funcname>`. The double-underscore between
/// module prefix and function name is the delimiter — picked because
/// JS identifiers can't contain `__` in user-visible code, so it can't
/// collide with sanitized user names.
fn scoped_fn_name(module_prefix: &str, hir_name: &str) -> String {
    format!("perry_fn_{}__{}", module_prefix, sanitize(hir_name))
}

/// Mangle a class method name into an LLVM symbol, scoped by module
/// prefix and class name.
///
/// `perry_method_<modprefix>__<class>__<method>`.
fn scoped_method_name(module_prefix: &str, class_name: &str, method_name: &str) -> String {
    format!(
        "perry_method_{}__{}__{}",
        module_prefix,
        sanitize(class_name),
        sanitize(method_name)
    )
}

/// Sanitize a name for use in an LLVM symbol — replace anything that isn't
/// `[A-Za-z0-9_]` with an underscore.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Host default triple.
fn default_target_triple() -> String {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "arm64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu".to_string()
    } else {
        "arm64-apple-macosx15.0.0".to_string()
    }
}
