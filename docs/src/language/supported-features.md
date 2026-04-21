# Supported TypeScript Features

Perry compiles a practical subset of TypeScript to native code. This page lists what's supported.

## Primitive Types

```typescript,no-test
const n: number = 42;
const s: string = "hello";
const b: boolean = true;
const u: undefined = undefined;
const nl: null = null;
```

All primitives are represented as 64-bit NaN-boxed values at runtime.

## Variables and Constants

```typescript,no-test
let x = 10;
const y = "immutable";
var z = true; // var is supported but let/const preferred
```

Perry infers types from initializers — `let x = 5` is inferred as `number` without an explicit annotation.

## Functions

```typescript,no-test
function add(a: number, b: number): number {
  return a + b;
}

// Optional parameters
function greet(name: string, greeting: string = "Hello"): string {
  return `${greeting}, ${name}!`;
}

// Rest parameters
function sum(...nums: number[]): number {
  return nums.reduce((a, b) => a + b, 0);
}

// Arrow functions
const double = (x: number) => x * 2;
```

## Classes

```typescript,no-test
class Animal {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return `${this.name} makes a noise`;
  }
}

class Dog extends Animal {
  speak(): string {
    return `${this.name} barks`;
  }
}

// Static methods
class Counter {
  private static instance: Counter;
  private count: number = 0;

  static getInstance(): Counter {
    if (!Counter.instance) {
      Counter.instance = new Counter();
    }
    return Counter.instance;
  }
}
```

Supported class features:
- Constructors
- Instance and static methods
- Instance and static properties
- Inheritance (`extends`)
- Method overriding
- `instanceof` checks (via class ID chain)
- Singleton patterns (static method return type inference)

## Enums

```typescript,no-test
// Numeric enums
enum Direction {
  Up,
  Down,
  Left,
  Right,
}

// String enums
enum Color {
  Red = "RED",
  Green = "GREEN",
  Blue = "BLUE",
}

const dir = Direction.Up;
const color = Color.Red;
```

Enums are compiled to constants and work across modules.

## Interfaces and Type Aliases

```typescript,no-test
interface User {
  name: string;
  age: number;
  email?: string;
}

type Point = { x: number; y: number };
type StringOrNumber = string | number;
type Callback = (value: number) => void;
```

Interfaces and type aliases are erased at compile time (like `tsc`). They exist only for documentation and editor tooling.

## Arrays

```typescript,no-test
const nums: number[] = [1, 2, 3];

// Array methods
nums.push(4);
nums.pop();
const len = nums.length;
const doubled = nums.map((x) => x * 2);
const filtered = nums.filter((x) => x > 2);
const sum = nums.reduce((acc, x) => acc + x, 0);
const found = nums.find((x) => x === 3);
const idx = nums.indexOf(3);
const joined = nums.join(", ");
const sliced = nums.slice(1, 3);
nums.splice(1, 1);
nums.unshift(0);
const sorted = nums.sort((a, b) => a - b);
const reversed = nums.reverse();
const includes = nums.includes(3);
const every = nums.every((x) => x > 0);
const some = nums.some((x) => x > 2);
nums.forEach((x) => console.log(x));
const flat = [[1, 2], [3]].flat();
const concatted = nums.concat([5, 6]);

// Array.from
const arr = Array.from(someIterable);

// Array.isArray
if (Array.isArray(value)) { /* ... */ }

// for...of iteration
for (const item of nums) {
  console.log(item);
}
```

## Objects

```typescript,no-test
const obj = { name: "Perry", version: 1 };
obj.name = "Perry 2";

// Dynamic property access
const key = "name";
const val = obj[key];

// Object.keys, Object.values, Object.entries
const keys = Object.keys(obj);
const values = Object.values(obj);
const entries = Object.entries(obj);

// Spread
const copy = { ...obj, extra: true };

// delete
delete obj[key];
```

## Destructuring

```typescript,no-test
// Array destructuring
const [a, b, ...rest] = [1, 2, 3, 4, 5];

// Object destructuring
const { name, age, email = "none" } = user;

// Rename
const { name: userName } = user;

// Rest pattern
const { id, ...remaining } = obj;

// Function parameter destructuring
function process({ name, age }: User) {
  console.log(name, age);
}
```

## Template Literals

```typescript,no-test
const name = "world";
const greeting = `Hello, ${name}!`;
const multiline = `
  Line 1
  Line 2
`;
const expr = `Result: ${1 + 2}`;
```

## Spread and Rest

```typescript,no-test
// Array spread
const combined = [...arr1, ...arr2];

// Object spread
const merged = { ...defaults, ...overrides };

// Rest parameters
function log(...args: any[]) { /* ... */ }
```

## Closures

```typescript,no-test
function makeCounter() {
  let count = 0;
  return {
    increment: () => ++count,
    get: () => count,
  };
}

const counter = makeCounter();
counter.increment();
console.log(counter.get()); // 1
```

