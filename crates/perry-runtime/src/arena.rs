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

        // Current block is full. Check GC trigger first — if it fires
        // and reclaims at least one fully-empty block (via
        // `arena_reset_empty_blocks`), we may be able to reuse that
        // block instead of pushing a new one.
        crate::gc::gc_check_trigger();

        // Retry the (possibly newly-reset) current block. arena.current
        // may have been changed by arena_reset_empty_blocks to point
        // at the lowest reset block.
        if let Some(ptr) = self.blocks[self.current].alloc(size, align) {
            return ptr;
        }

        // Scan forward for any other block with space — the GC may
        // have reset blocks we haven't tried yet. Without this scan,
        // we'd push a fresh block on the very first overflow even
        // though blocks `current+1..n_blocks` are all empty.
        for i in 0..self.blocks.len() {
            if i == self.current { continue; }
            if let Some(ptr) = self.blocks[i].alloc(size, align) {
                self.current = i;
                // Resync inline state to the new current block.
                INLINE_STATE.with(|s| unsafe {
                    let inline = &mut *s.get();
                    if !inline.data.is_null() {
                        let block = &self.blocks[self.current];
                        inline.data = block.data;
                        inline.offset = block.offset;
                        inline.size = block.size;
                    }
                });
                return ptr;
            }
        }

        // Still no room anywhere — push a fresh block.
        self.blocks.push(alloc_block(size));
        self.current = self.blocks.len() - 1;

        self.blocks[self.current].alloc(size, align)
            .expect("Fresh block should have space")
    }
}

thread_local! {
    static ARENA: UnsafeCell<Arena> = UnsafeCell::new(Arena::new());

    /// Inline allocator state — a cache of the current arena block's
    /// `(data, offset, size)` triple, exposed via a stable pointer so
    /// codegen can emit inline bump-allocate IR without going through
    /// any function call or `LocalKey::with` wrapper.
    ///
    /// `#[repr(C)]` on `InlineArenaState` keeps the field offsets stable
    /// (data=0, offset=8, size=16). The codegen reads/writes these fields
    /// directly via fixed GEPs, so changing the struct layout would
    /// silently break every emitted `new ClassName()`.
    static INLINE_STATE: UnsafeCell<InlineArenaState> = UnsafeCell::new(InlineArenaState {
        data: std::ptr::null_mut(),
        offset: 0,
        size: 0,
    });
}

/// Inline bump-allocator state. The codegen emits inline LLVM IR that
/// reads `data` and `offset`, computes `aligned + size`, checks against
/// `size`, stores the new offset, and returns `data + aligned`. The
/// underlying thread-local Arena is the source of truth between
/// inline-alloc bursts; this state is the source of truth during them.
///
/// Field offsets are load-bearing — the codegen GEPs into this struct
/// at hard-coded byte offsets (0/8/16). Do not reorder.
#[repr(C)]
pub struct InlineArenaState {
    pub data: *mut u8,    // offset  0  — current block's data pointer
    pub offset: usize,    // offset  8  — bump pointer (mutated inline)
    pub size: usize,      // offset 16  — current block's size
}

/// Get the per-thread inline arena state pointer. Called once per JS
/// function entry; the codegen caches the result in a stack slot and
/// reuses it for every `new ClassName()` in that function. The address
/// is stable for the lifetime of the thread, so caching is safe.
///
/// First call on each thread lazy-syncs from the underlying ARENA.
#[no_mangle]
pub extern "C" fn js_inline_arena_state() -> *mut InlineArenaState {
    INLINE_STATE.with(|s| {
        let state = unsafe { &mut *s.get() };
        if state.data.is_null() {
            // Lazy init: copy from underlying ARENA's current block.
            ARENA.with(|a| unsafe {
                let arena = &*a.get();
                let block = &arena.blocks[arena.current];
                state.data = block.data;
                state.offset = block.offset;
                state.size = block.size;
            });
        }
        state as *mut InlineArenaState
    })
}

