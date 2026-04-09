//! Type analysis helpers for expression codegen.
//!
//! Pure predicates and type refinement that don't emit IR themselves.
//! Used by `expr.rs`, `lower_call.rs`, `lower_string_method.rs`,
//! `lower_conditional.rs`, and `stmt.rs`.

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

use crate::expr::FnCtx;

/// Refine an `Any`-typed local's static type based on its initializer
/// expression. Returns Some(Type) when we can statically prove the
/// initializer produces a more specific type, so the `Stmt::Let`
/// lowerer can store the more specific type into `local_types` and
/// downstream code (`is_array_expr`, `is_string_expr`) can dispatch
/// to fast paths.
///
/// Recognizes:
/// - Array literals / spread / slice / map / filter / Object.keys → Array
/// - String literals / coerce / join → String
/// - **IndexGet on a known Array<T>** → element type T (so destructuring
///   nested arrays gets the right type for `__item_63 = arr[i]` patterns)
/// - **PropertyGet on a known class field** → the field's declared type
pub(crate) fn refine_type_from_init(ctx: &FnCtx<'_>, init: &Expr) -> Option<HirType> {
    match init {
        Expr::Array(_) | Expr::ArraySpread(_) => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        Expr::ArraySlice { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ObjectKeys(_)
        | Expr::ObjectValues(_)
        | Expr::ObjectEntries(_)
        | Expr::ArrayEntries { .. }
        | Expr::ArrayKeys { .. }
        | Expr::ArrayValues { .. }
        | Expr::StringMatch { .. }
        | Expr::StringMatchAll { .. } => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::String(_) | Expr::ArrayJoin { .. } | Expr::StringCoerce(_) => {
            Some(HirType::String)
        }
        // `let l = new ClassName<...>()` — refine to Named(ClassName)
        // so subsequent `l.method()` dispatch goes through the class
        // method registry instead of the universal fallback. This is
        // the difference between `l.size()` returning the real size
        // and returning undefined for generic class instances.
        Expr::New { class_name, .. } => Some(HirType::Named(class_name.clone())),
        // Compare results are now NaN-boxed booleans (TAG_TRUE/FALSE).
        // Type-refining the local as Boolean lets is_numeric_expr
        // skip the fast path (which would emit fcmp/sitofp on a NaN
        // bit pattern, giving wrong results) and routes printing
        // through js_console_log_dynamic which dispatches on the
        // NaN tag to print "true"/"false" instead of "1"/"0".
        Expr::Compare { .. } | Expr::Bool(_) | Expr::Logical { .. } => {
            Some(HirType::Boolean)
        }
        Expr::IndexGet { object, .. } => {
            // arr[i] where arr is Array<T> → element type T.
            // Handles both LocalGet(arr) and PropertyGet(this, "field")
            // — the latter lets `this.parts[i]` get the right type
            // when `parts: string[]`.
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(HirType::Array(elem_ty)) = ctx.local_types.get(arr_id) {
                    return Some((**elem_ty).clone());
                }
            }
            if let Some(ty) = static_type_of(ctx, object) {
                if let HirType::Array(elem_ty) = ty {
                    return Some(*elem_ty);
                }
            }
            None
        }
        Expr::PropertyGet { object, property } => {
            // obj.field where obj is a known class instance → field's
            // declared type. Reuses the same walk static_type_of uses.
            let receiver_class = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&receiver_class)?;
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| f.ty.clone())
        }
        _ => None,
    }
}

/// Compute the effective list of capture LocalIds for a closure. Starts
/// with the HIR's `captures` list (which may be empty if the closure
/// conversion pass missed it), then walks the body to find any LocalGet/
/// LocalSet/Update on ids that aren't params, inner-lets, or module
/// globals — those are the auto-detected captures.
///
/// Both the closure creation site (`Expr::Closure` lowering in
/// `lower_expr`) and the closure body site (`compile_closure` in
/// `codegen.rs`) call this so they agree on the slot indices.
pub(crate) fn compute_auto_captures(
    ctx: &FnCtx<'_>,
    params: &[perry_hir::Param],
    body: &[perry_hir::Stmt],
    explicit: &[u32],
) -> Vec<u32> {
    let mut out: Vec<u32> = explicit.to_vec();
    let mut referenced: std::collections::HashSet<u32> = std::collections::HashSet::new();
    crate::collectors::collect_ref_ids_in_stmts(body, &mut referenced);
    let mut inner_lets: std::collections::HashSet<u32> = std::collections::HashSet::new();
    crate::collectors::collect_let_ids(body, &mut inner_lets);
    let param_ids: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
    let already: std::collections::HashSet<u32> = out.iter().copied().collect();
    // Sort for determinism (HashSet iteration order is unspecified).
    let mut sorted: Vec<u32> = referenced.into_iter().collect();
    sorted.sort();
    for id in sorted {
        if !param_ids.contains(&id)
            && !inner_lets.contains(&id)
            && !already.contains(&id)
            && !ctx.module_globals.contains_key(&id)
        {
            out.push(id);
        }
    }
    out
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
        // NOTE: Expr::Compare is NOT numeric — it produces a NaN-boxed
        // TAG_TRUE/TAG_FALSE which `fcmp one cond, 0.0` would handle
        // incorrectly (NaN compared with 0.0 is unordered → false).
        // Comparisons go through the slow path (js_is_truthy) which
        // dispatches on the NaN tag.
        Expr::Binary { op, .. } => !matches!(op, BinaryOp::Add), // Add may concat strings
        Expr::Update { .. } => true,
        Expr::DateNow => true,
        _ => false,
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
pub(crate) fn is_bool_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Bool(_) => true,
        Expr::Compare { .. } => true,
        Expr::Logical { left, right, .. } => {
            is_bool_expr(ctx, left) && is_bool_expr(ctx, right)
        }
        Expr::Unary { op: UnaryOp::Not, .. } => true,
        Expr::BooleanCoerce(_) => true,
        Expr::IsFinite(_) | Expr::IsNaN(_) | Expr::NumberIsNaN(_) | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_) | Expr::IsUndefinedOrBareNan(_) => true,
        Expr::SetHas { .. } | Expr::SetDelete { .. } | Expr::MapHas { .. }
        | Expr::MapDelete { .. } => true,
        Expr::ArrayIncludes { .. } => true,
        Expr::LocalGet(id) => matches!(ctx.local_types.get(id), Some(HirType::Boolean)),
        _ => false,
    }
}

