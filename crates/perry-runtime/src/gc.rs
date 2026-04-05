//! Mark-sweep garbage collector for Perry
//!
//! Design:
//! - 8-byte GcHeader prepended to every heap allocation (invisible to callers)
//! - Arena objects (arrays/objects): discovered by walking arena blocks linearly (zero per-alloc tracking cost)
//! - Malloc objects (strings/closures/promises/bigints/errors): tracked in MALLOC_OBJECTS list
//! - Mark phase: conservative stack scan + explicit thread-local root scanning + type-specific tracing
//! - Sweep phase: free malloc objects; arena objects added to free list for reuse
//! - Trigger: only checked on new arena block allocation or explicit gc() call

use std::cell::{Cell, RefCell};
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::collections::HashSet;

/// GC header prepended to every heap allocation.
/// Callers receive a pointer AFTER this header (ptr + 8).
#[repr(C)]
pub struct GcHeader {
    /// GC_TYPE_ARRAY, GC_TYPE_STRING, etc.
    pub obj_type: u8,
    /// GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED
    pub gc_flags: u8,
    /// Reserved for future use
    pub _reserved: u16,
    /// Total allocation size (header + payload) for arena block walking
    pub size: u32,
}

pub const GC_HEADER_SIZE: usize = std::mem::size_of::<GcHeader>(); // 8 bytes

// Object type constants
pub const GC_TYPE_ARRAY: u8 = 1;
pub const GC_TYPE_OBJECT: u8 = 2;
pub const GC_TYPE_STRING: u8 = 3;
pub const GC_TYPE_CLOSURE: u8 = 4;
pub const GC_TYPE_PROMISE: u8 = 5;
pub const GC_TYPE_BIGINT: u8 = 6;
pub const GC_TYPE_ERROR: u8 = 7;
pub const GC_TYPE_MAP: u8 = 8;

// Flag constants
pub const GC_FLAG_MARKED: u8 = 0x01;
pub const GC_FLAG_ARENA: u8 = 0x02;
pub const GC_FLAG_PINNED: u8 = 0x04;

// NaN-boxing tag constants (duplicated from value.rs to avoid circular deps)
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

/// GC statistics
pub struct GcStats {
    pub collection_count: u64,
    pub total_freed_bytes: u64,
    pub last_pause_us: u64,
}

thread_local! {
    /// Malloc-allocated objects tracked for GC (strings, closures, bigints, promises, errors)
    static MALLOC_OBJECTS: RefCell<Vec<*mut GcHeader>> = RefCell::new(Vec::new());

    /// O(1) lookup set for validating malloc pointers (mirrors MALLOC_OBJECTS)
    static MALLOC_SET: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());

    /// Free list of arena slots available for reuse: (user_ptr, payload_size)
    pub(crate) static ARENA_FREE_LIST: RefCell<Vec<(*mut u8, usize)>> = RefCell::new(Vec::new());

    /// GC statistics
    static GC_STATS: RefCell<GcStats> = RefCell::new(GcStats {
        collection_count: 0,
        total_freed_bytes: 0,
        last_pause_us: 0,
    });

    /// Registered root scanner functions (promise queue, timers, etc.)
    static ROOT_SCANNERS: RefCell<Vec<fn(&mut dyn FnMut(f64))>> = RefCell::new(Vec::new());

    /// Module-level global variable addresses (registered by codegen)
    static GLOBAL_ROOTS: RefCell<Vec<*mut u64>> = RefCell::new(Vec::new());

    /// Reentrancy guard: true while gc_malloc/gc_realloc is mutating MALLOC_OBJECTS/MALLOC_SET.
    /// Prevents gc_check_trigger() from running a collection while allocation tracking is in progress,
    /// which would cause RefCell double-borrow panics (SIGABRT).
    static GC_IN_ALLOC: Cell<bool> = Cell::new(false);
}

/// Threshold: run GC when total arena bytes exceed this
const GC_THRESHOLD_BYTES: usize = 64 * 1024 * 1024; // 64MB

/// Threshold: run GC when tracked malloc objects exceed this count.
/// Prevents unbounded growth of cycle-scoped allocations (strings, closures) in
/// long-running services where arena usage stays flat (free list hits) but
/// malloc tracking accumulates. Previously GC was only triggered on arena block
/// allocation — services that never grew the arena never collected.
///
/// Tuned for backend services doing ~100-1000 RPC calls/cycle: triggers GC
/// every few cycles so memory stays bounded and glibc malloc_trim returns
/// pages to the OS promptly.
const GC_MALLOC_COUNT_THRESHOLD: usize = 10_000;

/// Allocate memory via malloc with GcHeader prepended.
/// Returns pointer to usable memory AFTER the header.
/// The allocation is tracked in MALLOC_OBJECTS.
pub fn gc_malloc(size: usize, obj_type: u8) -> *mut u8 {
    let total = GC_HEADER_SIZE + size;
    let layout = Layout::from_size_align(total, 8).unwrap();

    unsafe {
        let raw = alloc(layout);
        if raw.is_null() {
            panic!("gc_malloc: failed to allocate {} bytes", total);
        }

        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = 0; // not arena
        (*header)._reserved = 0;
        (*header).size = total as u32;

        let user_ptr = raw.add(GC_HEADER_SIZE);

        GC_IN_ALLOC.with(|f| f.set(true));
        MALLOC_OBJECTS.with(|list| {
            list.borrow_mut().push(header);
        });
        MALLOC_SET.with(|set| {
            set.borrow_mut().insert(header as usize);
        });
        GC_IN_ALLOC.with(|f| f.set(false));

        user_ptr
    }
}

