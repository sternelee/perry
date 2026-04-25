# Perry Benchmarks

This is the canonical, single-page comparison of Perry against
production-quality runtimes — **node, bun, Rust, Go, C++, Swift,
Java, Python, Zig**. It pulls together every benchmark in this
repo, lists the exact compiler flags used per language, calls out
where Perry leads and where it doesn't, and links to the design
docs that explain *why* the numbers look the way they do.

The format is designed for skeptics. Every implementation, every
flag, every methodology decision is in this page — no tables hidden
behind blog posts, no cherry-picked subsets.

> **Hardware:** Apple M1 Max (10 cores: 8P + 2E), 64 GB RAM, macOS
> 26.4. Default scheduling — no affinity pinning, no `taskset`,
> no thermal-throttle mitigation beyond best-of-N. Numbers from
> 2026-04-25 unless otherwise stated.
>
> **Methodology:** best-of-5 runs per cell, monotonic clock, no
> warmup unless noted (JS-family runtimes get a 3-iteration
> warmup before timed iterations to avoid charging Perry / Bun /
> Node for JIT cold-start). Time in milliseconds, RSS in MB, peak
> values from `/usr/bin/time -l`.

---

## TL;DR

### JSON parse + stringify (10k records, 50 iterations, ~1 MB blob)

| Implementation | Profile | Time (ms) | Peak RSS (MB) |
|---|---|---:|---:|
| **perry (gen-gc + lazy tape)** | optimized | **67** | 85 |
| rust serde_json (LTO+1cgu) | optimized | 183 | 11 |
| rust serde_json | idiomatic | 193 | 11 |
| bun | idiomatic | 240 | 81 |
| perry (mark-sweep, no lazy) | idiomatic | 341 | 102 |
| node | idiomatic | 361 | 180 |
| node --max-old=4096 | optimized | 364 | 182 |
| kotlin -server -Xmx512m (kotlinx.serialization) | optimized | 446 | 423 |
| kotlin (kotlinx.serialization) | idiomatic | 460 | 606 |
| c++ -O3 -flto (nlohmann/json) | optimized | 774 | 25 |
| go (encoding/json) | optimized | 783 | 22 |
| go (encoding/json) | idiomatic | 785 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 840 | 25 |
| swift -O -wmo (Foundation) | optimized | 3665 | 34 |
| swift -O (Foundation) | idiomatic | 3674 | 33 |

**Reading this**: Perry leads on time, beating every JS-family
runtime (Node, Bun) and every native runtime (Rust, Go, C++,
Swift, Kotlin). Perry's RSS is mid-pack — better than Node and
Kotlin (the JVM heap reservation is enormous), comparable to Bun,
higher than typed-struct languages (Go, Rust, C++). The RSS gap
to typed-struct languages is fundamental: dynamic JSON parsing
allocates a heap object per value; typed parsers materialize into
fixed-layout structs. Kotlin's high RSS reflects JVM heap
reservation, not working-set size.

### Compute microbenches (8 benchmarks, idiomatic flags)

Best of 5 runs, all times in milliseconds.

| Benchmark      | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun |  Python |
|----------------|------:|------:|------:|------:|------:|------:|------:|------:|--------:|
| fibonacci      |   302 |   314 |   304 |   440 |   394 |   276 |   991 |   510 |   15661 |
| loop_overhead  |    12 |    95 |    94 |    94 |    94 |    96 |    52 |    40 |    2934 |
| array_write    |     3 |     7 |     2 |     8 |     2 |     6 |     8 |     5 |     389 |
| array_read     |     4 |     9 |     9 |    10 |     9 |    10 |    12 |    15 |     337 |
| math_intensive |    14 |    46 |    49 |    47 |    47 |    50 |    48 |    50 |    2204 |
| object_create  |     0 |     0 |     0 |     0 |     0 |     4 |     8 |     6 |     158 |
| nested_loops   |    17 |     8 |     8 |     9 |     8 |    10 |    16 |    19 |     470 |
| accumulate     |    33 |    94 |    94 |    94 |    95 |    96 |   585 |    96 |    4916 |

