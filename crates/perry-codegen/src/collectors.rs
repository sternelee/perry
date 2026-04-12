//! Basic AST walkers for collecting closures, extern func refs, let ids,
//! and ref ids from HIR statements and expressions.
//!
//! Extracted from `codegen.rs` — purely structural refactor, no logic changes.

use std::collections::HashSet;

/// Walk for `Expr::Closure` instances and collect each one along with
/// its `func_id` so the codegen can emit the body as a top-level
/// function. Each closure expression is captured by clone (it's the
/// load-bearing data; the rest of the function context lives in
/// `compile_closure`).
pub(crate) fn collect_closures_in_stmts(
    stmts: &[perry_hir::Stmt],
    seen: &mut HashSet<perry_types::FuncId>,
    out: &mut Vec<(perry_types::FuncId, perry_hir::Expr)>,
) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Expr(e) | perry_hir::Stmt::Throw(e) => {
                collect_closures_in_expr(e, seen, out);
            }
            perry_hir::Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_closures_in_expr(e, seen, out);
                }
            }
            perry_hir::Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_closures_in_expr(e, seen, out);
                }
            }
            perry_hir::Stmt::If { condition, then_branch, else_branch } => {
                collect_closures_in_expr(condition, seen, out);
                collect_closures_in_stmts(then_branch, seen, out);
                if let Some(eb) = else_branch {
                    collect_closures_in_stmts(eb, seen, out);
                }
            }
            perry_hir::Stmt::While { condition, body } => {
                collect_closures_in_expr(condition, seen, out);
                collect_closures_in_stmts(body, seen, out);
            }
            perry_hir::Stmt::DoWhile { body, condition } => {
                collect_closures_in_stmts(body, seen, out);
                collect_closures_in_expr(condition, seen, out);
            }
            perry_hir::Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    collect_closures_in_stmts(std::slice::from_ref(init_stmt), seen, out);
                }
                if let Some(cond) = condition {
                    collect_closures_in_expr(cond, seen, out);
                }
                if let Some(upd) = update {
                    collect_closures_in_expr(upd, seen, out);
                }
                collect_closures_in_stmts(body, seen, out);
            }
            perry_hir::Stmt::Switch { discriminant, cases } => {
                collect_closures_in_expr(discriminant, seen, out);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_closures_in_expr(test, seen, out);
                    }
                    collect_closures_in_stmts(&case.body, seen, out);
                }
            }
            perry_hir::Stmt::Try { body, catch, finally } => {
                collect_closures_in_stmts(body, seen, out);
                if let Some(c) = catch {
                    collect_closures_in_stmts(&c.body, seen, out);
                }
                if let Some(f) = finally {
                    collect_closures_in_stmts(f, seen, out);
                }
            }
            perry_hir::Stmt::Labeled { body, .. } => {
                collect_closures_in_stmts(std::slice::from_ref(body.as_ref()), seen, out);
            }
            _ => {}
        }
    }
}

