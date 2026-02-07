# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and Cranelift for code generation.

**Current Version:** 0.2.133

## Workflow Requirements

**IMPORTANT:** Follow these practices for every code change:

1. **Update CLAUDE.md**: After making any code changes, update this file to document:
   - New features or fixes in the "Recent Changes" section
   - Any new patterns, APIs, or important implementation details
   - Changes to build commands or architecture

2. **Increment Version**: Bump the version number with every change:
   - Use patch increments (e.g., 0.2.40 → 0.2.41) for bug fixes and small changes
   - Use minor increments (e.g., 0.2.x → 0.3.0) for new features
   - Update the "Current Version" field at the top of this file

3. **Commit Changes**: Always commit after completing a change:
   - Include both the code changes and CLAUDE.md updates in the same commit
   - Use descriptive commit messages that summarize the change

## Build Commands

```bash
# Build all crates (release)
cargo build --release

# Build the runtime library (required for linking)
cargo build --release -p perry-runtime

# Run tests
cargo test

# Run tests for a specific crate
cargo test -p perry-hir

# Check / format / lint
cargo check
cargo fmt
cargo clippy
```

## Compiling TypeScript

```bash
# Compile a TypeScript file to executable
cargo run --release -- test_factorial.ts -o factorial

# Print HIR for debugging
cargo run --release -- test_factorial.ts --print-hir

# Produce object file only (no linking)
cargo run --release -- test_factorial.ts --no-link
```

## Architecture

```
TypeScript (.ts) → Parse (SWC) → AST → Lower → HIR → Transform → Codegen (Cranelift) → .o → Link (cc) → Executable
```

### Crate Structure

| Crate | Purpose |
|-------|---------|
| **perry** | CLI driver that orchestrates the pipeline |
| **perry-parser** | SWC wrapper for TypeScript parsing |
| **perry-types** | Type system definitions (Void, Boolean, Number, String, Array, Object, Function, Union, Promise, etc.) |
| **perry-hir** | HIR data structures (`ir.rs`) and AST→HIR lowering (`lower.rs`) |
| **perry-transform** | IR transformation passes (closure conversion, async lowering, inlining) |
| **perry-codegen** | Cranelift-based native code generation |
| **perry-runtime** | Runtime library linked into executables (`value.rs`, `object.rs`, `array.rs`, `string.rs`, `bigint.rs`, `closure.rs`, `promise.rs`, `builtins.rs`) |
| **perry-stdlib** | Standard library — Node.js API support (mysql2, redis, fetch, fastify, ws, etc.) |
| **perry-ui** | Platform-agnostic UI types (WidgetHandle, WidgetKind, StateId) |
| **perry-ui-macos** | macOS AppKit UI backend (NSWindow, NSButton, NSTextField, NSStackView) |
| **perry-jsruntime** | JavaScript interop via QuickJS |

### Key Data Flow

1. `perry_parser::parse_typescript()` produces SWC's `Module` AST
2. `perry_hir::lower_module()` converts AST to typed HIR with unique IDs
3. `perry_codegen::Compiler::compile_module()` generates native object code
4. System linker (`cc`) links object file with `libperry_runtime.a`

### HIR Structure

The HIR (`crates/perry-hir/src/ir.rs`) represents a simplified, typed intermediate form:
- **Module**: Contains globals, functions, classes, and init statements
- **Function**: Name, params with types, return type, body, async flag
- **Class**: Name, fields, constructor, instance/static methods
- **Statement**: Let, Expr, Return, If, While, For, Break, Continue, Throw, Try
- **Expression**: Literals, variable access (LocalGet/Set, GlobalGet/Set), operations, calls, object/array literals

## NaN-Boxing Implementation

Perry uses NaN-boxing to represent JavaScript values efficiently in 64 bits. Key tag constants in `perry-runtime/src/value.rs`:

```rust
TAG_UNDEFINED = 0x7FFC_0000_0000_0001
TAG_NULL      = 0x7FFC_0000_0000_0002
TAG_FALSE     = 0x7FFC_0000_0000_0003
TAG_TRUE      = 0x7FFC_0000_0000_0004
BIGINT_TAG    = 0x7FFA_0000_0000_0000  // BigInt pointer (lower 48 bits)
STRING_TAG    = 0x7FFF_0000_0000_0000  // String pointer (lower 48 bits)
POINTER_TAG   = 0x7FFD_0000_0000_0000  // Object/Array pointer (lower 48 bits)
INT32_TAG     = 0x7FFE_0000_0000_0000  // Int32 value (lower 32 bits)
```

### Key Runtime Functions

