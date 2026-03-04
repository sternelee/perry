//! High-level Intermediate Representation (HIR) for Perry
//!
//! The HIR is a typed, simplified representation of TypeScript code
//! that is easier to analyze and transform than the raw AST.

pub mod ir;
pub mod js_transform;
pub mod lower;
pub mod monomorph;

pub use ir::*;
pub use js_transform::{transform_js_imports, fix_cross_module_native_instances, fix_local_native_instances, ExportedNativeInstance};
pub use lower::{lower_module, lower_module_with_class_id, lower_module_with_class_id_and_types, fix_imported_enums};
pub use monomorph::monomorphize_module;
