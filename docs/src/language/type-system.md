# Type System

Perry erases types at compile time, similar to how `tsc` removes type annotations when emitting JavaScript. However, Perry also performs type inference to generate efficient native code.

## Type Inference

Perry infers types from expressions without requiring annotations:

```typescript,no-test
let x = 5;           // inferred as number
let s = "hello";     // inferred as string
let b = true;        // inferred as boolean
let arr = [1, 2, 3]; // inferred as number[]
```

Inference works through:
- **Literal values**: `5` → `number`, `"hi"` → `string`
- **Binary operations**: `a + b` where both are numbers → `number`
- **Variable propagation**: if `x` is `number`, then `let y = x` is `number`
- **Method returns**: `"hello".trim()` → `string`, `[1,2].length` → `number`
- **Function returns**: user-defined function return types are propagated to callers

```typescript,no-test
function double(n: number): number {
  return n * 2;
}
let result = double(5); // inferred as number
```

## Type Annotations

Standard TypeScript annotations work:

```typescript,no-test
let name: string = "Perry";
let count: number = 0;
let items: string[] = [];

function greet(name: string): string {
  return `Hello, ${name}`;
}

interface Config {
  port: number;
  host: string;
}
```

## Utility Types

Common TypeScript utility types are erased at compile time (they don't affect code generation):

```typescript,no-test
type Partial<T> = { [P in keyof T]?: T[P] };
type Pick<T, K> = { [P in K]: T[P] };
type Record<K, V> = { [P in K]: V };
type Omit<T, K> = Pick<T, Exclude<keyof T, K>>;
type ReturnType<T> = /* ... */;
type Readonly<T> = { readonly [P in keyof T]: T[P] };
```

These are all recognized and erased — they won't cause compilation errors.

## Generics

Generic type parameters are erased:

```typescript,no-test
function identity<T>(value: T): T {
  return value;
}

class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}

const box = new Box<number>(42);
```

At runtime, all values are NaN-boxed — the generic parameter doesn't affect code generation.

## Type Checking with `--type-check`

For stricter type checking, Perry can integrate with Microsoft's TypeScript checker:

```bash
perry file.ts --type-check
```

This resolves cross-file types, interfaces, and generics via an IPC protocol. It falls back gracefully if the type checker is not installed.

Without `--type-check`, Perry relies on its own inference engine, which handles common patterns but doesn't perform full TypeScript type checking.

## Union and Intersection Types

Union types are recognized syntactically but don't affect code generation:

```typescript,no-test
type StringOrNumber = string | number;

function process(value: StringOrNumber) {
  if (typeof value === "string") {
    console.log(value.toUpperCase());
  } else {
    console.log(value + 1);
  }
}
```

Use `typeof` checks for runtime type narrowing.

## Type Guards

```typescript,no-test
function isString(value: any): value is string {
  return typeof value === "string";
}

if (isString(x)) {
  console.log(x.toUpperCase());
}
```

The `value is string` annotation is erased, but the `typeof` check works at runtime.

## Next Steps

- [Supported Features](supported-features.md) — Complete feature list
- [Limitations](limitations.md) — What's not supported
