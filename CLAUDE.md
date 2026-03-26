# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and Cranelift for code generation.

**Current Version:** 0.4.19

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
