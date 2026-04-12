# LLVM vs Cranelift — Post-Cutover Benchmark Reality

This document was originally written before the Phase K hard cutover
to argue that LLVM was faster based on the Phase 2.1 measurement
(`100 × fib(35)`: LLVM 3536 ms vs Cranelift 6312 ms, **−44%**). It has
been **rewritten with the actual post-cutover numbers** because that
Phase 2.1 measurement turned out to be misleading: it was taken on a
stripped-down LLVM that did not yet pay the full NaN-boxing cost
introduced in Phase A. The honest numbers are below.

The Phase K hard cutover landed in commit `38bdf9f` (v0.5.0). The
parity sweep is **identical pre/post cutover** (102 MATCH / 9 DIFF /
0 CRASH / 13 NODE_FAIL — 91.8% match rate). This document is about
**performance**, which is a separate question.

## Headline result — Perry (LLVM) vs Node.js vs Bun

Median of 3 runs, macOS aarch64 (Apple Silicon, M-series), Apr 2026.
Node 24.4.1, Bun 1.3.5. Run with `cd benchmarks/suite && ./run_benchmarks.sh`.

| Benchmark        | Perry (LLVM) | Node.js | Bun    | vs Node | vs Bun  | Notes |
|------------------|--------------|---------|--------|---------|---------|---|
| string_concat    | 0–1 ms       | 2 ms    | 1 ms   | **2x faster** | tied | Inline string-builder fast path |
| closure          | 139 ms       | 305 ms  | 51 ms  | **2.2x faster** | 0.4x | Closure conversion is competitive |
| cold start       | 66 ms        | 119 ms  | 37 ms  | **1.8x faster** | 0.6x | Native binary, no JIT warmup |
| loop_overhead    | 98 ms        | 70 ms   | 40 ms  | 0.7x   | 0.4x  | Tight integer loop |
| array_write      | 20 ms        | 8 ms    | 5 ms   | 0.4x   | 0.25x | NaN-box per write |
| array_read       | 26 ms        | 13 ms   | 15 ms  | 0.5x   | 0.6x  | NaN-box per read |
| prime_sieve      | 11 ms        | 8 ms    | 6 ms   | 0.7x   | 0.5x  | Boolean array + branches |
| mandelbrot       | 47 ms        | 25 ms   | 29 ms  | 0.5x   | 0.6x  | f64 math, V8 has SIMD |
| nested_loops     | 57 ms        | 17 ms   | 20 ms  | 0.3x   | 0.35x | Nested f64 loops, V8 vectorizes |
| math_intensive   | 131 ms       | 50 ms   | 51 ms  | 0.4x   | 0.4x  | Harmonic series |
| matrix_multiply  | 184 ms       | 34 ms   | 34 ms  | 0.18x  | 0.18x | Nested loops, NaN-box per access |
| object_create    | 318 ms       | 8 ms    | 5 ms   | 0.025x | 0.016x | Property dispatch through runtime |
| binary_trees     | 479 ms       | 10 ms   | 7 ms   | 0.02x  | 0.015x | Tree allocation + traversal |
| factorial        | 1639 ms      | 604 ms  | 101 ms | 0.37x  | 0.06x | BigInt path |
| fibonacci(40)    | 1156 ms      | 1001 ms | 520 ms | 0.87x  | 0.45x | Recursive function calls |
| method_calls     | 1084 ms      | 11 ms   | 7 ms   | 0.01x  | 0.006x | 10M dispatches via runtime |

**Summary: 2 faster / 13 slower vs Node, 1 faster / 14 slower vs Bun.**

## What got better post-cutover

