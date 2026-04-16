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
/// Set on a keys-array that was handed out by `shape_cache_insert`.
/// `js_object_set_field_by_name` reads this bit to decide whether it
/// must clone before mutating (shared arrays can't be mutated in
/// place; fresh arrays allocated in the `keys.is_null()` branch can).
/// Without the bit the clone fires on every property added to every
/// fresh object literal — a 20-property row object allocates 19
/// throwaway keys_array clones per row.
pub const GC_FLAG_SHAPE_SHARED: u8 = 0x08;
/// Set on strings that live in the intern table. Prevents in-place
/// mutation and allows `js_object_set_field_by_name` to skip the
/// FNV-1a hash (pointer identity is sufficient for interned strings).
pub const GC_FLAG_INTERNED: u8 = 0x10;

// Object flags stored in GcHeader._reserved (u16) for Object.freeze/seal/preventExtensions
pub const OBJ_FLAG_FROZEN: u16 = 0x01;
pub const OBJ_FLAG_SEALED: u16 = 0x02;
pub const OBJ_FLAG_NO_EXTEND: u16 = 0x04;

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

    /// Fast empty-check for `ARENA_FREE_LIST` — kept in sync with the Vec
    /// length. The hot allocation path checks this `Cell` (a single load,
    /// no `RefCell::borrow_mut` cost) and skips the free-list lookup
    /// entirely when it's empty. Maintained by the GC sweep (sets) and
    /// `arena_alloc_gc` (clears when the Vec drains).
    pub(crate) static ARENA_FREE_LIST_NONEMPTY: std::cell::Cell<bool> =
        std::cell::Cell::new(false);

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

    /// Suppression flag: when true, gc_check_trigger() skips collection entirely.
    /// Used by JSON.parse to avoid mid-parse GC cycles — parse is synchronous
    /// and roots intermediate values in PARSE_ROOTS, so deferring GC until
    /// after parse completes is safe and eliminates O(n*m) GC overhead.
    static GC_SUPPRESSED: Cell<bool> = Cell::new(false);
}

/// Threshold: run GC when total arena bytes exceed this
const GC_THRESHOLD_INITIAL_BYTES: usize = 128 * 1024 * 1024; // 128MB
const GC_THRESHOLD_MAX_BYTES: usize = 1024 * 1024 * 1024; // 1GB cap on adaptive growth

thread_local! {
    /// Lower bound for the next GC trigger. Bumped after each
    /// `gc_collect_inner` based on collection effectiveness (see the
    /// adaptive logic in `gc_check_trigger`).
    ///
    /// The initial value is `GC_THRESHOLD_INITIAL_BYTES` (128MB —
    /// chosen so that the 96MB working set of a 1M-iter object_create
    /// or binary_trees benchmark fits under the threshold and pays
    /// zero GC cost). After every collection, if the sweep freed >75%
    /// of arena bytes, the per-program "step" is doubled (capped at
    /// 1GB) so subsequent allocation bursts don't pay GC overhead just
    /// because they re-cross the same line. For hot `new ClassName()`
    /// loops where every object dies between GC cycles, this means
    /// the FIRST burst pays for at most one collection and the rest
    /// run GC-free.
    ///
    /// If a sweep frees <25%, the step is halved (down to a 16MB
    /// floor) so live-set-bound programs don't grow their working
    /// set unboundedly between collections.
    static GC_NEXT_TRIGGER_BYTES: std::cell::Cell<usize> =
        std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES);

    /// Per-program adaptive GC step. Doubles (up to MAX) when sweeps
    /// are mostly-garbage; halves (down to 16MB) when sweeps reclaim
    /// little. Used to compute the next trigger after each GC as
    /// `post_total + step`.
    static GC_STEP_BYTES: std::cell::Cell<usize> =
        std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES);

    /// Lower bound for the next malloc-count-based GC trigger. After each
    /// collection, this is reset to `survivor_count + GC_MALLOC_COUNT_STEP`
    /// so that programs with large legitimate live sets (>10k tracked
    /// malloc objects) don't GC-thrash on every subsequent allocation.
    /// See `gc_check_trigger` for the update rule.
    static GC_NEXT_MALLOC_TRIGGER: std::cell::Cell<usize> =
        std::cell::Cell::new(100_000);
}

