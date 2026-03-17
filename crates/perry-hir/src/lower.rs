//! AST to HIR lowering
//!
//! Converts SWC's TypeScript AST into our HIR representation.

use anyhow::{anyhow, Result};
use perry_types::{FuncId, GlobalId, LocalId, Type, TypeParam};
use swc_ecma_ast as ast;
use std::collections::HashSet;

use crate::ir::*;

/// Context for lowering, tracks variable bindings
pub struct LoweringContext {
    /// Counter for generating unique local IDs
    pub(crate) next_local_id: LocalId,
    /// Counter for generating unique global IDs
    pub(crate) next_global_id: GlobalId,
    /// Counter for generating unique function IDs
    pub(crate) next_func_id: FuncId,
    /// Counter for generating unique class IDs
    pub(crate) next_class_id: ClassId,
    /// Counter for generating unique enum IDs
    pub(crate) next_enum_id: EnumId,
    /// Counter for generating unique interface IDs
    pub(crate) next_interface_id: InterfaceId,
    /// Counter for generating unique type alias IDs
    pub(crate) next_type_alias_id: TypeAliasId,
    /// Current scope's local variables: name -> (id, type)
    pub(crate) locals: Vec<(String, LocalId, Type)>,
    /// Global variables: name -> (id, type)
    pub(crate) globals: Vec<(String, GlobalId, Type)>,
    /// Functions: name -> id
    pub(crate) functions: Vec<(String, FuncId)>,
    /// Function parameter defaults: func_id -> (defaults, param_local_ids)
    pub(crate) func_defaults: Vec<(FuncId, Vec<Option<Expr>>, Vec<LocalId>)>,
    /// Classes: name -> id
    pub(crate) classes: Vec<(String, ClassId)>,
    /// Static members of classes: class_name -> (static_field_names, static_method_names)
    pub(crate) class_statics: Vec<(String, Vec<String>, Vec<String>)>,
    /// Enums: name -> (id, members with values)
    pub(crate) enums: Vec<(String, EnumId, Vec<(String, EnumValue)>)>,
    /// Interfaces: name -> id
    pub(crate) interfaces: Vec<(String, InterfaceId)>,
    /// Type aliases: name -> (id, type_params, aliased_type)
    pub(crate) type_aliases: Vec<(String, TypeAliasId, Vec<TypeParam>, Type)>,
    /// Imported functions: local_name -> original_name (the exported name in the source module)
    pub(crate) imported_functions: Vec<(String, String)>,
    /// Native module imports: local_name -> (module_name, method_name)
    /// For namespace imports (import * as x), method_name is None
    /// For named imports (import { v4 as uuid }), method_name is Some("v4")
    pub(crate) native_modules: Vec<(String, String, Option<String>)>,
    /// Built-in module aliases from require(): local_name -> module_name (e.g., "myFs" -> "fs")
    pub(crate) builtin_module_aliases: Vec<(String, String)>,
    /// Stack of type parameter scopes (for nested generics)
    pub(crate) type_param_scopes: Vec<HashSet<String>>,
    /// Native class instances: local_name -> (module_name, class_name)
    /// Tracks variables that hold instances of native module classes (e.g., EventEmitter)
    pub(crate) native_instances: Vec<(String, String, String)>,
    /// Current class being lowered (for arrow function `this` capture)
    pub(crate) current_class: Option<String>,
    /// Extern function types: name -> (param_types, return_type)
    /// Stores type information for declare function statements (FFI)
    pub(crate) extern_func_types: Vec<(String, Vec<Type>, Type)>,
    /// Source file path (for import.meta.url)
    pub(crate) source_file_path: String,
    /// Variables that hold closures or other values needing cross-module export globals
    /// (arrow functions, object literals, call expressions, arrays, new expressions)
    pub(crate) exportable_object_vars: HashSet<String>,
    /// Functions created during expression lowering (e.g., object literal methods)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_functions: Vec<Function>,
    /// Functions that return native module instances: func_name -> (module_name, class_name)
    /// Tracks user-defined functions whose return type annotation is a native module type
    /// (e.g., initializePool(): mysql.Pool -> ("mysql2/promise", "Pool"))
    pub(crate) func_return_native_instances: Vec<(String, String, String)>,
    /// Classes created during expression lowering (e.g., class expressions in `new (class extends X {})()`)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_classes: Vec<Class>,
    /// Function return types: func_name -> return_type
    /// Tracks return types of user-defined functions for call-site type inference
    pub(crate) func_return_types: Vec<(String, Type)>,
    /// Resolved types from external type checker (tsgo): byte_position -> Type
    /// Populated before lowering when --type-check is enabled
    pub resolved_types: Option<std::collections::HashMap<u32, Type>>,
    /// Module-level variable names pre-registered in the forward-declaration pass.
    /// Used to avoid duplicate define_local calls when the actual declaration is lowered.
    pub(crate) pre_registered_module_vars: HashSet<String>,
    /// Namespace exported variables: (namespace_name, member_name, local_id)
    /// Used to resolve Namespace.member access to module-level LocalGet
    pub(crate) namespace_vars: Vec<(String, String, LocalId)>,
    /// Current namespace being lowered (for resolving internal function calls as StaticMethodCall)
    pub(crate) current_namespace: Option<String>,
    /// Module-level native instances that survive scope exits.
    /// Used for variables assigned from native calls inside functions (e.g., `mongoClient = await MongoClient.connect(uri)`).
    pub(crate) module_native_instances: Vec<(String, String, String)>,
    /// Whether this module uses fetch() — requires perry-stdlib
    pub(crate) uses_fetch: bool,
    pub(crate) var_hoisted_ids: HashSet<LocalId>,
}

impl LoweringContext {
    pub fn new(source_file_path: impl Into<String>) -> Self {
        Self::with_class_id_start(source_file_path, 1)
    }

    pub fn with_class_id_start(source_file_path: impl Into<String>, start_class_id: ClassId) -> Self {
        Self {
            next_local_id: 0,
            next_global_id: 0,
            next_func_id: 0,
            next_class_id: start_class_id, // Start from the provided ID to avoid collisions across modules
            next_enum_id: 0,
            next_interface_id: 0,
            next_type_alias_id: 0,
            locals: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            func_defaults: Vec::new(),
            classes: Vec::new(),
            class_statics: Vec::new(),
            enums: Vec::new(),
            interfaces: Vec::new(),
            type_aliases: Vec::new(),
            imported_functions: Vec::new(),
            native_modules: Vec::new(),
            builtin_module_aliases: Vec::new(),
            type_param_scopes: Vec::new(),
            native_instances: Vec::new(),
            current_class: None,
            extern_func_types: Vec::new(),
            source_file_path: source_file_path.into(),
            exportable_object_vars: HashSet::new(),
            pending_functions: Vec::new(),
            func_return_native_instances: Vec::new(),
            pending_classes: Vec::new(),
            func_return_types: Vec::new(),
            resolved_types: None,
            pre_registered_module_vars: HashSet::new(),
            namespace_vars: Vec::new(),
            current_namespace: None,
            module_native_instances: Vec::new(),
            uses_fetch: false,
            var_hoisted_ids: HashSet::new(),
        }
    }

    pub(crate) fn fresh_interface(&mut self) -> InterfaceId {
        let id = self.next_interface_id;
        self.next_interface_id += 1;
        id
    }

    pub(crate) fn fresh_type_alias(&mut self) -> TypeAliasId {
        let id = self.next_type_alias_id;
        self.next_type_alias_id += 1;
        id
    }

    /// Enter a new type parameter scope (for generic function/class)
    pub(crate) fn enter_type_param_scope(&mut self, type_params: &[TypeParam]) {
        let scope: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        self.type_param_scopes.push(scope);
    }

    /// Exit the current type parameter scope
    pub(crate) fn exit_type_param_scope(&mut self) {
        self.type_param_scopes.pop();
    }

    /// Check if a name is a type parameter in the current scope
    pub(crate) fn is_type_param(&self, name: &str) -> bool {
        self.type_param_scopes.iter().any(|scope| scope.contains(name))
    }

    pub(crate) fn fresh_local(&mut self) -> LocalId {
        let id = self.next_local_id;
        self.next_local_id += 1;
        id
    }

    pub(crate) fn fresh_global(&mut self) -> GlobalId {
        let id = self.next_global_id;
        self.next_global_id += 1;
        id
    }

    pub(crate) fn fresh_func(&mut self) -> FuncId {
        let id = self.next_func_id;
        self.next_func_id += 1;
        id
    }

    pub(crate) fn fresh_class(&mut self) -> ClassId {
        let id = self.next_class_id;
        self.next_class_id += 1;
        id
    }

    pub(crate) fn fresh_enum(&mut self) -> EnumId {
        let id = self.next_enum_id;
        self.next_enum_id += 1;
        id
    }

    pub(crate) fn lookup_class(&self, name: &str) -> Option<ClassId> {
        self.classes.iter().find(|(n, _)| n == name).map(|(_, id)| *id)
    }

    pub(crate) fn register_class_statics(&mut self, class_name: String, static_fields: Vec<String>, static_methods: Vec<String>) {
        self.class_statics.push((class_name, static_fields, static_methods));
    }

    pub(crate) fn has_static_field(&self, class_name: &str, field_name: &str) -> bool {
        self.class_statics.iter()
            .find(|(cn, _, _)| cn == class_name)
            .map(|(_, fields, _)| fields.contains(&field_name.to_string()))
            .unwrap_or(false)
    }

    pub(crate) fn has_static_method(&self, class_name: &str, method_name: &str) -> bool {
        self.class_statics.iter()
            .find(|(cn, _, _)| cn == class_name)
            .map(|(_, _, methods)| methods.contains(&method_name.to_string()))
            .unwrap_or(false)
    }

    pub(crate) fn lookup_namespace_var(&self, ns_name: &str, member_name: &str) -> Option<LocalId> {
        self.namespace_vars.iter()
            .find(|(ns, member, _)| ns == ns_name && member == member_name)
            .map(|(_, _, id)| *id)
    }

    pub(crate) fn define_enum(&mut self, name: String, id: EnumId, members: Vec<(String, EnumValue)>) {
        self.enums.push((name, id, members));
    }

    pub(crate) fn lookup_enum(&self, name: &str) -> Option<(EnumId, &[(String, EnumValue)])> {
        self.enums.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, id, members)| (*id, members.as_slice()))
    }

    pub(crate) fn lookup_enum_member(&self, enum_name: &str, member_name: &str) -> Option<&EnumValue> {
        self.enums.iter()
            .find(|(n, _, _)| n == enum_name)
            .and_then(|(_, _, members)| {
                members.iter()
                    .find(|(m, _)| m == member_name)
                    .map(|(_, v)| v)
            })
    }

    pub(crate) fn define_local(&mut self, name: String, ty: Type) -> LocalId {
        let id = self.fresh_local();
        self.locals.push((name, id, ty));
        id
    }

    pub(crate) fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.locals.iter().rev().find(|(n, _, _)| n == name).map(|(_, id, _)| *id)
    }

    pub(crate) fn lookup_local_type(&self, name: &str) -> Option<&Type> {
        self.locals.iter().rev().find(|(n, _, _)| n == name).map(|(_, _, ty)| ty)
    }

    pub(crate) fn lookup_func(&self, name: &str) -> Option<FuncId> {
        // Reverse search so inner-scope functions shadow outer-scope same-name functions
        self.functions.iter().rev().find(|(n, _)| n == name).map(|(_, id)| *id)
    }

    pub(crate) fn lookup_func_name(&self, func_id: FuncId) -> Option<&str> {
        self.functions.iter().find(|(_, id)| *id == func_id).map(|(name, _)| name.as_str())
    }

    pub(crate) fn lookup_func_defaults(&self, func_id: FuncId) -> Option<(&[Option<Expr>], &[LocalId])> {
        self.func_defaults.iter()
            .find(|(id, _, _)| *id == func_id)
            .map(|(_, defaults, param_ids)| (defaults.as_slice(), param_ids.as_slice()))
    }

    /// Substitute parameter references in a default expression.
    /// Replaces LocalGet(callee_param_id) with the corresponding caller argument expression.
    pub(crate) fn substitute_param_refs_in_default(expr: &Expr, param_map: &[(LocalId, Expr)]) -> Expr {
        match expr {
            Expr::LocalGet(id) => {
                // Check if this LocalGet references one of the callee's parameters
                for (param_id, replacement) in param_map {
                    if id == param_id {
                        return replacement.clone();
                    }
                }
                // Not a parameter reference - keep as-is
                expr.clone()
            }
            Expr::Array(elements) => {
                Expr::Array(elements.iter().map(|e| Self::substitute_param_refs_in_default(e, param_map)).collect())
            }
            Expr::Object(fields) => {
                Expr::Object(fields.iter().map(|(k, v)| (k.clone(), Self::substitute_param_refs_in_default(v, param_map))).collect())
            }
            Expr::Binary { op, left, right } => {
                Expr::Binary {
                    op: *op,
                    left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                    right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
                }
            }
            Expr::Compare { op, left, right } => {
                Expr::Compare {
                    op: *op,
                    left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                    right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
                }
            }
            Expr::Logical { op, left, right } => {
                Expr::Logical {
                    op: *op,
                    left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                    right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
                }
            }
            Expr::Unary { op, operand } => {
                Expr::Unary {
                    op: *op,
                    operand: Box::new(Self::substitute_param_refs_in_default(operand, param_map)),
                }
            }
            Expr::Call { callee, args, type_args } => {
                Expr::Call {
                    callee: Box::new(Self::substitute_param_refs_in_default(callee, param_map)),
                    args: args.iter().map(|a| Self::substitute_param_refs_in_default(a, param_map)).collect(),
                    type_args: type_args.clone(),
                }
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                Expr::Conditional {
                    condition: Box::new(Self::substitute_param_refs_in_default(condition, param_map)),
                    then_expr: Box::new(Self::substitute_param_refs_in_default(then_expr, param_map)),
                    else_expr: Box::new(Self::substitute_param_refs_in_default(else_expr, param_map)),
                }
            }
            Expr::PropertyGet { object, property } => {
                Expr::PropertyGet {
                    object: Box::new(Self::substitute_param_refs_in_default(object, param_map)),
                    property: property.clone(),
                }
            }
            Expr::IndexGet { object, index } => {
                Expr::IndexGet {
                    object: Box::new(Self::substitute_param_refs_in_default(object, param_map)),
                    index: Box::new(Self::substitute_param_refs_in_default(index, param_map)),
                }
            }
            Expr::New { class_name, args, type_args } => {
                Expr::New {
                    class_name: class_name.clone(),
                    args: args.iter().map(|a| Self::substitute_param_refs_in_default(a, param_map)).collect(),
                    type_args: type_args.clone(),
                }
            }
            // Leaf expressions that don't contain LocalGet - return as-is
            _ => expr.clone(),
        }
    }

    pub(crate) fn lookup_imported_func(&self, name: &str) -> Option<&str> {
        self.imported_functions.iter().find(|(n, _)| n == name).map(|(_, orig)| orig.as_str())
    }

    pub(crate) fn register_imported_func(&mut self, local_name: String, original_name: String) {
        self.imported_functions.push((local_name, original_name));
    }

    pub(crate) fn register_extern_func_types(&mut self, name: String, param_types: Vec<Type>, return_type: Type) {
        self.extern_func_types.push((name, param_types, return_type));
    }

    pub(crate) fn lookup_extern_func_types(&self, name: &str) -> Option<(&Vec<Type>, &Type)> {
        self.extern_func_types
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, params, ret)| (params, ret))
    }

    pub(crate) fn register_native_module(&mut self, local_name: String, module_name: String, method_name: Option<String>) {
        self.native_modules.push((local_name, module_name, method_name));
    }

    pub(crate) fn lookup_native_module(&self, name: &str) -> Option<(&str, Option<&str>)> {
        self.native_modules.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, m, method)| (m.as_str(), method.as_ref().map(|s| s.as_str())))
    }

    pub(crate) fn register_builtin_module_alias(&mut self, local_name: String, module_name: String) {
        self.builtin_module_aliases.push((local_name, module_name));
    }

    pub(crate) fn lookup_builtin_module_alias(&self, name: &str) -> Option<&str> {
        self.builtin_module_aliases.iter().find(|(n, _)| n == name).map(|(_, m)| m.as_str())
    }

    pub(crate) fn register_native_instance(&mut self, local_name: String, module_name: String, class_name: String) {
        self.native_instances.push((local_name, module_name, class_name));
    }

    pub(crate) fn lookup_native_instance(&self, name: &str) -> Option<(&str, &str)> {
        // Check scoped instances first (function-local variables)
        self.native_instances.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
            .or_else(|| {
                // Check module-level instances (survive scope exits)
                self.module_native_instances.iter()
                    .find(|(n, _, _)| n == name)
                    .map(|(_, module, class)| (module.as_str(), class.as_str()))
            })
    }

    pub(crate) fn lookup_func_return_native_instance(&self, func_name: &str) -> Option<(&str, &str)> {
        self.func_return_native_instances.iter()
            .find(|(n, _, _)| n == func_name)
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
    }

    pub(crate) fn register_func_return_type(&mut self, name: String, ty: Type) {
        self.func_return_types.push((name, ty));
    }

    pub(crate) fn lookup_func_return_type(&self, name: &str) -> Option<&Type> {
        self.func_return_types.iter().rev()
            .find(|(n, _)| n == name)
            .map(|(_, ty)| ty)
    }

    pub(crate) fn enter_scope(&self) -> (usize, usize, usize) {
        (self.locals.len(), self.native_instances.len(), self.functions.len())
    }

    pub(crate) fn exit_scope(&mut self, mark: (usize, usize, usize)) {
        self.locals.truncate(mark.0);
        self.native_instances.truncate(mark.1);
        self.functions.truncate(mark.2);
    }

}

// Re-export extracted module functions
pub(crate) use crate::lower_types::*;
pub(crate) use crate::lower_patterns::*;
pub(crate) use crate::destructuring::*;
pub(crate) use crate::lower_decl::*;
pub(crate) use crate::analysis::*;
pub(crate) use crate::jsx::*;

pub fn lower_module(ast_module: &ast::Module, name: &str, source_file_path: &str) -> Result<Module> {
    lower_module_with_class_id(ast_module, name, source_file_path, 1).map(|(module, _)| module)
}

pub fn lower_module_with_class_id(ast_module: &ast::Module, name: &str, source_file_path: &str, start_class_id: ClassId) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_and_types(ast_module, name, source_file_path, start_class_id, None)
}

