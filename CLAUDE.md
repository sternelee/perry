# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.36

## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 22 tests). Each test exercises a feature cluster and is compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last sweep (post-v0.4.87):** **8/22 passing**, **347 total diff lines**.

| Status | Test | Diffs |
|--------|------|-------|
| ✅ PASS | `date_methods` | 0 |
| ✅ PASS | `encoding_timers` | 0 |
| ✅ PASS | `error_extensions` | 0 |
| ✅ PASS | `fetch_response` | 0 |
| ✅ PASS | `json_advanced` | 0 |
| ✅ PASS | `node_path` | 0 |
| ✅ PASS | `node_process` | 0 |
| ✅ PASS | `weakref_finalization` | 0 |
| 🟡 close | `regexp_advanced` | 2 (lookbehind only) |
| 🟡 close | `generators` | 3 |
| 🟡 close | `number_math` | 4 |
| 🟡 close | `string_methods` | 2 (lone surrogates only) |
| 🟡 mid | `class_advanced` | 18 |
| 🟡 mid | `proxy_reflect` | 27 (segfault) |
| 🟡 mid | `object_methods` | 28 |
| 🟡 mid | `node_fs` | 30 |
| 🟡 mid | `global_apis` | 30 |
| 🔴 work | `symbols` | 31 (segfault) |
| 🔴 work | `async_advanced` | 35 (segfault) |
| 🔴 work | `console_methods` | 40 |
| 🔴 work | `array_methods` | 45 |
| 🔴 work | `node_crypto_buffer` | 46 |

**Known categorical gaps**: lookbehind regex (Rust `regex` crate limitation), `Proxy`/`Reflect` not implemented, `Symbol(...)` returns garbage, `Object.getPrototypeOf` returns wrong sentinel, `console.dir` formatting differs from Node, `console.group*` doesn't indent, `console.table` works for the standard shapes, lone surrogate handling (`isWellFormed`/`toWellFormed` — needs WTF-8 support).

