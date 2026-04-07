//! Object representation for Perry
//!
//! Objects are heap-allocated with a header containing:
//! - Class ID (for type checking and vtable lookup)
//! - Field count
//! - Keys array pointer (for Object.keys() support)
//! - Fields array (inline)

use crate::JSValue;
use crate::ArrayHeader;
use crate::arena::arena_alloc_gc;
use std::cell::{Cell, RefCell};
use std::ptr;
use std::collections::HashMap;
use std::sync::RwLock;

/// Overflow field storage for objects that exceed their pre-allocated inline slot count.
/// Keyed by (obj_ptr as usize) -> (field_index -> JSValue bits).
/// This handles cases like Object.assign() adding many fields to an object
/// that was allocated with only 8 slots (e.g., @noble/curves Fp field with 21 properties).
thread_local! {
    static OVERFLOW_FIELDS: RefCell<HashMap<usize, HashMap<usize, u64>>> = RefCell::new(HashMap::new());
}

/// Recursion depth guard for js_native_call_method to prevent stack overflow
/// from circular module dependencies during initialization.
thread_local! {
    static CALL_METHOD_DEPTH: Cell<u32> = const { Cell::new(0) };
}
const MAX_CALL_METHOD_DEPTH: u32 = 512;

struct CallMethodDepthGuard;
impl CallMethodDepthGuard {
    fn enter(method_name: &str) -> Option<Self> {
        CALL_METHOD_DEPTH.with(|d| {
            let v = d.get();
            if v >= MAX_CALL_METHOD_DEPTH {
                // Silently return null object to prevent stack overflow
                None
            } else {
                // Debug logging disabled for production runs
                // if v <= 10 || v % 50 == 0 {
                //     eprintln!("[DEPTH GUARD] depth={} calling method '{}'", v, method_name);
                // }
                d.set(v + 1);
                Some(CallMethodDepthGuard)
            }
        })
    }
}
impl Drop for CallMethodDepthGuard {
    fn drop(&mut self) {
        CALL_METHOD_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// Static "null object" used as a safe return value when the depth guard triggers.
/// Instead of returning undefined (which callers may dereference as a null pointer),
/// we return a pointer to this valid-but-empty object so downstream code doesn't crash.
///
/// Uses a raw byte array with matching layout to avoid Sync issues with raw pointers.
#[repr(C, align(8))]
struct NullObjectBytes {
    object_type: u32,   // 1 = OBJECT_TYPE_REGULAR
    class_id: u32,      // 0
    parent_class_id: u32, // 0
    field_count: u32,   // 0
    keys_array: u64,    // 0 (null pointer as u64)
}
// Safety: this is a read-only zero-initialized struct with no interior mutability
unsafe impl Sync for NullObjectBytes {}

static NULL_OBJECT_BYTES: NullObjectBytes = NullObjectBytes {
    object_type: 1,
    class_id: 0,
    parent_class_id: 0,
    field_count: 0,
    keys_array: 0,
};

thread_local! {
    static SHAPE_CACHE: RefCell<HashMap<u32, *mut ArrayHeader>> = RefCell::new(HashMap::new());
}

/// GC root scanner: mark all cached shape keys arrays so they're not freed.
/// SHAPE_CACHE holds raw *mut ArrayHeader pointers shared across objects with the same shape.
/// Without this scanner, GC would free those arrays, leaving all objects with that shape
/// holding a dangling keys_array pointer.
pub fn scan_shape_cache_roots(mark: &mut dyn FnMut(f64)) {
    SHAPE_CACHE.with(|cache| {
        let cache = cache.borrow();
        for &arr_ptr in cache.values() {
            if !arr_ptr.is_null() {
                let jsval = JSValue::pointer(arr_ptr as *const u8);
                mark(f64::from_bits(jsval.bits()));
            }
        }
    });
}

/// GC root scanner: mark all JSValues stored in OVERFLOW_FIELDS.
/// OVERFLOW_FIELDS stores extra properties for objects that exceed their pre-allocated inline
/// slot count. The u64 JSValue bits may contain NaN-boxed pointers to heap objects (strings,
/// arrays, other objects) that are ONLY referenced via OVERFLOW_FIELDS. Without this scanner,
/// GC would free those referenced objects.
pub fn scan_overflow_fields_roots(mark: &mut dyn FnMut(f64)) {
    OVERFLOW_FIELDS.with(|m| {
        let m = m.borrow();
        for fields in m.values() {
            for &val_bits in fields.values() {
                // Mark any NaN-boxed heap pointer (POINTER_TAG, STRING_TAG, BIGINT_TAG)
                let tag = val_bits >> 48;
                if tag == 0x7FFD || tag == 0x7FFF || tag == 0x7FFA {
                    mark(f64::from_bits(val_bits));
                }
            }
        }
    });
}

/// Remove OVERFLOW_FIELDS entry for a freed object pointer.
/// Called from GC sweep when an ObjectHeader is collected, to prevent stale entries
/// from "infecting" new objects allocated at the same address.
pub fn clear_overflow_for_ptr(obj_ptr: usize) {
    OVERFLOW_FIELDS.with(|m| {
        m.borrow_mut().remove(&obj_ptr);
    });
}

/// Global class registry mapping class_id -> parent_class_id for inheritance chain lookups
static CLASS_REGISTRY: RwLock<Option<HashMap<u32, u32>>> = RwLock::new(None);

// ============================================================================
// Class method vtable registry — enables runtime dispatch for interface-typed
// and dynamically-typed method calls.  Each class registers its methods and
// getters at startup; js_native_call_method / js_dynamic_object_get_property
// look up the vtable by the object's class_id when static dispatch isn't possible.
// ============================================================================

/// Entry in the class method vtable
pub struct VTableMethodEntry {
    pub func_ptr: usize,
    pub param_count: u32,
}

/// Per-class vtable with methods and getters
pub struct ClassVTable {
    pub methods: HashMap<String, VTableMethodEntry>,
    pub getters: HashMap<String, usize>, // getter func_ptr (signature: fn(i64) -> f64)
}

/// Global vtable registry: class_id -> vtable
pub static CLASS_VTABLE_REGISTRY: RwLock<Option<HashMap<u32, ClassVTable>>> = RwLock::new(None);

/// Function pointer type for dispatching method calls on handle-based objects.
/// Handle-based objects use small integer IDs (1, 2, 3...) instead of real heap pointers.
/// This is registered by perry-stdlib to dispatch to Fastify, ioredis, etc.
type HandleMethodDispatchFn = unsafe extern "C" fn(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64;

static mut HANDLE_METHOD_DISPATCH: Option<HandleMethodDispatchFn> = None;

/// Function pointer type for dispatching property access on handle-based objects.
type HandlePropertyDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64;

pub static mut HANDLE_PROPERTY_DISPATCH: Option<HandlePropertyDispatchFn> = None;

/// Function pointer type for dispatching property set on handle-based objects.
type HandlePropertySetDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
);

pub static mut HANDLE_PROPERTY_SET_DISPATCH: Option<HandlePropertySetDispatchFn> = None;

/// Register a function to handle method calls on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_method_dispatch(f: HandleMethodDispatchFn) {
    HANDLE_METHOD_DISPATCH = Some(f);
}

/// Register a function to handle property access on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_dispatch(f: HandlePropertyDispatchFn) {
    HANDLE_PROPERTY_DISPATCH = Some(f);
}

/// Register a function to handle property set on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_set_dispatch(f: HandlePropertySetDispatchFn) {
    HANDLE_PROPERTY_SET_DISPATCH = Some(f);
}

/// Register a class method in the vtable registry.
/// Called at startup from the init function for every class method/getter.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
    });
    vtable.methods.insert(name, VTableMethodEntry {
        func_ptr: func_ptr as usize,
        param_count: param_count as u32,
    });
}

/// Register a class getter in the vtable registry.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
    });
    vtable.getters.insert(name, func_ptr as usize);
}

/// Call a vtable method with the correct arity.
/// All method params are f64, `this` is i64.
unsafe fn call_vtable_method(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
) -> f64 {
    #[inline(always)]
    unsafe fn arg_or_nan(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len { *args_ptr.add(idx) } else { f64::NAN }
    }

    match param_count {
        0 => {
            let f: extern "C" fn(i64) -> f64 = std::mem::transmute(func_ptr);
            let result = f(this);
            // eprintln!("[vtable_call] func=0x{:x} this=0x{:x} pc=0 result_bits=0x{:016x} result_f64={}", func_ptr, this, result.to_bits(), result);
            result
        }
        1 => {
            let f: extern "C" fn(i64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0))
        }
        2 => {
            let f: extern "C" fn(i64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1))
        }
        3 => {
            let f: extern "C" fn(i64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2))
        }
        4 => {
            let f: extern "C" fn(i64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3))
        }
        5 => {
            let f: extern "C" fn(i64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4))
        }
        6 => {
            let f: extern "C" fn(i64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5))
        }
        7 => {
            let f: extern "C" fn(i64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6))
        }
        8 => {
            let f: extern "C" fn(i64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6), arg_or_nan(args_ptr, args_len, 7))
        }
        9 => {
            let f: extern "C" fn(i64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6), arg_or_nan(args_ptr, args_len, 7), arg_or_nan(args_ptr, args_len, 8))
        }
        _ => {
            let f: extern "C" fn(i64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6), arg_or_nan(args_ptr, args_len, 7), arg_or_nan(args_ptr, args_len, 8), arg_or_nan(args_ptr, args_len, 9))
        }
    }
}

/// Register a class with its parent class ID in the global registry
fn register_class(class_id: u32, parent_class_id: u32) {
    let mut registry = CLASS_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry.as_mut().unwrap().insert(class_id, parent_class_id);
}

/// Look up parent class ID from the registry
fn get_parent_class_id(class_id: u32) -> Option<u32> {
    let registry = CLASS_REGISTRY.read().unwrap();
    registry.as_ref().and_then(|r| r.get(&class_id).copied())
}

/// Object header - precedes the fields in memory
#[repr(C)]
pub struct ObjectHeader {
    /// Type tag to distinguish from Error objects (must be first field!)
    /// Uses OBJECT_TYPE_REGULAR (1) for regular objects
    pub object_type: u32,
    /// Class ID for this object (used for instanceof, vtable lookup)
    pub class_id: u32,
    /// Parent class ID for inheritance chain (0 if no parent)
    pub parent_class_id: u32,
    /// Number of fields in this object
    pub field_count: u32,
    /// Pointer to array of key strings (for Object.keys() support)
    /// NULL for class instances (keys are defined by the class)
    pub keys_array: *mut ArrayHeader,
}

