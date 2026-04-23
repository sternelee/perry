# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.172

## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 28 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last sweep:** **18/28 passing** (re-measured v0.5.170 after Phase 3/4 work; test_gap_proxy_reflect flipped from fail→pass via Phase 4 method-call inference). Known failing: `array_methods`, `async_advanced`, `console_methods`, `error_extensions`, `fetch_response`, `global_apis`, `map_set_extended`, `object_methods`, `string_methods`, `typed_arrays`. Run via `/tmp/run_gap_tests.sh` after full rebuild.

**Known categorical gaps**: lookbehind regex (Rust `regex` crate), `console.dir`/`console.group*` formatting, lone surrogate handling (WTF-8).

## Workflow Requirements

**IMPORTANT:** Follow these practices for every code change made directly on `main` (maintainer workflow):

1. **Update CLAUDE.md**: Add 1-2 line entry in "Recent Changes" for new features/fixes
2. **Increment Version**: Bump patch version (e.g., 0.5.48 → 0.5.49)
3. **Commit Changes**: Include code changes and CLAUDE.md updates together

### External contributor PRs

PRs from outside contributors should **not** touch `[workspace.package] version` in `Cargo.toml`, the `**Current Version:**` line in `CLAUDE.md`, or add a "Recent Changes" entry. The maintainer bumps the version and writes the changelog entry at merge time — usually by rebasing the PR branch and amending. This avoids the patch-version collisions that happen when Perry's `main` ships several commits while a PR is in review (each on-main commit bumps the version; a PR that bumped to the same patch on day 1 is already behind by merge day). Contributors just write code; let the maintainer fold in the metadata last.

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

