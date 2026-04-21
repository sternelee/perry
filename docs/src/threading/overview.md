# Multi-Threading

Perry gives you real OS threads with a one-line API. No worker setup, no message ports, no structured clone overhead. Just `parallelMap`, `parallelFilter`, and `spawn`.

```typescript,no-test
import { parallelMap, parallelFilter, spawn } from "perry/thread";

// Process a million items across all CPU cores
const results = parallelMap(data, (item) => heavyComputation(item));

// Filter a large dataset in parallel
const valid = parallelFilter(records, (r) => r.score > threshold);

// Run expensive work in the background
const answer = await spawn(() => computeHash(largeFile));
```

This is something **no JavaScript runtime can do**. V8, Bun, and Deno are all locked to one thread per isolate. Perry compiles to native code — there are no isolates, no GIL, no structural limitations. Your code runs on real OS threads with the full power of every CPU core.

## Why This Matters

JavaScript's single-threaded model is its biggest performance bottleneck. Here's how runtimes try to work around it:

| Runtime | "Multi-threading" | Reality |
|---------|-------------------|---------|
| **Node.js** | `worker_threads` | Separate V8 isolates. Data copied via structured clone. ~2MB RAM per worker. Complex API. |
| **Deno** | `Worker` | Same as Node — isolated heaps, message passing only. |
| **Bun** | `Worker` | Same architecture. Faster structured clone, still isolated. |
| **Perry** | `parallelMap` / `spawn` | Real OS threads. Lightweight (8MB stack). One-line API. Compile-time safety. |

The fundamental problem: V8 uses a garbage-collected heap that **cannot be shared** between threads. Every "worker" is an entirely separate JavaScript engine instance with its own heap, its own GC, and its own copy of your data.

Perry doesn't have this limitation. It compiles TypeScript to native machine code. Values are transferred between threads using zero-cost copies for numbers and efficient serialization for objects — no separate engine instances, no multi-megabyte overhead per thread.

## Three Primitives

### `parallelMap` — Data-Parallel Processing

Split an array across all CPU cores. Each element is processed independently. Results are collected in order.

```typescript,no-test
import { parallelMap } from "perry/thread";

const prices = [100, 200, 300, 400, 500, 600, 700, 800];
const adjusted = parallelMap(prices, (price) => {
    // Heavy computation runs on a worker thread
    let result = price;
    for (let i = 0; i < 1000000; i++) {
        result = Math.sqrt(result * result + i);
    }
    return result;
});
```

Perry automatically:
1. Detects the number of CPU cores
2. Splits the array into chunks (one per core)
3. Spawns OS threads to process each chunk
4. Collects results in the original order
5. Returns a new array

For small arrays, Perry skips threading entirely and processes inline — no overhead for trivial cases.

### `parallelFilter` — Data-Parallel Filtering

Filter a large array across all CPU cores. Like `.filter()` but parallel:

```typescript,no-test
import { parallelFilter } from "perry/thread";

const users = getMillionUsers();

// Filter across all cores — order is preserved
const active = parallelFilter(users, (user) => {
    return user.lastLogin > cutoffDate && user.score > 100;
});
```

Same rules as `parallelMap`: closures cannot capture mutable variables (compile-time enforced), and values are deep-copied between threads.

### `spawn` — Background Threads

Run any computation in the background and get a Promise back. The main thread continues immediately.

```typescript,no-test
import { spawn } from "perry/thread";

// Start heavy work in the background
const handle = spawn(() => {
    let sum = 0;
    for (let i = 0; i < 100_000_000; i++) {
        sum += Math.sin(i);
    }
    return sum;
});

// Main thread keeps running — UI stays responsive
console.log("Computing...");

// Get the result when you need it
const result = await handle;
console.log("Done:", result);
```

`spawn` returns a standard Promise. You can `await` it, pass it to `Promise.all`, or chain `.then()` — it works exactly like any other async operation.

## Practical Examples

### Parallel Image Processing

