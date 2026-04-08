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
use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp, UpdateOp};
use perry_types::Type as HirType;

use crate::block::LlBlock;
use crate::function::LlFunction;
use crate::nanbox::{double_literal, POINTER_MASK_I64, POINTER_TAG_I64, STRING_TAG_I64};
use crate::strings::StringPool;
use crate::types::{DOUBLE, I1, I32, I64};

/// Inline NaN-box of a raw heap pointer with `POINTER_TAG`.
///
/// Equivalent to `js_nanbox_pointer(ptr)` for the common case where the
/// pointer is non-null and not already NaN-tagged. The runtime function
/// (`crates/perry-runtime/src/value.rs:405`) has extra guards (null →
/// TAG_NULL, already-tagged → preserve) that we don't need in the array/
/// object hot paths because those always return fresh heap pointers from
/// `js_array_alloc` / `js_array_push_f64` / `js_array_set_f64_extend` /
/// `js_object_alloc`. Replacing the function call with two SSA ops
/// (`or` + `bitcast`) eliminates ~200ms of call overhead per
/// bench_array_ops run.
fn nanbox_pointer_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, POINTER_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Inline NaN-box of a raw string handle with `STRING_TAG`. Same rationale
/// as `nanbox_pointer_inline` — string handles from `js_string_from_bytes`
/// / `js_string_concat` are always non-null heap pointers, so the runtime
/// `js_nanbox_string` guards are never hit in our hot paths.
fn nanbox_string_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, STRING_TAG_I64);
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
        Expr::LocalGet(id) => {
            let slot = ctx
                .locals
                .get(id)
                .ok_or_else(|| anyhow!("LocalGet({}): local not in scope", id))?
                .clone();
            Ok(ctx.block().load(DOUBLE, &slot))
        }

        // `total = expr` — store the new value into the local's alloca slot
        // and return it (matches JS semantics: assignment is an expression
        // whose value is the assigned value).
        Expr::LocalSet(id, value) => {
            let v = lower_expr(ctx, value)?;
            let slot = ctx
                .locals
                .get(id)
                .ok_or_else(|| anyhow!("LocalSet({}): local not in scope", id))?
                .clone();
            ctx.block().store(DOUBLE, &v, &slot);
            Ok(v)
        }

        // `i++` / `++i` / `i--` / `--i`. Postfix returns the OLD value,
        // prefix returns the NEW value. Inside a for-loop update slot the
        // result is discarded, but we honor JS semantics in case it's used
        // somewhere like `let x = i++`.
        Expr::Update { id, op, prefix } => {
            let slot = ctx
                .locals
                .get(id)
                .ok_or_else(|| anyhow!("Update({}): local not in scope", id))?
                .clone();
            let blk = ctx.block();
            let old = blk.load(DOUBLE, &slot);
            let new = match op {
                UpdateOp::Increment => blk.fadd(&old, "1.0"),
                UpdateOp::Decrement => blk.fsub(&old, "1.0"),
            };
            blk.store(DOUBLE, &new, &slot);
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
            let l = lower_expr(ctx, left)?;
            let r = lower_expr(ctx, right)?;
            let blk = ctx.block();
            let v = match op {
                BinaryOp::Add => blk.fadd(&l, &r),
                BinaryOp::Sub => blk.fsub(&l, &r),
                BinaryOp::Mul => blk.fmul(&l, &r),
                BinaryOp::Div => blk.fdiv(&l, &r),
                BinaryOp::Mod => blk.frem(&l, &r),
                other => bail!(
                    "perry-codegen-llvm Phase A: BinaryOp::{:?} not yet supported",
                    other
                ),
            };
            Ok(v)
        }

        // -------- Comparison --------
        // LLVM `fcmp` returns `i1`. We zext to double so the value fits the
        // standard number ABI used by the rest of the codegen — JS "true"
        // round-trips through numeric contexts as 1.0 and "false" as 0.0,
        // which is what Perry's runtime expects from typed boolean returns.
        Expr::Compare { op, left, right } => {
            let l = lower_expr(ctx, left)?;
            let r = lower_expr(ctx, right)?;
            let pred = match op {
                CompareOp::Eq | CompareOp::LooseEq => "oeq",
                CompareOp::Ne | CompareOp::LooseNe => "one",
                CompareOp::Lt => "olt",
                CompareOp::Le => "ole",
                CompareOp::Gt => "ogt",
                CompareOp::Ge => "oge",
            };
            let blk = ctx.block();
            let bit = blk.fcmp(pred, &l, &r);
            // `bit` is `i1`; zext to `i64` then sitofp to `double` so that
            // downstream consumers see a canonical 0.0/1.0 double.
            let as_i64 = blk.zext(crate::types::I1, &bit, crate::types::I64);
            Ok(blk.sitofp(crate::types::I64, &as_i64, DOUBLE))
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
            if !is_array_expr(ctx, object) {
                bail!(
                    "perry-codegen-llvm Phase B.9: IndexGet receiver must be a known array (got {})",
                    variant_name(object)
                );
            }
            let arr_box = lower_expr(ctx, object)?;
            let idx_double = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(&arr_box);
            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
            // Inline address arithmetic. uextend to i64, shift left 3
            // (multiply by 8), add 8 for the ArrayHeader, add to base.
            let idx_i64 = blk.zext(I32, &idx_i32, I64);
            let byte_offset = blk.shl(I64, &idx_i64, "3");
            let with_header = blk.add(I64, &byte_offset, "8");
            let element_addr = blk.add(I64, &arr_handle, &with_header);
            let element_ptr = blk.inttoptr(I64, &element_addr);
            Ok(blk.load(DOUBLE, &element_ptr))
        }

        // `arr.length` / `str.length` — INLINE. Both ArrayHeader and
        // StringHeader start with `length: u32` (`crates/perry-runtime/src
        // /array.rs` and `string.rs`). Same pattern: unbox pointer, load
        // u32 from offset 0, sitofp to double.
        Expr::PropertyGet { object, property }
            if property == "length"
                && (is_array_expr(ctx, object) || is_string_expr(ctx, object)) =>
        {
            let recv_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let recv_bits = blk.bitcast_double_to_i64(&recv_box);
            let recv_handle = blk.and(I64, &recv_bits, POINTER_MASK_I64);
            let len_ptr = blk.inttoptr(I64, &recv_handle);
            let len_i32 = blk.load(I32, &len_ptr);
            Ok(blk.sitofp(I32, &len_i32, DOUBLE))
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
            if !is_array_expr(ctx, object) {
                bail!(
                    "perry-codegen-llvm Phase B.9: IndexSet receiver must be a known array (got {})",
                    variant_name(object)
                );
            }
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
            // Assignment expressions evaluate to the assigned value.
            Ok(val_double)
        }

        // `obj.field = v` — generic object field write.
        Expr::PropertySet { object, property, value } => {
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
        Expr::PropertyGet { object, property } if !matches!(object.as_ref(), Expr::GlobalGet(_)) => {
            // The `!matches!` guard avoids stealing the `console.log`
            // dispatch path (which has `object: GlobalGet(0)` for the
            // `console` global) — that's still owned by `lower_call`.
            let obj_box = lower_expr(ctx, object)?;
            // Intern the field name and load its handle from the pool.
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
            let slot = ctx
                .locals
                .get(array_id)
                .ok_or_else(|| anyhow!("ArrayPush({}): local not in scope", array_id))?
                .clone();
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let arr_box = blk.load(DOUBLE, &slot);
            let arr_bits = blk.bitcast_double_to_i64(&arr_box);
            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
            let new_handle = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            // Inline nanbox_pointer — push always returns a real heap ptr.
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write the (possibly-reallocated) pointer back to the local
            // slot — without this, subsequent reads would use the stale
            // pre-realloc pointer and crash on access.
            blk.store(DOUBLE, &new_box, &slot);
            Ok(new_box)
        }

        // -------- Logical operators (Phase B.6) --------
        // `a && b` and `a || b` short-circuit. We compile `a` first, branch
        // on its truthiness (treating 0.0 as false / non-zero as true),
        // and either evaluate `b` or jump straight to the merge with `a`'s
        // value. The merge block uses a phi to pick the right result.
        // `??` (Coalesce) requires NaN-tag inspection (null/undefined
        // checks), so it lands in a later slice.
        Expr::Logical { op, left, right } => lower_logical(ctx, *op, left, right),

        // -------- Calls --------
        Expr::Call { callee, args, .. } => lower_call(ctx, callee, args),

        // -------- Unsupported (clear error) --------
        other => bail!(
            "perry-codegen-llvm Phase 2: expression {} not yet supported",
            variant_name(other)
        ),
    }
}

