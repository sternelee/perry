# Plan: Generational GC for Perry

**Status:** proposed, pre-implementation. Written 2026-04-24 after
v0.5.206 (Step 2 lazy JSON parse complete). This doc captures the
design before any code lands — the implementation is multi-week and
touches codegen + runtime + GC in concert, so agreement on direction
matters.

**Goal:** structurally beat Bun on peak RSS for general (non-JSON)
workloads. Today Perry's `bench_json_roundtrip` with lazy flag beats
Bun on time by 3.6× but still uses 30% more RSS. On any workload
where lazy parse doesn't apply, Perry's flat-arena GC leaves the
working set at peak-during-burst size until the next full GC cycle —
Bun's generational collector reclaims most of that in the nursery
between bursts.

## Why generational is the right answer

**The allocation distribution in typical JS workloads:**
- 90%+ of allocations die in the scope they were created.
- Most of the remaining 10% die in the next outer scope.
- A tiny minority survives to "old" age (module globals, caches,
  long-lived data structures).

Perry's current flat arena treats all allocations equally: every
`new X()` lands in the same per-thread arena block. A mark-sweep full
GC walks every reachable object across every block. On a 10k-record
`JSON.parse`, that's ~60k objects to mark-scan per cycle, even when
99% of them are trivially dead.

**Generational splits the arena by age:**
- **Young generation (nursery)** — small (1-4 MB), fills fast, swept
  often. Dead objects stay in the nursery and get reclaimed en-masse
  on reset. Living objects get promoted to the old generation.
- **Old generation** — larger (growable), swept rarely. Same
  mark-sweep we have today, just with smaller input.

On a typical burst (parse + process + discard), most objects never
leave the nursery. Minor GC resets the nursery in O(survivors), not
O(allocations). The working-set footprint drops to approximately
`sizeof(survivors) + nursery_size` (single-digit MB) instead of
`sizeof(all-allocs-since-last-major-GC)` (double-digit to
triple-digit MB on heavy workloads).

## Prerequisite: precise root tracking

Perry's current conservative stack scanner uses `setjmp` to snapshot
callee-saved registers, then walks the C stack scanning for bit-
patterns that look like valid heap pointers. This works for
non-moving GC because every valid pointer found is still a valid
pointer; false positives (non-pointer words that happen to look like
heap addresses) keep dead objects alive but don't corrupt memory.

**Generational GC requires precise roots.** Specifically:
- Minor GC needs to know which stack slots hold young-gen pointers so
  it can evacuate them to old-gen (if they survive).
- Without precise roots, we'd have to treat every looks-like-pointer
  word as "might be a young-gen pointer", scan its target, promote
  it. That's correctness-safe but re-introduces the cascade
  behavior issue #179 was fighting.

### Precise root tracking design

**Codegen emits a shadow stack at every safepoint.** A safepoint is
any program point where GC may run — today that's function entry,
allocation sites, and explicit `gc()` calls. The shadow stack is a
per-thread `Vec<*mut u8>` of live heap-pointer-typed locals.

For each function, at the IR level:
1. Entry: push a frame onto the shadow stack with a precomputed
   slot count for this function.
2. Before each safepoint: update the current frame's slots to reflect
   live pointer-typed locals at this point.
3. On exit: pop the frame.

Perry already knows which locals are pointer-typed (the HIR has
`HirType::String`/`Array`/`Object`/etc.). The shadow-stack update is
just a store of each live pointer into its pre-assigned slot in the
frame. Roughly 1-2 instructions per live pointer per safepoint.

**Cost:** measured in V8/Bun as 2-8% on pointer-heavy workloads.
Effectively free on computation-heavy workloads (few pointers, few
safepoints). On Perry's current benchmark mix:
- `07_object_create`: no escape, no safepoints in the hot loop → ~0%
- `12_binary_trees`: heavy pointer churn, many safepoints → ~5% est.
- `bench_json_roundtrip` direct: ~3% est. (mostly inside runtime)
- `bench_gc_pressure`: already GC-dominated, net win from faster GC

**Tradeoff:** the conservative stack scanner stops being authoritative
for heap roots. We keep it for the *C stack below our JS frames*
(runtime function local variables holding JSValues) because rewriting
every Rust runtime function to use a shadow stack is infeasible.
Minor GC treats conservatively-found pointers as "might be young-gen,
conservatively promote". This costs some over-promotion but preserves
correctness.

## Generational GC design

### Nursery layout

- Per-thread, 2 MB default (configurable via `PERRY_NURSERY_MB` env).
- Flat bump-allocator — same as today's arena but smaller.
- Fills fast, resets on every minor GC.
- Objects in nursery always have `GcHeader.gc_flags & GC_FLAG_YOUNG`.

### Minor GC (nursery collection)

Triggered when the nursery fills. Runs in one pass:

1. **Root scan** — walk the shadow stack (precise) + runtime
   register-roots (malloc side-table, parse roots, shape cache, etc.)
   + conservative scan of the C stack below JS frames.