```typescript,no-test
import { parallelMap } from "perry/thread";

// Each pixel processed on a separate core
const processed = parallelMap(pixels, (pixel) => {
    const r = Math.min(255, pixel.r * 1.2);
    const g = Math.min(255, pixel.g * 0.8);
    const b = Math.min(255, pixel.b * 1.1);
    return { r, g, b };
});
```

### Parallel Cryptographic Hashing

```typescript,no-test
import { parallelMap } from "perry/thread";

// Hash thousands of items across all cores
const passwords = ["pass1", "pass2", "pass3", /* ... thousands more */];
const hashed = parallelMap(passwords, (password) => {
    return computeHash(password);
});
```

### Multiple Independent Computations

```typescript,no-test
import { spawn } from "perry/thread";

// Three independent tasks run simultaneously on three OS threads
const task1 = spawn(() => analyzeDataset(dataA));
const task2 = spawn(() => analyzeDataset(dataB));
const task3 = spawn(() => analyzeDataset(dataC));

// All three run concurrently
const [result1, result2, result3] = await Promise.all([task1, task2, task3]);
```

### Keeping UI Responsive

```typescript,no-test
import { spawn } from "perry/thread";
import { Text, Button } from "perry/ui";

let statusText = "Ready";

Button("Start Analysis", async () => {
    statusText = "Analyzing...";

    // Heavy computation runs on a background thread
    // UI stays responsive — user can still interact
    const result = await spawn(() => {
        return runExpensiveAnalysis(data);
    });

    statusText = `Done: ${result}`;
});

Text(statusText);
```

### Captured Variables

Closures can capture outer variables. Captured values are automatically deep-copied to each worker thread:

```typescript,no-test
import { parallelMap } from "perry/thread";

const taxRate = 0.08;
const discount = 0.15;

// taxRate and discount are captured and copied to each thread
const finalPrices = parallelMap(prices, (price) => {
    const discounted = price * (1 - discount);
    return discounted * (1 + taxRate);
});
```

Numbers and booleans are zero-cost copies (just 64-bit values). Strings, arrays, and objects are deep-copied automatically.

## Safety

Perry enforces thread safety **at compile time**. You don't need to think about race conditions, mutexes, or data corruption.

### No Shared Mutable State

Closures passed to `parallelMap` and `spawn` **cannot capture mutable variables**. The compiler rejects this:

```typescript,no-test
let counter = 0;

// COMPILE ERROR: Closures passed to parallelMap cannot
// capture mutable variable 'counter'
parallelMap(data, (item) => {
    counter++;  // Not allowed
    return item;
});
```

This eliminates data races by design. If you need to aggregate results, use the return values:

```typescript,no-test
// Instead of mutating a shared counter, return values and reduce
const results = parallelMap(data, (item) => processItem(item));
const total = results.reduce((sum, r) => sum + r, 0);
```

### Independent Thread Arenas

Each worker thread has its own memory arena. Objects created on one thread can never be accessed from another thread. Values cross thread boundaries only through deep-copy serialization, which Perry handles automatically and invisibly.

## How It Works

Perry's threading model is built on three pillars:

**1. Native Code, Not Interpreted**

Perry compiles TypeScript to native machine code via LLVM. There's no interpreter, no VM, no isolate. A function pointer is just a function pointer — it's valid on any thread.

**2. Thread-Local Memory**

Each thread gets its own memory arena (bump allocator) and garbage collector. No synchronization overhead during computation. When a thread finishes, its arena is freed automatically.

**3. Serialized Transfer**

Values crossing thread boundaries are serialized to a thread-safe intermediate format and deserialized on the target thread. The cost depends on the value type:

| Value Type | Transfer Cost |
|-----------|--------------|
| Numbers, booleans, null, undefined | Zero-cost (64-bit copy) |
| Strings | O(n) byte copy |
| Arrays | O(n) deep copy of elements |
| Objects | O(n) deep copy of fields |
| Closures | Pointer + captured values |

For numeric workloads — the most common parallelizable tasks — the threading overhead is negligible.

## Next Steps

- [parallelMap Reference](parallel-map.md) — detailed API and performance tips
- [parallelFilter Reference](parallel-filter.md) — parallel array filtering
- [spawn Reference](spawn.md) — background threads and Promise integration