pub(crate) fn is_set_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::SetNew | Expr::SetNewFromArray(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Generic { base, .. }) if base == "Set"
        ),
        _ => false,
    }
}

pub(crate) fn is_map_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::MapNew | Expr::MapNewFromArray(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Generic { base, .. }) if base == "Map"
        ),
        _ => false,
    }
}

pub(crate) fn is_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) => true,
        Expr::LocalGet(id) => {
            match ctx.local_types.get(id) {
                Some(HirType::String) => true,
                // Union(String, Null/Void) — nullable strings are still
                // strings at runtime when non-null. The ?. and != null
                // guard paths lower the non-null case through the string
                // method dispatch. Without this, `(s: string | null).
                // toUpperCase()` fell through to the generic path and
                // returned undefined.
                Some(HirType::Union(members)) => {
                    members.iter().any(|m| matches!(m, HirType::String))
                }
                _ => false,
            }
        }
        // arr[i] where arr is Array<string> → element is a string.
        // Lets `this.parts[i].length` use the string fast path inline
        // without needing an intermediate let binding.
        Expr::IndexGet { object, .. } => {
            matches!(static_type_of(ctx, object), Some(HirType::Array(elem)) if matches!(*elem, HirType::String))
        }
        // Enum string members lower to string literals at the use
        // site, so a comparison like `c === Color.Red` should fire
        // the string equality fast path.
        Expr::EnumMember { enum_name, member_name } => {
            matches!(
                ctx.enums.get(&(enum_name.clone(), member_name.clone())),
                Some(perry_hir::EnumValue::String(_))
            )
        }
        Expr::Binary { op: BinaryOp::Add, left, right } => {
            is_string_expr(ctx, left) || is_string_expr(ctx, right)
        }
        // String coerce, JSON.stringify, ArrayJoin, etc. all return
        // strings.
        Expr::StringCoerce(_)
        | Expr::TypeOf(_)
        | Expr::ArrayJoin { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::PathJoin(..)
        | Expr::PathDirname(_)
        | Expr::PathBasename(_)
        | Expr::PathExtname(_)
        | Expr::PathResolve(_)
        | Expr::PathNormalize(_) => true,
        // `obj.toString()` always returns a string. Same for the
        // string-returning method family (trim, trimStart, trimEnd,
        // toLowerCase, toUpperCase, slice, substring, charAt, repeat,
        // replace, replaceAll, split's first elem, etc. — limited to
        // unary methods on a string receiver). Recognize these so
        // chained calls like `s.trimStart().trimEnd()` detect the
        // inner result as a string.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object } if matches!(
                    property.as_str(),
                    "toString" | "toLowerCase" | "toUpperCase" | "trim"
                        | "trimStart" | "trimEnd" | "slice" | "substring"
                        | "substr" | "charAt" | "repeat" | "replace"
                        | "replaceAll" | "padStart" | "padEnd" | "concat"
                ) && (
                    is_string_expr(ctx, object)
                        || matches!(property.as_str(), "toString")
                )
            ) =>
        {
            true
        }
        // PropertyGet on a known class field with declared type String.
        Expr::PropertyGet { object, property } => {
            let Some(class_name) = receiver_class_name(ctx, object) else {
                return false;
            };
            let Some(class) = ctx.classes.get(&class_name) else {
                return false;
            };
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| matches!(f.ty, HirType::String))
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// If the expression is a known instance of a Named class type, return
/// the class name. Used by the class method dispatch in lower_call to
/// pick the right `perry_method_<class>_<name>` function.
pub(crate) fn receiver_class_name(ctx: &FnCtx<'_>, e: &Expr) -> Option<String> {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id)? {
            HirType::Named(name) => Some(name.clone()),
            // Generic instantiation `Box<number>` — strip the type
            // args and use the base class name. The codegen erases
            // type parameters anyway, so the dispatch is identical
            // to the non-generic Named form.
            HirType::Generic { base, .. } if ctx.classes.contains_key(base) => {
                Some(base.clone())
            }
            _ => None,
        },
        // `new ClassName(...)` — the receiver class is the constructed class.
        // Lets `(new Config()).toString()` find Config's user toString.
        Expr::New { class_name, .. } => Some(class_name.clone()),
        // `ClassName.staticMethod(...)` chains often return an instance
        // of `ClassName` (factory pattern: `Color.red()`). Without type
        // info on the static method's return, assume it's the same class
        // so chained `.toString()` finds the user's toString.
        Expr::StaticMethodCall { class_name, .. } => Some(class_name.clone()),
        // `this` inside a constructor or method body — the class name is
        // at the top of class_stack (for inlined constructors) or comes
        // from the enclosing method's owning class.
        Expr::This => ctx.class_stack.last().cloned(),
        // `arr[i]` where `arr: ClassFoo[]` — the element type is the
        // array's parameter. Lets `items[2].display()` resolve the
        // method dispatch.
        Expr::IndexGet { object, .. } => {
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(HirType::Array(elem)) = ctx.local_types.get(arr_id) {
                    if let HirType::Named(name) = elem.as_ref() {
                        return Some(name.clone());
                    }
                }
            }
            None
        }
        // `this.field` or `obj.field` where the field's declared type
        // is a class. Walk the class definition to find the field's
        // type. Honors the parent inheritance chain.
        Expr::PropertyGet { object, property } => {
            let owner_class_name = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&owner_class_name)?;
            // Look in own fields, then walk parent chain.
            let field_ty = class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| &f.ty)
                .or_else(|| {
                    let mut parent = class.extends_name.as_deref();
                    while let Some(p) = parent {
                        if let Some(pc) = ctx.classes.get(p) {
                            if let Some(f) = pc.fields.iter().find(|f| f.name == *property) {
                                return Some(&f.ty);
                            }
                            parent = pc.extends_name.as_deref();
                        } else {
                            break;
                        }
                    }
                    None
                })?;
            match field_ty {
                HirType::Named(name) => Some(name.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Statically determine whether an expression is an array. Used for
/// dispatch on `arr.length` and `arr[i]`.
///
/// Recognizes:
/// - literal arrays `[a, b, c]` and `Expr::ArraySpread`
/// - LocalGet of an Array-typed local
/// - **PropertyGet on a class instance where the field is Array-typed**
///   (e.g. `this.items` when `Container.items: Item[]`)
/// - **NativeMethodCall results where the runtime returns an array**
///   (e.g. `arr.map(...)` — but those use the special Expr::ArrayMap
///   variant which is already handled)
pub(crate) fn is_array_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    matches!(static_type_of(ctx, e), Some(HirType::Array(_)))
}

/// Best-effort static type lookup for an expression. Returns the HIR
/// type when it's cheap to determine (literals, locals, field accesses
/// on known classes). Returns `None` when computing the type would
/// require a fuller type-checker pass.
pub(crate) fn static_type_of(ctx: &FnCtx<'_>, e: &Expr) -> Option<HirType> {
    match e {
        Expr::Array(_) => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::String(_) => Some(HirType::String),
        Expr::Number(_) | Expr::Integer(_) => Some(HirType::Number),
        Expr::Bool(_) => Some(HirType::Boolean),
        Expr::LocalGet(id) => ctx.local_types.get(id).cloned(),
        Expr::PropertyGet { object, property } => {
            // If the object is a known class instance, look up the field
            // type from the class definition.
            let receiver_class = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&receiver_class)?;
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| f.ty.clone())
                .or_else(|| {
                    // Walk up the inheritance chain.
                    let mut parent = class.extends_name.as_deref();
                    while let Some(p) = parent {
                        if let Some(pc) = ctx.classes.get(p) {
                            if let Some(field) = pc.fields.iter().find(|f| f.name == *property) {
                                return Some(field.ty.clone());
                            }
                            parent = pc.extends_name.as_deref();
                        } else {
                            break;
                        }
                    }
                    None
                })
        }
        Expr::This => {
            let cls = ctx.class_stack.last()?.clone();
            Some(HirType::Named(cls))
        }
        Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArraySpread(_)
        | Expr::ArraySlice { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayWith { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArrayFromMapped { .. }
        | Expr::ArrayFrom(_)
        | Expr::ArrayEntries(_)
        | Expr::ArrayKeys(_)
        | Expr::ArrayValues(_)
        | Expr::ObjectKeys(_)
        | Expr::ObjectValues(_)
        | Expr::ObjectEntries(_) => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        _ => None,
    }
}