/// Allocate a new object with the given class ID and field count
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    js_object_alloc_with_parent(class_id, 0, field_count)
}

/// Allocate a new object with class ID, parent class ID, and field count
/// The parent_class_id is used for instanceof inheritance checks
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc_with_parent(class_id: u32, parent_class_id: u32, field_count: u32) -> *mut ObjectHeader {
    // Register this class's parent for inheritance lookups
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate at least 8 field slots to match js_object_set_field_by_name's alloc_limit
    // assumption (max(field_count, 8)). Without this, empty objects ({}) with field_count=0
    // would have 0 field slots but js_object_set_field_by_name writes up to 8 fields inline,
    // causing heap buffer overflow into adjacent arena objects.
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = ptr::null_mut();

        // Initialize ALL allocated field slots to undefined (not just field_count)
        // We allocate max(field_count, 8) slots but must zero all of them to prevent
        // stale data from previously freed GC objects from bleeding through.
        let fields_ptr = (ptr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }

        ptr
    }
}

/// Fast object allocation using bump allocator - NO field initialization
/// This is significantly faster for hot paths where constructor immediately sets all fields
/// Returns a pointer to the object header with UNINITIALIZED fields
#[no_mangle]
pub extern "C" fn js_object_alloc_fast(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header only - fields left uninitialized for constructor to fill
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = ptr::null_mut();
    }

    ptr
}

/// Fast object allocation with parent class ID - NO field initialization
#[no_mangle]
pub extern "C" fn js_object_alloc_fast_with_parent(class_id: u32, parent_class_id: u32, field_count: u32) -> *mut ObjectHeader {

    // Only register class if it has a parent (one-time operation per class)
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = ptr::null_mut();
    }

    ptr
}

/// Allocate a class instance with a shape-cached keys array for field names.
/// This allows dynamic property access (obj.field1) to work on class instances,
/// not just object literals. Uses class_id as the shape_id for caching.
#[no_mangle]
pub extern "C" fn js_object_alloc_class_with_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    // Register parent class if needed
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
    }

    // Use class_id as shape_id for caching the keys array
    let keys_arr = SHAPE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // Use hash of (class_id, field_count) as shape_id to avoid collisions between
        // classes from different modules that might have the same class_id
        // This ensures unique shape_ids across all classes regardless of module
        let shape_id = class_id.wrapping_mul(10007).wrapping_add(field_count.wrapping_mul(100003)).wrapping_add(1000000);
        if let Some(&arr) = cache.get(&shape_id) {
            return arr;
        }

        // Create keys array from packed field names
        let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
        let num_keys = keys.len();

        let arr = crate::array::js_array_alloc_with_length(num_keys as u32);
        let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };

        for (i, key_bytes) in keys.iter().enumerate() {
            let str_ptr = crate::string::js_string_from_bytes(
                key_bytes.as_ptr(), key_bytes.len() as u32,
            );
            let nanboxed = f64::from_bits(
                crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK)
            );
            unsafe { *elements_ptr.add(i) = nanboxed; }
        }

        cache.insert(shape_id, arr);
        arr
    });

    unsafe { (*ptr).keys_array = keys_arr; }
    ptr
}

/// Allocate an object with a shape-cached keys array.
/// First call per shape_id creates the keys array from packed_keys (null-separated key names);
/// subsequent calls reuse the cached pointer. This eliminates per-object key string allocation
/// and array construction for repeated object literals with the same shape.
#[no_mangle]
pub extern "C" fn js_object_alloc_with_shape(
    shape_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate extra field slots for dynamic property growth (plain objects may get new fields)
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * 8;
    let total_size = header_size + fields_size;
    let obj_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*obj_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*obj_ptr).class_id = 0;
        (*obj_ptr).parent_class_id = 0;
        // field_count tracks the logical number of fields; extra allocated slots
        // are available for dynamic property growth via js_object_set_field_by_name
        (*obj_ptr).field_count = field_count;

        // Initialize all allocated field slots to undefined (including extra padding)
        let fields_ptr = (obj_ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
    }

    let keys_arr = SHAPE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&arr) = cache.get(&shape_id) {
            return arr;
        }
        let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
        let num_keys = keys.len();
        let arr = crate::array::js_array_alloc_with_length(num_keys as u32);
        let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
        for (i, key_bytes) in keys.iter().enumerate() {
            let str_ptr = crate::string::js_string_from_bytes(
                key_bytes.as_ptr(), key_bytes.len() as u32,
            );
            let nanboxed = f64::from_bits(
                crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK)
            );
            unsafe { *elements_ptr.add(i) = nanboxed; }
        }
        cache.insert(shape_id, arr);
        arr
    });

    unsafe { (*obj_ptr).keys_array = keys_arr; }

    obj_ptr
}

/// Clone a spread source object and reserve extra physical slot capacity for additional
/// static properties. Used to implement object spread: `{ ...src, key1: val1, key2: val2 }`.
///
/// - `src_f64`: the spread source object as a NaN-boxed f64 (POINTER_TAG or raw pointer)
/// - `extra_count`: number of additional static properties — reserves physical slot capacity
///   for them, but does NOT add their keys to the keys_array upfront. Codegen is expected to
///   call `js_object_set_field_by_name` for each static prop, which correctly overwrites keys
///   that already exist in the spread source (preserving JS "last key wins" semantics) and
///   appends new keys (using the reserved capacity).
/// - `_static_keys_ptr`/`_static_keys_len`: unused (kept for ABI compat). Previously these
///   were used to pre-populate static keys in keys_array, but that created duplicate entries
///   when a static key matched an existing spread key, and the linear-scan lookup returned
///   the first (stale) match instead of the intended last-key value.
///
/// Returns the new *mut ObjectHeader as an i64 raw pointer (NOT NaN-boxed).
/// The returned object's `field_count` equals the source's field_count (NOT src + extra),
/// but the physical allocation reserves enough slots so subsequent
/// `js_object_set_field_by_name` calls have somewhere to append.
#[no_mangle]
pub unsafe extern "C" fn js_object_clone_with_extra(
    src_f64: f64,
    extra_count: u32,
    _static_keys_ptr: *const u8,
    _static_keys_len: u32,
) -> *mut ObjectHeader {
    // Extract raw pointer from NaN-boxed f64
    let src_bits = src_f64.to_bits();
    let top16 = src_bits >> 48;
    let src_raw = if top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };

    let header_size = std::mem::size_of::<ObjectHeader>();

    // If source is invalid, create an empty object with enough capacity for the static props.
    // Physical slot count = max(extra_count, 8) to match js_object_set_field_by_name's
    // alloc_limit = max(field_count, 8) expectation.
    if src_raw < 0x10000 {
        let phys_slots = std::cmp::max(extra_count, 8);
        let total_size = header_size + phys_slots as usize * 8;
        let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
        (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*new_ptr).class_id = 0;
        (*new_ptr).parent_class_id = 0;
        (*new_ptr).field_count = 0;
        let fields_ptr = (new_ptr as *mut u8).add(header_size) as *mut u64;
        for i in 0..phys_slots as usize {
            ptr::write(fields_ptr.add(i), crate::value::TAG_UNDEFINED);
        }
        // Empty keys array with capacity reserved for the static props to come.
        let new_keys_arr = crate::array::js_array_alloc(extra_count);
        (*new_ptr).keys_array = new_keys_arr;
        return new_ptr;
    }

    let src_ptr = src_raw as *const ObjectHeader;
    let src_field_count = (*src_ptr).field_count;

    // Physical slot capacity: src_field_count + extra_count, but at least max(fc, 8) to match
    // js_object_set_field's alloc_limit check. Extra slots are scratch space for subsequent
    // js_object_set_field_by_name calls.
    let phys_slots = std::cmp::max(src_field_count + extra_count, 8);
    let total_size = header_size + phys_slots as usize * 8;
    let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
    (*new_ptr).class_id = 0;
    (*new_ptr).parent_class_id = 0;
    // Logical field count starts at src's count. js_object_set_field_by_name bumps it when
    // appending new keys.
    (*new_ptr).field_count = src_field_count;

    // Copy source fields (as raw f64/u64 words — preserves NaN-boxing)
    let src_fields = (src_ptr as *const u8).add(header_size) as *const u64;
    let dst_fields = (new_ptr as *mut u8).add(header_size) as *mut u64;
    for i in 0..src_field_count as usize {
        let field_val = *src_fields.add(i);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        let cleaned = if field_val == 0x7FFD_0000_0000_0000 {
            eprintln!("[CLONE_NULL_PTR] field {} from src={:p} — replacing with undefined", i, src_ptr);
            crate::value::TAG_UNDEFINED
        } else {
            field_val
        };
        ptr::write(dst_fields.add(i), cleaned);
    }
    // Initialize scratch slots to undefined
    for i in src_field_count as usize..phys_slots as usize {
        ptr::write(dst_fields.add(i), crate::value::TAG_UNDEFINED);
    }

    // Build keys array: copy ONLY src keys. Static keys are NOT added here — codegen uses
    // js_object_set_field_by_name for each static prop, which appends new keys via
    // js_array_push. Pre-size the keys capacity to avoid immediate reallocation on append.
    let src_keys_arr = (*src_ptr).keys_array;
    let new_keys_arr = crate::array::js_array_alloc(src_field_count + extra_count);
    let new_keys_elements = (new_keys_arr as *mut u8).add(8) as *mut f64;

    if !src_keys_arr.is_null() && (src_keys_arr as usize) >= 0x10000 {
        let src_key_len = (*src_keys_arr).length as usize;
        let src_key_elements = (src_keys_arr as *const u8).add(8) as *const f64;
        let copy_count = src_key_len.min(src_field_count as usize);
        for i in 0..copy_count {
            *new_keys_elements.add(i) = *src_key_elements.add(i);
        }
        (*new_keys_arr).length = copy_count as u32;
    } else {
        (*new_keys_arr).length = 0;
    }

    (*new_ptr).keys_array = new_keys_arr;

    new_ptr
}

