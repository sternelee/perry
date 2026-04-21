# Hello World

## Your First Program

Create a file called `hello.ts`:

```typescript
{{#include ../../examples/getting-started/hello.ts}}
```

Compile and run it:

```bash
perry hello.ts -o hello
./hello
```

Output:

```
Hello, Perry!
```

That's it. Perry compiled your TypeScript to a native executable — no Node.js, no bundler, no runtime.

## A Slightly Bigger Example

```typescript
{{#include ../../examples/getting-started/fibonacci.ts}}
```

```bash
perry fib.ts -o fib
./fib
```

This runs about 2x faster than Node.js because Perry compiles to native machine code with integer specialization.

## Using Variables and Functions

```typescript,no-test
const name: string = "World";
const items: number[] = [1, 2, 3, 4, 5];

const doubled = items.map((x) => x * 2);
const sum = doubled.reduce((acc, x) => acc + x, 0);

console.log(`Hello, ${name}!`);
console.log(`Sum of doubled: ${sum}`);
```

## Async Code

```typescript,no-test
async function fetchData(): Promise<string> {
  const response = await fetch("https://httpbin.org/get");
  const data = await response.json();
  return data.origin;
}

const ip = await fetchData();
console.log(`Your IP: ${ip}`);
```

```bash
perry fetch.ts -o fetch
./fetch
```

Perry compiles async/await to a native async runtime backed by Tokio.

## Multi-Threading

Perry can do something no JavaScript runtime can — run your code on multiple CPU cores:

```typescript,no-test
import { parallelMap, parallelFilter, spawn } from "perry/thread";

const data = [1, 2, 3, 4, 5, 6, 7, 8];

// Process all elements across all CPU cores
const doubled = parallelMap(data, (x) => x * 2);
console.log(doubled); // [2, 4, 6, 8, 10, 12, 14, 16]

// Run heavy work in the background
const result = await spawn(() => {
  let sum = 0;
  for (let i = 0; i < 100_000_000; i++) sum += i;
  return sum;
});
console.log(result);
```

This is real OS-level parallelism, not web workers or separate isolates. See [Multi-Threading](../threading/overview.md) for details.

## What the Compiler Produces

When you run `perry file.ts -o output`, Perry:

1. Parses your TypeScript with SWC
2. Lowers the AST to an intermediate representation (HIR)
3. Applies optimizations (inlining, closure conversion, etc.)
4. Generates native machine code with LLVM
5. Links with your system's C compiler

The result is a standalone executable with no external dependencies.

### Binary Size

| Program | Binary Size |
|---------|-------------|
| Hello world | ~300KB |
| CLI with fs/path | ~3MB |
| UI app | ~3MB |
| Full app with stdlib | ~48MB |

Perry automatically detects which runtime features you use and only links what's needed.

## Next Steps

- [Build a native UI app](first-app.md)
- [Configure your project](project-config.md)
- [Explore supported TypeScript features](../language/supported-features.md)
