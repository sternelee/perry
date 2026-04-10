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
        | Expr::ArrayFilter { array, callback } => {
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
        _ => {}
    }
}

/// Walk a sequence of statements and collect every Call to an
/// `Expr::ExternFuncRef`. Used by `compile_module` to pre-declare
/// every imported function as an LLVM extern at the top of the IR.
///
/// The output is `(function_name, param_count)`. Param count comes from
/// the call's args.len() — using args.len() rather than the
/// `ExternFuncRef.param_types` is more permissive (the import metadata
/// can carry an outdated count after Perry's lowering).
pub(crate) fn collect_extern_func_refs_in_stmts(
    stmts: &[perry_hir::Stmt],
    seen: &mut HashSet<String>,
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
    seen: &mut HashSet<String>,
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
