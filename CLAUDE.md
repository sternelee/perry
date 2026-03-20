# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and Cranelift for code generation.

**Current Version:** 0.2.197

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
| **perry** | CLI driver |
| **perry-parser** | SWC wrapper for TypeScript parsing |
| **perry-types** | Type system definitions |
| **perry-hir** | HIR data structures (`ir.rs`) and AST→HIR lowering (`lower.rs`) |
| **perry-transform** | IR passes (closure conversion, async lowering, inlining) |
| **perry-codegen** | Cranelift-based native code generation |
| **perry-runtime** | Runtime: value.rs, object.rs, array.rs, string.rs, gc.rs, arena.rs, etc. |
| **perry-stdlib** | Node.js API support (mysql2, redis, fetch, fastify, ws, etc.) |
| **perry-ui** / **perry-ui-macos** / **perry-ui-ios** | Native UI (AppKit/UIKit) |
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

## Native UI (`perry/ui`)

Declarative TypeScript compiles to AppKit/UIKit calls. 47 `perry_ui_*` FFI functions. Handle-based widget system (1-based i64 handles, NaN-boxed with POINTER_TAG). 5 reactive binding types dispatched from `state_set()`. `--target ios-simulator`/`--target ios` for cross-compilation.

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

**Implementation** (`crates/perry/src/commands/compile.rs`):
- `CompilationContext.compile_packages: HashSet<String>` — packages to compile natively
- `CompilationContext.compile_package_dirs: HashMap<String, PathBuf>` — dedup cache (first-found dir per package)
- `resolve_package_source_entry()` — prefers `src/index.ts` over `lib/index.js`
- `is_in_compile_package()` — checks if a file path is inside a listed package
- `resolve_import()` — redirects compile packages to first-found dir for dedup, marks as `NativeCompiled`
- `collect_modules()` — `.js` files inside compile packages bypass JS runtime routing

**Dedup logic**: When `@noble/hashes` appears in both `@noble/curves/node_modules/` and `@solana/web3.js/node_modules/`, the first-resolved directory is cached in `compile_package_dirs`. Subsequent imports redirect to the same copy, preventing duplicate linker symbols.

## Known Limitations

- **No runtime type checking**: Types erased at compile time. `typeof` via NaN-boxing tags. `instanceof` via class ID chain.
- **Single-threaded**: User code on one thread. Async I/O on tokio worker pool. Use `spawn_for_promise_deferred()` for safe cross-thread data transfer.

## Common Pitfalls & Patterns

### NaN-Boxing Mistakes
- **Double NaN-boxing**: If value is already F64, don't NaN-box again. Check `builder.func.dfg.value_type(val)`.
- **Wrong tag**: Strings=STRING_TAG, objects=POINTER_TAG, BigInt=BIGINT_TAG.
- **`as f64` vs `from_bits`**: `u64 as f64` is numeric conversion (WRONG). Use `f64::from_bits(u64)` to preserve bits.
- **Handle extraction**: Handle-based objects are small integers NaN-boxed with POINTER_TAG. Use `js_nanbox_get_pointer`, not bitcast.

### Cranelift Type Mismatches
- Loop counter optimization produces I32 — always convert before passing to F64/I64 functions
- Check `builder.func.dfg.value_type(val)` before conversion; handle F64↔I64, I32→F64, I32→I64
- `is_pointer && !is_union` for variable type determination
- Constructor parameters always F64 (NaN-boxed) at signature level

### Function Inlining (inline.rs)
- `try_inline_call` returns body with `Stmt::Return` — in `Stmt::Expr` context, convert `Return(Some(e))` → `Expr(e)`
- `substitute_locals` must handle ALL expression types

### Async / Threading
- Thread-local arenas: JSValues from tokio workers invalid on main thread
- Use `spawn_for_promise_deferred()` — return raw Rust data, convert to JSValue on main thread
- Async closures: Promise pointer (I64) must be NaN-boxed with POINTER_TAG before returning as F64

### Cross-Module Issues
- ExternFuncRef values are NaN-boxed — use `js_nanbox_get_pointer` to extract
- Module init order: topological sort by import dependencies
- Optional params need `imported_func_param_counts` propagation through re-exports
- Wrapper functions: truncate `call_args` to match declared signature

