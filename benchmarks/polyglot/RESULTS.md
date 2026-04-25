# Polyglot Benchmark Results

Perry vs 9 other runtimes on 8 identical benchmarks. All implementations
use `f64`/`double` arithmetic to match TypeScript's `number` type. No SIMD
intrinsics, no unsafe code, no non-default optimization flags — each
language's idiomatic release-mode build. A companion `RESULTS_OPT.md`
(phase 2 of this investigation) shows what happens when each language is
given flags equivalent to Perry's defaults.

See [`METHODOLOGY.md`](./METHODOLOGY.md) for iteration counts, clocks,
compiler versions, and a full explanation of which optimizations create
each delta.

## Results

**Run date:** 2026-04-25 — Perry commit `main` (v0.5.241).
**Hardware:** Apple M1 Max (10 cores, 64 GB RAM), macOS 26.4.
**Methodology:** best of 5 runs per cell, monotonic clock, no warmup.
All times in milliseconds. Lower is better.

| Benchmark      | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun |  Python |
|----------------|-------|-------|-------|-------|-------|-------|-------|-------|---------|
| fibonacci      |   302 |   314 |   304 |   440 |   394 |   276 |   991 |   510 |   15661 |
| loop_overhead  |    12 |    95 |    94 |    94 |    94 |    96 |    52 |    40 |    2934 |
| array_write    |     3 |     7 |     2 |     8 |     2 |     6 |     8 |     5 |     389 |
| array_read     |     4 |     9 |     9 |    10 |     9 |    10 |    12 |    15 |     337 |
| math_intensive |    14 |    46 |    49 |    47 |    47 |    50 |    48 |    50 |    2204 |
| object_create  |     0 |     0 |     0 |     0 |     0 |     4 |     8 |     6 |     158 |
| nested_loops   |    17 |     8 |     8 |     9 |     8 |    10 |    16 |    19 |     470 |
| accumulate     |    33 |    94 |    94 |    94 |    95 |    96 |   585 |    96 |    4916 |

**Honest regressions vs v0.5.164** (when this table was last refreshed,
before generational GC became the default in v0.5.237):
`nested_loops` 8 → 17 ms, `accumulate` 24 → 33 ms. Both caused by
the v0.5.237 gen-GC default flip — the per-allocation gen-GC machinery
(write-barrier potential, age-bump pass) is overhead that allocation-heavy
compute benches don't recoup. `PERRY_GEN_GC=0` recovers the 8 / 24 ms
baseline. The trade-off was deliberate; gen-GC's wins on long-running and
RSS-sensitive workloads (`test_memory_json_churn` 115 → 91 MB) outweigh
the small compute-bench regression. All other cells unchanged or
slightly faster.

