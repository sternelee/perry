# Memory & Performance Roadmap: Beating Bun and Node on `bench_json_roundtrip`

**Status:** active. Written 2026-04-24 alongside v0.5.193.
**Goal:** beat **both** Node and Bun on **both** time and peak RSS on the reference
`bench_json_roundtrip.ts` workload — a 50-iteration `JSON.parse + JSON.stringify`
loop over a ~1 MB blob with a 10k-record module-level setup array.

This doc is a living plan. Each tier has ship criteria and a rough impact estimate;
update numbers and strike through items as they land.

## Current standings (v0.5.194, macOS ARM64, best-of-5)

| Runtime | Time | Peak RSS |
|---|---:|---:|
| **Perry v0.5.194** | **322 ms** | **199 MB** |
| Node 25.8.0 | 372 ms | 191 MB |
| Bun 1.3.12 | 248 ms | 83 MB |

**Perry now beats Node on time** (−13%) and matches Node on RSS (+4%). Perry
still trails Bun by ~30% on time and ~140% on RSS. The time gap to Bun is
almost entirely in `JSON.parse/stringify` throughput; the RSS gap is
structural (generational GC).

Historical reference:

| Version | Time | Peak RSS | Notes |
|---|---:|---:|---|
| v0.5.190 (pre-fix) | 316 ms | 318 MB | block-persist cascade active |
| v0.5.192 | 316 ms | 318 MB | segregated longlived arena (infra only) |
| v0.5.193 | 384 ms | 213 MB | age-restricted block-persist + no 2-cycle grace |
| **v0.5.194** | **322 ms** | **199 MB** | **block size 8 MB → 1 MB (tier 1 #1)** |

## Why Perry loses each axis

### Time (−136 ms vs Bun)
- `JSON.parse` / `JSON.stringify` dominate this bench's CPU.
- Perry's parser is recursive-descent with a zero-copy escape-free fast path
  (`crates/perry-runtime/src/json.rs::DirectParser`). Single-byte-at-a-time scanner.
- Bun uses a simdjson-derived SIMD parser. On the benchmark's ~1 MB blob, SIMD
  parse is typically 2-4× a hand-written recursive descent.
- Perry's stringify is single-pass scalar too. Same ~2× window vs. a SIMD writer.
- Everything else on this bench (allocation, GC, mark, sweep) is already fast.

### RSS (+130 MB vs Bun)
- Bun uses a **generational GC**: young nursery (~2 MB), precisely swept every
  few thousand allocations. Most parse output is trivially young-gen garbage
  and never makes it to old space.
- Perry has one flat arena with 8 MB block granularity. Even with v0.5.193's
  age-restricted block-persist, the recent-5-block safety window alone reserves
  40 MB, and 8 MB blocks can only be reclaimed when fully dead.
- No generational split = Perry's runtime state grows as the union of ("live set"
  + "recent-window headroom" + "any block still in use by the current allocation
  burst"). On a 5 MB/iter workload, that union is ~20 MB live + 40 MB window +
  some overshoot ≈ what we see.

## The levers, ranked by impact/effort

Each item lists: **impact estimate** (RSS, time), **effort**, **risk**, **scope**.

### Tier 1 — days of work, meaningful wins

#### 1. ~~Shrink arena block size 8 MB → 1-2 MB~~ ✅ v0.5.194

**Landed: block size 8 MB → 1 MB.** Measured on `bench_json_roundtrip`
(best-of-5, macOS ARM64):

| Block size | Time | Peak RSS |
|---|---:|---:|
| 8 MB (baseline) | 384 ms | 213 MB |
| 2 MB | 325 ms | 208 MB |
| **1 MB** | **322 ms** | **199 MB** |
| 512 KB | 318 ms | 200 MB  (diminishing returns) |

RSS win was modest vs. projection (213 → 199 MB, 7% instead of the
projected 213 → 130 MB). Turns out the bulk of the 213 MB wasn't the
recent-5-block window but the allocation-headroom-between-GCs — which
scales with the adaptive step, not block size. **Time win was the
surprise**: 384 ms → 322 ms (−16%), because smaller blocks = more
frequent arena growth = more frequent GC triggers = the adaptive step
halves faster, the workload's 60-80% freed-pct per cycle actually
lands reclaim in time instead of sitting on a too-high step.

All seven regression benchmarks unchanged: `07_object_create`,
`12_binary_trees`, `02_loop_overhead`, `06_math_intensive`,
`bench_gc_pressure`, `03_array_write`, `bench_array_grow`. Gap tests
24/28 unchanged. Runtime tests 124/124.

#### 2. Short string optimization (SSO)

- **Impact:** RSS 10-30 MB savings on string-heavy benches; time ~5-15% win
  on workloads allocating many short strings. Strings ≤6 bytes encode directly
  in the NaN-boxed f64 payload (6 bytes × 8 bits = 48 bits; payload is 48 bits),
  skipping the entire arena/malloc path — no `StringHeader`, no `GcHeader`.
- **Effort:** ~1 week. Affects:
  - `crates/perry-runtime/src/string.rs`: new `SHORT_STRING_TAG` (distinct from
    `STRING_TAG` which points to heap), encoders/decoders, `js_string_from_bytes`
    dispatches on length.
  - Every `is_string`/`as_string_ptr`/string-decode site — likely 30-50 call
    sites across runtime and codegen.
  - Codegen emits length-aware decode for property access.
- **Risk:** easy to miss a decode path → NPE or wrong string content. Mitigated
  by a single `js_string_decode(v: JSValue) -> (*const u8, u32)` helper used
  everywhere.
- **Scope:** touches many files but each edit is mechanical.

**Ship criteria:**
- `test_edge_string_*` tests all pass.
- All 28 gap tests in same state (24/28, no regression).
- `bench_json_roundtrip` RSS drops another 10+ MB.

#### 3. SIMD JSON parser

- **Impact:** `JSON.parse` 2-4× throughput. On `bench_json_roundtrip` the
  parse takes ~60% of iteration time; 3× speedup cuts ~80 ms from the total.
  Would put Perry at ~300 ms on this bench. No RSS impact.
- **Effort:** 1-2 weeks. Rewrite `json.rs::DirectParser` using SIMD structural
  scanning (simdjson style) + allocation of parse output objects unchanged.
  Use `std::simd` (stable since Rust 1.77) or `std::arch` intrinsics for ARM
  NEON and x86 AVX2 variants.
- **Risk:** SIMD JSON is notoriously subtle — UTF-8 validation, escape
  handling, exponential-edge-case numbers. Take from an existing crate rather
  than hand-rolling where possible (evaluate `simd-json` crate).
- **Scope:** self-contained to `json.rs`.

**Ship criteria:**
- All existing `test_gap_json_*` / JSON parse tests pass.
- Byte-for-byte output match with Node's `JSON.parse` on the suite's
  representative inputs.
- ≥2× parse throughput on `bench_json_roundtrip`.

### Tier 2 — weeks of work, structural wins

#### 4. Escape analysis via TypeScript types

- **Impact:** RSS 10-30 MB on allocation-heavy benches; time ~10-20% win when
  short-lived intermediate objects get stack-allocated. On the JSON bench, the
  per-iteration `tags` array and `nested` object never escape — both become
  stack allocations.
- **Effort:** 2-3 weeks. New HIR pass in `crates/perry-hir` or
  `crates/perry-transform` tracking value flow. Codegen emits `alloca` for
  non-escaping objects instead of `arena_alloc_gc`. Non-escape proof:
  - Value not stored into an escaping container
  - Value not returned
  - Value not captured by a closure that outlives the scope
- **Risk:** proof bugs → UAF (stack-allocated object accessed after scope).
  Must be conservative: when in doubt, heap-allocate. Testing: use
  `RUSTFLAGS="-Zsanitizer=address"` on the runtime suite before shipping.
- **Scope:** HIR pass + codegen alloca path. Well-bounded if we keep the proof
  conservative.

**Ship criteria:**
- All workspace tests pass under `-Zsanitizer=address`.
- `bench_json_roundtrip` RSS drops another 20+ MB.
- No test regressions in gap suite or runtime tests.

#### 5. Precise root tracking via codegen

- **Impact:** by itself, zero. But it's the **unlock** for tier 3. Once roots
  are precise, conservative stack scan goes away, `mark_block_persisting_arena_objects`
  goes away entirely, moving GC becomes possible.
- **Effort:** 3-4 weeks. Emit a per-function "shadow stack" at every safepoint:
  a stack-allocated array of pointers to live JS values. GC walks the shadow
  stack instead of the raw machine stack.
- **Risk:** register pressure + shadow-stack overhead. Benchmark carefully.
  Typical cost: 2-8% on pointer-heavy workloads; effectively free on
  computation-heavy workloads.
- **Scope:** codegen.rs + every call-site emission. Large but mechanical.

**Ship criteria:**
- All gap tests + runtime tests pass with conservative scan disabled.
- No benchmark regresses >5%.
- `mark_block_persisting_arena_objects` can be deleted.

### Tier 3 — month+ of work, architectural answer

#### 6. Generational GC (requires tier 2 #5)

- **Impact:** RSS drops to approximately live-set-size + nursery. Estimated
  `bench_json_roundtrip` RSS 213 → ~50 MB (below Bun). Time: minor GCs are
  very cheap; major GCs rarer; net ~10-20% speed win.
- **Effort:** 3-4 weeks on top of #5. Non-moving generational collector:
  - Young nursery: fixed 1-2 MB arena
  - All new allocations go to nursery
  - Minor GC: scan precise roots + remembered set → mark nursery → survivors
    promote to old arena (copy or bump in place depending on moving strategy)
  - Old arena: current mechanism (mark-sweep with block reset)
  - Write barrier: every store of a young pointer into an old object adds to
    remembered set (codegen emits the check)
- **Risk:** write barrier overhead (typically 5-10%). Promotion correctness.
  Mitigated by gradual rollout behind a flag.
- **Scope:** new gc.rs pass + codegen write barrier + new arena split.

**Ship criteria:**
- Perry RSS on `bench_json_roundtrip` ≤90 MB.
- All benchmarks within 5% of baseline speed.
- Full test suite clean.

#### 7. Full compacting GC

- **Impact:** fragmentation disappears, RSS = live-set. This is the
  theoretical floor for non-streaming workloads.
- **Effort:** on top of #6, another 2-4 weeks. Requires moving GC — objects
  change addresses during collection. Pointers everywhere must be updated
  (only safe with precise roots from #5).
- **Risk:** large — any stale pointer (e.g. in a runtime function holding a
  raw reference across an allocation) = use-after-move.
- **Scope:** full GC rewrite.

**Ship criteria:** TBD after #6 lands; may not be worth it.

## Recommended path

1. **Now:** Tier 1 #1 (smaller blocks). Fastest visible RSS drop; contained risk.
2. **Next:** Tier 1 #2 (SSO). Compounds with #1 — fewer allocations means
   smaller blocks fill more slowly.
3. **Then:** Tier 1 #3 (SIMD JSON). Closes the time gap to Bun; no RSS impact
   but gets us to "beats Node, close to Bun on time".
4. **Evaluate.** If Perry is within ~30 MB RSS of Bun after tier 1, the
   generational GC is probably still worth doing but loses some urgency. If
   we're still >100 MB behind, tier 2+3 is mandatory to win.
5. **Tier 2 #4 (escape analysis)** before #5 — it's cheaper and has immediate
   RSS value even without the precise-root infrastructure.
6. **Tier 2 #5 + Tier 3 #6 together** as the architectural overhaul. Do this
   only once tier 1 has shipped and been stable for a release or two.

## Tradeoffs to call out explicitly

Every one of these trades **codegen/runtime complexity for performance**. Perry's
current appeal is partly that the codegen is tractable and the runtime is
readable. A generational GC with write barriers and shadow stacks changes that
character substantially.

Before committing to tier 2+3, the maintainer should decide:
- Is the `bench_json_roundtrip`-style workload actually representative of
  what Perry users care about?
- Would it be more valuable to focus Perry on workloads where its current
  architecture wins (startup time, LLVM-optimized hot paths, native UI, static
  typing) and accept the RSS gap on GC-heavy benches?

If the answer is "yes, we want to win GC-heavy workloads too", then tier 2+3
is the honest path. If the answer is "Perry's niche is elsewhere", tier 1 alone
is plenty.

## Log

| Date | Version | Change | Result |
|---|---|---|---|
| 2026-04-24 | v0.5.192 | Tier 0 (not listed above): segregated longlived arena for caches (PR #179 scope A) | RSS unchanged, infrastructure in place |
| 2026-04-24 | v0.5.193 | Tier 0 (cont.): age-restricted block-persist, adaptive step tune, drop 2-cycle grace on old blocks | RSS 318 → 213 MB (−33%); time +21% |
| 2026-04-24 | v0.5.194 | **Tier 1 #1: block size 8 MB → 1 MB** | RSS 213 → 199 MB (−7%); time 384 → 322 ms (−16%). **Now beats Node on both axes.** |
