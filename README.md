# Perry

**One codebase. Every platform. Native performance.**

Perry is a native TypeScript compiler written in Rust. It takes your TypeScript and compiles it straight to native executables — no Node.js, no Electron, no browser engine. Just fast, small binaries that run anywhere.

**Current Version:** 0.5.12 | [Website](https://perryts.com) | [Documentation](https://perryts.github.io/perry/) | [Showcase](https://perryts.com/showcase)

```bash
perry compile src/main.ts -o myapp
./myapp    # that's it — a standalone native binary
```

Perry uses [SWC](https://swc.rs/) for TypeScript parsing and [LLVM](https://llvm.org/) for native code generation. The output is a single binary with no runtime dependencies.

---

## Built with Perry

People are building real apps with Perry today. Here are some highlights:

| Project | What it is | Platforms |
|---------|-----------|-----------|
| [**Bloom Engine**](https://bloomengine.dev) | Native TypeScript game engine — Metal, DirectX 12, Vulkan, OpenGL. Write games in TS, ship native. | macOS, Windows, Linux, iOS, tvOS, Android |
| [**Mango**](https://github.com/MangoQuery/app) | Native MongoDB GUI. ~7 MB binary, <100 MB RAM, sub-second cold start. | macOS, Windows, Linux, iOS, Android |
| [**Hone**](https://hone.codes) | AI-powered native code editor with built-in terminal, Git, and LSP. | macOS, Windows, Linux, iOS, Android, Web |
| [**Pry**](https://github.com/nicktrebes/perry-pry) | Fast, native JSON viewer with tree navigation and search. | macOS, iOS, Android |
| [**dB Meter**](https://dbmeter.app) | Real-time sound level measurement with 60fps updates and per-device calibration. | iOS, macOS, Android |

### Screenshots

**Mango** — Native MongoDB GUI ([source](https://github.com/MangoQuery/app))

<p align="center">
  <img src="docs/images/showcase/mango-explorer.png" width="720" alt="Mango — database explorer view" />
</p>
<p align="center">
  <img src="docs/images/showcase/mango-editor.png" width="720" alt="Mango — document editor view" />
</p>

**Hone** — AI-powered native code editor ([hone.codes](https://hone.codes))

<p align="center">
  <img src="https://hone.codes/screenshot.png" width="720" alt="Hone — AI code editor built with Perry" />
</p>

> Have something you've built with Perry? Open a PR to add it here!

---

## Performance

Perry beats Node.js on every benchmark. Median of 3 runs, macOS ARM64 (Apple Silicon), Node.js v25.

| Benchmark | Perry | Node.js | vs Node | What it tests |
|-----------|-------|---------|---------|---------------|
| factorial | 24ms | 591ms | **24.6x faster** | Modular accumulation (integer fast path) |
| method_calls | 1ms | 11ms | **11x faster** | Class method dispatch (10M calls) |
| loop_overhead | 12ms | 53ms | **4.4x faster** | Tight numeric loop (100M iterations) |
| math_intensive | 14ms | 49ms | **3.5x faster** | Harmonic series with sqrt/sin/cos |
| array_read | 4ms | 13ms | **3.2x faster** | Sequential read (10M elements) |
| closure | 97ms | 303ms | **3.1x faster** | Closure creation + invocation (10M calls) |
| array_write | 3ms | 8ms | **2.6x faster** | Sequential write (10M elements) |
| fibonacci(40) | 309ms | 991ms | **3.2x faster** | Recursive function calls (i64 specialization) |
| string_concat | 1ms | 2ms | **2x faster** | 100K string appends |
| nested_loops | 9ms | 16ms | **1.7x faster** | Nested array access (3000x3000) |
| prime_sieve | 4ms | 7ms | **1.7x faster** | Sieve of Eratosthenes |
| matrix_multiply | 21ms | 34ms | **1.6x faster** | 500x500 matrix multiply |
| binary_trees | 9ms | 9ms | **tied** | Tree allocation + traversal (1.5M nodes) |
| mandelbrot | 24ms | 24ms | **tied** | Complex f64 iteration (1000x1000) |
| object_create | 9ms | 8ms | 0.9x | Object allocation (1M objects) |

Perry compiles to native machine code via LLVM — no JIT warmup, no interpreter overhead. Key optimizations: inline bump allocator for object allocation, i32 loop counters for bounded array access, `reassoc contract` fast-math flags for f64 vectorization, integer-modulo fast path (`fptosi → srem → sitofp` instead of `fmod`), elimination of redundant `js_number_coerce` calls on numeric function returns, and i64 specialization for pure numeric recursive functions.

### Perry vs compiled languages

Perry also competes with systems languages. All implementations use `f64`/`double` to match TypeScript's `number` type — no SIMD intrinsics, no unsafe code. See [`benchmarks/polyglot/`](benchmarks/polyglot/) for source and methodology.

| Benchmark | Perry | Rust | C++ | Go | Swift | Java | Node | Python |
|-----------|-------|------|-----|----|-------|------|------|--------|
| fibonacci | **309** | 316 | **309** | 446 | 399 | 279 | 991 | 15935 |
| loop_overhead | **12** | 95 | 96 | 96 | 95 | 97 | 53 | 2979 |
| array_write | **2** | 6 | **2** | 8 | **2** | 6 | 8 | 392 |
| array_read | **4** | 9 | 9 | 10 | 9 | 11 | 13 | 330 |
| math_intensive | **14** | 48 | 50 | 48 | 48 | 50 | 49 | 2212 |
| object_create | 8 | 0 | 0 | 0 | 0 | 4 | 8 | 161 |
| nested_loops | **8** | **8** | **8** | 9 | **8** | 10 | 17 | 470 |
| accumulate | **25** | 98 | 96 | 96 | 96 | 100 | 592 | 4919 |

Perry beats or ties Rust and C++ on 7 of 8 benchmarks. The polyglot benchmarks all use `f64`/`double` to match TypeScript's `number` type. Perry's advantage comes from domain-specific optimizations — `reassoc` fast-math flags, integer-modulo fast path, and i64 specialization for recursive numeric functions — that exploit properties of TypeScript's `number` type which strict-IEEE compilers can't assume by default. Perry beats Rust on fibonacci because it detects the integer pattern and specializes to `i64` registers, while Rust faithfully compiles the `f64` version. See [`benchmarks/polyglot/RESULTS.md`](benchmarks/polyglot/RESULTS.md) for full analysis.

### LLVM backend progress

Perry switched from Cranelift to LLVM as its sole code generation backend in v0.5.0. The initial cutover had significant performance regressions due to NaN-boxing overhead in the new backend. Subsequent optimization work recovered and surpassed the original numbers:

| Benchmark | Cranelift | LLVM v0.5.0 | LLVM now | Node.js |
|-----------|-----------|-------------|----------|---------|
| method_calls | 16ms | 1,084ms | **1ms** | 11ms |
| math_intensive | 370ms | 131ms | **14ms** | 49ms |
| object_create | 5ms | 318ms | **9ms** | 8ms |
| matrix_multiply | 61ms | 184ms | **21ms** | 34ms |
| nested_loops | 32ms | 57ms | **9ms** | 16ms |
| array_read | 4ms | 26ms | **4ms** | 13ms |
| mandelbrot | 71ms | 47ms | **24ms** | 24ms |
| string_concat | 7ms | 0–1ms | **1ms** | 2ms |
| prime_sieve | 11ms | 11ms | **4ms** | 7ms |
| fibonacci(40) | 505ms | 1,156ms | **309ms** | 991ms |

The Cranelift column is from the pre-v0.5.0 era (the old README on `main`). LLVM v0.5.0 was the initial cutover — it regressed badly because the new backend routed most operations through runtime helpers instead of inlining them. The current LLVM column shows the state after inline bump allocators, i32 loop counters, fast-math flags, integer-mod fast paths, loop-invariant length hoisting, and redundant number-coerce elimination. LLVM now beats both Cranelift and Node on every workload.

### A note on compile times

Cranelift is often praised for fast compilation, and it is — but the difference is smaller than you'd expect. Perry previously used Cranelift and switched to LLVM in v0.5.0. Compile times increased by only ~20-50ms (8-19%), because the bulk of Perry's compile time is SWC parsing, HIR lowering, and linking — not the codegen backend. On a typical file LLVM adds about 25ms over Cranelift while producing code that runs up to 24x faster. A worthwhile trade.

Run benchmarks yourself: `cd benchmarks/suite && ./run_benchmarks.sh` (requires node, cargo).

## Binary Size

Perry produces small, self-contained binaries with no external dependencies at run time:

| Program | Binary Size |
|---------|-------------|
| `console.log("Hello, world!")` | **~330KB** |
| hello world + `fs` / `path` / `process` imports | ~380KB |
| full stdlib app (fastify, mysql2, etc.) | ~48MB |
| with `--enable-js-runtime` (V8 embedded) | +~15MB |

Perry automatically detects which parts of the runtime your program uses and only links what's needed.

---

## Installation

### macOS (Homebrew)

```bash
brew install perryts/perry/perry
```

### Windows (winget)

```bash
winget install PerryTS.Perry
```

### Debian / Ubuntu (APT)

```bash
curl -fsSL https://perryts.github.io/perry-apt/perry.gpg.pub | sudo gpg --dearmor -o /usr/share/keyrings/perry.gpg
echo "deb [signed-by=/usr/share/keyrings/perry.gpg] https://perryts.github.io/perry-apt stable main" | sudo tee /etc/apt/sources.list.d/perry.list
sudo apt update && sudo apt install perry
```

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/PerryTS/perry/main/packaging/install.sh | sh
```

### From source

```bash
git clone https://github.com/PerryTS/perry.git
cd perry
cargo build --release
# Binary at: target/release/perry
```

### Requirements

Perry requires a C linker to link compiled executables:
- **macOS:** Xcode Command Line Tools (`xcode-select --install`)
- **Linux:** GCC or Clang (`sudo apt install build-essential`)
- **Windows:** MSVC (Visual Studio Build Tools)

Run `perry doctor` to verify your environment.

---

## Quick Start

```bash
# Initialize a new project
perry init my-project
cd my-project

# Compile and run
perry compile src/main.ts -o myapp
./myapp

# Or compile and run in one step
perry run .

# Check TypeScript compatibility
perry check src/

# Diagnose environment
perry doctor
```

---

## Real-World Example: API Server with ESM Modules

Perry supports standard ES module imports and npm packages. Here's a real-world API server with multi-file project structure:

**Project layout:**
```
my-api/
├── package.json
├── src/
│   ├── main.ts
│   ├── config.ts
│   └── routes/
│       └── users.ts
└── node_modules/
```

**src/config.ts**
```typescript
export const config = {
  port: 3000,
  dbHost: process.env.DB_HOST || 'localhost',
};
```

**src/routes/users.ts**
```typescript
export function getUsers(): object[] {
  return [
    { id: 1, name: 'Alice' },
    { id: 2, name: 'Bob' },
  ];
}

export function getUserById(id: number): object | undefined {
  return getUsers().find((u: any) => u.id === id);
}
```

**src/main.ts**
```typescript
import fastify from 'fastify';
import { config } from './config';
import { getUsers, getUserById } from './routes/users';

const app = fastify();

app.get('/api/users', async () => {
  return getUsers();
});

app.get('/api/users/:id', async (request) => {
  const { id } = request.params as { id: string };
  return getUserById(parseInt(id));
});

app.listen({ port: config.port }, () => {
  console.log(`Server running on port ${config.port}`);
});
```

**Compile and run:**
```bash
perry compile src/main.ts -o my-api && ./my-api
# or: perry run .
```

The output is a standalone binary — no `node_modules` needed at runtime.

---

## Example Projects

The `example-code/` directory contains ready-to-run projects showing Perry in real-world scenarios:

| Example | Stack | What it demonstrates |
|---------|-------|---------------------|
| **[express-postgres](example-code/express-postgres/)** | Express + PostgreSQL | Multi-file routes, middleware (CORS, Helmet), connection pooling, error handling |
| **[fastify-redis-mysql](example-code/fastify-redis-mysql/)** | Fastify + Redis + MySQL | Rate limiting, caching layer, database queries, dotenv config |
| **[hono-mongodb](example-code/hono-mongodb/)** | Hono + MongoDB | Lightweight HTTP framework with document database |
| **[nestjs-typeorm](example-code/nestjs-typeorm/)** | NestJS + TypeORM | Decorator-based architecture, dependency injection |
| **[nextjs-prisma](example-code/nextjs-prisma/)** | Next.js-style + Prisma | ORM integration, database migrations |
| **[koa-redis](example-code/koa-redis/)** | Koa + Redis | Middleware composition, session storage |
| **[http-server](example-code/http-server/)** | Raw HTTP | Low-level request handling, routing, JSON APIs |
| **[blockchain-demo](example-code/blockchain-demo/)** | Custom | Blockchain implementation in pure TypeScript |

Each example has its own `package.json` and can be compiled with:

```bash
cd example-code/fastify-redis-mysql
npm install
perry compile src/index.ts -o server && ./server
```

---

## Native UI

Perry includes a declarative UI system (`perry/ui`) that compiles directly to native platform widgets — no WebView, no Electron:

```typescript
import { App, VStack, HStack, Text, Button, State } from 'perry/ui';

const count = State(0);

App({
  title: 'My App',
  width: 400,
  height: 300,
  body: VStack(16, [
    Text(`Count: ${count.value}`),
    HStack(8, [
      Button('Decrement', () => count.set(count.value - 1)),
      Button('Increment', () => count.set(count.value + 1)),
    ]),
  ]),
});
```

**9 platforms from one codebase:**

| Platform | Backend | Target Flag |
|----------|---------|-------------|
| macOS | AppKit (NSView) | *(default on macOS)* |
| iOS / iPadOS | UIKit | `--target ios` / `--target ios-simulator` |
| tvOS | UIKit | `--target tvos` / `--target tvos-simulator` |
| watchOS | WatchKit | `--target watchos` / `--target watchos-simulator` |
| Android | Android Views (JNI) | `--target android` |
| Windows | Win32 | *(default on Windows)* |
| Linux | GTK4 | *(default on Linux)* |
| Web | DOM (JS codegen) | `--target web` |
| WebAssembly | DOM (WASM) | `--target wasm` |

**127+ UI functions** — widgets (Button, Text, TextField, Toggle, Slider, Picker, Table, Canvas, Image, ProgressView, SecureField, NavigationStack, ZStack, LazyVStack, Form/Section, CameraView), layouts (VStack, HStack), and system APIs (keychain, notifications, file dialogs, clipboard, dark mode, openURL, audio capture).

---

## Multi-Threading

The `perry/thread` module provides real OS threads with compile-time safety — no shared mutable state, no data races:

```typescript
import { parallelMap, parallelFilter, spawn } from 'perry/thread';

// Data-parallel array processing across all CPU cores
const results = parallelMap([1, 2, 3, 4, 5], n => fibonacci(n));

// Parallel filtering
const evens = parallelFilter(numbers, n => n % 2 === 0);

// Background thread with Promise
const result = await spawn(() => expensiveComputation());
```

Values cross threads via deep-copy. Each thread gets its own arena and GC. The compiler enforces that closures don't capture mutable state.

---

## Internationalization (i18n)

Compile-time localization with zero runtime overhead:

```typescript
import { t, Currency, ShortDate } from 'perry/i18n';

console.log(t('hello'));                    // "Hallo" (German locale)
console.log(t('items', { count: 3 }));     // "3 Artikel" (CLDR plural rules)
console.log(Currency(9.99, 'EUR'));         // "9,99 €"
console.log(ShortDate(Date.now()));        // "24.03.2026"
```

Configure in `perry.toml`:

```toml
[i18n]
default_locale = "en"
locales = ["en", "de", "fr", "ja"]
```

All locale strings are baked into the binary at compile time. Native locale detection on all 6 platforms. CLDR plural rules for 30+ locales.

---

## Home Screen Widgets (WidgetKit)

Build native home screen widgets from TypeScript — iOS, Android, watchOS, and Wear OS:

```bash
perry compile src/widget.ts --target ios-widget -o MyWidget
perry compile src/widget.ts --target android-widget -o MyWidget
perry compile src/widget.ts --target watchos-widget -o MyWidget
perry compile src/widget.ts --target wearos-tile -o MyWidget
```

---

## Cross-Platform Targets

```bash
# Desktop (default for host platform)
perry compile src/main.ts -o myapp

# Mobile
perry compile src/main.ts --target ios -o MyApp
perry compile src/main.ts --target ios-simulator -o MyApp
perry compile src/main.ts --target android -o MyApp

# TV / Watch
perry compile src/main.ts --target tvos -o MyApp
perry compile src/main.ts --target watchos -o MyApp

# Web
perry compile src/main.ts --target web -o app.html       # JavaScript output
perry compile src/main.ts --target wasm -o app.wasm      # WebAssembly output

# Home screen widgets
perry compile src/widget.ts --target ios-widget -o MyWidget
perry compile src/widget.ts --target android-widget -o MyWidget
perry compile src/widget.ts --target wearos-tile -o MyWidget
```

---

## Publishing

```bash
perry publish macos   # or: ios / android / linux
```

`perry publish` sends your TypeScript source to perry-hub (the cloud build server), which cross-compiles and signs for each target platform.

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
| ES modules (import/export, re-exports, `import * as`) | ✅ |
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

---

## Compiling npm Packages Natively

Perry can compile pure TypeScript/JavaScript npm packages directly to native code instead of routing them through the V8 runtime. Add a `perry.compilePackages` array to your `package.json`:

```json
{
  "perry": {
    "compilePackages": [
      "@noble/curves",
      "@noble/hashes",
      "superstruct"
    ]
  }
}
```

Then compile with `--enable-js-runtime` as usual. Packages in the list are compiled natively; all others use the V8 runtime.

**Good candidates:** Pure math/crypto libraries, serialization/encoding, data structures with no I/O.
**Keep as V8-interpreted:** Packages using HTTP/WebSocket, native addons, or unsupported Node.js builtins.

---

## Compiler Optimizations

- **NaN-Boxing** — all values are 64-bit words (f64/u64); no boxing overhead for numbers
- **Mark-Sweep GC** — conservative stack scan, arena block walking, 8-byte GcHeader per alloc
- **Parallel Compilation** — rayon-based module codegen, transform passes, and symbol scanning across CPU cores
- **FMA / CSE / Loop Unrolling** — fused multiply-add, common subexpression elimination, 8x loop unroll
- **i32 Loop Counters** — integer registers for loop variables (no f64 round-trips)
- **LICM** — loop-invariant code motion for nested loops
- **Shape-Cached Objects** — 5-6x faster object allocation
- **TimSort** — O(n log n) hybrid sort for `Array.sort()`
- **`__platform__` Constant** — compile-time platform elimination (dead code removal per target)

---

## Plugin System

Compile TypeScript as a native shared library plugin:

```bash
perry compile my-plugin.ts --output-type dylib -o my-plugin.dylib
```

```typescript
import { PluginRegistry } from 'perry/plugin';

export function activate(api: any) {
  api.registerTool('my-tool', (args: any) => { /* ... */ });
  api.on('event', (data: any) => { /* ... */ });
}
```

---

## Testing (Geisterhand)

Perry includes Geisterhand, an in-process UI testing framework with HTTP-driven interaction and screenshot capture:

```bash
perry compile src/main.ts --enable-geisterhand -o myapp
./myapp
# UI test server runs on http://localhost:7676
```

Supports screenshot capture on all native platforms. See the [Geisterhand docs](https://perryts.github.io/perry/testing/geisterhand.html) for details.

---

## Ecosystem

| Package | Description |
|---------|-------------|
| [**Bloom Engine**](https://bloomengine.dev) | Native TypeScript game engine — 2D/3D rendering, skeletal animation, spatial audio, physics. Metal/DirectX 12/Vulkan/OpenGL. |
| [perry-react](https://github.com/PerryTS/react) | React/JSX that compiles to native widgets. Standard React components → native macOS/iOS/Android app. |
| [perry-sqlite](https://github.com/PerryTS/sqlite) | SQLite with a Prisma-compatible API (`findMany`, `create`, `upsert`, `$transaction`, etc.) |
| [perry-postgres](https://github.com/PerryTS/postgres) | PostgreSQL with the same Prisma-compatible API |
| [perry-prisma](https://github.com/PerryTS/prisma) | MySQL with the same Prisma-compatible API |
| [perry-apn](https://github.com/PerryTS/push) | Apple Push Notifications (APNs) native library |
| [@perry/threads](https://github.com/PerryTS/perry/tree/main/packages/perry-threads) | Web Worker parallelism (`parallelMap`, `parallelFilter`, `spawn`) for browser/Node.js |
| [perry-starter](https://github.com/PerryTS/starter) | Minimal starter project — get up and running in 30 seconds |
| [perry-demo](https://demo.perryts.com) | Live benchmark dashboard comparing Perry vs Node.js vs Bun |
| [perry-react-dom](https://github.com/PerryTS/react-dom) | Perry React DOM bridge |

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

---

## Commands

| Command | What it does |
|---------|-------------|
| `perry compile <input.ts> -o <output>` | Compile TypeScript to a native binary |
| `perry run <path> [platform]` | Compile and run in one step (supports `ios`, `android`, etc.) |
| `perry init <name>` | Scaffold a new project |
| `perry check <path>` | Validate TypeScript compatibility without compiling |
| `perry publish <platform>` | Build, sign, and publish via the cloud build server |
| `perry doctor` | Check your development environment |
| `perry i18n extract` | Extract translatable strings from source |

### Compiler flags

```
-o, --output <name>      Output file name
--target <target>        ios | ios-simulator | tvos | tvos-simulator |
                         watchos | watchos-simulator | android |
                         web | wasm | ios-widget | android-widget |
                         wearos-tile | watchos-widget
--output-type <type>     executable | dylib
--enable-js-runtime      Embed V8 for npm package compatibility (+~15MB)
--enable-geisterhand     Enable UI testing server
--print-hir              Print HIR for debugging
```

---

## Project Structure

```
perry/
├── crates/
│   ├── perry/                  # CLI (compile, run, check, init, doctor, publish)
│   ├── perry-parser/           # SWC TypeScript parser
│   ├── perry-types/            # Type system
│   ├── perry-hir/              # HIR data structures and AST→HIR lowering
│   ├── perry-transform/        # IR passes (closure conversion, async, inlining)
│   ├── perry-codegen/          # LLVM native codegen
│   ├── perry-codegen-js/       # JavaScript codegen (--target web)
│   ├── perry-codegen-wasm/     # WebAssembly codegen (--target wasm)
│   ├── perry-codegen-swiftui/  # SwiftUI codegen (iOS/watchOS widgets)
│   ├── perry-codegen-glance/   # Android Glance widget codegen
│   ├── perry-codegen-wear-tiles/ # Wear OS Tiles codegen
│   ├── perry-runtime/          # Runtime (NaN-boxing, GC, arena, strings)
│   ├── perry-stdlib/           # Node.js API support (fastify, mysql2, redis, etc.)
│   ├── perry-ui-*/             # Native UI (macOS, iOS, tvOS, watchOS, Android, GTK4, Windows)
│   ├── perry-ui-geisterhand/   # UI testing framework
│   ├── perry-jsruntime/        # Optional V8 interop via QuickJS
│   └── perry-diagnostics/      # Error reporting
├── docs/                       # Documentation site (mdBook)
├── example-code/               # 8 example applications
├── benchmarks/                 # Benchmark suite (Perry vs Node.js vs Bun)
├── packages/                   # npm packages (@perry/threads)
└── test-files/                 # Test suite
```

---

## Runtime Characteristics

- **Garbage Collection** — mark-sweep GC with conservative stack scanning, arena block walking, 8-byte GcHeader per allocation
- **Single-Threaded by Default** — async I/O on Tokio workers, callbacks on main thread. Use `perry/thread` for explicit multi-threading.
- **No Runtime Type Checking** — types erased at compile time. Use `typeof` and `instanceof` for runtime checks.
- **Small Binaries** — ~330KB hello world, ~48MB with full stdlib. Automatically stripped.

---

## Development

```bash
cargo build --release                                    # Build everything
cargo build --release -p perry-runtime -p perry-stdlib   # Rebuild runtime (after changes)
cargo test --workspace --exclude perry-ui-ios            # Run tests
cargo run --release -- compile file.ts -o out && ./out   # Compile and run
cargo run --release -- compile file.ts --print-hir       # Debug HIR
```

### Adding a new feature

1. **HIR** — add node type to `crates/perry-hir/src/ir.rs`
2. **Lowering** — handle AST→HIR in `crates/perry-hir/src/lower.rs`
3. **Codegen** — generate LLVM IR in `crates/perry-codegen/src/codegen.rs`
4. **Runtime** — add runtime functions in `crates/perry-runtime/` if needed
5. **Test** — add `test-files/test_feature.ts`

---

## License

MIT