pub fn lower_module_with_class_id_and_types(ast_module: &ast::Module, name: &str, source_file_path: &str, start_class_id: ClassId, resolved_types: Option<std::collections::HashMap<u32, Type>>) -> Result<(Module, ClassId)> {
    let mut ctx = LoweringContext::with_class_id_start(source_file_path, start_class_id);
    ctx.resolved_types = resolved_types;
    let mut module = Module::new(name);

    // For .tsx files, pre-register JSX runtime symbols so JSX expressions can be lowered.
    // This injects an automatic import of { jsx, jsxs } from "react/jsx-runtime"
    // (remapped to perry-react via the user's packageAliases).
    // Fragment is NOT imported — it's inlined as the string "__Fragment" directly in JSX lowering.
    if source_file_path.ends_with(".tsx") {
        ctx.register_imported_func("__jsx".to_string(), "jsx".to_string());
        ctx.register_imported_func("__jsxs".to_string(), "jsxs".to_string());
        module.imports.push(Import {
            source: "react/jsx-runtime".to_string(),
            specifiers: vec![
                ImportSpecifier::Named { local: "__jsx".to_string(), imported: "jsx".to_string() },
                ImportSpecifier::Named { local: "__jsxs".to_string(), imported: "jsxs".to_string() },
            ],
            is_native: false,
            module_kind: ModuleKind::NativeCompiled,
            resolved_path: None,
        });
    }

    // Pre-scan: Find all function names that have implementations (bodies)
    // This is needed to properly handle TypeScript function overloads where
    // multiple signature-only declarations precede a single implementation
    let mut functions_with_bodies: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in &ast_module.body {
        let fn_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => Some(fn_decl),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Fn(fn_decl) = &export_decl.decl {
                    Some(fn_decl)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(fn_decl) = fn_decl {
            if fn_decl.function.body.is_some() {
                functions_with_bodies.insert(fn_decl.ident.sym.to_string());
            }
        }
    }

    // First pass: collect all function declarations (both exported and non-exported)
    // Skip 'declare function' statements (functions with no body) - they are external FFI
    // BUT: also skip overload signatures if an implementation exists
    for item in &ast_module.body {
        // Extract function declaration from both regular statements and export declarations
        let fn_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => Some(fn_decl),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Fn(fn_decl) = &export_decl.decl {
                    Some(fn_decl)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(fn_decl) = fn_decl {
            let func_name = fn_decl.ident.sym.to_string();

            // Skip signature-only declarations (no body)
            if fn_decl.function.body.is_none() {
                // If this function has an implementation elsewhere, skip the signature
                // (it's a TypeScript overload, not an external FFI declaration)
                if functions_with_bodies.contains(&func_name) {
                    continue;
                }

                // No implementation exists - treat as external FFI declaration
                // Extract parameter types for FFI signature
                let param_types: Vec<Type> = fn_decl.function.params.iter()
                    .map(|param| extract_param_type_with_ctx(&param.pat, None))
                    .collect();

                // Extract return type
                let return_type = fn_decl.function.return_type.as_ref()
                    .map(|rt| extract_ts_type(&rt.type_ann))
                    .unwrap_or(Type::Void);

                // Register as external function so calls resolve to ExternFuncRef
                ctx.register_imported_func(func_name.clone(), func_name.clone());
                // Also store type information for code generation
                ctx.register_extern_func_types(func_name, param_types, return_type);
                continue;
            }

            // Function has a body - each declaration gets a unique FuncId
            // (inner-scope functions shadow outer-scope same-name functions via reverse lookup)
            let func_id = ctx.fresh_func();
            ctx.functions.push((func_name.clone(), func_id));

            // Pre-register return type annotation for call-site type inference
            // (so variables initialized from function calls can infer their type)
            if let Some(rt) = &fn_decl.function.return_type {
                let return_type = extract_ts_type(&rt.type_ann);
                if !matches!(return_type, Type::Any) {
                    ctx.register_func_return_type(func_name, return_type);
                }
            }
        }
    }

    // Pre-register module-level variable declarations so function bodies
    // declared before the variable can still reference them via lookup_local
    for item in &ast_module.body {
        let var_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(v))) => Some(v),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(v) = &export_decl.decl {
                    Some(v)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(var_decl) = var_decl {
            for decl in &var_decl.decls {
                if let ast::Pat::Ident(ident) = &decl.name {
                    let name = ident.id.sym.to_string();
                    if ctx.lookup_local(&name).is_none() {
                        let ty = ident.type_ann.as_ref()
                            .map(|ann| extract_ts_type(&ann.type_ann))
                            .unwrap_or(Type::Any);
                        ctx.define_local(name.clone(), ty);
                        ctx.pre_registered_module_vars.insert(name);
                    }
                }
            }
        }
    }

    // Pre-register all class declarations so that static method calls between
    // classes declared in the same file resolve correctly regardless of declaration order.
    // Without this, SqrtPriceMath.getAmount0Delta calling FullMath.mulDivRoundingUp
    // fails if FullMath is declared after SqrtPriceMath.
    for item in &ast_module.body {
        let class_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Class(cd))) => Some(cd),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Class(cd) = &export_decl.decl {
                    Some(cd)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(cd) = class_decl {
            let name = cd.ident.sym.to_string();
            if ctx.lookup_class(&name).is_none() {
                let id = ctx.fresh_class();
                ctx.classes.push((name.clone(), id));
            }
            // Collect static field/method names
            let mut static_field_names = Vec::new();
            let mut static_method_names = Vec::new();
            for member in &cd.class.body {
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
            if !static_field_names.is_empty() || !static_method_names.is_empty() {
                // Only register if not already registered (lower_class_decl will re-register)
                if !ctx.class_statics.iter().any(|(cn, _, _)| cn == &name) {
                    ctx.register_class_statics(name, static_field_names, static_method_names);
                }
            }
        }
    }

    // Main pass: lower everything
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => {
                lower_stmt(&mut ctx, &mut module, stmt)?;
            }
            ast::ModuleItem::ModuleDecl(decl) => {
                lower_module_decl(&mut ctx, &mut module, decl)?;
            }
        }
        // Flush any pending functions created during expression lowering
        // (e.g., inline methods in object literals)
        for func in ctx.pending_functions.drain(..) {
            module.functions.push(func);
        }
        // Flush any pending classes created during expression lowering
        // (e.g., class expressions in `new (class extends Command { ... })()`)
        for class in ctx.pending_classes.drain(..) {
            module.classes.push(class);
        }
    }

    // Populate exported_native_instances by matching native_instances with exports
    for (local_name, module_name, class_name) in &ctx.native_instances {
        // Check if this native instance is exported
        for export in &module.exports {
            if let Export::Named { local, exported } = export {
                if local == local_name {
                    module.exported_native_instances.push((
                        exported.clone(),
                        module_name.clone(),
                        class_name.clone(),
                    ));
                }
            }
        }
    }

    // Populate exported_func_return_native_instances for functions that return native instances
    for (func_name, native_module, native_class) in &ctx.func_return_native_instances {
        // Check if this function is directly exported
        let is_exported = module.functions.iter().any(|f| f.name == *func_name && f.is_exported);
        if is_exported {
            module.exported_func_return_native_instances.push((
                func_name.clone(),
                native_module.clone(),
                native_class.clone(),
            ));
        } else {
            // Also check named exports (e.g., `export { getRedis }`)
            for export in &module.exports {
                if let Export::Named { local, exported } = export {
                    if local == func_name {
                        module.exported_func_return_native_instances.push((
                            exported.clone(),
                            native_module.clone(),
                            native_class.clone(),
                        ));
                    }
                }
            }
        }
    }

    module.uses_fetch = ctx.uses_fetch;
    Ok((module, ctx.next_class_id))
}

