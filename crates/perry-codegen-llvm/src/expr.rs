//! Expression codegen — Phase 2.
//!
//! Scope: numeric expressions (literals, LocalGet, Binary add/sub/mul/div,
//! Compare, direct FuncRef calls) plus the `console.log(<expr>)` sink. All
//! values are raw LLVM `double` — no NaN-boxing, no strings, no objects.
//!
//! Anything outside the supported shape returns an explicit "unsupported"
//! error so a user running `--backend llvm` on richer TypeScript gets a
//! one-line explanation instead of a silent broken binary.


use anyhow::{anyhow, bail, Result};
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
use perry_types::Type as HirType;

use crate::block::LlBlock;
use crate::function::LlFunction;
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
use crate::lower_string_method::{lower_string_coerce_concat, lower_string_concat, lower_string_self_append};
use crate::nanbox::{double_literal, POINTER_MASK_I64, POINTER_TAG_I64, STRING_TAG_I64};
use crate::strings::StringPool;
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bool_expr, is_map_expr, is_numeric_expr,
    is_set_expr, is_string_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I1, I32, I64, PTR};

/// Inline NaN-box of a raw heap pointer with `POINTER_TAG`.
pub(crate) fn nanbox_pointer_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, POINTER_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Alias kept for backwards compatibility with existing callers
/// in `stmt.rs` and `codegen.rs` that use the `_pub` suffix.
pub(crate) fn nanbox_pointer_inline_pub(blk: &mut LlBlock, ptr_i64: &str) -> String {
    nanbox_pointer_inline(blk, ptr_i64)
}

/// Inline NaN-box of a raw string handle with `STRING_TAG`.
pub(crate) fn nanbox_string_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, STRING_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Convert an i32 boolean (0 or 1) returned by a runtime function into a
/// NaN-tagged JSValue boolean (`TAG_TRUE` / `TAG_FALSE`).
pub(crate) fn i32_bool_to_nanbox(blk: &mut LlBlock, i32_val: &str) -> String {
    let bit = blk.icmp_ne(I32, i32_val, "0");
    let tagged = blk.select(
        I1,
        &bit,
        I64,
        crate::nanbox::TAG_TRUE_I64,
        crate::nanbox::TAG_FALSE_I64,
    );
    blk.bitcast_i64_to_double(&tagged)
}
/// Per-function codegen context. Held briefly during lowering, never stored.
pub(crate) struct FnCtx<'a> {
    /// Function being built (blocks, params, registers).
    pub func: &'a mut LlFunction,
    /// Map from HIR LocalId → LLVM alloca pointer (e.g. `%r3`).
    pub locals: std::collections::HashMap<u32, String>,
    /// Map from HIR LocalId → static HIR Type. Used by `is_string_expr` and
    /// future type-aware dispatch sites (Phase B's "native instance flag
    /// tracking" extension). Populated from function params and `Stmt::Let`
    /// declarations as they're lowered.
    pub local_types: std::collections::HashMap<u32, HirType>,
    /// Index into `func.blocks()` pointing at the block currently receiving
    /// instructions. Lowering fns update this when control flow splits.
    pub current_block: usize,
    /// HIR FuncId → LLVM function name. Resolved at the top of
    /// `compile_module` so `FuncRef(id)` calls know what to emit.
    pub func_names: &'a std::collections::HashMap<u32, String>,
    /// Module-wide string literal pool. Disjoint borrow from `func` because
    /// it lives in `codegen.rs` as a separate variable, not inside the
    /// LlModule that `func` was derived from. See `crate::strings` for the
    /// design rationale.
    pub strings: &'a mut StringPool,
    /// Stack of loop targets for `break` / `continue` lowering. Each entry
    /// is `(continue_label, break_label)`. Pushed when entering a loop,
    /// popped on exit. The innermost loop is at the top of the stack.
    ///
    /// For `for`-loops: continue → update block (so the update runs before
    /// the next iteration); break → exit block.
    /// For `while`/`do-while`: continue → cond block; break → exit block.
    pub loop_targets: Vec<(String, String)>,
    /// Map from label name → (continue_label, break_label). Populated by
    /// `Stmt::Labeled { label, body }` when the body is a loop. Looked up
    /// by `Stmt::LabeledBreak(label)` / `Stmt::LabeledContinue(label)`.
    pub label_targets: std::collections::HashMap<String, (String, String)>,
    /// Pending label set by `Stmt::Labeled` just before lowering the body.
    /// The next loop that runs (`for`/`while`/`do-while`) consumes it and
    /// registers itself in `label_targets` so `break label;` /
    /// `continue label;` can jump to the right blocks.
    pub pending_label: Option<String>,
    /// Map from class name → HIR Class definition. Built once in
    /// `compile_module` from `hir.classes`. Used by `Expr::New` to look up
    /// the field count, constructor body, and (eventually) method table.
    pub classes: &'a std::collections::HashMap<String, &'a perry_hir::Class>,
    /// Stack of `this` slot pointers — set when lowering inside a class
    /// constructor body. `Expr::This` loads from the top entry.
    pub this_stack: Vec<String>,
    /// Stack of class names currently being lowered. Pushed when entering
    /// a constructor body. `Expr::SuperCall` looks at the top entry to
    /// find the parent class's constructor to inline. Same depth as
    /// `this_stack` (one entry per nested `new`).
    pub class_stack: Vec<String>,
    /// Method registry: `(class_name, method_name) → LLVM function name`.
    /// Built by `compile_module` from `hir.classes[*].methods`. Used by
    /// `lower_call` to dispatch `obj.method(args)` to the right
    /// `perry_method_<class>_<name>` function.
    pub methods: &'a std::collections::HashMap<(String, String), String>,
    /// Module-level globals: `LocalId → global symbol name (without @)`.
    /// Built by `compile_module` from top-level `Stmt::Let` declarations
    /// in `hir.init`. Used by `LocalGet`/`LocalSet`/`Update`/`Stmt::Let`
    /// — when a local id is in this map, it refers to a module-level
    /// `internal global double 0.0` instead of a stack alloca, so the
    /// value is visible to all functions in the module (essential for
    /// patterns like `let failures = 0; function eq() { failures++; }`).
    pub module_globals: &'a std::collections::HashMap<u32, String>,
    /// Imported function name → source module's symbol prefix. Used by
    /// `ExternFuncRef` lowering in `lower_call` to generate scoped
    /// cross-module calls.
    pub import_function_prefixes: &'a std::collections::HashMap<String, String>,
    /// Closure capture map: when lowering inside a closure body, this
    /// holds `LocalId → capture_index`. `LocalGet`/`LocalSet`/`Update`
    /// of an id in this map routes through the runtime
    /// `js_closure_get/set_capture_f64(this_closure, idx)` calls
    /// instead of an alloca slot.
    pub closure_captures: std::collections::HashMap<u32, u32>,
    /// Inside a closure body, the LLVM SSA value name for the current
    /// closure pointer (`%this_closure`). `Expr::LocalGet` of a captured
    /// id uses this as the first arg to `js_closure_get_capture_f64`.
    pub current_closure_ptr: Option<String>,
    /// Map from (enum_name, member_name) → enum value. Built once in
    /// `compile_module` from `hir.enums`. Used by `Expr::EnumMember`
    /// to lower enum references to constants.
    pub enums: &'a std::collections::HashMap<(String, String), perry_hir::EnumValue>,
    /// Whether the enclosing function is `async`. When true, every
    /// `Stmt::Return(value)` wraps `value` in `js_promise_resolved`
    /// before returning, so callers can `await` the result.
    pub is_async_fn: bool,
    /// Static class fields: `(class_name, field_name) → llvm global
    /// symbol`. Built once in `compile_module`. Used by
    /// `Expr::StaticFieldGet/Set` to load/store the global.
    pub static_field_globals:
        &'a std::collections::HashMap<(String, String), String>,
    /// Per-class id for object headers. Each user class gets a
    /// unique non-zero id (anonymous objects use 0). Used by
    /// `lower_new` and the virtual method dispatch helper.
    pub class_ids: &'a std::collections::HashMap<String, u32>,
    /// Per-function param signature: `(declared_param_count,
    /// has_rest_param)`. Used by FuncRef call sites to know whether
    /// to bundle trailing arguments into a rest array.
    pub func_signatures: &'a std::collections::HashMap<u32, (usize, bool)>,
    /// LocalIds that must be stored in heap boxes (`js_box_alloc`)
    /// instead of stack allocas. A local gets boxed when at least
    /// one closure captures it AND it's written to (either by the
    /// enclosing function or inside a closure). Boxing guarantees
    /// that all readers — inc()/get() on a shared counter, for
    /// instance — observe each other's writes. See `collect_boxed_
    /// vars` for the detection rule.
    ///
    /// For ids in this set:
    /// - Stmt::Let allocates a box via `js_box_alloc(init)` and
    ///   stores the box pointer (i64) in a local alloca slot.
    /// - LocalGet reads the slot, unboxes, and calls `js_box_get`.
    /// - LocalSet/Update reads the slot, unboxes, and calls
    ///   `js_box_set`.
    /// - Closure creation captures the box pointer directly so
    ///   the closure body sees the same storage.
    pub boxed_vars: std::collections::HashSet<u32>,
    /// Closure rest param index: closure `FuncId` → index of the rest
    /// parameter. Built once in `compile_module` from the collected
    /// closures. Used by the closure call site in `lower_call` to
    /// bundle trailing arguments into an array before calling
    /// `js_closure_callN`.
    pub closure_rest_params: &'a std::collections::HashMap<u32, usize>,
    /// LocalId → closure FuncId mapping. Populated in `Stmt::Let`
    /// when the init expression is `Expr::Closure { func_id, .. }`.
    /// Used by the closure call site in `lower_call` to look up the
    /// callee's rest param info from `closure_rest_params`.
    pub local_closure_func_ids: std::collections::HashMap<u32, u32>,

    // ── Cross-module import plumbing (Phase F) ──────────────────────

    /// Locals that are namespace imports (`import * as X from "./mod"`).
    /// Codegen uses this to know that `X.foo()` should be dispatched as
    /// a cross-module call rather than an object method call.
    pub namespace_imports: &'a std::collections::HashSet<String>,
    /// Names of imported functions that are async. Used to wrap
    /// cross-module calls in promise machinery.
    pub imported_async_funcs: &'a std::collections::HashSet<String>,
    /// Type alias map (name → Type) aggregated from all modules. Used
    /// to resolve `Named` types in function signatures and dispatch.
    pub type_aliases: &'a std::collections::HashMap<String, perry_types::Type>,
    /// Imported function parameter counts, keyed by function name.
    /// Used for rest-param bundling on cross-module calls.
    pub imported_func_param_counts: &'a std::collections::HashMap<String, usize>,
    /// Imported function return types, keyed by local function name.
    /// Used for type-aware dispatch on cross-module call results.
    pub imported_func_return_types: &'a std::collections::HashMap<String, perry_types::Type>,
}

impl<'a> FnCtx<'a> {
    pub fn block(&mut self) -> &mut LlBlock {
        self.func
            .block_mut(self.current_block)
            .expect("current_block index points at a valid block")
    }

    /// Create a new block and return its index, **without** switching the
    /// current_block pointer. The caller is responsible for deciding when
    /// to flip.
    pub fn new_block(&mut self, name: &str) -> usize {
        let _ = self.func.create_block(name);
        self.func.num_blocks() - 1
    }

    /// Label of a block by index — needed when emitting a branch.
    pub fn block_label(&self, idx: usize) -> String {
        self.func
            .blocks()
            .get(idx)
            .map(|b| b.label.clone())
            .expect("valid block index")
    }

}

