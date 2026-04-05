//! Declaration lowering.
//!
//! Contains functions for lowering function declarations, class declarations,
//! enum declarations, interface declarations, type alias declarations,
//! constructors, class methods, getters, setters, and class properties.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{LoweringContext, lower_expr};
use crate::lower_types::*;
use crate::lower_patterns::*;
use crate::destructuring::*;
use crate::analysis::*;

pub(crate) fn lower_fn_decl(ctx: &mut LoweringContext, fn_decl: &ast::FnDecl) -> Result<Function> {
    let name = fn_decl.ident.sym.to_string();
    let func_id = ctx.lookup_func(&name).unwrap_or_else(|| ctx.fresh_func());

    // Extract type parameters from generic function declaration (e.g., function foo<T, U>(...))
    let type_params = fn_decl.function.type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type parameter scope for resolving T, U, etc. in body types
    ctx.enter_type_param_scope(&type_params);

    let scope_mark = ctx.enter_scope();

    // Lower parameters with type extraction (using context for type param resolution)
    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in fn_decl.function.params.iter() {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_default = get_param_default(ctx, &param.pat)?;
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        let is_rest = is_rest_param(&param.pat);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: param_default,
            is_rest,
        });
        // Track destructuring patterns (or an Assign wrapping one) for extraction stmts
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }

    // Register parameters with known native types as native instances
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            let native_info = match type_name.as_str() {
                "PluginApi" => Some(("perry/plugin", "PluginApi")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
                // Fastify types
                "FastifyInstance" => Some(("fastify", "App")),
                "FastifyRequest" => Some(("fastify", "Request")),
                "FastifyReply" => Some(("fastify", "Reply")),
                _ => None,
            };
            if let Some((module, class)) = native_info {
                ctx.register_native_instance(param.name.clone(), module.to_string(), class.to_string());
            }
        }
    }

    // Extract return type from function's type annotation (with context)
    let return_type = fn_decl.function.return_type.as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Check if return type is a native module type (e.g., mysql.Pool, mysql.PoolConnection)
    // For async functions, unwrap Promise<T> first
    let check_type = match &return_type {
        Type::Generic { base, type_args } if base == "Promise" => {
            type_args.first().unwrap_or(&return_type)
        }
        Type::Promise(inner) => inner.as_ref(),
        other => other,
    };
    if let Type::Named(type_name) = check_type {
        if let Some(dot_pos) = type_name.find('.') {
            let module_alias = &type_name[..dot_pos];
            let class_name = &type_name[dot_pos + 1..];
            if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                ctx.func_return_native_instances.push((
                    name.clone(),
                    module_name.to_string(),
                    class_name.to_string(),
                ));
            }
        } else {
            // Bare type name check (e.g., `Redis` instead of `ioredis.Redis`)
            let module_info = match type_name.as_str() {
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
                "Pool" => Some(("mysql2/promise", "Pool")),
                "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                _ => None,
            };
            if let Some((module, class)) = module_info {
                ctx.func_return_native_instances.push((
                    name.clone(), module.to_string(), class.to_string()
                ));
            }
        }
    }

    // Generate destructuring statements for patterns in parameters BEFORE lowering body
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    // Lower body
    let mut body = if let Some(ref block) = fn_decl.function.body {
        lower_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Prepend destructuring statements to body
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // After body lowering, check if any return statement returns a native instance.
    // This handles patterns like: function initDb() { const d = new Database(...); return d; }
    // where the return type annotation is `any` but the actual value is a native handle.
    let ni_start = scope_mark.1;
    if ctx.native_instances.len() > ni_start {
        if let Some(ref block) = fn_decl.function.body {
            find_native_return_in_stmts(&block.stmts, ctx, &name, ni_start);
        }
    }

    ctx.exit_scope(scope_mark);

    // Exit type parameter scope
    ctx.exit_type_param_scope();

    Ok(Function {
        id: func_id,
        name,
        type_params,
        params,
        return_type,
        body,
        is_async: fn_decl.function.is_async,
        is_generator: fn_decl.function.is_generator,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub(crate) fn lower_class_decl(ctx: &mut LoweringContext, class_decl: &ast::ClassDecl, is_exported: bool) -> Result<Class> {
    let name = class_decl.ident.sym.to_string();
    let class_id = match ctx.lookup_class(&name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(name.clone(), id);
            id
        }
    };

    // Set current class for arrow function `this` capture tracking
    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.clone());

    // Extract type parameters from generic class declaration (e.g., class Box<T>)
    let type_params = class_decl.class.type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type parameter scope for resolving T, U, etc. in member types
    ctx.enter_type_param_scope(&type_params);

    // Handle extends clause
    let (extends, extends_name, native_extends) = if let Some(ref super_class) = class_decl.class.super_class {
        if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            // First check if it's a native module class
            let native_parent = match parent_name.as_str() {
                "EventEmitter" => Some(("events".to_string(), "EventEmitter".to_string())),
                "AsyncLocalStorage" => Some(("async_hooks".to_string(), "AsyncLocalStorage".to_string())),
                "WebSocketServer" => Some(("ws".to_string(), "WebSocketServer".to_string())),
                _ => None,
            };
            if native_parent.is_some() {
                (None, None, native_parent)
            } else {
                // Always capture the parent name for imported classes that may not have a ClassId
                (ctx.lookup_class(&parent_name), Some(parent_name), None)
            }
        } else if let ast::Expr::Member(member) = super_class.as_ref() {
            // Handle member expression like ethers.JsonRpcProvider or module.ClassName
            let parent_name = extract_member_class_name(member);
            // For member expressions, we don't have ClassId - just store the name
            (None, Some(parent_name), None)
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    // First pass: collect static field/method names for early registration
    // This allows static method bodies to reference static fields
    let mut static_field_names = Vec::new();
    let mut static_method_names = Vec::new();
    for member in &class_decl.class.body {
        match member {
            ast::ClassMember::Method(method) if method.is_static => {
                if let ast::PropName::Ident(ident) = &method.key {
                    static_method_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::ClassProp(prop) if prop.is_static => {
                if let ast::PropName::Ident(ident) = &prop.key {
                    static_field_names.push(ident.sym.to_string());
                }
            }
            _ => {}
        }
    }

    // Register static members early so method bodies can reference them
    ctx.register_class_statics(name.clone(), static_field_names, static_method_names);

    let mut fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut constructor = None;
    let mut methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();

    // Second pass: actually lower the class members
    for member in &class_decl.class.body {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                constructor = Some(lower_constructor(ctx, &name, ctor)?);
            }
            ast::ClassMember::Method(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                // Get the property name for getters/setters
                let prop_name = match &method.key {
                    ast::PropName::Ident(ident) => ident.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };

                match method.kind {
                    ast::MethodKind::Getter => {
                        // Getter: no parameters, returns a value
                        let func = lower_getter_method(ctx, method)?;
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        // Setter: takes one parameter
                        let func = lower_setter_method(ctx, method)?;
                        setters.push((prop_name, func));
                    }
                    ast::MethodKind::Method => {
                        let func = lower_class_method(ctx, method)?;
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                }
            }
            ast::ClassMember::ClassProp(prop) => {
                // Skip computed/Symbol property keys
                match &prop.key {
                    ast::PropName::Ident(_) | ast::PropName::Str(_) => {},
                    _ => continue,
                }
                let field = lower_class_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                let field = lower_private_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            _ => {}
        }
    }

    // Detect fields from TypeScript parameter properties (e.g., constructor(public name: string)).
    // SWC represents these as TsParamProp in the AST. They must be registered as class fields
    // so that `this.name` access in methods can find them by field index.
    {
        let declared_field_names: std::collections::HashSet<String> = fields.iter().map(|f| f.name.clone()).collect();
        for member in &class_decl.class.body {
            if let ast::ClassMember::Constructor(ctor) = member {
                for param in &ctor.params {
                    if let ast::ParamOrTsParamProp::TsParamProp(ts_prop) = param {
                        let (param_name, param_type) = match &ts_prop.param {
                            ast::TsParamPropParam::Ident(ident) => {
                                let pname = ident.id.sym.to_string();
                                let ty = ident.type_ann.as_ref()
                                    .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                                    .unwrap_or(Type::Any);
                                (pname, ty)
                            }
                            ast::TsParamPropParam::Assign(assign) => {
                                let pname = get_pat_name(&assign.left).unwrap_or_default();
                                let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                                (pname, ty)
                            }
                        };
                        if !param_name.is_empty() && !declared_field_names.contains(&param_name) {
                            fields.push(ClassField {
                                name: param_name,
                                ty: param_type,
                                init: None,
                                is_private: false,
                                is_readonly: ts_prop.readonly,
                            });
                        }
                    }
                }
            }
        }
    }

    // Detect fields from constructor body `this.xxx = ...` assignments.
    // JavaScript classes (e.g., transpiled from TypeScript) often don't have ClassProp
    // declarations; instead they assign to `this` in the constructor body.
    {
        let declared_field_names: std::collections::HashSet<String> = fields.iter().map(|f| f.name.clone()).collect();
        for member in &class_decl.class.body {
            if let ast::ClassMember::Constructor(ctor) = member {
                if let Some(ref body) = ctor.body {
                    for stmt in &body.stmts {
                        if let ast::Stmt::Expr(expr_stmt) = stmt {
                            if let ast::Expr::Assign(assign) = &*expr_stmt.expr {
                                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(mem)) = &assign.left {
                                    if let ast::Expr::This(_) = &*mem.obj {
                                        if let ast::MemberProp::Ident(prop_ident) = &mem.prop {
                                            let fname = prop_ident.sym.to_string();
                                            if !declared_field_names.contains(&fname) {
                                                fields.push(ClassField {
                                                    name: fname,
                                                    ty: Type::Any,
                                                    init: None,
                                                    is_private: false,
                                                    is_readonly: false,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Dedup fields: keep first occurrence of each name
        let mut seen = std::collections::HashSet::new();
        fields.retain(|f| seen.insert(f.name.clone()));
    }

    // Exit type parameter scope
    ctx.exit_type_param_scope();

    // Restore previous current_class
    ctx.current_class = old_class;

    Ok(Class {
        id: class_id,
        name,
        type_params,
        extends,
        extends_name,
        native_extends,
        fields,
        constructor,
        methods,
        getters,
        setters,
        static_fields,
        static_methods,
        is_exported,
    })
}

/// Lower a class expression (ast::Class) to HIR.
/// Used for anonymous class expressions like `new (class extends Command { ... })()`.
pub(crate) fn lower_class_from_ast(ctx: &mut LoweringContext, class: &ast::Class, name: &str, is_exported: bool) -> Result<Class> {
    let class_id = match ctx.lookup_class(name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(name.to_string(), id);
            id
        }
    };

    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.to_string());

    let type_params = class.type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    ctx.enter_type_param_scope(&type_params);

    let (extends, extends_name, native_extends) = if let Some(ref super_class) = class.super_class {
        if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            let native_parent = match parent_name.as_str() {
                "EventEmitter" => Some(("events".to_string(), "EventEmitter".to_string())),
                "AsyncLocalStorage" => Some(("async_hooks".to_string(), "AsyncLocalStorage".to_string())),
                "WebSocketServer" => Some(("ws".to_string(), "WebSocketServer".to_string())),
                _ => None,
            };
            if native_parent.is_some() {
                (None, None, native_parent)
            } else {
                (ctx.lookup_class(&parent_name), Some(parent_name), None)
            }
        } else if let ast::Expr::Member(member) = super_class.as_ref() {
            let parent_name = extract_member_class_name(member);
            (None, Some(parent_name), None)
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    let mut static_field_names = Vec::new();
    let mut static_method_names = Vec::new();
    for member in &class.body {
        match member {
            ast::ClassMember::Method(method) if method.is_static => {
                if let ast::PropName::Ident(ident) = &method.key {
                    static_method_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::ClassProp(prop) if prop.is_static => {
                if let ast::PropName::Ident(ident) = &prop.key {
                    static_field_names.push(ident.sym.to_string());
                }
            }
            _ => {}
        }
    }
    ctx.register_class_statics(name.to_string(), static_field_names, static_method_names);

    let mut fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut constructor = None;
    let mut methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();

    for member in &class.body {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                constructor = Some(lower_constructor(ctx, name, ctor)?);
            }
            ast::ClassMember::Method(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                let prop_name = match &method.key {
                    ast::PropName::Ident(ident) => ident.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                match method.kind {
                    ast::MethodKind::Getter => {
                        let func = lower_getter_method(ctx, method)?;
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let func = lower_setter_method(ctx, method)?;
                        setters.push((prop_name, func));
                    }
                    ast::MethodKind::Method => {
                        let func = lower_class_method(ctx, method)?;
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                }
            }
            ast::ClassMember::ClassProp(prop) => {
                // Skip computed/Symbol property keys
                match &prop.key {
                    ast::PropName::Ident(_) | ast::PropName::Str(_) => {},
                    _ => continue,
                }
                let field = lower_class_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                let field = lower_private_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            _ => {}
        }
    }

    ctx.exit_type_param_scope();
    ctx.current_class = old_class;

    Ok(Class {
        id: class_id,
        name: name.to_string(),
        type_params,
        extends,
        extends_name,
        native_extends,
        fields,
        constructor,
        methods,
        getters,
        setters,
        static_fields,
        static_methods,
        is_exported,
    })
}

pub(crate) fn lower_enum_decl(ctx: &mut LoweringContext, enum_decl: &ast::TsEnumDecl, is_exported: bool) -> Result<Enum> {
    let name = enum_decl.id.sym.to_string();
    let enum_id = ctx.fresh_enum();

    let mut members = Vec::new();
    let mut next_value: i64 = 0;

    for member in &enum_decl.members {
        // Get member name
        let member_name = match &member.id {
            ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            ast::TsEnumMemberId::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        };

        // Get member value
        let value = if let Some(ref init) = member.init {
            match init.as_ref() {
                ast::Expr::Lit(ast::Lit::Num(n)) => {
                    let v = n.value as i64;
                    next_value = v + 1;
                    EnumValue::Number(v)
                }
                ast::Expr::Lit(ast::Lit::Str(s)) => {
                    EnumValue::String(s.value.as_str().unwrap_or("").to_string())
                }
                ast::Expr::Unary(unary) if unary.op == ast::UnaryOp::Minus => {
                    // Handle negative numbers like -1
                    if let ast::Expr::Lit(ast::Lit::Num(n)) = unary.arg.as_ref() {
                        let v = -(n.value as i64);
                        next_value = v + 1;
                        EnumValue::Number(v)
                    } else {
                        // Default to auto-increment
                        let v = next_value;
                        next_value += 1;
                        EnumValue::Number(v)
                    }
                }
                _ => {
                    // For complex expressions, default to auto-increment
                    let v = next_value;
                    next_value += 1;
                    EnumValue::Number(v)
                }
            }
        } else {
            // Auto-increment
            let v = next_value;
            next_value += 1;
            EnumValue::Number(v)
        };

        members.push(EnumMember {
            name: member_name,
            value,
        });
    }

    // Register the enum in the context for later lookups
    let member_values: Vec<(String, EnumValue)> = members.iter()
        .map(|m| (m.name.clone(), m.value.clone()))
        .collect();
    ctx.define_enum(name.clone(), enum_id, member_values);

    Ok(Enum {
        id: enum_id,
        name,
        members,
        is_exported,
    })
}

pub(crate) fn lower_interface_decl(ctx: &mut LoweringContext, iface_decl: &ast::TsInterfaceDecl, is_exported: bool) -> Result<Interface> {
    let name = iface_decl.id.sym.to_string();
    let iface_id = ctx.fresh_interface();

    // Extract type parameters
    let type_params = iface_decl.type_params.as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type param scope for resolving type references in body
    ctx.enter_type_param_scope(&type_params);

    // Extract extended interfaces
    let extends: Vec<Type> = iface_decl.extends.iter()
        .map(|ext| {
            let base_name = match &*ext.expr {
                ast::Expr::Ident(id) => id.sym.to_string(),
                _ => "unknown".to_string(),
            };
            // Handle type arguments if present
            if let Some(ref type_args) = ext.type_args {
                let args: Vec<Type> = type_args.params.iter()
                    .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                    .collect();
                if args.is_empty() {
                    Type::Named(base_name)
                } else {
                    Type::Generic {
                        base: base_name,
                        type_args: args,
                    }
                }
            } else {
                Type::Named(base_name)
            }
        })
        .collect();

    // Extract properties and methods from interface body
    let mut properties = Vec::new();
    let mut methods = Vec::new();

    for member in &iface_decl.body.body {
        match member {
            ast::TsTypeElement::TsPropertySignature(prop) => {
                let prop_name = match &*prop.key {
                    ast::Expr::Ident(id) => id.sym.to_string(),
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                let prop_type = prop.type_ann.as_ref()
                    .map(|ta| extract_ts_type_with_ctx(&ta.type_ann, Some(ctx)))
                    .unwrap_or(Type::Any);
                properties.push(InterfaceProperty {
                    name: prop_name,
                    ty: prop_type,
                    optional: prop.optional,
                    readonly: prop.readonly,
                });
            }
            ast::TsTypeElement::TsMethodSignature(method) => {
                let method_name = match &*method.key {
                    ast::Expr::Ident(id) => id.sym.to_string(),
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };

                // Method's own type parameters
                let method_type_params = method.type_params.as_ref()
                    .map(|tp| extract_type_params(tp))
                    .unwrap_or_default();

                // Enter method's type param scope
                ctx.enter_type_param_scope(&method_type_params);

                // Extract parameters
                let params: Vec<(String, Type, bool)> = method.params.iter()
                    .map(|p| {
                        let (name, ty) = get_fn_param_name_and_type_with_ctx(p, Some(ctx));
                        let optional = matches!(p, ast::TsFnParam::Ident(id) if id.optional);
                        (name, ty, optional)
                    })
                    .collect();

                // Extract return type
                let return_type = method.type_ann.as_ref()
                    .map(|ta| extract_ts_type_with_ctx(&ta.type_ann, Some(ctx)))
                    .unwrap_or(Type::Void);

                ctx.exit_type_param_scope();

                methods.push(InterfaceMethod {
                    name: method_name,
                    type_params: method_type_params,
                    params,
                    return_type,
                });
            }
            _ => {} // Skip other member types for now
        }
    }

    ctx.exit_type_param_scope();

    // Register interface in context
    ctx.interfaces.push((name.clone(), iface_id));

    Ok(Interface {
        id: iface_id,
        name,
        type_params,
        extends,
        properties,
        methods,
        is_exported,
    })
}

pub(crate) fn lower_type_alias_decl(ctx: &mut LoweringContext, alias_decl: &ast::TsTypeAliasDecl, is_exported: bool) -> Result<TypeAlias> {
    let name = alias_decl.id.sym.to_string();
    let alias_id = ctx.fresh_type_alias();

    // Extract type parameters
    let type_params = alias_decl.type_params.as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type param scope for resolving type references
    ctx.enter_type_param_scope(&type_params);

    // Extract the aliased type
    let ty = extract_ts_type_with_ctx(&alias_decl.type_ann, Some(ctx));

    ctx.exit_type_param_scope();

    // Register type alias in context
    ctx.type_aliases.push((name.clone(), alias_id, type_params.clone(), ty.clone()));

    Ok(TypeAlias {
        id: alias_id,
        name,
        type_params,
        ty,
        is_exported,
    })
}

pub(crate) fn lower_constructor(ctx: &mut LoweringContext, class_name: &str, ctor: &ast::Constructor) -> Result<Function> {
    let scope_mark = ctx.enter_scope();

    // Add 'this' as a special local
    let _this_id = ctx.define_local("this".to_string(), Type::Any);

    // Lower parameters with type extraction (using context for class type param resolution)
    let mut params = Vec::new();
    // Track TsParamProp params so we can synthesize `this.field = param` assignments
    let mut param_prop_assignments: Vec<(LocalId, String)> = Vec::new();
    for param in &ctor.params {
        match param {
            ast::ParamOrTsParamProp::Param(p) => {
                let param_name = get_pat_name(&p.pat)?;
                let param_type = extract_param_type_with_ctx(&p.pat, Some(ctx));
                let param_default = get_param_default(ctx, &p.pat)?;
                let is_rest = is_rest_param(&p.pat);
                let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_type,
                    default: param_default,
                    is_rest,
                });
            }
            ast::ParamOrTsParamProp::TsParamProp(ts_prop) => {
                // Handle parameter properties (e.g., constructor(public x: number))
                let (param_name, param_type) = match &ts_prop.param {
                    ast::TsParamPropParam::Ident(ident) => {
                        let name = ident.id.sym.to_string();
                        let ty = ident.type_ann.as_ref()
                            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                            .unwrap_or(Type::Any);
                        (name, ty)
                    }
                    ast::TsParamPropParam::Assign(assign) => {
                        let name = get_pat_name(&assign.left)?;
                        let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                        (name, ty)
                    }
                };
                let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                // Record this param for synthesizing `this.field = param` assignment
                param_prop_assignments.push((param_id, param_name.clone()));
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_type,
                    default: None,
                    is_rest: false, // TsParamProp cannot be a rest parameter
                });
            }
        }
    }

    // Lower body
    let mut body = if let Some(ref block) = ctor.body {
        lower_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Synthesize `this.field = param` assignments for parameter properties.
    // In TypeScript, `constructor(public name: string)` automatically assigns
    // `this.name = name` at the start of the constructor body.
    if !param_prop_assignments.is_empty() {
        let mut synthetic_stmts: Vec<Stmt> = Vec::new();
        for (param_id, field_name) in &param_prop_assignments {
            synthetic_stmts.push(Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: field_name.clone(),
                value: Box::new(Expr::LocalGet(*param_id)),
            }));
        }
        // Prepend synthetic assignments before the user-written constructor body
        synthetic_stmts.append(&mut body);
        body = synthetic_stmts;
    }

    ctx.exit_scope(scope_mark);

    Ok(Function {
        id: ctx.fresh_func(),
        name: format!("{}::constructor", class_name),
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub(crate) fn lower_class_method(ctx: &mut LoweringContext, method: &ast::ClassMethod) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        _ => return Err(anyhow!("Unsupported method key")),
    };

    // Lower decorators from the method's function
    let decorators = lower_decorators(ctx, &method.function.decorators);

    // Extract method-level type parameters (e.g., method<U>(x: U): T)
    // Note: Class-level type params are already in scope from lower_class_decl
    let type_params = method.function.type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter method's type param scope (nested inside class scope if applicable)
    ctx.enter_type_param_scope(&type_params);

    let scope_mark = ctx.enter_scope();

    // Add 'this' for instance methods
    if !method.is_static {
        ctx.define_local("this".to_string(), Type::Any);
    }

    // Lower parameters with type extraction (using context for type param resolution)
    let mut params = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_default = get_param_default(ctx, &param.pat)?;
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: param_default,
            is_rest,
        });
    }

    // Extract return type (with context)
    let return_type = method.function.return_type.as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Lower body
    let body = if let Some(ref block) = method.function.body {
        lower_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    ctx.exit_scope(scope_mark);

    // Exit method's type param scope
    ctx.exit_type_param_scope();

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params,
        params,
        return_type,
        body,
        is_async: method.function.is_async,
        is_generator: method.function.is_generator,
        is_exported: false,
        captures: Vec::new(),
        decorators,
    })
}

/// Lower a getter method (get propertyName(): Type { ... })
pub(crate) fn lower_getter_method(ctx: &mut LoweringContext, method: &ast::ClassMethod) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => format!("get_{}", ident.sym),
        ast::PropName::Str(s) => format!("get_{}", s.value.as_str().unwrap_or("")),
        _ => return Err(anyhow!("Unsupported getter key")),
    };

    let scope_mark = ctx.enter_scope();

    // Add 'this' for instance getters
    ctx.define_local("this".to_string(), Type::Any);

    // Getters have no parameters

    // Extract return type
    let return_type = method.function.return_type.as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Lower body
    let body = if let Some(ref block) = method.function.body {
        lower_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    ctx.exit_scope(scope_mark);

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params: Vec::new(),
        params: Vec::new(),
        return_type,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Lower a setter method (set propertyName(value: Type) { ... })
pub(crate) fn lower_setter_method(ctx: &mut LoweringContext, method: &ast::ClassMethod) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => format!("set_{}", ident.sym),
        ast::PropName::Str(s) => format!("set_{}", s.value.as_str().unwrap_or("")),
        _ => return Err(anyhow!("Unsupported setter key")),
    };

    let scope_mark = ctx.enter_scope();

    // Add 'this' for instance setters
    ctx.define_local("this".to_string(), Type::Any);

    // Setters have exactly one parameter
    let mut params = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: None,
            is_rest: false,
        });
    }

    // Lower body
    let body = if let Some(ref block) = method.function.body {
        lower_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    ctx.exit_scope(scope_mark);

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub(crate) fn lower_class_prop(ctx: &mut LoweringContext, prop: &ast::ClassProp) -> Result<ClassField> {
    let name = match &prop.key {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        _ => return Err(anyhow!("Unsupported property key")),
    };

    // Extract type from type annotation (using context for class type param resolution)
    let ty = prop.type_ann.as_ref()
        .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Lower initializer expression if present
    let init = prop.value.as_ref()
        .map(|e| lower_expr(ctx, e))
        .transpose()?;

    Ok(ClassField {
        name,
        ty,
        init,
        is_private: false, // TODO: check accessibility
        is_readonly: prop.readonly,
    })
}

pub(crate) fn lower_private_prop(ctx: &mut LoweringContext, prop: &ast::PrivateProp) -> Result<ClassField> {
    // Private fields use PrivateName which has a `name` field (without the # prefix in SWC)
    // We store the name with the # prefix to distinguish private fields
    let name = format!("#{}", prop.key.name.to_string());

    // Extract type from type annotation (using context for class type param resolution)
    let ty = prop.type_ann.as_ref()
        .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Lower initializer expression if present
    let init = prop.value.as_ref()
        .map(|e| lower_expr(ctx, e))
        .transpose()?;

    Ok(ClassField {
        name,
        ty,
        init,
        is_private: true,
        is_readonly: prop.readonly,
    })
}

pub(crate) fn lower_block_stmt(ctx: &mut LoweringContext, block: &ast::BlockStmt) -> Result<Vec<Stmt>> {
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        stmts.extend(lower_body_stmt(ctx, stmt)?);
    }
    Ok(stmts)
}

/// Lower a block statement that introduces its own lexical scope for
/// `let`/`const`. Inner bindings shadow outer ones and are removed on exit.
/// `var` declarations remain visible (function-scoped).
pub(crate) fn lower_block_stmt_scoped(ctx: &mut LoweringContext, block: &ast::BlockStmt) -> Result<Vec<Stmt>> {
    let mark = ctx.push_block_scope();
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        stmts.extend(lower_body_stmt(ctx, stmt)?);
    }
    ctx.pop_block_scope(mark);
    Ok(stmts)
}

pub(crate) fn lower_body_stmt(ctx: &mut LoweringContext, stmt: &ast::Stmt) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match stmt {
        ast::Stmt::Return(ret) => {
            let value = ret.arg.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            result.push(Stmt::Return(value));
        }
        ast::Stmt::If(if_stmt) => {
            let condition = lower_expr(ctx, &if_stmt.test)?;
            // Each branch introduces its own lexical scope for let/const.
            // Skip the extra push if the branch is already a BlockStmt (which
            // will push its own scope via lower_block_stmt_scoped), or another
            // If (else-if chain) which handles its own scoping.
            let then_branch = if matches!(*if_stmt.cons, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &if_stmt.cons)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &if_stmt.cons)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            let else_branch = if_stmt.alt.as_ref()
                .map(|s| {
                    if matches!(**s, ast::Stmt::Block(_)) || matches!(**s, ast::Stmt::If(_)) {
                        lower_body_stmt(ctx, s)
                    } else {
                        let mark = ctx.push_block_scope();
                        let stmts = lower_body_stmt(ctx, s);
                        ctx.pop_block_scope(mark);
                        stmts
                    }
                })
                .transpose()?;
            result.push(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }
        ast::Stmt::Block(block) => {
            // Bare block: introduce a lexical scope so let/const shadow
            // without leaking into the enclosing scope.
            result.extend(lower_block_stmt_scoped(ctx, block)?);
        }
        ast::Stmt::Expr(expr_stmt) => {
            // Desugar this.field.splice(...) to:
            //   let __temp = this.field;
            //   __temp.splice(...);
            //   this.field = __temp;
            // This avoids a codegen issue where calling js_array_splice directly
            // on a class field pointer corrupts the object memory.
            if let ast::Expr::Call(call) = expr_stmt.expr.as_ref() {
                if let ast::Callee::Expr(callee) = &call.callee {
                    if let ast::Expr::Member(member) = callee.as_ref() {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            if method_ident.sym.as_ref() == "splice" {
                                if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
                                    if let ast::Expr::This(_) = inner_member.obj.as_ref() {
                                        if let ast::MemberProp::Ident(field_ident) = &inner_member.prop {
                                            let field_name = field_ident.sym.to_string();
                                            // Create temp local
                                            let temp_id = ctx.fresh_local();
                                            let temp_name = format!("__splice_temp_{}", field_name);
                                            ctx.locals.push((temp_name.clone(), temp_id, Type::Array(Box::new(Type::Any))));

                                            // Stmt 1: let __temp = this.field;
                                            result.push(Stmt::Let {
                                                id: temp_id,
                                                name: temp_name.clone(),
                                                ty: Type::Array(Box::new(Type::Any)),
                                                mutable: true,
                                                init: Some(Expr::PropertyGet {
                                                    object: Box::new(Expr::This),
                                                    property: field_name.clone(),
                                                }),
                                            });

                                            // Stmt 2: __temp.splice(args...)
                                            let mut args_iter = call.args.iter()
                                                .map(|a| lower_expr(ctx, &a.expr))
                                                .collect::<Result<Vec<Expr>>>()?
                                                .into_iter();
                                            if let Some(start) = args_iter.next() {
                                                let delete_count = args_iter.next();
                                                let items: Vec<Expr> = args_iter.collect();
                                                result.push(Stmt::Expr(Expr::ArraySplice {
                                                    array_id: temp_id,
                                                    start: Box::new(start),
                                                    delete_count: delete_count.map(Box::new),
                                                    items,
                                                }));
                                            }

                                            // Stmt 3: this.field = __temp;
                                            result.push(Stmt::Expr(Expr::PropertySet {
                                                object: Box::new(Expr::This),
                                                property: field_name,
                                                value: Box::new(Expr::LocalGet(temp_id)),
                                            }));

                                            return Ok(result);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a destructuring assignment that needs special handling
            if let ast::Expr::Assign(assign) = expr_stmt.expr.as_ref() {
                if let ast::AssignTarget::Pat(pat) = &assign.left {
                    // This is a destructuring assignment at statement level
                    // We can emit proper Let statements for temporaries
                    let stmts = lower_destructuring_assignment_stmt(ctx, pat, &assign.right)?;
                    result.extend(stmts);
                    return Ok(result);
                }
            }
            let expr = lower_expr(ctx, &expr_stmt.expr)?;
            result.push(Stmt::Expr(expr));
        }
        ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
            let mutable = var_decl.kind != ast::VarDeclKind::Const;
            let is_var = var_decl.kind == ast::VarDeclKind::Var;
            for decl in &var_decl.decls {
                let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable)?;
                // `var` is function-scoped: mark each defined local so
                // `pop_block_scope` preserves it when leaving an inner block.
                if is_var {
                    for s in &stmts {
                        if let Stmt::Let { id, .. } = s {
                            ctx.var_hoisted_ids.insert(*id);
                        }
                    }
                }
                result.extend(stmts);
            }
        }
        ast::Stmt::Decl(ast::Decl::Class(class_decl)) => {
            // Class declared inside a function body (e.g., noble-curves' Point class)
            let class_name = class_decl.ident.sym.to_string();
            // Skip if a class with the same name already exists (avoids duplicate definitions
            // when the same class name appears at both module level and function body level)
            let already_exists = ctx.pending_classes.iter().any(|c| c.name == class_name)
                || ctx.classes_index.contains_key(&class_name);
            if !already_exists {
                let class = lower_class_decl(ctx, class_decl, false)?;
                ctx.pending_classes.push(class);
            }
        }
        ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
            // Inner function declarations are compiled as closures and assigned to local variables.
            if fn_decl.function.body.is_some() {
                let func_name = fn_decl.ident.sym.to_string();
                let func_id = ctx.fresh_func();
                let scope_mark = ctx.enter_scope();

                // Track outer locals for capture detection
                let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                    .map(|(name, id, _)| (name.clone(), *id))
                    .collect();

                // Lower parameters
                let mut params = Vec::new();
                let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
                for param in &fn_decl.function.params {
                    let param_name = get_pat_name(&param.pat)?;
                    let param_default = get_param_default(ctx, &param.pat)?;
                    let is_rest = is_rest_param(&param.pat);
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    params.push(Param {
                        id: param_id,
                        name: param_name,
                        ty: Type::Any,
                        default: param_default,
                        is_rest,
                    });
                    if is_destructuring_pattern(&param.pat) {
                        destructuring_params.push((param_id, param.pat.clone()));
                    }
                }

                // Generate destructuring stmts
                let mut destructuring_stmts = Vec::new();
                for (param_id, pat) in &destructuring_params {
                    let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
                    destructuring_stmts.extend(stmts);
                }

                // Lower body
                let mut body = if let Some(ref block) = fn_decl.function.body {
                    lower_block_stmt(ctx, block)?
                } else {
                    Vec::new()
                };

                if !destructuring_stmts.is_empty() {
                    let mut new_body = destructuring_stmts;
                    new_body.append(&mut body);
                    body = new_body;
                }

                ctx.exit_scope(scope_mark);

                // Detect captured variables
                let mut all_refs = Vec::new();
                let mut visited_closures = std::collections::HashSet::new();
                for stmt in &body {
                    collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
                }

                let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                    .map(|(_, id)| *id)
                    .collect();
                let param_ids: std::collections::HashSet<LocalId> = params.iter()
                    .map(|p| p.id)
                    .collect();

                let mut captures: Vec<LocalId> = all_refs.into_iter()
                    .filter(|id| outer_local_ids.contains(id) && !param_ids.contains(id))
                    .collect();
                captures.sort();
                captures.dedup();

                // Detect mutable captures
                let mut all_assigned = Vec::new();
                for stmt in &body {
                    collect_assigned_locals_stmt(stmt, &mut all_assigned);
                }
                let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
                let mutable_captures: Vec<LocalId> = captures.iter()
                    .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                    .copied()
                    .collect();

                let closure = Expr::Closure {
                    func_id,
                    params,
                    return_type: Type::Any,
                    body,
                    captures,
                    mutable_captures,
                    captures_this: false,
                    enclosing_class: None,
                    is_async: fn_decl.function.is_async,
                };

                // Define local variable and assign closure via Stmt::Let.
                // Use existing local if already pre-registered (function hoisting).
                let local_id = ctx.lookup_local(&func_name)
                    .unwrap_or_else(|| ctx.define_local(func_name.clone(), Type::Any));
                result.push(Stmt::Let {
                    id: local_id,
                    name: func_name,
                    ty: Type::Any,
                    init: Some(closure),
                    mutable: false,
                });
            }
        }
        ast::Stmt::While(while_stmt) => {
            let condition = lower_expr(ctx, &while_stmt.test)?;
            // While body introduces its own lexical scope.
            let body = if matches!(*while_stmt.body, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &while_stmt.body)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &while_stmt.body)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            result.push(Stmt::While { condition, body });
        }
        ast::Stmt::DoWhile(do_while_stmt) => {
            let body = lower_body_stmt(ctx, &do_while_stmt.body)?;
            let condition = lower_expr(ctx, &do_while_stmt.test)?;
            result.push(Stmt::DoWhile { body, condition });
        }
        ast::Stmt::Labeled(labeled_stmt) => {
            let label = labeled_stmt.label.sym.to_string();
            let inner = lower_body_stmt(ctx, &labeled_stmt.body)?;
            // If the body lowered to a single statement, wrap it directly.
            // Otherwise wrap the first statement (preserving any hoisted lets before it).
            if inner.len() == 1 {
                let body = inner.into_iter().next().unwrap();
                result.push(Stmt::Labeled { label, body: Box::new(body) });
            } else {
                // Multiple statements — take the last "real" loop/block as the labeled target,
                // and emit any preceding statements (e.g., hoisted lets from for-of/for-in desugar) first.
                let mut inner = inner;
                let last = inner.pop().unwrap();
                for s in inner {
                    result.push(s);
                }
                result.push(Stmt::Labeled { label, body: Box::new(last) });
            }
        }
        ast::Stmt::Break(break_stmt) => {
            if let Some(ref label) = break_stmt.label {
                result.push(Stmt::LabeledBreak(label.sym.to_string()));
            } else {
                result.push(Stmt::Break);
            }
        }
        ast::Stmt::Continue(continue_stmt) => {
            if let Some(ref label) = continue_stmt.label {
                result.push(Stmt::LabeledContinue(label.sym.to_string()));
            } else {
                result.push(Stmt::Continue);
            }
        }
        ast::Stmt::For(for_stmt) => {
            // Push a block scope covering init/test/update/body, so
            // `for (let i = 0; ...)` bindings don't leak to the enclosing scope.
            let for_scope_mark = ctx.push_block_scope();
            let init = if let Some(init) = &for_stmt.init {
                match init {
                    ast::VarDeclOrExpr::VarDecl(var_decl) => {
                        let is_var = var_decl.kind == ast::VarDeclKind::Var;
                        if is_var {
                            for decl in var_decl.decls.iter() {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                ctx.var_hoisted_ids.insert(id);
                                result.push(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr });
                            }
                            None
                        } else {
                            for decl in var_decl.decls.iter().skip(1) {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                result.push(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr });
                            }
                            if let Some(decl) = var_decl.decls.first() {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                Some(Box::new(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr }))
                            } else { None }
                        }
                    }
                    ast::VarDeclOrExpr::Expr(expr) => { Some(Box::new(Stmt::Expr(lower_expr(ctx, expr)?))) }
                }
            } else { None };
            let condition = for_stmt.test.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            let update = for_stmt.update.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            let body = lower_body_stmt(ctx, &for_stmt.body)?;
            ctx.pop_block_scope(for_scope_mark);
            result.push(Stmt::For { init, condition, update, body });
        }
        ast::Stmt::Try(try_stmt) => {
            // try body is its own lexical scope
            let body = lower_block_stmt_scoped(ctx, &try_stmt.block)?;

            // Lower catch clause (if present)
            let catch = if let Some(ref catch_clause) = try_stmt.handler {
                let scope_mark = ctx.enter_scope();

                // Lower catch parameter (if present)
                let param = if let Some(ref pat) = catch_clause.param {
                    let param_name = get_pat_name(pat)?;
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    Some((param_id, param_name))
                } else {
                    None
                };

                // Lower catch body
                let catch_body = lower_block_stmt(ctx, &catch_clause.body)?;

                ctx.exit_scope(scope_mark);

                Some(CatchClause { param, body: catch_body })
            } else {
                None
            };

            // finally block is its own lexical scope
            let finally = if let Some(ref finally_block) = try_stmt.finalizer {
                Some(lower_block_stmt_scoped(ctx, finally_block)?)
            } else {
                None
            };

            result.push(Stmt::Try { body, catch, finally });
        }
        ast::Stmt::Throw(throw_stmt) => {
            let expr = lower_expr(ctx, &throw_stmt.arg)?;
            result.push(Stmt::Throw(expr));
        }
        ast::Stmt::Switch(switch_stmt) => {
            let discriminant = lower_expr(ctx, &switch_stmt.discriminant)?;
            let mut cases = Vec::new();

            for case in &switch_stmt.cases {
                let test = case.test.as_ref()
                    .map(|e| lower_expr(ctx, e))
                    .transpose()?;

                let mut body = Vec::new();
                for stmt in &case.cons {
                    body.extend(lower_body_stmt(ctx, stmt)?);
                }

                cases.push(SwitchCase { test, body });
            }

            result.push(Stmt::Switch { discriminant, cases });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            // Desugar for-of to a regular for loop (same as in lower_stmt).
            // Push a block scope so loop variables and internal temporaries don't leak.
            let for_scope_mark = ctx.push_block_scope();
            let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;

            // If the iterable is a Map, wrap in MapEntries to convert to array
            let arr_expr = if let ast::Expr::Ident(ident) = &*for_of_stmt.right {
                let name = ident.sym.to_string();
                let is_map = ctx.lookup_local_type(&name)
                    .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                    .unwrap_or(false);
                if is_map {
                    Expr::MapEntries(Box::new(arr_expr))
                } else {
                    arr_expr
                }
            } else {
                arr_expr
            };

            let arr_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            ctx.locals.push((format!("__arr_{}", arr_id), arr_id, Type::Array(Box::new(Type::Any))));
            ctx.locals.push((format!("__idx_{}", idx_id), idx_id, Type::Number));

            // Store array reference
            result.push(Stmt::Let {
                id: arr_id,
                name: format!("__arr_{}", arr_id),
                ty: Type::Array(Box::new(Type::Any)),
                mutable: false,
                init: Some(arr_expr),
            });

            // IMPORTANT: Define iteration variables BEFORE lowering the body
            let item_id = ctx.fresh_local();
            ctx.locals.push((format!("__item_{}", item_id), item_id, Type::Any));

            // Pre-define all variables from the pattern
            let var_ids: Vec<(String, u32)> = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        match &decl.name {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                let id = ctx.define_local(name.clone(), Type::Any);
                                vec![(name, id)]
                            }
                            ast::Pat::Array(arr_pat) => {
                                let mut ids = Vec::new();
                                for elem in &arr_pat.elems {
                                    if let Some(elem_pat) = elem {
                                        if let ast::Pat::Ident(ident) = elem_pat {
                                            let name = ident.id.sym.to_string();
                                            let id = ctx.define_local(name.clone(), Type::Any);
                                            ids.push((name, id));
                                        }
                                    }
                                }
                                ids
                            }
                            ast::Pat::Object(obj_pat) => {
                                let mut ids = Vec::new();
                                for prop in &obj_pat.props {
                                    match prop {
                                        ast::ObjectPatProp::Assign(assign) => {
                                            let name = assign.key.sym.to_string();
                                            let id = ctx.define_local(name.clone(), Type::Any);
                                            ids.push((name, id));
                                        }
                                        ast::ObjectPatProp::KeyValue(kv) => {
                                            if let ast::Pat::Ident(ident) = &*kv.value {
                                                let name = ident.id.sym.to_string();
                                                let id = ctx.define_local(name.clone(), Type::Any);
                                                ids.push((name, id));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                ids
                            }
                            _ => {
                                let name = get_binding_name(&decl.name)?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                vec![(name, id)]
                            }
                        }
                    } else {
                        return Err(anyhow!("for-of requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(pat) => {
                    let name = get_pat_name(pat)?;
                    let id = ctx.define_local(name.clone(), Type::Any);
                    vec![(name, id)]
                }
                _ => return Err(anyhow!("Unsupported for-of left-hand side")),
            };

            // NOW lower the body
            let mut loop_body = lower_body_stmt(ctx, &for_of_stmt.body)?;

            // Build binding statements using pre-defined variable IDs
            let binding_stmts = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        let item_expr = Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        };

                        match &decl.name {
                            ast::Pat::Ident(_) => {
                                let (name, id) = var_ids[0].clone();
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }]
                            }
                            ast::Pat::Array(arr_pat) => {
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }];
                                let mut var_idx = 0;
                                for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                    if let Some(elem_pat) = elem {
                                        if let ast::Pat::Ident(_) = elem_pat {
                                            let (name, id) = var_ids[var_idx].clone();
                                            var_idx += 1;
                                            stmts.push(Stmt::Let {
                                                id,
                                                name,
                                                ty: Type::Any,
                                                mutable: false,
                                                init: Some(Expr::IndexGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    index: Box::new(Expr::Number(idx as f64)),
                                                }),
                                            });
                                        }
                                    }
                                }
                                stmts
                            }
                            ast::Pat::Object(obj_pat) => {
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }];
                                let mut var_idx = 0;
                                for prop in &obj_pat.props {
                                    match prop {
                                        ast::ObjectPatProp::Assign(assign) => {
                                            let prop_name = assign.key.sym.to_string();
                                            let (name, id) = var_ids[var_idx].clone();
                                            var_idx += 1;
                                            let init_value = if let Some(default_expr) = &assign.value {
                                                let prop_access = Expr::PropertyGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    property: prop_name,
                                                };
                                                let default_val = lower_expr(ctx, default_expr)?;
                                                let condition = Expr::Compare {
                                                    op: CompareOp::Ne,
                                                    left: Box::new(prop_access.clone()),
                                                    right: Box::new(Expr::Undefined),
                                                };
                                                Expr::Conditional {
                                                    condition: Box::new(condition),
                                                    then_expr: Box::new(prop_access),
                                                    else_expr: Box::new(default_val),
                                                }
                                            } else {
                                                Expr::PropertyGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    property: prop_name,
                                                }
                                            };
                                            stmts.push(Stmt::Let {
                                                id,
                                                name,
                                                ty: Type::Any,
                                                mutable: false,
                                                init: Some(init_value),
                                            });
                                        }
                                        ast::ObjectPatProp::KeyValue(kv) => {
                                            let key = match &kv.key {
                                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                _ => continue,
                                            };
                                            if let ast::Pat::Ident(_) = &*kv.value {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: Type::Any,
                                                    mutable: false,
                                                    init: Some(Expr::PropertyGet {
                                                        object: Box::new(Expr::LocalGet(item_id)),
                                                        property: key,
                                                    }),
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                stmts
                            }
                            _ => {
                                let (name, id) = var_ids[0].clone();
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(Expr::IndexGet {
                                        object: Box::new(Expr::LocalGet(arr_id)),
                                        index: Box::new(Expr::LocalGet(idx_id)),
                                    }),
                                }]
                            }
                        }
                    } else {
                        return Err(anyhow!("for-of requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(_) => {
                    let (name, id) = var_ids[0].clone();
                    vec![Stmt::Let {
                        id,
                        name,
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        }),
                    }]
                }
                _ => return Err(anyhow!("Unsupported for-of left-hand side")),
            };

            // Prepend the binding statements to the loop body
            for (i, stmt) in binding_stmts.into_iter().enumerate() {
                loop_body.insert(i, stmt);
            }

            // Create the for loop
            result.push(Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: idx_id,
                    name: format!("__idx_{}", idx_id),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(arr_id)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: idx_id,
                    op: UpdateOp::Increment,
                    prefix: true,
                }),
                body: loop_body,
            });
            ctx.pop_block_scope(for_scope_mark);
        }
        ast::Stmt::ForIn(for_in_stmt) => {
            // Desugar for-in to a for-of over Object.keys(obj) (same as in lower_stmt).
            // Push a block scope so loop variables don't leak.
            let for_scope_mark = ctx.push_block_scope();
            let key_name = match &for_in_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        get_binding_name(&decl.name)?
                    } else {
                        return Err(anyhow!("for-in requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(pat) => get_pat_name(pat)?,
                _ => return Err(anyhow!("Unsupported for-in left-hand side")),
            };

            let obj_expr = lower_expr(ctx, &for_in_stmt.right)?;
            let keys_expr = Expr::ObjectKeys(Box::new(obj_expr));
            let keys_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            let key_id = ctx.define_local(key_name.clone(), Type::String);

            // Store keys array reference
            result.push(Stmt::Let {
                id: keys_id,
                name: format!("__keys_{}", keys_id),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                init: Some(keys_expr),
            });

            // Lower the body and prepend key assignment
            let mut loop_body = lower_body_stmt(ctx, &for_in_stmt.body)?;
            loop_body.insert(0, Stmt::Let {
                id: key_id,
                name: key_name,
                ty: Type::String,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(keys_id)),
                    index: Box::new(Expr::LocalGet(idx_id)),
                }),
            });

            // Create the for loop
            result.push(Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: idx_id,
                    name: format!("__idx_{}", idx_id),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(keys_id)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: idx_id,
                    op: UpdateOp::Increment,
                    prefix: true,
                }),
                body: loop_body,
            });
            ctx.pop_block_scope(for_scope_mark);
        }
        _ => {
            // TODO: handle more statement types
        }
    }

    Ok(result)
}