### Closure Captures
- `collect_local_refs_expr()` must handle all expression types — catch-all silently skips refs
- Captured string/pointer values must be NaN-boxed before storing, not raw bitcast
- Loop counter i32 values: `fcvt_from_sint` to f64 before capture storage

### Loop Optimization
- Pattern 3 accumulator (8 f64 accumulators) — skip for string variables
- LICM: `hoisted_element_loads` + `hoisted_i32_products` cache invariant loads before inner loops
- `try_compile_index_as_i32` keeps i32 ops contained to array indexing only

### Handle-Based Dispatch
- TWO systems: `HANDLE_METHOD_DISPATCH` (methods) and `HANDLE_PROPERTY_DISPATCH` (properties)
- Both must be registered. Small pointer detection: value < 0x100000 = handle.

### UI Codegen
- NativeMethodCall has TWO arg paths: has-object and no-object — don't mix them up
- `js_object_get_field_by_name_f64` (NOT `js_object_get_field_by_name`) for runtime field extraction

### objc2 v0.6 API
- `define_class!` with `#[unsafe(super(NSObject))]`, `msg_send!` returns `Retained` directly
- All AppKit constructors require `MainThreadMarker`
- `CGPoint`/`CGSize`/`CGRect` in `objc2_core_foundation`

## Recent Changes

### v0.2.197
- **Cross-platform `menuClear` + `menuAddStandardAction` + Windows RefCell fix**: add `perry_ui_menu_clear` and `perry_ui_menu_add_standard_action` FFI to all 6 platforms (were macOS-only); fix `dispatch_menu_item` RefCell re-entrancy panic on Windows (extract callback before calling, matching macOS fix from v0.2.196); update web/WASM runtimes and feature parity test matrix

### v0.2.196
- **Fix `perry publish` showing wrong platform for Windows/Web**: `target_display` match was missing `"windows"` and `"web"` cases (fell through to `"macOS"`); also fix `is_macos` flag to exclude Windows/Web so they don't trigger macOS-specific signing/notarization logic

### v0.2.195
- **Documentation: comprehensive perry.toml reference**: `docs/src/cli/perry-toml.md` — every section (`[project]`, `[app]`, `[build]`, `[macos]`, `[ios]`, `[android]`, `[linux]`, `[publish]`, `[audit]`, `[verify]`), all fields with types/defaults, bundle ID resolution order, entry file resolution, build number auto-increment, distribution modes, environment variables, global config (`~/.perry/config.toml`), CI/CD example; linked from SUMMARY.md, project-config.md, and commands.md
- **Documentation: comprehensive geisterhand reference**: rewrote `docs/src/testing/geisterhand.md` — full API reference (all 14 HTTP endpoints with request/response formats), widget type and callback kind tables, platform setup for all 5 platforms + iOS device, test automation patterns (shell scripts, Python, CI pipelines, visual regression, chaos stress testing), architecture diagram, thread safety model, NaN-boxing bridge details, build system (auto-build, feature flags, manual cross-compilation, separate target dir), troubleshooting guide

### v0.2.194
- **CLI: platform as positional arg for `run` and `publish`**: `perry run ios`, `perry publish macos` instead of `--ios`/`--macos` flags — platform is the primary selector, not a composable modifier; shared `Platform` ValueEnum (macos/ios/android/linux/windows/web); `perry compile --target` unchanged (file is the primary arg there); updated docs and error messages

### v0.2.193
- **Fix bundle ID not reading from perry.toml**: `perry publish macos` (and all platforms) now reads `[app].bundle_id` from perry.toml — previously `AppConfig` struct was missing the `bundle_id` field so `[app].bundle_id` was silently ignored, falling through to default `com.perry.<name>`; also fixed `perry compile --target ios` and `perry run` to check perry.toml (`[ios]`, `[macos]`, `[app]`, `[project]`) before `package.json`

### v0.2.192
- **Configurable geisterhand port**: `--geisterhand-port <PORT>` CLI flag for both `perry compile` and `perry run` (implies `--enable-geisterhand`); port propagated through CompilationContext → Compiler → Cranelift codegen; default remains 7676; also forwarded in remote build manifests
- **Geisterhand docs update**: all 5 native platforms documented (added Linux/GTK4 and Windows sections); custom port usage; testing flags section added to CLI reference

