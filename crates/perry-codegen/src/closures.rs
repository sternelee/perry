//! Closure compilation for the codegen module.
//!
//! Contains methods for collecting, declaring, and compiling closures,
//! as well as mutable capture tracking and function reference wrapper generation.

use anyhow::{anyhow, Result};
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, Init, Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{HashMap, HashSet};

use perry_hir::{
    UpdateOp,
    ArrayElement,
    CallArg, Expr, Function, Stmt,
};
use perry_types::LocalId;
use cranelift_codegen::ir::StackSlotData;

use crate::types::{ClassMeta, EnumMemberValue, LocalInfo, ThisContext, LoopContext};
use crate::util::*;
use crate::stmt::{compile_stmt, compile_async_stmt};
use crate::expr::compile_expr;

/// Collect all LocalIds referenced (read or written) in a list of statements.
/// Used to filter module-level variable loading to only variables the closure
/// actually references, avoiding unnecessary Cranelift instructions.
fn collect_referenced_locals_stmts(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
    for stmt in stmts {
        collect_referenced_locals_stmt(stmt, out);
    }
}

fn collect_referenced_locals_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                collect_referenced_locals_expr(expr, out);
            }
        }
        Stmt::Expr(expr) => collect_referenced_locals_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_referenced_locals_expr(expr, out),
        Stmt::Return(None) => {}
        Stmt::If { condition, then_branch, else_branch } => {
            collect_referenced_locals_expr(condition, out);
            collect_referenced_locals_stmts(then_branch, out);
            if let Some(els) = else_branch {
                collect_referenced_locals_stmts(els, out);
            }
        }
        Stmt::While { condition, body } => {
            collect_referenced_locals_expr(condition, out);
            collect_referenced_locals_stmts(body, out);
        }
        Stmt::DoWhile { body, condition } => {
            collect_referenced_locals_stmts(body, out);
            collect_referenced_locals_expr(condition, out);
        }
        Stmt::Labeled { body, .. } => {
            collect_referenced_locals_stmt(body, out);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init { collect_referenced_locals_stmt(i, out); }
            if let Some(c) = condition { collect_referenced_locals_expr(c, out); }
            if let Some(u) = update { collect_referenced_locals_expr(u, out); }
            collect_referenced_locals_stmts(body, out);
        }
        Stmt::Throw(expr) => collect_referenced_locals_expr(expr, out),
        Stmt::Try { body, catch, finally } => {
            collect_referenced_locals_stmts(body, out);
            if let Some(c) = catch {
                collect_referenced_locals_stmts(&c.body, out);
            }
            if let Some(f) = finally {
                collect_referenced_locals_stmts(f, out);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_referenced_locals_expr(discriminant, out);
            for case in cases {
                if let Some(test) = &case.test {
                    collect_referenced_locals_expr(test, out);
                }
                collect_referenced_locals_stmts(&case.body, out);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn collect_referenced_locals_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
            out.insert(*id);
            if let Expr::LocalSet(_, val) = expr {
                collect_referenced_locals_expr(val, out);
            }
        }
        Expr::Update { id, .. } => { out.insert(*id); }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_referenced_locals_expr(left, out);
            collect_referenced_locals_expr(right, out);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand)
        | Expr::Await(operand) => {
            collect_referenced_locals_expr(operand, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_referenced_locals_expr(callee, out);
            for a in args { collect_referenced_locals_expr(a, out); }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_referenced_locals_expr(callee, out);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => collect_referenced_locals_expr(e, out),
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object { collect_referenced_locals_expr(obj, out); }
            for a in args { collect_referenced_locals_expr(a, out); }
        }
        Expr::PropertyGet { object, .. } => collect_referenced_locals_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_referenced_locals_expr(object, out);
            collect_referenced_locals_expr(value, out);
        }
        Expr::PropertyUpdate { object, .. } => collect_referenced_locals_expr(object, out),
        Expr::IndexGet { object, index } => {
            collect_referenced_locals_expr(object, out);
            collect_referenced_locals_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_referenced_locals_expr(object, out);
            collect_referenced_locals_expr(index, out);
            collect_referenced_locals_expr(value, out);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_referenced_locals_expr(object, out);
            collect_referenced_locals_expr(index, out);
        }
        Expr::Object(fields) => {
            for (_, e) in fields { collect_referenced_locals_expr(e, out); }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts { collect_referenced_locals_expr(e, out); }
        }
        Expr::Array(elems) => {
            for e in elems { collect_referenced_locals_expr(e, out); }
        }
        Expr::ArraySpread(elems) => {
            for elem in elems {
                match elem {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => collect_referenced_locals_expr(e, out),
                }
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_referenced_locals_expr(condition, out);
            collect_referenced_locals_expr(then_expr, out);
            collect_referenced_locals_expr(else_expr, out);
        }
        Expr::InstanceOf { expr, .. } => collect_referenced_locals_expr(expr, out),
        Expr::In { property, object } => {
            collect_referenced_locals_expr(property, out);
            collect_referenced_locals_expr(object, out);
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } | Expr::SuperCall(args) => {
            // NewDynamic also has callee
            if let Expr::NewDynamic { callee, .. } = expr {
                collect_referenced_locals_expr(callee, out);
            }
            for a in args { collect_referenced_locals_expr(a, out); }
        }
        Expr::SuperMethodCall { args, .. } | Expr::StaticMethodCall { args, .. } => {
            for a in args { collect_referenced_locals_expr(a, out); }
        }
        Expr::StaticFieldSet { value, .. } => collect_referenced_locals_expr(value, out),
        Expr::Yield { value, .. } => {
            if let Some(v) = value { collect_referenced_locals_expr(v, out); }
        }
        Expr::EnvGetDynamic(e) => collect_referenced_locals_expr(e, out),
        // Leaf nodes with no LocalId references
        _ => {}
    }
}

impl crate::codegen::Compiler {
    pub(crate) fn collect_closures_from_stmts_into(&self, stmts: &[Stmt], closures: &mut Vec<(u32, Vec<perry_hir::Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>, bool, Option<String>, bool)>, enclosing_class: Option<&str>) {
        for stmt in stmts {
            self.collect_closures_from_stmt(stmt, closures, enclosing_class);
        }
    }