- `js_nanbox_string(ptr)` / `js_nanbox_pointer(ptr)` / `js_nanbox_bigint(ptr)` — Wrap pointers with tags
- `js_nanbox_get_pointer(f64)` — Extract object/array pointer from NaN-boxed value
- `js_nanbox_get_bigint(f64)` — Extract BigInt pointer
- `js_get_string_pointer_unified(f64)` — Extract raw pointer from NaN-boxed or raw string
- `js_jsvalue_to_string(f64)` — Convert any NaN-boxed value to string
- `js_is_truthy(f64)` — Proper JavaScript truthiness semantics

### Module-Level Variables

- **Strings**: Stored as F64 (NaN-boxed with STRING_TAG), NOT I64 raw pointers
- **Arrays/Objects**: Stored as I64 (raw pointers)
- Functions access module variables via `module_var_data_ids` mapping

## Promise System

Promises use closure-based callbacks (`ClosurePtr`) instead of raw function pointers:

```rust
pub type ClosurePtr = *const crate::closure::ClosureHeader;
pub struct Promise {
    state: PromiseState,
    value: f64,
    reason: f64,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
    next: *mut Promise,
}
```

Callbacks are invoked via `js_closure_call1(closure, value)` which properly passes the closure environment.

## Native UI Architecture (`perry/ui`)

Perry supports native macOS GUI apps via `perry/ui`. Declarative TypeScript compiles to AppKit calls — no Electron, no WebView.

### TypeScript API

```typescript
import { App, VStack, HStack, Text, Button, State, Spacer, Divider, TextField, Toggle, Slider, ForEach } from "perry/ui"

const count = State(0)
const dark = State(0)

App({
    title: "Counter",
    width: 400,
    height: 300,
    body: VStack(16, [
        Text(`Count: ${count.value}`),
        Button("Increment", () => count.set(count.value + 1)),
        Slider(0, 100, count.value, (val: number) => count.set(val)),  // two-way binding
        dark.value ? Text("Dark ON") : Text("Dark OFF"),               // conditional rendering
        Toggle("Dark mode", (on: boolean) => dark.set(on ? 1 : 0)),
        ForEach(count, (i: number) => Text(`Item ${i}`)),              // dynamic list
        Divider(),
        TextField("Enter name", (text: string) => { console.log(text) }),
        Spacer(),
    ])
})
```

### Pipeline

```
TypeScript: import { Text, Button } from "perry/ui"
  → HIR: "perry/ui" in NATIVE_MODULES → NativeMethodCall { module: "perry/ui", method: "Text" }
  → Codegen: Special dispatch → calls perry_ui_* FFI functions
  → Linker: Detects perry/ui import → links libperry_ui_macos.a + AppKit framework
  → Runtime: perry-ui-macos uses objc2 for NSWindow, NSStackView, NSTextField, NSButton
```

### Handle-Based Widget System

All UI objects are stored in thread-local `Vec`s and referenced by **1-based i64 handles**. Handles are NaN-boxed with `POINTER_TAG` from generic `NativeMethodCall` codegen — callers must use `js_nanbox_get_pointer(f64) -> i64` to extract raw handles.

### Reactive State Binding

Perry/ui supports 5 types of reactive bindings, all dispatched from `state_set()`:

1. **Single-state text**: `Text("Count: " + state.value)` → `perry_ui_state_bind_text_numeric` (prefix/suffix)
2. **Multi-state text**: `` Text(`${a.value} + ${b.value}`) `` → `perry_ui_state_bind_text_template` (template parts)
3. **Two-way binding**: `Slider(0, 10, state.value, cb)` → `perry_ui_state_bind_slider` (slider position tracks state)
4. **Conditional rendering**: `state.value ? WidgetA : WidgetB` → `perry_ui_state_bind_visibility` (show/hide)
5. **Dynamic lists**: `ForEach(state, (i) => Widget)` → `perry_ui_for_each_init` (clear + rebuild on change)

### FFI Surface (all `#[no_mangle] pub extern "C"`)

