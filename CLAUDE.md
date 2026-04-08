# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and Cranelift for code generation.

**Current Version:** 0.4.80

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

### v0.4.80
- fix: `node-cron`'s `cron.schedule(expr, cb)` callback is now actually invoked. The previous implementation in `crates/perry-stdlib/src/cron.rs` spawned a tokio task that computed the next deadline and slept until it but had a TODO where the callback should fire (`// In a real implementation, we'd invoke js_callback_invoke(callback_id)`), so every scheduled job silently never ran. Two interlocking problems: (a) the callback was passed as `f64` (NaN-boxed), then truncated via `as u64` which produces 0 for a NaN bit pattern, so the closure pointer was always lost; (b) even with a correct pointer, calling `js_closure_call0` from a tokio worker thread would race the GC's conservative stack scanner and the per-thread arena. Fix mirrors the `INTERVAL_TIMERS` pattern in `perry-runtime/src/timer.rs`: new `CronTimer { id, schedule, callback: i64, next_deadline: Instant, running: Arc<AtomicBool>, cleared }` lives in a global `Mutex<Vec<CronTimer>>`, deadlines re-computed from the cron `Schedule` after each fire. Cron callbacks fire **on the main thread** from the CLI event loop in `module_init.rs`, which now also pumps `js_cron_timer_tick` / `js_cron_timer_has_pending` alongside the interval/callback ticks. `js_cron_schedule` signature changed from `(*const StringHeader, f64)` to `(*const StringHeader, i64)` so the closure pointer survives the call boundary; matching codegen branch in `expr.rs`'s `node-cron` static-method dispatch extracts the string pointer for the cron expression and passes the closure as raw `i64`. Cron callback closures registered as GC roots via lazy `gc_register_root_scanner(scan_cron_roots)` on first schedule (matches `timer.rs`'s `scan_timer_roots` pattern), so the closure can't be freed between ticks. `cron.schedule(...)` returns a job handle now properly tracked as a "CronJob" native instance via new mappings in both `lower.rs:lower_var_decl` (for `export const job = ...`) and `destructuring.rs:lower_var_decl_with_destructuring` (for plain `const job = ...`), so `job.stop()` / `job.start()` / `job.isRunning()` resolve to `js_cron_job_*` instead of falling through to dynamic dispatch. `job.stop()` now removes the timer entry from `CRON_TIMERS` so the event loop exits cleanly when no other timers remain. New `node-cron` entry added to `NATIVE_MODULES` in `perry-hir/src/ir.rs` so the auto-optimize feature detection in `compile.rs` correctly enables the `scheduler` Cargo feature when a project imports `node-cron`. Verified end-to-end with three tests: `cron.schedule("* * * * * *", cb)` fires every second; `job.stop()` after N ticks correctly halts further callbacks; cron + setInterval coexist in the same process. Reported by gscmaster-api downstream user.

### v0.4.79
- feat: RegExp lowering — `regex.exec(str)` → `Expr::RegExpExec`; `regex.source/.flags/.lastIndex` reads → `RegExpSource/Flags/LastIndex`; `regex.lastIndex = N` → `RegExpSetLastIndex`; `m.index/.groups` (where `m` was assigned from `regex.exec(...)`) → bare `RegExpExecIndex/Groups` reading runtime thread-locals. New `regex_exec_locals: HashSet<String>` tracker on `LoweringContext`, populated from `is_regex_exec_init()` (which strips `TsNonNull` wrappers). Both `replace`/`replaceAll` codegen sites in `expr.rs` now: (a) route `str.replace(regex, fn)` → `js_string_replace_regex_fn` callback path, and (b) use `js_string_replace_regex_named` for the string-replacement path so `$<name>` back-refs work (falls back to plain replace when no named refs are present). Fixed latent ARM64 ABI bug: `js_string_replace_regex_fn`'s callback param was declared as `I64` but the Rust function takes `f64` — on ARM64 this put a NaN-boxed closure in a GPR instead of an FPR, garbling the dispatch.
- fix: `js_instanceof` Error subclass handling restored — checks `GC_TYPE_ERROR` headers via `error_kind` against `CLASS_ID_TYPE_ERROR/RANGE_ERROR/REFERENCE_ERROR/SYNTAX_ERROR/AGGREGATE_ERROR`, and recognizes user classes that extend `Error` via the `extends_builtin_error` registry. Was lost when the Object.defineProperty agent (e584a16) overwrote object.rs from an older base. `test_gap_error_extensions` flipped from 24 diffs back to PASS.
- result: `test_gap_regexp_advanced` down to **2 diffs** (the only remaining is the unsupported `(?<=\$)\d+` lookbehind — Rust `regex` crate limitation, not codegen).

### v0.4.78
- feat: `TextEncoder`/`TextDecoder`, `encodeURI`/`decodeURI`/`encodeURIComponent`/`decodeURIComponent`, `structuredClone`, `queueMicrotask` -- new HIR variants and runtime functions for encoding APIs; `new TextEncoder().encode(str)` returns a Buffer (Uint8Array), `new TextDecoder().decode(buf)` returns a string, `.encoding` property returns `"utf-8"`. URI encoding follows RFC 2396 (encodeURI preserves reserved chars, encodeURIComponent encodes them). Timer IDs from `setTimeout`/`setInterval` now NaN-boxed with POINTER_TAG so `typeof` returns `"object"` and `clearTimeout`/`clearInterval` correctly recover the ID (previously small integer IDs were zeroed by `ensure_i64`'s small-value guard). `test_gap_encoding_timers.ts` down from 54 to 4 diff lines vs Node (remaining diff is pre-existing `charCodeAt` UTF-8 byte-level issue).

### v0.4.68
- feat: `console.time` / `timeEnd` / `timeLog` / `count` / `countReset` / `group` / `groupEnd` / `groupCollapsed` / `assert` / `dir` / `clear` — new runtime functions in `builtins.rs` backed by two thread-locals (`CONSOLE_TIMERS: HashMap<String, Instant>` and `CONSOLE_COUNTERS: HashMap<String, u64>`). Codegen dispatch added at the property-method site in `expr.rs` next to the existing `console.log` branch. Group methods print the label without indentation tracking yet (a follow-up could add the indent counter once ALL `js_console_log*` paths are taught to read it). `console.dir` is treated as an alias for `console.log` of the first argument. `console.clear` writes the ANSI clear sequence.

### v0.4.67
- feat: auto-detect optimal build profile — `perry compile` now inspects the project's imports and rebuilds perry-runtime + perry-stdlib in one cargo invocation with the smallest matching Cargo feature set (mongodb-only, http-client-only, etc.) AND switches `panic = "unwind"` → `panic = "abort"` whenever no `catch_unwind` callers are reachable (no `perry/ui`, `perry/thread`, `perry/plugin`, geisterhand). The chosen profile lives in a hash-keyed `target/perry-auto-{hash}/` directory so cargo's incremental cache works per (features, panic, target) tuple. New `CompilationContext.needs_thread` field tracks `perry/thread` imports. New `OptimizedLibs` struct returns both runtime + stdlib paths so the symbol-stub scan and the linker see the same artifacts. Falls back to the prebuilt full stdlib + unwind runtime when the workspace source isn't on disk or cargo isn't on PATH — never breaks a user's compile. Measured fully automatic (no flags): `await fetch(url)` 4.2 MB → **2.9 MB (-31%)**, mongodb 3.1 MB → **2.4 MB**, hello-world 0.5 MB → 0.4 MB, `perry/thread` programs correctly stay panic=unwind. The legacy `--minimal-stdlib` flag is now a hidden no-op alias; new `--no-auto-optimize` escape hatch falls back to the prebuilt libraries.

### v0.4.66
- feat: `path.relative` / `path.parse` / `path.format` / `path.normalize` / `path.basename(p, ext)` / `path.sep` / `path.delimiter` — new HIR variants `PathRelative`, `PathParse`, `PathFormat`, `PathNormalize`, `PathBasenameExt`, `PathSep`, `PathDelimiter`; runtime functions `js_path_relative` / `js_path_normalize` / `js_path_parse` / `js_path_format` / `js_path_basename_ext` / `js_path_sep_get` / `js_path_delimiter_get` in `crates/perry-runtime/src/path.rs`. Shared `normalize_str` helper handles `..`/`.`/double-slash collapse. `path.parse` returns a `{ root, dir, base, ext, name }` shape object via `js_object_alloc_with_shape`. `path.join` now also normalizes its result so `join('/a', 'b', '..', 'c') === '/a/c'` (matches Node). Lowering added to both `is_path_module` dispatch sites in `lower.rs`. `test_gap_node_path.ts` now passes with zero diffs vs Node.

### v0.4.65
- feat(wasm): `--target web` (alias `--target wasm`) now compiles real-world multi-module apps end-to-end. Mango (50 modules, 998 functions, classes, async, fetch with headers, Hone code editor FFI) compiles to WASM, validates, instantiates, and renders its welcome screen in the browser matching the native app. Major fixes:
  - **class param counts**: constructors/methods/getters/setters now register in `func_param_counts` so `new Foo(a, b)` against a 4-arg ctor pads with `TAG_UNDEFINED` instead of underflowing the WASM stack ("call needs 2, got 1"). `new ClassName` and `super()` call sites consume the registered count and emit padding.
  - **module-level `const`/`let` promoted to WASM globals**: top-level Lets are now in a `module_let_globals: BTreeMap<(usize, LocalId), u32>` indexed by (mod_idx, LocalId). Two modules with `let id=1` no longer alias each other (telemetry's `CHIRP_URL` was reading connection-store's `isWeb` Boolean), and functions can now access top-level consts (previously they couldn't — local maps didn't include init Lets). `LocalGet`/`LocalSet`/`Stmt::Let` check `module_let_globals` first; per-module init local maps prevent inner Let collisions.
  - **`FetchWithOptions` strings now interned**: `collect_strings_in_expr` was missing `Expr::FetchWithOptions` / `FetchGetWithAuth` / `FetchPostWithAuth` cases, so header keys ("Content-Type", "X-Chirp-Key") and URL/body literals fell through the catch-all and resolved to string id 0 ("Authorization"). Headers now serialize correctly.
  - **constructor field initializers**: were doing `local.get` on uninitialized `temp_local_i32` and corrupting memory at address 0. Now compute `sp - 24` and `local.set` the temp before storing fields.
  - **`temp_store_local`**: dedicated 2nd i64 temp for `emit_store_arg` so nested calls don't clobber `temp_local`.
  - **all user functions exported as `__wasm_func_<idx>`** so async JS function bodies can call back into WASM via `wasmInstance.exports`.
  - **Async JS Call emit**: added missing `Expr::ExternFuncRef` case (was producing `fromJsValue(funcRef)(args)` instead of `funcRef(args)`); converts f64 args to BigInt at the JS↔WASM i64 boundary.
  - **`new ClassName(...)` JS emit extra paren** removed.
  - `wasm_runtime.js`: FFI namespace wrapped in a `Proxy` that auto-stubs missing imports with no-ops returning `TAG_UNDEFINED` (lets apps with native FFI like Hone Editor instantiate in the browser); new `wrapImportsForI64` wraps every host import to bit-reinterpret BigInt args ↔ f64 internally so `BigInt(NaN)` doesn't crash on every NaN-boxed return value; `VStack`/`HStack` accept and append a children array; `scrollviewSetChild` (lowercase v) added alongside `scrollViewSetChild` to match user-facing imports.
  - Result: a 4 MB self-contained HTML file boots, runs init across all 50 modules, creates DOM widgets, makes real `fetch()` calls (with correct URL + headers), and renders mango's welcome screen.

### v0.4.64
- perf/cleanup: drop dead `postgres`/`redis`/`whoami` deps from `perry-runtime` — `perry-runtime/Cargo.toml` had `default = ["full"]` which transitively pulled `dep:postgres`, `dep:redis`, and `dep:whoami` into every Perry binary that links libperry_runtime.a. Verified via grep that none were imported: `postgres` and `whoami` had zero references anywhere, `redis` was only used by `redis_client.rs` whose `js_redis_*` symbols nothing in codegen ever resolved (perry-stdlib's `ioredis.rs` is the live Redis path via `js_ioredis_*`). Deleted `redis_client.rs`, removed the three `dep:` entries from the `full` feature list and from `[dependencies]`. Real Redis/Mongo/Postgres support is unchanged — perry-stdlib's `ioredis.rs`/`mongodb.rs`/`pg.rs` (sqlx) all still build and link end-to-end with `--minimal-stdlib`. Measured: minimal-stdlib `libperry_stdlib.a` for `--features http-client` shrank 56 MB → 55 MB and the `perry_runtime-*` member shrank 3.24 MB → 3.10 MB; final binary unchanged because `-dead_strip` was already removing the orphaned redis code at link time, but build time, archive size, and dep hygiene all improve.

### v0.4.63
- fix: complete JWT `keyid`/`kid` codegen wiring — v0.4.62 landed the runtime side (`sign_common` accepts `kid_ptr`, all three signers take a 4th arg) but the matching codegen + runtime_decls were missed, so the call site still passed 3 args to a 4-arg function and the kid was never set. This commit adds the missing pieces: `runtime_decls.rs` declares the 4th `i64` (kid StringHeader ptr), `expr.rs` jsonwebtoken.sign branch extracts `keyid` (alias `kid`) from a literal options object via `compile_expr` + `js_get_string_pointer_unified`, and also fixes a long-standing payload bug where `jwt.sign(JSON.stringify({...}), key, opts)` produced `{}` because the codegen always re-stringified via `js_json_stringify` with object type-hint — now `Expr::JsonStringify(_)` / `Expr::String(_)` / string-typed `LocalGet` payloads are forwarded as raw StringHeader pointers. Verified end-to-end: a Perry-signed `{ alg: ES256, kid }` token validates in Node `jsonwebtoken.verify` against the EC public key. Unblocks APNs provider tokens.

### v0.4.62
- fix: `Class.prototype` / `class Foo {}` used as a first-class value crashed Cranelift verifier — `Expr::ClassRef` fallback paths called `js_object_alloc_fast` with one i32 zero, but the runtime takes `(class_id: i32, field_count: i32)` (two i32 args). Both branches (class without ctor + unknown class) now pass two zero i32 args.
- feat: JWT `keyid`/`kid` runtime side — `sign_common` in `perry-stdlib/src/jsonwebtoken.rs` accepts a `kid_ptr: *const StringHeader` (null = no `kid` field) and threads it into `Header.kid`; all three signers (`js_jwt_sign` / `_es256` / `_rs256`) take a 4th arg. The codegen + runtime_decls bits were missed in this commit and follow in v0.4.63.

### v0.4.61
- feat: `--minimal-stdlib` rebuilds perry-stdlib with only the Cargo features the project's imports actually need — collects native module specifiers into a new `CompilationContext.native_module_imports` set, maps each via `commands/stdlib_features.rs` (e.g. `mysql2`→`database-mysql`, `fastify`→`http-server`, `mongodb`→`database-mongodb`, `crypto`→`crypto`, fetch usage→`http-client`), then `cargo build --release -p perry-stdlib --no-default-features --features <list>` into `target/perry-stdlib-minimal/`. Both the symbol-stub scan and the link path now share one `stdlib_lib_resolved` so they see the same archive. Falls back to the prebuilt full stdlib if cargo isn't on PATH, the Perry workspace source isn't on disk, or the rebuild fails — never breaks the user's compile. Measured 4.2 MB → 3.4 MB (19% smaller) on a fetch-only program; the stdlib archive itself drops from 191 MB to 56 MB for `http-client` only and 34 MB for no optional features.
- fix: perry-stdlib couldn't compile without `default = ["full"]` — `common/handle.rs` used `dashmap` unconditionally (now a non-optional dependency since the handle registry is always-on), `common/dispatch.rs` referenced `crate::fastify::*`/`crate::ioredis::*` without cfg gates (now `#[cfg(feature = "http-server")]`/`#[cfg(feature = "database-redis")]`), and `common/async_bridge.rs` imported `tokio` always (now `#[cfg(feature = "async-runtime")]`-gated in `common/mod.rs`). `crypto` feature now implies `async-runtime` + `ids` because bcrypt offloads to `tokio::task::spawn_blocking` and `crypto.randomUUID()` delegates to the `uuid` crate. `database-mongodb` now pulls in `dep:futures-util` for `Cursor::try_collect`.

