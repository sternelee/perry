# parallelMap

```typescript,no-test
import { parallelMap } from "perry/thread";

function parallelMap<T, U>(data: T[], fn: (item: T) => U): U[];
```

Processes every element of an array in parallel across all available CPU cores. Returns a new array with the results in the same order as the input.

## Basic Usage

```typescript,no-test
import { parallelMap } from "perry/thread";

const numbers = [1, 2, 3, 4, 5, 6, 7, 8];
const doubled = parallelMap(numbers, (x) => x * 2);
// [2, 4, 6, 8, 10, 12, 14, 16]
```

## How It Works

```
Input: [a, b, c, d, e, f, g, h]     (8 elements, 4 CPU cores)

  Core 1: [a, b] → map → [a', b']
  Core 2: [c, d] → map → [c', d']
  Core 3: [e, f] → map → [e', f']
  Core 4: [g, h] → map → [g', h']

Output: [a', b', c', d', e', f', g', h']   (same order as input)
```

Perry automatically detects the number of CPU cores and splits the array into equal chunks. Elements within each chunk are processed sequentially; chunks run concurrently across cores.

## Capturing Variables

The mapping function can reference variables from the outer scope. Captured values are deep-copied to each worker thread automatically:

```typescript,no-test
const exchangeRate = 1.12;
const fees = [0.01, 0.02, 0.015];

const converted = parallelMap(prices, (price) => {
    // exchangeRate is captured and copied to each thread
    return price * exchangeRate;
});
```

### What Can Be Captured

| Type | Supported | Transfer |
|------|-----------|----------|
| Numbers | Yes | Zero-cost (64-bit copy) |
| Booleans | Yes | Zero-cost |
| Strings | Yes | Byte copy |
| Arrays | Yes | Deep copy |
| Objects | Yes | Deep copy |
| `const` variables | Yes | Copied |
| `let`/`var` variables | Only if not reassigned | Copied |

### What Cannot Be Captured

Mutable variables — variables that are reassigned anywhere in the enclosing scope — are rejected at compile time:

```typescript,no-test
let total = 0;

// COMPILE ERROR: Cannot capture mutable variable 'total'
parallelMap(data, (item) => {
    total += item;   // Would be a data race
    return item;
});
```

Instead, return values and reduce:

```typescript,no-test
const results = parallelMap(data, (item) => item * 2);
const total = results.reduce((sum, x) => sum + x, 0);
```

## Performance

### When to Use parallelMap

Use `parallelMap` when the computation per element is **significantly heavier** than the cost of copying the element across threads.

**Good candidates** (CPU-bound work per element):
```typescript,no-test
// Heavy math
parallelMap(data, (x) => expensiveComputation(x));

// String processing on large strings
parallelMap(documents, (doc) => parseAndAnalyze(doc));

// Cryptographic operations
parallelMap(inputs, (input) => computeHash(input));
```

**Poor candidates** (trivial work per element):
```typescript,no-test
// Too simple — threading overhead outweighs the gain
parallelMap(numbers, (x) => x + 1);

// For trivial operations, use regular map
const result = numbers.map((x) => x + 1);
```

### Small Array Optimization

For arrays with fewer elements than CPU cores, Perry skips threading entirely and processes elements inline on the main thread. There's zero overhead for small inputs.

### Numeric Fast Path

When elements are pure numbers (no strings, objects, or arrays), Perry transfers them between threads at virtually zero cost — just 64-bit value copies with no serialization.

## Examples

### Matrix Row Processing

```typescript,no-test
import { parallelMap } from "perry/thread";

// Process each row of a matrix independently
const rows = [[1,2,3], [4,5,6], [7,8,9]];
const rowSums = parallelMap(rows, (row) => {
    let sum = 0;
    for (const val of row) sum += val;
    return sum;
});
// [6, 15, 24]
```

### Batch Validation

```typescript,no-test
import { parallelMap } from "perry/thread";

const users = [
    { name: "Alice", email: "alice@example.com" },
    { name: "Bob", email: "invalid" },
    { name: "Charlie", email: "charlie@example.com" },
];

const validationResults = parallelMap(users, (user) => {
    const emailValid = user.email.includes("@") && user.email.includes(".");
    const nameValid = user.name.length > 0 && user.name.length < 100;
    return { name: user.name, valid: emailValid && nameValid };
});
```

### Financial Calculations

```typescript,no-test
import { parallelMap } from "perry/thread";

const portfolios = getPortfolioData(); // thousands of portfolios

// Monte Carlo simulation across all cores
const riskScores = parallelMap(portfolios, (portfolio) => {
    let totalRisk = 0;
    for (let sim = 0; sim < 10000; sim++) {
        totalRisk += simulateReturns(portfolio);
    }
    return totalRisk / 10000;
});
```