2. **Mark & evacuate** — for each reachable nursery pointer:
   - If the object survived fewer than `PROMOTION_AGE` minor GCs
     (default 2), copy it to a fresh nursery slot.
   - Otherwise, promote: copy to old-gen.
   - In both cases, update the root to point at the new location.
3. **Forward old-gen → young-gen pointers** — every old-gen object
   that WROTE a young-gen pointer got its remembered-set bit flipped
   by the write barrier (next section). Scan just those, promote or
   update.
4. **Reset nursery** — sweep is free; the nursery's bump pointer
   moves back to start, any un-copied objects die.

**Critical:** minor GC only touches the nursery + the remembered
set's old-gen roots. It does NOT walk the entire old generation.
That's the asymptotic win.

### Major GC (full collection)

Triggered when old-gen passes a threshold (starts at current
`GC_THRESHOLD_INITIAL_BYTES`). Behaves like today's mark-sweep:

1. Scan all roots.
2. Mark through both generations.
3. Sweep old-gen (block reset for dead-only blocks — same as today).
4. Minor-GC the nursery to clear surviving-but-unreachable objects.
5. Clear the remembered set (it's regenerated by subsequent write
   barriers).

### Write barriers

Codegen emits a write barrier at every store of a heap pointer into
a heap object:

```
before:  *field = new_value
after:   *field = new_value
         if (new_value is young-gen pointer && field's owner is old-gen):
             add owner to remembered_set
```

**Minimal implementation:**
- Per-thread remembered set = `Vec<*mut GcHeader>`.
- Write barrier checks `owner.gc_flags & GC_FLAG_YOUNG == 0` (owner
  is old-gen) AND `new_value.gc_flags & GC_FLAG_YOUNG != 0` (value is
  young).
- If both, push `owner` to remembered_set (deduplicated via a
  `HashSet` or bit on `gc_flags`).

**Cost per store:** 2 loads (owner's flags, value's flags), 1 AND, 1
branch. ~3-5ns on a modern CPU. Adds up for stores-heavy workloads.

**Elision opportunities:** codegen can skip the barrier when:
- Store target is the nursery (both sides young — no barrier needed).
- Value being stored is a primitive (no barrier needed).
- Both operands are provably the same generation via static analysis.

### Promotion policy

Simple **age-based**: each nursery object carries a 2-bit age counter
in `GcHeader._reserved`. Each minor GC increments the age. When age
reaches `PROMOTION_AGE` (default 2), the object promotes to old-gen
instead of re-copying in the nursery.

**Why 2:** single-survival-and-die pattern is very common (parse
intermediates, string concat temporaries). Age 2 gives them a second
minor GC to actually die before we commit to moving them. Can be
tuned later with a PERRY_PROMOTION_AGE env var.

**Alternative considered:** size-based (large objects bypass nursery
and allocate directly in old-gen). Simpler to add later as an
optimization.

## Phased rollout

Four phases, each independently verifiable and revertable.

### Phase A — Precise root tracking (prerequisite)

**Scope:** codegen emits shadow-stack push/pop at function entry/exit
and slot updates at safepoints. No GC behavior change yet — the GC
still uses conservative scanning; the shadow stack is a parallel
mechanism that's built but not yet consumed.

**Ship criteria:**
- All existing tests pass byte-for-byte.
- Shadow-stack contents verified by a new test that walks the stack
  at a safepoint and checks slots match expected live pointers.