### v0.2.191
- **Geisterhand: in-process input fuzzer for Perry UI**: `--enable-geisterhand` flag embeds HTTP server (port 7676) for programmatic widget interaction; `perry-ui-geisterhand` crate (tiny-http server, chaos mode); global callback registry + main-thread dispatch queue in `perry-runtime/geisterhand_registry.rs` behind `#[cfg(feature = "geisterhand")]`; widget registration in all 5 native platform crates (macOS/iOS/Android/GTK4/Windows) for button, textfield, slider, toggle, picker, menu, click/hover/doubleclick; pump integration in each platform's timer; HTTP endpoints: `/widgets`, `/click/:h`, `/type/:h`, `/slide/:h`, `/toggle/:h`, `/state/:h`, `/hover/:h`, `/doubleclick/:h`, `/screenshot`, `/chaos/start|stop|status`; cross-thread screenshot via Condvar sync; built via `CARGO_TARGET_DIR=target/geisterhand`
- **Screenshot capture all 5 platforms**: macOS (CGWindowListCreateImage→NSBitmapImageRep→PNG, reads from APPS not WINDOWS), iOS (UIGraphicsImageRenderer→UIImagePNGRepresentation), Android (JNI View.draw→Bitmap.compress PNG), GTK4 (WidgetPaintable→GskRenderer.render_texture→GdkTexture.save_to_png_bytes), Windows (PrintWindow+GetDIBits→inline PNG encoder with stored zlib blocks, CRC32, Adler-32)
- **Auto-build geisterhand libs**: `--enable-geisterhand` automatically runs `cargo build` for perry-runtime, perry-ui-{platform}, perry-ui-geisterhand with correct features and cross-compilation targets when libs are missing; finds Perry workspace root by searching upward from executable; caches in `target/geisterhand/`
- **Geisterhand documentation**: `docs/src/testing/geisterhand.md` — full API reference, platform setup (macOS/iOS/Android), example app, architecture overview

### v0.2.189
- **WASM target: Firefox NaN canonicalization fix**: replace ALL bridge function calls with memory-based calling convention (`mem_call`/`mem_call_i32`); WASM writes f64 args to memory at 0xFF00 (preserves NaN-boxed payloads), JS reads via Float64Array; fixes Firefox corrupting STRING_TAG/POINTER_TAG values passed as function parameters; `__memDispatch` table routes 150+ bridge functions; `emit_bridgeN`/`emit_bridgeN_i32`/`emit_bridgeN_void` convenience methods; minimum 2 memory pages for arg buffer region

### v0.2.188
- **WASM target: full perry/ui support**: generic UI dispatcher (`ui_call`/`ui_call_method` bridge imports) routes all `perry/ui` and `perry/system` NativeMethodCalls through JS runtime; 170+ DOM-based UI functions ported from web runtime including widget creation (App, VStack, HStack, ZStack, Text, Button, TextField, SecureField, Toggle, Slider, ScrollView, Canvas, etc.), reactive State system (create/get/set/bindText/bindSlider/bindToggle/bindVisibility/bindForEach), styling (background, foreground, font, padding, border, opacity, animations), events (onClick, onHover, onDoubleClick), canvas drawing (fillRect, strokeRect, paths, text), menus, toolbars, sheets, dialogs, keyboard shortcuts, navigation stacks, windows, system APIs (openURL, isDarkMode, preferences, keychain, notifications); closures bridged through WASM indirect call table via `callWasmClosure` helper

### v0.2.187
- **WASM target: complete gap fixes**: class getters/setters auto-invoked via `__get_`/`__set_` prefix dispatch in `class_get_field`/`class_set_field`; bridge exception propagation (JSON.parse, RegExp, URL errors set `currentException` when inside try/catch); setTimeout/setInterval/clearTimeout/clearInterval bridges; response property bridges (`response_status`, `response_ok`, `response_headers_get`, `response_url`); `response.json()`/`response.text()` dispatch in NativeMethodCall; Buffer `copy`/`write`/`equals`/`isBuffer`/`byteLength` implemented; `crypto.sha256` via SubtleCrypto (async), `path.isAbsolute` fixed; fetch auth headers for `FetchGetWithAuth`/`FetchPostWithAuth`; expanded JS emitter (IndexGet/Set, ArrayPush, StringCoerce, Math, typeof, delete, Sequence, ExternFuncRef); NativeMethodCall unknown-method fallback to `class_call_method`; void method return fix