- **Single binary**: Perry produces a 533 KB self-contained executable
  (vs Bun's 57 MB runtime). No installation, no JIT warmup, no
  deopt cliffs.
- **Cold start**: 66 ms (vs Node 119 ms, Bun 37 ms). Faster than Node
  by ~2x.
- **Closure conversion**: 2.2x faster than Node on the closure benchmark.
- **String concatenation**: at-or-faster than Node and Bun on the
  in-place string-builder pattern.
- **Codebase weight**: −54,392 lines deleted in commit `38bdf9f`. The
  Cranelift backend (12 files, 53,760 LOC) is gone. The LLVM backend
  (perry-codegen, ~17K LOC) is the only codegen path.
- **Architectural simplicity**: one IR builder, one runtime ABI, one
  set of runtime decls. The `--backend` CLI flag is gone.

## What got slower post-cutover

The benchmarks above show Perry **slower than Node on 13 of 16
workloads**, often dramatically (method_calls and binary_trees are
~50–100x slower than Node). This is a regression vs the README's
**pre-cutover Cranelift numbers** which had Perry at ~Node-equivalent
or faster on most benchmarks.

The main reason is **NaN-boxing overhead per value access**. Cranelift's
hand-tuned IR generation inlined the box/unbox dance and bypassed many
runtime calls (`inline_nanbox_string`, `inline_get_string_pointer`,
direct `fcmp` for known-numeric operands, etc.). The LLVM backend
currently:

1. Boxes every value as NaN-tagged f64
2. Unboxes via runtime helpers on hot paths (instead of inlined IR)
3. Routes some method dispatch through `js_native_call_method` (the
   universal fallback) instead of direct calls
4. Reads object fields via `js_object_get_field_by_name` instead of
   direct loads with shape caching

The **Phase 2.1 measurement** that originally motivated the cutover
(LLVM −44% on `100 × fib(35)`) was taken on a Phase 2.1 LLVM that
didn't yet box anything — values flowed as raw f64 with no tag bits.
After Phase A landed NaN-boxing for the full value flow, the perf
collapsed to today's numbers. The headline number was real for that
configuration but didn't reflect the boxed reality the cutover ended
up with.

## What this means for v0.5.0

- **Correctness wins**: identical parity sweep, 91.8% match rate, full
  feature surface (classes, async, closures, exceptions, generators,
  symbols, typed arrays, crypto, fs, etc.).
- **Architecture wins**: simpler codebase, single backend, smaller
  source footprint, modern toolchain integration.
- **Performance has regressed** vs the pre-cutover Cranelift baseline
  on most micro-benchmarks. The optimization headroom is large (LLVM
  has many more knobs than Cranelift) but the work hasn't been done yet.

The next perf milestone is closing this gap. Concretely:

1. **Inline the NaN box/unbox dance** in the LLVM IR generator instead
   of calling `js_nanbox_*` helpers — this is the single biggest
   contributor to the method_calls / array_read / fibonacci regressions.
2. **Direct field access** via shape caching instead of dynamic
   property dispatch through `js_object_get_field_by_name`.
3. **Devirtualize known method calls** at codegen time — `counter.increment()`
   on a known `Counter` instance should compile to a direct call to
   `perry_method_Counter_increment(this)`, not a `js_native_call_method`
   round trip.
4. **Re-enable the Cranelift-era inline `fcmp` fast path** for known-
   numeric comparisons.
5. **Bitcode-link mode by default** (`PERRY_LLVM_BITCODE_LINK=1`) — Phase
   J landed v0.4.90 but the env-var gate is still in place.

These are tractable optimizations. None require an architectural change.

## Binary size + memory

| Metric    | Perry (LLVM) | Node.js | Bun |
|-----------|--------------|---------|-----|
| Binary    | 533 KB       | 30 B (script) | 57 MB (runtime) |
| Peak RSS  | 120 MB       | 74 MB   | 48 MB |

Perry's binary is the smallest by ~100x vs Bun. Peak RSS is currently
higher than both Node and Bun — this is GC arena sizing (Perry uses
8 MB arena blocks; tighter sizing or generational GC would improve this).

## Sources

- `benchmarks/suite/run_benchmarks.sh` — current run script
- Commit `38bdf9f` (v0.5.0) — Phase K hard cutover
- Commit `15eb485` — sanitize() digit-prefix fix that unblocked the
  `0X_*.ts` benchmark suite from compiling under LLVM
- README.md `Performance` section (lines 51–86) — pre-cutover Cranelift
  numbers, kept as historical reference