/// Lower a `Call` expression. Two shapes are supported:
/// 1. `FuncRef(id)(args...)` — direct call to a user function by HIR id.
/// 2. `console.log(expr)` where `expr` lowers to a double — emits a
///    `js_console_log_number` call and returns `0.0` as the statement value.
fn lower_call(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    // User function call via FuncRef.
    if let Expr::FuncRef(fid) = callee {
        let fname = ctx
            .func_names
            .get(fid)
            .ok_or_else(|| anyhow!("FuncRef({}): function name not resolved", fid))?
            .clone();

        // Lower all arguments first.
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();

        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // console.log(<numeric expr>) sink.
    if let Expr::PropertyGet { object, property } = callee {
        if matches!(object.as_ref(), Expr::GlobalGet(_)) && property == "log" {
            if args.len() != 1 {
                bail!(
                    "perry-codegen-llvm Phase A: console.log expects 1 arg, got {}",
                    args.len()
                );
            }
            // For statically-known number literals, take the optimized
            // `js_console_log_number` path which prints the f64 directly
            // without going through the NaN-tag dispatch. For everything
            // else (string literals, computed values whose runtime type
            // we don't track at codegen time, locals from union types),
            // route through `js_console_log_dynamic` which inspects the
            // NaN tag at runtime and dispatches to the right printer.
            //
            // js_console_log_dynamic falls through to the regular-number
            // printer when the value isn't NaN-tagged, so passing a raw
            // f64 (e.g. fibonacci(40)'s 102334155.0) still prints
            // correctly — verified in `crates/perry-runtime/src/builtins.rs:81`.
            let arg = &args[0];
            let is_number_literal = matches!(arg, Expr::Integer(_) | Expr::Number(_));
            let v = lower_expr(ctx, arg)?;
            let runtime_fn = if is_number_literal {
                "js_console_log_number"
            } else {
                "js_console_log_dynamic"
            };
            ctx.block().call_void(runtime_fn, &[(DOUBLE, &v)]);
            // console.log returns undefined. Phase A has no notion of
            // undefined; we return 0.0 as a sentinel — it's only valid
            // inside an Expr statement and the caller discards it.
            return Ok("0.0".to_string());
        }
    }

    bail!(
        "perry-codegen-llvm Phase 2: Call callee shape not supported ({})",
        variant_name(callee)
    )
}

/// Statically determine whether an expression evaluates to a real numeric
/// `double` (NOT a NaN-boxed value). Used by `lower_truthy` to decide
/// between the fast `fcmp one cond, 0.0` test and the runtime
/// `js_is_truthy` dispatch.
///
/// Recognizes:
/// - integer/number literals
/// - LocalGet of `Number`/`Int32`-typed locals
/// - arithmetic Binary / Compare results (always raw doubles in our model)
/// - the value of an Update (++/--) — also a raw double
///
/// CRUCIALLY excludes Bool, String, Array, Object — those produce
/// NaN-tagged doubles where `fcmp` is unsafe (NaN is unordered).
pub(crate) fn is_numeric_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Number) | Some(HirType::Int32)
        ),
        Expr::Binary { .. } | Expr::Compare { .. } | Expr::Update { .. } => true,
        Expr::DateNow => true,
        _ => false,
    }
}

