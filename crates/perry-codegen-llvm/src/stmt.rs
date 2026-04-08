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
            ctx.block().ret(DOUBLE, &v);
            Ok(())
        }
        Stmt::Return(None) => {
            // Phase 2 functions all return double. A bare `return;` in a
            // typed numeric function is unusual but we honor it by returning
            // 0.0 rather than erroring.
            ctx.block().ret(DOUBLE, "0.0");
            Ok(())
        }

        Stmt::Let { id, init, ty, .. } => {
            // Allocate a stack slot, then store the initializer if present.
            let slot = ctx.block().alloca(DOUBLE);
            if let Some(init_expr) = init {
                let v = lower_expr(ctx, init_expr)?;
                ctx.block().store(DOUBLE, &v, &slot);
            }
            ctx.locals.insert(*id, slot);
            // Track the static type so type-aware lowering paths
            // (string concat, future array dispatch, etc.) can detect
            // string-typed locals via `LocalGet`.
            ctx.local_types.insert(*id, ty.clone());
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

        other => bail!(
            "perry-codegen-llvm Phase 2: Stmt {} not yet supported",
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
        // `for (;;)` — unconditional jump into the body. Without break
        // support this is an infinite loop, but the verifier accepts it.
        ctx.block().br(&body_label);
    }

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

    // Exit block — subsequent statements continue here.
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