### v0.2.186
- **WASM target: Phases 1-6 implementation**: full class compilation (constructors, methods, static methods, getters/setters, field initializers, inheritance via `super()`, `instanceof` with parent chain walking); multi-module `ExternFuncRef` resolution via `func_name_map`; bridge-based try/catch/finally; URL/URLSearchParams (16 bridges), crypto, path, process/OS, Buffer/Uint8Array (13 bridges); async functions compiled to JS bridge (HIR→JS emitter for async bodies), fetch/promise bridges (`fetch_url`, `fetch_with_options`, `response_json/text`, `promise_new/resolve/then`, `await_promise`); 192+ runtime bridge imports

### v0.2.185
- **WASM target: complete implementation (Phases 0-4)**: fix for-loop local scoping, modulo, bitwise ops, nullish coalescing; add handle-based object/array system with JS bridge (30+ bridge functions); closures via indirect call table with capture support; higher-order array methods (map/filter/reduce/forEach/find/sort); class instantiation, enum members, switch statements; JSON parse/stringify, Map/Set, Date, Error, RegExp, string methods (trim/split/replace/includes/etc.); 139 runtime bridge imports total; comprehensive test passing 30/30 cases including fibonacci, closures with captures, and method chaining

### v0.2.184
- **Documentation**: add WebAssembly platform page, perry-styling/theming page, `perry run` command docs, `--minify` flag docs, `gc()` built-in docs; update SUMMARY.md, platform overview, CLI commands/flags

### v0.2.183
- **WebAssembly target (`--target wasm`)**: new `perry-codegen-wasm` crate compiles HIR directly to WASM bytecode using `wasm-encoder`; NaN-boxing scheme matches native perry-runtime (f64 values with STRING_TAG/POINTER_TAG); JS runtime bridge for strings, console, Math, type operations; outputs self-contained HTML (base64-embedded WASM) or raw `.wasm` binary; supports functions, control flow, string literals, numeric ops, console.log

### v0.2.182
- **Web target minification/obfuscation**: `--target web` now auto-minifies output — Rust-native JS minifier (`minify.rs`) strips comments and collapses whitespace; emitter-level name mangling (`gen_short_name`) obfuscates local variables, parameters, and non-exported functions (a,b,c,...); web runtime compressed from 3,337 lines to ~177; `--minify` CLI flag

### v0.2.181
- **iOS keyboard avoidance**: register for `UIKeyboardWillChangeFrameNotification`, adjust root view bottom constraint with animated layout, auto-scroll focused TextField into view above keyboard; `perry run --ios` now adds `--console` for live stdout/stderr streaming and embeds app icon from perry.toml
- **Fix `RefCell already borrowed` panic in state callbacks (GH-4)**: `state_set()` now snapshots onChange/forEach callbacks into a local Vec before invoking them, releasing the RefCell borrow first — fixes crash when perry-react's `useEffect` triggers a re-render that registers new `useState` onChange handlers during callback iteration
- **Fix fetch linker error without stdlib imports (GH-5)**: `fetch()` is a global built-in (no import needed) but `js_fetch_with_options` lives in perry-stdlib — added `uses_fetch` flag to HIR Module, set during lowering, checked in compile.rs to ensure stdlib is linked when `fetch()` is used

### v0.2.180
- **`perry run` command**: compile and launch in one step — auto-detects entry file (perry.toml, src/main.ts, main.ts), platform-aware device detection (iOS simulators via simctl, devices via devicectl, Android via adb), interactive prompts with dialoguer when multiple targets found, forwards program args via `--`
- **Remote build fallback**: `perry run --ios` auto-detects missing cross-compilation toolchain and falls back to Perry Hub build server — packages project, uploads, streams build progress via WebSocket, downloads .ipa, extracts .app, installs and launches on device/simulator; `--local`/`--remote` flags to force either path

### v0.2.179
- **Public beta notice for publish/verify**: one-time interactive prompt on first `perry publish` or `perry verify` run; opt-in automatic error reporting (sanitized, no credentials/paths) via Chirp telemetry; consent stored in `~/.perry/config.toml [beta]`

### v0.2.178
- **Fix `--enable-js-runtime` linker error on Linux/WSL**: add `--allow-multiple-definition` flag for ELF linker (was already present for Android but missing for Linux)