/// Initial step for the malloc-count-based GC trigger. Adaptive: doubles
/// when >75% of malloc objects are garbage (loop-scoped temporaries),
/// halves when <25% are garbage (large live set). Capped at
/// `GC_MALLOC_COUNT_STEP_MAX` to bound memory between collections.
///
/// Originally a single hardcoded threshold (`GC_MALLOC_COUNT_THRESHOLD`);
/// issue #34 showed that triggering GC from `gc_malloc` (needed for
/// malloc-heavy workloads that don't push arena blocks — e.g.
/// @perry/postgres's `parseBigIntDecimal` bigint chain) combined with a
/// hardcoded threshold would thrash for any program whose live set
/// exceeded the threshold. Making it a per-cycle step fixes that.
///
/// Issue #58: the constant 10k step caused ~100 GC cycles for 500k-iter
/// string-concat loops where almost every object is dead. Adaptive
/// doubling ramps the step to 160k+ after a few mostly-garbage sweeps,
/// cutting GC cycles from ~100 to ~10.
const GC_MALLOC_COUNT_STEP_INITIAL: usize = 100_000;
const GC_MALLOC_COUNT_STEP_MAX: usize = 2_000_000;
const GC_MALLOC_COUNT_STEP_MIN: usize = 10_000;

thread_local! {
    /// Per-program adaptive malloc-count step. Mirrors `GC_STEP_BYTES`
    /// behaviour: doubles when mostly-garbage, halves when mostly-live.
    static GC_MALLOC_COUNT_STEP: std::cell::Cell<usize> =
        std::cell::Cell::new(GC_MALLOC_COUNT_STEP_INITIAL);
}