- Zero benchmark regression (shadow-stack construction should be
  noise-level for Perry's workloads).

### Phase B — Flat-arena split into young + old regions

**Scope:** the existing single arena splits into `NURSERY_ARENA` +
`OLD_ARENA` (both thread-local). All allocations default to NURSERY.
No minor GC yet — nursery just grows until a full GC runs, same as
today. This phase is a no-op behavior change that sets up the
allocation paths.

**Ship criteria:**
- Existing `bench_json_roundtrip` numbers unchanged (lazy flag on or
  off).
- Nursery overflow correctly pushes new nursery blocks.
- All gap tests still 24/28.

### Phase C — Minor GC + write barriers

**Scope:**
- Codegen emits write barriers at every heap store (behind
  `PERRY_WRITE_BARRIERS=1` feature flag for initial rollout).
- Minor GC actually collects the nursery.
- `PERRY_GEN_GC=1` gates minor GC firing.

**Ship criteria:**
- `bench_json_roundtrip` direct path RSS drops to ≤70 MB (vs today's
  144 MB), time within 10% of today.
- `07_object_create` / `12_binary_trees` unchanged (tight hot loops
  already die entirely before GC, so minor GC would rarely fire).
- No test regressions under `PERRY_GEN_GC=1`.

### Phase D — Flip defaults + clean up conservative scanner

**Scope:**
- `PERRY_GEN_GC=1` becomes default.
- Conservative scanner shrinks to "scan only the C stack below JS
  frames" (the Rust runtime's local variables).
- `PERRY_GEN_GC=0` environment variable retains the old behavior for
  bisection.

**Ship criteria:**
- Six-week soak on `main` with no GC-related bug reports.
- All three bench_json benchmarks hit or exceed v0.5.206 numbers.
- Documentation updated to describe the generational GC as the
  default model.

## Estimated effort

- Phase A: 1-2 weeks (codegen changes + tests)
- Phase B: 1 week (pure allocator refactor)
- Phase C: 2-3 weeks (write-barrier codegen + minor-GC
  implementation + correctness hardening)
- Phase D: 1 week (flip flag + cleanup + documentation)

**Total: 5-7 weeks** for a single focused engineer; longer with other
on-call / review / PR cycle overhead.

## Expected wins

**On `bench_json_roundtrip` direct path (no lazy flag):**
- Today: 372 ms / 144 MB
- After generational: estimated 250-280 ms / 40-60 MB (RSS drops 2-3×
  as nursery collects per-iter allocations; time improves because
  minor GC is faster than the full mark-sweep we do today).

**On `bench_json_roundtrip` with lazy flag:**
- Today: 69 ms / 108 MB
- After generational: estimated 69 ms / 30-50 MB (lazy path doesn't
  allocate a tree, so the nursery stays tiny — most RSS win is from
  the tape allocations going through nursery → promoted on survival).

**On `bench_gc_pressure`:**
- Today: 17-19 ms / 26 MB
- After generational: estimated 8-12 ms / 15 MB (GC time drops
  because most garbage dies in the nursery; never reaches mark-sweep).

**On `07_object_create`:**
- Today: 0-1 ms / 6 MB
- After generational: unchanged (working set fits in one block under
  nursery threshold; GC never fires).

## Risks + mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Write barrier overhead > 10% on some workloads | medium | Elision passes; benchmark gate during Phase C |
| Promotion bugs → dangling pointers | high | Rust-level `cargo test -Z sanitizer=address` run on every Phase C commit |
| Shadow stack miss → premature collection | high | Phase A ships with a cross-check test that compares shadow stack against conservative scan results |
| Minor GC slow path regresses `bench_gc_pressure` | medium | Phase C ship criteria explicitly includes this bench |
| Generational GC interaction with lazy JSON parse | low | The lazy JSON tape is arena-allocated; it just lives in the nursery initially and promotes if it survives enough minor GCs — same as any other allocation |
| Rust soundness (shadow stack as `&mut [*mut u8]`) | medium | Use raw pointers + `UnsafeCell`; audit patterns align with `arena.rs`'s existing thread-local `UnsafeCell<Arena>` |

## Follow-ups / parked items

- **Large-object direct promotion:** objects > 16 KB allocate
  directly in old-gen, bypassing the nursery. Simpler to add after
  Phase C lands.
- **Concurrent marking:** old-gen major GC could run concurrently
  with mutator. Multi-week effort; park until generational itself
  is stable.
- **Compacting old-gen:** fragmentation bothers nothing today but
  might after a few months of real-world use. Defer until measured.
- **Card-marking write barrier:** per-card dirty bits instead of
  per-object remembered-set. Better for write-heavy workloads.
  Defer until we have data showing the simple remembered-set
  approach is the bottleneck.

## Contextual lazy JSON — heuristic runtime profiling / `@perry-lazy` pragma

Captured here (orthogonal to generational GC but tracked together):
the lazy JSON path from `docs/json-typed-parse-plan.md` is opt-in via
`PERRY_JSON_TAPE=1`. That's the right default for now (lazy is a
slight loss on workloads that materialize every element), but two
follow-up axes let users make the tradeoff contextual:

1. **`@perry-lazy` JSDoc pragma** — landing as part of this cycle.
   A function- or module-level `/** @perry-lazy */` comment opts
   `JSON.parse` calls in its scope into the lazy path. Node-
   compatible (comments erase). See follow-up commit.

2. **Heuristic runtime profiling** — parked. Observe blob size + post-
   parse access pattern at runtime and auto-route large blobs with
   `.length`-only access through the lazy path. Per-call-site state
   in a global hash. Switch-to-lazy after N successful length-only
   or stringify-only observations. Harder than it looks because it
   needs good de-opt behavior when a call site changes shape.

The generational GC change will also reduce the lazy-vs-direct RSS
gap on the hot path — lazy currently wins by parse-output-tree
avoidance, but generational reclaims that tree quickly either way.
Once generational lands, it may be worth re-measuring whether lazy
is still worth the code cost for the common case.

## Log

Will be filled in as phases ship.

| Date | Version | Phase | Result |
|---|---|---|---|
| TBD | | A — precise root tracking | |
| TBD | | B — young/old arena split | |
| TBD | | C — minor GC + write barriers | |
| TBD | | D — flip default + cleanup | |
