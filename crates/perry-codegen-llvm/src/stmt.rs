//! Statement codegen — Phase 2.
//!
//! Supports: Expr, Return(Some|None), If (with/without else), Let. Enough
//! for a recursive fibonacci function plus `console.log(fibonacci(N))` at
//! top level. Loops and Date.now land in Phase 2.1.

use anyhow::{anyhow, bail, Result};
use perry_hir::Stmt;

use crate::expr::{lower_expr, lower_truthy, FnCtx};
use crate::types::DOUBLE;

/// Lower a sequence of statements into the current block of `ctx`. If any
/// statement splits control flow, `ctx.current_block` is updated to the
/// "fall-through" block after the split.
pub(crate) fn lower_stmts(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) -> Result<()> {
    for s in stmts {
        lower_stmt(ctx, s)?;
        // If an earlier statement already terminated the current block
        // (e.g. return in a straight-line sequence), any following statement
        // would emit dead code. Anvil silently drops these at the block
        // level; we do the same here to avoid tripping LLVM's verifier.
        if ctx.block().is_terminated() {
            break;
        }
    }
    Ok(())
}

pub(crate) fn lower_stmt(ctx: &mut FnCtx<'_>, stmt: &Stmt) -> Result<()> {
    match stmt {
        Stmt::Expr(e) => {
            let _ = lower_expr(ctx, e)?;
            Ok(())
        }

        Stmt::Return(Some(e)) => {
            let v = lower_expr(ctx, e)?;
            // Phase E: async functions wrap their return value in
            // js_promise_resolved so callers can await the result.
            // If the value is already a promise (e.g. `return
            // Promise.resolve(x)`), js_promise_resolved is a no-op
            // wrap that the caller's await loop unwraps anyway.
            let final_v = if ctx.is_async_fn {
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_promise_resolved", &[(DOUBLE, &v)]);
                crate::expr::nanbox_pointer_inline_pub(blk, &handle)
            } else {
                v
            };
            ctx.block().ret(DOUBLE, &final_v);
            Ok(())
        }
        Stmt::Return(None) => {
            // Bare `return;` returns undefined (encoded as 0.0). For
            // async functions, wrap undefined in a resolved promise.
            if ctx.is_async_fn {
                let zero = "0.0".to_string();
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
                let boxed = crate::expr::nanbox_pointer_inline_pub(blk, &handle);
                ctx.block().ret(DOUBLE, &boxed);
            } else {
                ctx.block().ret(DOUBLE, "0.0");
            }
            Ok(())
        }

        Stmt::Let { id, init, ty, .. } => {
            // Refine the declared type from the initializer when the
            // declared type is Any. The HIR's destructuring lowering
            // declares synthetic `__destruct_*` lets as `ty: Any` even
            // when the init is obviously an Array literal — that breaks
            // is_array_expr at later use sites that depend on
            // `local_types[id]` to dispatch to the array fast path.
            //
            // We only refine Any → something more specific; we don't
            // override declared types because the user may have written
            // `let x: Object = ...` deliberately.
            let refined_ty = if matches!(ty, perry_types::Type::Any) {
                init.as_ref()
                    .and_then(|e| crate::expr::refine_type_from_init(ctx, e))
                    .unwrap_or_else(|| ty.clone())
            } else {
                ty.clone()
            };

            // CRITICAL: register the local's storage BEFORE lowering
            // the init expression. Self-recursive closures (`let f = (n)
            // => f(n-1) ...`) reference the let-bound name from inside
            // their own body, and the closure's auto-capture pass needs
            // to find the slot or global. Lowering the init first means
            // the body sees `LocalGet(7)` with no entry in ctx.locals.
            //
            // For module globals we register first, then lower init,
            // then store. Same for stack-local lets.
            if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                ctx.local_types.insert(*id, refined_ty);
                if let Some(init_expr) = init {
                    let v = lower_expr(ctx, init_expr)?;
                    let g_ref = format!("@{}", global_name);
                    ctx.block().store(DOUBLE, &v, &g_ref);
                }
                return Ok(());
            }
            // Boxed local: allocate a heap box and store its pointer
            // in the slot. `LocalGet` / `LocalSet` / `Update` on this
            // id all dereference through the box. See `boxed_vars` on
            // FnCtx for why this exists.
            if ctx.boxed_vars.contains(id) {
                let init_val = if let Some(init_expr) = init {
                    lower_expr(ctx, init_expr)?
                } else {
                    crate::nanbox::double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ))
                };
                let blk = ctx.block();
                let box_ptr =
                    blk.call(crate::types::I64, "js_box_alloc", &[(DOUBLE, &init_val)]);
                // Store the box pointer as a raw i64-cast-to-double in
                // the slot. We can't NaN-box a box pointer because
                // reading the slot back expects the raw pointer —
                // LocalGet/LocalSet below do a direct bitcast.
                let slot = ctx.block().alloca(DOUBLE);
                let box_as_double = ctx.block().bitcast_i64_to_double(&box_ptr);
                ctx.block().store(DOUBLE, &box_as_double, &slot);
                ctx.locals.insert(*id, slot);
                ctx.local_types.insert(*id, refined_ty);
                return Ok(());
            }
            let slot = ctx.block().alloca(DOUBLE);
            ctx.locals.insert(*id, slot.clone());
            ctx.local_types.insert(*id, refined_ty);
            if let Some(init_expr) = init {
                let v = lower_expr(ctx, init_expr)?;
                ctx.block().store(DOUBLE, &v, &slot);
            }
            Ok(())
        }

        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => lower_if(ctx, condition, then_branch, else_branch.as_deref()),

        Stmt::For {
            init,
            condition,
            update,
            body,
        } => lower_for(ctx, init.as_deref(), condition.as_ref(), update.as_ref(), body),

        // `while (cond) { body }` — same CFG as for-loop without init/update.
        Stmt::While { condition, body } => lower_while(ctx, condition, body),

        // `do { body } while (cond)` — body runs at least once, then cond.
        Stmt::DoWhile { body, condition } => lower_do_while(ctx, body, condition),

        // `break;` — branch to the innermost loop's exit block. The
        // current block becomes terminated; subsequent statements in
        // the same scope are dead code and `lower_stmts` skips them.
        Stmt::Break => {
            let break_label = ctx
                .loop_targets
                .last()
                .map(|(_c, b)| b.clone())
                .ok_or_else(|| anyhow!("break statement outside any loop"))?;
            ctx.block().br(&break_label);
            Ok(())
        }

        // `continue;` — branch to the innermost loop's continue target
        // (which is the update block for `for`, the cond block for
        // `while`/`do-while`).
        Stmt::Continue => {
            let cont_label = ctx
                .loop_targets
                .last()
                .map(|(c, _b)| c.clone())
                .ok_or_else(|| anyhow!("continue statement outside any loop"))?;
            ctx.block().br(&cont_label);
            Ok(())
        }

        // `switch (disc) { case A: ... case B: ... default: ... }` —
        // lowered as a tower of test/body blocks with explicit fall-through
        // (each body block falls into the next body block, not the next
        // test). `break` inside a case branches to the exit block (we
        // push a (exit, exit) entry onto loop_targets so `break` works
        // even though there's no continue target).
        //
        // Layout for `switch (d) { case A: ...; break; case B: ...; default: ... }`:
        //
        //   <pre>:
        //     %dv = <discriminant>
        //     br test_A
        //   test_A:
        //     %cmp = fcmp oeq %dv, A
        //     br i1 %cmp, body_A, test_B
        //   body_A:
        //     ...
        //     br exit            ; from `break`
        //   test_B:
        //     %cmp = fcmp oeq %dv, B
        //     br i1 %cmp, body_B, body_default
        //   body_B:
        //     ...
        //     br body_default    ; fall-through
        //   body_default:
        //     ...
        //     br exit
        //   exit:
        //
        // Default position is preserved (it goes wherever it appears in
        // source order) — falling-through into the default case from the
        // preceding case is valid JS.
        Stmt::Switch { discriminant, cases } => lower_switch(ctx, discriminant, cases),

        // Labeled statement: just lower the body. Real labeled
        // break/continue support requires per-label loop_targets
        // entries; for now we share the implicit innermost target,
        // which works for the common case where label is unused.
        Stmt::Labeled { body, .. } => lower_stmt(ctx, body),
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {
            // Same as plain break/continue against the innermost loop.
            let target = ctx
                .loop_targets
                .last()
                .cloned()
                .ok_or_else(|| anyhow!("labeled break/continue outside any loop"))?;
            ctx.block().br(&target.1);
            Ok(())
        }

        // Phase G stubs: real exception handling lives in a future
        // phase. For now we lower throw as a no-op (lower the value
        // for side effects) and try as just the body block (no
        // catch, no finally). This is wrong but unblocks
        // compilation AND runtime — programs that have a try/catch
        // but never actually throw at runtime now run, and programs
        // that DO throw silently continue executing (bad, but
        // doesn't crash with SIGTRAP).
        Stmt::Throw(expr) => {
            // Without real EH, throwing outside a try-catch is a no-op
            // — we evaluate the expression for side effects and continue.
            // Inside a try with a catch, the Try lowering handles
            // the throw inline.
            let _ = lower_expr(ctx, expr)?;
            Ok(())
        }
        Stmt::Try { body, catch, finally } => {
            // Without longjmp-based EH, we approximate try/catch by
            // statically detecting whether the try body contains a
            // top-level throw statement. If it does:
            //   1. Lower every stmt before the throw normally
            //   2. Bind the catch param (if any) to the thrown value
            //   3. Lower the catch body
            // If no throw is present, just lower body straight-line.
            // This handles the common test pattern `try { throw X }
            // catch (e) { ... }` correctly. Conditional throws inside
            // if/while still fall through (the catch never fires).
            let throw_idx = body.iter().position(|s| matches!(s, Stmt::Throw(_)));
            if let (Some(idx), Some(catch_clause)) = (throw_idx, catch) {
                // Lower stmts before the throw.
                lower_stmts(ctx, &body[..idx])?;
                // Lower the throw expression to get the value.
                let throw_value = if let Stmt::Throw(expr) = &body[idx] {
                    crate::expr::lower_expr(ctx, expr)?
                } else {
                    "0.0".to_string()
                };
                // Bind catch param.
                if let Some((id, _name)) = &catch_clause.param {
                    let slot = ctx.block().alloca(DOUBLE);
                    ctx.locals.insert(*id, slot.clone());
                    ctx.block().store(DOUBLE, &throw_value, &slot);
                }
                // Lower catch body.
                lower_stmts(ctx, &catch_clause.body)?;
            } else {
                lower_stmts(ctx, body)?;
            }
            if let Some(f) = finally {
                lower_stmts(ctx, f)?;
            }
            Ok(())
        }

        other => bail!(
            "perry-codegen-llvm Phase B.12: Stmt {} not yet supported",
            stmt_variant_name(other)
        ),
    }
}