/// Allocate memory via malloc with GcHeader prepended.
/// Returns pointer to usable memory AFTER the header.
/// The allocation is tracked in MALLOC_OBJECTS.
pub fn gc_malloc(size: usize, obj_type: u8) -> *mut u8 {
    let total = GC_HEADER_SIZE + size;
    let layout = Layout::from_size_align(total, 8).unwrap();

    // Issue #34: malloc-heavy workloads that don't push arena blocks
    // (e.g. the `n = n * 10n + digit` bigint accumulator inside
    // @perry/postgres's `parseBigIntDecimal`, or a decode loop producing
    // many short-lived strings) never trigger GC via the arena slow path.
    // Without this call MALLOC_OBJECTS grows unboundedly.
    //
    // We run the check BEFORE `alloc` so the sweep can't free the about-
    // to-be-returned pointer — after `alloc` the fresh user pointer lives
    // only in a caller-saved register and the conservative stack scan
    // (`setjmp` only captures callee-saved regs) can't see it as a root.
    // Running before means the fresh allocation simply doesn't exist yet
    // during the GC cycle.
    gc_check_trigger();

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

/// Batch-allocate multiple GC-tracked malloc objects in one go.
/// Amortises overhead: one `gc_check_trigger` call, one `MALLOC_OBJECTS`
/// extend, one `MALLOC_SET` extend — instead of N of each.
/// `sizes` contains the *payload* size for each object (excluding GcHeader).
/// Returns a Vec of user pointers (past the header), one per entry.
pub fn gc_malloc_batch(sizes: &[usize], obj_type: u8) -> Vec<*mut u8> {
    gc_check_trigger(); // once, not N times

    let n = sizes.len();
    let mut results = Vec::with_capacity(n);
    let mut headers = Vec::with_capacity(n);

    unsafe {
        GC_IN_ALLOC.with(|f| f.set(true));

        for &size in sizes {
            let total = GC_HEADER_SIZE + size;
            let layout = Layout::from_size_align(total, 8).unwrap();
            let raw = alloc(layout);
            if raw.is_null() {
                panic!("gc_malloc_batch: failed to allocate {} bytes", total);
            }
            let header = raw as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = 0;
            (*header)._reserved = 0;
            (*header).size = total as u32;

            headers.push(header);
            results.push(raw.add(GC_HEADER_SIZE));
        }

        MALLOC_OBJECTS.with(|list| {
            let mut list = list.borrow_mut();
            list.extend_from_slice(&headers);
        });
        MALLOC_SET.with(|set| {
            let mut set = set.borrow_mut();
            for &h in &headers {
                set.insert(h as usize);
            }
        });

        GC_IN_ALLOC.with(|f| f.set(false));
    }

    results
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

/// Suppress GC triggers. While suppressed, `gc_check_trigger` is a no-op.
/// Used by JSON.parse to avoid mid-parse GC cycles.
pub fn gc_suppress() {
    GC_SUPPRESSED.with(|c| c.set(true));
}

/// Resume GC triggers after suppression.
pub fn gc_unsuppress() {
    GC_SUPPRESSED.with(|c| c.set(false));
}

/// Rebaseline the malloc-count trigger to the current live set so that
/// objects just created during a GC-suppressed window (e.g. JSON.parse)
/// don't immediately trip a collection on the next allocation.
pub fn gc_bump_malloc_trigger() {
    let current = MALLOC_OBJECTS.with(|list| list.borrow().len());
    let step = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(current + step));
}

/// Check if GC should run. Called only when a new arena block is allocated.
/// Skips collection if we're inside gc_malloc/gc_realloc to prevent
/// RefCell double-borrow panics (reentrancy from allocation → arena grow → GC → sweep).
pub fn gc_check_trigger() {
    if GC_IN_ALLOC.with(|f| f.get()) || GC_SUPPRESSED.with(|f| f.get()) {
        return;
    }
    use crate::arena::arena_total_bytes;
    let total = arena_total_bytes();
    let next_trigger = GC_NEXT_TRIGGER_BYTES.with(|c| c.get());
    if total >= next_trigger {
        // Snapshot pre-GC in-use bytes to measure collection effectiveness.
        let pre_in_use = crate::arena::arena_in_use_bytes();
        gc_collect_inner();
        let post_in_use = crate::arena::arena_in_use_bytes();

        // Adaptive: if the GC was mostly garbage (>75% of in-use
        // bytes reclaimed), double the per-program step so the next
        // allocation burst doesn't trip GC at the same point. If the
        // GC freed almost nothing (<25%), halve the step — the
        // program has a large live set and we should collect more
        // frequently to avoid runaway memory growth.
        let freed = pre_in_use.saturating_sub(post_in_use);
        let mut step = GC_STEP_BYTES.with(|c| c.get());
        if pre_in_use > 0 {
            let pct_freed = (freed * 100) / pre_in_use;
            if pct_freed > 75 {
                step = (step * 2).min(GC_THRESHOLD_MAX_BYTES);
            } else if pct_freed < 25 {
                step = (step / 2).max(16 * 1024 * 1024);
            }
            GC_STEP_BYTES.with(|c| c.set(step));
        }
        let new_total = arena_total_bytes();
        GC_NEXT_TRIGGER_BYTES.with(|c| c.set(new_total + step));
        // Rebaseline malloc trigger too — the just-completed collection
        // swept malloc objects, so the next malloc-count trigger should
        // be relative to the new survivor count.
        let survivors = MALLOC_OBJECTS.with(|list| list.borrow().len());
        let mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
        return;
    }
    // Also trigger on malloc object count to bound memory growth for
    // services that stay within a single arena block but produce many
    // short-lived strings/closures/bigints per iteration. Since
    // gc_malloc now calls this (issue #34), the threshold is adaptive
    // — it's always `survivor_count + step` after each cycle, so
    // programs with large legitimate live sets don't thrash.
    //
    // Issue #58: the step is now adaptive — after each malloc-triggered
    // collection, if >75% of objects were garbage, double the step (up
    // to 500k). If <25% were garbage, halve it (down to 5k floor).
    // This lets tight loops that produce tons of dead temporaries
    // (string concat, object creation) ramp the step quickly so they
    // pay only a handful of GC cycles instead of ~100.
    let malloc_count = MALLOC_OBJECTS.with(|list| list.borrow().len());
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
    if malloc_count >= next_malloc_trigger {
        let pre_count = malloc_count;
        gc_collect_inner();
        let survivors = MALLOC_OBJECTS.with(|list| list.borrow().len());
        // Adapt the malloc-count step based on collection effectiveness.
        //
        // Issue #58 insight: in tight allocation loops the conservative
        // stack scanner keeps almost everything alive — GC finds <10%
        // garbage and wastes time walking 100k+ objects. In this regime
        // we should BACK OFF (increase the step) so the loop can finish
        // without GC interference. Once control returns to a higher scope
        // the dead objects will fall off the stack and become collectable.
        //
        // Conversely, when GC reclaims >75% it's working well and can
        // afford to stay at the current cadence or even speed up.
        let mut mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        if pre_count > 0 {
            let freed = pre_count.saturating_sub(survivors);
            let pct_freed = (freed * 100) / pre_count;
            if pct_freed < 15 {
                // GC is nearly useless — quadruple the step to back off fast
                mstep = (mstep * 4).min(GC_MALLOC_COUNT_STEP_MAX);
            } else if pct_freed < 50 {
                // GC is partially effective — double the step
                mstep = (mstep * 2).min(GC_MALLOC_COUNT_STEP_MAX);
            } else if pct_freed > 90 {
                // GC is highly effective — halve the step to collect sooner
                mstep = (mstep / 2).max(GC_MALLOC_COUNT_STEP_MIN);
            }
            // 50-90% freed: keep current step (balanced)
            GC_MALLOC_COUNT_STEP.with(|c| c.set(mstep));
        }
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
    }
}

/// Counter tracking "worker threads hold JSValue roots we can't scan"
/// state. Incremented by stdlib entry points that spawn tokio tasks which
/// invoke user closures on worker threads (WS server, HTTP server, etc.).
/// When > 0, the conservative main-thread stack scanner can't see all
/// live roots — collecting would free objects still referenced from
/// worker-thread stacks and SEGV on next access.
///
/// Issue #31: gc() from setInterval in a Fastify+WebSocket server crashed
/// within 60s of the first tick because WS worker threads held live refs
/// to message payload strings on their stacks. This counter lets stdlib
/// features signal "please skip user-initiated gc() while I'm running"
/// without a full stop-the-world mutex.
pub static GC_UNSAFE_ZONES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// One-shot warning so we don't spam stderr on every tick.
static GC_UNSAFE_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Manual GC trigger (callable from TypeScript as `gc()`). Skipped when
/// worker threads are active (see GC_UNSAFE_ZONES).
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    if GC_UNSAFE_ZONES.load(std::sync::atomic::Ordering::Acquire) > 0 {
        // One-shot warning — user likely has `setInterval(() => gc(), N)`
        // in a server; we don't want to print every 30s.
        if !GC_UNSAFE_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!(
                "perry: gc() skipped — a tokio-based server (WebSocket/HTTP) is active \
                 and may hold JSValue refs on worker threads that the main-thread GC \
                 can't see. Manual gc() is a no-op for the rest of this process."
            );
        }
        return;
    }
    gc_collect_inner();
}