    pub(crate) fn collect_closures_from_stmt(&self, stmt: &Stmt, closures: &mut Vec<(u32, Vec<perry_hir::Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>, bool, Option<String>, bool)>, enclosing_class: Option<&str>) {
        match stmt {
            Stmt::Let { init: Some(expr), .. } => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Stmt::Expr(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Stmt::Return(Some(expr)) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.collect_closures_from_expr(condition, closures, enclosing_class);
                for s in then_branch {
                    self.collect_closures_from_stmt(s, closures, enclosing_class);
                }
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        self.collect_closures_from_stmt(s, closures, enclosing_class);
                    }
                }
            }
            Stmt::While { condition, body } => {
                self.collect_closures_from_expr(condition, closures, enclosing_class);
                for s in body {
                    self.collect_closures_from_stmt(s, closures, enclosing_class);
                }
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    self.collect_closures_from_stmt(init_stmt, closures, enclosing_class);
                }
                if let Some(cond) = condition {
                    self.collect_closures_from_expr(cond, closures, enclosing_class);
                }
                if let Some(upd) = update {
                    self.collect_closures_from_expr(upd, closures, enclosing_class);
                }
                for s in body {
                    self.collect_closures_from_stmt(s, closures, enclosing_class);
                }
            }
            Stmt::Throw(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Stmt::Try { body, catch, finally } => {
                for s in body {
                    self.collect_closures_from_stmt(s, closures, enclosing_class);
                }
                if let Some(catch_clause) = catch {
                    for s in &catch_clause.body {
                        self.collect_closures_from_stmt(s, closures, enclosing_class);
                    }
                }
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        self.collect_closures_from_stmt(s, closures, enclosing_class);
                    }
                }
            }
            Stmt::Switch { discriminant, cases } => {
                self.collect_closures_from_expr(discriminant, closures, enclosing_class);
                for case in cases {
                    // Collect from case test expression (closures may appear in case tests)
                    if let Some(test) = &case.test {
                        self.collect_closures_from_expr(test, closures, enclosing_class);
                    }
                    for s in &case.body {
                        self.collect_closures_from_stmt(s, closures, enclosing_class);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn collect_closures_from_expr(&self, expr: &Expr, closures: &mut Vec<(u32, Vec<perry_hir::Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>, bool, Option<String>, bool)>, enclosing_class: Option<&str>) {
        match expr {
            Expr::Closure { func_id, params, return_type: _, body, captures, mutable_captures, captures_this, enclosing_class: closure_class, is_async } => {
                // Use the enclosing_class stored in the Closure itself (set during lowering)
                // This ensures the class context is preserved even after transformations
                closures.push((*func_id, params.clone(), body.clone(), captures.clone(), mutable_captures.clone(), *captures_this, closure_class.clone(), *is_async));
                // Also collect nested closures (they inherit the enclosing class from the outer closure)
                let nested_class = closure_class.as_deref().or(enclosing_class);
                for stmt in body {
                    self.collect_closures_from_stmt(stmt, closures, nested_class);
                }
            }
            Expr::Binary { left, right, .. } => {
                self.collect_closures_from_expr(left, closures, enclosing_class);
                self.collect_closures_from_expr(right, closures, enclosing_class);
            }
            Expr::Unary { operand, .. } => {
                self.collect_closures_from_expr(operand, closures, enclosing_class);
            }
            Expr::Call { callee, args, .. } => {
                self.collect_closures_from_expr(callee, closures, enclosing_class);
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::Array(elements) => {
                for elem in elements {
                    self.collect_closures_from_expr(elem, closures, enclosing_class);
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) => self.collect_closures_from_expr(e, closures, enclosing_class),
                        ArrayElement::Spread(e) => self.collect_closures_from_expr(e, closures, enclosing_class),
                    }
                }
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.collect_closures_from_expr(condition, closures, enclosing_class);
                self.collect_closures_from_expr(then_expr, closures, enclosing_class);
                self.collect_closures_from_expr(else_expr, closures, enclosing_class);
            }
            Expr::ArrayForEach { array, callback } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
            }
            Expr::ArrayMap { array, callback } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
            }
            Expr::ArrayFilter { array, callback } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
            }
            Expr::ArrayFind { array, callback } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
            }
            Expr::ArrayFindIndex { array, callback } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
            }
            Expr::ArraySome { array, callback } | Expr::ArrayEvery { array, callback } | Expr::ArrayFlatMap { array, callback } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
            }
            Expr::ArraySort { array, comparator } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(comparator, closures, enclosing_class);
            }
            Expr::ArrayReduce { array, callback, initial } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(callback, closures, enclosing_class);
                if let Some(init) = initial {
                    self.collect_closures_from_expr(init, closures, enclosing_class);
                }
            }
            Expr::ArrayJoin { array, separator } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                if let Some(sep) = separator {
                    self.collect_closures_from_expr(sep, closures, enclosing_class);
                }
            }
            Expr::ArrayFlat { array } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
            }
            Expr::NativeMethodCall { object, args, .. } => {
                // Collect closures from object (if present)
                if let Some(obj) = object {
                    self.collect_closures_from_expr(obj, closures, enclosing_class);
                }
                // Collect closures from arguments (important for callbacks)
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            // JS interop expressions that may contain closures
            Expr::JsCallFunction { module_handle, args, .. } => {
                self.collect_closures_from_expr(module_handle, closures, enclosing_class);
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::JsCallMethod { object, args, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::JsGetExport { module_handle, .. } => {
                self.collect_closures_from_expr(module_handle, closures, enclosing_class);
            }
            Expr::JsGetProperty { object, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
            }
            Expr::JsSetProperty { object, value, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::JsNew { module_handle, args, .. } => {
                self.collect_closures_from_expr(module_handle, closures, enclosing_class);
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::JsNewFromHandle { constructor, args } => {
                self.collect_closures_from_expr(constructor, closures, enclosing_class);
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::JsCreateCallback { closure, .. } => {
                // This is the critical case - we need to collect the closure inside JsCreateCallback
                self.collect_closures_from_expr(closure, closures, enclosing_class);
            }
            Expr::PropertyGet { object, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
            }
            Expr::PropertySet { object, value, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::IndexGet { object, index } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
                self.collect_closures_from_expr(index, closures, enclosing_class);
            }
            Expr::IndexSet { object, index, value } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
                self.collect_closures_from_expr(index, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::Await(inner) => {
                self.collect_closures_from_expr(inner, closures, enclosing_class);
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.collect_closures_from_expr(condition, closures, enclosing_class);
                self.collect_closures_from_expr(then_expr, closures, enclosing_class);
                self.collect_closures_from_expr(else_expr, closures, enclosing_class);
            }
            Expr::Logical { left, right, .. } => {
                self.collect_closures_from_expr(left, closures, enclosing_class);
                self.collect_closures_from_expr(right, closures, enclosing_class);
            }
            Expr::Compare { left, right, .. } => {
                self.collect_closures_from_expr(left, closures, enclosing_class);
                self.collect_closures_from_expr(right, closures, enclosing_class);
            }
            Expr::New { args, .. } => {
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::NewDynamic { callee, args } => {
                self.collect_closures_from_expr(callee, closures, enclosing_class);
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::Object(fields) => {
                for (_, val) in fields {
                    self.collect_closures_from_expr(val, closures, enclosing_class);
                }
            }
            Expr::ObjectSpread { parts } => {
                for (_, val) in parts {
                    self.collect_closures_from_expr(val, closures, enclosing_class);
                }
            }
            Expr::LocalSet(_, expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Expr::GlobalSet(_, expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Expr::CallSpread { callee, args, .. } => {
                self.collect_closures_from_expr(callee, closures, enclosing_class);
                for arg in args {
                    match arg {
                        CallArg::Expr(e) => self.collect_closures_from_expr(e, closures, enclosing_class),
                        CallArg::Spread(e) => self.collect_closures_from_expr(e, closures, enclosing_class),
                    }
                }
            }
            Expr::TypeOf(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Expr::Void(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Expr::Yield { value, .. } => {
                if let Some(v) = value {
                    self.collect_closures_from_expr(v, closures, enclosing_class);
                }
            }
            Expr::InstanceOf { expr, .. } => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Expr::In { property, object } => {
                self.collect_closures_from_expr(property, closures, enclosing_class);
                self.collect_closures_from_expr(object, closures, enclosing_class);
            }
            Expr::StaticFieldSet { value, .. } => {
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::StaticMethodCall { args, .. } => {
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::SuperCall(args) => {
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::SuperMethodCall { args, .. } => {
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::Sequence(exprs) => {
                for e in exprs {
                    self.collect_closures_from_expr(e, closures, enclosing_class);
                }
            }
            Expr::NetCreateServer { options, connection_listener } => {
                if let Some(opts) = options {
                    self.collect_closures_from_expr(opts, closures, enclosing_class);
                }
                if let Some(listener) = connection_listener {
                    self.collect_closures_from_expr(listener, closures, enclosing_class);
                }
            }
            Expr::NetCreateConnection { port, host, connect_listener } | Expr::NetConnect { port, host, connect_listener } => {
                self.collect_closures_from_expr(port, closures, enclosing_class);
                if let Some(h) = host {
                    self.collect_closures_from_expr(h, closures, enclosing_class);
                }
                if let Some(listener) = connect_listener {
                    self.collect_closures_from_expr(listener, closures, enclosing_class);
                }
            }
            Expr::ChildProcessExec { command, options, callback } => {
                self.collect_closures_from_expr(command, closures, enclosing_class);
                if let Some(opts) = options {
                    self.collect_closures_from_expr(opts, closures, enclosing_class);
                }
                if let Some(cb) = callback {
                    self.collect_closures_from_expr(cb, closures, enclosing_class);
                }
            }
            Expr::Delete(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            Expr::PropertyUpdate { object, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
            }
            Expr::IndexUpdate { object, index, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
                self.collect_closures_from_expr(index, closures, enclosing_class);
            }
            // Additional expressions that may contain closures
            Expr::FetchWithOptions { url, method, body, headers } => {
                self.collect_closures_from_expr(url, closures, enclosing_class);
                self.collect_closures_from_expr(method, closures, enclosing_class);
                self.collect_closures_from_expr(body, closures, enclosing_class);
                for (_, header_val) in headers {
                    self.collect_closures_from_expr(header_val, closures, enclosing_class);
                }
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.collect_closures_from_expr(url, closures, enclosing_class);
                self.collect_closures_from_expr(auth_header, closures, enclosing_class);
            }
            Expr::FetchPostWithAuth { url, auth_header, body } => {
                self.collect_closures_from_expr(url, closures, enclosing_class);
                self.collect_closures_from_expr(auth_header, closures, enclosing_class);
                self.collect_closures_from_expr(body, closures, enclosing_class);
            }
            Expr::MathMin(args) | Expr::MathMax(args) => {
                for arg in args {
                    self.collect_closures_from_expr(arg, closures, enclosing_class);
                }
            }
            Expr::MathMinSpread(e) | Expr::MathMaxSpread(e) => {
                self.collect_closures_from_expr(e, closures, enclosing_class);
            }
            Expr::MathPow(base, exp) | Expr::MathImul(base, exp) | Expr::MathAtan2(base, exp) => {
                self.collect_closures_from_expr(base, closures, enclosing_class);
                self.collect_closures_from_expr(exp, closures, enclosing_class);
            }
            Expr::ArraySplice { start, delete_count, items, .. } => {
                self.collect_closures_from_expr(start, closures, enclosing_class);
                if let Some(dc) = delete_count {
                    self.collect_closures_from_expr(dc, closures, enclosing_class);
                }
                for item in items {
                    self.collect_closures_from_expr(item, closures, enclosing_class);
                }
            }
            Expr::ArraySlice { array, start, end } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(start, closures, enclosing_class);
                if let Some(e) = end {
                    self.collect_closures_from_expr(e, closures, enclosing_class);
                }
            }
            Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } | Expr::ArrayPushSpread { source: value, .. } => {
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::MapSet { map, key, value } => {
                self.collect_closures_from_expr(map, closures, enclosing_class);
                self.collect_closures_from_expr(key, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
                self.collect_closures_from_expr(map, closures, enclosing_class);
                self.collect_closures_from_expr(key, closures, enclosing_class);
            }
            Expr::SetAdd { value, .. } => {
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
                self.collect_closures_from_expr(set, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::StringSplit(string, delimiter) => {
                self.collect_closures_from_expr(string, closures, enclosing_class);
                self.collect_closures_from_expr(delimiter, closures, enclosing_class);
            }
            Expr::StringFromCharCode(code) => {
                self.collect_closures_from_expr(code, closures, enclosing_class);
            }
            Expr::RegExpTest { regex, string } => {
                self.collect_closures_from_expr(regex, closures, enclosing_class);
                self.collect_closures_from_expr(string, closures, enclosing_class);
            }
            Expr::ParseInt { string, radix } => {
                self.collect_closures_from_expr(string, closures, enclosing_class);
                if let Some(r) = radix {
                    self.collect_closures_from_expr(r, closures, enclosing_class);
                }
            }
            Expr::BufferFrom { data, encoding } => {
                self.collect_closures_from_expr(data, closures, enclosing_class);
                if let Some(enc) = encoding {
                    self.collect_closures_from_expr(enc, closures, enclosing_class);
                }
            }
            Expr::BufferAlloc { size, fill } => {
                self.collect_closures_from_expr(size, closures, enclosing_class);
                if let Some(f) = fill {
                    self.collect_closures_from_expr(f, closures, enclosing_class);
                }
            }
            Expr::ChildProcessExecSync { command, options } => {
                self.collect_closures_from_expr(command, closures, enclosing_class);
                if let Some(opts) = options {
                    self.collect_closures_from_expr(opts, closures, enclosing_class);
                }
            }
            Expr::ChildProcessSpawnSync { command, args, options } => {
                self.collect_closures_from_expr(command, closures, enclosing_class);
                if let Some(a) = args {
                    self.collect_closures_from_expr(a, closures, enclosing_class);
                }
                if let Some(opts) = options {
                    self.collect_closures_from_expr(opts, closures, enclosing_class);
                }
            }
            Expr::ChildProcessSpawn { command, args, options } => {
                self.collect_closures_from_expr(command, closures, enclosing_class);
                if let Some(a) = args {
                    self.collect_closures_from_expr(a, closures, enclosing_class);
                }
                if let Some(opts) = options {
                    self.collect_closures_from_expr(opts, closures, enclosing_class);
                }
            }
            Expr::UrlNew { url, base } => {
                self.collect_closures_from_expr(url, closures, enclosing_class);
                if let Some(b) = base {
                    self.collect_closures_from_expr(b, closures, enclosing_class);
                }
            }
            Expr::UrlSearchParamsGet { params, name } | Expr::UrlSearchParamsHas { params, name } |
            Expr::UrlSearchParamsDelete { params, name } | Expr::UrlSearchParamsGetAll { params, name } => {
                self.collect_closures_from_expr(params, closures, enclosing_class);
                self.collect_closures_from_expr(name, closures, enclosing_class);
            }
            Expr::UrlSearchParamsSet { params, name, value } | Expr::UrlSearchParamsAppend { params, name, value } => {
                self.collect_closures_from_expr(params, closures, enclosing_class);
                self.collect_closures_from_expr(name, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            // File system operations
            Expr::FsReadFileSync(path) | Expr::FsExistsSync(path) | Expr::FsMkdirSync(path) | Expr::FsUnlinkSync(path)
            | Expr::FsReadFileBinary(path) | Expr::FsRmRecursive(path) => {
                self.collect_closures_from_expr(path, closures, enclosing_class);
            }
            // Dynamic environment variable access
            Expr::EnvGetDynamic(key_expr) => {
                self.collect_closures_from_expr(key_expr, closures, enclosing_class);
            }
            Expr::FsWriteFileSync(path, content) | Expr::FsAppendFileSync(path, content) => {
                self.collect_closures_from_expr(path, closures, enclosing_class);
                self.collect_closures_from_expr(content, closures, enclosing_class);
            }
            Expr::ChildProcessSpawnBackground { command, args, log_file, env_json } => {
                self.collect_closures_from_expr(command, closures, enclosing_class);
                if let Some(a) = args { self.collect_closures_from_expr(a, closures, enclosing_class); }
                self.collect_closures_from_expr(log_file, closures, enclosing_class);
                if let Some(e) = env_json { self.collect_closures_from_expr(e, closures, enclosing_class); }
            }
            Expr::ChildProcessGetProcessStatus(h) | Expr::ChildProcessKillProcess(h) => {
                self.collect_closures_from_expr(h, closures, enclosing_class);
            }
            // Path operations
            Expr::PathJoin(a, b) => {
                self.collect_closures_from_expr(a, closures, enclosing_class);
                self.collect_closures_from_expr(b, closures, enclosing_class);
            }
            Expr::PathDirname(path) | Expr::PathBasename(path) | Expr::PathExtname(path) | Expr::PathResolve(path) | Expr::PathIsAbsolute(path) | Expr::FileURLToPath(path) => {
                self.collect_closures_from_expr(path, closures, enclosing_class);
            }
            // JSON operations
            Expr::JsonParse(expr) | Expr::JsonStringify(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            // Math operations
            Expr::MathFloor(expr) | Expr::MathCeil(expr) | Expr::MathRound(expr) |
            Expr::MathAbs(expr) | Expr::MathSqrt(expr) |
            Expr::MathLog(expr) | Expr::MathLog2(expr) | Expr::MathLog10(expr) |
            Expr::MathSin(expr) | Expr::MathCos(expr) | Expr::MathTan(expr) |
            Expr::MathAsin(expr) | Expr::MathAcos(expr) | Expr::MathAtan(expr) => {
                self.collect_closures_from_expr(expr, closures, enclosing_class);
            }
            // Crypto operations
            Expr::CryptoRandomBytes(inner) | Expr::CryptoSha256(inner) | Expr::CryptoMd5(inner) => {
                self.collect_closures_from_expr(inner, closures, enclosing_class);
            }
            // Buffer operations
            Expr::BufferAllocUnsafe(inner) | Expr::BufferConcat(inner) | Expr::BufferIsBuffer(inner) |
            Expr::BufferByteLength(inner) | Expr::BufferLength(inner) => {
                self.collect_closures_from_expr(inner, closures, enclosing_class);
            }
            Expr::BufferToString { buffer, encoding } => {
                self.collect_closures_from_expr(buffer, closures, enclosing_class);
                if let Some(enc) = encoding {
                    self.collect_closures_from_expr(enc, closures, enclosing_class);
                }
            }
            Expr::BufferFill { buffer, value } => {
                self.collect_closures_from_expr(buffer, closures, enclosing_class);
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            Expr::BufferSlice { buffer, start, end } => {
                self.collect_closures_from_expr(buffer, closures, enclosing_class);
                if let Some(s) = start {
                    self.collect_closures_from_expr(s, closures, enclosing_class);
                }
                if let Some(e) = end {
                    self.collect_closures_from_expr(e, closures, enclosing_class);
                }
            }
            Expr::BufferCopy { source, target, target_start, source_start, source_end } => {
                self.collect_closures_from_expr(source, closures, enclosing_class);
                self.collect_closures_from_expr(target, closures, enclosing_class);
                if let Some(ts) = target_start {
                    self.collect_closures_from_expr(ts, closures, enclosing_class);
                }
                if let Some(ss) = source_start {
                    self.collect_closures_from_expr(ss, closures, enclosing_class);
                }
                if let Some(se) = source_end {
                    self.collect_closures_from_expr(se, closures, enclosing_class);
                }
            }
            Expr::BufferWrite { buffer, string, offset, encoding } => {
                self.collect_closures_from_expr(buffer, closures, enclosing_class);
                self.collect_closures_from_expr(string, closures, enclosing_class);
                if let Some(o) = offset {
                    self.collect_closures_from_expr(o, closures, enclosing_class);
                }
                if let Some(e) = encoding {
                    self.collect_closures_from_expr(e, closures, enclosing_class);
                }
            }
            Expr::BufferEquals { buffer, other } => {
                self.collect_closures_from_expr(buffer, closures, enclosing_class);
                self.collect_closures_from_expr(other, closures, enclosing_class);
            }
            Expr::BufferIndexGet { buffer, index } | Expr::BufferIndexSet { buffer, index, .. } => {
                self.collect_closures_from_expr(buffer, closures, enclosing_class);
                self.collect_closures_from_expr(index, closures, enclosing_class);
            }
            // Uint8Array operations
            Expr::Uint8ArrayNew(Some(arg)) | Expr::Uint8ArrayFrom(arg) | Expr::Uint8ArrayLength(arg) => {
                self.collect_closures_from_expr(arg, closures, enclosing_class);
            }
            Expr::Uint8ArrayGet { array, index } | Expr::Uint8ArraySet { array, index, .. } => {
                self.collect_closures_from_expr(array, closures, enclosing_class);
                self.collect_closures_from_expr(index, closures, enclosing_class);
            }
            // Map/Set size, clear, and iterator operations
            Expr::MapSize(map) | Expr::MapClear(map) |
            Expr::MapEntries(map) | Expr::MapKeys(map) | Expr::MapValues(map) => {
                self.collect_closures_from_expr(map, closures, enclosing_class);
            }
            Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) |
            Expr::SetNewFromArray(set) => {
                self.collect_closures_from_expr(set, closures, enclosing_class);
            }
            // Date operations
            Expr::DateNew(Some(arg)) => {
                self.collect_closures_from_expr(arg, closures, enclosing_class);
            }
            Expr::DateGetTime(date) | Expr::DateToISOString(date) | Expr::DateGetFullYear(date) |
            Expr::DateGetMonth(date) | Expr::DateGetDate(date) | Expr::DateGetHours(date) |
            Expr::DateGetMinutes(date) | Expr::DateGetSeconds(date) | Expr::DateGetMilliseconds(date) => {
                self.collect_closures_from_expr(date, closures, enclosing_class);
            }
            // Error operations
            Expr::ErrorNew(Some(msg)) => {
                self.collect_closures_from_expr(msg, closures, enclosing_class);
            }
            Expr::ErrorMessage(err) => {
                self.collect_closures_from_expr(err, closures, enclosing_class);
            }
            // URL getter operations
            Expr::UrlGetHref(url) | Expr::UrlGetPathname(url) | Expr::UrlGetProtocol(url) |
            Expr::UrlGetHost(url) | Expr::UrlGetHostname(url) | Expr::UrlGetPort(url) |
            Expr::UrlGetSearch(url) | Expr::UrlGetHash(url) | Expr::UrlGetOrigin(url) |
            Expr::UrlGetSearchParams(url) => {
                self.collect_closures_from_expr(url, closures, enclosing_class);
            }
            Expr::UrlSearchParamsNew(Some(init)) => {
                self.collect_closures_from_expr(init, closures, enclosing_class);
            }
            Expr::UrlSearchParamsToString(params) => {
                self.collect_closures_from_expr(params, closures, enclosing_class);
            }
            // String operations
            Expr::StringMatch { string, regex } | Expr::StringMatchAll { string, regex } => {
                self.collect_closures_from_expr(string, closures, enclosing_class);
                self.collect_closures_from_expr(regex, closures, enclosing_class);
            }
            Expr::StringReplace { string, pattern, replacement } => {
                self.collect_closures_from_expr(string, closures, enclosing_class);
                self.collect_closures_from_expr(pattern, closures, enclosing_class);
                self.collect_closures_from_expr(replacement, closures, enclosing_class);
            }
            // Object operations
            Expr::ObjectKeys(obj) | Expr::ObjectValues(obj) | Expr::ObjectEntries(obj) => {
                self.collect_closures_from_expr(obj, closures, enclosing_class);
            }
            Expr::ObjectRest { object, .. } => {
                self.collect_closures_from_expr(object, closures, enclosing_class);
            }
            // Array static methods
            Expr::ArrayIsArray(value) | Expr::ArrayFrom(value) => {
                self.collect_closures_from_expr(value, closures, enclosing_class);
            }
            // Global functions
            Expr::ParseFloat(s) | Expr::NumberCoerce(s) | Expr::BigIntCoerce(s) | Expr::StringCoerce(s) |
            Expr::BooleanCoerce(s) | Expr::IsNaN(s) | Expr::IsUndefinedOrBareNan(s) | Expr::IsFinite(s) | Expr::NumberIsNaN(s) | Expr::NumberIsFinite(s) | Expr::NumberIsInteger(s) | Expr::NumberIsSafeInteger(s) | Expr::StaticPluginResolve(s) => {
                self.collect_closures_from_expr(s, closures, enclosing_class);
            }
            // Yield expression
            Expr::Yield { value, .. } => {
                if let Some(val) = value {
                    self.collect_closures_from_expr(val, closures, enclosing_class);
                }
            }
            // Expressions with no inner expressions to traverse
            Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) |
            Expr::BigInt(_) | Expr::String(_) | Expr::I18nString { .. } | Expr::LocalGet(_) | Expr::GlobalGet(_) |
            Expr::Update { .. } | Expr::FuncRef(_) | Expr::ExternFuncRef { .. } |
            Expr::NativeModuleRef(_) | Expr::StaticFieldGet { .. } | Expr::This |
            Expr::EnumMember { .. } | Expr::ClassRef(_) | Expr::EnvGet(_) |
            Expr::ProcessUptime | Expr::ProcessCwd | Expr::ProcessArgv | Expr::ProcessMemoryUsage |
            Expr::MathRandom | Expr::CryptoRandomUUID |
            Expr::OsPlatform | Expr::OsArch | Expr::OsHostname | Expr::OsHomedir |
            Expr::OsTmpdir | Expr::OsTotalmem | Expr::OsFreemem | Expr::OsUptime |
            Expr::OsType | Expr::OsRelease | Expr::OsCpus | Expr::OsNetworkInterfaces |
            Expr::OsUserInfo | Expr::OsEOL |
            Expr::MapNew | Expr::SetNew | Expr::DateNow |
            Expr::ArrayPop(_) | Expr::ArrayShift(_) |
            Expr::Uint8ArrayNew(None) | Expr::DateNew(None) | Expr::ErrorNew(None) |
            Expr::UrlSearchParamsNew(None) |
            Expr::RegExp { .. } | Expr::JsLoadModule { .. } | Expr::ImportMetaUrl(_) => {
                // No inner expressions to traverse
            }
        }
    }

    /// Collect all mutable captures from closures in the given statements.
    /// Returns a set of LocalIds that need to be boxed at declaration time.
    pub(crate) fn collect_mutable_captures_from_stmts(&self, stmts: &[Stmt]) -> std::collections::HashSet<LocalId> {
        let mut mutable_captures: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
        for stmt in stmts {
            self.collect_mutable_captures_from_stmt(stmt, &mut mutable_captures);
        }
        mutable_captures
    }

    pub(crate) fn collect_mutable_captures_from_stmt(&self, stmt: &Stmt, captures: &mut std::collections::HashSet<LocalId>) {
        match stmt {
            Stmt::Let { init: Some(expr), .. } => {
                self.collect_mutable_captures_from_expr(expr, captures);
            }
            Stmt::Expr(expr) => {
                self.collect_mutable_captures_from_expr(expr, captures);
            }
            Stmt::Return(Some(expr)) => {
                self.collect_mutable_captures_from_expr(expr, captures);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.collect_mutable_captures_from_expr(condition, captures);
                for s in then_branch {
                    self.collect_mutable_captures_from_stmt(s, captures);
                }
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        self.collect_mutable_captures_from_stmt(s, captures);
                    }
                }
            }
            Stmt::While { condition, body } => {
                self.collect_mutable_captures_from_expr(condition, captures);
                for s in body {
                    self.collect_mutable_captures_from_stmt(s, captures);
                }
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    self.collect_mutable_captures_from_stmt(init_stmt, captures);
                }
                if let Some(cond) = condition {
                    self.collect_mutable_captures_from_expr(cond, captures);
                }
                if let Some(upd) = update {
                    self.collect_mutable_captures_from_expr(upd, captures);
                }
                for s in body {
                    self.collect_mutable_captures_from_stmt(s, captures);
                }
            }
            Stmt::Try { body, catch, finally } => {
                for s in body {
                    self.collect_mutable_captures_from_stmt(s, captures);
                }
                if let Some(catch_clause) = catch {
                    for s in &catch_clause.body {
                        self.collect_mutable_captures_from_stmt(s, captures);
                    }
                }
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        self.collect_mutable_captures_from_stmt(s, captures);
                    }
                }
            }
            Stmt::Switch { discriminant, cases } => {
                self.collect_mutable_captures_from_expr(discriminant, captures);
                for case in cases {
                    for s in &case.body {
                        self.collect_mutable_captures_from_stmt(s, captures);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn collect_mutable_captures_from_expr(&self, expr: &Expr, captures: &mut std::collections::HashSet<LocalId>) {
        match expr {
            Expr::Closure { mutable_captures, body, .. } => {
                // Add all mutable captures to the set
                for id in mutable_captures {
                    captures.insert(*id);
                }
                // Also collect from nested closures in the body
                for stmt in body {
                    self.collect_mutable_captures_from_stmt(stmt, captures);
                }
            }
            Expr::Binary { left, right, .. } => {
                self.collect_mutable_captures_from_expr(left, captures);
                self.collect_mutable_captures_from_expr(right, captures);
            }
            Expr::Unary { operand, .. } => {
                self.collect_mutable_captures_from_expr(operand, captures);
            }
            Expr::Call { callee, args, .. } => {
                self.collect_mutable_captures_from_expr(callee, captures);
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            Expr::Array(elements) => {
                for elem in elements {
                    self.collect_mutable_captures_from_expr(elem, captures);
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) => self.collect_mutable_captures_from_expr(e, captures),
                        ArrayElement::Spread(e) => self.collect_mutable_captures_from_expr(e, captures),
                    }
                }
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.collect_mutable_captures_from_expr(condition, captures);
                self.collect_mutable_captures_from_expr(then_expr, captures);
                self.collect_mutable_captures_from_expr(else_expr, captures);
            }
            Expr::ArrayForEach { array, callback } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(callback, captures);
            }
            Expr::ArrayMap { array, callback } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(callback, captures);
            }
            Expr::ArrayFilter { array, callback } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(callback, captures);
            }
            Expr::ArrayFind { array, callback } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(callback, captures);
            }
            Expr::ArrayFindIndex { array, callback } | Expr::ArraySome { array, callback } | Expr::ArrayEvery { array, callback } | Expr::ArrayFlatMap { array, callback } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(callback, captures);
            }
            Expr::ArraySort { array, comparator } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(comparator, captures);
            }
            Expr::ArrayReduce { array, callback, initial } => {
                self.collect_mutable_captures_from_expr(array, captures);
                self.collect_mutable_captures_from_expr(callback, captures);
                if let Some(init) = initial {
                    self.collect_mutable_captures_from_expr(init, captures);
                }
            }
            Expr::ArrayJoin { array, separator } => {
                self.collect_mutable_captures_from_expr(array, captures);
                if let Some(sep) = separator {
                    self.collect_mutable_captures_from_expr(sep, captures);
                }
            }
            Expr::ArrayFlat { array } => {
                self.collect_mutable_captures_from_expr(array, captures);
            }
            Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
                self.collect_mutable_captures_from_expr(val, captures);
            }
            Expr::PropertyGet { object, .. } => {
                self.collect_mutable_captures_from_expr(object, captures);
            }
            Expr::PropertySet { object, value, .. } => {
                self.collect_mutable_captures_from_expr(object, captures);
                self.collect_mutable_captures_from_expr(value, captures);
            }
            Expr::IndexGet { object, index } => {
                self.collect_mutable_captures_from_expr(object, captures);
                self.collect_mutable_captures_from_expr(index, captures);
            }
            Expr::IndexSet { object, index, value } => {
                self.collect_mutable_captures_from_expr(object, captures);
                self.collect_mutable_captures_from_expr(index, captures);
                self.collect_mutable_captures_from_expr(value, captures);
            }
            Expr::Object(fields) => {
                for (_, value) in fields {
                    self.collect_mutable_captures_from_expr(value, captures);
                }
            }
            Expr::ObjectSpread { parts } => {
                for (_, value) in parts {
                    self.collect_mutable_captures_from_expr(value, captures);
                }
            }
            Expr::New { args, .. } => {
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            Expr::NativeMethodCall { object, args, .. } => {
                // Collect from object (if present)
                if let Some(obj) = object {
                    self.collect_mutable_captures_from_expr(obj, captures);
                }
                // Collect from arguments (important for callbacks)
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            // JS interop expressions that may contain closures
            Expr::JsCallFunction { module_handle, args, .. } => {
                self.collect_mutable_captures_from_expr(module_handle, captures);
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            Expr::JsCallMethod { object, args, .. } => {
                self.collect_mutable_captures_from_expr(object, captures);
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            Expr::JsGetExport { module_handle, .. } => {
                self.collect_mutable_captures_from_expr(module_handle, captures);
            }
            Expr::JsGetProperty { object, .. } => {
                self.collect_mutable_captures_from_expr(object, captures);
            }
            Expr::JsSetProperty { object, value, .. } => {
                self.collect_mutable_captures_from_expr(object, captures);
                self.collect_mutable_captures_from_expr(value, captures);
            }
            Expr::JsNew { module_handle, args, .. } => {
                self.collect_mutable_captures_from_expr(module_handle, captures);
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            Expr::JsNewFromHandle { constructor, args } => {
                self.collect_mutable_captures_from_expr(constructor, captures);
                for arg in args {
                    self.collect_mutable_captures_from_expr(arg, captures);
                }
            }
            Expr::JsCreateCallback { closure, .. } => {
                self.collect_mutable_captures_from_expr(closure, captures);
            }
            Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } => {
                self.collect_mutable_captures_from_expr(value, captures);
            }
            Expr::ArrayPushSpread { source, .. } => {
                self.collect_mutable_captures_from_expr(source, captures);
            }
            _ => {}
        }
    }

    /// Collect FuncRef expressions that are used as values (not as call callees)
    /// These need wrapper functions for closure-compatible calling convention
    pub(crate) fn collect_func_refs_needing_wrappers_from_stmts(&self, stmts: &[Stmt], func_refs: &mut std::collections::BTreeSet<u32>) {
        for stmt in stmts {
            self.collect_func_refs_from_stmt(stmt, func_refs);
        }
    }

    pub(crate) fn collect_func_refs_from_stmt(&self, stmt: &Stmt, func_refs: &mut std::collections::BTreeSet<u32>) {
        match stmt {
            Stmt::Let { init: Some(expr), .. } => {
                self.collect_func_refs_from_expr(expr, func_refs);
            }
            Stmt::Expr(expr) => {
                self.collect_func_refs_from_expr(expr, func_refs);
            }
            Stmt::Return(Some(expr)) => {
                self.collect_func_refs_from_expr(expr, func_refs);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.collect_func_refs_from_expr(condition, func_refs);
                self.collect_func_refs_needing_wrappers_from_stmts(then_branch, func_refs);
                if let Some(else_stmts) = else_branch {
                    self.collect_func_refs_needing_wrappers_from_stmts(else_stmts, func_refs);
                }
            }
            Stmt::While { condition, body } => {
                self.collect_func_refs_from_expr(condition, func_refs);
                self.collect_func_refs_needing_wrappers_from_stmts(body, func_refs);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    self.collect_func_refs_from_stmt(init_stmt, func_refs);
                }
                if let Some(cond) = condition {
                    self.collect_func_refs_from_expr(cond, func_refs);
                }
                if let Some(upd) = update {
                    self.collect_func_refs_from_expr(upd, func_refs);
                }
                self.collect_func_refs_needing_wrappers_from_stmts(body, func_refs);
            }
            _ => {}
        }
    }

    pub(crate) fn collect_func_refs_from_expr(&self, expr: &Expr, func_refs: &mut std::collections::BTreeSet<u32>) {
        match expr {
            Expr::Call { callee, args, .. } => {
                // Callee FuncRef is NOT a wrapper candidate (it's being called directly)
                // But we need to check if callee is a complex expression
                if !matches!(callee.as_ref(), Expr::FuncRef(_)) {
                    self.collect_func_refs_from_expr(callee, func_refs);
                }
                // Args that are FuncRefs ARE wrapper candidates
                for arg in args {
                    match arg {
                        Expr::FuncRef(func_id) => {
                            func_refs.insert(*func_id);
                        }
                        _ => self.collect_func_refs_from_expr(arg, func_refs),
                    }
                }
            }
            Expr::Binary { left, right, .. } => {
                self.collect_func_refs_from_expr(left, func_refs);
                self.collect_func_refs_from_expr(right, func_refs);
            }
            Expr::Unary { operand, .. } => {
                self.collect_func_refs_from_expr(operand, func_refs);
            }
            Expr::Array(elements) => {
                for elem in elements {
                    match elem {
                        Expr::FuncRef(func_id) => {
                            func_refs.insert(*func_id);
                        }
                        _ => self.collect_func_refs_from_expr(elem, func_refs),
                    }
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) => {
                            if let Expr::FuncRef(func_id) = e {
                                func_refs.insert(*func_id);
                            } else {
                                self.collect_func_refs_from_expr(e, func_refs);
                            }
                        }
                        ArrayElement::Spread(e) => self.collect_func_refs_from_expr(e, func_refs),
                    }
                }
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.collect_func_refs_from_expr(condition, func_refs);
                self.collect_func_refs_from_expr(then_expr, func_refs);
                self.collect_func_refs_from_expr(else_expr, func_refs);
            }
            Expr::Closure { func_id, body, .. } => {
                self.collect_func_refs_needing_wrappers_from_stmts(body, func_refs);
            }
            Expr::ArrayForEach { array, callback } | Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } | Expr::ArrayFind { array, callback } | Expr::ArrayFindIndex { array, callback } | Expr::ArraySome { array, callback } | Expr::ArrayEvery { array, callback } | Expr::ArrayFlatMap { array, callback } => {
                self.collect_func_refs_from_expr(array, func_refs);
                match callback.as_ref() {
                    Expr::FuncRef(func_id) => {
                        func_refs.insert(*func_id);
                    }
                    _ => self.collect_func_refs_from_expr(callback, func_refs),
                }
            }
            Expr::ArraySort { array, comparator } => {
                self.collect_func_refs_from_expr(array, func_refs);
                match comparator.as_ref() {
                    Expr::FuncRef(func_id) => {
                        func_refs.insert(*func_id);
                    }
                    _ => self.collect_func_refs_from_expr(comparator, func_refs),
                }
            }
            Expr::ArrayReduce { array, callback, initial } => {
                self.collect_func_refs_from_expr(array, func_refs);
                match callback.as_ref() {
                    Expr::FuncRef(func_id) => {
                        func_refs.insert(*func_id);
                    }
                    _ => self.collect_func_refs_from_expr(callback, func_refs),
                }
                if let Some(init) = initial {
                    self.collect_func_refs_from_expr(init, func_refs);
                }
            }
            _ => {}
        }
    }

    /// Declare a closure function
    pub(crate) fn declare_closure(&mut self, func_id: u32, param_count: usize, capture_count: usize, is_async: bool) -> Result<()> {
        let mut sig = self.module.make_signature();

        // First parameter is the closure pointer (for accessing captures)
        sig.params.push(AbiParam::new(types::I64));

        // Then the regular parameters (all f64 for now)
        for _ in 0..param_count {
            sig.params.push(AbiParam::new(types::F64));
        }

        // All closures return F64 for consistent ABI with js_closure_call* functions
        // For async closures, the Promise pointer (I64) is bitcast to F64
        sig.returns.push(AbiParam::new(types::F64));
        if is_async {
            self.async_func_ids.insert(func_id);
        }

        let func_name = format!("__closure_{}", func_id);
        let clif_func_id = self.module.declare_function(&func_name, Linkage::Local, &sig)?;
        self.closure_func_ids.insert(func_id, clif_func_id);

        Ok(())
    }

    /// Compile a closure function
    pub(crate) fn compile_closure(&mut self, func_id: u32, params: &[perry_hir::Param], body: &[Stmt], captures: &[LocalId], mutable_captures: &[LocalId], captures_this: bool, enclosing_class: Option<&str>, is_async: bool) -> Result<()> {
        let clif_func_id = *self.closure_func_ids.get(&func_id)
            .ok_or_else(|| anyhow!("Closure not declared: {}", func_id))?;

        // Set up the function signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();

        // First parameter is closure pointer
        self.ctx.func.signature.params.push(AbiParam::new(types::I64));

        for (i, param) in params.iter().enumerate() {
            self.ctx.func.signature.params.push(AbiParam::new(types::F64));
            // Track rest parameter index for closures (same as functions.rs does for standalone functions)
            if param.is_rest {
                self.func_rest_param_index.insert(func_id, i);
            }
        }
        // All closures return F64 for consistent ABI with js_closure_call* functions
        // Async closures bitcast Promise pointer to F64 before returning
        self.ctx.func.signature.returns.push(AbiParam::new(types::F64));

        // Build a set of mutable captures for quick lookup
        let mutable_set: std::collections::HashSet<LocalId> = mutable_captures.iter().copied().collect();

        // Collect mutable captures for the closure body (for nested closures) - before borrowing self.ctx
        let boxed_vars = self.collect_mutable_captures_from_stmts(body);

        // Get class metadata if this closure captures `this`
        let class_meta = if captures_this {
            if let Some(class_name) = enclosing_class {
                self.classes.get(class_name).cloned()
            } else {
                None
            }
        } else {
            None
        };

        // Use fresh FunctionBuilderContext to avoid variable ID conflicts
        let mut closure_func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut closure_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // First param is closure pointer
            let closure_ptr_var = Variable::new(0);
            builder.declare_var(closure_ptr_var, types::I64);
            let closure_ptr = builder.block_params(entry_block)[0];
            builder.def_var(closure_ptr_var, closure_ptr);

            // Create variables for regular parameters
            let mut locals: HashMap<LocalId, LocalInfo> = HashMap::new();
            let mut next_var = 1usize;

            for (i, param) in params.iter().enumerate() {
                let var = Variable::new(next_var);
                next_var += 1;
                // Check parameter type to set appropriate LocalInfo flags
                let is_closure = matches!(param.ty, perry_types::Type::Function(_));
                // Check if parameter is a string type (including string enums like ChainName)
                let is_string = matches!(param.ty, perry_types::Type::String) || {
                    if let perry_types::Type::Named(ref name) = param.ty {
                        // Check if this is a string enum by looking up any member
                        self.enums.iter().any(|((enum_name, _), val)| {
                            enum_name == name && matches!(val, crate::types::EnumMemberValue::String(_))
                        })
                    } else {
                        false
                    }
                };
                let is_array = matches!(param.ty, perry_types::Type::Array(_));
                let is_bigint = matches!(param.ty, perry_types::Type::BigInt);
                // Detect Map/Set parameter types for proper property dispatch (map.size, set.size, etc.)
                let is_map = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Map")
                    || matches!(&param.ty, perry_types::Type::Named(name) if name == "Map");
                let is_set = matches!(&param.ty, perry_types::Type::Generic { base, .. } if base == "Set")
                    || matches!(&param.ty, perry_types::Type::Named(name) if name == "Set");
                // Any/Unknown are union types - they could be numbers, strings, objects, etc.
                // Don't treat them as pointers since we can't extract pointer from plain numbers
                let is_union_type = matches!(param.ty, perry_types::Type::Any | perry_types::Type::Unknown);
                let is_pointer = is_closure || is_string || is_array || is_map || is_set ||
                    matches!(param.ty, perry_types::Type::Object(_) | perry_types::Type::Named(_));
                // Use i64 for known pointer types, f64 for numbers and union types
                let var_type = if is_pointer && !is_union_type { types::I64 } else { types::F64 };
                builder.declare_var(var, var_type);
                // Parameters come in as f64 (potentially NaN-boxed), extract raw pointer if needed
                let val = builder.block_params(entry_block)[i + 1]; // +1 to skip closure ptr
                let final_val = if is_pointer && !is_union_type {
                    // The f64 may be NaN-boxed (e.g., object from array access)
                    // Extract the raw pointer using js_nanbox_get_pointer
                    let get_ptr_func = self.extern_funcs.get("js_nanbox_get_pointer")
                        .expect("js_nanbox_get_pointer not declared");
                    let get_ptr_ref = self.module.declare_func_in_func(*get_ptr_func, builder.func);
                    let call = builder.ins().call(get_ptr_ref, &[val]);
                    builder.inst_results(call)[0]
                } else {
                    val
                };
                builder.def_var(var, final_val);
                locals.insert(param.id, LocalInfo {
                    var,
                    name: Some(param.name.clone()),
                    class_name: resolve_class_name_from_type(&param.ty, &self.classes),
                    type_args: Vec::new(),
                    is_pointer: is_pointer && !is_union_type,
                    is_array,
                    is_string,
                    is_bigint,
                    is_closure, closure_func_id: None,
                    is_boxed: false,
                    is_map, is_set, is_buffer: false, is_event_emitter: false, is_union: is_union_type,
                    is_mixed_array: false,
                    is_integer: false,
                    is_integer_array: false,
                    is_i32: false, is_boolean: false,
                    i32_shadow: None,
                    bounded_by_array: None,
                    bounded_by_constant: None,
                    scalar_fields: None,
                    squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None, object_field_indices: None,
                });
            }

            // Generate default parameter checks for closure parameters.
            // When closures are called with fewer arguments than declared parameters,
            // the missing params receive whatever value was in the register (undefined behavior).
            // For params with default values, check if the value is TAG_UNDEFINED and substitute.
            for (i, param) in params.iter().enumerate() {
                if let Some(default_expr) = &param.default {
                    if let Some(info) = locals.get(&param.id) {
                        let var = info.var;
                        // Only handle simple literal defaults that can be compiled inline
                        let default_val_opt: Option<Value> = match default_expr {
                            perry_hir::Expr::Number(n) => {
                                Some(builder.ins().f64const(*n))
                            }
                            perry_hir::Expr::Integer(n) => {
                                Some(builder.ins().f64const(*n as f64))
                            }
                            perry_hir::Expr::Bool(b) => {
                                Some(builder.ins().f64const(if *b { 1.0 } else { 0.0 }))
                            }
                            perry_hir::Expr::String(s) => {
                                // Create string default value
                                if let Some(func_id) = self.extern_funcs.get("js_string_from_bytes") {
                                    let func_ref = self.module.declare_func_in_func(*func_id, builder.func);
                                    let bytes = s.as_bytes();
                                    if let Ok(data_id) = self.module.declare_anonymous_data(false, false) {
                                        let mut data_desc = cranelift_module::DataDescription::new();
                                        data_desc.define(bytes.to_vec().into_boxed_slice());
                                        if self.module.define_data(data_id, &data_desc).is_ok() {
                                            let data_val = self.module.declare_data_in_func(data_id, builder.func);
                                            let ptr = builder.ins().global_value(types::I64, data_val);
                                            let len = builder.ins().iconst(types::I32, bytes.len() as i64);
                                            let call = builder.ins().call(func_ref, &[ptr, len]);
                                            // String pointer needs to be stored as I64 if param is pointer-typed
                                            let str_ptr = builder.inst_results(call)[0];
                                            if info.is_pointer {
                                                Some(str_ptr)
                                            } else {
                                                // For f64-typed params, NaN-box the string pointer
                                                Some(builder.ins().bitcast(types::F64, MemFlags::new(), str_ptr))
                                            }
                                        } else { None }
                                    } else { None }
                                } else { None }
                            }
                            perry_hir::Expr::Undefined => None, // Default is undefined, no check needed
                            _ => None, // Complex defaults not handled inline
                        };

                        if let Some(default_val) = default_val_opt {
                            let param_val = builder.use_var(var);
                            let var_type = builder.func.dfg.value_type(param_val);
                            let is_undefined = if var_type == types::I64 {
                                let zero = builder.ins().iconst(types::I64, 0);
                                builder.ins().icmp(IntCC::Equal, param_val, zero)
                            } else {
                                // Compare against TAG_UNDEFINED (0x7FFC_0000_0000_0001).
                                // The call site pads missing args with TAG_UNDEFINED.
                                let raw_bits = builder.ins().bitcast(types::I64, MemFlags::new(), param_val);
                                let tag_undefined = builder.ins().iconst(types::I64, 0x7FFC_0000_0000_0001u64 as i64);
                                builder.ins().icmp(IntCC::Equal, raw_bits, tag_undefined)
                            };

                            let default_block = builder.create_block();
                            let continue_block = builder.create_block();
                            builder.ins().brif(is_undefined, default_block, &[], continue_block, &[]);

                            builder.switch_to_block(default_block);
                            builder.seal_block(default_block);
                            builder.def_var(var, default_val);
                            builder.ins().jump(continue_block, &[]);

                            builder.switch_to_block(continue_block);
                            builder.seal_block(continue_block);
                        }
                    }
                }
            }

            // Load captured variables from the closure object
            // Each capture is stored as f64 in the closure object
            // For mutable captures, the f64 is actually a bitcast of a box pointer
            let get_capture_func = self.extern_funcs.get("js_closure_get_capture_f64")
                .ok_or_else(|| anyhow!("js_closure_get_capture_f64 not declared"))?;
            let get_capture_ref = self.module.declare_func_in_func(*get_capture_func, builder.func);

            // If captures_this, load `this` from capture slot 0 first
            // Then other captures are offset by 1
            let capture_offset = if captures_this { 1 } else { 0 };

            // Variable to hold `this` if captured
            let this_var = if captures_this {
                // Load `this` from capture slot 0
                let idx = builder.ins().iconst(types::I32, 0);
                let call = builder.ins().call(get_capture_ref, &[closure_ptr, idx]);
                let this_f64 = builder.inst_results(call)[0];

                // `this` is stored as f64 (bitcast of i64 pointer), convert back to i64
                let this_ptr = ensure_i64(&mut builder, this_f64);

                // Store in a variable
                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, types::I64);
                builder.def_var(var, this_ptr);

                Some(var)
            } else {
                None
            };

            for (i, capture_id) in captures.iter().enumerate() {
                let is_mutable = mutable_set.contains(capture_id);
                // Load the captured value: js_closure_get_capture_f64(closure_ptr, index)
                // Index is offset by 1 if captures_this (slot 0 is `this`)
                let idx = builder.ins().iconst(types::I32, (i + capture_offset) as i64);
                let call = builder.ins().call(get_capture_ref, &[closure_ptr, idx]);
                let val_f64 = builder.inst_results(call)[0];

                if is_mutable {
                    // For mutable captures, the stored f64 is actually a box pointer
                    // Convert to i64 and store the box pointer
                    let var = Variable::new(next_var);
                    next_var += 1;
                    builder.declare_var(var, types::I64);
                    let box_ptr = ensure_i64(&mut builder, val_f64);
                    builder.def_var(var, box_ptr);

                    // Preserve type info from the original module-level variable
                    // so that array indexing, string comparison, typeof etc. work correctly
                    let orig = self.module_level_locals.get(capture_id);
                    locals.insert(*capture_id, LocalInfo {
                        var,
                        name: None, // Captures don't have a direct name
                        class_name: orig.and_then(|o| o.class_name.clone()),
                        type_args: orig.map(|o| o.type_args.clone()).unwrap_or_default(),
                        is_pointer: orig.map(|o| o.is_pointer).unwrap_or(false),
                        is_array: orig.map(|o| o.is_array).unwrap_or(false),
                        is_string: orig.map(|o| o.is_string).unwrap_or(false),
                        is_bigint: orig.map(|o| o.is_bigint).unwrap_or(false),
                        is_closure: orig.map(|o| o.is_closure).unwrap_or(false), closure_func_id: orig.and_then(|o| o.closure_func_id),
                        is_boxed: true,
                        is_map: orig.map(|o| o.is_map).unwrap_or(false),
                        is_set: orig.map(|o| o.is_set).unwrap_or(false),
                        is_buffer: orig.map(|o| o.is_buffer).unwrap_or(false),
                        is_event_emitter: orig.map(|o| o.is_event_emitter).unwrap_or(false),
                        is_union: orig.map(|o| o.is_union).unwrap_or(false),
                        is_mixed_array: orig.map(|o| o.is_mixed_array).unwrap_or(false),
                        is_integer: false,
                        is_integer_array: false,
                        is_i32: false, is_boolean: false,
                        i32_shadow: None,
                        bounded_by_array: None,
                        bounded_by_constant: None,
                        scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: orig.and_then(|o| o.module_var_data_id), class_ref_name: orig.and_then(|o| o.class_ref_name.clone()), object_field_indices: None,
                    });
                } else {
                    // For immutable captures, check if original was a pointer type
                    let orig = self.module_level_locals.get(capture_id);
                    let orig_is_pointer = orig.map(|o| o.is_pointer && !o.is_union).unwrap_or(false);

                    let var = Variable::new(next_var);
                    next_var += 1;

                    if orig_is_pointer {
                        // Pointer type: extract raw pointer from NaN-boxed F64
                        builder.declare_var(var, types::I64);
                        let ptr = ensure_i64(&mut builder, val_f64);
                        builder.def_var(var, ptr);
                    } else {
                        builder.declare_var(var, types::F64);
                        builder.def_var(var, val_f64);
                    }

                    // Preserve type info from the original variable for static analysis
                    // (typeof, string comparison, etc.)
                    locals.insert(*capture_id, LocalInfo {
                        var,
                        name: None, // Captures don't have a direct name
                        class_name: orig.and_then(|o| o.class_name.clone()),
                        type_args: orig.map(|o| o.type_args.clone()).unwrap_or_default(),
                        is_pointer: orig_is_pointer, // Pointer captures use I64
                        is_array: orig.map(|o| o.is_array).unwrap_or(false),
                        is_string: orig.map(|o| o.is_string).unwrap_or(false),
                        is_bigint: orig.map(|o| o.is_bigint).unwrap_or(false),
                        is_closure: orig.map(|o| o.is_closure).unwrap_or(false), closure_func_id: orig.and_then(|o| o.closure_func_id),
                        is_boxed: false,
                        is_map: orig.map(|o| o.is_map).unwrap_or(false),
                        is_set: orig.map(|o| o.is_set).unwrap_or(false),
                        is_buffer: orig.map(|o| o.is_buffer).unwrap_or(false),
                        is_event_emitter: orig.map(|o| o.is_event_emitter).unwrap_or(false),
                        is_union: orig.map(|o| o.is_union).unwrap_or(false),
                        is_mixed_array: orig.map(|o| o.is_mixed_array).unwrap_or(false),
                        is_integer: false,
                        is_integer_array: false,
                        is_i32: false, is_boolean: false,
                        i32_shadow: None,
                        bounded_by_array: None,
                        bounded_by_constant: None,
                        scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: orig.and_then(|o| o.module_var_data_id), class_ref_name: orig.and_then(|o| o.class_ref_name.clone()), object_field_indices: None,
                    });
                }
            }

            // Collect which module-level variables the closure body actually references.
            // Only load those, to avoid generating unnecessary Cranelift instructions
            // for all module-level variables in every closure.
            let mut referenced = std::collections::HashSet::new();
            collect_referenced_locals_stmts(body, &mut referenced);

            // Load module-level variables from their global slots
            // Only load variables that the closure body actually references
            for (local_id, data_id) in &self.module_var_data_ids {
                // Skip if not referenced by the closure body
                if !referenced.contains(local_id) {
                    continue;
                }
                // Skip if already in locals (e.g., passed as a capture)
                if locals.contains_key(local_id) {
                    continue;
                }

                // Get the type info from module_level_locals (populated during compile_init)
                let (var_type, local_info_template) = if let Some(info) = self.module_level_locals.get(local_id) {
                    let vt = info.cranelift_var_type();
                    (vt, info.clone())
                } else {
                    // Fallback to f64 if type info not available
                    (types::F64, LocalInfo {
                        var: Variable::new(0), // Will be overwritten
                        name: None,
                        class_name: None,
                        type_args: Vec::new(),
                        is_pointer: false,
                        is_array: false,
                        is_string: false,
                        is_bigint: false,
                        is_closure: false, closure_func_id: None,
                        is_boxed: false,
                        is_map: false, is_set: false, is_buffer: false, is_event_emitter: false, is_union: false,
                        is_mixed_array: false,
                        is_integer: false,
                        is_integer_array: false,
                        is_i32: false, is_boolean: false,
                        i32_shadow: None,
                        bounded_by_array: None,
                        bounded_by_constant: None,
                        scalar_fields: None,
                        squared_cache: None, product_cache: None, cached_array_ptr: None, const_value: None, hoisted_element_loads: None, hoisted_i32_products: None, module_var_data_id: None, class_ref_name: None, object_field_indices: None,
                    })
                };

                // Skip if this LocalId is already a closure parameter
                if locals.contains_key(local_id) {
                    continue;
                }

                let var = Variable::new(next_var);
                next_var += 1;
                builder.declare_var(var, var_type);

                // Load the value from the global data slot
                let global_val = self.module.declare_data_in_func(*data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_val);
                let val = builder.ins().load(var_type, MemFlags::new(), ptr, 0);
                builder.def_var(var, val);

                // Insert into locals so LocalGet can find it
                let mut info = local_info_template;
                info.var = var;
                info.module_var_data_id = Some(*data_id);
                locals.insert(*local_id, info);
            }

            // Create ThisContext if we captured `this`
            let this_ctx = if let Some(var) = this_var {
                let meta = class_meta.clone().unwrap_or_else(|| {
                    // Object literal method: no class metadata, but we still
                    // need a ThisContext so that `Expr::This` resolves to the
                    // captured object pointer and `this.prop` falls through
                    // to name-based property access.
                    ClassMeta {
                        id: 0,
                        parent_class: None,
                        native_parent: None,
                        own_field_count: 0,
                        field_count: 0,
                        field_indices: std::collections::HashMap::new(),
                        field_types: std::collections::HashMap::new(),
                        constructor_id: None,
                        method_ids: std::collections::HashMap::new(),
                        getter_ids: std::collections::HashMap::new(),
                        setter_ids: std::collections::HashMap::new(),
                        static_method_ids: std::collections::HashMap::new(),
                        static_field_ids: std::collections::HashMap::new(),
                        method_param_counts: std::collections::HashMap::new(),
                        method_return_types: std::collections::HashMap::new(),
                        static_method_return_types: std::collections::HashMap::new(),
                        type_params: Vec::new(),
                        field_inits: std::collections::HashMap::new(),
                    }
                });
                Some(ThisContext {
                    this_var: var,
                    class_meta: meta,
                })
            } else {
                None
            };

            // For async closures, create a Promise variable to track
            let promise_var = if is_async {
                let var = Variable::new(next_temp_var_id());
                builder.declare_var(var, types::I64);

                // Allocate the promise: js_promise_new()
                let promise_new = self.extern_funcs.get("js_promise_new")
                    .ok_or_else(|| anyhow!("js_promise_new not declared"))?;
                let func_ref = self.module.declare_func_in_func(*promise_new, builder.func);
                let call = builder.ins().call(func_ref, &[]);
                let promise_ptr = builder.inst_results(call)[0];
                builder.def_var(var, promise_ptr);

                Some(var)
            } else {
                None
            };

            // Compile the body - use compile_async_stmt for async closures.
            // Wrap in implicit try/catch so throws reject the Promise (matching JS semantics).
            if is_async {
                let promise_var_unwrapped = promise_var.unwrap();

                // === Implicit try/catch for async closure body ===
                let try_body_block = builder.create_block();
                let implicit_catch_block = builder.create_block();

                // Push try frame
                let try_push_func = self.extern_funcs.get("js_try_push")
                    .ok_or_else(|| anyhow!("js_try_push not declared"))?;
                let try_push_ref = self.module.declare_func_in_func(*try_push_func, builder.func);
                let call = builder.ins().call(try_push_ref, &[]);
                let jmp_buf_ptr = builder.inst_results(call)[0];

                // Call setjmp directly (must be in this stack frame)
                let setjmp_func = self.extern_funcs.get("setjmp")
                    .ok_or_else(|| anyhow!("setjmp not declared"))?;
                let setjmp_ref = self.module.declare_func_in_func(*setjmp_func, builder.func);
                let call = builder.ins().call(setjmp_ref, &[jmp_buf_ptr]);
                let setjmp_result = builder.inst_results(call)[0];

                // Branch: 0 = normal execution, non-0 = exception caught
                let zero = builder.ins().iconst(types::I32, 0);
                let is_normal = builder.ins().icmp(IntCC::Equal, setjmp_result, zero);
                builder.ins().brif(is_normal, try_body_block, &[], implicit_catch_block, &[]);

                // === Try body ===
                builder.switch_to_block(try_body_block);
                builder.seal_block(try_body_block);

                TRY_CATCH_DEPTH.with(|d| d.set(d.get() + 1));
                for stmt in body {
                    compile_async_stmt(
                        &mut builder,
                        &mut self.module,
                        &self.func_ids,
                        &self.closure_func_ids,
                        &self.func_wrapper_ids,
                        &self.extern_funcs,
                        &self.async_func_ids,
                        &self.closure_returning_funcs,
                        &self.classes,
                        &self.enums,
                        &self.func_param_types,
                        &self.func_union_params,
                        &self.func_return_types,
                        &self.func_hir_return_types,
                        &self.func_rest_param_index,
                        &self.imported_func_param_counts,
                        &mut locals,
                        &mut next_var,
                        stmt,
                        promise_var_unwrapped,
                        &boxed_vars,
                        true,  // Closures return F64, so bitcast Promise pointer
                    ).map_err(|e| anyhow!("In async closure (func_id={}, captures={:?}): {}", func_id, captures, e))?;
                }
                TRY_CATCH_DEPTH.with(|d| d.set(d.get() - 1));

                // If no explicit return, resolve with undefined and return the promise
                let current_block = builder.current_block().unwrap();
                if !is_block_filled(&builder, current_block) {
                    let promise_ptr = builder.use_var(promise_var_unwrapped);

                    // Resolve with undefined
                    let resolve_func = self.extern_funcs.get("js_promise_resolve")
                        .ok_or_else(|| anyhow!("js_promise_resolve not declared"))?;
                    let resolve_ref = self.module.declare_func_in_func(*resolve_func, builder.func);
                    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
                    let undef_val = builder.ins().f64const(f64::from_bits(TAG_UNDEFINED));
                    builder.ins().call(resolve_ref, &[promise_ptr, undef_val]);

                    // Pop implicit try frame before returning
                    let try_end_func = self.extern_funcs.get("js_try_end")
                        .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                    let try_end_ref = self.module.declare_func_in_func(*try_end_func, builder.func);
                    builder.ins().call(try_end_ref, &[]);

                    // NaN-box the Promise pointer with POINTER_TAG so caller can detect it
                    let nanbox_func = self.extern_funcs.get("js_nanbox_pointer")
                        .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                    let nanbox_ref = self.module.declare_func_in_func(*nanbox_func, builder.func);
                    let call = builder.ins().call(nanbox_ref, &[promise_ptr]);
                    let ret_val = builder.inst_results(call)[0];
                    builder.ins().return_(&[ret_val]);
                }

                // === Implicit catch: reject Promise with exception ===
                builder.switch_to_block(implicit_catch_block);
                builder.seal_block(implicit_catch_block);

                // Pop try frame
                {
                    let try_end_func = self.extern_funcs.get("js_try_end")
                        .ok_or_else(|| anyhow!("js_try_end not declared"))?;
                    let try_end_ref = self.module.declare_func_in_func(*try_end_func, builder.func);
                    builder.ins().call(try_end_ref, &[]);
                }

                // Get the exception value
                let get_exc_func = self.extern_funcs.get("js_get_exception")
                    .ok_or_else(|| anyhow!("js_get_exception not declared"))?;
                let get_exc_ref = self.module.declare_func_in_func(*get_exc_func, builder.func);
                let call = builder.ins().call(get_exc_ref, &[]);
                let exc_val = builder.inst_results(call)[0];

                // Clear exception
                let clear_exc_func = self.extern_funcs.get("js_clear_exception")
                    .ok_or_else(|| anyhow!("js_clear_exception not declared"))?;
                let clear_exc_ref = self.module.declare_func_in_func(*clear_exc_func, builder.func);
                builder.ins().call(clear_exc_ref, &[]);

                // Reject promise with exception
                let promise_ptr = builder.use_var(promise_var_unwrapped);
                let reject_func = self.extern_funcs.get("js_promise_reject")
                    .ok_or_else(|| anyhow!("js_promise_reject not declared"))?;
                let reject_ref = self.module.declare_func_in_func(*reject_func, builder.func);
                builder.ins().call(reject_ref, &[promise_ptr, exc_val]);

                // NaN-box and return the rejected promise
                let nanbox_func = self.extern_funcs.get("js_nanbox_pointer")
                    .ok_or_else(|| anyhow!("js_nanbox_pointer not declared"))?;
                let nanbox_ref = self.module.declare_func_in_func(*nanbox_func, builder.func);
                let call = builder.ins().call(nanbox_ref, &[promise_ptr]);
                let ret_val = builder.inst_results(call)[0];
                builder.ins().return_(&[ret_val]);
            } else {
                for stmt in body {
                    compile_stmt(
                        &mut builder,
                        &mut self.module,
                        &self.func_ids,
                        &self.closure_func_ids,
                        &self.func_wrapper_ids,
                        &self.extern_funcs,
                        &self.async_func_ids,
                        &self.closure_returning_funcs,
                        &self.classes,
                        &self.enums,
                        &self.func_param_types,
                        &self.func_union_params,
                        &self.func_return_types,
                        &self.func_hir_return_types,
                        &self.func_rest_param_index,
                        &self.imported_func_param_counts,
                        &mut locals,
                        &mut next_var,
                        stmt,
                        this_ctx.as_ref(),
                        None,
                        &boxed_vars,
                        None,
                    ).map_err(|e| anyhow!("In closure (func_id={}, captures={:?}): {}", func_id, captures, e))?;
                }

                // If no explicit return, return 0 with correct type
                let current_block = builder.current_block().unwrap();
                if !is_block_filled(&builder, current_block) {
                    let ret_type = builder.func.signature.returns.first().map(|p| p.value_type).unwrap_or(types::F64);
                    let zero = match ret_type {
                        types::I64 => builder.ins().iconst(types::I64, 0),
                        types::I32 => builder.ins().iconst(types::I32, 0),
                        _ => builder.ins().f64const(0.0),
                    };
                    builder.ins().return_(&[zero]);
                }
            }

            builder.finalize();
        }

        if let Err(e) = self.module.define_function(clif_func_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in closure_{} ({} params) ===", func_id, params.len());
            eprintln!("Error: {}", e);
            eprintln!("Debug: {:?}", e);
            return Err(anyhow!("Error compiling closure_{}: {}", func_id, e));
        }
        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    /// Get or create a closure-compatible wrapper for a named function
    /// This is needed when passing a named function as a callback argument
    pub(crate) fn get_or_create_func_wrapper(&mut self, func_id: u32) -> Result<cranelift_module::FuncId> {
        // Check if we already have a wrapper
        if let Some(&wrapper_id) = self.func_wrapper_ids.get(&func_id) {
            return Ok(wrapper_id);
        }

        // Find the original function in HIR
        let func = self.hir_functions.iter()
            .find(|f| f.id == func_id)
            .ok_or_else(|| anyhow!("Function not found for wrapper generation: {}", func_id))?
            .clone();

        // Create wrapper function signature: (closure_ptr, ...args) -> result
        // For exported functions, always return f64 to provide uniform ABI for cross-module calls
        // For local wrappers, use the original return type
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // closure_ptr (ignored)
        for _param in &func.params {
            sig.params.push(AbiParam::new(types::F64)); // all args as f64 for now
        }
        // Determine return type: for exported functions, always f64; for local, use actual type
        let original_return_abi = if func.is_async {
            types::I64 // async functions return Promise pointer
        } else {
            self.type_to_abi(&func.return_type)
        };
        // All wrappers return f64 for uniform cross-module ABI (NaN-boxing for type safety)
        sig.returns.push(AbiParam::new(types::F64));

        // Scope wrapper name with module prefix to prevent collisions between modules
        // that define functions with the same name (e.g., formatTokenAmount in lib/generic.ts and lib/risk-assessment.ts)
        let wrapper_name = if self.module_symbol_prefix.is_empty() {
            format!("__wrapper_{}", func.name)
        } else {
            format!("__wrapper_{}__{}", self.module_symbol_prefix, func.name)
        };
        // All wrapper functions use Export linkage. The wrapper name is scoped with the
        // module prefix (__wrapper_{prefix}__{name}), preventing symbol collisions even when
        // two modules define functions with the same name. Export linkage ensures that
        // func_addr produces a linker-resolved absolute address, which is more reliable
        // than Cranelift-internal resolution for Local symbols — particularly on x86_64 ELF
        // where Local func_addr relocations can produce incorrect addresses in large binaries.
        let linkage = Linkage::Export;
        let wrapper_id = self.module.declare_function(&wrapper_name, linkage, &sig)?;
        // Track whether we need to NaN-box the return value (always needed since we return f64)
        let needs_return_boxing = original_return_abi == types::I64;
        let is_string_return = matches!(func.return_type, perry_types::Type::String);

        // Pre-compute expected types for each parameter before borrowing self.ctx
        let param_expected_types: Vec<types::Type> = func.params.iter()
            .map(|p| self.type_to_abi(&p.ty))
            .collect();

        // Get original function id before borrowing self.ctx
        let original_func_id = *self.func_ids.get(&func_id)
            .ok_or_else(|| anyhow!("Original function not found: {}", func_id))?;

        // Get js_nanbox_get_pointer for extracting pointers from NaN-boxed values
        let nanbox_get_ptr_id = self.extern_funcs.get("js_nanbox_get_pointer").copied();

        // Compile the wrapper function
        // Use a fresh FunctionBuilderContext to avoid state accumulation from previous compilations
        // This is important because we generate wrappers after all regular functions are compiled,
        // and the shared func_ctx may have accumulated state that causes issues.
        self.ctx.func.signature = sig;
        let mut wrapper_func_ctx = FunctionBuilderContext::new();

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut wrapper_func_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Skip the closure_ptr (param 0) and forward the rest to the original function
            // Need to convert f64 wrapper params to the types the original function expects
            // For cross-module calls, i64 params are now properly NaN-boxed, so we need to
            // extract the pointer using js_nanbox_get_pointer instead of simple bitcast.
            // Copy block params to a Vec first to avoid borrow conflict
            let block_params: Vec<Value> = builder.block_params(entry_block).to_vec();
            let mut call_args: Vec<Value> = Vec::new();
            for (i, expected_type) in param_expected_types.iter().enumerate() {
                let wrapper_param = block_params[i + 1]; // +1 to skip closure_ptr
                if *expected_type == types::I64 {
                    // Original function expects i64, wrapper has f64 (NaN-boxed pointer)
                    // Use js_nanbox_get_pointer to properly extract the pointer
                    if let Some(get_ptr_id) = nanbox_get_ptr_id {
                        let get_ptr_ref = self.module.declare_func_in_func(get_ptr_id, builder.func);
                        let call = builder.ins().call(get_ptr_ref, &[wrapper_param]);
                        call_args.push(builder.inst_results(call)[0]);
                    } else {
                        // Fallback to bitcast if js_nanbox_get_pointer not available
                        let converted = ensure_i64(&mut builder, wrapper_param);
                        call_args.push(converted);
                    }
                } else {
                    // Same type (f64), pass directly
                    call_args.push(wrapper_param);
                }
            }

            // Call the original function
            let func_ref = self.module.declare_func_in_func(original_func_id, builder.func);

            // Get expected parameter count from the actual function signature
            let actual_sig = self.module.declarations().get_function_decl(original_func_id);
            let expected_param_count = actual_sig.signature.params.len();

            // First, ensure all arguments match the expected types
            let mut final_call_args: Vec<Value> = call_args.iter().enumerate()
                .map(|(i, &val)| {
                    if i < actual_sig.signature.params.len() {
                        let expected_type = actual_sig.signature.params[i].value_type;
                        let actual_type = builder.func.dfg.value_type(val);
                        if expected_type == types::I64 && actual_type == types::F64 {
                            // Strip NaN-box tag bits to get raw pointer
                            ensure_i64(&mut builder, val)
                        } else if expected_type == types::F64 && actual_type == types::I64 {
                            // i64 pointer -> f64: NaN-box with POINTER_TAG
                            inline_nanbox_pointer(&mut builder, val)
                        } else if expected_type == types::F64 && actual_type == types::I32 {
                            // i32 (from loop optimization) -> f64
                            builder.ins().fcvt_from_sint(types::F64, val)
                        } else if expected_type == types::I64 && actual_type == types::I32 {
                            // i32 (from loop optimization) -> i64
                            builder.ins().sextend(types::I64, val)
                        } else {
                            val
                        }
                    } else {
                        val
                    }
                })
                .collect();

            // Pad arguments if needed (for optional parameters), using correct types
            while final_call_args.len() < expected_param_count {
                let expected_type = actual_sig.signature.params[final_call_args.len()].value_type;
                if expected_type == types::I64 {
                    final_call_args.push(builder.ins().iconst(types::I64, 0));
                } else {
                    final_call_args.push(builder.ins().f64const(f64::from_bits(0x7FFC_0000_0000_0001u64)));
                }
            }
            final_call_args.truncate(expected_param_count);

            let call = builder.ins().call(func_ref, &final_call_args);
            let result = builder.inst_results(call)[0];

            // For exported wrappers returning i64 (pointers), NaN-box to f64 for uniform ABI
            let final_result = if needs_return_boxing {
                // Use js_nanbox_string for string returns, js_nanbox_pointer for others
                let nanbox_func_name = if is_string_return { "js_nanbox_string" } else { "js_nanbox_pointer" };
                let nanbox_func_id = self.extern_funcs.get(nanbox_func_name)
                    .ok_or_else(|| anyhow!("{} not declared", nanbox_func_name))?;
                let nanbox_ref = self.module.declare_func_in_func(*nanbox_func_id, builder.func);
                let nanbox_call = builder.ins().call(nanbox_ref, &[result]);
                builder.inst_results(nanbox_call)[0]
            } else {
                result
            };

            builder.ins().return_(&[final_result]);
            builder.finalize();
        }

        if let Err(e) = self.module.define_function(wrapper_id, &mut self.ctx) {
            eprintln!("=== VERIFIER ERROR in wrapper '{}' ===", func.name);
            eprintln!("Error: {}", e);
            return Err(anyhow!("Error compiling wrapper '{}': {}", func.name, e));
        }
        self.module.clear_context(&mut self.ctx);

        self.func_wrapper_ids.insert(func_id, wrapper_id);
        Ok(wrapper_id)
    }

    /// Generate wrapper functions for exported closures
    ///
    /// When TypeScript has `export const fn = () => {}`, this is an exported object containing
    /// a closure, not an exported function. Cross-module calls still expect `__wrapper_fn` to exist.
    /// This function scans init statements for such patterns and generates wrapper stubs that:
    /// 1. Load the closure from the `__export_fn` global
    /// 2. Call the closure with the provided arguments
    /// 3. Return the result
    pub(crate) fn generate_exported_closure_wrappers(&mut self, init_stmts: &[Stmt], exported_objects: &[String]) -> Result<()> {
        use perry_hir::ir::{Stmt, Expr};

        let exported_set: std::collections::HashSet<&String> = exported_objects.iter().collect();

        // Scan init statements for exported closures
        for stmt in init_stmts {
            if let Stmt::Let { name, init: Some(init_expr), .. } = stmt {
                if !exported_set.contains(name) {
                    continue;
                }

                // Check if the initializer is a Closure
                let (params, is_async) = match init_expr {
                    Expr::Closure { params, is_async, .. } => {
                        (params.clone(), *is_async)
                    },
                    _other => {
                        continue;
                    }
                };

                // Generate wrapper for this closure
                log::debug!("Generating closure wrapper for exported const: {}", name);

                // Build wrapper signature: (closure_ptr, ...args) -> f64
                let mut sig = self.module.make_signature();
                sig.params.push(AbiParam::new(types::I64)); // closure_ptr (will be ignored, we load from global)
                for _ in &params {
                    sig.params.push(AbiParam::new(types::F64));
                }
                sig.returns.push(AbiParam::new(types::F64));

                let wrapper_name = if self.module_symbol_prefix.is_empty() {
                    format!("__wrapper_{}", name)
                } else {
                    format!("__wrapper_{}__{}", self.module_symbol_prefix, name)
                };
                let wrapper_id = self.module.declare_function(&wrapper_name, Linkage::Export, &sig)?;

                // Get the data ID for the exported global
                let export_global_name = if self.module_symbol_prefix.is_empty() {
                    format!("__export_{}", name)
                } else {
                    format!("__export_{}__{}", self.module_symbol_prefix, name)
                };
                let data_id = match self.exported_object_ids.get(name) {
                    Some(id) => *id,
                    None => {
                        // If not already declared, declare it now
                        self.module.declare_data(&export_global_name, Linkage::Local, true, false)?
                    }
                };

                // Get closure call function based on param count
                let call_func_name_owned;
                let call_func_name: &str = match params.len() {
                    0 => "js_closure_call0",
                    1 => "js_closure_call1",
                    2 => "js_closure_call2",
                    3 => "js_closure_call3",
                    4 => "js_closure_call4",
                    5 => "js_closure_call5",
                    6 => "js_closure_call6",
                    7 => "js_closure_call7",
                    8 => "js_closure_call8",
                    n @ 9..=16 => {
                        call_func_name_owned = format!("js_closure_call{}", n);
                        &call_func_name_owned
                    }
                    _ => {
                        log::warn!("Exported closure {} has too many params ({}), skipping wrapper", name, params.len());
                        continue;
                    }
                };
                let closure_call_id = self.extern_funcs.get(call_func_name)
                    .copied()
                    .ok_or_else(|| anyhow!("{} not declared for closure wrapper", call_func_name))?;

                // Compile the wrapper function
                self.ctx.func.signature = sig.clone();
                let mut wrapper_func_ctx = FunctionBuilderContext::new();

                {
                    let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut wrapper_func_ctx);

                    let entry_block = builder.create_block();
                    builder.append_block_params_for_function_params(entry_block);
                    builder.switch_to_block(entry_block);
                    builder.seal_block(entry_block);

                    // Get block params (skip closure_ptr at index 0, we load from global instead)
                    let block_params: Vec<Value> = builder.block_params(entry_block).to_vec();

                    // Load the closure from the exported global
                    let data_gv = self.module.declare_data_in_func(data_id, builder.func);
                    let data_ptr = builder.ins().global_value(types::I64, data_gv);
                    // The global stores an f64 (NaN-boxed closure pointer with POINTER_TAG)
                    let closure_f64 = builder.ins().load(types::F64, MemFlags::new(), data_ptr, 0);
                    // Use js_nanbox_get_pointer to strip POINTER_TAG and get raw pointer
                    // (bitcast alone would preserve the tag bits, giving js_closure_call* a tagged pointer)
                    let get_ptr_func = self.extern_funcs.get("js_nanbox_get_pointer")
                        .ok_or_else(|| anyhow!("js_nanbox_get_pointer not declared for closure wrapper"))?;
                    let get_ptr_ref = self.module.declare_func_in_func(*get_ptr_func, builder.func);
                    let get_ptr_call = builder.ins().call(get_ptr_ref, &[closure_f64]);
                    let closure_ptr = builder.inst_results(get_ptr_call)[0];

                    // Build call args: [closure_ptr, ...args]
                    let mut call_args = vec![closure_ptr];
                    for i in 0..params.len() {
                        call_args.push(block_params[i + 1]); // +1 to skip wrapper's closure_ptr param
                    }

                    // Call the closure
                    let closure_call_ref = self.module.declare_func_in_func(closure_call_id, builder.func);
                    let call = builder.ins().call(closure_call_ref, &call_args);
                    let result = builder.inst_results(call)[0];

                    builder.ins().return_(&[result]);
                    builder.finalize();
                }

                if let Err(e) = self.module.define_function(wrapper_id, &mut self.ctx) {
                    eprintln!("=== VERIFIER ERROR in closure wrapper '{}' ===", name);
                    eprintln!("Error: {}", e);
                    return Err(anyhow!("Error compiling closure wrapper '{}': {}", name, e));
                }
                self.module.clear_context(&mut self.ctx);
            }
        }

        Ok(())
    }

}