Perry performs closure conversion — captured variables are stored in heap-allocated closure objects.

## Async/Await

```typescript,no-test
async function fetchUser(id: number): Promise<User> {
  const response = await fetch(`/api/users/${id}`);
  return await response.json();
}

// Top-level await
const data = await fetchUser(1);
```

Perry compiles async functions to a state machine backed by Tokio's async runtime.

## Promises

```typescript,no-test
const p = new Promise<number>((resolve, reject) => {
  resolve(42);
});

p.then((value) => console.log(value));

// Promise.all
const results = await Promise.all([fetch(url1), fetch(url2)]);
```

## Generators

```typescript,no-test
function* range(start: number, end: number) {
  for (let i = start; i < end; i++) {
    yield i;
  }
}

for (const n of range(0, 10)) {
  console.log(n);
}
```

## Map and Set

```typescript,no-test
const map = new Map<string, number>();
map.set("a", 1);
map.get("a");
map.has("a");
map.delete("a");
map.size;

const set = new Set<number>();
set.add(1);
set.has(1);
set.delete(1);
set.size;
```

## Regular Expressions

```typescript,no-test
const re = /hello\s+(\w+)/;
const match = "hello world".match(re);

if (re.test("hello perry")) {
  console.log("Matched!");
}

const replaced = "hello world".replace(/world/, "perry");
```

## Error Handling

```typescript,no-test
try {
  throw new Error("something went wrong");
} catch (e) {
  console.log(e.message);
} finally {
  console.log("cleanup");
}
```

## JSON

```typescript,no-test
const obj = JSON.parse('{"key": "value"}');
const str = JSON.stringify(obj);
const pretty = JSON.stringify(obj, null, 2);
```

## typeof and instanceof

```typescript,no-test
if (typeof x === "string") {
  console.log(x.length);
}

if (obj instanceof Dog) {
  obj.speak();
}
```

`typeof` checks NaN-boxing tags at runtime. `instanceof` walks the class ID chain.

## Modules

```typescript,no-test
// Named exports
export function helper() { /* ... */ }
export const VALUE = 42;

// Default export
export default class MyClass { /* ... */ }

// Import
import MyClass, { helper, VALUE } from "./module";
import * as utils from "./utils";

// Re-exports
export { helper } from "./module";
```

## BigInt

```typescript,no-test
const big = BigInt(9007199254740991);
const result = big + BigInt(1);

// Bitwise operations
const and = big & BigInt(0xFF);
const or = big | BigInt(0xFF);
const xor = big ^ BigInt(0xFF);
const shl = big << BigInt(2);
const shr = big >> BigInt(2);
const not = ~big;
```

## String Methods

```typescript,no-test
const s = "Hello, World!";
s.length;
s.toUpperCase();
s.toLowerCase();
s.trim();
s.split(", ");
s.includes("World");
s.startsWith("Hello");
s.endsWith("!");
s.indexOf("World");
s.slice(0, 5);
s.substring(0, 5);
s.replace("World", "Perry");
s.repeat(3);
s.charAt(0);
s.padStart(20);
s.padEnd(20);
```

## Math

```typescript,no-test
Math.floor(3.7);
Math.ceil(3.2);
Math.round(3.5);
Math.abs(-5);
Math.max(1, 2, 3);
Math.min(1, 2, 3);
Math.sqrt(16);
Math.pow(2, 10);
Math.random();
Math.PI;
Math.E;
Math.log(10);
Math.sin(0);
Math.cos(0);
```

## Date

```typescript,no-test
const now = Date.now();
const d = new Date();
d.getTime();
d.toISOString();
```

## Console

```typescript,no-test
console.log("message");
console.error("error");
console.warn("warning");
console.time("label");
console.timeEnd("label");
```

## Garbage Collection

Perry includes a mark-sweep garbage collector. It runs automatically when memory pressure is detected (~8MB arena blocks), but you can also trigger it manually:

```typescript,no-test
gc(); // Explicit garbage collection
```

The GC uses conservative stack scanning to find roots and supports arena-allocated objects (arrays, objects) and malloc-allocated objects (strings, closures, promises, BigInts, errors).

## JSX/TSX

Perry supports JSX syntax for UI component composition:

```typescript,no-test
// Component functions
function Greeting({ name }: { name: string }) {
  return <Text>{`Hello, ${name}!`}</Text>;
}

// JSX elements
<Button onClick={() => console.log("clicked")}>Click me</Button>

// Fragments
<>
  <Text>Line 1</Text>
  <Text>Line 2</Text>
</>

// Spread props
<Component {...props} extra="value" />

// Conditional rendering
{condition ? <Text>Yes</Text> : <Text>No</Text>}
```

JSX elements are transformed to function calls via the `jsx()`/`jsxs()` runtime.

## Next Steps

- [Type System](type-system.md) — Type inference and checking
- [Limitations](limitations.md) — What's not supported yet