/// Reallocate a malloc-tracked object, preserving GcHeader.
/// `old_user_ptr` is the pointer previously returned by gc_malloc.
/// Returns new user pointer (after header).
///
/// Safety: validates the pointer is actually tracked before dereferencing.
/// If the pointer was freed by GC or is arena-allocated, falls back to
/// fresh allocation to prevent SIGABRT from invalid realloc.
pub fn gc_realloc(old_user_ptr: *mut u8, new_payload_size: usize) -> *mut u8 {
    if old_user_ptr.is_null() {
        // Graceful fallback: allocate fresh instead of panicking
        return gc_malloc(new_payload_size, GC_TYPE_STRING);
    }

    let old_header = unsafe { old_user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader };

    // Validate the pointer is in our tracked set before dereferencing the header.
    // This prevents SIGABRT when gc_realloc is called on a pointer that was
    // freed by GC (use-after-free) or was never allocated by gc_malloc.
    let is_tracked = MALLOC_SET.with(|set| {
        set.borrow().contains(&(old_header as usize))
    });

    if !is_tracked {
        // Pointer is not tracked — it was freed by GC, is arena-allocated,
        // or was never allocated by gc_malloc. Allocate fresh.
        eprintln!("[perry] gc_realloc: untracked pointer {:p}, allocating fresh ({} bytes)",
            old_user_ptr, new_payload_size);
        return gc_malloc(new_payload_size, GC_TYPE_STRING);
    }

    // Also check arena flag — arena objects must not be passed to system realloc
    unsafe {
        if (*old_header).gc_flags & GC_FLAG_ARENA != 0 {
            eprintln!("[perry] gc_realloc: arena pointer {:p}, allocating fresh", old_user_ptr);
            let new_ptr = gc_malloc(new_payload_size, (*old_header).obj_type);
            let old_payload_size = (*old_header).size as usize - GC_HEADER_SIZE;
            let copy_size = old_payload_size.min(new_payload_size);
            std::ptr::copy_nonoverlapping(old_user_ptr, new_ptr, copy_size);
            return new_ptr;
        }
    }

    let old_total = unsafe { (*old_header).size as usize };
    let new_total = GC_HEADER_SIZE + new_payload_size;

    let old_layout = Layout::from_size_align(old_total, 8).unwrap();

    unsafe {
        let new_raw = realloc(old_header as *mut u8, old_layout, new_total);
        if new_raw.is_null() {
            panic!("gc_realloc: failed to reallocate to {} bytes", new_total);
        }

        let new_header = new_raw as *mut GcHeader;
        (*new_header).size = new_total as u32;

        // Update pointer in MALLOC_OBJECTS and MALLOC_SET if it changed
        if new_header != old_header {
            GC_IN_ALLOC.with(|f| f.set(true));
            MALLOC_OBJECTS.with(|list| {
                let mut list = list.borrow_mut();
                for ptr in list.iter_mut() {
                    if *ptr == old_header {
                        *ptr = new_header;
                        break;
                    }
                }
            });
            MALLOC_SET.with(|set| {
                let mut set = set.borrow_mut();
                set.remove(&(old_header as usize));
                set.insert(new_header as usize);
            });
            GC_IN_ALLOC.with(|f| f.set(false));
        }

        new_raw.add(GC_HEADER_SIZE)
    }
}

/// Register a root scanner function.
/// Each scanner is called during the mark phase to discover roots.
pub fn gc_register_root_scanner(scanner: fn(&mut dyn FnMut(f64))) {
    ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

/// Register a global variable address as a GC root.
/// Called by codegen in module init functions.
#[no_mangle]
pub extern "C" fn js_gc_register_global_root(ptr: i64) {
    GLOBAL_ROOTS.with(|roots| {
        roots.borrow_mut().push(ptr as *mut u64);
    });
}

/// Check if GC should run. Called only when a new arena block is allocated.
/// Skips collection if we're inside gc_malloc/gc_realloc to prevent
/// RefCell double-borrow panics (reentrancy from allocation → arena grow → GC → sweep).
pub fn gc_check_trigger() {
    if GC_IN_ALLOC.with(|f| f.get()) {
        return;
    }
    use crate::arena::arena_total_bytes;
    let total = arena_total_bytes();
    if total >= GC_THRESHOLD_BYTES {
        gc_collect_inner();
        return;
    }
    // Also trigger on malloc object count to bound memory growth for
    // services that stay within a single arena block but produce many
    // short-lived strings/closures per iteration.
    let malloc_count = MALLOC_OBJECTS.with(|list| list.borrow().len());
    if malloc_count >= GC_MALLOC_COUNT_THRESHOLD {
        gc_collect_inner();
    }
}

/// Manual GC trigger (callable from TypeScript as `gc()`)
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    gc_collect_inner();
}

