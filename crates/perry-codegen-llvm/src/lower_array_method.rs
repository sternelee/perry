//! Array method lowering for array-typed receivers.
//!
//! Contains `lower_array_method` which dispatches `.pop()`, `.join()`,
//! `.some()`, `.every()`, `.toString()`, `.concat()`, `.sort()`,
//! `.reverse()`, `.flat()`, `.flatMap()`.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{i32_bool_to_nanbox, lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

/// Lower `arr.method(args…)` for an array-typed receiver. Currently
/// supported: `pop`, `join`. `push` is handled separately by the HIR
/// `Expr::ArrayPush` variant (Phase B.7).
pub(crate) fn lower_array_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;

    match property {
        "pop" => {
            if !args.is_empty() {
                bail!("perry-codegen-llvm: Array.pop takes no args, got {}", args.len());
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Returns f64 directly (the popped element, NaN if empty).
            Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &recv_handle)]))
        }
        "join" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.join expects 1 arg (separator), got {}", args.len());
            }
            let sep_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let sep_handle = unbox_to_i64(blk, &sep_box);
            let result_handle = blk.call(
                I64,
                "js_array_join",
                &[(I64, &recv_handle), (I64, &sep_handle)],
            );
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "some" | "every" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.{} expects 1 arg, got {}", property, args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let runtime_fn = if property == "some" { "js_array_some" } else { "js_array_every" };
            Ok(blk.call(DOUBLE, runtime_fn, &[(I64, &recv_handle), (I64, &cb_handle)]))
        }
        "toString" => {
            // arr.toString() == arr.join(",")
            let key_idx = ctx.strings.intern(",");
            let handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let sep_box = blk.load(DOUBLE, &handle_global);
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let sep_handle = unbox_to_i64(blk, &sep_box);
            let result_handle = blk.call(
                I64,
                "js_array_join",
                &[(I64, &recv_handle), (I64, &sep_handle)],
            );
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "concat" => {
            // arr.concat(other) — call js_array_concat (already declared).
            // For simplicity we only handle single-argument concat.
            if args.len() != 1 {
                return Ok(recv_box);
            }
            let other_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let other_handle = unbox_to_i64(blk, &other_box);
            let result =
                blk.call(I64, "js_array_concat", &[(I64, &recv_handle), (I64, &other_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "sort" => {
            // arr.sort() — default comparator (stringwise compare).
            // arr.sort(cb) — custom comparator path.
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = if args.is_empty() {
                blk.call(I64, "js_array_sort_default", &[(I64, &recv_handle)])
            } else {
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cb_handle = unbox_to_i64(blk, &cb_box);
                blk.call(
                    I64,
                    "js_array_sort_with_comparator",
                    &[(I64, &recv_handle), (I64, &cb_handle)],
                )
            };
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }
        "reverse" => {
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_reverse", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "flat" => {
            // arr.flat() / arr.flat(depth) — depth is ignored for now.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_flat", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "flatMap" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.flatMap expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(
                I64,
                "js_array_flatMap",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // -------- Safety-net handlers for methods that normally arrive --------
        // as HIR variants but may reach here as generic MethodCall when
        // the HIR lowering doesn't recognize the pattern.
        "find" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.find expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            Ok(blk.call(DOUBLE, "js_array_find", &[(I64, &recv_handle), (I64, &cb_handle)]))
        }
        "findIndex" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.findIndex expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let i32_v = blk.call(I32, "js_array_findIndex", &[(I64, &recv_handle), (I64, &cb_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "findLast" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.findLast expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            Ok(blk.call(DOUBLE, "js_array_find_last", &[(I64, &recv_handle), (I64, &cb_handle)]))
        }
        "findLastIndex" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.findLastIndex expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let i32_v = blk.call(I32, "js_array_find_last_index", &[(I64, &recv_handle), (I64, &cb_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "reduce" => {
            // arr.reduce(callback) or arr.reduce(callback, initial)
            if args.is_empty() || args.len() > 2 {
                bail!("perry-codegen-llvm: Array.reduce expects 1-2 args, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let (has_initial, initial_box) = if args.len() == 2 {
                let init = lower_expr(ctx, &args[1])?;
                (1i32, init)
            } else {
                (0i32, "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let has_init_str = format!("{}", has_initial);
            Ok(blk.call(
                DOUBLE,
                "js_array_reduce",
                &[(I64, &recv_handle), (I64, &cb_handle), (I32, &has_init_str), (DOUBLE, &initial_box)],
            ))
        }
        "reduceRight" => {
            if args.is_empty() || args.len() > 2 {
                bail!("perry-codegen-llvm: Array.reduceRight expects 1-2 args, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let (has_initial, initial_box) = if args.len() == 2 {
                let init = lower_expr(ctx, &args[1])?;
                (1i32, init)
            } else {
                (0i32, "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let has_init_str = format!("{}", has_initial);
            Ok(blk.call(
                DOUBLE,
                "js_array_reduce_right",
                &[(I64, &recv_handle), (I64, &cb_handle), (I32, &has_init_str), (DOUBLE, &initial_box)],
            ))
        }
        "map" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.map expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(I64, "js_array_map", &[(I64, &recv_handle), (I64, &cb_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "filter" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.filter expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(I64, "js_array_filter", &[(I64, &recv_handle), (I64, &cb_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "forEach" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.forEach expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void("js_array_forEach", &[(I64, &recv_handle), (I64, &cb_handle)]);
            // forEach returns undefined
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "includes" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.includes expects 1 arg, got {}", args.len());
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_array_includes_f64", &[(I64, &recv_handle), (DOUBLE, &val_box)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }
        "indexOf" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.indexOf expects 1 arg, got {}", args.len());
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i32_v = blk.call(I32, "js_array_indexOf_f64", &[(I64, &recv_handle), (DOUBLE, &val_box)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "at" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.at expects 1 arg, got {}", args.len());
            }
            let idx_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            Ok(blk.call(DOUBLE, "js_array_at", &[(I64, &recv_handle), (DOUBLE, &idx_box)]))
        }
        "slice" => {
            // arr.slice(start) or arr.slice(start, end)
            if args.is_empty() || args.len() > 2 {
                bail!("perry-codegen-llvm: Array.slice expects 1-2 args, got {}", args.len());
            }
            let start_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let start_i32 = blk.fptosi(DOUBLE, &start_box, I32);
            let end_i32 = if args.len() == 2 {
                let end_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                blk.fptosi(DOUBLE, &end_box, I32)
            } else {
                // Use i32::MAX as sentinel for "to end"
                "2147483647".to_string()
            };
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_array_slice",
                &[(I64, &recv_handle), (I32, &start_i32), (I32, &end_i32)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "shift" => {
            if !args.is_empty() {
                bail!("perry-codegen-llvm: Array.shift takes no args, got {}", args.len());
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            Ok(blk.call(DOUBLE, "js_array_shift_f64", &[(I64, &recv_handle)]))
        }
        "entries" => {
            for a in args { let _ = lower_expr(ctx, a)?; }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_entries", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "keys" => {
            for a in args { let _ = lower_expr(ctx, a)?; }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_keys", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "values" => {
            for a in args { let _ = lower_expr(ctx, a)?; }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_values", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "fill" => {
            // arr.fill(value) — fills all elements with value, returns same array.
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.fill expects 1 arg, got {}", args.len());
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_fill", &[(I64, &recv_handle), (DOUBLE, &val_box)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "unshift" => {
            if args.len() != 1 {
                bail!("perry-codegen-llvm: Array.unshift expects 1 arg, got {}", args.len());
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_unshift_f64", &[(I64, &recv_handle), (DOUBLE, &val_box)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // Best-effort fallback: lower args for side effects, return
        // the receiver.
        _ => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(recv_box)
        }
    }
}