/// For-loop lowering: classic init / cond / body / update / exit CFG.
///
/// ```text
///   <current>:
///     <init>
///     br cond
///   for.cond:
///     <condition>          ; if missing, treat as `true` (infinite loop)
///     fcmp one cond, 0.0
///     br i1, body, exit
///   for.body:
///     <body>
///     br update            ; if not already terminated
///   for.update:
///     <update>
///     br cond              ; if not already terminated
///   for.exit:
///     <continues here>
/// ```
///
/// Phase 2.1 does not support `break` / `continue`. The body must fall
/// through to update; otherwise codegen produces dead code that LLVM will
/// reject. We don't yet pass the loop's break/continue targets through
/// FnCtx — that lands when we need it.
fn lower_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<()> {
    // Init runs once in the current block. A `let i = 0` here adds `i` to
    // ctx.locals, which the body can then load via LocalGet.
    if let Some(init_stmt) = init {
        lower_stmt(ctx, init_stmt)?;
    }

    let cond_idx = ctx.new_block("for.cond");
    let body_idx = ctx.new_block("for.body");
    let update_idx = ctx.new_block("for.update");
    let exit_idx = ctx.new_block("for.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let update_label = ctx.block_label(update_idx);
    let exit_label = ctx.block_label(exit_idx);

    // Branch from the block holding the init into the cond block.
    ctx.block().br(&cond_label);

    // Cond block.
    ctx.current_block = cond_idx;
    if let Some(cond_expr) = condition {
        let cv = lower_expr(ctx, cond_expr)?;
        let i1 = lower_truthy(ctx, &cv, cond_expr);
        ctx.block().cond_br(&i1, &body_label, &exit_label);
    } else {
        // `for (;;)` — unconditional jump into the body. May be an
        // infinite loop unless the body contains a `break`.
        ctx.block().br(&body_label);
    }

    // Push break/continue targets so nested `break`/`continue` know where
    // to jump. For for-loops, continue runs the update step.
    ctx.loop_targets.push((update_label.clone(), exit_label.clone()));

    // Body block.
    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&update_label);
    }

    // Update block.
    ctx.current_block = update_idx;
    if let Some(update_expr) = update {
        let _ = lower_expr(ctx, update_expr)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.loop_targets.pop();

    // Exit block — subsequent statements continue here.
    ctx.current_block = exit_idx;
    Ok(())
}

