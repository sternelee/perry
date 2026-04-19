# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.105

## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 22 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last sweep:** **14/28 passing**, **117 total diff lines**.

| Status | Test | Diffs |
|--------|------|-------|
| ✅ PASS | `array_methods`, `bigint`, `buffer_ops`, `closures`, `date_methods`, `error_extensions`, `fetch_response`, `generators`, `json_advanced`, `number_math`, `object_methods`, `proxy_reflect`, `regexp_advanced`, `symbols` | 0 |
| 🟡 close | `async_advanced` (4), `encoding_timers` (4), `node_crypto_buffer` (4), `node_fs` (4), `node_path` (4), `node_process` (4), `typeof_instanceof` (4), `weakref_finalization` (4) | 4 |
| 🟡 mid | `map_set_extended` (6), `string_methods` (8), `typed_arrays` (12), `class_advanced` (14) | 6–14 |
| 🔴 work | `global_apis` (22), `console_methods` (23) | 22–23 |

**Known categorical gaps**: lookbehind regex (Rust `regex` crate), `console.dir`/`console.group*` formatting, lone surrogate handling (WTF-8).

## Workflow Requirements

**IMPORTANT:** Follow these practices for every code change:

1. **Update CLAUDE.md**: Add 1-2 line entry in "Recent Changes" for new features/fixes
2. **Increment Version**: Bump patch version (e.g., 0.5.48 → 0.5.49)
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

Mark-sweep GC in `crates/perry-runtime/src/gc.rs` with conservative stack scanning. Arena objects (arrays, objects) discovered by linear block walking. Malloc objects (strings, closures, promises, bigints, errors) tracked in thread-local Vec. Triggers on arena block allocation (~8MB), malloc count threshold, or explicit `gc()` call. 8-byte GcHeader per allocation.

## Threading (`perry/thread`)

Single-threaded by default. `perry/thread` provides:
- **`parallelMap(array, fn)`** / **`parallelFilter(array, fn)`** — data-parallel across all cores
- **`spawn(fn)`** — background OS thread, returns Promise

Values cross threads via `SerializedValue` deep-copy. Each thread has independent arena + GC. Results from `spawn` flow back via `PENDING_THREAD_RESULTS` queue, drained during `js_promise_run_microtasks()`.

## Native UI (`perry/ui`)

Declarative TypeScript compiles to AppKit/UIKit calls. Handle-based widget system (1-based i64 handles, NaN-boxed with POINTER_TAG). `--target ios-simulator`/`--target ios`/`--target tvos-simulator`/`--target tvos` for cross-compilation.

**To add a new widget** — change 4 places:
1. Runtime: `crates/perry-ui-macos/src/widgets/` — create widget, `register_widget(view)`
2. FFI: `crates/perry-ui-macos/src/lib.rs` — `#[no_mangle] pub extern "C" fn perry_ui_<widget>_create`
3. Codegen: `crates/perry-codegen/src/codegen.rs` — declare extern + NativeMethodCall dispatch
4. HIR: `crates/perry-hir/src/lower.rs` — only if widget has instance methods

## Compiling npm Packages Natively (`perry.compilePackages`)

Configured in `package.json`:
```json
{ "perry": { "compilePackages": ["@noble/curves", "@noble/hashes"] } }
```
First-resolved directory cached in `compile_package_dirs`; subsequent imports redirect to the same copy (dedup).

## Known Limitations

- **No runtime type checking**: Types erased at compile time. `typeof` via NaN-boxing tags. `instanceof` via class ID chain.
- **No shared mutable state across threads**: No `SharedArrayBuffer` or `Atomics`.

## Common Pitfalls & Patterns

### NaN-Boxing Mistakes
- **Double NaN-boxing**: If value is already F64, don't NaN-box again. Check `builder.func.dfg.value_type(val)`.
- **Wrong tag**: Strings=STRING_TAG, objects=POINTER_TAG, BigInt=BIGINT_TAG.
- **`as f64` vs `from_bits`**: `u64 as f64` is numeric conversion (WRONG). Use `f64::from_bits(u64)` to preserve bits.

### LLVM Type Mismatches
- Loop counter optimization produces i32 — always convert before passing to f64/i64 functions
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

Keep entries to 1-2 lines max. Full details in CHANGELOG.md.