| Function | Signature | Description |
|----------|-----------|-------------|
| `perry_ui_app_create` | `(title: i64, w: f64, h: f64) -> i64` | Create NSWindow |
| `perry_ui_app_set_body` | `(app: i64, root: i64)` | Set root widget with Auto Layout |
| `perry_ui_app_run` | `(app: i64)` | Run NSApplication event loop |
| `perry_ui_text_create` | `(text_ptr: i64) -> i64` | Create NSTextField label |
| `perry_ui_button_create` | `(label: i64, on_press: f64) -> i64` | Create NSButton with closure |
| `perry_ui_vstack_create` / `hstack` | `(spacing: f64) -> i64` | Create NSStackView |
| `perry_ui_widget_add_child` | `(parent: i64, child: i64)` | Add child to container |
| `perry_ui_state_create` | `(initial: f64) -> i64` | Create reactive state cell |
| `perry_ui_state_get` / `set` | `(state: i64) -> f64` / `(state: i64, val: f64)` | Read/write state |
| `perry_ui_state_bind_text_numeric` | `(state: i64, text: i64, prefix: i64, suffix: i64)` | Bind text to state |
| `perry_ui_spacer_create` | `() -> i64` | Create flexible spacer view |
| `perry_ui_divider_create` | `() -> i64` | Create horizontal separator (NSBox) |
| `perry_ui_textfield_create` | `(placeholder: i64, on_change: f64) -> i64` | Create editable text field |
| `perry_ui_toggle_create` | `(label: i64, on_change: f64) -> i64` | Create switch + label |
| `perry_ui_slider_create` | `(min: f64, max: f64, initial: f64, on_change: f64) -> i64` | Create horizontal slider |
| `perry_ui_state_bind_slider` | `(state: i64, slider: i64)` | Two-way bind slider to state |
| `perry_ui_state_bind_toggle` | `(state: i64, toggle: i64)` | Two-way bind toggle to state |
| `perry_ui_state_bind_text_template` | `(text: i64, num_parts: i32, types: i64, values: i64)` | Multi-state text template binding |
| `perry_ui_state_bind_visibility` | `(state: i64, show: i64, hide: i64)` | Conditional visibility binding |
| `perry_ui_set_widget_hidden` | `(handle: i64, hidden: i64)` | Show/hide widget |
| `perry_ui_for_each_init` | `(container: i64, state: i64, closure: f64)` | Init dynamic list with ForEach |
| `perry_ui_widget_clear_children` | `(handle: i64)` | Remove all children from container |

### How to Add a New Widget

Changes required in **4 places**:

1. **Runtime** (`crates/perry-ui-macos/src/widgets/`): Create widget file, register with `register_widget(view)`
2. **FFI** (`crates/perry-ui-macos/src/lib.rs`): Add `#[no_mangle] pub extern "C" fn perry_ui_<widget>_create(...) -> i64`
3. **Codegen** (`crates/perry-codegen/src/codegen.rs`):
   - Declare extern function in `declare_runtime_functions` (search for `perry_ui_`)
   - Add dispatch entry in `NativeMethodCall` match (search for `"perry/ui"`)
   - Simple widgets: `("perry/ui", false, "Widget") => "perry_ui_widget_create"`
   - Container widgets: Follow VStack/HStack pattern for children array handling
4. **HIR** (`crates/perry-hir/src/lower.rs`): Only if widget has instance methods — register as native instance class like State

Build: `cargo build --release -p perry-ui-macos`. Non-UI programs are unaffected — UI libs only linked when `perry/ui` is imported.

## Known Working Features

- Arithmetic, comparisons, logical operators, variables, constants, type annotations
- Functions (regular, async, arrow, closures with up to 8 args)
- Classes with constructors, methods, inheritance
- Arrays with methods (push, pop, map, filter, find, join, reduce, etc.)
- Objects with property access (dot and bracket notation)
- Template literals with interpolation
- Promises (.then, .catch, .finally, Promise.all, Promise.resolve, Promise.reject)
- async/await with proper rejection propagation
- try/catch/finally
- fetch() with custom headers
- Multi-module compilation with imports/exports/re-exports
- Native modules: mysql2, ioredis, ws, fastify, ethers, pg, async_hooks, and more
- Native macOS UI via `perry/ui`
- BigInt with arithmetic and comparisons

## Known Limitations

### No Garbage Collection
Uses a **bump arena allocator** (`crates/perry-runtime/src/arena.rs`). Memory is never freed — arena grows in 8MB blocks. Best suited for short-running programs. `process.memoryUsage()` available to monitor.

### No Runtime Type Checking
TypeScript types are **erased at compile time**. `as` casts are no-ops. `typeof` works via NaN-boxing tag inspection. `instanceof` works for class instances via class ID chain. No runtime enforcement of interfaces or generics.

### Single-Threaded
User code runs on a single thread. Async I/O runs on a 4-thread tokio worker pool. Promise callbacks always execute on the main thread. Thread-local arenas mean JSValues cannot be shared between threads — use `spawn_for_promise_deferred()` for safe cross-thread data transfer.

## Cross-Platform Development

