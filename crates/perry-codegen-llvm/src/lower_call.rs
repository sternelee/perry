//! Call, new, and native method call lowering.
//!
//! Contains `lower_call`, `lower_new`, and `lower_native_method_call`.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, variant_name, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::lower_string_method;
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::type_analysis::{is_array_expr, is_map_expr, is_promise_expr, is_set_expr, is_string_expr, receiver_class_name};
use crate::types::{DOUBLE, I32, I64};

/// Lower a `Call` expression. Two shapes are supported:
/// 1. `FuncRef(id)(args...)` — direct call to a user function by HIR id.
/// 2. `console.log(expr)` where `expr` lowers to a double — emits a
///    `js_console_log_number` call and returns `0.0` as the statement value.
pub(crate) fn lower_call(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    // Closure-typed local call: `counter()` where `counter` is a
    // local of `Type::Function(...)`. Dispatch through the runtime
    // `js_closure_call<N>` family — the runtime extracts the function
    // pointer from the closure header and invokes it with the closure
    // as the first arg followed by the user args.
    if let Expr::LocalGet(id) = callee {
        if matches!(ctx.local_types.get(id), Some(HirType::Function(_))) {
            let recv_box = lower_expr(ctx, callee)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Check if this closure has rest params — if so, bundle
            // trailing args into an array (same pattern as FuncRef).
            let rest_idx = ctx
                .local_closure_func_ids
                .get(id)
                .and_then(|cfid| ctx.closure_rest_params.get(cfid))
                .copied();

            let effective_args: Vec<String> = if let Some(ri) = rest_idx {
                let fixed_count = ri;
                let mut result: Vec<String> =
                    lowered_args[..fixed_count.min(lowered_args.len())].to_vec();
                // Materialize the rest array from trailing args.
                let rest_slice = if fixed_count < lowered_args.len() {
                    &lowered_args[fixed_count..]
                } else {
                    &[]
                };
                let rest_count = rest_slice.len() as u32;
                let cap = rest_count.to_string();
                let mut arr = ctx
                    .block()
                    .call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in rest_slice {
                    let blk = ctx.block();
                    arr = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr), (DOUBLE, v)],
                    );
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &arr);
                result.push(rest_box);
                result
            } else {
                lowered_args
            };

            if effective_args.len() > 5 {
                bail!(
                    "perry-codegen-llvm Phase D.1: closure call with {} args (max 5)",
                    effective_args.len()
                );
            }
            let blk = ctx.block();
            let closure_handle = unbox_to_i64(blk, &recv_box);
            let runtime_fn = format!("js_closure_call{}", effective_args.len());
            let mut call_args: Vec<(crate::types::LlvmType, &str)> =
                vec![(I64, &closure_handle)];
            for v in &effective_args {
                call_args.push((DOUBLE, v.as_str()));
            }
            return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
        }
    }

    // User function call via FuncRef.
    if let Expr::FuncRef(fid) = callee {
        let Some(fname) = ctx.func_names.get(fid).cloned() else {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(double_literal(0.0));
        };

        // Rest parameter handling: if the called function has a
        // rest parameter, bundle all trailing args (those at and
        // beyond the rest position) into an array literal and
        // pass that as a single argument.
        let sig = ctx.func_signatures.get(fid).copied();
        let (declared_count, has_rest) = sig.unwrap_or((args.len(), false));
        let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
        if has_rest {
            // Rest is always the LAST declared param. Pass the
            // first (declared_count - 1) args as-is, then bundle
            // the rest into an array.
            let fixed_count = declared_count.saturating_sub(1);
            for a in args.iter().take(fixed_count) {
                lowered.push(lower_expr(ctx, a)?);
            }
            // Materialize the rest array.
            let rest_count = args.len().saturating_sub(fixed_count);
            let cap = (rest_count as u32).to_string();
            let mut current = ctx
                .block()
                .call(I64, "js_array_alloc", &[(I32, &cap)]);
            for a in args.iter().skip(fixed_count) {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                current = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current), (DOUBLE, &v)],
                );
            }
            let rest_box = nanbox_pointer_inline(ctx.block(), &current);
            lowered.push(rest_box);
        } else {
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();

        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // Cross-module function call via ExternFuncRef. The HIR carries the
    // function name; we look up the source module's prefix in
    // `import_function_prefixes` (built by the CLI from hir.imports) and
    // generate `perry_fn_<source_prefix>__<name>`. The function is
    // declared in the OTHER module's compilation; here we just emit a
    // direct LLVM call to its scoped name and the system linker
    // resolves the symbol when the .o files are linked together.
    if let Expr::ExternFuncRef { name, .. } = callee {
        // Soft fallback: built-in extern functions (setTimeout,
        // js_weakmap_set, etc.) often aren't in the import map
        // because the CLI only registers user-imported modules.
        // Lower args for side effects and return undefined.
        let Some(source_prefix) = ctx.import_function_prefixes.get(name).cloned() else {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(double_literal(0.0));
        };
        let fname = format!("perry_fn_{}__{}", source_prefix, name);
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    if let Expr::PropertyGet { object, property } = callee {
        // Number.prototype.toFixed(decimals) — call js_number_to_fixed.
        // Receiver is any number-typed value; we don't gate on
        // is_numeric_expr because tests often call it on Any locals.
        if property == "toFixed"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let dec = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle =
                blk.call(I64, "js_number_to_fixed", &[(DOUBLE, &v), (DOUBLE, &dec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toPrecision(digits)
        if property == "toPrecision"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let prec = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle =
                blk.call(I64, "js_number_to_precision", &[(DOUBLE, &v), (DOUBLE, &prec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toExponential(decimals)
        if property == "toExponential"
            && args.len() <= 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let dec = if args.is_empty() {
                "0.0".to_string()
            } else {
                lower_expr(ctx, &args[0])?
            };
            let blk = ctx.block();
            let handle =
                blk.call(I64, "js_number_to_exponential", &[(DOUBLE, &v), (DOUBLE, &dec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toString(radix) — special case where the
        // single arg is the radix (2..36). Routes through
        // js_jsvalue_to_string_radix so `(255).toString(16)` returns
        // "ff" instead of "255".
        if property == "toString"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            // Only treat as radix call if class doesn't have toString.
            let has_user_toString = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx.methods.contains_key(&(c.clone(), "toString".to_string())) {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_toString {
                let v = lower_expr(ctx, object)?;
                let radix_d = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let radix_i32 = blk.fptosi(DOUBLE, &radix_d, I32);
                let handle = blk.call(
                    I64,
                    "js_jsvalue_to_string_radix",
                    &[(DOUBLE, &v), (I32, &radix_i32)],
                );
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        // Universal `.toString()` — works for any JS value via the
        // runtime's js_jsvalue_to_string dispatch (numbers print as
        // their decimal form, strings as themselves, objects as
        // [object Object], etc.). Only intercepts if NO class
        // method dispatch can win (i.e. the receiver isn't a known
        // class with its own toString) — otherwise the user's
        // override wouldn't run.
        if property == "toString"
            && args.len() <= 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            // Check whether the receiver class (if any) defines
            // toString itself or via inheritance.
            let has_user_toString = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx.methods.contains_key(&(c.clone(), "toString".to_string())) {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_toString {
                let v = lower_expr(ctx, object)?;
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                let blk = ctx.block();
                let handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &v)]);
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        if is_string_expr(ctx, object) {
            return lower_string_method(ctx, object, property, args);
        }
        if is_array_expr(ctx, object) {
            return lower_array_method(ctx, object, property, args);
        }

        // -------- Promise.then / .catch / .finally --------
        // Promise pointers are NaN-boxed with POINTER_TAG. We unbox
        // to get the raw i64 promise handle, then call the runtime
        // `js_promise_then(promise, on_fulfilled, on_rejected)` which
        // returns a new promise handle that we re-box with POINTER_TAG.
        //
        // `.catch(cb)` is sugar for `.then(undefined, cb)`.
        if matches!(property.as_str(), "then" | "catch" | "finally")
            && is_promise_expr(ctx, object)
        {
            match property.as_str() {
                "then" => {
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_fulfilled_box = lower_expr(ctx, &args[0])?;
                        let on_rejected_box = if args.len() >= 2 {
                            lower_expr(ctx, &args[1])?
                        } else {
                            "0".to_string() // null → no rejection handler
                        };
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_fulfilled_handle = unbox_to_i64(blk, &on_fulfilled_box);
                        let on_rejected_i64 = if args.len() >= 2 {
                            unbox_to_i64(blk, &on_rejected_box)
                        } else {
                            "0".to_string() // null i64
                        };
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &on_fulfilled_handle),
                                (I64, &on_rejected_i64),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                "catch" => {
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_rejected_box = lower_expr(ctx, &args[0])?;
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_rejected_handle = unbox_to_i64(blk, &on_rejected_box);
                        let null_i64 = "0".to_string();
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &null_i64),
                                (I64, &on_rejected_handle),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                "finally" => {
                    // .finally(cb) — the callback takes no args and its
                    // return value is ignored. We pass it as on_fulfilled
                    // and on_rejected both set to the same closure; the
                    // runtime handles the "ignore return" semantics.
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_finally_box = lower_expr(ctx, &args[0])?;
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_finally_handle = unbox_to_i64(blk, &on_finally_box);
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &on_finally_handle),
                                (I64, &on_finally_handle),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                _ => {}
            }
        }

        // -------- Map.forEach / Set.forEach --------
        // The HIR emits these as generic Call { callee: PropertyGet }
        // because it skips ArrayForEach when the receiver is Map/Set.
        // Route to the runtime forEach implementations which iterate
        // entries and call the callback via js_closure_call2.
        if property == "forEach" && args.len() >= 1 {
            if is_map_expr(ctx, object) {
                let m_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void("js_map_foreach", &[(I64, &m_handle), (DOUBLE, &cb_box)]);
                return Ok(double_literal(0.0));
            }
            if is_set_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_foreach", &[(I64, &s_handle), (DOUBLE, &cb_box)]);
                return Ok(double_literal(0.0));
            }
        }

        // ── AbortController / AbortSignal dispatch ──
        // `new AbortController()` returns a NaN-boxed pointer
        // (refined to `Named("AbortController")`). The runtime's
        // ObjectHeader carries `signal` / `aborted` fields that the
        // generic property-get path reads. Method calls need explicit
        // interception because the class isn't in `ctx.classes`.
        if let Some(val) = lower_abort_controller_call(ctx, object, property, args)? {
            return Ok(val);
        }

        // ── Chained Web Fetch dispatch ──
        // `r.headers.get(k)` — the inner `r.headers` lowered to a
        // NativeMethodCall that returns an f64 Headers handle; route
        // the outer `.get(...)` (and friends) through the Headers FFI.
        // `r.clone().status` / `.text()` / etc — the inner clone call
        // returns an f64 Response handle; route the outer call through
        // the fetch dispatch.
        //
        // `new Response(...).text()` — likewise, when the receiver is
        // a direct `Expr::New { class_name: "Response"|"Headers"|"Request" }`
        // (no intermediate let binding).
        if let Expr::NativeMethodCall {
            module: chain_mod,
            method: chain_method,
            ..
        } = object.as_ref()
        {
            // Chain `<Response>.headers.<method>(...)` where chain_method == "headers".
            if chain_mod == "fetch" && chain_method == "headers" {
                if let Some(val) =
                    lower_fetch_native_method(ctx, "Headers", property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
            // Chain `<Response>.clone().<method>(...)` — dispatch as a
            // fetch method on the cloned handle.
            if chain_mod == "fetch" && chain_method == "clone" {
                if let Some(val) =
                    lower_fetch_native_method(ctx, "fetch", property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
        }
        // Chain `new Response(...).text()` / `.json()` etc.
        if let Expr::New { class_name: nc, .. } = object.as_ref() {
            let fetch_dispatch = matches!(
                nc.as_str(),
                "Response" | "Headers" | "Request"
            );
            if fetch_dispatch {
                let module = match nc.as_str() {
                    "Response" => "fetch",
                    "Headers" => "Headers",
                    "Request" => "Request",
                    _ => unreachable!(),
                };
                if let Some(val) =
                    lower_fetch_native_method(ctx, module, property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
        }

        // Class instance method call. The receiver's static type is
        // `Type::Named(<class>)` for typed instances.
        //
        // Resolution strategy:
        //   1. Walk the receiver's class + parent chain to find a
        //      method named `property`. The first match (most-derived
        //      that defines the method) is the static fallback.
        //   2. Find every subclass of the receiver's class that ALSO
        //      defines the same method — those are the virtual
        //      override candidates.
        //   3. If there are no overrides, emit a direct call to the
        //      static fallback (fast path, no runtime cost).
        //   4. If there ARE overrides, emit a switch on the object's
        //      runtime class_id: each override gets its own case
        //      calling its concrete method, default falls through to
        //      the static fallback.
        // Interface / dynamic dispatch fallback: when the static
        // class is unknown OR resolves to an interface name not in
        // the class registry, BUT the property name corresponds to
        // a method defined on at least one class in the registry,
        // emit a switch on class_id over all classes that have that
        // method.
        // Skip dynamic dispatch when the receiver is GlobalGet (e.g.
        // `console.log`). GlobalGet is a module-level global object
        // (console, Math, JSON, etc.), not a class instance. Without
        // this guard, `console.log()` gets hijacked by the interface
        // dispatch tower when a user class happens to have a method
        // with the same name (like `SimpleLogger.log()`).
        let is_global = matches!(object.as_ref(), Expr::GlobalGet(_));
        let needs_dynamic_dispatch = !is_global && match receiver_class_name(ctx, object) {
            None => true,
            Some(name) => !ctx.classes.contains_key(&name),
        };
        if needs_dynamic_dispatch {
            // Find all (class, method_name → fn_name) where the
            // method is defined directly on a class.
            let mut implementors: Vec<(u32, String)> = Vec::new();
            for ((cls, mname), fname) in ctx.methods.iter() {
                if mname != property {
                    continue;
                }
                if let Some(cid) = ctx.class_ids.get(cls).copied() {
                    implementors.push((cid, fname.clone()));
                }
            }
            if !implementors.is_empty() {
                let recv_box = lower_expr(ctx, object)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                lowered_args.push(recv_box.clone());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

                // Tower of icmp+br: each implementor's case calls
                // its concrete method, default returns 0.0 (the
                // closure-call fallback would also handle this but
                // returning a sentinel is cheaper).
                let mut case_idxs: Vec<usize> = Vec::with_capacity(implementors.len());
                for (i, _) in implementors.iter().enumerate() {
                    case_idxs.push(ctx.new_block(&format!("idispatch.case{}", i)));
                }
                let default_idx = ctx.new_block("idispatch.default");
                let merge_idx = ctx.new_block("idispatch.merge");
                let merge_label = ctx.block_label(merge_idx);

                for (i, (case_cid, _)) in implementors.iter().enumerate() {
                    let case_label = ctx.block_label(case_idxs[i]);
                    let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                    if i + 1 < implementors.len() {
                        let next_idx = ctx.new_block(&format!("idispatch.test{}", i + 1));
                        let next_lbl = ctx.block_label(next_idx);
                        ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                        ctx.current_block = next_idx;
                    } else {
                        let default_label = ctx.block_label(default_idx);
                        ctx.block().cond_br(&cmp, &case_label, &default_label);
                    }
                }

                let mut phi_inputs: Vec<(String, String)> = Vec::new();
                for ((_, fname), &case_idx) in implementors.iter().zip(case_idxs.iter()) {
                    ctx.current_block = case_idx;
                    let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                    let after_label = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }
                    phi_inputs.push((v, after_label));
                }
                ctx.current_block = default_idx;
                let v_def = double_literal(0.0);
                let def_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                phi_inputs.push((v_def, def_label));

                ctx.current_block = merge_idx;
                let phi_args: Vec<(&str, &str)> =
                    phi_inputs.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                return Ok(ctx.block().phi(DOUBLE, &phi_args));
            }
        }

        if let Some(class_name) = receiver_class_name(ctx, object) {
            // Step 1: walk parent chain for the static method name.
            let mut static_fn: Option<String> = None;
            let mut current_class = Some(class_name.clone());
            while let Some(cur) = current_class {
                let key = (cur.clone(), property.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    static_fn = Some(fname);
                    break;
                }
                current_class = ctx
                    .classes
                    .get(&cur)
                    .and_then(|c| c.extends_name.clone());
            }

            if let Some(fallback_fn) = static_fn {
                // Step 2: collect overriding subclasses. For each
                // subclass C transitively extending class_name, look
                // up which method C uses for `property` (walking C's
                // parent chain). If that resolves to a different
                // function than the static fallback, C needs an
                // explicit case in the dispatch table.
                let mut overrides: Vec<(u32, String)> = Vec::new();
                for (sub_name, &sub_id) in ctx.class_ids.iter() {
                    if *sub_name == class_name {
                        continue;
                    }
                    // Is sub_name transitively a subclass of class_name?
                    let mut parent =
                        ctx.classes.get(sub_name).and_then(|c| c.extends_name.clone());
                    let mut is_subclass = false;
                    while let Some(p) = parent {
                        if p == class_name {
                            is_subclass = true;
                            break;
                        }
                        parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
                    }
                    if !is_subclass {
                        continue;
                    }
                    // Resolve the method for sub_name by walking its
                    // own parent chain (NOT class_name's chain).
                    let mut cur = Some(sub_name.clone());
                    let mut sub_fn: Option<String> = None;
                    while let Some(c) = cur {
                        let key = (c.clone(), property.clone());
                        if let Some(fname) = ctx.methods.get(&key).cloned() {
                            sub_fn = Some(fname);
                            break;
                        }
                        cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                    }
                    if let Some(sub_fn) = sub_fn {
                        if sub_fn != fallback_fn {
                            overrides.push((sub_id, sub_fn));
                        }
                    }
                }

                let recv_box = lower_expr(ctx, object)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                lowered_args.push(recv_box.clone());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

                if overrides.is_empty() {
                    // Fast path: no virtual dispatch needed.
                    return Ok(ctx.block().call(DOUBLE, &fallback_fn, &arg_slices));
                }

                // Step 4: virtual dispatch via class_id switch.
                // Read class_id from the object header, then branch
                // to the right concrete method block.
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

                // Pre-create blocks: one per override + default + merge.
                let mut case_idxs: Vec<usize> = Vec::with_capacity(overrides.len());
                for (i, _) in overrides.iter().enumerate() {
                    case_idxs.push(ctx.new_block(&format!("vdispatch.case{}", i)));
                }
                let default_idx = ctx.new_block("vdispatch.default");
                let merge_idx = ctx.new_block("vdispatch.merge");

                // Default → fallback. We use a tower of icmp+br rather
                // than the LLVM `switch` instruction (which the IR
                // builder doesn't expose generically) — same shape,
                // slightly more verbose.
                let mut current_label = ctx.block().label.clone();
                for (i, (case_cid, _)) in overrides.iter().enumerate() {
                    let next_label = if i + 1 < overrides.len() {
                        // We'll start the next test in this same block
                        // — actually use a fresh block for the test.
                        format!("vdispatch.test{}", i + 1)
                    } else {
                        ctx.block_label(default_idx)
                    };
                    let case_label = ctx.block_label(case_idxs[i]);
                    // Make sure ctx.current_block points at the
                    // current test block.
                    let _ = current_label;
                    let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                    if i + 1 < overrides.len() {
                        // Create the next test block as a fresh block
                        // and branch into it on the false arm.
                        let next_idx = ctx.new_block(&format!("vdispatch.test{}", i + 1));
                        let next_lbl = ctx.block_label(next_idx);
                        ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                        ctx.current_block = next_idx;
                        current_label = next_lbl;
                    } else {
                        ctx.block().cond_br(&cmp, &case_label, &next_label);
                    }
                }

                // Each case block: call the override and branch to merge.
                let merge_label = ctx.block_label(merge_idx);
                let mut phi_inputs: Vec<(String, String)> = Vec::new();
                for ((_, fname), &case_idx) in overrides.iter().zip(case_idxs.iter()) {
                    ctx.current_block = case_idx;
                    let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                    let after_label = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }
                    phi_inputs.push((v, after_label));
                }

                // Default block: call the static fallback.
                ctx.current_block = default_idx;
                let v_def = ctx.block().call(DOUBLE, &fallback_fn, &arg_slices);
                let def_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v_def, def_label));

                // Merge: phi over all incoming case results.
                ctx.current_block = merge_idx;
                let phi_args: Vec<(&str, &str)> =
                    phi_inputs.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                return Ok(ctx.block().phi(DOUBLE, &phi_args));
            }
        }
    }

    // console.log(<args...>) sink.
    //
    // JS spec: console.log can take any number of args, separated by
    // single spaces. We approximate by emitting a separate dispatch
    // call per arg with a literal " " in between, then a final "\n".
    // The runtime functions take a NaN-boxed double and print it
    // followed by a single trailing space (for the inter-arg form)
    // or newline (for the final/single-arg form). For now we use the
    // existing js_console_log_dynamic for every arg — the runtime
    // already adds a newline, so multi-arg console.log will be
    // separated by newlines instead of spaces. Spec-compliant
    // separator handling lives in a future Phase I tweak.
    if let Expr::PropertyGet { object, property } = callee {
        if matches!(object.as_ref(), Expr::GlobalGet(_))
            && matches!(
                property.as_str(),
                "log" | "info" | "warn" | "error" | "debug"
                    | "dir" | "table" | "trace"
                    | "group" | "groupEnd" | "groupCollapsed"
                    | "time" | "timeEnd" | "timeLog"
                    | "count" | "countReset" | "clear" | "assert"
            )
        {
            // Catch-all for the entire console.* surface. Most of
            // them are best-effort: we route the args through
            // js_console_log_dynamic so the user at least sees the
            // values, then return undefined-as-double. Spec-compliant
            // dispatch (separate stderr for warn/error, dir's depth
            // option, table's tabular layout) lives in a future
            // phase that ports the Cranelift dispatch table.
            if args.is_empty() {
                ctx.block().call_void(
                    "js_console_log_dynamic",
                    &[(DOUBLE, &"0.0".to_string())],
                );
                return Ok("0.0".to_string());
            }
            // console.table(data) — dedicated table renderer.
            if property == "table" && args.len() == 1 {
                let v = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_console_table", &[(DOUBLE, &v)]);
                return Ok("0.0".to_string());
            }
            // Single-arg fast path: just print directly.
            if args.len() == 1 {
                let arg = &args[0];
                let is_number_literal = matches!(arg, Expr::Integer(_) | Expr::Number(_));
                let v = lower_expr(ctx, arg)?;
                let runtime_fn = if is_number_literal {
                    "js_console_log_number"
                } else {
                    "js_console_log_dynamic"
                };
                ctx.block().call_void(runtime_fn, &[(DOUBLE, &v)]);
                return Ok("0.0".to_string());
            }
            // Multi-arg: bundle all args into a heap array and call
            // js_console_log_spread, which uses the runtime's
            // format_jsvalue (Node-style util.inspect output for
            // objects/arrays). This is more accurate than
            // js_jsvalue_to_string which only does the JS toString
            // protocol (returns "[object Object]" for plain objects).
            let cap = (args.len() as u32).to_string();
            let mut current_arr = ctx
                .block()
                .call(I64, "js_array_alloc", &[(I32, &cap)]);
            for arg in args.iter() {
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                current_arr = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current_arr), (DOUBLE, &v)],
                );
            }
            let runtime_fn = match property.as_str() {
                "warn" => "js_console_warn_spread",
                "error" => "js_console_error_spread",
                _ => "js_console_log_spread",
            };
            ctx.block().call_void(runtime_fn, &[(I64, &current_arr)]);
            return Ok("0.0".to_string());
        }
    }

    // -------- Promise.resolve / reject / all / race / allSettled --------
    //
    // The HIR doesn't have dedicated PromiseResolve/Reject variants —
    // they appear as Call { callee: PropertyGet { GlobalGet(0), "resolve" } }.
    // Following Cranelift's `expr.rs:9617` heuristic, we assume any
    // GlobalGet receiver with a Promise-shaped property name is the
    // Promise constructor. (This conflicts with `console.resolve` etc.
    // — but those don't exist in JS.)
    if let Expr::PropertyGet { object, property } = callee {
        if matches!(object.as_ref(), Expr::GlobalGet(_)) {
            match property.as_str() {
                "resolve" => {
                    let value = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &value)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "reject" => {
                    let reason = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_rejected", &[(DOUBLE, &reason)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "all" | "race" | "allSettled" => {
                    if args.is_empty() {
                        return Ok(double_literal(0.0));
                    }
                    let arr_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let runtime_fn = match property.as_str() {
                        "all" => "js_promise_all",
                        "race" => "js_promise_race",
                        _ => "js_promise_all_settled",
                    };
                    let handle = blk.call(I64, runtime_fn, &[(I64, &arr_handle)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                _ => {}
            }
        }
    }

    // Fallthrough: assume the callee evaluates to a closure value at
    // runtime and dispatch through `js_closure_call<N>`. This catches:
    //   - LocalGet of an `: any`-typed local that the static check missed
    //   - Nested calls like `curry(1)(2)(3)` where the callee is itself
    //     a Call returning a function
    //   - PropertyGet on a class instance whose property is a closure
    //
    // The runtime checks the closure header on its own — if the value
    // isn't actually a closure, js_closure_call<N> handles the error.
    if args.len() <= 5 {
        let recv_box = lower_expr(ctx, callee)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &recv_box);
        let runtime_fn = format!("js_closure_call{}", args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
    }

    bail!(
        "perry-codegen-llvm: Call callee shape not supported ({}) with {} args",
        variant_name(callee),
        args.len()
    )
}

/// Lower `new ClassName(args…)` — Phase C.1.
///
/// Strategy: allocate an anonymous object via `js_object_alloc(0, N)`
/// where N is the field count, NaN-box the pointer, then inline the
/// constructor body with:
/// - a fresh local-id-keyed alloca slot for each constructor parameter
///   (pre-populated with the lowered argument value)
/// - a `this_stack` entry pointing at a slot holding the new object
///
/// `Expr::This` then loads from the top of `this_stack`. `this.x = v`
/// goes through the existing `Expr::PropertySet` path which targets
/// `js_object_set_field_by_name`.
///
/// Limitations of this first slice:
/// - No inheritance (parent classes ignored)
/// - No method calls on instances (just field reads/writes via the
///   existing PropertyGet/PropertySet paths)
/// - Constructor cannot use `return <expr>` (would terminate the
///   enclosing function, not the constructor body)
/// - No method dispatch or vtables — those land in Phase C.2/C.3
pub(crate) fn lower_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<String> {
    // Built-in Web classes that the runtime provides constructors for.
    // These are checked BEFORE the ctx.classes lookup because the user
    // code may shadow the name — if they do, the class lookup below
    // wins.
    if !ctx.classes.contains_key(class_name) {
        if let Some(val) = lower_builtin_new(ctx, class_name, args)? {
            return Ok(val);
        }
    }

    let class = match ctx.classes.get(class_name).copied() {
        Some(c) => c,
        None => {
            // Built-in / native class (Promise, Error, Date, etc.) with
            // no dedicated lower_builtin_new handler — lower args for
            // side effects (closures, string literal interning) and
            // return a sentinel. Real dispatch happens via later
            // NativeMethodCall / PropertyGet paths.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            // Allocate an empty object as the placeholder.
            let class_id = "0".to_string();
            let count = "0".to_string();
            let handle = ctx
                .block()
                .call(I64, "js_object_alloc", &[(I32, &class_id), (I32, &count)]);
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    };

    // Lower the args first (constructor params).
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered_args.push(lower_expr(ctx, a)?);
    }

    // Compute total field count including inherited parent fields.
    // The runtime allocates at least 8 inline slots regardless, so this
    // mostly matters for shapes >8 fields.
    let mut field_count = class.fields.len() as u32;
    let mut parent = class.extends_name.as_deref();
    while let Some(parent_name) = parent {
        if let Some(p) = ctx.classes.get(parent_name).copied() {
            field_count += p.fields.len() as u32;
            parent = p.extends_name.as_deref();
        } else {
            break;
        }
    }

    // Allocate the object with the per-class id and (if applicable)
    // parent class id, so the runtime registers the inheritance
    // chain for instanceof / virtual dispatch lookups.
    let cid = ctx.class_ids.get(class_name).copied().unwrap_or(0);
    let parent_cid = class
        .extends_name
        .as_deref()
        .and_then(|p| ctx.class_ids.get(p).copied())
        .unwrap_or(0);
    let cid_str = cid.to_string();
    let parent_cid_str = parent_cid.to_string();
    let n_str = field_count.to_string();
    let obj_handle = if parent_cid != 0 {
        ctx.block().call(
            I64,
            "js_object_alloc_with_parent",
            &[(I32, &cid_str), (I32, &parent_cid_str), (I32, &n_str)],
        )
    } else {
        ctx.block()
            .call(I64, "js_object_alloc", &[(I32, &cid_str), (I32, &n_str)])
    };
    let obj_box = nanbox_pointer_inline(ctx.block(), &obj_handle);

    // Allocate a `this` slot and store the new object there.
    let this_slot = ctx.block().alloca(DOUBLE);
    ctx.block().store(DOUBLE, &obj_box, &this_slot);
    ctx.this_stack.push(this_slot);
    ctx.class_stack.push(class_name.to_string());

    // Apply field initializers FIRST — TypeScript / ES2022 semantics:
    // class field initializers run at the start of the constructor body
    // (after super() for derived classes, before any user ctor code).
    // Walk the parent chain from the root down so parent fields are
    // initialized before the child's fields.
    apply_field_initializers_recursive(ctx, class_name)?;

    // If there's a constructor, inline its body. We allocate slots for
    // each constructor parameter and pre-populate them with the lowered
    // argument values. Locals/local_types are saved and restored to keep
    // the constructor's bindings scoped to its body — they don't leak
    // back into the enclosing function.
    if let Some(ctor) = &class.constructor {
        let saved_locals = ctx.locals.clone();
        let saved_local_types = ctx.local_types.clone();

        for (param, arg_val) in ctor.params.iter().zip(lowered_args.iter()) {
            let slot = ctx.block().alloca(DOUBLE);
            ctx.block().store(DOUBLE, arg_val, &slot);
            ctx.locals.insert(param.id, slot);
            ctx.local_types.insert(param.id, param.ty.clone());
        }

        // Lower the constructor body. Errors propagate.
        crate::stmt::lower_stmts(ctx, &ctor.body)?;

        // Restore the enclosing function's local scope.
        ctx.locals = saved_locals;
        ctx.local_types = saved_local_types;
    } else {
        // No own constructor — walk the parent chain to find an
        // inherited constructor and inline it. TypeScript semantics:
        // `class Child extends Parent {}` auto-forwards constructor
        // arguments to the parent constructor.
        let mut parent_name = class.extends_name.as_deref();
        while let Some(pname) = parent_name {
            if let Some(parent_class) = ctx.classes.get(pname).copied() {
                if let Some(parent_ctor) = &parent_class.constructor {
                    let saved_locals = ctx.locals.clone();
                    let saved_local_types = ctx.local_types.clone();

                    // Map constructor params from the parent's ctor to
                    // the supplied args. If caller passed fewer args
                    // than the parent expects, extra params get
                    // undefined.
                    for (i, param) in parent_ctor.params.iter().enumerate() {
                        let slot = ctx.block().alloca(DOUBLE);
                        if i < lowered_args.len() {
                            ctx.block().store(DOUBLE, &lowered_args[i], &slot);
                        } else {
                            let undef = crate::nanbox::double_literal(
                                f64::from_bits(crate::nanbox::TAG_UNDEFINED),
                            );
                            ctx.block().store(DOUBLE, &undef, &slot);
                        }
                        ctx.locals.insert(param.id, slot);
                        ctx.local_types.insert(param.id, param.ty.clone());
                    }

                    // Push the parent class name so `this` inside the
                    // parent ctor body resolves field names via the
                    // parent's field list.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(pname.to_string());

                    crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;

                    // Restore class_stack to the child.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(class_name.to_string());

                    ctx.locals = saved_locals;
                    ctx.local_types = saved_local_types;
                    break; // Found and inlined the parent ctor.
                }
                parent_name = parent_class.extends_name.as_deref();
            } else {
                break;
            }
        }
    }

    ctx.this_stack.pop();
    ctx.class_stack.pop();
    Ok(obj_box)
}

/// Walk the inheritance chain from the root down and apply each class's
/// field initializers to `this`. Call this inside `lower_new` after the
/// `this` slot is pushed but before the constructor body is inlined.
///
/// Initializers run in declaration order: root parent first, then each
/// child, matching JavaScript / TypeScript class semantics where fields
/// are initialized before user-written constructor code executes (field
/// initializers are conceptually prepended to the constructor body).
fn apply_field_initializers_recursive(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
) -> Result<()> {
    // Collect the inheritance chain from root down.
    let mut chain: Vec<String> = Vec::new();
    let mut cur = Some(class_name.to_string());
    while let Some(c) = cur {
        let Some(class) = ctx.classes.get(&c).copied() else { break };
        chain.push(c.clone());
        cur = class.extends_name.clone();
    }
    chain.reverse();

    for class_name_in_chain in chain {
        let class = match ctx.classes.get(&class_name_in_chain).copied() {
            Some(c) => c,
            None => continue,
        };
        // Collect (property_name, init_expr) pairs up-front to avoid
        // holding an immutable borrow of ctx.classes across lower_expr.
        let mut init_pairs: Vec<(String, Expr)> = Vec::new();
        for field in &class.fields {
            if let Some(init) = &field.init {
                init_pairs.push((field.name.clone(), init.clone()));
            }
        }
        if init_pairs.is_empty() {
            continue;
        }

        // Temporarily swap class_stack so `this.field` in the init
        // resolves against the correct class.
        ctx.class_stack.push(class_name_in_chain.clone());
        for (prop, init_expr) in init_pairs {
            // Build a PropertySet { this, prop, init_expr } and lower.
            let set_expr = Expr::PropertySet {
                object: Box::new(Expr::This),
                property: prop,
                value: Box::new(init_expr),
            };
            let _ = lower_expr(ctx, &set_expr)?;
        }
        ctx.class_stack.pop();
    }
    Ok(())
}

/// Lower a `NativeMethodCall { module, method, object, args }` (Phase H.1).
///
/// Currently supports:
/// - `array.push_single` / `array.push` (single-arg push) on typed arrays
/// - `array.pop_back` / `array.pop` on typed arrays
///
/// The receiver is either a `PropertyGet { object, property }` (the
/// `this.items.push(x)` case) or a `LocalGet` (the `arr.push(x)` case).
/// For both shapes we chain a get + push + write-back so reallocations
/// are reflected in the source storage.
pub(crate) fn lower_native_method_call(
    ctx: &mut FnCtx<'_>,
    module: &str,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<String> {
    // Web Fetch API dispatch — Response / Headers / Request / static
    // factories. Handled before the receiver-less early-out so that
    // `Response.json(v)` (object.is_none()) finds its runtime function.
    if let Some(val) = lower_fetch_native_method(ctx, module, method, object, args)? {
        return Ok(val);
    }

    // Receiver-less native method calls (e.g. plugin::setConfig(...)
    // as a static module function): lower args for side effects and
    // return a sentinel. Compilation succeeds; runtime gets a NaN.
    let Some(recv) = object else {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(double_literal(0.0));
    };
    let _ = (module, method); // shut up unused warnings on the early-out path

    if module == "array" && (method == "push_single" || method == "push") {
        if args.len() != 1 {
            bail!("array.push expects 1 arg, got {}", args.len());
        }
        let v = lower_expr(ctx, &args[0])?;
        // Lower the receiver once. We'll use the resulting box for both
        // the (Get → push → re-Set) cycle.
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let arr_handle = unbox_to_i64(blk, &arr_box);
        let new_handle = blk.call(
            I64,
            "js_array_push_f64",
            &[(I64, &arr_handle), (DOUBLE, &v)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        // Write the (possibly-realloc'd) pointer back to the receiver.
        // Two cases:
        //   1. recv = LocalGet(id) → store back to the local's slot
        //   2. recv = PropertyGet { obj, prop } → set obj.prop = new_box
        // Anything else: skip the write-back (the array may dangle on
        // realloc, but we don't crash at codegen).
        match recv {
            Expr::LocalGet(id) => {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    ctx.block().store(DOUBLE, &new_box, &slot);
                } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                    let g_ref = format!("@{}", global_name);
                    ctx.block().store(DOUBLE, &new_box, &g_ref);
                }
            }
            Expr::PropertyGet { object: obj_expr, property } => {
                let obj_box = lower_expr(ctx, obj_expr)?;
                let key_idx = ctx.strings.intern(property);
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
                    &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &new_box)],
                );
            }
            _ => {
                // No write-back — the receiver is some computed value.
                // The array may dangle on realloc, but the immediate
                // call sees the right pointer.
            }
        }
        // push returns the new length in JS spec; for now we return
        // the new boxed pointer (statement context discards it).
        return Ok(new_box);
    }

    if module == "array" && (method == "pop_back" || method == "pop") {
        if !args.is_empty() {
            bail!("array.pop expects 0 args, got {}", args.len());
        }
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let arr_handle = unbox_to_i64(blk, &arr_box);
        return Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &arr_handle)]));
    }

    // Unknown native method: lower the receiver and args for side
    // effects (so closures inside them get auto-collected and any
    // string literals get interned), then return a sentinel. This
    // unblocks compilation of programs that touch native modules
    // we haven't wired up yet — they'll produce garbage at runtime
    // but won't fail at codegen time.
    let _ = lower_expr(ctx, recv)?;
    for a in args {
        let _ = lower_expr(ctx, a)?;
    }
    Ok(double_literal(0.0))
}

/// Extract a raw string pointer (i64) from a NaN-boxed JSValue via the
/// unified helper. Handles string literals, concat results, and any
/// other expression that produces a NaN-boxed double.
fn get_raw_string_ptr(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    let v = lower_expr(ctx, e)?;
    let blk = ctx.block();
    Ok(blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &v)]))
}

/// Build a Headers handle from an inline object literal `{ "k": "v", ... }`.
/// Returns the f64 handle (raw numeric, not NaN-boxed).
fn build_headers_from_object(
    ctx: &mut FnCtx<'_>,
    props: &[(String, Expr)],
) -> Result<String> {
    let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
    for (k, vexpr) in props {
        let key_expr = Expr::String(k.clone());
        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
        let val_ptr = get_raw_string_ptr(ctx, vexpr)?;
        ctx.block().call(
            DOUBLE,
            "js_headers_set",
            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
        );
    }
    Ok(h)
}

/// Lower `new ClassName(args)` for the built-in Web classes that don't
/// live in `ctx.classes`. Returns `Ok(None)` if the class isn't one we
/// handle here (caller should fall through to the default path).
pub(crate) fn lower_builtin_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    match class_name {
        "Response" => {
            // new Response(body?, init?) — init = { status?, statusText?, headers? }
            let body_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
            } else {
                "0".to_string()
            };

            // Default init: status=200, statusText=null, headers=0
            let mut status_val = "200.0".to_string();
            let mut status_text_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();

            if args.len() >= 2 {
                if let Expr::Object(props) = &args[1] {
                    for (k, vexpr) in props {
                        match k.as_str() {
                            "status" => {
                                status_val = lower_expr(ctx, vexpr)?;
                            }
                            "statusText" => {
                                status_text_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "headers" => {
                                // Inline object → build a Headers handle.
                                // Any other expression → use as a Headers
                                // handle (numeric f64) directly.
                                if let Expr::Object(hprops) = vexpr {
                                    headers_handle = build_headers_from_object(ctx, hprops)?;
                                } else {
                                    headers_handle = lower_expr(ctx, vexpr)?;
                                }
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    // Not an object literal — still evaluate for side effects.
                    let _ = lower_expr(ctx, &args[1])?;
                }
            }

            let handle = ctx.block().call(
                DOUBLE,
                "js_response_new",
                &[
                    (I64, &body_ptr),
                    (DOUBLE, &status_val),
                    (I64, &status_text_ptr),
                    (DOUBLE, &headers_handle),
                ],
            );
            // Response handle is a plain numeric f64 (response-registry id).
            // DO NOT NaN-box — method dispatch expects raw f64.
            Ok(Some(handle))
        }

        "Headers" => {
            // new Headers(init?) — init can be an object literal or another
            // Headers/array iterable. Only inline object literals are
            // handled so far; anything else falls back to empty.
            let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
            if !args.is_empty() {
                if let Expr::Object(props) = &args[0] {
                    for (k, vexpr) in props {
                        let key_expr = Expr::String(k.clone());
                        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
                        let val_ptr = get_raw_string_ptr(ctx, vexpr)?;
                        ctx.block().call(
                            DOUBLE,
                            "js_headers_set",
                            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
                        );
                    }
                } else {
                    let _ = lower_expr(ctx, &args[0])?;
                }
            }
            Ok(Some(h))
        }

        "Request" => {
            // new Request(url, init?) — init = { method?, body?, headers? }
            let url_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
            } else {
                "0".to_string()
            };

            let mut method_ptr = "0".to_string();
            let mut body_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();

            if args.len() >= 2 {
                if let Expr::Object(props) = &args[1] {
                    for (k, vexpr) in props {
                        match k.as_str() {
                            "method" => {
                                method_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "body" => {
                                body_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "headers" => {
                                if let Expr::Object(hprops) = vexpr {
                                    headers_handle = build_headers_from_object(ctx, hprops)?;
                                } else {
                                    headers_handle = lower_expr(ctx, vexpr)?;
                                }
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[1])?;
                }
            }

            let handle = ctx.block().call(
                DOUBLE,
                "js_request_new",
                &[
                    (I64, &url_ptr),
                    (I64, &method_ptr),
                    (I64, &body_ptr),
                    (DOUBLE, &headers_handle),
                ],
            );
            Ok(Some(handle))
        }

        "AbortController" => {
            // Lower any incidental args for side effects (shouldn't have any).
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_abort_controller_new", &[]);
            // The runtime returns a raw *mut ObjectHeader — NaN-box with
            // POINTER_TAG so regular property get (`controller.signal`,
            // `controller.aborted`) works via js_object_get_field_by_name_f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
        }

        _ => Ok(None),
    }
}

/// Returns `true` if the expression statically resolves to an
/// `AbortController`-typed value (either a local whose declared type
/// is `Named("AbortController")` or a `new AbortController()` call).
fn is_abort_controller_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::New { class_name, .. } => class_name == "AbortController",
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Named(n)) if n == "AbortController"
        ),
        _ => false,
    }
}

/// Lower AbortController / AbortSignal method calls:
/// - `controller.abort(reason?)`
/// - `controller.signal.addEventListener("abort", cb)`
/// - `AbortSignal.timeout(ms)` (static)
///
/// Returns `None` if the call shape doesn't match one of the handled
/// patterns — caller falls through to the generic dispatch.
fn lower_abort_controller_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── AbortSignal.timeout(ms) static ──
    if property == "timeout" {
        if let Expr::GlobalGet(_) = object {
            // Can't distinguish AbortSignal.timeout from other globals
            // without more context — skip.
        }
    }
    // Static `AbortSignal.timeout(ms)` — matched via a PropertyGet on a
    // GlobalGet-shaped object isn't quite right because GlobalGet has
    // no name; best we can do is detect by property name "timeout" and
    // the local-isn't-a-known-thing. Skip for now.

    // ── controller.abort(reason?) ──
    if property == "abort" && is_abort_controller_expr(ctx, object) {
        let recv_box = lower_expr(ctx, object)?;
        let blk = ctx.block();
        let ctrl_handle = unbox_to_i64(blk, &recv_box);
        if args.is_empty() {
            blk.call_void("js_abort_controller_abort", &[(I64, &ctrl_handle)]);
        } else {
            let reason = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            blk.call_void(
                "js_abort_controller_abort_reason",
                &[(I64, &ctrl_handle), (DOUBLE, &reason)],
            );
        }
        return Ok(Some(double_literal(f64::from_bits(
            crate::nanbox::TAG_UNDEFINED,
        ))));
    }

    // ── controller.signal.addEventListener("abort", cb) ──
    if property == "addEventListener" && args.len() >= 2 {
        if let Expr::PropertyGet {
            object: inner_obj,
            property: inner_prop,
        } = object
        {
            if inner_prop == "signal" && is_abort_controller_expr(ctx, inner_obj) {
                let ctrl_box = lower_expr(ctx, inner_obj)?;
                let blk = ctx.block();
                let ctrl_handle = unbox_to_i64(blk, &ctrl_box);
                // Get the signal pointer.
                let signal_handle = blk.call(
                    I64,
                    "js_abort_controller_signal",
                    &[(I64, &ctrl_handle)],
                );
                let evt = lower_expr(ctx, &args[0])?;
                let listener = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                blk.call_void(
                    "js_abort_signal_add_listener",
                    &[(I64, &signal_handle), (DOUBLE, &evt), (DOUBLE, &listener)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
        }
    }

    Ok(None)
}

/// Dispatch for the Web Fetch API family: Response/Headers/Request
/// methods and property getters. Called before the generic
/// `lower_native_method_call` path so static factories
/// (`Response.json(v)`) also land here. Returns `Ok(None)` if the
/// (module, method) combination isn't handled.
///
/// Handle ABI note: Response/Headers/Request handles are plain numeric
/// doubles (ids into the runtime's registry), not NaN-boxed pointers.
/// Most runtime functions take the handle as f64; status/statusText/
/// ok/text/json take i64 and we convert via `fptosi`.
fn lower_fetch_native_method(
    ctx: &mut FnCtx<'_>,
    module: &str,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── Response static factories (no receiver) ──
    if module == "fetch" && object.is_none() {
        match method {
            "static_json" => {
                let v = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let handle = ctx
                    .block()
                    .call(DOUBLE, "js_response_static_json", &[(DOUBLE, &v)]);
                return Ok(Some(handle));
            }
            "static_redirect" => {
                let url_ptr = if !args.is_empty() {
                    get_raw_string_ptr(ctx, &args[0])?
                } else {
                    "0".to_string()
                };
                let status = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    "302.0".to_string()
                };
                let handle = ctx.block().call(
                    DOUBLE,
                    "js_response_static_redirect",
                    &[(I64, &url_ptr), (DOUBLE, &status)],
                );
                return Ok(Some(handle));
            }
            _ => {}
        }
    }

    // Everything below needs a receiver.
    let Some(recv) = object else {
        return Ok(None);
    };

    // ── Headers method dispatch ──
    if module == "Headers" {
        let h_handle = lower_expr(ctx, recv)?;
        match method {
            "set" | "append" => {
                if args.len() < 2 {
                    return Ok(Some(double_literal(0.0)));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let val_ptr = get_raw_string_ptr(ctx, &args[1])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_set",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr), (I64, &val_ptr)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            "get" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(0.0)));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let str_ptr = ctx.block().call(
                    I64,
                    "js_headers_get",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "has" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_FALSE,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                let out = ctx.block().call(
                    DOUBLE,
                    "js_headers_has",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                return Ok(Some(out));
            }
            "delete" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ))));
                }
                let key_ptr = get_raw_string_ptr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_delete",
                    &[(DOUBLE, &h_handle), (I64, &key_ptr)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            "forEach" => {
                if args.is_empty() {
                    return Ok(Some(double_literal(0.0)));
                }
                let cb = lower_expr(ctx, &args[0])?;
                ctx.block().call(
                    DOUBLE,
                    "js_headers_for_each",
                    &[(DOUBLE, &h_handle), (DOUBLE, &cb)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            _ => return Ok(None),
        }
    }

    // ── Request property getters ──
    if module == "Request" {
        let h_handle = lower_expr(ctx, recv)?;
        match method {
            "url" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_url", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "method" => {
                let str_ptr = ctx
                    .block()
                    .call(I64, "js_request_get_method", &[(DOUBLE, &h_handle)]);
                let blk = ctx.block();
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "body" => {
                let val = ctx
                    .block()
                    .call(DOUBLE, "js_request_get_body", &[(DOUBLE, &h_handle)]);
                return Ok(Some(val));
            }
            _ => return Ok(None),
        }
    }

    // ── Response methods / property getters ──
    if module == "fetch" {
        // Lower the receiver once. It may be a Response (f64 handle) or
        // a chained result from `.headers` / `.clone()` — in the former
        // case we dispatch the methods here; the chain cases are
        // recognised at the Call callsite in lower_call.
        let recv_handle = lower_expr(ctx, recv)?;
        match method {
            "text" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let promise = blk.call(I64, "js_fetch_response_text", &[(I64, &h_i64)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "json" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let promise = blk.call(I64, "js_fetch_response_json", &[(I64, &h_i64)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "status" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let status = blk.call(DOUBLE, "js_fetch_response_status", &[(I64, &h_i64)]);
                return Ok(Some(status));
            }
            "statusText" => {
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let str_ptr =
                    blk.call(I64, "js_fetch_response_status_text", &[(I64, &h_i64)]);
                return Ok(Some(nanbox_string_inline(blk, &str_ptr)));
            }
            "ok" => {
                // js_fetch_response_ok returns 1.0 or 0.0 as f64. Map to
                // TAG_TRUE/TAG_FALSE so console.log prints "true"/"false".
                let blk = ctx.block();
                let h_i64 = blk.fptosi(DOUBLE, &recv_handle, I64);
                let raw = blk.call(DOUBLE, "js_fetch_response_ok", &[(I64, &h_i64)]);
                let cmp = blk.fcmp("une", &raw, "0.0");
                let tagged = blk.select(
                    crate::types::I1,
                    &cmp,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(Some(blk.bitcast_i64_to_double(&tagged)));
            }
            "headers" => {
                let out = ctx.block().call(
                    DOUBLE,
                    "js_response_get_headers",
                    &[(DOUBLE, &recv_handle)],
                );
                return Ok(Some(out));
            }
            "clone" => {
                let out = ctx
                    .block()
                    .call(DOUBLE, "js_response_clone", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(out));
            }
            "arrayBuffer" => {
                let blk = ctx.block();
                let promise =
                    blk.call(I64, "js_response_array_buffer", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            "blob" => {
                let blk = ctx.block();
                let promise = blk.call(I64, "js_response_blob", &[(DOUBLE, &recv_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &promise)));
            }
            _ => return Ok(None),
        }
    }

    Ok(None)
}