### v0.2.177
- **Project-specific provisioning profiles**: `perry setup ios` now saves mobileprovision files as `{bundle_id}.mobileprovision` (dots replaced with underscores) instead of generic `perry.mobileprovision`, preventing multi-project overwrites

### v0.2.176
- **Anonymous telemetry**: opt-in usage statistics via Chirp API; one-time consent prompt on first interactive run, fire-and-forget background events for compile/init/publish/doctor/update; opt out via `PERRY_NO_TELEMETRY=1`, `CI=true`, or answering "no" at prompt

### v0.2.175
- **Documentation site**: mdBook-based docs (`docs/`) with 49 pages covering getting started, language features, UI widgets, 6 platforms, stdlib, system APIs, WidgetKit, plugins, CLI reference, and contributing; GitHub Pages CI workflow; `llms.txt` for LLM discoverability

### v0.2.174
- **`perry/widget` module + `--target ios-widget`**: compile TypeScript widget declarations to native SwiftUI WidgetKit extensions — `Widget({kind, render, entryFields, ...})` lowers to `WidgetDecl` HIR nodes, emitted as complete SwiftUI source (Entry struct, View, TimelineProvider, WidgetBundle) via new `perry-codegen-swiftui` crate; supports Text, VStack/HStack/ZStack, Image, Spacer, conditionals, template literals, font/color/padding/frame modifiers; generates Info.plist with `com.apple.widgetkit-extension` extension point

### v0.2.173
- **`perry publish` auto-export .p12**: auto-detect signing identity from macOS Keychain via `security find-identity`, export to temp .p12 with generated password, eliminates manual Keychain Access export step; falls back gracefully on non-macOS/non-interactive

### v0.2.172
- **Codebase refactor**: Split `codegen.rs` (40,749→1,588 lines) into 12 modules (types, util, stubs, runtime_decls, classes, functions, closures, module_init, stmt, expr) and `lower.rs` (11,320→5,421 lines) into 8 modules (analysis, enums, jsx, lower_types, lower_patterns, destructuring, lower_decl)
- Zero functionality changes — pure structural refactor for maintainability

### v0.2.171
- **Auto-update checker**: non-blocking background version check on every CLI invocation (24h cache), `perry update` for self-update (download + atomic binary replace)
- **Update sources**: checks custom server (env/config) → Perry Hub → GitHub API; opt-out via `PERRY_NO_UPDATE_CHECK=1`, `CI=true`, or `--quiet`
- **`perry doctor`**: now shows update status check (warning if newer version available)

### v0.2.170
- **FFI safety**: `catch_callback_panic` helper wraps all ObjC callback methods (timer, pump, shortcut, button, textfield, securefield, picker, toggle, slider, toolbar, table, menu, click) in `catch_unwind` — prevents `abort()` from Rust panics crossing FFI boundary
- **Accessibility**: Button and Text widgets now set `setAccessibilityLabel:` for UI automation tools
- **BigInt bitwise ops**: `js_dynamic_shr`, `js_dynamic_shl`, `js_dynamic_bitand`, `js_dynamic_bitor`, `js_dynamic_bitxor`, `js_dynamic_bitnot` — full BigInt + Number support
- **Button enhancements**: `setImage` (SF Symbols), `setContentTintColor`, `setImagePosition` — 3 new FFI functions
- **ScrollView**: pull-to-refresh (`setRefreshControl`/`endRefreshing`), flipped coordinate system for top-origin layout
- **Widget tree**: `removeChild`, `reorderChild` FFI functions for dynamic child management
- **File dialog**: `openFolderDialog` for directory selection
- **Closure ref collection**: `collect_local_refs_expr`/`collect_local_refs_stmt` now public for cross-crate use
- **Cross-platform**: Android canvas, picker, scrollview, button, callback improvements; iOS button/scrollview enhancements
- **Fetch**: extended HTTP client support in stdlib