/// Main GC collection
fn gc_collect_inner() {
    let start = std::time::Instant::now();

    // Build set of valid heap pointers for conservative stack scan validation
    let valid_ptrs = build_valid_pointer_set();

    // === MARK PHASE ===

    // 1. Conservative stack scan
    mark_stack_roots(&valid_ptrs);

    // 2. Scan registered global roots (module-level variables)
    mark_global_roots(&valid_ptrs);

    // 3. Run registered root scanners (promise queues, timers, etc.)
    mark_registered_roots(&valid_ptrs);

    // 4. Trace from marked roots (iterative worklist)
    trace_marked_objects(&valid_ptrs);

    // === SWEEP PHASE ===
    // sweep() now clears mark bits on surviving objects inline,
    // eliminating 2 redundant heap walks (arena + malloc).
    let freed_bytes = sweep();

    // Return released glibc heap pages to the kernel. Without this, glibc
    // keeps freed memory in its arena for reuse but never shrinks RSS, so
    // long-running services show unbounded RSS growth from transient
    // allocations (HTTP buffers, JSON parsers, etc.) even though the
    // Perry GC successfully frees the underlying objects.
    // No-op on non-glibc platforms (macOS, musl).
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::malloc_trim(0);
    }

    let elapsed_us = start.elapsed().as_micros() as u64;

    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.collection_count += 1;
        stats.total_freed_bytes += freed_bytes;
        stats.last_pause_us = elapsed_us;
    });
}

/// Build a set of all valid user-space pointers (pointers returned to callers).
/// Used to validate candidates found during conservative stack scanning.
fn build_valid_pointer_set() -> HashSet<usize> {
    let mut set = HashSet::new();

    // Arena objects: walk arena blocks
    crate::arena::arena_walk_objects(|header_ptr| {
        let user_ptr = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) };
        set.insert(user_ptr as usize);
    });

    // Malloc objects
    MALLOC_OBJECTS.with(|list| {
        let list = list.borrow();
        for &header in list.iter() {
            let user_ptr = unsafe { (header as *mut u8).add(GC_HEADER_SIZE) };
            set.insert(user_ptr as usize);
        }
    });

    set
}