/// Scan AST statements for `return <ident>` where the ident is a native instance.
/// Registers the containing function in `func_return_native_instances` so callers
/// can track `const db = initDb()` as returning a native handle.
fn find_native_return_in_stmts(
    stmts: &[ast::Stmt],
    ctx: &mut LoweringContext,
    func_name: &str,
    ni_start: usize,
) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Return(ret_stmt) => {
                if let Some(ref arg) = ret_stmt.arg {
                    if let ast::Expr::Ident(ident) = arg.as_ref() {
                        let var = ident.sym.as_ref();
                        for i in ni_start..ctx.native_instances.len() {
                            if ctx.native_instances[i].0 == var {
                                ctx.func_return_native_instances.push((
                                    func_name.to_string(),
                                    ctx.native_instances[i].1.clone(),
                                    ctx.native_instances[i].2.clone(),
                                ));
                                return;
                            }
                        }
                    }
                }
            }
            // Recurse into blocks that may contain returns
            ast::Stmt::Block(block) => {
                find_native_return_in_stmts(&block.stmts, ctx, func_name, ni_start);
            }
            ast::Stmt::If(if_stmt) => {
                if let ast::Stmt::Block(ref block) = *if_stmt.cons {
                    find_native_return_in_stmts(&block.stmts, ctx, func_name, ni_start);
                }
                if let Some(ref alt) = if_stmt.alt {
                    if let ast::Stmt::Block(ref block) = **alt {
                        find_native_return_in_stmts(&block.stmts, ctx, func_name, ni_start);
                    }
                }
            }
            ast::Stmt::Try(try_stmt) => {
                find_native_return_in_stmts(&try_stmt.block.stmts, ctx, func_name, ni_start);
                if let Some(ref handler) = try_stmt.handler {
                    find_native_return_in_stmts(&handler.body.stmts, ctx, func_name, ni_start);
                }
            }
            _ => {}
        }
        // Stop once registered (early return in Return arm handles the direct case;
        // check here for nested finds)
        if ctx.func_return_native_instances.iter().any(|(n, _, _)| n == func_name) {
            return;
        }
    }
}