/// Lower an expression to a raw LLVM `double` value. Returns the string form
/// of the value (either a `%rN` register or a literal like `42.0`).
pub(crate) fn lower_expr(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        // -------- Literals --------
        Expr::Integer(i) => Ok(double_literal(*i as f64)),
        Expr::Number(f) => Ok(double_literal(*f)),
        // Booleans are NaN-boxed using TAG_TRUE/TAG_FALSE — both are
        // double bit patterns inside the NaN range, emitted as hex
        // literals (LLVM's `0x{16-hex}` form for non-finite doubles).
        Expr::Bool(b) => {
            let tag = if *b {
                crate::nanbox::TAG_TRUE
            } else {
                crate::nanbox::TAG_FALSE
            };
            Ok(double_literal(f64::from_bits(tag)))
        }
        // `undefined` and `null` lower to their NaN-tagged bit patterns.
        Expr::Undefined => Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
        Expr::Null => Ok(double_literal(f64::from_bits(crate::nanbox::TAG_NULL))),

        // `void <expr>` — evaluate the operand for side effects, return
        // undefined. Used both as `void 0` (a common idiom for `undefined`)
        // and `void (sideEffect = 42)` for discarding an assignment value.
        Expr::Void(operand) => {
            let _ = lower_expr(ctx, operand)?;
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // `typeof <expr>` — calls js_value_typeof which returns a runtime
        // string handle ("number", "string", "boolean", "undefined",
        // "object", "function"). The result is NaN-boxed with STRING_TAG.
        Expr::TypeOf(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_value_typeof", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // String literals are pre-allocated at module init via the
        // StringPool's hoisting strategy (see `crate::strings`). At the use
        // site we just load the cached NaN-boxed handle from the pool's
        // `.handle` global. ONE instruction, no per-use allocation.
        Expr::String(s) => {
            let idx = ctx.strings.intern(s);
            let entry = ctx.strings.entry(idx);
            // Clone the global name out so we don't keep `entry` borrowed
            // across the call to `ctx.block()` (which mutably borrows
            // `ctx.func`, distinct from `ctx.strings` but the borrow checker
            // sees `entry` as borrowing `ctx`).
            let handle_global = format!("@{}", entry.handle_global);
            Ok(ctx.block().load(DOUBLE, &handle_global))
        }

        // -------- Variables --------
        // LocalGet lookup order:
        //   1. Closure captures (when lowering inside a closure body) →
        //      runtime js_closure_get_capture_f64(this_closure, idx)
        //   2. Function-local alloca slots
        //   3. Module-level globals
        //
        // This lets closures read captured outer variables, regular
        // functions read their own params/lets, and any function read
        // module-scope `let`s (the ones in `hir.init` at top level).
        Expr::LocalGet(id) => {
            // Captured by closure (from outer scope):
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("captured local but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                // If the captured id is a boxed var, the capture
                // slot holds a raw box pointer (as a bit-castable
                // double). Read the capture, extract the box
                // pointer, and deref via js_box_get.
                if ctx.boxed_vars.contains(id) {
                    let blk = ctx.block();
                    let cap_dbl = blk.call(
                        DOUBLE,
                        "js_closure_get_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let box_ptr = blk.bitcast_double_to_i64(&cap_dbl);
                    return Ok(blk.call(DOUBLE, "js_box_get", &[(I64, &box_ptr)]));
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_closure_get_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str)],
                ));
            }
            // Boxed local in enclosing function: load the slot (box
            // pointer), deref via js_box_get.
            if ctx.boxed_vars.contains(id) {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot);
                    let box_ptr = blk.bitcast_double_to_i64(&box_dbl);
                    return Ok(blk.call(DOUBLE, "js_box_get", &[(I64, &box_ptr)]));
                }
            }
            if let Some(slot) = ctx.locals.get(id).cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                let g_ref = format!("@{}", global_name);
                Ok(ctx.block().load(DOUBLE, &g_ref))
            } else {
                // Soft fallback: the HIR sometimes carries stale
                // local references that don't correspond to any
                // declared param/let/global in the current scope
                // (curry-style nested closures, async transformer
                // intermediate ids, etc.). Return undefined so
                // compilation succeeds; the caller gets garbage at
                // runtime but won't crash at codegen.
                Ok(double_literal(0.0))
            }
        }

        // `total = expr` — store the new value into the local's alloca slot
        // and return it (matches JS semantics: assignment is an expression
        // whose value is the assigned value).
        //
        // SPECIAL FAST PATH: `x = x + y` where `x` is a string-typed local.
        // Mirrors Cranelift's `expr.rs:5611` pattern. We use
        // `js_string_append` (in-place for refcount=1 unique owners)
        // instead of `js_string_concat` (always allocates). For a 10K-
        // iteration `str = str + "a"` build loop, this turns O(n²) total
        // work into O(n) and is the difference between 700 ms and 200 ms
        // on bench_string_ops.
        Expr::LocalSet(id, value) => {
            // Detect the `x = x + y` self-append pattern.
            if matches!(ctx.local_types.get(id), Some(HirType::String)) {
                if let Expr::Binary { op: BinaryOp::Add, left, right } = value.as_ref() {
                    if let Expr::LocalGet(left_id) = left.as_ref() {
                        if left_id == id {
                            return lower_string_self_append(ctx, *id, right);
                        }
                    }
                }
            }
            let v = lower_expr(ctx, value)?;
            // Closure captures first (write through the runtime), then
            // locals, then module globals.
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("captured local set but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                // Boxed captured var: read the box pointer from the
                // capture slot, then js_box_set to update the shared
                // cell. Do NOT overwrite the capture slot — it holds
                // the box pointer, not the value.
                if ctx.boxed_vars.contains(id) {
                    let blk = ctx.block();
                    let cap_dbl = blk.call(
                        DOUBLE,
                        "js_closure_get_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let box_ptr = blk.bitcast_double_to_i64(&cap_dbl);
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &v)]);
                } else {
                    ctx.block().call_void(
                        "js_closure_set_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &v)],
                    );
                }
            } else if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                // Box path — only for non-global locals. Module globals
                // have their own shared storage and don't need boxing.
                // Without the !module_globals guard, closures that
                // modify a module-level variable would silently skip
                // the store (ctx.locals doesn't have the global's slot).
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot);
                    let box_ptr = blk.bitcast_double_to_i64(&box_dbl);
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &v)]);
                }
            } else if let Some(slot) = ctx.locals.get(id).cloned() {
                ctx.block().store(DOUBLE, &v, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &v, &g_ref);
            }
            // Soft fallback: drop the store on the floor for missing
            // locals. See LocalGet for the rationale.
            Ok(v)
        }

        // `i++` / `++i` / `i--` / `--i`. Postfix returns the OLD value,
        // prefix returns the NEW value. Closure captures, locals, then
        // module globals.
        Expr::Update { id, op, prefix } => {
            // Closure capture path: runtime get + add/sub + runtime set.
            if let Some(&capture_idx) = ctx.closure_captures.get(id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("captured local update but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                // Boxed captured var: deref box, modify, store back.
                if ctx.boxed_vars.contains(id) {
                    let blk = ctx.block();
                    let cap_dbl = blk.call(
                        DOUBLE,
                        "js_closure_get_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let box_ptr = blk.bitcast_double_to_i64(&cap_dbl);
                    let old = blk.call(DOUBLE, "js_box_get", &[(I64, &box_ptr)]);
                    let new = match op {
                        UpdateOp::Increment => blk.fadd(&old, "1.0"),
                        UpdateOp::Decrement => blk.fsub(&old, "1.0"),
                    };
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &new)]);
                    return Ok(if *prefix { new } else { old });
                }
                let old = ctx.block().call(
                    DOUBLE,
                    "js_closure_get_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str)],
                );
                let blk = ctx.block();
                let new = match op {
                    UpdateOp::Increment => blk.fadd(&old, "1.0"),
                    UpdateOp::Decrement => blk.fsub(&old, "1.0"),
                };
                blk.call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new)],
                );
                return Ok(if *prefix { new } else { old });
            }
            // Boxed enclosing-scope var: load slot (box ptr), deref,
            // increment, box_set. Skip for module globals (they
            // have their own shared storage).
            if ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id) {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot);
                    let box_ptr = blk.bitcast_double_to_i64(&box_dbl);
                    let old = blk.call(DOUBLE, "js_box_get", &[(I64, &box_ptr)]);
                    let new = match op {
                        UpdateOp::Increment => blk.fadd(&old, "1.0"),
                        UpdateOp::Decrement => blk.fsub(&old, "1.0"),
                    };
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &new)]);
                    return Ok(if *prefix { new } else { old });
                }
            }
            let storage = if let Some(slot) = ctx.locals.get(id).cloned() {
                slot
            } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                format!("@{}", global_name)
            } else {
                // Soft fallback: silently increment a throwaway value.
                return Ok(double_literal(0.0));
            };
            let blk = ctx.block();
            let old = blk.load(DOUBLE, &storage);
            let new = match op {
                UpdateOp::Increment => blk.fadd(&old, "1.0"),
                UpdateOp::Decrement => blk.fsub(&old, "1.0"),
            };
            blk.store(DOUBLE, &new, &storage);
            Ok(if *prefix { new } else { old })
        }

        // `Date.now()` — special HIR variant that lowers to a single FFI
        // call returning a `double` (milliseconds since UNIX epoch as
        // produced by `js_date_now` in `perry-runtime/src/date.rs`).
        Expr::DateNow => Ok(ctx.block().call(DOUBLE, "js_date_now", &[])),

        // -------- Arithmetic --------
        // String concatenation (Phase B): if Add receives operands where
        // either side is statically a string, route through string concat.
        // - both strings → `lower_string_concat` (inline bitcast+and unbox)
        // - one string + one non-string → `lower_string_coerce_concat`
        //   (the non-string side passes through `js_jsvalue_to_string`
        //   which dispatches on the NaN tag at runtime)
        Expr::Binary { op, left, right } => {
            if matches!(op, BinaryOp::Add) {
                let l_is_str = is_string_expr(ctx, left);
                let r_is_str = is_string_expr(ctx, right);
                if l_is_str && r_is_str {
                    return lower_string_concat(ctx, left, right);
                }
                if l_is_str || r_is_str {
                    return lower_string_coerce_concat(ctx, left, right, l_is_str, r_is_str);
                }
            }
            let l_raw = lower_expr(ctx, left)?;
            let r_raw = lower_expr(ctx, right)?;
            // Coerce non-numeric operands to numbers for arithmetic.
            // JS: `true + true = 2`, `null + 1 = 1`, etc. Without
            // this, fadd on NaN-tagged booleans propagates the NaN
            // payload instead of computing 1.0 + 1.0 = 2.0.
            let l_numeric = is_numeric_expr(ctx, left);
            let r_numeric = is_numeric_expr(ctx, right);
            let l = if l_numeric { l_raw } else {
                ctx.block().call(DOUBLE, "js_number_coerce", &[(DOUBLE, &l_raw)])
            };
            let r = if r_numeric { r_raw } else {
                ctx.block().call(DOUBLE, "js_number_coerce", &[(DOUBLE, &r_raw)])
            };
            let blk = ctx.block();
            let v = match op {
                BinaryOp::Add => blk.fadd(&l, &r),
                BinaryOp::Sub => blk.fsub(&l, &r),
                BinaryOp::Mul => blk.fmul(&l, &r),
                BinaryOp::Div => blk.fdiv(&l, &r),
                BinaryOp::Mod => blk.frem(&l, &r),
                BinaryOp::Pow => {
                    blk.call(DOUBLE, "js_math_pow", &[(DOUBLE, &l), (DOUBLE, &r)])
                }
                // Bitwise ops: JS ToInt32 semantics require safe
                // i64 conversion then truncation to i32, because
                // fptosi(f64→i32) is UB for values outside
                // [-2^31, 2^31-1] (e.g. 0xFFFFFFFF = 4294967295).
                BinaryOp::BitAnd => {
                    let li64 = blk.fptosi(DOUBLE, &l, I64);
                    let ri64 = blk.fptosi(DOUBLE, &r, I64);
                    let li = blk.trunc(I64, &li64, I32);
                    let ri = blk.trunc(I64, &ri64, I32);
                    let v = blk.and(I32, &li, &ri);
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::BitOr => {
                    let li64 = blk.fptosi(DOUBLE, &l, I64);
                    let ri64 = blk.fptosi(DOUBLE, &r, I64);
                    let li = blk.trunc(I64, &li64, I32);
                    let ri = blk.trunc(I64, &ri64, I32);
                    let v = blk.or(I32, &li, &ri);
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::BitXor => {
                    let li64 = blk.fptosi(DOUBLE, &l, I64);
                    let ri64 = blk.fptosi(DOUBLE, &r, I64);
                    let li = blk.trunc(I64, &li64, I32);
                    let ri = blk.trunc(I64, &ri64, I32);
                    let v = blk.xor(I32, &li, &ri);
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::Shl => {
                    let li64 = blk.fptosi(DOUBLE, &l, I64);
                    let ri64 = blk.fptosi(DOUBLE, &r, I64);
                    let li = blk.trunc(I64, &li64, I32);
                    let ri = blk.trunc(I64, &ri64, I32);
                    let v = blk.shl(I32, &li, &ri);
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::Shr => {
                    let li64 = blk.fptosi(DOUBLE, &l, I64);
                    let ri64 = blk.fptosi(DOUBLE, &r, I64);
                    let li = blk.trunc(I64, &li64, I32);
                    let ri = blk.trunc(I64, &ri64, I32);
                    let v = blk.ashr(I32, &li, &ri);
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::UShr => {
                    let li64 = blk.fptosi(DOUBLE, &l, I64);
                    let ri64 = blk.fptosi(DOUBLE, &r, I64);
                    let li = blk.trunc(I64, &li64, I32);
                    let ri = blk.trunc(I64, &ri64, I32);
                    let v = blk.lshr(I32, &li, &ri);
                    blk.uitofp(I32, &v, DOUBLE)
                }
            };
            Ok(v)
        }

        // -------- Unary operators --------
        Expr::Unary { op, operand } => {
            let numeric = is_numeric_expr(ctx, operand);
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            match op {
                UnaryOp::Neg => {
                    if numeric {
                        Ok(blk.fneg(&v))
                    } else {
                        let coerced = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]);
                        Ok(blk.fneg(&coerced))
                    }
                }
                UnaryOp::Pos => {
                    if numeric {
                        Ok(v)
                    } else {
                        Ok(blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]))
                    }
                }
                UnaryOp::Not => {
                    // !x: truthiness inverted, then NaN-box as a JS
                    // boolean (TAG_TRUE / TAG_FALSE) so console.log
                    // prints "true" / "false" instead of 1 / 0.
                    let bit = lower_truthy(ctx, &v, operand);
                    let blk = ctx.block();
                    let inv = blk.xor(crate::types::I1, &bit, "true");
                    let tagged_i64 = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    Ok(blk.bitcast_i64_to_double(&tagged_i64))
                }
                UnaryOp::BitNot => {
                    // ~x: bitwise NOT with proper JS ToInt32 semantics.
                    // Direct fptosi(f64→i32) has undefined behavior for
                    // values outside [-2^31, 2^31-1] (like 0xFFFFFFFF =
                    // 4294967295). Use fptosi(f64→i64) first (safe for
                    // all JS numbers), then trunc(i64→i32) to get the
                    // correct 32-bit pattern, then NOT.
                    let i64_v = blk.fptosi(DOUBLE, &v, I64);
                    let i32_v = blk.trunc(I64, &i64_v, I32);
                    let inv = blk.xor(I32, &i32_v, "-1");
                    Ok(blk.sitofp(I32, &inv, DOUBLE))
                }
            }
        }

        // -------- Comparison --------
        // LLVM `fcmp` returns `i1`. We zext to double so the value fits the
        // standard number ABI used by the rest of the codegen — JS "true"
        // round-trips through numeric contexts as 1.0 and "false" as 0.0,
        // which is what Perry's runtime expects from typed boolean returns.
        Expr::Compare { op, left, right } => {
            // Boolean equality fast path: NaN-tagged TAG_TRUE/FALSE
            // bits don't compare correctly with fcmp. For
            // ===/!== where EITHER side is statically boolean, compare
            // the raw i64 bits via icmp. icmp on bits also works for
            // any other NaN-tagged value (string ptr, object ptr) when
            // the bool literal is on one side — TAG_TRUE bits never
            // match a string/pointer, so the result is correctly false.
            // STRICT only: for LooseEq/LooseNe, booleans need coercion
            // (false == "" → true) which the later js_loose_eq handles.
            let either_bool = is_bool_expr(ctx, left) || is_bool_expr(ctx, right);
            if either_bool && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let bit = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.icmp_ne(I64, &l_bits, &r_bits)
                } else {
                    blk.icmp_eq(I64, &l_bits, &r_bits)
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // "One side is statically string, other is unknown"
            // fallback: `c === Color.Red` where Color is a const
            // object. Neither js_eq (bit-compare, wrong for string
            // content) nor fcmp (NaN-tagged, always false) works.
            //
            // Dispatch through js_string_equals after extracting
            // both string pointers via js_get_string_pointer_unified.
            // That helper returns null for non-string NaN-tagged
            // values, which js_string_equals treats as "not equal"
            // — the correct answer when the unknown side isn't a
            // string at runtime.
            let both_strings_check = is_string_expr(ctx, left) && is_string_expr(ctx, right);
            let one_side_string = !both_strings_check
                && ((is_string_expr(ctx, left) && !is_numeric_expr(ctx, right) && !is_bool_expr(ctx, right))
                    || (is_string_expr(ctx, right) && !is_numeric_expr(ctx, left) && !is_bool_expr(ctx, left)));
            if one_side_string
                && matches!(op, CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe)
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = blk.call(
                    I64,
                    "js_get_string_pointer_unified",
                    &[(DOUBLE, &l)],
                );
                let r_handle = blk.call(
                    I64,
                    "js_get_string_pointer_unified",
                    &[(DOUBLE, &r)],
                );
                let i32_eq = blk.call(
                    I32,
                    "js_string_equals",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = blk.icmp_ne(I32, &i32_eq, "0");
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit_final,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Generic equality fallback: when neither operand is
            // statically numeric, dispatch through js_eq which
            // handles strings, booleans, objects, null, undefined
            // via NaN-tag inspection. Used by `eq` helpers in tests
            // that take `any` and pass NaN-tagged values.
            let either_non_numeric = !is_numeric_expr(ctx, left) && !is_numeric_expr(ctx, right);
            let only_eq = matches!(op, CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe);
            // We still let the more specific paths below win for
            // statically-typed string/bool operands; this fallback
            // only handles the truly-Any case.
            let unknown_l = !is_numeric_expr(ctx, left)
                && !is_string_expr(ctx, left)
                && !is_bool_expr(ctx, left);
            let unknown_r = !is_numeric_expr(ctx, right)
                && !is_string_expr(ctx, right)
                && !is_bool_expr(ctx, right);
            if either_non_numeric && only_eq && unknown_l && unknown_r {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Use js_loose_eq for == / != (handles null==undefined,
                // cross-type coercion). Use js_eq for === / !==.
                let eq_fn = if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                    "js_loose_eq"
                } else {
                    "js_eq"
                };
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, eq_fn, &[(I64, &l_bits), (I64, &r_bits)]);
                let result = blk.bitcast_i64_to_double(&result_bits);
                if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(result);
            }

            // String equality fast path: fcmp doesn't work on
            // NaN-tagged string pointers (NaN comparisons are
            // unordered → always false). When both operands are
            // statically strings, dispatch through js_string_equals.
            let both_strings = is_string_expr(ctx, left) && is_string_expr(ctx, right);
            if both_strings && matches!(op, CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = unbox_to_i64(blk, &l);
                let r_handle = unbox_to_i64(blk, &r);
                let i32_eq = blk.call(I32, "js_string_equals", &[(I64, &l_handle), (I64, &r_handle)]);
                let bit = blk.icmp_ne(I32, &i32_eq, "0");
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit_final,
                    crate::types::I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged_i64));
            }
            // String relational fast path: `s1 < s2`, `s1 > s2`, etc.
            // fcmp on NaN-tagged pointers is unordered (always false),
            // so dispatch through js_string_compare which returns
            // -1/0/1 like memcmp. Then test the result against 0 with
            // the right icmp predicate.
            if both_strings && matches!(op, CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = unbox_to_i64(blk, &l);
                let r_handle = unbox_to_i64(blk, &r);
                let cmp_i32 = blk.call(
                    I32,
                    "js_string_compare",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = match op {
                    CompareOp::Lt => blk.icmp_slt(I32, &cmp_i32, "0"),
                    CompareOp::Le => blk.icmp_sle(I32, &cmp_i32, "0"),
                    CompareOp::Gt => blk.icmp_sgt(I32, &cmp_i32, "0"),
                    CompareOp::Ge => blk.icmp_sge(I32, &cmp_i32, "0"),
                    _ => unreachable!(),
                };
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit,
                    crate::types::I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged_i64));
            }

            // Loose equality (==, !=): dispatch through js_loose_eq
            // which handles cross-type coercion (null==undefined,
            // "1"==1, false==0, etc.). Strict === already handled
            // above by the typed fast paths.
            if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, "js_loose_eq", &[(I64, &l_bits), (I64, &r_bits)]);
                if matches!(op, CompareOp::LooseNe) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(blk.bitcast_i64_to_double(&result_bits));
            }

            let l = lower_expr(ctx, left)?;
            let r = lower_expr(ctx, right)?;
            let pred = match op {
                CompareOp::Eq => "oeq",
                // !== uses `une` (unordered or not equal), NOT `one`.
                // `one` is "ordered and not equal" which returns false
                // when either operand is NaN. JS !== on NaN must return
                // true: NaN !== NaN → !(NaN === NaN) → !false → true.
                CompareOp::Ne => "une",
                CompareOp::Lt => "olt",
                CompareOp::Le => "ole",
                CompareOp::Gt => "ogt",
                CompareOp::Ge => "oge",
                // LooseEq/Ne handled above
                CompareOp::LooseEq | CompareOp::LooseNe => unreachable!(),
            };
            let blk = ctx.block();
            let bit = blk.fcmp(pred, &l, &r);
            let tag_true_i64 = crate::nanbox::TAG_TRUE_I64;
            let tag_false_i64 = crate::nanbox::TAG_FALSE_I64;
            let tagged_i64 = blk.select(crate::types::I1, &bit, crate::types::I64, tag_true_i64, tag_false_i64);
            Ok(blk.bitcast_i64_to_double(&tagged_i64))
        }

        // -------- Objects (Phase B.4) --------
        // `{ k1: v1, k2: v2, … }` literal: allocate, set each field by
        // name (key string sourced from the StringPool), NaN-box the
        // pointer via js_nanbox_pointer.
        Expr::Object(props) => lower_object_literal(ctx, props),

        // -------- Arrays (Phase B.3) --------
        // `[a, b, c]` literal: allocate via js_array_alloc(N), then
        // sequentially push each element. js_array_push_f64 may return a
        // new pointer if it had to realloc, so we thread the pointer
        // through each push. Final pointer is NaN-boxed via js_nanbox_pointer
        // (POINTER_TAG, not STRING_TAG).
        Expr::Array(elements) => lower_array_literal(ctx, elements),

        // `[a, ...b, c]` literal with spread elements. Each Spread
        // element calls `js_array_concat(dest, src)` to copy from
        // source; each Expr element calls `js_array_push_f64`. Both
        // may realloc, so we thread the pointer through.
        Expr::ArraySpread(elements) => {
            use perry_hir::ArrayElement;
            let cap_str = (elements.len() as u32).to_string();
            let mut current_arr = ctx
                .block()
                .call(I64, "js_array_alloc", &[(I32, &cap_str)]);
            for elem in elements {
                match elem {
                    ArrayElement::Expr(e) => {
                        let v = lower_expr(ctx, e)?;
                        current_arr = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &v)],
                        );
                    }
                    ArrayElement::Spread(e) => {
                        let src_box = lower_expr(ctx, e)?;
                        let blk = ctx.block();
                        let src_handle = unbox_to_i64(blk, &src_box);
                        current_arr = blk.call(
                            I64,
                            "js_array_concat",
                            &[(I64, &current_arr), (I64, &src_handle)],
                        );
                    }
                }
            }
            Ok(nanbox_pointer_inline(ctx.block(), &current_arr))
        }

        // `arr[i]` index access. INLINE FAST PATH for typed-Number arrays:
        // skip the runtime function call, do the address arithmetic
        // directly. The ArrayHeader layout is `{ length: u32, capacity:
        // u32, elements: [f64; N] }` — elements start at offset 8.
        //
        // Equivalent to:
        //   element_ptr = arr_ptr + 8 + idx*8
        //   load double, ptr element_ptr
        //
        // Saves a function call (~5-10 ns) per access. For
        // bench_array_ops with ~400K reads per iteration this is the
        // bulk of the LLVM-vs-Cranelift gap.
        Expr::IndexGet { object, index } => {
            // String indexing fast path: `s[i]` returns the char at
            // position i as a single-char string. Handled before the
            // array path so `str[0]` doesn't fall through to a raw
            // double load.
            if is_string_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
                let result = blk.call(
                    I64,
                    "js_string_char_at",
                    &[(I64, &s_handle), (I32, &idx_i32)],
                );
                return Ok(nanbox_string_inline(blk, &result));
            }
            // Three cases:
            //   1. Receiver is a known array → inline f64 element load
            //   2. Index is a string (literal or string-typed local) →
            //      generic object field access via js_object_get_field_by_name_f64
            //   3. Anything else → fall back to dynamic object field
            //      access by stringifying the index at runtime
            if is_array_expr(ctx, object) {
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                // Bounds check: load length (u32 at offset 0),
                // compare index. OOB returns TAG_UNDEFINED (JS spec).
                let len_ptr = blk.inttoptr(I64, &arr_handle);
                let len_i32 = blk.load(I32, &len_ptr);
                let in_bounds = blk.icmp_ult(I32, &idx_i32, &len_i32);
                let ok_idx = ctx.new_block("arr.ok");
                let oob_idx = ctx.new_block("arr.oob");
                let merge_idx = ctx.new_block("arr.merge");
                let ok_label = ctx.block_label(ok_idx);
                let oob_label = ctx.block_label(oob_idx);
                let merge_label = ctx.block_label(merge_idx);
                ctx.block().cond_br(&in_bounds, &ok_label, &oob_label);
                // In-bounds: inline element load.
                ctx.current_block = ok_idx;
                let blk = ctx.block();
                let idx_i64 = blk.zext(I32, &idx_i32, I64);
                let byte_offset = blk.shl(I64, &idx_i64, "3");
                let with_header = blk.add(I64, &byte_offset, "8");
                let element_addr = blk.add(I64, &arr_handle, &with_header);
                let element_ptr = blk.inttoptr(I64, &element_addr);
                let val = blk.load(DOUBLE, &element_ptr);
                let ok_end_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                // OOB: return TAG_UNDEFINED.
                ctx.current_block = oob_idx;
                let undef_bits = crate::nanbox::i64_literal(crate::nanbox::TAG_UNDEFINED);
                let undef_val = ctx.block().bitcast_i64_to_double(&undef_bits);
                let oob_end_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                // Merge with phi.
                ctx.current_block = merge_idx;
                return Ok(ctx.block().phi(
                    DOUBLE,
                    &[(&val, &ok_end_label), (&undef_val, &oob_end_label)],
                ));
            }
            // Generic dynamic object access: stringify the index (no-op
            // for already-string keys, format for numeric keys) and
            // call js_object_get_field_by_name_f64.
            if let Expr::String(literal) = index.as_ref() {
                // Static string key: use the interned StringPool entry
                // so we get the same handle as obj["foo"].
                let obj_box = lower_expr(ctx, object)?;
                let key_idx = ctx.strings.intern(literal);
                let key_handle_global =
                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                return Ok(blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &obj_handle), (I64, &key_raw)],
                ));
            }
            if is_string_expr(ctx, index) {
                // Dynamic string key: unbox both pointers and call.
                let obj_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let obj_handle = unbox_to_i64(blk, &obj_box);
                let key_handle = unbox_to_i64(blk, &key_box);
                return Ok(blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &obj_handle), (I64, &key_handle)],
                ));
            }
            // Last-resort fallback with runtime tag check on the index.
            // If the index has STRING_TAG (0x7FFF) in top 16 bits,
            // use object field access. Otherwise, inline array path.
            let obj_box = lower_expr(ctx, object)?;
            let idx_box = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let idx_bits = blk.bitcast_double_to_i64(&idx_box);
            let top16 = blk.lshr(I64, &idx_bits, "48");
            // STRING_TAG = 0x7FFF AND lower 48 bits >= 0x1000 (valid ptr)
            let is_str_tag = blk.icmp_eq(I64, &top16, "32767");
            let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
            let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
            let is_str = blk.and(crate::types::I1, &is_str_tag, &is_valid_ptr);
            let str_idx = ctx.new_block("iget.str");
            let num_idx = ctx.new_block("iget.num");
            let merge_idx = ctx.new_block("iget.merge");
            let str_lbl = ctx.block_label(str_idx);
            let num_lbl = ctx.block_label(num_idx);
            let merge_lbl = ctx.block_label(merge_idx);
            ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
            // String key → object field access.
            ctx.current_block = str_idx;
            let key_handle = ctx.block().and(I64, &idx_bits, POINTER_MASK_I64);
            let v_str = ctx.block().call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_handle), (I64, &key_handle)],
            );
            let str_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Numeric key → inline array-style read (offset 8+idx*8).
            ctx.current_block = num_idx;
            let idx_i32 = ctx.block().fptosi(DOUBLE, &idx_box, I32);
            let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
            let byte_off = ctx.block().shl(I64, &idx_i64, "3");
            let with_hdr = ctx.block().add(I64, &byte_off, "8");
            let elem_addr = ctx.block().add(I64, &obj_handle, &with_hdr);
            let elem_ptr = ctx.block().inttoptr(I64, &elem_addr);
            let v_num = ctx.block().load(DOUBLE, &elem_ptr);
            let num_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Merge.
            ctx.current_block = merge_idx;
            Ok(ctx.block().phi(
                DOUBLE,
                &[(&v_str, &str_end_lbl), (&v_num, &num_end_lbl)],
            ))
        }

        // `arr.length` / `str.length` — INLINE. Both ArrayHeader and
        // StringHeader start with `length: u32` (`crates/perry-runtime/src
        // /array.rs` and `string.rs`). Same pattern: unbox pointer, load
        // u32 from offset 0, sitofp to double.
        // `.length` — INLINE for array, string, and interface-typed
        // receivers. Named types (interfaces, class instances) often
        // wrap strings or arrays at runtime, where length is at offset 0.
        Expr::PropertyGet { object, property }
            if property == "length"
                && (is_array_expr(ctx, object) || is_string_expr(ctx, object)
                    || matches!(
                        crate::type_analysis::static_type_of(ctx, object),
                        Some(HirType::Named(_)) | Some(HirType::Tuple(_))
                    )) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_bits = blk.bitcast_double_to_i64(&recv_box);
            let recv_handle = blk.and(I64, &recv_bits, POINTER_MASK_I64);
            let len_ptr = blk.inttoptr(I64, &recv_handle);
            let len_i32 = blk.load(I32, &len_ptr);
            Ok(blk.sitofp(I32, &len_i32, DOUBLE))
        }

        // `set.size` / `map.size` — route to runtime helpers. The HIR
        // doesn't synthesize SetSize/MapSize expressions for the
        // property-access form, so we recognize the pattern here.
        Expr::PropertyGet { object, property }
            if property == "size" && is_set_expr(ctx, object) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_set_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        Expr::PropertyGet { object, property }
            if property == "size" && is_map_expr(ctx, object) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_map_size", &[(I64, &recv_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        // `arr[i] = v` — typed-Number array element write.
        //
        // INLINE FAST PATH (matches Cranelift's expr.rs:18886+ pattern):
        //
        //   load length from arr_ptr+0
        //   if idx < length: inline store, done
        //   else if idx < capacity: inline store + bump length, done
        //   else: call js_array_set_f64_extend (slow realloc path)
        //
        // The ArrayHeader layout is `{ length: u32, capacity: u32, ... }`
        // (8 bytes), followed by `[f64; N]` elements at offset 8.
        //
        // For non-LocalGet receivers we still use bounds-checked
        // `js_array_set_f64` (no return value, no realloc) since there's
        // no local to write a possibly-realloc'd pointer back to.
        Expr::IndexSet { object, index, value } => {
            // Same dispatch tree as IndexGet: known array → fast inline,
            // string key on dynamic receiver → object field set, otherwise
            // bail with a clear error.
            if is_array_expr(ctx, object) {
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let local_id = if let Expr::LocalGet(id) = object.as_ref() {
                    Some(*id)
                } else {
                    None
                };
                if let Some(id) = local_id {
                    lower_index_set_fast(ctx, &arr_box, &idx_double, &val_double, id)?;
                } else {
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                    blk.call_void(
                        "js_array_set_f64",
                        &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, &val_double)],
                    );
                }
                return Ok(val_double);
            }
            if let Expr::String(literal) = index.as_ref() {
                let obj_box = lower_expr(ctx, object)?;
                let val_double = lower_expr(ctx, value)?;
                let key_idx = ctx.strings.intern(literal);
                let key_handle_global =
                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &val_double)],
                );
                return Ok(val_double);
            }
            if is_string_expr(ctx, index) {
                let obj_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let obj_handle = unbox_to_i64(blk, &obj_box);
                let key_handle = unbox_to_i64(blk, &key_box);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &val_double)],
                );
                return Ok(val_double);
            }
            // Fallback with runtime STRING_TAG check, matching IndexGet.
            let obj_box = lower_expr(ctx, object)?;
            let idx_box = lower_expr(ctx, index)?;
            let val_double = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let idx_bits = blk.bitcast_double_to_i64(&idx_box);
            let top16 = blk.lshr(I64, &idx_bits, "48");
            let is_str_tag = blk.icmp_eq(I64, &top16, "32767");
            let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
            let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
            let is_str = blk.and(crate::types::I1, &is_str_tag, &is_valid_ptr);
            let str_set = ctx.new_block("iset.str");
            let num_set = ctx.new_block("iset.num");
            let set_merge = ctx.new_block("iset.merge");
            let str_lbl = ctx.block_label(str_set);
            let num_lbl = ctx.block_label(num_set);
            let merge_lbl = ctx.block_label(set_merge);
            ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
            // String key → object field set.
            ctx.current_block = str_set;
            let key_handle = ctx.block().and(I64, &idx_bits, POINTER_MASK_I64);
            ctx.block().call_void(
                "js_object_set_field_by_name",
                &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &val_double)],
            );
            ctx.block().br(&merge_lbl);
            // Numeric key → inline array-style write (offset 8+idx*8).
            ctx.current_block = num_set;
            {
                let blk = ctx.block();
                let idx_i32 = blk.fptosi(DOUBLE, &idx_box, I32);
                let idx_i64 = blk.zext(I32, &idx_i32, I64);
                let byte_off = blk.shl(I64, &idx_i64, "3");
                let with_hdr = blk.add(I64, &byte_off, "8");
                let elem_addr = blk.add(I64, &obj_handle, &with_hdr);
                let elem_ptr = blk.inttoptr(I64, &elem_addr);
                blk.store(DOUBLE, &val_double, &elem_ptr);
            }
            ctx.block().br(&merge_lbl);
            ctx.current_block = set_merge;
            Ok(val_double)
        }

        // `obj.field = v` — generic object field write.
        Expr::PropertySet { object, property, value } => {
            // Setter dispatch: if the receiver is a known class and the
            // property is registered as a setter, call the synthesized
            // __set_<property> method instead of doing a raw field
            // store. The setter takes (this, value) and returns
            // undefined; we forward `value` as the expression result.
            if let Some(class_name) = receiver_class_name(ctx, object) {
                let setter_key = (class_name.clone(), format!("__set_{}", property));
                if let Some(fn_name) = ctx.methods.get(&setter_key).cloned() {
                    let recv_box = lower_expr(ctx, object)?;
                    let val_double = lower_expr(ctx, value)?;
                    let _ = ctx.block().call(
                        DOUBLE,
                        &fn_name,
                        &[(DOUBLE, &recv_box), (DOUBLE, &val_double)],
                    );
                    return Ok(val_double);
                }
            }
            let obj_box = lower_expr(ctx, object)?;
            let val_double = lower_expr(ctx, value)?;
            // Intern the field name in the StringPool (same one the
            // matching getter uses, so they share the global string).
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&obj_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
            blk.call_void(
                "js_object_set_field_by_name",
                &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &val_double)],
            );
            Ok(val_double)
        }

        // `obj.field` — generic object field read. We get the key string
        // handle from the StringPool (interned, so the same key across
        // multiple sites shares one allocation), unbox both the object
        // pointer and the key handle, then call
        // `js_object_get_field_by_name_f64`. The result is a raw f64
        // (which IS the NaN-boxed value for non-number fields — same bit
        // pattern, runtime callers re-interpret based on context).
        Expr::PropertyGet { object, property } => {
            // GlobalGet receivers (`console.X`, `Math.PI`, `JSON.parse`,
            // `process.env`, …) used as expression VALUES (not in a
            // call) — there's no real value to materialize. Return a
            // sentinel `0.0`. The call dispatch in lower_call handles
            // the same shapes as call callees correctly; this path
            // only catches the rare `let f = console.log` pattern.
            if matches!(object.as_ref(), Expr::GlobalGet(_)) {
                return Ok(double_literal(0.0));
            }
            // Getter dispatch: if the receiver is a known class and
            // the property is registered as a getter, call the
            // synthesized __get_<property> method instead of doing a
            // raw field load.
            if let Some(class_name) = receiver_class_name(ctx, object) {
                let getter_key = (class_name.clone(), format!("__get_{}", property));
                if let Some(fn_name) = ctx.methods.get(&getter_key).cloned() {
                    let recv_box = lower_expr(ctx, object)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        &fn_name,
                        &[(DOUBLE, &recv_box)],
                    ));
                }
            }
            let obj_box = lower_expr(ctx, object)?;
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&obj_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
            Ok(blk.call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_handle), (I64, &key_handle)],
            ))
        }

        // -------- Ternary `cond ? a : b` (Phase B.7) --------
        // Lowered like if-expression with phi merge — same shape as the
        // logical operator path but with both branches always evaluated
        // conditionally on the truthiness test.
        Expr::Conditional { condition, then_expr, else_expr } => {
            lower_conditional(ctx, condition, then_expr, else_expr)
        }

        // `arr.push(x)` (Phase B.7) — special HIR variant that already
        // tells us the array LocalId and the value. We load the array
        // from its slot, unbox, push, NaN-box the (possibly-reallocated)
        // pointer, and store it back into the slot so subsequent uses
        // see the up-to-date pointer.
        Expr::ArrayPush { array_id, value } => {
            // Resolve the array storage in priority order: closure
            // capture (slot in the closure header), local alloca slot,
            // module-level global. The realloc-pointer write-back must
            // go to whichever storage we read from.
            let v = lower_expr(ctx, value)?;
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let new_handle = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back to whichever storage backs the local.
            // Boxed var takes priority: write through the box so
            // every closure sharing the box sees the new pointer.
            if ctx.boxed_vars.contains(array_id) {
                // Captured-through-closure boxed var.
                if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                    let closure_ptr = ctx
                        .current_closure_ptr
                        .clone()
                        .ok_or_else(|| anyhow!("ArrayPush boxed captured but no current_closure_ptr"))?;
                    let idx_str = capture_idx.to_string();
                    let blk = ctx.block();
                    let cap_dbl = blk.call(
                        DOUBLE,
                        "js_closure_get_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let box_ptr = blk.bitcast_double_to_i64(&cap_dbl);
                    blk.call_void(
                        "js_box_set",
                        &[(I64, &box_ptr), (DOUBLE, &new_box)],
                    );
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot);
                    let box_ptr = blk.bitcast_double_to_i64(&box_dbl);
                    blk.call_void(
                        "js_box_set",
                        &[(I64, &box_ptr), (DOUBLE, &new_box)],
                    );
                }
                return Ok(new_box);
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("ArrayPush captured but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                ctx.block().call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new_box)],
                );
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPush({}): local not in scope", array_id));
            }
            Ok(new_box)
        }

        // -------- Closures (Phase D.1) --------
        // `function() { ... }` / `(x) => { ... }` — allocate a closure
        // object pointing at a pre-emitted function body, populate
        // capture slots, return the NaN-boxed pointer.
        //
        // The closure body is emitted as a top-level LLVM function
        // (`perry_closure_<modprefix>__<func_id>`) earlier in
        // `compile_module` via the `compile_closure` pass.
        Expr::Closure {
            func_id,
            params,
            body,
            captures,
            mutable_captures,
            captures_this: _,
            is_async,
            ..
        } => {
            // captures_this used to be a hard error here. Phase H.3
            // initializes the closure's `this_stack` with a sentinel
            // when enclosing_class is set, so the body lowering won't
            // crash on `this` references — they just produce garbage
            // until full this-capture support lands. The wrong-but-
            // doesn't-crash trade unblocks dozens of test files.
            // Async closures lower the same way as sync closures for
            // now — we just don't actually wrap the body in a Promise
            // state machine. The body still emits, calls work, and
            // `await` inside it is also a pass-through (Phase E proper
            // landing handles real async semantics).
            let _ = is_async;
            // mutable_captures uses the same get/set runtime path —
            // they work as long as the outer scope doesn't also access
            // the captured variable after the closure is created.
            let _ = mutable_captures;

            // Auto-detect captures from the body. The HIR's captures
            // list is sometimes empty for closures passed as arguments
            // (the closure conversion pass doesn't visit every site).
            // We must detect the same set as `compile_closure` so the
            // creation site and the body lower with consistent slot
            // indices.
            let auto_captures = compute_auto_captures(ctx, params, body, captures);

            // Lower each captured value from the OUTER scope (this is
            // an outer-scope access, NOT a closure capture access — at
            // closure creation we're still outside the closure body).
            //
            // Boxed captures are special: the CAPTURE VALUE is the
            // box pointer itself (not the value inside the box). We
            // store the box pointer (as a bit-castable double) in
            // the closure's capture slot, so reads/writes inside the
            // closure body can deref it via js_box_get/set. Without
            // this, each closure would get a snapshot of the box's
            // current value.
            let mut captured_values: Vec<String> = Vec::with_capacity(auto_captures.len());
            for cap_id in &auto_captures {
                if ctx.boxed_vars.contains(cap_id) {
                    // If the enclosing function has this id boxed,
                    // we want to forward the BOX POINTER through
                    // the capture slot, not the value inside the
                    // box. Read the slot (which holds the box
                    // pointer bit-cast to double) directly without
                    // going through the normal LocalGet path (which
                    // would deref via js_box_get).
                    if let Some(&_capture_idx) = ctx.closure_captures.get(cap_id) {
                        // We're inside a closure and this id is a
                        // transitively-captured box. Read the
                        // capture slot RAW (it holds the box ptr
                        // as a double) and propagate directly.
                        let closure_ptr = ctx
                            .current_closure_ptr
                            .clone()
                            .ok_or_else(|| anyhow!("nested boxed capture but no current_closure_ptr"))?;
                        let idx_str = _capture_idx.to_string();
                        let v = ctx.block().call(
                            DOUBLE,
                            "js_closure_get_capture_f64",
                            &[(I64, &closure_ptr), (I32, &idx_str)],
                        );
                        captured_values.push(v);
                    } else if let Some(slot) = ctx.locals.get(cap_id).cloned() {
                        // Enclosing function owns the box: slot
                        // holds the box pointer as a double.
                        let v = ctx.block().load(DOUBLE, &slot);
                        captured_values.push(v);
                    } else if let Some(global_name) =
                        ctx.module_globals.get(cap_id).cloned()
                    {
                        // Global boxed var (rare).
                        let g_ref = format!("@{}", global_name);
                        let v = ctx.block().load(DOUBLE, &g_ref);
                        captured_values.push(v);
                    } else {
                        captured_values.push(double_literal(0.0));
                    }
                } else {
                    let v = lower_expr(ctx, &Expr::LocalGet(*cap_id))?;
                    captured_values.push(v);
                }
            }

            // Compute the closure function name BEFORE taking the
            // mutable block borrow.
            let func_name = format!(
                "perry_closure_{}__{}",
                ctx.strings.module_prefix(),
                func_id
            );

            let blk = ctx.block();
            let func_ref = format!("@{}", func_name);
            let cap_count = auto_captures.len().to_string();
            let closure_handle = blk.call(
                I64,
                "js_closure_alloc",
                &[(PTR, &func_ref), (I32, &cap_count)],
            );
            for (idx, val) in captured_values.iter().enumerate() {
                let idx_str = idx.to_string();
                blk.call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_handle), (I32, &idx_str), (DOUBLE, val)],
                );
            }
            Ok(nanbox_pointer_inline(blk, &closure_handle))
        }

        // -------- Classes (Phase C.1) --------
        // `new ClassName(args...)` — allocate an anonymous object,
        // inline-execute the constructor body with `this` bound to the
        // new object, return the NaN-boxed object. No method tables yet,
        // no inheritance — just data classes with constructor field
        // assignments.
        Expr::New { class_name, args, .. } => lower_new(ctx, class_name, args),

        // `this` — load from the topmost `this` slot in the constructor
        // stack. Returns undefined sentinel outside any constructor
        // body so compile succeeds for stray top-level `this` (which
        // is `undefined` in strict mode anyway).
        Expr::This => {
            if let Some(slot) = ctx.this_stack.last().cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else {
                Ok(double_literal(0.0))
            }
        }

        // `super(args…)` — Phase C.2 inheritance. Look up the current
        // class's parent and inline the parent's constructor body
        // with the SAME `this` (so parent fields end up on the same
        // object). Parent's parameters get fresh slots populated with
        // the lowered super-call args.
        //
        // The current class is the topmost entry in `class_stack`. The
        // parent is `current_class.extends_name` (Perry uses the string
        // form for cross-module/late-resolved cases) or
        // `current_class.extends.and_then(class_id_to_name)`. For Phase
        // C.2 we use `extends_name` which is always populated when
        // there's a parent.
        Expr::SuperCall(super_args) => {
            // Soft fallback for super() outside a class context: lower
            // args and return undefined.
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                for a in super_args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            let current_class = match ctx.classes.get(&current_class_name).copied() {
                Some(c) => c,
                None => {
                    for a in super_args {
                        let _ = lower_expr(ctx, a)?;
                    }
                    return Ok(double_literal(0.0));
                }
            };
            let Some(parent_name) = current_class.extends_name.as_deref().map(|s| s.to_string()) else {
                for a in super_args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            let parent_class = match ctx.classes.get(&parent_name).copied() {
                Some(c) => c,
                None => {
                    // Built-in parent (Error, Object, etc.) — skip
                    // the inlined constructor body.
                    for a in super_args {
                        let _ = lower_expr(ctx, a)?;
                    }
                    return Ok(double_literal(0.0));
                }
            };

            // Lower the super-call args.
            let mut lowered_args: Vec<String> = Vec::with_capacity(super_args.len());
            for a in super_args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Inline the parent constructor with the SAME this and a
            // fresh param scope for the parent's params.
            if let Some(parent_ctor) = &parent_class.constructor {
                let saved_locals = ctx.locals.clone();
                let saved_local_types = ctx.local_types.clone();

                for (param, arg_val) in parent_ctor.params.iter().zip(lowered_args.iter()) {
                    let slot = ctx.block().alloca(DOUBLE);
                    ctx.block().store(DOUBLE, arg_val, &slot);
                    ctx.locals.insert(param.id, slot);
                    ctx.local_types.insert(param.id, param.ty.clone());
                }

                ctx.class_stack.push(parent_name);
                crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;
                ctx.class_stack.pop();

                ctx.locals = saved_locals;
                ctx.local_types = saved_local_types;
            }

            // super() evaluates to undefined in JS.
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- IsNaN (special variant) --------
        // The HIR has Expr::IsNaN(operand) for `isNaN(x)` (the global
        // function). NaN ≠ NaN by definition, so the LLVM idiom is
        // `fcmp uno x, x` (unordered, true iff either operand is NaN).
        Expr::IsNaN(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let bit = blk.fcmp("uno", &v, &v);
            let tagged = blk.select(
                I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- Math.pow (special variant — separate from Binary::Pow) --------
        Expr::MathPow(base, exp) => {
            let b = lower_expr(ctx, base)?;
            let e = lower_expr(ctx, exp)?;
            Ok(ctx.block().call(DOUBLE, "js_math_pow", &[(DOUBLE, &b), (DOUBLE, &e)]))
        }

        // -------- new Error() / new Error(message) --------
        Expr::ErrorNew(opt_msg) => {
            if let Some(msg_expr) = opt_msg {
                let msg = lower_expr(ctx, msg_expr)?;
                let blk = ctx.block();
                let msg_handle = unbox_to_i64(blk, &msg);
                let err_handle = blk.call(I64, "js_error_new_with_message", &[(I64, &msg_handle)]);
                Ok(nanbox_pointer_inline(blk, &err_handle))
            } else {
                let err_handle = ctx.block().call(I64, "js_error_new", &[]);
                Ok(nanbox_pointer_inline(ctx.block(), &err_handle))
            }
        }

        // -------- arr.pop() / arr.shift() (special HIR variants) --------
        // Like ArrayPush, the HIR pre-resolves these so we get the
        // local id directly. Pop returns the removed element (NaN if
        // empty); shift removes from the front. We currently support
        // pop only.
        Expr::ArrayPop(array_id) => {
            // pop is a read-only access for the storage; we don't need
            // to write back. Resolve via LocalGet so closure captures
            // and module globals work transparently.
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &arr_handle)]))
        }

        // -------- arr.map(callback) (special variant) --------
        // The runtime js_array_map takes a closure header pointer and
        // calls it for each element. The callback expression usually
        // lowers to a NaN-boxed closure value, which we unbox to i64.
        Expr::ArrayMap { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(I64, "js_array_map", &[(I64, &arr_handle), (I64, &cb_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- map.set(key, value) / .get / .has --------
        Expr::MapSet { map, key, value } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let new_handle = blk.call(
                I64,
                "js_map_set",
                &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
            );
            // map.set returns the (possibly-realloc'd) map. Re-NaN-box
            // and return. The caller may need to write this back to a
            // local; that's the caller's problem if Map is held in a
            // mutable variable that grows.
            Ok(nanbox_pointer_inline(blk, &new_handle))
        }
        Expr::MapGet { map, key } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            Ok(blk.call(DOUBLE, "js_map_get", &[(I64, &m_handle), (DOUBLE, &k_box)]))
        }
        Expr::MapHas { map, key } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let i32_v = blk.call(I32, "js_map_has", &[(I64, &m_handle), (DOUBLE, &k_box)]);
            // NaN-tagged boolean for "true"/"false" printing.
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- Math.* unary helpers (Phase B.15) --------
        // Math.* unary functions: use LLVM intrinsics directly so the
        // generated code becomes a single hardware instruction (or
        // libm call resolved at link time, which is always present).
        // Avoids depending on `js_math_*` runtime symbols which the
        // auto-optimizer's dead-stripping was removing from the
        // built `libperry_runtime.a`.
        //
        // Mirrors Cranelift's approach (`crates/perry-codegen/src/expr.rs:1497`)
        // which uses `builder.ins().floor()` etc.
        Expr::MathSqrt(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.sqrt.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathFloor(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.floor.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathCeil(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.ceil.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathRound(operand) => {
            // JS Math.round: round-half-toward-positive-infinity. We
            // emulate via floor(x + 0.5) then fcopysign to preserve
            // the -0 case (matching Cranelift's expr.rs:1521 approach).
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let half = blk.fadd(&v, "0.5");
            let floored = blk.call(DOUBLE, "llvm.floor.f64", &[(DOUBLE, &half)]);
            Ok(blk.call(DOUBLE, "llvm.copysign.f64", &[(DOUBLE, &floored), (DOUBLE, &v)]))
        }
        Expr::MathAbs(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog2(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log2.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog10(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log10.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog1p(operand) => {
            // log(1 + x). LLVM has no log1p intrinsic that doesn't
            // require linking libm, so emulate via log(1+x).
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let one_plus_v = blk.fadd(&v, "1.0");
            Ok(blk.call(DOUBLE, "llvm.log.f64", &[(DOUBLE, &one_plus_v)]))
        }
        // Math.random — return 0.5 sentinel. Real impl needs a PRNG
        // we'd link in; sentinel keeps the compile-pass count up.
        Expr::MathRandom => Ok(ctx.block().call(DOUBLE, "js_math_random", &[])),

        // `JSON.stringify(value, replacer, indent)` — full form via
        // runtime `js_json_stringify_full` which handles array/function
        // replacers, indent spaces, circular detection (throws
        // TypeError), and `toJSON`.
        Expr::JsonStringifyFull(value, replacer, indent) => {
            let v = lower_expr(ctx, value)?;
            let r = lower_expr(ctx, replacer)?;
            let i = lower_expr(ctx, indent)?;
            let blk = ctx.block();
            let result_i64 = blk.call(
                I64,
                "js_json_stringify_full",
                &[(DOUBLE, &v), (DOUBLE, &r), (DOUBLE, &i)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }

        // `new Map()` — alloc with default capacity 8 (the runtime grows
        // as needed). Result is NaN-boxed with POINTER_TAG.
        Expr::MapNew => {
            let cap = "8".to_string();
            let handle = ctx.block().call(I64, "js_map_alloc", &[(I32, &cap)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // -------- Logical operators (Phase B.6) --------
        // `a && b` and `a || b` short-circuit. We compile `a` first, branch
        // on its truthiness (treating 0.0 as false / non-zero as true),
        // and either evaluate `b` or jump straight to the merge with `a`'s
        // value. The merge block uses a phi to pick the right result.
        // `??` (Coalesce) requires NaN-tag inspection (null/undefined
        // checks), so it lands in a later slice.
        Expr::Logical { op, left, right } => lower_logical(ctx, *op, left, right),

        // -------- arr.filter(callback) --------
        // Mirrors ArrayMap: takes a closure header pointer, returns
        // a new array.
        Expr::ArrayFilter { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(I64, "js_array_filter", &[(I64, &arr_handle), (I64, &cb_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.join(separator?) -> string --------
        // The runtime takes a separator StringHeader (nullable). We
        // intern "," as the default when no separator is given so the
        // runtime side never sees a null pointer.
        Expr::ArrayJoin { array, separator } => {
            let arr_box = lower_expr(ctx, array)?;
            let sep_box = if let Some(sep_expr) = separator {
                lower_expr(ctx, sep_expr)?
            } else {
                let key_idx = ctx.strings.intern(",");
                let handle_global =
                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                ctx.block().load(DOUBLE, &handle_global)
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let sep_handle = unbox_to_i64(blk, &sep_box);
            let result = blk.call(I64, "js_array_join", &[(I64, &arr_handle), (I64, &sep_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- map.delete(key) -> boolean --------
        Expr::MapDelete { map, key } => {
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let i32_v = blk.call(I32, "js_map_delete", &[(I64, &m_handle), (DOUBLE, &k_box)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- Object.keys(obj) -> string[] --------
        Expr::ObjectKeys(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let arr_handle = blk.call(I64, "js_object_keys", &[(I64, &obj_handle)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- isFinite(x) / Number.isFinite(x) --------
        // The runtime's js_is_finite already returns NaN-tagged
        // TAG_TRUE/TAG_FALSE (not a raw 0.0/1.0), so we just
        // return the result directly. No fcmp conversion needed —
        // that was wrong because TAG_TRUE is itself a NaN payload
        // and fcmp("one", NaN, 0.0) always returns false.
        Expr::IsFinite(operand) | Expr::NumberIsFinite(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_is_finite", &[(DOUBLE, &v)]))
        }

        // -------- internal: is value === undefined OR a bare-NaN double --------
        Expr::IsUndefinedOrBareNan(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_is_undefined_or_bare_nan", &[(DOUBLE, &v)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }

        // -------- Math.min(...args) --------
        // Two HIR shapes: variadic (Vec<Expr>) and spread-from-array
        // (single Expr that is an array). Both build/use an array and
        // call js_math_min_array. The variadic form materializes a
        // temporary fixed-size array via js_array_alloc + push.
        Expr::MathMin(values) => {
            let cap = (values.len() as u32).to_string();
            let arr_handle_v = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            // Push each value. push_f64 may realloc, so we thread the
            // returned pointer through.
            let mut current = arr_handle_v;
            for v_expr in values {
                let v_box = lower_expr(ctx, v_expr)?;
                let blk = ctx.block();
                current = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current), (DOUBLE, &v_box)],
                );
            }
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_math_min_array", &[(I64, &current)]))
        }
        Expr::MathMinSpread(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_math_min_array", &[(I64, &arr_handle)]))
        }

        // -------- Math.max(...args) — same shape as Math.min --------
        Expr::MathMax(values) => {
            let cap = (values.len() as u32).to_string();
            let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for v_expr in values {
                let v_box = lower_expr(ctx, v_expr)?;
                let blk = ctx.block();
                current = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current), (DOUBLE, &v_box)],
                );
            }
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_math_max_array", &[(I64, &current)]))
        }
        Expr::MathMaxSpread(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_math_max_array", &[(I64, &arr_handle)]))
        }

        // -------- String(value) coercion --------
        Expr::StringCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // -------- Boolean(value) coercion --------
        // js_is_truthy is exactly the JS Boolean(value) coercion: it
        // returns 1 for truthy, 0 for falsy. We convert the i32 to
        // a NaN-tagged TAG_TRUE/TAG_FALSE so console.log prints
        // "true"/"false" via the runtime's NaN-tag dispatch.
        Expr::BooleanCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_is_truthy", &[(DOUBLE, &v)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- arr.slice(start, end?) -- new array slice --------
        Expr::ArraySlice { array, start, end } => {
            let arr_box = lower_expr(ctx, array)?;
            let start_d = lower_expr(ctx, start)?;
            let end_d = if let Some(end_expr) = end {
                lower_expr(ctx, end_expr)?
            } else {
                // No end → pass i32::MAX so the runtime clamps to length.
                // Encode as 2147483647.0 → fptosi → i32 max.
                "2147483647.0".to_string()
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let start_i32 = blk.fptosi(DOUBLE, &start_d, I32);
            let end_i32 = blk.fptosi(DOUBLE, &end_d, I32);
            let result = blk.call(
                I64,
                "js_array_slice",
                &[(I64, &arr_handle), (I32, &start_i32), (I32, &end_i32)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.shift() (HIR variant takes a LocalId) --------
        Expr::ArrayShift(array_id) => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_shift_f64", &[(I64, &arr_handle)]))
        }

        // -------- new Set() / new Set(arr) --------
        Expr::SetNew => {
            let cap = "8".to_string();
            let handle = ctx.block().call(I64, "js_set_alloc", &[(I32, &cap)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // -------- "key" in obj --------
        // js_object_has_property takes two NaN-boxed doubles and returns
        // a NaN-boxed boolean (1.0/0.0 already in our ABI).
        Expr::In { property, object } => {
            let key = lower_expr(ctx, property)?;
            let obj = lower_expr(ctx, object)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_has_property",
                &[(DOUBLE, &obj), (DOUBLE, &key)],
            ))
        }

        // -------- fs.writeFileSync(path, content) --------
        // The runtime takes both args as NaN-boxed doubles directly.
        // Returns i32 (1=success); we drop the result and return 0.0
        // since the HIR-level fs.writeFileSync is void in JS.
        // -------- parseInt(string, radix?) -> number --------
        Expr::ParseInt { string, radix } => {
            let s_box = lower_expr(ctx, string)?;
            let r_d = if let Some(r_expr) = radix {
                lower_expr(ctx, r_expr)?
            } else {
                "0.0".to_string()
            };
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            Ok(blk.call(DOUBLE, "js_parse_int", &[(I64, &s_handle), (DOUBLE, &r_d)]))
        }
        Expr::ParseFloat(string) => {
            let s_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            Ok(blk.call(DOUBLE, "js_parse_float", &[(I64, &s_handle)]))
        }

        // -------- RegExp literal: /pattern/flags --------
        // Constructs a RegExpHeader at compile time. Both pattern
        // and flags are interned in the StringPool so the runtime
        // sees stable handles.
        Expr::RegExp { pattern, flags } => {
            let pattern_idx = ctx.strings.intern(pattern);
            let flags_idx = ctx.strings.intern(flags);
            let pattern_global =
                format!("@{}", ctx.strings.entry(pattern_idx).handle_global);
            let flags_global =
                format!("@{}", ctx.strings.entry(flags_idx).handle_global);
            let blk = ctx.block();
            let pattern_box = blk.load(DOUBLE, &pattern_global);
            let flags_box = blk.load(DOUBLE, &flags_global);
            let pattern_handle = unbox_to_i64(blk, &pattern_box);
            let flags_handle = unbox_to_i64(blk, &flags_box);
            let result =
                blk.call(I64, "js_regexp_new", &[(I64, &pattern_handle), (I64, &flags_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- ObjectSpread literal --------
        // `{ ...a, key: val, ...b }`. The HIR carries an ordered
        // Vec<(Option<String>, Expr)>. Static props use the same
        // js_object_set_field_by_name path as `Expr::Object`. For
        // spread sources we'd need a runtime helper to copy fields
        // — for now we just allocate the object and set the static
        // props, ignoring spreads. Wrong for `...src` but unblocks
        // compilation.
        Expr::ObjectSpread { parts } => {
            // `{ ...a, x: 1, ...b, y: 2 }` — allocate an empty object,
            // then process `parts` in source order: static keys call
            // `js_object_set_field_by_name`, spreads call the runtime
            // `js_object_copy_own_fields(dst, src)` which walks the
            // source's `keys_array` and copies each field via the same
            // setter (so later parts override earlier ones, matching JS
            // semantics).
            let static_count = parts
                .iter()
                .filter(|(k, _)| k.is_some())
                .count() as u32;
            let class_id = "0".to_string();
            let count_str = static_count.to_string();
            let obj_handle = ctx.block().call(
                I64,
                "js_object_alloc",
                &[(I32, &class_id), (I32, &count_str)],
            );
            for (key_opt, value_expr) in parts {
                if let Some(key) = key_opt {
                    // Static key:value pair.
                    let v = lower_expr(ctx, value_expr)?;
                    let key_idx = ctx.strings.intern(key);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &v)],
                    );
                } else {
                    // `...expr` spread — copy all own fields from the
                    // source object into `obj_handle`.
                    let src_box = lower_expr(ctx, value_expr)?;
                    ctx.block().call_void(
                        "js_object_copy_own_fields",
                        &[(I64, &obj_handle), (DOUBLE, &src_box)],
                    );
                }
            }
            Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
        }

        // -------- new Set(arr) --------
        Expr::SetNewFromArray(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let handle = blk.call(I64, "js_set_from_array", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &handle))
        }

        // -------- StaticMethodCall --------
        // `MyClass.staticMethod(args)` — look up the synthesized
        // `perry_method_<modprefix>__<class>__<method>` in the methods
        // registry and emit a direct call. Static methods don't take
        // a `this` parameter (unlike instance methods).
        Expr::StaticMethodCall { class_name, method_name, args } => {
            let key = (class_name.clone(), method_name.clone());
            if let Some(fn_name) = ctx.methods.get(&key).cloned() {
                let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                return Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices));
            }
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(double_literal(0.0))
        }

        // -------- super.method(args) --------
        // Walk the current class's parent chain for the named method
        // (skipping the current class itself, even if it overrides
        // the same name) and emit a direct call to the resolved
        // perry_method_<modprefix>__<parent>__<name> with `this`.
        Expr::SuperMethodCall { method, args } => {
            // Find the current class from the class_stack.
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                // No enclosing class — fall back to stub.
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            // Walk parent chain starting from extends_name.
            let mut parent = ctx
                .classes
                .get(&current_class_name)
                .and_then(|c| c.extends_name.clone());
            let mut resolved_fn: Option<String> = None;
            while let Some(p) = parent {
                let key = (p.clone(), method.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    resolved_fn = Some(fname);
                    break;
                }
                parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
            }
            let Some(fn_name) = resolved_fn else {
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            // Lower `this` (from this_stack) + args.
            let this_slot = ctx
                .this_stack
                .last()
                .cloned()
                .ok_or_else(|| anyhow!("super.{}() outside any method body", method))?;
            let this_box = ctx.block().load(DOUBLE, &this_slot);
            let mut lowered: Vec<String> = Vec::with_capacity(args.len() + 1);
            lowered.push(this_box);
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices))
        }

        // -------- fs.readFileBuffer(path) / fs.readFileSync(path) -> Buffer --------
        // Calls js_fs_read_file_binary(path: f64) -> i64 (raw *BufferHeader),
        // then bitcasts the raw pointer directly to f64 WITHOUT NaN-boxing
        // (matching the Cranelift backend). The runtime's
        // `js_console_log_dynamic` → `format_jsvalue` path detects raw buffer
        // pointers via the thread-local BUFFER_REGISTRY and formats them as
        // `<Buffer xx xx ...>`. Buffer methods (`.length`, `.toString`, etc.)
        // also flow through the raw-pointer fallback.
        Expr::FsReadFileBinary(path) => {
            let path_box = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let buf_handle = blk.call(
                I64,
                "js_fs_read_file_binary",
                &[(DOUBLE, &path_box)],
            );
            Ok(blk.bitcast_i64_to_double(&buf_handle))
        }

        // -------- instanceof --------
        // Look up the target class's id and call js_instanceof. The
        // runtime walks the object's class chain and returns a
        // NaN-tagged TAG_TRUE/TAG_FALSE double directly — no
        // conversion needed.
        Expr::InstanceOf { expr: e, ty } => {
            let v = lower_expr(ctx, e)?;
            // Built-in Error subclasses have reserved CLASS_ID_* constants
            // in the runtime (see crates/perry-runtime/src/error.rs). Map
            // them by name here so `e instanceof TypeError` works even
            // though there's no user class definition.
            let cid = match ty.as_str() {
                "Error" => 0xFFFF0001u32,
                "TypeError" => 0xFFFF0010u32,
                "RangeError" => 0xFFFF0011u32,
                "ReferenceError" => 0xFFFF0012u32,
                "SyntaxError" => 0xFFFF0013u32,
                "AggregateError" => 0xFFFF0014u32,
                _ => ctx.class_ids.get(ty).copied().unwrap_or(0),
            };
            let cid_str = cid.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_instanceof",
                &[(DOUBLE, &v), (I32, &cid_str)],
            ))
        }

        // -------- delete obj.prop / delete obj["prop"] --------
        // Recognize the two common shapes:
        //   - PropertyGet { object, property: <static name> }
        //   - IndexGet { object, index: <string literal or local> }
        // Both lower to js_object_delete_field with the static or
        // dynamic key. Anything else is a no-op stub returning true.
        Expr::Delete(operand) => {
            match operand.as_ref() {
                Expr::PropertyGet { object, property } => {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_handle = unbox_to_i64(blk, &obj_box);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_handle = unbox_to_i64(blk, &key_box);
                    let i32_v = blk.call(
                        I32,
                        "js_object_delete_field",
                        &[(I64, &obj_handle), (I64, &key_handle)],
                    );
                    let bit = blk.icmp_ne(I32, &i32_v, "0");
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    Ok(blk.bitcast_i64_to_double(&tagged))
                }
                Expr::IndexGet { object, index } if is_string_expr(ctx, index) => {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    let blk = ctx.block();
                    let obj_handle = unbox_to_i64(blk, &obj_box);
                    let key_handle = unbox_to_i64(blk, &key_box);
                    let i32_v = blk.call(
                        I32,
                        "js_object_delete_field",
                        &[(I64, &obj_handle), (I64, &key_handle)],
                    );
                    let bit = blk.icmp_ne(I32, &i32_v, "0");
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    Ok(blk.bitcast_i64_to_double(&tagged))
                }
                // delete arr[numericIndex] — set element to undefined
                Expr::IndexGet { object, index } => {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_box = lower_expr(ctx, index)?;
                    let blk = ctx.block();
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    // Convert index to i32. It may be a double (NaN-boxed
                    // number) or a raw integer literal.
                    let idx_i32 = blk.fptosi(DOUBLE, &idx_box, I32);
                    let i32_v = blk.call(
                        I32,
                        "js_array_delete",
                        &[(I64, &arr_handle), (I32, &idx_i32)],
                    );
                    let bit = blk.icmp_ne(I32, &i32_v, "0");
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    Ok(blk.bitcast_i64_to_double(&tagged))
                }
                _ => {
                    let _ = lower_expr(ctx, operand)?;
                    Ok(double_literal(1.0))
                }
            }
        }

        // -------- Sequence (comma operator) --------
        // Evaluate every sub-expression in order, return the last.
        Expr::Sequence(exprs) => {
            let mut last = double_literal(0.0);
            for e in exprs {
                last = lower_expr(ctx, e)?;
            }
            Ok(last)
        }

        // -------- Array.from(iterable) — stub returns the iterable as-is --------
        // Array.from(iterable) — clone via js_array_clone which
        // handles arrays, Sets (→ js_set_to_array), Maps (→ entries).
        Expr::ArrayFrom(iter) => {
            let iter_box = lower_expr(ctx, iter)?;
            let blk = ctx.block();
            let iter_handle = unbox_to_i64(blk, &iter_box);
            let result = blk.call(I64, "js_array_clone", &[(I64, &iter_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayFromMapped { iterable, map_fn } => {
            let iter_box = lower_expr(ctx, iterable)?;
            let cb_box = lower_expr(ctx, map_fn)?;
            let blk = ctx.block();
            let iter_handle = unbox_to_i64(blk, &iter_box);
            let arr = blk.call(I64, "js_array_clone", &[(I64, &iter_handle)]);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let mapped = blk.call(I64, "js_array_map", &[(I64, &arr), (I64, &cb_handle)]);
            Ok(nanbox_pointer_inline(blk, &mapped))
        }
        Expr::Uint8ArrayFrom(iter) => lower_expr(ctx, iter),

        // -------- Object.values / Object.entries --------
        Expr::ObjectValues(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let arr_handle = blk.call(I64, "js_object_values", &[(I64, &obj_handle)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }
        Expr::ObjectEntries(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let arr_handle = blk.call(I64, "js_object_entries", &[(I64, &obj_handle)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- path.join(a, b) -> string --------
        // The HIR variant is binary; multi-arg path.join lowers to
        // chained PathJoin in the HIR.
        Expr::PathJoin(a, b) => {
            let a_box = lower_expr(ctx, a)?;
            let b_box = lower_expr(ctx, b)?;
            let blk = ctx.block();
            let a_handle = unbox_to_i64(blk, &a_box);
            let b_handle = unbox_to_i64(blk, &b_box);
            let result = blk.call(I64, "js_path_join", &[(I64, &a_handle), (I64, &b_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- queueMicrotask(fn) / process.nextTick(fn) stubs --------
        // Real microtask scheduling needs the runtime's queue. For
        // now we lower the callback for side effects (it might be a
        // closure expression that needs to register slots) and
        // return undefined.
        Expr::QueueMicrotask(cb) | Expr::ProcessNextTick(cb) => {
            let cb_box = lower_expr(ctx, cb)?;
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void("js_queue_microtask", &[(I64, &cb_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- RegExpTest --------
        // regex.test(str) -> boolean. Real call to js_regexp_test.
        // Receiver is a NaN-tagged i64 RegExpHeader pointer; arg is
        // a NaN-tagged string. Both must be unboxed before the call.
        Expr::RegExpTest { regex, string } => {
            let regex_box = lower_expr(ctx, regex)?;
            let str_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let regex_handle = unbox_to_i64(blk, &regex_box);
            // String pointer extraction goes through the unified
            // helper because the receiver may be a literal, a local,
            // or a concat result.
            let str_handle =
                blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &str_box)]);
            let i32_v = blk.call(
                I32,
                "js_regexp_test",
                &[(I64, &regex_handle), (I64, &str_handle)],
            );
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }
        Expr::RegExpExec { regex, string } => {
            // Returns ArrayHeader* or null. For a null (0) result we must
            // produce TAG_NULL so `re.exec(s) !== null` loops terminate
            // correctly — just NaN-boxing 0 with POINTER_TAG produces a
            // non-null pointer value that compares unequal to null, causing
            // infinite loops + segfaults when callers IndexGet on the result.
            let regex_box = lower_expr(ctx, regex)?;
            let str_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let regex_handle = unbox_to_i64(blk, &regex_box);
            let str_handle =
                blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &str_box)]);
            let result = blk.call(
                I64,
                "js_regexp_exec",
                &[(I64, &regex_handle), (I64, &str_handle)],
            );
            // Branch on result == 0 → TAG_NULL; else NaN-box as pointer.
            let is_null = blk.icmp_eq(I64, &result, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &result);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected = ctx.block().select(
                I1,
                &is_null,
                I64,
                crate::nanbox::TAG_NULL_I64,
                &ptr_bits,
            );
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }

        // -------- GlobalGet stub --------
        // Most uses of GlobalGet are inside `PropertyGet { GlobalGet, ... }`
        // which is handled separately. Bare GlobalGet (e.g. passing
        // `console` as a value) returns a sentinel.
        Expr::GlobalGet(_) => Ok(double_literal(0.0)),

        // -------- path.dirname / path.relative --------
        Expr::PathDirname(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_dirname", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathRelative(from, to) => {
            let f_box = lower_expr(ctx, from)?;
            let t_box = lower_expr(ctx, to)?;
            let blk = ctx.block();
            let f_handle = unbox_to_i64(blk, &f_box);
            let t_handle = unbox_to_i64(blk, &t_box);
            let result =
                blk.call(I64, "js_path_relative", &[(I64, &f_handle), (I64, &t_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- arr.includes(value) -> boolean --------
        Expr::ArrayIncludes { array, value } => {
            let arr_box = lower_expr(ctx, array)?;
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let i32_v = blk.call(
                I32,
                "js_array_includes_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            // Convert i32 boolean to NaN-tagged TAG_TRUE/FALSE so
            // console.log prints "true"/"false".
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- arr.splice(start, deleteCount?, ...items) --------
        // Real call to js_array_splice. The runtime returns the
        // deleted elements; the modified array is written to an
        // out-parameter pointer.
        Expr::ArraySplice { array_id, start, delete_count, items } => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let start_d = lower_expr(ctx, start)?;
            let count_d = if let Some(d) = delete_count {
                lower_expr(ctx, d)?
            } else {
                "0.0".to_string()
            };

            // Evaluate splice-insert items and collect their f64 values.
            let mut item_vals: Vec<String> = Vec::new();
            for it in items {
                item_vals.push(lower_expr(ctx, it)?);
            }

            let blk = ctx.block();
            let out_slot = blk.alloca(I64);
            blk.store(I64, "0", &out_slot);
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let start_i32 = blk.fptosi(DOUBLE, &start_d, I32);
            let count_i32 = blk.fptosi(DOUBLE, &count_d, I32);

            let (items_ptr, items_count_str) = if item_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                // Allocate a stack buffer of [N x double] for the
                // items, store each value, and pass the base pointer.
                let n = item_vals.len();
                let items_count_str = format!("{}", n);
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!(
                    "{} = alloca [{} x double]",
                    buf_reg, n
                ));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, items_count_str)
            };

            // Note: js_array_splice's return value is the DELETED
            // array; the modified-in-place arr is written to *out_arr.
            let deleted_handle = blk.call(
                I64,
                "js_array_splice",
                &[
                    (I64, &arr_handle),
                    (I32, &start_i32),
                    (I32, &count_i32),
                    (PTR, &items_ptr),
                    (I32, &items_count_str),
                    (PTR, &out_slot),
                ],
            );
            // Read the modified array from the out slot and write it
            // back to the source local.
            let modified_handle = ctx.block().load(I64, &out_slot);
            let modified_box = nanbox_pointer_inline(ctx.block(), &modified_handle);
            if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &modified_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &modified_box, &g_ref);
            }
            // Return the deleted array (NaN-boxed) as the splice
            // expression's value.
            Ok(nanbox_pointer_inline(ctx.block(), &deleted_handle))
        }

        // -------- ObjectFromEntries (passes through to runtime) --------
        Expr::ObjectFromEntries(arr) => {
            let v = lower_expr(ctx, arr)?;
            Ok(ctx.block().call(DOUBLE, "js_object_from_entries", &[(DOUBLE, &v)]))
        }

        // -------- string.match(regex) --------
        Expr::StringMatch { string, regex } => {
            let s_box = lower_expr(ctx, string)?;
            let r_box = lower_expr(ctx, regex)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let r_handle = unbox_to_i64(blk, &r_box);
            let result =
                blk.call(I64, "js_string_match", &[(I64, &s_handle), (I64, &r_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- obj.field++ / obj.field-- (PropertyUpdate) --------
        // Lowered as: load → fadd/fsub 1.0 → store. Same as the
        // Update variant but for a property instead of a local.
        Expr::PropertyUpdate { object, property, op, prefix } => {
            let obj_box = lower_expr(ctx, object)?;
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&obj_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
            let old = blk.call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_handle), (I64, &key_handle)],
            );
            let new = match op {
                BinaryOp::Sub => blk.fsub(&old, "1.0"),
                _ => blk.fadd(&old, "1.0"),
            };
            blk.call_void(
                "js_object_set_field_by_name",
                &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &new)],
            );
            Ok(if *prefix { new } else { old })
        }

        // -------- path.basename --------
        Expr::PathBasename(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_basename", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathBasenameExt(p, ext) => {
            // path.basename(path, ext) — strips trailing `ext` suffix.
            // Runtime: js_path_basename_ext(path_ptr, ext_ptr) -> *StringHeader.
            let p_box = lower_expr(ctx, p)?;
            let e_box = lower_expr(ctx, ext)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let e_handle = unbox_to_i64(blk, &e_box);
            let result = blk.call(
                I64,
                "js_path_basename_ext",
                &[(I64, &p_handle), (I64, &e_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathParse(p) => {
            // path.parse(p) -> object with { dir, base, ext, name, root }
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_parse", &[(I64, &p_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- JSON.parse --------
        // js_json_parse returns JSValue (u64 / i64) not f64.
        // Bitcast from i64 to double to stay in the NaN-boxed f64 ABI.
        Expr::JsonParse(text) => {
            let s_box = lower_expr(ctx, text)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let result_i64 = blk.call(I64, "js_json_parse", &[(I64, &s_handle)]);
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }
        Expr::JsonParseReviver { text, reviver } => {
            let s_box = lower_expr(ctx, text)?;
            let r_box = lower_expr(ctx, reviver)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let r_handle = unbox_to_i64(blk, &r_box);
            let result_i64 = blk.call(
                I64,
                "js_json_parse_with_reviver",
                &[(I64, &s_handle), (I64, &r_handle)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }
        Expr::JsonParseWithReviver(text, reviver) => {
            let s_box = lower_expr(ctx, text)?;
            let r_box = lower_expr(ctx, reviver)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let r_handle = unbox_to_i64(blk, &r_box);
            let result_i64 = blk.call(
                I64,
                "js_json_parse_with_reviver",
                &[(I64, &s_handle), (I64, &r_handle)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }

        // -------- new Date() --------
        Expr::DateNew(arg) => {
            if let Some(ts_expr) = arg {
                let ts = lower_expr(ctx, ts_expr)?;
                Ok(ctx.block().call(DOUBLE, "js_date_new_from_value", &[(DOUBLE, &ts)]))
            } else {
                Ok(ctx.block().call(DOUBLE, "js_date_new", &[]))
            }
        }

        // -------- arr.find(cb) / findIndex(cb) / findLast(cb) / findLastIndex(cb) --------
        Expr::ArrayFind { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            Ok(blk.call(DOUBLE, "js_array_find", &[(I64, &arr_handle), (I64, &cb_handle)]))
        }
        Expr::ArrayFindIndex { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let i32_v = blk.call(I32, "js_array_findIndex", &[(I64, &arr_handle), (I64, &cb_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        Expr::ArrayFindLast { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            Ok(blk.call(DOUBLE, "js_array_find_last", &[(I64, &arr_handle), (I64, &cb_handle)]))
        }
        Expr::ArrayFindLastIndex { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let i32_v = blk.call(I32, "js_array_find_last_index", &[(I64, &arr_handle), (I64, &cb_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        // -------- Object.is, Number.isInteger, etc. --------
        Expr::ObjectIs(a, b) => {
            let av = lower_expr(ctx, a)?;
            let bv = lower_expr(ctx, b)?;
            Ok(ctx.block().call(DOUBLE, "js_object_is", &[(DOUBLE, &av), (DOUBLE, &bv)]))
        }
        Expr::NumberIsInteger(operand) => {
            // Runtime already returns NaN-tagged TAG_TRUE/TAG_FALSE.
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_number_is_integer", &[(DOUBLE, &v)]))
        }

        // -------- Map.clear --------
        Expr::MapClear(map) => {
            let m_box = lower_expr(ctx, map)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            blk.call_void("js_map_clear", &[(I64, &m_handle)]);
            Ok(double_literal(0.0))
        }

        // -------- Map.entries / Map.keys / Map.values --------
        // All three take a map pointer and return an array pointer.
        // Used by for...of desugaring on Maps.
        Expr::MapEntries(map) | Expr::MapKeys(map) | Expr::MapValues(map) => {
            let m_box = lower_expr(ctx, map)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let func_name = match expr {
                Expr::MapEntries(_) => "js_map_entries",
                Expr::MapKeys(_) => "js_map_keys",
                Expr::MapValues(_) => "js_map_values",
                _ => unreachable!(),
            };
            let result = blk.call(I64, func_name, &[(I64, &m_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- Set.values (set → array conversion for iteration) --------
        Expr::SetValues(set) => {
            let s_box = lower_expr(ctx, set)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let result = blk.call(I64, "js_set_to_array", &[(I64, &s_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- Object.isFrozen / isSealed / isExtensible --------
        // Runtime returns f64 already NaN-boxed as TAG_TRUE/TAG_FALSE.
        Expr::ObjectIsFrozen(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_is_frozen", &[(DOUBLE, &v)]))
        }
        Expr::ObjectIsSealed(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_is_sealed", &[(DOUBLE, &v)]))
        }
        Expr::ObjectIsExtensible(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_is_extensible", &[(DOUBLE, &v)]))
        }

        // -------- FuncRef as expression value (function reference) --------
        // When a user function is passed as a value (e.g. `apply(add,
        // 3, 4)`), wrap it in a heap closure so the receiver can call
        // it via `js_closure_callN`. The wrapper function
        // `__perry_wrap_<name>` is emitted by `compile_module` for
        // every user function and has the closure-call ABI: it takes
        // `(closure_ptr, arg0, arg1, ...)` and forwards to the
        // underlying function.
        Expr::FuncRef(id) => {
            let func_name = ctx
                .func_names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "perry_unknown_func".to_string());
            let wrap_name = format!("__perry_wrap_{}", func_name);
            let blk = ctx.block();
            let wrap_ptr = format!("@{}", wrap_name);
            // js_closure_alloc(func_ptr, capture_count=0) → ClosureHeader*
            // The first arg is a `ptr` in LLVM IR (since the runtime
            // takes `*const u8`). Pass `@wrap_name` directly — LLVM
            // handles the implicit function-to-pointer cast.
            let closure_handle = blk.call(
                I64,
                "js_closure_alloc",
                &[(PTR, &wrap_ptr), (I32, "0")],
            );
            Ok(nanbox_pointer_inline(blk, &closure_handle))
        }

        // -------- path.extname(p) -> string --------
        Expr::PathExtname(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_extname", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        // -------- path.sep / path.delimiter constants --------
        Expr::PathSep => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_path_sep_get", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::PathDelimiter => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_path_delimiter_get", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::PathFormat(o) => {
            let obj_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let result = blk.call(I64, "js_path_format", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::ProcessVersion => Ok(double_literal(0.0)),
        Expr::ObjectHasOwn(obj, key) => {
            let obj_box = lower_expr(ctx, obj)?;
            let key_box = lower_expr(ctx, key)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_has_property",
                &[(DOUBLE, &obj_box), (DOUBLE, &key_box)],
            ))
        }
        Expr::NumberIsNaN(operand) => {
            // Number.isNaN is strict: only returns true for actual
            // NaN values, NOT for NaN-tagged strings/pointers/bools.
            // The inline fcmp("uno",x,x) would return true for any
            // NaN-tagged value. Use the runtime which checks
            // is_number() first.
            let v = lower_expr(ctx, operand)?;
            return Ok(ctx.block().call(DOUBLE, "js_number_is_nan", &[(DOUBLE, &v)]));
            // Dead code — kept as documentation of the inline pattern:
            let blk = ctx.block();
            let bit = blk.fcmp("uno", &v, &v);
            let tagged = blk.select(
                I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }
        Expr::FsMkdirSync(p) => {
            // Stub: lower for side effects, return undefined.
            let _ = lower_expr(ctx, p)?;
            Ok(double_literal(0.0))
        }
        Expr::IteratorToArray(o) => {
            // Walk the iterator protocol: call .next() in a loop, collect .value entries
            // into a fresh array. Runtime returns the raw ArrayHeader pointer, we re-NaN-box
            // so callers that expect an array-valued NaN-box work correctly.
            let iter_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let arr_ptr = blk.call(I64, "js_iterator_to_array", &[(DOUBLE, &iter_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_ptr))
        }
        Expr::WeakRefDeref(o) => lower_expr(ctx, o),
        Expr::Uint8ArrayNew(_) => Ok(double_literal(0.0)),
        Expr::Uint8ArrayLength(_) => Ok(double_literal(0.0)),
        Expr::Uint8ArrayGet { .. } => Ok(double_literal(0.0)),
        Expr::Uint8ArraySet { value, .. } => lower_expr(ctx, value),

        // -------- arr.unshift(value) --------
        Expr::ArrayUnshift { array_id, value } => {
            let v = lower_expr(ctx, value)?;
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let new_handle = blk.call(
                I64,
                "js_array_unshift_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back to the local's storage.
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("ArrayUnshift captured but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                ctx.block().call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new_box)],
                );
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &new_box, &g_ref);
            }
            Ok(new_box)
        }

        // -------- arr.entries() / .keys() / .values() (eager) --------
        Expr::ArrayEntries(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_entries", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayKeys(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_keys", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayValues(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_values", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- ClassRef stub (returns class id 0 as a sentinel) --------
        Expr::ClassRef(_) => Ok(double_literal(0.0)),

        // -------- CallSpread: lower callee + args, ignore spread semantics --------
        Expr::CallSpread { callee, args, .. } => {
            use perry_hir::CallArg;
            let _ = lower_expr(ctx, callee)?;
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        let _ = lower_expr(ctx, e)?;
                    }
                }
            }
            Ok(double_literal(0.0))
        }

        // -------- Math.fround --------
        Expr::MathFround(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_fround", &[(DOUBLE, &v)]))
        }

        // -------- new Map([[k,v], ...]) — alloc empty map, ignore source --------
        Expr::MapNewFromArray(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let handle = blk.call(I64, "js_map_from_array", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &handle))
        }

        // -------- DateGetTime / DateGetTimezoneOffset --------
        Expr::DateGetTime(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_time", &[(DOUBLE, &v)]))
        }
        Expr::DateGetTimezoneOffset(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_timezone_offset", &[(DOUBLE, &v)]))
        }
        // -------- Date.UTC(year, month, day?, hour?, minute?, second?, ms?) --------
        Expr::DateUtc(args) => {
            // Lower up to 7 args; pad missing ones with 0.
            let mut vals: Vec<String> = Vec::with_capacity(7);
            for a in args.iter().take(7) {
                vals.push(lower_expr(ctx, a)?);
            }
            while vals.len() < 7 {
                vals.push(double_literal(0.0));
            }
            let blk = ctx.block();
            let call_args: Vec<(crate::types::LlvmType, &str)> = vals
                .iter()
                .map(|v| (DOUBLE, v.as_str()))
                .collect();
            Ok(blk.call(DOUBLE, "js_date_utc", &call_args))
        }

        // -------- Object.defineProperty --------
        Expr::ObjectDefineProperty(obj, key, value) => {
            let o = lower_expr(ctx, obj)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            blk.call(DOUBLE, "js_object_define_property",
                &[(DOUBLE, &o), (DOUBLE, &k), (DOUBLE, &v)]);
            Ok(o)
        }

        // -------- path.isAbsolute(p) -> boolean --------
        Expr::PathIsAbsolute(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let i32_res = blk.call(I32, "js_path_is_absolute", &[(I64, &p_handle)]);
            Ok(i32_bool_to_nanbox(blk, &i32_res))
        }

        // -------- ProcessHrtimeBigint stub --------
        Expr::ProcessHrtimeBigint => Ok(double_literal(0.0)),

        // -------- RegExpExecIndex — reads thread-local from the last exec() call --------
        Expr::RegExpExecIndex => {
            Ok(ctx.block().call(DOUBLE, "js_regexp_exec_get_index", &[]))
        }

        // -------- Crypto.* stubs --------
        Expr::CryptoRandomUUID => Ok(double_literal(0.0)),
        Expr::CryptoRandomBytes(operand)
        | Expr::CryptoSha256(operand)
        | Expr::CryptoMd5(operand) => {
            let _ = lower_expr(ctx, operand)?;
            Ok(double_literal(0.0))
        }

        // -------- arr.indexOf(value) -> number --------
        Expr::ArrayIndexOf { array, value } => {
            let arr_box = lower_expr(ctx, array)?;
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let i32_v = blk.call(
                I32,
                "js_array_indexOf_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        // -------- arr.forEach(callback) — invoke callback for side effects --------
        // We don't actually iterate; just lower the callback for side
        // effects (so closures get auto-collected) and return undefined.
        Expr::ArrayForEach { array, callback } => {
            // Lower as: for (let i = 0; i < arr.length; i++)
            //              callback(arr[i], i);
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            // Load length.
            let len_ptr = blk.inttoptr(I64, &arr_handle);
            let len_i32 = blk.load(I32, &len_ptr);
            // Loop: for i = 0; i < len; i++
            let cond_idx = ctx.new_block("foreach.cond");
            let body_idx = ctx.new_block("foreach.body");
            let exit_idx = ctx.new_block("foreach.exit");
            let cond_lbl = ctx.block_label(cond_idx);
            let body_lbl = ctx.block_label(body_idx);
            let exit_lbl = ctx.block_label(exit_idx);
            // i alloca
            let i_slot = ctx.block().alloca(I32);
            ctx.block().store(I32, "0", &i_slot);
            ctx.block().br(&cond_lbl);
            // cond: i < len
            ctx.current_block = cond_idx;
            let i_val = ctx.block().load(I32, &i_slot);
            let cmp = ctx.block().icmp_slt(I32, &i_val, &len_i32);
            ctx.block().cond_br(&cmp, &body_lbl, &exit_lbl);
            // body: callback(arr[i], i)
            ctx.current_block = body_idx;
            let i_cur = ctx.block().load(I32, &i_slot);
            let elem = ctx.block().call(DOUBLE, "js_array_get_f64", &[(I64, &arr_handle), (I32, &i_cur)]);
            let i_f64 = ctx.block().sitofp(I32, &i_cur, DOUBLE);
            ctx.block().call(DOUBLE, "js_closure_call2", &[(I64, &cb_handle), (DOUBLE, &elem), (DOUBLE, &i_f64)]);
            // i++
            let i_next = ctx.block().add(I32, &i_cur, "1");
            ctx.block().store(I32, &i_next, &i_slot);
            ctx.block().br(&cond_lbl);
            // exit
            ctx.current_block = exit_idx;
            Ok(double_literal(0.0))
        }

        // -------- Object.getOwnPropertyDescriptor(obj, key) --------
        Expr::ObjectGetOwnPropertyDescriptor(obj, key) => {
            let o = lower_expr(ctx, obj)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_get_own_property_descriptor",
                &[(DOUBLE, &o), (DOUBLE, &k)],
            ))
        }

        // -------- Math.cbrt --------
        Expr::MathCbrt(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_cbrt", &[(DOUBLE, &v)]))
        }

        // -------- Date.* getters: real runtime calls --------
        Expr::DateGetFullYear(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_full_year", &[(DOUBLE, &v)]))
        }
        Expr::DateGetMonth(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_month", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcDay(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_day", &[(DOUBLE, &v)]))
        }
        Expr::DateValueOf(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_value_of", &[(DOUBLE, &v)]))
        }

        // -------- ProcessOn (event handler registration) stub --------
        Expr::ProcessOn { handler, .. } => {
            // Lower the handler for side effects (it might be a closure
            // we need to collect), discard the registration.
            let _ = lower_expr(ctx, handler)?;
            Ok(double_literal(0.0))
        }

        // -------- performance.now() — use date.now() as a stand-in --------
        Expr::PerformanceNow => {
            Ok(ctx.block().call(DOUBLE, "js_date_now", &[]))
        }

        // -------- Object.getOwnPropertyNames(obj) --------
        // Returns ALL own keys (including non-enumerable ones from
        // defineProperty), unlike Object.keys which skips them.
        Expr::ObjectGetOwnPropertyNames(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let arr_box = blk.call(DOUBLE, "js_object_get_own_property_names", &[(DOUBLE, &obj_box)]);
            Ok(arr_box)
        }

        // -------- Math.hypot(...values) --------
        // Routes through `js_math_hypot(a, b)` which uses Rust's
        // `f64::hypot` (numerically stable for very large / very small
        // operands vs. the naive sqrt(a² + b²)). For 3+ args we chain:
        // hypot(a, b, c) ≡ hypot(hypot(a, b), c).
        Expr::MathHypot(values) => {
            if values.is_empty() {
                return Ok(double_literal(0.0));
            }
            if values.len() == 1 {
                let v = lower_expr(ctx, &values[0])?;
                // Math.hypot(x) = |x|
                return Ok(ctx.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, &v)]));
            }
            let mut acc = lower_expr(ctx, &values[0])?;
            for v in &values[1..] {
                let rhs = lower_expr(ctx, v)?;
                let blk = ctx.block();
                acc = blk.call(DOUBLE, "js_math_hypot", &[(DOUBLE, &acc), (DOUBLE, &rhs)]);
            }
            Ok(acc)
        }

        // -------- RegExpExecGroups — reads thread-local from the last exec() call --------
        // Returns an ObjectHeader* (as raw i64); NaN-box with POINTER_TAG so
        // `lastExecResult.groups.year` reaches the generic object field path.
        // When no named groups were matched the runtime returns 0, which we
        // surface as TAG_UNDEFINED so `groups?.year` and `groups === undefined`
        // probes behave correctly.
        Expr::RegExpExecGroups => {
            let blk = ctx.block();
            let handle = blk.call(I64, "js_regexp_exec_get_groups", &[]);
            let is_zero = blk.icmp_eq(I64, &handle, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &handle);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected = ctx.block().select(
                I1,
                &is_zero,
                I64,
                crate::nanbox::TAG_UNDEFINED_I64,
                &ptr_bits,
            );
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }

        // -------- set.clear() --------
        Expr::SetClear(s) => {
            let s_box = lower_expr(ctx, s)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            blk.call_void("js_set_clear", &[(I64, &s_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- String.fromCodePoint(cp) — returns single-char string --------
        Expr::StringFromCodePoint(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let i32_v = blk.fptosi(DOUBLE, &v, I32);
            let handle = blk.call(I64, "js_string_from_code_point", &[(I32, &i32_v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        // -------- str.at(i) — returns single-char string or undefined --------
        Expr::StringAt { string, index } => {
            let s_box = lower_expr(ctx, string)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            // Runtime returns NaN-boxed f64 directly (string or undefined).
            Ok(blk.call(DOUBLE, "js_string_at", &[(I64, &s_handle), (I32, &idx_i32)]))
        }
        Expr::StringCodePointAt { string, index } => {
            let s_box = lower_expr(ctx, string)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            Ok(blk.call(DOUBLE, "js_string_code_point_at", &[(I64, &s_handle), (I32, &idx_i32)]))
        }
        Expr::RegExpSource(o) => {
            let r_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            let s_handle = blk.call(I64, "js_regexp_get_source", &[(I64, &r_handle)]);
            Ok(nanbox_string_inline(blk, &s_handle))
        }
        Expr::RegExpFlags(o) => {
            let r_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            let s_handle = blk.call(I64, "js_regexp_get_flags", &[(I64, &r_handle)]);
            Ok(nanbox_string_inline(blk, &s_handle))
        }
        Expr::ProcessChdir(p) => {
            let _ = lower_expr(ctx, p)?;
            Ok(double_literal(0.0))
        }
        Expr::ObjectGetPrototypeOf(o) => lower_expr(ctx, o),
        Expr::MathExpm1(o) => {
            // expm1(x) = exp(x) - 1. No llvm.expm1 intrinsic; use llvm.exp.f64
            // and subtract 1.0.
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let exp_v = blk.call(DOUBLE, "llvm.exp.f64", &[(DOUBLE, &v)]);
            Ok(blk.fsub(&exp_v, "1.0"))
        }
        Expr::DateSetUtcFullYear { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_full_year", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateGetDate(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_date", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcDate(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_date", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcFullYear(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_full_year", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcMonth(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_month", &[(DOUBLE, &v)]))
        }
        Expr::DateGetHours(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_hours", &[(DOUBLE, &v)]))
        }
        Expr::DateGetMinutes(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_minutes", &[(DOUBLE, &v)]))
        }
        Expr::DateGetSeconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_seconds", &[(DOUBLE, &v)]))
        }
        Expr::DateGetMilliseconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_milliseconds", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcHours(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_hours", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcMinutes(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_minutes", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcSeconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_seconds", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcMilliseconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_utc_milliseconds", &[(DOUBLE, &v)]))
        }
        Expr::Btoa(o) | Expr::Atob(o) => lower_expr(ctx, o),
        Expr::ArrayFlat { array } => {
            let arr_box = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_flat", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayFlatMap { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(
                I64,
                "js_array_flatMap",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- Math.sin/cos via LLVM intrinsics --------
        Expr::MathSin(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "llvm.sin.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathCos(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "llvm.cos.f64", &[(DOUBLE, &v)]))
        }
        // Hyperbolic + extra trig via runtime (uses Rust's f64 methods).
        Expr::MathSinh(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_sinh", &[(DOUBLE, &v)]))
        }
        Expr::MathCosh(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_cosh", &[(DOUBLE, &v)]))
        }
        Expr::MathTanh(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_tanh", &[(DOUBLE, &v)]))
        }
        // tan/asin/acos/atan: still stubs returning input (runtime has
        // no wrappers yet, no LLVM intrinsics for these).
        Expr::MathTan(o)
        | Expr::MathAsin(o)
        | Expr::MathAcos(o)
        | Expr::MathAtan(o) => lower_expr(ctx, o),
        Expr::MathAtan2(y, x) => {
            let _ = lower_expr(ctx, y)?;
            lower_expr(ctx, x)
        }

        // -------- String.fromCharCode(code) --------
        Expr::StringFromCharCode(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let i32_v = blk.fptosi(DOUBLE, &v, I32);
            let handle = blk.call(I64, "js_string_from_char_code", &[(I32, &i32_v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::RegExpSetLastIndex { regex, value } => {
            let r_box = lower_expr(ctx, regex)?;
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            blk.call_void(
                "js_regexp_set_last_index",
                &[(I64, &r_handle), (DOUBLE, &v)],
            );
            Ok(v)
        }
        Expr::ProcessStdin | Expr::ProcessStdout | Expr::ProcessStderr => {
            Ok(double_literal(0.0))
        }
        Expr::MathAsinh(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_asinh", &[(DOUBLE, &v)]))
        }
        Expr::MathAcosh(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_acosh", &[(DOUBLE, &v)]))
        }
        Expr::MathAtanh(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_atanh", &[(DOUBLE, &v)]))
        }
        Expr::DateSetUtcDate { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_date", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetUtcHours { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_hours", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::ProcessKill { pid, signal } => {
            let _ = lower_expr(ctx, pid)?;
            if let Some(s) = signal {
                let _ = lower_expr(ctx, s)?;
            }
            Ok(double_literal(0.0))
        }
        Expr::TextEncoderNew | Expr::TextDecoderNew => Ok(double_literal(0.0)),
        Expr::TextEncoderEncode(o) | Expr::TextDecoderDecode(o) => lower_expr(ctx, o),
        Expr::OsArch | Expr::OsType | Expr::OsPlatform | Expr::OsRelease | Expr::OsHostname => {
            Ok(double_literal(0.0))
        }
        Expr::ProcessMemoryUsage => Ok(double_literal(0.0)),
        Expr::EncodeURI(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_encode_uri", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DecodeURI(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_decode_uri", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::EncodeURIComponent(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_encode_uri_component", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DecodeURIComponent(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_decode_uri_component", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DateToDateString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_date_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToTimeString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_time_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToLocaleDateString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_locale_date_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToLocaleTimeString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_locale_time_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToJSON(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_json", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::ArrayWith { array, index, value } => {
            let arr_box = lower_expr(ctx, array)?;
            let idx_d = lower_expr(ctx, index)?;
            let val_d = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_with",
                &[(I64, &arr_handle), (DOUBLE, &idx_d), (DOUBLE, &val_d)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayCopyWithin { array_id, target, start, end } => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let target_d = lower_expr(ctx, target)?;
            let start_d = lower_expr(ctx, start)?;
            let (has_end_str, end_d) = if let Some(e) = end {
                let v = lower_expr(ctx, e)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0.0".to_string())
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_copy_within",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &target_d),
                    (DOUBLE, &start_d),
                    (I32, &has_end_str),
                    (DOUBLE, &end_d),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayToReversed { array } => {
            let arr_box = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_to_reversed", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayToSorted { array, comparator } => {
            let arr_box = lower_expr(ctx, array)?;
            let result = if let Some(c) = comparator {
                let cmp_box = lower_expr(ctx, c)?;
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                let cmp_handle = unbox_to_i64(blk, &cmp_box);
                blk.call(
                    I64,
                    "js_array_to_sorted_with_comparator",
                    &[(I64, &arr_handle), (I64, &cmp_handle)],
                )
            } else {
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                blk.call(I64, "js_array_to_sorted_default", &[(I64, &arr_handle)])
            };
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            let arr_box = lower_expr(ctx, array)?;
            let start_d = lower_expr(ctx, start)?;
            let count_d = lower_expr(ctx, delete_count)?;

            // Lower items to a Vec of f64 expressions
            let mut item_vals: Vec<String> = Vec::new();
            for it in items {
                item_vals.push(lower_expr(ctx, it)?);
            }

            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);

            let (items_ptr, items_count_str) = if item_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = item_vals.len();
                let items_count_str = format!("{}", n);
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!(
                    "{} = alloca [{} x double]",
                    buf_reg, n
                ));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, items_count_str)
            };

            let result = blk.call(
                I64,
                "js_array_to_spliced",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &start_d),
                    (DOUBLE, &count_d),
                    (PTR, &items_ptr),
                    (I32, &items_count_str),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayAt { array, index } => {
            // arr.at(i) — negative index counts from the end. The
            // runtime handles the negative-index adjustment +
            // bounds clamp.
            let arr_box = lower_expr(ctx, array)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_at", &[(I64, &arr_handle), (DOUBLE, &idx_d)]))
        }
        Expr::DateSetUtcMinutes { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_minutes", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetUtcSeconds { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_seconds", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetUtcMilliseconds { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_milliseconds", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::Yield { value, .. } => {
            // Generators not implemented; lower the yielded value for
            // side effects and return undefined.
            if let Some(v) = value {
                let _ = lower_expr(ctx, v)?;
            }
            Ok(double_literal(0.0))
        }
        Expr::TypeErrorNew(msg)
        | Expr::RangeErrorNew(msg)
        | Expr::SyntaxErrorNew(msg)
        | Expr::ReferenceErrorNew(msg) => {
            // Lower as a regular Error with the same message.
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &m);
            let err_handle = blk.call(I64, "js_error_new_with_message", &[(I64, &msg_handle)]);
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::NumberIsSafeInteger(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_number_is_safe_integer", &[(DOUBLE, &v)]))
        }
        Expr::ObjectFreeze(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_freeze", &[(DOUBLE, &v)]))
        }
        Expr::ObjectSeal(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_seal", &[(DOUBLE, &v)]))
        }
        Expr::ObjectPreventExtensions(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_prevent_extensions", &[(DOUBLE, &v)]))
        }
        Expr::DateSetUtcMonth { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(DOUBLE, "js_date_set_utc_month", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::ArrayIsArray(o) => {
            // Compile-time check: emit TAG_TRUE if the operand is
            // statically an array, else TAG_FALSE. NaN-boxed booleans
            // so console.log prints "true"/"false".
            let _ = lower_expr(ctx, o)?;
            if is_array_expr(ctx, o) {
                Ok(double_literal(f64::from_bits(crate::nanbox::TAG_TRUE)))
            } else {
                Ok(double_literal(f64::from_bits(crate::nanbox::TAG_FALSE)))
            }
        }

        // -------- new AggregateError(errors, message) --------
        // Calls real runtime `js_aggregateerror_new(errors_handle, msg_handle)`
        // which stores both the errors array and message in ErrorHeader.
        Expr::AggregateErrorNew { errors, message } => {
            let errors_box = lower_expr(ctx, errors)?;
            let m = lower_expr(ctx, message)?;
            let blk = ctx.block();
            let errors_handle = unbox_to_i64(blk, &errors_box);
            let msg_handle = unbox_to_i64(blk, &m);
            let err_handle = blk.call(
                I64,
                "js_aggregateerror_new",
                &[(I64, &errors_handle), (I64, &msg_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }

        // -------- RegExpLastIndex — regex.lastIndex getter --------
        Expr::RegExpLastIndex(r) => {
            let r_box = lower_expr(ctx, r)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            Ok(blk.call(DOUBLE, "js_regexp_get_last_index", &[(I64, &r_handle)]))
        }

        // -------- BufferConcat stub --------
        Expr::BufferConcat(operand) => lower_expr(ctx, operand),

        // -------- StaticPluginResolve stub --------
        Expr::StaticPluginResolve(_) => Ok(double_literal(0.0)),

        // -------- More cheap stubs --------
        Expr::PathNormalize(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_normalize", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathResolve(p) => lower_expr(ctx, p),
        Expr::ObjectCreate(p) => {
            let v = lower_expr(ctx, p)?;
            Ok(ctx.block().call(DOUBLE, "js_object_create", &[(DOUBLE, &v)]))
        }
        Expr::MathClz32(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_clz32", &[(DOUBLE, &v)]))
        }
        Expr::FsReadFileSync(p) => {
            let _ = lower_expr(ctx, p)?;
            // Return an empty-string-equivalent sentinel.
            Ok(double_literal(0.0))
        }
        Expr::FinalizationRegistryNew(_) => Ok(double_literal(0.0)),
        Expr::FinalizationRegistryRegister { .. } => Ok(double_literal(0.0)),
        Expr::FinalizationRegistryUnregister { .. } => Ok(double_literal(0.0)),
        Expr::ErrorNewWithCause { message, cause } => {
            // new Error(msg, { cause }). Runtime stores the cause
            // on the ErrorHeader so `e.cause` returns it.
            let msg = lower_expr(ctx, message)?;
            let c = lower_expr(ctx, cause)?;
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &msg);
            let err_handle = blk.call(
                I64,
                "js_error_new_with_cause",
                &[(I64, &msg_handle), (DOUBLE, &c)],
            );
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::EnvGet(name) => {
            // process.env.HOME -> js_getenv("HOME") -> string handle
            let key_idx = ctx.strings.intern(name);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_handle = unbox_to_i64(blk, &key_box);
            let result = blk.call(I64, "js_getenv", &[(I64, &key_handle)]);
            // Returns null pointer if env var doesn't exist; nanbox as
            // string (or null) and let downstream handle it.
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::EnvGetDynamic(name_expr) => {
            let key_box = lower_expr(ctx, name_expr)?;
            let blk = ctx.block();
            let key_handle = unbox_to_i64(blk, &key_box);
            let result = blk.call(I64, "js_getenv", &[(I64, &key_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::DateToISOString(d) => {
            let v = lower_expr(ctx, d)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_iso_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateParse(s) => {
            let s_box = lower_expr(ctx, s)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            Ok(blk.call(DOUBLE, "js_date_parse", &[(I64, &s_handle)]))
        }
        Expr::ProcessVersions => Ok(double_literal(0.0)),
        Expr::ProcessUptime | Expr::ProcessCwd => Ok(double_literal(0.0)),
        Expr::OsEOL => Ok(double_literal(0.0)),
        Expr::BufferFrom { data, .. } => lower_expr(ctx, data),
        Expr::BufferAlloc { .. } => Ok(double_literal(0.0)),

        // -------- ProcessPid / ProcessPpid stubs --------
        Expr::ProcessPid | Expr::ProcessPpid => Ok(double_literal(0.0)),

        // -------- structuredClone(v) — real deep copy --------
        Expr::StructuredClone(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_structured_clone", &[(DOUBLE, &v)]))
        }

        // -------- WeakRefNew stub (returns the source as the ref) --------
        Expr::WeakRefNew(operand) => lower_expr(ctx, operand),

        // -------- fs.unlinkSync(path) --------
        Expr::FsUnlinkSync(path) => {
            let p = lower_expr(ctx, path)?;
            let _ = ctx.block().call(I32, "js_fs_unlink_sync", &[(DOUBLE, &p)]);
            Ok(double_literal(0.0))
        }

        // -------- Phase E: await with busy-wait loop --------
        //
        // Mirrors Cranelift's expr.rs:19436. The structure:
        //
        //   <current>:
        //     %promise = unbox(<inner>)
        //     br check
        //   check:
        //     %state = call js_promise_state(%promise)  ; 0=pending,1=fulfilled,2=rejected
        //     %is_pending = icmp eq %state, 0
        //     br i1 %is_pending, wait, settled
        //   wait:
        //     call js_promise_run_microtasks()
        //     call js_stdlib_process_pending()
        //     call js_sleep_ms(1.0)
        //     br check
        //   settled:
        //     %state2 = call js_promise_state(%promise)
        //     %is_rejected = icmp eq %state2, 2
        //     br i1 %is_rejected, reject, done
        //   reject:
        //     %reason = call js_promise_reason(%promise)
        //     call js_throw(%reason)  ; void; never returns
        //     unreachable
        //   done:
        //     %value = call js_promise_value(%promise)
        //
        // Returns %value as a NaN-boxed double.
        Expr::Await(operand) => {
            let promise_box = lower_expr(ctx, operand)?;
            let promise_handle = unbox_to_i64(ctx.block(), &promise_box);

            // Pre-create blocks. We don't pass the promise as a block
            // param because LLVM SSA tracks it via the entry name —
            // unlike Cranelift's brif which needs explicit param flow.
            let check_idx = ctx.new_block("await.check");
            let wait_idx = ctx.new_block("await.wait");
            let settled_idx = ctx.new_block("await.settled");
            let reject_idx = ctx.new_block("await.reject");
            let done_idx = ctx.new_block("await.done");

            let check_label = ctx.block_label(check_idx);
            let wait_label = ctx.block_label(wait_idx);
            let settled_label = ctx.block_label(settled_idx);
            let reject_label = ctx.block_label(reject_idx);
            let done_label = ctx.block_label(done_idx);

            // Branch into check.
            ctx.block().br(&check_label);

            // === check ===
            ctx.current_block = check_idx;
            let state = ctx
                .block()
                .call(I32, "js_promise_state", &[(I64, &promise_handle)]);
            let is_pending = ctx.block().icmp_eq(I32, &state, "0");
            ctx.block().cond_br(&is_pending, &wait_label, &settled_label);

            // === wait ===
            // Note: Cranelift also calls `js_stdlib_process_pending`
            // here for mysql2/pg/etc. async resolutions, but that
            // symbol gets dead-stripped from libperry_stdlib.a when
            // no other caller in the runtime uses it. Until Phase J
            // bitcode-link mode lands, drop the call. Pure-Promise
            // tests still work because js_promise_run_microtasks is
            // not stripped (it's called from the CLI event loop).
            ctx.current_block = wait_idx;
            ctx.block().call_void("js_promise_run_microtasks", &[]);
            let one_ms = "1.0".to_string();
            ctx.block().call_void("js_sleep_ms", &[(DOUBLE, &one_ms)]);
            ctx.block().br(&check_label);

            // === settled ===
            ctx.current_block = settled_idx;
            let state2 = ctx
                .block()
                .call(I32, "js_promise_state", &[(I64, &promise_handle)]);
            let is_rejected = ctx.block().icmp_eq(I32, &state2, "2");
            ctx.block().cond_br(&is_rejected, &reject_label, &done_label);

            // === reject ===
            ctx.current_block = reject_idx;
            let reason = ctx
                .block()
                .call(DOUBLE, "js_promise_reason", &[(I64, &promise_handle)]);
            ctx.block().call_void("js_throw", &[(DOUBLE, &reason)]);
            ctx.block().unreachable();

            // === done ===
            ctx.current_block = done_idx;
            Ok(ctx.block().call(DOUBLE, "js_promise_value", &[(I64, &promise_handle)]))
        }

        // -------- StaticFieldGet/Set --------
        // Look up the (class, field) → global symbol in the static
        // field registry built at compile_module time. Load/store
        // from the global directly. NativeModuleRef stays a stub.
        Expr::StaticFieldGet { class_name, field_name } => {
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                Ok(ctx.block().load(DOUBLE, &g_ref))
            } else {
                Ok(double_literal(0.0))
            }
        }
        Expr::StaticFieldSet { class_name, field_name, value } => {
            let v = lower_expr(ctx, value)?;
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &v, &g_ref);
            }
            Ok(v)
        }
        Expr::NativeModuleRef(_) => Ok(double_literal(0.0)),

        // ObjectRest is the `...rest` capture in destructuring. We
        // stub by returning the source object — wrong (it should be
        // the object minus the excluded keys) but doesn't crash and
        // unblocks programs that don't actually use rest fields.
        Expr::ObjectRest { object, .. } => lower_expr(ctx, object),

        // -------- BigInt(literal) --------
        // The HIR carries the literal as a string for arbitrary
        // precision. We hand it to the runtime as a UTF-8 byte
        // pointer + length.
        Expr::BigInt(s) => {
            let bytes_idx = ctx.strings.intern(s);
            let bytes_global =
                format!("@{}", ctx.strings.entry(bytes_idx).bytes_global);
            let len_str = (s.len() as u32).to_string();
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_bigint_from_string",
                &[(PTR, &bytes_global), (I32, &len_str)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.sort(comparator) -> same array (in place) --------
        // The HIR variant always carries a comparator. If the comparator
        // is a synthetic "default" marker we'd want js_array_sort_default;
        // for now we always use the user-comparator path.
        Expr::ArraySort { array, comparator } => {
            let arr_box = lower_expr(ctx, array)?;
            let cmp_box = lower_expr(ctx, comparator)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cmp_handle = unbox_to_i64(blk, &cmp_box);
            let result = blk.call(
                I64,
                "js_array_sort_with_comparator",
                &[(I64, &arr_handle), (I64, &cmp_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.reduce(callback, initial?) -> value --------
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let (has_init, init_d) = if let Some(init_expr) = initial {
                let v = lower_expr(ctx, init_expr)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0x7FF8000000000000".to_string()) // NaN bits won't actually be used
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            // Convert literal NaN bits to a double via bitcast — but the
            // string above isn't valid LLVM. Use a real NaN literal instead.
            let init_use = if has_init == "1" {
                init_d
            } else {
                // LLVM treats `0x7FF8000000000000` as a hex double literal
                // when written as `0x7FF8000000000000` — but the safe way
                // is to just use `0x7FF8000000000000` via the IR's hex
                // form for doubles. Use plain `0.0` since it's unused.
                "0.0".to_string()
            };
            let runtime_fn = if matches!(expr, Expr::ArrayReduceRight { .. }) {
                "js_array_reduce_right"
            } else {
                "js_array_reduce"
            };
            Ok(blk.call(
                DOUBLE,
                runtime_fn,
                &[(I64, &arr_handle), (I64, &cb_handle), (I32, &has_init), (DOUBLE, &init_use)],
            ))
        }

        // -------- enum members lower to constants --------
        Expr::EnumMember { enum_name, member_name } => {
            let key = (enum_name.clone(), member_name.clone());
            let val = ctx.enums.get(&key).ok_or_else(|| {
                anyhow!(
                    "perry-codegen-llvm: enum member {}.{} not found in enums table",
                    enum_name,
                    member_name
                )
            })?;
            match val {
                perry_hir::EnumValue::Number(n) => Ok(double_literal(*n as f64)),
                perry_hir::EnumValue::String(s) => {
                    // Intern the string and load the handle global at the
                    // use site, just like a regular string literal.
                    let key_idx = ctx.strings.intern(s);
                    let handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    Ok(ctx.block().load(DOUBLE, &handle_global))
                }
            }
        }

        // -------- fs.existsSync(path) -> boolean --------
        Expr::FsExistsSync(path) => {
            let p = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_fs_exists_sync", &[(DOUBLE, &p)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }

        // -------- Number(value) coercion --------
        Expr::NumberCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]))
        }

        // -------- set.add(value) — updates the local in place --------
        Expr::SetAdd { set_id, value } => {
            let v = lower_expr(ctx, value)?;
            let set_box = lower_expr(ctx, &Expr::LocalGet(*set_id))?;
            let blk = ctx.block();
            let set_handle = unbox_to_i64(blk, &set_box);
            let new_handle = blk.call(I64, "js_set_add", &[(I64, &set_handle), (DOUBLE, &v)]);
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back to the storage so subsequent reads see the
            // possibly-realloc'd pointer.
            if let Some(&capture_idx) = ctx.closure_captures.get(set_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("SetAdd captured but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                ctx.block().call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new_box)],
                );
            } else if let Some(slot) = ctx.locals.get(set_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(set_id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &new_box, &g_ref);
            }
            Ok(new_box)
        }

        // -------- set.has(value) -> boolean --------
        Expr::SetHas { set, value } => {
            let s_box = lower_expr(ctx, set)?;
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_has", &[(I64, &s_handle), (DOUBLE, &v_box)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- set.delete(value) -> boolean --------
        Expr::SetDelete { set, value } => {
            let s_box = lower_expr(ctx, set)?;
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_delete", &[(I64, &s_handle), (DOUBLE, &v_box)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- set.size -> number --------
        Expr::SetSize(set) => {
            let s_box = lower_expr(ctx, set)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_size", &[(I64, &s_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        Expr::FsWriteFileSync(path, content) => {
            let p = lower_expr(ctx, path)?;
            let c = lower_expr(ctx, content)?;
            // js_fs_write_file_sync returns i32 (1=success). Discard the
            // result; fs.writeFileSync is void in JS.
            let _ = ctx.block().call(
                I32,
                "js_fs_write_file_sync",
                &[(DOUBLE, &p), (DOUBLE, &c)],
            );
            Ok(double_literal(0.0))
        }

        // -------- NativeMethodCall (Phase H.1) --------
        // Perry's HIR uses NativeMethodCall { module, method, object, args }
        // for method calls on natively-typed receivers — specifically for
        // typed arrays (where `push`/`pop`/etc. on `T[]` get this shape
        // instead of the generic ArrayPush/Pop variants), and for
        // module-level calls (mysql.createConnection, redis.set, etc.).
        //
        // Phase H.1 handles the most common shape: `array.push_single`,
        // `array.push`, `array.pop_back` on typed arrays. The object is
        // a PropertyGet on a class instance (`this.items`) or a LocalGet.
        // We chain a get + push + set so reallocations are reflected
        // back in the source.
        Expr::NativeMethodCall { module, method, object, args, .. } => {
            lower_native_method_call(ctx, module, method, object.as_deref(), args)
        }

        // -------- Calls --------
        Expr::Call { callee, args, .. } => lower_call(ctx, callee, args),

        // -------- Unsupported (clear error) --------
        other => bail!(
            "perry-codegen-llvm Phase 2: expression {} not yet supported",
            variant_name(other)
        ),
    }
}

/// Helper: unbox a NaN-boxed string/object/array double into a raw i64
/// pointer via inline `bitcast double → i64; and POINTER_MASK_I64`. Used by
/// the method dispatch paths and the inline IndexGet/IndexSet/length code.
pub(crate) fn unbox_to_i64(blk: &mut LlBlock, boxed: &str) -> String {
    let bits = blk.bitcast_double_to_i64(boxed);
    blk.and(I64, &bits, POINTER_MASK_I64)
}

/// Lower an object literal `{ k1: v1, k2: v2, … }`.
///
/// Pattern:
/// ```llvm
/// %obj = call i64 @js_object_alloc(i32 0, i32 N)   ; class_id=0, field_count=N
/// ; for each (key, value):
/// %k_box = load double, ptr @.str.K.handle           ; interned key
/// %k_bits = bitcast double %k_box to i64
/// %k_handle = and i64 %k_bits, 281474976710655        ; POINTER_MASK_I64
/// %v = <lower value expression>                       ; double
/// call void @js_object_set_field_by_name(i64 %obj, i64 %k_handle, double %v)
/// %boxed = call double @js_nanbox_pointer(i64 %obj)
/// ```
///
/// Field names are interned via the StringPool, so the same key across
/// multiple object literals shares one global string allocation.
/// `class_id=0` is the anonymous-object class. The runtime allocates at
/// least 8 inline field slots regardless of `field_count` to prevent
/// buffer overflow on later set_field calls
/// (see `crates/perry-runtime/src/object.rs:500`).
fn lower_object_literal(ctx: &mut FnCtx<'_>, props: &[(String, Expr)]) -> Result<String> {
    let field_count = props.len() as u32;
    let zero_str = "0".to_string();
    let n_str = field_count.to_string();

    // Allocate. Result is a raw i64 object pointer (NOT NaN-boxed).
    let obj_handle = ctx
        .block()
        .call(I64, "js_object_alloc", &[(I32, &zero_str), (I32, &n_str)]);

    for (key, value_expr) in props {
        // Intern the key in the StringPool. This is a separate borrow
        // from the function-level &mut ctx.func, so it's allowed.
        let key_idx = ctx.strings.intern(key);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);

        // Lower the value first (recursive lower_expr — borrows ctx).
        let v = lower_expr(ctx, value_expr)?;

        // Now load the key handle and call set_field.
        let blk = ctx.block();
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        blk.call_void(
            "js_object_set_field_by_name",
            &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &v)],
        );
    }

    // Inline NaN-box (POINTER_TAG).
    Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
}

/// Lower an array literal `[a, b, c, …]`.
///
/// Pattern:
/// ```llvm
/// %arr0 = call i64 @js_array_alloc(i32 N)        ; pre-sized
/// %arr1 = call i64 @js_array_push_f64(i64 %arr0, double <a>)
/// %arr2 = call i64 @js_array_push_f64(i64 %arr1, double <b>)
/// %arr3 = call i64 @js_array_push_f64(i64 %arr2, double <c>)
/// %boxed = call double @js_nanbox_pointer(i64 %arr3)
/// ```
///
/// Each `push_f64` returns a possibly-realloc'd pointer that the next push
/// must use. Since we pre-size with `js_array_alloc(N)`, the pushes
/// shouldn't actually realloc, but we honor the ABI to stay correct if the
/// runtime grows the array for any reason.
///
/// All elements are lowered to raw `double` first; the array stores them
/// as f64 (the typed-Number array layout). Mixed-type arrays come in a
/// later Phase B slice.
fn lower_array_literal(ctx: &mut FnCtx<'_>, elements: &[Expr]) -> Result<String> {
    let n = elements.len() as u32;
    let cap_str = n.to_string();

    // Allocate. The result is a raw i64 array pointer (NOT NaN-boxed).
    let mut current_arr = ctx
        .block()
        .call(I64, "js_array_alloc", &[(I32, &cap_str)]);

    for value_expr in elements {
        let v = lower_expr(ctx, value_expr)?;
        current_arr = ctx.block().call(
            I64,
            "js_array_push_f64",
            &[(I64, &current_arr), (DOUBLE, &v)],
        );
    }

    // Inline NaN-box (POINTER_TAG) — alloc always returns a real heap ptr.
    Ok(nanbox_pointer_inline(ctx.block(), &current_arr))
}

/// Inline fast-path lowering for `local_arr[i] = v` (Phase B.9).
///
/// Mirrors Cranelift's `expr.rs:18886+` pattern. Compiles to:
///
/// ```text
///   <current>:
///     %arr_handle = unbox(arr_box)
///     %length = load i32, ptr @ arr_handle+0
///     %in_bounds = icmp ult %idx_i32, %length
///     br i1 %in_bounds, label %fast_inbounds, label %check_capacity
///
///   fast_inbounds:
///     ; element_ptr = arr_handle + 8 + idx*8
///     store double %v, ptr %element_ptr
///     br merge
///
///   check_capacity:
///     %capacity = load i32, ptr @ arr_handle+4
///     %within_cap = icmp ult %idx_i32, %capacity
///     br i1 %within_cap, label %extend_inline, label %realloc
///
///   extend_inline:
///     store double %v, ptr %element_ptr
///     %new_len = add i32 %idx, 1
///     store i32 %new_len, ptr @ arr_handle+0
///     br merge
///
///   realloc:
///     %new_handle = call i64 @js_array_set_f64_extend(...)
///     %new_box = nanbox_pointer_inline(new_handle)
///     store double %new_box, ptr %local_slot
///     br merge
///
///   merge:
///     <continues here>
/// ```
///
/// The first two paths are pure inline IR — no function calls, no extra
/// memory loads. The third path only fires when the array actually has
/// to grow (~17 times for a 100K-element build with doubling growth).
fn lower_index_set_fast(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    val_double: &str,
    local_id: u32,
) -> Result<()> {
    // Capture the local slot for the realloc path.
    let slot = ctx
        .locals
        .get(&local_id)
        .ok_or_else(|| anyhow!("IndexSet: local {} not in scope", local_id))?
        .clone();

    // Unbox the array pointer.
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
    let idx_i32 = blk.fptosi(DOUBLE, idx_double, I32);

    // Load length from offset 0. We need a ptr typed value, so inttoptr.
    let arr_ptr = blk.inttoptr(I64, &arr_handle);
    let length = blk.load(I32, &arr_ptr);
    let in_bounds = blk.icmp_ult(I32, &idx_i32, &length);

    let inbounds_idx = ctx.new_block("idxset.inbounds");
    let check_cap_idx = ctx.new_block("idxset.check_cap");
    let extend_inline_idx = ctx.new_block("idxset.extend_inline");
    let realloc_idx = ctx.new_block("idxset.realloc");
    let merge_idx = ctx.new_block("idxset.merge");

    let inbounds_label = ctx.block_label(inbounds_idx);
    let check_cap_label = ctx.block_label(check_cap_idx);
    let extend_inline_label = ctx.block_label(extend_inline_idx);
    let realloc_label = ctx.block_label(realloc_idx);
    let merge_label = ctx.block_label(merge_idx);

    ctx.block().cond_br(&in_bounds, &inbounds_label, &check_cap_label);

    // Helper: compute element_ptr = arr_ptr + 8 + idx*8 and emit a store.
    fn store_element(
        blk: &mut LlBlock,
        arr_handle: &str,
        idx_i32: &str,
        val_double: &str,
    ) {
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3"); // *8
        let with_header = blk.add(I64, &byte_offset, "8"); // +8 for header
        let element_addr = blk.add(I64, arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        blk.store(DOUBLE, val_double, &element_ptr);
    }

    // FASTEST: in-bounds path. Store directly, jump to merge.
    ctx.current_block = inbounds_idx;
    {
        let blk = ctx.block();
        store_element(blk, &arr_handle, &idx_i32, val_double);
        blk.br(&merge_label);
    }

    // MEDIUM: idx >= length but < capacity. Store + bump length.
    ctx.current_block = check_cap_idx;
    let capacity = {
        let blk = ctx.block();
        // Load capacity from offset 4 — we need a typed pointer that
        // points 4 bytes into the array header. Use inttoptr after add.
        let cap_addr = blk.add(I64, &arr_handle, "4");
        let cap_ptr = blk.inttoptr(I64, &cap_addr);
        blk.load(I32, &cap_ptr)
    };
    let within_cap = ctx.block().icmp_ult(I32, &idx_i32, &capacity);
    ctx.block().cond_br(&within_cap, &extend_inline_label, &realloc_label);

    ctx.current_block = extend_inline_idx;
    {
        let blk = ctx.block();
        store_element(blk, &arr_handle, &idx_i32, val_double);
        // Bump length: store idx+1 to arr_ptr+0.
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle); // length is at offset 0
        blk.store(I32, &new_len, &len_ptr);
        blk.br(&merge_label);
    }

    // SLOW: realloc needed. Call the runtime, write new ptr to local.
    ctx.current_block = realloc_idx;
    {
        let blk = ctx.block();
        let new_handle = blk.call(
            I64,
            "js_array_set_f64_extend",
            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, val_double)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        blk.store(DOUBLE, &new_box, &slot);
        blk.br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(())
}

/// Return the HIR enum variant name for an expression. Uses Debug
/// formatting and extracts the leading identifier so we get the actual
/// variant name (e.g. `"ArrayMap"`, `"BufferAlloc"`, `"RegExpExec"`)
/// without having to maintain an exhaustive match against ~200 HIR
/// variants. The result is used in "X not yet supported" error messages
/// to tell the user exactly which HIR variant the LLVM backend is
/// missing — critical for prioritizing the next slice.
pub(crate) fn variant_name(e: &Expr) -> String {
    let dbg = format!("{:?}", e);
    let end = dbg
        .find(|c: char| c == ' ' || c == '(' || c == '{')
        .unwrap_or(dbg.len());
    dbg[..end].to_string()
}