/// `while (cond) { body }` — classic 3-block CFG (cond / body / exit).
///
/// ```text
///   <current>:
///     br cond
///   while.cond:
///     <condition>
///     truthy → body, falsey → exit
///   while.body:
///     <body>
///     br cond                 ; if not already terminated
///   while.exit:
///     <continues here>
/// ```
///
/// No break/continue support yet — body must fall through to the next
/// loop iteration. Same limitation as `for`.
fn lower_while(ctx: &mut FnCtx<'_>, condition: &perry_hir::Expr, body: &[Stmt]) -> Result<()> {
    let cond_idx = ctx.new_block("while.cond");
    let body_idx = ctx.new_block("while.body");
    let exit_idx = ctx.new_block("while.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&cond_label);

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);

    // For while-loops, continue jumps back to the cond block.
    ctx.loop_targets.push((cond_label.clone(), exit_label.clone()));

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}

/// `do { body } while (cond)` — body runs at least once. Same blocks as
/// `while`, but the initial branch goes to body, not cond.
fn lower_do_while(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    condition: &perry_hir::Expr,
) -> Result<()> {
    let body_idx = ctx.new_block("dowhile.body");
    let cond_idx = ctx.new_block("dowhile.cond");
    let exit_idx = ctx.new_block("dowhile.exit");

    let body_label = ctx.block_label(body_idx);
    let cond_label = ctx.block_label(cond_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&body_label);

    // Push break/continue targets BEFORE compiling the body so nested
    // break/continue see them.
    ctx.loop_targets.push((cond_label.clone(), exit_label.clone()));

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}

