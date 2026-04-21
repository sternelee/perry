# parallelFilter

```typescript,no-test
import { parallelFilter } from "perry/thread";

function parallelFilter<T>(data: T[], predicate: (item: T) => boolean): T[];
```

Filters an array in parallel across all available CPU cores. Returns a new array containing only the elements where the predicate returned a truthy value. Order is preserved.

## Basic Usage

```typescript,no-test
import { parallelFilter } from "perry/thread";

const numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const evens = parallelFilter(numbers, (x) => x % 2 === 0);
// [2, 4, 6, 8, 10]
```

## How It Works

```
Input: [a, b, c, d, e, f, g, h]     (8 elements, 4 CPU cores)

  Core 1: [a, b] → test → [a]       (b filtered out)
  Core 2: [c, d] → test → [c, d]    (both kept)
  Core 3: [e, f] → test → []        (both filtered out)
  Core 4: [g, h] → test → [h]       (g filtered out)

Output: [a, c, d, h]                 (concatenated in original order)
```

Each core independently tests its chunk of elements. Results are merged in the original element order after all threads complete.

## Why Not Just Use `.filter()`?

Regular `.filter()` runs on a single thread. For large arrays with expensive predicates, `parallelFilter` distributes the work:

```typescript,no-test
// Single-threaded — one core does all the work
const results = data.filter((item) => expensivePredicate(item));

// Parallel — all cores share the work
import { parallelFilter } from "perry/thread";
const results = parallelFilter(data, (item) => expensivePredicate(item));
```

The tradeoff: `parallelFilter` has overhead from copying values between threads. Use it when the predicate is expensive enough to justify that cost.

## Capturing Variables

Like `parallelMap`, the predicate can capture outer variables. Captures are deep-copied to each thread:

```typescript,no-test
import { parallelFilter } from "perry/thread";

const minScore = 85;
const maxAge = 30;

// minScore and maxAge are captured and copied to each thread
const qualified = parallelFilter(candidates, (c) => {
    return c.score >= minScore && c.age <= maxAge;
});
```

Mutable variables cannot be captured — the compiler rejects this at compile time.

## Examples

### Filtering Large Datasets

```typescript,no-test
import { parallelFilter } from "perry/thread";

const transactions = getTransactionLog(); // millions of records

const suspicious = parallelFilter(transactions, (tx) => {
    return tx.amount > 10000
        && tx.country !== tx.user.homeCountry
        && tx.timestamp.hour < 6;
});
```

### Combined with parallelMap

```typescript,no-test
import { parallelMap, parallelFilter } from "perry/thread";

// Step 1: Filter to relevant items (parallel)
const active = parallelFilter(users, (u) => u.isActive && u.age >= 18);

// Step 2: Transform the filtered results (parallel)
const profiles = parallelMap(active, (u) => ({
    name: u.name,
    score: computeScore(u),
}));
```

### Predicate with Heavy Computation

```typescript,no-test
import { parallelFilter } from "perry/thread";

// Each predicate call does significant work — perfect for parallelization
const valid = parallelFilter(certificates, (cert) => {
    return verifyCertificateChain(cert) && !isRevoked(cert);
});
```

## Performance

Use `parallelFilter` when:
- The array has many elements (hundreds or more)
- The predicate function does meaningful work per element
- You need to keep the UI responsive during filtering

For trivial predicates on small arrays, regular `.filter()` is faster (no threading overhead).