fn lower_module_decl(
    ctx: &mut LoweringContext,
    module: &mut Module,
    decl: &ast::ModuleDecl,
) -> Result<()> {
    match decl {
        ast::ModuleDecl::Import(import_decl) => {
            // Skip type-only imports (import type { ... } from '...') - they have no runtime value
            if import_decl.type_only {
                return Ok(());
            }

            // Get the source module path
            let raw_source = import_decl.src.value.as_str().unwrap_or("").to_string();
            // Normalize "node:" prefix (e.g., "node:async_hooks" -> "async_hooks")
            let source = raw_source.strip_prefix("node:").unwrap_or(&raw_source).to_string();

            // Check if this is a native module import
            let is_native = is_native_module(&source);

            // Parse import specifiers
            let mut specifiers = Vec::new();
            for spec in &import_decl.specifiers {
                match spec {
                    ast::ImportSpecifier::Named(named) => {
                        // Skip individual type-only specifiers (import { type Foo, Bar })
                        if named.is_type_only { continue; }
                        let local = named.local.sym.to_string();
                        let imported = named.imported
                            .as_ref()
                            .map(|i| match i {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            })
                            .unwrap_or_else(|| local.clone());
                        if is_native {
                            // Register as native module function with the original method name
                            // e.g., import { v4 as uuid } from 'uuid' -> uuid maps to uuid.v4
                            ctx.register_native_module(local.clone(), source.clone(), Some(imported.clone()));
                            // Auto-register parentPort from worker_threads as a native instance
                            // (it's a singleton, not created via `new`)
                            if source == "worker_threads" && imported == "parentPort" {
                                ctx.register_native_instance(local.clone(), "worker_threads".to_string(), "MessagePort".to_string());
                            }
                        } else {
                            // Register as imported function (we assume all imports are functions for now)
                            ctx.register_imported_func(local.clone(), imported.clone());
                        }
                        specifiers.push(ImportSpecifier::Named { imported, local });
                    }
                    ast::ImportSpecifier::Default(default) => {
                        let local = default.local.sym.to_string();
                        if is_native {
                            // Default import of native module (e.g., import mysql from 'mysql2/promise')
                            // Default exports don't have a method name
                            ctx.register_native_module(local.clone(), source.clone(), None);
                        } else {
                            // Default import from JS module - register so calls resolve to ExternFuncRef
                            // Use "default" as the original name since default imports map to the "default" export
                            ctx.register_imported_func(local.clone(), "default".to_string());
                        }
                        specifiers.push(ImportSpecifier::Default { local });
                    }
                    ast::ImportSpecifier::Namespace(ns) => {
                        let local = ns.local.sym.to_string();
                        if is_native {
                            // Namespace import of native module (e.g., import * as mysql from 'mysql2')
                            // Methods are called via the namespace, so no specific method name
                            ctx.register_native_module(local.clone(), source.clone(), None);
                            // Also register as builtin module alias so method-level
                            // recognition works (child_process, fs, os, etc.)
                            ctx.register_builtin_module_alias(local.clone(), source.clone());
                        } else {
                            // Namespace import from JS module - register so calls resolve to ExternFuncRef
                            ctx.register_imported_func(local.clone(), local.clone());
                        }
                        specifiers.push(ImportSpecifier::Namespace { local });
                    }
                }
            }

            // Determine module kind based on the source and whether it's native
            let module_kind = if is_native {
                ModuleKind::NativeRust
            } else {
                // Default to NativeCompiled - the compiler driver will update this
                // based on file resolution
                ModuleKind::NativeCompiled
            };

            module.imports.push(Import {
                source,
                specifiers,
                is_native,
                module_kind,
                resolved_path: None, // Will be set by compiler driver during module resolution
            });
        }
        ast::ModuleDecl::ExportDecl(export) => {
            match &export.decl {
                ast::Decl::Fn(fn_decl) => {
                    let mut func = lower_fn_decl(ctx, fn_decl)?;
                    func.is_exported = true;
                    let func_name = func.name.clone();
                    let func_id = func.id;
                    // Register return type for call-site inference
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func_name.clone(), func.return_type.clone());
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> = func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    ctx.func_defaults.push((func.id, defaults, param_ids));
                    module.functions.push(func);
                    // Track in exports
                    module.exports.push(Export::Named {
                        local: func_name.clone(),
                        exported: func_name.clone(),
                    });
                    // Track exported function for cross-module value passing
                    module.exported_functions.push((func_name, func_id));
                }
                ast::Decl::Var(var_decl) => {
                    // Handle exported variables
                    for decl in &var_decl.decls {
                        let name = get_binding_name(&decl.name)?;
                        let ty = extract_binding_type(&decl.name);
                        if let Some(init) = &decl.init {
                            // Check if this is a native class instantiation and register it
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name = class_ident.sym.as_ref();
                                    // Map class names to their modules
                                    let module_name = match class_name {
                                        "EventEmitter" => Some("events"),
                                        "AsyncLocalStorage" => Some("async_hooks"),
                                        "WebSocket" | "WebSocketServer" => Some("ws"),
                                        "Redis" => Some("ioredis"),
                                        "LRUCache" => Some("lru-cache"),
                                        "Command" => Some("commander"),
                                        "Big" => Some("big.js"),
                                        "Decimal" => Some("decimal.js"),
                                        "BigNumber" => Some("bignumber.js"),
                                        // Database clients
                                        "Pool" => Some("pg"),
                                        "Client" => Some("pg"),
                                        "MongoClient" => Some("mongodb"),
                                        _ => None,
                                    };
                                    if let Some(native_module) = module_name {
                                        ctx.register_native_instance(name.clone(), native_module.to_string(), class_name.to_string());
                                    }
                                }
                            }

                            // Check if this is an awaited native class instantiation (e.g., await new Redis())
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::New(new_expr) = await_expr.arg.as_ref() {
                                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                        let class_name = class_ident.sym.as_ref();
                                        // Map class names to their modules
                                        let module_name = match class_name {
                                            "EventEmitter" => Some("events"),
                                            "AsyncLocalStorage" => Some("async_hooks"),
                                            "WebSocket" | "WebSocketServer" => Some("ws"),
                                            "Redis" => Some("ioredis"),
                                            "LRUCache" => Some("lru-cache"),
                                            "Command" => Some("commander"),
                                            "Big" => Some("big.js"),
                                            "Decimal" => Some("decimal.js"),
                                            "BigNumber" => Some("bignumber.js"),
                                            // Database clients
                                            "Pool" => Some("pg"),
                                            "Client" => Some("pg"),
                                            "MongoClient" => Some("mongodb"),
                                            _ => None,
                                        };
                                        if let Some(native_module) = module_name {
                                            ctx.register_native_instance(name.clone(), native_module.to_string(), class_name.to_string());
                                        }
                                    }
                                }
                            }

                            // Check if this is a native module factory function call (e.g., mysql.createPool())
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let ast::Callee::Expr(callee) = &call_expr.callee {
                                    if let ast::Expr::Member(member) = callee.as_ref() {
                                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                            let obj_name = obj_ident.sym.as_ref();
                                            // Check if it's a known native module
                                            if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                    let method_name = method_ident.sym.as_ref();
                                                    // Map factory functions to their class names
                                                    let class_name = match (module_name, method_name) {
                                                        ("mysql2" | "mysql2/promise", "createPool") => Some("Pool"),
                                                        ("mysql2" | "mysql2/promise", "createConnection") => Some("Connection"),
                                                        ("pg", "connect") => Some("Client"),
                                                        ("http" | "https", "request" | "get") => Some("ClientRequest"),
                                                        _ => None,
                                                    };
                                                    if let Some(class_name) = class_name {
                                                        ctx.register_native_instance(name.clone(), module_name.to_string(), class_name.to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Check if this is a direct call to a default import from a native module
                                    // e.g., Fastify() where Fastify is imported from 'fastify'
                                    if let ast::Expr::Ident(func_ident) = callee.as_ref() {
                                        let func_name = func_ident.sym.as_ref();
                                        // Check if this is a default import from a native module
                                        if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                                            // Register as native instance - the "class" is the module name for default exports
                                            ctx.register_native_instance(name.clone(), module_name.to_string(), "App".to_string());
                                        }
                                        // Check if this is a named import that returns a handle (e.g., State from perry/ui)
                                        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
                                            if module_name == "perry/ui" {
                                                match method_name {
                                                    "State" | "Sheet" | "Toolbar" | "Window" | "LazyVStack"
                                                    | "NavigationStack" | "Picker" | "Table" | "TabBar" => {
                                                        ctx.register_native_instance(name.clone(), module_name.to_string(), method_name.to_string());
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is an awaited factory call (e.g., const client = await MongoClient.connect(uri))
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                                let obj_name = obj_ident.sym.as_ref();
                                                if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                        let class_name = match (module_name, method_ident.sym.as_ref()) {
                                                            ("mongodb", "connect") => Some("MongoClient"),
                                                            ("mysql2" | "mysql2/promise", "createPool") => Some("Pool"),
                                                            ("mysql2" | "mysql2/promise", "createConnection") => Some("Connection"),
                                                            ("pg", "connect") => Some("Client"),
                                                            ("http" | "https", "request" | "get") => Some("ClientRequest"),
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = class_name {
                                                            ctx.register_native_instance(name.clone(), module_name.to_string(), class_name.to_string());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is a `new NativeClass(...)` expression
                            // e.g., const db = new Database('mango.db') where Database is from better-sqlite3
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name_str = class_ident.sym.as_ref();
                                    // Check if this class comes from a native module import
                                    let native_info = ctx.lookup_native_module(class_name_str)
                                        .map(|(m, _)| m.to_string());
                                    if let Some(module_name) = native_info {
                                        ctx.register_native_instance(name.clone(), module_name.clone(), class_name_str.to_string());
                                        ctx.module_native_instances.push((name.clone(), module_name, class_name_str.to_string()));
                                    }
                                }
                            }

                            // Check if this is a method call on a registered native instance (chaining).
                            // e.g., const db = client.db(name) where client is a mongodb native instance.
                            {
                                // Unwrap await if present
                                let actual_init = if let ast::Expr::Await(await_expr) = init.as_ref() {
                                    await_expr.arg.as_ref()
                                } else {
                                    init.as_ref()
                                };
                                if let ast::Expr::Call(call_expr) = actual_init {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                                let obj_name = obj_ident.sym.to_string();
                                                if let Some((module_name, _class)) = ctx.lookup_native_instance(&obj_name)
                                                    .map(|(m, c)| (m.to_string(), c.to_string()))
                                                {
                                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                        let method_name = method_ident.sym.as_ref();
                                                        // Determine if the method returns a handle (another native instance)
                                                        let returns_handle = match (module_name.as_str(), method_name) {
                                                            ("mongodb", "db") => Some("Database"),
                                                            ("mongodb", "collection") => Some("Collection"),
                                                            ("mysql2" | "mysql2/promise", "getConnection") => Some("PoolConnection"),
                                                            ("better-sqlite3", "prepare") => Some("Statement"),
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = returns_handle {
                                                            ctx.register_native_instance(name.clone(), module_name, class_name.to_string());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is an arrow function with a native return type
                            // e.g., export const getRedis = async (): Promise<Redis> => { ... }
                            if let ast::Expr::Arrow(arrow) = init.as_ref() {
                                if let Some(ref rt) = arrow.return_type {
                                    let return_type = extract_ts_type_with_ctx(&rt.type_ann, Some(ctx));
                                    // Unwrap Promise<T> for async functions
                                    let check_type = match &return_type {
                                        Type::Generic { base, type_args } if base == "Promise" => {
                                            type_args.first().unwrap_or(&return_type)
                                        }
                                        Type::Promise(inner) => inner.as_ref(),
                                        other => other,
                                    };
                                    if let Type::Named(type_name) = check_type {
                                        let module_info = match type_name.as_str() {
                                            "Redis" => Some(("ioredis", "Redis")),
                                            "EventEmitter" => Some(("events", "EventEmitter")),
                                            "Pool" => Some(("mysql2/promise", "Pool")),
                                            "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
                                            "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                                            _ => {
                                                // Also check dotted names (e.g., mysql.Pool)
                                                if let Some(dot_pos) = type_name.find('.') {
                                                    let module_alias = &type_name[..dot_pos];
                                                    let class_name = &type_name[dot_pos + 1..];
                                                    if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                                                        Some((module_name, class_name))
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            }
                                        };
                                        if let Some((module, class)) = module_info {
                                            ctx.func_return_native_instances.push((
                                                name.clone(), module.to_string(), class.to_string()
                                            ));
                                        }
                                    }
                                }
                            }

                            // Track exported values that need cross-module access.
                            // Any exported const/let with an initializer needs a global data slot
                            // so that importing modules can read its value at runtime.
                            // Previously this only matched Object/Call/Array/New/Arrow expressions,
                            // which caused exported string, number, bigint, and boolean constants
                            // to be undefined when imported by other modules.
                            let needs_export_global = true;

                            // Check if this is a Widget({...}) call from perry/widget
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let Some(widget_decl) = try_lower_widget_decl(ctx, call_expr) {
                                    module.widgets.push(widget_decl);
                                    continue;
                                }
                            }

                            let expr = lower_expr(ctx, init)?;
                            let id = if ctx.pre_registered_module_vars.remove(&name) {
                                let id = ctx.lookup_local(&name).unwrap();
                                if let Some((_, _, existing_ty)) = ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name) {
                                    *existing_ty = ty.clone();
                                }
                                id
                            } else {
                                ctx.define_local(name.clone(), ty.clone())
                            };
                            module.init.push(Stmt::Let {
                                id,
                                name: name.clone(),
                                ty,
                                mutable: matches!(var_decl.kind, ast::VarDeclKind::Let | ast::VarDeclKind::Var),
                                init: Some(expr),
                            });
                            module.exports.push(Export::Named {
                                local: name.clone(),
                                exported: name.clone(),
                            });

                            // Register exported values that need cross-module globals
                            if needs_export_global {
                                module.exported_objects.push(name.clone());
                            }

                            // Handle identifier aliases: export const foo = existingVar;
                            if let ast::Expr::Ident(ident) = init.as_ref() {
                                let ref_name = ident.sym.to_string();
                                if let Some(func_id) = ctx.lookup_func(&ref_name) {
                                    // Function alias - add to exported_functions
                                    module.exported_functions.push((name, func_id));
                                } else {
                                    // Non-function alias (e.g., export const alias = someObject)
                                    // Needs its own export global for cross-module access
                                    module.exported_objects.push(name.clone());
                                }
                            }
                        }
                    }
                }
                ast::Decl::Class(class_decl) => {
                    let class = lower_class_decl(ctx, class_decl, true)?;
                    let class_name = class.name.clone();
                    module.classes.push(class);
                    module.exports.push(Export::Named {
                        local: class_name.clone(),
                        exported: class_name,
                    });
                }
                ast::Decl::TsEnum(enum_decl) => {
                    let en = lower_enum_decl(ctx, enum_decl, true)?;
                    let enum_name = en.name.clone();
                    module.enums.push(en);
                    module.exported_objects.push(enum_name.clone());
                    module.exports.push(Export::Named {
                        local: enum_name.clone(),
                        exported: enum_name,
                    });
                }
                ast::Decl::TsInterface(iface_decl) => {
                    let iface = lower_interface_decl(ctx, iface_decl, true)?;
                    let iface_name = iface.name.clone();
                    module.interfaces.push(iface);
                    module.exports.push(Export::Named {
                        local: iface_name.clone(),
                        exported: iface_name,
                    });
                }
                ast::Decl::TsTypeAlias(alias_decl) => {
                    let alias = lower_type_alias_decl(ctx, alias_decl, true)?;
                    let alias_name = alias.name.clone();
                    module.type_aliases.push(alias);
                    module.exports.push(Export::Named {
                        local: alias_name.clone(),
                        exported: alias_name,
                    });
                }
                ast::Decl::TsModule(ts_module) => {
                    // export namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare {
                        if let Some(ref body) = ts_module.body {
                            let ns_name = match &ts_module.id {
                                ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                                ast::TsModuleName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            };
                            let class = lower_namespace_as_class(ctx, module, &ns_name, body, true)?;
                            let class_name = class.name.clone();
                            module.classes.push(class);
                            module.exports.push(Export::Named {
                                local: class_name.clone(),
                                exported: class_name,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        ast::ModuleDecl::ExportNamed(export_named) => {
            // Skip type-only exports (export type { ... }) - they have no runtime value
            if export_named.type_only {
                return Ok(());
            }
            // export { foo, bar as baz }
            // export { foo } from "source"
            if let Some(ref src) = export_named.src {
                // Re-export from another module
                let source = src.value.as_str().unwrap_or("").to_string();
                for spec in &export_named.specifiers {
                    if let ast::ExportSpecifier::Named(named) = spec {
                        // Skip individual type-only specifiers (export { type Foo, Bar })
                        if named.is_type_only { continue; }
                        let local = match &named.orig {
                            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                            ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        };
                        let exported = named.exported
                            .as_ref()
                            .map(|e| match e {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            })
                            .unwrap_or_else(|| local.clone());
                        module.exports.push(Export::ReExport {
                            source: source.clone(),
                            imported: local,
                            exported,
                        });
                    }
                }
            } else {
                // Local export: export { foo, bar as baz }
                for spec in &export_named.specifiers {
                    if let ast::ExportSpecifier::Named(named) = spec {
                        // Skip individual type-only specifiers (export { type Foo, Bar })
                        if named.is_type_only { continue; }
                        let local = match &named.orig {
                            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                            ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        };
                        let exported = named.exported
                            .as_ref()
                            .map(|e| match e {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            })
                            .unwrap_or_else(|| local.clone());
                        module.exports.push(Export::Named { local: local.clone(), exported: exported.clone() });

                        // If the local name refers to a function, add it to exported_functions
                        // so that a wrapper function is generated for cross-module calls
                        if let Some(func_id) = ctx.lookup_func(&local) {
                            module.exported_functions.push((exported.clone(), func_id));
                        }

                        // Check if the variable is a closure or other exportable object
                        // by looking through init statements
                        for stmt in &module.init {
                            if let Stmt::Let { name, init: Some(init_expr), .. } = stmt {
                                if name == &local {
                                    let is_exportable = matches!(init_expr,
                                        Expr::Closure { .. } |
                                        Expr::Object(_) |
                                        Expr::Array(_) |
                                        Expr::Call { .. } |
                                        Expr::New { .. } |
                                        Expr::JsNew { .. }
                                    );
                                    if is_exportable {
                                        module.exported_objects.push(exported.clone());
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        ast::ModuleDecl::ExportDefaultDecl(export_default) => {
            // export default function foo() {} or export default class Foo {}
            match &export_default.decl {
                ast::DefaultDecl::Fn(fn_expr) => {
                    if let Some(ref ident) = fn_expr.ident {
                        // Named function: export default function foo() {}
                        // Create a function and mark it as default export
                        let func_name = ident.sym.to_string();
                        // TODO: properly lower function expression
                        module.exports.push(Export::Named {
                            local: func_name,
                            exported: "default".to_string(),
                        });
                    }
                }
                ast::DefaultDecl::Class(class_expr) => {
                    if let Some(ref ident) = class_expr.ident {
                        let class_name = ident.sym.to_string();
                        module.exports.push(Export::Named {
                            local: class_name,
                            exported: "default".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        ast::ModuleDecl::ExportAll(export_all) => {
            // export * from "source"
            let source = export_all.src.value.as_str().unwrap_or("").to_string();
            module.exports.push(Export::ExportAll { source });
        }
        ast::ModuleDecl::ExportDefaultExpr(export_default_expr) => {
            // export default <expr>
            let lowered = lower_expr(ctx, &export_default_expr.expr)?;

            // If the expression is a FuncRef, add to exported_functions for proper wrapper generation
            if let Expr::FuncRef(func_id) = &lowered {
                // Find the function and add as exported with name "default"
                let func_id = *func_id;
                module.exported_functions.push(("default".to_string(), func_id));
                // Also mark the function as exported
                for func in &mut module.functions {
                    if func.id == func_id {
                        func.is_exported = true;
                        break;
                    }
                }
                module.exports.push(Export::Named {
                    local: "default".to_string(),
                    exported: "default".to_string(),
                });
            } else {
                // For other expressions (closures, calls, etc.), create a synthetic "default" variable
                let id = ctx.define_local("default".to_string(), Type::Any);
                module.init.push(Stmt::Let {
                    id,
                    name: "default".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(lowered),
                });
                module.exported_objects.push("default".to_string());
                module.exports.push(Export::Named {
                    local: "default".to_string(),
                    exported: "default".to_string(),
                });
            }
        }
        _ => {
            // TsImportEquals, TsExportAssignment, TsNamespaceExport - TypeScript specific
        }
    }
    Ok(())
}

/// Lower a TypeScript namespace declaration into a synthetic class with static methods.
/// `export namespace Slug { export function create() { ... } }` becomes a class `Slug`
/// with a static method `create`. Exported namespace variables are lowered as module-level
/// locals (not static fields) and accessed via compile-time namespace resolution.
/// Private namespace members (non-exported) are lowered as module-level variables.
fn lower_namespace_as_class(
    ctx: &mut LoweringContext,
    module: &mut Module,
    ns_name: &str,
    body: &ast::TsNamespaceBody,
    is_exported: bool,
) -> Result<Class> {
    let class_id = ctx.lookup_class(ns_name).unwrap_or_else(|| {
        let id = ctx.fresh_class();
        ctx.classes.push((ns_name.to_string(), id));
        id
    });

    let items = match body {
        ast::TsNamespaceBody::TsModuleBlock(block) => &block.body,
        ast::TsNamespaceBody::TsNamespaceDecl(_) => {
            // Nested namespace (namespace A.B { }) — not supported yet
            return Ok(Class {
                id: class_id,
                name: ns_name.to_string(),
                type_params: Vec::new(),
                extends: None,
                extends_name: None,
                native_extends: None,
                fields: Vec::new(),
                constructor: None,
                methods: Vec::new(),
                getters: Vec::new(),
                setters: Vec::new(),
                static_fields: Vec::new(),
                static_methods: Vec::new(),
                is_exported,
            });
        }
    };

    let mut static_methods = Vec::new();
    let mut static_method_names = Vec::new();

    // First pass: collect exported function names, pre-register all functions and variables
    // (so namespace members can reference each other regardless of declaration order)
    for item in items {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_some() {
                            let name = fn_decl.ident.sym.to_string();
                            static_method_names.push(name.clone());
                            // Pre-register exported functions so other namespace members can call them
                            if ctx.lookup_func(&name).is_none() {
                                let id = ctx.fresh_func();
                                ctx.functions.push((name, id));
                            }
                        }
                    }
                    ast::Decl::Var(var_decl) => {
                        // Pre-register exported namespace variables as module-level locals
                        for decl in &var_decl.decls {
                            if let Ok(name) = get_binding_name(&decl.name) {
                                if ctx.lookup_local(&name).is_none() {
                                    let ty = extract_binding_type(&decl.name);
                                    ctx.define_local(name.clone(), ty);
                                    ctx.pre_registered_module_vars.insert(name);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Pre-register non-exported functions (hoisted like JS)
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => {
                if fn_decl.function.body.is_some() {
                    let name = fn_decl.ident.sym.to_string();
                    if ctx.lookup_func(&name).is_none() {
                        let id = ctx.fresh_func();
                        ctx.functions.push((name, id));
                    }
                }
            }
            // Pre-register non-exported variables
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(var_decl))) => {
                for decl in &var_decl.decls {
                    if let ast::Pat::Ident(ident) = &decl.name {
                        let name = ident.id.sym.to_string();
                        if ctx.lookup_local(&name).is_none() {
                            let ty = ident.type_ann.as_ref()
                                .map(|ann| extract_ts_type(&ann.type_ann))
                                .unwrap_or(Type::Any);
                            ctx.define_local(name.clone(), ty);
                            ctx.pre_registered_module_vars.insert(name);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Register class and statics early so method bodies can reference them
    ctx.register_class_statics(ns_name.to_string(), Vec::new(), static_method_names.clone());

    // Set current namespace so internal function calls resolve as StaticMethodCall
    let prev_namespace = ctx.current_namespace.take();
    ctx.current_namespace = Some(ns_name.to_string());

    // Second pass: lower all items
    for item in items {
        match item {
            // Non-exported items → module-level variables/functions
            ast::ModuleItem::Stmt(stmt) => {
                lower_stmt(ctx, module, stmt)?;
            }
            // Exported items
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_none() {
                            continue; // Skip declare functions
                        }
                        let func = lower_fn_decl(ctx, fn_decl)?;
                        // Register return type for call-site inference
                        if !matches!(func.return_type, Type::Any) {
                            ctx.register_func_return_type(func.name.clone(), func.return_type.clone());
                        }
                        static_methods.push(func);
                    }
                    ast::Decl::Var(var_decl) => {
                        // Lower exported namespace variables as module-level locals
                        let mutable = var_decl.kind != ast::VarDeclKind::Const;
                        for decl in &var_decl.decls {
                            let name = get_binding_name(&decl.name)?;
                            let ty = extract_binding_type(&decl.name);
                            if let Some(init) = &decl.init {
                                let expr = lower_expr(ctx, init)?;
                                let id = if ctx.pre_registered_module_vars.remove(&name) {
                                    let id = ctx.lookup_local(&name).unwrap();
                                    if let Some((_, _, existing_ty)) = ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name) {
                                        *existing_ty = ty.clone();
                                    }
                                    id
                                } else {
                                    ctx.define_local(name.clone(), ty.clone())
                                };
                                module.init.push(Stmt::Let {
                                    id,
                                    name: name.clone(),
                                    ty,
                                    mutable,
                                    init: Some(expr),
                                });
                                // Track as namespace variable for Ns.member access resolution
                                ctx.namespace_vars.push((ns_name.to_string(), name.clone(), id));
                                // Export the variable for cross-module access
                                if is_exported {
                                    module.exported_objects.push(name.clone());
                                    module.exports.push(Export::Named {
                                        local: name.clone(),
                                        exported: name.clone(),
                                    });
                                }
                            }
                        }
                    }
                    ast::Decl::Class(class_decl) => {
                        let class = lower_class_decl(ctx, class_decl, is_exported)?;
                        module.classes.push(class);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Restore previous namespace context
    ctx.current_namespace = prev_namespace;

    Ok(Class {
        id: class_id,
        name: ns_name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods,
        is_exported,
    })
}

fn lower_stmt(
    ctx: &mut LoweringContext,
    module: &mut Module,
    stmt: &ast::Stmt,
) -> Result<()> {
    match stmt {
        ast::Stmt::Decl(decl) => {
            match decl {
                ast::Decl::Fn(fn_decl) => {
                    // Skip declare functions (no body) - they are external FFI declarations
                    if fn_decl.function.body.is_none() {
                        return Ok(());
                    }
                    let func = lower_fn_decl(ctx, fn_decl)?;
                    // Register return type for call-site inference
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func.name.clone(), func.return_type.clone());
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> = func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    ctx.func_defaults.push((func.id, defaults, param_ids));
                    module.functions.push(func);
                }
                ast::Decl::Var(var_decl) => {
                    let mutable = var_decl.kind != ast::VarDeclKind::Const;
                    for decl in &var_decl.decls {
                        // Check if this is a Widget({...}) call from perry/widget
                        if let Some(init) = &decl.init {
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let Some(widget_decl) = try_lower_widget_decl(ctx, call_expr) {
                                    module.widgets.push(widget_decl);
                                    continue;
                                }
                            }
                        }
                        let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable)?;
                        module.init.extend(stmts);
                    }
                }
                ast::Decl::Class(class_decl) => {
                    let class = lower_class_decl(ctx, class_decl, false)?;
                    module.classes.push(class);
                }
                ast::Decl::TsEnum(enum_decl) => {
                    let en = lower_enum_decl(ctx, enum_decl, false)?;
                    module.enums.push(en);
                }
                ast::Decl::TsInterface(iface_decl) => {
                    let iface = lower_interface_decl(ctx, iface_decl, false)?;
                    module.interfaces.push(iface);
                }
                ast::Decl::TsTypeAlias(alias_decl) => {
                    let alias = lower_type_alias_decl(ctx, alias_decl, false)?;
                    module.type_aliases.push(alias);
                }
                ast::Decl::TsModule(ts_module) => {
                    // namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare {
                        if let Some(ref body) = ts_module.body {
                            let ns_name = match &ts_module.id {
                                ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                                ast::TsModuleName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            };
                            let class = lower_namespace_as_class(ctx, module, &ns_name, body, false)?;
                            module.classes.push(class);
                        }
                    }
                }
                _ => {}
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            // Check if this is a destructuring assignment that needs special handling
            if let ast::Expr::Assign(assign) = expr_stmt.expr.as_ref() {
                if let ast::AssignTarget::Pat(pat) = &assign.left {
                    // This is a destructuring assignment at statement level
                    // We can emit proper Let statements for temporaries
                    let stmts = lower_destructuring_assignment_stmt(ctx, pat, &assign.right)?;
                    module.init.extend(stmts);
                    return Ok(());
                }
            }
            let expr = lower_expr(ctx, &expr_stmt.expr)?;
            module.init.push(Stmt::Expr(expr));
        }
        ast::Stmt::If(if_stmt) => {
            let condition = lower_expr(ctx, &if_stmt.test)?;
            let then_branch = lower_body_stmt(ctx, &if_stmt.cons)?;
            let else_branch = if_stmt.alt.as_ref()
                .map(|s| lower_body_stmt(ctx, s))
                .transpose()?;
            module.init.push(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }
        ast::Stmt::While(while_stmt) => {
            let condition = lower_expr(ctx, &while_stmt.test)?;
            let body = lower_body_stmt(ctx, &while_stmt.body)?;
            module.init.push(Stmt::While { condition, body });
        }
        ast::Stmt::For(for_stmt) => {
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
                                module.init.push(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr });
                            }
                            None
                        } else {
                            for decl in var_decl.decls.iter().skip(1) {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                module.init.push(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr });
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
            module.init.push(Stmt::For { init, condition, update, body });
        }
        ast::Stmt::Block(block) => {
            let stmts = lower_block_stmt(ctx, block)?;
            for stmt in stmts {
                module.init.push(stmt);
            }
        }
        ast::Stmt::Try(try_stmt) => {
            // Lower try body
            let body = lower_block_stmt(ctx, &try_stmt.block)?;

            // Lower catch clause (if present)
            let catch = if let Some(ref catch_clause) = try_stmt.handler {
                let scope_mark = ctx.enter_scope();

                let param = if let Some(ref pat) = catch_clause.param {
                    let param_name = get_pat_name(pat)?;
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    Some((param_id, param_name))
                } else {
                    None
                };

                let catch_body = lower_block_stmt(ctx, &catch_clause.body)?;
                ctx.exit_scope(scope_mark);

                Some(CatchClause { param, body: catch_body })
            } else {
                None
            };

            // Lower finally (if present)
            let finally = if let Some(ref finally_block) = try_stmt.finalizer {
                Some(lower_block_stmt(ctx, finally_block)?)
            } else {
                None
            };

            module.init.push(Stmt::Try { body, catch, finally });
        }
        ast::Stmt::Throw(throw_stmt) => {
            let expr = lower_expr(ctx, &throw_stmt.arg)?;
            module.init.push(Stmt::Throw(expr));
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

            module.init.push(Stmt::Switch { discriminant, cases });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            // Desugar for-of to a regular for loop:
            // for (const x of arr) { body }
            // becomes:
            // { let __arr = arr; for (let __i = 0; __i < __arr.length; __i++) { const x = __arr[__i]; body } }

            // Lower the iterable expression (the array)
            let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;

            // If the iterable is a Map, wrap in MapEntries to convert to array
            // This handles: for (const [k, v] of myMap) { ... }
            // Also extract the Map's key/value type args for proper type propagation.
            let mut map_key_type: Option<Type> = None;
            let mut map_val_type: Option<Type> = None;
            let arr_expr = if let ast::Expr::Ident(ident) = &*for_of_stmt.right {
                let name = ident.sym.to_string();
                let map_type_args = ctx.lookup_local_type(&name)
                    .and_then(|ty| {
                        if let Type::Generic { base, type_args } = ty {
                            if base == "Map" { Some(type_args.clone()) } else { None }
                        } else {
                            None
                        }
                    });
                if let Some(type_args) = map_type_args {
                    if type_args.len() >= 2 {
                        map_key_type = Some(type_args[0].clone());
                        map_val_type = Some(type_args[1].clone());
                    }
                    Expr::MapEntries(Box::new(arr_expr))
                } else {
                    arr_expr
                }
            } else {
                arr_expr
            };

            // Determine the array element type: Tuple(K, V) for Maps, Any otherwise
            let elem_type = if let (Some(ref k), Some(ref v)) = (&map_key_type, &map_val_type) {
                Type::Tuple(vec![k.clone(), v.clone()])
            } else {
                Type::Any
            };
            let arr_type = Type::Array(Box::new(elem_type.clone()));

            // Create internal variables for the array and index
            let arr_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            // Register these in the context so they can be looked up
            ctx.locals.push((format!("__arr_{}", arr_id), arr_id, arr_type.clone()));
            ctx.locals.push((format!("__idx_{}", idx_id), idx_id, Type::Number));

            // Store array reference: let __arr = arr
            module.init.push(Stmt::Let {
                id: arr_id,
                name: format!("__arr_{}", arr_id),
                ty: arr_type,
                mutable: false,
                init: Some(arr_expr),
            });

            // IMPORTANT: Define iteration variables BEFORE lowering the body
            // so the body can reference them
            let item_id = ctx.fresh_local();
            ctx.locals.push((format!("__item_{}", item_id), item_id, elem_type.clone()));

            // Pre-define all variables from the pattern so body can reference them
            let var_ids: Vec<(String, u32)> = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        match &decl.name {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                let id = ctx.define_local(name.clone(), elem_type.clone());
                                vec![(name, id)]
                            }
                            ast::Pat::Array(arr_pat) => {
                                let mut ids = Vec::new();
                                for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                    if let Some(elem_pat) = elem {
                                        if let ast::Pat::Ident(ident) = elem_pat {
                                            let name = ident.id.sym.to_string();
                                            // For Map destructuring [k, v], use key type for idx 0, value type for idx 1
                                            let var_type = if let Type::Tuple(ref types) = elem_type {
                                                types.get(idx).cloned().unwrap_or(Type::Any)
                                            } else {
                                                Type::Any
                                            };
                                            let id = ctx.define_local(name.clone(), var_type);
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

            // NOW lower the body - variables are defined so body can reference them
            let mut loop_body = lower_body_stmt(ctx, &for_of_stmt.body)?;

            // Build binding statements using the pre-defined variable IDs
            let binding_stmts = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        let item_expr = Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        };

                        match &decl.name {
                            ast::Pat::Ident(_) => {
                                // Simple binding: for (const x of arr)
                                let (name, id) = var_ids[0].clone();
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: elem_type.clone(),
                                    mutable: false,
                                    init: Some(item_expr),
                                }]
                            }
                            ast::Pat::Array(arr_pat) => {
                                // Array destructuring: for (const [a, b] of arr)
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: elem_type.clone(),
                                    mutable: false,
                                    init: Some(item_expr),
                                }];

                                // Extract each element using pre-defined IDs
                                let mut var_idx = 0;
                                for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                    if let Some(elem_pat) = elem {
                                        if let ast::Pat::Ident(_) = elem_pat {
                                            let (name, id) = var_ids[var_idx].clone();
                                            var_idx += 1;
                                            // For Map destructuring, use the Tuple element type
                                            let var_type = if let Type::Tuple(ref types) = elem_type {
                                                types.get(idx).cloned().unwrap_or(Type::Any)
                                            } else {
                                                Type::Any
                                            };
                                            stmts.push(Stmt::Let {
                                                id,
                                                name,
                                                ty: var_type,
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
                                // Object destructuring: for (const { a, b } of arr)
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }];

                                // Extract each property using pre-defined IDs
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

            // Create the for loop:
            // for (let __i = 0; __i < __arr.length; __i++) { ... }
            module.init.push(Stmt::For {
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
        }
        ast::Stmt::ForIn(for_in_stmt) => {
            // Desugar for-in to a for-of over Object.keys(obj):
            // for (const key in obj) { body }
            // becomes:
            // { let __keys = Object.keys(obj); for (let __i = 0; __i < __keys.length; __i++) { const key = __keys[__i]; body } }

            // Get the iteration variable name
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

            // Lower the object expression
            let obj_expr = lower_expr(ctx, &for_in_stmt.right)?;

            // Create Object.keys(obj) expression to get the array of keys
            let keys_expr = Expr::ObjectKeys(Box::new(obj_expr));

            // Create internal variables for the keys array and index
            let keys_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            let key_id = ctx.define_local(key_name.clone(), Type::String);

            // Store keys array reference: let __keys = Object.keys(obj)
            module.init.push(Stmt::Let {
                id: keys_id,
                name: format!("__keys_{}", keys_id),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                init: Some(keys_expr),
            });

            // Lower the body
            let mut loop_body = lower_body_stmt(ctx, &for_in_stmt.body)?;

            // Prepend: const key = __keys[__i]
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

            // Create the for loop:
            // for (let __i = 0; __i < __keys.length; __i++) { ... }
            module.init.push(Stmt::For {
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
        }
        _ => {}
    }
    Ok(())
}

/// Assign a value to an expression target (used for unwrapped paren/type-assertion targets).
/// Converts an Expr (which should be an ident or member access) into an assignment.
fn lower_expr_assignment(ctx: &mut LoweringContext, expr: &ast::Expr, value: Box<Expr>) -> Result<Expr> {
    match expr {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalSet(id, value))
            } else {
                eprintln!("  Warning: Assignment to undeclared variable '{}', creating implicit local", name);
                let id = ctx.define_local(name, Type::Any);
                Ok(Expr::LocalSet(id, value))
            }
        }
        ast::Expr::Member(member) => {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldSet {
                                class_name: obj_name,
                                field_name,
                                value,
                            });
                        }
                    }
                }
            }
            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    Ok(Expr::PropertySet { object, property, value })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::IndexSet { object, index, value })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name.to_string());
                    Ok(Expr::PropertySet { object, property, value })
                }
            }
        }
        // Recursively unwrap parens and type annotations
        ast::Expr::Paren(paren) => lower_expr_assignment(ctx, &paren.expr, value),
        ast::Expr::TsAs(ts_as) => lower_expr_assignment(ctx, &ts_as.expr, value),
        ast::Expr::TsNonNull(ts_nn) => lower_expr_assignment(ctx, &ts_nn.expr, value),
        ast::Expr::TsTypeAssertion(ts_ta) => lower_expr_assignment(ctx, &ts_ta.expr, value),
        ast::Expr::TsSatisfies(ts_sat) => lower_expr_assignment(ctx, &ts_sat.expr, value),
        _ => Err(anyhow!("Unsupported expression as assignment target: {:?}", expr)),
    }
}

pub(crate) fn lower_expr(ctx: &mut LoweringContext, expr: &ast::Expr) -> Result<Expr> {
    match expr {
        ast::Expr::Lit(lit) => lower_lit(lit),
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalGet(id))
            } else if let Some(id) = ctx.lookup_func(&name) {
                Ok(Expr::FuncRef(id))
            } else if let Some((module_name, method_name)) = ctx.lookup_native_module(&name) {
                // Special handling for worker_threads named imports
                if module_name == "worker_threads" {
                    if let Some(method) = method_name {
                        if method == "workerData" {
                            // workerData is a property-like import that calls a getter function
                            return Ok(Expr::NativeMethodCall {
                                module: "worker_threads".to_string(),
                                class_name: None,
                                object: None,
                                method: "workerData".to_string(),
                                args: Vec::new(),
                            });
                        }
                        if method == "parentPort" {
                            // parentPort is a singleton handle - call getter function
                            return Ok(Expr::NativeMethodCall {
                                module: "worker_threads".to_string(),
                                class_name: None,
                                object: None,
                                method: "parentPort".to_string(),
                                args: Vec::new(),
                            });
                        }
                    }
                }
                // Native module reference (e.g., mysql from 'mysql2/promise')
                Ok(Expr::NativeModuleRef(module_name.to_string()))
            } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
                // Imported function - reference by its original exported name
                // Look up type information if available
                let (param_types, return_type) = ctx.lookup_extern_func_types(orig_name)
                    .map(|(p, r)| (p.clone(), r.clone()))
                    .unwrap_or_else(|| (Vec::new(), Type::Any));
                Ok(Expr::ExternFuncRef {
                    name: orig_name.to_string(),
                    param_types,
                    return_type,
                })
            } else if is_builtin_function(&name) {
                // Built-in global function (setTimeout, etc.)
                Ok(Expr::ExternFuncRef {
                    name,
                    param_types: Vec::new(),
                    return_type: Type::Any,
                })
            } else if ctx.lookup_class(&name).is_some() {
                // Class used as a first-class value (e.g., { Point: Point })
                Ok(Expr::ClassRef(name))
            } else if name == "undefined" {
                // Global undefined identifier
                Ok(Expr::Undefined)
            } else if name == "null" {
                // Global null identifier (though typically written as literal)
                Ok(Expr::Null)
            } else if name == "NaN" {
                // Global NaN identifier
                Ok(Expr::Number(f64::NAN))
            } else if name == "Infinity" {
                // Global Infinity identifier
                Ok(Expr::Number(f64::INFINITY))
            } else {
                // Assume it's a global (like console)
                if name != "console" && name != "process" && name != "globalThis" && name != "Buffer"
                    && name != "Date" && name != "JSON" && name != "Math" && name != "Object"
                    && name != "Array" && name != "String" && name != "Number" && name != "Boolean"
                    && name != "Error" && name != "TypeError" && name != "RangeError" && name != "Promise"
                    && name != "Map" && name != "Set" && name != "RegExp" && name != "Symbol"
                    && name != "WeakMap" && name != "WeakSet" && name != "Proxy" && name != "Reflect"
                    && name != "Uint8Array" && name != "Int8Array" && name != "TextEncoder" && name != "TextDecoder"
                    && name != "URL" && name != "URLSearchParams" && name != "AbortController" && name != "FormData"
                    && name != "Headers" && name != "fetch" && name != "crypto" && name != "performance"
                    && name != "queueMicrotask" && name != "structuredClone" && name != "atob" && name != "btoa"
                    && name != "BigInt" {
                }
                Ok(Expr::GlobalGet(0)) // TODO: proper global lookup
            }
        }
        ast::Expr::Bin(bin) => {
            // Handle 'in' operator: property in object
            if matches!(bin.op, ast::BinaryOp::In) {
                let property = Box::new(lower_expr(ctx, &bin.left)?);
                let object = Box::new(lower_expr(ctx, &bin.right)?);
                return Ok(Expr::In { property, object });
            }

            // Handle instanceof specially - needs to extract class name
            if matches!(bin.op, ast::BinaryOp::InstanceOf) {
                let expr = Box::new(lower_expr(ctx, &bin.left)?);
                // Right side can be an identifier (ClassName) or member expression (Module.ClassName)
                let ty = match bin.right.as_ref() {
                    ast::Expr::Ident(ident) => ident.sym.to_string(),
                    ast::Expr::Member(member) => {
                        // Handle Module.ClassName - extract the full qualified name
                        let obj_name = if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            obj_ident.sym.to_string()
                        } else {
                            "Unknown".to_string()
                        };
                        let prop_name = match &member.prop {
                            ast::MemberProp::Ident(prop_ident) => prop_ident.sym.to_string(),
                            _ => "Unknown".to_string(),
                        };
                        format!("{}.{}", obj_name, prop_name)
                    }
                    _ => {
                        // For complex expressions, use a generic type name
                        "Object".to_string()
                    }
                };
                return Ok(Expr::InstanceOf { expr, ty });
            }

            let left = Box::new(lower_expr(ctx, &bin.left)?);
            let right = Box::new(lower_expr(ctx, &bin.right)?);

            match bin.op {
                // Arithmetic
                ast::BinaryOp::Add => Ok(Expr::Binary { op: BinaryOp::Add, left, right }),
                ast::BinaryOp::Sub => Ok(Expr::Binary { op: BinaryOp::Sub, left, right }),
                ast::BinaryOp::Mul => Ok(Expr::Binary { op: BinaryOp::Mul, left, right }),
                ast::BinaryOp::Div => Ok(Expr::Binary { op: BinaryOp::Div, left, right }),
                ast::BinaryOp::Mod => Ok(Expr::Binary { op: BinaryOp::Mod, left, right }),
                ast::BinaryOp::Exp => Ok(Expr::Binary { op: BinaryOp::Pow, left, right }),

                // Comparison (treat == same as === for typed code)
                ast::BinaryOp::EqEq => Ok(Expr::Compare { op: CompareOp::Eq, left, right }),
                ast::BinaryOp::EqEqEq => Ok(Expr::Compare { op: CompareOp::Eq, left, right }),
                ast::BinaryOp::NotEq => Ok(Expr::Compare { op: CompareOp::Ne, left, right }),
                ast::BinaryOp::NotEqEq => Ok(Expr::Compare { op: CompareOp::Ne, left, right }),
                ast::BinaryOp::Lt => Ok(Expr::Compare { op: CompareOp::Lt, left, right }),
                ast::BinaryOp::LtEq => Ok(Expr::Compare { op: CompareOp::Le, left, right }),
                ast::BinaryOp::Gt => Ok(Expr::Compare { op: CompareOp::Gt, left, right }),
                ast::BinaryOp::GtEq => Ok(Expr::Compare { op: CompareOp::Ge, left, right }),

                // Logical
                ast::BinaryOp::LogicalAnd => Ok(Expr::Logical { op: LogicalOp::And, left, right }),
                ast::BinaryOp::LogicalOr => Ok(Expr::Logical { op: LogicalOp::Or, left, right }),
                ast::BinaryOp::NullishCoalescing => Ok(Expr::Logical { op: LogicalOp::Coalesce, left, right }),

                // Bitwise
                ast::BinaryOp::BitAnd => Ok(Expr::Binary { op: BinaryOp::BitAnd, left, right }),
                ast::BinaryOp::BitOr => Ok(Expr::Binary { op: BinaryOp::BitOr, left, right }),
                ast::BinaryOp::BitXor => Ok(Expr::Binary { op: BinaryOp::BitXor, left, right }),
                ast::BinaryOp::LShift => Ok(Expr::Binary { op: BinaryOp::Shl, left, right }),
                ast::BinaryOp::RShift => Ok(Expr::Binary { op: BinaryOp::Shr, left, right }),
                ast::BinaryOp::ZeroFillRShift => Ok(Expr::Binary { op: BinaryOp::UShr, left, right }),

                _ => Err(anyhow!("Unsupported binary operator: {:?}", bin.op)),
            }
        }
        ast::Expr::Unary(unary) => {
            let operand = Box::new(lower_expr(ctx, &unary.arg)?);
            match unary.op {
                ast::UnaryOp::Minus => Ok(Expr::Unary { op: UnaryOp::Neg, operand }),
                ast::UnaryOp::Plus => Ok(Expr::Unary { op: UnaryOp::Pos, operand }),
                ast::UnaryOp::Bang => Ok(Expr::Unary { op: UnaryOp::Not, operand }),
                ast::UnaryOp::Tilde => Ok(Expr::Unary { op: UnaryOp::BitNot, operand }),
                ast::UnaryOp::TypeOf => Ok(Expr::TypeOf(operand)),
                ast::UnaryOp::Delete => Ok(Expr::Delete(operand)),
                ast::UnaryOp::Void => Ok(Expr::Void(operand)),
                _ => Err(anyhow!("Unsupported unary operator: {:?}", unary.op)),
            }
        }
        ast::Expr::Call(call) => {
            // Check if any argument has spread
            let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

            let mut args = call.args.iter()
                .map(|arg| lower_expr(ctx, &arg.expr))
                .collect::<Result<Vec<_>>>()?;

            // If spread is present, create CallSpread instead of Call
            let spread_args: Option<Vec<CallArg>> = if has_spread {
                Some(call.args.iter().zip(args.iter())
                    .map(|(ast_arg, lowered)| {
                        if ast_arg.spread.is_some() {
                            CallArg::Spread(lowered.clone())
                        } else {
                            CallArg::Expr(lowered.clone())
                        }
                    })
                    .collect())
            } else {
                None
            };

            match &call.callee {
                ast::Callee::Super(_) => {
                    // super() call in constructor
                    Ok(Expr::SuperCall(args))
                }
                ast::Callee::Expr(expr) => {
                    // Check for super.method() call
                    if let ast::Expr::SuperProp(super_prop) = expr.as_ref() {
                        if let ast::SuperProp::Ident(ident) = &super_prop.prop {
                            return Ok(Expr::SuperMethodCall {
                                method: ident.sym.to_string(),
                                args,
                            });
                        }
                    }

                    // Check for native module method calls (e.g., mysql.createConnection())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();

                            // Check for process module methods
                            if obj_name == "process" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "uptime" => return Ok(Expr::ProcessUptime),
                                        "cwd" => return Ok(Expr::ProcessCwd),
                                        "memoryUsage" => return Ok(Expr::ProcessMemoryUsage),
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for os module methods FIRST (before generic NativeMethodCall)
                            let is_os_module = obj_name == "os" ||
                                ctx.lookup_builtin_module_alias(&obj_name) == Some("os");
                            if is_os_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "platform" => return Ok(Expr::OsPlatform),
                                        "arch" => return Ok(Expr::OsArch),
                                        "hostname" => return Ok(Expr::OsHostname),
                                        "homedir" => return Ok(Expr::OsHomedir),
                                        "tmpdir" => return Ok(Expr::OsTmpdir),
                                        "totalmem" => return Ok(Expr::OsTotalmem),
                                        "freemem" => return Ok(Expr::OsFreemem),
                                        "uptime" => return Ok(Expr::OsUptime),
                                        "type" => return Ok(Expr::OsType),
                                        "release" => return Ok(Expr::OsRelease),
                                        "cpus" => return Ok(Expr::OsCpus),
                                        "networkInterfaces" => return Ok(Expr::OsNetworkInterfaces),
                                        "userInfo" => return Ok(Expr::OsUserInfo),
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Buffer static methods
                            if obj_name == "Buffer" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            let encoding = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferFrom {
                                                data: Box::new(data),
                                                encoding
                                            });
                                        }
                                        "alloc" => {
                                            let size = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            let fill = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferAlloc {
                                                size: Box::new(size),
                                                fill
                                            });
                                        }
                                        "allocUnsafe" => {
                                            let size = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            return Ok(Expr::BufferAllocUnsafe(Box::new(size)));
                                        }
                                        "concat" => {
                                            let list = args.get(0).cloned().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::BufferConcat(Box::new(list)));
                                        }
                                        "isBuffer" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::BufferIsBuffer(Box::new(obj)));
                                        }
                                        "byteLength" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::String("".to_string()));
                                            return Ok(Expr::BufferByteLength(Box::new(data)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Uint8Array static methods
                            if obj_name == "Uint8Array" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::Uint8ArrayFrom(Box::new(data)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Object static methods
                            if obj_name == "Object" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "keys" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectKeys(Box::new(obj)));
                                        }
                                        "values" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectValues(Box::new(obj)));
                                        }
                                        "entries" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectEntries(Box::new(obj)));
                                        }
                                        // Object.freeze(obj) is a no-op in Perry (we don't enforce immutability)
                                        "freeze" | "seal" | "preventExtensions" | "create" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(obj);
                                        }
                                        // Object.assign(target, src1, src2, ...) - treat as object spread
                                        // Each non-object arg is spread; object literal args are inlined
                                        "assign" => {
                                            let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                                            for arg in &args {
                                                match arg {
                                                    Expr::Object(props) => {
                                                        // Inline object literal props as static key-value pairs
                                                        for (key, val) in props {
                                                            parts.push((Some(key.clone()), val.clone()));
                                                        }
                                                    }
                                                    _ => {
                                                        // Spread non-object expression
                                                        parts.push((None, arg.clone()));
                                                    }
                                                }
                                            }
                                            // If no spreads and only static props, return plain Object
                                            let has_spread = parts.iter().any(|(k, _)| k.is_none());
                                            if !has_spread {
                                                let static_props: Vec<(String, Expr)> = parts.into_iter()
                                                    .filter_map(|(k, v)| k.map(|key| (key, v)))
                                                    .collect();
                                                return Ok(Expr::Object(static_props));
                                            }
                                            return Ok(Expr::ObjectSpread { parts });
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Array static methods
                            if obj_name == "Array" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "isArray" => {
                                            let value = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ArrayIsArray(Box::new(value)));
                                        }
                                        "from" => {
                                            let value = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ArrayFrom(Box::new(value)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for net module methods
                            let is_net_module = obj_name == "net" ||
                                ctx.lookup_builtin_module_alias(&obj_name) == Some("net");
                            if is_net_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "createServer" => {
                                            let options = args.get(0).cloned().map(Box::new);
                                            let connection_listener = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::NetCreateServer { options, connection_listener });
                                        }
                                        "createConnection" | "connect" => {
                                            let port = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            let host = args.get(1).cloned().map(Box::new);
                                            let connect_listener = args.get(2).cloned().map(Box::new);
                                            return Ok(Expr::NetCreateConnection {
                                                port: Box::new(port),
                                                host,
                                                connect_listener
                                            });
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            if let Some((module_name, _imported_method)) = ctx.lookup_native_module(&obj_name) {
                                // Skip modules handled specifically below (path, fs, child_process, etc.)
                                let is_handled_module = module_name == "path" || module_name == "node:path"
                                    || module_name == "fs" || module_name == "node:fs"
                                    || module_name == "child_process" || module_name == "node:child_process"
                                    || module_name == "crypto" || module_name == "node:crypto"
                                    || module_name == "os" || module_name == "node:os"
                                    || module_name == "net" || module_name == "node:net";
                                if !is_handled_module {
                                    // This is a call on a native module (e.g., mysql.createConnection)
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.to_string();
                                        return Ok(Expr::NativeMethodCall {
                                            module: module_name.to_string(),
                                            class_name: None,  // Will be set by js_transform if needed
                                            object: None,  // Static call on module itself
                                            method: method_name,
                                            args,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // Check for static method calls (e.g., Counter.increment())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            if ctx.lookup_class(&obj_name).is_some() {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.to_string();
                                    if ctx.has_static_method(&obj_name, &method_name) {
                                        return Ok(Expr::StaticMethodCall {
                                            class_name: obj_name,
                                            method_name,
                                            args,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // Check for native instance method calls (e.g., emitter.on(), ws.send())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            // Clone module_name and class_name to avoid borrow issues
                            let native_instance = ctx.lookup_native_instance(&obj_name)
                                .map(|(m, c)| (m.to_string(), c.to_string()));
                            if obj_name == "pool" {
                            }
                            if let Some((module_name, class_name)) = native_instance {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.to_string();
                                    // Get the object expression (the instance variable)
                                    let object_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::NativeMethodCall {
                                        module: module_name,
                                        class_name: Some(class_name),  // Use the registered class name
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                            }
                        }

                        // Check for method calls on new Big/Decimal/BigNumber() expressions
                        // e.g., new Big("100").div(2)
                        if let Some(module_name) = detect_native_instance_expr(&member.obj) {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.to_string();
                                let object_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::NativeMethodCall {
                                    module: module_name.to_string(),
                                    class_name: None,  // Will be set by js_transform if needed
                                    object: Some(Box::new(object_expr)),
                                    method: method_name,
                                    args,
                                });
                            }
                        }

                        // Check for chained method calls on registered native instances
                        // e.g., r1.times(...).times(...) where r1 is a Big
                        // The inner call might lower to a NativeMethodCall, and we need to chain properly
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.to_string();
                            // Lower the object expression first
                            let object_expr = lower_expr(ctx, &member.obj)?;
                            // Check if it's a NativeMethodCall for a math library
                            if let Expr::NativeMethodCall { module, class_name, .. } = &object_expr {
                                // Methods that return the same type (builder pattern)
                                let is_math_lib = matches!(module.as_str(), "big.js" | "decimal.js" | "bignumber.js");
                                let is_fluent_method = matches!(method_name.as_str(),
                                    "plus" | "minus" | "times" | "div" | "mod" |
                                    "pow" | "sqrt" | "abs" | "neg" | "round" | "floor" | "ceil" | "toFixed"
                                );
                                if is_math_lib && is_fluent_method {
                                    return Ok(Expr::NativeMethodCall {
                                        module: module.clone(),
                                        class_name: class_name.clone(),
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                            }
                        }
                    }

                    // Check for fs.methodName() calls (including require('fs') aliases)
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            // Check if this is 'fs' directly or an alias from require('fs')
                            let obj_name = obj_ident.sym.as_ref();
                            let is_fs_module = obj_name == "fs" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("fs");
                            if is_fs_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "readFileSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsReadFileSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "writeFileSync" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let path = iter.next().unwrap();
                                                let content = iter.next().unwrap();
                                                return Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content)));
                                            }
                                        }
                                        "existsSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsExistsSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "mkdirSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsMkdirSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "unlinkSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsUnlinkSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "readFileBuffer" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "rmRecursive" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsRmRecursive(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for path.methodName() calls (including require('path') aliases)
                            let is_path_module = obj_name == "path" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("path");
                            if is_path_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "join" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let mut result = iter.next().unwrap();
                                                for next_arg in iter {
                                                    result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                                }
                                                return Ok(result);
                                            }
                                        }
                                        "dirname" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathDirname(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "basename" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathBasename(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "extname" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathExtname(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "resolve" => {
                                            if args.len() >= 1 {
                                                // path.resolve(a, b, c) => resolve(join(a, b, c))
                                                // For single arg, just resolve directly
                                                let mut iter = args.into_iter();
                                                let first = iter.next().unwrap();
                                                let mut joined = first;
                                                for next_arg in iter {
                                                    joined = Expr::PathJoin(Box::new(joined), Box::new(next_arg));
                                                }
                                                return Ok(Expr::PathResolve(Box::new(joined)));
                                            }
                                        }
                                        "isAbsolute" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathIsAbsolute(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for JSON.methodName() calls
                            if obj_ident.sym.as_ref() == "JSON" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "parse" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::JsonParse(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "stringify" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::JsonStringify(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Math.methodName() calls
                            if obj_ident.sym.as_ref() == "Math" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "floor" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathFloor(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "ceil" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathCeil(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "round" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathRound(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "abs" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAbs(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "sqrt" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathSqrt(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log2" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog2(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log10" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog10(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "pow" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let base = args_iter.next().unwrap();
                                                let exp = args_iter.next().unwrap();
                                                return Ok(Expr::MathPow(Box::new(base), Box::new(exp)));
                                            }
                                        }
                                        "min" => {
                                            return Ok(Expr::MathMin(args));
                                        }
                                        "max" => {
                                            return Ok(Expr::MathMax(args));
                                        }
                                        "random" => {
                                            return Ok(Expr::MathRandom);
                                        }
                                        "imul" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let a = args_iter.next().unwrap();
                                                let b = args_iter.next().unwrap();
                                                return Ok(Expr::MathImul(Box::new(a), Box::new(b)));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for String.methodName() static calls
                            if obj_ident.sym.as_ref() == "String" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "fromCharCode" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::StringFromCharCode(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for crypto.methodName() calls (including require('crypto') aliases)
                            let is_crypto_module = obj_name == "crypto" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("crypto");
                            if is_crypto_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "randomBytes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoRandomBytes(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "randomUUID" => {
                                            return Ok(Expr::CryptoRandomUUID);
                                        }
                                        "sha256" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoSha256(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "md5" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoMd5(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for os.methodName() calls (including require('os') aliases)
                            let is_os_module = obj_name == "os" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("os");
                            if is_os_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "platform" => {
                                            return Ok(Expr::OsPlatform);
                                        }
                                        "arch" => {
                                            return Ok(Expr::OsArch);
                                        }
                                        "hostname" => {
                                            return Ok(Expr::OsHostname);
                                        }
                                        "homedir" => {
                                            return Ok(Expr::OsHomedir);
                                        }
                                        "tmpdir" => {
                                            return Ok(Expr::OsTmpdir);
                                        }
                                        "totalmem" => {
                                            return Ok(Expr::OsTotalmem);
                                        }
                                        "freemem" => {
                                            return Ok(Expr::OsFreemem);
                                        }
                                        "uptime" => {
                                            return Ok(Expr::OsUptime);
                                        }
                                        "type" => {
                                            return Ok(Expr::OsType);
                                        }
                                        "release" => {
                                            return Ok(Expr::OsRelease);
                                        }
                                        "cpus" => {
                                            return Ok(Expr::OsCpus);
                                        }
                                        "networkInterfaces" => {
                                            return Ok(Expr::OsNetworkInterfaces);
                                        }
                                        "userInfo" => {
                                            return Ok(Expr::OsUserInfo);
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Buffer.methodName() static calls
                            if obj_name == "Buffer" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            let encoding = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferFrom {
                                                data: Box::new(data),
                                                encoding,
                                            });
                                        }
                                        "alloc" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let size = args_iter.next().unwrap();
                                                let fill = args_iter.next().map(Box::new);
                                                return Ok(Expr::BufferAlloc {
                                                    size: Box::new(size),
                                                    fill,
                                                });
                                            }
                                        }
                                        "allocUnsafe" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferAllocUnsafe(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "concat" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferConcat(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isBuffer" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferIsBuffer(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "byteLength" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferByteLength(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for child_process named imports (execSync, spawnSync, spawn, exec)
                            let is_child_process_module = ctx.lookup_builtin_module_alias(obj_name) == Some("child_process");
                            if is_child_process_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "execSync" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessExecSync {
                                                    command: Box::new(command),
                                                    options,
                                                });
                                            }
                                        }
                                        "spawnSync" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawnSync {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    options,
                                                });
                                            }
                                        }
                                        "spawn" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawn {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    options,
                                                });
                                            }
                                        }
                                        "exec" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let options = args_iter.next().map(Box::new);
                                                let callback = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessExec {
                                                    command: Box::new(command),
                                                    options,
                                                    callback,
                                                });
                                            }
                                        }
                                        "spawnBackground" => {
                                            if args.len() >= 3 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let log_file = args_iter.next().unwrap();
                                                let env_json = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawnBackground {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    log_file: Box::new(log_file),
                                                    env_json,
                                                });
                                            }
                                        }
                                        "getProcessStatus" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ChildProcessGetProcessStatus(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "killProcess" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ChildProcessKillProcess(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for net.methodName() calls
                            let is_net_module = obj_name == "net" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("net");
                            if is_net_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "createServer" => {
                                            let mut args_iter = args.into_iter();
                                            let options = args_iter.next().map(Box::new);
                                            let connection_listener = args_iter.next().map(Box::new);
                                            return Ok(Expr::NetCreateServer {
                                                options,
                                                connection_listener,
                                            });
                                        }
                                        "createConnection" | "connect" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let port = args_iter.next().unwrap();
                                                let host = args_iter.next().map(Box::new);
                                                let connect_listener = args_iter.next().map(Box::new);
                                                return Ok(Expr::NetCreateConnection {
                                                    port: Box::new(port),
                                                    host,
                                                    connect_listener,
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Date.now() static method call
                            if obj_ident.sym.as_ref() == "Date" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "now" {
                                        return Ok(Expr::DateNow);
                                    }
                                }
                            }
                        }

                        // Check for Date instance method calls (date.getTime(), etc.)
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "getTime" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetTime(Box::new(date_expr)));
                                }
                                "toISOString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToISOString(Box::new(date_expr)));
                                }
                                "getFullYear" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetFullYear(Box::new(date_expr)));
                                }
                                "getMonth" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMonth(Box::new(date_expr)));
                                }
                                "getDate" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetDate(Box::new(date_expr)));
                                }
                                "getHours" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetHours(Box::new(date_expr)));
                                }
                                "getMinutes" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMinutes(Box::new(date_expr)));
                                }
                                "getSeconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetSeconds(Box::new(date_expr)));
                                }
                                "getMilliseconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMilliseconds(Box::new(date_expr)));
                                }
                                _ => {} // Fall through to other handling
                            }
                        }

                        // Check for array method calls (arr.push, arr.pop, etc.)
                        // These are called on local variables, not global modules
                        // IMPORTANT: Only apply to actual Array types, not String types
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                                let arr_name = arr_ident.sym.to_string();
                                // Check that this is NOT a String type (Array, Set, Map are all OK)
                                // When type is unknown, only enter array block for array-only methods
                                // (push, pop, etc.), NOT for methods shared with strings (indexOf,
                                // includes, split) — those are handled by the general dispatch which
                                // checks is_string at codegen time.
                                let type_info = ctx.lookup_local_type(&arr_name);
                                let is_known_string = type_info.map(|ty| matches!(ty, Type::String)).unwrap_or(false);
                                let is_known_not_string = type_info.map(|ty| !matches!(ty, Type::String | Type::Any | Type::Unknown)).unwrap_or(false);
                                let is_ambiguous_method = matches!(method_name,
                                    "indexOf" | "includes" | "slice"
                                );
                                let is_not_string = if is_known_string {
                                    false  // definitely a string, skip array block
                                } else if is_known_not_string {
                                    true   // definitely not a string, enter array block
                                } else if is_ambiguous_method {
                                    false  // type unknown + ambiguous method, skip array block (fall through to general dispatch)
                                } else {
                                    true   // type unknown + array-only method (push, pop, etc.), enter array block
                                };
                                if is_not_string {
                                if let Some(array_id) = ctx.lookup_local(&arr_name) {
                                    match method_name {
                                        "push" => {
                                            if args.len() >= 1 {
                                                // Check if the argument has spread operator
                                                if call.args.len() >= 1 && call.args[0].spread.is_some() {
                                                    return Ok(Expr::ArrayPushSpread {
                                                        array_id,
                                                        source: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                                return Ok(Expr::ArrayPush {
                                                    array_id,
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "pop" => {
                                            return Ok(Expr::ArrayPop(array_id));
                                        }
                                        "shift" => {
                                            return Ok(Expr::ArrayShift(array_id));
                                        }
                                        "unshift" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayUnshift {
                                                    array_id,
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            // arr.slice(start, end?) - returns new array
                                            // Only convert to ArraySlice if we KNOW it's an Array type
                                            // (Type::Any could be a string, which has its own .slice() method)
                                            let is_definitely_array = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Array(_)))
                                                .unwrap_or(false);
                                            if is_definitely_array && args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                            // Fall through to normal Call handling for strings or unknown types
                                        }
                                        "splice" => {
                                            // arr.splice(start, deleteCount?, ...items) - returns deleted elements
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArraySplice {
                                                    array_id,
                                                    start: Box::new(start),
                                                    delete_count: delete_count.map(Box::new),
                                                    items,
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                            let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                            if !is_map_or_set && args.len() >= 1 {
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "map" => {
                                            // Only use ArrayMap if receiver is not a class instance
                                            let is_class_instance = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Named(_) | Type::Generic { .. }) && !matches!(ty, Type::Array(_)))
                                                .unwrap_or(false);
                                            if !is_class_instance && args.len() >= 1 {
                                                return Ok(Expr::ArrayMap {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "filter" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayFilter {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "find" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayFind {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "findIndex" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayFindIndex {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "join" => {
                                            // arr.join(separator?) -> string
                                            let separator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayJoin {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                separator,
                                            });
                                        }
                                        "flat" => {
                                            // arr.flat() -> flattened array
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                            });
                                        }
                                        // Map methods (only apply to actual Map/Set types)
                                        "set" => {
                                            // Check if this is a Map or Set type before treating as Map.set()
                                            let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                            if is_map_or_set && args.len() >= 2 {
                                                // map.set(key, value) - returns the map for chaining
                                                let mut args_iter = args.into_iter();
                                                let key = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::MapSet {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(key),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "get" => {
                                            // Check if this is a Map type before treating as Map.get()
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.len() >= 1 {
                                                // map.get(key) - returns value or undefined
                                                return Ok(Expr::MapGet {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "has" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if (is_set || is_map) && args.len() >= 1 {
                                                let value = args.into_iter().next().unwrap();
                                                if is_set {
                                                    return Ok(Expr::SetHas {
                                                        set: Box::new(Expr::LocalGet(array_id)),
                                                        value: Box::new(value),
                                                    });
                                                } else {
                                                    return Ok(Expr::MapHas {
                                                        map: Box::new(Expr::LocalGet(array_id)),
                                                        key: Box::new(value),
                                                    });
                                                }
                                            }
                                        }
                                        "delete" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if (is_set || is_map) && args.len() >= 1 {
                                                let value = args.into_iter().next().unwrap();
                                                if is_set {
                                                    return Ok(Expr::SetDelete {
                                                        set: Box::new(Expr::LocalGet(array_id)),
                                                        value: Box::new(value),
                                                    });
                                                } else {
                                                    return Ok(Expr::MapDelete {
                                                        map: Box::new(Expr::LocalGet(array_id)),
                                                        key: Box::new(value),
                                                    });
                                                }
                                            }
                                        }
                                        "clear" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_set {
                                                return Ok(Expr::SetClear(Box::new(Expr::LocalGet(array_id))));
                                            } else if is_map {
                                                return Ok(Expr::MapClear(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            // Fall through if neither Set nor Map
                                        }
                                        // Map iterator methods: entries(), keys(), values()
                                        "entries" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapEntries(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        "keys" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapKeys(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        "values" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set && args.is_empty() {
                                                return Ok(Expr::SetValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        // Set methods
                                        "add" => {
                                            // Check if this is a Set type before treating as Set.add()
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set && args.len() >= 1 {
                                                // set.add(value) - returns the set for chaining
                                                let value = args.into_iter().next().unwrap();
                                                return Ok(Expr::SetAdd {
                                                    set_id: array_id,
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }

                                    // URLSearchParams methods
                                    let is_url_search_params = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "URLSearchParams"))
                                        .unwrap_or(false);
                                    if is_url_search_params {
                                        match method_name {
                                            "get" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsGet {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "has" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsHas {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "set" => {
                                                if args.len() >= 2 {
                                                    let mut args_iter = args.into_iter();
                                                    let name_arg = args_iter.next().unwrap();
                                                    let value_arg = args_iter.next().unwrap();
                                                    return Ok(Expr::UrlSearchParamsSet {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(name_arg),
                                                        value: Box::new(value_arg),
                                                    });
                                                }
                                            }
                                            "append" => {
                                                if args.len() >= 2 {
                                                    let mut args_iter = args.into_iter();
                                                    let name_arg = args_iter.next().unwrap();
                                                    let value_arg = args_iter.next().unwrap();
                                                    return Ok(Expr::UrlSearchParamsAppend {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(name_arg),
                                                        value: Box::new(value_arg),
                                                    });
                                                }
                                            }
                                            "delete" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsDelete {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "toString" => {
                                                return Ok(Expr::UrlSearchParamsToString(
                                                    Box::new(Expr::LocalGet(array_id))
                                                ));
                                            }
                                            "getAll" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsGetAll {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                }  // close is_array_type check
                            }

                            // Check for array methods on property access (e.g., this.items.push(value))
                            // This handles cases where the array is a property of an object, not a local variable
                            if let ast::Expr::Member(obj_member) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(obj_prop_ident) = &obj_member.prop {
                                    let property_name = obj_prop_ident.sym.to_string();
                                    // Lower the object expression (e.g., 'this' or a local variable)
                                    let object_expr = lower_expr(ctx, &obj_member.obj)?;

                                    match method_name {
                                        "push" => {
                                            if args.len() >= 1 {
                                                // For now, fall through to generic Call handling
                                                // We'll compile this in codegen using inline property access
                                                // property-based push: object.{property}.push()
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    // Check for array methods on imported variables (e.g., import { CHAIN_NAMES } from './module')
                    // These don't have local IDs but are ExternFuncRef values
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                                    let arr_name = arr_ident.sym.to_string();
                                    // Check if this is an imported variable (not a local)
                                    if ctx.lookup_local(&arr_name).is_none() {
                                        if let Some(orig_name) = ctx.lookup_imported_func(&arr_name) {
                                            // This is an imported variable - create ExternFuncRef for it
                                            let (param_types, return_type) = ctx.lookup_extern_func_types(orig_name)
                                                .map(|(p, r)| (p.clone(), r.clone()))
                                                .unwrap_or_else(|| (Vec::new(), Type::Any));
                                            let extern_ref = Expr::ExternFuncRef {
                                                name: orig_name.to_string(),
                                                param_types,
                                                return_type,
                                            };
                                            match method_name {
                                                "join" => {
                                                    // arr.join(separator?) -> string
                                                    let separator = args.into_iter().next().map(Box::new);
                                                    return Ok(Expr::ArrayJoin {
                                                        array: Box::new(extern_ref),
                                                        separator,
                                                    });
                                                }
                                                "map" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayMap {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "filter" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayFilter {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "forEach" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayForEach {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "find" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayFind {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "sort" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArraySort {
                                                            array: Box::new(extern_ref),
                                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "indexOf" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayIndexOf {
                                                            array: Box::new(extern_ref),
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "includes" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayIncludes {
                                                            array: Box::new(extern_ref),
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "slice" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let start = args_iter.next().unwrap();
                                                        let end = args_iter.next();
                                                        return Ok(Expr::ArraySlice {
                                                            array: Box::new(extern_ref),
                                                            start: Box::new(start),
                                                            end: end.map(Box::new),
                                                        });
                                                    }
                                                }
                                                "reduce" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let callback = args_iter.next().unwrap();
                                                        let initial = args_iter.next().map(Box::new);
                                                        return Ok(Expr::ArrayReduce {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(callback),
                                                            initial,
                                                        });
                                                    }
                                                }
                                                "flat" => {
                                                    return Ok(Expr::ArrayFlat {
                                                        array: Box::new(extern_ref),
                                                    });
                                                }
                                                _ => {} // Fall through for other methods
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for array methods on inline array literals (e.g., ['a', 'b'].join('-'))
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                if let ast::Expr::Array(_arr_lit) = member.obj.as_ref() {
                                    // Lower the array literal
                                    let array_expr = lower_expr(ctx, &member.obj)?;
                                    match method_name {
                                        "join" => {
                                            // ['a', 'b'].join(separator?) -> string
                                            let separator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayJoin {
                                                array: Box::new(array_expr),
                                                separator,
                                            });
                                        }
                                        "map" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayMap {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "filter" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayFilter {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "find" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayFind {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(array_expr),
                                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(array_expr),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(array_expr),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(array_expr),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "flat" => {
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(array_expr),
                                            });
                                        }
                                        _ => {} // Fall through for other methods
                                    }
                                }
                            }
                        }
                    }

                    // Check for array-only methods on any expression (e.g., Object.entries(x).reduce(...))
                    // ONLY match methods that are unique to arrays (not shared with strings)
                    // "includes", "indexOf", "slice", "join" also exist on strings, so skip those
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                match method_name {
                                    "reduce" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduce {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                    "map" if args.len() >= 1 => {
                                        // Skip if receiver is a known class instance (e.g., Box.map())
                                        // Check both local variables with class types AND new expressions
                                        let is_class_instance = match member.obj.as_ref() {
                                            ast::Expr::Ident(ident) => {
                                                ctx.lookup_local_type(&ident.sym.to_string())
                                                    .map(|ty| matches!(ty, Type::Named(_) | Type::Generic { .. }) && !matches!(ty, Type::Array(_)))
                                                    .unwrap_or(false)
                                            }
                                            ast::Expr::New(_) => {
                                                // new ClassName(...).map() - always a class instance, not an array
                                                true
                                            }
                                            _ => false,
                                        };
                                        if !is_class_instance {
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            return Ok(Expr::ArrayMap {
                                                array: Box::new(array_expr),
                                                callback: Box::new(args.into_iter().next().unwrap()),
                                            });
                                        }
                                    }
                                    "filter" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFilter {
                                            array: Box::new(array_expr),
                                            callback: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                    "forEach" if args.len() >= 1 => {
                                        // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                        let is_map_or_set = if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false)
                                        } else {
                                            false
                                        };
                                        if !is_map_or_set {
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            return Ok(Expr::ArrayForEach {
                                                array: Box::new(array_expr),
                                                callback: Box::new(args.into_iter().next().unwrap()),
                                            });
                                        }
                                    }
                                    "find" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFind {
                                            array: Box::new(array_expr),
                                            callback: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                    "findIndex" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFindIndex {
                                            array: Box::new(array_expr),
                                            callback: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                    "sort" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArraySort {
                                            array: Box::new(array_expr),
                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                    // .join() is exclusively an Array method (strings don't have it),
                                    // so we can always safely lower to ArrayJoin regardless of the
                                    // receiver expression type. Previously this only matched specific
                                    // array-returning expressions, which caused .split().join() chains
                                    // to fall through to generic dispatch and produce wrong results.
                                    "join" if args.len() <= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let separator = if args.is_empty() { None } else { Some(Box::new(args.into_iter().next().unwrap())) };
                                        return Ok(Expr::ArrayJoin {
                                            array: Box::new(array_expr),
                                            separator,
                                        });
                                    }
                                    "indexOf" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) |
                                            Expr::ArrayFrom(_) | Expr::StringSplit(_, _) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_)
                                        ) {
                                            let value_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::ArrayIndexOf {
                                                array: Box::new(array_expr),
                                                value: Box::new(value_expr),
                                            });
                                        }
                                    }
                                    "includes" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) |
                                            Expr::ArrayFrom(_) | Expr::StringSplit(_, _) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_)
                                        ) {
                                            let value_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::ArrayIncludes {
                                                array: Box::new(array_expr),
                                                value: Box::new(value_expr),
                                            });
                                        }
                                    }
                                    "flat" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFlat {
                                            array: Box::new(array_expr),
                                        });
                                    }
                                    "push" if args.len() >= 1 => {
                                        // Generic expr.push(value) or expr.push(...spread)
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if call.args.len() >= 1 && call.args[0].spread.is_some() {
                                            return Ok(Expr::NativeMethodCall {
                                                module: "array".to_string(),
                                                method: "push_spread".to_string(),
                                                class_name: None,
                                                object: Some(Box::new(array_expr)),
                                                args: args,
                                            });
                                        } else {
                                            return Ok(Expr::NativeMethodCall {
                                                module: "array".to_string(),
                                                method: "push_single".to_string(),
                                                class_name: None,
                                                object: Some(Box::new(array_expr)),
                                                args: args,
                                            });
                                        }
                                    }
                                    _ => {} // Fall through - ambiguous methods on non-array expressions use generic dispatch
                                }
                            }
                        }
                    }

                    // Check for regex .test() method call on any expression
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                if method_ident.sym.as_ref() == "test" && args.len() == 1 {
                                    // Check if the object is a regex literal or a local assigned to a regex
                                    let is_regex_obj = match member.obj.as_ref() {
                                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                                        ast::Expr::Ident(ident) => {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Any | Type::Unknown) || matches!(ty, Type::Named(n) if n == "RegExp"))
                                                .unwrap_or(true)
                                        }
                                        _ => false,
                                    };
                                    if is_regex_obj {
                                        let regex_expr = lower_expr(ctx, &member.obj)?;
                                        // Only emit RegExpTest if the object is actually a regex
                                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                            let string_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::RegExpTest {
                                                regex: Box::new(regex_expr),
                                                string: Box::new(string_expr),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for string .match(regex) method call
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                if method_ident.sym.as_ref() == "match" && args.len() == 1 {
                                    // Check if the argument is a regex literal or a local holding a regex
                                    let arg_is_regex = match call.args.first().map(|a| a.expr.as_ref()) {
                                        Some(ast::Expr::Lit(ast::Lit::Regex(_))) => true,
                                        Some(ast::Expr::Ident(ident)) => {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Any | Type::Unknown))
                                                .unwrap_or(true)
                                        }
                                        _ => false,
                                    };
                                    if arg_is_regex {
                                        let string_expr = lower_expr(ctx, &member.obj)?;
                                        let regex_expr = args.remove(0);
                                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                            return Ok(Expr::StringMatch {
                                                string: Box::new(string_expr),
                                                regex: Box::new(regex_expr),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for global built-in function calls (parseInt, parseFloat, Number, String, isNaN, isFinite)
                    if let ast::Expr::Ident(ident) = expr.as_ref() {
                        let func_name = ident.sym.as_ref();
                        match func_name {
                            "parseInt" => {
                                let string_arg = if args.len() >= 1 {
                                    Box::new(args.remove(0))
                                } else {
                                    return Err(anyhow!("parseInt requires at least one argument"));
                                };
                                let radix_arg = if !args.is_empty() {
                                    Some(Box::new(args.remove(0)))
                                } else {
                                    None
                                };
                                return Ok(Expr::ParseInt { string: string_arg, radix: radix_arg });
                            }
                            "parseFloat" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::ParseFloat(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("parseFloat requires one argument"));
                                }
                            }
                            "Number" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::NumberCoerce(Box::new(args.remove(0))));
                                } else {
                                    // Number() with no args returns 0
                                    return Ok(Expr::Number(0.0));
                                }
                            }
                            "BigInt" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::BigIntCoerce(Box::new(args.remove(0))));
                                } else {
                                    // BigInt() with no args returns 0n
                                    return Ok(Expr::BigInt("0".to_string()));
                                }
                            }
                            "String" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StringCoerce(Box::new(args.remove(0))));
                                } else {
                                    // String() with no args returns ""
                                    return Ok(Expr::String(String::new()));
                                }
                            }
                            "isNaN" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::IsNaN(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("isNaN requires one argument"));
                                }
                            }
                            "isFinite" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::IsFinite(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("isFinite requires one argument"));
                                }
                            }
                            "perryResolveStaticPlugin" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StaticPluginResolve(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("perryResolveStaticPlugin requires one argument"));
                                }
                            }
                            "fetchWithAuth" => {
                                // fetchWithAuth(url, authHeader) -> Promise<Response>
                                // Calls js_fetch_get_with_auth(url, auth_header)
                                if args.len() >= 2 {
                                    let url = args.remove(0);
                                    let auth_header = args.remove(0);
                                    ctx.uses_fetch = true;
                                    return Ok(Expr::FetchGetWithAuth {
                                        url: Box::new(url),
                                        auth_header: Box::new(auth_header),
                                    });
                                } else {
                                    return Err(anyhow!("fetchWithAuth requires url and authHeader arguments"));
                                }
                            }
                            "fetchPostWithAuth" => {
                                // fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
                                // Calls js_fetch_post_with_auth(url, auth_header, body)
                                if args.len() >= 3 {
                                    let url = args.remove(0);
                                    let auth_header = args.remove(0);
                                    let body = args.remove(0);
                                    ctx.uses_fetch = true;
                                    return Ok(Expr::FetchPostWithAuth {
                                        url: Box::new(url),
                                        auth_header: Box::new(auth_header),
                                        body: Box::new(body),
                                    });
                                } else {
                                    return Err(anyhow!("fetchPostWithAuth requires url, authHeader, and body arguments"));
                                }
                            }
                            "fetch" => {
                                // Handle fetch(url) and fetch(url, options)
                                // Extract URL (first argument)
                                let url = if args.len() >= 1 {
                                    args.remove(0)
                                } else {
                                    return Err(anyhow!("fetch requires at least a URL argument"));
                                };

                                // Check if there's an options object (second argument)
                                if args.len() >= 1 {
                                    // Extract options from the object literal
                                    // We need to get the original AST to extract the object properties
                                    if let Some(options_arg) = call.args.get(1) {
                                        if let ast::Expr::Object(obj) = &*options_arg.expr {
                                            // Extract method, body, and headers from options
                                            let mut method = Expr::String("GET".to_string());
                                            let mut body = Expr::Undefined;
                                            let mut headers_obj: Vec<(String, Expr)> = Vec::new();

                                            for prop in &obj.props {
                                                if let ast::PropOrSpread::Prop(prop) = prop {
                                                    match prop.as_ref() {
                                                        ast::Prop::KeyValue(kv) => {
                                                            let key = match &kv.key {
                                                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                                _ => continue,
                                                            };
                                                            match key.as_str() {
                                                                "method" => {
                                                                    method = lower_expr(ctx, &kv.value)?;
                                                                }
                                                                "body" => {
                                                                    body = lower_expr(ctx, &kv.value)?;
                                                                }
                                                                "headers" => {
                                                                    // Extract headers object
                                                                    if let ast::Expr::Object(headers_ast) = &*kv.value {
                                                                        for hprop in &headers_ast.props {
                                                                            if let ast::PropOrSpread::Prop(hprop) = hprop {
                                                                                if let ast::Prop::KeyValue(hkv) = hprop.as_ref() {
                                                                                    let hkey = match &hkv.key {
                                                                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                                                        _ => continue,
                                                                                    };
                                                                                    let hval = lower_expr(ctx, &hkv.value)?;
                                                                                    headers_obj.push((hkey, hval));
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                        ast::Prop::Shorthand(ident) => {
                                                            // Handle shorthand properties like { body } which means { body: body }
                                                            let key = ident.sym.to_string();
                                                            let value = if let Some(local_id) = ctx.lookup_local(&key) {
                                                                Expr::LocalGet(local_id)
                                                            } else {
                                                                continue;
                                                            };
                                                            match key.as_str() {
                                                                "method" => method = value,
                                                                "body" => body = value,
                                                                _ => {}
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }

                                            // Create a FetchWithOptions expression
                                            ctx.uses_fetch = true;
                                            return Ok(Expr::FetchWithOptions {
                                                url: Box::new(url),
                                                method: Box::new(method),
                                                body: Box::new(body),
                                                headers: headers_obj,
                                            });
                                        }
                                    }
                                }

                                // Simple fetch(url) with no options - use GET
                                ctx.uses_fetch = true;
                                return Ok(Expr::FetchWithOptions {
                                    url: Box::new(url),
                                    method: Box::new(Expr::String("GET".to_string())),
                                    body: Box::new(Expr::Undefined),
                                    headers: Vec::new(),
                                });
                            }
                            _ => {} // Fall through to generic handling
                        }

                        // Check if this is a named import from child_process (e.g., execSync, spawnSync)
                        if let Some((module_name, _method)) = ctx.lookup_native_module(func_name) {
                            if module_name == "child_process" {
                                match func_name {
                                    "execSync" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessExecSync {
                                                command: Box::new(command),
                                                options,
                                            });
                                        }
                                    }
                                    "spawnSync" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawnSync {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                options,
                                            });
                                        }
                                    }
                                    "spawn" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawn {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                options,
                                            });
                                        }
                                    }
                                    "exec" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let options = args_iter.next().map(Box::new);
                                            let callback = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessExec {
                                                command: Box::new(command),
                                                options,
                                                callback,
                                            });
                                        }
                                    }
                                    "spawnBackground" => {
                                        if args.len() >= 3 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let log_file = args_iter.next().unwrap();
                                            let env_json = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawnBackground {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                log_file: Box::new(log_file),
                                                env_json,
                                            });
                                        }
                                    }
                                    "getProcessStatus" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::ChildProcessGetProcessStatus(
                                                Box::new(args.into_iter().next().unwrap())
                                            ));
                                        }
                                    }
                                    "killProcess" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::ChildProcessKillProcess(
                                                Box::new(args.into_iter().next().unwrap())
                                            ));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from path (e.g., join, dirname, basename)
                            if module_name == "path" {
                                match func_name {
                                    "join" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let mut result = iter.next().unwrap();
                                            for next_arg in iter {
                                                result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                            }
                                            return Ok(result);
                                        }
                                    }
                                    "dirname" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathDirname(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "basename" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathBasename(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "extname" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathExtname(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "resolve" => {
                                        if args.len() >= 1 {
                                            let mut iter = args.into_iter();
                                            let first = iter.next().unwrap();
                                            let mut joined = first;
                                            for next_arg in iter {
                                                joined = Expr::PathJoin(Box::new(joined), Box::new(next_arg));
                                            }
                                            return Ok(Expr::PathResolve(Box::new(joined)));
                                        }
                                    }
                                    "isAbsolute" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathIsAbsolute(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from url (e.g., fileURLToPath)
                            if module_name == "url" {
                                match func_name {
                                    "fileURLToPath" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FileURLToPath(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from fs (e.g., existsSync, mkdirSync, etc.)
                            if module_name == "fs" {
                                match func_name {
                                    "readFileSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsReadFileSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "writeFileSync" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path = iter.next().unwrap();
                                            let content = iter.next().unwrap();
                                            return Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content)));
                                        }
                                    }
                                    "existsSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsExistsSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "mkdirSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsMkdirSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "unlinkSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsUnlinkSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "appendFileSync" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path = iter.next().unwrap();
                                            let content = iter.next().unwrap();
                                            return Ok(Expr::FsAppendFileSync(Box::new(path), Box::new(content)));
                                        }
                                    }
                                    "readFileBuffer" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "rmRecursive" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsRmRecursive(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }
                        }

                        // Check if this is a direct call on an aliased named import
                        // e.g., uuid() where import { v4 as uuid } from 'uuid'
                        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
                            return Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            });
                        }

                        // Check if this is a call on a default import from a native module
                        // e.g., Fastify() where import Fastify from 'fastify'
                        if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                            return Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: "default".to_string(), // Use "default" for default export calls
                                args,
                            });
                        }
                    }

                    let callee_expr = lower_expr(ctx, expr)?;

                    // Fill in default arguments if callee is a known function
                    let mut args = args;
                    if let Expr::FuncRef(func_id) = &callee_expr {
                        if let Some((defaults, param_ids)) = ctx.lookup_func_defaults(*func_id) {
                            let defaults = defaults.to_vec();
                            let param_ids = param_ids.to_vec();
                            let num_provided = args.len();
                            // Build substitution map: callee param LocalId -> actual arg expression
                            // For provided args, map to the caller's arg expression
                            // For defaulted args, map to the expanded default (built incrementally)
                            let mut param_map: Vec<(LocalId, Expr)> = Vec::new();
                            for i in 0..param_ids.len().min(num_provided) {
                                param_map.push((param_ids[i], args[i].clone()));
                            }
                            // Fill in missing arguments with their defaults, substituting
                            // any parameter references to use the caller's scope
                            for i in num_provided..defaults.len() {
                                if let Some(default_expr) = &defaults[i] {
                                    let substituted = LoweringContext::substitute_param_refs_in_default(
                                        default_expr, &param_map
                                    );
                                    // Add this expanded default to the map so later defaults
                                    // can reference it (e.g., c = b where b was also defaulted)
                                    if i < param_ids.len() {
                                        param_map.push((param_ids[i], substituted.clone()));
                                    }
                                    args.push(substituted);
                                }
                            }
                        }
                    }

                    // If inside a namespace, convert calls to namespace functions into StaticMethodCall
                    if let Expr::FuncRef(func_id) = &callee_expr {
                        if let Some(ref ns_name) = ctx.current_namespace {
                            if let Some(func_name) = ctx.lookup_func_name(*func_id) {
                                if ctx.has_static_method(ns_name, func_name) {
                                    let method_name = func_name.to_string();
                                    let class_name = ns_name.clone();
                                    return Ok(Expr::StaticMethodCall {
                                        class_name,
                                        method_name,
                                        args,
                                    });
                                }
                            }
                        }
                    }

                    let callee = Box::new(callee_expr);
                    // Extract explicit type arguments if present (e.g., identity<number>(x))
                    let type_args = call.type_args.as_ref()
                        .map(|ta| ta.params.iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect())
                        .unwrap_or_default();

                    // Use CallSpread if any argument has spread
                    if let Some(spread_args) = spread_args {
                        Ok(Expr::CallSpread { callee, args: spread_args, type_args })
                    } else {
                        Ok(Expr::Call { callee, args, type_args })
                    }
                }
                ast::Callee::Import(_) => {
                    // Dynamic import: import('module')
                    // Extract the module path from the first argument if available
                    let module_path = if let Some(first_arg) = args.first() {
                        if let Expr::String(s) = first_arg {
                            s.clone()
                        } else {
                            "<dynamic>".to_string()
                        }
                    } else {
                        "<unknown>".to_string()
                    };
                    eprintln!("Warning: Dynamic import('{}') not fully supported, returning undefined", module_path);
                    // Dynamic imports return a Promise that resolves to the module
                    // For now, return undefined as we'd need full runtime support
                    Ok(Expr::Undefined)
                }
            }
        }
        ast::Expr::Member(member) => {
            // Check if this is process.* property access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == "process" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        match prop_ident.sym.as_ref() {
                            "argv" => return Ok(Expr::ProcessArgv),
                            "platform" => return Ok(Expr::OsPlatform),
                            "arch" => return Ok(Expr::OsArch),
                            _ => {}
                        }
                    }
                }
            }

            // Check if this is a process.env.VARNAME or process.env[expr] access
            if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
                if let ast::Expr::Ident(obj_ident) = inner_member.obj.as_ref() {
                    if obj_ident.sym.as_ref() == "process" {
                        if let ast::MemberProp::Ident(prop_ident) = &inner_member.prop {
                            if prop_ident.sym.as_ref() == "env" {
                                // This is process.env access
                                match &member.prop {
                                    ast::MemberProp::Ident(var_ident) => {
                                        // process.env.VARNAME (static key)
                                        let var_name = var_ident.sym.to_string();
                                        return Ok(Expr::EnvGet(var_name));
                                    }
                                    ast::MemberProp::Computed(computed) => {
                                        // process.env[expr] (dynamic key)
                                        let key_expr = Box::new(lower_expr(ctx, &computed.expr)?);
                                        return Ok(Expr::EnvGetDynamic(key_expr));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            // Check for Math constants (e.g., Math.PI, Math.E)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == "Math" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let val = match prop_ident.sym.as_ref() {
                            "PI" => Some(std::f64::consts::PI),
                            "E" => Some(std::f64::consts::E),
                            "LN2" => Some(std::f64::consts::LN_2),
                            "LN10" => Some(std::f64::consts::LN_10),
                            "LOG2E" => Some(std::f64::consts::LOG2_E),
                            "LOG10E" => Some(std::f64::consts::LOG10_E),
                            "SQRT2" => Some(std::f64::consts::SQRT_2),
                            "SQRT1_2" => Some(std::f64::consts::FRAC_1_SQRT_2),
                            _ => None,
                        };
                        if let Some(v) = val {
                            return Ok(Expr::Number(v));
                        }
                    }
                }
            }

            // Check if this is an enum member access (e.g., Color.Red)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_enum(&obj_name).is_some() {
                    // This is an enum access
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let member_name = prop_ident.sym.to_string();
                        return Ok(Expr::EnumMember {
                            enum_name: obj_name,
                            member_name,
                        });
                    }
                }
            }

            // Check if this is a static field access (e.g., Counter.count)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldGet {
                                class_name: obj_name,
                                field_name,
                            });
                        }
                    }
                }
            }

            // Check if this is a namespace variable access (e.g., Flag.OPENCODE_AUTO_SHARE)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let member_name = prop_ident.sym.to_string();
                    if let Some(local_id) = ctx.lookup_namespace_var(&obj_name, &member_name) {
                        return Ok(Expr::LocalGet(local_id));
                    }
                }
            }

            // Check if this is os.EOL property access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.as_ref();
                let is_os_module = obj_name == "os" ||
                    ctx.lookup_builtin_module_alias(obj_name) == Some("os");
                if is_os_module {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        if prop_ident.sym.as_ref() == "EOL" {
                            return Ok(Expr::OsEOL);
                        }
                    }
                }
            }

            // Check for native instance property access (e.g., response.status, response.ok)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                // Clone module_name early to avoid borrow issues
                let native_instance = ctx.lookup_native_instance(&obj_name)
                    .map(|(m, _c)| m.to_string());
                if let Some(module_name) = native_instance {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let property_name = prop_ident.sym.to_string();
                        // For properties that map to FFI functions, generate a NativeMethodCall
                        // with no args (property getter)
                        let object_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Expr::NativeMethodCall {
                            module: module_name,
                            class_name: None,
                            object: Some(Box::new(object_expr)),
                            method: property_name,
                            args: Vec::new(),
                        });
                    }
                }
            }

            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    Ok(Expr::PropertyGet { object, property })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    // Specialize for Uint8Array/Buffer variables → byte-level access
                    if let Expr::LocalGet(id) = &*object {
                        if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                            if matches!(ty, Type::Named(n) if n == "Uint8Array") {
                                return Ok(Expr::Uint8ArrayGet { array: object, index });
                            }
                        }
                    }
                    Ok(Expr::IndexGet { object, index })
                }
                ast::MemberProp::PrivateName(private) => {
                    // Private field access: this.#field -> PropertyGet with "#field"
                    let property = format!("#{}", private.name.to_string());
                    Ok(Expr::PropertyGet { object, property })
                }
            }
        }
        ast::Expr::Paren(paren) => {
            lower_expr(ctx, &paren.expr)
        }
        ast::Expr::Assign(assign) => {
            // Detect assignments from native module calls and register for cross-function tracking.
            // e.g., `mongoClient = await MongoClient.connect(uri)` registers mongoClient as a mongodb instance.
            if assign.op == ast::AssignOp::Assign {
                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(target_ident)) = &assign.left {
                    let var_name = target_ident.id.sym.to_string();
                    // Unwrap await if present
                    let inner_rhs = if let ast::Expr::Await(await_expr) = assign.right.as_ref() {
                        await_expr.arg.as_ref()
                    } else {
                        assign.right.as_ref()
                    };
                    // Check for NativeModule.method() call (e.g., MongoClient.connect(uri))
                    if let ast::Expr::Call(call_expr) = inner_rhs {
                        if let ast::Callee::Expr(callee) = &call_expr.callee {
                            if let ast::Expr::Member(member) = callee.as_ref() {
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    let obj_name = obj_ident.sym.as_ref();
                                    if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                            let class_name = match (module_name, method_ident.sym.as_ref()) {
                                                ("mongodb", "connect") => Some("MongoClient"),
                                                ("pg", "connect") => Some("Client"),
                                                _ => Some("Instance"),
                                            };
                                            if let Some(class_name) = class_name {
                                                ctx.module_native_instances.push((var_name.clone(), module_name.to_string(), class_name.to_string()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Check for variable-to-variable assignment: `x = y` where y is a known native instance.
                    // e.g., `mongoClient = client` where client was tracked from MongoClient.connect().
                    if let ast::Expr::Ident(rhs_ident) = inner_rhs {
                        let rhs_name = rhs_ident.sym.as_ref();
                        if let Some((module, class)) = ctx.lookup_native_instance(rhs_name) {
                            ctx.module_native_instances.push((var_name, module.to_string(), class.to_string()));
                        }
                    }
                }
            }

            let rhs = lower_expr(ctx, &assign.right)?;

            // Handle compound assignment operators (+=, -=, *=, /=, etc.)
            let value = match assign.op {
                ast::AssignOp::Assign => Box::new(rhs),
                ast::AssignOp::AddAssign => {
                    // a += b becomes a = a + b
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::SubAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Sub,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::MulAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Mul,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::DivAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Div,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::ModAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Mod,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::BitAndAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitAnd,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::BitOrAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitOr,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::BitXorAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::LShiftAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Shl,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::RShiftAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Shr,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::ZeroFillRShiftAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::UShr,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::ExpAssign => {
                    // a **= b becomes a = a ** b
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Pow,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::AndAssign => {
                    // a &&= b becomes a = a && b (short-circuit: only evaluates b if a is truthy)
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Logical {
                        op: LogicalOp::And,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::OrAssign => {
                    // a ||= b becomes a = a || b (short-circuit: only evaluates b if a is falsy)
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Logical {
                        op: LogicalOp::Or,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::NullishAssign => {
                    // a ??= b becomes a = a ?? b (short-circuit: only evaluates b if a is null/undefined)
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Logical {
                        op: LogicalOp::Coalesce,
                        left,
                        right: Box::new(rhs),
                    })
                }
                _ => return Err(anyhow!("Unsupported assignment operator: {:?}", assign.op)),
            };

            match &assign.left {
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
                    let name = ident.id.sym.to_string();
                    if let Some(id) = ctx.lookup_local(&name) {
                        Ok(Expr::LocalSet(id, value))
                    } else {
                        // Variable not found in scope — likely a closure capture that wasn't
                        // properly tracked. Create an implicit local to avoid hard failure.
                        eprintln!("  Warning: Assignment to undeclared variable '{}', creating implicit local", name);
                        let id = ctx.define_local(name, Type::Any);
                        Ok(Expr::LocalSet(id, value))
                    }
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) => {
                    // Check if this is a static field assignment (e.g., Counter.count = 5)
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        let obj_name = obj_ident.sym.to_string();
                        if ctx.lookup_class(&obj_name).is_some() {
                            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                let field_name = prop_ident.sym.to_string();
                                if ctx.has_static_field(&obj_name, &field_name) {
                                    return Ok(Expr::StaticFieldSet {
                                        class_name: obj_name,
                                        field_name,
                                        value,
                                    });
                                }
                            }
                        }
                    }

                    let object = Box::new(lower_expr(ctx, &member.obj)?);
                    match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let property = ident.sym.to_string();
                            Ok(Expr::PropertySet { object, property, value })
                        }
                        ast::MemberProp::Computed(computed) => {
                            let index = Box::new(lower_expr(ctx, &computed.expr)?);
                            // Specialize for Uint8Array/Buffer variables → byte-level access
                            if let Expr::LocalGet(id) = &*object {
                                if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                                    if matches!(ty, Type::Named(n) if n == "Uint8Array") {
                                        return Ok(Expr::Uint8ArraySet { array: object, index, value });
                                    }
                                }
                            }
                            Ok(Expr::IndexSet { object, index, value })
                        }
                        ast::MemberProp::PrivateName(private) => {
                            // Private field assignment: this.#field = value
                            let property = format!("#{}", private.name.to_string());
                            Ok(Expr::PropertySet { object, property, value })
                        }
                    }
                }
                ast::AssignTarget::Pat(pat) => {
                    // Destructuring assignment: [a, b] = expr or { a, b } = expr
                    // We need to lower this to a sequence of assignments
                    lower_destructuring_assignment(ctx, pat, value)
                }
                // Unwrap TypeScript type annotations and parentheses for assignment
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
                    lower_expr_assignment(ctx, &paren.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
                    lower_expr_assignment(ctx, &ts_as.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
                    lower_expr_assignment(ctx, &ts_nn.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
                    lower_expr_assignment(ctx, &ts_ta.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
                    lower_expr_assignment(ctx, &ts_sat.expr, value)
                }
                other => Err(anyhow!("Unsupported assignment target: {:?}", other)),
            }
        }
        ast::Expr::Cond(cond) => {
            let condition = Box::new(lower_expr(ctx, &cond.test)?);
            let then_expr = Box::new(lower_expr(ctx, &cond.cons)?);
            let else_expr = Box::new(lower_expr(ctx, &cond.alt)?);
            Ok(Expr::Conditional { condition, then_expr, else_expr })
        }
        ast::Expr::Array(array) => {
            // Check if any elements are spread elements
            let has_spread = array.elems.iter()
                .filter_map(|elem| elem.as_ref())
                .any(|elem| elem.spread.is_some());

            if has_spread {
                // Use ArraySpread for arrays with spread elements
                let elements = array.elems.iter()
                    .filter_map(|elem| elem.as_ref())
                    .map(|elem| {
                        let expr = lower_expr(ctx, &elem.expr)?;
                        if elem.spread.is_some() {
                            Ok(ArrayElement::Spread(expr))
                        } else {
                            Ok(ArrayElement::Expr(expr))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::ArraySpread(elements))
            } else {
                // No spread elements, use regular Array
                let elements = array.elems.iter()
                    .filter_map(|elem| elem.as_ref())
                    .map(|elem| lower_expr(ctx, &elem.expr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::Array(elements))
            }
        }
        ast::Expr::Object(obj) => {
            // Check if any spread elements exist; if so, use ObjectSpread
            let has_spread = obj.props.iter().any(|p| matches!(p, ast::PropOrSpread::Spread(_)));
            if has_spread {
                let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                for prop in &obj.props {
                    match prop {
                        ast::PropOrSpread::Spread(spread) => {
                            let spread_expr = lower_expr(ctx, &spread.expr)?;
                            parts.push((None, spread_expr));
                        }
                        ast::PropOrSpread::Prop(prop) => {
                            match prop.as_ref() {
                                ast::Prop::KeyValue(kv) => {
                                    let key = match &kv.key {
                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                        ast::PropName::Num(n) => n.value.to_string(),
                                        _ => continue,
                                    };
                                    let value = lower_expr(ctx, &kv.value)?;
                                    parts.push((Some(key), value));
                                }
                                ast::Prop::Shorthand(ident) => {
                                    let name = ident.sym.to_string();
                                    let value = if let Some(func_id) = ctx.lookup_func(&name) {
                                        Expr::FuncRef(func_id)
                                    } else if let Some(local_id) = ctx.lookup_local(&name) {
                                        Expr::LocalGet(local_id)
                                    } else if ctx.lookup_class(&name).is_some() {
                                        Expr::ClassRef(name.clone())
                                    } else {
                                        continue;
                                    };
                                    parts.push((Some(name), value));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return Ok(Expr::ObjectSpread { parts });
            }
            let mut props = Vec::new();
            for prop in &obj.props {
                match prop {
                    ast::PropOrSpread::Prop(prop) => {
                        match prop.as_ref() {
                            ast::Prop::KeyValue(kv) => {
                                let key = match &kv.key {
                                    ast::PropName::Ident(ident) => ident.sym.to_string(),
                                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                    ast::PropName::Num(n) => n.value.to_string(),
                                    ast::PropName::Computed(computed) => {
                                        // Handle computed property keys like [ChainName.ETHEREUM]
                                        // Try to resolve enum member access to string keys
                                        match computed.expr.as_ref() {
                                            ast::Expr::Member(member) => {
                                                if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) = (member.obj.as_ref(), &member.prop) {
                                                    let enum_name = obj.sym.to_string();
                                                    let member_name = prop.sym.to_string();
                                                    if let Some(value) = ctx.lookup_enum_member(&enum_name, &member_name) {
                                                        match value {
                                                            EnumValue::String(s) => s.clone(),
                                                            EnumValue::Number(n) => n.to_string(),
                                                        }
                                                    } else {
                                                        continue;
                                                    }
                                                } else {
                                                    continue;
                                                }
                                            }
                                            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                                            ast::Expr::Lit(ast::Lit::Num(n)) => n.value.to_string(),
                                            _ => continue,
                                        }
                                    }
                                    _ => continue,
                                };
                                let value = lower_expr(ctx, &kv.value)?;
                                props.push((key, value));
                            }
                            ast::Prop::Shorthand(ident) => {
                                // Shorthand property: { help } → { help: help }
                                let name = ident.sym.to_string();
                                let value = if let Some(func_id) = ctx.lookup_func(&name) {
                                    Expr::FuncRef(func_id)
                                } else if let Some(local_id) = ctx.lookup_local(&name) {
                                    Expr::LocalGet(local_id)
                                } else if ctx.lookup_class(&name).is_some() {
                                    Expr::ClassRef(name.clone())
                                } else {
                                    continue;
                                };
                                props.push((name, value));
                            }
                            ast::Prop::Method(method) => {
                                // Inline method: { help(): string { ... } }
                                let key = match &method.key {
                                    ast::PropName::Ident(ident) => ident.sym.to_string(),
                                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                    _ => continue,
                                };
                                let func_id = ctx.fresh_func();
                                // Use a unique synthetic name to avoid collisions
                                let func_name = format!("__obj_method_{}_{}", key, func_id);

                                // Snapshot outer locals for capture analysis
                                let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                                    .map(|(name, id, _)| (name.clone(), *id))
                                    .collect();

                                let scope_mark = ctx.enter_scope();
                                let mut params = Vec::new();
                                for param in method.function.params.iter() {
                                    let param_name = get_pat_name(&param.pat)?;
                                    let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
                                    let param_default = get_param_default(ctx, &param.pat)?;
                                    let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                                    params.push(Param {
                                        id: param_id,
                                        name: param_name,
                                        ty: param_type,
                                        default: param_default,
                                        is_rest: is_rest_param(&param.pat),
                                    });
                                }
                                let return_type = method.function.return_type.as_ref()
                                    .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
                                    .unwrap_or(Type::Any);
                                let body = if let Some(ref block) = method.function.body {
                                    lower_block_stmt(ctx, block)?
                                } else {
                                    Vec::new()
                                };
                                ctx.exit_scope(scope_mark);

                                // Capture analysis (same pattern as arrow/function expressions)
                                let mut all_refs = Vec::new();
                                let mut visited_closures = std::collections::HashSet::new();
                                for stmt in &body {
                                    collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
                                }
                                let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                                    .map(|(_, id)| *id)
                                    .collect();
                                let method_param_ids: std::collections::HashSet<LocalId> = params.iter()
                                    .map(|p| p.id)
                                    .collect();
                                let mut captures: Vec<LocalId> = all_refs.into_iter()
                                    .filter(|id| outer_local_ids.contains(id) && !method_param_ids.contains(id))
                                    .collect();
                                captures.sort();
                                captures.dedup();

                                if captures.is_empty() {
                                    // No captures: keep as standalone Function + FuncRef
                                    ctx.functions.push((func_name.clone(), func_id));
                                    let defaults: Vec<Option<Expr>> = params.iter().map(|p| p.default.clone()).collect();
                                    let param_ids: Vec<LocalId> = params.iter().map(|p| p.id).collect();
                                    ctx.func_defaults.push((func_id, defaults, param_ids));
                                    ctx.pending_functions.push(Function {
                                        id: func_id,
                                        name: func_name,
                                        type_params: Vec::new(),
                                        params,
                                        return_type,
                                        body,
                                        is_async: method.function.is_async,
                                        is_generator: false,
                                        is_exported: false,
                                        captures: Vec::new(),
                                        decorators: Vec::new(),
                                    });
                                    props.push((key, Expr::FuncRef(func_id)));
                                } else {
                                    // Has captures: emit as Closure
                                    let mut all_assigned = Vec::new();
                                    for stmt in &body {
                                        collect_assigned_locals_stmt(stmt, &mut all_assigned);
                                    }
                                    let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
                                    let mutable_captures: Vec<LocalId> = captures.iter()
                                        .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                                        .copied()
                                        .collect();
                                    let captures_this = closure_uses_this(&body);
                                    let enclosing_class = if captures_this {
                                        ctx.current_class.clone()
                                    } else {
                                        None
                                    };
                                    props.push((key, Expr::Closure {
                                        func_id,
                                        params,
                                        return_type,
                                        body,
                                        captures,
                                        mutable_captures,
                                        captures_this,
                                        enclosing_class,
                                        is_async: method.function.is_async,
                                    }));
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Ok(Expr::Object(props))
        }
        ast::Expr::This(_) => {
            // Always use Expr::This - the codegen will handle it with ThisContext
            Ok(Expr::This)
        }
        ast::Expr::New(new_expr) => {
            // Try to extract class name from callee
            match new_expr.callee.as_ref() {
                ast::Expr::Ident(ident) => {
                    let class_name = ident.sym.to_string();

                    // Handle built-in types
                    if class_name == "Map" {
                        // new Map() -> create empty map
                        return Ok(Expr::MapNew);
                    }
                    if class_name == "Set" {
                        // new Set() or new Set(iterable)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::SetNew);
                        } else {
                            return Ok(Expr::SetNewFromArray(Box::new(args.into_iter().next().unwrap())));
                        }
                    }
                    if class_name == "Date" {
                        // new Date() or new Date(timestamp)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::DateNew(None));
                        } else {
                            return Ok(Expr::DateNew(Some(Box::new(args.into_iter().next().unwrap()))));
                        }
                    }
                    // Handle Error and its subclasses
                    if class_name == "Error" || class_name == "TypeError" || class_name == "RangeError"
                        || class_name == "ReferenceError" || class_name == "SyntaxError"
                        || class_name == "BugIndicatingError" {
                        // new Error() or new Error(message)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::ErrorNew(None));
                        } else {
                            return Ok(Expr::ErrorNew(Some(Box::new(args.into_iter().next().unwrap()))));
                        }
                    }

                    // Handle URL class
                    if class_name == "URL" {
                        // new URL(url) or new URL(url, base)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let mut args_iter = args.into_iter();
                        let url_arg = args_iter.next()
                            .ok_or_else(|| anyhow!("URL constructor requires at least 1 argument"))?;
                        let base_arg = args_iter.next();
                        return Ok(Expr::UrlNew {
                            url: Box::new(url_arg),
                            base: base_arg.map(Box::new),
                        });
                    }

                    // Handle URLSearchParams class
                    if class_name == "URLSearchParams" {
                        // new URLSearchParams() or new URLSearchParams(init)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let init_arg = args.into_iter().next();
                        return Ok(Expr::UrlSearchParamsNew(init_arg.map(Box::new)));
                    }

                    // Handle Uint8Array constructor
                    if class_name == "Uint8Array" {
                        // new Uint8Array() or new Uint8Array(length) or new Uint8Array(array)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::Uint8ArrayNew(None));
                        } else if args.len() == 1 {
                            return Ok(Expr::Uint8ArrayNew(Some(Box::new(args.into_iter().next().unwrap()))));
                        }
                        // 2+ args: fall through to Expr::New to handle
                        // new Uint8Array(buffer, byteOffset, length) etc.
                    }

                    let args = new_expr.args.as_ref()
                        .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                        .transpose()?
                        .unwrap_or_default();
                    // Extract explicit type arguments if present (e.g., new Box<number>(42))
                    let type_args = new_expr.type_args.as_ref()
                        .map(|ta| ta.params.iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect())
                        .unwrap_or_default();
                    Ok(Expr::New { class_name, args, type_args })
                }
                // Non-identifier callee (e.g., new (condition ? A : B)() or new someVar())
                _ => {
                    // Check for class expressions: new (class extends X { ... })()
                    let class_expr_opt = match new_expr.callee.as_ref() {
                        ast::Expr::Class(ce) => Some(ce),
                        ast::Expr::Paren(paren) => match paren.expr.as_ref() {
                            ast::Expr::Class(ce) => Some(ce),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(class_expr) = class_expr_opt {
                        let synthetic_name = format!("__anon_class_{}", ctx.fresh_class());
                        let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
                        ctx.pending_classes.push(class);
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let type_args = new_expr.type_args.as_ref()
                            .map(|ta| ta.params.iter()
                                .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                                .collect())
                            .unwrap_or_default();
                        return Ok(Expr::New { class_name: synthetic_name, args, type_args });
                    }

                    let callee = Box::new(lower_expr(ctx, &new_expr.callee)?);
                    let args = new_expr.args.as_ref()
                        .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                        .transpose()?
                        .unwrap_or_default();
                    Ok(Expr::NewDynamic { callee, args })
                }
            }
        }
        ast::Expr::Arrow(arrow) => {
            // Lower arrow function to a closure
            let func_id = ctx.fresh_func();
            let scope_mark = ctx.enter_scope();

            // Track which locals exist before entering the closure scope
            let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                .map(|(name, id, _)| (name.clone(), *id))
                .collect();

            // Lower parameters and collect destructuring info
            let mut params = Vec::new();
            let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
            for param in &arrow.params {
                let param_name = get_pat_name(param)?;
                let param_default = get_param_default(ctx, param)?;
                let is_rest = is_rest_param(param);
                let param_ty = get_pat_type(param, ctx);
                let param_id = ctx.define_local(param_name.clone(), param_ty.clone());
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_ty,
                    default: param_default,
                    is_rest,
                });
                // Track destructuring patterns to generate extraction statements
                if is_destructuring_pattern(param) {
                    destructuring_params.push((param_id, param.clone()));
                }
            }

            // Register arrow function parameters with known native types as native instances
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
                        // HTTP/HTTPS types
                        "IncomingMessage" => Some(("http", "IncomingMessage")),
                        "ClientRequest" => Some(("http", "ClientRequest")),
                        "ServerResponse" => Some(("http", "ServerResponse")),
                        _ => None,
                    };
                    if let Some((module, class)) = native_info {
                        ctx.register_native_instance(param.name.clone(), module.to_string(), class.to_string());
                    }
                }
            }

            // Generate Let statements for destructuring patterns BEFORE lowering body
            // This ensures the destructured variable names are defined when the body references them
            let mut destructuring_stmts = Vec::new();
            for (param_id, pat) in &destructuring_params {
                let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
                destructuring_stmts.extend(stmts);
            }

            // Lower body
            let mut body = match &*arrow.body {
                ast::BlockStmtOrExpr::BlockStmt(block) => lower_block_stmt(ctx, block)?,
                ast::BlockStmtOrExpr::Expr(expr) => {
                    let return_expr = lower_expr(ctx, expr)?;
                    vec![Stmt::Return(Some(return_expr))]
                }
            };

            // Prepend destructuring statements to body
            if !destructuring_stmts.is_empty() {
                let mut new_body = destructuring_stmts;
                new_body.append(&mut body);
                body = new_body;
            }

            ctx.exit_scope(scope_mark);

            // Detect captured variables: locals referenced in the body that were defined in outer scope
            let mut all_refs = Vec::new();
                let mut visited_closures = std::collections::HashSet::new();
            for stmt in &body {
                collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
            }

            // Filter to only include outer locals (not parameters or locals defined within the closure)
            let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                .map(|(_, id)| *id)
                .collect();
            let param_ids: std::collections::HashSet<LocalId> = params.iter()
                .map(|p| p.id)
                .collect();

            // Find unique captures: refs that are in outer_locals but not params
            let mut captures: Vec<LocalId> = all_refs.into_iter()
                .filter(|id| outer_local_ids.contains(id) && !param_ids.contains(id))
                .collect();
            captures.sort();
            captures.dedup();

            // Detect which captures are assigned to inside the closure (need boxing)
            let mut all_assigned = Vec::new();
            for stmt in &body {
                collect_assigned_locals_stmt(stmt, &mut all_assigned);
            }
            let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
            let mutable_captures: Vec<LocalId> = captures.iter()
                .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                .copied()
                .collect();

            // Check if this arrow function uses `this` (needs to capture it from enclosing scope)
            let captures_this = closure_uses_this(&body);

            // Store enclosing class name for arrow functions that capture `this`
            let enclosing_class = if captures_this {
                ctx.current_class.clone()
            } else {
                None
            };

            Ok(Expr::Closure {
                func_id,
                params,
                return_type: Type::Any,
                body,
                captures,
                mutable_captures,
                captures_this,
                enclosing_class,
                is_async: arrow.is_async,
            })
        }
        ast::Expr::Fn(fn_expr) => {
            // Lower function expression to a closure (similar to arrow)
            let func_id = ctx.fresh_func();
            let scope_mark = ctx.enter_scope();

            // Track which locals exist before entering the closure scope
            let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                .map(|(name, id, _)| (name.clone(), *id))
                .collect();

            // Lower parameters and collect destructuring info
            let mut params = Vec::new();
            let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
            for param in &fn_expr.function.params {
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
                // Track destructuring patterns to generate extraction statements
                if is_destructuring_pattern(&param.pat) {
                    destructuring_params.push((param_id, param.pat.clone()));
                }
            }

            // Generate Let statements for destructuring patterns BEFORE lowering body
            let mut destructuring_stmts = Vec::new();
            for (param_id, pat) in &destructuring_params {
                let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
                destructuring_stmts.extend(stmts);
            }

            // Lower body
            let mut body = if let Some(ref block) = fn_expr.function.body {
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

            // Detect which captures are assigned to inside the closure (need boxing)
            let mut all_assigned = Vec::new();
            for stmt in &body {
                collect_assigned_locals_stmt(stmt, &mut all_assigned);
            }
            let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
            let mutable_captures: Vec<LocalId> = captures.iter()
                .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                .copied()
                .collect();

            // Regular function expressions do NOT capture `this` from enclosing scope
            // (they have their own `this` binding determined by how they're called)
            let captures_this = false;

            Ok(Expr::Closure {
                func_id,
                params,
                return_type: Type::Any,
                body,
                captures,
                mutable_captures,
                captures_this,
                enclosing_class: None,
                is_async: fn_expr.function.is_async,
            })
        }
        ast::Expr::Await(await_expr) => {
            let inner = Box::new(lower_expr(ctx, &await_expr.arg)?);
            Ok(Expr::Await(inner))
        }
        ast::Expr::SuperProp(super_prop) => {
            // super.property access - used in super.method() calls
            // When used as a call target, the Call expression will detect this
            // For now, we'll error on direct property access (super.prop without call)
            match &super_prop.prop {
                ast::SuperProp::Ident(_ident) => {
                    // This is typically used in Call expressions like super.method()
                    // We return a placeholder that will be handled specially
                    Err(anyhow!("Direct super property access not yet supported, use super.method()"))
                }
                ast::SuperProp::Computed(_) => {
                    Err(anyhow!("Computed super property access not supported"))
                }
            }
        }
        ast::Expr::Update(update) => {
            // Handle ++x, x++, --x, x--
            let binary_op = match update.op {
                ast::UpdateOp::PlusPlus => BinaryOp::Add,
                ast::UpdateOp::MinusMinus => BinaryOp::Sub,
            };

            match update.arg.as_ref() {
                // Simple identifier: x++ or ++x
                ast::Expr::Ident(ident) => {
                    let name = ident.sym.to_string();
                    let id = ctx.lookup_local(&name)
                        .ok_or_else(|| anyhow!("Undefined variable in update expression: {}", name))?;
                    let op = match update.op {
                        ast::UpdateOp::PlusPlus => UpdateOp::Increment,
                        ast::UpdateOp::MinusMinus => UpdateOp::Decrement,
                    };
                    Ok(Expr::Update {
                        id,
                        op,
                        prefix: update.prefix,
                    })
                }
                // Member expression: this.count++ or obj.prop++ or obj[key]++
                ast::Expr::Member(member) => {
                    let object = lower_expr(ctx, &member.obj)?;
                    match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let property = ident.sym.to_string();
                            // Desugar: this.count++ becomes (tmp = this.count, this.count = tmp + 1, tmp)
                            // For prefix ++this.count becomes (this.count = this.count + 1, this.count)
                            // We simplify to just: this.count = this.count + 1
                            // The return value semantics are handled at codegen
                            Ok(Expr::PropertyUpdate {
                                object: Box::new(object),
                                property,
                                op: binary_op,
                                prefix: update.prefix,
                            })
                        }
                        ast::MemberProp::PrivateName(priv_name) => {
                            let property = format!("#{}", priv_name.name);
                            Ok(Expr::PropertyUpdate {
                                object: Box::new(object),
                                property,
                                op: binary_op,
                                prefix: update.prefix,
                            })
                        }
                        ast::MemberProp::Computed(comp) => {
                            // Computed property: obj[key]++
                            let index = lower_expr(ctx, &comp.expr)?;
                            Ok(Expr::IndexUpdate {
                                object: Box::new(object),
                                index: Box::new(index),
                                op: binary_op,
                                prefix: update.prefix,
                            })
                        }
                    }
                }
                _ => Err(anyhow!("Update expression only supports identifiers and member expressions")),
            }
        }
        ast::Expr::Tpl(tpl) => {
            // Template literal: `Hello, ${name}!`
            // quasis = ["Hello, ", "!"], exprs = [name]
            // We desugar this to string concatenation

            if tpl.quasis.is_empty() {
                return Ok(Expr::String(String::new()));
            }

            // Start with the first quasi
            let first_raw = tpl.quasis.first()
                .map(|q| q.raw.as_ref())
                .unwrap_or("");
            let mut result = Expr::String(unescape_template(first_raw));

            // Interleave expressions and remaining quasis
            for (i, expr) in tpl.exprs.iter().enumerate() {
                let lowered = lower_expr(ctx, expr)?;
                // Concatenate: result + toString(expr)
                result = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(result),
                    right: Box::new(lowered),
                };

                // Add the next quasi (if it's non-empty)
                if let Some(quasi) = tpl.quasis.get(i + 1) {
                    let quasi_str: &str = quasi.raw.as_ref();
                    if !quasi_str.is_empty() {
                        result = Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(result),
                            right: Box::new(Expr::String(unescape_template(quasi_str))),
                        };
                    }
                }
            }

            Ok(result)
        }
        ast::Expr::OptChain(opt_chain) => {
            // Optional chaining: obj?.prop or obj?.[index] or obj?.method()
            // Convert to: obj == null ? undefined : obj.prop
            match &*opt_chain.base {
                ast::OptChainBase::Member(member) => {
                    // obj?.prop -> obj == null ? undefined : obj.prop
                    let obj_expr = lower_expr(ctx, &member.obj)?;

                    // Get the property access
                    let prop_expr = match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let prop_name = ident.sym.to_string();
                            Expr::PropertyGet {
                                object: Box::new(obj_expr.clone()),
                                property: prop_name,
                            }
                        }
                        ast::MemberProp::Computed(comp) => {
                            let index = lower_expr(ctx, &comp.expr)?;
                            Expr::IndexGet {
                                object: Box::new(obj_expr.clone()),
                                index: Box::new(index),
                            }
                        }
                        _ => return Err(anyhow!("Unsupported optional chain property type")),
                    };

                    // Generate: obj == null ? undefined : prop_expr
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::Eq,
                            left: Box::new(obj_expr),
                            right: Box::new(Expr::Null),
                        }),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(prop_expr),
                    })
                }
                ast::OptChainBase::Call(call) => {
                    // obj?.method() -> obj == null ? undefined : obj.method()
                    let callee = &call.callee;

                    // Check for spread arguments
                    let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

                    let args = call.args.iter()
                        .map(|arg| lower_expr(ctx, &arg.expr))
                        .collect::<Result<Vec<_>>>()?;

                    let callee_expr = lower_expr(ctx, callee)?;

                    // For method calls, we need to check the object
                    // This is simplified - full implementation would need more context
                    if has_spread {
                        let spread_args: Vec<CallArg> = call.args.iter().zip(args.iter())
                            .map(|(ast_arg, lowered)| {
                                if ast_arg.spread.is_some() {
                                    CallArg::Spread(lowered.clone())
                                } else {
                                    CallArg::Expr(lowered.clone())
                                }
                            })
                            .collect();
                        Ok(Expr::CallSpread {
                            callee: Box::new(callee_expr),
                            args: spread_args,
                            type_args: Vec::new(),
                        })
                    } else {
                        Ok(Expr::Call {
                            callee: Box::new(callee_expr),
                            args,
                            type_args: Vec::new(),
                        })
                    }
                }
            }
        }
        ast::Expr::TsAs(ts_as) => {
            // TypeScript 'as' type assertion - at runtime, just evaluate the expression
            // The type assertion is compile-time only
            lower_expr(ctx, &ts_as.expr)
        }
        ast::Expr::TsNonNull(ts_non_null) => {
            // TypeScript non-null assertion (value!) - at runtime, just the expression
            lower_expr(ctx, &ts_non_null.expr)
        }
        ast::Expr::TsTypeAssertion(ts_assertion) => {
            // TypeScript angle-bracket type assertion (<Type>value) - same as 'as', compile-time only
            lower_expr(ctx, &ts_assertion.expr)
        }
        ast::Expr::TsConstAssertion(ts_const) => {
            // TypeScript 'as const' assertion - at runtime, just evaluate the expression
            // The const assertion only affects type inference, not runtime behavior
            lower_expr(ctx, &ts_const.expr)
        }
        ast::Expr::TsSatisfies(ts_satisfies) => {
            // TypeScript 'satisfies' operator - compile-time type check only
            lower_expr(ctx, &ts_satisfies.expr)
        }
        ast::Expr::TsInstantiation(ts_inst) => {
            // TypeScript generic instantiation (func<Type>) - at runtime, just the expression
            lower_expr(ctx, &ts_inst.expr)
        }
        ast::Expr::Seq(seq) => {
            // Comma operator: evaluate all expressions left-to-right, return the last value
            // e.g., (a++, b++, c) evaluates a++, then b++, then returns c
            // All expressions must be evaluated for side effects (e.g., for-loop updates: it3--, i++)
            let mut exprs = Vec::new();
            for expr in &seq.exprs {
                exprs.push(lower_expr(ctx, expr)?);
            }
            if exprs.len() == 1 {
                Ok(exprs.pop().unwrap())
            } else {
                Ok(Expr::Sequence(exprs))
            }
        }
        ast::Expr::MetaProp(meta_prop) => {
            // import.meta expression
            // We only support import.meta itself - property access (.url) is handled in Member expr
            // For now, return a placeholder object that will be handled in property access
            match meta_prop.kind {
                ast::MetaPropKind::ImportMeta => {
                    // Return the file:// URL directly for import.meta.url
                    // Since import.meta is typically accessed via .url, we generate the URL here
                    let file_url = format!("file://{}", ctx.source_file_path);
                    Ok(Expr::Object(vec![
                        ("url".to_string(), Expr::String(file_url)),
                    ]))
                }
                ast::MetaPropKind::NewTarget => {
                    // new.target - not commonly used, return undefined for now
                    Ok(Expr::Undefined)
                }
            }
        }
        ast::Expr::Yield(y) => {
            let value = match &y.arg {
                Some(arg) => Some(Box::new(lower_expr(ctx, arg)?)),
                None => None,
            };
            Ok(Expr::Yield { value, delegate: y.delegate })
        }
        ast::Expr::TaggedTpl(tagged) => {
            // Tagged template literals: tag`...`
            // Currently only String.raw is supported — it returns the raw string
            // without escape processing (backslashes kept literal).
            let is_string_raw = match &*tagged.tag {
                ast::Expr::Member(member) => {
                    let obj_is_string = match &member.obj.as_ref() {
                        ast::Expr::Ident(id) => id.sym.as_ref() == "String",
                        _ => false,
                    };
                    let prop_is_raw = match &member.prop {
                        ast::MemberProp::Ident(id) => id.sym.as_ref() == "raw",
                        _ => false,
                    };
                    obj_is_string && prop_is_raw
                }
                _ => false,
            };

            if !is_string_raw {
                return Err(anyhow!("Unsupported tagged template literal (only String.raw is supported): {:?}", tagged.tag));
            }

            let tpl = &*tagged.tpl;
            if tpl.quasis.is_empty() {
                return Ok(Expr::String(String::new()));
            }

            // For String.raw, use raw strings directly (no escape processing)
            let first_raw = tpl.quasis.first()
                .map(|q| q.raw.as_ref())
                .unwrap_or("");
            let mut result = Expr::String(first_raw.to_string());

            for (i, expr) in tpl.exprs.iter().enumerate() {
                let lowered = lower_expr(ctx, expr)?;
                result = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(result),
                    right: Box::new(lowered),
                };

                if let Some(quasi) = tpl.quasis.get(i + 1) {
                    let quasi_str: &str = quasi.raw.as_ref();
                    if !quasi_str.is_empty() {
                        result = Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(result),
                            right: Box::new(Expr::String(quasi_str.to_string())),
                        };
                    }
                }
            }

            Ok(result)
        }
        // Class expression used as a value (not in `new` context)
        ast::Expr::Class(class_expr) => {
            let ident_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
            let synthetic_name = ident_name.unwrap_or_else(|| format!("__anon_class_{}", ctx.fresh_class()));
            let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
            ctx.pending_classes.push(class);
            // Return as a New expression with no args (creates the class object reference)
            Ok(Expr::New { class_name: synthetic_name, args: vec![], type_args: vec![] })
        }
        ast::Expr::JSXElement(jsx) => {
            lower_jsx_element(ctx, jsx)
        }
        ast::Expr::JSXFragment(jsx) => {
            lower_jsx_fragment(ctx, jsx)
        }
        _ => Err(anyhow!("Unsupported expression type: {:?}", expr)),
    }
}