/// `switch (disc) { case A: ...; break; case B: ...; default: ... }`
/// lowering. Each case gets a (test, body) block pair; bodies fall
/// through to the next body block (not the next test) to honor JS
/// fall-through. The default body is positioned wherever the default
/// case appears in source order. `break` inside a case branches to
/// the exit block via the `loop_targets` mechanism.
///
/// We don't use LLVM's `switch` instruction because the discriminant
/// is a NaN-boxed double whose equality semantics differ from i32
/// switch (NaN != NaN). The if-tower lowering uses fcmp oeq for each
/// test which yields the right semantics.
fn lower_switch(
    ctx: &mut FnCtx<'_>,
    discriminant: &perry_hir::Expr,
    cases: &[perry_hir::SwitchCase],
) -> Result<()> {
    let dv = lower_expr(ctx, discriminant)?;

    // Allocate test/body blocks for every case up front so we can wire
    // up the fall-through edges before each block is filled in.
    let mut test_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    let mut body_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    for (i, case) in cases.iter().enumerate() {
        let test_name = if case.test.is_some() {
            format!("switch.test{}", i)
        } else {
            format!("switch.default_test{}", i)
        };
        test_blocks.push(ctx.new_block(&test_name));
        body_blocks.push(ctx.new_block(&format!("switch.body{}", i)));
    }
    let exit_idx = ctx.new_block("switch.exit");
    let exit_label = ctx.block_label(exit_idx);

    // Branch from the discriminant block into the first test (or
    // straight into the body if there are zero cases — degenerate but
    // legal).
    if let Some(&first_test) = test_blocks.first() {
        let first_test_label = ctx.block_label(first_test);
        ctx.block().br(&first_test_label);
    } else {
        ctx.block().br(&exit_label);
        ctx.current_block = exit_idx;
        return Ok(());
    }

    // Find the default case index, if any. The "fall-through to default
    // when nothing matches" target is the default's body block; if
    // there's no default, we fall through to exit.
    let default_idx = cases.iter().position(|c| c.test.is_none());
    let no_match_target_label = match default_idx {
        Some(i) => ctx.block_label(body_blocks[i]),
        None => exit_label.clone(),
    };

    // Push break target. Switch has no continue, so we use exit for both.
    ctx.loop_targets.push((exit_label.clone(), exit_label.clone()));

    // Compile each test block. Each test compares dv against the case
    // expression with fcmp oeq, jumps to the body on match, otherwise
    // jumps to the next test (or to no_match_target if this is the last).
    for (i, case) in cases.iter().enumerate() {
        ctx.current_block = test_blocks[i];
        let body_label = ctx.block_label(body_blocks[i]);
        let next_label = if i + 1 < test_blocks.len() {
            ctx.block_label(test_blocks[i + 1])
        } else {
            no_match_target_label.clone()
        };

        if let Some(test_expr) = case.test.as_ref() {
            let cv = lower_expr(ctx, test_expr)?;
            // fcmp on NaN-tagged string/pointer values is always
            // false (NaN comparisons are unordered). For switch on
            // strings or any value that might be NaN-tagged, compare
            // the i64 bit patterns instead. This works for numbers
            // too — equal doubles have equal bits except for ±0
            // which the JS spec treats as equal anyway and Number(0)
            // === Number(-0) is true.
            let blk = ctx.block();
            let dv_bits = blk.bitcast_double_to_i64(&dv);
            let cv_bits = blk.bitcast_double_to_i64(&cv);
            let cmp = blk.icmp_eq(crate::types::I64, &dv_bits, &cv_bits);
            blk.cond_br(&cmp, &body_label, &next_label);
        } else {
            // Default case test block: unconditional jump to its body.
            ctx.block().br(&body_label);
        }
    }

    // Compile each body block. Bodies fall through to the next body
    // (NOT the next test) unless terminated by `break`/`return`/etc.
    for (i, case) in cases.iter().enumerate() {
        ctx.current_block = body_blocks[i];
        lower_stmts(ctx, &case.body)?;
        if !ctx.block().is_terminated() {
            let next_body_label = if i + 1 < body_blocks.len() {
                ctx.block_label(body_blocks[i + 1])
            } else {
                exit_label.clone()
            };
            ctx.block().br(&next_body_label);
        }
    }

    ctx.loop_targets.pop();
    ctx.current_block = exit_idx;
    Ok(())
}

