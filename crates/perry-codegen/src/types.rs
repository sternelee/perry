//! Type definitions for the codegen module.

use std::collections::{BTreeMap, HashMap};
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
    pub field_indices: BTreeMap<String, u32>,
    /// Mapping from field name to type (includes inherited fields)
    pub field_types: BTreeMap<String, perry_types::Type>,
    /// Constructor function ID (if any)
    pub constructor_id: Option<cranelift_module::FuncId>,
    /// Method function IDs: method name -> func_id (includes inherited methods)
    pub method_ids: BTreeMap<String, cranelift_module::FuncId>,
    /// Getter function IDs: property name -> func_id
    pub getter_ids: BTreeMap<String, cranelift_module::FuncId>,
    /// Setter function IDs: property name -> func_id
    pub setter_ids: BTreeMap<String, cranelift_module::FuncId>,
    /// Static method function IDs: method name -> func_id
    pub static_method_ids: BTreeMap<String, cranelift_module::FuncId>,
    /// Static field global IDs: field name -> (data_id, has_init)
    pub static_field_ids: BTreeMap<String, cranelift_module::DataId>,
    /// Method parameter counts: method name -> param count (for vtable registration)
    pub method_param_counts: BTreeMap<String, usize>,
    /// Method return types: method name -> return type (for determining if method returns string)
    pub method_return_types: BTreeMap<String, perry_types::Type>,
    /// Static method return types: method name -> return type (for singleton pattern getInstance() etc.)
    pub static_method_return_types: BTreeMap<String, perry_types::Type>,
    /// Type parameters of the class (e.g., ["T"] for class Box<T>)
    pub type_params: Vec<String>,
    /// Field default initializer expressions: field name -> init expr
    /// Used by Expr::New to initialize field defaults before calling constructor
    pub field_inits: BTreeMap<String, Expr>,
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