/// Copy all own enumerable fields from `src` into `dst`, using `js_object_set_field_by_name`
/// semantics (overwrite existing, append new). Used for multi-spread object literals like
/// `{...a, ...b}` to apply each additional spread after the first has been cloned via
/// `js_object_clone_with_extra`.
#[no_mangle]
pub unsafe extern "C" fn js_object_copy_own_fields(dst_i64: i64, src_f64: f64) {
    // Extract dst pointer (may be NaN-boxed or raw)
    let dst_bits = dst_i64 as u64;
    let dst_top16 = dst_bits >> 48;
    let dst_raw = if dst_top16 >= 0x7FF8 {
        (dst_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        dst_bits as usize
    };
    if dst_raw < 0x10000 {
        return;
    }
    let dst = dst_raw as *mut ObjectHeader;

    // Extract src pointer (NaN-boxed f64)
    let src_bits = src_f64.to_bits();
    let src_top16 = src_bits >> 48;
    let src_raw = if src_top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };
    if src_raw < 0x10000 {
        return;
    }
    let src = src_raw as *const ObjectHeader;

    // Iterate src's keys and copy each value via set_field_by_name.
    let src_keys = (*src).keys_array;
    if src_keys.is_null() || (src_keys as usize) < 0x10000 {
        return;
    }
    let key_count = crate::array::js_array_length(src_keys) as usize;
    let src_field_count = (*src).field_count as usize;
    let header_size = std::mem::size_of::<ObjectHeader>();
    let src_fields = (src as *const u8).add(header_size) as *const u64;

    for i in 0..key_count.min(src_field_count) {
        let key_val = crate::array::js_array_get(src_keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let key_ptr = key_val.as_string_ptr();
        let field_bits = *src_fields.add(i);
        let field_f64 = f64::from_bits(field_bits);
        js_object_set_field_by_name(dst, key_ptr, field_f64);
    }
}

/// Get a field from an object by index
#[no_mangle]
pub extern "C" fn js_object_get_field(obj: *const ObjectHeader, field_index: u32) -> JSValue {
    let obj = { let b = obj as usize; let t = b >> 48; if t >= 0x7FF8 { if t == 0x7FFC || (b & 0x0000_FFFF_FFFF_FFFF) == 0 || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000 { return JSValue::undefined(); } (b & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader } else { obj } };
    if obj.is_null() || (obj as usize) < 0x10000 { return JSValue::undefined(); }
    unsafe {
        // Bounds check: check inline fields first, then overflow map
        let fc = (*obj).field_count;
        if field_index >= fc {
            // Check overflow map for fields that didn't fit in inline storage
            let overflow_val = OVERFLOW_FIELDS.with(|m| {
                m.borrow().get(&(obj as usize)).and_then(|fields| fields.get(&(field_index as usize)).copied())
            });
            return match overflow_val {
                Some(bits) => JSValue::from_bits(bits),
                None => JSValue::undefined(),
            };
        }
        // Guard: corrupted objects with unreasonably large field_count
        if fc > 10000 {
            return JSValue::undefined();
        }
        let fields_ptr = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
        let val = *fields_ptr.add(field_index as usize);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        if val.bits() == 0x7FFD_0000_0000_0000 {
            eprintln!("[NULL_PTR_FIELD_GET] obj={:p} field_index={} class_id={} field_count={}", obj, field_index, (*obj).class_id, (*obj).field_count);
            return JSValue::undefined();
        }
        val
    }
}

/// Set a field on an object by index
#[no_mangle]
pub extern "C" fn js_object_set_field(obj: *mut ObjectHeader, field_index: u32, value: JSValue) {
    let obj = { let b = obj as usize; let t = b >> 48; if t >= 0x7FF8 { if t == 0x7FFC || (b & 0x0000_FFFF_FFFF_FFFF) == 0 || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000 { return; } (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader } else { obj } };
    if obj.is_null() || (obj as usize) < 0x10000 { return; }
    unsafe {
        // Bounds check: guard against out-of-range field writes that corrupt adjacent
        // arena allocations. js_object_alloc_with_shape uses max(field_count, 8) physical
        // slots, but the stored field_count is the logical count. Class objects from
        // js_object_alloc_class_with_keys use exactly field_count slots.
        // We use a generous limit of max(field_count, 8) to avoid false positives from
        // js_object_alloc_with_shape's extra padding while still catching real overflows.
        let stored_field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(stored_field_count, 8);
        if field_index >= alloc_limit {
            eprintln!(
                "[PERRY WARN] js_object_set_field: OOB write field_index={} alloc_limit={} (field_count={}) obj={:p} class_id={}",
                field_index, alloc_limit, stored_field_count, obj, (*obj).class_id
            );
            return;
        }
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        let vbits = value.bits();
        let value = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            eprintln!("[WARN_NULL_PTR] js_object_set_field: null POINTER_TAG at obj={:p} field_index={} class_id={} — replacing with undefined", obj, field_index, (*obj).class_id);
            JSValue::undefined()
        } else {
            value
        };
        let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        ptr::write(fields_ptr.add(field_index as usize), value);
    }
}

/// Get the class ID of an object
#[no_mangle]
pub extern "C" fn js_object_get_class_id(obj: *const ObjectHeader) -> u32 {
    unsafe { (*obj).class_id }
}

/// Free an object (for manual memory management / testing)
#[no_mangle]
pub extern "C" fn js_object_free(_obj: *mut ObjectHeader) {
    // No-op: GC handles deallocation of arena-allocated objects
}

/// Convert an object pointer to a JSValue
#[no_mangle]
pub extern "C" fn js_object_to_value(obj: *const ObjectHeader) -> JSValue {
    JSValue::pointer(obj as *const u8)
}

/// Extract an object pointer from a JSValue
#[no_mangle]
pub extern "C" fn js_value_to_object(value: JSValue) -> *mut ObjectHeader {
    value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

/// Get a field as f64 (returns raw JSValue bits as f64)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_f64(obj: *const ObjectHeader, field_index: u32) -> f64 {
    let value = js_object_get_field(obj, field_index);
    f64::from_bits(value.bits())
}

/// Set a field from f64 (interprets raw bits as JSValue)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_set_field_f64(obj: *mut ObjectHeader, field_index: u32, value: f64) {
    // Check frozen flag — frozen objects reject all writes
    if !obj.is_null() && (obj as usize) > 0x10000 {
        unsafe {
            let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return;
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Set a field by index with a raw f64 value (for dynamic object creation)
/// This is a convenience wrapper that takes field_index as u32 and value as f64
#[no_mangle]
pub extern "C" fn js_object_set_field_by_index(obj: *mut ObjectHeader, _key: *const crate::string::StringHeader, field_index: u32, value: f64) {
    if obj.is_null() { return; }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Set the keys array for an object (used for Object.keys() support)
/// The keys_array should be an array of string pointers
#[no_mangle]
pub extern "C" fn js_object_set_keys(obj: *mut ObjectHeader, keys_array: *mut ArrayHeader) {
    unsafe {
        (*obj).keys_array = keys_array;
    }
}

/// Get the keys of an object as an array of strings
/// Returns the stored keys array, or an empty array if no keys were stored
#[no_mangle]
pub extern "C" fn js_object_keys(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            // Return an empty array if no keys are stored
            crate::array::js_array_alloc(0)
        } else {
            keys
        }
    }
}

/// Get the values of an object as an array
/// Returns an array of the object's field values
#[no_mangle]
pub extern "C" fn js_object_values(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let field_count = (*obj).field_count as usize;
        let result = crate::array::js_array_alloc(field_count as u32);

        for i in 0..field_count {
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
            // Store the raw f64 bits (which may be NaN-boxed)
            crate::array::js_array_push_f64(result, f64::from_bits(value.bits()));
        }

        result
    }
}

/// Get the entries of an object as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_object_entries(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let keys = (*obj).keys_array;
        let field_count = (*obj).field_count as usize;
        let result = crate::array::js_array_alloc(field_count as u32);

        for i in 0..field_count {
            // Create a pair array [key, value]
            let pair = crate::array::js_array_alloc(2);

            // Get the key (from keys array if available)
            if !keys.is_null() && (i as u32) < crate::array::js_array_length(keys) {
                let key = crate::array::js_array_get_f64(keys, i as u32);
                crate::array::js_array_push_f64(pair, key);
            } else {
                // No key available, use empty string
                crate::array::js_array_push_f64(pair, 0.0);
            }

            // Get the value
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
            crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));

            // Push the pair to result (NaN-box the array pointer)
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            crate::array::js_array_push_f64(result, pair_boxed);
        }

        result
    }
}

/// Check if a property exists in an object by its string key name
/// Returns NaN-boxed true if the property exists, NaN-boxed false otherwise
/// This implements the JavaScript 'in' operator: "key" in obj
#[no_mangle]
pub extern "C" fn js_object_has_property(obj: f64, key: f64) -> f64 {
    let nanbox_false = f64::from_bits(0x7FFC_0000_0000_0003u64); // TAG_FALSE
    let nanbox_true = f64::from_bits(0x7FFC_0000_0000_0004u64);  // TAG_TRUE

    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());

    if !obj_val.is_pointer() {
        return nanbox_false;
    }

    let obj_ptr = obj_val.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return nanbox_false;
    }

    if !key_val.is_string() {
        return nanbox_false;
    }

    let key_str = key_val.as_string_ptr();

    unsafe {
        let keys = (*obj_ptr).keys_array;
        if keys.is_null() {
            return nanbox_false;
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let stored_key_val = crate::array::js_array_get(keys, i as u32);
            if stored_key_val.is_string() {
                let stored_key = stored_key_val.as_string_ptr();
                if crate::string::js_string_equals(key_str, stored_key) != 0 {
                    // Check if the field was deleted (set to undefined by delete operator)
                    let field_val = js_object_get_field(obj_ptr, i as u32);
                    if field_val.is_undefined() {
                        return nanbox_false;
                    }
                    return nanbox_true;
                }
            }
        }

        nanbox_false
    }
}