/// Get the GcHeader for a user pointer (pointer returned by gc_malloc or arena_alloc_gc).
/// The header is located GC_HEADER_SIZE bytes before the user pointer.
#[inline]
unsafe fn header_from_user_ptr(user_ptr: *const u8) -> *mut GcHeader {
    (user_ptr as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader
}

/// Try to mark a value (if it's a heap pointer). Returns true if newly marked.
fn try_mark_value(value_bits: u64, valid_ptrs: &HashSet<usize>) -> bool {
    let tag = value_bits & TAG_MASK;
    let ptr_val = (value_bits & POINTER_MASK) as usize;

    // Check if this is a tagged pointer
    let is_heap_ptr = match tag {
        t if t == POINTER_TAG => true,
        t if t == STRING_TAG => true,
        t if t == BIGINT_TAG => true,
        _ => false,
    };

    if !is_heap_ptr || ptr_val == 0 {
        return false;
    }

    // Validate against known heap pointers
    if !valid_ptrs.contains(&ptr_val) {
        return false;
    }

    // Mark it
    unsafe {
        let header = header_from_user_ptr(ptr_val as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false; // Already marked
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false; // Pinned objects are always live
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
        true
    }
}

/// Conservative stack scan: scan the current thread's stack for heap pointers.
/// Handles BOTH NaN-boxed pointers (POINTER_TAG/STRING_TAG/BIGINT_TAG) AND raw I64 pointers.
/// Raw I64 pointers arise from Perry's `is_array`/`is_string`/`is_pointer`/`is_closure` local
/// variables — Cranelift stores these as raw I64 words (not NaN-boxed) in registers and on stack.
fn mark_stack_roots(valid_ptrs: &HashSet<usize>) {
    // Capture callee-saved registers into a buffer via setjmp
    let mut jmp_buf = [0u64; 32]; // oversized for safety
    unsafe {
        // Use setjmp to capture register state
        extern "C" {
            fn setjmp(env: *mut u64) -> i32;
        }
        setjmp(jmp_buf.as_mut_ptr());
    }

    // Scan the register buffer (covers callee-saved regs: x19-x28 on AArch64, rbx/rbp/r12-r15 on x86_64)
    for &word in &jmp_buf {
        try_mark_value_or_raw(word, valid_ptrs);
    }

    // Get stack bounds
    let stack_top: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let sp: u64;
        std::arch::asm!("mov {}, sp", out(reg) sp);
        stack_top = sp as usize;
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let sp: u64;
        std::arch::asm!("mov {}, rsp", out(reg) sp);
        stack_top = sp as usize;
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        // Fallback: skip stack scan on unsupported architectures
        return;
    }

    let stack_bottom = get_stack_bottom();
    if stack_bottom == 0 {
        return; // Can't determine stack bounds
    }

    // Walk the stack from current SP to stack bottom.
    // Each 8-byte word may be: NaN-boxed pointer, raw I64 heap pointer, return addr, or plain value.
    let mut addr = stack_top;
    while addr < stack_bottom {
        let word = unsafe { *(addr as *const u64) };
        try_mark_value_or_raw(word, valid_ptrs);
        addr += 8;
    }
}

/// Mark a value if it is a heap pointer — either NaN-boxed OR a raw I64 pointer.
/// Returns true if newly marked.
/// This is used for conservative scanning where Perry stores raw I64 pointers (for is_string/
/// is_array/is_pointer/is_closure vars) alongside NaN-boxed F64 values.
#[inline]
fn try_mark_value_or_raw(word: u64, valid_ptrs: &HashSet<usize>) -> bool {
    // First try NaN-boxed interpretation (POINTER_TAG / STRING_TAG / BIGINT_TAG)
    if try_mark_value(word, valid_ptrs) {
        return true;
    }
    // Fallback: treat as raw (non-NaN-boxed) heap pointer.
    // Perry's is_string/is_array/is_pointer/is_closure Cranelift locals store raw I64 addresses.
    // Validate against the known-heap-pointer set to avoid false positives from return addresses
    // and plain integers. Valid heap pointers are in the lower 48-bit address space and
    // won't have NaN-boxing tags in upper bits (already rejected above).
    let raw_ptr = word as usize;
    if raw_ptr < 0x1000 || raw_ptr > 0x0000_FFFF_FFFF_FFFF {
        return false; // Too small (null/invalid) or has upper bits set (NaN tag or non-address)
    }
    if !valid_ptrs.contains(&raw_ptr) {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(raw_ptr as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false; // Already marked
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false; // Pinned objects are always live
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
    true
}

/// Get the bottom (highest address) of the current thread's stack.
#[cfg(target_os = "macos")]
fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> *mut std::ffi::c_void;
        fn pthread_get_stackaddr_np(thread: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    unsafe {
        let thread = pthread_self();
        pthread_get_stackaddr_np(thread) as usize
    }
}

#[cfg(target_os = "linux")]
fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> usize;
        fn pthread_attr_init(attr: *mut [u64; 8]) -> i32;
        fn pthread_getattr_np(thread: usize, attr: *mut [u64; 8]) -> i32;
        fn pthread_attr_getstack(attr: *const [u64; 8], stackaddr: *mut *mut u8, stacksize: *mut usize) -> i32;
        fn pthread_attr_destroy(attr: *mut [u64; 8]) -> i32;
    }
    unsafe {
        let thread = pthread_self();
        let mut attr = [0u64; 8];
        pthread_attr_init(&mut attr);
        if pthread_getattr_np(thread, &mut attr) != 0 {
            return 0;
        }
        let mut stackaddr: *mut u8 = std::ptr::null_mut();
        let mut stacksize: usize = 0;
        pthread_attr_getstack(&attr, &mut stackaddr, &mut stacksize);
        pthread_attr_destroy(&mut attr);
        stackaddr as usize + stacksize
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_stack_bottom() -> usize {
    0 // Stack scanning not supported
}

/// Mark global roots (module-level variables registered by codegen).
fn mark_global_roots(valid_ptrs: &HashSet<usize>) {
    GLOBAL_ROOTS.with(|roots| {
        let roots = roots.borrow();
        for &root_ptr in roots.iter() {
            if !root_ptr.is_null() {
                let value = unsafe { *root_ptr };
                // First try NaN-boxed interpretation (exported globals, closures, etc.)
                if !try_mark_value(value, valid_ptrs) {
                    // Module variable globals store raw I64 pointers (not NaN-boxed).
                    // The codegen stores raw pointer values for is_pointer && !is_union types
                    // but the GC needs NaN-box tags to identify heap pointers.
                    // Try the raw value directly as a heap pointer address.
                    // This is safe: we validate against valid_ptrs (known heap allocations),
                    // and f64 number values have upper bits set (exponent) so they won't
                    // falsely match real heap addresses in the lower 48-bit address space.
                    let raw_ptr = value as usize;
                    if raw_ptr != 0 && valid_ptrs.contains(&raw_ptr) {
                        unsafe {
                            let header = header_from_user_ptr(raw_ptr as *const u8);
                            if (*header).gc_flags & GC_FLAG_MARKED == 0
                                && (*header).gc_flags & GC_FLAG_PINNED == 0
                            {
                                (*header).gc_flags |= GC_FLAG_MARKED;
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Run registered root scanners (promise queues, timers, exception state).
fn mark_registered_roots(valid_ptrs: &HashSet<usize>) {
    // Collect scanners first to avoid borrow conflicts
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());

    for scanner in scanners {
        scanner(&mut |value: f64| {
            try_mark_value(value.to_bits(), valid_ptrs);
        });
    }
}

/// Trace from marked objects: follow references iteratively using a worklist.
fn trace_marked_objects(valid_ptrs: &HashSet<usize>) {
    // Collect all currently-marked objects into a worklist
    let mut worklist: Vec<*mut GcHeader> = Vec::new();

    // Arena objects
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            if (*header).gc_flags & GC_FLAG_MARKED != 0 {
                worklist.push(header);
            }
        }
    });

    // Malloc objects
    MALLOC_OBJECTS.with(|list| {
        let list = list.borrow();
        for &header in list.iter() {
            unsafe {
                if (*header).gc_flags & GC_FLAG_MARKED != 0 {
                    worklist.push(header);
                }
            }
        }
    });

    // Process worklist
    let mut i = 0;
    while i < worklist.len() {
        let header = worklist[i];
        i += 1;

        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            match (*header).obj_type {
                GC_TYPE_ARRAY => {
                    trace_array(user_ptr, valid_ptrs, &mut worklist);
                }
                GC_TYPE_OBJECT => {
                    trace_object(user_ptr, valid_ptrs, &mut worklist);
                }
                GC_TYPE_CLOSURE => {
                    trace_closure(user_ptr, valid_ptrs, &mut worklist);
                }
                GC_TYPE_PROMISE => {
                    trace_promise(user_ptr, valid_ptrs, &mut worklist);
                }
                GC_TYPE_ERROR => {
                    trace_error(user_ptr, valid_ptrs, &mut worklist);
                }
                GC_TYPE_MAP => {
                    trace_map(user_ptr, valid_ptrs, &mut worklist);
                }
                GC_TYPE_STRING | GC_TYPE_BIGINT => {
                    // Leaf nodes - no children to trace
                }
                _ => {}
            }
        }
    }
}

/// Trace Map entries — scan all key-value pairs in the Map's entries array.
/// Maps store NaN-boxed JSValues (strings, arrays, objects) as keys and values.
/// Values may also be raw I64 pointers (for typed arrays/maps stored in maps).
unsafe fn trace_map(user_ptr: *mut u8, valid_ptrs: &HashSet<usize>, worklist: &mut Vec<*mut GcHeader>) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;

    // Sanity check
    if size > capacity || size > 100_000 {
        return;
    }

    let entries = (*map).entries as *const u64;
    if entries.is_null() {
        return;
    }

    // Each entry is 2 x f64 (key + value)
    for i in 0..(size as usize) {
        let key_bits = *entries.add(i * 2);
        let val_bits = *entries.add(i * 2 + 1);

        // Mark and trace key
        if try_mark_value_or_raw(key_bits, valid_ptrs) {
            // Newly marked — add to worklist for transitive tracing
            let ptr_val = extract_ptr_from_bits(key_bits);
            if ptr_val > 0 && valid_ptrs.contains(&ptr_val) {
                worklist.push(header_from_user_ptr(ptr_val as *const u8));
            }
        }
        // Mark and trace value
        if try_mark_value_or_raw(val_bits, valid_ptrs) {
            let ptr_val = extract_ptr_from_bits(val_bits);
            if ptr_val > 0 && valid_ptrs.contains(&ptr_val) {
                worklist.push(header_from_user_ptr(ptr_val as *const u8));
            }
        }
    }
}

/// Extract a raw pointer value from NaN-boxed or raw bits.
fn extract_ptr_from_bits(bits: u64) -> usize {
    let tag = bits & TAG_MASK;
    match tag {
        t if t == POINTER_TAG || t == STRING_TAG || t == BIGINT_TAG => {
            (bits & POINTER_MASK) as usize
        }
        _ => {
            // Raw pointer (no NaN-boxing tag)
            let raw = bits as usize;
            if raw >= 0x1000 && raw <= 0x0000_FFFF_FFFF_FFFF { raw } else { 0 }
        }
    }
}

/// Trace array elements.
/// Elements may be NaN-boxed JSValues OR raw I64 pointers (codegen stores raw I64 for
/// is_pointer/is_array/is_string typed arrays via js_array_set_jsvalue).
unsafe fn trace_array(user_ptr: *mut u8, valid_ptrs: &HashSet<usize>, worklist: &mut Vec<*mut GcHeader>) {
    let arr = user_ptr as *const crate::array::ArrayHeader;
    let length = (*arr).length;
    let capacity = (*arr).capacity;

    // Sanity checks: reject corrupt length/capacity to avoid scanning wild memory
    if length > capacity || length > 65536 {
        return;
    }

    let elements = (user_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;

    for i in 0..length as usize {
        let val_bits = *elements.add(i);
        if try_mark_value_or_raw(val_bits, valid_ptrs) {
            let tag = val_bits & TAG_MASK;
            let ptr_val = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                (val_bits & POINTER_MASK) as usize
            } else {
                val_bits as usize // raw pointer
            };
            let header = header_from_user_ptr(ptr_val as *const u8);
            worklist.push(header);
        }
    }
}

/// Trace object fields and keys array.
/// Fields may be NaN-boxed JSValues OR raw I64 pointers (codegen stores some fields as raw I64).
/// keys_array may be a raw pointer (*mut ArrayHeader) OR NaN-boxed (codegen may NaN-box it).
unsafe fn trace_object(user_ptr: *mut u8, valid_ptrs: &HashSet<usize>, worklist: &mut Vec<*mut GcHeader>) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;

    // Sanity check: reject corrupt field_count to avoid scanning wild memory.
    // Object fields start after ObjectHeader (24 bytes). Max reasonable: ~64K fields.
    if field_count > 65536 {
        return;
    }

    let fields = (user_ptr as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;

    // Trace each field — use try_mark_value_or_raw since codegen may store raw I64 pointers
    // (e.g., for is_pointer variables) alongside NaN-boxed JSValues.
    for i in 0..field_count as usize {
        let val_bits = *fields.add(i);
        if try_mark_value_or_raw(val_bits, valid_ptrs) {
            let tag = val_bits & TAG_MASK;
            let ptr_val = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                (val_bits & POINTER_MASK) as usize
            } else {
                val_bits as usize // raw pointer
            };
            let header = header_from_user_ptr(ptr_val as *const u8);
            worklist.push(header);
        }
    }

    // Trace keys_array pointer.
    // The codegen may store keys_array as either a raw pointer or a NaN-boxed POINTER_TAG value.
    // Read the raw 64-bit value and handle both cases.
    let keys_raw = (*obj).keys_array as u64;
    if keys_raw != 0 {
        // Extract the actual pointer: strip NaN-boxing tags if present
        let keys_ptr = if keys_raw >> 48 >= 0x7FF8 {
            // NaN-boxed: extract lower 48 bits as pointer
            (keys_raw & POINTER_MASK) as usize
        } else {
            keys_raw as usize
        };
        if keys_ptr != 0 && keys_ptr >= 0x1000 && valid_ptrs.contains(&keys_ptr) {
            let keys_header = header_from_user_ptr(keys_ptr as *const u8);
            if (*keys_header).gc_flags & GC_FLAG_MARKED == 0 {
                (*keys_header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(keys_header);
            }
        }
    }
}

/// Trace closure captures
/// Captures may be NaN-boxed JSValues OR raw I64 pointers bitcast to F64.
/// Perry's codegen stores `is_string`/`is_array`/`is_closure` captures as raw I64 in some paths.
unsafe fn trace_closure(user_ptr: *mut u8, valid_ptrs: &HashSet<usize>, worklist: &mut Vec<*mut GcHeader>) {
    let closure = user_ptr as *const crate::closure::ClosureHeader;
    let capture_count = crate::closure::real_capture_count((*closure).capture_count);
    let captures = (user_ptr as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *const u64;

    for i in 0..capture_count as usize {
        let val_bits = *captures.add(i);
        if try_mark_value_or_raw(val_bits, valid_ptrs) {
            // Determine the actual heap pointer: NaN-boxed uses lower 48 bits, raw uses full value
            let tag = val_bits & TAG_MASK;
            let ptr_val = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                (val_bits & POINTER_MASK) as usize
            } else {
                val_bits as usize // raw pointer
            };
            let header = header_from_user_ptr(ptr_val as *const u8);
            worklist.push(header);
        }
    }
}

/// Trace promise fields
unsafe fn trace_promise(user_ptr: *mut u8, valid_ptrs: &HashSet<usize>, worklist: &mut Vec<*mut GcHeader>) {
    let promise = user_ptr as *const crate::promise::Promise;

    // Trace value and reason — may be NaN-boxed JSValues or raw I64 pointers
    for &val_bits in &[(*promise).value.to_bits(), (*promise).reason.to_bits()] {
        if try_mark_value_or_raw(val_bits, valid_ptrs) {
            let tag = val_bits & TAG_MASK;
            let ptr_val = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                (val_bits & POINTER_MASK) as usize
            } else {
                val_bits as usize
            };
            let header = header_from_user_ptr(ptr_val as *const u8);
            worklist.push(header);
        }
    }

    // Trace on_fulfilled and on_rejected (closure pointers)
    let on_fulfilled = (*promise).on_fulfilled;
    if !on_fulfilled.is_null() {
        let ptr_usize = on_fulfilled as usize;
        if valid_ptrs.contains(&ptr_usize) {
            let header = header_from_user_ptr(on_fulfilled as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }

    let on_rejected = (*promise).on_rejected;
    if !on_rejected.is_null() {
        let ptr_usize = on_rejected as usize;
        if valid_ptrs.contains(&ptr_usize) {
            let header = header_from_user_ptr(on_rejected as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }

    // Trace next promise in chain
    let next = (*promise).next;
    if !next.is_null() {
        let next_usize = next as usize;
        if valid_ptrs.contains(&next_usize) {
            let header = header_from_user_ptr(next as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }
}

/// Trace error fields (message, name, stack are all StringHeader pointers)
unsafe fn trace_error(user_ptr: *mut u8, valid_ptrs: &HashSet<usize>, worklist: &mut Vec<*mut GcHeader>) {
    let error = user_ptr as *const crate::error::ErrorHeader;

    for &str_ptr in &[(*error).message, (*error).name, (*error).stack] {
        if !str_ptr.is_null() {
            let ptr_usize = str_ptr as usize;
            if valid_ptrs.contains(&ptr_usize) {
                let header = header_from_user_ptr(str_ptr as *const u8);
                if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    worklist.push(header);
                }
            }
        }
    }
}

/// Sweep: free unmarked malloc objects; add unmarked arena objects to free list.
/// Returns total bytes freed.
fn sweep() -> u64 {
    let mut freed_bytes: u64 = 0;

    // Sweep malloc objects
    MALLOC_OBJECTS.with(|list| {
        let mut list = list.borrow_mut();
        let mut i = 0;
        while i < list.len() {
            let header = list[i];
            unsafe {
                if (*header).gc_flags & GC_FLAG_PINNED != 0 {
                    // Pinned objects are always kept alive — clear mark bit inline
                    (*header).gc_flags &= !GC_FLAG_MARKED;
                    i += 1;
                    continue;
                }
                if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                    // Unmarked: free it
                    let total_size = (*header).size as usize;
                    freed_bytes += total_size as u64;

                    // For Maps, also free the separately-allocated entries array
                    if (*header).obj_type == GC_TYPE_MAP {
                        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                        let map = user_ptr as *const crate::map::MapHeader;
                        let entries = (*map).entries;
                        if !entries.is_null() {
                            let cap = (*map).capacity as usize;
                            if cap > 0 {
                                let ent_size = (cap * 16).max(8); // ENTRY_SIZE = 16
                                let ent_layout = Layout::from_size_align(ent_size, 8).unwrap();
                                dealloc(entries as *mut u8, ent_layout);
                            }
                        }
                    }

                    let layout = Layout::from_size_align(total_size, 8).unwrap();
                    // Remove from tracking set BEFORE dealloc
                    MALLOC_SET.with(|set| {
                        set.borrow_mut().remove(&(header as usize));
                    });
                    dealloc(header as *mut u8, layout);
                    list.swap_remove(i);
                    // Don't increment i — swap_remove moved last element here
                } else {
                    // Surviving object — clear mark bit inline to avoid separate heap walk
                    (*header).gc_flags &= !GC_FLAG_MARKED;
                    i += 1;
                }
            }
        }
    });

    // Sweep arena objects: add unmarked to free list
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            if (*header).gc_flags & GC_FLAG_PINNED != 0 {
                // Pinned objects are always kept alive — clear mark bit inline
                (*header).gc_flags &= !GC_FLAG_MARKED;
                return;
            }
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                let total_size = (*header).size as usize;
                let payload_size = total_size - GC_HEADER_SIZE;
                let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                freed_bytes += total_size as u64;

                // If this is an ObjectHeader, remove its OVERFLOW_FIELDS entry before
                // zeroing memory. This prevents stale entries from "infecting" new objects
                // that might be allocated at the same address.
                if (*header).obj_type == GC_TYPE_OBJECT {
                    crate::object::clear_overflow_for_ptr(user_ptr as usize);
                }

                // Zero the memory to prevent stale pointer retention
                std::ptr::write_bytes(user_ptr, 0, payload_size);

                // Add to free list for reuse
                ARENA_FREE_LIST.with(|fl| {
                    fl.borrow_mut().push((user_ptr, payload_size));
                });
            } else {
                // Surviving object — clear mark bit inline to avoid separate heap walk
                (*header).gc_flags &= !GC_FLAG_MARKED;
            }
        }
    });

    freed_bytes
}

/// Clear mark bits on all surviving objects
fn clear_marks() {
    // Clear arena objects
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
    });

    // Clear malloc objects
    MALLOC_OBJECTS.with(|list| {
        let list = list.borrow();
        for &header in list.iter() {
            unsafe {
                (*header).gc_flags &= !GC_FLAG_MARKED;
            }
        }
    });
}

// ============================================================================
// Root scanner registrations (called during module init)
// ============================================================================

/// Root scanner for promise task queue and scheduled resolves
pub fn promise_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::promise::scan_promise_roots(mark);
}

/// Root scanner for timer callbacks
pub fn timer_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::timer::scan_timer_roots(mark);
}

/// Root scanner for current exception
pub fn exception_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::exception::scan_exception_roots(mark);
}

