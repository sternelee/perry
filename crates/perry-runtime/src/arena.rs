//! Fast bump allocator for short-lived objects
//!
//! Uses thread-local bump allocation for fast object creation.
//! Objects allocated here are not individually freed - the entire arena
//! can be reset at once (e.g., at end of program or during GC).

use std::cell::UnsafeCell;
use std::alloc::{alloc, Layout};

/// Size of each arena block (8MB)
const BLOCK_SIZE: usize = 8 * 1024 * 1024;

/// Create a block of at least the given size (for oversized allocations)
fn alloc_block(min_size: usize) -> ArenaBlock {
    let size = if min_size <= BLOCK_SIZE { BLOCK_SIZE } else {
        // Round up to next multiple of BLOCK_SIZE
        ((min_size + BLOCK_SIZE - 1) / BLOCK_SIZE) * BLOCK_SIZE
    };
    let layout = Layout::from_size_align(size, 16).unwrap();
    let data = unsafe { alloc(layout) };
    if data.is_null() {
        panic!("Failed to allocate arena block of {} bytes", size);
    }
    ArenaBlock {
        data,
        size,
        offset: 0,
    }
}

/// A single arena block
struct ArenaBlock {
    data: *mut u8,
    size: usize,
    offset: usize,
}

impl ArenaBlock {
    fn new() -> Self {
        alloc_block(BLOCK_SIZE)
    }

    /// Try to allocate within this block, respecting alignment
    #[inline]
    fn alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        // Align offset up
        let aligned_offset = (self.offset + align - 1) & !(align - 1);
        if aligned_offset + size > self.size {
            return None;
        }

        let ptr = unsafe { self.data.add(aligned_offset) };
        self.offset = aligned_offset + size;
        Some(ptr)
    }
}

/// Thread-local arena allocator
///
/// When a thread exits (e.g., worker threads from `perry/thread`), the Drop
/// impl frees all arena blocks so memory isn't leaked.
struct Arena {
    blocks: Vec<ArenaBlock>,
    current: usize,
}

impl Drop for Arena {
    fn drop(&mut self) {
        for block in &self.blocks {
            let layout = std::alloc::Layout::from_size_align(block.size, 16).unwrap();
            unsafe { std::alloc::dealloc(block.data, layout); }
        }
    }
}

impl Arena {
    fn new() -> Self {
        Arena {
            blocks: vec![ArenaBlock::new()],
            current: 0,
        }
    }

    #[inline]
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // Try current block first
        if let Some(ptr) = self.blocks[self.current].alloc(size, align) {
            return ptr;
        }

        // Need a new block — sized to fit the allocation
        // Check GC trigger before allocating new block
        crate::gc::gc_check_trigger();

        self.blocks.push(alloc_block(size));
        self.current += 1;

        self.blocks[self.current].alloc(size, align)
            .expect("Fresh block should have space")
    }
}

thread_local! {
    static ARENA: UnsafeCell<Arena> = UnsafeCell::new(Arena::new());
}

/// Allocate memory from the thread-local arena
/// This is very fast - just a pointer bump in the common case
#[inline]
pub fn arena_alloc(size: usize, align: usize) -> *mut u8 {
    ARENA.with(|arena| {
        let arena = unsafe { &mut *arena.get() };
        arena.alloc(size, align)
    })
}

/// Allocate from arena with a GcHeader prepended.
/// Returns pointer to usable memory AFTER the GcHeader.
/// The object is NOT added to any tracking list — arena objects are discovered
/// by walking arena blocks linearly.
///
/// `#[inline(always)]` so the bitcode-link path can fully inline
/// this into user IR — the bump-pointer pattern is small enough
/// (~10 instructions on the fast path) that inlining is a clear win
/// and the slow path (free-list walk + new arena block) is gated
/// behind a cold branch.
#[inline(always)]
pub fn arena_alloc_gc(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_HEADER_SIZE, GC_FLAG_ARENA};

    // Hot path: bump-allocate from the current arena block, skipping the
    // free-list walk entirely. The free-list-nonempty `Cell` is a single
    // unboxed load (no `RefCell::borrow_mut` cost) and is `false` for the
    // first GC cycle of every benchmark — which is when allocation-heavy
    // micro-benchmarks like object_create / binary_trees run their tight
    // loops. Walking an empty Vec was costing ~10ns per alloc (borrow,
    // iterate, drop) for nothing; this `Cell` check is ~1ns.
    let reused = if crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.get()) {
        crate::gc::ARENA_FREE_LIST.with(|fl| {
            let mut fl = fl.borrow_mut();
            // Find a slot that fits (exact or slightly larger)
            let mut best_idx = None;
            let mut best_waste = usize::MAX;
            for (idx, &(_, slot_size)) in fl.iter().enumerate() {
                if slot_size >= size && slot_size - size < best_waste {
                    best_waste = slot_size - size;
                    best_idx = Some(idx);
                    if best_waste == 0 {
                        break; // Perfect fit
                    }
                }
            }
            if let Some(idx) = best_idx {
                let (ptr, _slot_size) = fl.swap_remove(idx);
                if fl.is_empty() {
                    crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
                }
                Some(ptr)
            } else {
                None
            }
        })
    } else {
        None
    };

    if let Some(user_ptr) = reused {
        // Reusing a free-list slot: the GcHeader is already in place (before user_ptr)
        // Just update it
        unsafe {
            let header = user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = GC_FLAG_ARENA;
            (*header)._reserved = 0;
            // size field already set from original allocation
        }
        return user_ptr;
    }

    let total = GC_HEADER_SIZE + size;
    let raw = arena_alloc(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA;
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate an object of known size from the arena
/// Returns a properly aligned pointer
#[no_mangle]
pub extern "C" fn js_arena_alloc(size: u32) -> *mut u8 {
    arena_alloc(size as usize, 8)
}

/// Get total bytes reserved across all arena blocks
pub fn arena_total_bytes() -> usize {
    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        let mut total: usize = 0;
        for block in &arena.blocks {
            total += block.size;
        }
        total
    })
}

/// Walk all GcHeader objects in arena blocks linearly.
/// Calls `callback` for each GcHeader pointer found.
/// Objects are discovered by their `size` field (hop from one to the next).
pub fn arena_walk_objects(mut callback: impl FnMut(*mut u8)) {
    use crate::gc::GcHeader;

    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            let mut offset = 0usize;
            while offset < block.offset {
                // Align to 8 bytes (all our allocations are 8-byte aligned)
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }

                let header_ptr = unsafe { block.data.add(aligned) };
                let header = header_ptr as *const GcHeader;

                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        // Invalid header — we've hit uninitialized or non-GC memory.
                        // This can happen because arena_alloc() (without GC) is still
                        // used for some allocations. Skip the rest of this block.
                        break;
                    }

                    // Only process if this looks like a valid GC object
                    let obj_type = (*header).obj_type;
                    if obj_type >= 1 && obj_type <= 7 {
                        callback(header_ptr);
                    }

                    offset = aligned + total_size;
                }
            }
        }
    });
}

/// Get arena memory statistics: (heap_used, heap_total)
/// heap_used = total bytes allocated across all blocks
/// heap_total = total bytes reserved across all blocks
#[no_mangle]
pub extern "C" fn js_arena_stats(out_used: *mut u64, out_total: *mut u64) {
    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        let mut used: u64 = 0;
        let mut total: u64 = 0;
        for block in &arena.blocks {
            used += block.offset as u64;
            total += block.size as u64;
        }
        unsafe {
            *out_used = used;
            *out_total = total;
        }
    });
}
