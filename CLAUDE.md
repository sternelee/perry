# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and Cranelift for code generation.

**Current Version:** 0.4.84

## Workflow Requirements

**IMPORTANT:** Follow these practices for every code change:

1. **Update CLAUDE.md**: Add 1-2 line entry in "Recent Changes" for new features/fixes
2. **Increment Version**: Bump patch version (e.g., 0.2.147 → 0.2.148)
3. **Commit Changes**: Include code changes and CLAUDE.md updates together

## Build Commands

```bash
cargo build --release                          # Build all crates
cargo build --release -p perry-runtime -p perry-stdlib  # Rebuild runtime (MUST rebuild stdlib too!)
cargo test --workspace --exclude perry-ui-ios  # Run tests (exclude iOS on macOS host)
cargo run --release -- file.ts -o output && ./output    # Compile and run TypeScript
cargo run --release -- file.ts --print-hir              # Debug: print HIR
```

## Architecture

```
TypeScript (.ts) → Parse (SWC) → AST → Lower → HIR → Transform → Codegen (Cranelift) → .o → Link (cc) → Executable
```

| Crate | Purpose |
|-------|---------|
| **perry** | CLI driver (parallel module codegen via rayon) |
| **perry-parser** | SWC wrapper for TypeScript parsing |
| **perry-types** | Type system definitions |
| **perry-hir** | HIR data structures (`ir.rs`) and AST→HIR lowering (`lower.rs`) |
| **perry-transform** | IR passes (closure conversion, async lowering, inlining) |
| **perry-codegen** | Cranelift-based native code generation |
| **perry-runtime** | Runtime: value.rs, object.rs, array.rs, string.rs, gc.rs, arena.rs, thread.rs |
| **perry-stdlib** | Node.js API support (mysql2, redis, fetch, fastify, ws, etc.) |
| **perry-ui** / **perry-ui-macos** / **perry-ui-ios** / **perry-ui-tvos** | Native UI (AppKit/UIKit) |
| **perry-jsruntime** | JavaScript interop via QuickJS |

## NaN-Boxing

Perry uses NaN-boxing to represent JavaScript values in 64 bits (`perry-runtime/src/value.rs`):

```
TAG_UNDEFINED = 0x7FFC_0000_0000_0001    BIGINT_TAG  = 0x7FFA (lower 48 = ptr)
TAG_NULL      = 0x7FFC_0000_0000_0002    POINTER_TAG = 0x7FFD (lower 48 = ptr)
TAG_FALSE     = 0x7FFC_0000_0000_0003    INT32_TAG   = 0x7FFE (lower 32 = int)
TAG_TRUE      = 0x7FFC_0000_0000_0004    STRING_TAG  = 0x7FFF (lower 48 = ptr)
```

Key functions: `js_nanbox_string/pointer/bigint`, `js_nanbox_get_pointer`, `js_get_string_pointer_unified`, `js_jsvalue_to_string`, `js_is_truthy`

**Module-level variables**: Strings stored as F64 (NaN-boxed), Arrays/Objects as I64 (raw pointers). Access via `module_var_data_ids`.

## Garbage Collection

Mark-sweep GC in `crates/perry-runtime/src/gc.rs` with conservative stack scanning. Arena objects (arrays, objects) discovered by linear block walking (zero per-alloc tracking). Malloc objects (strings, closures, promises, bigints, errors) tracked in thread-local Vec. Triggers on new arena block allocation (~8MB) or explicit `gc()` call. 8-byte GcHeader per allocation.

## Threading (`perry/thread`)

User code is single-threaded by default. `perry/thread` module provides three primitives with compile-time safety (no mutable captures allowed):

- **`parallelMap(array, fn)`** — data-parallel array processing across all CPU cores
- **`parallelFilter(array, fn)`** — data-parallel array filtering across all CPU cores
- **`spawn(fn)`** — background OS thread, returns Promise

Values cross threads via `SerializedValue` deep-copy (zero-cost for numbers, O(n) for strings/arrays/objects). Each thread has independent arena + GC. Arena `Drop` frees blocks when worker threads exit. Results from `spawn` flow back via `PENDING_THREAD_RESULTS` queue, drained during `js_promise_run_microtasks()`.

**Compiler pipeline** also parallelized via rayon: module codegen, transform passes, and nm symbol scanning.

## Native UI (`perry/ui`)