- **v0.5.172** — Fix #20: `console.trace()` now emits a real native backtrace (via `std::backtrace::Backtrace::force_capture`) to stderr after the `Trace: <msg>` line instead of only echoing the message. New `js_console_trace` in `builtins.rs` filters `std::backtrace_rs` / `js_console_trace` noise and collapses duplicate unresolved frames; symbolicated frames require `PERRY_DEBUG_SYMBOLS=1` (without it, LLVM-stripped builds show `__mh_execute_header` frames).
- **v0.5.170** — Phase 4.1: method-call return-type inference (`lower_types.rs:232`) consults a new `class_method_return_types` registry so `new C().label()` binds with the method's inferred type instead of `Type::Any`. 40 HIR tests green; gap stable 18/28. Inheritance chain lookup not yet implemented.
- **v0.5.169** — Phase 4 expansion: body-based return-type inference now covers class methods (`lower_decl.rs:1532`), getters, and arrow expressions (`lower_types.rs:210`). Async wraps in `Promise<T>`; generators skipped; annotation wins over inference. Gap 17→18/28.
- **v0.5.168** — Fix #150: `Object.getOwnPropertyDescriptor` returned `undefined` on Phase 3 anon-shape literals — `mark_all_candidate_refs_in_expr` (`collectors.rs:3353`) catch-all now escapes all candidates on un-enumerated HIR variants, matching the defensive pattern already used elsewhere.
- **v0.5.167** — Phases 1+4 of Static-Hermes object-layout parity: (1) object-literal shape inference replaces the `Expr::Object → Type::Any` bail with a walker that builds `Type::Object` for closed shapes; (4) body-based return-type inference for free functions. Phase 3 (synthetic `__AnonShape_N` class) infra landed but disabled — needs escape analysis or annotation-opt-in to avoid breaking `Object.*`/`JSON.stringify`/Proxy semantics. 28 new HIR integration tests.
- **v0.5.166** — Fix #145: README's "beats every benchmark" claim now names the `json_roundtrip` exception (1.6× slower than Node, 2.4× slower than Bun). Added row to the comparison table; filed #149 for the stdlib JSON perf work.
- **v0.5.165** — Fix #144: TypeScript decorators were parsed into HIR but silently dropped at codegen. New `reject_decorators` helper in `lower_decl.rs:394` hard-errors on every decoration point; README flipped to `❌`. Same warn→bail reasoning as v0.5.119.
- **v0.5.164** — Fix #140: restore autovectorization of pure-accumulator loops regressed v0.5.22→v0.5.162. i32 shadow slot now gated on actual index-use (`collect_index_used_locals`); asm-barrier skipped when body writes to outer locals. `loop_overhead` 32→12ms, `math_intensive` 48→14ms, `accumulate` 97→24ms.
- **v0.5.163** — docs+chore (#139, tracking #140): deleted 17 stale `bench_*.ts` scratchpads + 15 result logs, reran polyglot suite, documented `loop_overhead`/`math_intensive`/`accumulate` regressions vs v0.5.22 baseline and fast-math-default narrative in README + `RESULTS.md`.
- **v0.5.162** — Fix #136: `sendToClient(handle, msg)` / `closeClient(handle)` named imports from `'ws'` silently no-op'd (missing from the receiver-less dispatch table). Added `js_ws_send_to_client` / `js_ws_close_client` bridges + `NativeModSig` entries.
- **v0.5.161** — Fix #135: `--target web` hung on `break`/`continue` nested in `if`/`switch`/`try`. WASM emitter's hardcoded `Br(1)`/`Br(0)` replaced with `Br(block_depth - break_depth.last())` — same formula labeled-break/continue already used.
- **v0.5.160** — PR #134 (closes #131): V2.2 per-module on-disk object cache at `.perry-cache/objects/<target>/<key:016x>.o`. Atomic tmp+rename writes; djb2 key covers source hash + `CompileOptions` + codegen env vars + perry version. `--no-cache`/`PERRY_NO_CACHE=1`/`perry cache info|clean`; ~29% warm speedup.
- **v0.5.159** — Fix winget submission in `release-packages.yml`: `wingetcreate --submit` looks up the authenticated user's fork, not the org's. Pre-step now resolves the token's user via `gh api /user` and syncs `<user>/winget-pkgs` before submit.
- **v0.5.158** — Fix #133: five `--target web` (WASM) bugs — (1) primitive method dispatch via `typeof` fast-path in `__classDispatch`; (2) `Math.sin`/`cos`/`tan`/`atan2`/`exp` (+ `MathHypot`) routed through `emit_memcall`; (3) `xs.push` on top-level arrays (module globals, not locals) via new `emit_local_or_global_get`; (4) Firefox/Safari NaN-tag canonicalization at FFI boundary — new `wrapFfiForI64` decodes BigInt directly; (5) INT32-tagged constants crashed wasm-bindgen — `__bitsToJsValue` now decodes INT32_TAG.
- **v0.5.157** — Fix #128: `obj.field` read NaN on `--target android` (Bionic Scudo allocator places heap below the Darwin mimalloc window). Codegen receiver guard in PIC + `.length` fast-path replaced `handle > 2 TB && < 128 TB` with platform-independent NaN-box tag check `(bits >> 48) & 0xFFFD == 0x7FFD`.
- **v0.5.156** — Fix `await-tests` gate (v0.5.155): swap `gh run list --workflow "Name"` (silently returned `[]` in `release`-event context on CI) for `gh api /repos/.../actions/workflows/{test,simctl-tests}.yml/runs?head_sha=$SHA`. Retry-once + fail-loud on `gh api` errors.
- **v0.5.155** — Gate `release-packages.yml` on green `Tests` + `Simulator Tests (iOS)` for the exact tagged commit (new `await-tests` job, 45-min deadline). Added `tags: ['v*']` to `test.yml`'s push trigger so Tests actually runs on the release tag SHA. `workflow_dispatch` bypass preserved.
- **v0.5.154** — Drop `run_with_timeout` wrapper from simctl launch path. Its bash watcher-subshell + caller `set -e` produced spurious `exit 143` on the fast path; with `--console-pty` gone, the 30s timeout's reason-to-exist is gone too.
- **v0.5.153** — Instrument `scripts/run_simctl_tests.sh` with per-phase timestamped trace lines to localize the post-v0.5.152 `~2:04` hang; added explicit `timeout-minutes: 60`/`45` to simctl-tests workflow.
- **v0.5.152** — Drop `--console-pty` from `scripts/run_simctl_tests.sh`: when simctl's stdout is file-redirected the PTY master never sees EOF, so simctl hangs past `LAUNCH_TIMEOUT` even after the app `process::exit(0)`s. Trade "clean-exit verification" for "bundle launches" — still enough tier-2 signal.
- **v0.5.151** — Closed four perry/ui gaps: (1) `alertWithButtons(title, msg, string[], (i)=>void)` (split from 2-arg `alert`); (2) `preferencesSet`/`Get` widened to `string | number` in `.d.ts`; (3) macOS `onTerminate`/`onActivate` lifecycle hooks (AppDelegate overrides + test-mode `invoke_*` helpers); (4) `LazyVStack` rewritten on `NSTableView` with real virtualization (~15 realized rows for 1000-count).
- **v0.5.150** — `--app-bundle-id` CLI flag now honored on `--target ios-simulator`/`--target ios`. iOS branch in `compile.rs:6442` previously resolved via perry.toml → package.json → default only, ignoring the CLI. Closes the tier-2 simctl workflow (pairs with v0.5.147–149).
- **v0.5.149** — iOS-simulator `Info.plist` now emits `iPhoneSimulator` / `iphonesimulator` / `iphonesimulator26.4` for `CFBundleSupportedPlatforms`/`DTPlatformName`/`DTSDKName` (was hardcoded `iPhoneOS`, causing `FBSOpenApplicationServiceErrorDomain code=4` on `simctl launch`).
- **v0.5.148** — `xcrun simctl launch` has **no `--setenv` flag**. Use `SIMCTL_CHILD_KEY=VAL` env prefix on the calling shell instead; simctl strips the prefix and forwards. Inline comment warns future readers so it doesn't get un-fixed a fourth time.
- **v0.5.147** — `scripts/run_simctl_tests.sh`: split `--setenv=KEY=VAL` into `--setenv KEY=VAL` (two argv tokens). Superseded by v0.5.148 after verifying simctl has no such flag at all.
- **v0.5.146** — `perry.nativeLibrary.targets.<plat>.metal_sources` sibling to `swift_sources` (closes #124). New `compile_metallib_for_bundle` shells out to `xcrun -sdk <sdk> metal/metallib` and writes `<app>.app/default.metallib`. Unblocks Bloom's SwiftUI shader path.

Older entries → CHANGELOG.md.
