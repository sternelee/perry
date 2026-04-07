//! Type definitions for the codegen module.

use std::collections::HashMap;
use cranelift::prelude::*;
use cranelift_codegen::ir::Block;
use cranelift_frontend::Variable;
use cranelift_module;
use perry_hir::Expr;
use perry_types::LocalId;

/// Information about a local variable
#[derive(Debug, Clone)]
pub(crate) struct LocalInfo {
    /// The Cranelift variable
    pub var: Variable,
    /// The variable name (for runtime lookup of dynamic constructors)
    pub name: Option<String>,
    /// If this is an object, what class is it? (None for primitives)
    pub class_name: Option<String>,
    /// Type arguments for generic class instances (e.g., Box<string> has type_args = [Type::String])
    pub type_args: Vec<perry_types::Type>,
    /// Is this stored as i64 (pointer) or f64 (number)?
    pub is_pointer: bool,
    /// Is this an array?
    pub is_array: bool,
    /// Is this a string?
    pub is_string: bool,
    /// Is this a bigint?
    pub is_bigint: bool,
    /// Is this a closure?
    pub is_closure: bool,
    /// If this is a closure, its HIR func_id (for rest param lookup etc.)
    pub closure_func_id: Option<u32>,
    /// Is this a boxed mutable capture? (stored as box pointer)
    pub is_boxed: bool,
    /// Is this a Map?
    pub is_map: bool,
    /// Is this a Set?
    pub is_set: bool,
    /// Is this a Buffer?
    pub is_buffer: bool,
    /// Is this an EventEmitter?
    pub is_event_emitter: bool,
    /// Is this a union type? (uses dynamic typing at runtime)
    pub is_union: bool,
    /// Is this a mixed-type array? (element type is union or any)
    pub is_mixed_array: bool,
    /// Is this an integer value? (for native i64 arithmetic optimization)
    pub is_integer: bool,
    /// Is this an integer-only array? (SMI array optimization)
    pub is_integer_array: bool,
    /// Is this variable stored as native i32? (loop counter optimization)
    pub is_i32: bool,
    /// Is this a boolean value? (from comparisons - stored as NaN-boxed TAG_TRUE/TAG_FALSE)
    pub is_boolean: bool,
    /// Shadow i32 variable for integer values (optimization for array indexing)
    pub i32_shadow: Option<Variable>,
    /// Bounds check elimination: if this index variable is bounded by an array's length
    pub bounded_by_array: Option<LocalId>,
    /// Bounds check elimination: if this index variable is bounded by a constant
    pub bounded_by_constant: Option<i64>,
    /// Scalar-replaced fields (escape analysis): maps field names to scalar variables
    /// When set, PropertyGet/PropertySet use these variables instead of heap loads/stores
    pub scalar_fields: Option<HashMap<String, Variable>>,
    /// CSE: cached value of this variable squared (var * var)
    /// When set, Binary { op: Mul, left: LocalGet(id), right: LocalGet(id) } uses this
    pub squared_cache: Option<Variable>,
    /// CSE: cached products with other variables (var * other_var)
    /// Maps other_var_id -> cache_variable for x*y patterns
    pub product_cache: Option<HashMap<LocalId, Variable>>,
    /// Cached raw I64 pointer for arrays (avoids redundant js_nanbox_get_pointer calls in loops)
    pub cached_array_ptr: Option<Variable>,
    /// Compile-time constant value for const variables initialized with literals
    pub const_value: Option<f64>,
    /// LICM: Hoisted element loads from invariant array accesses (arr[outer_idx] hoisted out of inner loop)
    /// Maps index_var_id -> cache_variable containing the pre-loaded f64 value
    pub hoisted_element_loads: Option<HashMap<LocalId, Variable>>,
    /// LICM: Hoisted i32 products from invariant index computations (i*size hoisted out of inner loop)
    /// Maps other_var_id -> cache_variable containing the pre-computed i32 product
    pub hoisted_i32_products: Option<HashMap<LocalId, Variable>>,
    /// If this is a module-level variable, the DataId of its global slot.
    /// Used by closure capture code to share storage with named functions.
    pub module_var_data_id: Option<cranelift_module::DataId>,
    /// If this variable was assigned a class reference (e.g., `const cls = MyClass`),
    /// store the class name so `new cls()` can be resolved to `new MyClass()`.
    pub class_ref_name: Option<String>,
    /// Known object field ordering (from object literal assignment).
    /// Maps field name to index for direct offset access instead of hash lookup.
    pub object_field_indices: Option<HashMap<String, u32>>,
}

