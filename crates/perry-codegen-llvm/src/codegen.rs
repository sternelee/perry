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
    let class_table: HashMap<String, &perry_hir::Class> = hir
        .classes
        .iter()
        .map(|c| (c.name.clone(), c))
        .collect();

    // Module-level globals registry. Pre-walk:
    //   1. Collect every LocalId referenced from any function or method
    //      body (LocalGet / LocalSet / Update). Those that aren't a
    //      function/method's own param or Let must be module-level.
    //   2. Walk hir.init's top-level Lets and globalize ONLY the ones in
    //      that set. Lets that are only referenced from main itself stay
    //      as cheap stack alloca (preserves perf for the bench
    //      benchmarks that don't share state with helper functions).
    let mut referenced_from_fn: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        let mut local_defs: std::collections::HashSet<u32> = f.params.iter().map(|p| p.id).collect();
        collect_let_ids(&f.body, &mut local_defs);
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(&f.body, &mut refs);
        for r in refs {
            if !local_defs.contains(&r) {
                referenced_from_fn.insert(r);
            }
        }
    }
    for c in &hir.classes {
        for m in &c.methods {
            let mut local_defs: std::collections::HashSet<u32> = m.params.iter().map(|p| p.id).collect();
            collect_let_ids(&m.body, &mut local_defs);
            let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
            collect_ref_ids_in_stmts(&m.body, &mut refs);
            for r in refs {
                if !local_defs.contains(&r) {
                    referenced_from_fn.insert(r);
                }
            }
        }
        if let Some(ctor) = &c.constructor {
            let mut local_defs: std::collections::HashSet<u32> = ctor.params.iter().map(|p| p.id).collect();
            collect_let_ids(&ctor.body, &mut local_defs);
            let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
            collect_ref_ids_in_stmts(&ctor.body, &mut refs);
            for r in refs {
                if !local_defs.contains(&r) {
                    referenced_from_fn.insert(r);
                }
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

    // Method registry: (class_name, method_name) → LLVM function name.
    // Built from `class.methods` so the dispatch in `lower_call` knows
    // which mangled function name to call for `obj.method(args)`. Method
    // names are also scoped by module prefix.
    let method_names: HashMap<(String, String), String> = hir
        .classes
        .iter()
        .flat_map(|c| {
            let prefix = module_prefix.clone();
            c.methods
                .iter()
                .map(move |m| {
                    let key = (c.name.clone(), m.name.clone());
                    let val = scoped_method_name(&prefix, &c.name, &m.name);
                    (key, val)
                })
        })
        .collect();

    // Resolve user function names up-front so body lowering can emit
    // forward/recursive calls without worrying about emission order.
    // Names are scoped by module prefix to avoid cross-module collisions.
    let mut func_names: HashMap<u32, String> = HashMap::new();
    for f in &hir.functions {
        func_names.insert(f.id, scoped_fn_name(&module_prefix, &f.name));
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

    // Lower each user function into the module.
    for f in &hir.functions {
        compile_function(&mut llmod, f, &func_names, &mut strings, &class_table, &method_names, &module_globals, &opts.import_function_prefixes)
            .with_context(|| format!("lowering function '{}'", f.name))?;
    }

    // Lower each class method as `perry_method_<modprefix>__<class>__<name>(
    // this_box, arg0, arg1, ...) -> double`. Methods are emitted as
    // standalone LLVM functions; the dispatch in `lower_call` calls
    // them directly.
    for class in &hir.classes {
        for method in &class.methods {
            compile_method(&mut llmod, class, method, &func_names, &mut strings, &class_table, &method_names, &module_globals, &opts.import_function_prefixes)
                .with_context(|| format!("lowering method '{}::{}'", class.name, method.name))?;
        }
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
        &module_prefix,
        opts.is_entry_module,
        &opts.non_entry_module_prefixes,
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
    crate::linker::compile_ll_to_object(&ll_text, opts.target.as_deref())
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

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        classes,
        this_stack: Vec::new(),
        class_stack: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
    };
    stmt::lower_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of '{}'", f.name))?;

    // Defensive: a well-typed numeric function always returns via an
    // explicit `return`, but we emit `ret double 0.0` as a fallback so
    // the LLVM verifier doesn't reject a missing terminator.
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

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        classes,
        this_stack: vec![this_slot],
        class_stack: vec![class.name.clone()],
        methods,
        module_globals,
        import_function_prefixes,
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
fn compile_module_entry(
    llmod: &mut LlModule,
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    module_prefix: &str,
    is_entry: bool,
    non_entry_module_prefixes: &[String],
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

        let mut ctx = FnCtx {
            func: main,
            locals: HashMap::new(),
            local_types: HashMap::new(),
            current_block: 0,
            func_names,
            strings,
            loop_targets: Vec::new(),
            classes,
            this_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
        };
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

        let mut ctx = FnCtx {
            func: init_fn,
            locals: HashMap::new(),
            local_types: HashMap::new(),
            current_block: 0,
            func_names,
            strings,
            loop_targets: Vec::new(),
            classes,
            this_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
        };
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

/// Walk a sequence of statements and collect every Call to an
/// `Expr::ExternFuncRef`. Used by `compile_module` to pre-declare
/// every imported function as an LLVM extern at the top of the IR.
///
/// The output is `(function_name, param_count)`. Param count comes from
/// the call's args.len() — using args.len() rather than the
/// `ExternFuncRef.param_types` is more permissive (the import metadata
/// can carry an outdated count after Perry's lowering).
fn collect_extern_func_refs_in_stmts(
    stmts: &[perry_hir::Stmt],
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<(String, usize)>,
) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Expr(e) | perry_hir::Stmt::Throw(e) => {
                collect_extern_func_refs_in_expr(e, seen, out);
            }
            perry_hir::Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_extern_func_refs_in_expr(e, seen, out);
                }
            }
            perry_hir::Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_extern_func_refs_in_expr(e, seen, out);
                }
            }
            perry_hir::Stmt::If { condition, then_branch, else_branch } => {
                collect_extern_func_refs_in_expr(condition, seen, out);
                collect_extern_func_refs_in_stmts(then_branch, seen, out);
                if let Some(eb) = else_branch {
                    collect_extern_func_refs_in_stmts(eb, seen, out);
                }
            }
            perry_hir::Stmt::While { condition, body } => {
                collect_extern_func_refs_in_expr(condition, seen, out);
                collect_extern_func_refs_in_stmts(body, seen, out);
            }
            perry_hir::Stmt::DoWhile { body, condition } => {
                collect_extern_func_refs_in_stmts(body, seen, out);
                collect_extern_func_refs_in_expr(condition, seen, out);
            }
            perry_hir::Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    collect_extern_func_refs_in_stmts(std::slice::from_ref(init_stmt), seen, out);
                }
                if let Some(cond) = condition {
                    collect_extern_func_refs_in_expr(cond, seen, out);
                }
                if let Some(upd) = update {
                    collect_extern_func_refs_in_expr(upd, seen, out);
                }
                collect_extern_func_refs_in_stmts(body, seen, out);
            }
            _ => {}
        }
    }
}

