// Fibonacci benchmark variant.
//
// Identical compute workload to `bench_fibonacci.ts` (recursive fib(35),
// 5 warmup + 100 timed iterations) but the output uses raw `console.log`
// of numbers instead of string concatenation.
//
// Output format (3 lines):
//   <total_ms>
//   <iterations>
//   <avg_ms>
//
// Compare against `bench_fibonacci.ts` for the Cranelift baseline — the
// timing is directly comparable because the compute kernel is identical.

function fibonacci(n: number): number {
  if (n <= 1) {
    return n;
  }
  return fibonacci(n - 1) + fibonacci(n - 2);
}

const WARMUP_ITERATIONS = 5;
const TIMED_ITERATIONS = 100;
const FIB_N = 35;

// Opacity barrier against LLVM -O2 constant folding.
//
// `fibonacci` is a pure function of `n`. With a constant `n=FIB_N`, clang
// at -O2 will:
//   1. Inline-and-fold `fibonacci(35)` to its result `9227465` at compile
//      time (proven by an earlier run where the printed accumulators
//      exactly matched 5*9227465 and 100*9227465).
//   2. Then the for loop becomes `acc += <constant>` 100 times, which is
//      itself constant-folded to a single store.
//   3. The whole timed phase collapses to two assignments. 0 ms measured.
//
// To prevent this, the input to `fibonacci` must be opaque to LLVM. We
// derive it from `Date.now() - Date.now()` inside the loop body: two
// consecutive external calls whose return values LLVM cannot prove
// anything about. The difference is *almost always* 0 (or rarely 1 ms),
// so we still effectively measure fib(35) — but the optimizer cannot
// hoist (LICM) or constant-fold the call.
//
// The two `js_date_now` calls per iteration cost ~100 ns total, which is
// noise against ~50 ms of fibonacci compute per iteration. Both backends
// pay this overhead so the relative comparison stays fair.

let warmup_acc = 0;
for (let i = 0; i < WARMUP_ITERATIONS; i = i + 1) {
  const offset = Date.now() - Date.now();
  warmup_acc = warmup_acc + fibonacci(FIB_N + offset);
}

let timed_acc = 0;
const start = Date.now();
for (let i = 0; i < TIMED_ITERATIONS; i = i + 1) {
  const offset = Date.now() - Date.now();
  timed_acc = timed_acc + fibonacci(FIB_N + offset);
}
const end = Date.now();

const total = end - start;
const avg = total / TIMED_ITERATIONS;

// Print the accumulators so they have a downstream observer — without
// this, even `let timed_acc = ...` is dead and the optimizer eliminates
// the loop. The values are deterministic: fib(35) = 9227465, summed 100
// times = 922746500.
console.log(warmup_acc);
console.log(timed_acc);
console.log(total);
console.log(TIMED_ITERATIONS);
console.log(avg);
