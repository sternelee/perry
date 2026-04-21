# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.119

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

- **v0.5.119** — Fix the "styling example silently does nothing on Windows" bug from #114 by attacking the confusion at the source: the docs and the compiler's error reporting. Root cause was not Windows-specific — the user's snippet called an instance-method styling API that doesn't exist (`label.setColor("#333333")`, `btn.setCornerRadius(8)`, `stack.setPadding(20)`, `count.get()`) alongside an `App(title, builder)` callback form that also doesn't exist, and the compiler swallowed every one of those calls as a silent no-op. The dedicated `App` arm at `crates/perry-codegen/src/lower_call.rs:2437` only matched `args.len() == 1` with an Object-literal body; a 2-arg `App("title", () => {...})` fell through to the receiver-less early-out (`return TAG_UNDEFINED`), so `perry_ui_app_create` / `perry_ui_app_run` were never emitted — `main()` returned immediately, `/SUBSYSTEM:WINDOWS` swallowed the process, the user saw "nothing happens." The two `eprintln!("perry/ui warning: ... not in dispatch table", ...)` sites (lines 2427 + 2651) were intentional (comment at line 2424: "Warn at compile time so missing methods are visible instead of silently returning 0.0") but a warning stream interleaved with hundreds of LNK4006 linker warnings is invisible in practice; I flipped both to `bail!` so the build now fails loudly. The `App(...)` arm gained explicit `bail!`s for `args.len() != 1` and for non-Object first arg, with error text naming the expected config-object shape ("There is no `App(title, builder)` callback form"). Since upgrading warn→error would break `test_ui_comprehensive.ts` (which legitimately called `scrollviewSetOffset`/`appSetMinSize`/`appSetMaxSize` — real runtime FFIs at `crates/perry-ui-windows/src/lib.rs:114,120,531` that had never been registered in the compile-time `PERRY_UI_TABLE`), I added those three rows near the `appSetTimer` entry. `appSet{Min,Max}Size` → `(Widget, F64, F64) → Void`; `scrollviewSetOffset` → `(Widget, F64) → Void` — the lowercase-v spelling matches the real runtime symbol `perry_ui_scrollview_set_offset(i64, f64)` which takes a single vertical offset, unlike the 3-arg `scrollViewSetOffset` already in the table that matches `index.d.ts:240`'s declaration (the pre-existing mismatch between declared signature and runtime FFI is a separate bug, untouched). `docs/src/ui/styling.md` was the other half of the fix: every code snippet except the bottom "Complete Styling Example" promised a `label.setFontSize(24)` / `btn.setCornerRadius(8)` / `setColor("#FF0000")` instance-method API with hex-string colors that has never existed — `types/perry/ui/index.d.ts:12-23` only puts `animateOpacity`/`animatePosition` on `Widget`. Rewrote the whole page to the real free-function API: `textSetFontSize(widget, size)`, `widgetSetBackgroundColor(widget, r, g, b, a)` with RGBA floats in [0, 1] (plus a "divide each byte by 255" hint for hex-familiar readers), `setCornerRadius(widget, r)`, `setPadding(widget, top, left, bottom, right)` as 4 args not 1, `widgetSetBorderColor/Width`, `widgetSetEnabled(w, 0|1)`, `widgetSetBackgroundGradient(w, r1,g1,b1,a1, r2,g2,b2,a2, angle)`. The "Complete Styling Example" and `card()` composition helper at the bottom were rewritten to compile end-to-end and verified on Windows (a native AppKit-equivalent window actually appears). The `setFrame` line from the old docs was dropped (no such free function in index.d.ts). Added an explicit callout that `App(...)` accepts only the config-object form. Version collision note: this commit was originally authored as v0.5.118 on a local branch while #116 (glibc npm manifests) also shipped as v0.5.118 to origin; rebased and bumped to v0.5.119 so the two commits deconflict. **Verification coverage**: (a) `cargo check -p perry-codegen --release` clean. (b) All 5 `test-files/test_ui_*.ts` still compile (`test_ui_comprehensive.ts` was the risk — it calls the three newly-registered methods). (c) User's `#114` reproducer now emits `perry/ui: '.get(...)' is not a known instance method (args: 0)` and refuses to link. (d) A minimal `App("title", fn)` snippet (without `.get()`) emits the distinct `App(...) takes a single config object literal ... no App(title, builder) callback form`. (e) The rewritten Complete Styling Example from `styling.md` compiles to a 689 KB Windows binary that opens a real window (confirmed via `Start-Process` + 2-second `HasExited` check). The LLVM backend change is shared across all non-wasm targets so the error upgrade applies on macOS/Linux/Windows/iOS/tvOS/watchOS/Android identically; the three PERRY_UI_TABLE rows resolve to runtime symbols that exist on every platform's `perry-ui-*` crate.
- **v0.5.118** — Drop `libc: ["glibc"]` from glibc Linux npm manifests (closes #116). npm's libc auto-detection returns empty on some real-world builds (custom kernels, certain Node versions), causing it to skip both glibc and musl variants. Unconstrained glibc package now installs by default; musl packages keep `libc: ["musl"]` and the wrapper's `isMusl()` still picks correctly at runtime.
- **v0.5.117** — Wire `URL` / `URLSearchParams` through the LLVM backend (closes #111). Added codegen arms for all `UrlNew`/`UrlSearchParams*`/`UrlGet*` HIR variants that fell through the `--backend llvm` catch-all; fixed `runtime_decls.rs` ABI mismatch (I64→DOUBLE) and runtime's `create_url_object` now stores a real URLSearchParams object in `searchParams`.
- **v0.5.116** — Fix `animateOpacity`/`animatePosition` end-to-end (closes #109). Web/wasm signature mismatched native (2 user args, not 3); duration unit inconsistent across platforms (unified to seconds); state-reactive animation desugars to IIFE with `stateOnChange` subscribers. **Breaking**: durations previously passed in ms to native UI are now seconds.
- **v0.5.115** — Fix `find_native_library` target-key mapping for watchOS (closes #107). `--target watchos[-simulator]` silently resolved to `"macos"` via catch-all; added the missing watchos arm.
- **v0.5.114** — Add `--features watchos-game-loop` so Metal/wgpu engines run on watchOS (closes #106). New `watchos_game_loop.rs` provides C `main` → WKApplicationMain with a fallback delegate; compile-side threads the feature into auto-rebuild and swaps to plain clang linker.
- **v0.5.114** (#108) — `console.log` on Windows was silently producing no output; MSVC linker paired `/SUBSYSTEM:WINDOWS` with `/ENTRY:mainCRTStartup`, suppressing stdio attach. Gated on `needs_ui`: CLI programs get CONSOLE, UI programs keep WINDOWS.
- **v0.5.113** — Make `--target watchos[-simulator]` compile end-to-end (closes #105). watchOS is Rust Tier-3 — auto-rebuild needs `+nightly -Zbuild-std`; also fixed `_main → _perry_main_init` objcopy rename to compute the expected stem from `args.input.file_stem()` instead of substring-matching `main_ts`.
- **v0.5.112** — Wire up auto-reactive `Text(\`...${state.value}...\`)` in HIR lowering (closes #104). Desugars to an IIFE that creates the widget, registers `stateOnChange` per distinct state read, and returns the widget handle; also walks `Expr::Sequence` in WASM string collection.
- **v0.5.111** — Loosen flaky CI bound on `event_pump::tests::wait_returns_when_timer_due` (150 ms → 500 ms). No runtime behavior change.
- **v0.5.110** — Wire up `ForEach(state, render)` codegen in `perry-ui-macos` (followup to #103). Synthesize a VStack container + call `perry_ui_for_each_init`; prior generic fallback returned an invalid handle and the window ran `BackgroundOnly`.
- **v0.5.109** — Fix `perry init` TypeScript stubs + UI docs (closes #103). `State<T>` generic, `ForEach` exported, docs rewritten to real runtime signatures (`TextField(placeholder, onChange)` etc.) — the fictional state-first forms silently segfaulted at launch.
- **v0.5.108** — Honor `PERRY_RUNTIME_DIR` / `PERRY_LIB_DIR` env vars in `find_library` (closes #101). Error now lists every path searched.
- **v0.5.107** — First end-to-end release with npm distribution live. `@perryts/perry` + seven per-platform optional-dep packages publish via OIDC Trusted Publisher.
- **v0.5.106** — Swap `lettre`'s `tokio1-native-tls` for `tokio1-rustls-tls`. Eliminates `openssl-sys` from the dep tree; unblocks musl CI.
- **v0.5.105** — `Int32Array.length` returned 0 — `js_value_length_f64` only handled NaN-boxed pointers; typed arrays flow as raw `bitcast i64→double`. Added raw-pointer arm guarded on the Darwin mimalloc heap window.
- **v0.5.104** — Extend v0.5.103 inliner fix: `substitute_locals` also walks `WeakRef*`/`FinalizationRegistry*`/`Object{Keys,…}`/`Math{Sqrt,…}` wrappers. Same `_ => {}` catch-all root cause.
- **v0.5.103** — Inliner `substitute_locals` now traverses single-operand wrappers (`IsUndefinedOrBareNan`, `IsNaN`, coerce, `TypeOf`, `Void`, `Await`, etc.). Destructuring defaults were reading the wrong slot via unmapped LocalGets.
- **v0.5.102** — Class-instance scalar replacement no longer drops the constructor when a getter/setter is invoked (closes test_getters_setters/test_gap_class_advanced). Added `is_class_getter`/`is_class_setter` to escape analysis.
- **v0.5.101** — Three CI parity fixes: `[] instanceof Array` (CLASS_ID_ARRAY + GC_TYPE_ARRAY byte); `>>> 0` initializers no longer seeded as i32; `arr.length` stale after `shift`/`pop` (dropped `!invariant.load`).
- **v0.5.100** — Walk Array-method HIR variants (`ArrayAt`/`ArrayEntries`/...) in `collect_ref_ids_in_expr` so escape analysis sees the candidate ID. gap_array_methods DIFF(22)→DIFF(4).

Older entries → CHANGELOG.md.