- **GitHub Actions**: Templates in `templates/github-actions/` (ci.yml, release.yml) — copy to `.github/workflows/` to activate
- **Docker**: `Dockerfile` (multi-stage), `Dockerfile.dev` (development), `docker-compose.yml` (with MySQL, Redis, PostgreSQL)
- See `docs/CROSS_PLATFORM.md` for details

## Test Files

Root-level `test_*.ts` files serve as integration tests. Compile and run: `cargo run --release -- test_factorial.ts && ./test_factorial`

## Debugging Tips

1. **Print HIR**: `--print-hir` shows the intermediate representation
2. **Keep object files**: `--keep-intermediates` to inspect .o files
3. **Check value types**: NaN-boxed values can be inspected by their bit patterns
4. **Module init order**: Entry module calls `_perry_init_*` for each imported module

## Common Pitfalls & Patterns

These are recurring issues encountered during development. Check these first when debugging.

### NaN-Boxing Mistakes
- **Double NaN-boxing**: If value is already F64 (NaN-boxed), don't NaN-box again. Check `builder.func.dfg.value_type(val)` first.
- **Wrong tag**: Strings use STRING_TAG, objects use POINTER_TAG, BigInt uses BIGINT_TAG.
- **`as f64` vs `from_bits`**: Rust `u64 as f64` is numeric conversion (corrupts NaN-boxing). Use `f64::from_bits(u64_value)` to preserve bit patterns.
- **Handle extraction**: Handle-based objects (Fastify, ioredis, UI widgets) are small integers NaN-boxed with POINTER_TAG. Use `js_nanbox_get_pointer` to extract, not bitcast.

### Cranelift Type Mismatches
- Loop counter optimization produces I32 values — always convert before passing to functions expecting F64/I64
- Check `builder.func.dfg.value_type(val)` before conversion; handle all combinations: F64↔I64, I32→F64, I32→I64
- Variables declared as F64 (due to `is_union` flag) must not receive I64 values — check `is_pointer && !is_union`
- Constructor parameters are always F64 (NaN-boxed) at signature level, even for pointer types

### Function Inlining (inline.rs)
- `try_inline_call` returns full function body including `Stmt::Return` — in `Stmt::Expr` context, convert `Return(Some(e))` → `Expr(e)`
- `substitute_locals` must handle ALL expression types (Object, JSON, Set/Map/Array methods, Await, etc.)

### Async / Threading
- Thread-local arenas: JSValues created on tokio worker threads are invalid on main thread
- Use `spawn_for_promise_deferred()` — return raw Rust data from async block, convert to JSValue on main thread via converter that runs during `js_stdlib_process_pending()`
- Async closures: `Expr::Closure` has `is_async` field; Promise pointer (I64) must be NaN-boxed with POINTER_TAG (not bitcast) before returning as F64

### Cross-Module Issues
- ExternFuncRef values are NaN-boxed — use `js_nanbox_get_pointer` to extract, not bitcast
- Module init order: topological sort by import dependencies
- Functions with optional params: `imported_func_param_counts` propagation needed, including through re-exports
- Wrapper functions: truncate `call_args` to match declared signature when caller provides excess args

### Closure Captures
- `collect_local_refs_expr()` must explicitly handle all expression types — catch-all patterns silently skip variable references
- Captured string/pointer values must be NaN-boxed before storing (`js_nanbox_string`/`js_nanbox_pointer`), not raw bitcast
- Loop counter i32 values must be converted to f64 via `fcvt_from_sint` before capture storage

### Loop Optimization
- Generic accumulator optimization (Pattern 3: `x = x + f(i)`) creates 8 f64 accumulators — skip for string variables
- While-loop unrolling disabled (bloated i-cache); CSE optimization (x*x caching) still active
- Const propagation: `const` numeric literals emit `f64const` inline instead of variable loads
- Array pointer caching: `cached_array_ptr` hoists `js_nanbox_get_pointer` out of for-loops
- i32 index arithmetic: `try_compile_index_as_i32` keeps integer ops contained to array indexing only
- LICM: `hoisted_element_loads` caches invariant `arr[outer_idx]` loads before inner loops
- LICM: `hoisted_i32_products` caches invariant `a * b` i32 products before inner loops
- LICM detection functions defined before `if can_unroll` so both paths can use them

### Handle-Based Dispatch
- TWO dispatch systems: `HANDLE_METHOD_DISPATCH` (method calls) and `HANDLE_PROPERTY_DISPATCH` (property access)
- Both must be registered for handles to work fully
- Small pointer detection: value < 0x100000 indicates handle, not real pointer