fn collect_extern_func_refs_in_expr(
    e: &perry_hir::Expr,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<(String, usize)>,
) {
    use perry_hir::Expr;
    match e {
        Expr::Call { callee, args, .. } => {
            if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                if seen.insert(name.clone()) {
                    out.push((name.clone(), args.len()));
                }
            }
            collect_extern_func_refs_in_expr(callee, seen, out);
            for a in args {
                collect_extern_func_refs_in_expr(a, seen, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_extern_func_refs_in_expr(left, seen, out);
            collect_extern_func_refs_in_expr(right, seen, out);
        }
        Expr::Unary { operand, .. } | Expr::Void(operand) | Expr::TypeOf(operand) => {
            collect_extern_func_refs_in_expr(operand, seen, out);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_extern_func_refs_in_expr(condition, seen, out);
            collect_extern_func_refs_in_expr(then_expr, seen, out);
            collect_extern_func_refs_in_expr(else_expr, seen, out);
        }
        Expr::PropertyGet { object, .. } => collect_extern_func_refs_in_expr(object, seen, out),
        Expr::PropertySet { object, value, .. } => {
            collect_extern_func_refs_in_expr(object, seen, out);
            collect_extern_func_refs_in_expr(value, seen, out);
        }
        Expr::IndexGet { object, index } => {
            collect_extern_func_refs_in_expr(object, seen, out);
            collect_extern_func_refs_in_expr(index, seen, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_extern_func_refs_in_expr(object, seen, out);
            collect_extern_func_refs_in_expr(index, seen, out);
            collect_extern_func_refs_in_expr(value, seen, out);
        }
        Expr::Array(elements) => {
            for el in elements {
                collect_extern_func_refs_in_expr(el, seen, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_extern_func_refs_in_expr(v, seen, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_extern_func_refs_in_expr(a, seen, out);
            }
        }
        Expr::LocalSet(_, value) => collect_extern_func_refs_in_expr(value, seen, out),
        _ => {}
    }
}

/// Walk a sequence of statements and collect all LocalIds defined by
/// `Stmt::Let` (function-local declarations). Used by the module-globals
/// pre-walk to distinguish "this id is the function's own local" from
/// "this id refers to a module-level let".
fn collect_let_ids(stmts: &[perry_hir::Stmt], out: &mut std::collections::HashSet<u32>) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Let { id, .. } => {
                out.insert(*id);
            }
            perry_hir::Stmt::If { then_branch, else_branch, .. } => {
                collect_let_ids(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_let_ids(eb, out);
                }
            }
            perry_hir::Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_let_ids(std::slice::from_ref(init_stmt), out);
                }
                collect_let_ids(body, out);
            }
            perry_hir::Stmt::While { body, .. } | perry_hir::Stmt::DoWhile { body, .. } => {
                collect_let_ids(body, out);
            }
            _ => {}
        }
    }
}