/// Get a field by its string key name
/// Returns the field value or undefined if the key is not found
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name(obj: *const ObjectHeader, key: *const crate::StringHeader) -> JSValue {
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as usize;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC || (raw as usize) < 0x10000 {
                // undefined/null tag, null pointer, or small handle — return undefined
                return JSValue::undefined();
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x10000 {
        return JSValue::undefined();
    }
    unsafe {
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return JSValue::undefined();
            }
        }

        // Check for CLOSURE_MAGIC at offset 12 (closures may share GC_TYPE_OBJECT arena slot)
        {
            let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
            if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
                return JSValue::undefined();
            }
        }

        let keys = (*obj).keys_array;

        if keys.is_null() {
            return JSValue::undefined();
        }

        // Validate keys_array is a real heap pointer (upper 16 bits must be 0 for ARM64/x86-64 user space).
        // If the object is actually a non-Object type (closure, array, map, etc.), keys_array at offset
        // 16 may contain garbage. An invalid upper 16-bit value catches this case defensively.
        let keys_ptr = keys as usize;
        if keys_ptr >> 48 != 0 || keys_ptr < 0x10000 {
            return JSValue::undefined();
        }

        // Extra safety: detect ASCII-like pointer values (e.g., 0x656e6f6c63 = "clone")
        // that indicate a string value leaked into the keys_array pointer field.
        // Valid ARM64 heap pointers from mmap on macOS have top_byte (bits 32-39) < 0x20.
        // NOTE: This heuristic is macOS-specific. On Linux/Android, mmap can return
        // pointers with top_byte in the printable ASCII range (0x20-0x7E), so we skip
        // this check on non-macOS platforms.
        #[cfg(target_os = "macos")]
        {
            let top_byte = (keys_ptr >> 32) as u8;
            let byte4 = ((keys_ptr >> 24) & 0xFF) as u8;
            if top_byte >= 0x20 && top_byte <= 0x7E && byte4 >= 0x20 && byte4 <= 0x7E {
                eprintln!(
                    "[PERRY WARN] js_object_get_field_by_name: ASCII-like keys_ptr=0x{:x} obj={:p} class_id={} — corrupted keys_array (heap overflow?)",
                    keys_ptr, obj, (*obj).class_id
                );
                return JSValue::undefined();
            }
        }

        // Cross-platform safety: validate keys_array has a valid GcHeader.
        // If the keys_array pointer is corrupt (e.g., due to a stale reference after GC,
        // or a func_addr relocation issue on x86_64), the GcHeader check catches it
        // before we dereference the array contents.
        {
            let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_gc_type = (*keys_gc).obj_type;
            // keys_array must be GC_TYPE_ARRAY (arena-allocated array)
            if keys_gc_type != crate::gc::GC_TYPE_ARRAY {
                return JSValue::undefined();
            }
        }

        // Fast path: check field index cache (keys_array_ptr + key_hash → field_index)
        // Objects with the same shape share the same keys_array, so we cache per-shape lookups.
        let key_bytes = std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).length as usize,
        );
        let key_hash = {
            let mut h: u32 = 0x811c9dc5;
            for &b in key_bytes {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h
        };
        let keys_id = keys as usize;

        // Thread-local inline cache: fixed-size direct-mapped cache (no allocation, no HashMap)
        // Each entry stores (keys_ptr, key_hash, field_index) for collision-safe validation
        const FIELD_CACHE_SIZE: usize = 1024;
        thread_local! {
            static FIELD_CACHE: std::cell::UnsafeCell<[(usize, u32, u32); FIELD_CACHE_SIZE]> =
                std::cell::UnsafeCell::new([(0usize, 0u32, 0u32); FIELD_CACHE_SIZE]);
        }
        let cache_idx = (keys_id.wrapping_add(key_hash as usize)) % FIELD_CACHE_SIZE;
        let cached = FIELD_CACHE.with(|c| {
            let cache = &*c.get();
            let entry = cache[cache_idx];
            if entry.0 == keys_id && entry.1 == key_hash { Some(entry.2) } else { None }
        });
        if let Some(field_idx) = cached {
            return js_object_get_field(obj, field_idx);
        }

        // Slow path: linear scan through keys array
        let key_count = crate::array::js_array_length(keys) as usize;
        let field_count = (*obj).field_count as usize;

        if key_count > 65536 {
            return JSValue::undefined();
        }

        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Cache this lookup for next time
                    FIELD_CACHE.with(|c| {
                        let cache = &mut *c.get();
                        cache[cache_idx] = (keys_id, key_hash, i as u32);
                    });
                    if i < alloc_limit {
                        return js_object_get_field(obj, i as u32);
                    } else {
                        return OVERFLOW_FIELDS.with(|m| {
                            m.borrow()
                                .get(&(obj as usize))
                                .and_then(|fields| fields.get(&i))
                                .map(|&bits| JSValue::from_bits(bits))
                                .unwrap_or(JSValue::undefined())
                        });
                    }
                }
            }
        }

        // Key not found
        JSValue::undefined()
    }
}

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const crate::StringHeader) -> f64 {
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// Set a field value by its string key name (dynamic property access)
/// This searches the keys array for a match and sets the corresponding value.
/// If the key doesn't exist, it adds it to the object.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name(obj: *mut ObjectHeader, key: *const crate::StringHeader, value: f64) {
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as usize;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                return;
            }
            if (raw as usize) < 0x10000 {
                // Small handle — dispatch to handle property set if registered
                unsafe {
                    if let Some(dispatch) = HANDLE_PROPERTY_SET_DISPATCH {
                        if !key.is_null() {
                            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).length as usize;
                            dispatch(raw as i64, name_ptr, name_len, value);
                        }
                    }
                }
                return;
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x10000 {
        // Small non-null value — could be a stripped handle (after ensure_i64 stripped NaN-box tag)
        if !obj.is_null() && (obj as usize) > 0 {
            unsafe {
                if let Some(dispatch) = HANDLE_PROPERTY_SET_DISPATCH {
                    if !key.is_null() {
                        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key).length as usize;
                        dispatch(obj as i64, name_ptr, name_len, value);
                    }
                }
            }
        }
        return;
    }
    // Safety: obj is a valid heap pointer (> 0x10000) at this point
    unsafe {
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT && gc_type != crate::gc::GC_TYPE_CLOSURE {
            // Not a heap object/closure — only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return;
            }
        }

        // Check if this is a ClosureHeader — closures support dynamic props via separate storage.
        // ClosureHeader has CLOSURE_MAGIC (0x434C4F53) at offset 12.
        // Without this check, (*obj).keys_array reads capture[0] → corruption/crash.
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).length as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    crate::closure::closure_set_dynamic_prop(obj as usize, name_str, value);
                }
            }
            return;
        }

        // Check Object.freeze/seal/preventExtensions flags
        let obj_flags = (*gc_header)._reserved;
        let is_frozen = obj_flags & crate::gc::OBJ_FLAG_FROZEN != 0;
        let is_sealed_or_no_extend = obj_flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;

        let keys = (*obj).keys_array;

        // Validate keys_array is a real heap pointer or null.
        // If the object is a non-Object type, keys at offset 16 may contain garbage.
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if keys_ptr >> 48 != 0 || keys_ptr < 0x10000 {
                // Invalid keys_array pointer — silently ignore to avoid crash
                return;
            }
        }

        // If no keys array exists, create one (adding new key)
        if keys.is_null() {
            // Frozen or sealed/non-extensible objects reject new keys
            if is_frozen || is_sealed_or_no_extend {
                return;
            }
            // Create a new keys array with the key
            let new_keys = crate::array::js_array_alloc(4);
            let new_keys = crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
            (*obj).keys_array = new_keys;

            // Reallocate fields to hold at least one value
            // Note: We assume the object has enough field slots pre-allocated
            js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
            // Bump field_count so Object.keys()/values()/entries() see the new property.
            if (*obj).field_count == 0 {
                (*obj).field_count = 1;
            }
            return;
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // Keys are stored as string pointers (NaN-boxed)
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Found it - update the field (frozen objects reject writes)
                    if is_frozen {
                        return;
                    }
                    if i < alloc_limit {
                        js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                    } else {
                        // This key was previously stored in the overflow map — update it there
                        let vbits = value.to_bits();
                        let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                            crate::value::TAG_UNDEFINED
                        } else { vbits };
                        OVERFLOW_FIELDS.with(|m| {
                            m.borrow_mut()
                                .entry(obj as usize)
                                .or_default()
                                .insert(i, vbits);
                        });
                    }
                    return;
                }
            }
        }

        // Key not found - add it to the object.
        // Frozen/sealed/non-extensible objects reject new keys
        if is_frozen || is_sealed_or_no_extend {
            return;
        }
        // CRITICAL: The keys_array may be SHARED via SHAPE_CACHE (multiple objects with
        // the same shape hash share the same keys array). We must clone it before mutating
        // to avoid corrupting other objects' keys.
        let owned_keys = if key_count == (*obj).field_count as usize {
            // Keys array matches the original shape — it's potentially shared.
            // Clone it to get an independent copy before adding new keys.
            let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
            let src_data = (keys as *const u8).add(8) as *const f64;
            let dst_data = (cloned as *mut u8).add(8) as *mut f64;
            for i in 0..key_count {
                *dst_data.add(i) = *src_data.add(i);
            }
            (*cloned).length = key_count as u32;
            (*obj).keys_array = cloned;
            cloned
        } else {
            // Already mutated — keys_array is already our own copy
            keys
        };

        // Check if we have a spare physical slot (js_object_alloc_with_shape allocates max(N,8) slots).
        // Class objects (js_object_alloc_class_with_keys) have only exactly field_count slots;
        // attempting to write to new_index = key_count would overflow into the next heap allocation.
        let new_index = key_count;
        if new_index >= alloc_limit {
            // No inline room — store in the overflow HashMap so the value is not lost.
            // Also add the key to keys_array so Object.keys() sees it.
            let vbits = value.to_bits();
            let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                eprintln!("[WARN_NULL_PTR] overflow new store: null POINTER_TAG at obj={:p} new_index={} — replacing with undefined", obj, new_index);
                crate::value::TAG_UNDEFINED
            } else { vbits };
            let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            (*obj).keys_array = new_keys;
            OVERFLOW_FIELDS.with(|m| {
                m.borrow_mut()
                    .entry(obj as usize)
                    .or_default()
                    .insert(new_index, vbits);
            });
            return;
        }
        // First, add the key to the keys array (may reallocate)
        let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
        // Update the object's keys_array pointer in case js_array_push reallocated
        (*obj).keys_array = new_keys;

        // Set the field at the new index and update logical field_count
        js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
        // Bump field_count to reflect the newly added property
        if new_index as u32 >= (*obj).field_count {
            (*obj).field_count = new_index as u32 + 1;
        }
    }
}

/// Delete a field from an object by its string key name
/// Returns 1 if the field was deleted (or didn't exist), 0 otherwise
/// Note: In strict mode, this would return 0 for non-configurable properties,
/// but we don't track configurability, so we always return 1.
#[no_mangle]
pub extern "C" fn js_object_delete_field(obj: *mut ObjectHeader, key: *const crate::StringHeader) -> i32 {
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            // No keys array means no fields to delete, but delete "succeeds" vacuously
            return 1;
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // Keys are stored as string pointers (NaN-boxed)
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Found it - set the field to undefined
                    js_object_set_field(obj, i as u32, JSValue::undefined());
                    return 1;
                }
            }
        }

        // Key not found - delete still "succeeds" for non-existent properties
        1
    }
}