Declarative TypeScript compiles to AppKit/UIKit calls. 47 `perry_ui_*` FFI functions. Handle-based widget system (1-based i64 handles, NaN-boxed with POINTER_TAG). 5 reactive binding types dispatched from `state_set()`. `--target ios-simulator`/`--target ios`/`--target tvos-simulator`/`--target tvos` for cross-compilation.

**To add a new widget** — change 4 places:
1. Runtime: `crates/perry-ui-macos/src/widgets/` — create widget, `register_widget(view)`
2. FFI: `crates/perry-ui-macos/src/lib.rs` — `#[no_mangle] pub extern "C" fn perry_ui_<widget>_create`
3. Codegen: `crates/perry-codegen/src/codegen.rs` — declare extern + NativeMethodCall dispatch
4. HIR: `crates/perry-hir/src/lower.rs` — only if widget has instance methods

## Compiling npm Packages Natively (`perry.compilePackages`)

Projects can list npm packages to compile natively instead of routing to V8. Configured in `package.json`:

```json
{ "perry": { "compilePackages": ["@noble/curves", "@noble/hashes"] } }
```

**Dedup logic**: When `@noble/hashes` appears in multiple `node_modules/`, the first-resolved directory is cached in `compile_package_dirs`. Subsequent imports redirect to the same copy, preventing duplicate linker symbols.

## Known Limitations

- **No runtime type checking**: Types erased at compile time. `typeof` via NaN-boxing tags. `instanceof` via class ID chain.
- **No shared mutable state across threads**: Thread primitives enforce immutable captures at compile time. No `SharedArrayBuffer` or `Atomics`.

## Common Pitfalls & Patterns

### NaN-Boxing Mistakes
- **Double NaN-boxing**: If value is already F64, don't NaN-box again. Check `builder.func.dfg.value_type(val)`.
- **Wrong tag**: Strings=STRING_TAG, objects=POINTER_TAG, BigInt=BIGINT_TAG.
- **`as f64` vs `from_bits`**: `u64 as f64` is numeric conversion (WRONG). Use `f64::from_bits(u64)` to preserve bits.

### Cranelift Type Mismatches
- Loop counter optimization produces I32 — always convert before passing to F64/I64 functions
- Check `builder.func.dfg.value_type(val)` before conversion; handle F64↔I64, I32→F64, I32→I64
- Constructor parameters always F64 (NaN-boxed) at signature level

### Async / Threading
- Thread-local arenas: JSValues from tokio workers invalid on main thread
- Use `spawn_for_promise_deferred()` — return raw Rust data, convert to JSValue on main thread
- Async closures: Promise pointer (I64) must be NaN-boxed with POINTER_TAG before returning as F64

### Cross-Module Issues
- ExternFuncRef values are NaN-boxed — use `js_nanbox_get_pointer` to extract
- Module init order: topological sort by import dependencies
- Optional params need `imported_func_param_counts` propagation through re-exports

### Closure Captures
- `collect_local_refs_expr()` must handle all expression types — catch-all silently skips refs
- Captured string/pointer values must be NaN-boxed before storing, not raw bitcast
- Loop counter i32 values: `fcvt_from_sint` to f64 before capture storage

### Handle-Based Dispatch
- TWO systems: `HANDLE_METHOD_DISPATCH` (methods) and `HANDLE_PROPERTY_DISPATCH` (properties)
- Both must be registered. Small pointer detection: value < 0x100000 = handle.

### objc2 v0.6 API
- `define_class!` with `#[unsafe(super(NSObject))]`, `msg_send!` returns `Retained` directly
- All AppKit constructors require `MainThreadMarker`

## Recent Changes

For older versions (v0.4.80 and earlier), see CHANGELOG.md.

### v0.4.84
- feat: `Array.prototype.entries()` / `keys()` / `values()` — eagerly materialized as new HIR variants `ArrayEntries`/`ArrayKeys`/`ArrayValues` + runtime functions `js_array_entries`/`_keys`/`_values` in `array.rs`. Lowered in 3 dispatch sites (LocalGet, extern_ref, inline literal). Fixes the segfault in `test_gap_array_methods.ts` where `for (const e of arr.entries())` previously fell through to `js_native_call_method` and iterated garbage.

### v0.4.83
- feat: `console.table` — new `js_console_table` runtime function in `builtins.rs` renders array-of-objects, array-of-arrays, and single-object inputs as Node-style box-drawing tables (`┌─┬─┐` chars, single-quoted string cells, left-aligned). Codegen dispatch added in `expr.rs` next to the existing `console.dir`/`console.time` cluster; runtime declared in `runtime_decls.rs`. The three table outputs in `test_gap_console_methods.ts` now match Node byte-for-byte (diff drops 62 → 41).