/// Convert a lowered condition value to an `i1` for `cond_br`.
///
/// Fast path: if the expression is statically a numeric double, emit
/// `fcmp one cond, 0.0` (5-cycle ALU op).
///
/// Slow path: for everything else (booleans, strings, objects, unions),
/// dispatch through `js_is_truthy(double) -> i32` which inspects the
/// NaN tag to handle null/undefined/false correctly. The slow path is a
/// function call but produces correct results across the entire JS
/// truthiness table.
pub(crate) fn lower_truthy(ctx: &mut FnCtx<'_>, cond_val: &str, cond_expr: &Expr) -> String {
    if is_numeric_expr(ctx, cond_expr) {
        ctx.block().fcmp("one", cond_val, "0.0")
    } else {
        let i32_truthy = ctx
            .block()
            .call(I32, "js_is_truthy", &[(DOUBLE, cond_val)]);
        ctx.block().icmp_ne(I32, &i32_truthy, "0")
    }
}

/// Statically determine whether an expression is a string. Conservative —
/// returns `false` for anything that requires type information we don't
/// track (function-call returns, dynamic property access).
///
/// Recognizes:
/// - literal strings (`"foo"`)
/// - LocalGet of string-typed locals (params with `: string`, `let x = "a"`)
/// - recursive Add of strings (`"a" + "b" + s`)
fn is_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) => true,
        Expr::LocalGet(id) => matches!(ctx.local_types.get(id), Some(HirType::String)),
        Expr::Binary { op: BinaryOp::Add, left, right } => {
            is_string_expr(ctx, left) && is_string_expr(ctx, right)
        }
        _ => false,
    }
}