/// Delete a field from an object using a dynamic key (could be string or number index)
/// For arrays, this sets the element to undefined
/// Returns 1 if successful, 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_dynamic(obj: *mut ObjectHeader, key: f64) -> i32 {
    let key_val = JSValue::from_bits(key.to_bits());

    // If the key is a string, use js_object_delete_field
    if key_val.is_string() {
        let key_str = key_val.as_string_ptr();
        return js_object_delete_field(obj, key_str);
    }

    // If the key is a number, treat as array index
    if key_val.is_number() {
        let index = key_val.as_number() as usize;
        // Try to treat it as an array and set the element to undefined
        // This is a simplified implementation - real JS delete on arrays
        // creates a hole (sparse array), but we just set to undefined
        let arr = obj as *mut crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr) as usize;
        if index < len {
            crate::array::js_array_set(arr, index as u32, JSValue::undefined());
            return 1;
        }
    }

    // For other types, delete succeeds vacuously
    1
}

/// Create a rest object from destructuring: copies all properties from src except excluded keys.
/// exclude_keys is an array of NaN-boxed string pointers (the explicitly destructured keys).
/// Returns a pointer to a new object with the remaining key-value pairs.
#[no_mangle]
pub extern "C" fn js_object_rest(src: *const ObjectHeader, exclude_keys: *const ArrayHeader) -> *mut ObjectHeader {
    if src.is_null() {
        return js_object_alloc(0, 0);
    }
    unsafe {
        let keys = (*src).keys_array;
        if keys.is_null() {
            return js_object_alloc(0, 0);
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        let exclude_count = if exclude_keys.is_null() { 0 } else { crate::array::js_array_length(exclude_keys) as usize };

        // Collect indices of keys to include (not in exclude list and not undefined/deleted)
        let mut include_indices: Vec<usize> = Vec::new();
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() { continue; }
            let key_str = key_val.as_string_ptr();

            // Check if field was deleted
            let field_val = js_object_get_field(src, i as u32);
            if field_val.is_undefined() { continue; }

            // Check if this key is in the exclude list
            let mut excluded = false;
            for j in 0..exclude_count {
                let ex_val = crate::array::js_array_get(exclude_keys, j as u32);
                if ex_val.is_string() {
                    let ex_str = ex_val.as_string_ptr();
                    if crate::string::js_string_equals(key_str, ex_str) != 0 {
                        excluded = true;
                        break;
                    }
                }
            }
            if !excluded {
                include_indices.push(i);
            }
        }

        // Allocate new object with the right number of fields
        let rest_count = include_indices.len() as u32;
        let rest_obj = js_object_alloc(0, rest_count);

        // Create keys array for the rest object
        let rest_keys = crate::array::js_array_alloc_with_length(rest_count);
        (*rest_obj).keys_array = rest_keys;

        // Copy included key-value pairs
        for (new_idx, &src_idx) in include_indices.iter().enumerate() {
            let key_val = crate::array::js_array_get(keys, src_idx as u32);
            crate::array::js_array_set(rest_keys, new_idx as u32, key_val);

            let field_val = js_object_get_field(src, src_idx as u32);
            js_object_set_field(rest_obj, new_idx as u32, field_val);
        }

        rest_obj
    }
}

/// Check if a value is an instance of a class with the given class_id
/// Walks the inheritance chain to check parent classes
/// Returns NaN-boxed TAG_TRUE / TAG_FALSE so the result identifies as a boolean.
#[no_mangle]
pub extern "C" fn js_instanceof(value: f64, class_id: u32) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let true_val = f64::from_bits(TAG_TRUE);
    let false_val = f64::from_bits(TAG_FALSE);

    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    // Special handling for Uint8Array/Buffer (class_id 0xFFFF0004)
    // Perry buffers are raw BufferHeader pointers bitcast to f64 (not NaN-boxed),
    // so the normal POINTER_TAG check doesn't work for them.
    // We use a thread-local buffer registry to identify buffer pointers.
    if class_id == crate::buffer::BUFFER_TYPE_ID {
        // Check if NaN-boxed pointer
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::buffer::is_registered_buffer(addr) {
                return true_val;
            }
        }
        // Check if raw pointer (buffer values are bitcast, not NaN-boxed)
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 {
            if crate::buffer::is_registered_buffer(bits as usize) {
                return true_val;
            }
        }
        return false_val;
    }

    // Only objects (pointers) can be instances of classes
    if !jsval.is_pointer() {
        return false_val;
    }

    // Get the object pointer
    let obj_ptr = jsval.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_val;
    }

    unsafe {
        // Check if the object's class_id matches directly
        let obj_class_id = (*obj_ptr).class_id;
        if obj_class_id == class_id {
            return true_val;
        }

        // Walk up the inheritance chain using the class registry
        let mut current_class = obj_class_id;
        while let Some(parent_id) = get_parent_class_id(current_class) {
            if parent_id == 0 {
                break;
            }
            if parent_id == class_id {
                return true_val;
            }
            current_class = parent_id;
        }

        false_val
    }
}

