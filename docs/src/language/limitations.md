# Limitations

Perry compiles a practical subset of TypeScript. This page documents what's not supported or works differently from Node.js/tsc.

## No Runtime Type Checking

Types are erased at compile time. There is no runtime type system — Perry doesn't generate type guards or runtime type metadata.

```typescript,no-test
// These annotations are erased — no runtime effect
const x: number = someFunction(); // No runtime check that result is actually a number
```

Use explicit `typeof` checks where runtime type discrimination is needed.

## No eval() or Dynamic Code

Perry compiles to native code ahead of time. Dynamic code execution is not possible:

```typescript,no-test
// Not supported
eval("console.log('hi')");
new Function("return 42");
```

## No Decorators

TypeScript decorators are not currently supported:

```typescript,no-test
// Not supported
@Component
class MyClass {}
```

## No Reflection

There is no `Reflect` API or runtime type metadata:

```typescript,no-test
// Not supported
Reflect.getMetadata("design:type", target, key);
```

## No Dynamic require()

Only static imports are supported:

```typescript,no-test
// Supported
import { foo } from "./module";

// Not supported
const mod = require("./module");
const mod = await import("./module");
```

## No Prototype Manipulation

Perry compiles classes to fixed structures. Dynamic prototype modification is not supported:

```typescript,no-test
// Not supported
MyClass.prototype.newMethod = function() {};
Object.setPrototypeOf(obj, proto);
```

## No Symbol Type

The `Symbol` primitive type is not currently supported:

```typescript,no-test
// Not supported
const sym = Symbol("description");
```

## No WeakMap/WeakRef

Weak references are not implemented:

```typescript,no-test
// Not supported
const wm = new WeakMap();
const wr = new WeakRef(obj);
```

## No Proxy

The `Proxy` object is not supported:

```typescript,no-test
// Not supported
const proxy = new Proxy(target, handler);
```

## Limited Error Types

`Error` and basic `throw`/`catch` work, but custom error subclasses have limited support:

```typescript,no-test
// Works
throw new Error("message");

// Limited
class CustomError extends Error {
  code: number;
  constructor(msg: string, code: number) {
    super(msg);
    this.code = code;
  }
}
```

## Threading Model

Perry supports real multi-threading via `parallelMap` and `spawn` from `perry/thread`. See [Multi-Threading](../threading/overview.md).

Threads do not share mutable state — closures passed to thread primitives cannot capture mutable variables (enforced at compile time). Values are deep-copied across thread boundaries. There is no `SharedArrayBuffer` or `Atomics`.

## No Computed Property Names

Dynamic property keys in object literals are limited:

```typescript,no-test
// Supported
const key = "name";
obj[key] = "value";

// Not supported
const obj = { [key]: "value" };
```

## npm Package Compatibility

Not all npm packages work with Perry:

- **Natively supported**: ~50 popular packages (fastify, mysql2, redis, etc.) — these are compiled natively. See [Standard Library](../stdlib/overview.md).
- **`compilePackages`**: Pure TS/JS packages can be compiled natively via [configuration](../getting-started/project-config.md).
- **Not supported**: Packages requiring native addons (`.node` files), `eval()`, dynamic `require()`, or Node.js internals.

## Workarounds

### Dynamic Behavior

For cases where you need dynamic behavior, use the JavaScript runtime fallback:

```typescript,no-test
import { jsEval } from "perry/jsruntime";
// Routes specific code through QuickJS for dynamic evaluation
```

### Type Narrowing

Since there's no runtime type checking, use explicit checks:

```typescript,no-test
// Instead of relying on type narrowing from generics
if (typeof value === "string") {
  // String path
} else if (typeof value === "number") {
  // Number path
}
```

## Next Steps

- [Supported Features](supported-features.md) — What does work
- [Type System](type-system.md) — How types are handled