### v0.4.82
- feat: tagged template literals — `tag\`Hello ${name},${42}!\`` now desugars to `tag(["Hello ", ",", "!"], name, 42)` for any user function (`String.raw` keeps its existing fast path). Unblocks `test_gap_class_advanced` (was COMPILE_ERROR, now 26 diffs). Implementation in `lower.rs:9244` walks `tpl.quasis` for cooked strings and `tpl.exprs` for interpolated values.
- fix: `JSON.stringify(undefined)` now returns NaN-boxed `undefined` instead of empty string. Root cause: `lower_types.rs:275` had `JSON.stringify` return `Type::String`, so `const x = JSON.stringify(undefined)` declared `x` as a string slot and the runtime's TAG_UNDEFINED bits got mis-handled. Changed to `Type::Union(vec![Type::String, Type::Void])` so callers route through dynamic dispatch.
- fix: `JSON.stringify(circular)` now throws an actual `TypeError` instance (was a plain Error with `name` patched to "TypeError"). All three circular-detection sites in `json.rs` now call `js_typeerror_new` so `e instanceof TypeError` returns true. `test_gap_json_advanced` flipped from 6 diffs to PASS.
- fix: `js_cron_timer_tick`/`js_cron_timer_has_pending` link errors when `scheduler` Cargo feature is disabled. The cron agent's commit (`a6fd7a0`) declared these symbols unconditionally in `runtime_decls.rs` and called them from the CLI event loop, but they only existed in `cron.rs` (gated behind `scheduler`). Added unconditional 0-returning stubs in `perry-stdlib/src/lib.rs` under `#[cfg(not(feature = "scheduler"))]` so non-cron projects (e.g. `node:crypto`-only) still link. `test_gap_node_crypto_buffer` flipped from COMPILE_ERROR back to 44 diffs.
- result: gap test sweep is now **6/22 passing** (up from 3/22), 412 total diffs (down from 450). New PASS: `test_gap_json_advanced`, `test_gap_encoding_timers`, `test_gap_node_path`, `test_gap_node_process` (the last three were already passing — the previous count missed them because Node prints stderr warnings about module type, which the new sweep script filters out).

### v0.4.81
- fix: chained `arr.sort(cb).slice(0, N)` (and similar `.map().sort().slice()`) corrupted to a non-array, segfaulting `JSON.stringify`. Added `slice` to the array-producing-receiver whitelist in `lower.rs`'s generic chain handler so it routes to the array path instead of `js_native_call_method`.

### v0.4.80
- fix: `node-cron`'s `cron.schedule(expr, cb)` callback now actually fires (was a TODO stub). New `CronTimer` struct mirrors `INTERVAL_TIMERS`, callbacks fire on the main thread from the CLI event loop, closure pointer survives via `i64` ABI, registered as a GC root, and `cron.schedule(...)` returns a `CronJob` handle so `job.stop()`/`start()`/`isRunning()` resolve correctly.

### v0.4.79
- feat: RegExp lowering — `regex.exec/source/flags/lastIndex`, `m.index/.groups`, `replace(regex, fn)` callback path, `$<name>` named back-refs. Fixed an ARM64 ABI bug where the callback param was declared `I64` but the Rust function takes `f64`.
- fix: `js_instanceof` Error subclass handling restored (was lost when an Object.defineProperty agent overwrote `object.rs` from an older base).

### v0.4.78
- feat: `TextEncoder`/`TextDecoder`, `encodeURI[Component]`/`decodeURI[Component]`, `structuredClone`, `queueMicrotask`. Timer IDs from `setTimeout`/`setInterval` now NaN-boxed with POINTER_TAG so `clearTimeout`/`clearInterval` can recover the ID.

### v0.4.68
- feat: `console.time`/`timeEnd`/`timeLog`/`count`/`countReset`/`group`/`groupEnd`/`groupCollapsed`/`assert`/`dir`/`clear` — backed by thread-local `CONSOLE_TIMERS` and `CONSOLE_COUNTERS`.

### v0.4.67
- feat: auto-detect optimal build profile — `perry compile` rebuilds runtime + stdlib with the smallest matching feature set and switches `panic = "unwind"` → `"abort"` when no `catch_unwind` callers are reachable. Measured: `await fetch(url)` 4.2 MB → 2.9 MB (-31%) automatically. Legacy `--minimal-stdlib` is now a hidden no-op alias; `--no-auto-optimize` is the escape hatch.
