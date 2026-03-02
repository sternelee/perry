# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and Cranelift for code generation.

**Current Version:** 0.2.168

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
