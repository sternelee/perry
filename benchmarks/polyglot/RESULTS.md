# Polyglot Benchmark Results

Perry vs 7 languages on 8 identical benchmarks. All implementations use `f64`/`double` arithmetic to match TypeScript's `number` type. No SIMD intrinsics, no unsafe code — standard idiomatic code in each language.

## Results

Best of 3 runs, macOS ARM64 (Apple Silicon M-series), April 2026.

| Benchmark      | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |  Python |
|----------------|-------|-------|-------|-------|-------|-------|-------|---------|
| fibonacci      |   309 |   316 |   309 |   446 |   399 |   279 |   991 |   15935 |
| loop_overhead  |    12 |    95 |    96 |    96 |    95 |    97 |    53 |    2979 |
| array_write    |     2 |     6 |     2 |     8 |     2 |     6 |     8 |     392 |
| array_read     |     4 |     9 |     9 |    10 |     9 |    11 |    13 |     330 |
| math_intensive |    14 |    48 |    50 |    48 |    48 |    50 |    49 |    2212 |
| object_create  |     8 |     0 |     0 |     0 |     0 |     4 |     8 |     161 |
| nested_loops   |     8 |     8 |     8 |     9 |     8 |    10 |    17 |     470 |
| accumulate     |    25 |    98 |    96 |    96 |    96 |   100 |   592 |    4919 |

All times in milliseconds. Lower is better.

## How to reproduce

```bash
cd benchmarks/polyglot
bash run_all.sh        # best of 3 runs (default)
bash run_all.sh 5      # best of 5 runs
```

**Requirements:** Perry (built from this repo), Node.js, Go, Rust (`rustc`), C++ (`g++` or `clang++`), Swift, Java (`javac` + `java`), Python 3. Zig is optional (currently skipped due to macOS SDK compatibility). All must be in `$PATH`.

**What the script does:**
1. Builds Perry from source (`cargo build --release`)
2. Compiles each Perry benchmark `.ts` to a native binary
3. Compiles `bench.cpp` with `g++ -O3`, `bench.rs` with `rustc -O`, `bench.swift` with `swiftc -O`, `bench.go` with `go build`, `bench.java` with `javac`
4. Runs each benchmark N times per language, takes the best (lowest) time
5. Outputs a markdown table

## Why Perry beats compiled languages on some benchmarks

These results are real but need context. Perry is not "faster than C++." Perry is faster than C++ *compiled with default optimization flags on benchmarks that use f64 for everything.* Three specific optimizations create the advantage:

### 1. Fast-math reassociation (loop_overhead, math_intensive)

Perry emits `reassoc contract` flags on every f64 arithmetic instruction. This lets LLVM break serial accumulator chains like `sum = sum + 1.0` into parallel accumulators, unroll 8x, and vectorize with NEON.

Rust, C++, Go, and Swift compile with strict IEEE 754 by default. Under IEEE rules, `(a + b) + c != a + (b + c)` for floating-point — so the compiler cannot reorder the additions. Every `fadd` depends on the previous one: 3-cycle latency per iteration, fully serialized. That's why Rust/C++/Go/Swift all land at ~95ms for loop_overhead: they're hitting the `fadd` latency wall.

Perry at 12ms means LLVM split the accumulator into ~8 parallel chains across 2 NEON FPUs. C++ would get the same result with `-ffast-math`, but the default is strict.

### 2. Integer-mod fast path (accumulate)

`i % 1000` on f64 is `fmod()`, which on ARM is a **libm function call** (~30ns per call). All languages in this benchmark use `double` to match TypeScript semantics, so they all call `fmod` — hence ~96ms across the board.

Perry detects at compile time that both operands are provably integer-valued (via `is_integer_valued_expr` static analysis) and emits `fptosi → srem → sitofp` instead. `srem` is a single hardware instruction (~1-2 cycles). 25ms vs 96ms — the entire gap is `srem` vs `fmod`.

If the C++ benchmark used `int` instead of `double`, it would be ~2ms.

### 3. i32 loop counter + bounds elimination (array_write, array_read)

Perry detects `for (let i = 0; i < arr.length; i++)` and maintains a parallel i32 counter alongside the f64 counter. Array indexing uses the i32 directly (no float-to-int conversion per iteration), and bounds checks are skipped entirely because the codegen proved `i < arr.length` statically.

The other languages use `double` array indices (to match TS semantics), paying a float-to-int conversion on every access.

## Where Perry loses — and why

### fibonacci (tied with C++, faster than Rust)

Perry at 309ms ties C++ (309ms) and beats Rust (316ms) on recursive `fib(40)`. This happened through two optimizations: eliminating redundant `js_number_coerce` calls (936ms → 401ms), then i64 specialization for pure numeric recursive functions (401ms → 309ms).

Perry beats Rust because the Rust benchmark uses `f64` (to match TypeScript's `number` type), while Perry's codegen detects that `fib` only receives integers and emits an `i64` variant with `sub`/`add`/`cmp` (1 cycle each) instead of `fsub`/`fadd`/`fcmp` (2-3 cycles). Both compile through LLVM — same optimizer, different input. If Rust used `fn fib(n: i64) -> i64`, it would run at ~308ms.

Only Java (279ms) is faster — the JVM JIT applies aggressive inlining on the recursive hot path that AOT compilation can't match without whole-program optimization.

### object_create (Rust/C++/Go/Swift show 0ms)

The "0ms" results are real but misleading. These languages use stack-allocated structs for `Point { x, y }`. The optimizer inlines the constructor, proves the struct never escapes, and computes the sum at compile time — the allocation is eliminated entirely. Perry uses GC-managed heap allocation (arena bump allocator), which cannot be eliminated. This is an inherent cost of Perry's dynamic value model.

## Benchmark descriptions

| Benchmark | What it measures | Workload |
|-----------|-----------------|----------|
| fibonacci | Recursive function call overhead | `fib(40)` — ~2 billion recursive calls |
| loop_overhead | Raw loop iteration throughput | `sum += 1.0` for 100M iterations |
| array_write | Sequential array write | Write `arr[i] = i` for 10M elements |
| array_read | Sequential array read | Sum 10M array elements |
| math_intensive | f64 arithmetic throughput | `result += 1.0/i` for 50M iterations |
| object_create | Object allocation + field access | Create 1M `Point(x, y)` structs, sum fields |
| nested_loops | Cache behavior + nested iteration | 3000x3000 double-nested array access |
| accumulate | Integer modulo on f64 | `sum += i % 1000` for 100M iterations |

## Compiler versions used

| Language | Compiler | Flags |
|----------|----------|-------|
| Perry | perry (LLVM backend) | default (clang -O3 -ffast-math internally) |
| Rust | rustc 1.92.0 | `-O` (release mode) |
| C++ | Apple clang 21.0 | `-O3 -std=c++17` |
| Go | go 1.21.3 | default |
| Swift | Swift 6.3 | `-O` |
| Java | javac + JVM | default (JIT) |
| Node.js | v25.8.0 | `--experimental-strip-types` |
| Python | 3.14.3 | default (CPython interpreter) |

## Source files

Each language implements all 8 benchmarks in a single file:

- `bench.cpp` — C++17
- `bench.rs` — Rust (no dependencies)
- `bench.go` — Go
- `bench.swift` — Swift
- `bench.java` — Java
- `bench.py` — Python 3
- `bench.zig` — Zig (may need manual build)
- Perry benchmarks in `../suite/*.ts`

All implementations use the same algorithm, same data types (`f64`/`double`), same iteration counts, and same output format (`benchmark_name:elapsed_ms`).