fn collect_closures_in_expr(
    e: &perry_hir::Expr,
    seen: &mut HashSet<perry_types::FuncId>,
    out: &mut Vec<(perry_types::FuncId, perry_hir::Expr)>,
) {
    use perry_hir::{ArrayElement, Expr};
    // Helper closure that recurses into a sub-expression. We use a
    // local closure rather than a method so we can keep the same
    // recursion entry point.
    let mut walk = |sub: &Expr,
                    seen: &mut HashSet<perry_types::FuncId>,
                    out: &mut Vec<(perry_types::FuncId, Expr)>| {
        collect_closures_in_expr(sub, seen, out);
    };
    match e {
        Expr::Closure { func_id, body, .. } => {
            if seen.insert(*func_id) {
                out.push((*func_id, e.clone()));
            }
            // Recurse into the closure body so nested closures are
            // collected too.
            collect_closures_in_stmts(body, seen, out);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk(left, seen, out);
            walk(right, seen, out);
        }
        Expr::Unary { operand, .. } | Expr::Void(operand) | Expr::TypeOf(operand) => {
            walk(operand, seen, out);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            walk(condition, seen, out);
            walk(then_expr, seen, out);
            walk(else_expr, seen, out);
        }
        Expr::Call { callee, args, .. } => {
            walk(callee, seen, out);
            for a in args {
                walk(a, seen, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            walk(callee, seen, out);
            for a in args {
                use perry_hir::CallArg;
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => walk(e, seen, out),
                }
            }
        }
        Expr::PropertyGet { object, .. } => walk(object, seen, out),
        Expr::PropertySet { object, value, .. } => {
            walk(object, seen, out);
            walk(value, seen, out);
        }
        Expr::IndexGet { object, index } => {
            walk(object, seen, out);
            walk(index, seen, out);
        }
        Expr::IndexSet { object, index, value } => {
            walk(object, seen, out);
            walk(index, seen, out);
            walk(value, seen, out);
        }
        Expr::LocalSet(_, value) => walk(value, seen, out),
        Expr::Array(elements) => {
            for el in elements {
                walk(el, seen, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e, seen, out),
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk(v, seen, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk(a, seen, out);
            }
        }
        // Any expression that takes a callback can hide a closure.
        // The catch-all `_ => {}` would silently miss them, leading
        // to "use of undefined value @perry_closure_*" link errors.
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback } => {
            walk(array, seen, out);
            walk(callback, seen, out);
        }
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            walk(array, seen, out);
            walk(callback, seen, out);
            if let Some(init) = initial {
                walk(init, seen, out);
            }
        }
        Expr::ArraySort { array, comparator } => {
            walk(array, seen, out);
            walk(comparator, seen, out);
        }
        Expr::ArrayFlatMap { array, callback } => {
            walk(array, seen, out);
            walk(callback, seen, out);
        }
        Expr::ArrayFlat { array } => walk(array, seen, out),
        Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback } => {
            walk(array, seen, out);
            walk(callback, seen, out);
        }
        Expr::ArrayUnshift { value, .. } => walk(value, seen, out),
        Expr::ArrayIncludes { array, value } => {
            walk(array, seen, out);
            walk(value, seen, out);
        }
        Expr::ArrayIndexOf { array, value } => {
            walk(array, seen, out);
            walk(value, seen, out);
        }
        Expr::ArraySplice { start, delete_count, items, .. } => {
            walk(start, seen, out);
            if let Some(d) = delete_count {
                walk(d, seen, out);
            }
            for it in items {
                walk(it, seen, out);
            }
        }
        Expr::ArrayEntries(o) | Expr::ArrayKeys(o) | Expr::ArrayValues(o) => {
            walk(o, seen, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            walk(array, seen, out);
            if let Some(c) = comparator {
                walk(c, seen, out);
            }
        }
        Expr::ArrayToReversed { array } | Expr::ArrayFlat { array } => walk(array, seen, out),
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            walk(array, seen, out);
            walk(start, seen, out);
            walk(delete_count, seen, out);
            for it in items {
                walk(it, seen, out);
            }
        }
        Expr::ArrayWith { array, index, value } => {
            walk(array, seen, out);
            walk(index, seen, out);
            walk(value, seen, out);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
            walk(target, seen, out);
            walk(start, seen, out);
            if let Some(e) = end {
                walk(e, seen, out);
            }
        }
        Expr::ArrayAt { array, index } => {
            walk(array, seen, out);
            walk(index, seen, out);
        }
        Expr::QueueMicrotask(cb) | Expr::ProcessNextTick(cb) => {
            walk(cb, seen, out);
        }
        Expr::ProcessOn { event, handler } => {
            walk(event, seen, out);
            walk(handler, seen, out);
        }
        Expr::Sequence(es) => {
            for e in es {
                walk(e, seen, out);
            }
        }
        Expr::Delete(o) => walk(o, seen, out),
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                walk(e, seen, out);
            }
        }
        Expr::SetNewFromArray(arr) => walk(arr, seen, out),
        Expr::StaticMethodCall { args, .. } | Expr::SuperMethodCall { args, .. } => {
            for a in args {
                walk(a, seen, out);
            }
        }
        Expr::SuperCall(args) => {
            for a in args {
                walk(a, seen, out);
            }
        }
        Expr::ArrayFrom(o) | Expr::Uint8ArrayFrom(o) => walk(o, seen, out),
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg { walk(a, seen, out); }
        }
        Expr::ArrayFromMapped { iterable, map_fn } => {
            walk(iterable, seen, out);
            walk(map_fn, seen, out);
        }
        Expr::FsExistsSync(p) | Expr::FsReadFileBinary(p) | Expr::FsUnlinkSync(p) => walk(p, seen, out),
        Expr::ParseInt { string, radix } => {
            walk(string, seen, out);
            if let Some(r) = radix {
                walk(r, seen, out);
            }
        }
        Expr::PathJoin(a, b) => {
            walk(a, seen, out);
            walk(b, seen, out);
        }
        Expr::ObjectValues(o) | Expr::ObjectEntries(o) => walk(o, seen, out),
        Expr::ObjectGroupBy { items, key_fn } => {
            walk(items, seen, out);
            walk(key_fn, seen, out);
        }
        Expr::RegExpTest { regex, string } | Expr::RegExpExec { regex, string } => {
            walk(regex, seen, out);
            walk(string, seen, out);
        }
        Expr::Await(o) => walk(o, seen, out),
        Expr::ObjectRest { object, .. } => walk(object, seen, out),
        Expr::StaticFieldSet { value, .. } => walk(value, seen, out),
        Expr::ArraySlice { array, start, end } => {
            walk(array, seen, out);
            walk(start, seen, out);
            if let Some(e) = end {
                walk(e, seen, out);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            walk(array, seen, out);
            if let Some(sep) = separator {
                walk(sep, seen, out);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            walk(array, seen, out);
            walk(start, seen, out);
            if let Some(e) = end {
                walk(e, seen, out);
            }
        }
        Expr::ArrayPush { value, .. } => walk(value, seen, out),
        Expr::MathPow(a, b) => {
            walk(a, seen, out);
            walk(b, seen, out);
        }
        Expr::MathSqrt(o)
        | Expr::MathFloor(o)
        | Expr::MathCeil(o)
        | Expr::MathRound(o)
        | Expr::MathAbs(o)
        | Expr::MathMinSpread(o)
        | Expr::MathMaxSpread(o)
        | Expr::IsFinite(o)
        | Expr::IsNaN(o)
        | Expr::IsUndefinedOrBareNan(o)
        | Expr::NumberIsNaN(o)
        | Expr::NumberIsFinite(o)
        | Expr::StringCoerce(o)
        | Expr::BooleanCoerce(o)
        | Expr::NumberCoerce(o)
        | Expr::ObjectKeys(o)
        | Expr::SetSize(o)
        | Expr::ParseFloat(o)
        | Expr::Await(o) => {
            walk(o, seen, out);
        }
        Expr::ParseInt { string, radix } => {
            walk(string, seen, out);
            if let Some(r) = radix {
                walk(r, seen, out);
            }
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                walk(v, seen, out);
            }
        }
        Expr::MapSet { map, key, value } => {
            walk(map, seen, out);
            walk(key, seen, out);
            walk(value, seen, out);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            walk(map, seen, out);
            walk(key, seen, out);
        }
        Expr::SetAdd { value, .. } => walk(value, seen, out),
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            walk(set, seen, out);
            walk(value, seen, out);
        }
        Expr::ErrorNew(opt) => {
            if let Some(o) = opt {
                walk(o, seen, out);
            }
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            walk(value, seen, out);
            walk(replacer, seen, out);
            walk(indent, seen, out);
        }
        Expr::JsonParseReviver { text, reviver } => {
            walk(text, seen, out);
            walk(reviver, seen, out);
        }
        Expr::JsonParseWithReviver(text, reviver) => {
            walk(text, seen, out);
            walk(reviver, seen, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk(o, seen, out);
            }
            for a in args {
                walk(a, seen, out);
            }
        }
        Expr::FsWriteFileSync(p, c) => {
            walk(p, seen, out);
            walk(c, seen, out);
        }
        Expr::FsExistsSync(p) | Expr::FsReadFileBinary(p) => walk(p, seen, out),
        Expr::In { property, object } => {
            walk(property, seen, out);
            walk(object, seen, out);
        }
        Expr::InstanceOf { expr, .. } => walk(expr, seen, out),
        // WeakRef / FinalizationRegistry: the target / callback operands can
        // be inline closures (e.g. `new FinalizationRegistry(held => ...)`),
        // so we must descend into them or the closure body never gets its
        // LLVM function emitted and codegen drops an `@perry_closure_*`
        // reference into IR with no matching definition.
        Expr::WeakRefNew(o) | Expr::WeakRefDeref(o) | Expr::FinalizationRegistryNew(o) => {
            walk(o, seen, out);
        }
        Expr::FinalizationRegistryRegister { registry, target, held, token } => {
            walk(registry, seen, out);
            walk(target, seen, out);
            walk(held, seen, out);
            if let Some(t) = token {
                walk(t, seen, out);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            walk(registry, seen, out);
            walk(token, seen, out);
        }
        // atob/btoa: the argument is just a string expression, but it could
        // still contain a nested closure (e.g. inside a ternary), so walk it.
        Expr::Atob(o) | Expr::Btoa(o) | Expr::StructuredClone(o) => walk(o, seen, out),
        // `new <expr>(args…)` — both the callee expression and any arg
        // can hide a closure (e.g. `new SomeBuilder(x => ...)`).
        Expr::NewDynamic { callee, args } => {
            walk(callee, seen, out);
            for a in args {
                walk(a, seen, out);
            }
        }
        // fetch(url, { method, body, headers }) — headers values can be
        // computed expressions containing closures (rare but legal).
        Expr::FetchWithOptions { url, method, body, headers } => {
            walk(url, seen, out);
            walk(method, seen, out);
            walk(body, seen, out);
            for (_, v) in headers {
                walk(v, seen, out);
            }
        }
        Expr::FetchGetWithAuth { url, auth_header } => {
            walk(url, seen, out);
            walk(auth_header, seen, out);
        }
        Expr::FetchPostWithAuth { url, auth_header, body } => {
            walk(url, seen, out);
            walk(auth_header, seen, out);
            walk(body, seen, out);
        }
        // I18n strings carry interpolation params that are arbitrary
        // expressions (so a closure could appear inside `${formatter()}`).
        Expr::I18nString { params, .. } => {
            for (_, v) in params {
                walk(v, seen, out);
            }
        }
        // Yield expressions wrap an inner value that may itself be a closure.
        Expr::Yield { value, .. } => {
            if let Some(v) = value { walk(v, seen, out); }
        }
        // Child process expressions — walk all sub-expressions.
        Expr::ChildProcessExecSync { command, options } => {
            walk(command, seen, out);
            if let Some(o) = options { walk(o, seen, out); }
        }
        Expr::ChildProcessSpawnSync { command, args, options } |
        Expr::ChildProcessSpawn { command, args, options } => {
            walk(command, seen, out);
            if let Some(a) = args { walk(a, seen, out); }
            if let Some(o) = options { walk(o, seen, out); }
        }
        Expr::ChildProcessExec { command, options, callback } => {
            walk(command, seen, out);
            if let Some(o) = options { walk(o, seen, out); }
            if let Some(c) = callback { walk(c, seen, out); }
        }
        Expr::ChildProcessSpawnBackground { command, args, log_file, env_json } => {
            walk(command, seen, out);
            if let Some(a) = args { walk(a, seen, out); }
            walk(log_file, seen, out);
            if let Some(e) = env_json { walk(e, seen, out); }
        }
        Expr::ChildProcessGetProcessStatus(h) |
        Expr::ChildProcessKillProcess(h) => walk(h, seen, out),
        // Reflect.* and other iterator/json wrappers — can carry callbacks.
        Expr::IteratorToArray(o) | Expr::ArrayIsArray(o) => walk(o, seen, out),
        Expr::JsonStringify(o) | Expr::JsonParse(o) => walk(o, seen, out),
        Expr::JsonStringifyPretty { value, replacer, space } => {
            walk(value, seen, out);
            if let Some(r) = replacer { walk(r, seen, out); }
            walk(space, seen, out);
        }
        _ => {}
    }
}