**Original v0.5.164 fix** ([#140](https://github.com/PerryTS/perry/issues/140))
that established the current `loop_overhead`/`math_intensive`/`accumulate`
baseline: an `asm sideeffect` loop-body barrier (from #74) and an
over-eager i32 shadow counter were blocking LLVM's vectorizer on pure-
accumulator loops; v0.5.164 scoped the i32 shadow to counters that
actually appear in an index subtree and refined the loop-body barrier
to fire only on truly-empty bodies — restoring the `<2 x double>`
parallel-accumulator reduction (vectorization width 2, interleave count
4). Perry beats Rust 3–8× on these cells.

## How to reproduce

```bash
cd benchmarks/polyglot
bash run_all.sh        # best of 3 runs (default)
bash run_all.sh 5      # best of 5 runs (what the above table used)
```

**Required:** Perry (`cargo build --release` from repo root).
**Optional** (any subset works; missing runtimes show as `-`): Node.js,
Bun, Static Hermes (`shermes`), Rust (`rustc`), C++ (`g++` or `clang++`),
Swift, Go, Java (`javac` + `java`), Python 3.

See [`METHODOLOGY.md`](./METHODOLOGY.md) for what each benchmark measures,
compiler versions, why certain cells look the way they do, and where Perry
wins (`loop_overhead`, `math_intensive`, `accumulate`, `array_read`) vs
where it matches the compiled pack (`nested_loops`, `fibonacci`) vs where
it loses (`object_create`).

## Benchmark-by-benchmark summary

### `loop_overhead` — `sum += 1.0` × 100M
Perry 12 ms vs Rust/C++/Go/Swift/Java ~94 ms — **Perry wins 7–8×**. Two
stacked optimizations produce this: (1) Perry emits `reassoc contract`
on f64 ops because JS/TS `number` semantics can't observe the difference
(no signalling NaNs, no fenv, no strict `-0` rules at the operator
level), so LLVM's IndVarSimplify can rewrite `sum + 1.0 × N` as an
integer induction variable. Rust/C++/Go/Swift default to strict-IEEE
`fadd`, which has a 3-cycle latency wall and is unreassociable. (2) On
top of the integer rewrite LLVM autovectorizes the body into a
`<2 x double>` parallel-accumulator reduction with interleave count 4
(four SIMD lanes worth of fadd happening per iteration). `g++ -O3
-ffast-math` on `bench.cpp` drops C++ from 96 ms to 11 ms, matching
Perry — same LLVM, same pipeline, one flag. See
[RESULTS_OPT.md](./RESULTS_OPT.md) for the per-language opt-sweep (C++
opt = 12 ms, Rust opt = 24 ms on stable, Go = 99 ms because Go has no
fast-math flag).

This cell regressed to 32 ms between v0.5.91 and v0.5.162 after an
`asm sideeffect` barrier (from #74) and an unconditional i32 shadow slot
started blocking the vectorizer. Restored in v0.5.164 via [#140](https://github.com/PerryTS/perry/issues/140).

### `math_intensive` — `result += 1.0/i` × 50M
Perry 14 ms, Rust/C++/Go/Swift/Java/Node/Bun all ~46–49 ms — **Perry
wins 3×**. Same fast-math default + vectorization story as
`loop_overhead`: the reciprocal divide has a 10+ cycle latency, but
vectorizing into a parallel 4-accumulator reduction keeps 4 independent
divides in flight so the scheduler hides the latency. C++ `-ffast-math`
matches Perry at 14 ms per [RESULTS_OPT.md](./RESULTS_OPT.md). Regressed
to 48 ms at v0.5.162, restored in v0.5.164 ([#140](https://github.com/PerryTS/perry/issues/140)).

### `accumulate` — `sum += i % 1000` × 100M
Perry 24 ms, Rust/C++/Go/Swift/Java/Bun all 93–98 ms — **Perry wins 4×**.
Node 583 ms is an outlier because V8 doesn't inline the libm `fmod` call
on this pattern. Perry's type analysis emits `srem` instead of `fmod`
for the mod op (same optimization Node misses), LLVM vectorizes the
resulting integer chain + fadd reduction, and the two together beat
Rust's strict-IEEE scalar `fmod`+`fadd` by 4×. Regressed to 97 ms at
v0.5.162, restored in v0.5.164 ([#140](https://github.com/PerryTS/perry/issues/140)).

### `array_read` — sum 10M-element `number[]`
Perry 3 ms, C++/Swift 9 ms, Rust 10 ms, Go 10 ms, Java 11 ms. Perry
detects `for (let i = 0; i < arr.length; i++)` as statically in-bounds,
skips the JS `undefined`-on-OOB check, caches the length at loop entry,
and maintains a parallel i32 counter so the index is never a float → int
conversion. LLVM then autovectorizes to NEON 2-wide f64. C++ `std::vector`
has no bounds check by default but pays the chunk-boundary check from
`-O3`'s vectorizer framing. Rust's iterator form (not used here) matches
Perry — see `bench_opt.rs` (phase 2).

### `array_write` — `arr[i] = i` × 10M
Perry 2 ms, C++/Swift 2 ms, Rust 7 ms, Go 9 ms. Perry matches C++ here.
The Rust result is `-O` with bounds-checked indexing; `.iter_mut()` would
match Perry.

### `nested_loops` — 3000×3000 flat-array sum
All compiled languages 8–10 ms. Perry 9 ms. This benchmark is
cache-bound, not compute-bound — there is no optimization lever to pull.
Perry matches the compiled pack.

### `fibonacci` — recursive `fib(40)`
Java 280 ms (JIT inlining), C++ 310 ms, Perry 311 ms, Rust 319 ms — the
top four languages all land within 10 ms of each other. Perry's type
inference refines the TS `number` parameter to `i64` (because the function
only ever performs integer operations), producing `add/sub/icmp` (1 cycle
each) instead of the `fadd/fsub/fcmp` (2–3 cycles) that the f64-typed Rust
and C++ benchmarks emit. The reason Perry isn't dramatically further
ahead is that LLVM's recursion-folding optimizations on fib-shaped code
recover most of the gap at -O3. The Rust `f64→i64` switch is a one-line
change (tested in `bench_opt.rs`) and drops Rust to ~280 ms.

### `object_create` — allocate 1M `{x, y}` pairs, sum fields
Rust/C++/Go/Swift 0 ms: the compiler proves the struct never escapes and
eliminates the whole loop. Java 5 ms, Bun 5 ms, Node 8 ms, Perry 2 ms,
Hermes 2 ms. Perry is competitive here only because of the v0.5.17
scalar-replacement pass; without it this benchmark was ~10 ms. The 0 ms
floor from statically-typed compiled languages is an inherent tradeoff of
compiling a dynamic language — see `METHODOLOGY.md`.

## Source files

- `bench.cpp` — C++17
- `bench.rs` — Rust (no dependencies)
- `bench.go` — Go
- `bench.swift` — Swift
- `bench.java` — Java
- `bench.py` — Python 3
- `bench.zig` — Zig (may need manual build; not in the current table)
- Perry / Node / Bun / Hermes run the TS files in `../suite/`

All implementations use the same algorithm, same data types (`f64` /
`double` throughout), same iteration counts, and the same output format
(`benchmark_name:elapsed_ms`) so the runner can grep a single key per row.