### v0.2.169
- Type inference: `infer_type_from_expr()` infers types from literals, binary ops, variable propagation, known method returns, and user-defined function return types — eliminates `Type::Any` for common patterns (`let x = 5` → `Number`, `let s = "hi".trim()` → `String`, etc.)
- `--type-check` flag: optional tsgo IPC integration (Microsoft's native TS checker) resolves cross-file types, interfaces, and generics via msgpack protocol over stdio — graceful fallback if tsgo not installed

### v0.2.168
- Native application menu bars: `menuBarCreate`, `menuBarAddMenu`, `menuBarAttach`, `menuAddSeparator`, `menuAddSubmenu`, `menuAddItem` with optional keyboard shortcut (4th arg)
- macOS (NSMenu), Windows (HMENU/SetMenu), GTK4 (GMenu/set_menubar), Web (DOM), iOS/Android stubs — 6 new FFI functions across all 6 platforms

### v0.2.167
- `perry.compilePackages`: compile pure TS/JS npm packages natively instead of V8 — configured in package.json, prefers TS source over compiled JS, deduplicates across nested node_modules to prevent duplicate linker symbols

### v0.2.166
- `packages/perry-styling`: first-party design system bridge — token codegen CLI (`perry-styling generate --tokens tokens.json --out theme.ts`), typed `PerryTheme`/`ResolvedTheme`, ergonomic flat-primitive styling helpers (`applyBg`, `applyRadius`, `applyBorderColor`, `applyGradient`, etc.), compile-time platform constants (`isMac`, `isMobile`, etc. via `__platform__`)
- New FFI: `perry_ui_widget_set_border_color`, `perry_ui_widget_set_border_width`, `perry_ui_widget_set_edge_insets`, `perry_ui_widget_set_opacity` (macOS; stubs needed on other platforms)

### v0.2.165
- Background process management: `child_process.spawnBackground(cmd, args, logFile, envJson?)` → `{pid, handleId}`, `getProcessStatus(handleId)` → `{alive, exitCode}`, `killProcess(handleId)` — non-blocking process spawning with global registry
- Binary file read: `fs.readFileBuffer(path)` → Buffer (binary-safe, uses `fs::read()` not `read_to_string`)
- Recursive directory removal: `fs.rmRecursive(path)` → boolean (uses `fs::remove_dir_all`)
- `__platform__` compile-time constant: `declare const __platform__: number` in any module emits an i64 constant (0=macOS,1=iOS,2=Android,3=Windows,4=Linux) determined at compile time; Cranelift constant-folds comparisons and eliminates dead branches — enables zero-cost platform branching

### v0.2.164
- `perry publish`: auto-register free license on first use (no `--register`/`--github-token` flags needed); sends empty JSON to `/api/v1/license/register`
- Remove debug logging from runtime: `eprintln!` in `js_fs_read_file_sync`, `READ_FILE_TRACE` atomic, verbose `js_nanbox_string` trace

### v0.2.163
- Table widget: NSTableView-backed `Table(rowCount, colCount, renderFn)` with column headers, widths, row selection
- `setColumnHeader(col, title)`, `setColumnWidth(col, width)`, `updateRowCount(count)`, `setOnRowSelect(cb)`, `getSelectedRow()` instance methods
- macOS: full NSTableView + NSScrollView implementation with delegate (numberOfRows, viewForColumn, selectionDidChange)
- Web: DOM `<table>` implementation with header cells, click-to-select, same API
- iOS/Android/GTK4/Windows: stubs returning 0/-1 (pending native implementation)
- HIR: "Table" registered as native instance type; codegen: 6 extern declarations + dispatch + arg handling
- Fix pre-existing duplicate `perry_ui_widget_clear_children` / `perry_ui_widget_add_child_at` in perry-ui-windows

### v0.2.162
- Web platform full feature parity: 60 new JS functions (67→127/127, 100%), all 6 platforms now fully covered
- Web: app lifecycle (timer, activate/terminate), multi-window (floating divs), state wrappers, lazy VStack, sheets, toolbar, context menus
- Web: keychain (localStorage), notifications (Notification API), clipboard, dialogs (file/save/alert), keyboard shortcuts, canvas gradient
- Fix GTK4 compilation: add cairo-rs dep, add prelude imports for Cast/ApplicationExt, use load_from_data for CssProvider

### v0.2.161
- Android full feature parity: 62 new functions via JNI (50→112/112), all 5 native platforms now at 88%
- JNI widgets: SecureField (EditText+ES_PASSWORD), ProgressBar, Spinner+ArrayAdapter, Canvas (Bitmap), FrameLayout (ZStack/NavStack), ImageView
- Dialogs: alert (PerryBridge.showAlert), sheets (Dialog modal), multi-window (Dialog), toolbar (horizontal LinearLayout)
- System APIs: open_url (Intent.ACTION_VIEW), isDarkMode (Configuration.uiMode), SharedPreferences, keychain, notifications
- Property setters, ViewPropertyAnimator (opacity/position), state onChange/textfield binding, Typeface font family

### v0.2.160
- Windows full feature parity: 62 new functions implemented (50→112/112), matching macOS/iOS/GTK4 coverage
- Win32 widgets: SecureField (ES_PASSWORD), ProgressView (PROGRESS_CLASSW), Form/Section (GroupBox), ZStack, Picker (ComboBox), Canvas (GDI), NavStack, LazyVStack, Image
- Dialogs: save file dialog (IFileSaveDialog), alert (MessageBoxW), sheets, multi-window, toolbar
- System APIs: open_url (ShellExecuteW), dark mode (Registry), preferences (Registry), keychain (CredWrite/Read/Delete), notifications
- Property setters, state onChange/textfield binding, font family, app lifecycle (timer, activate, terminate)

### v0.2.159
- GTK4 full feature parity: 62 new functions implemented (50→112/112), matching macOS/iOS coverage
- New widgets: SecureField, ProgressView, Form/Section, ZStack, Picker, Canvas, NavStack, LazyVStack, Image
- Dialogs: save file dialog, alert (MessageDialog), sheets (modal windows), multi-window support
- Toolbar (HeaderBar), system APIs (open_url, dark mode, preferences, keychain, notifications)
- Property setters: enabled, tooltip, control size, corner radius, background color/gradient, hover, double-click, opacity/position animation
- State: onChange callbacks, two-way textfield binding, font family support

### v0.2.158
- Cross-platform feature parity test suite: `perry-ui-test` crate with source-scanning symbol verification for all 6 platforms (macOS, iOS, Android, GTK4, Windows, Web)
- 127-entry feature matrix, per-platform FFI parity tests, coverage report (`PERRY_PRINT_MATRIX=1`), untracked symbol warnings

### v0.2.157
- 12 new UI/system features: saveFileDialog, Alert, Sheet, Toolbar, LazyVStack, Window (multi-window), State↔TextField binding
- App lifecycle hooks (onTerminate, onActivate), Keychain (Security.framework), local notifications (UNUserNotificationCenter)
- setFontFamily implementation (monospaced + named fonts), string-aware preferencesSet/Get (NSString ↔ NaN-boxed strings)

### v0.2.156
- `--target web`: new perry-codegen-js crate emits JavaScript from HIR, producing self-contained HTML files
- Web runtime maps perry/ui widgets to DOM elements (flexbox, CSS), State to reactive JS objects
- Skips Cranelift/inline/generator transforms for web; JS engines handle closures, async, generators natively

### v0.2.155
- 20+ new UI widgets and APIs: SecureField, ProgressView, Image, Picker, Form/Section, NavigationStack, ZStack (both macOS + iOS)
- Cross-cutting widget APIs: setEnabled, setOnHover, setOnDoubleClick, animateOpacity, animatePosition, setTooltip, setControlSize, setFontFamily
- `perry/system` module: openURL, isDarkMode, preferencesSet/Get (NSUserDefaults/UIDefaults)
- State onChange callbacks, string-aware state binding (NaN-boxed STRING_TAG values)
- Fix `delete obj[stringKey]` verifier error: NaN-box string pointer before passing to `js_object_delete_dynamic`

### v0.2.153
- Automatic binary size reduction: detect stdlib needs from imports, link runtime-only when possible (0.3MB vs 48MB for hello world)
- Move JSON functions (parse/stringify) from perry-stdlib to perry-runtime so JSON works without stdlib
- Dead code stripping (`-Wl,-dead_strip` / `--gc-sections`) and automatic `strip` on final binary
- `requires_stdlib()` in ir.rs maps native module imports to stdlib vs runtime-only

### v0.2.151
- Generic plugin system v2: hook priority (lower=first), 3 hook modes (filter/action/waterfall), ABI v2
- Plugin metadata: `setMetadata(name, version, description)`, displayed on load and in `listPlugins()`
- Event bus: `api.on(event, handler)` / `api.emit(event, data)` / `emitEvent(event, data)` for plugin communication
- Tool invocation: `invokeTool(name, args)` calls plugin-registered tools from host
- Introspection: `listPlugins()`, `listHooks()`, `listTools()` return arrays of registered items
- Config system: `setConfig(key, value)` (host) / `api.getConfig(key)` (plugin)
- Fix: `plugin_activate` now calls user's `activate(api)`, `plugin_deactivate` calls `deactivate()`

### v0.2.150
- Native plugin system: `--output-type dylib` compiles plugins to .dylib/.so shared libraries
- Plugin runtime: `perry/plugin` module with PluginRegistry, dlopen/dlclose, hook dispatch, tool/service/route registration
- Plugin entry points: `plugin_activate(api_handle)` / `plugin_deactivate()` / `perry_plugin_abi_version()` codegen
- Host support: `loadPlugin()`, `discoverPlugins()`, `emitHook()` host-side functions, `-rdynamic` for symbol export

### v0.2.149
- `string.match()` support: add HIR lowering for `.match(regex)` calls, fix NaN-boxing of string elements in match result arrays
- `regex.test()` verified end-to-end with inline and variable regex patterns
- Object destructuring verified: shorthand, rename, defaults, rest patterns all working
- Method chaining verified: `arr.filter().map()`, `arr.map().reduce()` work correctly
- Utility type erasure verified: Partial, Pick, Record, Omit, ReturnType, Readonly all erased at compile time

### v0.2.148
- `Array.from()` support: new `ArrayFrom` HIR node, `js_array_clone` + `js_set_to_array` runtime functions
- Singleton pattern: track `static_method_return_types` in ClassMeta for proper type inference (e.g., `getInstance()`)
- Multi-module class ID management: thread `next_class_id` through module collection to prevent collisions
- Array mutation on properties: `this.arr.push()` / `obj.arr.push()` stores new pointer back to field
- `js_array_unshift_jsvalue` runtime function for NaN-boxed unshift support
- Map/Set NaN-boxing fixes: robust cross-tag string equality in `jsvalue_eq()`, strict STRING_TAG for map keys
- Fix closure return type: NaN-box pointer (not bitcast) when closure returns I64 for F64 function
- Fix string/boolean confusion: additional `is_string` check in binary ops prevents treating booleans as string pointers
- Native module overridability: user-defined classes can shadow builtins (EventEmitter, Redis, WebSocket, etc.)
- Remove hot-path debug logging: disabled DYNAMIC-ARRAY-GET/LENGTH, ARRAY-IS-ARRAY, HAS-PROP, OBJECT-SET-FIELD eprintln

### v0.2.147
- Mark-sweep garbage collection: gc.rs, arena_alloc_gc, conservative stack scan, root scanning, `gc()` built-in
- `js_object_free()`/`js_promise_free()` now no-ops (GC handles deallocation)

### v0.2.143-v0.2.146
- Fix fs.readFileSync SIGSEGV: accept NaN-boxed f64 instead of raw pointers
- Fix i64→f64 type mismatches in NativeMethodCall args and cross-module calls (inline_nanbox_pointer)
- Fix duplicate symbol linker errors with jsruntime stub generation

### v0.2.140-v0.2.142
- Shape-cached object allocation (5-6x faster object creation)
- Inline NaN-box string ops (2x faster string concat vs Node)
- i32 shadow variables for integer function parameters

### v0.2.135-v0.2.139
- Module-scoped cross-module symbols for 183+ module projects
- Stub generation for unresolved npm dependencies
- iOS support: perry-ui-ios crate + `--target ios-simulator` CLI flag
- Fix keyboard shortcuts before App(), arena crash on >8MB allocations

### v0.2.126-v0.2.134
- Perry UI Phase A: 24 FFI functions (styling, scrolling, clipboard, keyboard shortcuts, menus)
- UI widgets: Spacer, Divider, TextField, Toggle, Slider
- Reactive bindings: multi-state text, two-way binding, conditional rendering, ForEach
- Inline truthiness checks (eliminate js_is_truthy FFI), LICM for nested loops
- clearTimeout, fileURLToPath, cross-module enum exports, worker_threads module

### Older (v0.2.37-v0.2.125)
See CHANGELOG.md for detailed history. Key milestones:
- v0.2.116: Native UI module (perry/ui)
- v0.2.115: Integer function specialization (fibonacci 2x faster than Node)
- v0.2.102: Topological module init ordering
- v0.2.79: Fastify-compatible HTTP runtime
- v0.2.49: First production worker (MySQL, LLM APIs, scoring)
- v0.2.37: NaN-boxing foundation