/// Root scanner for object shape cache (keys arrays shared across objects with same shape)
pub fn shape_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_shape_cache_roots(mark);
}

/// Root scanner for OVERFLOW_FIELDS (per-object extra properties beyond inline slots)
pub fn overflow_fields_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_overflow_fields_roots(mark);
}

/// Initialize GC root scanners. Called once at runtime startup.
pub fn gc_init() {
    gc_register_root_scanner(promise_root_scanner);
    gc_register_root_scanner(timer_root_scanner);
    gc_register_root_scanner(exception_root_scanner);
    gc_register_root_scanner(shape_cache_root_scanner);
    gc_register_root_scanner(overflow_fields_root_scanner);
}

/// FFI: initialize GC (called from compiled code startup)
#[no_mangle]
pub extern "C" fn js_gc_init() {
    gc_init();
}

/// FFI: get GC stats
#[no_mangle]
pub extern "C" fn js_gc_stats(out_collections: *mut u64, out_freed: *mut u64, out_pause_us: *mut u64) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        unsafe {
            if !out_collections.is_null() {
                *out_collections = stats.collection_count;
            }
            if !out_freed.is_null() {
                *out_freed = stats.total_freed_bytes;
            }
            if !out_pause_us.is_null() {
                *out_pause_us = stats.last_pause_us;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_malloc_basic() {
        // Allocate a string-type object
        let ptr = gc_malloc(64, GC_TYPE_STRING);
        assert!(!ptr.is_null());

        // Verify header is set correctly
        unsafe {
            let header = header_from_user_ptr(ptr);
            assert_eq!((*header).obj_type, GC_TYPE_STRING);
            assert_eq!((*header).gc_flags, 0); // not arena, not marked
            assert_eq!((*header).size as usize, GC_HEADER_SIZE + 64);
        }

        // Verify it's tracked in MALLOC_OBJECTS
        let tracked = MALLOC_SET.with(|set| {
            let header = unsafe { header_from_user_ptr(ptr) };
            set.borrow().contains(&(header as usize))
        });
        assert!(tracked, "allocated object should be tracked in MALLOC_SET");
    }

    #[test]
    fn test_gc_malloc_different_types() {
        let string_ptr = gc_malloc(32, GC_TYPE_STRING);
        let closure_ptr = gc_malloc(48, GC_TYPE_CLOSURE);
        let bigint_ptr = gc_malloc(16, GC_TYPE_BIGINT);

        unsafe {
            assert_eq!((*header_from_user_ptr(string_ptr)).obj_type, GC_TYPE_STRING);
            assert_eq!((*header_from_user_ptr(closure_ptr)).obj_type, GC_TYPE_CLOSURE);
            assert_eq!((*header_from_user_ptr(bigint_ptr)).obj_type, GC_TYPE_BIGINT);
        }
    }

    #[test]
    fn test_gc_collect_updates_stats() {
        // Get initial stats
        let initial_count = GC_STATS.with(|s| s.borrow().collection_count);

        // Run GC
        gc_collect_inner();

        // Stats should have incremented
        let new_count = GC_STATS.with(|s| s.borrow().collection_count);
        assert_eq!(new_count, initial_count + 1, "collection count should increment");
    }

    #[test]
    fn test_gc_header_size() {
        assert_eq!(GC_HEADER_SIZE, 8, "GC header should be 8 bytes");
    }

    #[test]
    fn test_gc_realloc_basic() {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        assert!(!ptr.is_null());

        // Write some data
        unsafe {
            std::ptr::write_bytes(ptr, 0xAB, 32);
        }

        // Reallocate to larger size
        let new_ptr = gc_realloc(ptr, 128);
        assert!(!new_ptr.is_null());

        // Verify old data preserved (first 32 bytes should still be 0xAB)
        unsafe {
            for i in 0..32 {
                assert_eq!(*new_ptr.add(i), 0xAB,
                    "byte {} should be preserved after realloc", i);
            }
        }

        // Verify tracking updated
        let tracked = MALLOC_SET.with(|set| {
            let header = unsafe { header_from_user_ptr(new_ptr) };
            set.borrow().contains(&(header as usize))
        });
        assert!(tracked, "reallocated object should be tracked");
    }

    #[test]
    fn test_gc_realloc_null_allocates_fresh() {
        let ptr = gc_realloc(std::ptr::null_mut(), 64);
        assert!(!ptr.is_null(), "realloc(null) should allocate fresh");
    }

    #[test]
    fn test_gc_mark_flags() {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        unsafe {
            let header = header_from_user_ptr(ptr);

            // Initially not marked
            assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);

            // Mark it
            (*header).gc_flags |= GC_FLAG_MARKED;
            assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);

            // Clear mark
            (*header).gc_flags &= !GC_FLAG_MARKED;
            assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);
        }
    }

    #[test]
    fn test_gc_pinned_flag() {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        unsafe {
            let header = header_from_user_ptr(ptr);

            // Pin it
            (*header).gc_flags |= GC_FLAG_PINNED;

            // Run GC - pinned objects should survive
            gc_collect_inner();

            // Verify still tracked
            let tracked = MALLOC_SET.with(|set| {
                set.borrow().contains(&(header as usize))
            });
            assert!(tracked, "pinned object should survive GC");

            // Unpin
            (*header).gc_flags &= !GC_FLAG_PINNED;
        }
    }

    #[test]
    fn test_build_valid_pointer_set() {
        // Allocate some objects
        let ptr1 = gc_malloc(32, GC_TYPE_STRING);
        let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);

        let valid_set = build_valid_pointer_set();

        // Our malloc objects should be in the valid set
        assert!(valid_set.contains(&(ptr1 as usize)), "ptr1 should be in valid set");
        assert!(valid_set.contains(&(ptr2 as usize)), "ptr2 should be in valid set");
    }

    #[test]
    fn test_clear_marks_resets_all() {
        // Allocate and mark some objects
        let ptr1 = gc_malloc(32, GC_TYPE_STRING);
        let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);

        unsafe {
            (*header_from_user_ptr(ptr1)).gc_flags |= GC_FLAG_MARKED;
            (*header_from_user_ptr(ptr2)).gc_flags |= GC_FLAG_MARKED;
        }

        clear_marks();

        unsafe {
            assert_eq!((*header_from_user_ptr(ptr1)).gc_flags & GC_FLAG_MARKED, 0,
                "mark should be cleared on ptr1");
            assert_eq!((*header_from_user_ptr(ptr2)).gc_flags & GC_FLAG_MARKED, 0,
                "mark should be cleared on ptr2");
        }
    }
}