/// Lower `cond ? then_expr : else_expr` to a 4-block CFG with a phi at
/// the merge: condition → conditional cond_br → then → merge ← else.
/// Both then and else are always lowered (no short-circuit), but only one
/// runs at runtime depending on the condition.
fn lower_conditional(
    ctx: &mut FnCtx<'_>,
    condition: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Result<String> {
    let cond = lower_expr(ctx, condition)?;
    let cond_bool = lower_truthy(ctx, &cond, condition);

    let then_idx = ctx.new_block("ternary.then");
    let else_idx = ctx.new_block("ternary.else");
    let merge_idx = ctx.new_block("ternary.merge");

    let then_label = ctx.block_label(then_idx);
    let else_label = ctx.block_label(else_idx);
    let merge_label = ctx.block_label(merge_idx);

    ctx.block().cond_br(&cond_bool, &then_label, &else_label);

    ctx.current_block = then_idx;
    let then_val = lower_expr(ctx, then_expr)?;
    let then_after_label = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = else_idx;
    let else_val = lower_expr(ctx, else_expr)?;
    let else_after_label = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[(&then_val, &then_after_label), (&else_val, &else_after_label)],
    ))
}

/// Lower `a && b` / `a || b` with short-circuit evaluation.
///
/// Pattern (for `&&` — `||` swaps the cond_br targets):
/// ```llvm
///   ; <current>: evaluate left, branch on truthiness
///   %l = <lower left>
///   %lb = fcmp one double %l, 0.0
///   br i1 %lb, label %then, label %merge
/// then:
///   %r = <lower right>
///   br label %merge
/// merge:
///   %result = phi double [ %l, %left_block ], [ %r, %right_block ]
/// ```
///
/// The phi predecessors are captured AFTER lowering each side, because
/// `lower_expr` may itself create new blocks (nested if/logical/etc.) and
/// the actual incoming block is the last block of that subexpression's
/// codegen, not the original entry block we started in.
///
/// `??` (Coalesce) needs runtime null/undefined NaN-tag checks via
/// `js_is_truthy` or a dedicated `js_is_nullish` helper — deferred.
fn lower_logical(
    ctx: &mut FnCtx<'_>,
    op: LogicalOp,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    if matches!(op, LogicalOp::Coalesce) {
        bail!("perry-codegen-llvm Phase B.6: `??` (nullish coalesce) not yet supported");
    }

    // Lower left in the current block.
    let l = lower_expr(ctx, left)?;
    // Capture the post-left block — left's lowering may have created new
    // blocks via nested control flow.
    let l_block_label = ctx.block().label.clone();
    // Truthiness test: fast fcmp for numeric, js_is_truthy for NaN-boxed.
    let l_bool = lower_truthy(ctx, &l, left);

    let then_idx = ctx.new_block("logical.then");
    let merge_idx = ctx.new_block("logical.merge");
    let then_label = ctx.block_label(then_idx);
    let merge_label = ctx.block_label(merge_idx);

    match op {
        LogicalOp::And => {
            // a && b: if a true, evaluate b; otherwise short-circuit to merge
            ctx.block().cond_br(&l_bool, &then_label, &merge_label);
        }
        LogicalOp::Or => {
            // a || b: if a true, short-circuit to merge; otherwise evaluate b
            ctx.block().cond_br(&l_bool, &merge_label, &then_label);
        }
        LogicalOp::Coalesce => unreachable!("guarded above"),
    }

    // The "then" block evaluates the right side.
    ctx.current_block = then_idx;
    let r = lower_expr(ctx, right)?;
    let r_block_label = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Merge block: phi between l (short-circuit path) and r (normal path).
    ctx.current_block = merge_idx;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&l, &l_block_label), (&r, &r_block_label)]))
}

/// Statically determine whether an expression is an array. Used for
/// dispatch on `arr.length` and `arr[i]`.
fn is_array_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Array(_) => true,
        Expr::LocalGet(id) => matches!(ctx.local_types.get(id), Some(HirType::Array(_))),
        _ => false,
    }
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

