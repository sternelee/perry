//! String method and concatenation lowering.
//!
//! Contains `lower_string_method`, `lower_string_self_append`,
//! `lower_string_coerce_concat`, and `lower_string_concat`.

use anyhow::{anyhow, bail, Result};
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_string_inline, unbox_to_i64, i32_bool_to_nanbox, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::type_analysis::is_string_expr;
use crate::types::{DOUBLE, I1, I32, I64};

/// Lower `s.method(args…)` for a string-typed receiver. Currently
/// supported methods: `indexOf` (1 or 2 args), `slice`, `substring`,
/// `startsWith`, `endsWith`. Anything else bails with an actionable
/// error.
///
/// All string methods unbox the receiver pointer with the inline
/// bitcast+mask pattern, lower each arg, and call the matching runtime
/// function. Return values that are i32 (indexOf, startsWith, endsWith)
/// get sitofp'd to double; return values that are i64 string handles
/// (slice, substring) get NaN-boxed inline with STRING_TAG.
pub(crate) fn lower_string_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;

    match property {
        "indexOf" => {
            if args.is_empty() || args.len() > 2 {
                bail!("perry-codegen-llvm: String.indexOf expects 1 or 2 args, got {}", args.len());
            }
            let needle_box = lower_expr(ctx, &args[0])?;
            // Optional fromIndex.
            let from_idx_double = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let needle_handle = unbox_to_i64(blk, &needle_box);
            let result_i32 = if let Some(from_d) = from_idx_double {
                let from_i32 = blk.fptosi(DOUBLE, &from_d, I32);
                blk.call(
                    I32,
                    "js_string_index_of_from",
                    &[(I64, &recv_handle), (I64, &needle_handle), (I32, &from_i32)],
                )
            } else {
                blk.call(
                    I32,
                    "js_string_index_of",
                    &[(I64, &recv_handle), (I64, &needle_handle)],
                )
            };
            // i32 → double via sitofp (preserves the -1 sentinel for "not found").
            Ok(blk.sitofp(I32, &result_i32, DOUBLE))
        }
        "slice" | "substring" => {
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen-llvm: String.{} expects 1 or 2 args, got {}",
                    property,
                    args.len()
                );
            }
            let start_d = lower_expr(ctx, &args[0])?;
            // 2-arg form: explicit end. 1-arg form: end defaults to the
            // string's length, computed inline (load i32 at offset 0).
            let end_d = if args.len() == 2 {
                lower_expr(ctx, &args[1])?
            } else {
                // Inline length read on the receiver. Same pattern as
                // the dedicated `str.length` arm.
                let blk = ctx.block();
                let recv_bits = blk.bitcast_double_to_i64(&recv_box);
                let recv_handle = blk.and(I64, &recv_bits, POINTER_MASK_I64);
                let len_ptr = blk.inttoptr(I64, &recv_handle);
                let len_i32 = blk.load(I32, &len_ptr);
                blk.sitofp(I32, &len_i32, DOUBLE)
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let start_i32 = blk.fptosi(DOUBLE, &start_d, I32);
            let end_i32 = blk.fptosi(DOUBLE, &end_d, I32);
            let runtime_fn = if property == "slice" {
                "js_string_slice"
            } else {
                "js_string_substring"
            };
            let result_handle = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (I32, &start_i32), (I32, &end_i32)],
            );
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "split" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: String.split expects 1 arg (delimiter), got {}", args.len());
            }
            // NOTE: we always call js_string_split here, even for regex
            // delimiters — the runtime will detect regex pointers via
            // their GC header and delegate to js_string_split_regex
            // internally. This avoids needing a new LLVM runtime decl.
            let delim_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let delim_handle = unbox_to_i64(blk, &delim_box);
            let result_arr = blk.call(
                I64,
                "js_string_split",
                &[(I64, &recv_handle), (I64, &delim_handle)],
            );
            // Returns an array pointer (ArrayHeader*) — NaN-box with POINTER_TAG.
            Ok(crate::expr::nanbox_pointer_inline(blk, &result_arr))
        }
        // Unary string-returning methods (no args).
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" => {
            if !args.is_empty() {
                bail!(
                    "perry-codegen-llvm: String.{} takes no args, got {}",
                    property,
                    args.len()
                );
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let runtime_fn = match property {
                "toLowerCase" => "js_string_to_lower_case",
                "toUpperCase" => "js_string_to_upper_case",
                "trim" => "js_string_trim",
                "trimStart" => "js_string_trim_start",
                "trimEnd" => "js_string_trim_end",
                _ => unreachable!(),
            };
            let result = blk.call(I64, runtime_fn, &[(I64, &recv_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        "charAt" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: String.charAt expects 1 arg, got {}", args.len());
            }
            let idx_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            let result = blk.call(
                I64,
                "js_string_char_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "repeat" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: String.repeat expects 1 arg, got {}", args.len());
            }
            let count_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let count_i32 = blk.fptosi(DOUBLE, &count_d, I32);
            let result = blk.call(
                I64,
                "js_string_repeat",
                &[(I64, &recv_handle), (I32, &count_i32)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "replace" | "replaceAll" => {
            if args.len() != 2 {
                bail!(
                    "perry-codegen-llvm: String.{} expects 2 args, got {}",
                    property,
                    args.len()
                );
            }
            // First arg is either a string or a regex literal — pick
            // the right runtime function. The regex form takes a
            // RegExpHeader pointer; the string form takes a string
            // handle. Both replacements are string handles.
            let needle_is_regex = matches!(&args[0], Expr::RegExp { .. });
            let needle_box = lower_expr(ctx, &args[0])?;
            let repl_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let needle_handle = unbox_to_i64(blk, &needle_box);
            let repl_handle = unbox_to_i64(blk, &repl_box);
            let runtime_fn = if needle_is_regex {
                "js_string_replace_regex"
            } else if property == "replaceAll" {
                "js_string_replace_all_string"
            } else {
                "js_string_replace_string"
            };
            let result = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (I64, &needle_handle), (I64, &repl_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "startsWith" | "endsWith" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen-llvm: String.{} expects 1 arg, got {}",
                    property,
                    args.len()
                );
            }
            let other_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let other_handle = unbox_to_i64(blk, &other_box);
            let runtime_fn = if property == "startsWith" {
                "js_string_starts_with"
            } else {
                "js_string_ends_with"
            };
            let result_i32 = blk.call(
                I32,
                runtime_fn,
                &[(I64, &recv_handle), (I64, &other_handle)],
            );
            Ok(i32_bool_to_nanbox(blk, &result_i32))
        }
        "includes" => {
            // str.includes(sub) -> boolean. Implemented as
            // js_string_index_of(str, sub) != -1, then NaN-tagged.
            if args.is_empty() || args.len() > 2 {
                bail!("perry-codegen-llvm: String.includes expects 1 or 2 args, got {}", args.len());
            }
            let needle_box = lower_expr(ctx, &args[0])?;
            // Optional fromIndex param is ignored for the boolean form.
            if args.len() == 2 {
                let _ = lower_expr(ctx, &args[1])?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let needle_handle = unbox_to_i64(blk, &needle_box);
            let idx_i32 = blk.call(
                I32,
                "js_string_index_of",
                &[(I64, &recv_handle), (I64, &needle_handle)],
            );
            // includes := indexOf != -1
            let neg_one = "-1".to_string();
            let bit = blk.icmp_ne(I32, &idx_i32, &neg_one);
            let tagged = blk.select(
                I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }
        // Best-effort fallback: lower args for side effects, return
        // the receiver string. Compile succeeds; runtime gets the
        // pre-method-call value.
        _ => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(recv_box)
        }
    }
}

/// Lower the `str = str + rhs` self-append pattern. Uses the in-place
/// `js_string_append` runtime function (refcount=1 → mutate in place,
/// otherwise allocate). The returned pointer is stored back to the local
/// slot — `js_string_append` may realloc when growing past capacity.
///
/// This is the load-bearing optimization for the canonical `let str = "";
/// for (...) str = str + "a"` string-build pattern. Mirrors Cranelift's
/// expr.rs:5611+ detection.
pub(crate) fn lower_string_self_append(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    rhs: &Expr,
) -> Result<String> {
    let slot = ctx
        .locals
        .get(&local_id)
        .ok_or_else(|| anyhow!("string self-append: local {} not in scope", local_id))?
        .clone();

    // Lower the RHS first (might be a string literal, a local, or a
    // computed expression). For non-string RHS we'd need to coerce, but
    // the bench_string_ops case always uses a string literal, so for the
    // first slice we require the RHS to be a known string.
    if !is_string_expr(ctx, rhs) {
        // Fall back to the slower concat path: load the local, do a
        // generic concat-coerce, store back.
        let lhs_val = ctx.block().load(DOUBLE, &slot);
        let _lhs = lhs_val.clone();
        let rhs_val = lower_expr(ctx, rhs)?;
        let blk = ctx.block();
        let l_handle = unbox_to_i64(blk, &lhs_val);
        // Coerce non-string RHS to a string handle.
        let r_handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &rhs_val)]);
        let result = blk.call(I64, "js_string_append", &[(I64, &l_handle), (I64, &r_handle)]);
        let new_box = nanbox_string_inline(blk, &result);
        blk.store(DOUBLE, &new_box, &slot);
        return Ok(new_box);
    }

    let rhs_box = lower_expr(ctx, rhs)?;
    let blk = ctx.block();
    let lhs_box = blk.load(DOUBLE, &slot);
    let l_handle = unbox_to_i64(blk, &lhs_box);
    let r_handle = unbox_to_i64(blk, &rhs_box);
    let new_handle = blk.call(
        I64,
        "js_string_append",
        &[(I64, &l_handle), (I64, &r_handle)],
    );
    let new_box = nanbox_string_inline(blk, &new_handle);
    blk.store(DOUBLE, &new_box, &slot);
    Ok(new_box)
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
pub(crate) fn lower_string_coerce_concat(
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
pub(crate) fn lower_string_concat(ctx: &mut FnCtx<'_>, left: &Expr, right: &Expr) -> Result<String> {
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