- **v0.5.105** — `Int32Array.length` (and other typed-array `.length`) returned 0 because `js_value_length_f64` only handled NaN-boxed pointers (top16 == 0x7FFD); typed arrays sometimes flow as raw `bitcast i64 → double` with top16 == 0. Added a raw-pointer arm guarded on the Darwin mimalloc heap window (≥ 2 TB, < 128 TB) that consults `is_registered_buffer` / `lookup_typed_array_kind`.
- **v0.5.104** — Extend the v0.5.103 inliner fix: `substitute_locals` now also walks `WeakRefNew`/`WeakRefDeref`/`FinalizationRegistryNew`/`Object{Keys,Values,Entries,FromEntries,IsFrozen,IsSealed,IsExtensible,Create}`/`ArrayFrom`/`Uint8ArrayFrom`/`IteratorToArray`/`StructuredClone`/`QueueMicrotask`/`ProcessNextTick`/`Json{Parse,Stringify}`/`ArrayIsArray`/`Math{Sqrt,Floor,Ceil,Round,Abs,Log,Log2,Log10,Log1p,Clz32,MinSpread,MaxSpread}`. Same root cause as v0.5.103 (catch-all `_ => {}` skipped these single-operand wrappers); same user-visible class of bug — inlined function bodies referenced unmapped pre-inline LocalIds and read the wrong slot. Verified `test_gap_weakref_finalization`'s `function createAndDeref() { const inner = {...}; const innerRef = new WeakRef(inner); ... }` now correctly carries `inner` through to the WeakRef after inlining.
- **v0.5.103** — Inliner `substitute_locals` now traverses single-operand wrapper expressions (`IsUndefinedOrBareNan`, `IsNaN`, `IsFinite`, `Number`/`String`/`Boolean` coerce, `TypeOf`, `Void`, `Await`, `Delete`, `ParseFloat`, etc.). Prior catch-all `_ => {}` left LocalGet refs inside these wrappers unmapped: a destructuring default like `function f({ a, b = "default" })` lowered to `Conditional { condition: IsUndefinedOrBareNan(LocalGet(orig)), ... else_expr: LocalGet(orig) }`. The else_expr was correctly remapped (caught by the Conditional arm + LocalGet arm), but the IsUndefinedOrBareNan-wrapped condition fell through the catch-all → kept the OLD function-internal LocalGet id → the `condition` always read the wrong slot → default branch never fired → `greet({ name: "World" })` printed `undefined, World!` instead of `Hello, World!`. Removed redundant duplicate `Expr::Await` arm at the same time.
- **v0.5.102** — Class-instance scalar replacement no longer drops the constructor when a getter or setter is invoked. `let r = new C(...); console.log(r.gettableProp)` segfaulted because the escape analysis in `collect_non_escaping_news` treated `r.gettableProp` as a plain field read (safe), kept `r` scalar-replaced, the constructor was never emitted (its body was folded into per-field stores into `r`'s slot), and the getter dispatch then passed the uninitialized scalar slot as `this_arg` → `mov w8, #0x18; ldr d0, [x8]` (NULL deref at field offset 24). Methods were already covered by v0.5.95's Call/CallSpread escape rule, but getters/setters lower as bare `Expr::PropertyGet`/`PropertySet`/`PropertyUpdate` with no `Call` wrapper. Fix: thread the class table through `collect_non_escaping_news`/`check_escapes_in_stmts`/`check_escapes_in_expr` and add `is_class_getter`/`is_class_setter` (walking the inheritance chain). The PropertyGet arm now escapes when the property is a known getter; PropertySet/PropertyUpdate likewise escape on known setters. Unblocks test_getters_setters, test_edge_classes, and test_gap_class_advanced (all previously segfaulted at the first getter call mid-test).
- **v0.5.101** — Three CI parity fixes. (a) `[] instanceof Array` returned false because `js_instanceof` had no Array arm — added CLASS_ID_ARRAY (0xFFFF0024) detected via the `GC_TYPE_ARRAY` byte at `obj-8`. (b) `let x = -1 >>> 0` stored as -1 instead of 4294967295 because `collect_integer_let_ids` seeded `>>> 0` initializers as i32 slots (BitOr is fine, UShr is not — `>>> 0` produces an UNSIGNED u32 that doesn't round-trip through a signed i32). Removed UShr from the seed pattern. (c) `arr.length` was returning a stale value after `arr.shift()`/`arr.pop()` because `safe_load_i32_from_ptr` (the inline length fast path) used `!invariant.load` metadata — LLVM forwarded an earlier load past the mutating call per spec. Switched to a plain load. Skipped network tests (test_net_min/socket/upgrade_tls, test_tls_connect) in `run_parity_tests.sh` since CI runners don't host the listener (Connection refused). +3 PASS in gap suite, +N more in parity suite (test_array_methods, test_bitwise, test_gap_typeof_instanceof now pass).
- **v0.5.100** — Walk Array-method HIR variants (`ArrayAt`/`ArrayEntries`/`ArrayKeys`/`ArrayValues`/etc.) in `collect_ref_ids_in_expr` so the array escape analysis catch-all sees the candidate ID. Without these arms `let arr = [...]; arr.at(i)` stayed scalar-replaced and `js_array_at(NULL, i)` returned `undefined`. gap_array_methods DIFF(22)→DIFF(4).
- **v0.5.99** — Gate handle dispatchers by method vocabulary (closes #91). v0.5.98/#88 reorder put `HashHandle` before `is_net_socket_handle`, which fixed hash mis-routing but broke the symmetric case: `socket.write` on a socket whose id collides with a live `HashHandle` routed to `dispatch_hash` and silently returned undefined. Fix: each common-registry dispatcher arm now AND-gates `with_handle::<T>` with a `matches!` on its method vocabulary so unrecognized methods fall through to net.
- **v0.5.98** — Two `'data'`-callback bugs (closes #87, #88). #87: removed the `is_pointer()` gate on field-scan callable dispatch in both `js_native_call_method` paths so `box.resolve(val)` (Promise executor's resolve stored as raw `transmute(ClosureHeader*→f64)`) actually invokes the closure. #88: reorder `js_handle_method_dispatch` to check `HashHandle` before `is_net_socket_handle` so `crypto.createHash().update(buf).digest()` inside a socket `'data'` callback returns the right digest on the first call (handle id collision across registries).
- **v0.5.97** — Two fixes (closes #85, #86). #85: cross-module class constructors now honor defaulted parameters — new `build_default_param_stmts` in `lower_decl.rs` prepends `if (param === undefined) param = default` to constructor and function bodies, so the body is self-sufficient when a cross-module caller pads missing args with `TAG_UNDEFINED`. #86: `crypto.createHash(alg).update(x).digest()` chained across statements works via new `HashHandle` in `perry-stdlib/src/crypto.rs` + `dispatch_hash` arm in `js_handle_method_dispatch`; `js_stdlib_init_dispatch()` now wired into the `main` prologue so handle dispatch is live for sync-only programs.
- **v0.5.96** — Condvar-backed event loop wait (closes #84). Replaces `js_sleep_ms(10.0)` / `js_sleep_ms(1.0)` in the generated event loop and await busy-wait with `js_wait_for_event()` (`Condvar::wait_timeout` on a shared mutex). Producers (async_bridge, net, ws, http, thread, promise resolve/reject) wake via `js_notify_main_thread()`. `setTimeout(0)×100` goes from 11 ms/iter → 0 ms/iter; setTimeout skew now 1–2 ms (was ~950 ms).
- **v0.5.95** — Bundle fixes for #78–#82 (`@perry/mysql` AOT port). `Buffer.isBuffer` codegens; #79 root cause was scalar-replacement escape analysis treating `PropertyGet { LocalGet(id) }` callees as safe even when the PropertyGet is a method call (fix marks them escaped); uncaught exceptions probe `.message`/`.stack` on user-class throws; `perry check --check-deps` text honest about codegen not running; `process.env` as a value works via new `Expr::ProcessEnv` + lazy `js_process_env()`.
- **v0.5.94** — Cross-module class method dispatch for transitively-reachable classes (closes #83). `import { makeThing } from './lib'` where `makeThing(): Promise<Thing>` left `Thing` invisible to dispatch tables. Fix in `perry/src/commands/compile.rs`: walk `ctx.native_modules` for every module in the transitive origin set and register every `class.is_exported` class with its true defining-module prefix; dedup by class name. Verified against `@perry/postgres`.
- **v0.5.93** — `js_promise_resolved` unwraps inner Promises (closes #77). `async function f(): Promise<T> { return new Promise(...); }` was double-wrapping: the outer's `value` ended up as the inner Promise struct itself. Fix: in `promise.rs`, check `js_value_is_promise(value)` and route through `js_promise_resolve_with_promise` for the chaining adoption path.
- **v0.5.92** — Wire up `process.exit(code?)` (closes #75). New `Expr::ProcessExit` HIR variant + `js_process_exit` codegen dispatch — previously fell through to `NativeMethodCall` and silently no-op'd, so `main().then(() => process.exit(0))` couldn't terminate while `js_stdlib_has_active_handles` reported live sockets.
- **v0.5.91** — Empty `asm sideeffect` barrier in pure loop bodies (closes #74). Prevents LLVM loop-deletion from erasing observably-pure loops used as timing probes; gated on body purity so vectorizable loops keep full optimization budget.
- **v0.5.90** — Release-gated regression workflow + CI-ready `benchmarks/compare.sh`. Hard-gate on version tags (>20% speed / >30% RAM / >15% binary-size regressions block the release); warn-only on main.
- **v0.5.89** — Fix `.github/workflows/test.yml` YAML parse error (dedented content inside `run: |` block scalars terminated them early). No runtime/codegen changes.
- **v0.5.88** — Test/CI/benchmark infrastructure: five-job CI workflow, `benchmarks/compare.sh` + `quick.sh`, seven new microbenchmarks, gap/stress/regression tests, `test-coverage/` audit. No runtime/codegen changes.
- **v0.5.87** — Defer arena block reset for recent blocks (#73 final). Never reset the current + 4 preceding blocks; require 2 consecutive dead observations on older blocks. Bench: 92% SUCCESS (was 64%).
- **v0.5.86** — Root-cause fix for #73: `ValidPointerSet::enclosing_object` handles interior pointers (`arr + 8` in runtime higher-order fns); `mark_stack_roots` captures d0-d31 on ARM64 to catch caller-saved FP regs. SIGSEGV 30%→2%.
- **v0.5.85** — SIGSEGV guard on #73: `clean_arr_ptr` asserts `length ≤ capacity ≤ 100M`; `new Array(N)` sets `GC_FLAG_PINNED` to protect against arena block reset. SIGSEGV 30%→10%.
- **v0.5.84** — Tighten receiver bounds to Darwin mimalloc window (2 TB floor) in inline `.length`, PIC receiver guard, and `clean_arr_ptr`. Crash rate 40%→17%.
- **v0.5.83** — Type-validate inline `.length` receiver: range-guard to 4GB–128TB + GC-type-byte check (only load u32@0 for `GC_TYPE_ARRAY`/`STRING`). Everything else routes through `js_value_length_f64`.
- **v0.5.82** — PIC GC-type-byte check (closes #72): AND `obj_type == GC_TYPE_OBJECT` at `handle-8` with `keys_val == cached_keys` before `pic.hit`. Fixes `array.length` returning element[2] when an Array receiver reached the object PIC.
- **v0.5.81** — Small-value JSON.stringify micro-opts (issue #67): drop redundant entry-side `STRINGIFY_STACK.clear()`, guard exit clear with `is_empty` borrow, `#[inline]` on `stringify_value`/`stringify_object`. small_stringify_100k min=13ms.
- **v0.5.80** — Dangling `!alias.scope`/`!noalias` metadata fix (closes #71): module-wide `LlModule.buffer_alias_counter` so Buffer-using functions emit unique scope ids instead of colliding at scope_idx 0.
- **v0.5.79** — Small-value JSON.stringify fixed-cost reduction (closes #67): shape-template guard for field_count<5, arena-allocate result, closure-field detection via `CLOSURE_MAGIC`, `STRINGIFY_DEPTH` fast path. small_stringify_100k 22→14ms.
- **v0.5.78** — Non-pointer receiver guard on PropertyGet PIC (closes #70): wrap PIC in `icmp_ugt obj_handle, 0x100000` so `globalThis`-as-0.0 falls through to TAG_UNDEFINED instead of segfaulting.
- **v0.5.77** — Scalar replacement for non-escaping object literals (closes #66): `let o = {…}` with only known-key PropertyGet/Set/Update and no capture/escape becomes N stack allocas. Issue benchmarks: all 0-1ms (was up to 79ms).
- **v0.5.76** — Windows x86_64 support: `-march=native` on x86, module-level IC counter, `_setjmp` on MSVC, `f64` `this` in vtable calls, 0x100000 ptr floor. Test suite 88→108 PASS.

Older entries → CHANGELOG.md.
