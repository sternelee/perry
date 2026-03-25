//! Runtime function declarations for the Cranelift codegen.
//!
//! Contains the `declare_runtime_functions` method which declares all FFI
//! function signatures needed by the runtime (console, NaN-boxing, objects,
//! arrays, strings, math, file system, BigInt, stdlib modules, UI, plugins, etc.).

use anyhow::Result;
use cranelift::prelude::*;
use cranelift_codegen::ir::AbiParam;
use cranelift_module::{Linkage, Module};
use std::borrow::Cow;

use crate::codegen::Compiler;

impl Compiler {
    pub(crate) fn declare_runtime_functions(&mut self) -> Result<()> {
        // Declare js_console_log_number(f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_console_log_number",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_log_number"), func_id);
        }

        // Declare js_console_log_dynamic(f64) -> void (for union types)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_console_log_dynamic",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_log_dynamic"), func_id);
        }

        // Declare js_console_error_number(f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_console_error_number",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_error_number"), func_id);
        }

        // Declare js_console_error_dynamic(f64) -> void (for union types)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_console_error_dynamic",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_error_dynamic"), func_id);
        }

        // Declare js_console_warn_number(f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_console_warn_number",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_warn_number"), func_id);
        }

        // Declare js_console_warn_dynamic(f64) -> void (for union types)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_console_warn_dynamic",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_warn_dynamic"), func_id);
        }

        // Declare js_string_error(i64) -> void (for console.error with strings)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(
                "js_string_error",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_error"), func_id);
        }

        // Declare js_string_warn(i64) -> void (for console.warn with strings)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(
                "js_string_warn",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_warn"), func_id);
        }

        // Declare js_bigint_error(i64) -> void (for console.error with bigints)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(
                "js_bigint_error",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_error"), func_id);
        }

        // Declare js_bigint_warn(i64) -> void (for console.warn with bigints)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(
                "js_bigint_warn",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_warn"), func_id);
        }

        // Declare js_console_log_spread(arr: *const ArrayHeader) -> void (for console.log with spread)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_console_log_spread",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_log_spread"), func_id);
        }

        // Declare js_console_error_spread(arr: *const ArrayHeader) -> void (for console.error with spread)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_console_error_spread",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_error_spread"), func_id);
        }

        // Declare js_console_warn_spread(arr: *const ArrayHeader) -> void (for console.warn with spread)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_console_warn_spread",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_console_warn_spread"), func_id);
        }

        // Declare js_array_print(arr: *const ArrayHeader) -> void (for console.log with array)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_array_print",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_print"), func_id);
        }

        // Declare js_nanbox_pointer(i64) -> f64 (for union types with pointers)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // raw pointer
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed pointer
            let func_id = self.module.declare_function(
                "js_nanbox_pointer",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanbox_pointer"), func_id);
        }

        // Declare js_nanbox_string(i64) -> f64 (for union types with strings)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // raw string pointer
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed string (uses STRING_TAG)
            let func_id = self.module.declare_function(
                "js_nanbox_string",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanbox_string"), func_id);
        }

        // Declare js_nanbox_bigint(i64) -> f64 (for BigInt values)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // raw BigInt pointer
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed BigInt (uses BIGINT_TAG)
            let func_id = self.module.declare_function(
                "js_nanbox_bigint",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanbox_bigint"), func_id);
        }

        // Declare js_checkpoint(n: i32) -> void (debug checkpoint for crash localization)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // checkpoint id
            let func_id = self.module.declare_function(
                "js_checkpoint",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_checkpoint"), func_id);
        }

        // Declare js_nanbox_get_string_pointer(f64) -> i64 (extract string pointer from NaN-boxed value)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed string value
            sig.returns.push(AbiParam::new(types::I64)); // raw string pointer
            let func_id = self.module.declare_function(
                "js_nanbox_get_string_pointer",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanbox_get_string_pointer"), func_id);
        }

        // Declare js_get_string_pointer_unified(f64) -> i64 (extract string pointer from either NaN-boxed or raw pointer)
        // This handles both properly NaN-boxed strings and raw pointers stored via bitcast
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // f64 value (NaN-boxed or bitcast pointer)
            sig.returns.push(AbiParam::new(types::I64)); // raw string pointer
            let func_id = self.module.declare_function(
                "js_get_string_pointer_unified",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_get_string_pointer_unified"), func_id);
        }

        // Declare js_nanbox_get_pointer(f64) -> i64 (extract pointer from NaN-boxed value)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed pointer value
            sig.returns.push(AbiParam::new(types::I64)); // raw pointer
            let func_id = self.module.declare_function(
                "js_nanbox_get_pointer",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanbox_get_pointer"), func_id);
        }

        // Declare js_nanbox_get_bigint(f64) -> i64 (extract BigInt pointer from NaN-boxed value)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed BigInt value
            sig.returns.push(AbiParam::new(types::I64)); // raw BigInt pointer
            let func_id = self.module.declare_function(
                "js_nanbox_get_bigint",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanbox_get_bigint"), func_id);
        }

        // Declare js_is_truthy(f64) -> i32 (check if value is truthy in JavaScript terms)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::I32)); // 1 if truthy, 0 if falsy
            let func_id = self.module.declare_function(
                "js_is_truthy",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_is_truthy"), func_id);
        }

        // Declare js_object_alloc(class_id: i32, field_count: i32) -> *mut ObjectHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // class_id
            sig.params.push(AbiParam::new(types::I32)); // field_count
            sig.returns.push(AbiParam::new(types::I64)); // object pointer
            let func_id = self.module.declare_function(
                "js_object_alloc",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_alloc"), func_id);
        }

        // Declare js_object_alloc_with_parent(class_id: i32, parent_class_id: i32, field_count: i32) -> *mut ObjectHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // class_id
            sig.params.push(AbiParam::new(types::I32)); // parent_class_id
            sig.params.push(AbiParam::new(types::I32)); // field_count
            sig.returns.push(AbiParam::new(types::I64)); // object pointer
            let func_id = self.module.declare_function(
                "js_object_alloc_with_parent",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_alloc_with_parent"), func_id);
        }

        // Declare js_object_alloc_fast(class_id: i32, field_count: i32) -> *mut ObjectHeader (i64)
        // Fast bump allocation without field initialization
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // class_id
            sig.params.push(AbiParam::new(types::I32)); // field_count
            sig.returns.push(AbiParam::new(types::I64)); // object pointer
            let func_id = self.module.declare_function(
                "js_object_alloc_fast",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_alloc_fast"), func_id);
        }

        // Declare js_object_alloc_fast_with_parent(class_id: i32, parent_class_id: i32, field_count: i32) -> *mut ObjectHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // class_id
            sig.params.push(AbiParam::new(types::I32)); // parent_class_id
            sig.params.push(AbiParam::new(types::I32)); // field_count
            sig.returns.push(AbiParam::new(types::I64)); // object pointer
            let func_id = self.module.declare_function(
                "js_object_alloc_fast_with_parent",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_alloc_fast_with_parent"), func_id);
        }

        // Declare js_object_alloc_class_with_keys(class_id: i32, parent_class_id: i32, field_count: i32, packed_keys: i64, packed_keys_len: i32) -> *mut ObjectHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // class_id
            sig.params.push(AbiParam::new(types::I32)); // parent_class_id
            sig.params.push(AbiParam::new(types::I32)); // field_count
            sig.params.push(AbiParam::new(types::I64)); // packed_keys ptr
            sig.params.push(AbiParam::new(types::I32)); // packed_keys_len
            sig.returns.push(AbiParam::new(types::I64)); // object pointer
            let func_id = self.module.declare_function(
                "js_object_alloc_class_with_keys",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_alloc_class_with_keys"), func_id);
        }

        // Declare js_object_alloc_with_shape(shape_id: I32, field_count: I32, packed_keys: I64, packed_keys_len: I32) -> I64
        // Shape-cached allocation: first call per shape_id creates keys array, subsequent calls reuse cache
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // shape_id
            sig.params.push(AbiParam::new(types::I32)); // field_count
            sig.params.push(AbiParam::new(types::I64)); // packed_keys ptr
            sig.params.push(AbiParam::new(types::I32)); // packed_keys_len
            sig.returns.push(AbiParam::new(types::I64)); // object pointer
            let func_id = self.module.declare_function(
                "js_object_alloc_with_shape",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_alloc_with_shape"), func_id);
        }

        // Declare js_object_clone_with_extra(src_f64: F64, extra_count: I32, keys_ptr: I64, keys_len: I32) -> I64
        // Clones a spread source object, allocating extra slots for static props.
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // src_f64 (NaN-boxed spread source)
            sig.params.push(AbiParam::new(types::I32)); // extra_count
            sig.params.push(AbiParam::new(types::I64)); // static_keys_ptr
            sig.params.push(AbiParam::new(types::I32)); // static_keys_len
            sig.returns.push(AbiParam::new(types::I64)); // new *mut ObjectHeader (raw pointer)
            let func_id = self.module.declare_function(
                "js_object_clone_with_extra",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_clone_with_extra"), func_id);
        }

        // Declare js_create_native_module_namespace(module_name_ptr: i64, module_name_len: i64) -> f64
        // Creates a native module namespace object for `import * as X from 'module'`
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // module_name_ptr
            sig.params.push(AbiParam::new(types::I64)); // module_name_len
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed object
            let func_id = self.module.declare_function(
                "js_create_native_module_namespace",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_create_native_module_namespace"), func_id);
        }

        // Declare js_native_module_bind_method(namespace_obj: f64, method_name_ptr: i64, method_name_len: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // namespace_obj (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // method_name_ptr
            sig.params.push(AbiParam::new(types::I64)); // method_name_len
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed closure
            let func_id = self.module.declare_function(
                "js_native_module_bind_method",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_native_module_bind_method"), func_id);
        }

        // Declare js_instanceof(value: f64, class_id: i32) -> f64 (boolean as 1.0/0.0)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value (NaN-boxed pointer)
            sig.params.push(AbiParam::new(types::I32)); // class_id
            sig.returns.push(AbiParam::new(types::F64)); // boolean result
            let func_id = self.module.declare_function(
                "js_instanceof",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_instanceof"), func_id);
        }

        // Declare js_object_has_property(obj: f64, key: f64) -> f64 (boolean as 1.0/0.0)
        // Used for the 'in' operator: "key" in obj
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // object (NaN-boxed pointer)
            sig.params.push(AbiParam::new(types::F64)); // key (NaN-boxed string)
            sig.returns.push(AbiParam::new(types::F64)); // boolean result
            let func_id = self.module.declare_function(
                "js_object_has_property",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_has_property"), func_id);
        }

        // Declare js_object_get_field(obj: i64, field_index: i32) -> f64 (JSValue as f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::I32)); // field index
            sig.returns.push(AbiParam::new(types::F64)); // field value
            let func_id = self.module.declare_function(
                "js_object_get_field_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_get_field_f64"), func_id);
        }

        // Declare js_object_set_field(obj: i64, field_index: i32, value: f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::I32)); // field index
            sig.params.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_object_set_field_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_set_field_f64"), func_id);
        }

        // js_object_keys(obj: i64) -> *mut ArrayHeader (array of string keys)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_object_keys",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_keys"), func_id);
        }

        // js_dynamic_object_keys(ptr: i64) -> *mut ArrayHeader (handles Error objects too)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_dynamic_object_keys",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_object_keys"), func_id);
        }

        // js_object_values(obj: i64) -> *mut ArrayHeader (array of values)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_object_values",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_values"), func_id);
        }

        // js_object_entries(obj: i64) -> *mut ArrayHeader (array of [key, value] pairs)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_object_entries",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_entries"), func_id);
        }

        // js_object_rest(src: i64, exclude_keys: i64) -> *mut ObjectHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // source object pointer
            sig.params.push(AbiParam::new(types::I64)); // exclude keys array pointer
            sig.returns.push(AbiParam::new(types::I64)); // new object pointer
            let func_id = self.module.declare_function(
                "js_object_rest",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_rest"), func_id);
        }

        // js_array_is_array(value: f64) -> f64 (1.0 if array, 0.0 otherwise)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::F64)); // boolean result
            let func_id = self.module.declare_function(
                "js_array_is_array",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_is_array"), func_id);
        }

        // js_object_get_field_by_name_f64(obj: i64, key_str: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::I64)); // key string pointer
            sig.returns.push(AbiParam::new(types::F64)); // field value
            let func_id = self.module.declare_function(
                "js_object_get_field_by_name_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_get_field_by_name_f64"), func_id);
        }

        // js_object_set_field_by_name(obj: i64, key_str: i64, value: f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::I64)); // key string pointer
            sig.params.push(AbiParam::new(types::F64)); // value to set
            let func_id = self.module.declare_function(
                "js_object_set_field_by_name",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_set_field_by_name"), func_id);
        }

        // js_object_set_keys(obj: i64, keys_array: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::I64)); // keys array pointer
            let func_id = self.module.declare_function(
                "js_object_set_keys",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_set_keys"), func_id);
        }

        // Array runtime functions
        // js_array_from_f64(elements: *const f64, count: u32) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // elements pointer
            sig.params.push(AbiParam::new(types::I32)); // count
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_array_from_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_from_f64"), func_id);
        }

        // js_array_length(arr: *const ArrayHeader) -> u32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.returns.push(AbiParam::new(types::I32)); // length
            let func_id = self.module.declare_function(
                "js_array_length",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_length"), func_id);
        }

        // js_array_get_f64(arr: *const ArrayHeader, index: u32) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::F64)); // element value
            let func_id = self.module.declare_function(
                "js_array_get_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_get_f64"), func_id);
        }

        // js_array_get_f64_unchecked(arr: *const ArrayHeader, index: u32) -> f64
        // Fast path: skips polymorphic registry checks (buffer/set/map)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::F64)); // element value
            let func_id = self.module.declare_function(
                "js_array_get_f64_unchecked",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_get_f64_unchecked"), func_id);
        }

        // js_array_set_f64(arr: *mut ArrayHeader, index: u32, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_array_set_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_set_f64"), func_id);
        }

        // js_array_set_f64_unchecked(arr: *mut ArrayHeader, index: u32, value: f64)
        // Fast path: skips polymorphic registry checks (buffer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_array_set_f64_unchecked",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_set_f64_unchecked"), func_id);
        }

        // js_array_set_f64_extend(arr: *mut ArrayHeader, index: u32, value: f64) -> *mut ArrayHeader
        // This version extends the array if needed (JavaScript semantics)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_set_f64_extend",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_set_f64_extend"), func_id);
        }

        // js_array_alloc(capacity: u32) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // capacity
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_array_alloc",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_alloc"), func_id);
        }

        // js_array_alloc_with_length(capacity: u32) -> *mut ArrayHeader
        // Like js_array_alloc but sets length = capacity (for `new Array(n)`)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // capacity
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_array_alloc_with_length",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_alloc_with_length"), func_id);
        }

        // js_array_push_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_push_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_push_f64"), func_id);
        }

        // js_array_pop_f64(arr: *mut ArrayHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.returns.push(AbiParam::new(types::F64)); // popped value
            let func_id = self.module.declare_function(
                "js_array_pop_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_pop_f64"), func_id);
        }

        // js_array_shift_f64(arr: *mut ArrayHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.returns.push(AbiParam::new(types::F64)); // shifted value
            let func_id = self.module.declare_function(
                "js_array_shift_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_shift_f64"), func_id);
        }

        // js_array_unshift_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_unshift_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_unshift_f64"), func_id);
        }

        // js_array_unshift_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // value (NaN-boxed as u64)
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_unshift_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_unshift_jsvalue"), func_id);
        }

        // js_array_indexOf_f64(arr: *const ArrayHeader, value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I32)); // index (-1 if not found)
            let func_id = self.module.declare_function(
                "js_array_indexOf_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_indexOf_f64"), func_id);
        }

        // js_array_indexOf_jsvalue(arr: *const ArrayHeader, value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::I32)); // index (-1 if not found)
            let func_id = self.module.declare_function(
                "js_array_indexOf_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_indexOf_jsvalue"), func_id);
        }

        // js_array_includes_f64(arr: *const ArrayHeader, value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I32)); // 1 if found, 0 if not
            let func_id = self.module.declare_function(
                "js_array_includes_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_includes_f64"), func_id);
        }

        // js_array_includes_jsvalue(arr: *const ArrayHeader, value: f64) -> i32
        // Uses deep equality comparison for NaN-boxed values (handles string content comparison)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::F64)); // value (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I32)); // 1 if found, 0 if not
            let func_id = self.module.declare_function(
                "js_array_includes_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_includes_jsvalue"), func_id);
        }

        // js_array_slice(arr: *const ArrayHeader, start: i32, end: i32) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // start index
            sig.params.push(AbiParam::new(types::I32)); // end index
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_slice",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_slice"), func_id);
        }

        // js_array_splice(arr: *mut ArrayHeader, start: i32, delete_count: i32, items: *const f64, items_count: u32, out_arr: *mut *mut ArrayHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // start index
            sig.params.push(AbiParam::new(types::I32)); // delete count
            sig.params.push(AbiParam::new(types::I64)); // items pointer
            sig.params.push(AbiParam::new(types::I32)); // items count
            sig.params.push(AbiParam::new(types::I64)); // out_arr pointer (for updated array)
            sig.returns.push(AbiParam::new(types::I64)); // deleted elements array pointer
            let func_id = self.module.declare_function(
                "js_array_splice",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_splice"), func_id);
        }

        // js_array_concat(dest: *mut ArrayHeader, src: *const ArrayHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // destination array pointer
            sig.params.push(AbiParam::new(types::I64)); // source array pointer
            sig.returns.push(AbiParam::new(types::I64)); // new destination array pointer
            let func_id = self.module.declare_function(
                "js_array_concat",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_concat"), func_id);
        }

        // js_array_flat(arr: *const ArrayHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.returns.push(AbiParam::new(types::I64)); // new flattened array pointer
            let func_id = self.module.declare_function(
                "js_array_flat",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_flat"), func_id);
        }

        // js_array_clone(src: *const ArrayHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // source array pointer
            sig.returns.push(AbiParam::new(types::I64)); // new cloned array pointer
            let func_id = self.module.declare_function(
                "js_array_clone",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_clone"), func_id);
        }

        // === JSValue-based array functions for mixed-type arrays ===

        // js_array_from_jsvalue(elements: *const u64, count: u32) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // elements pointer (u64 array)
            sig.params.push(AbiParam::new(types::I32)); // count
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_array_from_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_from_jsvalue"), func_id);
        }

        // js_array_get_jsvalue(arr: *const ArrayHeader, index: u32) -> u64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::I64)); // JSValue bits (u64)
            let func_id = self.module.declare_function(
                "js_array_get_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_get_jsvalue"), func_id);
        }

        // js_handle_array_get(array_handle: f64, index: i32) -> f64
        // V8 array element access for JS handle arrays (e.g., mysql2 results)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array handle (NaN-boxed JS_HANDLE_TAG)
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::F64)); // result (NaN-boxed)
            let func_id = self.module.declare_function(
                "js_handle_array_get",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_handle_array_get"), func_id);
        }

        // js_array_set_jsvalue(arr: *mut ArrayHeader, index: u32, value: u64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::I64)); // JSValue bits (u64)
            let func_id = self.module.declare_function(
                "js_array_set_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_set_jsvalue"), func_id);
        }

        // js_array_set_jsvalue_extend(arr: *mut ArrayHeader, index: u32, value: u64) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::I64)); // JSValue bits (u64)
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_set_jsvalue_extend",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_set_jsvalue_extend"), func_id);
        }

        // js_array_push_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // JSValue bits (u64)
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_push_jsvalue",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_push_jsvalue"), func_id);
        }

        // js_dynamic_array_get(arr_value: f64, index: i32) -> f64
        // Unified array access that handles both JS handle arrays and native arrays
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array value (may be JS handle or native ptr)
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::F64)); // element value as f64
            let func_id = self.module.declare_function(
                "js_dynamic_array_get",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_array_get"), func_id);
        }

        // js_dynamic_array_length(arr_value: f64) -> i32
        // Unified array length that handles both JS handle arrays and native arrays
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array value (may be JS handle or native ptr)
            sig.returns.push(AbiParam::new(types::I32)); // length
            let func_id = self.module.declare_function(
                "js_dynamic_array_length",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_array_length"), func_id);
        }

        // js_dynamic_object_get_property(obj_value: f64, property_name_ptr: i64, property_name_len: usize) -> f64
        // Unified property access that handles both JS handle objects and native objects
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // object value (may be JS handle or native ptr)
            sig.params.push(AbiParam::new(types::I64)); // property name ptr
            sig.params.push(AbiParam::new(types::I64)); // property name length
            sig.returns.push(AbiParam::new(types::F64)); // property value as f64
            let func_id = self.module.declare_function(
                "js_dynamic_object_get_property",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_object_get_property"), func_id);
        }

        // js_collection_method_dispatch(obj: f64, method_ptr: i64, method_len: i64, arg0: f64, arg1: f64) -> f64
        // Dynamic dispatch for Map/Set methods when type is unknown at compile time
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // object value
            sig.params.push(AbiParam::new(types::I64)); // method name ptr
            sig.params.push(AbiParam::new(types::I64)); // method name length
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function(
                "js_collection_method_dispatch",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_collection_method_dispatch"), func_id);
        }

        // js_array_forEach(arr: *const ArrayHeader, callback: *const ClosureHeader) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            // No return value
            let func_id = self.module.declare_function(
                "js_array_forEach",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_forEach"), func_id);
        }

        // js_array_map(arr: *const ArrayHeader, callback: *const ClosureHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_map",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_map"), func_id);
        }

        // js_array_sort_with_comparator(arr: *mut ArrayHeader, comparator: *const ClosureHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // comparator closure pointer
            sig.returns.push(AbiParam::new(types::I64)); // same array pointer (in-place sort)
            let func_id = self.module.declare_function(
                "js_array_sort_with_comparator",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_sort_with_comparator"), func_id);
        }

        // js_array_filter(arr: *const ArrayHeader, callback: *const ClosureHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.returns.push(AbiParam::new(types::I64)); // new array pointer
            let func_id = self.module.declare_function(
                "js_array_filter",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_filter"), func_id);
        }

        // js_array_find(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 (element or NaN)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.returns.push(AbiParam::new(types::F64)); // element or NaN if not found
            let func_id = self.module.declare_function(
                "js_array_find",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_find"), func_id);
        }

        // js_array_findIndex(arr: *const ArrayHeader, callback: *const ClosureHeader) -> i32 (index or -1)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.returns.push(AbiParam::new(types::I32)); // index or -1
            let func_id = self.module.declare_function(
                "js_array_findIndex",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_findIndex"), func_id);
        }

        // js_dynamic_array_find(arr_value: f64, callback: *const ClosureHeader) -> f64
        // Handles both JS handle arrays and native arrays
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array value (may be NaN-boxed or JS handle)
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.returns.push(AbiParam::new(types::F64)); // element or NaN if not found
            let func_id = self.module.declare_function(
                "js_dynamic_array_find",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_array_find"), func_id);
        }

        // js_dynamic_array_findIndex(arr_value: f64, callback: *const ClosureHeader) -> f64
        // Handles both JS handle arrays and native arrays
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array value (may be NaN-boxed or JS handle)
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.returns.push(AbiParam::new(types::F64)); // index as f64 or -1.0
            let func_id = self.module.declare_function(
                "js_dynamic_array_findIndex",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_array_findIndex"), func_id);
        }

        // js_array_reduce(arr, callback, has_initial, initial) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // callback closure pointer
            sig.params.push(AbiParam::new(types::I32)); // has_initial flag
            sig.params.push(AbiParam::new(types::F64)); // initial value
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function(
                "js_array_reduce",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_reduce"), func_id);
        }

        // js_array_join(arr, separator) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.params.push(AbiParam::new(types::I64)); // separator string pointer (nullable)
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_array_join",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_array_join"), func_id);
        }

        // js_array_length (for getting length after push to return)
        // Already declared above

        // Map runtime functions
        // js_map_alloc(capacity: u32) -> *mut MapHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // capacity
            sig.returns.push(AbiParam::new(types::I64)); // map pointer
            let func_id = self.module.declare_function(
                "js_map_alloc",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_alloc"), func_id);
        }

        // js_map_size(map: *const MapHeader) -> u32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.returns.push(AbiParam::new(types::I32)); // size
            let func_id = self.module.declare_function(
                "js_map_size",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_size"), func_id);
        }

        // js_map_set(map: *mut MapHeader, key: f64, value: f64) -> *mut MapHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.params.push(AbiParam::new(types::F64)); // key (as JSValue bits)
            sig.params.push(AbiParam::new(types::F64)); // value (as JSValue bits)
            sig.returns.push(AbiParam::new(types::I64)); // new map pointer
            let func_id = self.module.declare_function(
                "js_map_set",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_set"), func_id);
        }

        // js_map_get(map: *const MapHeader, key: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // value (as JSValue bits)
            let func_id = self.module.declare_function(
                "js_map_get",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_get"), func_id);
        }

        // js_map_has(map: *const MapHeader, key: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::I32)); // 1 if found, 0 if not
            let func_id = self.module.declare_function(
                "js_map_has",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_has"), func_id);
        }

        // js_map_delete(map: *mut MapHeader, key: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::I32)); // 1 if deleted, 0 if not found
            let func_id = self.module.declare_function(
                "js_map_delete",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_delete"), func_id);
        }

        // js_map_clear(map: *mut MapHeader) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            // No return value
            let func_id = self.module.declare_function(
                "js_map_clear",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_clear"), func_id);
        }

        // js_map_entries(map: *const MapHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_map_entries",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_entries"), func_id);
        }

        // js_map_keys(map: *const MapHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_map_keys",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_keys"), func_id);
        }

        // js_map_values(map: *const MapHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_map_values",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_values"), func_id);
        }

        // js_map_foreach(map: *const MapHeader, callback: f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // map pointer
            sig.params.push(AbiParam::new(types::F64)); // callback (closure as f64)
            let func_id = self.module.declare_function(
                "js_map_foreach",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_map_foreach"), func_id);
        }

        // Set runtime functions
        // js_set_alloc(capacity: u32) -> *mut SetHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // capacity
            sig.returns.push(AbiParam::new(types::I64)); // set pointer
            let func_id = self.module.declare_function(
                "js_set_alloc",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_alloc"), func_id);
        }

        // js_set_from_array(arr: *const ArrayHeader) -> *mut SetHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array pointer
            sig.returns.push(AbiParam::new(types::I64)); // set pointer
            let func_id = self.module.declare_function(
                "js_set_from_array",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_from_array"), func_id);
        }

        // js_set_size(set: *const SetHeader) -> u32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // set pointer
            sig.returns.push(AbiParam::new(types::I32)); // size
            let func_id = self.module.declare_function(
                "js_set_size",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_size"), func_id);
        }

        // js_set_add(set: *mut SetHeader, value: f64) -> *mut SetHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // set pointer
            sig.params.push(AbiParam::new(types::F64)); // value (as JSValue bits)
            sig.returns.push(AbiParam::new(types::I64)); // new set pointer
            let func_id = self.module.declare_function(
                "js_set_add",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_add"), func_id);
        }

        // js_set_has(set: *const SetHeader, value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // set pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I32)); // 1 if found, 0 if not
            let func_id = self.module.declare_function(
                "js_set_has",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_has"), func_id);
        }

        // js_set_delete(set: *mut SetHeader, value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // set pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I32)); // 1 if deleted, 0 if not found
            let func_id = self.module.declare_function(
                "js_set_delete",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_delete"), func_id);
        }

        // js_set_clear(set: *mut SetHeader) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // set pointer
            // No return value
            let func_id = self.module.declare_function(
                "js_set_clear",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_clear"), func_id);
        }

        // js_set_to_array(set: *const SetHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // set pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer
            let func_id = self.module.declare_function(
                "js_set_to_array",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_to_array"), func_id);
        }

        // String runtime functions
        // js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data pointer
            sig.params.push(AbiParam::new(types::I32)); // length
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_string_from_bytes",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_from_bytes"), func_id);
        }

        // js_string_length(s: *const StringHeader) -> u32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I32)); // length
            let func_id = self.module.declare_function(
                "js_string_length",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_length"), func_id);
        }

        // js_string_concat(a: *const StringHeader, b: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_concat",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_concat"), func_id);
        }

        // js_string_append(dest: *mut StringHeader, src: *const StringHeader) -> *mut StringHeader
        // In-place append with reallocation if needed - for `str = str + x` patterns
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // dest pointer (mutable)
            sig.params.push(AbiParam::new(types::I64)); // src pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer (may be reallocated)
            let func_id = self.module.declare_function(
                "js_string_append",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_append"), func_id);
        }

        // js_number_to_string(value: f64) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // number value
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_number_to_string",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_number_to_string"), func_id);
        }

        // js_number_to_fixed(value: f64, decimals: f64) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // number value
            sig.params.push(AbiParam::new(types::F64)); // decimal places
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_number_to_fixed",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_number_to_fixed"), func_id);
        }

        // js_jsvalue_to_string(value: f64) -> *mut StringHeader
        // Converts any NaN-boxed value to string (handles strings, numbers, etc.)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_jsvalue_to_string",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_jsvalue_to_string"), func_id);
        }

        // js_jsvalue_to_string_radix(value: f64, radix: i32) -> *mut StringHeader
        // Converts any NaN-boxed value to string with radix (handles BigInt, numbers, etc.)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.params.push(AbiParam::new(types::I32)); // radix
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_jsvalue_to_string_radix",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_jsvalue_to_string_radix"), func_id);
        }

        // js_ensure_string_ptr(value: f64) -> i64
        // Ensures a value is a native string pointer - handles raw pointers, NaN-boxed strings, and JS handles
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value (may be raw pointer, NaN-boxed, or JS handle)
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_ensure_string_ptr",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_ensure_string_ptr"), func_id);
        }

        // js_string_slice(s: *const StringHeader, start: i32, end: i32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // start
            sig.params.push(AbiParam::new(types::I32)); // end
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_slice",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_slice"), func_id);
        }

        // js_string_substring(s: *const StringHeader, start: i32, end: i32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // start
            sig.params.push(AbiParam::new(types::I32)); // end
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_substring",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_substring"), func_id);
        }

        // js_string_char_at(s: *const StringHeader, index: i32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::I64)); // result pointer (single-char string)
            let func_id = self.module.declare_function(
                "js_string_char_at",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_char_at"), func_id);
        }

        // js_string_char_code_at(s: *const StringHeader, index: i32) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::F64)); // UTF-16 code unit or NaN
            let func_id = self.module.declare_function(
                "js_string_char_code_at",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_char_code_at"), func_id);
        }

        // js_string_pad_start(s: *const StringHeader, target_length: u32, pad_string: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // target length
            sig.params.push(AbiParam::new(types::I64)); // pad string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_pad_start",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_pad_start"), func_id);
        }

        // js_string_pad_end(s: *const StringHeader, target_length: u32, pad_string: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // target length
            sig.params.push(AbiParam::new(types::I64)); // pad string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_pad_end",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_pad_end"), func_id);
        }

        // js_string_alloc_space() -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_alloc_space",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_alloc_space"), func_id);
        }

        // js_string_trim(s: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_trim",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_trim"), func_id);
        }

        // js_string_trim_start(s: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_string_trim_start", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_trim_start"), func_id);
        }

        // js_string_trim_end(s: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_string_trim_end", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_trim_end"), func_id);
        }

        // js_string_to_lower_case(s: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_to_lower_case",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_to_lower_case"), func_id);
        }

        // js_string_to_upper_case(s: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_string_to_upper_case",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_to_upper_case"), func_id);
        }

        // js_string_index_of(haystack: *const StringHeader, needle: *const StringHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // haystack pointer
            sig.params.push(AbiParam::new(types::I64)); // needle pointer
            sig.returns.push(AbiParam::new(types::I32)); // index or -1
            let func_id = self.module.declare_function(
                "js_string_index_of",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_index_of"), func_id);
        }

        // js_string_index_of_from(haystack: *const StringHeader, needle: *const StringHeader, from_index: i32) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // haystack pointer
            sig.params.push(AbiParam::new(types::I64)); // needle pointer
            sig.params.push(AbiParam::new(types::I32)); // from_index
            sig.returns.push(AbiParam::new(types::I32)); // index or -1
            let func_id = self.module.declare_function(
                "js_string_index_of_from",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_index_of_from"), func_id);
        }

        // js_string_split(s: *const StringHeader, delimiter: *const StringHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I64)); // delimiter pointer
            sig.returns.push(AbiParam::new(types::I64)); // result array pointer
            let func_id = self.module.declare_function(
                "js_string_split",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_split"), func_id);
        }

        // js_string_from_char_code(code: i32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // character code
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_string_from_char_code",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_from_char_code"), func_id);
        }

        // js_string_starts_with(s: *const StringHeader, prefix: *const StringHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I64)); // prefix pointer
            sig.returns.push(AbiParam::new(types::I32)); // 0 or 1
            let func_id = self.module.declare_function(
                "js_string_starts_with",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_starts_with"), func_id);
        }

        // js_string_ends_with(s: *const StringHeader, suffix: *const StringHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I64)); // suffix pointer
            sig.returns.push(AbiParam::new(types::I32)); // 0 or 1
            let func_id = self.module.declare_function(
                "js_string_ends_with",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_ends_with"), func_id);
        }

        // js_string_repeat(s: *const StringHeader, count: i32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I32)); // count
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_string_repeat",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_repeat"), func_id);
        }

        // js_string_print(s: *const StringHeader)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_string_print",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_print"), func_id);
        }

        // js_getenv(name_ptr: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer (or null)
            let func_id = self.module.declare_function(
                "js_getenv",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_getenv"), func_id);
        }

        // js_process_exit(code: f64) -> void (never returns)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // exit code
            let func_id = self.module.declare_function(
                "js_process_exit",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_process_exit"), func_id);
        }

        // File system runtime functions - all accept NaN-boxed f64 string values
        // js_fs_read_file_sync(path_value: f64) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I64)); // content string pointer (or null)
            let func_id = self.module.declare_function(
                "js_fs_read_file_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_read_file_sync"), func_id);
        }

        // js_fs_write_file_sync(path_value: f64, content_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed content string
            sig.returns.push(AbiParam::new(types::I32)); // 1 on success, 0 on failure
            let func_id = self.module.declare_function(
                "js_fs_write_file_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_write_file_sync"), func_id);
        }

        // js_fs_append_file_sync(path_value: f64, content_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed content string
            sig.returns.push(AbiParam::new(types::I32)); // 1 on success, 0 on failure
            let func_id = self.module.declare_function(
                "js_fs_append_file_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_append_file_sync"), func_id);
        }

        // js_fs_exists_sync(path_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I32)); // 1 if exists, 0 if not
            let func_id = self.module.declare_function(
                "js_fs_exists_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_exists_sync"), func_id);
        }

        // js_fs_mkdir_sync(path_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I32)); // 1 on success, 0 on failure
            let func_id = self.module.declare_function(
                "js_fs_mkdir_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_mkdir_sync"), func_id);
        }

        // js_fs_unlink_sync(path_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I32)); // 1 on success, 0 on failure
            let func_id = self.module.declare_function(
                "js_fs_unlink_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_unlink_sync"), func_id);
        }

        // js_fs_readdir_sync(path_value: f64) -> f64 (NaN-boxed array pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed array pointer
            let func_id = self.module.declare_function(
                "js_fs_readdir_sync",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_readdir_sync"), func_id);
        }

        // js_fs_is_directory(path_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I32)); // 1 if dir, 0 if not
            let func_id = self.module.declare_function(
                "js_fs_is_directory",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_is_directory"), func_id);
        }

        // js_fs_read_file_binary(path_value: f64) -> i64 (BufferHeader ptr or null)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I64)); // buffer pointer (or null)
            let func_id = self.module.declare_function(
                "js_fs_read_file_binary",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_read_file_binary"), func_id);
        }

        // js_fs_rm_recursive(path_value: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed path string
            sig.returns.push(AbiParam::new(types::I32)); // 1 on success, 0 on failure
            let func_id = self.module.declare_function(
                "js_fs_rm_recursive",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_fs_rm_recursive"), func_id);
        }

        // Path runtime functions
        // js_path_join(a: *const StringHeader, b: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a string pointer
            sig.params.push(AbiParam::new(types::I64)); // b string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_path_join",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_path_join"), func_id);
        }

        // js_path_dirname(path: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_path_dirname",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_path_dirname"), func_id);
        }

        // js_path_basename(path: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_path_basename",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_path_basename"), func_id);
        }

        // js_path_extname(path: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_path_extname",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_path_extname"), func_id);
        }

        // js_path_resolve(path: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function(
                "js_path_resolve",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_path_resolve"), func_id);
        }

        // js_path_is_absolute(path: *const StringHeader) -> i32 (boolean)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string pointer
            sig.returns.push(AbiParam::new(types::I32)); // boolean result
            let func_id = self.module.declare_function(
                "js_path_is_absolute",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_path_is_absolute"), func_id);
        }

        // BigInt runtime functions
        // js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data pointer
            sig.params.push(AbiParam::new(types::I32)); // length
            sig.returns.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function(
                "js_bigint_from_string",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_from_string"), func_id);
        }

        // js_bigint_from_string_radix(data: *const u8, len: u32, radix: i32) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data pointer
            sig.params.push(AbiParam::new(types::I32)); // length
            sig.params.push(AbiParam::new(types::I32)); // radix
            sig.returns.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function(
                "js_bigint_from_string_radix",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_from_string_radix"), func_id);
        }

        // js_bigint_to_buffer(a: *const BigIntHeader, length: i32) -> *mut BufferHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.params.push(AbiParam::new(types::I32)); // length
            sig.returns.push(AbiParam::new(types::I64)); // buffer pointer
            let func_id = self.module.declare_function(
                "js_bigint_to_buffer",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_to_buffer"), func_id);
        }

        // js_bigint_is_negative(a: *const BigIntHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.returns.push(AbiParam::new(types::I32)); // 0 or 1
            let func_id = self.module.declare_function(
                "js_bigint_is_negative",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_is_negative"), func_id);
        }

        // js_bigint_from_i64(value: i64) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function(
                "js_bigint_from_i64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_from_i64"), func_id);
        }

        // js_bigint_from_f64(value: f64) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function(
                "js_bigint_from_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_from_f64"), func_id);
        }

        // js_bigint_neg(a: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_neg",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_neg"), func_id);
        }

        // js_bigint_is_zero(a: *const BigIntHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.returns.push(AbiParam::new(types::I32)); // 1=zero, 0=non-zero
            let func_id = self.module.declare_function(
                "js_bigint_is_zero",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_is_zero"), func_id);
        }

        // js_bigint_add(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_add",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_add"), func_id);
        }

        // js_bigint_sub(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_sub",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_sub"), func_id);
        }

        // js_bigint_mul(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_mul",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_mul"), func_id);
        }

        // js_bigint_div(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_div",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_div"), func_id);
        }

        // js_bigint_mod(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_mod",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_mod"), func_id);
        }

        // js_bigint_pow(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_pow",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_pow"), func_id);
        }

        // js_bigint_shl(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_shl",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_shl"), func_id);
        }

        // js_bigint_shr(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_shr",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_shr"), func_id);
        }

        // js_bigint_and(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_and",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_and"), func_id);
        }

        // js_bigint_or(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_or",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_or"), func_id);
        }

        // js_bigint_xor(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I64)); // result pointer
            let func_id = self.module.declare_function(
                "js_bigint_xor",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_xor"), func_id);
        }

        // js_bigint_cmp(a: *const BigIntHeader, b: *const BigIntHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a pointer
            sig.params.push(AbiParam::new(types::I64)); // b pointer
            sig.returns.push(AbiParam::new(types::I32)); // -1, 0, or 1
            let func_id = self.module.declare_function(
                "js_bigint_cmp",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_cmp"), func_id);
        }

        // js_bigint_print(a: *const BigIntHeader)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function(
                "js_bigint_print",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_print"), func_id);
        }

        // js_bigint_to_f64(a: *const BigIntHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.returns.push(AbiParam::new(types::F64)); // f64 result
            let func_id = self.module.declare_function(
                "js_bigint_to_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_to_f64"), func_id);
        }

        // js_bigint_to_string(a: *const BigIntHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_bigint_to_string",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_to_string"), func_id);
        }

        // js_bigint_to_string_radix(a: *const BigIntHeader, radix: i32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.params.push(AbiParam::new(types::I32)); // radix
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_bigint_to_string_radix",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_bigint_to_string_radix"), func_id);
        }

        // Closure runtime functions
        // js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut ClosureHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // func_ptr
            sig.params.push(AbiParam::new(types::I32)); // capture_count
            sig.returns.push(AbiParam::new(types::I64)); // closure pointer
            let func_id = self.module.declare_function(
                "js_closure_alloc",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_alloc"), func_id);
        }

        // js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_closure_set_capture_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_set_capture_f64"), func_id);
        }

        // js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_closure_get_capture_f64",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_get_capture_f64"), func_id);
        }

        // js_closure_unbind_this(val: f64) -> f64
        // Clones a closure with CAPTURES_THIS_FLAG and clears slot 0 (this).
        // No-op for non-closure values.
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::F64)); // result (possibly new closure)
            let func_id = self.module.declare_function(
                "js_closure_unbind_this",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_unbind_this"), func_id);
        }

        // js_closure_call0(closure: *const ClosureHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.returns.push(AbiParam::new(types::F64)); // return value
            let func_id = self.module.declare_function(
                "js_closure_call0",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call0"), func_id);
        }

        // js_closure_call1(closure: *const ClosureHeader, arg0: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.returns.push(AbiParam::new(types::F64)); // return value
            let func_id = self.module.declare_function(
                "js_closure_call1",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call1"), func_id);
        }

        // js_closure_call2(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.returns.push(AbiParam::new(types::F64)); // return value
            let func_id = self.module.declare_function(
                "js_closure_call2",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call2"), func_id);
        }

        // js_closure_call3(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.params.push(AbiParam::new(types::F64)); // arg2
            sig.returns.push(AbiParam::new(types::F64)); // return value
            let func_id = self.module.declare_function(
                "js_closure_call3",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call3"), func_id);
        }

        // js_closure_call4(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.params.push(AbiParam::new(types::F64)); // arg2
            sig.params.push(AbiParam::new(types::F64)); // arg3
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_closure_call4",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call4"), func_id);
        }

        // js_closure_call5(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.params.push(AbiParam::new(types::F64)); // arg2
            sig.params.push(AbiParam::new(types::F64)); // arg3
            sig.params.push(AbiParam::new(types::F64)); // arg4
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_closure_call5",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call5"), func_id);
        }

        // js_closure_call6(closure: *const ClosureHeader, arg0-5: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.params.push(AbiParam::new(types::F64)); // arg2
            sig.params.push(AbiParam::new(types::F64)); // arg3
            sig.params.push(AbiParam::new(types::F64)); // arg4
            sig.params.push(AbiParam::new(types::F64)); // arg5
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_closure_call6",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call6"), func_id);
        }

        // js_closure_call7(closure: *const ClosureHeader, arg0-6: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.params.push(AbiParam::new(types::F64)); // arg2
            sig.params.push(AbiParam::new(types::F64)); // arg3
            sig.params.push(AbiParam::new(types::F64)); // arg4
            sig.params.push(AbiParam::new(types::F64)); // arg5
            sig.params.push(AbiParam::new(types::F64)); // arg6
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_closure_call7",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call7"), func_id);
        }

        // js_closure_call8(closure: *const ClosureHeader, arg0-7: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure
            sig.params.push(AbiParam::new(types::F64)); // arg0
            sig.params.push(AbiParam::new(types::F64)); // arg1
            sig.params.push(AbiParam::new(types::F64)); // arg2
            sig.params.push(AbiParam::new(types::F64)); // arg3
            sig.params.push(AbiParam::new(types::F64)); // arg4
            sig.params.push(AbiParam::new(types::F64)); // arg5
            sig.params.push(AbiParam::new(types::F64)); // arg6
            sig.params.push(AbiParam::new(types::F64)); // arg7
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_closure_call8",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_closure_call8"), func_id);
        }

        // js_closure_call9 through js_closure_call16
        for n in 9..=16u32 {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure
            for _ in 0..n {
                sig.params.push(AbiParam::new(types::F64)); // argN
            }
            sig.returns.push(AbiParam::new(types::F64));
            let name = format!("js_closure_call{}", n);
            let func_id = self.module.declare_function(
                &name,
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Owned(name), func_id);
        }

        // Box runtime functions for mutable captured variables
        // js_box_alloc(initial_value: f64) -> *mut Box
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // initial value
            sig.returns.push(AbiParam::new(types::I64)); // box pointer
            let func_id = self.module.declare_function(
                "js_box_alloc",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_box_alloc"), func_id);
        }

        // js_box_get(ptr: *mut Box) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // box pointer
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_box_get",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_box_get"), func_id);
        }

        // js_box_set(ptr: *mut Box, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // box pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_box_set",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_box_set"), func_id);
        }

        // Exception handling runtime functions
        // js_try_push() -> *mut i32 (pointer to jmp_buf)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // pointer
            let func_id = self.module.declare_function(
                "js_try_push",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_try_push"), func_id);
        }

        // setjmp(env: *mut i32) -> i32 (0 if normal entry, non-zero if from longjmp)
        // This is a libc function that must be called directly from generated code
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pointer
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "setjmp",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("setjmp"), func_id);
        }

        // js_try_end()
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function(
                "js_try_end",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_try_end"), func_id);
        }

        // js_throw(value: f64) -> !
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_throw",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_throw"), func_id);
        }

        // js_get_exception() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_get_exception",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_get_exception"), func_id);
        }

        // js_clear_exception()
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function(
                "js_clear_exception",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_clear_exception"), func_id);
        }

        // js_enter_finally()
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function(
                "js_enter_finally",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_enter_finally"), func_id);
        }

        // js_leave_finally()
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function(
                "js_leave_finally",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_leave_finally"), func_id);
        }



        // js_has_exception() -> i32
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "js_has_exception",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_has_exception"), func_id);
        }

        // Promise runtime functions
        // js_promise_new() -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(
                "js_promise_new",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_new"), func_id);
        }

        // js_promise_resolve(promise: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.params.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_promise_resolve",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_resolve"), func_id);
        }

        // js_promise_resolve_with_promise(outer: i64, inner: i64)
        // Used when returning a Promise from an async function - chains the promises
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // outer promise pointer
            sig.params.push(AbiParam::new(types::I64)); // inner promise pointer
            let func_id = self.module.declare_function(
                "js_promise_resolve_with_promise",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_resolve_with_promise"), func_id);
        }

        // js_promise_state(promise: i64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.returns.push(AbiParam::new(types::I32)); // state
            let func_id = self.module.declare_function(
                "js_promise_state",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_state"), func_id);
        }

        // js_promise_value(promise: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(
                "js_promise_value",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_value"), func_id);
        }

        // js_promise_reason(promise: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.returns.push(AbiParam::new(types::F64)); // reason
            let func_id = self.module.declare_function(
                "js_promise_reason",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_reason"), func_id);
        }

        // js_promise_result(promise: i64) -> f64
        // Returns value if fulfilled, reason if rejected
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.returns.push(AbiParam::new(types::F64)); // result (value or reason)
            let func_id = self.module.declare_function(
                "js_promise_result",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_result"), func_id);
        }

        // js_promise_resolved(value: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // promise pointer
            let func_id = self.module.declare_function(
                "js_promise_resolved",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_resolved"), func_id);
        }

        // js_promise_rejected(reason: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // reason
            sig.returns.push(AbiParam::new(types::I64)); // promise pointer
            let func_id = self.module.declare_function(
                "js_promise_rejected",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_rejected"), func_id);
        }

        // js_promise_all(promises_arr: i64) -> i64
        // Takes an array of promises, returns a promise that resolves with array of results
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array of promises pointer
            sig.returns.push(AbiParam::new(types::I64)); // result promise pointer
            let func_id = self.module.declare_function(
                "js_promise_all",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_all"), func_id);
        }

        // js_promise_run_microtasks() -> i32
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "js_promise_run_microtasks",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_run_microtasks"), func_id);
        }

        // js_promise_schedule_resolve(promise: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_promise_schedule_resolve",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_schedule_resolve"), func_id);
        }

        // js_promise_new_with_executor(executor: i64) -> i64
        // Create a Promise with an executor callback (resolve, reject) => void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // executor closure pointer
            sig.returns.push(AbiParam::new(types::I64)); // promise pointer
            let func_id = self.module.declare_function(
                "js_promise_new_with_executor",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_new_with_executor"), func_id);
        }

        // js_promise_reject(promise: i64, reason: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.params.push(AbiParam::new(types::F64)); // reason
            let func_id = self.module.declare_function(
                "js_promise_reject",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_reject"), func_id);
        }

        // js_promise_then(promise: i64, on_fulfilled: i64, on_rejected: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.params.push(AbiParam::new(types::I64)); // on_fulfilled callback (nullable)
            sig.params.push(AbiParam::new(types::I64)); // on_rejected callback (nullable)
            sig.returns.push(AbiParam::new(types::I64)); // new promise
            let func_id = self.module.declare_function(
                "js_promise_then",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_then"), func_id);
        }

        // js_promise_catch(promise: i64, on_rejected: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.params.push(AbiParam::new(types::I64)); // on_rejected callback (nullable)
            sig.returns.push(AbiParam::new(types::I64)); // new promise
            let func_id = self.module.declare_function(
                "js_promise_catch",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_catch"), func_id);
        }

        // js_promise_finally(promise: i64, on_finally: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // promise pointer
            sig.params.push(AbiParam::new(types::I64)); // on_finally callback (nullable)
            sig.returns.push(AbiParam::new(types::I64)); // new promise
            let func_id = self.module.declare_function(
                "js_promise_finally",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_promise_finally"), func_id);
        }

        // Timer functions
        // js_set_timeout(delay_ms: f64) -> *mut Promise (i64)
        // Native setTimeout that returns a Promise directly
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(
                "js_set_timeout",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_timeout"), func_id);
        }

        // js_set_timeout_callback(callback: i64, delay_ms: f64) -> i64
        // JS-style setTimeout that takes a callback function
        // Also exposed as "setTimeout" for TypeScript compatibility
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // callback (closure pointer)
            sig.params.push(AbiParam::new(types::F64)); // delay_ms
            sig.returns.push(AbiParam::new(types::I64)); // timer ID (or 0)
            let func_id = self.module.declare_function(
                "js_set_timeout_callback",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_timeout_callback"), func_id);
            // Also register as "setTimeout" for TypeScript code (2-arg version)
            self.extern_funcs.insert(Cow::Borrowed("setTimeout"), func_id);
        }

        // js_sleep_ms(ms: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_sleep_ms",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_sleep_ms"), func_id);
        }

        // setInterval(callback: i64, interval_ms: f64) -> i64
        // JS-style setInterval that takes a callback function and interval
        // Returns an interval ID for use with clearInterval
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // callback (closure pointer)
            sig.params.push(AbiParam::new(types::F64)); // interval_ms
            sig.returns.push(AbiParam::new(types::I64)); // interval ID
            let func_id = self.module.declare_function(
                "setInterval",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("setInterval"), func_id);
        }

        // clearInterval(interval_id: i64)
        // Stops an interval timer by its ID
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // interval_id
            let func_id = self.module.declare_function(
                "clearInterval",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("clearInterval"), func_id);
        }

        // clearTimeout(timer_id: i64)
        // Stops a callback timer by its ID
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // timer_id
            let func_id = self.module.declare_function(
                "clearTimeout",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("clearTimeout"), func_id);
        }

        // js_interval_timer_tick() -> i32
        // Process expired interval timers
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "js_interval_timer_tick",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_interval_timer_tick"), func_id);
        }

        // js_interval_timer_has_pending() -> i32
        // Check if there are pending interval timers
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "js_interval_timer_has_pending",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_interval_timer_has_pending"), func_id);
        }

        // ========================================================================
        // worker_threads stdlib functions
        // ========================================================================

        // js_worker_threads_get_worker_data() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_worker_threads_get_worker_data",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_worker_threads_get_worker_data"), func_id);
        }

        // js_worker_threads_parent_port() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_worker_threads_parent_port",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_worker_threads_parent_port"), func_id);
        }

        // js_worker_threads_post_message(data: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // data
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_worker_threads_post_message",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_worker_threads_post_message"), func_id);
        }

        // js_worker_threads_on(event_ptr: i64, callback: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // event string ptr
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(
                "js_worker_threads_on",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_worker_threads_on"), func_id);
        }

        // js_worker_threads_has_pending() -> i32
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "js_worker_threads_has_pending",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_worker_threads_has_pending"), func_id);
        }

        // ========================================================================
        // Perry threading primitives (perry/thread)
        // ========================================================================

        // js_thread_parallel_map(array_val: f64, closure_val: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array (NaN-boxed)
            sig.params.push(AbiParam::new(types::F64)); // closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::F64)); // result array (NaN-boxed)
            let func_id = self.module.declare_function(
                "js_thread_parallel_map",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_thread_parallel_map"), func_id);
        }

        // js_thread_parallel_filter(array_val: f64, closure_val: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // array (NaN-boxed)
            sig.params.push(AbiParam::new(types::F64)); // predicate closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::F64)); // filtered array (NaN-boxed)
            let func_id = self.module.declare_function(
                "js_thread_parallel_filter",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_thread_parallel_filter"), func_id);
        }

        // js_thread_spawn(closure_val: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::F64)); // promise (NaN-boxed)
            let func_id = self.module.declare_function(
                "js_thread_spawn",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_thread_spawn"), func_id);
        }

        // ========================================================================
        // MySQL2 stdlib functions
        // ========================================================================

        // js_mysql2_create_connection(config: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config object pointer
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_create_connection",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_create_connection"), func_id);
        }

        // js_mysql2_connection_end(conn: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_connection_end",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_connection_end"), func_id);
        }

        // js_mysql2_connection_query(conn: i64, sql: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.params.push(AbiParam::new(types::I64)); // sql string pointer
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_connection_query",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_connection_query"), func_id);
        }

        // js_mysql2_connection_execute(conn: i64, sql: i64, params: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.params.push(AbiParam::new(types::I64)); // sql string pointer
            sig.params.push(AbiParam::new(types::I64)); // params array (as JSValue bits)
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_connection_execute",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_connection_execute"), func_id);
        }

        // js_mysql2_connection_begin_transaction(conn: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_connection_begin_transaction",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_connection_begin_transaction"), func_id);
        }

        // js_mysql2_connection_commit(conn: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_connection_commit",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_connection_commit"), func_id);
        }

        // js_mysql2_connection_rollback(conn: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_connection_rollback",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_connection_rollback"), func_id);
        }

        // backOff(fn_ptr: i64, options_ptr: i64) -> i64 (Promise pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // closure pointer
            sig.params.push(AbiParam::new(types::I64)); // options object pointer
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "backOff",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("backOff"), func_id);
        }

        // js_mysql2_create_pool(config: i64) -> i64 (Handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config object pointer
            sig.returns.push(AbiParam::new(types::I64)); // pool handle
            let func_id = self.module.declare_function(
                "js_mysql2_create_pool",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_create_pool"), func_id);
        }

        // js_mysql2_pool_query(pool: i64, sql: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pool handle
            sig.params.push(AbiParam::new(types::I64)); // sql string pointer
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_pool_query",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_query"), func_id);
        }

        // js_mysql2_pool_execute(pool: i64, sql: i64, params: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pool handle
            sig.params.push(AbiParam::new(types::I64)); // sql string pointer
            sig.params.push(AbiParam::new(types::I64)); // params array (as JSValue bits)
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_pool_execute",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_execute"), func_id);
        }

        // js_mysql2_pool_end(pool: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pool handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_pool_end",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_end"), func_id);
        }

        // js_mysql2_pool_get_connection(pool: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pool handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_pool_get_connection",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_get_connection"), func_id);
        }

        // js_mysql2_pool_connection_release(conn: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            let func_id = self.module.declare_function(
                "js_mysql2_pool_connection_release",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_connection_release"), func_id);
        }

        // js_mysql2_pool_connection_query(conn: i64, sql: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.params.push(AbiParam::new(types::I64)); // sql string pointer
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_pool_connection_query",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_connection_query"), func_id);
        }

        // js_mysql2_pool_connection_execute(conn: i64, sql: i64, params: i64) -> *mut Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // connection handle
            sig.params.push(AbiParam::new(types::I64)); // sql string pointer
            sig.params.push(AbiParam::new(types::I64)); // params array (as JSValue bits)
            sig.returns.push(AbiParam::new(types::I64)); // Promise pointer
            let func_id = self.module.declare_function(
                "js_mysql2_pool_connection_execute",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_mysql2_pool_connection_execute"), func_id);
        }

        // js_stdlib_process_pending() -> i32 (number of resolutions processed)
        if self.needs_stdlib {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function(
                "js_stdlib_process_pending",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_stdlib_process_pending"), func_id);
        }

        // js_stdlib_init_dispatch() - registers handle method dispatch for native modules
        if self.needs_stdlib {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function(
                "js_stdlib_init_dispatch",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_stdlib_init_dispatch"), func_id);
        }

        // ========================================================================
        // UUID Functions
        // ========================================================================

        // js_uuid_v4() -> *mut StringHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_uuid_v4",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_uuid_v4"), func_id);
        }

        // js_uuid_v1() -> *mut StringHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_uuid_v1",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_uuid_v1"), func_id);
        }

        // js_uuid_v7() -> *mut StringHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_uuid_v7",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_uuid_v7"), func_id);
        }

        // js_uuid_validate(str: i64) -> f64 (boolean as 0.0/1.0)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::F64)); // boolean as f64
            let func_id = self.module.declare_function(
                "js_uuid_validate",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_uuid_validate"), func_id);
        }

        // js_uuid_version(str: i64) -> f64 (version number)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::F64)); // version number
            let func_id = self.module.declare_function(
                "js_uuid_version",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_uuid_version"), func_id);
        }

        // js_uuid_nil() -> *mut StringHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function(
                "js_uuid_nil",
                Linkage::Import,
                &sig,
            )?;
            self.extern_funcs.insert(Cow::Borrowed("js_uuid_nil"), func_id);
        }

        // ========================================================================
        // Bcrypt Functions
        // ========================================================================

        // js_bcrypt_hash(password: i64, salt_rounds: f64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password string ptr
            sig.params.push(AbiParam::new(types::F64)); // salt rounds
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_bcrypt_hash", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_bcrypt_hash"), func_id);
        }

        // js_bcrypt_compare(password: i64, hash: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password string ptr
            sig.params.push(AbiParam::new(types::I64)); // hash string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_bcrypt_compare", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_bcrypt_compare"), func_id);
        }

        // js_bcrypt_gen_salt(rounds: f64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // rounds
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_bcrypt_gen_salt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_bcrypt_gen_salt"), func_id);
        }

        // js_bcrypt_hash_sync(password: i64, salt_rounds: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password string ptr
            sig.params.push(AbiParam::new(types::F64)); // salt rounds
            sig.returns.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("js_bcrypt_hash_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_bcrypt_hash_sync"), func_id);
        }

        // js_bcrypt_compare_sync(password: i64, hash: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password string ptr
            sig.params.push(AbiParam::new(types::I64)); // hash string ptr
            sig.returns.push(AbiParam::new(types::F64)); // boolean as f64
            let func_id = self.module.declare_function("js_bcrypt_compare_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_bcrypt_compare_sync"), func_id);
        }

        // ========================================================================
        // Redis (ioredis) Functions
        // ========================================================================

        // js_ioredis_new(config: i64) -> Handle (i64) - synchronous, connects lazily
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config ptr
            sig.returns.push(AbiParam::new(types::I64)); // Handle (not Promise)
            let func_id = self.module.declare_function("js_ioredis_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_new"), func_id);
        }

        // js_ioredis_set(handle: i64, key: i64, value: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::I64)); // value string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_set", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_set"), func_id);
        }

        // js_ioredis_get(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_get"), func_id);
        }

        // js_ioredis_del(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_del", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_del"), func_id);
        }

        // js_ioredis_exists(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_exists", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_exists"), func_id);
        }

        // js_ioredis_incr(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_incr", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_incr"), func_id);
        }

        // js_ioredis_decr(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_decr", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_decr"), func_id);
        }

        // js_ioredis_expire(handle: i64, key: i64, seconds: f64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::F64)); // seconds
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_expire", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_expire"), func_id);
        }

        // js_ioredis_quit(handle: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_quit", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_quit"), func_id);
        }

        // js_ioredis_connect(handle: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_connect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_connect"), func_id);
        }

        // js_ioredis_setex(handle: i64, key: i64, seconds: f64, value: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::F64)); // seconds
            sig.params.push(AbiParam::new(types::I64)); // value string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_setex", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_setex"), func_id);
        }

        // js_ioredis_hget(handle: i64, key: i64, field: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::I64)); // field string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_hget", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_hget"), func_id);
        }

        // js_ioredis_hset(handle: i64, key: i64, field: i64, value: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::I64)); // field string ptr
            sig.params.push(AbiParam::new(types::I64)); // value string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_hset", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_hset"), func_id);
        }

        // js_ioredis_hgetall(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_hgetall", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_hgetall"), func_id);
        }

        // js_ioredis_hdel(handle: i64, key: i64, field: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::I64)); // field string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_hdel", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_hdel"), func_id);
        }

        // js_ioredis_hlen(handle: i64, key: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_hlen", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_hlen"), func_id);
        }

        // js_ioredis_disconnect(handle: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_ioredis_disconnect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_disconnect"), func_id);
        }

        // js_ioredis_ping(handle: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ioredis_ping", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ioredis_ping"), func_id);
        }

        // ========================================================================
        // Crypto Functions
        // ========================================================================

        // js_crypto_sha256(data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data string ptr
            sig.returns.push(AbiParam::new(types::I64)); // hex string ptr
            let func_id = self.module.declare_function("js_crypto_sha256", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_sha256"), func_id);
        }

        // js_crypto_md5(data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data string ptr
            sig.returns.push(AbiParam::new(types::I64)); // hex string ptr
            let func_id = self.module.declare_function("js_crypto_md5", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_md5"), func_id);
        }

        // js_crypto_random_bytes_hex(size: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // size
            sig.returns.push(AbiParam::new(types::I64)); // hex string ptr
            let func_id = self.module.declare_function("js_crypto_random_bytes_hex", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_random_bytes_hex"), func_id);
        }

        // js_crypto_random_bytes_buffer(size: f64) -> i64 (buffer ptr)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // size
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_crypto_random_bytes_buffer", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_random_bytes_buffer"), func_id);
        }

        // js_crypto_random_uuid() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // uuid string ptr
            let func_id = self.module.declare_function("js_crypto_random_uuid", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_random_uuid"), func_id);
        }

        // js_crypto_hmac_sha256(key: i64, data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::I64)); // data string ptr
            sig.returns.push(AbiParam::new(types::I64)); // hex string ptr
            let func_id = self.module.declare_function("js_crypto_hmac_sha256", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_hmac_sha256"), func_id);
        }

        // ========================================================================
        // OS Functions
        // ========================================================================

        // js_os_platform() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_platform", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_platform"), func_id);
        }

        // js_os_arch() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_arch", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_arch"), func_id);
        }

        // js_os_hostname() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_hostname", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_hostname"), func_id);
        }

        // js_os_homedir() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_homedir", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_homedir"), func_id);
        }

        // js_os_tmpdir() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_tmpdir", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_tmpdir"), func_id);
        }

        // js_os_totalmem() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_os_totalmem", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_totalmem"), func_id);
        }

        // js_os_freemem() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_os_freemem", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_freemem"), func_id);
        }

        // js_os_uptime() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_os_uptime", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_uptime"), func_id);
        }

        // js_process_uptime() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_process_uptime", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_process_uptime"), func_id);
        }

        // js_process_cwd() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_process_cwd", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_process_cwd"), func_id);
        }

        // js_process_argv() -> i64 (array ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_process_argv", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_process_argv"), func_id);
        }

        // js_process_memory_usage() -> f64 (NaN-boxed object pointer)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_process_memory_usage", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_process_memory_usage"), func_id);
        }

        // js_os_type() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_type", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_type"), func_id);
        }

        // js_os_release() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_release", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_release"), func_id);
        }

        // js_os_eol() -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_eol", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_eol"), func_id);
        }

        // js_os_cpus() -> i64 (array ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_cpus", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_cpus"), func_id);
        }

        // js_os_network_interfaces() -> i64 (object ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_network_interfaces", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_network_interfaces"), func_id);
        }

        // js_os_user_info() -> i64 (object ptr)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_os_user_info", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_os_user_info"), func_id);
        }

        // ========================================================================
        // Buffer Functions
        // ========================================================================

        // js_buffer_from_string(str_ptr: i64, encoding: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string ptr
            sig.params.push(AbiParam::new(types::I32)); // encoding
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_buffer_from_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_from_string"), func_id);
        }

        // js_buffer_from_array(arr_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array ptr
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_buffer_from_array", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_from_array"), func_id);
        }

        // js_buffer_from_value(value: i64, encoding: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // value (string, array, or buffer)
            sig.params.push(AbiParam::new(types::I32)); // encoding
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_buffer_from_value", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_from_value"), func_id);
        }

        // js_buffer_alloc(size: i32, fill: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // size
            sig.params.push(AbiParam::new(types::I32)); // fill
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_buffer_alloc", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_alloc"), func_id);
        }

        // js_buffer_alloc_unsafe(size: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // size
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_buffer_alloc_unsafe", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_alloc_unsafe"), func_id);
        }

        // js_buffer_concat(arr_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array ptr
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_buffer_concat", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_concat"), func_id);
        }

        // js_buffer_is_buffer(ptr: i64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ptr
            sig.returns.push(AbiParam::new(types::I32)); // boolean
            let func_id = self.module.declare_function("js_buffer_is_buffer", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_is_buffer"), func_id);
        }

        // js_buffer_byte_length(str_ptr: i64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string ptr
            sig.returns.push(AbiParam::new(types::I32)); // length
            let func_id = self.module.declare_function("js_buffer_byte_length", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_byte_length"), func_id);
        }

        // js_buffer_to_string(buf_ptr: i64, encoding: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer ptr
            sig.params.push(AbiParam::new(types::I32)); // encoding
            sig.returns.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("js_buffer_to_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_to_string"), func_id);
        }

        // js_buffer_length(buf_ptr: i64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer ptr
            sig.returns.push(AbiParam::new(types::I32)); // length
            let func_id = self.module.declare_function("js_buffer_length", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_length"), func_id);
        }

        // js_buffer_slice(buf_ptr: i64, start: i32, end: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer ptr
            sig.params.push(AbiParam::new(types::I32)); // start
            sig.params.push(AbiParam::new(types::I32)); // end
            sig.returns.push(AbiParam::new(types::I64)); // new buffer ptr
            let func_id = self.module.declare_function("js_buffer_slice", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_slice"), func_id);
        }

        // js_buffer_copy(src: i64, dst: i64, target_start: i32, source_start: i32, source_end: i32) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // src buffer ptr
            sig.params.push(AbiParam::new(types::I64)); // dst buffer ptr
            sig.params.push(AbiParam::new(types::I32)); // target_start
            sig.params.push(AbiParam::new(types::I32)); // source_start
            sig.params.push(AbiParam::new(types::I32)); // source_end
            sig.returns.push(AbiParam::new(types::I32)); // bytes copied
            let func_id = self.module.declare_function("js_buffer_copy", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_copy"), func_id);
        }

        // js_buffer_write(buf_ptr: i64, str_ptr: i64, offset: i32, encoding: i32) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer ptr
            sig.params.push(AbiParam::new(types::I64)); // string ptr
            sig.params.push(AbiParam::new(types::I32)); // offset
            sig.params.push(AbiParam::new(types::I32)); // encoding
            sig.returns.push(AbiParam::new(types::I32)); // bytes written
            let func_id = self.module.declare_function("js_buffer_write", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_write"), func_id);
        }

        // js_buffer_set_from(target: i64, source: i64, offset: i32) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // target buffer ptr
            sig.params.push(AbiParam::new(types::I64)); // source buffer ptr
            sig.params.push(AbiParam::new(types::I32)); // offset
            let func_id = self.module.declare_function("js_buffer_set_from", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_set_from"), func_id);
        }

        // js_buffer_fill(buf_ptr: i64, value: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buf_ptr
            sig.params.push(AbiParam::new(types::I32)); // fill value
            sig.returns.push(AbiParam::new(types::I64)); // same buffer ptr
            let func_id = self.module.declare_function("js_buffer_fill", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_fill"), func_id);
        }

        // js_buffer_equals(buf1: i64, buf2: i64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer 1 ptr
            sig.params.push(AbiParam::new(types::I64)); // buffer 2 ptr
            sig.returns.push(AbiParam::new(types::I32)); // boolean
            let func_id = self.module.declare_function("js_buffer_equals", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_equals"), func_id);
        }

        // js_buffer_get(buf_ptr: i64, index: i32) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer ptr
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.returns.push(AbiParam::new(types::I32)); // byte value
            let func_id = self.module.declare_function("js_buffer_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_get"), func_id);
        }

        // js_buffer_set(buf_ptr: i64, index: i32, value: i32) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer ptr
            sig.params.push(AbiParam::new(types::I32)); // index
            sig.params.push(AbiParam::new(types::I32)); // value
            let func_id = self.module.declare_function("js_buffer_set", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_buffer_set"), func_id);
        }

        // ========================================================================
        // Child Process Functions
        // ========================================================================

        // js_child_process_exec_sync(cmd_ptr: i64, options_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // command string ptr
            sig.params.push(AbiParam::new(types::I64)); // options object ptr
            sig.returns.push(AbiParam::new(types::I64)); // buffer ptr
            let func_id = self.module.declare_function("js_child_process_exec_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_child_process_exec_sync"), func_id);
        }

        // js_child_process_spawn_sync(cmd_ptr: i64, args_ptr: i64, options_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // command string ptr
            sig.params.push(AbiParam::new(types::I64)); // args array ptr
            sig.params.push(AbiParam::new(types::I64)); // options object ptr
            sig.returns.push(AbiParam::new(types::I64)); // result object ptr
            let func_id = self.module.declare_function("js_child_process_spawn_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_child_process_spawn_sync"), func_id);
        }

        // js_child_process_spawn_background(cmd_val: f64, args_ptr: i64, log_file_val: f64, env_json_val: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed cmd string
            sig.params.push(AbiParam::new(types::I64)); // args array ptr (raw)
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed log file path
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed env JSON string
            sig.returns.push(AbiParam::new(types::I64)); // result object ptr {pid, handleId}
            let func_id = self.module.declare_function("js_child_process_spawn_background", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_child_process_spawn_background"), func_id);
        }

        // js_child_process_get_process_status(handle_id: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // handle ID
            sig.returns.push(AbiParam::new(types::I64)); // result object ptr {alive, exitCode}
            let func_id = self.module.declare_function("js_child_process_get_process_status", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_child_process_get_process_status"), func_id);
        }

        // js_child_process_kill_process(handle_id: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // handle ID
            sig.returns.push(AbiParam::new(types::I32)); // 1=success, 0=failure
            let func_id = self.module.declare_function("js_child_process_kill_process", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_child_process_kill_process"), func_id);
        }

        // ========================================================================
        // Net Functions
        // ========================================================================

        // js_net_create_server(options_ptr: i64, connection_listener_ptr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // options object ptr
            sig.params.push(AbiParam::new(types::I64)); // connection listener ptr
            sig.returns.push(AbiParam::new(types::F64)); // server handle
            let func_id = self.module.declare_function("js_net_create_server", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_net_create_server"), func_id);
        }

        // js_net_create_connection(port: i32, host_ptr: i64, connect_listener_ptr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // port
            sig.params.push(AbiParam::new(types::I64)); // host string ptr
            sig.params.push(AbiParam::new(types::I64)); // connect listener ptr
            sig.returns.push(AbiParam::new(types::F64)); // socket handle
            let func_id = self.module.declare_function("js_net_create_connection", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_net_create_connection"), func_id);
        }

        // ========================================================================
        // Zlib Functions
        // ========================================================================

        // js_zlib_gzip_sync(data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data ptr
            sig.returns.push(AbiParam::new(types::I64)); // compressed ptr
            let func_id = self.module.declare_function("js_zlib_gzip_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_zlib_gzip_sync"), func_id);
        }

        // js_zlib_gunzip_sync(data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data ptr
            sig.returns.push(AbiParam::new(types::I64)); // decompressed ptr
            let func_id = self.module.declare_function("js_zlib_gunzip_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_zlib_gunzip_sync"), func_id);
        }

        // js_zlib_deflate_sync(data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data ptr
            sig.returns.push(AbiParam::new(types::I64)); // compressed ptr
            let func_id = self.module.declare_function("js_zlib_deflate_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_zlib_deflate_sync"), func_id);
        }

        // js_zlib_inflate_sync(data: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data ptr
            sig.returns.push(AbiParam::new(types::I64)); // decompressed ptr
            let func_id = self.module.declare_function("js_zlib_inflate_sync", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_zlib_inflate_sync"), func_id);
        }

        // js_zlib_gzip(data: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_zlib_gzip", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_zlib_gzip"), func_id);
        }

        // js_zlib_gunzip(data: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_zlib_gunzip", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_zlib_gunzip"), func_id);
        }

        // ========================================================================
        // Fetch Functions (node-fetch)
        // ========================================================================

        // js_fetch_get(url: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_get"), func_id);
        }

        // js_fetch_get_with_auth(url: i64, auth_header: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.params.push(AbiParam::new(types::I64)); // auth header string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_get_with_auth", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_get_with_auth"), func_id);
        }

        // js_fetch_post_with_auth(url: i64, auth_header: i64, body: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.params.push(AbiParam::new(types::I64)); // auth header string ptr
            sig.params.push(AbiParam::new(types::I64)); // body string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_post_with_auth", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_post_with_auth"), func_id);
        }

        // js_fetch_post(url: i64, body: i64, content_type: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.params.push(AbiParam::new(types::I64)); // body string ptr
            sig.params.push(AbiParam::new(types::I64)); // content_type string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_post", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_post"), func_id);
        }

        // js_fetch_with_options(url: i64, method: i64, body: i64, headers_json: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.params.push(AbiParam::new(types::I64)); // method string ptr
            sig.params.push(AbiParam::new(types::I64)); // body string ptr (nullable)
            sig.params.push(AbiParam::new(types::I64)); // headers JSON string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_with_options", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_with_options"), func_id);
            // Also register as "fetch" for global fetch calls
            self.extern_funcs.insert(Cow::Borrowed("fetch"), func_id);
        }

        // js_fetch_text(url: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_text", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_text"), func_id);
        }

        // js_fetch_response_status(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // status
            let func_id = self.module.declare_function("js_fetch_response_status", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_response_status"), func_id);
        }

        // js_fetch_response_ok(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // ok (boolean)
            let func_id = self.module.declare_function("js_fetch_response_ok", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_response_ok"), func_id);
        }

        // js_fetch_response_status_text(handle: i64) -> *mut StringHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // StringHeader ptr
            let func_id = self.module.declare_function("js_fetch_response_status_text", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_response_status_text"), func_id);
        }

        // js_fetch_response_text(handle: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_response_text", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_response_text"), func_id);
        }

        // js_fetch_response_json(handle: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_fetch_response_json", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_response_json"), func_id);
        }

        // SSE Streaming functions
        // js_fetch_stream_start(url: i64, method: i64, body: i64, headers_json: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fetch_stream_start", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_stream_start"), func_id);
        }
        // js_fetch_stream_poll(handle: f64) -> i64 (StringHeader ptr)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fetch_stream_poll", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_stream_poll"), func_id);
        }
        // js_fetch_stream_status(handle: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fetch_stream_status", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_stream_status"), func_id);
        }
        // js_fetch_stream_close(handle: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fetch_stream_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fetch_stream_close"), func_id);
        }

        // ========================================================================
        // HTTP/HTTPS Client Functions (http, https)
        // ========================================================================

        // js_http_request(options: f64, callback: i64) -> i64 (ClientRequest handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // options object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // response callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // ClientRequest handle
            let func_id = self.module.declare_function("js_http_request", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_request"), func_id);
        }

        // js_https_request(options: f64, callback: i64) -> i64 (ClientRequest handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // options object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // response callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // ClientRequest handle
            let func_id = self.module.declare_function("js_https_request", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_https_request"), func_id);
        }

        // js_http_get(url_or_options: f64, callback: i64) -> i64 (ClientRequest handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // url string or options object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // response callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // ClientRequest handle
            let func_id = self.module.declare_function("js_http_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_get"), func_id);
        }

        // js_https_get(url_or_options: f64, callback: i64) -> i64 (ClientRequest handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // url string or options object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // response callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // ClientRequest handle
            let func_id = self.module.declare_function("js_https_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_https_get"), func_id);
        }

        // js_http_client_request_write(handle: i64, body: f64) -> i64 (handle for chaining)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ClientRequest handle
            sig.params.push(AbiParam::new(types::F64)); // body (NaN-boxed string)
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_http_client_request_write", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_client_request_write"), func_id);
        }

        // js_http_client_request_end(handle: i64, body: f64) -> i64 (handle for chaining)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ClientRequest handle
            sig.params.push(AbiParam::new(types::F64)); // optional body (NaN-boxed string or undefined)
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_http_client_request_end", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_client_request_end"), func_id);
        }

        // js_http_on(handle: i64, event_name: i64, callback: i64) -> i64 (handle for chaining)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle (ClientRequest or IncomingMessage)
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_http_on", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_on"), func_id);
        }

        // js_http_set_header(handle: i64, name: i64, value: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // header name string ptr
            sig.params.push(AbiParam::new(types::I64)); // header value string ptr
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_http_set_header", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_set_header"), func_id);
        }

        // js_http_set_timeout(handle: i64, ms: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // timeout milliseconds
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_http_set_timeout", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_set_timeout"), func_id);
        }

        // js_http_status_code(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // IncomingMessage handle
            sig.returns.push(AbiParam::new(types::F64)); // status code
            let func_id = self.module.declare_function("js_http_status_code", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_status_code"), func_id);
        }

        // js_http_status_message(handle: i64) -> i64 (StringHeader ptr)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // IncomingMessage handle
            sig.returns.push(AbiParam::new(types::I64)); // StringHeader ptr
            let func_id = self.module.declare_function("js_http_status_message", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_status_message"), func_id);
        }

        // js_http_response_headers(handle: i64) -> f64 (NaN-boxed object)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // IncomingMessage handle
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed headers object
            let func_id = self.module.declare_function("js_http_response_headers", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_response_headers"), func_id);
        }

        // ========================================================================
        // WebSocket Functions (ws)
        // ========================================================================

        // js_ws_connect(url: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string ptr
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ws_connect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_connect"), func_id);
        }

        // js_ws_connect_start(url: f64) -> f64 (ws_id directly, no Promise)
        // Takes NaN-boxed string directly; extracts pointer internally
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // url NaN-boxed string
            sig.returns.push(AbiParam::new(types::F64)); // ws_id as f64
            let func_id = self.module.declare_function("js_ws_connect_start", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_connect_start"), func_id);
        }

        // js_ws_message_count(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // count
            let func_id = self.module.declare_function("js_ws_message_count", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_message_count"), func_id);
        }

        // js_ws_send(handle: i64, message: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // message string ptr
            let func_id = self.module.declare_function("js_ws_send", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_send"), func_id);
        }

        // js_ws_close(handle: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_ws_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_close"), func_id);
        }

        // js_ws_is_open(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // is_open (boolean)
            let func_id = self.module.declare_function("js_ws_is_open", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_is_open"), func_id);
        }

        // js_ws_receive(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // message string ptr
            let func_id = self.module.declare_function("js_ws_receive", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_receive"), func_id);
        }

        // js_ws_wait_for_message(handle: i64, timeout_ms: f64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // timeout_ms
            sig.returns.push(AbiParam::new(types::I64)); // Promise ptr
            let func_id = self.module.declare_function("js_ws_wait_for_message", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_wait_for_message"), func_id);
        }

        // js_ws_server_new(opts_f64: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // opts (NaN-boxed object or number)
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_ws_server_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_server_new"), func_id);
        }

        // js_ws_handle_to_i64(val_f64: f64) -> i64
        // Converts WS values to i64: NaN-boxed pointers (server) or plain f64 (client)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_ws_handle_to_i64", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_handle_to_i64"), func_id);
        }

        // js_ws_on(handle: i64, event_name: i64, callback: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // returns handle for chaining
            let func_id = self.module.declare_function("js_ws_on", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_on"), func_id);
        }

        // js_ws_server_close(handle: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_ws_server_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ws_server_close"), func_id);
        }

        // ========================================================================
        // EventEmitter Functions (events)
        // ========================================================================

        // js_event_emitter_new() -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_event_emitter_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_new"), func_id);
        }

        // js_event_emitter_on(handle: i64, event_name: i64, callback: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // returns handle for chaining
            let func_id = self.module.declare_function("js_event_emitter_on", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_on"), func_id);
        }

        // js_event_emitter_emit(handle: i64, event_name: i64, arg: f64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.params.push(AbiParam::new(types::F64)); // argument
            sig.returns.push(AbiParam::new(types::F64)); // returns bool as f64
            let func_id = self.module.declare_function("js_event_emitter_emit", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_emit"), func_id);
        }

        // js_event_emitter_emit0(handle: i64, event_name: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.returns.push(AbiParam::new(types::F64)); // returns bool as f64
            let func_id = self.module.declare_function("js_event_emitter_emit0", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_emit0"), func_id);
        }

        // js_event_emitter_remove_listener(handle: i64, event_name: i64, callback: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::I64)); // returns handle for chaining
            let func_id = self.module.declare_function("js_event_emitter_remove_listener", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_remove_listener"), func_id);
        }

        // js_event_emitter_remove_all_listeners(handle: i64, event_name: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr (or null)
            sig.returns.push(AbiParam::new(types::I64)); // returns handle for chaining
            let func_id = self.module.declare_function("js_event_emitter_remove_all_listeners", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_remove_all_listeners"), func_id);
        }

        // js_event_emitter_listener_count(handle: i64, event_name: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // event name string ptr
            sig.returns.push(AbiParam::new(types::F64)); // count
            let func_id = self.module.declare_function("js_event_emitter_listener_count", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_event_emitter_listener_count"), func_id);
        }

        // ========================================================================
        // AsyncLocalStorage (async_hooks)
        // ========================================================================

        // js_async_local_storage_new() -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_async_local_storage_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_async_local_storage_new"), func_id);
        }

        // js_async_local_storage_run(handle: i64, store: f64, callback: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // store (any NaN-boxed value)
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function("js_async_local_storage_run", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_async_local_storage_run"), func_id);
        }

        // js_async_local_storage_get_store(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // store or undefined
            let func_id = self.module.declare_function("js_async_local_storage_get_store", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_async_local_storage_get_store"), func_id);
        }

        // js_async_local_storage_enter_with(handle: i64, store: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // store
            let func_id = self.module.declare_function("js_async_local_storage_enter_with", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_async_local_storage_enter_with"), func_id);
        }

        // js_async_local_storage_exit(handle: i64, callback: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // callback closure ptr
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function("js_async_local_storage_exit", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_async_local_storage_exit"), func_id);
        }

        // js_async_local_storage_disable(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_async_local_storage_disable", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_async_local_storage_disable"), func_id);
        }

        // ========================================================================
        // LRUCache
        // ========================================================================

        // js_lru_cache_new(max_size: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // max_size
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_lru_cache_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_new"), func_id);
        }

        // js_lru_cache_get(handle: i64, key: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function("js_lru_cache_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_get"), func_id);
        }

        // js_lru_cache_set(handle: i64, key: f64, value: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // returns handle
            let func_id = self.module.declare_function("js_lru_cache_set", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_set"), func_id);
        }

        // js_lru_cache_has(handle: i64, key: f64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // bool as f64
            let func_id = self.module.declare_function("js_lru_cache_has", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_has"), func_id);
        }

        // js_lru_cache_delete(handle: i64, key: f64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // bool as f64
            let func_id = self.module.declare_function("js_lru_cache_delete", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_delete"), func_id);
        }

        // js_lru_cache_clear(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_lru_cache_clear", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_clear"), func_id);
        }

        // js_lru_cache_size(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // size
            let func_id = self.module.declare_function("js_lru_cache_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_size"), func_id);
        }

        // js_lru_cache_peek(handle: i64, key: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function("js_lru_cache_peek", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lru_cache_peek"), func_id);
        }

        // ========================================================================
        // Commander
        // ========================================================================

        // js_commander_new() -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_new"), func_id);
        }

        // js_commander_name(handle: i64, name: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // name string
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_name", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_name"), func_id);
        }

        // js_commander_description(handle: i64, desc: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // description string
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_description", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_description"), func_id);
        }

        // js_commander_version(handle: i64, version: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // version string
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_version", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_version"), func_id);
        }

        // js_commander_option(handle: i64, flags: i64, desc: i64, default: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // flags string
            sig.params.push(AbiParam::new(types::I64)); // description string
            sig.params.push(AbiParam::new(types::I64)); // default value string (or null)
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_option", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_option"), func_id);
        }

        // js_commander_required_option(handle: i64, flags: i64, desc: i64, default: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // flags string
            sig.params.push(AbiParam::new(types::I64)); // description string
            sig.params.push(AbiParam::new(types::I64)); // default value (or null)
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_required_option", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_required_option"), func_id);
        }

        // js_commander_action(handle: i64, callback: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // callback closure
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_action", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_action"), func_id);
        }

        // js_commander_command(handle: i64, name: i64) -> i64 (new subcommand handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // name string
            sig.returns.push(AbiParam::new(types::I64)); // new handle
            let func_id = self.module.declare_function("js_commander_command", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_command"), func_id);
        }

        // js_commander_parse(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_parse", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_parse"), func_id);
        }

        // js_commander_opts(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_commander_opts", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_opts"), func_id);
        }

        // js_commander_get_option(handle: i64, name: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // name string
            sig.returns.push(AbiParam::new(types::I64)); // value string
            let func_id = self.module.declare_function("js_commander_get_option", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_get_option"), func_id);
        }

        // js_commander_get_option_number(handle: i64, name: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // name string
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function("js_commander_get_option_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_get_option_number"), func_id);
        }

        // js_commander_get_option_bool(handle: i64, name: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // name string
            sig.returns.push(AbiParam::new(types::F64)); // bool as f64
            let func_id = self.module.declare_function("js_commander_get_option_bool", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_commander_get_option_bool"), func_id);
        }

        // ========================================================================
        // Decimal (Big.js / Decimal.js / BigNumber.js)
        // ========================================================================

        // js_decimal_from_number(value: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_decimal_from_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_from_number"), func_id);
        }

        // js_decimal_from_string(value: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // value string
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_decimal_from_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_from_string"), func_id);
        }

        // js_decimal_plus(handle: i64, other: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_plus", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_plus"), func_id);
        }

        // js_decimal_plus_number(handle: i64, other: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // other number
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_plus_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_plus_number"), func_id);
        }

        // js_decimal_minus(handle: i64, other: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_minus", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_minus"), func_id);
        }

        // js_decimal_times(handle: i64, other: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_times", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_times"), func_id);
        }

        // js_decimal_div(handle: i64, other: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_div", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_div"), func_id);
        }

        // js_decimal_to_fixed(handle: i64, decimals: f64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // decimals
            sig.returns.push(AbiParam::new(types::I64)); // result string
            let func_id = self.module.declare_function("js_decimal_to_fixed", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_to_fixed"), func_id);
        }

        // js_decimal_to_string(handle: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // result string
            let func_id = self.module.declare_function("js_decimal_to_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_to_string"), func_id);
        }

        // js_decimal_to_number(handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function("js_decimal_to_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_to_number"), func_id);
        }

        // js_decimal_sqrt(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_sqrt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_sqrt"), func_id);
        }

        // js_decimal_abs(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // result handle
            let func_id = self.module.declare_function("js_decimal_abs", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_abs"), func_id);
        }

        // js_decimal_eq(handle: i64, other: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::F64)); // bool as f64
            let func_id = self.module.declare_function("js_decimal_eq", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_eq"), func_id);
        }

        // js_decimal_lt(handle: i64, other: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::F64)); // bool as f64
            let func_id = self.module.declare_function("js_decimal_lt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_lt"), func_id);
        }

        // js_decimal_gt(handle: i64, other: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other handle
            sig.returns.push(AbiParam::new(types::F64)); // bool as f64
            let func_id = self.module.declare_function("js_decimal_gt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_decimal_gt"), func_id);
        }

        // ========================================================================
        // Tier 3: dotenv, jsonwebtoken, nanoid, slugify, validator
        // ========================================================================

        // js_dotenv_config() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64)); // success flag
            let func_id = self.module.declare_function("js_dotenv_config", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dotenv_config"), func_id);
        }

        // js_dotenv_config_path(path: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string
            sig.returns.push(AbiParam::new(types::F64)); // success flag
            let func_id = self.module.declare_function("js_dotenv_config_path", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dotenv_config_path"), func_id);
        }

        // js_dotenv_parse(content: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // content string
            sig.returns.push(AbiParam::new(types::I64)); // JSON string
            let func_id = self.module.declare_function("js_dotenv_parse", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dotenv_parse"), func_id);
        }

        // js_jwt_sign(payload: i64, secret: i64, expiry: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // payload
            sig.params.push(AbiParam::new(types::I64)); // secret
            sig.params.push(AbiParam::new(types::F64)); // expiry seconds
            sig.returns.push(AbiParam::new(types::I64)); // token string
            let func_id = self.module.declare_function("js_jwt_sign", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_jwt_sign"), func_id);
        }

        // js_jwt_verify(token: i64, secret: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // token
            sig.params.push(AbiParam::new(types::I64)); // secret
            sig.returns.push(AbiParam::new(types::I64)); // payload or null
            let func_id = self.module.declare_function("js_jwt_verify", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_jwt_verify"), func_id);
        }

        // js_jwt_decode(token: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // token
            sig.returns.push(AbiParam::new(types::I64)); // decoded payload
            let func_id = self.module.declare_function("js_jwt_decode", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_jwt_decode"), func_id);
        }

        // js_nanoid(size: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // size
            sig.returns.push(AbiParam::new(types::I64)); // id string
            let func_id = self.module.declare_function("js_nanoid", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanoid"), func_id);
        }

        // js_nanoid_custom(alphabet: i64, size: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // alphabet
            sig.params.push(AbiParam::new(types::F64)); // size
            sig.returns.push(AbiParam::new(types::I64)); // id string
            let func_id = self.module.declare_function("js_nanoid_custom", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_nanoid_custom"), func_id);
        }

        // js_slugify(str: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.returns.push(AbiParam::new(types::I64)); // slug
            let func_id = self.module.declare_function("js_slugify", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_slugify"), func_id);
        }

        // js_slugify_strict(str: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.returns.push(AbiParam::new(types::I64)); // slug
            let func_id = self.module.declare_function("js_slugify_strict", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_slugify_strict"), func_id);
        }

        // Validator functions - all take i64 string and return f64 boolean
        for name in &[
            "js_validator_is_email", "js_validator_is_url", "js_validator_is_uuid",
            "js_validator_is_alpha", "js_validator_is_alphanumeric", "js_validator_is_numeric",
            "js_validator_is_hexadecimal", "js_validator_is_int", "js_validator_is_float",
            "js_validator_is_empty", "js_validator_is_json", "js_validator_is_lowercase",
            "js_validator_is_uppercase",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_validator_contains(str: i64, substr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_validator_contains", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_validator_contains"), func_id);
        }

        // js_validator_equals(str1: i64, str2: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_validator_equals", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_validator_equals"), func_id);
        }

        // js_validator_is_length(str: i64, min: f64, max: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_validator_is_length", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_validator_is_length"), func_id);
        }

        // ========================================================================
        // Tier 4: pg (PostgreSQL)
        // ========================================================================

        // js_pg_connect(config: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config object
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_connect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_connect"), func_id);
        }

        // js_pg_client_end(client: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // client handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_client_end", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_client_end"), func_id);
        }

        // js_pg_client_query(client: i64, sql: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // client handle
            sig.params.push(AbiParam::new(types::I64)); // sql string
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_client_query", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_client_query"), func_id);
        }

        // js_pg_client_query_params(client: i64, sql: i64, params: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // client handle
            sig.params.push(AbiParam::new(types::I64)); // sql string
            sig.params.push(AbiParam::new(types::I64)); // params array
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_client_query_params", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_client_query_params"), func_id);
        }

        // js_pg_create_pool(config: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config object
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_create_pool", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_create_pool"), func_id);
        }

        // js_pg_pool_query(pool: i64, sql: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pool handle
            sig.params.push(AbiParam::new(types::I64)); // sql string
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_pool_query", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_pool_query"), func_id);
        }

        // js_pg_pool_end(pool: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pool handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_pg_pool_end", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_pg_pool_end"), func_id);
        }

        // ========================================================================
        // Tier 4: nodemailer
        // ========================================================================

        // js_nodemailer_create_transport(config: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config object
            sig.returns.push(AbiParam::new(types::F64)); // handle
            let func_id = self.module.declare_function("js_nodemailer_create_transport", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_nodemailer_create_transport"), func_id);
        }

        // js_nodemailer_send_mail(transport: i64, options: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // transport handle
            sig.params.push(AbiParam::new(types::I64)); // mail options
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_nodemailer_send_mail", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_nodemailer_send_mail"), func_id);
        }

        // js_nodemailer_verify(transport: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // transport handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_nodemailer_verify", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_nodemailer_verify"), func_id);
        }

        // ========================================================================
        // Tier 4: crypto extended (AES, pbkdf2, scrypt)
        // ========================================================================

        // js_crypto_aes256_encrypt(data: i64, key: i64, iv: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.params.push(AbiParam::new(types::I64)); // iv
            sig.returns.push(AbiParam::new(types::I64)); // encrypted (base64)
            let func_id = self.module.declare_function("js_crypto_aes256_encrypt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_aes256_encrypt"), func_id);
        }

        // js_crypto_aes256_decrypt(data: i64, key: i64, iv: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // data (base64)
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.params.push(AbiParam::new(types::I64)); // iv
            sig.returns.push(AbiParam::new(types::I64)); // decrypted
            let func_id = self.module.declare_function("js_crypto_aes256_decrypt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_aes256_decrypt"), func_id);
        }

        // js_crypto_pbkdf2(password: i64, salt: i64, iterations: f64, keyLength: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password
            sig.params.push(AbiParam::new(types::I64)); // salt
            sig.params.push(AbiParam::new(types::F64)); // iterations
            sig.params.push(AbiParam::new(types::F64)); // keyLength
            sig.returns.push(AbiParam::new(types::I64)); // derived key (hex)
            let func_id = self.module.declare_function("js_crypto_pbkdf2", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_pbkdf2"), func_id);
        }

        // js_crypto_scrypt(password: i64, salt: i64, keyLength: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password
            sig.params.push(AbiParam::new(types::I64)); // salt
            sig.params.push(AbiParam::new(types::F64)); // keyLength
            sig.returns.push(AbiParam::new(types::I64)); // derived key (hex)
            let func_id = self.module.declare_function("js_crypto_scrypt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_scrypt"), func_id);
        }

        // js_crypto_scrypt_custom(password: i64, salt: i64, keyLength: f64, logN: f64, r: f64, p: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password
            sig.params.push(AbiParam::new(types::I64)); // salt
            sig.params.push(AbiParam::new(types::F64)); // keyLength
            sig.params.push(AbiParam::new(types::F64)); // logN
            sig.params.push(AbiParam::new(types::F64)); // r
            sig.params.push(AbiParam::new(types::F64)); // p
            sig.returns.push(AbiParam::new(types::I64)); // derived key (hex)
            let func_id = self.module.declare_function("js_crypto_scrypt_custom", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_scrypt_custom"), func_id);
        }

        // ========================================================================
        // Tier 4: crypto E2E (X25519, AES-256-GCM, HKDF)
        // ========================================================================

        // js_crypto_x25519_keypair() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // JSON string ptr
            let func_id = self.module.declare_function("js_crypto_x25519_keypair", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_x25519_keypair"), func_id);
        }

        // js_crypto_x25519_shared_secret(secret: i64, public: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // secret key (hex)
            sig.params.push(AbiParam::new(types::I64)); // public key (hex)
            sig.returns.push(AbiParam::new(types::I64)); // shared secret (hex)
            let func_id = self.module.declare_function("js_crypto_x25519_shared_secret", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_x25519_shared_secret"), func_id);
        }

        // js_crypto_aes256_gcm_encrypt(plaintext: i64, key: i64, nonce: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // plaintext
            sig.params.push(AbiParam::new(types::I64)); // key (hex)
            sig.params.push(AbiParam::new(types::I64)); // nonce (hex)
            sig.returns.push(AbiParam::new(types::I64)); // ciphertext (base64)
            let func_id = self.module.declare_function("js_crypto_aes256_gcm_encrypt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_aes256_gcm_encrypt"), func_id);
        }

        // js_crypto_aes256_gcm_decrypt(ciphertext: i64, key: i64, nonce: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ciphertext (base64)
            sig.params.push(AbiParam::new(types::I64)); // key (hex)
            sig.params.push(AbiParam::new(types::I64)); // nonce (hex)
            sig.returns.push(AbiParam::new(types::I64)); // plaintext
            let func_id = self.module.declare_function("js_crypto_aes256_gcm_decrypt", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_aes256_gcm_decrypt"), func_id);
        }

        // js_crypto_random_nonce() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // hex nonce
            let func_id = self.module.declare_function("js_crypto_random_nonce", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_random_nonce"), func_id);
        }

        // js_crypto_hkdf_sha256(ikm: i64, salt: i64, info: i64, length: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ikm (hex)
            sig.params.push(AbiParam::new(types::I64)); // salt (hex)
            sig.params.push(AbiParam::new(types::I64)); // info (utf8)
            sig.params.push(AbiParam::new(types::F64)); // length
            sig.returns.push(AbiParam::new(types::I64)); // derived key (hex)
            let func_id = self.module.declare_function("js_crypto_hkdf_sha256", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_crypto_hkdf_sha256"), func_id);
        }

        // ========================================================================
        // Tier 4: dayjs/date-fns
        // ========================================================================

        // js_dayjs_now() -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_now", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_now"), func_id);
        }

        // js_dayjs_from_timestamp(timestamp: f64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_from_timestamp", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_from_timestamp"), func_id);
        }

        // js_dayjs_parse(dateStr: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_parse", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_parse"), func_id);
        }

        // js_dayjs_format(handle: i64, pattern: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_dayjs_format", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_format"), func_id);
        }

        // js_dayjs_to_iso_string(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_dayjs_to_iso_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_to_iso_string"), func_id);
        }

        // Dayjs getter methods (handle: i64) -> f64
        for name in &[
            "js_dayjs_value_of", "js_dayjs_unix", "js_dayjs_year", "js_dayjs_month",
            "js_dayjs_date", "js_dayjs_day", "js_dayjs_hour", "js_dayjs_minute",
            "js_dayjs_second", "js_dayjs_millisecond", "js_dayjs_is_valid",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_dayjs_add(handle: i64, value: f64, unit: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_add", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_add"), func_id);
        }

        // js_dayjs_subtract(handle: i64, value: f64, unit: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_subtract", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_subtract"), func_id);
        }

        // js_dayjs_start_of(handle: i64, unit: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_start_of", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_start_of"), func_id);
        }

        // js_dayjs_end_of(handle: i64, unit: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_end_of", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_end_of"), func_id);
        }

        // js_dayjs_diff(handle: i64, other: i64, unit: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_dayjs_diff", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dayjs_diff"), func_id);
        }

        // Dayjs comparison methods (handle: i64, other: i64) -> f64
        for name in &["js_dayjs_is_before", "js_dayjs_is_after", "js_dayjs_is_same"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // date-fns compatible functions
        // js_datefns_format(timestamp: f64, pattern: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_datefns_format", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_datefns_format"), func_id);
        }

        // js_datefns_parse_iso(dateStr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_datefns_parse_iso", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_datefns_parse_iso"), func_id);
        }

        // date-fns add functions (timestamp: f64, amount: f64) -> f64
        for name in &["js_datefns_add_days", "js_datefns_add_months", "js_datefns_add_years"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // date-fns difference functions (left: f64, right: f64) -> f64
        for name in &[
            "js_datefns_difference_in_days", "js_datefns_difference_in_hours",
            "js_datefns_difference_in_minutes", "js_datefns_is_after", "js_datefns_is_before",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // date-fns startOf/endOf functions (timestamp: f64) -> f64
        for name in &["js_datefns_start_of_day", "js_datefns_end_of_day"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // ========================================================================
        // Tier 5: axios (HTTP client)
        // ========================================================================

        // js_axios_get(url: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_axios_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_axios_get"), func_id);
        }

        // js_axios_post(url: i64, body: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url
            sig.params.push(AbiParam::new(types::I64)); // body
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_axios_post", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_axios_post"), func_id);
        }

        // js_axios_put(url: i64, body: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url
            sig.params.push(AbiParam::new(types::I64)); // body
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_axios_put", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_axios_put"), func_id);
        }

        // js_axios_delete(url: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_axios_delete", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_axios_delete"), func_id);
        }

        // js_axios_request(config: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_axios_request", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_axios_request"), func_id);
        }

        // js_axios_create(config: i64) -> f64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // config (JSON)
            sig.returns.push(AbiParam::new(types::F64)); // handle
            let func_id = self.module.declare_function("js_axios_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_axios_create"), func_id);
        }

        // ========================================================================
        // Tier 5: argon2 (password hashing)
        // ========================================================================

        // js_argon2_hash(password: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_argon2_hash", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_argon2_hash"), func_id);
        }

        // js_argon2_hash_options(password: i64, options: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // password
            sig.params.push(AbiParam::new(types::I64)); // options (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_argon2_hash_options", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_argon2_hash_options"), func_id);
        }

        // js_argon2_verify(hash: i64, password: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // hash
            sig.params.push(AbiParam::new(types::I64)); // password
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_argon2_verify", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_argon2_verify"), func_id);
        }

        // ========================================================================
        // Tier 5: mongodb
        // ========================================================================

        // js_mongodb_connect(uri: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // uri
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_mongodb_connect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_connect"), func_id);
        }

        // js_mongodb_client_db(client: i64, name: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // client handle
            sig.params.push(AbiParam::new(types::I64)); // name
            sig.returns.push(AbiParam::new(types::I64)); // db handle
            let func_id = self.module.declare_function("js_mongodb_client_db", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_client_db"), func_id);
        }

        // js_mongodb_db_collection(db: i64, name: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // db handle
            sig.params.push(AbiParam::new(types::I64)); // name
            sig.returns.push(AbiParam::new(types::I64)); // collection handle
            let func_id = self.module.declare_function("js_mongodb_db_collection", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_db_collection"), func_id);
        }

        // MongoDB collection methods (coll: i64, filter: i64) -> Promise (i64)
        for name in &[
            "js_mongodb_collection_find_one", "js_mongodb_collection_find",
            "js_mongodb_collection_delete_one", "js_mongodb_collection_delete_many",
            "js_mongodb_collection_count",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // collection handle
            sig.params.push(AbiParam::new(types::I64)); // filter (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_mongodb_collection_insert_one(coll: i64, doc: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // collection handle
            sig.params.push(AbiParam::new(types::I64)); // doc (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_mongodb_collection_insert_one", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_collection_insert_one"), func_id);
        }

        // js_mongodb_collection_insert_many(coll: i64, docs: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // collection handle
            sig.params.push(AbiParam::new(types::I64)); // docs (JSON array)
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_mongodb_collection_insert_many", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_collection_insert_many"), func_id);
        }

        // MongoDB update methods (coll: i64, filter: i64, update: i64) -> Promise (i64)
        for name in &["js_mongodb_collection_update_one", "js_mongodb_collection_update_many"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // collection handle
            sig.params.push(AbiParam::new(types::I64)); // filter (JSON)
            sig.params.push(AbiParam::new(types::I64)); // update (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_mongodb_client_close(client: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // client handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_mongodb_client_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_client_close"), func_id);
        }

        // js_mongodb_client_list_databases(client: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // client handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_mongodb_client_list_databases", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_client_list_databases"), func_id);
        }

        // js_mongodb_db_list_collections(db: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // db handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_mongodb_db_list_collections", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_mongodb_db_list_collections"), func_id);
        }

        // ========================================================================
        // Tier 5: sqlite (better-sqlite3 compatible)
        // ========================================================================

        // js_sqlite_open(path: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path
            sig.returns.push(AbiParam::new(types::I64)); // db handle
            let func_id = self.module.declare_function("js_sqlite_open", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sqlite_open"), func_id);
        }

        // js_sqlite_prepare(db: i64, sql: i64) -> i64 (statement handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // db handle
            sig.params.push(AbiParam::new(types::I64)); // sql
            sig.returns.push(AbiParam::new(types::I64)); // statement handle
            let func_id = self.module.declare_function("js_sqlite_prepare", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sqlite_prepare"), func_id);
        }

        // SQLite statement methods (stmt: i64, params: i64) -> i64
        for name in &["js_sqlite_stmt_run", "js_sqlite_stmt_get", "js_sqlite_stmt_all"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // statement handle
            sig.params.push(AbiParam::new(types::I64)); // params (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // result
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_sqlite_exec(db: i64, sql: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // db handle
            sig.params.push(AbiParam::new(types::I64)); // sql
            let func_id = self.module.declare_function("js_sqlite_exec", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sqlite_exec"), func_id);
        }

        // js_sqlite_close(db: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // db handle
            let func_id = self.module.declare_function("js_sqlite_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sqlite_close"), func_id);
        }

        // js_sqlite_transaction(db: i64) -> i64 (transaction handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // db handle
            sig.returns.push(AbiParam::new(types::I64)); // transaction handle
            let func_id = self.module.declare_function("js_sqlite_transaction", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sqlite_transaction"), func_id);
        }

        // SQLite transaction methods (tx: i64)
        for name in &["js_sqlite_transaction_commit", "js_sqlite_transaction_rollback"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // transaction handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // ========================================================================
        // Tier 5: sharp (image processing)
        // ========================================================================

        // js_sharp_from_file(path: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path
            sig.returns.push(AbiParam::new(types::I64)); // image handle
            let func_id = self.module.declare_function("js_sharp_from_file", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_from_file"), func_id);
        }

        // js_sharp_from_buffer(buffer: i64, len: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer
            sig.params.push(AbiParam::new(types::F64)); // length
            sig.returns.push(AbiParam::new(types::I64)); // image handle
            let func_id = self.module.declare_function("js_sharp_from_buffer", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_from_buffer"), func_id);
        }

        // js_sharp_resize(handle: i64, width: f64, height: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            sig.returns.push(AbiParam::new(types::I64)); // new image handle
            let func_id = self.module.declare_function("js_sharp_resize", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_resize"), func_id);
        }

        // Sharp methods (handle: i64, value: f64) -> i64 (handle)
        for name in &["js_sharp_rotate", "js_sharp_blur", "js_sharp_quality"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::I64)); // new image handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // Sharp methods (handle: i64) -> i64 (handle)
        for name in &["js_sharp_grayscale", "js_sharp_flip", "js_sharp_flop", "js_sharp_negate"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.returns.push(AbiParam::new(types::I64)); // new image handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_sharp_to_format(handle: i64, format: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.params.push(AbiParam::new(types::I64)); // format
            sig.returns.push(AbiParam::new(types::I64)); // new image handle
            let func_id = self.module.declare_function("js_sharp_to_format", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_to_format"), func_id);
        }

        // js_sharp_to_file(handle: i64, path: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.params.push(AbiParam::new(types::I64)); // path
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_sharp_to_file", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_to_file"), func_id);
        }

        // js_sharp_to_buffer(handle: i64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_sharp_to_buffer", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_to_buffer"), func_id);
        }

        // js_sharp_metadata(handle: i64) -> i64 (JSON string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // image handle
            sig.returns.push(AbiParam::new(types::I64)); // JSON metadata
            let func_id = self.module.declare_function("js_sharp_metadata", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_sharp_metadata"), func_id);
        }

        // ========================================================================
        // Tier 5: cheerio (HTML parsing)
        // ========================================================================

        // js_cheerio_load(html: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // html
            sig.returns.push(AbiParam::new(types::I64)); // document handle
            let func_id = self.module.declare_function("js_cheerio_load", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_load"), func_id);
        }

        // js_cheerio_load_fragment(html: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // html
            sig.returns.push(AbiParam::new(types::I64)); // document handle
            let func_id = self.module.declare_function("js_cheerio_load_fragment", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_load_fragment"), func_id);
        }

        // js_cheerio_select(doc: i64, selector: i64) -> i64 (selection handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // document handle
            sig.params.push(AbiParam::new(types::I64)); // selector
            sig.returns.push(AbiParam::new(types::I64)); // selection handle
            let func_id = self.module.declare_function("js_cheerio_select", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_select"), func_id);
        }

        // Cheerio selection methods (sel: i64) -> i64 (string)
        for name in &["js_cheerio_selection_text", "js_cheerio_selection_html"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.returns.push(AbiParam::new(types::I64)); // string
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_cheerio_selection_attr(sel: i64, attr: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.params.push(AbiParam::new(types::I64)); // attribute name
            sig.returns.push(AbiParam::new(types::I64)); // attribute value
            let func_id = self.module.declare_function("js_cheerio_selection_attr", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_selection_attr"), func_id);
        }

        // js_cheerio_selection_length(sel: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.returns.push(AbiParam::new(types::F64)); // length
            let func_id = self.module.declare_function("js_cheerio_selection_length", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_selection_length"), func_id);
        }

        // Cheerio selection navigation (sel: i64) -> i64 (selection handle)
        for name in &["js_cheerio_selection_first", "js_cheerio_selection_last", "js_cheerio_selection_parent"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.returns.push(AbiParam::new(types::I64)); // new selection handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_cheerio_selection_eq(sel: i64, index: f64) -> i64 (selection handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.params.push(AbiParam::new(types::F64)); // index
            sig.returns.push(AbiParam::new(types::I64)); // new selection handle
            let func_id = self.module.declare_function("js_cheerio_selection_eq", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_selection_eq"), func_id);
        }

        // Cheerio selection find/children (sel: i64, selector: i64) -> i64 (selection handle)
        for name in &["js_cheerio_selection_find", "js_cheerio_selection_children"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.params.push(AbiParam::new(types::I64)); // selector
            sig.returns.push(AbiParam::new(types::I64)); // new selection handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_cheerio_selection_has_class(sel: i64, class: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.params.push(AbiParam::new(types::I64)); // class name
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_cheerio_selection_has_class", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_selection_has_class"), func_id);
        }

        // js_cheerio_selection_is(sel: i64, selector: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.params.push(AbiParam::new(types::I64)); // selector
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_cheerio_selection_is", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_selection_is"), func_id);
        }

        // Cheerio array methods (sel: i64) -> i64 (array)
        for name in &["js_cheerio_selection_to_array", "js_cheerio_selection_texts"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.returns.push(AbiParam::new(types::I64)); // array
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_cheerio_selection_attrs(sel: i64, attr: i64) -> i64 (array)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // selection handle
            sig.params.push(AbiParam::new(types::I64)); // attribute name
            sig.returns.push(AbiParam::new(types::I64)); // array
            let func_id = self.module.declare_function("js_cheerio_selection_attrs", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cheerio_selection_attrs"), func_id);
        }

        // ========================================================================
        // Tier 5: lodash (utility functions)
        // ========================================================================

        // Lodash array functions (arr: i64, n: f64) -> i64 (array)
        for name in &[
            "js_lodash_drop", "js_lodash_drop_right", "js_lodash_take", "js_lodash_take_right",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array
            sig.params.push(AbiParam::new(types::F64)); // n
            sig.returns.push(AbiParam::new(types::I64)); // new array
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_lodash_chunk(arr: i64, size: f64) -> i64 (array)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array
            sig.params.push(AbiParam::new(types::F64)); // size
            sig.returns.push(AbiParam::new(types::I64)); // chunked array
            let func_id = self.module.declare_function("js_lodash_chunk", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_chunk"), func_id);
        }

        // Lodash array functions (arr: i64) -> i64 (array)
        for name in &[
            "js_lodash_compact", "js_lodash_flatten", "js_lodash_initial",
            "js_lodash_tail", "js_lodash_uniq", "js_lodash_reverse",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array
            sig.returns.push(AbiParam::new(types::I64)); // new array
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // Lodash array functions (arr: i64, arr2: i64) -> i64 (array)
        for name in &["js_lodash_concat", "js_lodash_difference"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array1
            sig.params.push(AbiParam::new(types::I64)); // array2
            sig.returns.push(AbiParam::new(types::I64)); // new array
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // Lodash array getters (arr: i64) -> f64
        for name in &["js_lodash_first", "js_lodash_last", "js_lodash_size"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // array
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // Lodash string functions (str: i64) -> i64 (string)
        for name in &[
            "js_lodash_camel_case", "js_lodash_capitalize", "js_lodash_kebab_case",
            "js_lodash_lower_case", "js_lodash_snake_case", "js_lodash_start_case",
            "js_lodash_upper_case", "js_lodash_upper_first", "js_lodash_lower_first",
            "js_lodash_trim", "js_lodash_trim_start", "js_lodash_trim_end",
            "js_lodash_escape", "js_lodash_unescape",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.returns.push(AbiParam::new(types::I64)); // new string
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_lodash_pad(str: i64, length: f64) -> i64 (string)
        for name in &["js_lodash_pad", "js_lodash_pad_start", "js_lodash_pad_end"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.params.push(AbiParam::new(types::F64)); // length
            sig.returns.push(AbiParam::new(types::I64)); // padded string
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_lodash_repeat(str: i64, n: f64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.params.push(AbiParam::new(types::F64)); // n
            sig.returns.push(AbiParam::new(types::I64)); // repeated string
            let func_id = self.module.declare_function("js_lodash_repeat", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_repeat"), func_id);
        }

        // js_lodash_truncate(str: i64, length: f64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.params.push(AbiParam::new(types::F64)); // length
            sig.returns.push(AbiParam::new(types::I64)); // truncated string
            let func_id = self.module.declare_function("js_lodash_truncate", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_truncate"), func_id);
        }

        // js_lodash_starts_with(str: i64, target: i64) -> f64 (bool)
        for name in &["js_lodash_starts_with", "js_lodash_ends_with", "js_lodash_includes"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.params.push(AbiParam::new(types::I64)); // target
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_lodash_split(str: i64, separator: i64) -> i64 (array)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.params.push(AbiParam::new(types::I64)); // separator
            sig.returns.push(AbiParam::new(types::I64)); // array
            let func_id = self.module.declare_function("js_lodash_split", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_split"), func_id);
        }

        // js_lodash_replace(str: i64, pattern: i64, replacement: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.params.push(AbiParam::new(types::I64)); // pattern
            sig.params.push(AbiParam::new(types::I64)); // replacement
            sig.returns.push(AbiParam::new(types::I64)); // new string
            let func_id = self.module.declare_function("js_lodash_replace", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_replace"), func_id);
        }

        // Lodash number functions
        // js_lodash_clamp(value: f64, lower: f64, upper: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.params.push(AbiParam::new(types::F64)); // lower
            sig.params.push(AbiParam::new(types::F64)); // upper
            sig.returns.push(AbiParam::new(types::F64)); // clamped value
            let func_id = self.module.declare_function("js_lodash_clamp", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_clamp"), func_id);
        }

        // js_lodash_in_range(value: f64, start: f64, end: f64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.params.push(AbiParam::new(types::F64)); // start
            sig.params.push(AbiParam::new(types::F64)); // end
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_lodash_in_range", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_in_range"), func_id);
        }

        // js_lodash_random(lower: f64, upper: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // lower
            sig.params.push(AbiParam::new(types::F64)); // upper
            sig.returns.push(AbiParam::new(types::F64)); // random value
            let func_id = self.module.declare_function("js_lodash_random", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_lodash_random"), func_id);
        }

        // ========================================================================
        // Tier 5: moment (date manipulation)
        // ========================================================================

        // js_moment_now() -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_moment_now", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_moment_now"), func_id);
        }

        // js_moment_from_timestamp(timestamp: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // timestamp
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_moment_from_timestamp", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_moment_from_timestamp"), func_id);
        }

        // js_moment_parse(dateStr: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // date string
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_moment_parse", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_moment_parse"), func_id);
        }

        // js_moment_format(handle: i64, pattern: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // pattern
            sig.returns.push(AbiParam::new(types::I64)); // formatted string
            let func_id = self.module.declare_function("js_moment_format", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_moment_format"), func_id);
        }

        // Moment getters (handle: i64) -> f64
        for name in &[
            "js_moment_value_of", "js_moment_unix", "js_moment_year", "js_moment_month",
            "js_moment_date", "js_moment_day", "js_moment_hour", "js_moment_minute",
            "js_moment_second", "js_moment_millisecond", "js_moment_is_valid",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::F64)); // value
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_moment_add(handle: i64, value: f64, unit: i64) -> i64 (handle)
        for name in &["js_moment_add", "js_moment_subtract"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.params.push(AbiParam::new(types::I64)); // unit
            sig.returns.push(AbiParam::new(types::I64)); // new handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_moment_start_of(handle: i64, unit: i64) -> i64 (handle)
        for name in &["js_moment_start_of", "js_moment_end_of"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // unit
            sig.returns.push(AbiParam::new(types::I64)); // new handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_moment_diff(handle: i64, other: i64, unit: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // other
            sig.params.push(AbiParam::new(types::I64)); // unit
            sig.returns.push(AbiParam::new(types::F64)); // diff
            let func_id = self.module.declare_function("js_moment_diff", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_moment_diff"), func_id);
        }

        // ========================================================================
        // Tier 5: cron/node-cron (job scheduling)
        // ========================================================================

        // js_cron_validate(expr: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // expression
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_cron_validate", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_validate"), func_id);
        }

        // js_cron_schedule(expr: i64, callback_id: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // expression
            sig.params.push(AbiParam::new(types::F64)); // callback_id
            sig.returns.push(AbiParam::new(types::I64)); // job handle
            let func_id = self.module.declare_function("js_cron_schedule", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_schedule"), func_id);
        }

        // Cron job control (handle: i64)
        for name in &["js_cron_job_start", "js_cron_job_stop"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // job handle
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_cron_job_is_running(handle: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // job handle
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_cron_job_is_running", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_job_is_running"), func_id);
        }

        // js_cron_next_date(handle: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // job handle
            sig.returns.push(AbiParam::new(types::I64)); // ISO string
            let func_id = self.module.declare_function("js_cron_next_date", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_next_date"), func_id);
        }

        // js_cron_next_dates(handle: i64, count: f64) -> i64 (array)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // job handle
            sig.params.push(AbiParam::new(types::F64)); // count
            sig.returns.push(AbiParam::new(types::I64)); // array of ISO strings
            let func_id = self.module.declare_function("js_cron_next_dates", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_next_dates"), func_id);
        }

        // js_cron_describe(expr: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // expression
            sig.returns.push(AbiParam::new(types::I64)); // description
            let func_id = self.module.declare_function("js_cron_describe", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_describe"), func_id);
        }

        // js_cron_set_interval(callback_id: f64, interval: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // callback_id
            sig.params.push(AbiParam::new(types::F64)); // interval_ms
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_cron_set_interval", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_set_interval"), func_id);
        }

        // js_cron_clear_interval(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_cron_clear_interval", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_clear_interval"), func_id);
        }

        // js_cron_set_timeout(callback_id: f64, timeout: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // callback_id
            sig.params.push(AbiParam::new(types::F64)); // timeout_ms
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_cron_set_timeout", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_set_timeout"), func_id);
        }

        // js_cron_clear_timeout(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_cron_clear_timeout", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_cron_clear_timeout"), func_id);
        }

        // ========================================================================
        // Tier 5: rate-limiter-flexible
        // ========================================================================

        // js_ratelimit_create(options: i64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // options (JSON)
            sig.returns.push(AbiParam::new(types::I64)); // handle
            let func_id = self.module.declare_function("js_ratelimit_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ratelimit_create"), func_id);
        }

        // Rate limiter async methods (handle: i64, key: i64) -> Promise (i64)
        for name in &["js_ratelimit_get", "js_ratelimit_delete"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // Rate limiter methods with points (handle: i64, key: i64, points: f64) -> Promise (i64)
        for name in &["js_ratelimit_consume", "js_ratelimit_penalty", "js_ratelimit_reward"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.params.push(AbiParam::new(types::F64)); // points
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_ratelimit_block(handle: i64, key: i64, duration: f64) -> Promise (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.params.push(AbiParam::new(types::F64)); // duration
            sig.returns.push(AbiParam::new(types::I64)); // Promise
            let func_id = self.module.declare_function("js_ratelimit_block", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ratelimit_block"), func_id);
        }

        // ========================================================================
        // Perry Native Framework: HTTP Server
        // ========================================================================

        // js_http_server_create(port: f64) -> i64 (handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // port
            sig.returns.push(AbiParam::new(types::I64)); // server handle
            let func_id = self.module.declare_function("js_http_server_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_server_create"), func_id);
        }

        // js_http_server_accept_v2(server: i64) -> i64 (request handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // server handle
            sig.returns.push(AbiParam::new(types::I64)); // request handle
            let func_id = self.module.declare_function("js_http_server_accept_v2", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_server_accept_v2"), func_id);
        }

        // js_http_server_close(server: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // server handle
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_http_server_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_server_close"), func_id);
        }

        // Request property getters (req: i64) -> i64 (string)
        for name in &[
            "js_http_request_method",
            "js_http_request_path",
            "js_http_request_query",
            "js_http_request_body",
            "js_http_request_content_type",
            "js_http_request_query_all",
            "js_http_request_headers_all",
        ] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.returns.push(AbiParam::new(types::I64)); // string
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_http_request_header(req: i64, name: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::I64)); // header name
            sig.returns.push(AbiParam::new(types::I64)); // header value
            let func_id = self.module.declare_function("js_http_request_header", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_request_header"), func_id);
        }

        // js_http_request_query_param(req: i64, name: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::I64)); // param name
            sig.returns.push(AbiParam::new(types::I64)); // param value
            let func_id = self.module.declare_function("js_http_request_query_param", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_request_query_param"), func_id);
        }

        // js_http_request_id(req: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.returns.push(AbiParam::new(types::F64)); // request id
            let func_id = self.module.declare_function("js_http_request_id", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_request_id"), func_id);
        }

        // js_http_request_body_length(req: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.returns.push(AbiParam::new(types::F64)); // body length
            let func_id = self.module.declare_function("js_http_request_body_length", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_request_body_length"), func_id);
        }

        // Boolean request checks (req: i64, arg: i64) -> f64 (bool)
        for name in &["js_http_request_has_header", "js_http_request_is_method"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::I64)); // name/method
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // Response functions: js_http_respond_text/json/html(req: i64, status: f64, body: i64) -> f64
        for name in &["js_http_respond_text", "js_http_respond_json", "js_http_respond_html"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::F64)); // status
            sig.params.push(AbiParam::new(types::I64)); // body
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_http_respond_with_headers(req: i64, status: f64, body: i64, headers: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::F64)); // status
            sig.params.push(AbiParam::new(types::I64)); // body
            sig.params.push(AbiParam::new(types::I64)); // headers json
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_http_respond_with_headers", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_respond_with_headers"), func_id);
        }

        // js_http_respond_redirect(req: i64, url: i64, permanent: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::I64)); // url
            sig.params.push(AbiParam::new(types::F64)); // permanent (bool)
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_http_respond_redirect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_respond_redirect"), func_id);
        }

        // js_http_respond_not_found(req: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_http_respond_not_found", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_respond_not_found"), func_id);
        }

        // js_http_respond_error(req: i64, status: f64, message: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // request handle
            sig.params.push(AbiParam::new(types::F64)); // status
            sig.params.push(AbiParam::new(types::I64)); // message
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_http_respond_error", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_respond_error"), func_id);
        }

        // js_http_respond_status_text(status: f64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // status
            sig.returns.push(AbiParam::new(types::I64)); // status text
            let func_id = self.module.declare_function("js_http_respond_status_text", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_http_respond_status_text"), func_id);
        }

        // ========================================================================
        // Perry Native Framework: JSON
        // ========================================================================

        // js_json_parse(text: i64) -> i64 (JSValue bits)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // text
            sig.returns.push(AbiParam::new(types::I64)); // JSValue bits (returned in x0)
            let func_id = self.module.declare_function("js_json_parse", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_parse"), func_id);
        }

        // js_json_stringify(value: f64, type_hint: u32) -> i64 (generic stringify for any JSValue)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // value (JSValue)
            sig.params.push(AbiParam::new(types::I32)); // type_hint (0=unknown, 1=object, 2=array)
            sig.returns.push(AbiParam::new(types::I64)); // json string
            let func_id = self.module.declare_function("js_json_stringify", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_stringify"), func_id);
        }

        // JSON stringify functions (various types) -> i64 (string)
        for name in &["js_json_stringify_null"] {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // string
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }

        // js_json_stringify_string(str: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string
            sig.returns.push(AbiParam::new(types::I64)); // json string
            let func_id = self.module.declare_function("js_json_stringify_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_stringify_string"), func_id);
        }

        // js_json_stringify_number(num: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // number
            sig.returns.push(AbiParam::new(types::I64)); // json string
            let func_id = self.module.declare_function("js_json_stringify_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_stringify_number"), func_id);
        }

        // js_json_stringify_bool(bool: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // bool as f64
            sig.returns.push(AbiParam::new(types::I64)); // json string
            let func_id = self.module.declare_function("js_json_stringify_bool", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_stringify_bool"), func_id);
        }

        // js_json_is_valid(text: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // text
            sig.returns.push(AbiParam::new(types::F64)); // bool
            let func_id = self.module.declare_function("js_json_is_valid", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_is_valid"), func_id);
        }

        // js_json_get_string(json: i64, key: i64) -> i64 (string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // json string
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.returns.push(AbiParam::new(types::I64)); // value string
            let func_id = self.module.declare_function("js_json_get_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_get_string"), func_id);
        }

        // js_json_get_number(json: i64, key: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // json string
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // value number
            let func_id = self.module.declare_function("js_json_get_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_get_number"), func_id);
        }

        // js_json_get_bool(json: i64, key: i64) -> f64 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // json string
            sig.params.push(AbiParam::new(types::I64)); // key
            sig.returns.push(AbiParam::new(types::F64)); // value bool
            let func_id = self.module.declare_function("js_json_get_bool", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_json_get_bool"), func_id);
        }

        // ========================================================================
        // Perry Native Framework: Math
        // ========================================================================

        // js_math_pow(base: f64, exp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // base
            sig.params.push(AbiParam::new(types::F64)); // exponent
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function("js_math_pow", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_pow"), func_id);
        }

        // js_math_log(x: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_math_log", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_log"), func_id);
        }

        // js_math_log2(x: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_math_log2", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_log2"), func_id);
        }

        // js_math_log10(x: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_math_log10", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_log10"), func_id);
        }

        // js_math_random() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64)); // random number 0..1
            let func_id = self.module.declare_function("js_math_random", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_random"), func_id);
        }

        // js_math_min_array(arr_ptr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_math_min_array", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_min_array"), func_id);
        }

        // js_math_max_array(arr_ptr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_math_max_array", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_math_max_array"), func_id);
        }

        // ========================================================================
        // Perry Native Framework: Date
        // ========================================================================

        // js_date_now() -> f64 (timestamp in milliseconds)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_now", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_now"), func_id);
        }

        // js_date_new() -> f64 (timestamp in milliseconds)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_new"), func_id);
        }

        // js_date_new_from_timestamp(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_new_from_timestamp", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_new_from_timestamp"), func_id);
        }

        // js_date_new_from_value(value: f64) -> f64  (handles both string and number args)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_new_from_value", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_new_from_value"), func_id);
        }

        // js_date_get_time(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_time", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_time"), func_id);
        }

        // js_date_to_iso_string(timestamp: f64) -> *mut StringHeader (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_date_to_iso_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_to_iso_string"), func_id);
        }

        // js_date_get_full_year(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_full_year", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_full_year"), func_id);
        }

        // js_date_get_month(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_month", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_month"), func_id);
        }

        // js_date_get_date(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_date", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_date"), func_id);
        }

        // js_date_get_hours(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_hours", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_hours"), func_id);
        }

        // js_date_get_minutes(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_minutes", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_minutes"), func_id);
        }

        // js_date_get_seconds(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_seconds", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_seconds"), func_id);
        }

        // js_date_get_milliseconds(timestamp: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_date_get_milliseconds", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_date_get_milliseconds"), func_id);
        }

        // Error runtime functions
        // js_error_new() -> *mut ErrorHeader
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // error pointer
            let func_id = self.module.declare_function("js_error_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_error_new"), func_id);
        }

        // js_error_new_with_message(message: *mut StringHeader) -> *mut ErrorHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // message pointer
            sig.returns.push(AbiParam::new(types::I64)); // error pointer
            let func_id = self.module.declare_function("js_error_new_with_message", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_error_new_with_message"), func_id);
        }

        // js_error_get_message(error: *mut ErrorHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // error pointer
            sig.returns.push(AbiParam::new(types::I64)); // message pointer
            let func_id = self.module.declare_function("js_error_get_message", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_error_get_message"), func_id);
        }

        // Delete operator runtime functions
        // js_object_delete_field(obj: *mut ObjectHeader, field_name: *const StringHeader) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::I64)); // field name pointer
            sig.returns.push(AbiParam::new(types::I32)); // bool (1 = success, 0 = failure)
            let func_id = self.module.declare_function("js_object_delete_field", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_delete_field"), func_id);
        }

        // js_object_delete_dynamic(obj: *mut ObjectHeader, key: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // object pointer
            sig.params.push(AbiParam::new(types::F64)); // key (could be string pointer or number index)
            sig.returns.push(AbiParam::new(types::I32)); // bool (1 = success, 0 = failure)
            let func_id = self.module.declare_function("js_object_delete_dynamic", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_object_delete_dynamic"), func_id);
        }

        // URL runtime functions
        // js_url_new(url: *mut StringHeader) -> *mut UrlHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string pointer
            sig.returns.push(AbiParam::new(types::I64)); // url pointer
            let func_id = self.module.declare_function("js_url_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_new"), func_id);
        }

        // js_url_file_url_to_path(url_f64: f64) -> f64 (NaN-boxed string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // url (NaN-boxed string)
            sig.returns.push(AbiParam::new(types::F64)); // result (NaN-boxed string)
            let func_id = self.module.declare_function("js_url_file_url_to_path", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_file_url_to_path"), func_id);
        }

        // js_url_new_with_base(url: *mut StringHeader, base: *mut StringHeader) -> *mut UrlHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url string pointer
            sig.params.push(AbiParam::new(types::I64)); // base string pointer
            sig.returns.push(AbiParam::new(types::I64)); // url pointer
            let func_id = self.module.declare_function("js_url_new_with_base", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_new_with_base"), func_id);
        }

        // js_url_get_href(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_href", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_href"), func_id);
        }

        // js_url_get_pathname(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_pathname", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_pathname"), func_id);
        }

        // js_url_get_protocol(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_protocol", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_protocol"), func_id);
        }

        // js_url_get_host(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_host", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_host"), func_id);
        }

        // js_url_get_hostname(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_hostname", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_hostname"), func_id);
        }

        // js_url_get_port(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_port", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_port"), func_id);
        }

        // js_url_get_search(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_search", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_search"), func_id);
        }

        // js_url_get_hash(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_hash", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_hash"), func_id);
        }

        // js_url_get_origin(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_get_origin", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_origin"), func_id);
        }

        // js_url_get_search_params(url: *mut UrlHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // url pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer (for now, later would return URLSearchParams)
            let func_id = self.module.declare_function("js_url_get_search_params", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_get_search_params"), func_id);
        }

        // URLSearchParams runtime functions
        // js_url_search_params_new(init: *mut StringHeader) -> *mut ObjectHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // init string pointer
            sig.returns.push(AbiParam::new(types::I64)); // URLSearchParams object pointer
            let func_id = self.module.declare_function("js_url_search_params_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_new"), func_id);
        }

        // js_url_search_params_new_empty() -> *mut ObjectHeader
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // URLSearchParams object pointer
            let func_id = self.module.declare_function("js_url_search_params_new_empty", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_new_empty"), func_id);
        }

        // js_url_search_params_get(params: *mut ObjectHeader, name: *mut StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            sig.returns.push(AbiParam::new(types::I64)); // value (string pointer or null)
            let func_id = self.module.declare_function("js_url_search_params_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_get"), func_id);
        }

        // js_url_search_params_has(params: *mut ObjectHeader, name: *mut StringHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            sig.returns.push(AbiParam::new(types::F64)); // boolean
            let func_id = self.module.declare_function("js_url_search_params_has", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_has"), func_id);
        }

        // js_url_search_params_set(params: *mut ObjectHeader, name: *mut StringHeader, value: *mut StringHeader) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            sig.params.push(AbiParam::new(types::I64)); // value string pointer
            let func_id = self.module.declare_function("js_url_search_params_set", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_set"), func_id);
        }

        // js_url_search_params_append(params: *mut ObjectHeader, name: *mut StringHeader, value: *mut StringHeader) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            sig.params.push(AbiParam::new(types::I64)); // value string pointer
            let func_id = self.module.declare_function("js_url_search_params_append", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_append"), func_id);
        }

        // js_url_search_params_delete(params: *mut ObjectHeader, name: *mut StringHeader) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            let func_id = self.module.declare_function("js_url_search_params_delete", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_delete"), func_id);
        }

        // js_url_search_params_to_string(params: *mut ObjectHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_url_search_params_to_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_to_string"), func_id);
        }

        // js_url_search_params_get_all(params: *mut ObjectHeader, name: *mut StringHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // params pointer
            sig.params.push(AbiParam::new(types::I64)); // name string pointer
            sig.returns.push(AbiParam::new(types::F64)); // array
            let func_id = self.module.declare_function("js_url_search_params_get_all", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_url_search_params_get_all"), func_id);
        }

        // AbortController runtime functions
        // js_abort_controller_new() -> *mut ObjectHeader
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // AbortController object pointer
            let func_id = self.module.declare_function("js_abort_controller_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_abort_controller_new"), func_id);
        }

        // js_abort_controller_signal(controller: *mut ObjectHeader) -> *mut ObjectHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // controller pointer
            sig.returns.push(AbiParam::new(types::I64)); // signal object pointer
            let func_id = self.module.declare_function("js_abort_controller_signal", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_abort_controller_signal"), func_id);
        }

        // js_abort_controller_abort(controller: *mut ObjectHeader)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // controller pointer
            let func_id = self.module.declare_function("js_abort_controller_abort", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_abort_controller_abort"), func_id);
        }

        // RegExp runtime functions
        // js_regexp_new(pattern: *const StringHeader, flags: *const StringHeader) -> *mut RegExpHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pattern pointer
            sig.params.push(AbiParam::new(types::I64)); // flags pointer
            sig.returns.push(AbiParam::new(types::I64)); // regexp pointer
            let func_id = self.module.declare_function("js_regexp_new", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_regexp_new"), func_id);
        }

        // js_regexp_test(re: *const RegExpHeader, s: *const StringHeader) -> bool
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // regex pointer
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I32)); // bool (i32)
            let func_id = self.module.declare_function("js_regexp_test", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_regexp_test"), func_id);
        }

        // js_string_match(s: *const StringHeader, re: *const RegExpHeader) -> *mut ArrayHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I64)); // regex pointer
            sig.returns.push(AbiParam::new(types::I64)); // array pointer (or null)
            let func_id = self.module.declare_function("js_string_match", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_match"), func_id);
        }

        // js_string_replace_regex(s: *const StringHeader, re: *const RegExpHeader, replacement: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I64)); // regex pointer
            sig.params.push(AbiParam::new(types::I64)); // replacement string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function("js_string_replace_regex", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_replace_regex"), func_id);
        }

        // js_string_replace_string(s: *const StringHeader, pattern: *const StringHeader, replacement: *const StringHeader) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::I64)); // pattern string pointer
            sig.params.push(AbiParam::new(types::I64)); // replacement string pointer
            sig.returns.push(AbiParam::new(types::I64)); // result string pointer
            let func_id = self.module.declare_function("js_string_replace_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_replace_string"), func_id);
        }

        // js_value_typeof(value: f64) -> *mut StringHeader (returns the typeof string)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_value_typeof", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_value_typeof"), func_id);
        }

        // js_string_equals(a: *const StringHeader, b: *const StringHeader) -> bool
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a string pointer
            sig.params.push(AbiParam::new(types::I64)); // b string pointer
            sig.returns.push(AbiParam::new(types::I32)); // bool (i32)
            let func_id = self.module.declare_function("js_string_equals", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_equals"), func_id);
        }

        // js_string_compare(a: *const StringHeader, b: *const StringHeader) -> i32
        // Lexicographic comparison: -1 if a < b, 0 if a == b, 1 if a > b
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // a string pointer
            sig.params.push(AbiParam::new(types::I64)); // b string pointer
            sig.returns.push(AbiParam::new(types::I32)); // -1, 0, or 1
            let func_id = self.module.declare_function("js_string_compare", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_compare"), func_id);
        }

        // js_dynamic_string_equals(a: f64, b: f64) -> i32
        // Compares strings that may be NaN-boxed (from PropertyGet) or raw pointers (from literals)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // a value (may be NaN-boxed or raw)
            sig.params.push(AbiParam::new(types::F64)); // b value (may be NaN-boxed or raw)
            sig.returns.push(AbiParam::new(types::I32)); // bool (i32)
            let func_id = self.module.declare_function("js_dynamic_string_equals", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_string_equals"), func_id);
        }

        // js_jsvalue_equals(a: f64, b: f64) -> i32
        // Generic JS === comparison: handles BigInt value equality, string content equality, number bit equality.
        // Used for === when operand types are unknown (e.g., Any-typed parameters).
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // a (NaN-boxed JSValue)
            sig.params.push(AbiParam::new(types::F64)); // b (NaN-boxed JSValue)
            sig.returns.push(AbiParam::new(types::I32)); // 1=equal, 0=not equal
            let func_id = self.module.declare_function("js_jsvalue_equals", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_jsvalue_equals"), func_id);
        }

        // js_jsvalue_compare(a: f64, b: f64) -> i32
        // Generic JS relational comparison: handles BigInt, INT32, Number.
        // Returns -1 (a < b), 0 (a == b), 1 (a > b).
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_jsvalue_compare", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_jsvalue_compare"), func_id);
        }

        // Dynamic arithmetic dispatch: BigInt or float based on runtime NaN-box tag.
        // Used when operands are union-typed (Type::Any) and may hold BigInt values.
        for &name in &["js_dynamic_mul", "js_dynamic_add", "js_dynamic_sub", "js_dynamic_div", "js_dynamic_mod",
                       "js_dynamic_shr", "js_dynamic_shl", "js_dynamic_bitand", "js_dynamic_bitor", "js_dynamic_bitxor"] {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // a (NaN-boxed JSValue)
            sig.params.push(AbiParam::new(types::F64)); // b (NaN-boxed JSValue)
            sig.returns.push(AbiParam::new(types::F64)); // result (NaN-boxed JSValue)
            let func_id = self.module.declare_function(name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed(name), func_id);
        }
        // js_dynamic_neg(a: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // a (NaN-boxed JSValue)
            sig.returns.push(AbiParam::new(types::F64)); // result (NaN-boxed JSValue)
            let func_id = self.module.declare_function("js_dynamic_neg", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_dynamic_neg"), func_id);
        }

        // js_parse_int(str: i64, radix: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::F64)); // radix
            sig.returns.push(AbiParam::new(types::F64)); // result number
            let func_id = self.module.declare_function("js_parse_int", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_parse_int"), func_id);
        }

        // js_parse_float(str: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::F64)); // result number
            let func_id = self.module.declare_function("js_parse_float", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_parse_float"), func_id);
        }

        // js_number_coerce(value: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::F64)); // result number
            let func_id = self.module.declare_function("js_number_coerce", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_number_coerce"), func_id);
        }

        // js_string_coerce(value: f64) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_string_coerce", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_string_coerce"), func_id);
        }

        // js_is_nan(value: f64) -> f64 (boolean as 1.0/0.0)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::F64)); // boolean result
            let func_id = self.module.declare_function("js_is_nan", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_is_nan"), func_id);
        }

        // js_is_finite(value: f64) -> f64 (boolean as 1.0/0.0)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::F64)); // boolean result
            let func_id = self.module.declare_function("js_is_finite", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_is_finite"), func_id);
        }

        // js_ethers_format_units(bigint: i64, decimals: f64) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.params.push(AbiParam::new(types::F64)); // decimals
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_ethers_format_units", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ethers_format_units"), func_id);
        }

        // js_ethers_parse_units(str: i64, decimals: f64) -> i64 (bigint pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.params.push(AbiParam::new(types::F64)); // decimals
            sig.returns.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function("js_ethers_parse_units", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ethers_parse_units"), func_id);
        }

        // js_keccak256_native_bytes(buf: i64) -> i64 (buffer pointer: raw 32-byte hash)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer pointer
            sig.returns.push(AbiParam::new(types::I64)); // buffer pointer
            let func_id = self.module.declare_function("js_keccak256_native_bytes", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_keccak256_native_bytes"), func_id);
        }

        // js_keccak256_native(buf: i64) -> i64 (string pointer: "0x" + hex hash)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // buffer pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_keccak256_native", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_keccak256_native"), func_id);
        }

        // js_ethers_get_address(str: i64) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_ethers_get_address", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ethers_get_address"), func_id);
        }

        // js_ethers_parse_ether(str: i64) -> i64 (bigint pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // string pointer
            sig.returns.push(AbiParam::new(types::I64)); // bigint pointer
            let func_id = self.module.declare_function("js_ethers_parse_ether", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ethers_parse_ether"), func_id);
        }

        // js_ethers_format_ether(bigint: i64) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bigint pointer
            sig.returns.push(AbiParam::new(types::I64)); // string pointer
            let func_id = self.module.declare_function("js_ethers_format_ether", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_ethers_format_ether"), func_id);
        }

        // ============================================
        // Fastify HTTP Framework FFI functions
        // ============================================

        // js_fastify_create() -> Handle (i64)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_create"), func_id);
        }

        // js_fastify_create_with_opts(opts: f64) -> Handle (i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // opts object
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_create_with_opts", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_create_with_opts"), func_id);
        }

        // js_fastify_get(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::I64)); // path string
            sig.params.push(AbiParam::new(types::I64)); // handler closure
            sig.returns.push(AbiParam::new(types::I32)); // bool
            let func_id = self.module.declare_function("js_fastify_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_get"), func_id);
        }

        // js_fastify_post(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_post", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_post"), func_id);
        }

        // js_fastify_put(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_put", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_put"), func_id);
        }

        // js_fastify_delete(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_delete", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_delete"), func_id);
        }

        // js_fastify_patch(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_patch", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_patch"), func_id);
        }

        // js_fastify_head(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_head", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_head"), func_id);
        }

        // js_fastify_options(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_options", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_options"), func_id);
        }

        // js_fastify_all(app: Handle, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_all", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_all"), func_id);
        }

        // js_fastify_route(app: Handle, method: i64, path: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_route", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_route"), func_id);
        }

        // js_fastify_add_hook(app: Handle, hook_name: i64, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_add_hook", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_add_hook"), func_id);
        }

        // js_fastify_set_error_handler(app: Handle, handler: i64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_set_error_handler", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_set_error_handler"), func_id);
        }

        // js_fastify_register(app: Handle, plugin: i64, opts: f64) -> bool (i32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::I64)); // plugin closure
            sig.params.push(AbiParam::new(types::F64)); // opts object
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_register", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_register"), func_id);
        }

        // js_fastify_listen(app: Handle, opts: f64, callback: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::F64)); // opts object (contains port)
            sig.params.push(AbiParam::new(types::I64)); // callback closure
            let func_id = self.module.declare_function("js_fastify_listen", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_listen"), func_id);
        }

        // ---- Context/Request/Reply methods ----

        // js_fastify_req_method(ctx: Handle) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_method", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_method"), func_id);
        }

        // js_fastify_req_url(ctx: Handle) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_url", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_url"), func_id);
        }

        // js_fastify_req_params(ctx: Handle) -> i64 (string pointer - JSON)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_params", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_params"), func_id);
        }

        // js_fastify_req_param(ctx: Handle, name: i64) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_param", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_param"), func_id);
        }

        // js_fastify_req_query(ctx: Handle) -> i64 (string pointer - JSON)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_query", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_query"), func_id);
        }

        // js_fastify_req_query_object(ctx: Handle) -> f64 (NaN-boxed JS object)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fastify_req_query_object", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_query_object"), func_id);
        }

        // js_fastify_req_body(ctx: Handle) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_body", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_body"), func_id);
        }

        // js_fastify_req_json(ctx: Handle) -> f64 (NaN-boxed object)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fastify_req_json", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_json"), func_id);
        }

        // js_fastify_req_headers(ctx: Handle) -> i64 (NaN-boxed JS object with all headers)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_headers", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_headers"), func_id);
        }

        // js_fastify_req_header(ctx: Handle, name: i64) -> i64 (string pointer)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_req_header", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_header"), func_id);
        }

        // js_fastify_req_get_user_data(ctx: Handle) -> f64 (NaN-boxed JSValue)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ctx handle
            sig.returns.push(AbiParam::new(types::F64)); // JSValue
            let func_id = self.module.declare_function("js_fastify_req_get_user_data", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_get_user_data"), func_id);
        }

        // js_fastify_req_set_user_data(ctx: Handle, data: f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // ctx handle
            sig.params.push(AbiParam::new(types::F64)); // JSValue data
            let func_id = self.module.declare_function("js_fastify_req_set_user_data", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_req_set_user_data"), func_id);
        }

        // js_fastify_reply_status(ctx: Handle, code: f64) -> i64 (handle - chainable)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_reply_status", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_reply_status"), func_id);
        }

        // js_fastify_reply_header(ctx: Handle, name: i64, value: i64) -> i64 (handle - chainable)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("js_fastify_reply_header", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_reply_header"), func_id);
        }

        // js_fastify_reply_send(ctx: Handle, data: f64) -> i32 (bool)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("js_fastify_reply_send", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_reply_send"), func_id);
        }

        // js_fastify_ctx_json(ctx: Handle, data: f64, status: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fastify_ctx_json", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_ctx_json"), func_id);
        }

        // js_fastify_ctx_text(ctx: Handle, text: i64, status: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fastify_ctx_text", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_ctx_text"), func_id);
        }

        // js_fastify_ctx_html(ctx: Handle, html: i64, status: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fastify_ctx_html", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_ctx_html"), func_id);
        }

        // js_fastify_ctx_redirect(ctx: Handle, url: i64, status: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("js_fastify_ctx_redirect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_fastify_ctx_redirect"), func_id);
        }

        // ============================================
        // Perry UI FFI functions
        // ============================================

        // perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            sig.returns.push(AbiParam::new(types::I64)); // app handle
            let func_id = self.module.declare_function("perry_ui_app_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_create"), func_id);
        }

        // perry_ui_app_set_body(app: i64, root: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::I64)); // root widget handle
            let func_id = self.module.declare_function("perry_ui_app_set_body", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_body"), func_id);
        }

        // perry_ui_app_run(app: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            let func_id = self.module.declare_function("perry_ui_app_run", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_run"), func_id);
        }

        // perry_ui_app_set_icon(path_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string ptr
            let func_id = self.module.declare_function("perry_ui_app_set_icon", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_icon"), func_id);
        }

        // perry_ui_app_set_frameless(app_handle: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed boolean
            let func_id = self.module.declare_function("perry_ui_app_set_frameless", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_frameless"), func_id);
        }

        // perry_ui_app_set_level(app_handle: i64, value_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("perry_ui_app_set_level", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_level"), func_id);
        }

        // perry_ui_app_set_transparent(app_handle: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed boolean
            let func_id = self.module.declare_function("perry_ui_app_set_transparent", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_transparent"), func_id);
        }

        // perry_ui_app_set_vibrancy(app_handle: i64, value_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("perry_ui_app_set_vibrancy", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_vibrancy"), func_id);
        }

        // perry_ui_app_set_activation_policy(app_handle: i64, value_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("perry_ui_app_set_activation_policy", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_activation_policy"), func_id);
        }

        // perry_ui_poll_open_file() -> i64 (returns NaN-boxed string)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("perry_ui_poll_open_file", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_poll_open_file"), func_id);
        }

        // perry_ui_text_create(text_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // text string ptr
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_text_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_create"), func_id);
        }

        // perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // label string ptr
            sig.params.push(AbiParam::new(types::F64)); // on_press closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_button_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_create"), func_id);
        }

        // perry_ui_vstack_create(spacing: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // spacing
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_vstack_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_vstack_create"), func_id);
        }

        // perry_ui_hstack_create(spacing: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // spacing
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_hstack_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_hstack_create"), func_id);
        }

        // perry_ui_splitview_create(left_width: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // left_width
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_splitview_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_splitview_create"), func_id);
        }

        // perry_ui_splitview_add_child(parent: i64, child: i64, index: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            sig.params.push(AbiParam::new(types::F64)); // child index
            let func_id = self.module.declare_function("perry_ui_splitview_add_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_splitview_add_child"), func_id);
        }

        // perry_ui_vbox_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_vbox_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_vbox_create"), func_id);
        }

        // perry_ui_vbox_add_child(parent: i64, child: i64, slot: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_vbox_add_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_vbox_add_child"), func_id);
        }

        // perry_ui_vbox_finalize(parent: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_vbox_finalize", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_vbox_finalize"), func_id);
        }

        // perry_ui_frame_split_create(left_width: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // left_width
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_frame_split_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_frame_split_create"), func_id);
        }

        // perry_ui_frame_split_add_child(parent: i64, child: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_frame_split_add_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_frame_split_add_child"), func_id);
        }

        // perry_ui_widget_add_child(parent: i64, child: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_widget_add_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_add_child"), func_id);
        }

        // perry_ui_widget_add_overlay(parent: i64, child: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_widget_add_overlay", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_add_overlay"), func_id);
        }

        // perry_ui_widget_set_overlay_frame(handle: i64, x: f64, y: f64, w: f64, h: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // x
            sig.params.push(AbiParam::new(types::F64)); // y
            sig.params.push(AbiParam::new(types::F64)); // w
            sig.params.push(AbiParam::new(types::F64)); // h
            let func_id = self.module.declare_function("perry_ui_widget_set_overlay_frame", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_overlay_frame"), func_id);
        }

        // perry_ui_widget_remove_child(parent: i64, child: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_widget_remove_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_remove_child"), func_id);
        }

        // perry_ui_widget_reorder_child(parent: i64, from_index: f64, to_index: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::F64)); // from_index
            sig.params.push(AbiParam::new(types::F64)); // to_index
            let func_id = self.module.declare_function("perry_ui_widget_reorder_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_reorder_child"), func_id);
        }

        // perry_ui_state_create(initial: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // initial value
            sig.returns.push(AbiParam::new(types::I64)); // state handle
            let func_id = self.module.declare_function("perry_ui_state_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_create"), func_id);
        }

        // perry_ui_state_get(state: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.returns.push(AbiParam::new(types::F64)); // current value
            let func_id = self.module.declare_function("perry_ui_state_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_get"), func_id);
        }

        // perry_ui_state_set(state: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.params.push(AbiParam::new(types::F64)); // new value
            let func_id = self.module.declare_function("perry_ui_state_set", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_set"), func_id);
        }

        // perry_ui_state_bind_text_numeric(state_handle: i64, text_handle: i64, prefix_ptr: i64, suffix_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.params.push(AbiParam::new(types::I64)); // text widget handle
            sig.params.push(AbiParam::new(types::I64)); // prefix string ptr
            sig.params.push(AbiParam::new(types::I64)); // suffix string ptr
            let func_id = self.module.declare_function("perry_ui_state_bind_text_numeric", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_bind_text_numeric"), func_id);
        }

        // perry_ui_spacer_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_spacer_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_spacer_create"), func_id);
        }

        // perry_ui_divider_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_divider_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_divider_create"), func_id);
        }

        // perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // placeholder string ptr
            sig.params.push(AbiParam::new(types::F64)); // on_change closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_textfield_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_create"), func_id);
        }

        // perry_ui_textarea_create(placeholder_ptr: i64, on_change: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // placeholder string ptr
            sig.params.push(AbiParam::new(types::F64)); // on_change closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_textarea_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textarea_create"), func_id);
        }

        // perry_ui_textarea_set_string(handle: i64, text_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // text string ptr
            let func_id = self.module.declare_function("perry_ui_textarea_set_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textarea_set_string"), func_id);
        }

        // perry_ui_textarea_get_string(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("perry_ui_textarea_get_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textarea_get_string"), func_id);
        }

        // perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // label string ptr
            sig.params.push(AbiParam::new(types::F64)); // on_change closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_toggle_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_toggle_create"), func_id);
        }

        // perry_ui_slider_create(min: f64, max: f64, initial: f64, on_change: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // min
            sig.params.push(AbiParam::new(types::F64)); // max
            sig.params.push(AbiParam::new(types::F64)); // initial
            sig.params.push(AbiParam::new(types::F64)); // on_change closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_slider_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_slider_create"), func_id);
        }

        // perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.params.push(AbiParam::new(types::I64)); // slider handle
            let func_id = self.module.declare_function("perry_ui_state_bind_slider", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_bind_slider"), func_id);
        }

        // perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.params.push(AbiParam::new(types::I64)); // toggle handle
            let func_id = self.module.declare_function("perry_ui_state_bind_toggle", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_bind_toggle"), func_id);
        }

        // perry_ui_state_bind_text_template(text_handle: i64, num_parts: i32, types_ptr: i64, values_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // text handle
            sig.params.push(AbiParam::new(types::I32)); // num_parts
            sig.params.push(AbiParam::new(types::I64)); // types array ptr
            sig.params.push(AbiParam::new(types::I64)); // values array ptr
            let func_id = self.module.declare_function("perry_ui_state_bind_text_template", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_bind_text_template"), func_id);
        }

        // perry_ui_state_bind_visibility(state_handle: i64, show_handle: i64, hide_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.params.push(AbiParam::new(types::I64)); // show widget handle
            sig.params.push(AbiParam::new(types::I64)); // hide widget handle (0 = none)
            let func_id = self.module.declare_function("perry_ui_state_bind_visibility", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_bind_visibility"), func_id);
        }

        // perry_ui_set_widget_hidden(handle: i64, hidden: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // hidden (0 or 1)
            let func_id = self.module.declare_function("perry_ui_set_widget_hidden", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_set_widget_hidden"), func_id);
        }

        // perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // stack handle
            sig.params.push(AbiParam::new(types::I64)); // flag (0 or 1)
            let func_id = self.module.declare_function("perry_ui_stack_set_detaches_hidden", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_stack_set_detaches_hidden"), func_id);
        }

        // perry_ui_stack_set_distribution(handle: i64, distribution: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // stack handle
            sig.params.push(AbiParam::new(types::F64)); // distribution mode
            let func_id = self.module.declare_function("perry_ui_stack_set_distribution", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_stack_set_distribution"), func_id);
        }

        // perry_ui_stack_set_alignment(handle: i64, alignment: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // stack handle
            sig.params.push(AbiParam::new(types::F64)); // alignment
            let func_id = self.module.declare_function("perry_ui_stack_set_alignment", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_stack_set_alignment"), func_id);
        }

        // perry_ui_for_each_init(container_handle: i64, state_handle: i64, render_closure: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // container handle
            sig.params.push(AbiParam::new(types::I64)); // state handle
            sig.params.push(AbiParam::new(types::F64)); // render closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_for_each_init", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_for_each_init"), func_id);
        }

        // perry_ui_widget_clear_children(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_widget_clear_children", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_clear_children"), func_id);
        }

        // ============================================
        // Perry UI Phase A: Enhanced Widget Functions
        // ============================================

        // perry_ui_text_set_string(handle: i64, text_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // text string ptr
            let func_id = self.module.declare_function("perry_ui_text_set_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_string"), func_id);
        }

        // perry_ui_vstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // spacing
            sig.params.push(AbiParam::new(types::F64)); // top
            sig.params.push(AbiParam::new(types::F64)); // left
            sig.params.push(AbiParam::new(types::F64)); // bottom
            sig.params.push(AbiParam::new(types::F64)); // right
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_vstack_create_with_insets", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_vstack_create_with_insets"), func_id);
        }

        // perry_ui_hstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // spacing
            sig.params.push(AbiParam::new(types::F64)); // top
            sig.params.push(AbiParam::new(types::F64)); // left
            sig.params.push(AbiParam::new(types::F64)); // bottom
            sig.params.push(AbiParam::new(types::F64)); // right
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_hstack_create_with_insets", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_hstack_create_with_insets"), func_id);
        }

        // perry_ui_scrollview_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_scrollview_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_create"), func_id);
        }

        // perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // scroll handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_scrollview_set_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_set_child"), func_id);
        }

        // perry_ui_clipboard_read() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed string
            let func_id = self.module.declare_function("perry_ui_clipboard_read", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_clipboard_read"), func_id);
        }

        // perry_ui_clipboard_write(text_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // text string ptr
            let func_id = self.module.declare_function("perry_ui_clipboard_write", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_clipboard_write"), func_id);
        }

        // perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::F64)); // modifiers bitfield
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_add_keyboard_shortcut", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_add_keyboard_shortcut"), func_id);
        }

        // perry_ui_register_global_hotkey(key_ptr: i64, modifiers: f64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // key string ptr
            sig.params.push(AbiParam::new(types::F64)); // modifiers bitfield
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_register_global_hotkey", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_register_global_hotkey"), func_id);
        }

        // perry_system_get_app_icon(path_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path string ptr
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_system_get_app_icon", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_get_app_icon"), func_id);
        }

        // perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // r
            sig.params.push(AbiParam::new(types::F64)); // g
            sig.params.push(AbiParam::new(types::F64)); // b
            sig.params.push(AbiParam::new(types::F64)); // a
            let func_id = self.module.declare_function("perry_ui_text_set_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_color"), func_id);
        }

        // perry_ui_text_set_font_size(handle: i64, size: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // size
            let func_id = self.module.declare_function("perry_ui_text_set_font_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_font_size"), func_id);
        }

        // perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // size
            sig.params.push(AbiParam::new(types::F64)); // weight
            let func_id = self.module.declare_function("perry_ui_text_set_font_weight", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_font_weight"), func_id);
        }

        // perry_ui_text_set_wraps(handle: i64, max_width: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // max_width
            let func_id = self.module.declare_function("perry_ui_text_set_wraps", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_wraps"), func_id);
        }

        // perry_ui_text_set_selectable(handle: i64, selectable: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // selectable (0 or 1)
            let func_id = self.module.declare_function("perry_ui_text_set_selectable", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_selectable"), func_id);
        }

        // perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_button_set_text_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_text_color"), func_id);
        }

        // perry_ui_button_set_bordered(handle: i64, bordered: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // bordered (0 or 1)
            let func_id = self.module.declare_function("perry_ui_button_set_bordered", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_bordered"), func_id);
        }

        // perry_ui_widget_set_width(handle: i64, width: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_set_width", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_width"), func_id);
        }

        // perry_ui_widget_set_height(handle: i64, height: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_set_height", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_height"), func_id);
        }

        // perry_ui_widget_set_hugging(handle: i64, priority: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_set_hugging", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_hugging"), func_id);
        }

        // perry_ui_widget_match_parent_height(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_widget_match_parent_height", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_match_parent_height"), func_id);
        }

        // perry_ui_widget_match_parent_width(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_widget_match_parent_width", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_match_parent_width"), func_id);
        }

        // perry_ui_button_set_title(handle: i64, title_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            let func_id = self.module.declare_function("perry_ui_button_set_title", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_title"), func_id);
        }

        // perry_ui_button_set_image(handle: i64, name_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // SF Symbol name string ptr
            let func_id = self.module.declare_function("perry_ui_button_set_image", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_image"), func_id);
        }

        // perry_ui_button_set_content_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // r
            sig.params.push(AbiParam::new(types::F64)); // g
            sig.params.push(AbiParam::new(types::F64)); // b
            sig.params.push(AbiParam::new(types::F64)); // a
            let func_id = self.module.declare_function("perry_ui_button_set_content_tint_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_content_tint_color"), func_id);
        }

        // perry_ui_button_set_image_position(handle: i64, position: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // position
            let func_id = self.module.declare_function("perry_ui_button_set_image_position", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_image_position"), func_id);
        }

        // perry_ui_textfield_focus(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_textfield_focus", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_focus"), func_id);
        }

        // perry_ui_textfield_set_string(handle: i64, text_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // text string ptr
            let func_id = self.module.declare_function("perry_ui_textfield_set_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_string"), func_id);
        }

        // perry_ui_textfield_get_string(handle: i64) -> i64 (string ptr)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.returns.push(AbiParam::new(types::I64)); // string ptr
            let func_id = self.module.declare_function("perry_ui_textfield_get_string", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_get_string"), func_id);
        }

        // perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // callback closure
            let func_id = self.module.declare_function("perry_ui_textfield_set_on_submit", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_on_submit"), func_id);
        }

        // perry_ui_textfield_set_on_focus(handle: i64, on_focus: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // callback closure
            let func_id = self.module.declare_function("perry_ui_textfield_set_on_focus", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_on_focus"), func_id);
        }

        // perry_ui_textfield_blur_all()
        {
            let mut sig = self.module.make_signature();
            let func_id = self.module.declare_function("perry_ui_textfield_blur_all", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_blur_all"), func_id);
        }

        // perry_ui_textfield_set_borderless(handle: i64, borderless: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_textfield_set_borderless", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_borderless"), func_id);
        }

        // perry_ui_textfield_set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_textfield_set_background_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_background_color"), func_id);
        }

        // perry_ui_textfield_set_font_size(handle: i64, size: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_textfield_set_font_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_font_size"), func_id);
        }

        // perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_textfield_set_text_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_textfield_set_text_color"), func_id);
        }

        // perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // scroll handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_scrollview_scroll_to", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_scroll_to"), func_id);
        }

        // perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // scroll handle
            sig.returns.push(AbiParam::new(types::F64)); // offset
            let func_id = self.module.declare_function("perry_ui_scrollview_get_offset", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_get_offset"), func_id);
        }

        // perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // scroll handle
            sig.params.push(AbiParam::new(types::F64)); // offset
            let func_id = self.module.declare_function("perry_ui_scrollview_set_offset", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_set_offset"), func_id);
        }

        // perry_ui_scrollview_set_refresh_control(scroll_handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // scroll handle
            sig.params.push(AbiParam::new(types::F64)); // callback
            let func_id = self.module.declare_function("perry_ui_scrollview_set_refresh_control", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_set_refresh_control"), func_id);
        }

        // perry_ui_scrollview_end_refreshing(scroll_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // scroll handle
            let func_id = self.module.declare_function("perry_ui_scrollview_end_refreshing", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_scrollview_end_refreshing"), func_id);
        }

        // perry_ui_menu_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // menu handle
            let func_id = self.module.declare_function("perry_ui_menu_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_create"), func_id);
        }

        // perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_menu_add_item", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_add_item"), func_id);
        }

        // perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            let func_id = self.module.declare_function("perry_ui_widget_set_context_menu", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_context_menu"), func_id);
        }

        // perry_ui_menu_add_item_with_shortcut(menu: i64, title: i64, cb: f64, shortcut: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // shortcut string ptr
            let func_id = self.module.declare_function("perry_ui_menu_add_item_with_shortcut", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_add_item_with_shortcut"), func_id);
        }

        // perry_ui_menu_add_standard_action(menu: i64, title: i64, selector: i64, shortcut: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            sig.params.push(AbiParam::new(types::I64)); // selector string ptr
            sig.params.push(AbiParam::new(types::I64)); // shortcut string ptr
            let func_id = self.module.declare_function("perry_ui_menu_add_standard_action", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_add_standard_action"), func_id);
        }

        // perry_ui_menu_clear(menu: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            let func_id = self.module.declare_function("perry_ui_menu_clear", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_clear"), func_id);
        }

        // perry_ui_menu_add_separator(menu: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            let func_id = self.module.declare_function("perry_ui_menu_add_separator", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_add_separator"), func_id);
        }

        // perry_ui_menu_add_submenu(menu: i64, title: i64, submenu: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            sig.params.push(AbiParam::new(types::I64)); // submenu handle
            let func_id = self.module.declare_function("perry_ui_menu_add_submenu", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menu_add_submenu"), func_id);
        }

        // perry_ui_menubar_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64)); // bar handle
            let func_id = self.module.declare_function("perry_ui_menubar_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menubar_create"), func_id);
        }

        // perry_ui_menubar_add_menu(bar: i64, title: i64, menu: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bar handle
            sig.params.push(AbiParam::new(types::I64)); // title string ptr
            sig.params.push(AbiParam::new(types::I64)); // menu handle
            let func_id = self.module.declare_function("perry_ui_menubar_add_menu", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menubar_add_menu"), func_id);
        }

        // perry_ui_menubar_attach(bar: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // bar handle
            let func_id = self.module.declare_function("perry_ui_menubar_attach", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_menubar_attach"), func_id);
        }

        // perry_ui_open_file_dialog(callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_open_file_dialog", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_open_file_dialog"), func_id);
        }

        // perry_ui_open_folder_dialog(callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_open_folder_dialog", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_open_folder_dialog"), func_id);
        }

        // perry_ui_button_set_image(handle: i64, name_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::I64)); // image name string ptr
            let func_id = self.module.declare_function("perry_ui_button_set_image", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_image"), func_id);
        }

        // perry_ui_button_set_image_position — duplicate removed (already declared above with i64 params)

        // perry_ui_button_set_content_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // r
            sig.params.push(AbiParam::new(types::F64)); // g
            sig.params.push(AbiParam::new(types::F64)); // b
            sig.params.push(AbiParam::new(types::F64)); // a
            let func_id = self.module.declare_function("perry_ui_button_set_content_tint_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_button_set_content_tint_color"), func_id);
        }

        // perry_ui_widget_remove_child(parent: i64, child: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            let func_id = self.module.declare_function("perry_ui_widget_remove_child", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_remove_child"), func_id);
        }

        // perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            let func_id = self.module.declare_function("perry_ui_app_set_min_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_min_size"), func_id);
        }

        // perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // app handle
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            let func_id = self.module.declare_function("perry_ui_app_set_max_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_max_size"), func_id);
        }

        // perry_ui_widget_add_child_at(parent: i64, child: i64, index: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // parent handle
            sig.params.push(AbiParam::new(types::I64)); // child handle
            sig.params.push(AbiParam::new(types::F64)); // index
            let func_id = self.module.declare_function("perry_ui_widget_add_child_at", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_add_child_at"), func_id);
        }

        // perry_ui_embed_nsview(nsview_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // nsview_ptr
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_embed_nsview", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_embed_nsview"), func_id);
        }

        // ============================================
        // Perry UI — Weather App Extensions
        // ============================================

        // perry_ui_app_set_timer(interval_ms: f64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // interval_ms
            sig.params.push(AbiParam::new(types::F64)); // callback closure (NaN-boxed)
            let func_id = self.module.declare_function("perry_ui_app_set_timer", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_set_timer"), func_id);
        }

        // perry_ui_widget_set_background_gradient(handle: i64, r1-a1, r2-a2, direction: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // r1
            sig.params.push(AbiParam::new(types::F64)); // g1
            sig.params.push(AbiParam::new(types::F64)); // b1
            sig.params.push(AbiParam::new(types::F64)); // a1
            sig.params.push(AbiParam::new(types::F64)); // r2
            sig.params.push(AbiParam::new(types::F64)); // g2
            sig.params.push(AbiParam::new(types::F64)); // b2
            sig.params.push(AbiParam::new(types::F64)); // a2
            sig.params.push(AbiParam::new(types::F64)); // direction (0=vertical, 1=horizontal)
            let func_id = self.module.declare_function("perry_ui_widget_set_background_gradient", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_background_gradient"), func_id);
        }

        // perry_ui_widget_set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // r
            sig.params.push(AbiParam::new(types::F64)); // g
            sig.params.push(AbiParam::new(types::F64)); // b
            sig.params.push(AbiParam::new(types::F64)); // a
            let func_id = self.module.declare_function("perry_ui_widget_set_background_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_background_color"), func_id);
        }

        // perry_ui_widget_set_corner_radius(handle: i64, radius: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // radius
            let func_id = self.module.declare_function("perry_ui_widget_set_corner_radius", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_corner_radius"), func_id);
        }

        // perry_ui_widget_set_edge_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // widget handle
            sig.params.push(AbiParam::new(types::F64)); // top
            sig.params.push(AbiParam::new(types::F64)); // left
            sig.params.push(AbiParam::new(types::F64)); // bottom
            sig.params.push(AbiParam::new(types::F64)); // right
            let func_id = self.module.declare_function("perry_ui_widget_set_edge_insets", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_edge_insets"), func_id);
        }

        // perry_ui_canvas_create(width: f64, height: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            sig.returns.push(AbiParam::new(types::I64)); // widget handle
            let func_id = self.module.declare_function("perry_ui_canvas_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_create"), func_id);
        }

        // perry_ui_canvas_clear(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // canvas handle
            let func_id = self.module.declare_function("perry_ui_canvas_clear", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_clear"), func_id);
        }

        // perry_ui_canvas_begin_path(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // canvas handle
            let func_id = self.module.declare_function("perry_ui_canvas_begin_path", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_begin_path"), func_id);
        }

        // perry_ui_canvas_move_to(handle: i64, x: f64, y: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // canvas handle
            sig.params.push(AbiParam::new(types::F64)); // x
            sig.params.push(AbiParam::new(types::F64)); // y
            let func_id = self.module.declare_function("perry_ui_canvas_move_to", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_move_to"), func_id);
        }

        // perry_ui_canvas_line_to(handle: i64, x: f64, y: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // canvas handle
            sig.params.push(AbiParam::new(types::F64)); // x
            sig.params.push(AbiParam::new(types::F64)); // y
            let func_id = self.module.declare_function("perry_ui_canvas_line_to", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_line_to"), func_id);
        }

        // perry_ui_canvas_stroke(handle: i64, r: f64, g: f64, b: f64, a: f64, lineWidth: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // canvas handle
            sig.params.push(AbiParam::new(types::F64)); // r
            sig.params.push(AbiParam::new(types::F64)); // g
            sig.params.push(AbiParam::new(types::F64)); // b
            sig.params.push(AbiParam::new(types::F64)); // a
            sig.params.push(AbiParam::new(types::F64)); // lineWidth
            let func_id = self.module.declare_function("perry_ui_canvas_stroke", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_stroke"), func_id);
        }

        // perry_ui_canvas_fill_gradient(handle: i64, r1-a1, r2-a2, direction: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // canvas handle
            sig.params.push(AbiParam::new(types::F64)); // r1
            sig.params.push(AbiParam::new(types::F64)); // g1
            sig.params.push(AbiParam::new(types::F64)); // b1
            sig.params.push(AbiParam::new(types::F64)); // a1
            sig.params.push(AbiParam::new(types::F64)); // r2
            sig.params.push(AbiParam::new(types::F64)); // g2
            sig.params.push(AbiParam::new(types::F64)); // b2
            sig.params.push(AbiParam::new(types::F64)); // a2
            sig.params.push(AbiParam::new(types::F64)); // direction
            let func_id = self.module.declare_function("perry_ui_canvas_fill_gradient", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_canvas_fill_gradient"), func_id);
        }

        // perry_ui_camera_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_camera_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_create"), func_id);
        }

        // perry_ui_camera_start(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_camera_start", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_start"), func_id);
        }

        // perry_ui_camera_stop(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_camera_stop", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_stop"), func_id);
        }

        // perry_ui_camera_freeze(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_camera_freeze", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_freeze"), func_id);
        }

        // perry_ui_camera_unfreeze(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_camera_unfreeze", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_unfreeze"), func_id);
        }

        // perry_ui_camera_sample_color(x: f64, y: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_camera_sample_color", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_sample_color"), func_id);
        }

        // perry_ui_camera_set_on_tap(handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_camera_set_on_tap", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_camera_set_on_tap"), func_id);
        }

        // ============================================
        // Perry UI Phase B: New widgets + interactions
        // ============================================

        // perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_securefield_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_securefield_create"), func_id);
        }

        // perry_ui_progressview_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_progressview_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_progressview_create"), func_id);
        }

        // perry_ui_progressview_set_value(handle: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_progressview_set_value", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_progressview_set_value"), func_id);
        }

        // perry_ui_image_create_symbol(name_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_image_create_symbol", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_image_create_symbol"), func_id);
        }

        // perry_ui_image_create_file(path_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_image_create_file", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_image_create_file"), func_id);
        }

        // perry_ui_image_set_size(handle: i64, width: f64, height: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_image_set_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_image_set_size"), func_id);
        }

        // perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_image_set_tint", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_image_set_tint"), func_id);
        }

        // perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_qrcode_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_qrcode_create"), func_id);
        }

        // perry_ui_qrcode_set_data(handle: i64, data_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_qrcode_set_data", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_qrcode_set_data"), func_id);
        }

        // perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_picker_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_picker_create"), func_id);
        }

        // perry_ui_picker_add_item(handle: i64, title_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_picker_add_item", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_picker_add_item"), func_id);
        }

        // perry_ui_picker_set_selected(handle: i64, index: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_picker_set_selected", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_picker_set_selected"), func_id);
        }

        // perry_ui_picker_get_selected(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_picker_get_selected", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_picker_get_selected"), func_id);
        }

        // perry_ui_tabbar_create(on_change: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_tabbar_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_tabbar_create"), func_id);
        }

        // perry_ui_tabbar_add_tab(handle: i64, label_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_tabbar_add_tab", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_tabbar_add_tab"), func_id);
        }

        // perry_ui_tabbar_set_selected(handle: i64, index: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_tabbar_set_selected", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_tabbar_set_selected"), func_id);
        }

        // perry_ui_form_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_form_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_form_create"), func_id);
        }

        // perry_ui_section_create(title_ptr: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_section_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_section_create"), func_id);
        }

        // perry_ui_navstack_create(title_ptr: i64, body_handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_navstack_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_navstack_create"), func_id);
        }

        // perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_navstack_push", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_navstack_push"), func_id);
        }

        // perry_ui_navstack_pop(handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_navstack_pop", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_navstack_pop"), func_id);
        }

        // perry_ui_zstack_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_zstack_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_zstack_create"), func_id);
        }

        // perry_ui_widget_set_enabled(handle: i64, enabled: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_widget_set_enabled", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_enabled"), func_id);
        }

        // perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_widget_set_tooltip", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_tooltip"), func_id);
        }

        // perry_ui_widget_set_control_size(handle: i64, size: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_widget_set_control_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_control_size"), func_id);
        }

        // perry_ui_widget_set_on_hover(handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_set_on_hover", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_on_hover"), func_id);
        }

        // perry_ui_widget_set_on_double_click(handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_set_on_double_click", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_on_double_click"), func_id);
        }

        // perry_ui_widget_set_on_click(handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_set_on_click", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_set_on_click"), func_id);
        }

        // perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_ms: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_animate_opacity", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_animate_opacity"), func_id);
        }

        // perry_ui_widget_animate_position(handle: i64, dx: f64, dy: f64, duration_ms: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_widget_animate_position", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_widget_animate_position"), func_id);
        }

        // perry_ui_state_on_change(state_handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_state_on_change", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_on_change"), func_id);
        }

        // perry_ui_text_set_font_family(handle: i64, family_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_text_set_font_family", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_text_set_font_family"), func_id);
        }

        // perry_ui_save_file_dialog(callback: f64, default_name: i64, allowed_types: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // callback closure
            sig.params.push(AbiParam::new(types::I64)); // default name string ptr
            sig.params.push(AbiParam::new(types::I64)); // allowed types string ptr
            let func_id = self.module.declare_function("perry_ui_save_file_dialog", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_save_file_dialog"), func_id);
        }

        // perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_state_bind_textfield", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_state_bind_textfield"), func_id);
        }

        // perry_ui_alert(title: i64, message: i64, buttons: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // title
            sig.params.push(AbiParam::new(types::I64)); // message
            sig.params.push(AbiParam::new(types::I64)); // buttons array
            sig.params.push(AbiParam::new(types::F64)); // callback
            let func_id = self.module.declare_function("perry_ui_alert", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_alert"), func_id);
        }

        // perry_ui_sheet_create(width: f64, height: f64, title: f64) -> i64
        // title is NaN-boxed string - Rust side extracts pointer via js_nanbox_get_pointer
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            sig.params.push(AbiParam::new(types::F64)); // title (NaN-boxed)
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_sheet_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_sheet_create"), func_id);
        }

        // perry_ui_sheet_present(sheet_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_sheet_present", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_sheet_present"), func_id);
        }

        // perry_ui_sheet_dismiss(sheet_handle: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_sheet_dismiss", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_sheet_dismiss"), func_id);
        }

        // perry_ui_app_on_terminate(callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_app_on_terminate", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_on_terminate"), func_id);
        }

        // perry_ui_app_on_activate(callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_app_on_activate", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_app_on_activate"), func_id);
        }

        // perry_ui_toolbar_create() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_toolbar_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_toolbar_create"), func_id);
        }

        // perry_ui_toolbar_add_item(toolbar: i64, label: i64, icon: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_ui_toolbar_add_item", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_toolbar_add_item"), func_id);
        }

        // perry_ui_toolbar_attach(toolbar: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_toolbar_attach", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_toolbar_attach"), func_id);
        }

        // perry_ui_window_create(title: i64, width: f64, height: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // title
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_window_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_create"), func_id);
        }

        // perry_ui_window_set_body(window: i64, widget: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_window_set_body", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_set_body"), func_id);
        }

        // perry_ui_window_show(window: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_window_show", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_show"), func_id);
        }

        // perry_ui_window_close(window: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_window_close", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_close"), func_id);
        }

        // perry_ui_window_hide(window: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_window_hide", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_hide"), func_id);
        }

        // perry_ui_window_set_size(window: i64, width: f64, height: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // window handle
            sig.params.push(AbiParam::new(types::F64)); // width
            sig.params.push(AbiParam::new(types::F64)); // height
            let func_id = self.module.declare_function("perry_ui_window_set_size", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_set_size"), func_id);
        }

        // perry_ui_window_on_focus_lost(window: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // window handle
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed closure
            let func_id = self.module.declare_function("perry_ui_window_on_focus_lost", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_window_on_focus_lost"), func_id);
        }

        // perry_ui_lazyvstack_create(count: f64, render: f64) -> i64
        // count arrives as f64 (JS number) - Rust side casts to i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // count (JS number)
            sig.params.push(AbiParam::new(types::F64)); // render closure
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_lazyvstack_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_lazyvstack_create"), func_id);
        }

        // perry_ui_lazyvstack_update(handle: i64, count: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_lazyvstack_update", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_lazyvstack_update"), func_id);
        }

        // perry_ui_table_create(row_count: f64, col_count: f64, render: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // row_count (JS number)
            sig.params.push(AbiParam::new(types::F64)); // col_count (JS number)
            sig.params.push(AbiParam::new(types::F64)); // render closure
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_table_create", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_table_create"), func_id);
        }
        // perry_ui_table_set_column_header(handle: i64, col: i64, title_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // col
            sig.params.push(AbiParam::new(types::I64)); // title_ptr (StringHeader*)
            let func_id = self.module.declare_function("perry_ui_table_set_column_header", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_table_set_column_header"), func_id);
        }
        // perry_ui_table_set_column_width(handle: i64, col: i64, width: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // col
            sig.params.push(AbiParam::new(types::F64)); // width
            let func_id = self.module.declare_function("perry_ui_table_set_column_width", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_table_set_column_width"), func_id);
        }
        // perry_ui_table_update_row_count(handle: i64, count: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::I64)); // count
            let func_id = self.module.declare_function("perry_ui_table_update_row_count", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_table_update_row_count"), func_id);
        }
        // perry_ui_table_set_on_row_select(handle: i64, callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.params.push(AbiParam::new(types::F64)); // callback closure
            let func_id = self.module.declare_function("perry_ui_table_set_on_row_select", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_table_set_on_row_select"), func_id);
        }
        // perry_ui_table_get_selected_row(handle: i64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // handle
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_ui_table_get_selected_row", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_ui_table_get_selected_row"), func_id);
        }

        // ============================================
        // Perry System APIs (perry/system module)
        // ============================================

        // perry_system_open_url(url_ptr: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_open_url", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_open_url"), func_id);
        }

        // perry_system_is_dark_mode() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_is_dark_mode", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_is_dark_mode"), func_id);
        }

        // perry_system_preferences_set(key_ptr: i64, value: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_preferences_set", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_preferences_set"), func_id);
        }

        // perry_system_preferences_get(key_ptr: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_preferences_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_preferences_get"), func_id);
        }

        // perry_system_request_location(callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // callback (NaN-boxed closure)
            let func_id = self.module.declare_function("perry_system_request_location", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_request_location"), func_id);
        }

        // perry_system_keychain_save(key: i64, value: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_keychain_save", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_keychain_save"), func_id);
        }

        // perry_system_keychain_get(key: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_keychain_get", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_keychain_get"), func_id);
        }

        // perry_system_keychain_delete(key: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_keychain_delete", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_keychain_delete"), func_id);
        }

        // perry_system_notification_send(title: i64, body: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_notification_send", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_notification_send"), func_id);
        }

        // perry_system_request_location(callback: f64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_request_location", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_request_location"), func_id);
        }

        // ============================================
        // Perry Audio System FFI functions
        // ============================================

        // perry_system_audio_start() -> i64 (1=success, 0=failure)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_audio_start", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_audio_start"), func_id);
        }

        // perry_system_audio_stop()
        {
            let mut sig = self.module.make_signature();
            let func_id = self.module.declare_function("perry_system_audio_stop", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_audio_stop"), func_id);
        }

        // perry_system_audio_get_level() -> f64 (current dB(A) level)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_audio_get_level", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_audio_get_level"), func_id);
        }

        // perry_system_audio_get_peak() -> f64 (peak sample value)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_audio_get_peak", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_audio_get_peak"), func_id);
        }

        // perry_system_audio_get_waveform(count: f64) -> f64 (NaN-boxed array handle)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_system_audio_get_waveform", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_audio_get_waveform"), func_id);
        }

        // perry_system_get_device_model() -> i64 (NaN-boxed string)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_system_get_device_model", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_system_get_device_model"), func_id);
        }

        // ============================================
        // Perry Plugin System FFI functions
        // ============================================

        // perry_plugin_register_hook(api_handle: i64, hook_name: f64, handler: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // hook name (NaN-boxed string)
            sig.params.push(AbiParam::new(types::F64)); // handler closure (NaN-boxed)
            sig.returns.push(AbiParam::new(types::F64)); // undefined
            let func_id = self.module.declare_function("perry_plugin_register_hook", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_register_hook"), func_id);
        }

        // perry_plugin_register_tool(api_handle: i64, name: f64, desc: f64, handler: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // tool name
            sig.params.push(AbiParam::new(types::F64)); // description
            sig.params.push(AbiParam::new(types::F64)); // handler closure
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_register_tool", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_register_tool"), func_id);
        }

        // perry_plugin_register_service(api_handle: i64, name: f64, start_fn: f64, stop_fn: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // service name
            sig.params.push(AbiParam::new(types::F64)); // start function
            sig.params.push(AbiParam::new(types::F64)); // stop function
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_register_service", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_register_service"), func_id);
        }

        // perry_plugin_register_route(api_handle: i64, path: f64, handler: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // route path
            sig.params.push(AbiParam::new(types::F64)); // handler closure
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_register_route", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_register_route"), func_id);
        }

        // perry_plugin_get_config(api_handle: i64, key: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // config key
            sig.returns.push(AbiParam::new(types::F64)); // config value
            let func_id = self.module.declare_function("perry_plugin_get_config", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_get_config"), func_id);
        }

        // perry_plugin_log(api_handle: i64, level: i64, message: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::I64)); // log level
            sig.params.push(AbiParam::new(types::F64)); // message
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_log", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_log"), func_id);
        }

        // perry_plugin_load(path: f64) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // path (NaN-boxed string)
            sig.returns.push(AbiParam::new(types::I64)); // plugin id
            let func_id = self.module.declare_function("perry_plugin_load", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_load"), func_id);
        }

        // perry_plugin_unload(plugin_id: i64)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // plugin id
            let func_id = self.module.declare_function("perry_plugin_unload", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_unload"), func_id);
        }

        // perry_plugin_emit_hook(hook_name: f64, context: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // hook name
            sig.params.push(AbiParam::new(types::F64)); // context
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function("perry_plugin_emit_hook", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_emit_hook"), func_id);
        }

        // perry_plugin_discover(dir_path: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // directory path
            sig.returns.push(AbiParam::new(types::F64)); // array of paths (NaN-boxed)
            let func_id = self.module.declare_function("perry_plugin_discover", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_discover"), func_id);
        }

        // perry_plugin_init()
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function("perry_plugin_init", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_init"), func_id);
        }

        // perry_plugin_count() -> i64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_plugin_count", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_count"), func_id);
        }

        // perry_plugin_register_hook_ex(api_handle: i64, hook_name: f64, handler: f64, priority: i64, mode: i64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // hook name
            sig.params.push(AbiParam::new(types::F64)); // handler closure
            sig.params.push(AbiParam::new(types::I64)); // priority
            sig.params.push(AbiParam::new(types::I64)); // mode
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_register_hook_ex", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_register_hook_ex"), func_id);
        }

        // perry_plugin_set_metadata(api_handle: i64, name: f64, version: f64, description: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // name
            sig.params.push(AbiParam::new(types::F64)); // version
            sig.params.push(AbiParam::new(types::F64)); // description
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_set_metadata", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_set_metadata"), func_id);
        }

        // perry_plugin_on(api_handle: i64, event: f64, handler: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // event name
            sig.params.push(AbiParam::new(types::F64)); // handler closure
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_on", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_on"), func_id);
        }

        // perry_plugin_emit(api_handle: i64, event: f64, data: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // api handle
            sig.params.push(AbiParam::new(types::F64)); // event name
            sig.params.push(AbiParam::new(types::F64)); // data
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_emit", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_emit"), func_id);
        }

        // perry_plugin_emit_event(event: f64, data: f64) -> f64 (host-side)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // event name
            sig.params.push(AbiParam::new(types::F64)); // data
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_emit_event", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_emit_event"), func_id);
        }

        // perry_plugin_invoke_tool(name: f64, args: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // tool name
            sig.params.push(AbiParam::new(types::F64)); // args
            sig.returns.push(AbiParam::new(types::F64)); // result
            let func_id = self.module.declare_function("perry_plugin_invoke_tool", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_invoke_tool"), func_id);
        }

        // perry_plugin_set_config(key: f64, value: f64) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // key
            sig.params.push(AbiParam::new(types::F64)); // value
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_plugin_set_config", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_set_config"), func_id);
        }

        // perry_plugin_list_plugins() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64)); // array of plugin objects
            let func_id = self.module.declare_function("perry_plugin_list_plugins", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_list_plugins"), func_id);
        }

        // perry_plugin_list_hooks() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64)); // array of hook name strings
            let func_id = self.module.declare_function("perry_plugin_list_hooks", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_list_hooks"), func_id);
        }

        // perry_plugin_list_tools() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64)); // array of tool objects
            let func_id = self.module.declare_function("perry_plugin_list_tools", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_plugin_list_tools"), func_id);
        }

        // ============================================
        // V8 JavaScript Runtime FFI functions
        // ============================================

        // js_runtime_init() -> void
        // Initialize the V8 JavaScript runtime
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function("js_runtime_init", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_runtime_init"), func_id);
        }

        // js_load_module(path_ptr: i64, path_len: i64) -> u64 (module handle)
        // Load a JavaScript module and return a handle
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // path pointer
            sig.params.push(AbiParam::new(types::I64)); // path length
            sig.returns.push(AbiParam::new(types::I64)); // module handle (u64)
            let func_id = self.module.declare_function("js_load_module", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_load_module"), func_id);
        }

        // js_get_export(module_handle: u64, name_ptr: i64, name_len: i64) -> f64 (NaN-boxed value)
        // Get an export from a loaded module
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // module handle
            sig.params.push(AbiParam::new(types::I64)); // export name pointer
            sig.params.push(AbiParam::new(types::I64)); // export name length
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed value
            let func_id = self.module.declare_function("js_get_export", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_get_export"), func_id);
        }

        // js_call_function(module_handle: u64, name_ptr: i64, name_len: i64, args_ptr: i64, args_len: i64) -> f64
        // Call a function from a loaded module
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // module handle
            sig.params.push(AbiParam::new(types::I64)); // function name pointer
            sig.params.push(AbiParam::new(types::I64)); // function name length
            sig.params.push(AbiParam::new(types::I64)); // args array pointer
            sig.params.push(AbiParam::new(types::I64)); // args count
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value
            let func_id = self.module.declare_function("js_call_function", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_call_function"), func_id);
        }

        // js_native_call_method(object: f64, name_ptr: i64, name_len: i64, args_ptr: i64, args_len: i64) -> f64
        // Call a method on a JavaScript object
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // method name pointer
            sig.params.push(AbiParam::new(types::I64)); // method name length
            sig.params.push(AbiParam::new(types::I64)); // args array pointer
            sig.params.push(AbiParam::new(types::I64)); // args count
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value
            let func_id = self.module.declare_function("js_native_call_method", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_native_call_method"), func_id);
        }

        // js_register_class_method(class_id: i64, name_ptr: i64, name_len: i64, func_ptr: i64, param_count: i64) -> void
        // Register a class method in the vtable for runtime dispatch
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // class_id
            sig.params.push(AbiParam::new(types::I64)); // name_ptr
            sig.params.push(AbiParam::new(types::I64)); // name_len
            sig.params.push(AbiParam::new(types::I64)); // func_ptr
            sig.params.push(AbiParam::new(types::I64)); // param_count
            let func_id = self.module.declare_function("js_register_class_method", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_register_class_method"), func_id);
        }

        // js_register_class_getter(class_id: i64, name_ptr: i64, name_len: i64, func_ptr: i64) -> void
        // Register a class getter in the vtable for runtime dispatch
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // class_id
            sig.params.push(AbiParam::new(types::I64)); // name_ptr
            sig.params.push(AbiParam::new(types::I64)); // name_len
            sig.params.push(AbiParam::new(types::I64)); // func_ptr
            let func_id = self.module.declare_function("js_register_class_getter", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_register_class_getter"), func_id);
        }

        // js_native_call_value(func_value: f64, args_ptr: i64, args_len: i64) -> f64
        // Call a JavaScript function value directly (for callback parameters)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // function value (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // args array pointer
            sig.params.push(AbiParam::new(types::I64)); // args count
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value
            let func_id = self.module.declare_function("js_native_call_value", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_native_call_value"), func_id);
        }

        // js_await_js_promise(value: f64) -> f64
        // Await a V8 JS Promise (runs V8 event loop until settled, returns resolved value)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // JS handle (NaN-boxed)
            sig.returns.push(AbiParam::new(types::F64)); // resolved value (NaN-boxed)
            let func_id = self.module.declare_function("js_await_js_promise", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_await_js_promise"), func_id);
        }

        // Await any promise (JS handle OR native POINTER_TAG), returns resolved value
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // NaN-boxed value
            sig.returns.push(AbiParam::new(types::F64)); // resolved value (NaN-boxed)
            let func_id = self.module.declare_function("js_await_any_promise", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_await_any_promise"), func_id);
        }

        // js_get_property(object: f64, name_ptr: i64, name_len: i64) -> f64
        // Get a property from a JavaScript object
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // property name pointer
            sig.params.push(AbiParam::new(types::I64)); // property name length
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value
            let func_id = self.module.declare_function("js_get_property", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_get_property"), func_id);
        }

        // js_set_property(object: f64, name_ptr: i64, name_len: i64, value: f64) -> void
        // Set a property on a JavaScript object
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // object (NaN-boxed)
            sig.params.push(AbiParam::new(types::I64)); // property name pointer
            sig.params.push(AbiParam::new(types::I64)); // property name length
            sig.params.push(AbiParam::new(types::F64)); // value (NaN-boxed)
            let func_id = self.module.declare_function("js_set_property", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_set_property"), func_id);
        }

        // js_new_instance(module: u64, class_ptr: i64, class_len: i64, args_ptr: i64, args_len: i64) -> f64
        // Create a new instance of a JavaScript class
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // module handle
            sig.params.push(AbiParam::new(types::I64)); // class name pointer
            sig.params.push(AbiParam::new(types::I64)); // class name length
            sig.params.push(AbiParam::new(types::I64)); // args array pointer
            sig.params.push(AbiParam::new(types::I64)); // args count
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value (JS handle)
            let func_id = self.module.declare_function("js_new_instance", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_new_instance"), func_id);
        }

        // js_new_from_handle(constructor: f64, args_ptr: i64, args_len: i64) -> f64
        // Create a new instance using a JS handle to a constructor
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64)); // constructor (NaN-boxed JS handle)
            sig.params.push(AbiParam::new(types::I64)); // args array pointer
            sig.params.push(AbiParam::new(types::I64)); // args count
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value (JS handle)
            let func_id = self.module.declare_function("js_new_from_handle", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_new_from_handle"), func_id);
        }

        // js_create_callback(func_ptr: i64, closure_env: i64, param_count: i64) -> f64
        // Create a V8 function that wraps a native callback
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // function pointer
            sig.params.push(AbiParam::new(types::I64)); // closure environment pointer
            sig.params.push(AbiParam::new(types::I64)); // parameter count
            sig.returns.push(AbiParam::new(types::F64)); // NaN-boxed return value (JS handle)
            let func_id = self.module.declare_function("js_create_callback", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_create_callback"), func_id);
        }

        // ============================================
        // Garbage Collection FFI functions
        // ============================================

        // js_gc_collect() -> void
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function("js_gc_collect", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_gc_collect"), func_id);
            // Also register as "gc" for TypeScript code: gc()
            self.extern_funcs.insert(Cow::Borrowed("gc"), func_id);
        }

        // js_gc_register_global_root(ptr: i64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // pointer to module global
            let func_id = self.module.declare_function("js_gc_register_global_root", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_gc_register_global_root"), func_id);
        }

        // js_gc_init() -> void
        {
            let sig = self.module.make_signature();
            let func_id = self.module.declare_function("js_gc_init", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("js_gc_init"), func_id);
        }

        // perry_register_static_plugin(path: *StringHeader, value: f64) -> void
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_register_static_plugin", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_register_static_plugin"), func_id);
        }

        // perry_resolve_static_plugin(path: *StringHeader) -> f64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_resolve_static_plugin", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_resolve_static_plugin"), func_id);
        }

        // ============================================
        // Platform screen detection functions
        // ============================================
        // perry_get_screen_width() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_get_screen_width", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_get_screen_width"), func_id);
        }
        // perry_get_screen_height() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_get_screen_height", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_get_screen_height"), func_id);
        }
        // perry_get_scale_factor() -> f64
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_get_scale_factor", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_get_scale_factor"), func_id);
        }
        // perry_get_orientation() -> f64 (NaN-boxed string)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_get_orientation", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_get_orientation"), func_id);
        }
        // perry_get_device_idiom() -> f64 (0=phone, 1=pad, 2=tv)
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::F64));
            let func_id = self.module.declare_function("perry_get_device_idiom", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_get_device_idiom"), func_id);
        }

        // ============================================
        // External native library FFI functions
        // ============================================
        // Declared from perry.nativeLibrary manifests in package.json
        for (func_name, params, returns) in &self.native_library_functions {
            let mut sig = self.module.make_signature();
            for param_type in params {
                sig.params.push(AbiParam::new(Self::parse_cranelift_type(param_type)));
            }
            if returns != "void" {
                sig.returns.push(AbiParam::new(Self::parse_cranelift_type(returns)));
            }
            let func_id = self.module.declare_function(func_name, Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Owned(func_name.clone()), func_id);
        }

        // --- i18n runtime functions ---

        // perry_i18n_get_locale_index() -> i32
        {
            let mut sig = self.module.make_signature();
            sig.returns.push(AbiParam::new(types::I32));
            let func_id = self.module.declare_function("perry_i18n_get_locale_index", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_get_locale_index"), func_id);
        }

        // perry_i18n_init(locale_codes: *const *const u8, locale_lens: *const u32, count: u32)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // locale_codes array pointer
            sig.params.push(AbiParam::new(types::I64)); // locale_lens array pointer
            sig.params.push(AbiParam::new(types::I32)); // count
            let func_id = self.module.declare_function("perry_i18n_init", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_init"), func_id);
        }

        // perry_i18n_interpolate(template: *mut StringHeader, names: *const *mut StringHeader,
        //                        values: *const *mut StringHeader, count: u32) -> *mut StringHeader
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64)); // template_ptr
            sig.params.push(AbiParam::new(types::I64)); // param_names array
            sig.params.push(AbiParam::new(types::I64)); // param_values array
            sig.params.push(AbiParam::new(types::I32)); // param_count
            sig.returns.push(AbiParam::new(types::I64)); // result string ptr
            let func_id = self.module.declare_function("perry_i18n_interpolate", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_interpolate"), func_id);
        }

        // perry_i18n_plural_category(locale_idx: i32, count: f64) -> i32
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I32)); // locale_idx
            sig.params.push(AbiParam::new(types::F64)); // count
            sig.returns.push(AbiParam::new(types::I32)); // category (0-5)
            let func_id = self.module.declare_function("perry_i18n_plural_category", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_plural_category"), func_id);
        }

        // perry_i18n_format_number(value: f64, locale_idx: i32) -> i64 (StringHeader*)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_i18n_format_number", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_format_number"), func_id);
        }

        // perry_i18n_format_currency(value: f64, locale_idx: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_i18n_format_currency", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_format_currency"), func_id);
        }

        // perry_i18n_format_percent(value: f64, locale_idx: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_i18n_format_percent", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_format_percent"), func_id);
        }

        // perry_i18n_format_date(timestamp: f64, style: i32, locale_idx: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I32));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_i18n_format_date", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_format_date"), func_id);
        }

        // perry_i18n_format_time(timestamp: f64, locale_idx: i32) -> i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::F64));
            sig.params.push(AbiParam::new(types::I32));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function("perry_i18n_format_time", Linkage::Import, &sig)?;
            self.extern_funcs.insert(Cow::Borrowed("perry_i18n_format_time"), func_id);
        }

        Ok(())
    }
}