/// Call a method on an object with dynamic dispatch
/// This is used for runtime method calls when the method cannot be resolved statically.
/// object: NaN-boxed f64 containing an object pointer
/// method_name_ptr: pointer to the method name string (raw bytes, not StringHeader)
/// method_name_len: length of the method name
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_method to avoid symbol collision
/// with js_call_method in perry-jsruntime which handles V8 JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Get the method name (parsed early for depth guard logging)
    let method_name = if method_name_ptr.is_null() || method_name_len == 0 {
        ""
    } else {
        let bytes = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        std::str::from_utf8(bytes).unwrap_or("")
    };

    // RAII recursion depth guard: prevent stack overflow from circular module deps.
    // The guard auto-decrements on drop, covering all ~20 return points in this function.
    // When max depth is hit, return a pointer to a static empty object instead of undefined.
    // This prevents crashes when callers NaN-unbox the result and dereference it as a pointer.
    let _depth_guard = match CallMethodDepthGuard::enter(method_name) {
        Some(g) => g,
        None => {
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }
    };

    // Check if this is a JS handle (V8 object from JS runtime)
    if crate::value::is_js_handle(object) {
        let func_ptr = crate::value::JS_HANDLE_CALL_METHOD.load(std::sync::atomic::Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: unsafe extern "C" fn(f64, *const i8, usize, *const f64, usize) -> f64 =
                std::mem::transmute(func_ptr);
            let result = func(object, method_name_ptr, method_name_len, args_ptr, args_len);
            return result;
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    let jsval = JSValue::from_bits(object.to_bits());

    // Handle BigInt method calls (NaN-boxed with BIGINT_TAG 0x7FFA)
    if jsval.is_bigint() {
        let bigint_ptr = crate::bigint::clean_bigint_ptr(
            (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader
        );
        match method_name {
            "isZero" => {
                let result = crate::bigint::js_bigint_is_zero(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "isNeg" | "isNegative" => {
                let result = crate::bigint::js_bigint_is_negative(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "toNumber" => {
                return crate::bigint::js_bigint_to_f64(bigint_ptr);
            }
            "toString" => {
                let result_ptr = if args_len > 0 && !args_ptr.is_null() {
                    let radix_f64 = *args_ptr;
                    let radix = radix_f64 as i32;
                    crate::bigint::js_bigint_to_string_radix(bigint_ptr, radix)
                } else {
                    crate::bigint::js_bigint_to_string(bigint_ptr)
                };
                return f64::from_bits(JSValue::string_ptr(result_ptr).bits());
            }
            "add" | "sub" | "mul" | "div" | "mod" | "umod" | "pow"
            | "and" | "or" | "xor" | "shln" | "shrn" | "maskn"
            | "eq" | "lt" | "lte" | "gt" | "gte" | "cmp"
            | "fromTwos" | "toTwos" => {
                return dispatch_bigint_binary_method(bigint_ptr, method_name, args_ptr, args_len);
            }
            _ => {
                // Unknown BigInt method - fall through to general dispatch
            }
        }
    }

    // Check for raw handle integer: Perry may bit-cast an i64 handle directly to f64,
    // producing a subnormal float (bits == handle_id, no NaN-box tag). Values 0 < bits < 0x100000
    // with no tag are raw handle IDs from Perry's integer-typed handle parameters.
    let raw_bits = object.to_bits();
    if raw_bits > 0 && raw_bits < 0x100000 {
        if let Some(dispatch) = HANDLE_METHOD_DISPATCH {
            return dispatch(
                raw_bits as i64,
                method_name.as_ptr(),
                method_name.len(),
                args_ptr,
                args_len,
            );
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    // Handles are used by Fastify, ioredis, and other native modules that store
    // objects in a registry and use integer IDs to reference them.
    if jsval.is_pointer() {
        let raw_ptr = jsval.as_pointer::<u8>() as usize;
        if raw_ptr > 0 && raw_ptr < 0x100000 {
            // This is a handle, not a real memory pointer - dispatch to stdlib
            if let Some(dispatch) = HANDLE_METHOD_DISPATCH {
                return dispatch(
                    raw_ptr as i64,
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                );
            }
            // No dispatcher registered, return undefined
            return f64::from_bits(0x7FF8_0000_0000_0001);
        }

        // Guard: null pointer (raw_ptr == 0) means null POINTER_TAG (0x7FFD_0000_0000_0000)
        // Produced by codegen bugs (uninitialized I64 NaN-boxed). Return undefined instead of crashing.
        if raw_ptr == 0 {
            eprintln!("[NULL_PTR_METHOD_CALL] js_native_call_method: null pointer object for method '{}'", method_name);
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Check if this is a native module namespace object (e.g., fs, os, path)
        let obj = jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader to confirm this is actually an object before reading class_id
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
            }

            // Scan object fields for a callable property (closure stored via IndexSet)
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if keys_ptr >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_key = crate::string::js_string_from_bytes(
                            method_name.as_ptr(),
                            method_name.len() as u32,
                        );
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if key_val.is_string() {
                                let stored_key = key_val.as_string_ptr();
                                if crate::string::js_string_equals(method_key, stored_key) != 0 {
                                    let field_val = js_object_get_field(obj as *mut _, i as u32);
                                    if field_val.is_pointer() {
                                        return crate::closure::js_native_call_value(
                                            f64::from_bits(field_val.bits()),
                                            args_ptr,
                                            args_len,
                                        );
                                    }
                                    // Field found but not callable — return the value as-is
                                    return f64::from_bits(field_val.bits());
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup for class instances
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        if let Some(vtable) = reg.get(&class_id) {
                            if let Some(entry) = vtable.methods.get(method_name) {
                                let this_i64 = jsval.as_pointer::<u8>() as i64;
                                return call_vtable_method(
                                    entry.func_ptr, this_i64,
                                    args_ptr, args_len, entry.param_count,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Check Map/Set registries for raw or NaN-boxed pointers.
    // Maps/Sets are allocated with plain alloc (no GcHeader), so they can't be
    // dispatched through the ObjectHeader path below.
    {
        let check_ptr = if jsval.is_pointer() {
            (raw_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if !object.is_nan() && raw_bits >= 0x100000 && (raw_bits >> 48) == 0 {
            raw_bits as usize
        } else {
            0
        };
        if check_ptr >= 0x10000 {
            if crate::map::is_registered_map(check_ptr) {
                let map = check_ptr as *mut crate::map::MapHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
                    "get" if !args.is_empty() => crate::map::js_map_get(map, args[0]),
                    "set" if args.len() >= 2 => {
                        let result = crate::map::js_map_set(map, args[0], args[1]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => crate::map::js_map_has(map, args[0]) as f64,
                    "delete" if !args.is_empty() => crate::map::js_map_delete(map, args[0]) as f64,
                    "clear" => { crate::map::js_map_clear(map); f64::from_bits(crate::value::TAG_UNDEFINED) }
                    "size" => crate::map::js_map_size(map) as f64,
                    "entries" => f64::from_bits(JSValue::pointer(crate::map::js_map_entries(map) as *mut u8).bits()),
                    "keys" => f64::from_bits(JSValue::pointer(crate::map::js_map_keys(map) as *mut u8).bits()),
                    "values" => f64::from_bits(JSValue::pointer(crate::map::js_map_values(map) as *mut u8).bits()),
                    "forEach" if !args.is_empty() => { crate::map::js_map_foreach(map, args[0]); f64::from_bits(crate::value::TAG_UNDEFINED) }
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            if crate::set::is_registered_set(check_ptr) {
                let set = check_ptr as *mut crate::set::SetHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
                    "add" if !args.is_empty() => {
                        let result = crate::set::js_set_add(set, args[0]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => crate::set::js_set_has(set, args[0]) as f64,
                    "delete" if !args.is_empty() => crate::set::js_set_delete(set, args[0]) as f64,
                    "clear" => { crate::set::js_set_clear(set); f64::from_bits(crate::value::TAG_UNDEFINED) }
                    "size" => crate::set::js_set_size(set) as f64,
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
        }
    }

    // Handle raw pointer values without NaN-box tags.
    // Perry sometimes bitcasts I64 pointers to F64 without NaN-boxing (POINTER_TAG).
    // These appear as subnormal floats with bits in the valid heap address range
    // (0x100000 .. 0x0000_FFFF_FFFF_FFFF, upper 16 bits = 0).
    if !jsval.is_pointer() && !object.is_nan() && raw_bits >= 0x100000 && (raw_bits >> 48) == 0 {
        // Looks like a raw heap pointer — re-wrap as POINTER_TAG and retry
        let reboxed = f64::from_bits(0x7FFD_0000_0000_0000u64 | raw_bits);
        let reboxed_jsval = JSValue::from_bits(reboxed.to_bits());
        let obj = reboxed_jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader before accessing
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            // Check for native module namespace
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
            }

            // Field name scan on this object
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if keys_ptr >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_key = crate::string::js_string_from_bytes(
                            method_name.as_ptr(),
                            method_name.len() as u32,
                        );
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if key_val.is_string() {
                                let stored_key = key_val.as_string_ptr();
                                if crate::string::js_string_equals(method_key, stored_key) != 0 {
                                    let field_val = js_object_get_field(obj as *mut _, i as u32);
                                    if field_val.is_pointer() {
                                        return crate::closure::js_native_call_value(
                                            f64::from_bits(field_val.bits()),
                                            args_ptr,
                                            args_len,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        if let Some(vtable) = reg.get(&class_id) {
                            if let Some(entry) = vtable.methods.get(method_name) {
                                let this_i64 = raw_bits as i64;
                                return call_vtable_method(
                                    entry.func_ptr, this_i64,
                                    args_ptr, args_len, entry.param_count,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Handle common method calls
    match method_name {
        // Function.prototype.bind - returns the same function for native closures
        // This is a simplification - real bind() creates a new function with bound 'this'
        "bind" => {
            // For native closures, we return the function as-is
            // The 'this' binding is handled at the call site
            return object;
        }

        // Common string methods on string values
        "toString" => {
            if jsval.is_string() {
                return object;
            } else if jsval.is_bigint() {
                let ptr = jsval.as_bigint_ptr();
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_number() {
                let n = jsval.as_number();
                let s = if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    (n as i64).to_string()
                } else {
                    n.to_string()
                };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_bool() {
                let s = if jsval.as_bool() { "true" } else { "false" };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_undefined() {
                let s = "undefined";
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_null() {
                let s = "null";
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            }
        }

        // Array methods - delegate to array runtime
        "push" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            if args_len > 0 && !args_ptr.is_null() {
                let val = *args_ptr;
                crate::array::js_array_push_f64(arr, val);
            }
            return crate::array::js_array_length(arr) as f64;
        }
        "pop" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            return crate::array::js_array_pop_f64(arr);
        }
        "length" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>();
            return crate::array::js_array_length(arr) as f64;
        }

        _ => {}
    }

    // If it's an object with a method stored as a closure in a field,
    // try to find and call it
    if jsval.is_pointer() {
        let obj = jsval.as_pointer::<ObjectHeader>();

        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            // Only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }

        // Check for CLOSURE_MAGIC at offset 12 — closures have different layout
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }

        let keys = (*obj).keys_array;

        if !keys.is_null() {
            // Validate keys_array pointer before dereferencing
            let keys_ptr = keys as usize;
            if keys_ptr >> 48 != 0 || keys_ptr < 0x10000 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            // Detect ASCII-like pointer values (corrupted keys_array) — macOS only
            // On Linux/Android, valid mmap pointers can have bytes 32-39 in ASCII range
            #[cfg(target_os = "macos")]
            {
                let top_byte = (keys_ptr >> 32) as u8;
                let byte4 = ((keys_ptr >> 24) & 0xFF) as u8;
                if top_byte >= 0x20 && top_byte <= 0x7E && byte4 >= 0x20 && byte4 <= 0x7E {
                    let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                    return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
                }
            }

            // Search for the method in the object's fields
            let key_count = crate::array::js_array_length(keys) as usize;
            // Sanity check key_count
            if key_count > 65536 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            let method_key = crate::string::js_string_from_bytes(
                method_name.as_ptr(),
                method_name.len() as u32,
            );

            for i in 0..key_count {
                let key_val = crate::array::js_array_get(keys, i as u32);
                if key_val.is_string() {
                    let stored_key = key_val.as_string_ptr();
                    if crate::string::js_string_equals(method_key, stored_key) != 0 {
                        // Found the method - get it and call it if it's a closure
                        let field_val = js_object_get_field(obj as *mut _, i as u32);
                        if field_val.is_pointer() {
                            // Assume it's a closure and call it
                            return crate::closure::js_native_call_value(
                                f64::from_bits(field_val.bits()),
                                args_ptr,
                                args_len,
                            );
                        }
                    }
                }
            }
        }

        // Vtable lookup: check if this class has a registered method in the vtable
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    if let Some(vtable) = reg.get(&class_id) {
                        if let Some(entry) = vtable.methods.get(method_name) {
                            let this_i64 = jsval.as_pointer::<u8>() as i64;
                            return call_vtable_method(
                                entry.func_ptr, this_i64,
                                args_ptr, args_len, entry.param_count,
                            );
                        }
                    }
                }
            }
        }
    }

    // Method not found — return a safe "null object" pointer instead of undefined.
    // The generated code often NaN-unboxes the result as a pointer and dereferences it
    // without null-checking. Returning undefined (0x7FFC000000000001) unboxes to null,
    // causing a SIGSEGV. A null object with field_count=0 is safe to dereference.
    let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
    f64::from_bits(JSValue::pointer(null_obj_ptr).bits())
}

/// Dispatch a method call on a native module namespace object.
/// Extracts the module name from the object and dispatches to the appropriate
/// runtime function based on (module_name, method_name).
unsafe fn dispatch_native_module_method(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract the module name from field 0 of the namespace object
    let module_field = js_object_get_field(obj as *mut _, 0);
    let module_name = if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).length as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    };

    // Helper: get arg N as f64
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() { *args_ptr.add(n) } else { f64::from_bits(JSValue::undefined().bits()) }
    };

    // Helper: extract raw string pointer from a NaN-boxed f64 value
    let arg_str_ptr = |n: usize| -> *const crate::StringHeader {
        let v = arg(n);
        let jsv = JSValue::from_bits(v.to_bits());
        if jsv.is_string() {
            jsv.as_string_ptr()
        } else {
            std::ptr::null()
        }
    };

    // Helper: convert i32 boolean to NaN-boxed TAG_TRUE / TAG_FALSE
    let bool_to_f64 = |v: i32| -> f64 {
        if v != 0 {
            f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
        }
    };

    // Helper: convert *mut StringHeader to NaN-boxed string f64
    let str_to_f64 = |ptr: *mut crate::StringHeader| -> f64 {
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };

    match (module_name, method_name) {
        // ── fs module (args are NaN-boxed f64, booleans return as i32→f64) ──
        ("fs", "existsSync") => bool_to_f64(crate::fs::js_fs_exists_sync(arg(0))),
        ("fs", "readFileSync") => str_to_f64(crate::fs::js_fs_read_file_sync(arg(0))),
        ("fs", "writeFileSync") => bool_to_f64(crate::fs::js_fs_write_file_sync(arg(0), arg(1))),
        ("fs", "appendFileSync") => bool_to_f64(crate::fs::js_fs_append_file_sync(arg(0), arg(1))),
        ("fs", "mkdirSync") => bool_to_f64(crate::fs::js_fs_mkdir_sync(arg(0))),
        ("fs", "unlinkSync") => bool_to_f64(crate::fs::js_fs_unlink_sync(arg(0))),
        ("fs", "readdirSync") => crate::fs::js_fs_readdir_sync(arg(0)),
        ("fs", "isDirectory") => bool_to_f64(crate::fs::js_fs_is_directory(arg(0))),

        // ── os module (no args, return string or f64) ──
        ("os", "tmpdir") => str_to_f64(crate::os::js_os_tmpdir()),
        ("os", "homedir") => str_to_f64(crate::os::js_os_homedir()),
        ("os", "platform") => str_to_f64(crate::os::js_os_platform()),
        ("os", "arch") => str_to_f64(crate::os::js_os_arch()),
        ("os", "hostname") => str_to_f64(crate::os::js_os_hostname()),
        ("os", "type") => str_to_f64(crate::os::js_os_type()),
        ("os", "release") => str_to_f64(crate::os::js_os_release()),
        ("os", "eol") => str_to_f64(crate::os::js_os_eol()),
        ("os", "totalmem") => crate::os::js_os_totalmem(),
        ("os", "freemem") => crate::os::js_os_freemem(),
        ("os", "uptime") => crate::os::js_os_uptime(),

        // ── path module (args are NaN-boxed strings → extract raw StringHeader ptr) ──
        ("path", "dirname") => str_to_f64(crate::path::js_path_dirname(arg_str_ptr(0))),
        ("path", "basename") => str_to_f64(crate::path::js_path_basename(arg_str_ptr(0))),
        ("path", "extname") => str_to_f64(crate::path::js_path_extname(arg_str_ptr(0))),
        ("path", "resolve") => str_to_f64(crate::path::js_path_resolve(arg_str_ptr(0))),
        ("path", "join") => str_to_f64(crate::path::js_path_join(arg_str_ptr(0), arg_str_ptr(1))),
        ("path", "isAbsolute") => bool_to_f64(crate::path::js_path_is_absolute(arg_str_ptr(0))),

        _ => {
            // Method not found on native module — return undefined
            f64::from_bits(JSValue::undefined().bits())
        }
    }
}

/// Special class ID for native module namespace objects
/// This is used to identify objects that represent native module namespaces
pub const NATIVE_MODULE_CLASS_ID: u32 = 0xFFFFFFFE;

/// Create a native module namespace object
/// This is used for `import * as X from 'module'` patterns
/// The returned object identifies itself as an object (typeof returns "object")
/// and stores the module name for debugging purposes
///
/// module_name_ptr: pointer to the module name string bytes
/// module_name_len: length of the module name
/// Returns the object as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_create_native_module_namespace(module_name_ptr: *const u8, module_name_len: usize) -> f64 {
    // Create an object with one field to store the module name
    let obj = js_object_alloc(NATIVE_MODULE_CLASS_ID, 1);

    unsafe {
        // Create a string from the module name
        let module_name = crate::string::js_string_from_bytes(module_name_ptr, module_name_len as u32);

        // Store the module name in the first field
        js_object_set_field(obj, 0, JSValue::string_ptr(module_name));

        // Create a keys array with one key: "__module__"
        let keys_array = crate::array::js_array_alloc(1);
        let key_bytes = b"__module__";
        let key_str = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        crate::array::js_array_push(keys_array, JSValue::string_ptr(key_str));
        js_object_set_keys(obj, keys_array);
    }

    // Return as NaN-boxed pointer
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Access a property on a native module namespace object.
/// For method references (e.g., `fs.existsSync`), creates a bound method closure.
/// For constant properties (e.g., `path.sep`, `fs.constants`), returns the value directly.
#[no_mangle]
pub extern "C" fn js_native_module_bind_method(
    namespace_obj: f64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let property_name = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(property_name_ptr, property_name_len))
    };

    // Extract module name from the namespace object's first field
    let module_name = unsafe { get_module_name_from_namespace(namespace_obj) };

    // Check for known constant properties first
    if let Some(val) = unsafe { get_native_module_constant(module_name, property_name, namespace_obj) } {
        return val;
    }

    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }

    // Not a constant — create a bound method closure
    let heap_name = unsafe {
        let layout = std::alloc::Layout::from_size_align(property_name_len, 1).unwrap();
        let ptr = std::alloc::alloc(layout);
        std::ptr::copy_nonoverlapping(property_name_ptr, ptr, property_name_len);
        ptr
    };

    let closure = crate::closure::js_closure_alloc(
        crate::closure::BOUND_METHOD_FUNC_PTR,
        3,
    );
    crate::closure::js_closure_set_capture_f64(closure, 0, namespace_obj);
    crate::closure::js_closure_set_capture_ptr(closure, 1, heap_name as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, property_name_len as i64);

    crate::value::js_nanbox_pointer(closure as i64)
}

/// Extract the module name string from a native module namespace object.
unsafe fn get_module_name_from_namespace(namespace_obj: f64) -> &'static str {
    let jsval = JSValue::from_bits(namespace_obj.to_bits());
    if !jsval.is_pointer() {
        return "";
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x1000 {
        return "";
    }
    let module_field = js_object_get_field(obj as *mut _, 0);
    if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).length as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    }
}

/// Return constant (non-method) property values for native modules.
/// Returns None for method names, which should create bound closures instead.
unsafe fn get_native_module_constant(
    module_name: &str,
    property: &str,
    namespace_obj: f64,
) -> Option<f64> {
    let str_val = |s: &str| -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };

    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        { 0x0100 as f64 }
        #[cfg(target_os = "linux")]
        { 0x20000 as f64 }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        { 0x0100 as f64 }
    };

    // Helper for fs constants — shared between "fs" and "fs.constants" modules.
    // Using a nested match (module first, then property) instead of OR patterns
    // on tuples, because rustc's match optimizer can miscompile tuple OR patterns
    // by absorbing one alternative's entries into the other branch's decision tree.
    let fs_const = |prop: &str| -> Option<f64> {
        match prop {
            "F_OK" => Some(0.0),
            "R_OK" => Some(4.0),
            "W_OK" => Some(2.0),
            "X_OK" => Some(1.0),
            "O_RDONLY" => Some(0.0),
            "O_WRONLY" => Some(1.0),
            "O_RDWR" => Some(2.0),
            "O_NOFOLLOW" => Some(o_nofollow),
            "O_CREAT" => Some(0x200 as f64),
            "O_TRUNC" => Some(0x400 as f64),
            "O_APPEND" => Some(0x8 as f64),
            "O_EXCL" => Some(0x800 as f64),
            "COPYFILE_EXCL" => Some(1.0),
            "COPYFILE_FICLONE" => Some(2.0),
            "COPYFILE_FICLONE_FORCE" => Some(4.0),
            "S_IRUSR" => Some(0o400 as f64),
            "S_IWUSR" => Some(0o200 as f64),
            "S_IXUSR" => Some(0o100 as f64),
            "S_IRGRP" => Some(0o040 as f64),
            "S_IWGRP" => Some(0o020 as f64),
            "S_IXGRP" => Some(0o010 as f64),
            "S_IROTH" => Some(0o004 as f64),
            "S_IWOTH" => Some(0o002 as f64),
            "S_IXOTH" => Some(0o001 as f64),
            _ => None,
        }
    };

    match module_name {
        "path" => match property {
            "sep" => if cfg!(windows) { Some(str_val("\\")) } else { Some(str_val("/")) },
            "delimiter" => if cfg!(windows) { Some(str_val(";")) } else { Some(str_val(":")) },
            "posix" => Some(create_sub_namespace("path.posix")),
            "win32" => Some(create_sub_namespace("path.win32")),
            _ => None,
        },
        "path.posix" => match property {
            "sep" => Some(str_val("/")),
            "delimiter" => Some(str_val(":")),
            _ => None,
        },
        "path.win32" => match property {
            "sep" => Some(str_val("\\")),
            "delimiter" => Some(str_val(";")),
            _ => None,
        },
        "fs" => match property {
            "constants" => Some(create_sub_namespace("fs.constants")),
            _ => fs_const(property),
        },
        "fs.constants" => fs_const(property),
        "os" => match property {
            "EOL" => if cfg!(windows) { Some(str_val("\r\n")) } else { Some(str_val("\n")) },
            "constants" => Some(create_sub_namespace("os.constants")),
            _ => None,
        },
        _ => None,
    }
}

/// Create a NativeModuleRef sub-namespace (e.g. "fs.constants", "path.posix").
/// The compiled code treats the result as another NativeModuleRef, so chained
/// property accesses like `fs.constants.O_RDONLY` work through the dispatch table.
fn create_sub_namespace(name: &str) -> f64 {
    js_create_native_module_namespace(name.as_ptr(), name.len())
}

/// Create (and cache) the fs.constants object with POSIX file system constants.
unsafe fn create_fs_constants_object() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CACHED: AtomicU64 = AtomicU64::new(0);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    // POSIX file-access constants
    let field_names: &[&str] = &[
        "F_OK", "R_OK", "W_OK", "X_OK",
        "O_RDONLY", "O_WRONLY", "O_RDWR",
        "O_NOFOLLOW", "COPYFILE_EXCL",
    ];
    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        { 0x0100 as f64 }
        #[cfg(target_os = "linux")]
        { 0x20000 as f64 }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        { 0x0100 as f64 }
    };
    let field_values: &[f64] = &[
        0.0, 4.0, 2.0, 1.0,   // F_OK, R_OK, W_OK, X_OK
        0.0, 1.0, 2.0,        // O_RDONLY, O_WRONLY, O_RDWR
        o_nofollow,            // O_NOFOLLOW
        1.0,                   // COPYFILE_EXCL
    ];

    // Build null-separated packed keys: "F_OK\0R_OK\0..."
    let packed = field_names.join("\0");
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF01, // unique shape_id for fs.constants
        field_names.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );

    for (i, &val) in field_values.iter().enumerate() {
        js_object_set_field(obj, i as u32, JSValue::number(val));
    }

    let result = crate::value::js_nanbox_pointer(obj as i64);
    CACHED.store(result.to_bits(), Ordering::Relaxed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_alloc_and_fields() {
        let obj = js_object_alloc(1, 3);

        // Check header
        assert_eq!(js_object_get_class_id(obj), 1);

        // Fields should be undefined initially
        let f0 = js_object_get_field(obj, 0);
        assert!(f0.is_undefined());

        // Set and get a field
        js_object_set_field(obj, 0, JSValue::number(42.0));
        let f0 = js_object_get_field(obj, 0);
        assert!(f0.is_number());
        assert_eq!(f0.as_number(), 42.0);

        // Set another field
        js_object_set_field(obj, 2, JSValue::bool(true));
        let f2 = js_object_get_field(obj, 2);
        assert!(f2.is_bool());
        assert!(f2.as_bool());

        // Clean up
        js_object_free(obj);
    }

    #[test]
    fn test_object_to_value_roundtrip() {
        let obj = js_object_alloc(5, 2);
        js_object_set_field(obj, 0, JSValue::number(123.0));

        let value = js_object_to_value(obj);
        assert!(value.is_pointer());

        let obj2 = js_value_to_object(value);
        assert_eq!(js_object_get_class_id(obj2), 5);

        let f0 = js_object_get_field(obj2, 0);
        assert_eq!(f0.as_number(), 123.0);

        js_object_free(obj);
    }
}

/// Dispatch BigInt binary methods (add, sub, mul, div, mod, etc.)
/// Called from js_native_call_method when object is BIGINT_TAG.
unsafe fn dispatch_bigint_binary_method(
    a: *const crate::bigint::BigIntHeader,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract second operand from args (if any)
    let b = if args_len > 0 && !args_ptr.is_null() {
        let arg_f64 = *args_ptr;
        let arg_jsval = JSValue::from_bits(arg_f64.to_bits());
        if arg_jsval.is_bigint() {
            crate::bigint::clean_bigint_ptr(
                (arg_f64.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader
            )
        } else {
            // Try to convert number to BigInt
            crate::bigint::js_bigint_from_f64(arg_f64)
        }
    } else {
        std::ptr::null()
    };

    match method {
        // Binary arithmetic → returns BigInt
        "add" => {
            let result = crate::bigint::js_bigint_add(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "sub" => {
            let result = crate::bigint::js_bigint_sub(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "mul" => {
            let result = crate::bigint::js_bigint_mul(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "div" => {
            let result = crate::bigint::js_bigint_div(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "mod" | "umod" => {
            let result = crate::bigint::js_bigint_mod(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "pow" => {
            let result = crate::bigint::js_bigint_pow(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "and" => {
            let result = crate::bigint::js_bigint_and(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "or" => {
            let result = crate::bigint::js_bigint_or(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "xor" => {
            let result = crate::bigint::js_bigint_xor(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "shln" => {
            let result = crate::bigint::js_bigint_shl(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "shrn" => {
            let result = crate::bigint::js_bigint_shr(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "maskn" => {
            // maskn(bits) — mask to lowest N bits
            let result = crate::bigint::js_bigint_and(a, b); // approximate
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        // Comparison → returns boolean/number
        "eq" => {
            let result = crate::bigint::js_bigint_eq(a, b);
            return f64::from_bits(JSValue::bool(result != 0).bits());
        }
        "lt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result < 0).bits());
        }
        "lte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result <= 0).bits());
        }
        "gt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result > 0).bits());
        }
        "gte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result >= 0).bits());
        }
        "cmp" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return result as f64;
        }
        "fromTwos" | "toTwos" => {
            // TODO: implement proper two's complement conversion
            return f64::from_bits(JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits());
        }
        _ => {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
}

/// Object.fromEntries(entries) — build an object from an array of [key, value] pairs or a Map.
/// `entries` is an array of arrays, or a Map. Returns a NaN-boxed pointer to a new object.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    // Extract pointer from NaN-boxed value
    let bits = entries_value.to_bits();
    let raw_ptr = if (bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        bits as *const u8
    } else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if raw_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    unsafe {
        // Check GcHeader to see if this is a Map
        let gc_header = (raw_ptr).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_MAP {
            // It's a Map — convert via js_map_entries first
            let map_ptr = raw_ptr as *const crate::map::MapHeader;
            let entries_arr = crate::map::js_map_entries(map_ptr);
            // Recursively call ourselves with the entries array (NaN-boxed pointer)
            let arr_boxed = crate::value::js_nanbox_pointer(entries_arr as i64);
            return js_object_from_entries(arr_boxed);
        }

        // It's an array of [key, value] pairs
        let arr_ptr = raw_ptr as *const ArrayHeader;
        let length = (*arr_ptr).length as usize;
        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Iterate entries: each entry is itself an array [key, value]
        let entries_data = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..length {
            let entry_val = *entries_data.add(i);
            // Get the inner entry array
            let entry_bits = entry_val.to_bits();
            let entry_arr = if (entry_bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                (entry_bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
            } else if entry_bits != 0 && entry_bits <= 0x0000_FFFF_FFFF_FFFF {
                entry_bits as *const ArrayHeader
            } else {
                continue;
            };
            if entry_arr.is_null() || (*entry_arr).length < 2 {
                continue;
            }
            let entry_data = (entry_arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let key_val = *entry_data;
            let val_val = *entry_data.add(1);
            // Convert key to string
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() { continue; }
            js_object_set_field_by_name(obj, key_str, val_val);
        }
        // Return as NaN-boxed pointer
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}

/// Object.is(a, b) — SameValue algorithm
/// Like ===, except: NaN === NaN (true) and +0 !== -0 (false).
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is(a: f64, b: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Handle NaN: SameValue treats NaN as equal to NaN
    let a_jsval = crate::JSValue::from_bits(a_bits);
    let b_jsval = crate::JSValue::from_bits(b_bits);

    if a_jsval.is_number() && b_jsval.is_number() {
        let an = a_jsval.as_number();
        let bn = b_jsval.as_number();
        if an.is_nan() && bn.is_nan() {
            return f64::from_bits(TAG_TRUE);
        }
        // Distinguish +0 / -0 by bit pattern
        if an == 0.0 && bn == 0.0 {
            if a_bits == b_bits { return f64::from_bits(TAG_TRUE); }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn { return f64::from_bits(TAG_TRUE); }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison
    if a_jsval.is_string() && b_jsval.is_string() {
        let result = crate::string::js_string_equals(
            a_jsval.as_string_ptr() as *const crate::StringHeader,
            b_jsval.as_string_ptr() as *const crate::StringHeader,
        );
        if result != 0 { return f64::from_bits(TAG_TRUE); }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits { f64::from_bits(TAG_TRUE) } else { f64::from_bits(TAG_FALSE) }
}

/// Object.hasOwn(obj, key) — check if obj has its own property `key`.
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_has_own(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let obj_jsval = crate::JSValue::from_bits(obj_value.to_bits());
    if !obj_jsval.is_pointer() {
        return f64::from_bits(TAG_FALSE);
    }
    let obj_ptr = obj_jsval.as_pointer::<ObjectHeader>() as *const ObjectHeader;
    if obj_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    // Convert key to string
    let key_str = crate::builtins::js_string_coerce(key_value);
    if key_str.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let result = js_object_get_field_by_name(obj_ptr, key_str);
    // If property exists (not undefined), return true
    if result.is_undefined() {
        f64::from_bits(TAG_FALSE)
    } else {
        f64::from_bits(TAG_TRUE)
    }
}

/// Helper: extract object pointer from NaN-boxed f64. Returns null on failure.
unsafe fn extract_obj_ptr(value: f64) -> *mut ObjectHeader {
    let jsval = crate::JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            bits as *mut ObjectHeader
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: get GcHeader for an object pointer
unsafe fn gc_header_for(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}

/// Object.defineProperty(obj, key, descriptor) — set the value from descriptor.value on obj.
/// Perry doesn't support writable/enumerable/configurable flags; this just sets the value.
/// Returns the object (NaN-boxed pointer).
#[no_mangle]
pub extern "C" fn js_object_define_property(obj_value: f64, key_value: f64, descriptor_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        // Extract descriptor object
        let desc_ptr = extract_obj_ptr(descriptor_value);
        if desc_ptr.is_null() {
            return obj_value;
        }
        // Look for "value" field in descriptor
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
        if !value_field.is_undefined() {
            js_object_set_field_by_name(obj, key_str, f64::from_bits(value_field.bits()));
        }
        // Return the object
        obj_value
    }
}

/// Object.getOwnPropertyDescriptor(obj, key) — returns { value, writable: true, enumerable: true, configurable: true }
/// or TAG_UNDEFINED if the property doesn't exist.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptor(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Look up the property
        let value = js_object_get_field_by_name(obj, key_str);
        if value.is_undefined() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Build descriptor: { value, writable: true, enumerable: true, configurable: true }
        // packed_keys = "value\0writable\0enumerable\0configurable\0"
        let packed = b"value\0writable\0enumerable\0configurable";
        let desc = js_object_alloc_with_shape(
            0x0D_E5_C0, // unique shape_id for property descriptors
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        // Set fields
        let header_size = std::mem::size_of::<ObjectHeader>();
        let fields = (desc as *mut u8).add(header_size) as *mut f64;
        *fields = f64::from_bits(value.bits()); // value
        *fields.add(1) = f64::from_bits(TAG_TRUE); // writable
        *fields.add(2) = f64::from_bits(TAG_TRUE); // enumerable
        *fields.add(3) = f64::from_bits(TAG_TRUE); // configurable
        // Return NaN-boxed pointer
        f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.getOwnPropertyNames(obj) — returns all own property names (same as keys for now).
/// Takes raw i64 pointer, returns raw i64 pointer to array.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_names(obj_ptr: i64) -> i64 {
    // Perry doesn't distinguish enumerable/non-enumerable — return all keys
    if obj_ptr == 0 || (obj_ptr as usize) < 0x10000 {
        return crate::array::js_array_alloc(0) as i64;
    }
    let obj = obj_ptr as *const ObjectHeader;
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return crate::array::js_array_alloc(0) as i64;
        }
        // Clone the keys array
        let len = crate::array::js_array_length(keys) as usize;
        let result = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            crate::array::js_array_push_f64(result, f64::from_bits(key_val.bits()));
        }
        result as i64
    }
}

/// Object.create(proto) — create empty object. Perry ignores prototype; Object.create(null) returns {}.
#[no_mangle]
pub extern "C" fn js_object_create(_proto_value: f64) -> f64 {
    let obj = js_object_alloc(0, 0);
    // Return NaN-boxed pointer
    f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.freeze(obj) — sets the frozen flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_freeze(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// Object.seal(obj) — sets the sealed flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_seal(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// Object.preventExtensions(obj) — sets the no-extend flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_prevent_extensions(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// Object.isFrozen(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_frozen(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously frozen
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isSealed(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_sealed(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously sealed
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_SEALED != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isExtensible(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_extensible(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_FALSE); // non-objects are not extensible
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            f64::from_bits(TAG_FALSE)
        } else {
            f64::from_bits(TAG_TRUE)
        }
    }
}

/// Object.getPrototypeOf(obj) — Perry doesn't have a prototype chain, returns null.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(_obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    f64::from_bits(TAG_NULL)
}