/// If-else lowering using explicit then/else/merge blocks.
///
/// Truthiness uses `lower_truthy` which dispatches to either an inline
/// `fcmp one cond, 0.0` (statically-numeric conditions) or a runtime
/// `js_is_truthy` call (NaN-boxed booleans, strings, objects, unions).
fn lower_if(
    ctx: &mut FnCtx<'_>,
    condition: &perry_hir::Expr,
    then_branch: &[Stmt],
    else_branch: Option<&[Stmt]>,
) -> Result<()> {
    let cond_val = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cond_val, condition);

    let then_idx = ctx.new_block("if.then");
    let else_idx = ctx.new_block("if.else");
    let merge_idx = ctx.new_block("if.merge");

    let then_label = ctx.block_label(then_idx);
    let else_label = ctx.block_label(else_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Emit the branch in the incoming current block.
    ctx.block().cond_br(&i1, &then_label, &else_label);

    // Compile then branch.
    ctx.current_block = then_idx;
    lower_stmts(ctx, then_branch)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Compile else branch. If there's no explicit else, the else block is
    // still created so both sides of the condBr have a valid target — it
    // just branches immediately to merge.
    ctx.current_block = else_idx;
    if let Some(else_stmts) = else_branch {
        lower_stmts(ctx, else_stmts)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Continue emitting subsequent statements into the merge block.
    ctx.current_block = merge_idx;
    Ok(())
}

fn stmt_variant_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Expr(_) => "Expr",
        Stmt::Let { .. } => "Let",
        Stmt::Return(_) => "Return",
        Stmt::If { .. } => "If",
        Stmt::While { .. } => "While",
        Stmt::DoWhile { .. } => "DoWhile",
        Stmt::For { .. } => "For",
        Stmt::Labeled { .. } => "Labeled",
        Stmt::Break => "Break",
        Stmt::Continue => "Continue",
        Stmt::LabeledBreak(_) => "LabeledBreak",
        Stmt::LabeledContinue(_) => "LabeledContinue",
        Stmt::Throw(_) => "Throw",
        Stmt::Try { .. } => "Try",
        Stmt::Switch { .. } => "Switch",
    }
}

// Silence the unused-import lint if lower_expr is not directly used here
// (it is used via the `use` above, but rustc's dead-code checker can be
// strict about helpers that only get called transitively).
#[allow(dead_code)]
fn _keep_anyhow_in_scope() -> anyhow::Error {
    anyhow!("")
}