/// Increment GC_UNSAFE_ZONES. Called by stdlib when spawning tokio tasks
/// that invoke user closures on worker threads.
#[no_mangle]
pub extern "C" fn js_gc_enter_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Decrement GC_UNSAFE_ZONES. Called when a stdlib feature that owns
/// worker threads shuts down (e.g. ws_server_close).
#[no_mangle]
pub extern "C" fn js_gc_exit_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
}

/// Threshold-based GC trigger (safe for use from the event loop).
/// Only runs collection if arena or malloc thresholds are exceeded.
#[no_mangle]
pub extern "C" fn gc_check_trigger_export() {
    gc_check_trigger();
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

    // 5. Block-persistence pass: arena blocks survive whole or not at all, so
    //    arena objects sharing a block with a root-reachable object persist
    //    even when not themselves reachable. Their malloc children must stay
    //    alive too (issues #43 / #44).
    mark_block_persisting_arena_objects(&valid_ptrs);

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

/// A sorted-`Vec`-backed set of valid user-space heap pointers,
/// used to validate candidate addresses found during the conservative
/// stack scan. Builds in O(n) push + O(n log n) sort, then answers
/// `contains` via O(log n) binary search.
///
/// Profiling showed that `HashSet<usize>` with 700k entries was the
/// dominant GC cost in `object_create` — even after pre-sizing, the
/// 700k inserts were ~10-15ms per collection because of repeated
/// hash computation + cache misses on the hash bucket array.
/// Sorted-Vec is ~3x faster on this workload (~5ms build) and the
/// O(log n) lookup is fast enough that the few thousand stack-scan
/// candidate validations per GC barely move the total.
pub(crate) struct ValidPointerSet {
    sorted: Vec<usize>,
}

impl ValidPointerSet {
    fn new(capacity: usize) -> Self {
        Self { sorted: Vec::with_capacity(capacity) }
    }
    fn insert(&mut self, ptr: usize) {
        self.sorted.push(ptr);
    }
    fn finalize(&mut self) {
        self.sorted.sort_unstable();
    }
    pub(crate) fn contains(&self, ptr: &usize) -> bool {
        self.sorted.binary_search(ptr).is_ok()
    }
}

/// Build a set of all valid user-space pointers (pointers returned to callers).
/// Used to validate candidates found during conservative stack scanning.
fn build_valid_pointer_set() -> ValidPointerSet {
    let malloc_count = MALLOC_OBJECTS.with(|list| list.borrow().len());
    // 48 bytes is a conservative under-estimate (smaller than the
    // typical 96-byte class instance) so the Vec doesn't realloc.
    let arena_estimate = crate::arena::arena_total_bytes() / 48;
    let mut set = ValidPointerSet::new(malloc_count + arena_estimate + 64);

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

    set.finalize();
    set
}

/// Get the GcHeader for a user pointer (pointer returned by gc_malloc or arena_alloc_gc).
/// The header is located GC_HEADER_SIZE bytes before the user pointer.
#[inline]
unsafe fn header_from_user_ptr(user_ptr: *const u8) -> *mut GcHeader {
    (user_ptr as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader
}

/// Try to mark a value (if it's a heap pointer). Returns true if newly marked.
fn try_mark_value(value_bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
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
/// variables — codegen stores these as raw I64 words (not NaN-boxed) in registers and on stack.
fn mark_stack_roots(valid_ptrs: &ValidPointerSet) {
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
fn try_mark_value_or_raw(word: u64, valid_ptrs: &ValidPointerSet) -> bool {
    // First try NaN-boxed interpretation (POINTER_TAG / STRING_TAG / BIGINT_TAG)
    if try_mark_value(word, valid_ptrs) {
        return true;
    }
    // Fallback: treat as raw (non-NaN-boxed) heap pointer.
    // Perry's is_string/is_array/is_pointer/is_closure locals store raw I64 addresses.
    // Validate against the known-heap-pointer set to avoid false positives from return addresses
    // and plain integers. Valid heap pointers are in the lower 48-bit address space and
    // won't have NaN-boxing tags in upper bits (already rejected above).
    let raw_ptr_u64 = word as u64;
    if raw_ptr_u64 < 0x1000 || raw_ptr_u64 > 0x0000_FFFF_FFFF_FFFF {
        return false; // Too small (null/invalid) or has upper bits set (NaN tag or non-address)
    }
    let raw_ptr = raw_ptr_u64 as usize;
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
fn mark_global_roots(valid_ptrs: &ValidPointerSet) {
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
fn mark_registered_roots(valid_ptrs: &ValidPointerSet) {
    // Collect scanners first to avoid borrow conflicts
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());

    for scanner in scanners {
        scanner(&mut |value: f64| {
            try_mark_value(value.to_bits(), valid_ptrs);
        });
    }
}

/// Process a worklist of already-marked headers: follow references iteratively,
/// marking newly-reached objects and pushing them onto the worklist.
fn drain_trace_worklist(worklist: &mut Vec<*mut GcHeader>, valid_ptrs: &ValidPointerSet) {
    let mut i = 0;
    while i < worklist.len() {
        let header = worklist[i];
        i += 1;

        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            match (*header).obj_type {
                GC_TYPE_ARRAY => trace_array(user_ptr, valid_ptrs, worklist),
                GC_TYPE_OBJECT => trace_object(user_ptr, valid_ptrs, worklist),
                GC_TYPE_CLOSURE => trace_closure(user_ptr, valid_ptrs, worklist),
                GC_TYPE_PROMISE => trace_promise(user_ptr, valid_ptrs, worklist),
                GC_TYPE_ERROR => trace_error(user_ptr, valid_ptrs, worklist),
                GC_TYPE_MAP => trace_map(user_ptr, valid_ptrs, worklist),
                GC_TYPE_STRING | GC_TYPE_BIGINT => {}
                _ => {}
            }
        }
    }
}

/// Trace from marked objects: follow references iteratively using a worklist.
fn trace_marked_objects(valid_ptrs: &ValidPointerSet) {
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

    drain_trace_worklist(&mut worklist, valid_ptrs);
}

/// Block-persistence pass: arena block reset is all-or-nothing, so any arena
/// object in a block that has at least one reachable object will persist in
/// memory whether or not the object itself was reached from a root. Any
/// malloc children referenced by those persisting arena objects must therefore
/// be kept alive — otherwise they get freed by sweep and the persisting arena
/// object holds dangling pointers.
///
/// Why this matters: during `arr.push(new_obj)`, the new object is in a
/// caller-saved register between its allocation and the write into `arr`.
/// If array growth triggers GC in that window, conservative stack scanning
/// (setjmp only captures callee-saved regs) doesn't see the new object as a
/// root. The arena block containing the new object still survives (other
/// objects in that block are reachable from `arr`), so the new object's
/// memory is intact. But its malloc-allocated string fields ("Record X",
/// email, etc.) get swept, and JSON.stringify later reads freed memory.
/// Repro: issues #43 / #44.
///
/// Iterates until fixed point because marking an arena object may trace a
/// child in a previously-dead block, making it live in the next round.
fn mark_block_persisting_arena_objects(valid_ptrs: &ValidPointerSet) {
    let mut worklist: Vec<*mut GcHeader> = Vec::new();
    loop {
        let n_blocks = crate::arena::arena_block_count();
        let mut block_has_live: Vec<bool> = vec![false; n_blocks];

        // Pass 1: compute which blocks have any reachable (marked/pinned) object.
        crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 {
                    if block_idx < block_has_live.len() {
                        block_has_live[block_idx] = true;
                    }
                }
            }
        });

        // Pass 2: mark any unmarked arena object in a live block and enqueue.
        let mut newly_marked = 0usize;
        crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
            if block_idx >= block_has_live.len() || !block_has_live[block_idx] {
                return;
            }
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    worklist.push(header);
                    newly_marked += 1;
                }
            }
        });

        if newly_marked == 0 {
            break;
        }

        // Trace newly marked; may mark children in previously-dead blocks,
        // requiring another round to pick them up.
        drain_trace_worklist(&mut worklist, valid_ptrs);
    }
}

