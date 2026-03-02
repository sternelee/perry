# Perry

A native TypeScript compiler written in Rust. Compiles TypeScript source code directly to native executables for macOS, iOS, Android, Windows, GTK4 (Linux), and Web — no Node.js, no Electron, no browser engine.

**Current Version:** 0.2.165 | **Status:** Active Development

## What it does

```bash
perry compile src/main.ts -o myapp
./myapp
```

That's it. TypeScript in, native binary out. The binary runs standalone with no runtime dependencies. Perry uses [SWC](https://swc.rs/) for parsing and [Cranelift](https://cranelift.dev/) for native code generation.

## Performance

*Median of 5 runs on macOS ARM64 (Apple Silicon) vs Node.js v24*

| Benchmark | Perry | Node.js | Speedup |
|-----------|-------|---------|---------|
| loop_overhead | 50ms | 59ms | **1.2x** |
| array_write | 7ms | 9ms | **1.3x** |
| array_read | 4ms | 12ms | **3.0x** |
| fibonacci | 621ms | 1318ms | **2.1x** |
| math_intensive | 22ms | 66ms | **3.0x** |
| object_create | 2ms | 7ms | **3.5x** |
| string_concat | 2ms | 5ms | **2.5x** |
| method_calls | 5ms | 15ms | **3.0x** |
| closure | 14ms | 63ms | **4.5x** |
| binary_trees | 3ms | 8ms | **2.7x** |

**Average speedup: 2.2x faster than Node.js**

> Run benchmarks: `cd benchmarks/suite && ./run_benchmarks.sh`

## Installation

```bash
git clone https://github.com/PerryTS/perry.git
cd perry
cargo build --release
# Binary at: target/release/perry
```

## Quick Start

```bash
# Compile and run
perry compile src/main.ts -o myapp
./myapp

# Initialize a new project
perry init my-project
cd my-project

# Check TypeScript compatibility
perry check src/

# Diagnose environment
perry doctor
```

---

## Native UI

Perry includes a declarative UI system (`perry/ui`) that compiles directly to native platform widgets — no WebView, no Electron:

```typescript
import { App, VStack, HStack, Text, Button, State } from 'perry/ui';

const count = new State(0);

App(
  VStack(
    Text('Counter').fontSize(24).bold(),
    Text('').bindText(count, n => `Count: ${n}`),
    HStack(
      Button('Decrement', () => count.set(count.get() - 1)),
      Button('Increment', () => count.set(count.get() + 1)),
    ),
  ),
  { title: 'My App', width: 400, height: 300 }
);
```

**Supported platforms:**

| Platform | Backend | Status |
|----------|---------|--------|
| macOS | AppKit (NSView) | ✅ Full |
| iOS | UIKit | ✅ Full |
| Android | Android Views (JNI) | ✅ Full |
| Windows | Win32 | ✅ Full |
| Linux | GTK4 | ✅ Full |
| Web | DOM (JS codegen) | ✅ Full |

**127 UI functions** — widgets (Button, Text, TextField, Toggle, Slider, Picker, Table, Canvas, Image, ProgressView, SecureField, NavigationStack, ZStack, LazyVStack, Form/Section), layouts (VStack, HStack), and system APIs (keychain, notifications, file dialogs, clipboard, dark mode, openURL).

---

## Cross-Platform Publishing

```bash
# Build for all platforms via the build server
perry publish --macos --ios --android

# Build for web (outputs self-contained HTML)
perry compile src/main.ts --target web -o dist/app.html
```

