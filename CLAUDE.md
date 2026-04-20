# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.116

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

- **v0.5.116** — Fix `animateOpacity`/`animatePosition` end-to-end (closes #109). Three independent bugs, all surfaced by a single user Web/WASM repro where `label.animateOpacity(visible.value ? 1.0 : 0.0, 0.3)` rendered text stuck at opacity ≈0.3 and button clicks did nothing. (a) **Signature mismatch**: web/wasm runtimes had `perry_ui_animate_opacity(h, from, to, duration)` (3 user args) while docs + all 7 native FFIs use `(target, durationSecs)` (2 user args). The 2-arg call was JS-binding to `from=1.0, to=0.3, duration=undefined` → `el.style.opacity = 0.3` = the reported "ghost" appearance. Rewrote both `crates/perry-codegen-js/src/web_runtime.js` and `crates/perry-codegen-wasm/src/wasm_runtime.js` to the canonical 2-arg form, reading the widget's *current* opacity for the animation start. Same treatment for `animate_position` (now takes `(dx, dy, durationSecs)` relative to current position, mirroring the native delta-based API instead of the absolute-coordinate from/to form). (b) **Unit inconsistency across platforms**: docs promised seconds; WASM runtime used `${duration}ms`; web JS used `${duration}s`; native macOS/iOS/tvOS all ran `duration_secs = duration_ms / 1000.0` (so `0.3` seconds → `0.0003` seconds visually instant); Android/GTK4 treated input directly as ms. Unified to seconds everywhere: dropped the `/1000.0` in macOS/iOS/tvOS `widgets/mod.rs` (use the param directly with `NSAnimationContext::setDuration` / `CABasicAnimation::setDuration`); Android multiplies by 1000 before passing to `ViewPropertyAnimator::setDuration(Long)`; GTK4 computes `((duration_secs * 1000.0) / 16.0).max(1.0)` steps. Renamed the FFI param `duration_ms` → `duration_secs` across all 7 `lib.rs` + `widgets/mod.rs` + watchOS stub. WASM runtime now emits `${durationSecs}s`. (c) **No reactivity**: `label.animateOpacity(visible.value ? 1.0 : 0.0, 0.3)` ran once at module top level and was never re-evaluated on `visible.set(...)`. Mirrors the #104 Text-template reactivity gap — now fixed analogously in `crates/perry-hir/src/lower.rs::try_desugar_reactive_animate`: when the receiver is a method call whose name is `animateOpacity`/`animatePosition` and any arg reads `<ident>.value` on a registered `perry/ui` State, desugar to an IIFE that (i) stores the widget handle in a local, (ii) runs the initial animation, (iii) registers a `stateOnChange` subscriber per distinct read-state that re-lowers the target/dx/dy expressions against current values and re-invokes the animate call. New helper `collect_state_value_reads` walks ternaries, bin ops, unary, template literals, call args, array/object lits, parens, and TS-cast wrappers so state reads nested inside `visible.value ? 1.0 : 0.0` or `count.value + 1` are still detected. To keep the HIR target-agnostic the desugar emits `NativeMethodCall { method: "widgetAnimateOpacity" }` — matches the existing native UiSig at `crates/perry-codegen/src/lower_call.rs:3703` and new `widgetAnimateOpacity` entries in `crates/perry-codegen-wasm/src/emit.rs:354` + `crates/perry-codegen-js/src/emit.rs:3090` that alias to the runtime symbol. Also updated `types/perry/ui/index.d.ts`: `Widget` is now a branded number intersected with `WidgetMethods { animateOpacity(target, durationSecs); animatePosition(dx, dy, durationSecs); }` so `label.animateOpacity(...)` type-checks without `// @ts-ignore`; the free-function `widgetAnimateOpacity`/`widgetAnimatePosition` signatures had been declared with the wrong arg order `(widget, duration, opacity)`, corrected to match runtime. Docs at `docs/src/ui/animation.md` rewritten to document the target/delta semantics and the state-reactive pattern. **Breaking change**: callers who passed durations in milliseconds to native UI (e.g. `btn.animatePosition(100, 200, 500)` expecting half a second) now animate for 500 seconds; the docs always said seconds but the impls silently contradicted. No caller in the workspace relies on the old behavior. **Verification coverage**: `cargo check -p perry-hir -p perry-codegen-js -p perry-codegen-wasm -p perry-ui-macos` compiles clean. Dynamic verification on web/wasm is the blocker — see followup rebuild + browser test with the exact `animasyon.ts` repro from #109.
- **v0.5.115** — Fix `find_native_library` target-key mapping for watchOS (closes #107). The `target_key` match at `crates/perry/src/commands/compile.rs:1504` had arms for ios/android/tvos/linux/windows/web with a `_ => "macos"` catch-all — so `--target watchos[-simulator]` silently resolved to `"macos"` and tried to build the package's `native/macos/` crate for the watch-sim triple (arm64-apple-watchos10.0-simulator), failing with unrelated errors like `rfd` not implementing `FileSaveDialogImpl`. Added the missing `Some("watchos-simulator") | Some("watchos") => "watchos"` arm. Now `perry.nativeLibrary.targets.watchos` in `package.json` is honored, matching the existing tvos/ios arms. Unblocks the native-side followup mentioned in #106 (bloom's `native/watchos/` crate).
- **v0.5.114** — Add `--features watchos-game-loop` so Metal/wgpu engines (bloom) can run on watchOS (closes #106). Mirrors the iOS precedent at `crates/perry-runtime/src/ios_game_loop.rs`: new `crates/perry-runtime/src/watchos_game_loop.rs` gated on `cfg(all(target_os = "watchos", feature = "watchos-game-loop"))` provides `pub extern "C" fn main()` that (a) registers a fallback `PerryWatchGameLoopAppDelegate` WKApplicationDelegate subclass dynamically via objc runtime, (b) spawns `_perry_user_main` on a background "perry-game" thread, (c) calls `perry_register_native_classes()` so the native lib can override the delegate, (d) enters `WKApplicationMain(0, NULL, "PerryWatchGameLoopAppDelegate")` on the main thread. Fallback delegate's `applicationDidFinishLaunching` calls `perry_scene_will_connect(NULL)` (NULL because watchOS has no UIWindowScene equivalent — native lib resolves the active WKApplication root itself). FFI surface mirrors iOS: `perry_register_native_classes` + `perry_scene_will_connect` + new `perry_watchos_classes_registered` callback. Compile-side plumbing in `crates/perry/src/commands/compile.rs`: (a) `build_optimized_libs` now takes `cli_features: &[String]` and threads `perry-runtime/watchos-game-loop` (also `perry-runtime/ios-game-loop`, previously missing) into the auto-rebuild cargo `--features` list, so the module is actually compiled into the rebuilt runtime archive; (b) `_main` rename target switches from `_perry_main_init` (SwiftUI scene-tree path from #105) to `_perry_user_main` when the feature is on, so the Rust runtime's `main()` wins the process entry; (c) linker driver flips from `swiftc` + `PerryWatchApp.swift` to plain `clang -target arm64-apple-watchos10.0(-simulator) -isysroot <sdk>` since no SwiftUI is needed; (d) framework list drops `SwiftUI`, adds `QuartzCore` + `-lobjc` (Metal.framework is deliberately NOT linked — it's absent from the watchOS SDK; the native lib must dlopen it or bundle its own path for device Metal). Info.plist unchanged (`WKApplication=true`, `UIDeviceFamily=[4]`) — `WKApplicationMain` takes the delegate class name as an argument so `NSPrincipalClass` isn't needed. **Verification coverage**: cross-compile of `perry-runtime` for `aarch64-apple-watchos-sim` with `--features watchos-game-loop` via `cargo +nightly -Zbuild-std=std,panic_abort` succeeds (0 errors, 133 existing warnings). End-to-end `perry compile test.ts --target watchos-simulator --features watchos-game-loop` auto-rebuilds the runtime with the feature enabled (19.1 MB), invokes clang with the right sysroot + QuartzCore, and fails link with the expected `Undefined symbols: _perry_register_native_classes, _perry_scene_will_connect` — confirming the C `main` from `watchos_game_loop.rs` is present in the archive and references the native-lib FFI contract correctly. With a bloom-style native lib supplying those two symbols the link would complete. Not dynamically verified on watchOS hardware or simulator — no Metal.framework in the watchOS SDK means the Metal-surface path itself can't be tested without the native lib bloom is pending. The default (non-game-loop) `--target watchos-simulator` path still produces a working `.app` bundle (regression-tested with `test_ui_counter.ts`), so #105 is not disturbed.
- **v0.5.113** — Make `--target watchos-simulator` / `--target watchos` actually compile end-to-end (closes #105). The scaffolding was already there (CLI parse, `perry-ui-watchos` crate w/ scene-tree + PerryWatchApp.swift renderer, WKApplication/UIDeviceFamily=[4] Info.plist, framework link list) — two concrete gaps blocked it. (a) `auto_rebuild_runtime_and_stdlib` in `crates/perry/src/commands/compile.rs:1141` invoked plain `cargo build -p perry-runtime -p perry-stdlib --target aarch64-apple-watchos-sim` but watchOS is Tier-3 in Rust; cargo dies with "can't find crate for `core`" because no prebuilt libstd ships for the triple. Fix: when `target` is `{tvos,watchos}[-simulator]` prepend `+nightly` and append `-Zbuild-std=std,panic_abort` (matches the pattern the `build_native_library` path at line 5914 already uses for user-declared native libs). (b) The `_main → _perry_main_init` objcopy rename inside the swiftc linker path only matched object files whose name contained the literal substring `main_ts` — fine if the user's entry is `main.ts`, silently skipped otherwise, so every non-`main.ts` watchOS build failed at link with `Undefined symbols: "_perry_main_init"`. Fix: compute the expected stem from `args.input.file_stem()` (`test_ui_counter.ts` → `test_ui_counter_ts`) and match on full stem equality. Verified end-to-end: `perry compile test-files/test_ui_counter.ts --target watchos-simulator -o /tmp/TestCounter` now produces a `/tmp/TestCounter.app` that installs via `simctl install`, launches under `WKApplicationExtensionMain` → `SwiftUI.ExtensionDelegate`, and renders the expected "Count: 0" Text + Increment Button through the scene-tree / SwiftUI bridge on an Apple Watch Series 10 (42mm) sim running watchOS 26.4. Only watchOS-specific behaviour left for v1 (per the issue): Digital Crown input — that's the engine's `WKCrownSequencer` backend, explicitly not Perry's responsibility.
- **v0.5.112** — Wire up auto-reactive `Text(\`...${state.value}...\`)` in the HIR lowering (closes #104). Detect at `ast::Expr::Call` when the callee resolves to `perry/ui`'s `Text` import and the first arg is a template literal containing one or more `<ident>.value` reads where `<ident>` is a registered `State` native instance. Desugar to an IIFE closure whose body (a) creates the Text widget with the initial concat, (b) registers a `stateOnChange` subscriber per distinct state (each capturing the widget handle, re-evaluating a fresh concat of the same template, and calling `textSetString`), (c) returns the widget handle. The outer IIFE is necessary because `collect_module_let_ids` only tracks `Stmt::Let` — a bare `LocalSet/LocalGet` inside an `Expr::Sequence` at module top level had no WASM global or local slot and the `LocalGet` returned `TAG_UNDEFINED`, silently dropping the widget from its parent container. Also traversed into `Expr::Sequence` in `perry-codegen-wasm/src/emit.rs::collect_strings_in_expr` (the `Update | Sequence` catch-all had skipped the whole body, causing "Count: ", `perry_ui_text_create`, `perry_ui_state_on_change`, `perry_ui_text_set_string` to be absent from the WASM string table, so the memcalls resolved to 0 in the name table). Added `"stateOnChange"` alongside `"onChange"`/`"state_on_change"` in WASM `map_ui_method`. Verified in jsdom: `Text(\`Count: ${count.value}\`)` plus three clicks of `Increment` now yields "Count: 3" (was stuck at "Count: 0" on every click). `test_ui_state_binding.ts`'s four different template-literal forms (`\`Count: ${count.value}\``, `\`Value: ${count.value} items\``, `\`${count.value}\``, `\`${count.value}!\``) all update together through +1 and -1 clicks. Native macOS binary still compiles and runs (0.8 MB, no crash); the same HIR flows through LLVM codegen's closure + stateOnChange dispatch. Same root cause existed on every platform (ios/macos/gtk4/windows/android/watchos/tvos/wasm) — the docs promised template-literal reactivity at state.md:33 but no backend emitted the binding — web/wasm was just where the user happened to try it. **Verification coverage**: dynamically verified via click-through on web/wasm (jsdom) and macOS native (AppleScript driving the AppKit window). iOS/tvOS/watchOS/Android/gtk4/windows were statically audited — all 7 platform `lib.rs` files expose identical FFI signatures `perry_ui_state_on_change(i64, f64)` + `perry_ui_text_set_string(i64, i64)` with real (non-stub) bodies, `state_set` fires subscribers through the same `js_closure_call1(closure_ptr, value: f64)` extern C ABI, and watchOS's scene-tree model calls `bump_version()` in `tree::with_node_mut` so text mutations correctly invalidate. Since the LLVM codegen path is shared across all non-wasm targets and the HIR is target-agnostic, the same emitted calls land on identical pathways — but they haven't been exercised on hardware.
- **v0.5.111** — Loosen flaky CI bound on `event_pump::tests::wait_returns_when_timer_due` (150 ms → 500 ms). GitHub runner overshot 150 ms by 12 ms on one run; condvar `wait_timeout` + scheduling slack on a loaded shared runner can legitimately exceed 150 ms for a 50 ms-deadline wait. 500 ms still proves "does not block indefinitely" (`IDLE_CAP_MS` is 1000 ms). No runtime behavior change.
- **v0.5.110** — Wire up `ForEach(state, render)` codegen in `perry-ui-macos` path (followup to #103). Previously `perry/ui` warned `method 'ForEach' not in dispatch table (args: 2)` and the generic fallback returned 0/undefined; the outer VStack's `widget_add_child` was then called with an invalid handle, AppKit silently refused to attach the window body, and the process ran with `lsappinfo type="BackgroundOnly"` — no window shown. New special case in `lower_call.rs` (next to `VStack`/`HStack`): synthesize a `perry_ui_vstack_create(8.0)` container, call `perry_ui_for_each_init(container, state_handle, render_closure)`, return the NaN-boxed container pointer. Verified: `test_min4.ts` (VStack containing ForEach over State<string[]>) and the rebuilt todo app both now launch with `type="Foreground"`. The `ForEach` UiSig isn't table-registered because it's variadic-in-shape (needs side-effectful container synthesis + for_each_init call + return), same pattern as VStack/HStack.
- **v0.5.109** — Fix `perry init` TypeScript type stubs and the UI docs that exercised them (closes #103). `types/perry/ui/index.d.ts`: `State` is now generic (`State<T = number>`) so `State<string[]>([])` and `State("")` type-check instead of erroring on the old number-only signature; added `ForEach(count: State<number>, render)` export (was used in the todo-app docs example but missing from the stub); `stateBindTextfield` now takes `State<string>`. Rewrote the docs examples in `docs/src/getting-started/first-app.md` (counter + todo), `docs/src/ui/state.md` (two-way binding section, onChange snippet, complete example), `docs/src/ui/widgets.md` (TextField/SecureField/Toggle/Slider/Picker/Form), and `docs/src/ui/dialogs.md` to use the actual runtime signatures — `TextField(placeholder, onChange)`, `Slider(min, max, onChange)`, `Picker(onChange)` + `pickerAddItem`, etc. — instead of the fictional `TextField(state, placeholder)` / `count.onChange(...)` forms. The old forms silently segfaulted at launch because `UiArgKind::Str` at `lower_call.rs:2557` routes the first arg through `get_raw_string_ptr`, and a State handle (i64) reinterpreted as a NaN-boxed string derefs garbage. No runtime/codegen change — the stubs are embedded via `include_str!` at compile time and ship to users via `perry init` / `perry types`. Verified: 5 new example binaries (counter, todo, controls, form, onchange) all `tsc --noEmit` clean AND compile + launch without crashing.
- **v0.5.108** — Honor `PERRY_RUNTIME_DIR` / `PERRY_LIB_DIR` env vars in `find_library` so out-of-tree installs (perry binary in `/usr/local/bin`, source tree elsewhere) can point at an explicit lib dir. The "Could not find libperry_runtime.a" error now lists every candidate path it searched and names the env var as a fix. Closes #101.
- **v0.5.107** — First end-to-end release with npm distribution live. `@perryts/perry` + seven per-platform optional-dep packages (`@perryts/perry-{darwin-arm64,darwin-x64,linux-x64,linux-arm64,linux-x64-musl,linux-arm64-musl,win32-x64}`) publish via OIDC Trusted Publisher from `release-packages.yml` on each GitHub Release. `npx @perryts/perry compile file.ts` works on all seven platforms. No runtime/codegen change.
- **v0.5.106** — Swap `lettre`'s `tokio1-native-tls` feature for `tokio1-rustls-tls` in `crates/perry-stdlib/Cargo.toml`. Eliminates `openssl-sys` / `native-tls` from the transitive dep tree (they were the only holdouts; the policy comment at Cargo.toml:35 already states "rustls only to avoid OpenSSL"). Unblocks the musl CI build — `openssl-sys` was failing with "Could not find openssl via pkg-config: cross-compilation unsupported" on `x86_64-unknown-linux-musl`. No functional change for SMTP clients; rustls provides the same TLS surface.
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