impl LocalInfo {
    /// Determines the Cranelift type for this variable when used as a module-level global slot.
    /// Must match the type used in the module init (stmt.rs) to ensure consistency.
    /// - Boxed vars (mutable captures) hold a raw box pointer → I64
    /// - Pointer types without union → I64 (raw pointer)
    /// - Everything else → F64 (NaN-boxed)
    ///
    /// The boxed-var case is critical: the module init's stmt.rs path declares
    /// box_var as I64 and stores it to the global slot. Functions/closures/methods
    /// that load this slot must use I64 too — otherwise they read the box pointer
    /// bits as an f64 value, then pass an F64 to js_box_set, causing either a
    /// Cranelift type mismatch or (after a stray bitcast) a NULL box pointer crash.
    pub fn cranelift_var_type(&self) -> cranelift::prelude::types::Type {
        if self.is_boxed {
            cranelift::prelude::types::I64
        } else if self.is_pointer && !self.is_union {
            cranelift::prelude::types::I64
        } else {
            cranelift::prelude::types::F64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift::prelude::types;
    use std::collections::HashMap;

    /// Create a default LocalInfo with all flags false (number variable).
    fn default_local() -> LocalInfo {
        LocalInfo {
            var: Variable::new(0),
            name: None,
            class_name: None,
            type_args: Vec::new(),
            is_pointer: false,
            is_array: false,
            is_string: false,
            is_bigint: false,
            is_closure: false,
            closure_func_id: None,
            is_boxed: false,
            is_map: false,
            is_set: false,
            is_buffer: false,
            is_event_emitter: false,
            is_union: false,
            is_mixed_array: false,
            is_integer: false,
            is_integer_array: false,
            is_i32: false,
            is_boolean: false,
            i32_shadow: None,
            bounded_by_array: None,
            bounded_by_constant: None,
            scalar_fields: None,
            squared_cache: None,
            product_cache: None,
            cached_array_ptr: None,
            const_value: None,
            hoisted_element_loads: None,
            hoisted_i32_products: None,
            module_var_data_id: None,
            class_ref_name: None,
            object_field_indices: None,
        }
    }

    #[test]
    fn number_var_uses_f64() {
        let info = default_local();
        assert_eq!(info.cranelift_var_type(), types::F64);
    }

    #[test]
    fn pointer_var_uses_i64() {
        let mut info = default_local();
        info.is_pointer = true;
        assert_eq!(info.cranelift_var_type(), types::I64);
    }

    #[test]
    fn array_pointer_uses_i64() {
        let mut info = default_local();
        info.is_pointer = true;
        info.is_array = true;
        assert_eq!(info.cranelift_var_type(), types::I64);
    }

    #[test]
    fn union_pointer_uses_f64() {
        // Union types are NaN-boxed and must use F64
        let mut info = default_local();
        info.is_pointer = true;
        info.is_union = true;
        assert_eq!(info.cranelift_var_type(), types::F64);
    }

    /// Regression test for the Android FP flush-to-zero bug (v0.4.28).
    /// An untyped array (`const CX = []`, ty=Unknown) must NOT have is_union=true
    /// when is_array=true. If is_union were set, cranelift_var_type() returns F64,
    /// but the module init stores the value as I64. Loading an I64 pointer as F64
    /// corrupts it on platforms with FP flush-to-zero (Android ARM).
    #[test]
    fn untyped_array_not_union_android_regression() {
        let mut info = default_local();
        info.is_pointer = true;
        info.is_array = true;
        // This is the key: even if the HIR type is Unknown/Any, is_union must NOT
        // be set when we know the concrete type is array. analyze_module_var_types
        // in codegen.rs must match this expectation.
        info.is_union = false;
        assert_eq!(info.cranelift_var_type(), types::I64,
            "Untyped arrays (ty=Unknown) with is_array=true must use I64, not F64. \
             F64 corrupts pointers on Android ARM (FP flush-to-zero).");
    }

    /// Verify that if is_union is incorrectly set on an array, we get F64 (the bug).
    /// This documents the failure mode so we can ensure analyze_module_var_types
    /// never produces this combination.
    #[test]
    fn union_array_would_use_f64_documents_bug() {
        let mut info = default_local();
        info.is_pointer = true;
        info.is_array = true;
        info.is_union = true; // BUG: this should never happen for arrays
        // This test documents the broken behavior: is_union overrides is_pointer
        assert_eq!(info.cranelift_var_type(), types::F64,
            "Documents the bug: if is_union is incorrectly set on an array, F64 is used");
    }

    #[test]
    fn closure_pointer_uses_i64() {
        let mut info = default_local();
        info.is_pointer = true;
        info.is_closure = true;
        assert_eq!(info.cranelift_var_type(), types::I64);
    }

    #[test]
    fn map_pointer_uses_i64() {
        let mut info = default_local();
        info.is_pointer = true;
        info.is_map = true;
        assert_eq!(info.cranelift_var_type(), types::I64);
    }

    #[test]
    fn set_pointer_uses_i64() {
        let mut info = default_local();
        info.is_pointer = true;
        info.is_set = true;
        assert_eq!(info.cranelift_var_type(), types::I64);
    }

    #[test]
    fn string_not_pointer_uses_f64() {
        // Strings are NaN-boxed (STRING_TAG), stored as F64
        let mut info = default_local();
        info.is_string = true;
        // is_pointer is false for strings in module vars
        assert_eq!(info.cranelift_var_type(), types::F64);
    }
}

/// Metadata about a compiled class
#[derive(Debug, Clone)]
pub(crate) struct ClassMeta {
    /// Class ID
    pub id: u32,
    /// Parent class name (for inheritance)
    pub parent_class: Option<String>,
    /// Native parent class (module, class_name) - e.g., ("events", "EventEmitter")
    pub native_parent: Option<(String, String)>,
    /// Number of own fields (not including inherited)
    pub own_field_count: u32,
    /// Total number of fields (including inherited)
    pub field_count: u32,
    /// Mapping from field name to index (includes inherited fields)
    pub field_indices: HashMap<String, u32>,
    /// Mapping from field name to type (includes inherited fields)
    pub field_types: HashMap<String, perry_types::Type>,
    /// Constructor function ID (if any)
    pub constructor_id: Option<cranelift_module::FuncId>,
    /// Method function IDs: method name -> func_id (includes inherited methods)
    pub method_ids: HashMap<String, cranelift_module::FuncId>,
    /// Getter function IDs: property name -> func_id
    pub getter_ids: HashMap<String, cranelift_module::FuncId>,
    /// Setter function IDs: property name -> func_id
    pub setter_ids: HashMap<String, cranelift_module::FuncId>,
    /// Static method function IDs: method name -> func_id
    pub static_method_ids: HashMap<String, cranelift_module::FuncId>,
    /// Static field global IDs: field name -> (data_id, has_init)
    pub static_field_ids: HashMap<String, cranelift_module::DataId>,
    /// Method parameter counts: method name -> param count (for vtable registration)
    pub method_param_counts: HashMap<String, usize>,
    /// Method return types: method name -> return type (for determining if method returns string)
    pub method_return_types: HashMap<String, perry_types::Type>,
    /// Static method return types: method name -> return type (for singleton pattern getInstance() etc.)
    pub static_method_return_types: HashMap<String, perry_types::Type>,
    /// Type parameters of the class (e.g., ["T"] for class Box<T>)
    pub type_params: Vec<String>,
    /// Field default initializer expressions: field name -> init expr
    /// Used by Expr::New to initialize field defaults before calling constructor
    pub field_inits: HashMap<String, Expr>,
}

/// Enum member value (resolved at compile time)
#[derive(Clone)]
pub enum EnumMemberValue {
    Number(i64),
    String(String),
}

/// Context for 'this' in constructors/methods
pub(crate) struct ThisContext {
    /// Variable holding 'this' pointer (i64)
    pub this_var: Variable,
    /// Class metadata for field lookup
    pub class_meta: ClassMeta,
}

/// Context for loops (for break/continue and bounds check elimination)
pub(crate) struct LoopContext {
    /// Block to jump to for 'break'
    pub exit_block: Block,
    /// Block to jump to for 'continue'
    pub header_block: Block,
    /// Codegen-time try-catch depth when this loop was entered.
    /// break/continue emit (current_depth - try_depth) js_try_end() calls.
    pub try_depth: usize,
    /// Bounds check elimination: maps index variable to (array_var, cached_length_value)
    /// When i is bounded by arr.length, arr[i] can skip bounds check
    pub bounded_indices: HashMap<LocalId, (LocalId, Value)>,
}