/// Lower `string + non_string` (or vice versa) concat with runtime
/// coercion of the non-string side. The non-string operand passes through
/// `js_jsvalue_to_string` which inspects its NaN tag and produces the
/// canonical JS string form (numbers via the formatter at
/// `crates/perry-runtime/src/value.rs:825`, booleans → `"true"`/`"false"`,
/// objects → `"[object Object]"`, etc.).
///
/// The string-typed side still uses the fast inline `bitcast double → i64;
/// and POINTER_MASK_I64` unbox; only the non-string side pays the function
/// call. Both operand handles then feed `js_string_concat`.
fn lower_string_coerce_concat(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    l_is_string: bool,
    r_is_string: bool,
) -> Result<String> {
    let l_box = lower_expr(ctx, left)?;
    let r_box = lower_expr(ctx, right)?;
    let blk = ctx.block();

    let l_handle = if l_is_string {
        let bits = blk.bitcast_double_to_i64(&l_box);
        blk.and(I64, &bits, POINTER_MASK_I64)
    } else {
        blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &l_box)])
    };

    let r_handle = if r_is_string {
        let bits = blk.bitcast_double_to_i64(&r_box);
        blk.and(I64, &bits, POINTER_MASK_I64)
    } else {
        blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &r_box)])
    };

    let result_handle = blk.call(
        I64,
        "js_string_concat",
        &[(I64, &l_handle), (I64, &r_handle)],
    );
    Ok(nanbox_string_inline(blk, &result_handle))
}

/// Lower a static `s1 + s2` string concatenation. Both operands must
/// already be statically string-typed (caller's responsibility — see
/// `is_string_expr`).
///
/// Pattern:
/// ```llvm
/// ; %l_box and %r_box are NaN-boxed strings (double values with STRING_TAG)
/// %l_bits = bitcast double %l_box to i64
/// %l_handle = and i64 %l_bits, 281474976710655   ; POINTER_MASK_I64
/// %r_bits = bitcast double %r_box to i64
/// %r_handle = and i64 %r_bits, 281474976710655
/// %result_handle = call i64 @js_string_concat(i64 %l_handle, i64 %r_handle)
/// %result_box = call double @js_nanbox_string(i64 %result_handle)
/// ```
///
/// The bitcast+and is the inline-fast unboxing pattern. We avoid calling
/// the slower `js_nanbox_get_pointer` (which does the same thing in Rust)
/// to keep concat hot-path overhead minimal.
fn lower_string_concat(ctx: &mut FnCtx<'_>, left: &Expr, right: &Expr) -> Result<String> {
    let l_box = lower_expr(ctx, left)?;
    let r_box = lower_expr(ctx, right)?;
    let blk = ctx.block();
    let l_bits = blk.bitcast_double_to_i64(&l_box);
    let l_handle = blk.and(I64, &l_bits, POINTER_MASK_I64);
    let r_bits = blk.bitcast_double_to_i64(&r_box);
    let r_handle = blk.and(I64, &r_bits, POINTER_MASK_I64);
    let result_handle = blk.call(
        I64,
        "js_string_concat",
        &[(I64, &l_handle), (I64, &r_handle)],
    );
    // Inline NaN-box (STRING_TAG) — concat always returns a real heap ptr.
    Ok(nanbox_string_inline(blk, &result_handle))
}

pub(crate) fn variant_name(e: &Expr) -> &'static str {
    match e {
        Expr::Undefined => "Undefined",
        Expr::Null => "Null",
        Expr::Bool(_) => "Bool",
        Expr::Number(_) => "Number",
        Expr::Integer(_) => "Integer",
        Expr::BigInt(_) => "BigInt",
        Expr::String(_) => "String",
        Expr::I18nString { .. } => "I18nString",
        Expr::LocalGet(_) => "LocalGet",
        Expr::LocalSet(_, _) => "LocalSet",
        Expr::GlobalGet(_) => "GlobalGet",
        Expr::GlobalSet(_, _) => "GlobalSet",
        Expr::Update { .. } => "Update",
        Expr::Binary { .. } => "Binary",
        Expr::Unary { .. } => "Unary",
        Expr::Compare { .. } => "Compare",
        Expr::Logical { .. } => "Logical",
        Expr::Call { .. } => "Call",
        Expr::CallSpread { .. } => "CallSpread",
        Expr::FuncRef(_) => "FuncRef",
        Expr::ExternFuncRef { .. } => "ExternFuncRef",
        Expr::NativeModuleRef(_) => "NativeModuleRef",
        Expr::NativeMethodCall { .. } => "NativeMethodCall",
        Expr::PropertyGet { .. } => "PropertyGet",
        Expr::PropertySet { .. } => "PropertySet",
        Expr::PropertyUpdate { .. } => "PropertyUpdate",
        _ => "<other>",
    }
}