/// Walk a sequence of statements and collect all LocalIds referenced via
/// `LocalGet`, `LocalSet`, or `Update`. Used together with `collect_let_ids`
/// to detect references to module-level lets that need globalization.
fn collect_ref_ids_in_stmts(stmts: &[perry_hir::Stmt], out: &mut std::collections::HashSet<u32>) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Expr(e) | perry_hir::Stmt::Throw(e) => collect_ref_ids_in_expr(e, out),
            perry_hir::Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_ref_ids_in_expr(e, out);
                }
            }
            perry_hir::Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_ref_ids_in_expr(e, out);
                }
            }
            perry_hir::Stmt::If { condition, then_branch, else_branch } => {
                collect_ref_ids_in_expr(condition, out);
                collect_ref_ids_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_ref_ids_in_stmts(eb, out);
                }
            }
            perry_hir::Stmt::While { condition, body } => {
                collect_ref_ids_in_expr(condition, out);
                collect_ref_ids_in_stmts(body, out);
            }
            perry_hir::Stmt::DoWhile { body, condition } => {
                collect_ref_ids_in_stmts(body, out);
                collect_ref_ids_in_expr(condition, out);
            }
            perry_hir::Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    collect_ref_ids_in_stmts(std::slice::from_ref(init_stmt), out);
                }
                if let Some(cond) = condition {
                    collect_ref_ids_in_expr(cond, out);
                }
                if let Some(upd) = update {
                    collect_ref_ids_in_expr(upd, out);
                }
                collect_ref_ids_in_stmts(body, out);
            }
            _ => {}
        }
    }
}

fn collect_ref_ids_in_expr(e: &perry_hir::Expr, out: &mut std::collections::HashSet<u32>) {
    use perry_hir::Expr;
    match e {
        Expr::LocalGet(id) => {
            out.insert(*id);
        }
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            collect_ref_ids_in_expr(value, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } | Expr::Logical { left, right, .. } => {
            collect_ref_ids_in_expr(left, out);
            collect_ref_ids_in_expr(right, out);
        }
        Expr::Unary { operand, .. } | Expr::Void(operand) | Expr::TypeOf(operand) => {
            collect_ref_ids_in_expr(operand, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_ref_ids_in_expr(callee, out);
            for a in args {
                collect_ref_ids_in_expr(a, out);
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_ref_ids_in_expr(condition, out);
            collect_ref_ids_in_expr(then_expr, out);
            collect_ref_ids_in_expr(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => {
            collect_ref_ids_in_expr(object, out);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_ref_ids_in_expr(object, out);
            collect_ref_ids_in_expr(value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_ref_ids_in_expr(object, out);
            collect_ref_ids_in_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_ref_ids_in_expr(object, out);
            collect_ref_ids_in_expr(index, out);
            collect_ref_ids_in_expr(value, out);
        }
        Expr::ArrayPush { array_id, value } => {
            out.insert(*array_id);
            collect_ref_ids_in_expr(value, out);
        }
        Expr::Array(elements) => {
            for el in elements {
                collect_ref_ids_in_expr(el, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_ref_ids_in_expr(v, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_ref_ids_in_expr(a, out);
            }
        }
        _ => {}
    }
}

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