/// Trace Map entries — scan all key-value pairs in the Map's entries array.
/// Maps store NaN-boxed JSValues (strings, arrays, objects) as keys and values.
/// Values may also be raw I64 pointers (for typed arrays/maps stored in maps).
unsafe fn trace_map(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, worklist: &mut Vec<*mut GcHeader>) {
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
            if bits >= 0x1000 && bits <= 0x0000_FFFF_FFFF_FFFF { bits as usize } else { 0 }
        }
    }
}

/// Trace array elements.
/// Elements may be NaN-boxed JSValues OR raw I64 pointers (codegen stores raw I64 for
/// is_pointer/is_array/is_string typed arrays via js_array_set_jsvalue).
unsafe fn trace_array(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, worklist: &mut Vec<*mut GcHeader>) {
    let arr = user_ptr as *const crate::array::ArrayHeader;
    let length = (*arr).length;
    let capacity = (*arr).capacity;

    // Sanity check: reject corrupt length/capacity to avoid scanning wild memory.
    // The 16M cap is a garbage-recognition guard (no realistic array exceeds it);
    // real programs routinely push >65k items into arrays (issue #44 repro hits 100k).
    if length > capacity || length > 16_000_000 {
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
unsafe fn trace_object(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, worklist: &mut Vec<*mut GcHeader>) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;

    // Sanity check: reject corrupt field_count to avoid scanning wild memory.
    // 1M is a garbage-recognition guard — legitimate objects never have that many fields.
    if field_count > 1_000_000 {
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
unsafe fn trace_closure(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, worklist: &mut Vec<*mut GcHeader>) {
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
unsafe fn trace_promise(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, worklist: &mut Vec<*mut GcHeader>) {
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

/// Trace error fields (message, name, stack are StringHeader pointers; cause is f64; errors is array)
unsafe fn trace_error(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, worklist: &mut Vec<*mut GcHeader>) {
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

    // Trace `cause` if it's a NaN-boxed pointer-like value
    let cause_bits = (*error).cause.to_bits();
    let top16 = (cause_bits >> 48) as u16;
    // POINTER_TAG=0x7FFD, STRING_TAG=0x7FFF, BIGINT_TAG=0x7FFA
    if top16 == 0x7FFD || top16 == 0x7FFF || top16 == 0x7FFA {
        let cause_ptr = (cause_bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
        if !cause_ptr.is_null() {
            let ptr_usize = cause_ptr as usize;
            if valid_ptrs.contains(&ptr_usize) {
                let header = header_from_user_ptr(cause_ptr);
                if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    worklist.push(header);
                }
            }
        }
    }

    // Trace `errors` array
    let errors_ptr = (*error).errors;
    if !errors_ptr.is_null() {
        let ptr_usize = errors_ptr as usize;
        if valid_ptrs.contains(&ptr_usize) {
            let header = header_from_user_ptr(errors_ptr as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
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

    // Sweep arena objects. Two-phase strategy:
    //
    //   1. Fast probe pass: walk objects, clear mark bits, count
    //      dead bytes, track whether ANY block has a live object.
    //      If no live anywhere → entire arena is reclaimable. Skip
    //      every per-block tracking structure and reset all blocks
    //      to offset=0 in O(1). This is the common case for tight
    //      `new ClassName()` loops where nothing escapes.
    //
    //   2. Slow tracking pass (only when some block has live objects):
    //      walk again, this time bucketing dead objects per block so
    //      we can decide which blocks are fully empty (reset) vs
    //      partially empty (push their dead objects to the free list
    //      in a single batched extend).
    //
    // The two-pass split avoids the per-object HashMap insert cost
    // (~50ns) on the common all-dead path, where it would account for
    // 700k × 50ns = 35ms per GC cycle.
    // Sweep arena objects with per-block live tracking.
    //
    // For each object, walk and check mark/pinned state:
    //   - live → set `block_has_live[block_idx]` and clear the mark
    //     bit inline so we don't need a separate pass.
    //   - dead → zero its payload memory (so stale pointers don't
    //     retain other objects on the next GC cycle).
    //
    // We deliberately do NOT push dead objects onto the global
    // ARENA_FREE_LIST. The inline bump allocator never reads the
    // free list — it uses the per-block reset instead. Pushing
    // dead objects to the free list would cost ~50ns per object
    // × ~700k objects per GC × ~12 GC cycles per benchmark = 420ms
    // of pure waste in `object_create`. The function-call allocator
    // path (`js_object_alloc_class_inline_keys` → `arena_alloc_gc`)
    // is the only consumer of the free list, and it's only used
    // for shapes the inline path doesn't cover (anonymous classes,
    // closure body new'd from a slot, etc.) — those are rare enough
    // that running them through the slow path is fine.
    //
    // After the walk, `arena_reset_empty_blocks` resets every block
    // with zero live objects to offset=0. This is the load-bearing
    // optimization that lets the inline bump allocator reuse memory
    // across GC cycles instead of page-faulting through fresh blocks.
    let n_blocks = crate::arena::arena_block_count();
    let mut block_has_live: Vec<bool> = vec![false; n_blocks];

    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            if (*header).gc_flags & GC_FLAG_PINNED != 0 {
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                (*header).gc_flags &= !GC_FLAG_MARKED;
                return;
            }
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                let total_size = (*header).size as usize;
                let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                freed_bytes += total_size as u64;

                if (*header).obj_type == GC_TYPE_OBJECT {
                    crate::object::clear_overflow_for_ptr(user_ptr as usize);
                }

                // Note: We deliberately do NOT zero the dead object's
                // payload here. trace_object/trace_array/trace_closure
                // walk objects PRECISELY (only `field_count` /
                // `length` / `capture_count` slots), so unused slots
                // and dead-object payloads are never scanned by the
                // mark phase. The conservative stack scan only walks
                // the C stack, not arbitrary heap memory. So stale
                // pointer-looking bytes inside dead-object payloads
                // can never trigger a false positive — and zeroing
                // them was costing ~2-3ms per `object_create` GC for
                // memory bandwidth (700k × 88 bytes = 62MB written).
            } else {
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                (*header).gc_flags &= !GC_FLAG_MARKED;
            }
        }
    });

    // Reset every block that ended up with zero live objects.
    crate::arena::arena_reset_empty_blocks(&block_has_live);

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

/// Root scanner for the shape-transition cache used by the dynamic-key
/// write path (`obj[name] = value`). Same role as `shape_cache_root_scanner`
/// — without it, GC would free cached target keys_arrays that no live
/// object currently references directly.
pub fn transition_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_transition_cache_roots(mark);
}

/// Root scanner for OVERFLOW_FIELDS (per-object extra properties beyond inline slots)
pub fn overflow_fields_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_overflow_fields_roots(mark);
}

/// Root scanner for in-progress JSON.parse frames (issue #46).
/// Without this, GC triggered mid-parse would sweep in-progress arrays/objects
/// and the fresh string/object values about to be pushed into them.
pub fn json_parse_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::json::scan_parse_roots(mark);
}

/// Initialize GC root scanners. Called once at runtime startup.
pub fn gc_init() {
    gc_register_root_scanner(promise_root_scanner);
    gc_register_root_scanner(timer_root_scanner);
    gc_register_root_scanner(exception_root_scanner);
    gc_register_root_scanner(shape_cache_root_scanner);
    gc_register_root_scanner(transition_cache_root_scanner);
    gc_register_root_scanner(overflow_fields_root_scanner);
    gc_register_root_scanner(json_parse_root_scanner);
    gc_register_root_scanner(intern_table_root_scanner);
}

/// Root scanner for the string intern table.
pub fn intern_table_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::string::scan_intern_table_roots(mark);
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