**Next-impact targets** (biggest single-commit wins): `console.dir` formatting + `console.group` indent (~15 lines), `Promise.withResolvers` + segfault fix (~35 lines), `URL`/`Blob`/`AbortController` extensions (~15 lines), `Proxy` identity stub (~10 lines), `Symbol` sentinel stub (~10 lines).

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
TypeScript (.ts) → Parse (SWC) → AST → Lower → HIR → Transform → Codegen (LLVM) → .o → Link (cc) → Executable
```

| Crate | Purpose |
|-------|---------|
| **perry** | CLI driver (parallel module codegen via rayon) |
| **perry-parser** | SWC wrapper for TypeScript parsing |
| **perry-types** | Type system definitions |
| **perry-hir** | HIR data structures (`ir.rs`) and AST→HIR lowering (`lower.rs`) |
| **perry-transform** | IR passes (closure conversion, async lowering, inlining) |
| **perry-codegen** | LLVM-based native code generation |
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

### LLVM Type Mismatches
- Loop counter optimization produces i32 — always convert before passing to f64/i64 functions
- Check LLVM value types before conversion; handle f64↔i64, i32→f64, i32→i64
- Constructor parameters always f64 (NaN-boxed) at signature level

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

Keep entries here to 1-2 lines max. Detailed write-ups live in CHANGELOG.md.

- **v0.5.36** — Buffer param `src[i]` reads/writes bytes (closes #42). HIR lowering of computed-member access on locals now treats `Type::Named("Buffer")` as a synonym for `Uint8Array`, routing to `Uint8ArrayGet`/`Set` instead of the generic `IndexGet` that returned NaN-boxed pointer bits as a denormal f64.
- **v0.5.35** — `process.argv.slice(N)` returns a real array (closes #41). `Expr::ProcessArgv` added to the HIR `.slice()` array-receiver allow-list so it lowers to `ArraySlice` instead of falling through to String.slice semantics.
- **v0.5.34** — `Math.imul(a, b)` lowers in the LLVM backend (closes #40). `fptosi→trunc i32→mul→sitofp` inline sequence — matches Node for every 32-bit-wrap case. Unblocks FNV-1a-32 / MurmurHash3 / xxhash32 / CRC32 / PCG in user TS.
- **v0.5.33** — JSON.stringify/parse on large arrays (closes #43, #44). GC now transitively marks arena-block-persisting objects (fixes malloc children freed under `arr.push` when new object lives only in caller-saved regs); `trace_array` length cap raised 65k → 16M; `stringify_value` dispatches on GC `obj_type` tag instead of capacity heuristic that misread length-≥10k arrays as strings.
- **v0.5.32** — BigInt bitwise ops (`&`, `|`, `^`, `<<`, `>>`) dispatch through the runtime's bigint helpers (closes #39). Previously these fell through to the i32 ToInt32 path, which `fptosi`'d NaN-boxed bigint bits and returned garbage — XOR gave small signed ints, AND-masking collapsed to 0.
- **v0.5.31** — `new Uint8Array(n)` with non-literal `n` allocates correctly (closes #38). Runtime dispatch `js_uint8array_new(val)` inspects the NaN-box tag and routes numeric lengths to `js_uint8array_alloc` instead of misreading them as `ArrayHeader*`.
- **v0.5.30** — Dynamic property write at Node parity (closes #37). Shape-transition cache `(prev_keys, key_ptr) → (next_keys, slot_idx)` skips linear scan; `Vec<u64>` overflow replaces nested HashMap; last-accessed Vec cache skips outer HashMap lookup; inlined fast-path field write; single `ANY_DESCRIPTORS_IN_USE` gate. 10k×20 build: 43.3→6.4ms (-85%). cols≥20: parity/edge vs Node v25 (cols=80: 22.4 vs 22.6ms).
- **v0.5.29** — Row-object alloc perf (-14% on @perry/postgres 10k-row bulk decode): skip needless keys_array clones via `GC_FLAG_SHAPE_SHARED`, defer descriptor-lookup String alloc, i64 bigint fast path.
- **v0.5.28** — Register module-level `let`/`const` globals as GC roots (closes #36). Stops sweep of `const X = new Map(...)` when only the stack-less global holds the ref.
- **v0.5.27** — GC root scanners for `ws` / `http` / `events` / `fastify` listener closures (refs #35). Follow-up sweep after v0.5.26.
- **v0.5.26** — GC root scanner for `net.Socket` listener closures in `NET_LISTENERS` (closes #35). Unblocked after v0.5.25 made malloc-triggered GC common.
- **v0.5.25** — GC fires from `gc_malloc` + per-thread adaptive malloc-count threshold (closes #34). 2M bigint allocs: 8.45 GB → 36 MB peak RSS.
- **v0.5.24** — Bigint literals use `BIGINT_TAG`, `BigInt()` coercion, `Binary` ops dispatch to bigint runtime when statically typed (closes #33).
- **v0.5.23** — Module init follows topological order (not alphabetical); `import * as O` namespace property dispatch (closes #32).
- **v0.5.22** — Doc URL swaps; compile output gated behind `--verbose`; CI pins `MACOSX_DEPLOYMENT_TARGET=13.0`.
- **v0.5.21** — Fastify handler params tagged `FastifyRequest`/`FastifyReply` in HIR; `gc()` no-ops while tokio servers live (closes #30, #31).
- **v0.5.20** — `String.length` returns UTF-16 code units (`"café".length` → 4, `"😀".length` → 2) (closes #18 partially).
- **v0.5.19** — Restore native module dispatch (mysql/pg/redis/mongo/sqlite/fastify/ws) lost in v0.5.0 cutover; fix `gc()` symbol; drop `--warn-unresolved-symbols` (closes #28).
- **v0.5.18** — Native `axios` dispatch; fetch GET segfault fix; async pump wired into await loop; `.d.ts` stubs for `perry/ui|thread|i18n|system` (closes #24-#27).
- **v0.5.17** — Escape analysis + scalar replacement of non-escaping objects (zero heap allocs on hot paths). Perry beats Node on all 15 benchmarks.
- **v0.5.16** — watchOS device target uses `arm64_32` (ILP32) triple instead of `aarch64`.
- **v0.5.15** — perry/ui `State` constructor + `.value`/`.set()` dispatch; `is_perry_builtin()` guard in check-deps (closes #24, #25).
- **v0.5.14** — Windows build fix: `date.rs` split into `#[cfg(unix)]` (`localtime_r`) / `#[cfg(windows)]` (`localtime_s`) branches.
- **v0.5.13** — `Buffer.indexOf`/`includes` routed to buffer dispatch instead of string-method path.
- **v0.5.12** — perry/ui full widget dispatch (~40 methods, VStack/HStack/Button special cases); mango renders full UI.
- **v0.5.11** — Inline-allocator: post-init boundary for keys_array load; `js_register_class_parent` so `instanceof` walks inheriting classes. Parity 80% → 94%.
- **v0.5.10** — `perry/ui.App({...})` dispatch — mango actually launches (enters `NSApplication.run()`).
- **v0.5.9** — `let C = SomeClass; new C()` resolves the alias; `refine_type_from_init` follows through `local_class_aliases`.
- **v0.5.2** — Fast-math FMFs on `fadd`/`fsub`/...; integer-modulo fast path (`fptosi → srem → sitofp`). Beats Node on 8/11 numeric benchmarks.
- **v0.5.0** — Cranelift backend deleted; LLVM is the only codegen. Parity identical pre/post: 102 MATCH / 9 DIFF (91.8%).
