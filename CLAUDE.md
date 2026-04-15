# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.27

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

For older versions (v0.4.144 and earlier), see CHANGELOG.md.

### v0.5.27 — GC root scanners for `ws` / `http` / `events` / `fastify` closures (refs #35)
- **fix**: follow-up sweep to v0.5.26 — the net.Socket scanner pattern extended to every other stdlib module that stores user closures in Rust-side registries not visible to the GC mark phase. Same latent bug in each: user closure passed across the FFI, stored as `i64` inside a `Mutex<HashMap>` (ws's `WS_CLIENT_LISTENERS`) or inside a struct held by the handle registry (`WsServerHandle.listeners`, `ClientRequestHandle.response_callback` + `.listeners`, `IncomingMessageHandle.listeners`, `EventEmitterHandle.listeners`, `FastifyApp.routes[].handler` + `.hooks.*` + `.error_handler` + `.plugins[].handler`) — any malloc-triggered GC between registration and dispatch would sweep the closure and the next invocation would hit freed memory.
- New helper `common::for_each_handle_of::<T, _>(|t| ...)` walks the `DashMap`-backed handle registry, downcast_ref'ing each entry to `T`. Each stdlib module adds its own `scan_X_roots(mark)` and a `Once`-guarded `ensure_gc_scanner_registered()` called from the module's create / on / connect entry points, mirroring the cron/net templates.
- **ws.rs**: scans `WS_CLIENT_LISTENERS` (global) + every `WsServerHandle` in the registry. Registered from `js_ws_on`, `js_ws_connect`, `js_ws_connect_start`, `js_ws_server_new`.
- **http.rs**: scans every `ClientRequestHandle` (response_callback + 'error' listeners) and `IncomingMessageHandle` ('data' / 'end' / 'error' listeners). Registered from `js_http_request`, `js_https_request`, `js_http_get`, `js_https_get`, `js_http_on`.
- **events.rs**: scans every `EventEmitterHandle`'s listener map. Registered from `js_event_emitter_new` and `js_event_emitter_on`. (Note: `new EventEmitter()` has a pre-existing HIR gap that routes through the user-class `New` path instead of the factory — unrelated to this fix, still happens in v0.5.26.)
- **fastify/mod.rs**: scans every `FastifyApp`'s routes, all 8 hook lists (onRequest/preParsing/preValidation/preHandler/preSerialization/onSend/onResponse/onError), `error_handler`, and plugin handlers. Registered from `js_fastify_create` / `js_fastify_create_with_opts`. Tokio dispatch copies the app into an `Arc` but `Route`/`Hooks` are `Clone` with closures stored by `i64` value — the tokio-side copy references the same `ClosureHeader` alloc, so marking via the registry entry covers both paths.
- **not covered** (intentional, no observed issue): `commander.rs` action callbacks (comment says "not automatically invoked"), `async_local_storage.rs` / `worker_threads.rs` (closures invoked immediately then discarded, never held across a GC boundary).

### v0.5.26 — GC root scanner for `net.Socket` listener closures (closes #35)
- **fix**: `sock.on('data', cb)` stored the closure pointer in `NET_LISTENERS: Mutex<HashMap<i64, HashMap<String, Vec<i64>>>>` as a bare `i64`, with no root scanner registered — so GC's mark phase couldn't see it. Before v0.5.25 this was a latent bug: GC only fired on arena block overflow, and event-driven code (like `@perry/postgres`'s data listener) rarely tripped it. Once v0.5.25 made `gc_malloc` trigger GC, any wrapper-heavy synchronous work (row decode, JSON parse, allocation burst between events) would fire a sweep with the listener unmarked — the sweep freed the closure, and the next dispatched `'data'` event called `js_closure_call1` on freed memory. In the pg driver the result was: iter 0 fired echoes fine (no GC yet), iter 1+ called a dead closure, the driver's parse loop stopped advancing, the outer `conn.query(...)` promise never resolved, and main() silently exited 0 when the pump had nothing left to do — exactly the symptom in the ticket.
- New `scan_net_roots(mark)` walks `NET_LISTENERS`, re-NaN-boxes each callback `i64` with `POINTER_TAG`, and calls `mark` — mirrors the existing `cron::scan_cron_roots` / `timer::scan_timer_roots` pattern. Registered lazily via a `Once` from `spawn_socket_task` (first `net.createConnection` / `tls.connect`) and `js_net_socket_on` (first `.on(...)` call on any socket), so programs that never use net don't pay the registration cost. Repro: synthetic TCP client + external echo server + 30k-iteration wrapper-allocation burst between sends — before: `dataCb=0 bytes=0` (listener freed after iter 0); after: `dataCb=5 bytes=35` ✓.
- **known remaining**: the same latent pattern still exists for `ws.rs`'s `WS_CLIENT_LISTENERS` + `WsServerHandle.listeners`, and `http.rs`'s `ClientRequest.response_callback` + `IncomingMessage.listeners`. Those registries are also Rust-side-only references to user closures — if a WS client or HTTP request lives across a GC cycle triggered by malloc pressure, its listeners will be swept. Filed as a follow-up sweep; not fixed in this commit to keep the scope tight to the issue #35 report.

### v0.5.25 — GC from `gc_malloc` + adaptive malloc-count trigger (closes #34)
- **fix**: malloc-heavy workloads never triggered GC. `gc_check_trigger()` was only called from the arena slow path (when a block fills), but code that produces many short-lived malloc-tracked objects without pushing arena blocks — e.g. `@perry/postgres`'s `parseBigIntDecimal` (`n = n * 10n + digit` creates 2 new bigints per digit via `gc_malloc`) — accumulates indefinitely in `MALLOC_OBJECTS` until the process OOMs or heap corruption trips a malloc-allocator abort. The reported symptom was exit 139 on the second 1000-row × 20-column query or the first 10000-row query. New `gc_check_trigger()` call at the *entry* of `gc_malloc` — critically NOT at the end: running it after the header is pushed into `MALLOC_OBJECTS` would have the sweep free the about-to-be-returned pointer, since the fresh `user_ptr` lives only in a caller-saved register that setjmp's callee-saved-only conservative stack scan can't see. Running before means the allocation simply doesn't exist during any GC cycle this call triggers.
- **fix**: the malloc-count threshold was a hardcoded 10,000 in `gc_check_trigger`. Before this commit that was tolerable because the trigger rarely fired; now that `gc_malloc` calls it every allocation, a program with >10k legitimate live malloc objects (e.g. any backend holding a decent-sized cache) would GC-thrash — every single new alloc would re-trip the threshold. Replaced with a per-thread `GC_NEXT_MALLOC_TRIGGER: Cell<usize>` that's rebaselined after each collection to `survivor_count + GC_MALLOC_COUNT_STEP` (10k). Same update happens on the arena-triggered GC path so both triggers stay in sync.
- Repro synthetic: `parseBigIntDecimal('' + i)` 2M times — before: **8.45 GB peak RSS**; after: **36 MB** (233× reduction; even beats Node's 73 MB since Perry's BigInt is 1024-bit fixed-width vs Node's heap-allocated variable-width).

### v0.5.24 — bigint arithmetic + `BigInt()` coercion (closes #33)
- **fix**: bigint literals were NaN-boxed with `POINTER_TAG` (`0x7FFD`) instead of `BIGINT_TAG` (`0x7FFA`), so `typeof 5n` returned `"object"` and the runtime's `JSValue::is_bigint()` check (used by `js_dynamic_add/sub/mul/div/mod`) said no — arithmetic on bigints fell through to `fadd/fsub/...` on the NaN-tagged bits and produced `NaN`. New `nanbox_bigint_inline` + `BIGINT_TAG_I64` constant; `Expr::BigInt` now uses the bigint tag.
- **feat**: `Expr::BigIntCoerce` was unimplemented (`BigInt(42)`/`BigInt("9223...")` failed to compile with `expression BigIntCoerce not yet supported`). Lowers to `js_bigint_from_f64` (which already dispatches on the NaN tag — pass-through for bigint, i64 conversion for int32, string parse for strings, truncate for doubles) and re-boxes with BIGINT_TAG.
- **feat**: `Expr::Binary` with either operand statically bigint-typed now dispatches to `js_dynamic_add/sub/mul/div/mod` instead of float ops. The runtime helpers unbox, call `js_bigint_<op>`, and re-box. Mixed `bigint × int32` also works (they upcast to bigint). `is_bigint_expr` extended to recognize nested bigint `Binary` ops so `(n * 10n) + d` routes through bigint dispatch all the way up — unblocks the `@perry/postgres` `parseBigIntDecimal` pattern (digit-by-digit accumulator loop).
- **fix**: `js_console_log_dynamic` fell through to the float-number branch for bigint values because `is_bigint()` wasn't in the dispatch chain — `console.log(x)` (single-arg) printed `NaN` for every bigint. Added an `is_bigint()` branch that routes through the existing `format_jsvalue` (which already knows to print `<digits>n`).
- Regression test: `test-files/test_gap_bigint.ts` — matches Node byte-for-byte.

### v0.5.23 — module init order + namespace import dispatch (closes #32)
- **fix**: `non_entry_module_prefixes` in `crates/perry/src/commands/compile.rs` was iterating `ctx.native_modules` (a `BTreeMap<PathBuf, _>`) which produces alphabetical path order, silently discarding the topologically-sorted `non_entry_module_names` built ~700 lines earlier. Any project whose leaf modules sort AFTER their dependents (e.g. `types/registry.ts` > `connection.ts`) had its init sequence reversed — a top-level `registerDefaultCodecs()` call in `register-defaults.ts` would run BEFORE `types/registry.ts`'s init allocated the `REGISTRY_OIDS` array, so every push wrote to a stale (0.0-initialized) global while later readers loaded the correctly-initialized one. Symptom: module-level registries/plugin tables appeared empty to every consumer even though primitives (`let registered = false`) looked shared. Fix: iterate the already-sorted `non_entry_module_names` instead.
- **fix**: `import * as O from './oids'; O.OID_INT2` in `crates/perry-codegen/src/expr.rs` was falling through the PropertyGet handler to the generic `js_object_get_field_by_name_f64(TAG_TRUE, "OID_INT2")` path because the ExternFuncRef-of-namespace case wasn't distinguished from ExternFuncRef-of-variable. The namespace binding `O` has no `perry_fn_<src>__O` getter (it's a namespace, not an exported value), so calling the getter path would link-fail; the codegen fell back to lowering `O` as the TAG_TRUE sentinel and did a field lookup on that, silently returning `undefined` for every namespaced import. Added a PropertyGet fast path: if `object` is `ExternFuncRef { name }` and `name` is in `ctx.namespace_imports`, resolve `property` through `import_function_prefixes` (already populated by the namespace-export walk in compile.rs) and emit a direct `perry_fn_<source_prefix>__<property>()` call. Second half of GH #32 — the registry duplication report was actually two separate bugs stacked together.
- Regression test: `test-files/module-init-order/` (leaf registry + namespace import + top-level registerAll() call + main consumer). Without either fix, `count=0` and all lookups return `MISSING`; with both fixes, `count=3` and lookups resolve correctly.

### v0.5.22 — doc example URLs + compile output noise cleanup (refs #26)
- **docs**: fetch/axios quickstart examples in `docs/src/stdlib/http.md` and `docs/native-libraries.md` swapped from `https://api.example.com/data` (IANA-reserved placeholder that never resolves) to `https://jsonplaceholder.typicode.com/posts/1` (public JSON test API) so copy-paste-and-run works for first-time users. In-widget scaffolding examples left alone — those are snippets inside larger user apps.
- **compile**: `Module init order (0 modules):` (leftover debug aid from a past crash diagnosis) and `auto-optimize: Perry workspace source not found, using prebuilt libperry_runtime.a + libperry_stdlib.a` (fires 100% of the time for Homebrew/apt users since they don't have the workspace) are now gated behind `--verbose`. The rest of the compile output (`Collecting modules...`, `Generating code...`, `Wrote object file`, `Linking (with stdlib)...`, `Wrote executable`, `Binary size`) stays — those are legit progress markers. Threaded `verbose: u8` through `compile::run()` → `build_optimized_libs()` (previously `_verbose`, unused).
- **ci**: `.github/workflows/release-packages.yml` now pins `MACOSX_DEPLOYMENT_TARGET=13.0` for the macOS bottle builds. The `macos-15` runner was stamping `LC_BUILD_VERSION` on every stdlib `.o` with the host's 15.x version, so any user linking on macOS 14 or earlier saw `ld: warning: ... was built for newer 'macOS' version (15.5) than being linked (14.x)` across dozens of object files in libperry_stdlib.a. Functionally harmless, visually ugly. Will take effect on the next release cut — users on existing bottles still see the warnings until then.

### v0.5.21 — fastify header dispatch + gc() safety in servers (closes #30, #31)
- **fix**: `request.header('X')` / `request.headers['X']` returned undefined/null in Fastify handlers because the handler param was typed `any`, so the HIR didn't tag it as `FastifyRequest` → property access fell through to generic object lookup instead of the fastify FFI. New `pre_scan_fastify_handler_params()` in the HIR pre-registers the first two params of `app.get|post|put|delete|patch|head|options|all|addHook|setErrorHandler` arrow handlers as fastify Request/Reply native instances. Also added `NA_JSV` (pass NaN-boxed bits as i64) and `NR_STR` (NaN-box string return with STRING_TAG) arg/return kinds so the receiver methods `js_fastify_req_header(ctx, name: i64)` etc. get the right ABI shape; without this the bitcast was wrong and `JSON.stringify` on the returned string segfaulted.
- **fix**: `gc()` from `setInterval` SEGVd in Fastify+WS servers because the mark-sweep GC only scans the main thread's stack, but tokio worker threads hold live JSValue refs on their stacks that the scanner can't see → GC frees still-referenced objects → next access crashes. Added `GC_UNSAFE_ZONES` atomic in perry-runtime; Fastify/WS server creation increments it, WS server close decrements it. `js_gc_collect()` now checks the counter and skips collection (with a one-shot warning) when any tokio-based server is active. Full stop-the-world GC synchronization is a v0.5.22 followup.

### v0.5.20 — String.length returns UTF-16 code units (closes #18 partially)
- **fix**: `String.length` now returns UTF-16 code unit count instead of UTF-8 byte count, matching JavaScript semantics. `"café".length` → 4 (was 5), `"日本語".length` → 3 (was 9), `"😀".length` → 2 (was 4). `StringHeader` gains `utf16_len` at offset 0 (codegen inline `.length` unchanged) + `byte_len` for internal ops. All position-based APIs (`charAt`, `slice`, `substring`, `indexOf`, `lastIndexOf`, `padStart`, `padEnd`, `toCharArray`) converted to UTF-16 indexing with ASCII fast path. `test_gap_string_methods` DIFF (4) → DIFF (2, lone surrogates only). Fixes NFC/NFD `.normalize().length` parity.

### v0.5.19 — fix Fastify/MySQL segfault on Linux, restore native module dispatch, fix gc() (closes #28)
- **fix**: `gc()` calls emitted bare `gc` symbol instead of `js_gc_collect` — caused `undefined reference to 'gc'` linker error (macOS) or segfault at runtime (Linux with `--warn-unresolved-symbols`). Added explicit dispatch in `lower_call.rs` ExternFuncRef handler.
- **fix**: Fastify/MySQL/WS/pg/ioredis/MongoDB/better-sqlite3 binaries compiled but did nothing at runtime — the entire native module dispatch table from the old Cranelift codegen was lost in the v0.5.0 LLVM cutover. All `NativeMethodCall` nodes for these modules fell through to the catch-all that returns `double 0.0`, so no runtime functions were ever called. Added `NATIVE_MODULE_TABLE` with table-driven dispatch for ~100 methods across 15+ native modules.
- **fix**: removed `--warn-unresolved-symbols` from Linux linker flags — this flag silently converted link errors to warnings, producing binaries with null function pointers that segfaulted at runtime instead of failing at link time.
- **fix**: MySQL `pool.query()`/`pool.execute()` routed to `js_mysql2_connection_*` instead of `js_mysql2_pool_*` — caused "Invalid connection handle" errors. Added `class_filter` to `NativeModSig` so `class_name: "Pool"` dispatches to pool-specific runtime functions; `"PoolConnection"` dispatches to pool-connection functions. HIR `class_name` now threaded through to `lower_native_method_call`.
- **fix**: `new WebSocketServer({port: N})` went through the empty-object placeholder in `lower_builtin_new` instead of calling `js_ws_server_new`. Added dedicated `WebSocketServer` case. Fixed `js_ws_send` arg type (was NA_F64, now NA_STR matching the `(i64, i64)` runtime signature).

### v0.5.18 — native axios, fetch segfault fix, type stubs (closes #24, #25, #26, #27)
- **feat**: native `axios` dispatch — `axios.get/post/put/delete/patch` and `response.status/.data/.statusText` now compile natively without `--enable-js-runtime` or npm install. Added to `NATIVE_MODULES`, HIR native instance tracking, codegen dispatch, and `http-client` feature mapping.
- **fix**: `await fetch(url)` segfaulted because `body` (undefined for GET) NaN-unboxed to `0x1`, dereferenced as a valid pointer. Fixed `string_from_header` to treat pointers below page size as invalid.
- **fix**: await loop never drained stdlib async queue — added `js_run_stdlib_pump()` call so tokio-based fetch/DB results actually resolve.
- **fix**: `llvm-ar not found` warning downgraded from `ERROR` to soft skip with install instructions (non-fatal, strip-dedup is optional).
- **feat**: `.d.ts` type stubs for `perry/ui`, `perry/thread`, `perry/i18n`, `perry/system`. `perry init` generates `tsconfig.json` with paths; new `perry types` command for existing projects.

### v0.5.17 (llvm-backend) — scalar replacement of non-escaping objects + Static Hermes benchmarks
- **perf**: escape analysis identifies `let p = new Point(x, y)` where `p` never escapes (only PropertyGet/PropertySet uses); fields are decomposed into stack allocas that LLVM promotes to registers — zero heap allocation. `object_create` 10ms→4ms (2.5x), `binary_trees` 9ms→3ms (3x), peak RSS 97MB→5MB. Perry now beats Node.js on all 15 benchmarks.
- **feat**: benchmark suite (`benchmarks/suite/run_benchmarks.sh`) now includes Static Hermes (Meta's AOT JS compiler) as a 4th comparison target alongside Node.js and Bun, with automatic TS→JS type-stripping. Updated README with full 4-way comparison tables and refreshed polyglot numbers.

### v0.5.16 (llvm-backend) — watchOS device target: arm64_32 instead of arm64
- **fix**: `--target watchos` emitted `aarch64-apple-watchos` (regular 64-bit ARM) objects, but Apple Watch hardware requires `arm64_32` (ILP32 — 32-bit pointers on 64-bit ARM). Changed LLVM triple to `arm64_32-apple-watchos`, Rust target to `arm64_32-apple-watchos`, and link triple to `arm64_32-apple-watchos10.0`. The simulator target (`watchos-simulator`) is unchanged — it correctly uses host-native aarch64. This fixes the ABI incompatibility that prevented device builds from linking with the LLVM-based runtime.

### v0.5.15 (llvm-backend) — perry/ui State dispatch + check-deps fix (closes #24, #25)
- **fix**: `State(0)` constructor and `.value`/`.set()` instance methods were missing from the LLVM codegen dispatch tables, producing "not in dispatch table" warnings and silently returning `undefined`. Added `State` → `perry_ui_state_create` to `PERRY_UI_TABLE` and `value` → `perry_ui_state_get` / `set` → `perry_ui_state_set` to `PERRY_UI_INSTANCE_TABLE`.
- **fix**: `perry check --check-deps` flagged `perry/ui`, `perry/thread`, `perry/i18n` as missing npm packages (R003) and as unsupported Node.js built-ins (U006). New `is_perry_builtin()` guard skips resolution and diagnostics for all `perry/*` imports.

### v0.5.14 (llvm-backend) — Windows build fix: date.rs POSIX-only APIs
- **fix**: `timestamp_to_local_components` used `libc::localtime_r` and `tm_gmtoff`, both POSIX-only — broke the Windows CI build. Split into `#[cfg(unix)]` (keeps `localtime_r` + `tm_gmtoff`) and `#[cfg(windows)]` (uses `libc::localtime_s` / `libc::gmtime_s`, derives tz offset by comparing local vs UTC breakdowns).

### v0.5.13 (llvm-backend) — Buffer.indexOf/includes dispatch fix
- **fix**: `Buffer.indexOf()` and `Buffer.includes()` were incorrectly routed through the string method path in codegen, because the `is_string_only_method` guard didn't exclude `Uint8Array`/`Buffer` types. Added a `static_type_of` check that skips the string dispatch when the receiver is typed as `Uint8Array` or `Buffer`, letting these methods fall through to `dispatch_buffer_method` via `js_native_call_method` as intended.
- **cleanup**: removed leftover debug `eprintln!` in `js_buffer_index_of`.

### v0.5.12 (llvm-backend) — perry/ui widget dispatch — mango renders its full UI
- **feat**: follow-up to v0.5.10 which landed only `App({...})`. This commit adds the rest of the perry/ui surface to `lower_native_method_call` via a table-driven dispatcher (`PERRY_UI_TABLE` of `UiSig { method, runtime, args, ret }` entries using `UiArgKind::{Widget,Str,F64,Closure,I64Raw}` / `UiReturnKind::{Widget,F64,Void}`). ~40 widget methods covered in one pass: `Text` / `TextField` / `TextArea` / `Spacer` / `Divider` / `ScrollView` constructors; `menuCreate` / `menuAddItem` / `menuBarCreate` / `menuBarAttach` / `menuBarAddMenu`; text setters (`textSetFontSize` / `textSetColor` / `textSetString` / `textSetFontFamily` / `textSetFontWeight` / `textSetWraps`); button setters (`buttonSetBordered` / `buttonSetTextColor` / `buttonSetTitle`); widget mutators (`widgetAddChild` / `widgetClearChildren` / `widgetSetHidden` / `widgetSetWidth` / `widgetSetHeight` / `widgetSetHugging` / `widgetMatchParentWidth` / `widgetMatchParentHeight` / `widgetSetBackgroundColor` / `widgetSetBackgroundGradient` / `setCornerRadius`); stack mutators (`stackSetAlignment` / `stackSetDistribution`); `scrollviewSetChild`; `textfieldSetString` / `textareaSetString`. Runtime fns lazy-declared via `ctx.pending_declares`.
- **feat**: `VStack` / `HStack` get a dedicated special case because the TS call shape (`VStack(spacing, [children])` or `VStack([children])`) doesn't fit the table — spacing is optional and children is a variadic array that needs one `perry_ui_widget_add_child` call per element. We stash the parent handle in an entry alloca so subsequent blocks reload it, then walk the array fast path.
- **feat**: `Button` also gets a special case because the handler closure arg must stay NaN-boxed (f64), not unboxed to i64, and the label is a raw cstr pointer — neither shape is expressible as a single `UiArgKind` row.
- **fix**: one naming inconsistency found while building the table — the runtime fn is `perry_ui_set_widget_hidden` (with `set` first, unlike every other `widget_*` setter). Fixed in the table.
- **result**: `mango/src/app.ts -o Mango` now launches and renders the full UI tree — title bar, "Welcome to Mango" heading, "MongoDB Study Tool" subtitle, "Databases & Collections / Query & Plan / Edit & Insert / Index Viewer" menu items, and the orange "+ New Connection" button all visible in the screenshot. Verified by launching the compiled binary, positioning the window onscreen via osascript, and `/usr/sbin/screencapture`. The v0.5.0 LLVM cutover regression (mango compiled clean but exited silently with an empty window) is fully resolved.

### v0.5.11 (llvm-backend) — inline-allocator regression fixes (parity 80% → 94%)
- **fix**: the inline bump-allocator hoist (v0.5.0-followup) cached `@perry_class_keys_<class>` in a function-entry stack slot, but the entry-block hoist ran BEFORE `__perry_init_strings_*` (which is what populates the global). So freshly-allocated objects had a null `keys_array` and `js_object_get_field_by_name` returned `undefined` for every field — `test_array_of_objects` showed `sorted[0].name → undefined`. New `LlFunction::entry_init_boundary` + `entry_post_init_setup`: alloca stays at the very top (dominates), but the load+store splices in AFTER the init prelude. `mark_entry_init_boundary()` is called immediately after `js_gc_init` / `__perry_init_strings_*` / non-entry module inits in `compile_module_entry`.
- **fix**: the inline allocator skipped `register_class(child, parent)` (the runtime allocators do it on every alloc). With every class instance going through the inline path, the CLASS_REGISTRY was never populated and `instanceof` walks broke at the first hop — `test_edge_classes` showed `square instanceof Rectangle → false` for a `class Square extends Rectangle extends Shape`. New public `js_register_class_parent(child, parent)` extern; codegen emits one call per inheriting class in `__perry_init_strings_*` (sorted by class id).
- **infra**: parity script normalize_output now strips Node v25 `MODULE_TYPELESS_PACKAGE_JSON` warnings (4 lines printed to stderr per test file without `"type": "module"` in package.json — pure environmental noise that started after the Node v25 upgrade).
- **result**: parity sweep 96 PASS / 6 FAIL / 0 COMPILE_FAIL = **94.1%**, beating the v0.5.0 baseline of 91.8%. Remaining 6 DIFFs are all pre-existing (timer precision, lookbehind regex, lone surrogates, NFC/NFD, async-generator baseline) — verified by reproducing on the pre-optimization commit. Numeric benchmarks (object_create 8ms, binary_trees 7ms, factorial 25ms) still beat or tie Node on every workload — the fix didn't regress any of the v0.5.2 wins.

### v0.5.10 (llvm-backend) — `perry/ui.App({...})` dispatch — mango actually launches
- **fix**: the LLVM backend port (v0.5.0 cutover) silently dropped `perry/ui` dispatch — receiver-less `NativeMethodCall { module: "perry/ui", method, object: None }` fell into `lower_native_method_call`'s catch-all early-out at `lower_call.rs:1922` and returned `double 0.0`. So `App({title, width, height, body})` at the end of any perry/ui app silently no-op'd, the binary completed init without entering `NSApplication.run()`, and exited with no output. Mango compiled cleanly under v0.5.0 through v0.5.9 but couldn't actually launch — the regression was masked because the driver doesn't have an integration test that runs the resulting binary. New per-method dispatch in `lower_call.rs::lower_native_method_call` that recognizes `perry/ui.App({...})`, walks the args[0] object literal for `title` / `width` / `height` / `icon` / `body`, lazy-declares `perry_ui_app_create` / `perry_ui_app_set_icon` / `perry_ui_app_set_body` / `perry_ui_app_run` via `pending_declares`, and emits the create/set-icon/set-body/run sequence. Verified by compiling `mango/src/app.ts -o Mango`, launching the binary, and screenshotting a native macOS window titled "Mango" (menubar shows Mango/Edit/Window — proof that NSApplication.run() is now being entered). The window's content area is empty because the other perry/ui constructors (Text/Button/VStack/HStack/etc.) are still in the same dropped state — full widget dispatch is the next followup. This commit lands `App()` only as a focused proof-of-concept that the linking + runtime + Mach-O code path works end to end.

### v0.5.9 (llvm-backend) — `let C = SomeClass; new C()` correctness + alias type refinement
- **fix**: `let C = SomeClass; new C()` now actually creates an instance of `SomeClass` instead of returning the empty-object placeholder. New `local_class_aliases: HashMap<String, String>` and `local_id_to_name: HashMap<u32, String>` fields on `FnCtx`, populated by `Stmt::Let` when the init is `Expr::ClassRef(name)` (direct alias) or `Expr::LocalGet(other_id)` where `other_id`'s name is itself an alias (chain — `let A = X; let B = A; new B()`). `lower_new` shadows its `class_name` parameter with the resolved name early so the rest of the function (alloc + ctor inline + field offsets) uses the real class. Critically, `refine_type_from_init` for `Expr::New` *also* resolves through `local_class_aliases`, so `let b: any = new C()` refines `b`'s static type to `Named("SomeClass")` not `Named("C")` — without this, the PropertyGet fast path would look up "C" in `ctx.classes`, find nothing, fall through to `js_object_get_field_by_name_f64`, and return undefined for fields that were correctly initialized in memory by the inline allocator. Verified with three test shapes: direct alias (`const C = Foo; const a = new C()`), 3-step chain (`const A = Bar; const B = A; const b = new B()`), and in-function (`function f() { const D = Foo; return new D() }`). Mango compiles cleanly.

### v0.5.8 (llvm-backend) — `Expr::NewDynamic` static reroute + conditional callee branching
- **fix**: workspace `Cargo.toml` was missing `[profile.release.package]` `strip = false` overrides for `perry-ui-ios`, `perry-ui-tvos`, `perry-ui-android`, `perry-ui-watchos`. Same staticlib+`#[no_mangle] extern "C"` FFI contract as `perry-ui-macos` (which already had the override + the explicit "UI crates must NOT strip — they export `#[no_mangle] extern "C"` symbols" comment), so a release build of those four would have silently stripped their `perry_ui_*` symbols and broken linking user binaries on `--target ios-simulator`/`ios`/`tvos-simulator`/`tvos`/android. Hadn't bitten yet because all four are in `members` but not `default-members` — a plain `cargo build --release` skips them. Added the four missing profile blocks (`strip = false`, `codegen-units = 16`) alongside the existing macOS/gtk4/windows/geisterhand ones. No code changes, no version bump.
- **fix**: `new (Foo)()` (parenthesized ClassRef) and `new (cond ? FooClass : BarClass)()` (conditional callee) now dispatch to the right class instead of returning the empty-object placeholder. Two new shapes recognized in the `Expr::NewDynamic` lowering: (a) `Expr::ClassRef(name)` callees reroute straight to `lower_new(name, args)`, mirroring the existing `globalThis.X` reroute; (b) `Expr::Conditional { condition, then_expr, else_expr }` callees synthesize a `NewDynamic { callee: <branch>, args }` per branch and emit a runtime cond_br + phi via the existing `lower_conditional` helper, so each branch independently runs `lower_new` (or recursively the NewDynamic fallback). Nested ternaries work because the inner NewDynamic recurses through the same handler. New `try_static_class_name(callee)` helper centralizes the static-reroute pattern. The truly-dynamic fallback (`new someVar()` where the callee is a runtime value) still emits an empty-object placeholder — that needs a `js_new_dynamic(callee_value, args)` runtime helper to inspect the value's NaN tag and dispatch to the right class constructor, tracked as a v0.5.8 followup. Verified end-to-end with two TS tests: `new (cond ? Foo : Bar)()` (5 cases including a nested ternary) and `new (Foo)()` + `new arr[0]()` (placeholder fallback). Mango compiles cleanly.

### v0.5.7 (llvm-backend) — `Expr::I18nString` compile-time resolution + runtime interpolation
- **fix**: localized strings now resolve to the right translation at compile time. Previously the `Expr::I18nString` lowering returned the verbatim KEY string regardless of the project's `default_locale`, so any user calling `t("Hello")` from `perry/i18n` got `"Hello"` instead of `"Hallo"` even with `default_locale = "de"`. New `expr::I18nLowerCtx` (threaded through `CrossModuleCtx`) carries the i18n table from `opts.i18n_table` and the default locale index. The lowering pulls `translations[default_locale_idx * key_count + string_idx]` at compile time, parses `{name}` placeholders, lowers each interpolation param's value, and emits a `js_string_concat` chain that interleaves interned literal fragments with `js_string_coerce`'d param values. Empty / missing translation cells fall back to the source key. Plurals (`plural_forms`/`plural_param`) are still ignored — uses the canonical `string_idx` form, leaving CLDR plural rule selection as a followup. Also fixed: `lower_call.rs::lower_native_method_call` was discarding `NativeMethodCall { module: "perry/i18n", method: "t", object: None, args: [I18nString] }` and returning `double 0.0` because the receiver-less early-out path didn't know about `t()`. Now special-cases the `t()` unwrap and lowers the inner I18nString directly. Added `default_locale_idx` to `CompileOptions::i18n_table` (5-tuple). Verified end-to-end with a 2-locale test: en/de translations resolve correctly when `default_locale` is switched, and missing/empty cells fall back to the source key. Mango still compiles cleanly (89 localizable strings across 13 locales).

### v0.5.6 (llvm-backend) — perry-stdlib auto-optimize `hex` crate fix
- **fix**: `crates/perry-stdlib/src/sqlite.rs:54` was using `hex::encode(b)` to format SQLite `Blob` columns as hex strings, but the `hex` crate dep in `perry-stdlib`'s `Cargo.toml` is gated behind the `crypto` Cargo feature. Auto-optimize rebuilds that enabled only `database-sqlite` (e.g. mango: `better-sqlite3` + `mongodb` + fetch, no crypto) failed with `error[E0433]: failed to resolve: use of unresolved module or unlinked crate hex` and fell back to the prebuilt full stdlib, leaving every user binary 100KB+ larger than necessary. Replaced with a hand-rolled nibble loop (`const HEX: &[u8; 16] = b"0123456789abcdef"; for &byte in b { out.push(HEX[(byte >> 4) as usize]); out.push(HEX[(byte & 0x0f) as usize]); }`) so sqlite no longer depends on hex. Surgical fix — no Cargo.toml or auto-optimize logic changes. Mango now goes through the auto-optimize rebuild path: prebuilt-fallback 5.18 MB → optimized 5.01 MB (~168 KB / 3.4% savings, mostly from features the user doesn't import being stripped). Original fix done as a worktree-isolated subagent task; the agent's commit was based on a stale `llvm-backend` HEAD so the sqlite.rs change was applied manually here on top of v0.5.5.

### v0.5.5 (llvm-backend) — `alloca_entry` sweep
- **fix**: 7 cross-block alloca sites in `expr.rs` / `lower_call.rs` / `stmt.rs` migrated to `LlFunction.alloca_entry()` to close the latent SSA dominance hazards flagged in v0.5.2's followup list. Migrated: catch-clause exception binding (capturable by nested closures in the catch body), `super()`-inlined parent ctor params (capturable by closures inside the parent ctor body), `forEach` loop counter (spans cond/body/exit successor blocks), `Await` result slot (spans check/wait/settled/done/merge blocks; can be lowered inside a nested if-arm), `NewClass` `this_slot` (pushed on `this_stack` for the entire inlined ctor body with nested closures capturing `this`), and the inlined-ctor param slots in two places. Left alone with comment: `js_array_splice out_slot` (single-block scratch, dominance-safe by construction). Mango compiles + links cleanly. Original sweep done as a worktree-isolated subagent task because main was being concurrently edited; cherry-picked back here.

### v0.5.4 (llvm-backend) — `Expr::ExternFuncRef`-as-value via static `ClosureHeader` thunks
- **fix**: imported functions can now be passed as callbacks, stored in variables, and called indirectly. Previously `Expr::ExternFuncRef` lowered as a value returned a `TAG_TRUE` sentinel that worked for `if (importedFn)` truthiness checks but crashed at runtime the moment anything tried to dispatch through `js_closure_callN`. The fix mirrors the existing `__perry_wrap_<name>` machinery for local funcs (`crates/perry-codegen/src/codegen.rs:870-904`): for every entry in `opts.import_function_prefixes`, `compile_module` now emits a thin `__perry_wrap_extern_<src>__<name>` wrapper (`internal` linkage so per-module copies don't collide at link time) plus a static `ClosureHeader` constant `__perry_extern_closure_<src>__<name>` whose `func_ptr` points at the wrapper and `type_tag = CLOSURE_MAGIC`. The expr.rs lowering returns `ptrtoint @<global> to i64` NaN-boxed as POINTER. New `LlModule.add_internal_constant()` helper. Verified end-to-end with a TS test that uses `arr.map(double)`, `if (double)`, `f === g`, and `fn(3, 4)` indirect call — all four cases produce correct output (was `[undef, undef, ...]` and `undefined` before). Mango unaffected (entry path uses truthiness only).

### v0.5.3 (llvm-backend) — driver hard-fails on entry-module codegen errors
- **fix**: `crates/perry/src/commands/compile.rs` now refuses to link when the entry module is in `failed_modules`. The original 0.5.0 mango bug was a misdiagnosis chain: 13 modules (including `mango/src/app.ts`) failed codegen, the driver silently replaced each with an empty `_perry_init_*` stub, and the link step exploded with `Undefined symbols for architecture arm64: "_main"` — a downstream symptom that took manual digging to trace back to the real codegen errors hidden in cargo build noise. The driver now (a) prints a loud box-drawn failure summary right after the parallel compile loop, *before* `build_optimized_libs` floods stdout, (b) marks the entry module with `(entry)` in the failure list, and (c) returns `Err` immediately if the entry module is in the list, with a message explaining why. Non-entry failures keep the previous "stub the init, continue linking" behavior but get the same loud summary so the codegen errors aren't drowned in the cargo noise. `use_color` (was `_use_color`) is now wired through to ANSI red on the headers.

### v0.5.2 (llvm-backend) — crushing the numeric benchmarks
- **perf**: `fadd/fsub/fmul/fdiv/frem/fneg` IR builder now emits `reassoc contract` fast-math flags. Clang's `-ffast-math` does NOT retroactively apply to ops in a `.ll` input — the FMFs must be on each instruction. Adding `reassoc contract` lets LLVM break serial accumulator chains into parallel accumulators + 8x-unroll + NEON 2-wide vectorize. **`loop_overhead` 99ms → 13ms (4.1x faster than Node 54ms); `math_intensive` 50ms → 14ms (3.3x faster than Node)**.
- **perf**: Integer-modulo fast path in `BinaryOp::Mod` when both operands are provably integer-valued. New `crate::collectors::collect_integer_locals` walker tracks locals that start from an `Integer` literal and are only ever mutated via `Update` (++/--, no `LocalSet`). Mod-by-integer on such values emits `fptosi → srem → sitofp` instead of `frem double`, which lowers to a libm `fmod()` call on ARM (no hardware instruction). LLVM's SCEV then replaces the div with a reciprocal-multiplication `msub` and hoists the conversions. **`factorial` (sum += i % 1000) 1553ms → 24ms — 64x faster, 25x faster than Node 603ms**.
- Perry now beats Node on 8/11 numeric benchmarks (loop_overhead, math_intensive, factorial, closure, mandelbrot, matrix_multiply, array_read, nested_loops); ties on 2; loses on object_create/binary_trees only (blocked on inline bump-allocator, a pending refactor).

### v0.5.1 (llvm-backend) — mango compile sweep
- feat: 13 LLVM-backend gap fixes that let `mango` compile end-to-end with 0.5.0 (was hitting 13 module-level codegen errors that the driver silently turned into empty `_perry_init_*` stubs, leaving the link with no `_main`). Fixed: `Array.slice()` 0-arg, variadic `arr.push(a,b,c,…)`, `Expr::ArraySome`/`ArrayEvery`/`NewDynamic`/`FetchWithOptions`/`I18nString`/`ExternFuncRef`-as-value, `js_closure_call6..16` (was capped at 5). Killed the buggy cross-module pre-walker (`collect_extern_func_refs_in_*`) and replaced it with **lazy declares** via `FnCtx.pending_declares`, drained after each compile pass — fixes `use of undefined value @perry_fn_*` from cross-module calls inside closures, try/switch, and array callbacks. Closure pre-walker now also walks getters/setters/static_methods (was only methods+ctor) and recurses through ArraySome/Every/NewDynamic/FetchWithOptions/I18nString/Yield. New `LlFunction.alloca_entry()` hoists `Stmt::Let` slots to the entry block — fixes pre-existing SSA dominance verifier failure when a `let` declared inside an `if` arm is captured by a closure in a sibling branch. Mango binary: 4.9MB, links clean.

### v0.5.0 — Phase K hard cutover (LLVM-only)
- **Cranelift backend deleted.** `crates/perry-codegen-llvm/` renamed to `crates/perry-codegen/` as the only codegen path. `--backend` CLI flag removed; all `cranelift*` workspace deps dropped. Parity sweep identical pre/post: **102 MATCH / 9 DIFF / 0 CRASH / 91.8%**. Remaining DIFFs are 8 nondeterministic (timing/RNG/UUID) + async-generator baseline + long-tail features (lookbehind regex, UTF-8/UTF-16 length gap, lone surrogates).

### v0.4.146-followup-2 (llvm-backend)
- feat: `test_gap_array_methods` DIFF (3) → **MATCH**. Four coordinated fixes: 16-pass microtask drain in `main()` so top-level `.then(cb)` fires; `is_promise_expr` recognizes async-FuncRef calls via new `local_async_funcs` HashSet; nested `async function*` declarations hoist to top-level so generator transform sees them; `scan_expr_for_max_local`/`_max_func` in `perry-transform/generator.rs` now walk all array fast-path variants (ArrayMap/Filter/etc.) to prevent LocalId/FuncId collisions.

### v0.4.146-followup (llvm-backend)
- feat: **`Object.groupBy`**, **`Array.fromAsync`**, optional-chain array fast path (`obj?.map(...)` folds through array dispatch), `typeof Object.<method>` → `"function"` constant fold. `test_gap_array_methods` DIFF (7) → DIFF (3).

### v0.4.148 (llvm-backend)
- feat: `test_gap_node_crypto_buffer` DIFF (54) → **MATCH**. Full Node-style Buffer/crypto surface: new `dispatch_buffer_method` in `object.rs` routes `js_native_call_method` for any registered buffer (read/write numeric family, `swap*`, `indexOf`/`includes`, `slice`/`fill`/`compare`/`toString(enc)`); `crypto.getRandomValues`, `Buffer.compare/from/alloc/concat` wired; `Buffer.from([arr])` path decodes via `js_buffer_from_value`; type inference refines `Buffer.from`/`crypto.randomBytes` to `Named("Uint8Array")`; crypto `createHash(...).update(...).digest(enc)` chain detected as string; `bigint_value_to_i64` accepts POINTER_TAG-boxed BigInt pointers.

### v0.4.147 (llvm-backend)
- feat: `test_gap_symbols` DIFF (4) → **MATCH**. `Symbol.hasInstance` and `Symbol.toStringTag` via HIR class lowering of well-known keys (lifts to `__perry_wk_hasinstance_*`/`__perry_wk_tostringtag_*`), new `CLASS_HAS_INSTANCE_REGISTRY`/`CLASS_TO_STRING_TAG_REGISTRY` in runtime, and `Object.prototype.toString.call(x)` → `js_object_to_string` dispatch in HIR.

### v0.4.146 (llvm-backend)
- feat: `Symbol.toPrimitive` semantic support — `+currency` / `` `${currency}` `` / `currency + 0` all consult `obj[Symbol.toPrimitive]` via new `js_to_primitive(v, hint)` hook threaded through `js_number_coerce` and `js_jsvalue_to_string`. Well-known symbol cache in `symbol.rs`; computed-key method lowering via new `PostInit::SetMethodWithThis` variant. `test_gap_symbols` DIFF (10) → DIFF (4).

### v0.4.145 (llvm-backend)
- feat: real **TypedArray** support (Int8/Int16/Int32, Uint16/Uint32, Float32/Float64). New `typedarray.rs` with `TYPED_ARRAY_REGISTRY`; generic array helpers (`js_array_at`, `js_array_to_sorted`, `js_array_with`, `js_array_find_last`, etc.) detect typed-array pointers and dispatch per-kind, preserving `Int32Array(N) [ ... ]` Node format on round-trip. Reserved class IDs `0xFFFF0030..0037` for `instanceof`. `test_gap_array_methods` DIFF (35) → DIFF (7).