/// Slow path for inline bump alloc. Called from emitted IR when the
/// fast-path bump check fails (would overflow the current block).
///
/// Sequence:
///   1. Sync inline state's offset back to the underlying ARENA block
///      (so the alloc that's about to push a new block sees the right
///      "current" offset, and so any concurrent GC walk sees all live
///      objects from the inline-alloc burst).
///   2. Allocate via the existing `Arena::alloc` path — handles new
///      block + GC trigger via `alloc_slow`.
///   3. Resync inline state to point at whichever block the alloc
///      landed in (may be the same block if there was leftover space,
///      or a fresh block from `alloc_slow`).
///
/// Returns the raw pointer (the codegen writes the GcHeader at this
/// address and the ObjectHeader at +8 — same layout the inline path
/// produces).
#[no_mangle]
pub extern "C" fn js_inline_arena_slow_alloc(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
) -> *mut u8 {
    let state_ref = unsafe { &mut *state };
    ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        // Sync inline-state offset back to underlying block (so
        // arena_walk_objects and the slow-path GC trigger see the
        // post-burst offset).
        arena.blocks[arena.current].offset = state_ref.offset;
        // Allocate via existing path (may push a new block + run GC).
        let ptr = arena.alloc(size, align);
        // Resync inline state to the (possibly new) current block.
        let block = &arena.blocks[arena.current];
        state_ref.data = block.data;
        state_ref.offset = block.offset;
        state_ref.size = block.size;
        ptr
    })
}

/// Sync the inline arena state's offset back to the underlying arena
/// block. Call before any code path that walks the arena (GC scan,
/// `arena_walk_objects`, allocation accounting) so the block's offset
/// reflects the inline-burst's true high-water mark.
///
/// Cheap when no inline allocs have happened yet (state.data is null);
/// otherwise it's a thread-local read + a single store.
pub fn sync_inline_arena_state() {
    INLINE_STATE.with(|s| unsafe {
        let state = &*s.get();
        if !state.data.is_null() {
            ARENA.with(|a| {
                let arena = &mut *(*a).get();
                arena.blocks[arena.current].offset = state.offset;
            });
        }
    });
}

/// Allocate memory from the thread-local arena
/// This is very fast - just a pointer bump in the common case
///
/// Coexists with the inline allocator: every call here syncs the
/// inline state's offset back to the underlying block first (so we
/// don't overwrite inline-allocated memory), then allocates, then
/// resyncs the inline state to the post-alloc state of the block.
/// The two extra TLS reads cost ~5-10ns per call, which is fine
/// because non-inline allocations (`js_string_from_bytes`,
/// `js_closure_alloc`, etc.) are infrequent compared to the
/// per-class-instance hot path that uses the inline allocator.
#[inline]
pub fn arena_alloc(size: usize, align: usize) -> *mut u8 {
    INLINE_STATE.with(|inline_s| unsafe {
        let inline = &mut *inline_s.get();
        ARENA.with(|a| {
            let arena = &mut *(*a).get();
            // Sync inline → block before allocating, if the inline
            // state has been initialized.
            if !inline.data.is_null() {
                arena.blocks[arena.current].offset = inline.offset;
            }
            let ptr = arena.alloc(size, align);
            // Resync block → inline (may have advanced to a new block).
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
            ptr
        })
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

/// Get bytes currently in use (sum of `block.offset` across blocks).
/// Used by adaptive GC to measure how much actual data the program is
/// holding live, separately from how much arena space we've reserved.
/// After a GC sweep that resets empty blocks, in-use bytes drop
/// dramatically while reserved bytes stay constant.
pub fn arena_in_use_bytes() -> usize {
    sync_inline_arena_state();
    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        let mut used: usize = 0;
        for block in &arena.blocks {
            used += block.offset;
        }
        used
    })
}

/// Walk all GcHeader objects in arena blocks linearly.
/// Calls `callback` for each GcHeader pointer found.
/// Objects are discovered by their `size` field (hop from one to the next).
pub fn arena_walk_objects(mut callback: impl FnMut(*mut u8)) {
    use crate::gc::GcHeader;

    // Sync inline state's offset back to the underlying block first,
    // so the walk sees objects that the inline allocator has emitted
    // since the last non-inline alloc.
    sync_inline_arena_state();

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

/// Like `arena_walk_objects` but also passes the block's index alongside
/// each header — used by the GC sweep to track per-block live counts in
/// a `Vec<bool>` (O(1) lookups) so it can reset fully-empty blocks back
/// to offset=0 in O(blocks) instead of O(objects).
pub fn arena_walk_objects_with_block_index(mut callback: impl FnMut(*mut u8, usize)) {
    use crate::gc::GcHeader;

    sync_inline_arena_state();

    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for (block_idx, block) in arena.blocks.iter().enumerate() {
            let mut offset = 0usize;
            while offset < block.offset {
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }
                let header_ptr = unsafe { block.data.add(aligned) };
                let header = header_ptr as *const GcHeader;
                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        break;
                    }
                    let obj_type = (*header).obj_type;
                    if obj_type >= 1 && obj_type <= 7 {
                        callback(header_ptr, block_idx);
                    }
                    offset = aligned + total_size;
                }
            }
        }
    });
}