Perry's **`loop_overhead`, `math_intensive`, `accumulate`, `array_read`**
wins (3-8× over native) come from a single source: Perry emits
`reassoc contract` on f64 ops because TypeScript's `number` semantics
can't observe the difference (no signalling NaNs, no fenv, no strict
`-0` rules at the operator level), so LLVM's IndVarSimplify can rewrite
`sum + 1.0 × N` as an integer induction variable and the autovectorizer
generates `<2 x double>` parallel-accumulator reductions with interleave
count 4. C++ closes the gap with `-O3 -ffast-math`; see
[`benchmarks/polyglot/RESULTS_OPT.md`](polyglot/RESULTS_OPT.md) for
the per-language flag-tuning table that backs out this entire result.

`fibonacci` (302 ms): Perry matches the compiled pack within 2-12ms
(Java's HotSpot JIT is ~9% faster). `object_create`: tied with native
(all 0 ms — working set fits in one arena block, GC never fires).

**Honest regressions vs the v0.5.164 baseline** (when these benches
were last refreshed, before gen-GC became default):

- `nested_loops` 8 → 17 ms (+9 ms). Caused by the v0.5.237
  generational GC default flip — gen-GC adds per-allocation overhead
  (write-barrier potential, age-bump pass) that's pure cost on
  workloads that don't benefit from it. Set `PERRY_GEN_GC=0` to recover
  the 8 ms baseline.
- `accumulate` 24 → 33 ms (+9 ms). Same root cause; same
  workaround.
- `array_read` 3 → 4 ms (+1 ms). Within noise.
- All other cells unchanged or slightly improved (`fibonacci`
  309 → 302, `array_write` 3 → 3, `math_intensive` 14 → 14).

The trade-off was deliberate: gen-GC's wins on long-running and
allocation-heavy workloads (`test_memory_json_churn` 115 → 91 MB
in v0.5.237) outweigh the small compute-bench regressions, and
the escape hatch is right there. Listed here unapologetically
because the point of this page is to be defensible.

---

## How to read this page

The **compute microbenches** measure compiler choices: loop iteration
throughput, arithmetic latency, sequential array access, recursive
call overhead, object allocation patterns. These are probes into
specific code-generation behavior, not workload simulators. Don't
extrapolate to "language X is N× faster than Y on real applications".

The **JSON benchmark** is closer to real-world: parse a 1 MB structured
JSON blob (10k records, each with 5 fields including a nested object
and a string array), stringify it, repeat 50 times. This catches GC
pressure, allocator throughput, encoding/decoding pipeline cost — the
things real services spend most of their time on.

The **memory benchmarks** are RSS-plateau and GC-aggression regression
tests. They run sustained allocate-and-discard loops for 200k iterations
and assert RSS stays under a per-test ceiling. They catch slow leaks
that microbenchmarks miss.

Every entry below is run twice — **idiomatic** (the language's default
release-mode build, what most projects ship with) and **optimized**
(aggressive flags: LTO, single codegen unit, fast-math where applicable,
etc.). This is intentional. Some readers correctly point out that
"Perry's defaults are themselves aggressive" — so we show every
language's full ceiling, not just its conservative starting point.

---

## 1. JSON polyglot — full data

[`benchmarks/json_polyglot/`](json_polyglot/) — implementation sources +
runner.

### Workload

```typescript
const items = [];
for (let i = 0; i < 10000; i++) {
  items.push({
    id: i,
    name: "item_" + i,
    value: i * 3.14159,
    tags: ["tag_" + (i % 10), "tag_" + (i % 5)],
    nested: { x: i, y: i * 2 }
  });
}
const blob = JSON.stringify(items);  // ~1 MB

// 50 iterations
for (let iter = 0; iter < 50; iter++) {
  const parsed = JSON.parse(blob);
  JSON.stringify(parsed);
}
```

Identical workload in 7 languages: TypeScript (run on Perry / Bun /
Node), Go, Rust, Swift, C++. Each language's implementation lives in
[`bench.<ext>`](json_polyglot/) with the same checksumming logic so
correctness is verifiable.

### Compiler flags used (verbatim)

| Profile | Language | Flags |
|---|---|---|
| optimized | Perry | `cargo build --release -p perry` (LLVM `-O3` equivalent, lazy JSON tape default for ≥1 KB blobs since v0.5.210, gen-GC default ON since v0.5.237) |
| idiomatic | Perry (escape hatch) | `PERRY_GEN_GC=0 PERRY_JSON_TAPE=0` (full mark-sweep, no lazy parse) — included for honesty so a skeptic can see the un-tuned floor |
| idiomatic | Bun | `bun bench.ts` (no flags — Bun is JIT, no compile step) |
| idiomatic | Node | `node --experimental-strip-types bench.ts` |
| optimized | Node | `node --experimental-strip-types --max-old-space-size=4096 bench.ts` |
| idiomatic | Go | `go build` (default) |
| optimized | Go | `go build -ldflags="-s -w" -trimpath` (smaller binary; ~no perf delta — included for completeness, see "honest disclaimers" below) |
| idiomatic | Rust | `cargo build --release` (`opt-level=3`, `lto=false`, `codegen-units=16`) |
| optimized | Rust | `cargo build --profile release-aggressive` (`opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic=abort`, `strip=true`) |
| idiomatic | Swift | `swiftc -O bench.swift` |
| optimized | Swift | `swiftc -O -wmo bench.swift` (whole-module optimization) |
| idiomatic | Kotlin | `java -cp ... BenchKt` (JVM defaults, kotlinx.serialization) |
| optimized | Kotlin | `java -server -Xmx512m -cp ... BenchKt` (server JIT + heap tuning) |
| idiomatic | C++ | `clang++ -std=c++17 -O2` |
| optimized | C++ | `clang++ -std=c++17 -O3 -flto` |

### JSON libraries used

| Language | Library | Why this one |
|---|---|---|
| Perry | built-in `JSON.parse` / `JSON.stringify` (with optional [lazy tape](../docs/json-typed-parse-plan.md)) | Standard JS API, no library to choose |
| Bun / Node | built-in `JSON.parse` / `JSON.stringify` | Standard JS API |
| Go | `encoding/json` | Standard library; what every Go project starts with |
| Rust | `serde_json` (1.0) | The de facto standard; ~ubiquitous in the Rust ecosystem |
| Swift | `Foundation.JSONEncoder` / `JSONDecoder` | Apple's standard |
| Kotlin | `kotlinx.serialization-json` (1.9.0) | The official Kotlin serialization library; uses compile-time-generated (de)serializers, no reflection |
| C++ | nlohmann/json (3.12.0) | The de facto popular C++ JSON library; not the fastest available (RapidJSON / simdjson are faster) but what most projects reach for |

**Faster C++ libraries exist** (RapidJSON, simdjson). We deliberately
benchmark nlohmann/json because that's what real C++ projects use 90%
of the time. If you need to compare against simdjson, it would beat
Perry on time for *parse-only* workloads (it's SIMD-accelerated parse,
no stringify).

### Honest disclaimers on the JSON numbers

- **Perry's `lazy tape` win is workload-specific.** On
  parse-then-iterate-every-element workloads, lazy tape is a net
  loss — it pays the tape build cost without amortizing the
  materialize-on-demand savings. On parse-then-`.length`-or-
  stringify workloads (which this bench is), lazy tape wins
  decisively. See [`audit-lazy-json.md`](../docs/audit-lazy-json.md)
  for the access-pattern matrix.
- **Rust's RSS lead is fundamental.** Rust's serde_json
  deserializes into typed structs (Vec<Item> with stack-laid-out
  fields). Perry, Bun, Node parse into dynamic heap objects (one
  alloc per value). The 8× RSS gap (11 MB Rust vs 85 MB Perry) is
  the cost of dynamic typing — it can't be closed without giving up
  TypeScript's `any` semantics. The fix is to teach Perry's parser
  about typed targets at compile time; tracked as
  [`json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md)
  (Step 2 partially done; more in flight).
- **Go's `optimized` ≈ idiomatic.** `-ldflags="-s -w" -trimpath`
  strips debug info; no measurable perf delta. Included so the
  table doesn't look like Go was unfairly held back. Go has no
  `-ffast-math` flag; `accumulate` and `loop_overhead` deltas in
  the compute table are unrecoverable in stock Go.
- **Swift's slow time is real, not a setup problem.** `-O -wmo`
  is what Swift Package Manager release builds use. The Foundation
  JSON pipeline goes through `Mirror`-based reflection on `Codable`
  types and is genuinely slow on macOS. swift-json is faster; not
  included because this is the standard.
- **Kotlin's RSS is JVM heap reservation, not working-set.** The
  JVM eagerly reserves up to `-Xmx` even when actual heap usage is
  much smaller. `-Xmx512m` gives 423 MB peak RSS; default settings
  reserve more (606 MB observed). The actual JSON working-set in
  Kotlin is comparable to Java/JVM peers. The 423-606 MB RSS
  number is correct for "what the OS sees the process holding"
  but is not a fair comparison of allocator efficiency.
- **Perry's "mark-sweep, no lazy" entry isn't recommended for
  production** — it disables the lazy JSON tape (v0.5.210) and the
  generational GC default (v0.5.237). It exists so you can see the
  untuned floor and compare against it.

---

## 2. Compute microbenches — full data

[`benchmarks/polyglot/`](polyglot/) — 10 implementations across 8
benchmarks. Existing run, last refreshed 2026-04-22 at v0.5.164.

### Idiomatic flags table (current)

See [`RESULTS.md`](polyglot/RESULTS.md) for the full table reproduced
in the TL;DR above. Compiler details:

| Language | Compiler | Idiomatic flag |
|---|---|---|
| Perry | self-hosted Rust, LLVM 22 | `cargo build --release -p perry` |
| Rust | rustc 1.85 stable | `cargo build --release` |
| C++ | clang++ 17 (Apple) | `clang++ -O3 -std=c++17` |
| Go | go 1.21 | `go build` |
| Swift | swiftc 6.0 (Apple) | `swiftc -O` |
| Java | javac 21 + java 21 (HotSpot) | default `java -cp .` |
| Kotlin (JSON only) | kotlinc 2.3.21 | `java -cp ... BenchKt` |
| Node.js | v20 | `node --experimental-strip-types` |
| Bun | 1.3 | `bun` |
| Static Hermes | shermes 0.13 | `shermes -O` (skipped if not installed) |
| Python | 3.12 | `python3` |

Kotlin is JSON-only (not in the compute polyglot table) because the
compute polyglot runner predates Kotlin support; adding it would
require porting the 8-benchmark `bench.kt` to match the existing
`bench.cpp`/`bench.go`/etc. shape. Tracked as a follow-up.

### Optimized flags + delta table

[`RESULTS_OPT.md`](polyglot/RESULTS_OPT.md) holds the full opt-tuning
sweep. Highlights:

- **C++ `-O3 -ffast-math` matches Perry to the millisecond** on
  `loop_overhead` (12 = 12) and `math_intensive` (14 = 14).
- **Rust on stable can't reach Perry on `loop_overhead`** because
  there's no way to expose LLVM's `reassoc` flag on individual
  fadd instructions without nightly's `fadd_fast` intrinsic. With
  manual i64 accumulator + iterator form: 99 → 24 ms (still 2× off).
- **Go cannot close the gap at all**: no `-ffast-math`, no
  `reassoc` flag, the Go compiler doesn't ship that pipeline.
- **Swift `-O -wmo` closes 71-75% of the gap** on
  `loop_overhead` / `math_intensive` / `accumulate`.

### What each microbench actually measures

[`METHODOLOGY.md`](polyglot/METHODOLOGY.md) — full
benchmark-by-benchmark explanation: what's in the inner loop, what
LLVM does with it, what each language's compiler does differently,
why the cell is the number it is. Read this if you suspect any cell
of being unfair.

---

## 3. Memory + GC stability

[`scripts/run_memory_stability_tests.sh`](../scripts/run_memory_stability_tests.sh)
+ [`test-files/test_memory_*.ts`](../test-files/) +
[`test-files/test_gc_*.ts`](../test-files/) — 6 tests × 3 GC mode
combos (default / mark-sweep escape hatch / gen-gc + write
barriers) = 18 runs per CI invocation.

### What each test catches

All numbers from the most recent run on this commit (M1 Max, macOS
26.4). The test asserts RSS stays under the per-test ceiling; the
"Current" column is the actual measured peak.

| Test | What it catches | RSS limit | default | mark-sweep | gen-gc+wb |
|---|---|---:|---:|---:|---:|
| `test_memory_long_lived_loop.ts` | Block-pinning, PARSE_KEY_CACHE leak, tenuring-trap regressions | 100 MB | 54 MB | 54 MB | 54 MB |
| `test_memory_json_churn.ts` | Sparse-cache leak, materialized-tree retention, tape-buffer leak | 200 MB | 91 MB | 91 MB | 91 MB |
| `test_memory_string_churn.ts` | SSO-fast-path-miss alloc, heap-string GC loss | 100 MB | 48 MB | 48 MB | 48 MB |
| `test_memory_closure_churn.ts` | Box leak, closure-env retention, shadow-stack slot leak | 50 MB | 13 MB | 13 MB | 13 MB |
| `test_gc_aggressive_forced.ts` | Conservative-scanner misses, parse-suppressed interleaving, write-barrier mid-mutation | 50 MB | 9 MB | 9 MB | 9 MB |
| `test_gc_deep_recursion.ts` | Stack-scan correctness during deep recursion | 30 MB | 6 MB | 6 MB | 6 MB |

All 18 cells (6 tests × 3 modes) PASS on this commit.

`test_memory_json_churn` dropped from 115 MB → **91 MB** when the
generational-GC default flipped to ON in v0.5.237 (-21%).

### bench_json_roundtrip RSS history

Direct path (`PERRY_JSON_TAPE=0`, 50 iterations of 10k-record parse +
stringify, peak RSS via `/usr/bin/time -l`):

| Version | RSS (MB) | Time (ms) | Change |
|---|---:|---:|---|
| pre-tier-1 (v0.5.193) | ~213 | ~322 | baseline |
| v0.5.198 (threshold 64 MB) | 144 | 364 | tuned initial threshold |
| v0.5.231 (C4b-γ-1, evac no-op) | 109 | ~80 | block-persist + tenuring + arena fixes |
| v0.5.234 (C4b-γ-2, evac live) | 142 | 358 | rebuilt baseline (post-other-changes) |
| v0.5.235 (C4b-δ, dealloc) | 142 | 358 | dealloc fires but peak is pre-first-GC |
| v0.5.236 (C4b-δ-tune, ceiling) | 107 | 358 | trigger ceiling stops step doubling past 64 MB |
| v0.5.237 (gen-gc default ON) | 102 | 372 | minor GC fires by default |
| v0.5.241 (current, this commit) | **102** | **375** | unchanged from v0.5.237; suite re-run for this README |

Default (lazy + gen-gc), the case `bench_json_roundtrip` measures with
no env vars: **66 ms / 85 MB**, currently best in class on time across
every other measured runtime.

### Other Perry benches (best-of-5, M1 Max, this commit)

| Benchmark | Time (ms) | Peak RSS (MB) |
|---|---:|---:|
| `bench_json_roundtrip` (default, lazy + gen-gc) | 66 | 85 |
| `bench_json_roundtrip` (`PERRY_JSON_TAPE=0`) | 375 | 102 |
| `bench_json_roundtrip` (`PERRY_GEN_GC=0`) | 66 | 85 |
| `bench_json_roundtrip` (both opts off) | 349 | 102 |
| `bench_json_readonly` (default) | 67 | 81 |
| `bench_json_readonly` (`PERRY_JSON_TAPE=0`) | 279 | 103 |
| `07_object_create` | 0 | 6 |
| `12_binary_trees` | 0 | 6 |
| `bench_gc_pressure` | 16 | 25 |
| `04_array_read` | 4 | 211 |
| `05_fibonacci` | 309 | 6 |
| `08_string_concat` | 0 | 6 |

---

## 4. Strengths

Where Perry actually wins, and a one-line "why" per item.

- **JSON parse + stringify roundtrip** (this page's TL;DR) — Perry is
  faster than every other measured runtime: 3.6× over Bun, 5.4× over
  Node, 2.7× over Rust serde_json (LTO), 6.7× over Kotlin
  kotlinx.serialization (server JIT), 11.6× over C++ nlohmann -O3
  -flto, 11.7× over Go encoding/json, 54.7× over Swift Foundation.
  The win comes from the lazy JSON tape (v0.5.204+): parse builds a
  12-byte-per-value tape instead of materializing a tree; stringify
  on an unmutated parse memcpy's the original blob. See
  [`json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md).
- **f64-arithmetic-heavy tight loops** (`loop_overhead`,
  `math_intensive`, `accumulate`) — 3-8× faster than native because
  TypeScript's `number` semantics let LLVM apply `reassoc contract`
  flags that strict-IEEE languages can't. C++ `-O3 -ffast-math`
  closes this gap; nothing else on the list can.
- **Object allocation in tight loops** (`object_create`, 1M iters) —
  ties native (0 ms). Working set fits in one arena block; GC never
  fires; the inline bump allocator is ~5 instructions per `new`.
- **Generational GC defaults that adapt** (`test_memory_json_churn`
  dropped 115 → 91 MB just from flipping the default) — the
  Bartlett-style mostly-copying generational implementation
  (v0.5.234-237) catches sustained-allocation workloads that pure
  mark-sweep handles poorly.

---

## 5. Weaknesses

The ones we already know about and what's tracked:

- **RSS on dynamic-JSON workloads is high vs typed-struct
  languages.** 85 MB vs Rust's 11 MB on the bench above. Fundamental
  to dynamic typing — every JSON value is a heap NaN-boxed object.
  Mitigation in flight: typed JSON parse (`JSON.parse<T>(blob)`) lets
  the compiler emit packed-keys pre-resolution.
  Step 1 done in v0.5.200.
- **GC pause is stop-the-world.** No concurrent marking. On
  `bench_gc_pressure`, this is 1-2 ms per cycle. On a multi-GB heap
  it would be much more. Tracked as a follow-up in
  [`generational-gc-plan.md`](../docs/generational-gc-plan.md)'s
  "Other parked items" section.
- **No old-generation compaction.** V8, JSC, HotSpot all compact
  old-gen; Perry doesn't. Fragmentation eventually accumulates;
  tracked as a follow-up.
- **Shadow stack is opt-in for the tracer's precision win.** The
  conservative C-stack scan still runs unconditionally because
  shrinking it requires platform-specific FP-chain walking; deferred
  with rationale in
  [`generational-gc-plan.md`](../docs/generational-gc-plan.md)
  §"Deferred follow-ups".
- **TypeScript parity gaps.** 28-test gap-test suite, 18 currently
  passing. Known categorical gaps (lookbehind regex, `console.dir`
  formatting, lone surrogate handling) tracked at
  [`typescript-parity-gaps.md`](../docs/typescript-parity-gaps.md).
- **No JIT.** Compiled code is fixed at build time. JS-engine JIT
  warmup gives V8/JSC a long-tail advantage on iteration-heavy code
  that Perry can't match.
- **Single-threaded by default.** `perry/thread` provides
  parallelMap / spawn but values cross threads via deep-copy
  serialization (no SharedArrayBuffer). Real shared-memory threading
  is not implemented.
- **No incremental / concurrent compilation.** Build time is
  monolithic; incremental rebuilds in v0.5.143's `perry dev` watch
  mode help but full compiles are not yet incremental.

---

## 6. What this page does not measure

- **GC latency / tail latency.** Reported numbers are throughput
  (best-of-5 wall clock). A 99th-percentile pause measurement would
  show Perry's stop-the-world GC at a disadvantage vs Go's concurrent
  collector or HotSpot ZGC.
- **JIT warmup behavior.** JS-family runtimes (Node, Bun) get
  3-iteration warmup before timed iterations to avoid charging them
  for cold-JIT compilation. Real cold-start latency is much worse for
  V8 / JSC than for Perry / Go / Rust binaries.
- **Async / await.** Every benchmark on this page is synchronous.
  Async runtime overhead, event-loop scheduling, microtask draining
  — not measured here.
- **I/O.** No file, network, or DB benchmark. Perry's `perry/thread`
  + tokio integration for HTTP / WebSocket / DB is benchmarked
  separately (see [`docs/`](../docs/) — partial).
- **Realistic application workloads.** Microbenches are probes,
  not workload simulators. The "Perry is X× faster than Y" claim
  is only true on the specific workload shape measured.
- **Memory pressure under contention.** All benches run on an
  otherwise-idle machine. Behavior under co-located-tenant pressure
  is not measured.
- **Compile time / binary size.** Perry compiles slower than Go (Go
  is famously fast at compile-time). Binary size is ~1 MB for hello
  world; comparable to Go but bigger than Rust release binaries with
  panic=abort + strip.

---

## 7. Reproducing

### JSON polyglot

```bash
# In repo root, build Perry:
cargo build --release -p perry-runtime -p perry-stdlib -p perry

# Install the C++ JSON dependency (macOS):
brew install nlohmann-json

# Run the polyglot suite:
cd benchmarks/json_polyglot
./run.sh             # best-of-5 (default)
RUNS=10 ./run.sh     # best-of-10 for tighter numbers
```

Outputs `benchmarks/json_polyglot/RESULTS.md` with the full table.

### Compute microbenches

```bash
cd benchmarks/polyglot
./run_all.sh         # best-of-3
./run_all.sh 5       # best-of-5 (what RESULTS.md uses)
```

Missing language toolchains show as `-` in the table; the script
degrades gracefully.

### Memory stability tests

```bash
bash scripts/run_memory_stability_tests.sh
```

Runs 18 test combinations (6 tests × 3 GC modes), prints PASS/FAIL +
RSS per cell. Wired into CI via `.github/workflows/test.yml`.

---

## 8. Design / implementation references

- [`docs/generational-gc-plan.md`](../docs/generational-gc-plan.md) —
  the GC architecture: phases A-D, write barriers, evacuation,
  conservative pinning, plus the academic + industry lineage
  appendix (Bartlett 1988, Ungar 1984, Cheney 1970, etc.).
- [`docs/json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md) —
  the JSON pipeline design: tape format, lazy materialization,
  typed-parse plan.
- [`docs/audit-lazy-json.md`](../docs/audit-lazy-json.md) — external
  reviewer reference for the lazy-parse correctness guarantees +
  access-pattern matrix.
- [`docs/memory-perf-roadmap.md`](../docs/memory-perf-roadmap.md) —
  RSS optimization roadmap (tier 1: NaN-boxing, tier 2: SSO, tier 3:
  generational GC).
- [`docs/sso-migration-plan.md`](../docs/sso-migration-plan.md) —
  Small String Optimization rollout sequencing.
- [`benchmarks/polyglot/METHODOLOGY.md`](polyglot/METHODOLOGY.md) —
  per-microbenchmark explanation, compiler versions, why each cell
  is the number it is.
- [`CHANGELOG.md`](../CHANGELOG.md) — every version, every change,
  with measured impact where applicable.

If you spot something that looks unfair, biased, or wrong: open an
issue at https://github.com/PerryTS/perry/issues with the
benchmark name, your alternative implementation, and the toolchain
versions you ran with. The point of this page is to be defensible,
not to win. Numbers that don't survive scrutiny don't belong here.
