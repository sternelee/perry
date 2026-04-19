# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.99

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

- **v0.5.99** — `socket.write(bytes)` via Map-retrieved object inside a `'data'` callback no longer silently drops bytes (closes #91). Regression introduced by v0.5.98/#88 reorder: that fix moved `HashHandle` BEFORE `is_net_socket_handle` in `perry-stdlib/src/common/dispatch.rs::js_handle_method_dispatch` because a hash with common-registry id colliding with a live socket id was being mis-routed to net (`h.update(buf).digest()` returning length-0). The reorder fixed that direction but introduced the symmetric bug: a net socket whose `NEXT_NET_ID` slot collides with a live `HashHandle` in the common registry now routes `socket.write` to `dispatch_hash`, which has no `write` arm and silently returns undefined — the bytes never reach `js_net_socket_write`. User-visible at `@perry/mysql`: `st.sock.write(handshakeResponse)` returned but the 101-byte HandshakeResponse41 never landed; the driver hit its 10s connect timeout. The `st.writeBytes(bytes)` workaround (closure over the original `sock` variable) succeeded because the closure body's `sock.write(b)` is a closure-captured local with known `net.Socket` type → static `NATIVE_MODULE_TABLE` dispatch → direct `js_net_socket_write` call (bypasses the runtime handle dispatcher entirely). Root cause is fundamental: handle id namespaces are not unified — `net.createConnection` uses `NEXT_NET_ID`, the common registry uses `NEXT_HANDLE`, both start at 1. So id=1 simultaneously identifies a live socket AND the first object created in the common registry; without method-name disambiguation the dispatcher cannot tell which registry the call meant. Fix: each common-registry dispatcher arm in `js_handle_method_dispatch` now AND-gates its `with_handle::<T>` registry check with a `matches!` against the dispatcher's actual method vocabulary — Fastify app (`get`/`post`/.../`listen`), Fastify context (`send`/`status`/...`headers`), ioredis (`connect`/`get`/.../`disconnect`), HashHandle (`update`/`digest`). When the method isn't in a dispatcher's vocabulary, the arm falls through to the next, eventually reaching `is_net_socket_handle` which uses NET_SOCKETS exclusively. Net dispatch is left as-is: it runs last and only matches when the id is genuinely in NET_SOCKETS, so it doesn't need a method gate (and method-gating it would refuse legitimate writes when a method got added before the table here). Verified end-to-end: with `st.sock.write(bytes)` patched throughout `@perry/mysql/src/connection.ts` (workaround removed), the AOT smoke test now connects, returns `SELECT 1` rows, and closes cleanly — pre-fix it hit the 10s timeout. v0.5.98/#88 regression check still passes (sha256 in `'data'` callback returns the canonical 32-byte digest on first call). 111 runtime + 38 perry CLI + 38 codegen/hir tests pass; gap suite unchanged. Proper long-term fix is a single unified id space across net/common/buffer/etc.; this is the surgical method-vocabulary gate.
- **v0.5.98** — Two bundled fixes for bugs that only surfaced inside net-socket `'data'` callbacks. (#87) `obj.method(x)` where `method` is a plain closure stored on a regular object (like `{ resolve }` boxing a Promise executor's resolve function) now actually invokes the closure. Root cause: `js_native_call_method` in `perry-runtime/src/object.rs` has TWO field-scan paths — an early one around line 3116 and a later one around line 3424 — and BOTH gated the callable-dispatch on `field_val.is_pointer()` (POINTER_TAG check). The Promise executor stores its resolve/reject as `transmute(ClosureHeader* → f64)` so the bits live OUTSIDE the NaN range, `is_pointer()` returned false, the early path fell through to `return field_val_bits`, and `box.resolve(val)` became a no-op that returned the raw closure pointer instead of calling `js_promise_resolve`. The awaiting coroutine then never woke. Direct `resolve(val)` worked because the Call path goes through `js_closure_call1` which accepts raw-pointer bits via `get_valid_func_ptr` / `CLOSURE_MAGIC` validation. Fix: in both scan paths, unconditionally call `js_native_call_value` on the found field — it already validates CLOSURE_MAGIC internally and safely returns undefined for non-callables. Prior `is_pointer()` gate removed; also removed the "field found but not callable — return value as-is" fallback which was silently wrong JS semantics anyway (Node throws `"is not a function"` for non-callable `.method()`). (#88) `const h = crypto.createHash('sha256'); h.update(buf); h.digest()` inside a socket 'data' callback returned a Buffer with `length === 0` on the FIRST invocation; subsequent calls were correct. Priming with `sha256(Buffer.alloc(0))` worked around it. Root cause: `net.createConnection` allocates its socket handle in `NET_SOCKETS` via its own `NEXT_NET_ID` counter (starts at 1), while `crypto.createHash` allocates its `HashHandle` in the common `HANDLES` registry via `NEXT_HANDLE` (also starts at 1). In `perry-stdlib/src/common/dispatch.rs::js_handle_method_dispatch`, the net check (`is_net_socket_handle(handle)`) ran BEFORE the hash check and returned true for id=1 regardless of which registry actually owned that semantic slot — so `h.update(buf)` where h's handle-id collided with the live socket's id routed to `dispatch_net_socket` instead of `dispatch_hash`, the socket dispatcher returned a sentinel/stale value, and `digest().length` surfaced as 0. Works on 2nd+ call because by then the hash handle id has advanced past the socket's. Fix: reorder `js_handle_method_dispatch` to check `HashHandle` (via `with_handle::<HashHandle>`, which consults the typed common registry and only matches if the slot downcasts to the right type) before `is_net_socket_handle`. Fastify/ioredis dispatchers are already safe by construction because their `with_handle::<T>` checks fail when the id belongs to a different type. Proper long-term fix would unify id spaces but this is out of scope here. Both fixes tested against local TCP server repros (box.resolve → awaiter resumes; sha256 in data callback → 32-byte digests on every call). 111 runtime + 38 perry CLI + 38 codegen/hir tests pass; gap suite 14/28 passing, 127 total diff lines (down from 131 — `global_apis` improved from DIFF(12) to DIFF(8) now that plain object-method dispatch actually fires).
- **v0.5.97** — Two bundled fixes. (#85) Cross-module class constructors now honor defaulted parameters when the caller omits them. Root cause: the same-module HIR `fill_default_arguments` pass (`perry-hir/src/monomorph.rs`) only fills `Expr::New` arg lists for classes in the caller's own `module.classes` map; the importing module doesn't know the source module's defaults, so `lower_call.rs`'s cross-module path padded missing args with `TAG_UNDEFINED` and the inlined constructor body had no default-apply code to fix it (defaults were applied at the call site, not in the body). Fix: new `build_default_param_stmts()` helper in `perry-hir/src/lower_decl.rs` prepends `if (param === undefined) { param = <default>; }` to every constructor body (and regular function body — same bug class, identical fix) so the body is self-sufficient regardless of how many args the caller passed. Keeps the existing call-site fill as an optimization for inline calls. The #79 `new Cursor(buf)` repro now prints `before=0 / first=253 after=1` — upstream of `@perry/mysql`'s `BufferCursor` pos-tracking breakage. (#86) `crypto.createHash(alg).update(x).digest()` now returns a real Buffer when the user binds the hash to a local before chaining (the three-level chain-collapse in `perry-codegen/src/expr.rs` only caught the single-expression form; three-statement shapes fell through to `js_native_call_method` which returned a non-Perry NaN → `typeof === 'number'`). New `HashHandle` in `perry-stdlib/src/crypto.rs` wraps sha1/sha256/sha512/md5 state behind the existing handle registry; `js_crypto_create_hash(alg)` returns a small-integer handle NaN-boxed with POINTER_TAG, and `common/dispatch.rs::js_handle_method_dispatch` routes subsequent `.update(x)` / `.digest(enc?)` into `dispatch_hash`. Codegen adds a new standalone `Expr::Call` arm for `crypto.createHash(alg)` that calls the runtime fn directly (distinct from the chain-collapse arm, which still fires when all three calls are in a single expression — preserves the fast path). Also wired `js_stdlib_init_dispatch()` into the generated `main` prologue (guarded on `CompileOptions.needs_stdlib` to keep runtime-only links linking) so `HANDLE_METHOD_DISPATCH` is registered before any handle-returning call runs; previously it was only called lazily from `ensure_pump_registered`, which never fired for sync-only programs. SHA-1('hello') now returns `aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d`. 111 runtime + 38 perry CLI + 38 codegen/hir tests pass; gap suite unchanged (14/28 passing, 131 total diff lines — matched pre-change baseline exactly).
- **v0.5.96** — Condvar-backed event loop wait (closes #84). Replaces the old `js_sleep_ms(10.0)` in the generated event loop (`perry-codegen/src/codegen.rs:2291`) and `js_sleep_ms(1.0)` in the await busy-wait (`perry-codegen/src/expr.rs:6224`) with `js_wait_for_event()` — a `Condvar::wait_timeout` on a shared `(Mutex<bool>, Condvar)` exposed from new `perry-runtime/src/event_pump.rs`. Budget is computed per-call as `min(js_timer_next_deadline, js_callback_timer_next_deadline, js_interval_timer_next_deadline, 1000ms idle cap)` — new `js_callback_timer_next_deadline` added in `timer.rs` so `setTimeout(cb, N)` callback-timer deadlines size the wait correctly (without it, `setTimeout(r, 10)` inside `new Promise((r) => setTimeout(r, 10))` hit the 1 s idle cap because TIMER_QUEUE / INTERVAL_TIMERS were both empty). Producers wake the main thread via `js_notify_main_thread()` after enqueueing: wired into `queue_promise_resolution` / `queue_deferred_resolution` in `async_bridge.rs` (covers fetch/ioredis/bcrypt/zlib/spawn_for_promise), `net::push_event` (all net.Socket events), new helper `push_ws_event` (18 WS push sites — originally replaced via `replace_all` on the `.push(` pattern which accidentally rewrote the helper's own body into infinite recursion; re-patched by hand), new `push_http_event` (3 HTTP client sites), `thread.rs::queue_thread_result` (perry/thread.spawn result), and inside `js_promise_resolve` / `js_promise_reject` themselves — needed because the await busy-wait's `js_timer_tick` / `js_callback_timer_tick` can resolve the awaited promise synchronously within a single iteration, after which `js_timer_next_deadline` goes to -1 and `js_wait_for_event` would otherwise block for the 1 s idle cap before the next check-block iteration read the resolved state (first pass of the issue #84 repro showed 1002 ms/iter; adding the resolve-side notify drops it to 0 ms/iter). Verified: `setTimeout(0)` × 100 goes from ~1100 ms (11 ms/iter) → 0 ms/iter (matches/beats Bun ~120 ms, Node ~130 ms); `setTimeout(10/50/100)` skew 1–2 ms (was ~950 ms, which was IDLE_CAP minus actual); promise chain × 100 resolves in 1 ms; 3 new `event_pump` unit tests assert <50 µs notify-wake latency, notify-before-wait survival, and timer-bounded wait correctness. 111 runtime + 38 perry CLI + codegen/hir tests pass; gap suite unchanged (CLAUDE.md's "14/28 passing" baseline was stale — pre-change was 13 passes; same after).
- **v0.5.95** — Bundle fixes for #78–#82 hit while porting `@perry/mysql` to AOT. (a) `Buffer.isBuffer(x)` now codegens: new `Expr::BufferIsBuffer` arm in `perry-codegen/src/expr.rs` calls the already-existing `js_buffer_is_buffer` runtime fn and wraps the i32 result via `i32_bool_to_nanbox`. (b) `#79` root cause was NOT a stale-field read — scalar-replacement escape analysis in `perry-codegen/src/collectors.rs::check_escapes_in_expr` treated every `PropertyGet { LocalGet(id) }` as safe, including when the PropertyGet is the callee of a `Call`. So `new Cursor()` stayed scalar-replaced (fields as allocas, object never allocated) but `c.readUInt8()` (lowered as `Call { PropertyGet { LocalGet(c), "readUInt8" } }`) passed uninitialized `%this` into the method → SIGSEGV on the first `this.pos = X`. Fix: in the `Call` and `CallSpread` arms, when the callee is `PropertyGet { LocalGet(id) }` and `id` is a scalar-replacement candidate, mark it escaped. (c) Uncaught-exception printer in `perry-runtime/src/exception.rs::js_throw` now uses `js_jsvalue_to_string` as the generic fallback and probes `.message`/`.stack` on `OBJECT_TYPE_REGULAR` throws (user-class error shapes) instead of printing opaque `[object] (type=1, bits=0x…)` — Error objects also now emit their stack on the next line. (d) `perry check --check-deps` no longer claims "Compilation is guaranteed to succeed"; the check never runs codegen, so the text now reads "Parsing, HIR lowering, and dependency checks passed (codegen not verified — run `perry compile` for end-to-end validation)". JSON `compilation_guaranteed` key kept for backcompat. (e) `process.env` as a value: new `Expr::ProcessEnv` HIR variant + runtime `js_process_env()` that lazily builds a JS object populated from `std::env::vars()` on first call (thread-local cache). HIR lowering now emits `ProcessEnv` for bare `process.env`, `globalThis.process.env` (walking `TsAs`/`TsNonNull`/`Paren` wrappers so `(globalThis as any).process.env` also works), and the static `process.env.KEY` fast path (EnvGet) still short-circuits `js_getenv` for perf. `const e = process.env; Object.keys(e).length` now returns the real env size instead of 0. No gap-suite regressions; `global_apis` went from DIFF(12) to DIFF(8). 108 runtime + 38 perry CLI + 38 codegen/hir tests pass.
- **v0.5.94** — Cross-module class method dispatch for transitively-reachable classes (closes #83). `import { makeThing } from './lib'` where `makeThing(): Promise<Thing>` left `Thing` invisible to the importing module's dispatch tables because `Thing` itself was never in the specifier list: `receiver_class_name` returned None (the HIR's `await makeThing()` binding comes back as `Any`), dynamic dispatch then enumerated `ctx.methods` looking for implementors of `doWork`, found none (because `opts.imported_classes` only held explicitly-named imports), and fell through to `js_native_call_method` which returned the ObjectHeader itself as a stub. User-visible effect: `t.doWork('hi')` returned `[object Object]` without ever entering the method body. Fix in `perry/src/commands/compile.rs`: after processing each named import's specifiers, walk `ctx.native_modules` (NOT the `exported_classes` BTreeMap — its re-export propagation loop at lines 4110-4173 stamps alias entries under every re-exporter's path, so `Pool` keyed by `pool.ts` AND `index.ts` would hand us the wrong `src_path` → wrong mangled `perry_method_<prefix>__<Class>__<method>` symbol → linker-level "Undefined symbols" failure on real-world driver packages like `@perry/postgres`) for every module in the transitive origin set (`resolved_path` + everything `all_module_exports[resolved_path]` transitively points to), and register every `class.is_exported` class from each such module in `imported_classes` with the class's TRUE defining-module prefix. Dedup by class name in the live vec (not a pre-computed snapshot) so multiple import statements referencing the same chain don't stack duplicate `@perry_class_keys_<modprefix>__<Class>` globals in IR. Same-name local classes still win via the existing `class_table.contains_key(effective_name)` check in `compile_module`. Verified against the issue's repro and the `@perry/postgres` driver: `t.doWork('hi')` / `conn.query('SELECT 1')` now enter the method body, produce the correct return value, and the driver completes cold-start cleanly. Cross-module inheritance (`Derived extends Base`) also verified. 108 runtime + 38 perry CLI + 38 codegen/hir tests pass; gap suite unchanged.
- **v0.5.93** — `js_promise_resolved` unwraps inner Promises (closes #77). `async function f(): Promise<T> { return new Promise<T>((r) => setTimeout(() => r(obj), 50)); }` lowers `return <expr>` (see `perry-codegen/src/stmt.rs::Stmt::Return`) to `js_promise_resolved(v)` to wrap the value in the outer promise. Previously `js_promise_resolved` unconditionally called `js_promise_resolve(p, v)`, so when `v` was a NaN-boxed pointer to another Promise it got stored as the outer's `value` verbatim — `await f()` observed the outer as `Fulfilled`, unwrapped its value, and handed the user back the inner Promise struct itself. `typeof` reported `"object"`, every user-declared field read as `undefined` (the Promise struct has only `state`/`value`/`reason`/`on_fulfilled`/`on_rejected`/`next`), and the inner's `setTimeout` callback fired much later with nobody awaiting it. Fix: in `js_promise_resolved` (`promise.rs`), check `js_value_is_promise(value)` and route through the existing `js_promise_resolve_with_promise` chaining path when the input is itself a promise. This matches ES-spec `Promise.resolve(p) === p` adoption semantics for the async-function return path. User's repro now prints `[producer] fired` before `[main] got` with the correct field values, matching Bun. Also fixes the `@perry/postgres` driver's `query()` — it returns `Promise<QueryResult>`, which was resolving to the stub before `ReadyForQuery` arrived. Edge cases verified: primitives, direct object literals, already-fulfilled, timer-pending, and double-nested async all produce correct values. 108 runtime tests + 38 perry CLI tests + 38 codegen/hir tests pass; gap suite unchanged.
- **v0.5.92** — Wire up `process.exit(code?)` (closes #75). New `Expr::ProcessExit(Option<Box<Expr>>)` in HIR, detected for the `process.exit` member call in `lower.rs::ast::Expr::Call` alongside `chdir`/`kill`, lowered in `expr.rs` as `call void @js_process_exit(double %code)` (defaulting to 0.0 when the arg is omitted). Matching emit path added in `perry-codegen-js` (passthrough to Node `process.exit`) and `perry-codegen-wasm` (undefined stub — wasm has no `_exit`). Runtime `js_process_exit` was already defined in `perry-runtime/src/process.rs` and calls `_exit(code as i32)`; codegen just wasn't dispatching to it, so `process.exit(0)` fell through to generic NativeMethodCall and silently no-op'd. User-visible effect: `main().then(() => process.exit(0))` at the tail of a net.Socket program now actually terminates the process instead of returning to the event loop, which keeps spinning as long as `js_stdlib_has_active_handles` reports live sockets. 108 runtime tests + 38 perry CLI tests + 49 codegen/hir/wasm/js tests pass; gap suite unchanged.
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