### UI Codegen (perry/ui)
- NativeMethodCall has TWO arg-passing paths: has-object (appends to `call_args` starting with handle) and no-object (builds complete `call_args`)
- For has-object methods like `State.set()`, add args in the has-object chain, NOT in the no-object section
- Use `js_object_get_field_by_name_f64` (NOT `js_object_get_field_by_name`) for runtime field extraction
- StringHeader format: `{ length: u32, capacity: u32 }` + data (not null-terminated) — use `str_from_header` helper

### objc2 v0.6 API (perry-ui-macos)
- `define_class!` (not `declare_class!`) with `#[unsafe(super(NSObject))]`
- `Retained::cast_unchecked` (not `Retained::cast`)
- `msg_send!` returns `Retained` directly (not `msg_send_id!`)
- All AppKit constructors require `MainThreadMarker`
- `CGPoint`/`CGSize`/`CGRect` are in `objc2_core_foundation`

## Recent Changes

### v0.2.133
- Move array allocation from system malloc to arena bump allocator
  - `js_array_alloc()` now uses `arena_alloc()` instead of `std::alloc::alloc()`
  - `js_array_grow()` allocates new block from arena + `ptr::copy_nonoverlapping` (no `realloc`)
  - Eliminates per-array malloc overhead — bump allocation is a single pointer increment
  - **object_create benchmark: 13-14ms → 2-3ms (5x faster, now 2-3x faster than Node's 6-7ms)**
- Fix `new Array(n)` pre-allocation — `arr[i] = val` now hits in-bounds fast path
  - New `js_array_alloc_with_length(n)` sets both `length = n` and `capacity = n`
  - Previous `js_array_alloc(n)` set `length = 0`, so `arr[i]` always took the slow extend path
  - Codegen: `new Array(n)` emits `js_array_alloc_with_length` (single-arg), `new Array()` and `new Array(a,b,c)` still use `js_array_alloc`
  - `arr.length` now correctly returns `n` after `new Array(n)`

### v0.2.132
- Advanced Reactive UI Phase 4: Multi-state text, two-way binding, conditional rendering, dynamic lists
  - **Two-way binding** (4A): `Slider(0, 10, count.value, cb)` — slider position auto-updates when state changes
    - `perry_ui_state_bind_slider(state, slider)` / `perry_ui_state_bind_toggle(state, toggle)` FFI
    - `slider::set_value()` and `toggle::set_state()` runtime functions
    - `TOGGLE_SWITCHES` side map stores NSSwitch reference for programmatic state setting
    - Codegen detects `state.value` in Slider arg[2], emits bind call after widget creation
  - **Multi-state text binding** (4B): `` Text(`${a.value} + ${b.value} = ...`) `` — updates when any referenced state changes
    - `detect_text_parts()` walks Binary(Add) chains, collects `Literal` and `StateValue` parts
    - Single-state optimization: 1 state ref uses existing prefix/suffix TextBinding (faster)
    - Multi-state: 2+ state refs use `MultiTextBinding` with `TextPart::Literal`/`TextPart::StateRef` template
    - `perry_ui_state_bind_text_template(text, num_parts, types_ptr, values_ptr)` FFI
    - `MULTI_TEXT_BINDINGS` + `MULTI_TEXT_INDEX` for O(1) state→binding lookup
    - `rebuild_multi_text()` reads current state values to format template on each update
  - **Conditional rendering** (4C): `state.value ? Text("ON") : Text("OFF")` — toggles widget visibility
    - `Expr::Conditional` detection in VStack/HStack children loop
    - `Expr::Logical { And }` detection for `state.value && Widget` pattern
    - Both branches compiled and added to container; `perry_ui_state_bind_visibility` sets initial + reactive hidden state
    - `widgets::set_hidden()` wraps `setHidden:` on NSView
    - `VisibilityBinding` struct with show_handle/hide_handle; `is_truthy_f64()` for JS truthiness
  - **Dynamic lists** (4D): `ForEach(count, (i) => Text(\`Item ${i}\`))` — rebuilds children on state change
    - `ForEach` imported from perry/ui, creates VStack(0) container at compile time
    - `perry_ui_for_each_init(container, state, closure)` does initial render + registers binding
    - `ForEachBinding` stores container handle + render closure (NaN-boxed)
    - `state_set()` calls `clear_children()` then `render_for_each()` to rebuild
    - `widgets::clear_children()` removes all `arrangedSubviews` from NSStackView
    - Closure called with `js_closure_call1(closure, index)` for each 0..count
  - All binding types dispatched from `state_set()`: text, multi-text, slider, toggle, visibility, forEach
  - 8 new FFI exports in perry-ui-macos/lib.rs
  - 8 new extern declarations in codegen.rs `declare_runtime_functions`
  - New demo: `test-files/test_ui_phase4.ts`

### v0.2.131
- Eliminate js_is_truthy FFI calls from `if` statements, for-loop conditions, and while-loop conditions
  - `Stmt::If` with Compare conditions compiled directly to `fcmp` (no FFI)
  - `Stmt::If` with `&&` (Logical And) compiles each side via `compile_condition_inline`
  - `Stmt::If` with other conditions uses inline bitwise truthiness check
  - Inline truthiness: `(val - TAG_UNDEFINED) <=u 2` checks undefined/null/false, `(val << 1) == 0` checks ±0.0
  - For-loop non-BCE condition fallback: Compare → fcmp, other → inline truthiness
  - While-loop non-counter path: uses `compile_condition_inline` for And conditions, `inline_truthiness_check` for catch-all
  - New helper functions: `compile_condition_inline()` (Compare→fcmp or inline truthiness), `inline_truthiness_check()` (pure Cranelift IR, no FFI)
- Add i32 shadow variables for integer function parameters
  - At function entry, Number params (not reassigned in body) get an i32 shadow via `fcvt_to_sint`
  - `try_compile_index_as_i32` already checks `i32_shadow` — now it's populated for params
  - Avoids repeated `fcvt_to_sint` when params like `size` are used in array index arithmetic
  - Scans function body for assignments to skip reassigned params (shadow would be stale)
  - Uses `next_temp_var_id()` for shadow variable IDs (no conflicts)

### v0.2.130
- Generalized reactive state text bindings for perry/ui
  - Supports prefix+suffix patterns: `` Text(`Value: ${count.value} items`) ``
  - Supports bare state: `` Text(`${count.value}`) ``
  - Supports suffix-only: `` Text(`${count.value}!`) ``
  - Previous prefix-only pattern continues to work: `` Text(`Count: ${count.value}`) ``
  - `detect_state_in_text_arg()` recursively walks nested `Binary(Add)` chains from template literal desugaring
  - Runtime `TextBinding` now has `suffix` field; `state_set()` formats `"{prefix}{value}{suffix}"`
  - FFI `perry_ui_state_bind_text_numeric` updated to accept 4th `suffix_ptr: i64` param
  - Text widget moved from generic dispatch to special handler for standalone binding detection
  - New demo: `test-files/test_ui_state_binding.ts`

### v0.2.129
- Loop-Invariant Code Motion (LICM) for nested loops
  - **Invariant array element hoisting**: `arr[i]` inside inner j-loop is loaded once before the loop, not every iteration
    - Detects `IndexGet { LocalGet(arr), LocalGet(idx) }` where `idx` is not the loop counter and not assigned in the loop body
    - Pre-computes the element load (arr_ptr + 8 + idx*8) and caches in a Cranelift variable
    - `Expr::IndexGet` handler checks `hoisted_element_loads` map before computing inline access
  - **Invariant i32 product hoisting**: `i * size` inside inner k-loop is computed once before the loop
    - Detects `Binary { Mul, LocalGet(a), LocalGet(b) }` in array index expressions where both operands are loop-invariant
    - Pre-computes `imul(a_i32, b_i32)` and caches in a Cranelift variable
    - `try_compile_index_as_i32` checks `hoisted_i32_products` map before computing `imul`
  - Both optimizations apply to unrolled and non-unrolled for-loop paths
  - Two new fields on `LocalInfo`: `hoisted_element_loads` and `hoisted_i32_products`
  - LICM helper functions shared between unrolled and non-unrolled paths: `collect_invariant_array_loads_stmts`, `collect_assigned_ids_stmts`, `collect_invariant_products_stmts`
  - **nested_loops benchmark: ~26ms → ~21ms (19% faster, now matches Node.js ~20ms)**
  - **matrix_multiply benchmark: ~46ms → ~41ms (11% faster)**

### v0.2.128
- Add `clearTimeout` support
  - Timer callbacks now have unique IDs (via `NEXT_CALLBACK_TIMER_ID` thread-local counter)
  - `js_set_timeout_callback` returns real timer IDs instead of 0
  - `clearTimeout(timer_id)` marks timers as cleared and removes them
  - `js_callback_timer_tick` skips cleared timers
  - Added `clearTimeout` extern declaration in codegen (I64 → void)
- Add `fileURLToPath` from `url` module
  - New `FileURLToPath(Box<Expr>)` HIR expression variant
  - `js_url_file_url_to_path` runtime function strips `file://` prefix and percent-decodes
  - Added to all expression traversal functions (collect_local_refs, collect_assigned_locals, substitute_locals, collect_closures, is_string_expr)
  - `import { fileURLToPath } from 'url'` now works correctly
- Add cross-module enum exports
  - Enum definitions propagated from exporting module to importing module via `exported_enums` HashMap
  - Re-export propagation supports `export * from "./module"` chains
  - Post-lowering HIR fixup pass (`fix_imported_enums`) replaces `PropertyGet { ExternFuncRef, property }` with `EnumMember` or inlined `Expr::String`
  - `register_imported_enum` method on Compiler registers enum values for codegen lookup
  - Numeric enums emit `f64const` inline; string enums inline as `Expr::String` for proper type detection
  - Fixed pre-existing bug: string `EnumMember` values now NaN-boxed with STRING_TAG (was using raw bitcast)
- Add `worker_threads` module (parentPort, workerData)
  - `import { parentPort, workerData } from 'worker_threads'` now compiles and links
  - `workerData`: Reads `PERRY_WORKER_DATA` env var, JSON-parses it → NaN-boxed value
  - `parentPort.postMessage(data)`: JSON-stringify data, write to stdout
  - `parentPort.on('message', callback)`: Register message callback, start background stdin reader thread
  - `parentPort.on('close', callback)`: Register close callback for stdin EOF
  - `js_worker_threads_process_pending()`: Processes queued stdin messages on main thread via `js_closure_call1`
  - Integrated into `js_stdlib_process_pending()` event loop
  - `js_worker_threads_has_pending()`: Check if stdin reader is active (for keep-alive)
  - JSON functions accessed via `extern "C"` declarations (linked at link time from perry-stdlib)
  - HIR: `parentPort` auto-registered as native instance (MessagePort class) at import time
  - HIR: `workerData` resolves to `NativeMethodCall` getter (not `NativeModuleRef`)
  - Codegen: 5 extern declarations, NativeMethodCall dispatch for all methods
  - Communication protocol: One JSON message per line on stdin/stdout

### v0.2.127
- Add 5 new perry/ui widgets: Spacer, Divider, TextField, Toggle, Slider
  - **Spacer**: Transparent NSView with low content-hugging priority — stretches to fill available space in stack views
  - **Divider**: NSBox with separator type — horizontal line between sections
  - **TextField**: Editable NSTextField with placeholder string and onChange callback
    - Uses NSNotificationCenter to observe `NSControlTextDidChangeNotification`
    - Callback receives NaN-boxed string (STRING_TAG) of current text content
    - API: `TextField("placeholder", (text: string) => { ... })`
  - **Toggle**: NSSwitch + NSTextField label in horizontal NSStackView, with onChange callback
    - Callback receives TAG_TRUE/TAG_FALSE NaN-boxed boolean values
    - API: `Toggle("label", (checked: boolean) => { ... })`
  - **Slider**: Horizontal NSSlider with min/max/initial values and onChange callback
    - Continuous mode — fires callback while dragging
    - Callback receives plain f64 value (no NaN-boxing needed for numbers)
    - API: `Slider(0, 100, 50, (value: number) => { ... })`
  - All callback widgets follow the Button pattern: `define_class!` for target, thread-local HashMap for callbacks, `std::mem::forget(target)` to prevent deallocation
  - Added `NSBox`, `NSSwitch`, `NSSlider` features to objc2-app-kit Cargo.toml
  - Special codegen handlers for TextField, Toggle, Slider (string pointer extraction + closure handling)
  - Spacer and Divider use generic dispatch (no args)
  - New demo: `test-files/test_ui_controls.ts`

### v0.2.126
- Eliminate js_is_truthy FFI calls in while-loop conditions for Compare expressions
  - While-loop conditions like `x*x + y*y <= 4.0 && iter < MAX_ITER` previously compiled Compare
    expressions to f64 (via `select(fcmp, 1.0, 0.0)`), then called `js_is_truthy` FFI to convert
    back to bool — a wasteful round-trip on every iteration (~50M iterations for mandelbrot)
  - Now detects Compare expressions in while-loop conditions and compiles them directly as `fcmp`,
    producing an I8 bool without any FFI call
  - Optimized all while-loop condition paths: direct Compare, And(Compare, Compare), and the
    non-optimized fallback path (no counter detected)
  - **Mandelbrot benchmark: 48ms → 27ms (44% faster, now matches Node.js ~26ms)**
- Extend constant folding to handle LocalGet with const_value
  - `get_constant_value()` now checks `locals` for variables with `const_value` set
  - Expressions like `WIDTH / 2.0` (where `const WIDTH = 800`) now fold to `f64const(400.0)`

### v0.2.124-v0.2.125
- Reactive text binding: `Text("prefix" + State.value)` auto-updates when State changes
- Disable while-loop unrolling (i-cache pressure); keep CSE optimization
- Const value propagation for numeric literals (`const_value: Option<f64>` in `LocalInfo`)

### v0.2.122-v0.2.123
- Fix Cmd+Q and button callbacks in perry/ui (extract raw closure pointer via `js_nanbox_get_pointer`)
- Fix VStack/HStack children not rendering (use `js_nanbox_get_pointer` for child handle extraction)

## Changelog Summary (v0.2.37-v0.2.121)

### Performance Optimizations (v0.2.115-v0.2.121)
- **Integer function specialization** (v0.2.115): Detect integer-only functions, generate i64 variants. Fibonacci 2x faster than Node.js.
- **Array pointer caching** (v0.2.115): Hoist `js_nanbox_get_pointer` out of for-loops. Matrix multiply ~91ms → ~71ms.
- **i32 index arithmetic** (v0.2.117-v0.2.120): Contained i32 ops for array indexing only (`try_compile_index_as_i32`). Matrix multiply → ~41ms (Node ~39ms).
- **JSON.stringify optimization** (v0.2.99): Shared buffer, type hints, inline object field writes.
- **Self-recursive call fast path** (v0.2.99): Skip conversion passes when argument types match.

### Native UI (v0.2.116-v0.2.121)
- v0.2.116: Initial perry/ui module — Text, Button, VStack/HStack, State, App
- v0.2.119: Fix SIGILL crash, module init variable ID conflicts
- v0.2.121: Fix StringHeader format mismatch, Auto Layout constraints

### Fastify Framework (v0.2.79-v0.2.114)
- v0.2.79: Fastify-compatible HTTP runtime (routing, hooks, plugins)
- v0.2.80: Codegen integration (30+ extern functions, NativeMethodCall mappings)
- v0.2.82: Method-specific return type handling (route methods return bool, not handle)
- v0.2.102-v0.2.103: Handle-based method/property dispatch for cross-module calls
- v0.2.114: Fix `as f64` vs `from_bits` NaN-boxing corruption in request properties

### Async Closures & Promises (v0.2.39-v0.2.106)
- v0.2.39: Promise callbacks rewritten to use ClosurePtr
- v0.2.55: Promise.all()
- v0.2.58: `spawn_for_promise_deferred()` for thread-safe async operations
- v0.2.105-v0.2.106: `is_async` field on `Expr::Closure`, async closure Promise NaN-boxing

### Cross-Module System (v0.2.57-v0.2.110)
- v0.2.57: Cross-module array exports with NaN-boxing
- v0.2.78: `imported_func_param_counts` for optional parameter propagation
- v0.2.81: Re-export propagation for chained `export * from` patterns
- v0.2.102: Topological sorting for module init order
- v0.2.110: Fix module-level variable LocalId collisions with function parameters

### Cranelift Type System Fixes (v0.2.83-v0.2.96)
- Systematic I32 conversion fixes across 6+ codegen locations (v0.2.90, v0.2.96)
- `is_pointer && !is_union` checks for variable type determination (v0.2.83)
- Try/catch block variable type restoration after longjmp (v0.2.91)
- Constructor parameters always F64 at signature level (v0.2.112)

### Native Module Ecosystem (v0.2.41-v0.2.98)
- mysql2: pool, connections, prepared statements, timeouts (v0.2.41-v0.2.74)
- ioredis: synchronous constructor, handle-based dispatch (v0.2.54-v0.2.68)
- WebSocketServer from `ws` module (v0.2.98)
- AsyncLocalStorage from `async_hooks` (v0.2.97)
- ethers.js: BigInt, Keccak-256, EIP-55 addresses (v0.2.64-v0.2.75)
- Closure calls extended to 8 args (v0.2.88)

### Foundation (v0.2.37-v0.2.51)
- NaN-box string literals with STRING_TAG, undefined as TAG_UNDEFINED (v0.2.37)
- Boolean TAG_TRUE/TAG_FALSE representation (v0.2.51)
- BigInt with BIGINT_TAG, arithmetic, comparisons (v0.2.50)
- Inline array literal method calls (v0.2.104)
- Function inlining fixes: substitute_locals coverage, return-in-Expr-context (v0.2.94-v0.2.108)

**Milestone: v0.2.49** — Full production worker running as native binary (MySQL, LLM APIs, string parsing, scoring)