// NOTE: `collect_extern_func_refs_in_*` previously lived here as a
// pre-walker that scanned the HIR for cross-module Call sites and
// added a `declare` for each one to the LLVM module. It missed any
// Expr::ExternFuncRef hidden inside an Expr variant the walker didn't
// recurse into (Closure body, ArrayMap callback, Stmt::Try, etc.),
// which produced clang "use of undefined value @perry_fn_*" errors.
// Replaced by lazy declares emitted from `lower_call.rs` directly via
// `FnCtx.pending_declares`, drained back into the module after each
// compile_function/method/closure/static call returns.

/// Walk a sequence of statements and collect all LocalIds defined by
/// `Stmt::Let` (function-local declarations). Used by the module-globals
/// pre-walk to distinguish "this id is the function's own local" from
/// "this id refers to a module-level let".
pub(crate) fn collect_let_ids(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
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
pub(crate) fn collect_ref_ids_in_stmts(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
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

fn collect_ref_ids_in_expr(e: &perry_hir::Expr, out: &mut HashSet<u32>) {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let mut walk = |sub: &Expr, out: &mut HashSet<u32>| {
        collect_ref_ids_in_expr(sub, out);
    };
    match e {
        Expr::LocalGet(id) => {
            out.insert(*id);
        }
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            walk(value, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk(left, out);
            walk(right, out);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::IsFinite(operand)
        | Expr::IsNaN(operand)
        | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand)
        | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand)
        | Expr::ParseFloat(operand)
        | Expr::ObjectKeys(operand)
        | Expr::ObjectValues(operand)
        | Expr::ObjectEntries(operand)
        | Expr::ObjectFromEntries(operand)
        | Expr::ObjectIsFrozen(operand)
        | Expr::ObjectIsSealed(operand)
        | Expr::ObjectIsExtensible(operand)
        | Expr::ObjectCreate(operand)
        | Expr::SetSize(operand)
        | Expr::SetClear(operand)
        | Expr::ArrayFrom(operand)
        | Expr::Uint8ArrayFrom(operand)
        | Expr::IteratorToArray(operand)
        | Expr::WeakRefNew(operand)
        | Expr::WeakRefDeref(operand)
        | Expr::StructuredClone(operand)
        | Expr::QueueMicrotask(operand)
        | Expr::ProcessNextTick(operand)
        | Expr::FsExistsSync(operand)
        | Expr::FsReadFileSync(operand)
        | Expr::FsReadFileBinary(operand)
        | Expr::FsUnlinkSync(operand)
        | Expr::FsMkdirSync(operand)
        | Expr::PathDirname(operand)
        | Expr::PathBasename(operand)
        | Expr::PathExtname(operand)
        | Expr::PathResolve(operand)
        | Expr::PathNormalize(operand)
        | Expr::PathFormat(operand)
        | Expr::PathParse(operand)
        | Expr::DateToISOString(operand)
        | Expr::DateParse(operand)
        | Expr::EnvGetDynamic(operand)
        | Expr::ErrorNew(Some(operand))
        | Expr::FinalizationRegistryNew(operand)
        | Expr::Uint8ArrayNew(Some(operand))
        | Expr::Uint8ArrayLength(operand)
        | Expr::JsonParse(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathAbs(operand)
        | Expr::MathLog(operand)
        | Expr::MathLog2(operand)
        | Expr::MathLog10(operand)
        | Expr::MathLog1p(operand)
        | Expr::MathClz32(operand)
        | Expr::MathMinSpread(operand)
        | Expr::MathMaxSpread(operand) => {
            walk(operand, out);
        }
        Expr::Call { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => walk(e, out),
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk(o, out);
            }
            for a in args {
                walk(a, out);
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            walk(condition, out);
            walk(then_expr, out);
            walk(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => walk(object, out),
        Expr::PropertySet { object, value, .. } => {
            walk(object, out);
            walk(value, out);
        }
        Expr::PropertyUpdate { object, .. } => walk(object, out),
        Expr::IndexGet { object, index } => {
            walk(object, out);
            walk(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            walk(object, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::ArrayPush { array_id, value } => {
            out.insert(*array_id);
            walk(value, out);
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            out.insert(*id);
        }
        Expr::ArraySplice { array_id, start, delete_count, items } => {
            out.insert(*array_id);
            walk(start, out);
            if let Some(d) = delete_count {
                walk(d, out);
            }
            for it in items {
                walk(it, out);
            }
        }
        Expr::Array(elements) => {
            for el in elements {
                walk(el, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e, out),
                }
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySort { array, comparator: callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            walk(array, out);
            walk(callback, out);
        }
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            walk(array, out);
            walk(callback, out);
            if let Some(init) = initial {
                walk(init, out);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            walk(array, out);
            if let Some(sep) = separator {
                walk(sep, out);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            walk(array, out);
            walk(start, out);
            if let Some(e) = end {
                walk(e, out);
            }
        }
        Expr::ArrayIncludes { array, value } => {
            walk(array, out);
            walk(value, out);
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                walk(e, out);
            }
        }
        Expr::ObjectRest { object, .. } => walk(object, out),
        Expr::ObjectIs(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::ObjectHasOwn(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::New { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::MapNew | Expr::SetNew => {}
        Expr::SetNewFromArray(arr) => walk(arr, out),
        Expr::MapSet { map, key, value } => {
            walk(map, out);
            walk(key, out);
            walk(value, out);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            walk(map, out);
            walk(key, out);
        }
        Expr::MapClear(map) => walk(map, out),
        Expr::SetAdd { set_id, value } => {
            out.insert(*set_id);
            walk(value, out);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            walk(set, out);
            walk(value, out);
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                walk(v, out);
            }
        }
        Expr::MathPow(a, b) | Expr::PathJoin(a, b) | Expr::PathRelative(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::PathBasenameExt(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            walk(value, out);
            walk(replacer, out);
            walk(indent, out);
        }
        Expr::JsonParseReviver { text, reviver } => {
            walk(text, out);
            walk(reviver, out);
        }
        Expr::JsonParseWithReviver(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::Closure { body, captures, .. } => {
            // Closure literals don't introduce captures into the outer
            // scope, but their explicit captures + body references may
            // mention outer locals that need to be globalized.
            for c in captures {
                out.insert(*c);
            }
            collect_ref_ids_in_stmts(body, out);
        }
        Expr::ParseInt { string, radix } => {
            walk(string, out);
            if let Some(r) = radix {
                walk(r, out);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk(e, out);
            }
        }
        Expr::InstanceOf { expr, .. } => walk(expr, out),
        Expr::In { property, object } => {
            walk(property, out);
            walk(object, out);
        }
        Expr::SuperCall(args)
        | Expr::SuperMethodCall { args, .. }
        | Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::FsWriteFileSync(p, c) => {
            walk(p, out);
            walk(c, out);
        }
        Expr::ErrorNewWithCause { message, cause } => {
            walk(message, out);
            walk(cause, out);
        }
        Expr::DateNew(Some(arg)) => walk(arg, out),
        Expr::Uint8ArrayGet { array, index } => {
            walk(array, out);
            walk(index, out);
        }
        Expr::Uint8ArraySet { array, index, value } => {
            walk(array, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg { walk(a, out); }
        }
        Expr::ObjectGroupBy { items, key_fn } => {
            walk(items, out);
            walk(key_fn, out);
        }
        Expr::ArrayFromMapped { iterable, map_fn } => {
            walk(iterable, out);
            walk(map_fn, out);
        }
        Expr::RegExpTest { regex, string }
        | Expr::RegExpExec { regex, string } => {
            walk(regex, out);
            walk(string, out);
        }
        Expr::StringMatch { string, regex } => {
            walk(string, out);
            walk(regex, out);
        }
        Expr::BufferFrom { data, encoding } => {
            walk(data, out);
            if let Some(e) = encoding {
                walk(e, out);
            }
        }
        Expr::BufferAlloc { size, fill } => {
            walk(size, out);
            if let Some(f) = fill {
                walk(f, out);
            }
        }
        Expr::FinalizationRegistryRegister { registry, target, held, token } => {
            walk(registry, out);
            walk(target, out);
            walk(held, out);
            if let Some(t) = token {
                walk(t, out);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            walk(registry, out);
            walk(token, out);
        }
        Expr::StaticFieldSet { value, .. } => walk(value, out),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Integer-valued local detection
// ---------------------------------------------------------------------------

/// Collect LocalIds that are provably integer-valued for the lifetime of the
/// function. Used by `BinaryOp::Mod` lowering to emit integer modulo
/// (`fptosi → srem → sitofp`) instead of `frem double`, which lowers to a
/// libm `fmod()` call on ARM (no hardware instruction) and costs ~15ns per
/// iteration.
///
/// A local qualifies iff:
///   1. It's declared with `Let { init: Some(Expr::Integer(_)) }` — i.e. it
///      starts as a whole number, not a fraction.
///   2. It has NO `Expr::LocalSet(id, _)` anywhere in the function body.
///      The only permitted mutation is `Expr::Update { id, .. }` (++/--),
///      which by definition preserves the integer invariant.
///
/// Rule 2 is strict: any `LocalSet` (even one storing an integer literal)
/// excludes the local, because proving the rhs is also integer-valued would
/// require a recursive analysis we don't have. Rule 2 naturally covers the
/// common case — for-loop counters — without any type inference machinery.
///
/// Closure captures are handled correctly: writes from inside a closure body
/// go through `LocalSet` in the HIR, so rule 2 excludes any local that's
/// captured mutably. Read-only captures are fine and remain qualified.
pub(crate) fn collect_integer_locals(stmts: &[perry_hir::Stmt]) -> HashSet<u32> {
    let mut candidates: HashSet<u32> = HashSet::new();
    collect_integer_let_ids(stmts, &mut candidates);
    let mut ever_localset: HashSet<u32> = HashSet::new();
    collect_localset_ids_in_stmts(stmts, &mut ever_localset);
    candidates.retain(|id| !ever_localset.contains(id));
    candidates
}

fn collect_integer_let_ids(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let { id, init: Some(Expr::Integer(_)), .. } => {
                out.insert(*id);
            }
            Stmt::If { then_branch, else_branch, .. } => {
                collect_integer_let_ids(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_integer_let_ids(eb, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_integer_let_ids(std::slice::from_ref(init_stmt), out);
                }
                collect_integer_let_ids(body, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_integer_let_ids(body, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_integer_let_ids(body, out);
                if let Some(c) = catch {
                    collect_integer_let_ids(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_integer_let_ids(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_integer_let_ids(&c.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_integer_let_ids(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

/// Exhaustive walker mirroring `collect_ref_ids_in_expr` but only recording
/// targets of `LocalSet`. Update (++/--) and LocalGet are intentionally NOT
/// recorded — they preserve integer-ness. Keep this in sync with
/// `collect_ref_ids_in_expr`: any new HIR Expr variant must recurse into its
/// sub-expressions here, or the walker may miss a LocalSet hidden inside it
/// and wrongly mark its target as integer-valued.
fn collect_localset_ids_in_stmts(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => collect_localset_ids_in_expr(e, out),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_localset_ids_in_expr(e, out);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_localset_ids_in_expr(e, out);
                }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                collect_localset_ids_in_expr(condition, out);
                collect_localset_ids_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_localset_ids_in_stmts(eb, out);
                }
            }
            Stmt::While { condition, body } => {
                collect_localset_ids_in_expr(condition, out);
                collect_localset_ids_in_stmts(body, out);
            }
            Stmt::DoWhile { body, condition } => {
                collect_localset_ids_in_stmts(body, out);
                collect_localset_ids_in_expr(condition, out);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    collect_localset_ids_in_stmts(std::slice::from_ref(init_stmt), out);
                }
                if let Some(cond) = condition {
                    collect_localset_ids_in_expr(cond, out);
                }
                if let Some(upd) = update {
                    collect_localset_ids_in_expr(upd, out);
                }
                collect_localset_ids_in_stmts(body, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_localset_ids_in_stmts(body, out);
                if let Some(c) = catch {
                    collect_localset_ids_in_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_localset_ids_in_stmts(f, out);
                }
            }
            Stmt::Switch { discriminant, cases } => {
                collect_localset_ids_in_expr(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        collect_localset_ids_in_expr(t, out);
                    }
                    collect_localset_ids_in_stmts(&c.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_localset_ids_in_stmts(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

fn collect_localset_ids_in_expr(e: &perry_hir::Expr, out: &mut HashSet<u32>) {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let mut walk = |sub: &Expr, out: &mut HashSet<u32>| {
        collect_localset_ids_in_expr(sub, out);
    };
    match e {
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            walk(value, out);
        }
        // Intentionally NOT recorded — these preserve integer-ness.
        Expr::LocalGet(_) | Expr::Update { .. } => {}
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk(left, out);
            walk(right, out);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::IsFinite(operand)
        | Expr::IsNaN(operand)
        | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand)
        | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand)
        | Expr::ParseFloat(operand)
        | Expr::ObjectKeys(operand)
        | Expr::ObjectValues(operand)
        | Expr::ObjectEntries(operand)
        | Expr::ObjectFromEntries(operand)
        | Expr::ObjectIsFrozen(operand)
        | Expr::ObjectIsSealed(operand)
        | Expr::ObjectIsExtensible(operand)
        | Expr::ObjectCreate(operand)
        | Expr::SetSize(operand)
        | Expr::SetClear(operand)
        | Expr::ArrayFrom(operand)
        | Expr::Uint8ArrayFrom(operand)
        | Expr::IteratorToArray(operand)
        | Expr::WeakRefNew(operand)
        | Expr::WeakRefDeref(operand)
        | Expr::StructuredClone(operand)
        | Expr::QueueMicrotask(operand)
        | Expr::ProcessNextTick(operand)
        | Expr::FsExistsSync(operand)
        | Expr::FsReadFileSync(operand)
        | Expr::FsReadFileBinary(operand)
        | Expr::FsUnlinkSync(operand)
        | Expr::FsMkdirSync(operand)
        | Expr::PathDirname(operand)
        | Expr::PathBasename(operand)
        | Expr::PathExtname(operand)
        | Expr::PathResolve(operand)
        | Expr::PathNormalize(operand)
        | Expr::PathFormat(operand)
        | Expr::PathParse(operand)
        | Expr::DateToISOString(operand)
        | Expr::DateParse(operand)
        | Expr::EnvGetDynamic(operand)
        | Expr::ErrorNew(Some(operand))
        | Expr::FinalizationRegistryNew(operand)
        | Expr::Uint8ArrayNew(Some(operand))
        | Expr::Uint8ArrayLength(operand)
        | Expr::JsonParse(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathAbs(operand)
        | Expr::MathLog(operand)
        | Expr::MathLog2(operand)
        | Expr::MathLog10(operand)
        | Expr::MathLog1p(operand)
        | Expr::MathClz32(operand)
        | Expr::MathMinSpread(operand)
        | Expr::MathMaxSpread(operand) => {
            walk(operand, out);
        }
        Expr::Call { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => walk(e, out),
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk(o, out);
            }
            for a in args {
                walk(a, out);
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            walk(condition, out);
            walk(then_expr, out);
            walk(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => walk(object, out),
        Expr::PropertySet { object, value, .. } => {
            walk(object, out);
            walk(value, out);
        }
        Expr::PropertyUpdate { object, .. } => walk(object, out),
        Expr::IndexGet { object, index } => {
            walk(object, out);
            walk(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            walk(object, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::ArrayPush { value, .. } => walk(value, out),
        Expr::ArraySplice { start, delete_count, items, .. } => {
            walk(start, out);
            if let Some(d) = delete_count {
                walk(d, out);
            }
            for it in items {
                walk(it, out);
            }
        }
        Expr::Array(elements) => {
            for el in elements {
                walk(el, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e, out),
                }
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySort { array, comparator: callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            walk(array, out);
            walk(callback, out);
        }
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            walk(array, out);
            walk(callback, out);
            if let Some(init) = initial {
                walk(init, out);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            walk(array, out);
            if let Some(sep) = separator {
                walk(sep, out);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            walk(array, out);
            walk(start, out);
            if let Some(e) = end {
                walk(e, out);
            }
        }
        Expr::ArrayIncludes { array, value } => {
            walk(array, out);
            walk(value, out);
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                walk(e, out);
            }
        }
        Expr::ObjectRest { object, .. } => walk(object, out),
        Expr::ObjectIs(a, b) | Expr::ObjectHasOwn(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::New { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::MapNew | Expr::SetNew => {}
        Expr::SetNewFromArray(arr) => walk(arr, out),
        Expr::MapSet { map, key, value } => {
            walk(map, out);
            walk(key, out);
            walk(value, out);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            walk(map, out);
            walk(key, out);
        }
        Expr::MapClear(map) => walk(map, out),
        Expr::SetAdd { value, .. } => walk(value, out),
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            walk(set, out);
            walk(value, out);
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                walk(v, out);
            }
        }
        Expr::MathPow(a, b) | Expr::PathJoin(a, b) | Expr::PathRelative(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::PathBasenameExt(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            walk(value, out);
            walk(replacer, out);
            walk(indent, out);
        }
        Expr::JsonParseReviver { text, reviver } => {
            walk(text, out);
            walk(reviver, out);
        }
        Expr::JsonParseWithReviver(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::Closure { body, .. } => {
            collect_localset_ids_in_stmts(body, out);
        }
        Expr::ParseInt { string, radix } => {
            walk(string, out);
            if let Some(r) = radix {
                walk(r, out);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk(e, out);
            }
        }
        Expr::InstanceOf { expr, .. } => walk(expr, out),
        Expr::In { property, object } => {
            walk(property, out);
            walk(object, out);
        }
        Expr::SuperCall(args)
        | Expr::SuperMethodCall { args, .. }
        | Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::FsWriteFileSync(p, c) => {
            walk(p, out);
            walk(c, out);
        }
        Expr::ErrorNewWithCause { message, cause } => {
            walk(message, out);
            walk(cause, out);
        }
        Expr::DateNew(Some(arg)) => walk(arg, out),
        Expr::Uint8ArrayGet { array, index } => {
            walk(array, out);
            walk(index, out);
        }
        Expr::Uint8ArraySet { array, index, value } => {
            walk(array, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg { walk(a, out); }
        }
        Expr::ObjectGroupBy { items, key_fn } => {
            walk(items, out);
            walk(key_fn, out);
        }
        Expr::ArrayFromMapped { iterable, map_fn } => {
            walk(iterable, out);
            walk(map_fn, out);
        }
        Expr::RegExpTest { regex, string }
        | Expr::RegExpExec { regex, string } => {
            walk(regex, out);
            walk(string, out);
        }
        Expr::StringMatch { string, regex } => {
            walk(string, out);
            walk(regex, out);
        }
        Expr::BufferFrom { data, encoding } => {
            walk(data, out);
            if let Some(e) = encoding {
                walk(e, out);
            }
        }
        Expr::BufferAlloc { size, fill } => {
            walk(size, out);
            if let Some(f) = fill {
                walk(f, out);
            }
        }
        Expr::FinalizationRegistryRegister { registry, target, held, token } => {
            walk(registry, out);
            walk(target, out);
            walk(held, out);
            if let Some(t) = token {
                walk(t, out);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            walk(registry, out);
            walk(token, out);
        }
        Expr::StaticFieldSet { value, .. } => walk(value, out),
        _ => {}
    }
}


// -------- Integer specialization for pure numeric recursive functions --------

use perry_hir::{Expr, Stmt, Function, BinaryOp};

/// A function is i64-specializable if it's a pure numeric recursive fn.
pub fn is_integer_specializable(f: &Function) -> bool {
    if f.is_async || f.is_generator { return false; }
    if !matches!(f.return_type, perry_types::Type::Number) { return false; }
    if !f.params.iter().all(|p| matches!(p.ty, perry_types::Type::Number)) { return false; }
    i64s_stmts(&f.body, f.id)
}
fn i64s_stmts(ss: &[Stmt], sid: u32) -> bool {
    ss.iter().all(|s| match s {
        Stmt::Return(Some(e)) => i64s_expr(e, sid),
        Stmt::Return(None) => true,
        Stmt::If { condition, then_branch, else_branch } =>
            i64s_expr(condition, sid) && i64s_stmts(then_branch, sid)
            && else_branch.as_ref().map_or(true, |eb| i64s_stmts(eb, sid)),
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } => i64s_expr(e, sid),
        Stmt::Let { init: None, .. } => true,
        _ => false,
    })
}
fn i64s_expr(e: &Expr, sid: u32) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::LocalGet(_) => true,
        Expr::Binary { op, left, right } =>
            matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
            && i64s_expr(left, sid) && i64s_expr(right, sid),
        Expr::Compare { left, right, .. } =>
            i64s_expr(left, sid) && i64s_expr(right, sid),
        Expr::Call { callee, args, .. } =>
            matches!(callee.as_ref(), Expr::FuncRef(id) if *id == sid)
            && args.iter().all(|a| i64s_expr(a, sid)),
        Expr::Conditional { condition, then_expr, else_expr } =>
            i64s_expr(condition, sid) && i64s_expr(then_expr, sid) && i64s_expr(else_expr, sid),
        _ => false,
    }
}

/// Emit an i64-specialized function directly as LLVM IR text.
pub fn emit_i64_function(
    llmod: &mut crate::module::LlModule,
    f: &Function,
    i64_name: &str,
) {
    use crate::types::I64;
    let params: Vec<(crate::types::LlvmType, String)> = f
        .params.iter().map(|p| (I64, format!("%arg{}", p.id))).collect();
    let lf = llmod.define_function(i64_name, I64, params);
    let _ = lf.create_block("entry");
    let mut locals: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    {
        let blk = lf.block_mut(0).unwrap();
        for p in &f.params {
            let slot = blk.alloca(I64);
            blk.store(I64, &format!("%arg{}", p.id), &slot);
            locals.insert(p.id, slot);
        }
    }
    let mut cx = I64Cx { f: lf, cur: 0, locals, sn: i64_name.to_string(), sid: f.id };
    i64_body(&mut cx, &f.body);
    if !cx.f.block_mut(cx.cur).unwrap().is_terminated() {
        cx.f.block_mut(cx.cur).unwrap().ret(I64, "0");
    }
}
struct I64Cx<'a> { f: &'a mut crate::function::LlFunction, cur: usize, locals: std::collections::HashMap<u32, String>, sn: String, sid: u32 }

fn i64_body(cx: &mut I64Cx<'_>, ss: &[Stmt]) {
    use crate::types::I64;
    for s in ss {
        if cx.f.block_mut(cx.cur).unwrap().is_terminated() { break; }
        match s {
            Stmt::Return(Some(e)) => { let v = i64_val(cx, e); cx.f.block_mut(cx.cur).unwrap().ret(I64, &v); }
            Stmt::Return(None) => { cx.f.block_mut(cx.cur).unwrap().ret(I64, "0"); }
            Stmt::If { condition, then_branch, else_branch } => {
                let cond = i64_cond(cx, condition);
                let _ = cx.f.create_block("i64.then");
                let ti = cx.f.num_blocks() - 1;
                let tl = cx.f.blocks()[ti].label.clone();
                let ei = if else_branch.is_some() { let _ = cx.f.create_block("i64.else"); cx.f.num_blocks() - 1 } else { 0 };
                let el = if else_branch.is_some() { cx.f.blocks()[ei].label.clone() } else { String::new() };
                let _ = cx.f.create_block("i64.merge");
                let mi = cx.f.num_blocks() - 1;
                let ml = cx.f.blocks()[mi].label.clone();
                let target_else = if else_branch.is_some() { &el } else { &ml };
                cx.f.block_mut(cx.cur).unwrap().cond_br(&cond, &tl, target_else);
                cx.cur = ti;
                i64_body(cx, then_branch);
                if !cx.f.block_mut(cx.cur).unwrap().is_terminated() { cx.f.block_mut(cx.cur).unwrap().br(&ml); }
                if let Some(eb) = else_branch { cx.cur = ei; i64_body(cx, eb); if !cx.f.block_mut(cx.cur).unwrap().is_terminated() { cx.f.block_mut(cx.cur).unwrap().br(&ml); } }
                cx.cur = mi;
            }
            _ => {}
        }
    }
}
fn i64_cond(cx: &mut I64Cx<'_>, e: &Expr) -> String {
    use crate::types::I64;
    if let Expr::Compare { op, left, right } = e {
        let l = i64_val(cx, left); let r = i64_val(cx, right);
        let blk = cx.f.block_mut(cx.cur).unwrap();
        return match op {
            perry_hir::CompareOp::Le => blk.icmp_sle(I64, &l, &r),
            perry_hir::CompareOp::Lt => blk.icmp_slt(I64, &l, &r),
            perry_hir::CompareOp::Gt => blk.icmp_sgt(I64, &l, &r),
            perry_hir::CompareOp::Ge => blk.icmp_sge(I64, &l, &r),
            perry_hir::CompareOp::Eq | perry_hir::CompareOp::LooseEq => blk.icmp_eq(I64, &l, &r),
            perry_hir::CompareOp::Ne | perry_hir::CompareOp::LooseNe => blk.icmp_ne(I64, &l, &r),
        };
    }
    let v = i64_val(cx, e); cx.f.block_mut(cx.cur).unwrap().icmp_ne(I64, &v, "0")
}
fn i64_val(cx: &mut I64Cx<'_>, e: &Expr) -> String {
    use crate::types::I64;
    match e {
        Expr::Integer(n) => n.to_string(),
        Expr::Number(n) => (*n as i64).to_string(),
        Expr::LocalGet(id) => {
            if let Some(slot) = cx.locals.get(id).cloned() { cx.f.block_mut(cx.cur).unwrap().load(I64, &slot) }
            else { "0".to_string() }
        }
        Expr::Binary { op, left, right } => {
            let l = i64_val(cx, left); let r = i64_val(cx, right);
            let blk = cx.f.block_mut(cx.cur).unwrap();
            match op { BinaryOp::Add => blk.add(I64, &l, &r), BinaryOp::Sub => blk.sub(I64, &l, &r), BinaryOp::Mul => blk.mul(I64, &l, &r), _ => "0".to_string() }
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::FuncRef(id) = callee.as_ref() {
                if *id == cx.sid {
                    let mut lo: Vec<(crate::types::LlvmType, String)> = Vec::new();
                    for a in args { let v = i64_val(cx, a); lo.push((I64, v)); }
                    let refs: Vec<(crate::types::LlvmType, &str)> = lo.iter().map(|(t, v)| (*t, v.as_str())).collect();
                    let nm = cx.sn.clone();
                    return cx.f.block_mut(cx.cur).unwrap().call(I64, &nm, &refs);
                }
            }
            "0".to_string()
        }
        _ => "0".to_string(),
    }
}