/// How many arena blocks are currently allocated. Used by the sweep to
/// size its per-block live-tracking `Vec<bool>` before walking objects.
pub fn arena_block_count() -> usize {
    ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() })
}

/// Fast path for the common case where the entire arena is empty
/// after GC (every object dead). Resets every block's offset to 0,
/// clears the free list, sets `current = 0`, and resyncs the inline
/// state. Avoids the per-block tracking HashMap that
/// `arena_reset_empty_blocks` needs.
///
/// This is what makes tight `new ClassName()` loops competitive with
/// V8: when the workload allocates short-lived class instances and
/// nothing escapes, GC observes that all 700k+ objects from the
/// previous burst are dead and reclaims the entire arena in O(1).
pub fn arena_reset_all_blocks_to_zero() {
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        for block in arena.blocks.iter_mut() {
            block.offset = 0;
        }
        arena.current = 0;
        // Free list is now invalid (all entries point into reset blocks).
        crate::gc::ARENA_FREE_LIST.with(|fl| fl.borrow_mut().clear());
        crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
        // Resync inline state to block 0 (offset 0, full size).
        INLINE_STATE.with(|s| {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &arena.blocks[0];
                inline.data = block.data;
                inline.offset = 0;
                inline.size = block.size;
            }
        });
    });
}

/// Reset arena blocks that have zero live objects after a GC sweep.
/// `live_block_data_ptrs` is the set of `block.data` pointers that
/// the sweep observed at least one live (marked or pinned) object in.
/// Any other block — i.e. one with `offset > 0` but no live objects —
/// is reclaimed by setting `offset = 0`. Free-list entries pointing
/// into the reset blocks are filtered out so the next allocation
/// doesn't hand back a stale slot in a region the inline allocator
/// is about to overwrite.
///
/// This is the load-bearing optimization that makes the inline bump
/// allocator perform competitively with V8 on tight `new` loops:
/// without it, every iteration page-faults through fresh memory once
/// the working set crosses ~64MB; with it, GC reclaims empty blocks
/// in place and the inline allocator keeps reusing the same ~8MB
/// arena block forever.
pub fn arena_reset_empty_blocks(block_has_live: &[bool]) {
    let n_live = block_has_live.iter().filter(|&&b| b).count();
    let n_total = block_has_live.len();
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        let mut reset_block_ranges: Vec<(usize, usize)> = Vec::new();
        for (i, block) in arena.blocks.iter_mut().enumerate() {
            // A block is reclaimable iff (a) it has at least one
            // allocation, and (b) the sweep observed zero live objects
            // in it. The `block_has_live` slice is indexed by block
            // index — entries past its length default to "no live"
            // (e.g. blocks added during the sweep itself).
            let live = block_has_live.get(i).copied().unwrap_or(false);
            if block.offset > 0 && !live {
                reset_block_ranges.push((block.data as usize, block.size));
                block.offset = 0;
            }
        }
        if reset_block_ranges.is_empty() {
            return;
        }
        // Filter the free list: remove entries pointing into any reset
        // block. The bump allocator will overwrite those slots, so the
        // free list must not hand them back.
        crate::gc::ARENA_FREE_LIST.with(|fl| {
            let mut fl = fl.borrow_mut();
            fl.retain(|&(ptr, _)| {
                let p = ptr as usize;
                !reset_block_ranges
                    .iter()
                    .any(|&(base, size)| p >= base && p < base + size)
            });
            if fl.is_empty() {
                crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
            }
        });
        // Walk back the `current` index to the first reset block —
        // i.e., one with `offset == 0`. If we just picked the first
        // block with any free space we'd land on the live block that
        // still has 80 bytes left at the end (not enough for a 96-byte
        // class instance), and the next alloc would push a fresh
        // block. The reset blocks are the whole point of this routine
        // — make sure we actually use one.
        let mut new_current = arena.current;
        for (i, block) in arena.blocks.iter().enumerate() {
            if block.offset == 0 {
                new_current = i;
                break;
            }
        }
        arena.current = new_current;
        let _ = (n_live, n_total);
        INLINE_STATE.with(|s| {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
        });
    });
}

/// Get arena memory statistics: (heap_used, heap_total)
/// heap_used = total bytes allocated across all blocks
/// heap_total = total bytes reserved across all blocks
#[no_mangle]
pub extern "C" fn js_arena_stats(out_used: *mut u64, out_total: *mut u64) {
    // Sync inline state so the "used" count reflects the inline-burst
    // high-water mark, not just the last sync point.
    sync_inline_arena_state();
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