/// Unescape template literal strings (handle \n, \t, etc.)
fn _unescape_template() {}

/// Try to lower a Widget({...}) call from perry/widget into a WidgetDecl.
/// Returns Some(WidgetDecl) if this is a widget declaration, None otherwise.
fn try_lower_widget_decl(
    ctx: &LoweringContext,
    call_expr: &ast::CallExpr,
) -> Option<WidgetDecl> {
    // Check callee is a function imported from perry/widget named "Widget"
    let callee = match &call_expr.callee {
        ast::Callee::Expr(expr) => expr,
        _ => return None,
    };
    let func_name = match callee.as_ref() {
        ast::Expr::Ident(ident) => ident.sym.as_ref(),
        _ => return None,
    };
    let (module, method) = ctx.lookup_native_module(func_name)?;
    if module != "perry/widget" {
        return None;
    }
    let method_name = method.unwrap_or(func_name);
    if method_name != "Widget" {
        return None;
    }

    // First arg should be the config object literal
    let config_obj = match call_expr.args.first() {
        Some(arg) => match arg.expr.as_ref() {
            ast::Expr::Object(obj) => obj,
            _ => return None,
        },
        None => return None,
    };

    let mut kind = String::new();
    let mut display_name = String::new();
    let mut description = String::new();
    let mut supported_families: Vec<String> = Vec::new();
    let mut entry_fields: Vec<(String, WidgetFieldType)> = Vec::new();
    let mut render_body: Vec<WidgetNode> = Vec::new();
    let mut entry_param_name = "entry".to_string();

    for prop in &config_obj.props {
        let kv = match prop {
            ast::PropOrSpread::Prop(p) => match p.as_ref() {
                ast::Prop::KeyValue(kv) => kv,
                ast::Prop::Method(method) => {
                    let key = prop_name_to_string(&method.key);
                    if key == "render" {
                        // Extract parameter name
                        if let Some(param) = method.function.params.first() {
                            if let ast::Pat::Ident(ident) = &param.pat {
                                entry_param_name = ident.id.sym.to_string();
                            }
                        }
                        // Extract type annotation for entry fields (only if not already specified via entryFields)
                        if entry_fields.is_empty() {
                            if let Some(param) = method.function.params.first() {
                                extract_entry_fields_from_param(&param.pat, &mut entry_fields);
                            }
                        }
                        // Parse render body
                        if let Some(body) = &method.function.body {
                            for stmt in &body.stmts {
                                if let ast::Stmt::Return(ret) = stmt {
                                    if let Some(arg) = &ret.arg {
                                        if let Some(node) = parse_widget_node(arg) {
                                            render_body.push(node);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
                _ => continue,
            },
            _ => continue,
        };

        let key = prop_name_to_string(&kv.key);
        match key.as_str() {
            "kind" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    kind = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "displayName" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    display_name = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "description" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    description = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "supportedFamilies" => {
                if let ast::Expr::Array(arr) = kv.value.as_ref() {
                    for elem in &arr.elems {
                        if let Some(ast::ExprOrSpread { expr, .. }) = elem {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                                supported_families.push(s.value.as_str().unwrap_or("").to_string());
                            }
                        }
                    }
                }
            }
            "entryFields" => {
                // Allow explicit entry field declarations
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let field_name = prop_name_to_string(&field_kv.key);
                                let field_type = match field_kv.value.as_ref() {
                                    ast::Expr::Lit(ast::Lit::Str(s)) => {
                                        match s.value.as_str().unwrap_or("") {
                                            "number" => WidgetFieldType::Number,
                                            "boolean" => WidgetFieldType::Boolean,
                                            _ => WidgetFieldType::String,
                                        }
                                    }
                                    _ => WidgetFieldType::String,
                                };
                                entry_fields.push((field_name, field_type));
                            }
                        }
                    }
                }
            }
            "render" => {
                // Arrow function: render: (entry) => VStack(...)
                match kv.value.as_ref() {
                    ast::Expr::Arrow(arrow) => {
                        // Extract parameter name
                        if let Some(param) = arrow.params.first() {
                            if let ast::Pat::Ident(ident) = param {
                                entry_param_name = ident.id.sym.to_string();
                            }
                        }
                        // Extract entry fields from type annotation (only if not already specified via entryFields)
                        if entry_fields.is_empty() {
                            if let Some(param) = arrow.params.first() {
                                extract_entry_fields_from_param(param, &mut entry_fields);
                            }
                        }
                        // Parse body
                        match arrow.body.as_ref() {
                            ast::BlockStmtOrExpr::Expr(expr) => {
                                if let Some(node) = parse_widget_node(expr) {
                                    render_body.push(node);
                                }
                            }
                            ast::BlockStmtOrExpr::BlockStmt(block) => {
                                for stmt in &block.stmts {
                                    if let ast::Stmt::Return(ret) = stmt {
                                        if let Some(arg) = &ret.arg {
                                            if let Some(node) = parse_widget_node(arg) {
                                                render_body.push(node);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {} // Skip timeline and other fields handled differently
        }
    }

    if kind.is_empty() {
        kind = "com.perry.widget".to_string();
    }

    Some(WidgetDecl {
        kind,
        display_name,
        description,
        supported_families,
        entry_fields,
        render_body,
        entry_param_name,
    })
}

/// Extract entry fields from a typed parameter pattern (e.g., `entry: MyEntry`)
fn extract_entry_fields_from_param(pat: &ast::Pat, fields: &mut Vec<(String, WidgetFieldType)>) {
    // Try to get type annotation
    let type_ann = match pat {
        ast::Pat::Ident(ident) => ident.type_ann.as_ref(),
        _ => None,
    };
    if let Some(ann) = type_ann {
        if let ast::TsType::TsTypeLit(lit) = ann.type_ann.as_ref() {
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        let field_name = ident.sym.to_string();
                        // Skip 'date' as it's always present in TimelineEntry
                        if field_name == "date" {
                            continue;
                        }
                        let field_type = if let Some(ann) = &prop.type_ann {
                            match ann.type_ann.as_ref() {
                                ast::TsType::TsKeywordType(kw) => match kw.kind {
                                    ast::TsKeywordTypeKind::TsNumberKeyword => WidgetFieldType::Number,
                                    ast::TsKeywordTypeKind::TsBooleanKeyword => WidgetFieldType::Boolean,
                                    _ => WidgetFieldType::String,
                                },
                                _ => WidgetFieldType::String,
                            }
                        } else {
                            WidgetFieldType::String
                        };
                        fields.push((field_name, field_type));
                    }
                }
            }
        }
    }
}

/// Parse a widget node from an AST expression.
/// Recognizes calls like Text("hello"), VStack({...}, [...]), Image({systemName: "star"}), etc.
fn parse_widget_node(expr: &ast::Expr) -> Option<WidgetNode> {
    match expr {
        ast::Expr::Call(call) => {
            let func_name = match &call.callee {
                ast::Callee::Expr(e) => match e.as_ref() {
                    ast::Expr::Ident(ident) => ident.sym.to_string(),
                    _ => return None,
                },
                _ => return None,
            };

            match func_name.as_str() {
                "Text" => {
                    let content = call.args.first()
                        .map(|arg| parse_text_content(&arg.expr))
                        .unwrap_or(WidgetTextContent::Literal(String::new()));
                    let modifiers = parse_modifiers_from_args(&call.args, 1);
                    Some(WidgetNode::Text { content, modifiers })
                }
                "VStack" | "HStack" | "ZStack" => {
                    let kind = match func_name.as_str() {
                        "VStack" => WidgetStackKind::VStack,
                        "HStack" => WidgetStackKind::HStack,
                        "ZStack" => WidgetStackKind::ZStack,
                        _ => unreachable!(),
                    };
                    parse_stack_node(kind, &call.args)
                }
                "Image" => {
                    parse_image_node(&call.args)
                }
                "Spacer" => {
                    Some(WidgetNode::Spacer)
                }
                _ => None,
            }
        }
        ast::Expr::Cond(cond) => {
            // Ternary: condition ? then : else
            parse_conditional_node(cond)
        }
        _ => None,
    }
}

/// Parse text content from an expression
fn parse_text_content(expr: &ast::Expr) -> WidgetTextContent {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            WidgetTextContent::Literal(s.value.as_str().unwrap_or("").to_string())
        }
        ast::Expr::Member(member) => {
            // entry.fieldName
            if let ast::MemberProp::Ident(prop) = &member.prop {
                WidgetTextContent::Field(prop.sym.to_string())
            } else {
                WidgetTextContent::Literal(String::new())
            }
        }
        ast::Expr::Tpl(tpl) => {
            // Template literal: `Score: ${entry.score}`
            let mut parts = Vec::new();
            for (i, quasi) in tpl.quasis.iter().enumerate() {
                let raw = quasi.raw.as_ref().to_string();
                if !raw.is_empty() {
                    parts.push(WidgetTemplatePart::Literal(raw));
                }
                if i < tpl.exprs.len() {
                    if let ast::Expr::Member(member) = tpl.exprs[i].as_ref() {
                        if let ast::MemberProp::Ident(prop) = &member.prop {
                            parts.push(WidgetTemplatePart::Field(prop.sym.to_string()));
                        }
                    }
                }
            }
            WidgetTextContent::Template(parts)
        }
        _ => WidgetTextContent::Literal(String::new()),
    }
}

/// Parse a stack node (VStack, HStack, ZStack) from call arguments.
/// Supports two patterns:
///   VStack([child1, child2])
///   VStack({ spacing: 8 }, [child1, child2])
fn parse_stack_node(kind: WidgetStackKind, args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let mut spacing = None;
    let mut children = Vec::new();
    let mut modifiers = Vec::new();
    let mut children_arg_idx = 0;

    // Check if first arg is config object
    if let Some(first) = args.first() {
        match first.expr.as_ref() {
            ast::Expr::Object(obj) => {
                // First arg is config: { spacing: 8 }
                for prop in &obj.props {
                    if let ast::PropOrSpread::Prop(p) = prop {
                        if let ast::Prop::KeyValue(kv) = p.as_ref() {
                            let key = prop_name_to_string(&kv.key);
                            if key == "spacing" {
                                if let ast::Expr::Lit(ast::Lit::Num(n)) = kv.value.as_ref() {
                                    spacing = Some(n.value);
                                }
                            }
                        }
                    }
                }
                children_arg_idx = 1;
            }
            ast::Expr::Array(_) => {
                // First arg is children array directly
                children_arg_idx = 0;
            }
            _ => {}
        }
    }

    // Parse children array
    if let Some(arg) = args.get(children_arg_idx) {
        if let ast::Expr::Array(arr) = arg.expr.as_ref() {
            for elem in &arr.elems {
                if let Some(ast::ExprOrSpread { expr, .. }) = elem {
                    if let Some(node) = parse_widget_node(expr) {
                        children.push(node);
                    }
                }
            }
        }
    }

    // Parse modifiers from remaining args
    let modifier_start = children_arg_idx + 1;
    modifiers = parse_modifiers_from_args(args, modifier_start);

    Some(WidgetNode::Stack { kind, spacing, children, modifiers })
}

/// Parse an Image node from call arguments.
/// Image({ systemName: "star.fill" })
fn parse_image_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let first = args.first()?;
    let system_name = match first.expr.as_ref() {
        ast::Expr::Object(obj) => {
            let mut name = String::new();
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if key == "systemName" {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                name = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                    }
                }
            }
            name
        }
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
        _ => return None,
    };

    let modifiers = parse_modifiers_from_args(args, 1);
    Some(WidgetNode::Image { system_name, modifiers })
}

/// Parse a conditional node from a ternary expression
fn parse_conditional_node(cond: &ast::CondExpr) -> Option<WidgetNode> {
    // Parse condition: entry.field > value, entry.field == value, etc.
    let (field, op, value) = parse_condition(&cond.test)?;
    let then_node = parse_widget_node(&cond.cons)?;
    let else_node = parse_widget_node(&cond.alt);

    Some(WidgetNode::Conditional {
        field,
        op,
        value,
        then_node: Box::new(then_node),
        else_node: else_node.map(Box::new),
    })
}

/// Parse a binary condition expression
fn parse_condition(expr: &ast::Expr) -> Option<(String, WidgetConditionOp, WidgetTextContent)> {
    match expr {
        ast::Expr::Bin(bin) => {
            let field = match bin.left.as_ref() {
                ast::Expr::Member(member) => {
                    if let ast::MemberProp::Ident(prop) = &member.prop {
                        prop.sym.to_string()
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let op = match bin.op {
                ast::BinaryOp::Gt => WidgetConditionOp::GreaterThan,
                ast::BinaryOp::Lt => WidgetConditionOp::LessThan,
                ast::BinaryOp::EqEq | ast::BinaryOp::EqEqEq => WidgetConditionOp::Equals,
                ast::BinaryOp::NotEq | ast::BinaryOp::NotEqEq => WidgetConditionOp::NotEquals,
                _ => return None,
            };
            let value = parse_text_content(&bin.right);
            Some((field, op, value))
        }
        ast::Expr::Member(member) => {
            // Truthy check: entry.isActive
            if let ast::MemberProp::Ident(prop) = &member.prop {
                Some((
                    prop.sym.to_string(),
                    WidgetConditionOp::Truthy,
                    WidgetTextContent::Literal(String::new()),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse modifiers from a chained method call or from arguments.
/// In the TypeScript API, modifiers are passed as the last argument (object):
///   Text("hello", { font: "title", fontWeight: "bold", foregroundColor: "blue" })
fn parse_modifiers_from_args(args: &[ast::ExprOrSpread], start_idx: usize) -> Vec<WidgetModifier> {
    let mut modifiers = Vec::new();
    if let Some(arg) = args.get(start_idx) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if let Some(m) = parse_single_modifier(&key, &kv.value) {
                            modifiers.push(m);
                        }
                    }
                }
            }
        }
    }
    modifiers
}

/// Parse a single modifier from key/value
fn parse_single_modifier(key: &str, value: &ast::Expr) -> Option<WidgetModifier> {
    match key {
        "font" => {
            match value {
                ast::Expr::Lit(ast::Lit::Str(s)) => {
                    let font = match s.value.as_str().unwrap_or("") {
                        "headline" => WidgetFont::Headline,
                        "title" => WidgetFont::Title,
                        "title2" => WidgetFont::Title2,
                        "title3" => WidgetFont::Title3,
                        "body" => WidgetFont::Body,
                        "caption" => WidgetFont::Caption,
                        "caption2" => WidgetFont::Caption2,
                        "footnote" => WidgetFont::Footnote,
                        "subheadline" => WidgetFont::Subheadline,
                        "largeTitle" => WidgetFont::LargeTitle,
                        name => WidgetFont::Named(name.to_string()),
                    };
                    Some(WidgetModifier::Font(font))
                }
                ast::Expr::Lit(ast::Lit::Num(n)) => {
                    Some(WidgetModifier::Font(WidgetFont::System(n.value)))
                }
                _ => None,
            }
        }
        "fontWeight" | "weight" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::FontWeight(s.value.as_str().unwrap_or("").to_string()))
            } else {
                None
            }
        }
        "foregroundColor" | "color" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::ForegroundColor(s.value.as_str().unwrap_or("").to_string()))
            } else {
                None
            }
        }
        "padding" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::Padding(n.value))
            } else {
                None
            }
        }
        "cornerRadius" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::CornerRadius(n.value))
            } else {
                None
            }
        }
        "background" | "backgroundColor" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::Background(s.value.as_str().unwrap_or("").to_string()))
            } else {
                None
            }
        }
        "opacity" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::Opacity(n.value))
            } else {
                None
            }
        }
        "lineLimit" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::LineLimit(n.value as u32))
            } else {
                None
            }
        }
        "frame" => {
            if let ast::Expr::Object(obj) = value {
                let mut width = None;
                let mut height = None;
                for prop in &obj.props {
                    if let ast::PropOrSpread::Prop(p) = prop {
                        if let ast::Prop::KeyValue(kv) = p.as_ref() {
                            let k = prop_name_to_string(&kv.key);
                            if let ast::Expr::Lit(ast::Lit::Num(n)) = kv.value.as_ref() {
                                match k.as_str() {
                                    "width" => width = Some(n.value),
                                    "height" => height = Some(n.value),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Some(WidgetModifier::Frame { width, height })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract a property name from a PropName
fn prop_name_to_string(name: &ast::PropName) -> String {
    match name {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        ast::PropName::Num(n) => format!("{}", n.value),
        _ => String::new(),
    }
}