### v0.4.60
- feat: `js_jwt_sign_es256` / `js_jwt_sign_rs256` + reqwest `http2` feature — new ES256 (EC PEM key) and RS256 (RSA PEM key) JWT signers in `perry-stdlib/src/jsonwebtoken.rs` via shared `sign_common` helper using `EncodingKey::from_ec_pem`/`from_rsa_pem`. Codegen detects `jwt.sign(payload, key, { algorithm: 'ES256' | 'RS256' })` literal in `expr.rs` and reroutes `func_name` to the appropriate signer (HS256 stays default). Both registered in `runtime_decls.rs` (loop over the three names, identical signature) and stubbed in Android `stdlib_stubs.rs`. Also enables reqwest `http2` feature in `perry-stdlib/Cargo.toml`. Unblocks FCM (Firebase Cloud Messaging) OAuth assertion signing.

### v0.4.59
- feat: `Promise.allSettled` and `Promise.any` — both implemented as new runtime functions `js_promise_all_settled` / `js_promise_any` modeled on `js_promise_all`/`race`. `allSettled` builds `{ status: "fulfilled", value }` / `{ status: "rejected", reason }` result objects via `js_object_alloc_with_shape`. `any` settles with the first fulfilled promise, or rejects with an array of rejection reasons if all reject (Perry doesn't have `AggregateError` yet — it uses a plain array)

### v0.4.58
- feat: `String.prototype.at` / `String.prototype.codePointAt` / `String.fromCodePoint` — full Unicode support with UTF-16 code unit indexing semantics (matches JS spec, including surrogate pair handling for emoji); multi-arg `String.fromCodePoint(a, b, c)` and `String.fromCharCode(a, b, c)` lowered to a chain of binary string concats; new HIR variants `StringFromCodePoint`/`StringAt`/`StringCodePointAt` and runtime functions `js_string_from_code_point`/`js_string_at`/`js_string_code_point_at`; `is_string_expr` updated in 4 dispatch sites so concatenated `StringFromCodePoint` results take the string-add path
- fix: closure captures whose ids weren't bound in the construction site's `locals` were silently skipped — slot defaulted to `0.0`, the closure body's first read produced a NULL box pointer, and `js_box_get`/`js_box_set` printed warnings + dropped the write. `expr.rs` closure-construction loop now (a) looks the id up in a new `MODULE_VAR_DATA_IDS` thread-local and stores the global slot address as the box pointer if it's a module-level var, or (b) allocates a fresh zero-initialized box, in either case preserving slot index alignment. Supporting plumbing: `cranelift_var_type()` returns `I64` for `is_boxed=true` (module-level boxed primitives loaded from their global slot must use the box-pointer type, not F64); `compile_module` publishes `module_var_data_ids` to the thread-local AFTER class capture promotion and BEFORE any `compile_closure`/`compile_function`/`compile_class_method`/`compile_init` call. Verified end-to-end on arbcrypto's `l0-fee-updater` (228 modules) — previously printed the same null-box-pointer pattern in production every cycle, now zero warnings across consecutive 30s cycles.

### v0.4.57
- perf: Windows binaries now dead-strip unused code — `compile.rs` Windows linker branch passes `/OPT:REF /OPT:ICF` to MSVC link.exe / lld-link, the COFF equivalent of `--gc-sections` / `-dead_strip`. These flags are documented as defaults under `/RELEASE` but Perry doesn't pass `/RELEASE`, so the linker fell back to `/OPT:NOREF` and pulled the entire perry-stdlib archive even when only a fraction was used. First step toward shrinking Windows binaries; pairs with upcoming lazy runtime declarations and stdlib subsystem feature-gating.

### v0.4.56
- fix: `for (let i = 0; i < capturedArr.length; i++) total += capturedArr[i]` inside closures returned garbage — two for-loop optimizations (i32 counter promotion and array pointer caching) read the box pointer instead of the actual array pointer for boxed mutable captures; both now skip when `is_boxed`. `test_closure_capture_types` passes.
- fix: `getArea(shape)` where `shape` is typed as parent class but holds a subclass instance — method calls on local variables with class type now check for subclass overrides and route through `js_native_call_method` (runtime vtable dispatch) when the method is polymorphic, matching the existing `this.method()` virtual dispatch logic. `test_super_calls` passes.
- fix: `fs.existsSync()` returned `1`/`0` instead of `true`/`false` — all three dispatch paths (HIR `FsExistsSync`, native module fallback, and `js_native_call_method` runtime) now return NaN-boxed TAG_TRUE/TAG_FALSE booleans. `fs.readFileSync(path)` without encoding now returns a Buffer (via `FsReadFileBinary`) matching Node.js semantics; new `js_buffer_print` runtime function for `<Buffer xx xx ...>` console output. `test_cli_simulation` passes.

### v0.4.55
- fix: `makeStack().push(1)` pattern — object type literals (`{ push: (v) => void, ... }`) no longer misidentified as arrays. Three-layer fix: (1) `TsTypeLit` now extracts to `Type::Object(ObjectType)` instead of `Type::Any`, so `lookup_local_type` returns a proper object type; (2) HIR lowering skips the `ArrayPush`/`NativeMethodCall` fast paths when the receiver is `Type::Object`; (3) codegen `expr.push(value)` interception checks `is_pointer && !is_array` to avoid treating objects as arrays. Also fixed `widen_mutable_captures` not tracking `ArrayPush`/`ArrayPop`/`ArraySplice` as mutations — when one closure does `items.push(v)` and a sibling reads `items.length`, both must use boxed access to share the array pointer. `test_edge_closures` passes.
- fix: `Set.forEach`/`Map.forEach` closures referencing module-level `Set`/`Map` variables — `collect_referenced_locals_expr` in `closures.rs` was missing match arms for `SetHas`, `SetAdd`, `SetDelete`, `SetSize`, `SetClear`, `SetValues`, `MapSet`, `MapGet`, `MapHas`, `MapDelete`, `MapSize`, `MapClear`, `MapEntries`, `MapKeys`, `MapValues`, `MapNewFromArray`; the `_ => {}` catch-all silently dropped the references, so closures didn't load the collection from its global slot. `test_edge_map_set` passes. All 27/27 edge tests now pass.

### v0.4.54
- fix: `obj[Direction.Up]` SIGSEGV when index evaluates to `0` — `js_get_string_pointer_unified` returned null for `0.0` because the `bits != 0` guard blocked the number-to-string conversion path; also `is_string_index_expr_get` now returns `false` for `PropertyGet` on object literals (have `object_field_indices`) whose fields may be numeric. `test_edge_enums_const` passes (was 18 diff lines).

### v0.4.53
- fix: `const`/`let` inside arrow-function and function-expression bodies were being hoisted to the top of the body — lower.rs classified every `ast::Decl::Var` as hoistable alongside `var` and `function` declarations, so `const result = fn(n)` inside an `if/else` branch would run its initializer eagerly (e.g., memoize returning a closure that called its `fn` capture before the cache-hit check). Now only `VarDeclKind::Var` is hoisted; `Let`/`Const` remain in lexical position. `test_edge_higher_order` memoize test passes.
- fix: `groups["a"].length` on `Record<string, T[]>` returned undefined — the `.length` dispatch in `PropertyGet` codegen didn't treat `Expr::IndexGet` as a dynamic-array candidate, so the intermediate array from `obj[stringKey]` fell through to `js_dynamic_object_get_property("length")` (always undefined). Added `IndexGet` to the `use_dynamic_length` detection so it routes through `js_dynamic_array_length`. `test_edge_objects_records` passes.
- fix: `console.log(-0)` / `console.log(Math.round(-0.5))` printed `0` instead of `-0` — runtime number-printing paths used `.fract() == 0.0 && n.abs() < i64::MAX as f64` which treats ±0 identically. Added an `is_negative_zero` bit-pattern check to `js_console_log`, `js_console_log_dynamic`, `js_console_log_number`, their `error`/`warn` counterparts, and `format_jsvalue`/`format_jsvalue_for_json`. `String(-0)` and `JSON.stringify(-0)` still return `"0"` per ECMA-262. `test_edge_numeric` passes.
- fix: regex `.split(/.../)` caused SIGBUS — `js_string_split` received a regex pointer and fed it to `js_get_string_pointer_unified`, reading out of bounds. New runtime `js_string_split_regex` (uses `regex.split()`) wired through a regex-literal dispatch in both split codegen paths in `expr.rs`.
- feat: `String.prototype.search(regex)` — new runtime `js_string_search_regex` (uses `regex.find()`, byte→char offset for Unicode correctness) + codegen dispatch; added `search` to the outer string-method guard so string-literal receivers reach the inner match arm.
- fix: global `str.match(/.../g)` stored in a local returned garbage — `Expr::StringMatch`/`StringMatchAll` codegen now NaN-boxes the result with `POINTER_TAG` (null→`TAG_NULL`) instead of raw bitcast; `stmt.rs` local type inference marks `StringMatch`/`StringMatchAll` inits as `is_array=true` so `.length`/`[i]`/`.join()` dispatch correctly.
- fix: HIR lowering rejected `str.match(reG)` when `reG` was typed as `Type::Named("RegExp")` (regex literals get that type from `infer_type_from_expr`) — now accepts known-regex locals explicitly. `test_edge_json_regex` now passes fully (was 16 diff lines).
- fix: arrow-expression-body capture in a multi-closure object (`{ inc: () => ++v, get: () => v }`) returned garbage — HIR post-pass now widens every `Expr::Closure`'s `mutable_captures` to include any capture that is assigned inside a sibling (or nested) closure in the same lexical scope, so `get`'s read and `inc`'s write observe the same boxed `v`.
- fix: self-referential `const f = (n) => ... f(n-1)` at module level returned NaN — module-level LocalIds are now tracked via new `LoweringContext::{module_level_ids, scope_depth, inside_block_scope}` and stripped from closure `captures`, so the closure body loads `f` from its global data slot at call time instead of reading the not-yet-assigned capture slot. `scope_depth`/`inside_block_scope` counters keep per-iteration `const captured = i` inside top-level `for` loops out of the filter.
- fix: `Array.prototype` methods — `reverse()`, `sort()` (no comparator, string-default), `fill(value)`, `concat(other)` now work. New runtime functions `js_array_reverse`, `js_array_sort_default`, `js_array_fill`, `js_array_concat_new`; codegen dispatches them in the generic array-method path when the receiver is a known array. `js_array_is_array` now returns NaN-boxed TAG_TRUE/TAG_FALSE instead of 1.0/0.0. `matrix[i].length` fixed by adding `Expr::IndexGet` to the dynamic-length fallback list. `test_edge_arrays` passes with zero diff vs Node.
- fix: `Set.forEach(cb)` was undispatched — new `js_set_foreach` runtime function plus codegen dispatch in both the direct-LocalGet and `this.field` paths in `expr.rs`. Calls closure with `(value, value)` to match JS Set semantics where key===value.
- feat: `new Map([["k", v], ...])` constructor with iterable of `[key, value]` pairs — new `Expr::MapNewFromArray` HIR variant + `js_map_from_array` runtime function. Threaded through monomorph, analysis, codegen, closures, WASM/JS emitters, and util.rs expr-kind classifier.
- feat: `Array.from(iterable, mapFn)` two-arg form — new `Expr::ArrayFromMapped` HIR variant; codegen clones-or-set-to-array's the iterable then calls `js_array_map(arr, cb)`. `Array.from(new Set(...))` now also correctly dispatches to `js_set_to_array` (previously only `LocalGet` of a Set worked).
- fix: `.size` on Set/Map that is a mutable closure capture backed by a module-level global slot — the `PropertyGet` dispatch for `is_set`/`is_map` now reads through `js_box_get` when `info.is_boxed`, recovering the actual collection pointer instead of treating the slot address as the collection.
- fix: `g[numericKey] = v` on `Record<number, T>` (and other plain objects) silently stored into array slot memory — `IndexSet` now detects non-array local objects in both the `is_union_index` branch and the integer-key fallback, converting the numeric key via `js_jsvalue_to_string` and calling `js_object_set_field_by_name`. Unlocks `test_edge_iteration` which uses `groups[len] = []` then `groups[len].push(word)` inside a loop.
- fix: generic container classes (`Stack<T>`, `LinkedList<T>`, `Pipeline<T>`, `Observable<T>`, etc.) now dispatch user methods correctly — `arr_ident.push/pop/shift/…` HIR lowering in `perry-hir/src/lower.rs` no longer matches a local whose type is a user-defined class (`Type::Named` / `Type::Generic` where the base is in `ctx.lookup_class`); previously `numStack.push(1)` was lowered to `Expr::ArrayPush` and `numStack.size()`/`peek()` saw `items.length === 0`. The generic-expression `expr.push(value)` lowering path and the codegen `expr.push(value)` fast path in `perry-codegen/src/expr.rs` received the same guard for user-class receivers. Tuple return values like `[A, B]` from generic functions now also track as pointers via `HirType::Tuple(_)` in `is_typed_pointer`/`is_typed_array`. `test_edge_generics` now passes full Node.js parity.
- fix: nested `Record<string, Record<string, T>>` on a class field returned garbage — the "property-based array indexing" fast path in `IndexGet` codegen unconditionally treated `obj.field[key]` as an array lookup, calling `js_array_get_jsvalue` with a float-from-string-bitcast index; now guarded by checking `class_meta.field_types[field_name]` is actually `Type::Array`/`Type::Tuple` and that the index isn't a string. StateMachine, pipeline, observer patterns in `test_edge_complex_patterns` now work.
- fix: `this.items.pop()` / `this.items.shift()` inside method bodies (after inlining expands `this` → local var) now dispatch to `js_array_pop_f64`/`js_array_shift_f64` via a new `PropertyGet.pop/shift` intercept in `perry-codegen/src/expr.rs`; previously these fell through to `js_native_call_method` which has no array-method support.
- fix: class string fields returned empty after multiple closure-arg method calls in init — method inliner's `find_max_local_id` undercounted by not recursing into `Expr::Closure` bodies/params; new `find_max_local_id_in_module` scans init, all functions, class ctors/methods/getters/setters/static_methods (including their param ids) to compute a module-wide max so inliner-allocated `Let` ids never collide with existing HIR ids anywhere. Without this, an inlined init-level `Let` could land on the same LocalId as a class ctor param, causing the ctor's module-var loader to silently skip the conflicting slot and leaving `this.field` reading from uninitialized memory.
- fix: inherited methods in parent classes always static-dispatched `this.method()` to the parent's own method instead of virtual dispatch (e.g., `Shape.describe()` reading `this.area()` always got `Shape.area()` even on a `Rectangle` instance). `this.method()` now detects subclass overrides via `method_ids` comparison and routes through `js_native_call_method` (which uses the runtime vtable keyed by the object's actual `class_id`) when the method is polymorphic.
- fix: HIR `lower_class_decl` no longer adds shadow instance fields for `this.x = ...` assignments to inherited fields — previously `class Square extends Rectangle { constructor(side) { super(...); this.kind = "Square"; } }` added `kind` as a second own field of `Square`, so after `resolve_class_fields` merged parent indices, `Shape.describe()` read `this.kind` at the parent's offset (holding "Rectangle") while `sq.kind` read it at the shadowed offset (holding "Square"). New `class_field_names` registry in LoweringContext lets each class see the full ancestor field set to skip inherited names. `test_edge_classes` now passes fully.

