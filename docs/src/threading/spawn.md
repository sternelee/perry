# spawn

```typescript,no-test
import { spawn } from "perry/thread";

function spawn<T>(fn: () => T): Promise<T>;
```

Runs a closure on a new OS thread and returns a Promise that resolves when the thread completes. The main thread continues immediately — UI and other work are not blocked.

## Basic Usage

```typescript,no-test
import { spawn } from "perry/thread";

const result = await spawn(() => {
    // This runs on a separate OS thread
    let sum = 0;
    for (let i = 0; i < 100_000_000; i++) {
        sum += i;
    }
    return sum;
});

console.log(result); // 4999999950000000
```

## Non-Blocking

`spawn` returns immediately. The main thread doesn't wait:

```typescript,no-test
import { spawn } from "perry/thread";

console.log("1. Starting background work");

const handle = spawn(() => {
    // Runs on a background thread
    return expensiveComputation();
});

console.log("2. Main thread continues immediately");

const result = await handle;
console.log("3. Got result:", result);
```

Output:
```
1. Starting background work
2. Main thread continues immediately
3. Got result: <computed value>
```

## Multiple Concurrent Tasks

Spawn multiple tasks and they run truly concurrently — one OS thread per `spawn` call:

```typescript,no-test
import { spawn } from "perry/thread";

const t1 = spawn(() => analyzeCustomers(regionA));
const t2 = spawn(() => analyzeCustomers(regionB));
const t3 = spawn(() => analyzeCustomers(regionC));

// All three run simultaneously on separate OS threads
const [r1, r2, r3] = await Promise.all([t1, t2, t3]);

console.log("Region A:", r1);
console.log("Region B:", r2);
console.log("Region C:", r3);
```

Unlike Node.js `worker_threads`, each `spawn` is a lightweight OS thread (~8MB stack), not a full V8 isolate (~2MB heap + startup cost).

## Capturing Variables

Like `parallelMap`, `spawn` closures can capture outer variables. They are deep-copied to the background thread:

```typescript,no-test
import { spawn } from "perry/thread";

const config = { iterations: 1000, seed: 42 };
const dataset = loadData();

const result = await spawn(() => {
    // config and dataset are copied to this thread
    return runSimulation(config, dataset);
});
```

Mutable variables cannot be captured — this is enforced at compile time.

## Returning Complex Values

`spawn` can return any value type. Complex values (objects, arrays, strings) are serialized back to the main thread automatically:

```typescript,no-test
import { spawn } from "perry/thread";

const stats = await spawn(() => {
    const values = computeExpensiveValues();
    return {
        mean: average(values),
        median: median(values),
        stddev: standardDeviation(values),
        count: values.length,
    };
});

console.log(stats.mean, stats.median);
```

## UI Integration

`spawn` is ideal for keeping native UIs responsive during heavy computation:

```typescript,no-test
import { spawn } from "perry/thread";
import { Text, Button, VStack } from "perry/ui";

let status = "Ready";
let result = "";

VStack(10, [
    Text(status),
    Text(result),
    Button("Analyze", async () => {
        status = "Processing...";

        // Background thread — UI stays responsive
        const data = await spawn(() => {
            return runAnalysis(largeDataset);
        });

        result = `Found ${data.count} patterns`;
        status = "Done";
    }),
]);
```

Without `spawn`, the analysis would freeze the UI. With `spawn`, the user can still scroll, tap other buttons, or navigate while the computation runs.

## Compared to Node.js worker_threads

```typescript,no-test
// ── Node.js: ~15 lines, separate file needed ──────────
// worker.js
const { parentPort, workerData } = require("worker_threads");
const result = heavyComputation(workerData);
parentPort.postMessage(result);

// main.js
const { Worker } = require("worker_threads");
const worker = new Worker("./worker.js", {
    workerData: inputData,
});
worker.on("message", (result) => {
    console.log(result);
});
worker.on("error", (err) => { /* handle */ });


// ── Perry: 1 line ─────────────────────────────────────
const result = await spawn(() => heavyComputation(inputData));
```

No separate files. No message ports. No event handlers. No structured clone. One line.

## Examples

### Background File Processing

```typescript,no-test
import { spawn } from "perry/thread";
import { readFileSync } from "fs";

// Read and process a large file without blocking
const analysis = await spawn(() => {
    const content = readFileSync("large-dataset.csv");
    return parseAndAnalyze(content);
});
```

### Parallel API Calls with Processing

```typescript,no-test
import { spawn } from "perry/thread";

// Fetch data, then process it on a background thread
const rawData = await fetch("https://api.example.com/data").then(r => r.json());

// CPU-intensive processing happens off the main thread
const processed = await spawn(() => {
    return transformAndEnrich(rawData);
});
```

### Deferred Computation

```typescript,no-test
import { spawn } from "perry/thread";

// Start computation early, use result later
const precomputed = spawn(() => buildLookupTable(params));

// ... do other setup work ...

// Result is ready (or we wait for it)
const table = await precomputed;
```