`perry publish` sends your TypeScript source to [perry-hub](https://github.com/PerryTS/perry-hub) (the cloud build server), which cross-compiles and signs for each target platform.

**Targets:**

```bash
perry compile src/main.ts --target ios-simulator -o MyApp
perry compile src/main.ts --target ios -o MyApp
perry compile src/main.ts --target web -o app.html  # JS codegen, no Cranelift
```

---

## Supported Language Features

### Core TypeScript

| Feature | Status |
|---------|--------|
| Variables (let, const, var) | ✅ |
| All operators (+, -, *, /, %, **, &, \|, ^, <<, >>, ???, ?., ternary) | ✅ |
| Control flow (if/else, for, while, switch, break, continue) | ✅ |
| Try-catch-finally, throw | ✅ |
| Functions, arrow functions, rest params, defaults | ✅ |
| Closures with mutable captures | ✅ |
| Classes (inheritance, private fields #, static, getters/setters, super) | ✅ |
| Generics (monomorphized at compile time) | ✅ |
| Interfaces, type aliases, union types, type guards | ✅ |
| Async/await, Promise | ✅ |
| Generators (function*) | ✅ |
| ES modules (import/export, re-exports) | ✅ |
| Destructuring (array, object, rest, defaults, rename) | ✅ |
| Spread operator in calls and literals | ✅ |
| RegExp (test, match, replace) | ✅ |
| BigInt (256-bit) | ✅ |
| Decorators | ✅ |

### Standard Library

| Module | Functions |
|--------|-----------|
| `console` | log, error, warn, debug |
| `fs` | readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync, readdirSync, statSync, readFileBuffer, rmRecursive |
| `path` | join, dirname, basename, extname, resolve |
| `process` | env, exit, cwd, argv, uptime, memoryUsage |
| `JSON` | parse, stringify |
| `Math` | floor, ceil, round, abs, sqrt, pow, min, max, random, log, sin, cos, tan, PI |
| `Date` | Date.now(), new Date(), toISOString(), component getters |
| `crypto` | randomBytes, randomUUID, sha256, md5 |
| `os` | platform, arch, hostname, homedir, tmpdir, totalmem, freemem, uptime, type, release |
| `Buffer` | from, alloc, allocUnsafe, byteLength, isBuffer, concat; instance methods |
| `child_process` | execSync, spawnSync, spawnBackground, getProcessStatus, killProcess |
| `Map` | get, set, has, delete, size, clear, forEach, keys, values, entries |
| `Set` | add, has, delete, size, clear, forEach |
| `setTimeout/clearTimeout` | ✅ |
| `setInterval/clearInterval` | ✅ |
| `worker_threads` | parentPort, workerData |

### Native npm Package Implementations

These packages are natively implemented in Rust — no Node.js required:

| Category | Packages |
|----------|----------|
| **HTTP** | fastify, axios, node-fetch, ws (WebSocket) |
| **Database** | mysql2, pg, ioredis |
| **Security** | bcrypt, argon2, jsonwebtoken |
| **Utilities** | dotenv, uuid, nodemailer, zlib, node-cron |

For SQLite and PostgreSQL with a Prisma-like API, see the [ecosystem packages](#ecosystem) below.

---

## Compiler Optimizations

- **NaN-Boxing** — all values are 64-bit words (f64/u64); no boxing overhead for numbers
- **Mark-Sweep GC** — conservative stack scan, arena block walking, 8-byte GcHeader per alloc
- **FMA / CSE / Loop Unrolling** — fused multiply-add, common subexpression elimination, 8x loop unroll
- **i32 Loop Counters** — integer registers for loop variables (no f64 round-trips)
- **LICM** — loop-invariant code motion for nested loops
- **Shape-Cached Objects** — 5–6x faster object allocation
- **Automatic Binary Size Reduction** — links runtime-only when stdlib isn't needed (~300KB vs 48MB for hello world); dead code stripping and `strip` on final binary
- **`__platform__` Constant** — compile-time platform tag (0=macOS, 1=iOS, 2=Android, 3=Windows, 4=Linux); Cranelift constant-folds comparisons and eliminates dead platform branches

---

## Plugin System

Compile TypeScript as a native shared library plugin:

```bash
perry compile my-plugin.ts --output-type dylib -o my-plugin.dylib
```

Use `perry/plugin` in TypeScript:

```typescript
import { PluginRegistry } from 'perry/plugin';

export function activate(api: any) {
  api.registerTool('my-tool', (args: any) => { /* ... */ });
  api.on('event', (data: any) => { /* ... */ });
}
```

---

## System Module

```typescript
import { openURL, isDarkMode, preferencesSet, preferencesGet } from 'perry/system';

openURL('https://example.com');
console.log(isDarkMode());           // true/false
preferencesSet('theme', 'dark');
const theme = preferencesGet('theme');
```

---

## Project Structure

```
perry/
├── crates/
│   ├── perry/              # CLI driver (compile, check, init, doctor, publish)
│   ├── perry-parser/       # SWC TypeScript parser wrapper
│   ├── perry-types/        # Type system definitions
│   ├── perry-hir/          # HIR data structures (ir.rs) and AST→HIR lowering (lower.rs)
│   ├── perry-transform/    # IR passes: closure conversion, async lowering, inlining
│   ├── perry-codegen/      # Cranelift-based native code generation
│   ├── perry-codegen-js/   # JavaScript codegen for --target web
│   ├── perry-runtime/      # Runtime: value.rs, object.rs, gc.rs, array.rs, string.rs, ...
│   ├── perry-stdlib/       # Node.js API support (fastify, mysql2, redis, fetch, ws, ...)
│   ├── perry-ui-macos/     # AppKit widget implementations
│   ├── perry-ui-ios/       # UIKit widget implementations
│   ├── perry-jsruntime/    # Optional V8 JavaScript interop via QuickJS
│   └── perry-diagnostics/  # Error reporting
├── test-files/             # Test suite
├── benchmarks/             # Benchmark suite
├── example-code/           # Example applications
└── CLAUDE.md               # Developer notes
```

---

## Ecosystem

Perry's standard library covers the compiler and runtime. These separate packages extend the ecosystem:

| Package | Description |
|---------|-------------|
| [perry-react](https://github.com/PerryTS/react) | React/JSX → native widgets. Write standard React components; compile to a native macOS/iOS/Android app. |
| [perry-sqlite](https://github.com/PerryTS/perry-sqlite) | SQLite with a Prisma-compatible API (`findMany`, `create`, `upsert`, `$transaction`, etc.) |
| [perry-postgres](https://github.com/PerryTS/perry-postgres) | PostgreSQL with the same Prisma-compatible API |
| [perry-prisma](https://github.com/PerryTS/perry-prisma) | MySQL with the same Prisma-compatible API |
| [perry-apn](https://github.com/PerryTS/perry-apn) | Apple Push Notifications (APNs) native library |
| [perry-hub](https://github.com/PerryTS/perry-hub) | Cloud build server: receives `perry publish` uploads, manages licenses, dispatches builds |
| [perry-pry](https://github.com/PerryTS/perry-pry) | Example app: native JSON viewer (macOS/Linux/Windows) built with `perry/ui` |
| [perry-starter](https://github.com/PerryTS/perry-starter) | Minimal starter project with hello world and benchmarks |

### perry-react

Write React components that compile to native widgets — no DOM, no browser:

```tsx
import { useState } from 'react';
import { createRoot } from 'react-dom/client';

function Counter() {
  const [n, setN] = useState(0);
  return (
    <div>
      <h1>Count: {n}</h1>
      <button onClick={() => setN(n + 1)}>+</button>
    </div>
  );
}

createRoot(null, { title: 'Counter', width: 300, height: 200 }).render(<Counter />);
```

```json
{
  "perry": {
    "packageAliases": {
      "react": "perry-react",
      "react-dom": "perry-react",
      "react/jsx-runtime": "perry-react"
    }
  }
}
```

### perry-sqlite / perry-postgres / perry-prisma

Drop-in replacements for `@prisma/client` backed by Rust (sqlx):

```typescript
import { PrismaClient } from 'perry-sqlite';

const prisma = new PrismaClient();
await prisma.$connect();

const users = await prisma.user.findMany({
  where: { email: { contains: '@example.com' } },
  orderBy: { createdAt: 'desc' },
  take: 20,
});

await prisma.$disconnect();
```

Supported operations: `findMany`, `findFirst`, `findUnique`, `create`, `createMany`, `update`, `updateMany`, `upsert`, `delete`, `deleteMany`, `count`, `$transaction`, `$executeRaw`, `$queryRaw`.

---

## Commands

### `perry compile`

```bash
perry compile <input.ts> [options]

  -o, --output <name>      Output file name
  --target <target>        ios-simulator | ios | web (default: native host)
  --output-type <type>     executable | dylib (default: executable)
  --print-hir              Print HIR for debugging
  --no-link                Produce object file only
  --keep-intermediates     Keep .o files
  --enable-js-runtime      Embed V8 for JS module compatibility (increases binary size ~15MB)
```

### `perry check`

Validates TypeScript compatibility without compiling.

```bash
perry check <path> [--check-deps] [--fix] [--fix-dry-run]
```

### `perry init`

Scaffolds a new Perry project.

```bash
perry init <project-name>
```

### `perry publish`

Builds, signs, and publishes your app for multiple platforms via the cloud build server.

```bash
perry publish --macos --ios --android [--license-key KEY]
```

### `perry doctor`

Checks the development environment (Rust toolchain, linker, platform SDKs).

```bash
perry doctor
```

---

## Runtime Characteristics

- **Garbage Collection** — mark-sweep GC with conservative stack scanning. Triggers on new arena block allocation (~8MB) or explicit `gc()` call. 8-byte GcHeader per allocation.
- **Single-Threaded User Code** — async I/O runs on Tokio worker threads; callbacks dispatch on the main thread.
- **No Runtime Type Checking** — types are erased at compile time. Use `typeof` and `instanceof` for runtime inspection.
- **Small Binaries** — ~300KB for hello world (runtime-only); ~48MB with full stdlib. Binaries are automatically stripped.

---

## Development

```bash
# Build all crates
cargo build --release

# Rebuild runtime + stdlib (required after runtime changes)
cargo build --release -p perry-runtime -p perry-stdlib

# Run tests (exclude iOS crate on macOS host)
cargo test --workspace --exclude perry-ui-ios

# Compile and run a TypeScript file
cargo run --release -- compile file.ts -o output && ./output

# Debug: print HIR
cargo run --release -- compile file.ts --print-hir

# Format / lint
cargo fmt
cargo clippy
```

### Adding a New Feature

1. **HIR** — add node type to `crates/perry-hir/src/ir.rs`
2. **Lowering** — handle AST→HIR in `crates/perry-hir/src/lower.rs`
3. **Codegen** — generate Cranelift IR in `crates/perry-codegen/src/codegen.rs`
4. **Runtime** — add runtime functions in `crates/perry-runtime/` if needed
5. **Test** — add `test-files/test_feature.ts`

---

## License

MIT