### v0.4.52
- feat: labeled `break`/`continue` and `do...while` loops — new HIR variants `Labeled`, `LabeledBreak`, `LabeledContinue`, `DoWhile`; thread-local `LABEL_STACK`/`PENDING_LABEL` in codegen lets nested loops resolve labels without restructuring `loop_ctx`. `contains_loop_control` now recurses into nested loops to detect labeled control flow (prevents unsafe for-unrolling when an inner loop's `break outer`/`continue outer` targets an unrolled outer loop). `test_edge_control_flow` passes.
- fix: block scoping for `let`/`const` — inner-block bindings no longer leak to the enclosing scope. New `push_block_scope`/`pop_block_scope` on `LoweringContext` wrap bare blocks, `if`/`else` branches, `while`/`for`/`for-of`/`for-in` bodies, and `try`/`finally` blocks. `var` declarations (tracked via `var_hoisted_ids`) are preserved across block exits so they remain function-scoped per JS semantics.
- fix: destructuring — nested patterns, defaults, rest, and computed keys now work across `let`/`const` bindings and function parameters. Introduced recursive `lower_pattern_binding` helper as single source of truth; `lower_fn_decl` now generates destructuring extraction stmts for top-level function parameters (previously only inner/arrow functions did). `test_edge_destructuring` passes fully.
- fix: destructuring defaults correctly apply for out-of-bounds array reads — Perry's number arrays return bare IEEE NaN for OOB indices instead of `TAG_UNDEFINED`, so the previous `tmp !== undefined` check failed. Added `js_is_undefined_or_bare_nan` runtime helper + `Expr::IsUndefinedOrBareNan` IR node that matches either pattern, routed through the `Pat::Assign` desugaring.
- fix: `for (const ch of "hello")` produced garbage — ForOf lowering now detects string iterables via new `is_ast_string_expr` helper in `perry-hir/src/lower.rs`; typing the internal `__arr` holder as `Type::String` routes `__arr.length` and `__arr[__idx]` through the existing `is_string_object_expr` codegen path that calls `js_string_char_at` and NaN-boxes the 1-char result.
- fix: `[..."hello"]` array spread produced garbage — `ArrayElement::Spread` codegen in `perry-codegen/src/expr.rs` now detects string spread sources (`is_string_spread_expr`) and iterates `StringHeader.length` via `js_string_char_at` instead of `js_array_get_f64`.
- fix: object spread override semantics — `{...base, x: 10}` now correctly returns `10` for `x` (previously returned `base.x` because `js_object_clone_with_extra` added the static key as a duplicate entry in `keys_array`, and the linear scan returned the first match). Runtime `js_object_clone_with_extra` now reserves scratch slot capacity only; codegen routes static props through `js_object_set_field_by_name` (find-or-append with overwrite). Multi-spread `{...a, ...b}` supported via new `js_object_copy_own_fields` runtime helper. Also fixed latent `field_count=0` bump bug in `js_object_set_field_by_name`.
- feat: `String.prototype.lastIndexOf(needle)` — new `js_string_last_index_of` runtime function (uses `str::rfind`), wired through `runtime_decls.rs` and dispatched in both the LocalGet-string and generic string-method paths of expr.rs; returns f64 index or -1.
- fix: `"str " + array` printed `[object Object]` instead of the joined contents — string-concat codegen now detects `Expr::Array`/`is_array` LocalGet operands and routes them through `js_array_join` with a `,` separator per JS `Array.prototype.toString` semantics; `js_jsvalue_to_string` also learns to detect arrays via the `GcHeader.obj_type` so other stringification paths get the same behavior for free. Makes `test_edge_strings` pass full Node.js parity.

### v0.4.51

### v0.4.50
- feat: comprehensive edge-case test suite — 26 test files in `test-files/test_edge_*.ts` covering closures, classes, generics, truthiness, arrays, strings, type narrowing, control flow, operators, destructuring, async/promises, objects/records, interfaces, numeric edge cases, error handling, iteration, regex/JSON, and complex real-world patterns
- fix: boolean return values now NaN-boxed (TAG_TRUE/TAG_FALSE) instead of f64 0.0/1.0 — affects `Map.has/delete`, `Set.has/delete`, `Array.includes`, `String.includes/startsWith/endsWith`, `isNaN`/`isFinite`, `js_instanceof`; new `i32_to_nanbox_bool` helper in util.rs
- fix: `super.method()` in subclass methods caused "super.X() called outside of class context" — method inliner was inlining methods containing `super.*` calls into the caller, losing the class context; `body_contains_super_call` now prevents inlining of such methods in `perry-transform/src/inline.rs`
- fix: `Number.MAX_SAFE_INTEGER`, `MIN_SAFE_INTEGER`, `EPSILON`, `MAX_VALUE`, `MIN_VALUE`, `POSITIVE/NEGATIVE_INFINITY`, `NaN` constants now supported on the `Number` namespace
- feat: `Number.isNaN`, `Number.isFinite`, `Number.isInteger`, `Number.isSafeInteger` — strict (no coercion) versions via new runtime functions that return NaN-boxed booleans
- feat: `Math.trunc` and `Math.sign` — desugared at HIR level to conditional floor/ceil and sign-checking respectively
- fix: `Math.round(0.5)` returned 0 due to Cranelift's `nearest` using IEEE round-half-to-even; now uses `floor(x + 0.5)` for JS round-half-away-from-zero semantics
- fix: `!null`, `!undefined`, `!NaN`, `!!null`, `!!""+""` — unary Not now uses `js_is_truthy`/NaN-aware comparison for all NaN-boxed operand kinds including string concatenation, template literals, logical/conditional results, and Null/Undefined literals; numeric fallback uses `(val == 0) || (val != val)` to treat NaN as falsy
- fix: `"" || "default"` returned empty string — Logical OR now calls `js_is_truthy` on I64 string pointers (wrapped via `inline_nanbox_string`) instead of raw null-pointer check, so empty strings are correctly treated as falsy
- fix: `null === undefined` returned true — Compare with null/undefined now uses strict equality (compares against specific NaN-boxed tag) instead of the old "is any nullish" loose semantics
- fix: `Infinity` printed as `inf` in `String(Infinity)` / `number.toString()` / array join — `js_number_to_string`, `js_string_coerce`, and `js_array_join` now format `NaN`/`Infinity`/`-Infinity`/`-0` per JS semantics
- fix: `EventEmitter` class name in user code collided with Perry's native EventEmitter — workaround: renamed user class in test (Perry needs a proper name-scoping fix later)
- test: comprehensive edge-case parity suite — 7 of 26 tests now pass against Node.js `--experimental-strip-types`, up from 3; several others are within 1–6 diff lines of passing

### v0.4.49
- fix: x86_64 SIGSEGV when `Contract.call()` returns a tuple — `js_array_map` (and forEach/filter/find/findIndex/some/every/flatMap) called `js_closure_call1` passing only the element, not the index; callbacks using `(_, i) => value[i]` got garbage in `i` from uninitialized xmm1 register on x86_64, causing SIGSEGV on `value[garbage]`. Changed all array iteration functions to use `js_closure_call2(callback, element, index)` matching JS semantics. Also fixed all remaining `extern "C" fn -> bool` ABI mismatches across perry-runtime and perry-stdlib (17 functions)

### v0.4.48
- fix: x86_64 SIGSEGV in `Contract()` with 20-module ethkit — wrapper functions for FuncRef callbacks (e.g., `.map(resolveType)`) now use `Linkage::Export` instead of `Linkage::Local`; module-scoped names prevent collisions while Export linkage ensures correct `func_addr` resolution on x86_64 ELF; also added cross-platform GcHeader validation for `keys_array` in `js_object_get_field_by_name` to catch corrupted object pointers (Linux lacked the macOS-only ASCII heuristic guard)

### v0.4.47
- fix: module-local function wrappers use `Linkage::Local` — prevents cross-module symbol collisions when two modules share filename + function names (e.g., two `contract.ts` files both with `resolveType`); fixes x86_64 wrong dispatch in large module graphs
- feat: `Promise.race` implemented — `js_promise_race` runtime function with resolve/reject handlers; settles with first promise that completes
- fix: `obj[c.name]` returned garbage when `c` is from `any`-typed array element — `is_string_index_expr_get` now defaults `PropertyGet` to string except for known class instances with numeric fields
- fix: union-typed `obj[integerKey]` used string-key lookup instead of `js_dynamic_array_get` — added `is_union` to `is_known_array` check for correct runtime dispatch
- fix: cross-module `await` on `Promise<[T, T]>` tuple — added `Tuple` to Await expression-inference handler's inner_type match (one-line fix at line 810)

### v0.4.46
- feat: `String.replaceAll(pattern, replacement)` — string-pattern replaceAll via new `js_string_replace_all_string` runtime function; dispatched in both local-variable and generic-expression codegen paths
- feat: `String.matchAll(regex)` — new `StringMatchAll` HIR expression + `js_string_match_all` runtime returning array of match arrays with capture groups; supports `for...of`, spread, and `.map()` iteration
- fix: `arr.shift()?.trim()` / `arr.pop()?.trim()` returned wrong element — optional chaining re-evaluated the side-effecting shift/pop in the else branch; codegen now caches the shift/pop result via `OPT_CHAIN_CACHE` thread-local; HIR lowering nests chained methods (`.trim().toLowerCase()`) inside the inner conditional's else branch instead of creating redundant outer conditionals
- fix: `Buffer.subarray()`/`Buffer.slice()` on derived buffers — `is_buffer_expr` in stmt.rs now detects `buf.slice()`/`buf.subarray()` via local buffer check; `is_string_expr` excludes buffer locals; inline buffer method dispatch added for non-LocalGet buffer objects (e.g. `Buffer.from(...).subarray(3)`)
- fix: SQLite `stmt.run(params)` / `stmt.get(params)` / `stmt.all(params)` — parameters were ignored; codegen now builds a JS array from all arguments; runtime `params_from_array` reads NaN-boxed values (strings, numbers, null, booleans) directly instead of JSON deserialization
- fix: SQLite `stmt.run()` result object — `{ changes, lastInsertRowid }` now allocated with named keys via `js_object_alloc_with_shape` so property access works
- feat: `db.pragma('journal_mode')` — added codegen dispatch + runtime declaration for `js_sqlite_pragma`; result NaN-boxed as string
- feat: `db.transaction(fn)` — returns a wrapper closure that calls BEGIN/fn/COMMIT; runtime `sqlite_tx_wrapper` function captures db_handle + original closure
- fix: `.length` on `Call` results (e.g. `stmt.all().length`) — `Expr::Call` added to dynamic array length detection in PropertyGet handler
- fix: cross-module function call dispatched to wrong export in large modules on x86_64 — exported overload signatures (no body) were pushed to `module.functions` alongside the implementation, and codegen compiled the first entry (empty-body overload) then skipped the real implementation; also changed `func_refs_needing_wrappers` from `HashSet` to `BTreeSet` for deterministic wrapper generation order across platforms

### v0.4.45
- fix(wasm): multi-module `FuncRef` resolution — per-module func_map snapshots prevent cross-module FuncId collisions; void function tracking pushes TAG_UNDEFINED for stack consistency; missing arguments padded with TAG_UNDEFINED for optional params

### v0.4.44
- fix: `obj[numericKey]` on `Record<number, T>` returned garbage — `IndexGet` treated all numeric indices as array offsets; now detects non-array objects in both the union-index dispatch path and the plain-index fallback, converting numeric keys to strings via `js_jsvalue_to_string` for property lookup. Also fixed `is_string_index_expr_get` treating all `PropertyGet` as string-producing (broke `obj[classField]` where field is number).
- fix: `!('key' in obj)` always returned false — `in` operator returns NaN-boxed TAG_TRUE/TAG_FALSE but `!` used float comparison (NaN != 0.0 is true); added `Expr::In` to `needs_truthy_check`. Root cause of ethkit `Contract()` SIGSEGV: provider detection ternary evaluated wrong branch, setting `provider` to `undefined`.
- fix: `trimStart()`/`trimEnd()` dispatched to correct runtime functions in all codegen paths — previously fell through to generic dispatch returning null bytes; broke ethkit ABI `parseSignature()` output type parsing
- fix: cross-module default array parameter `param: T[] = []` caused SIGSEGV — `Expr::Array([])` default not handled inline, function received null pointer; added `js_array_alloc(0)` fallback
- fix: `IndexSet` union-index string-key path NaN-boxes I64 closures/objects with POINTER_TAG — `ensure_f64` raw bitcast stripped the tag, making closures stored via `obj[dynamicKey]` uncallable through `js_native_call_method`
- fix: `.filter(Boolean)` desugaring applied to all 4 HIR lowering paths (was only in local variable path); extracted `maybe_wrap_builtin_callback` as `LoweringContext` method
- fix: null pointer guards in closure capture getters and `Promise.all` fulfill/reject handlers
- fix: cross-module `await` on `Promise<[T, T]>` (tuple) returned undefined on indexing — `Tuple` type not recognized in the Await expression-inference handler alongside `Array`; also added `Tuple` to `is_typed_pointer`, `is_typed_array`, and split-function local type analysis

### v0.4.43
- feat(wasm): FFI support — `declare function` statements generate WASM imports under `"ffi"` namespace; enables Bloom Engine and other native libraries to provide GPU rendering, audio, etc. to WASM code
- feat(wasm): void FFI functions push TAG_UNDEFINED for stack consistency; `extern_funcs` field added to HIR Module
- feat(wasm): `bootPerryWasm(base64, ffiImports)` accepts optional FFI import providers; `__perryToJsValue`/`__perryFromJsValue` exposed globally for external FFI bridges

### v0.4.42
- fix: `Boolean()` constructor — added `BooleanCoerce` HIR/codegen handling via `js_is_truthy`; previously returned `undefined` for all inputs
- fix: `!!string` always false — `Expr::String` and `Expr::Unary(Not)` now route through `js_is_truthy` instead of float comparison which treated NaN-boxed strings as zero
- fix: `String(x)` on string locals/params returned "NaN" — `StringCoerce` NaN-boxed I64 string pointers with POINTER_TAG instead of STRING_TAG, so `js_string_coerce` didn't recognize them as strings
- fix: `.filter(Boolean)` / `.map(Number)` / `.map(String)` — desugar bare built-in identifiers to synthetic closures in all 4 HIR lowering paths (local vars, imported vars, inline array literals, generic expressions)
- fix: `analyze_module_var_types` set `is_union=true` for Unknown/Any even when concrete type (array, closure, map, set, buffer) was known — caused I64/F64 type mismatch corrupting pointers on Android ARM (FP flush-to-zero)
- fix: null pointer guards in closure capture getters (`js_closure_get_capture_f64/ptr`) and `Promise.all` fulfill/reject handlers — prevents SIGSEGV when closures are corrupted before async callbacks fire

### v0.4.41
- feat: `perry publish` passes `features` from perry.toml project config to build manifest — enables feature-gated builds on the server side
- fix: tvOS stdlib builds — upgrade mongodb 2.8→3.5 to eliminate socket2 0.4.x (no tvOS support); all socket2 deps now ≥0.5 which includes tvOS
- test: add module-level array loop read tests, cross-module exported function array lookup tests, and Android label/i18n resource tests

### v0.4.40
- fix: Windows VStack/HStack `WS_CLIPCHILDREN` with local `WM_CTLCOLORSTATIC` handling — Text controls now fill their own background with ancestor color instead of relying on parent paint-through, fixing blank text over gradient backgrounds
- fix: Windows `WM_MOUSEWHEEL` forwarded to window under cursor — scroll events now reach embedded views and ScrollViews instead of only the focused window
- fix: Windows layout Fill distribution uses local tracking instead of permanently mutating widget flags — repeated layout passes with changing visibility no longer accumulate stale `fills_remaining`
- fix: Windows Image `setSize` DPI-scales to match layout coordinates — images no longer appear at wrong size on high-DPI displays

### v0.4.39
- fix: Android VStack default height changed from MATCH_PARENT to WRAP_CONTENT — prevents VStacks from expanding to fill parent, matching iOS UIStackView behavior; use `widgetMatchParentHeight()` to opt-in

### v0.4.38
- feat: `perry setup tvos` — guided wizard for tvOS App Store Connect credentials and bundle ID (reuses shared Apple credentials from iOS/macOS)
- feat: `perry publish tvos` — full tvOS publishing support with bundle ID, entry point, deployment target, encryption exempt, and Info.plist config via `[tvos]` section in perry.toml
- perf: direct object field get/set via compile-time known field indices — skips runtime hash lookup for object literals

### v0.4.37
- fix: `is_string` locals (i64 pointers) passed to functions expecting f64 now NaN-box with STRING_TAG instead of POINTER_TAG — fixes `textfieldGetString` return values becoming `undefined` when used in `encodeURIComponent`, `||`, or cross-module calls (GH-10, GH-12)
- fix: JS interop fallback (`js_call_function`/`js_native_call_method`) NaN-boxes string args with STRING_TAG instead of raw bitcast — fixes string corruption in native module calls (GH-10, GH-11, GH-12)

### v0.4.36
- perf: object field lookup inline cache — FNV-1a hash + 512-entry thread-local direct-mapped cache in `js_object_get_field_by_name`, skips linear key scan on cache hit
- feat: iOS/tvOS game loop reads `NSPrincipalClass` from Info.plist for custom UIApplication subclass; tvOS Info.plist includes scene manifest + `BloomApplication`
- feat: tvOS/watchOS (tier 3) compilation uses `cargo +nightly -Zbuild-std`; iOS/tvOS linker adds `-framework Metal -lobjc`
- fix: GTK4 `ImageFile` path resolution type mismatch (`PathBuf` → `String`); codegen `LocalInfo` missing `object_field_indices` field in closures/stmt

### v0.4.35
- fix: Windows Image widget rewritten with GDI+ alpha-blended WM_PAINT — PNG transparency now composites correctly over parent backgrounds (gradients, solid colors). Replaced SS_BITMAP (opaque BitBlt) with custom PerryImage window class that paints ancestor backgrounds into the DC first, then draws via `GdipDrawImageRectI` with full alpha support.

### v0.4.34
- fix: Windows VStack/HStack removed `WS_CLIPCHILDREN` — parent gradient/solid backgrounds now paint through child areas so transparent text/images show correctly over gradients
- fix: Windows layout respects `fixed_height`/`fixed_width` on cross-axis — Image with `setSize(56,56)` no longer stretches to parent height in HStack

### v0.4.33
- fix: Windows `ImageFile` now resolves relative paths against the exe directory (parity with macOS/GTK) — installed/published executables can find assets next to the binary instead of relying on cwd
- fix: `perry compile` now copies `assets/`, `logo/`, `resources/`, `images/` directories next to the output exe on Windows/Linux (non-bundle targets), matching macOS `.app` bundle behavior

### v0.4.32
- fix: macOS `ImageFile` `setSize` now resizes the underlying NSImage to match — previously only the view frame changed, leaving the intrinsic content size mismatched; also sets `NSImageScaleProportionallyUpOrDown`
- fix: macOS `ImageFile` resolves relative paths via NSBundle.mainBundle.resourcePath first, then executable dir — fixes images in `.app` bundles
- fix: Android APK now bundles `assets/`, `logo/`, `resources/`, `images/` directories — `ImageFile('assets/foo.png')` works at runtime

### v0.4.31
- fix: Windows Text widgets now transparent over gradient backgrounds — `WM_CTLCOLORSTATIC` returns `NULL_BRUSH` instead of ancestor's solid brush, so parent gradient/solid paints show through correctly
- fix: Windows Image bitmap transparency uses ancestor background color — `reload_bitmap_scaled` fills transparent areas with the nearest ancestor's bg color instead of white, so images blend with gradient/colored containers

### v0.4.30
- fix: `arr[i]` in for-loop inside function returned `arr[0]` for every `i` — LICM incorrectly hoisted loop-counter-indexed array reads as invariant when BCE didn't fire (module-level `const` limits like `MAX_COINS` had `is_integer=false` despite having `const_value`); also `collect_assigned_ids` only scanned loop body, missing the `update` expression where the counter is assigned

### v0.4.29
- fix: Android crash in UI pump ticks — perry-native thread exited after `main()` returned, dropping the thread-local arena and freeing all module-level arrays/objects; UI thread's pump tick then called `getLevelInfo()` on dangling pointers → segfault. Fixed by parking the perry-native thread after init instead of letting it exit.
- fix: Android `-Bsymbolic` linker flag prevents ELF symbol interposition (process's `main()` vs perry's `main()`)

### v0.4.28
- fix: module-level arrays/objects with `Unknown`/`Any` HIR type loaded as F64 instead of I64 in functions — `analyze_module_var_types` set `is_union=true` for Unknown/Any, causing `is_pointer && !is_union` to select F64; init stored I64 but functions loaded F64, corrupting pointers on Android (FP flush-to-zero); now arrays/closures/maps/sets/buffers always use I64

### v0.4.27
- fix: Android `JNI_GetCreatedJavaVMs` undefined symbol — `jni-sys` declares extern ref but Android has no `libjvm.so` (`libnativehelper` only at API 31+); Perry's linker step now compiles a C stub `.o` and links it into the `.so`

### v0.4.26
- fix: Android UI builds had undefined `js_nanbox_*` symbols — `strip_duplicate_objects_from_lib` removed `perry_runtime-*` objects from the UI lib while `skip_runtime` prevented the standalone runtime from being linked; skip strip-dedup on Android (like Windows) since `--allow-multiple-definition` handles duplicates

### v0.4.25
- fix: Windows layout engine now reloads Image bitmaps at layout size — `widgetSetWidth`/`widgetSetHeight` on images previously left the bitmap at its original pixel dimensions, causing clipped/invisible images

### v0.4.24
- feat: macOS cross-compilation from Linux — codegen triple, framework search paths, `-lobjc`, CoreGraphics/Metal/IOKit/DiskArbitration frameworks, `find_ui_library` for macOS
- feat: iOS Info.plist now includes all Apple-required keys, CFBundleIcons with standard naming, version/build_number from perry.toml, UILaunchScreen dict
- fix: bitwise NOT (`~x`) wrapping semantics — `f64→i64→i32` (ireduce) for JS ToInt32 instead of `fcvt_to_sint_sat` which saturated at i32::MAX
- fix: IndexGet string detection — property access returning array (e.g., `log.topics[0]`) treated as potential string for proper comparison codegen
- fix: `Array.filter/find/some/every/flatMap` callback dispatch + module init ordering
- fix: null arithmetic coercion — `Math.max(null, 5)` etc. coerces null to 0 via `js_number_coerce`
- fix: `new X(args)` resolves cross-module imported constructor functions and exported const functions via `__export_` data slot
- fix: `new Date(stringVariable)` properly NaN-boxes with STRING_TAG for string detection
- fix: `is_macho` uses target triple instead of host `cfg!` check; always generate `main` for entry module on iOS/macOS cross-compile
- fix: ld64.lld `sdk_version` set to 26.0 (Apple requires iOS 18+); `/FORCE:MULTIPLE` for Windows cross-compile duplicate symbols

### v0.4.23
- fix: i18n translations now propagate to rayon worker threads — parallel module codegen was missing the i18n string table, causing untranslated output; also walks parent dirs to find `perry.toml`
- fix: iOS crashes — gate `ios_game_loop` behind feature flag, catch panics in UI callback trampolines (button, scrollview, tabbar), panic hook writes crash log to Documents
- fix: iOS Spacer crash — removed NSLayoutConstraint from spacer creation that caused layout engine conflicts
- fix: iOS/macOS duplicate symbol crash — `strip_duplicate_objects_from_lib` now works cross-platform (not just Windows), deduplicating perry_runtime from UI staticlib
- feat: iOS cross-compilation from Linux using `ld64.lld` + Apple SDK sysroot (`PERRY_IOS_SYSROOT` env var)
- fix: `ld64.lld` flags — use `-dead_strip` directly instead of `-Wl,-dead_strip` for cross-iOS linking
- fix: `perry run` improvements — reads app metadata from perry.toml/package.json, applies `[publish].exclude` to tarballs, uses `create_project_tarball_with_excludes`
- fix: threading resilience — `catch_unwind` in spawn, poisoned mutex recovery in `PENDING_THREAD_RESULTS`, tokio fallback to current-thread runtime on iOS

### v0.4.22
- fix: module-level array `.push()` lost values when called from non-inlinable functions inside for/while/if/switch bodies — `stmt_contains_call` only checked conditions, not bodies, so module vars weren't reloaded from global slots after compound statements containing nested calls

### v0.4.19
- fix: Spacer() inside VStack now properly expands — iOS: added zero-height constraint at low priority + low compression resistance; Android: VStack uses MATCH_PARENT height so weight=1 takes effect
- fix: iPad camera orientation — preview layer now updates `videoOrientation` on device rotation via `UIDeviceOrientationDidChangeNotification` observer
- fix: V8 interop symbols (`js_new_from_handle`, `js_call_function`, etc.) now have no-op stubs in perry-runtime — pre-built iOS/Android libraries no longer fail with undefined symbols

### v0.4.18
- perf: fold negative number literals at HIR level — `-14.2` lowers to `Number(-14.2)` instead of `Unary(Neg, Number(14.2))`, eliminating unnecessary `fneg` instructions in array literals and arithmetic

### v0.4.17
- fix: iOS builds failed with undefined `_js_new_from_handle` — `is_macho` excluded iOS so `_` prefix wasn't stripped during symbol scanning, preventing stub generation for V8 interop symbols
- fix: Android large exported arrays (>128 elements) were null — stack-based init caused SEGV on aarch64-android; arrays >128 elements now use direct heap allocation instead of stack slots

### v0.4.16
- fix: `===`/`!==` failed for concatenated/OR-defaulted strings — `is_string_expr` didn't recognize `Expr::Logical` (OR/coalesce) or `Expr::Conditional`, causing mixed I64/F64 representation; also fixed operator precedence in `is_dynamic_string_compare` and added NaN-boxing safety net for I64 string locals in fallback comparison path

### v0.4.15
- fix: Windows non-UI programs no longer fail with 216 unresolved `perry_ui_*` symbols — UI/system/plugin/screen FFI declarations guarded behind `needs_ui` flag (GH-9)
- feat: release packages now include platform UI libraries — `libperry_ui_macos.a` (macOS), `libperry_ui_gtk4.a` (Linux), `perry_ui_windows.lib` (Windows)

### v0.4.14
- fix: Linux linker no longer requires PulseAudio for non-UI programs — `-lpulse-simple -lpulse` moved behind `needs_ui` guard (GH-8)
- fix: `perry run .` now works — positional args parsed flexibly so non-platform values are treated as input path instead of erroring
- perf: native `fcmp` for numeric comparisons — known-numeric operands emit Cranelift `fcmp` instead of `js_jsvalue_compare` runtime call; mandelbrot 30% faster
- perf: `compile_condition_to_bool` fast path — numeric `Compare` in loop/if conditions produces I8 boolean directly, skipping NaN-box round-trip
- perf: in-place string append with capacity tracking — `js_string_append` reuses allocation when refcount=1 and capacity allows; string_concat 125x faster
- perf: deferred module-var write-back in loops — skip global stores inside simple loops, flush at exit
- perf: short-circuit `&&`/`||` in `compile_condition_to_bool` — proper branching instead of always-evaluate-both with `band`/`bor`
- chore: rerun all benchmarks with Node v25 + Bun 1.3, add Bun to all entries, full README with context for wins AND losses

### v0.4.13
- fix: VStack/HStack use GravityAreas distribution + top/leading gravity — children pack from top-left instead of stretching or centering
- fix: `getAppIcon` crash in callbacks — wrapped in `autoreleasepool` for safe use during TextField onChange and other AppKit event dispatch
- fix: `appSetSize` codegen — moved to early special handling to avoid generic dispatch type mismatch
- fix: Windows frameless windows get rounded corners via `DWMWA_WINDOW_CORNER_PREFERENCE` (Win11+)

### v0.4.12
- fix: `getAppIcon` crash during UI callbacks — retain autoreleased NSImage immediately to survive autorelease pool drains
- feat: `appSetSize(width, height)` — dynamically resize the main app window (macOS/Windows/GTK4)
- fix: rounded corners on frameless+vibrancy windows — deferred corner radius to `app_run` after vibrancy/body setup, added Windows 11 `DWMWA_WINDOW_CORNER_PREFERENCE`

### v0.4.11
- feat: `registerGlobalHotkey` — system-wide hotkey via NSEvent global/local monitors (macOS), Win32 RegisterHotKey+WM_HOTKEY (Windows), stub with warning (Linux)
- feat: `getAppIcon` — app/file icon as Image widget via NSWorkspace.iconForFile (macOS), .desktop Icon= parsing + theme lookup (Linux), stub (Windows)

### v0.4.10
- feat: `window_hide`, `window_set_size`, `window_on_focus_lost` — multi-window management APIs across macOS, Windows, GTK4, with no-op stubs on iOS/tvOS/watchOS/Android

### v0.4.9
- feat: Window config properties for launcher-style apps — `frameless`, `level`, `transparent`, `vibrancy`, `activationPolicy` on `App({})` config object (macOS/Windows/Linux)

### v0.4.8
- feat: Android camera support — `CameraView` widget using Camera2 API via JNI, with live preview, color sampling, freeze/unfreeze, and tap handler (parity with iOS)

### v0.4.7
- feat: Windows x86_64 binary in GitHub releases — CI builds perry.exe + .lib runtime libs, packaged as .zip
- feat: winget package manager support — auto-publishes `PerryTS.Perry` on each release via wingetcreate

### v0.4.6
- fix: `this.field.splice()` on class fields caused memory corruption — HIR desugars to temp variable pattern
- fix: i18n locale detection uses NSBundle.preferredLocalizations on iOS (respects per-app language settings)
- fix: `perry_system_preferences_get` handles NSArray values (e.g., AppleLanguages) on iOS
- fix: `clear_children`/`remove_child` safe subview removal — snapshot before mutation, reverse order, metadata map cleanup (macOS + iOS)

### v0.4.5
- feat: `@perry/threads` npm package — standalone Web Worker parallelism (`parallelMap`, `parallelFilter`, `spawn`) + perry/thread WASM integration via worker pool with per-worker WASM instances
- fix: WASM `%` (modulo) and `**` (exponent) operators caused validation error — `f64` values stored into `i64` temp local; now use `emit_store_arg` path like `+`

### v0.4.4
- feat: tvOS (Apple TV) target support — `--target tvos`/`--target tvos-simulator`, UIKit-based perry-ui-tvos crate, `__platform__ === 6`, app bundle creation, simulator detection

### v0.4.3
- fix: fetch().then() callbacks never fired in native UI apps — `spawn()` didn't call `ensure_pump_registered()`, so resolved promises were never drained

### v0.4.2
- fix: `=== false`/`=== true` always returned true — codegen used `ensure_i64` which collapsed both TAG_TRUE and TAG_FALSE to 0; now uses raw bitcast
- fix: `===`/`!==` with NaN-boxed INT32 vs f64 (e.g. parsed data `=== 5`) always returned false — added INT32→f64 coercion in `js_jsvalue_equals`
- fix: negative number equality/comparison broken — `bits < 0x7FF8...` unsigned check excluded negative f64 (sign bit set); now uses proper tag-range check

### v0.4.1
- Performance: Set O(n)→O(1) via HashMap side-table, string comparison via SIMD memcmp
- Performance: GC pass consolidation (4→3 passes), expanded `_unchecked` array access paths in codegen
- Performance: BTreeMap→HashMap across codegen Compiler struct (20+ fields), `Cow<'static, str>` for 950 extern func keys
- Performance: HashMap indices for HIR lowering (functions, classes, imports) and monomorphization lookups
- Tests: 50+ new Rust unit tests for Set, GC, Array, String, HIR lowering, monomorphization
- fix: Windows test builds — geisterhand UI dispatch uses registered function pointers instead of extern declarations, eliminating linker errors when UI crate is not linked.

### v0.4.0
- `perry/thread` module: `parallelMap`, `parallelFilter`, and `spawn` — real OS threads with compile-time safety. `SerializedValue` deep-copy, thread-local arenas with `Drop`, promise integration via `PENDING_THREAD_RESULTS`.
- Parallel compiler pipeline via rayon: module codegen, transform passes, nm symbol scanning all across CPU cores.
- Array.sort() upgraded from O(n²) insertion sort to O(n log n) TimSort-style hybrid.
- Comprehensive threading docs in `docs/src/threading/` (4 pages).

### v0.3.3
- `perry publish`: `.env` loading, `[publish] exclude` in perry.toml

### v0.3.2
- watchOS native app support (`--target watchos`/`--target watchos-simulator`)

### v0.3.0
- Compile-time i18n system (`perry/i18n` module): zero-ceremony localization, `[i18n]` config, embedded string table, native locale detection (6 platforms), CLDR plural rules, format wrappers

### Older (v0.2.37-v0.2.203)
See CHANGELOG.md for detailed history. Key milestones:
- v0.2.198: WidgetKit (iOS + Android + watchOS + Wear OS)
- v0.2.191: Geisterhand UI testing framework
- v0.2.183-189: WebAssembly target (`--target wasm`)
- v0.2.180: `perry run` command with remote build fallback
- v0.2.172: Codebase refactor (codegen.rs split into 12 modules, lower.rs into 8)
- v0.2.156-162: Cross-platform UI parity (all 6 platforms at 100%)
- v0.2.150-151: Native plugin system
- v0.2.147: Mark-sweep garbage collection
- v0.2.116: Native UI module (perry/ui)
- v0.2.115: Integer function specialization (fibonacci 2x faster than Node)
- v0.2.79: Fastify-compatible HTTP runtime
- v0.2.37: NaN-boxing foundation
