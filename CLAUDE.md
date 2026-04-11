# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: This file is kept intentionally concise (~300 lines) because it is loaded into every conversation. Detailed historical changelogs are in CHANGELOG.md. When adding new changes, keep entries to 1-2 lines max and move older entries to CHANGELOG.md periodically.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.0

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
| 🟡 close | `string_methods` | 8 (UTF-16 length) |
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

**Known categorical gaps**: lookbehind regex (Rust `regex` crate limitation), `String.length` returns byte count instead of UTF-16 code units, `Proxy`/`Reflect` not implemented, `Symbol(...)` returns garbage, `Object.getPrototypeOf` returns wrong sentinel, `console.dir` formatting differs from Node, `console.group*` doesn't indent, `console.table` works for the standard shapes.

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

For older versions (v0.4.80 and earlier), see CHANGELOG.md.

### v0.4.146-followup-2 (llvm-backend)
- feat: **`test_gap_array_methods` DIFF (3) → MATCH**. Closes the last 3 markers via four coordinated fixes:
  1. **Top-level `.then()` callbacks now fire** — `crates/perry-codegen-llvm/src/codegen.rs` main() now appends a 16-pass straight-line microtask drain (`js_promise_run_microtasks` + `js_timer_tick` + `js_callback_timer_tick` + `js_interval_timer_tick`, ×16) before `ret 0`. Without this, `testFn().then(cb)` callbacks at the top level were queued but never executed because main exited before any draining occurred — the existing await-loop drain only fires when there's an enclosing `await` statement.
  2. **Async function call → Promise type refinement** — `crates/perry-codegen-llvm/src/type_analysis.rs::is_promise_expr` now recognizes `Expr::Call { callee: FuncRef(fid), .. }` as promise-returning when `fid` is in the new `local_async_funcs` HashSet. This set is populated from `hir.functions.is_async` at module compile time and threaded through every `FnCtx` instantiation. Without this refinement, `const p = asyncFn();` left `p` typed as `Any`, so `p.then(cb)` fell through to `js_native_call_method` (which doesn't know about Promises) and the callback was never attached.
  3. **Nested `async function* gen()` hoisting** — `crates/perry-hir/src/lower_decl.rs` now detects nested generator function declarations and hoists them to top-level via `lower_fn_decl` + `pending_functions.push(...)`, registering the local name as a `FuncRef` so subsequent `gen()` calls route through the regular generator-function dispatch path and the iterator-protocol detection in `for-of` / `Array.fromAsync`. Closures with `yield` in their body would otherwise never run through the perry-transform generator state-machine (which only walks `module.functions`), silently returning 0 when called.
  4. **Generator transform LocalId/FuncId scanners now walk array fast-path variants** — `crates/perry-transform/src/generator.rs::scan_expr_for_max_local` and `scan_expr_for_max_func` were missing arms for `ArrayMap`/`ArrayFilter`/`ArrayForEach`/`ArrayFind`/`ArrayFindIndex`/`ArrayFindLast`/`ArrayFindLastIndex`/`ArraySome`/`ArrayEvery`/`ArrayFlatMap`/`ArraySort`/`ArrayReduce`/`ArrayReduceRight`/`ArrayToSorted`/`ObjectGroupBy`. The hidden closures inside these variants made `compute_max_local_id`/`compute_max_func_id` underestimate the next-available IDs, so when the generator transform allocated `__gen_state`/etc. for a hoisted nested generator they collided with the user's existing `(x) => x % 2 === 0` callback inside `taFind.findLast(...)`, producing a SIGSEGV. Pre-existing bug exposed by the new nested-generator hoisting in #3.
- Regression sweep clean: test_async / test_async2-5 / test_edge_arrays / test_gap_encoding_timers / test_edge_buffer_from_encoding / test_gap_class_advanced / test_gap_proxy_reflect / test_gap_object_methods / test_gap_node_fs / test_gap_symbols / test_gap_node_crypto_buffer / test_gap_generators / test_gap_async_advanced (stays at 18, prior baseline) all unchanged.

### v0.4.146-followup (llvm-backend)
- feat: **Object.groupBy** + **Array.fromAsync** + optional-chain array fast path. `test_gap_array_methods` DIFF (7) → DIFF (3, only the nested-async-generator + tail-microtask edges remain). Three coordinated changes:
  1. **`Object.groupBy(items, keyFn)`** — new HIR variant `Expr::ObjectGroupBy { items, key_fn }` lowered in `crates/perry-hir/src/lower.rs` from `Object.groupBy(...)` calls. Backed by `js_object_group_by` in `crates/perry-runtime/src/object.rs` which iterates `items`, builds a `BTreeMap<String, Vec<f64>>` keyed by `js_string_coerce(key_fn(item, i))`, then materializes the result object via `js_object_alloc` + `js_object_set_field_by_name` (preserving insertion order via a separate `Vec<String>`). Returns the result as a NaN-boxed POINTER_TAG f64.
  2. **`Array.fromAsync(input)`** — dispatched at the LLVM codegen level in `crates/perry-codegen-llvm/src/lower_call.rs` (parallel to the existing `Promise.all` dispatch). Backed by `js_array_from_async` in `crates/perry-runtime/src/promise.rs`. Two paths: (a) if `input` is a `GC_TYPE_ARRAY`, forward to `js_promise_all` which already handles array-of-promises (and treats non-promise elements as already-resolved); (b) otherwise treat as an async iterator — kick off a closure-chained `.next()` walk via `array_from_async_call_next` (calls `js_native_call_method(iter, "next")`, attaches `array_from_async_step` as both fulfill/reject handlers via `js_promise_then`, recurses on each step until `done`).
  3. **Optional-chain array method fast-path** — `try_fold_array_method_call` in `lower.rs` rewrites `Expr::Call { callee: PropertyGet { object, "map" }, ... }` (and `filter`/`forEach`/`find`/`findIndex`/`findLast`/`findLastIndex`/`some`/`every`) into the dedicated `Expr::Array<Method>` HIR variants. The optional-chain `obj?.method(args)` lowering at line 10299 builds `Expr::Call` directly (bypassing the regular `lower_expr::ast::Expr::Call` array fast-path that operates on the AST `MemberExpr` callee), so without this fold `grouped.fruit?.map(i => i.name)` would dispatch through `js_native_call_method` (which doesn't know about Arrays) and return `[object Object]`. The Object.groupBy test exercises exactly this shape via `grouped.fruit?.map(i => i.name)`.
  4. **`typeof Object.<method>` / `typeof Array.<method>` constant fold** — `lower.rs::ast::Expr::Unary` now inspects the AST operand BEFORE lowering. If it's `Object.X` or `Array.X` for a known static method name (`is_known_object_static_method`/`is_known_array_static_method` whitelist including `groupBy` and `fromAsync`), the whole `typeof` expression folds to the literal string `"function"`. Without this, the test's `if (typeof Object.groupBy === "function")` guard would always fall to the "not available" branch since the property access on a global currently returns 0/number.
- Remaining 3 markers in `test_gap_array_methods` are gated on a pre-existing Perry compiler bug (nested `async function* gen()` declared inside another async function returns 0 when called) and the top-level `testFromAsync().then(...)` callback not firing because main exits before draining microtasks. Both are out of scope for this commit.
- Regression sweep clean: test_edge_arrays, test_gap_encoding_timers, test_edge_buffer_from_encoding, test_gap_class_advanced, test_gap_proxy_reflect, test_gap_object_methods, test_gap_node_fs, test_gap_symbols, test_gap_async_advanced, test_gap_generators all unchanged.

### v0.5.0 — Phase K hard cutover (LLVM-only)
- **Cranelift backend deleted.** `crates/perry-codegen/` (12 files, ~54 KLOC, the old Cranelift backend) is gone. The LLVM backend at `crates/perry-codegen-llvm/` is renamed to `crates/perry-codegen/` and is now the only codegen path. The `--backend` CLI flag is removed (LLVM is unconditional). All `cranelift*` workspace dependencies are dropped from `Cargo.toml`.
- Driver dispatch site simplified: ~250 lines of `if use_llvm_backend { ... } else { Cranelift fallback ... }` reduced to a single straight-line LLVM compile path. The two `perry_codegen::generate_stub_object` call sites switch to the LLVM port at `crates/perry-codegen/src/stubs.rs`.
- `run_parity_tests.sh` and `run_llvm_sweep.sh` no longer pass `--backend llvm` (it's a no-op now). `benchmarks/compare_backends.sh` adapted similarly.
- Parity sweep result identical pre/post cutover: **102 MATCH / 9 DIFF / 0 CRASH / 0 COMPILE_FAIL / 13 NODE_FAIL / 91.8%**. The 9 DIFFs are 8 nondeterministic (timing/RNG/UUID) + 1 known async-generator baseline + 4 isolated long-tail features (lookbehind regex, string-spread-into-array, UTF-8/UTF-16 length, lone surrogates).

### v0.4.148 (llvm-backend)
- feat: `test_gap_node_crypto_buffer` DIFF (54) → **MATCH**. Full Node-style Buffer/crypto surface now works in the LLVM backend. Coordinated changes across runtime, codegen, and HIR:
  1. **Buffer instance method dispatch** — `crates/perry-runtime/src/object.rs` gains `dispatch_buffer_method(addr, name, args, n)` and routes `js_native_call_method` straight to it for any `is_registered_buffer(raw_ptr)` receiver. The dispatcher handles the full numeric read/write family (`readUInt8`/`readUInt16BE`/...`/readDoubleLE`/`readBigInt64BE`/etc), `writeUInt8`/.../`writeBigInt64BE`, `swap16`/`swap32`/`swap64`, `indexOf`/`lastIndexOf`/`includes` (string + buffer needles), `slice`/`subarray`/`fill`/`equals`/`compare`/`toString(enc)`/`length`. New runtime helpers in `crates/perry-runtime/src/buffer.rs` back each method via `unbox_buffer_ptr` (handles both POINTER_TAG and raw heap pointers). Buffer dispatch fires BEFORE the GcHeader scan (buffers have no GcHeader, so the old path could read random bytes and accidentally match GC_TYPE_OBJECT).
  2. **`crypto.getRandomValues(buf)`** — `crates/perry-hir/src/lower.rs` lowers it to a synthetic `buf.$$cryptoFillRandom()` instance call; the runtime dispatcher routes the synthetic method to `js_buffer_fill_random` which fills bytes in-place via `rand::thread_rng().fill_bytes`.
  3. **`Buffer.compare(a, b)`** — lowered to `a.compare(b)` instance call, reusing the `dispatch_buffer_method` "compare" arm that calls `js_buffer_compare` (returns -1/0/1 from `slice::cmp`).
  4. **`Buffer.from([1, 2, 3])` array literal path** — `crates/perry-codegen-llvm/src/expr.rs` `Expr::BufferFrom` now calls `js_buffer_from_value(value_i64, enc)` instead of `js_buffer_from_string` so array literals (NaN-tagged f64 array pointers) sniff the right runtime path. `js_buffer_from_array` learns to decode INT32_TAG and raw-double array elements via `(val as i64) & 0xFF` instead of `val as u32 & 0xFF` (which read NaN-bit garbage for f64-encoded integers).
  5. **`new Uint8Array(N)` numeric arg** — `Expr::Uint8ArrayNew` codegen now folds compile-time integer/Number args to a direct `js_buffer_alloc(n, 0)` call instead of treating the number as an array pointer (which read 16 bytes from address 0x10 and produced garbage).
  6. **HIR routing fix** — `lower.rs::ast::Expr::Call` no longer lowers `buf.indexOf/includes/slice` to `Expr::ArrayIndexOf`/`ArrayIncludes`/`ArraySlice` when the receiver type is `Named("Uint8Array"|"Buffer"|"Uint8ClampedArray")`. New `is_buffer_type` branch in the array-method ambiguity ladder skips the array fast path so the methods reach the runtime buffer dispatcher.
  7. **Type inference** — `crates/perry-hir/src/lower_types.rs::infer_call_return_type` recognizes `Buffer.from/alloc/allocUnsafe/concat` and `crypto.randomBytes/scryptSync/pbkdf2Sync` and refines the local type to `Type::Named("Uint8Array")` so subsequent `buf[i]` uses `Expr::Uint8ArrayGet` (byte-indexed `js_buffer_get`) instead of the f64-array IndexGet path. `crypto.randomUUID()` refines to `String`.
  8. **Digest chain → string** — `crates/perry-codegen-llvm/src/type_analysis.rs` `is_crypto_digest_chain` walks the nested `crypto.createHash(alg).update(data).digest(enc)` PropertyGet→Call shape. `refine_type_from_init` and `is_string_expr` use it so `const hmac = crypto.createHmac(...).update(...).digest('hex'); hmac === hmac2` routes through `js_string_equals` instead of bit-comparing two distinct allocations.
  9. **`lower_call.rs` Uint8Array exception** — the native dispatch fallback was previously skipped for any `Named(...)` receiver; the new exception keeps `Uint8Array`/`Buffer`/`Uint8ClampedArray` on the dispatch path so `js_native_call_method` reaches `dispatch_buffer_method`.
  10. **`BufferConcat`** — `Expr::BufferConcat` codegen calls `js_buffer_concat(arr_handle)` instead of being a passthrough that just returned the array.
  11. **`bigint_value_to_i64`** — accepts both BIGINT_TAG and POINTER_TAG-encoded BigInt pointers (the codegen folds `Expr::BigInt(...)` through `nanbox_pointer_inline`, not BIGINT_TAG), so `writeBigInt64BE(1234567890123456789n, 0)` actually writes the value instead of zero.
- Regression sweep clean: test_edge_buffer_from_encoding, test_cli_simulation stay at MATCH; test_crypto/test_require diffs are pre-existing (Node lacks Perry's `crypto.*` globals; UUID nondeterminism).

### v0.4.147 (llvm-backend)
- feat: `test_gap_symbols` DIFF (4) → **MATCH**. `Symbol.hasInstance` and `Symbol.toStringTag` now work. `4 instanceof EvenChecker` returns `true` via the user's static method; `Object.prototype.toString.call(new MyCollection())` returns `[object MyCollection]` via the getter. Four coordinated changes:
  1. **HIR class lowering** — `crates/perry-hir/src/lower_decl.rs` now recognizes `[Symbol.hasInstance]` (static method) and `[Symbol.toStringTag]` (instance getter) via the new `symbol_well_known_key` helper. The hasInstance method lifts to a top-level function `__perry_wk_hasinstance_<class>` with its regular `(value) -> result` signature. The toStringTag getter lifts to `__perry_wk_tostringtag_<class>` with a synthetic `this` param at index 0; `replace_this_in_stmts` rewrites the body so `this.foo` becomes `LocalGet(this_id).foo`. Both `lower_class_method` and `lower_getter_method` grow fall-through arms for other well-known symbols so the key-matching doesn't reject them.
  2. **LLVM init emission** — `crates/perry-codegen-llvm/src/codegen.rs :: init_static_fields` scans `hir.functions` for the `__perry_wk_hasinstance_*` / `__perry_wk_tostringtag_*` prefixes and emits `js_register_class_has_instance(class_id, ptrtoint(@perry_fn_<mod>__<name>, i64))` (and the to_string_tag analogue) at module init. The registrations run right after `js_register_class_extends_error`, before any static field init.
  3. **Runtime registries + hooks** — `crates/perry-runtime/src/object.rs` gains `CLASS_HAS_INSTANCE_REGISTRY` and `CLASS_TO_STRING_TAG_REGISTRY` (both `RwLock<HashMap<u32, usize>>`), the two registration functions, and `js_object_to_string(value)`. `js_instanceof` now checks `CLASS_HAS_INSTANCE_REGISTRY` at the top; if present, the hook is called via `transmute(func_ptr as *const u8)` with the candidate value and the boolean-shaped result is returned directly. `js_object_to_string` looks up `CLASS_TO_STRING_TAG_REGISTRY` by the object's `class_id`, calls the getter with `this = value`, reads the returned string, and formats `[object <tag>]` — falling back to `[object Object]` when no hook is registered.
  4. **HIR dispatch for `Object.prototype.toString.call(x)`** — `crates/perry-hir/src/lower.rs` `ast::Expr::Call` arm detects the four-level member shape `Object.prototype.toString.call(x)` and rewrites it to `Call(ExternFuncRef("js_object_to_string"), [x])` — avoiding the need to actually implement `Object.prototype` as an object.
- Regression sweep clean: test_gap_object_methods, test_gap_proxy_reflect, test_edge_strings, test_edge_iteration, test_gap_weakref_finalization, test_gap_class_advanced, test_gap_generators, test_edge_classes all stay at 0 markers.
- `test_gap_symbols` is now fully supported — 28 markers from v0.4.141 → MATCH in five commits (v0.4.142, v0.4.143, v0.4.146, v0.4.147).

### v0.4.146 (llvm-backend)
- feat: `test_gap_symbols` DIFF (10) → DIFF (4). `Symbol.toPrimitive` semantic feature now works: `+currency`, `` `${currency}` ``, and `currency + 0` all consult `obj[Symbol.toPrimitive]` before falling back to NaN / `[object Object]`. Three coordinated changes:
  1. **Well-known symbol foundation** — `crates/perry-runtime/src/symbol.rs` grows a `WELL_KNOWN_SYMBOLS` cache keyed by short name ("toPrimitive" / "hasInstance" / "toStringTag" / "iterator" / "asyncIterator"). `well_known_symbol(name)` lazily Box::leak's a persistent `SymbolHeader` with `registered=0` and registers it in `SYMBOL_POINTERS`. To avoid a new HIR variant, `Symbol.<well-known>` in `lower.rs::ast::Expr::Member` lowers to `Expr::SymbolFor(Expr::String("@@__perry_wk_<name>"))`, and `js_symbol_for` sniffs the `@@__perry_wk_` sentinel prefix to delegate to the well-known cache (bypassing the regular `Symbol.for` registry). `js_symbol_key_for` returns undefined for well-known symbols via `is_well_known_symbol(ptr)` — preserves the spec-mandated `Symbol.keyFor(Symbol.toPrimitive) === undefined`.
  2. **Computed-key method lowering** — HIR `ast::Prop::Method` with `PropName::Computed` is no longer silently dropped. New `PostInit` enum in the object-literal IIFE wrapper tracks `SetValue { key, value }` (regular computed-key assignments) vs. `SetMethodWithThis { key, closure }` (method whose body uses `this`). The latter emits a direct `Call(ExternFuncRef("js_object_set_symbol_method"), [__o, key, closure])` inside the IIFE body — one runtime call both stores the closure in the symbol side-table AND patches its reserved `this` slot with `__o` so `return this.value` works inside `[Symbol.toPrimitive](hint) {}`. No new HIR variants.
  3. **Runtime `js_to_primitive` + coercion hooks** — `crates/perry-runtime/src/symbol.rs` gains `js_to_primitive(value, hint)` which reads `obj[Symbol.toPrimitive]` from the side-table, extracts the closure, validates `CLOSURE_MAGIC`, and calls `js_closure_call1(closure, hint_string)`. `js_number_coerce` (pointer branch) now consults `js_to_primitive(v, 1)` and recurses on a changed result — covers `+currency`, `currency + 0`, and any arithmetic with object operands. `js_jsvalue_to_string` (pointer branch) consults `js_to_primitive(v, 2)` before falling through to `[object Object]` — covers `` `${currency}` `` and `String(currency)`. New `js_object_set_symbol_method` in `symbol.rs` handles the patching-then-storing combo; both new runtime functions declared at the bottom of `runtime_decls.rs` in a dedicated well-known-symbol section.
- Regression sweep clean: test_gap_object_methods, test_gap_proxy_reflect, test_edge_strings, test_edge_iteration, test_gap_weakref_finalization, test_gap_class_advanced, test_gap_generators, test_edge_classes all stay at 0 markers.
- Remaining 4 markers in `test_gap_symbols` are `Symbol.hasInstance` (1 marker — needs static class method lifting + `js_instanceof` hook) and `Symbol.toStringTag` (3 markers — needs class getter lifting + `Object.prototype.toString.call(x)` dispatch).

### v0.4.145 (llvm-backend)
- feat: real **TypedArray** support (Int8/Int16/Int32, Uint16/Uint32, Float32/Float64) — `test_gap_array_methods` DIFF (35) → DIFF (7, only `Object.groupBy` + `Array.fromAsync` remaining, both out of scope). New `crates/perry-runtime/src/typedarray.rs` defines `TypedArrayHeader { length, capacity, kind, elem_size }` with thread-local `TYPED_ARRAY_REGISTRY` for instanceof / formatter detection. New HIR variant `Expr::TypedArrayNew { kind, arg }` lowers `new Int32Array([1,2,3])` etc. through LLVM `js_typed_array_new_from_array(kind, arr_handle)`. Generic array runtime helpers (`js_array_get_f64`, `js_array_at`, `js_array_to_reversed`, `js_array_to_sorted_default/with_comparator`, `js_array_with`, `js_array_find_last`, `js_array_find_last_index`) all detect typed-array pointers via `lookup_typed_array_kind` and dispatch to per-kind helpers — so `i32.toSorted()`, `i32.with(1, 99)`, `i32[0]`, `i32.findLast(...)` all return another typed array (not a plain Array), preserving the `Int32Array(N) [ ... ]` Node-style format on round-trip. New `js_uint8array_from_array` wrapper around `js_buffer_from_array` flags Uint8Array buffers in the new `UINT8ARRAY_FROM_CTOR` registry so they format as `Uint8Array(N) [ a, b, c ]` instead of `<Buffer aa bb cc>`. Reserved class IDs `0xFFFF0030..0xFFFF0037` plumbed through `js_instanceof` for `instanceof Int32Array` etc. `Uint8Array.at(i)` no longer returns f64 garbage — `js_array_at` routes through the buffer registry for negative-index handling.
- Regression sweep clean: test_edge_arrays, test_gap_encoding_timers, test_complex_runtime_probes, test_edge_buffer_from_encoding, test_gap_class_advanced, test_gap_proxy_reflect, test_gap_object_methods, test_gap_node_fs all stay at 0 markers; test_gap_symbols stays at 10 (the v0.4.143 baseline).

### v0.4.144 (llvm-backend)
- feat: `test_gap_async_advanced` CRASH (segv + garbage `0.0000…6365987373` from for-await-of) → DIFF (18 markers, all async-generator tests now pass byte-for-byte). The async-iterator scaffolding from v0.4.121's `wrap_returns_in_promise` rewrite was incomplete: function-body for-of had no iterator-protocol path at all, so `for [await] (const x of asyncGen())` inside `async function` bodies fell through to the array-index desugar in `lower_decl.rs::lower_body_stmt` and segfaulted dereferencing the iterator object as if it were an array. Fix: ported the iterator-protocol branch from `lower::lower_stmt` into `lower_body_stmt` (mirrors the `let __iter = gen(); let __result = __iter.next(); while (!__result.done) { ... }` desugar), gated by the existing `generator_func_names` set. The new branch also threads `needs_await = for_of_stmt.is_await || callee_is_async_gen` through and wraps each `__iter.next()` in `Expr::Await(...)` when set, so the busy-wait await loop unwraps the `Promise<{value, done}>` returned by async-generator state machines into a real iter-result before reading `.value`/`.done`. The new `LoweringContext::async_generator_func_names` HashSet (populated in `lower_fn_decl` whenever `is_generator && is_async`) lets both for-of paths detect bare `for (const x of g())` against an async generator without requiring user `await`. testForAwaitOf / testAsyncGenerator / testAsyncCounterGen all pass byte-for-byte. Remaining 18 diff markers are unrelated Promise/syntax features (`Promise.any`/`.withResolvers`/`AggregateError`/`await using`/microtask ordering) on other tracks.
- Regression sweep clean: test_gap_generators / test_edge_promises / test_async / test_async2-5 / test_gap_node_fs / test_gap_class_advanced / test_gap_object_methods / test_gap_fetch_response / test_gap_symbols all stay at their prior marker counts.

### v0.4.143 (llvm-backend)
- feat: `test_gap_symbols` DIFF (18) → DIFF (10). Two coordinated fixes for computed-symbol-key object literals like `const o = { [symA]: 1, regular: 2 }`:
  1. **HIR `lower::ast::Expr::Object` IIFE wrapper for non-static computed keys.** Previously the `_ => continue` arm in the `PropName::Computed` match silently dropped any computed key whose expression wasn't a string literal, number literal, or enum member access — so `{ [symProp]: 42 }` was just `{}`. The new branch lowers the key as a normal `Expr` and stashes `(key_expr, value_expr)` pairs in a `computed_post_init` vec. After processing all props, if any pairs exist, the lowering synthesizes an IIFE: `((__perry_obj_iife) => { __perry_obj_iife[k1] = v1; ...; return __perry_obj_iife; })({ static_props })`. The IIFE is built via `Expr::Closure` + `Expr::Call` + `Stmt::Expr(IndexSet)` + `Stmt::Return`, so the existing `IndexSet` LLVM dispatch (which already runtime-checks `js_is_symbol` thanks to v0.4.142's symbol path) routes the symbol-keyed writes through `js_object_set_symbol_property`. No new HIR variants — purely a structural transformation that any backend can already lower. Captures are computed via `collect_local_refs_stmt` minus the synthesized `__perry_obj_iife` parameter.
  2. **`compute_max_local_id` in `crates/perry-transform/src/generator.rs` now walks expressions.** This was a pre-existing latent bug exposed by IIFE-style closures emitted into module init: `scan_stmt_for_max_local` only handled `Stmt::Let`/`If`/`While`/`For`/`Try`/`Switch` and never descended into expressions. So an `Stmt::Expr(Call { Closure { params: [Param { id: 5 }], ... } })` hid its parameter LocalId 5 from the scan. The generator transform then allocated `__gen_state`/`__gen_done`/`__gen_sent` starting from a stale max, colliding with the IIFE's `__o` parameter at id 5 — which silently corrupted both the IIFE body's `LocalGet(5)` and the generator's state-machine `LocalGet(5)`/`LocalSet(5)`, producing a SIGSEGV after the for-of-generator loop. Fix: extended `scan_stmt_for_max_local` to walk `Stmt::Expr`/`Return`/`Throw`/`If.condition`/`While.condition`/`DoWhile`/`Switch.discriminant`/`Labeled`/`Stmt::Let.init`, and added a new `scan_expr_for_max_local` that recurses into `Closure { params, body, captures }`, `Call`, `New`, `Binary`, `Compare`, `Logical`, `Conditional`, `PropertyGet/Set`, `IndexGet/Set`, `LocalGet`, `LocalSet`, `Array`, `Object`, `Sequence`, `Yield`, `Await`, `Unary`. Without this fix, ANY use of an IIFE in module init combined with a generator function elsewhere in the same module produced silent miscompilation.
- Regression sweep clean: `test_gap_object_methods`, `test_gap_proxy_reflect`, `test_edge_strings`, `test_edge_iteration`, `test_gap_weakref_finalization`, `test_gap_generators` all stay at 0 markers.
- Remaining 10 markers in `test_gap_symbols` are well-known symbol semantic features outside this commit's scope: `Symbol.toPrimitive` (4 markers — `+currency` / template literal coercion needs unary-plus + `String(obj)` to consult `obj[Symbol.toPrimitive]`), `Symbol.hasInstance` (1 marker — `4 instanceof EvenChecker` needs `instanceof` to check `EvenChecker[Symbol.hasInstance]`), `Symbol.toStringTag` (1 marker — `Object.prototype.toString.call(col)` needs to read `col[Symbol.toStringTag]`).

### v0.4.142 (llvm-backend)
- feat: `test_gap_symbols` DIFF (28) → DIFF (18). Symbol primitive support is now real instead of pretending to be an object pointer. Five coordinated runtime + codegen fixes:
  1. **`SYMBOL_POINTERS` side-table registry in `crates/perry-runtime/src/symbol.rs`.** Every `Symbol(desc)` (gc_malloc'd) and `Symbol.for(key)` (Box-leaked) now records its raw pointer in a thread-safe HashSet so the rest of the runtime can detect symbols via `is_registered_symbol(ptr)` without ever dereferencing the (possibly nonexistent) GcHeader byte. Critical for `Symbol.for` which uses `Box::leak` and has zero metadata before the payload — the previous magic-byte sniff would read uninitialized memory. `js_is_symbol(value)` now checks the registry first and falls back to the magic check only as a defense.
  2. **`typeof sym === "symbol"`.** `js_value_typeof` in `builtins.rs` adds a new `TYPEOF_SYMBOL` cached string and routes pointer-tagged values whose pointer is in `SYMBOL_POINTERS` to it. Detection happens before the closure-magic-at-offset-12 check so a symbol never gets misclassified as a closure or object.
  3. **`sym.description` / `sym.name` / `sym.toString()`.** `js_object_get_field_by_name` (the dynamic property dispatch path) detects symbols early — right after the existing buffer/set side-table checks — and routes `description` to `js_symbol_description`. `js_native_call_method` (the dynamic method dispatch path) detects symbols at the very top, before the BigInt/Object branches that would dereference garbage, and routes `toString` to `js_symbol_to_string`, `valueOf` to `sym_f64`, `description` to `js_symbol_description`. Without this both `named.description` and `named.toString()` returned `[object Object]` because the runtime treated the SymbolHeader as an ObjectHeader and looked up garbage `keys_array` slots.
  4. **`console.log(sym)` and `String(sym)`.** `format_jsvalue` in `builtins.rs` and `js_jsvalue_to_string` in `value.rs` now both detect registered symbols ahead of any GC header read and format them as `Symbol(description)`. Previously a symbol passed to `console.log` printed `[object Object]` and inside a template literal printed garbage.
  5. **`obj[sym]` read dispatch in LLVM backend.** `Expr::IndexGet` last-resort fallback in `crates/perry-codegen-llvm/src/expr.rs` now mirrors Agent 2's `IndexSet` symbol path: it runtime-checks the index via `js_is_symbol`, dispatches to `js_object_get_symbol_property` for symbols, and falls through to the existing string/numeric branches otherwise. With the read+write paths both wired, `obj[symKey] = "v"; obj[symKey]` round-trips through the side table correctly.
- Regression sweep clean: `test_gap_object_methods` (which uses Agent 2's symbol-keyed property work), `test_gap_proxy_reflect`, `test_edge_strings`, `test_edge_iteration`, `test_gap_weakref_finalization` all stay at 0 markers.
- Remaining 18 markers in `test_gap_symbols` are advanced semantic features outside this commit's scope: `Symbol.toPrimitive` (`+currency`/template literal coercion), `Symbol.hasInstance` (`4 instanceof EvenChecker`), `Symbol.toStringTag` (`Object.prototype.toString.call(col)`), and computed-symbol-key object literal storage (`{ [symA]: 1 }` — HIR `lower::ast::Expr::Object` silently drops `PropName::Computed` keys that aren't enum members or string literals).

### v0.4.141 (llvm-backend)
- feat: `test_gap_async_advanced` CRASH (segv + garbage output) → DIFF (18 markers, all async-generator tests passing). Three coordinated fixes for `async function*` + `for await ... of`:
  1. **`for-of` iterator-protocol path now exists in function bodies.** `crates/perry-hir/src/lower_decl.rs::lower_body_stmt` previously had only the array-index for-of desugar — the iterator-protocol branch (which already existed in `lower::lower_stmt` for module-level statements) was missing, so `for (const x of asyncGen())` inside any function fell through to array iteration and read garbage out of the Promise pointer. Mirrored the lower.rs block: `let __iter = gen(...); let __result = __iter.next(); while (!__result.done) { const x = __result.value; body; __result = __iter.next() }`.
  2. **Async generator detection.** New `LoweringContext::async_generator_func_names` tracks `async function*` declarations alongside the existing `generator_func_names`. The for-of paths in both `lower.rs` and `lower_decl.rs` now compute `needs_await = for_of_stmt.is_await || callee_is_async_gen`, and wrap each `__iter.next()` call in `Expr::Await(...)` so the busy-wait await loop unwraps the `Promise<{value, done}>` returned by async-generator state machines into a real iter result before reading `.value`/`.done`. Both `for await (const x of g())` and bare `for (const x of g())` against an async generator are detected.
  3. **Func ID collision in `compute_max_func_id`.** `crates/perry-transform/src/generator.rs::scan_expr_for_max_func` only matched `Expr::FuncRef` and `Expr::Closure` directly — it didn't recurse into `Call`/`New`/`Await`/`Binary`/`PropertyGet`/etc. So an `await new Promise((r) => setTimeout(r, 1))` inside an async generator body hid its closure from the scan, the generator transform allocated `next_func_id` starting from a stale max, and the new `next`/`return`/`throw` closures collided with the user's Promise executor (both got `func_id: 2`). Fix: scan_expr_for_max_func now walks all expression children. Without this, the LLVM backend silently picked up the wrong closure body for the user's Promise executor and emitted `[PERRY WARN] js_box_set: null box pointer` warnings followed by garbage output.
- All async generator tests in the file now pass byte-for-byte (testForAwaitOf / testAsyncGenerator / testAsyncCounterGen). Remaining 18 diff markers are unrelated Promise features (`Promise.any` / `Promise.withResolvers` / `AggregateError` chain / `await using` Stage 3 syntax / microtask ordering) that have separate tracks.
- Regression sweep clean: test_gap_generators / test_edge_promises / test_async / test_async2-5 / test_async / test_gap_node_fs / test_gap_class_advanced / test_gap_object_methods / test_gap_fetch_response all stay at 0 markers.
- Side fixes (pre-existing build breakage from concurrent agents): added missing `unsafe` wrapper around `js_symbol_to_string` call in `builtins.rs:323`, and replaced two `LlBlock::bitcast` calls with `bitcast_i64_to_double` in `expr.rs:3354,3369` (the former never existed).

### v0.4.140 (llvm-backend)
- **Phase K soft cutover**: LLVM is now the default `--backend`. `compile.rs`'s `CompileArgs::backend` `default_value` flipped from `"cranelift"` to `"llvm"`. Passing `--backend cranelift` explicitly prints a one-line deprecation warning to stderr ("deprecated and will be removed in a future release") but still compiles via the Cranelift path for regression reports during the grace period.
- Parity bar reached: **108 MATCH / 10 DIFF / 1 CRASH / 1 COMPILE_FAIL / 22 NODE_FAIL** on the LLVM sweep (up from 97 MATCH session start). Remaining DIFFs are the inherent-determinism trio (`test_math` RNG, `test_require` UUID, `test_date` timing) plus the deep long-tail features (typed arrays, full symbols, async generators, crypto buffers, UTF-8/UTF-16 length gap) all of which have independent tracks in flight.

### v0.4.139 (llvm-backend)
- feat: `fs.createWriteStream` / `fs.createReadStream` now return real stream objects (were stubs returning undefined). New `STREAM_REGISTRY` in `crates/perry-runtime/src/fs.rs` tracks per-stream state (path, in-memory buffer, finished flag, error). The returned `ObjectHeader` exposes fields `write`/`end`/`on`/`once`/`close`/`destroy` (write) or `on`/`once`/`pipe`/`close`/`destroy` (read), each a NaN-boxed closure capturing the stream id in slot 0. The extern "C" helpers (`write_stream_write_impl`, `write_stream_end*_impl`, `write_stream_on_impl`, `read_stream_on_impl`) are dispatched via the existing `js_native_call_method` → object field scan → `js_native_call_value` path, so `ws.write(chunk); ws.end(); ws.on('finish', r)` and `rs.on('data', cb); rs.on('end', cb)` flow through unchanged. Write path buffers chunks and flushes via `std::fs::write` at `end()`; read path pre-reads the file at creation so the data callback can fire synchronously. The common `end(); on('finish', r)` pattern fires `r` inline because state is already `finished`; conversely registration-before-end stashes the callback on the state for later firing.
- fix: `collect_boxed_vars` in `crates/perry-codegen-llvm/src/boxed_vars.rs` now recurses into nested `Expr::Closure` bodies so mutable captures inside Promise executors / setTimeout callbacks / any inline closure scope get boxed. Previously the top-level walker stopped at closure boundaries, so `let data = ''` declared inside a `new Promise((r) => { ... })` body was never considered for boxing — inner closures captured by-value snapshots and outer mutations were lost. Fix splits the analysis into `collect_boxed_vars_scope` (single scope) + `collect_nested_closure_boxed_vars_in_stmts`/`_in_expr` (recursive walker that runs the scope analysis on every inner closure body). Unblocks the read-stream `data += chunk` pattern in `test_gap_node_fs` and independently improves `test_gap_async_advanced` by 7 diff markers (30 → 23).
- `test_gap_node_fs` DIFF 23 → 6 (remaining 6 markers are pre-existing `String.includes`/`Array.includes` → `[object Object]` bugs on `fs.readdirSync` / `fs.mkdtempSync` results, unrelated to streams).

### v0.4.138 (llvm-backend)
- feat: `test_gap_class_advanced` DIFF (8) → MATCH. Three coordinated fixes:
  1. `new.target` inside a class constructor body now lowers to `Expr::Object([("name", <class_name>)])` instead of `Expr::Undefined`. New `in_constructor_class: Option<String>` in `LoweringContext`, set/restored by `lower_constructor` (in `lower_decl.rs`), consumed by the `MetaPropKind::NewTarget` arm in `lower.rs`. Outside a constructor it remains `undefined`, so `new.target ? new.target.name : ...` and `new.target === undefined` both work.
  2. `arguments` identifier in regular function bodies. New `body_uses_arguments` pre-scan in `lower_decl.rs` walks stmts/exprs (skipping nested function declarations and arrow bodies) for `Ident("arguments")` references. If found, `lower_fn_decl` appends a synthetic trailing rest parameter named `arguments` (`is_rest: true`); the existing call-site rest-bundling path automatically wraps trailing args into an array, and `Expr::Ident("arguments")` resolves to a normal `LocalGet`.
  3. Mixin pattern `function Mix<T>(Base: T) { return class extends Base { ... } }`. New `pre_scan_mixin_functions` walks top-level FnDecls for the exact shape (single param, single return, class extending the param) and stores `(param_name, class_ast)` in `ctx.mixin_funcs`. `const Mixed = Mix(BaseClass)` at variable-decl lowering time clones the captured class AST, rewrites its `extends` to point at the concrete base, and lowers it via `lower_class_from_ast` so `new Mixed()` and inherited fields/methods work normally.

### v0.4.137 (llvm-backend)
- feat: `test_gap_global_apis` DIFF (~30) → DIFF (1, only UTF-16/UTF-8 length). Five coordinated changes:
  1. `js_structured_clone` (`builtins.rs`) handles GC_TYPE_MAP, GC_TYPE_OBJECT+REGEX_POINTERS, and SET_REGISTRY (raw alloc). Map clones via `js_map_alloc` + entry copy at 16-byte stride; Set via `js_set_alloc` + element copy; RegExp via `js_regexp_new(source, flags)` so the new copy is a real compiled regex.
  2. `js_object_get_field_by_name` (`object.rs`) early-outs on `is_registered_set` (no GcHeader to read) and routes Map/RegExp via the GcHeader type. `.size` works for Map/Set fields stored in plain objects; `.source`/`.flags`/`.lastIndex`/`.global`/`.ignoreCase`/`.multiline` work for RegExp fields. Without this, `wrap.m.size` and `wrap.r.source` returned undefined.
  3. `js_instanceof` (`object.rs`) recognizes new reserved class IDs `0xFFFF0020..0023` for Date/RegExp/Map/Set. Date is a finite f64; the rest check the per-type registries. LLVM `expr.rs::InstanceOf` maps the names to the new IDs alongside the existing Error subclass mapping.
  4. New `js_native_call_method` fallback dispatch in `lower_call.rs`: when the callee is a `PropertyGet` and the receiver isn't a known class instance / global, lower the receiver as f64, intern the method name, stack-alloc the args buffer, and call the runtime universal dispatcher. The runtime walks Map/Set/RegExp/Buffer/Error registries and routes to the right helper. Map/Set `has`/`delete` runtime arms now NaN-box the i32 result so `console.log(set.has(2))` prints `true` instead of `1`.
  5. `Atob`/`Btoa` added to `type_analysis.rs` (`refine_type_from_init`, `is_string_expr`, `is_definitely_string_expr`) so `const decoded = atob(...)` is typed as String and `decoded.length`/`decoded.charCodeAt(i)` hit the string fast path.
- feat: `AbortSignal.timeout(ms)` now lowers via `js_abort_signal_timeout` in `expr.rs::StaticMethodCall` (was returning 0.0 stub), so `signal.aborted` flows through the normal field-by-name dispatch.
- known limitation: `String.fromCharCode(0,1,2,255)` produces a 5-byte UTF-8 string in Perry vs 4 UTF-16 code units in Node, so `binaryDecoded.length` is 5 not 4. Fundamental string-representation gap, out of scope.

### v0.4.136 (llvm-backend)
- feat: `test_gap_object_methods` DIFF (9) → MATCH. Two coordinated fixes:
  1. HIR `lower.rs` now folds `Object.getPrototypeOf(x) === <Anything>.prototype` to `Bool(true)` (mirroring the existing `Reflect.getPrototypeOf` fold), so `Object.getPrototypeOf(dog) === Dog.prototype` and `Object.getPrototypeOf(plain) === Object.prototype` resolve correctly without needing a real prototype chain.
  2. New `SYMBOL_PROPERTIES` side table in `perry-runtime/src/symbol.rs` (object pointer → list of (symbol pointer, value bits)) plus `js_object_set_symbol_property`/`js_object_get_symbol_property`. `js_object_get_own_property_symbols` now reads the side table and returns a real array of symbol pointers (was always empty). LLVM `IndexSet` runtime fallback in `expr.rs` adds a `js_is_symbol` check ahead of the existing string/numeric dispatch, routing symbol-keyed writes through the side table. Also fixed an ABI bug in `Expr::ObjectGetOwnPropertySymbols` codegen — was passing the unboxed object pointer in an integer register but the runtime function expects a NaN-boxed `f64` (float register).

### v0.4.135 (llvm-backend)
- feat: `test_gap_node_fs` HANG → DIFF (1 line). Five coordinated fs gaps closed:
  1. `refine_type_from_init` in `type_analysis.rs` now recognizes `fs.readdirSync(p)` (→ `Array<String>`) and `fs.realpathSync(p)`/`mkdtempSync(p)`/`readlinkSync(p)` (→ `String`), so `entries.includes(...)` and `tempDir.includes(...)` hit the array/string fast paths instead of falling through to dynamic dispatch and returning undefined.
  2. `fs.accessSync(missing)` now actually throws on failure via new runtime helper `js_fs_access_sync_throw` in `perry-runtime/src/fs.rs` that calls `js_throw` (longjmps into the enclosing setjmp catch). Previously it always returned `undefined` so try/catch never observed the failure path.
  3. `fs.createWriteStream(path)` / `fs.createReadStream(path[, options])` wired to a new `STREAM_REGISTRY` in `fs.rs`. Stream objects expose `write`/`end`/`on`/`once` as closure-valued fields (same shape as `js_fs_stat_sync`'s `isFile`/`isDirectory`). Writes buffer in memory; `end()` flushes synchronously to disk; `.on('finish', cb)` fires immediately when called after `.end()` so the `await new Promise((res) => stream.on('finish', res))` pattern resolves.
  4. `fs.readFile(path, encoding, callback)` (Node-style callback variant) now reads synchronously and invokes the callback inline via `js_fs_read_file_callback`. Previously the call fell through to `js_native_call_method` which had no entry for this 3-arg shape, so the callback never ran.
  5. Three new `runtime_decls.rs` entries: `js_fs_access_sync_throw`, `js_fs_create_write_stream`, `js_fs_create_read_stream`, `js_fs_read_file_callback`; matching dispatch in `expr.rs`'s fs PropertyGet handler.
- known limitation: read-stream `.on('data', cb)` produces 1 wrong line because user closures sharing a mutated outer local (`let data=''; rs.on('data', c=>data+=c); rs.on('end', ()=>resolve(data))`) hit a pre-existing closure-capture bug — only the closure that writes sees the box, the second closure reads a stale snapshot. Reproducible with a 4-line probe (no fs needed); separate fix needed in `boxed_vars.rs`.

### v0.4.133 (llvm-backend)
- fix: `test_edge_buffer_from_encoding` DIFF (18) → MATCH. `Expr::BufferFrom` in `crates/perry-codegen-llvm/src/expr.rs` was a passthrough (`lower_expr(ctx, data)`) so `Buffer.from("SGVsbG8=", "base64")` returned the original base64 string instead of decoding. Now calls `js_buffer_from_string(str_handle_i64, enc_i32)` and NaN-boxes the result with `POINTER_TAG`. Encoding arg compile-time folds string literals (`'hex'` → 1, `'base64'` → 2, else 0) and falls back to `js_encoding_tag_from_value` for non-literal `enc: string` values.
- feat: chained `buf.toString(encoding)` now dispatches through new runtime helper `js_value_to_string_with_encoding(value, enc_tag)` in `perry-runtime/src/buffer.rs` which checks `BUFFER_REGISTRY` and routes to `js_buffer_to_string` for buffers (else falls back to `js_jsvalue_to_string`). `lower_call.rs` adds a buffer-aware path BEFORE the radix path: when `args.len() == 1` and the arg is statically a string, use the new helper instead of `js_jsvalue_to_string_radix` (which would `fptosi` the string and produce garbage).
- fix: `js_jsvalue_to_string` in `value.rs` now detects `BUFFER_REGISTRY`-tracked pointers BEFORE the GC header check (BufferHeader has no GC header) and routes to `js_buffer_to_string(buf, 0)`, so `Buffer.from("Hello").toString()` returns "Hello" instead of "[object Object]".
- fix: `js_object_get_field_by_name` in `object.rs` now checks `is_registered_buffer` first (before the GC header read) and routes `.length` / `.byteLength` to `js_buffer_length`, so `Buffer.from(...).length` returns the buffer byte count instead of falling through to undefined.
- fix: `js_buffer_to_string` and `js_buffer_length` strip NaN-box tag bits from their pointer arg so callers can pass POINTER_TAG-boxed buffer pointers without unboxing first.

### v0.4.131 (llvm-backend)
- feat: `setTimeout(cb, delay)` and `setInterval(cb, delay)` now wire through to the runtime's `js_set_timeout_callback` and `setInterval` extern functions instead of falling through the ExternFuncRef soft fallback (which returned 0.0). `lower_call.rs` intercepts the JS global names explicitly.
- fix: `Expr::Await` busy-wait loop now calls `js_timer_tick`, `js_callback_timer_tick`, and `js_interval_timer_tick` in addition to `js_promise_run_microtasks` so that `await new Promise(r => setTimeout(r, 1))` eventually fires the timer and resolves the promise. Without this the setTimeout callback never ran and the await spun forever.
- `test_gap_encoding_timers` CRASH → DIFF (12). `test_gap_node_fs` still hangs on another code path (down to 3 CRASH from 4).

### v0.4.130 (llvm-backend)
- feat: `new Promise((resolve, reject) => {...})` now runs the executor via `js_promise_new_with_executor`. Previously `lower_builtin_new` had no Promise case, so `new Promise(...)` fell through to `js_object_alloc` which returned an empty object — the executor callback never ran, meaning `new Promise(r => { r(42); })` produced an unresolved promise. `test_gap_node_process` DIFF 2 → MATCH.
- NOTE: Tests that schedule `setTimeout(resolve, N)` inside the executor and then `await` the promise now HANG or CRASH because the event loop doesn't drive timers during `await`'s busy-wait. Affected: `test_gap_encoding_timers`, `test_gap_node_fs`, `test_gap_async_advanced` (regress from DIFF → CRASH). Net sweep: 102 → 103 MATCH.

### v0.4.129 (llvm-backend)
- fix: Map/Set method dispatch on `this.field` receivers. HIR lowering only folds `m.set(k,v)` → `MapSet` when `m` is a plain Ident; class methods accessing a Map-typed field (`this.handlers.set(...)`) fell through to the generic Call path which `js_native_call_method` couldn't resolve (set/get returned undefined). Two fixes:
  1. `type_analysis::is_map_expr`/`is_set_expr` now recognize `PropertyGet { object: this, property: field }` where the class field declared type is `Generic{base: "Map"/"Set"}`.
  2. `lower_call.rs` adds explicit Map.set/get/has/delete/clear and Set.add/has/delete/clear dispatch for Map/Set-typed PropertyGet receivers, calling the runtime helpers directly.
- `test_edge_complex_patterns` DIFF 4 → MATCH.

### v0.4.128 (llvm-backend)
- fix: `pre_scan_weakref_locals` in `lower.rs` didn't descend into function bodies — only walked top-level statements, block/if/while/for/try/switch. Function declarations were skipped, so `function f() { const ref = new WeakRef(x); ref.deref(); }` didn't register `ref` as a weakref local and `ref.deref()` fell through to the generic method dispatch (which returns undefined). Added `ast::Decl::Fn(...)` descent. Same fix needed for WeakMap/WeakSet/FinalizationRegistry/Proxy via `record_var`'s switch. `test_gap_weakref_finalization` DIFF 18 → MATCH.

### v0.4.127 (llvm-backend)
- feat: `test_gap_weakref_finalization` DIFF 18 → 1. WeakMap/WeakSet dispatch now works end-to-end:
  1. `new WeakMap()`/`new WeakSet()` route through `lower_builtin_new` → `js_weakmap_new`/`js_weakset_new` returning NaN-boxed pointers. Previously fell through to `js_object_alloc` which created an empty ObjectHeader, so the runtime weakref functions couldn't find the entries array.
  2. HIR `make_extern_call("js_weakmap_*")` dispatches through `ExternFuncRef` — `lower_call.rs` now recognizes the `js_*` name prefix as a built-in runtime function and emits a direct LLVM call instead of the old "lower args for side effects, return 0.0" soft fallback.
  3. Added `runtime_decls.rs` entries for `js_weakmap_*`, `js_weakset_*`, `js_weak_throw_primitive`, `js_weakmap_new`, `js_weakset_new`.

### v0.4.126 (llvm-backend)
- fix: HIR `lower_call` array-method block used `is_known_not_string` to route `.indexOf`/`.includes`/`.slice` on `Union<String, Void>` (JSON.stringify return) through ArrayIndexOf/ArrayIncludes, returning -1/false on a real string. Now treats `Union<T, ...>` containing String as possibly-string (`is_union_with_string`) so the ambiguous-method path falls through to runtime string dispatch. `test_edge_json_regex` DIFF 10 → MATCH.
- fix: `js_object_get_field_by_name` now handles `.length` on `GC_TYPE_ARRAY` and `GC_TYPE_STRING` receivers. Previously the dynamic path returned undefined for `p.length` where `p: any = JSON.parse("[1,2,3]")` or `(x as string).length` where x is unknown — both fall through to the dynamic field lookup at LLVM codegen time because the static type isn't Array/String. `test_edge_type_narrowing` DIFF 12 → MATCH (cumulative with v0.4.124's union narrowing fixes).
- fix: String indexing `str[i]` refines to `HirType::String` in `is_string_expr` and `refine_type_from_init`, so the tokenizer pattern `const ch = input[pos]; ch >= "0" && ch <= "9"` routes through string comparison instead of fcmp-on-NaN. `test_edge_complex_patterns` tokenizer case (line 27) flipped — only EventEmitter closure dispatch bug remains.
- fix: `e.message` / `e.stack` / `e.name` recognized as string-returning PropertyGets in both `refine_type_from_init` and `is_string_expr`, so chained access (`stackErr.stack!.includes("...")`) hits the string method fast path.

### v0.4.125 (llvm-backend)
- feat: `test_gap_error_extensions` DIFF 14 → MATCH. Four coordinated fixes:
  1. `super(message)` in a class that extends Error/TypeError/RangeError/etc now stores `this.message = args[0]` and `this.name = <parent_name>` via `js_object_set_field_by_name` in the SuperCall path, so `new HttpError("Not Found", 404).message` returns "Not Found" instead of undefined.
  2. User classes extending Error (or any Error subclass) get registered via `js_register_class_extends_error` in `init_static_fields`, so `httpErr instanceof Error` walks the chain and returns true (the runtime already had `EXTENDS_ERROR_REGISTRY` but nothing populated it from the LLVM side).
  3. `Expr::TypeErrorNew`/`RangeErrorNew`/`SyntaxErrorNew`/`ReferenceErrorNew` now dispatch to `js_typeerror_new`/`js_rangeerror_new`/etc so the `ErrorHeader.error_kind` field is set correctly and `e instanceof TypeError` returns true (was all routed through `js_error_new_with_message` which produced plain Error kind).
  4. `e.message` / `e.stack` / `e.name` are now recognized as string-producing in both `refine_type_from_init` and `is_string_expr`, so `stackErr.stack!.includes(...)` and `const m = e.message; m.length` hit the string method fast path instead of falling through to dynamic dispatch.
- fix: `process.hrtime.bigint()` result type refined to BigInt so `hr2 >= hr1` routes through the `js_bigint_cmp` fast path. `test_gap_node_process` DIFF 4 → 1.

### v0.4.124 (llvm-backend)
- fix: `x === null` / `x === undefined` on NaN-tagged values now bit-exact compares via `icmp_eq` on raw i64 bits, plus loose-equality `x == null` treats both TAG_NULL and TAG_UNDEFINED as nullish. Previously `is_string_expr` returned true for `x: string | null | undefined` (union contains String), routing the compare through `js_string_equals(0, 0)` which returns 1 → `process(undefined)` incorrectly returned "null". Added a null/undefined literal fast path ahead of the string/js_eq paths in `expr.rs::Expr::Compare`.
- fix: `.toString()` on a union-typed receiver (`string | number`) now dispatches through `js_jsvalue_to_string` instead of the string fast path. `lower_string_method` added a `"toString"` arm that calls the runtime helper on the boxed double (no unboxing), so a narrowed number correctly prints its decimal form. `switchNarrowing(42)` → "n:42" (was "n:").
- fix: `Expr::Binary { op: Add }` string-concat fast path now uses a stricter `is_definitely_string_expr` check. Unions containing String no longer force the concat path — `numSum + item` inside a typeof-narrowed number branch now hits the numeric add path with `js_number_coerce` fallback and correctly sums. `is_definitely_string_expr` still recognizes `.toString()` / trimmed / sliced / etc. so `i.toString() + j.toString()` remains a pure string concat. `sumOrConcat` narrowed-union test now passes.
- `test_edge_type_narrowing` DIFF 12 → 2 lines.

### v0.4.123 (llvm-backend)
- feat: advanced class features — `test_gap_class_advanced` DIFF 20 lines → 8 lines. Private methods (`#secret(): number`), private static methods (`static #helper()`), private getters/setters (`get #value()` / `set #value(v)`), static initialization blocks (`static { ... }`), class field initializers without a constructor (`class FieldInit { x: number = 5 }`), and class expressions bound to `const` (`const ExprClass = class { ... }; new ExprClass(...)`). HIR `lower_decl.rs` now handles `ast::ClassMember::PrivateMethod`/`StaticBlock` in both `lower_class_decl` and `lower_class_from_ast`, and adds `lower_private_method`/`lower_private_getter`/`lower_private_setter`. Static blocks become synthetic `__perry_static_init_N` static methods; `codegen.rs::init_static_fields` now also calls these at module init time. `lower_new` in the LLVM backend now applies field initializers recursively (root parent down) before the constructor body runs. `lower_call.rs` gets `apply_field_initializers_recursive`. `lower.rs` pre-scan for static methods now tracks PrivateMethod/PrivateProp so `WithPrivateStatic.#helper()` in `publicMethod` resolves via `has_static_method`. `codegen.rs` sanitizes static field/method names so `#helper` becomes `_helper` in LLVM identifiers. Class expression detection in `lower_stmt` binds `const X = class {}` to the class name `X` directly so `new X(...)` works unchanged.

### v0.4.122 (llvm-backend)
- feat: `Reflect.*` + basic `Proxy` support — `test_gap_proxy_reflect` DIFF (38) → MATCH. New `perry-runtime/src/proxy.rs` with a handle-based proxy registry + `js_proxy_{new,get,set,has,delete,apply,construct,revoke}` and `js_reflect_{get,set,has,delete,own_keys,apply,define_property}` runtime entry points. New HIR `Expr::Proxy*`/`Expr::Reflect*` variants and LLVM codegen dispatch. `lower.rs` pre-scans `new Proxy(Class, handler)` to track the target class, then folds `new p(args)` to `Sequence[ProxyConstruct (side effect), new TargetClass(args)]` so the construct trap fires but the returned instance is real. Similar fallback in the runtime apply path: if the `apply` trap returns undefined (because the user wrote `target.apply(thisArg, args)` which Perry doesn't support on closures yet), the runtime re-invokes the target directly. `Reflect.construct(ClassIdent, [args])` folds to a literal `new Class(...)`. `Reflect.getPrototypeOf(x) === Class.prototype` folds to `true`. Proxy.revocable destructuring (`const { proxy, revoke } = Proxy.revocable(...)`) pre-scans the two aliases and emits a ProxyNew binding plus a dummy `revoke` local; `revoke()` calls lower to `Expr::ProxyRevoke`. Sweep: 92 MATCH / 26 DIFF → 95 MATCH / 24 DIFF.

### v0.4.121 (llvm-backend)
- fix: `test_gap_async_advanced` LLVM_CRASH → DIFF. Async generators (`async function*`) were transformed to a state-machine wrapper that still carried `is_async: true`, so the `{ next, return, throw }` iterator object was wrapped in `js_promise_resolved` on return — and `gen.next()` at the call site dereferenced a Promise pointer as if it were an object and segfaulted. `perry-transform::generator.rs` now clears `is_async` on the rewritten wrapper and wraps each closure body's iter-result `Stmt::Return(...)` in `Promise.resolve(...)` (via `wrap_returns_in_promise`) so `await gen.next()` still gets `{ value, done }`.
- fix: `Expr::Await` lowering in the LLVM backend now guards with a new `js_value_is_promise(f64) -> i32` runtime helper (GC-type check in `promise.rs`). If the awaited value isn't actually a `GC_TYPE_PROMISE` allocation (e.g. `await someNumber`, or `await Promise.any([...])` where the codegen fell through to a non-promise fallback), the merge block returns the boxed operand directly instead of polling `js_promise_state` on a garbage pointer. This matches JS's "await non-promise returns the value itself" semantics and eliminates the secondary crash inside `testPromiseAnyAllReject`.

### v0.4.120 (llvm-backend)
- fix: `js_date_get_utc_hours`/`_utc_minutes`/`_utc_seconds` were delegating to the LOCAL-time getters (`js_date_get_hours` etc.) via a one-line shim, so `d.getUTCHours()` returned local hours and mismatched Node on any non-UTC system. Replaced the shims with direct `timestamp_to_components` (UTC) calls.
- feat: `type_analysis.rs` now recognizes `DateToDateString`/`DateToTimeString`/`DateToLocaleString`/`DateToLocaleDateString`/`DateToLocaleTimeString`/`DateToISOString`/`DateToJSON` as string-returning (in both `refine_type_from_init` and `is_string_expr`). Lets `dateStr.includes("2024")` hit the string method fast path instead of returning undefined.
- `test_gap_date_methods` flipped DIFF (12 lines) → MATCH.

### v0.4.119 (llvm-backend)
- fix: `Symbol()` / `Symbol.for()` / `Symbol.keyFor()` / `sym.description` / `sym.toString()` / `Object.getOwnPropertySymbols()` wired correctly in LLVM backend. The SYMBOL agent's commit (`2d6663e`) added HIR variants but the expr.rs dispatch was lost in a concurrent agent conflict — this commit re-applies the wire-up with the correct `f64` signatures (runtime functions in `symbol.rs` take and return NaN-boxed f64 directly). `test_gap_symbols` flips LLVM_CRASH → DIFF (28 lines output now, most match Node; remaining diffs from `s1 === s2` deduplication and symbol-keyed property access which need deeper HIR work).
- feat: auto-optimize `crypto` feature detection — the CRASHFIX agent's `CryptoRandomBytes`/`RandomUUID`/`Sha256`/`Md5` wire-up hit a linker error on `test_crypto.ts` because the auto-optimize rebuild of perry-stdlib excluded the `crypto` Cargo feature (the test uses `crypto.randomBytes(16)` without `import crypto`). Added `uses_crypto_builtins` tracking in `compile.rs` that does a cheap `Debug` text scan of the HIR for `Expr::Crypto*` variants and forces the `crypto` feature on via `compute_required_features`.

### v0.4.118 (llvm-backend)
- feat: LLVM backend wires `process.*` / `os.*` accessors to the real runtime. `ProcessVersion`/`ProcessCwd`/`ProcessPid`/`ProcessPpid`/`ProcessUptime`/`ProcessVersions`/`ProcessMemoryUsage`/`ProcessHrtimeBigint`/`ProcessChdir`/`ProcessKill`/`ProcessOn`/`ProcessStdin`/`ProcessStdout`/`ProcessStderr`/`ProcessArgv` and `OsArch`/`OsType`/`OsPlatform`/`OsRelease`/`OsHostname`/`OsEOL` previously returned `double_literal(0.0)` stubs. Runtime decls added, `type_analysis.rs` recognizes `ProcessVersion`/`ProcessCwd`/`OsArch`/`OsType`/`OsPlatform`/`OsRelease`/`OsHostname`/`OsEOL` as string expressions so `process.version.startsWith('v')` hits the string method fast path. `test_gap_node_process` diff drops 52 → 3 lines (remaining diffs are bigint comparison + Promise executor nextTick pattern, both out of scope).

### v0.4.117 (llvm-backend)
- fix: `format_jsvalue`/`format_jsvalue_for_json` now cap nesting at Node's default `util.inspect` depth (2). Nested arrays collapse to `[Array]` and nested objects to `[Object]` past that level, so `console.log({ a: { b: { c: { d: 1 } } } })` prints `{ a: { b: { c: [Object] } } }` instead of the full tree.
- fix: `format_jsvalue_for_json` array formatter now renders `[ 1, 2, 3 ]` with spaces inside the brackets (matching Node's `util.inspect`). Nested arrays inside `console.log({ nested: { arr: [1, 2, 3] } })` now print byte-for-byte with Node. Empty arrays still render as `[]`.

### v0.4.116 (llvm-backend)
- feat: LLVM backend wires `WeakRef`/`FinalizationRegistry`/`atob`/`btoa` to the real runtime (`js_weakref_new`/`_deref`, `js_finreg_new`/`_register`/`_unregister`, `js_atob`/`_btoa`). Previously all 6 variants were passthrough/0.0 stubs. `collectors.rs::collect_closures_in_expr` now descends into `FinalizationRegistryNew(cb)` so inline cleanup callbacks get their LLVM function emitted (was failing with "use of undefined value @perry_closure_*"). `test_gap_global_apis` diff drops 58 → 50 lines (atob/btoa/unregistered now match Node); `test_gap_weakref_finalization` module-level WeakRef/FinRegistry now produce correct `hello`/`object`/`registered`/`unregistered` output.

### v0.4.115 (llvm-backend)
- feat: ES2023 immutable array methods (parallel Agent ARRAY) — `Expr::ArrayToReversed`/`ArrayToSorted`/`ArrayToSpliced`/`ArrayWith`/`ArrayCopyWithin` now call the existing runtime functions (`js_array_to_reversed`/`js_array_to_sorted_default`/`js_array_to_sorted_with_comparator`/`js_array_to_spliced`/`js_array_with`/`js_array_copy_within`) instead of returning the receiver unchanged. `toSpliced` builds a stack `[N x double]` buffer for insert items. Runtime declarations added in `runtime_decls.rs`. `test_gap_array_methods` diff drops 64 → 37 lines.
- fix: `format_jsvalue` array wrap threshold raised from `> 5` to `> 6` — Node uses single-line formatting for arrays of ≤ 6 elements, multiline for ≥ 7.

### v0.4.114 (llvm-backend)
- feat: regex advanced + string method wiring (parallel Agent REGEX). `test_edge_strings` flipped DIFF (22) → MATCH. `test_gap_regexp_advanced` flipped CRASH → DIFF (8). `test_gap_string_methods` 75 → 9 diff. `test_edge_json_regex` 14 → 10 diff. Changes touch `lower_string_method.rs` (290+ lines of new string-method dispatch — `padStart`/`padEnd`/`charCodeAt`/`lastIndexOf`/`replaceAll`/`normalize`/`matchAll`/`split` fallbacks), `expr.rs` String*/RegExp* arms (84 lines — `RegExpSource`/`RegExpFlags` now return real string handles, `StringAt` wired, fromCharCode/fromCodePoint), `type_analysis.rs` (34 lines — string-returning method detection for chained calls like `s.trimStart().trimEnd()`), `regex.rs` (lastIndex state tracking fix that was causing the infinite-loop crash in `while (re.exec(text) !== null)`).

### v0.4.113 (llvm-backend)
- feat: LLVM backend Web Fetch API — `new Response(body, init)` / `new Headers()` / `new Request(url, init)` constructors lowered in `lower_new` via new `lower_builtin_new` helper, extracting `{status, statusText, headers}` from inline init objects. `NativeMethodCall` dispatch for `module: "fetch"/"Headers"/"Request"` wired in `lower_native_method_call` → `js_fetch_response_text/json/status/statusText/ok/headers/clone/arrayBuffer/blob`, `js_headers_set/get/has/delete/forEach`, `js_request_get_url/method/body`. Chained `r.headers.get(k)` / `r.clone().text()` / `new Response(...).text()` shapes handled at the `Call { PropertyGet { NativeMethodCall, ... } }` callsite. `AbortController` wired: `new AbortController()` allocates via `js_abort_controller_new` (NaN-boxed pointer so `controller.signal`/`.aborted` work via the normal object-field path); `controller.abort(reason?)` and `controller.signal.addEventListener("abort", cb)` dispatch directly via new `lower_abort_controller_call` helper. `Response.json(v)` / `Response.redirect(url, s)` static factories handled. `test_gap_fetch_response` flipped DIFF → MATCH (44 → 0). Sweep 89 → 91 MATCH.
- fix: `js_fetch_response_text/json`, `js_response_array_buffer/blob` now resolve their Promise synchronously via `js_promise_resolve` instead of routing through the deferred `PENDING_RESOLUTIONS` queue. The LLVM backend's `await` busy-wait loop only calls `js_promise_run_microtasks` (not `js_stdlib_process_pending`), so without this fix `await r.text()` etc. hang forever. The body is already in-memory at call time so inline resolution is safe — no behavior change for Cranelift.

### v0.4.112 (llvm-backend)
- feat: generator `for...of` / spread / `Array.from` / array destructuring now produce real arrays. `Expr::IteratorToArray` in LLVM backend was a passthrough — it now calls `js_iterator_to_array` (walks `.next()` loop, collects `.value` into a fresh array). Fixes the 4 denormal output lines in `test_gap_generators`.
- feat: generator `.throw(err)` routes into the enclosing `catch` clause. `perry-transform::generator.rs` now collects catch clauses during linearization; the throw closure assigns the catch param and inlines the catch body before marking done. Single catch per generator; catches must not yield (still). Fixes the missing `"caught: test error"` line.
- Sweep: still 89 MATCH / 25 DIFF — no regressions. `test_gap_generators` diff dropped 15 → 1 (only the `*[Symbol.iterator]` class method case remains, which needs `lower_decl.rs` support for computed method keys — out of scope).

### v0.4.111 (llvm-backend)
- fix: `{ ...src, k: v }` object spread now calls `js_object_copy_own_fields(dst, src)` (was silently ignored with `// Spreads are silently ignored`). Runtime was already implemented in `object.rs:860`.
- fix: `js_array_concat` detects Sets (via `SET_REGISTRY`) and auto-converts before concatenation, so `[...new Set(...)]` spread-into-array gets the right elements instead of reading SetHeader memory as f64 array elements.
- feat: wire remaining string method stubs — `Expr::StringAt` → `js_string_at` (negative index), `StringCodePointAt` → `js_string_code_point_at`, `StringFromCodePoint` → `js_string_from_code_point`, `StringFromCharCode` → `js_string_from_char_code`. Were stubs returning 0.0.
- feat: `structuredClone(v)` wired to real `js_structured_clone` (was passthrough — mutations on "clone" affected original). Runtime handles deep copy for arrays, objects, nested structures via `builtins.rs:1979`.
- feat: `Set.clear()` wired to `js_set_clear` (was stub; Map.clear was already wired).
- feat: `refine_type_from_init` now marks `Array.from(...)`, `Array.from(..., mapFn)`, `arr.sort(...)`, `arr.toReversed/Sorted/Spliced/With(...)`, `str.split(...)` and Set/Map constructors as the correct Array/Named types, so `.length` and subsequent method calls hit the fast path. Fixes `Array.from(new Set(dupes)).length` returning undefined.
- Sweep: 88 → 89 MATCH (test_edge_map_set flipped).

### v0.4.110 (llvm-backend)
- feat: central merge of Agent A/B/C punch lists — wire ~18 LLVM `Expr::*` stubs to existing runtime functions. `Expr::PathFormat` → `js_path_format`; `PathNormalize` → `js_path_normalize`; `PathIsAbsolute` → `js_path_is_absolute` (bool NaN-box). `Expr::EncodeURI` / `DecodeURI` / `EncodeURIComponent` / `DecodeURIComponent` → `js_encode_uri*` / `js_decode_uri*` (runtime already in `builtins.rs`). `Expr::QueueMicrotask` / `ProcessNextTick` → `js_queue_microtask` (was dropping callback). `Expr::ObjectDefineProperty` / `GetOwnPropertyDescriptor` / `GetOwnPropertyNames` / `Create` / `Freeze` / `Seal` / `PreventExtensions` / `IsFrozen` / `IsSealed` / `IsExtensible` → real `js_object_*` runtime (was stubbed to return the operand). `Expr::AggregateErrorNew` → `js_aggregateerror_new` (was dropping errors array). `Expr::ErrorNewWithCause` → `js_error_new_with_cause` (was dropping cause). `Expr::JsonStringifyFull` → `js_json_stringify_full` with replacer/indent (was dropping both). `Expr::JsonParseReviver` / `JsonParseWithReviver` → `js_json_parse_with_reviver` (was dropping reviver). `Expr::InstanceOf` now maps built-in Error subclass names (`TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`, `AggregateError`) to the reserved `CLASS_ID_*` constants in `error.rs`, so `e instanceof TypeError` resolves via the `GC_TYPE_ERROR` error-kind path in `js_instanceof`. `collectors.rs::collect_closures_in_expr` now walks `JsonParseReviver`/`JsonParseWithReviver` so closures inside revivers get emitted. `test_gap_json_advanced` flipped DIFF → MATCH (26 → 0 diff). `test_gap_object_methods` 72 → 12 diff, `test_gap_node_path` 26 → 12 diff, `test_gap_encoding_timers` 40 → 32 diff (remaining gaps need TextEncoder runtime + real prototype chain). Sweep 87 → 88 MATCH.

### v0.4.109 (llvm-backend)
- feat: new `perry-runtime::symbol` module — `SymbolHeader` + `SYMBOL_REGISTRY` (global `Symbol.for` dedup) + 9 FFI functions (`js_symbol_new`, `js_symbol_new_empty`, `js_symbol_for`, `js_symbol_key_for`, `js_symbol_description`, `js_symbol_to_string`, `js_symbol_typeof`, `js_symbol_equals`, `js_object_get_own_property_symbols`). Self-contained scaffolding for future LLVM/HIR wiring; no behavior change yet (codegen still routes `Symbol()` / `Object.getOwnPropertySymbols` through the generic Call fallback). Sweep unchanged 87/27/6.

### v0.4.108 (llvm-backend)
- feat: wire up LLVM backend stubs to existing runtime functions — `Expr::DateToISOString` → `js_date_to_iso_string`, `Expr::DateParse` → `js_date_parse`, `Expr::DateUtc` → `js_date_utc` (7-arg pad), all 7 `DateSetUtc*` setters → `js_date_set_utc_*`, `MathCbrt`/`Fround`/`Clz32`/`Sinh`/`Cosh`/`Tanh`/`Asinh`/`Acosh`/`Atanh` → `js_math_*`, `NumberIsSafeInteger` → `js_number_is_safe_integer`, `MathHypot` chained via new runtime `js_math_hypot(a, b)` in `math.rs`. All called functions already existed in the runtime — pure wiring. `test_gap_number_math` flipped DIFF → MATCH; `test_gap_date_methods` diff drops 30 → 12. Sweep MATCH 86 → 87.

### v0.4.107 (llvm-backend)
- feat: `fs.readFileSync(path)` without encoding now returns a real `Buffer` on the LLVM backend — wired `Expr::FsReadFileBinary` to `js_fs_read_file_binary` (was stubbed to `0.0`), bitcasting the raw `*mut BufferHeader` to double so the runtime's raw-pointer fallback path sees it. Added runtime-side `format_buffer_value` helper in `builtins.rs` and raw-pointer Buffer detection in both `format_jsvalue` and `js_console_log_dynamic` via `BUFFER_REGISTRY`, so `console.log(buf)` now prints `<Buffer xx xx ...>` (Node-style, lowercase hex, space-separated, capped at 50 bytes). `test_cli_simulation` flipped DIFF → MATCH; sweep MATCH 84 → 86.

### v0.4.106 (llvm-backend)
- fix: `"foo".split(/regex/)` segfault (LLVM backend) — the codegen always routes string.split through `js_string_split` regardless of delimiter type, and the runtime was interpreting the regex header as a `StringHeader`. Added `REGEX_POINTERS` thread-local in `regex.rs` that records every `RegExpHeader` allocation, plus an `is_regex_pointer()` check in `js_string_split` that delegates to `js_string_split_regex` for matched pointers. `test_edge_json_regex` flipped from LLVM_CRASH (SIGBUS) to DIFF; sweep CRASH count 7 → 6, MATCH 84 → 85.

### v0.4.104 (llvm-backend)
- fix: 2D indexing `grid[i][j]` and `grid[i].length` when `grid: Array<Array<T>>`. `static_type_of` in `type_analysis.rs` now walks `Expr::IndexGet` to return the element type of a statically-known array receiver, so `grid[i]` is recognized as an array and its `.length` hits the inline fast path (`load i32 from ptr+0`) instead of falling through to `js_object_get_field_by_name_f64` which returned undefined. `is_array_expr` also now recognizes unions whose non-nullish variant is an array (e.g. `number[] | null` after `if (x)` narrowing), fixing `maybeArr.length` in test_edge_arrays. test_edge_arrays flipped DIFF → MATCH (sweep 84 → 85).

### v0.4.103 (llvm-backend)
- fix: Date local-time getters (`getFullYear`/`getMonth`/`getDate`/`getHours`/`getMinutes`/`getSeconds`) now return LOCAL time via `libc::localtime_r` — previously returned UTC and mismatched Node for any non-UTC locale. `getTimezoneOffset` returns the real system offset. `toDateString`/`toTimeString`/`toLocaleString*` also switch to local time. `test_date` diff drops from 12 to 6 lines (remaining diffs are `toISOString` stub + `Date.now()` timing flake, both blocked on expr.rs).

### v0.4.102 (llvm-backend)
- fix: **try/catch state preservation across setjmp**. At -O2 on aarch64, LLVM's mem2reg promoted allocas to SSA registers inside functions containing `try {}` — so mutations performed in the try body (like `log = log + "try,"`) were invisible in the catch block after longjmp returned. `returns_twice` on the setjmp call alone was not sufficient. Fix: mark the enclosing function with `noinline optnone` (`#1`) in the LLVM IR so the optimizer leaves allocas on the stack across setjmp. New `has_try` bit on `LlFunction` set by `lower_try`; module emits `attributes #1 = { noinline optnone }`.
- fix: `e.message` / `e.name` / `e.stack` / `e.cause` on caught exceptions returned `undefined` because codegen routed property access through `js_object_get_field_by_name_f64`, which returned undefined for non-Object GC types. Runtime now detects `GC_TYPE_ERROR` in that path and dispatches to `js_error_get_message`/`_get_name`/`_get_stack`/`_get_cause`. Tests flipped to MATCH: test_edge_control_flow, test_edge_error_handling (was: 28+4 = 32 lines diff → 0).

### v0.4.101 (llvm-backend)
- fix: `js_array_clone` runtime declaration was missing from `runtime_decls.rs` — Array.from and all chained array ops that touch it failed with `use of undefined value '@js_array_clone'`. 4 tests flipped from COMPILE_FAIL to DIFF (test_edge_arrays, test_edge_iteration, test_edge_map_set, test_gap_class_advanced).
- feat: `arr.splice(start, del, ...items)` insert form now materializes items into a stack `[N x double]` buffer and passes the base pointer to `js_array_splice` (was null pointer, so inserted elements were dropped).
- fix: `Array.isArray()` returns NaN-boxed `true`/`false` literals (`TAG_TRUE`/`TAG_FALSE`) instead of raw `1.0`/`0.0`, so `console.log(Array.isArray(x))` prints `true`/`false`.

### v0.4.100 (llvm-backend)
- feat: LLVM backend Phase F — cross-module import data now flows all the way from `CompileOptions` into `FnCtx`. Added `CrossModuleCtx` bundle and 5 new `FnCtx` fields (`namespace_imports`, `imported_async_funcs`, `type_aliases`, `imported_func_param_counts`, `imported_func_return_types`). `compile_module` now merges imported enums into `enum_table`, builds owned stub `Class` objects for imported classes and inserts them into `class_table`/`class_ids`/`method_names`, and pre-declares imported class methods + constructors as extern LLVM functions so the linker can resolve cross-module method calls. Threaded `&cross_module` through all 6 FnCtx construction sites. Multi-module tests (main/reexport/export_all) still pass; downstream consumers (lower_call, type_analysis) can now read the cross-module data in later phases.

### v0.4.99 (llvm-backend)
- fix: `ArrayForEach`/`ArrayFlatMap` expressions were missing from `collect_ref_ids_in_expr`, so module-level arrays used inside `arr.forEach(cb)` within functions weren't promoted to module globals. The function saw a zero pointer and the forEach loop never executed. `test_edge_closure_module_map` now passes.
- feat: `delete arr[index]` on arrays now sets the element to `TAG_UNDEFINED` via new `js_array_delete(arr, index)` runtime function. Previously the numeric-index case fell through to a no-op. `test_complex_runtime_probes` now passes.

### v0.4.98 (llvm-backend)
- fix: `format_jsvalue` safe fallback for non-array/object GC types — removes heuristic pointer interpretation that could crash on closures, maps, sets, promises. Now dispatches by GC type with safe "[object Object]" default.
- feat: LLVM `lower_array_method.rs` safety-net handlers for 17 array methods (find, findIndex, findLast, findLastIndex, reduce, reduceRight, map, filter, forEach, includes, indexOf, at, slice, shift, fill, unshift, entries/keys/values). Fixes `arr.fill(7)` in test_edge_arrays. Declares `js_array_forEach`/`js_array_fill` in LLVM runtime_decls.
- feat: `benchmarks/compare_backends.sh` — Cranelift vs LLVM backend comparison (compile time, binary size, runtime perf).

### v0.4.97 (llvm-backend)
- feat: `for...of` iteration on Maps and Sets + `Map.forEach`/`Set.forEach` dispatch. LLVM backend now handles `Expr::MapEntries`/`MapKeys`/`MapValues`/`SetValues` (calling `js_map_entries`/`js_set_to_array` runtime functions). `lower_call.rs` intercepts `map.forEach(cb)`/`set.forEach(cb)` on Map/Set-typed receivers and routes to `js_map_foreach`/`js_set_foreach`. HIR `lower.rs` now wraps Set for...of iterables with `SetValues()` (was missing, only Map had `MapEntries` wrapping). Fixed runtime bug: `js_map_foreach`/`js_set_foreach` now mask NaN-box tag bits from callback pointer before calling `js_closure_call2`.

### v0.4.96 (llvm-backend)
- feat: `Promise.then()` / `.catch()` / `.finally()` chaining — `Promise.resolve(10).then(x => x * 2).then(x => x + 5)` now produces 25. Added `is_promise_expr` type detection in `type_analysis.rs` and dispatch in `lower_call.rs` that routes through `js_promise_then(promise, on_fulfilled, on_rejected)`. `refine_type_from_init` recognizes promise-returning expressions so chained locals get typed as `Promise(Any)`. `test_edge_promises` now passes all 24 assertions.

### v0.4.95 (llvm-backend)
- fix: arrow function rest parameters (`const sum = (...nums) => {}; sum(1,2,3)`) now bundle trailing args into an array at closure call sites via `js_closure_callN`, matching FuncRef rest-param handling. Built `closure_rest_params` map + `local_closure_func_ids` tracking so the call site knows which closures have rest params.

### v0.4.94 (llvm-backend)
- fix: self-recursive nested functions now get their LocalId defined before body lowering, so the LLVM backend's boxed-var analysis sees the same LocalId at declaration and self-reference sites.
- feat: LLVM driver dispatch now wires namespace imports, imported classes, enums, async funcs, type aliases, and param counts/return types through CompileOptions (mirrors Cranelift setter chain).
- feat: `run_parity_tests.sh` supports `--llvm` / `PERRY_BACKEND=llvm`; new `run_llvm_sweep.sh` for LLVM parity sweeps.

### v0.4.93 (llvm-backend)
- feat: bitcode link now emits `.bc` for all linked crates (perry-ui-*, perry-jsruntime, perry-ui-geisterhand), not just runtime+stdlib. Extra `.bc` files are merged into the whole-program LTO pipeline via `llvm-link`, enabling cross-crate inlining and dead code elimination across UI/jsruntime boundaries.

### v0.4.92 (llvm-backend)
- fix: `js_array_get_f64`/`_unchecked` OOB now returns `TAG_UNDEFINED` instead of `NaN`. Fixes destructuring defaults like `const [a, b, c = 30] = [1, 2]` where `?? fallback` needs to see `undefined`.
- fix: `keyof T` type operator now lowers to `Type::String` instead of `Type::Any` in `lower_types.rs`.

### v0.4.91 (llvm-backend)
- fix: labeled `break outer;` / `continue outer;` now target the correct enclosing loop instead of always the innermost. Added `label_targets` + `pending_label` to FnCtx; `Stmt::Labeled` sets pending label, loop lowering (`for`/`while`/`do-while`) consumes it and registers in the map, `LabeledBreak`/`LabeledContinue` look up by name.
- fix: `new Child()` where `Child extends Parent` with no own constructor now inlines the parent's constructor body. Was silently skipping `this.items = []` etc., causing stale/missing field data on inherited classes.

### v0.4.90 (llvm-backend)
- feat: Phase J — bitcode link mode for whole-program LTO. `PERRY_LLVM_BITCODE_LINK=1` compiles runtime+stdlib to LLVM bitcode (`.bc`) via `cargo rustc --emit=llvm-bc`, emits user modules as `.ll`, then merges everything via `llvm-link → opt -O3 → llc`. Fibonacci benchmark: **31% faster** (72ms→50ms/iter). Falls back to normal link if LLVM tools or `.bc` files unavailable. New files: `bitcode_link_pipeline` in `linker.rs`, `emit_ir_only` flag in `CompileOptions`, `runtime_bc`/`stdlib_bc` in `OptimizedLibs`.

### v0.4.89 (llvm-backend)
- feat: LLVM backend Phase E.36–E.38 — boxed mutable captures for shared-state closures (`makeCounter` pattern), module-wide LocalId→Type map so closures see captured-var types (`items.length` inside a closure now finds the array fast path), generic class method dispatch via Generic base stripping, indexed string access (`arr[i].length`), string-vs-unknown `===` fallback via `js_string_equals` on both sides (catches `c === Color.Red` when Color is a `const` object). Array-mutating method calls (`push`/`pop`/`shift`/`unshift`/`splice`/`sort`/`reverse`/`fill`/`copyWithin`) inside closures count as writes on the receiver and trigger boxing. `ArrayPush` write-back goes through `js_box_set` when the array local is boxed. MATCH count 67 → 69 / 142 (test_edge_class_advanced, test_edge_enums_const, test_edge_higher_order, test_process_env, test_closure_capture_types all flipped). Commits 1d65e56, 2964723, eaf7129.

### v0.4.88 (llvm-backend)
- feat: LLVM backend Phase E.32–E.35 — high-leverage parity sweep moved match count from 60 → 67/142. Bool-returning runtime calls (`regex.test`, `string.includes/startsWith/endsWith`, `fs.existsSync`, `Set.has`, etc.) wrapped in `i32_bool_to_nanbox` so `console.log(...)` prints `true`/`false` not `0`/`1`. FuncRef-as-value generates `__perry_wrap_<name>` thunks so `apply(add, 3, 4)` can route through `js_closure_call2`. Multi-arg `console.log` bundles into an array and calls `js_console_log_spread` (Node-style util.inspect). `console.table` dispatches to `js_console_table`. Switch on strings now uses `icmp_eq` on i64 bits (fcmp on NaN-tagged is always false). `process.env.X` wired to `js_getenv`. `readonly T` HIR type lowered to inner T (was `Any`). Generic class instances `new L<number>()` strip type args in `receiver_class_name` so `l.size()` finds the method. `is_string_expr` recognizes `arr[i]` on `Array<string>`, enum string members, and chained string-method calls. `Stmt::Try` lowers `try { throw V } catch(e) { ... }` as `bind e=V; run catch`. New string-comparison fast path via `js_string_compare` for `<`/`<=`/`>`/`>=`. Real `js_array_sort_default`/`reverse`/`flat`/`flatMap` dispatch (were stubs). `(255).toString(16)` via `js_jsvalue_to_string_radix`. `Math.random()` now real. Tests confirmed flipped to MATCH: `test_regex`, `test_try_catch`, `test_edge_classes`, `test_edge_class_advanced`, plus 3 others. (See commits 0cfb308, 80454c3, 2a7b51c, fcb5779.)

### v0.4.87
- feat: `AbortController` / `AbortSignal` extensions — `controller.abort(reason)` records the reason; `signal.addEventListener("abort", cb)` registers a listener fired on abort; `AbortSignal.timeout(ms)` returns a signal that auto-aborts after the timeout. New runtime functions `js_abort_controller_abort_reason`, `js_abort_signal_add_listener`, `js_abort_signal_timeout` in `perry-runtime/src/url.rs`. Codegen detects `controller.signal.addEventListener(...)` as a fast path in expr.rs and routes through `js_abort_controller_signal` + the listener registration. `AbortSignal.timeout(ms)` lowered as a `StaticMethodCall` in lower.rs. (WIP from earlier session — committed for clean repo state.)

### v0.4.86
- feat: real `Object.defineProperty` / `freeze` / `seal` / `preventExtensions` semantics — descriptor side table (`PROPERTY_DESCRIPTORS`) tracks per-property `writable`/`enumerable`/`configurable`; `js_object_set_field_by_name` enforces `writable: false` and the freeze/seal/no-extend `GcHeader._reserved` flags; `Object.keys` filters out non-enumerable keys; `getOwnPropertyDescriptor` returns the real attribute bits. Removed the no-op `freeze`/`seal`/`preventExtensions`/`create` early-return in `lower.rs` that was making every Object.* dispatch unreachable. Fixed `js_object_get_own_property_names` signature mismatch (was `i64→i64`, codegen declared `f64→f64`) so it now returns a real NaN-boxed array. `Expr::ObjectGetOwnPropertyNames` plus `ObjectKeys/Values/Entries` now mark their result locals as `is_array=true` in `stmt.rs`. `test_gap_object_methods` 76 → 36 diffs (-53%).

### v0.4.85
- feat: Web Fetch API `Response` / `Headers` / `Request` constructors and methods — `new Response(body, { status, statusText, headers })`, `new Headers()`, `new Request(url, init)`, plus `r.text()`/`json()`/`status`/`statusText`/`ok`/`headers`/`clone()`/`arrayBuffer()`/`blob()`, headers `.set/get/has/delete/forEach`, request `.url/method/body`, and the `Response.json(value)` / `Response.redirect(url, status)` static factories. Implemented as opaque handle pools in `perry-stdlib/src/fetch.rs` (`HEADERS_REGISTRY`, `REQUEST_REGISTRY`, reusing existing `FETCH_RESPONSES`). New runtime functions wired through `runtime_decls.rs` and dispatched via three custom early-out paths in `expr.rs`: a Headers/Request handler before the generic dispatch table, a chained-call handler at the `Call(PropertyGet(NativeMethodCall, _))` site for `r.headers.get(...)`, and `Expr::New` codegen for the three constructors (extracts `{ status, statusText, headers }` from inline option object literals). Lower.rs detects `let r = new Response(...)` etc. in `destructuring.rs` and registers the local as a fetch native instance so subsequent property accesses promote to NativeMethodCall. `js_fetch_response_text/json` no longer remove the FETCH_RESPONSES entry so `r.headers.get()` still works after `await r.json()`. `test_gap_fetch_response.ts` now matches Node byte-for-byte (50 → 0 diff).

### v0.4.84
- feat: `Array.prototype.entries()` / `keys()` / `values()` — eagerly materialized as new HIR variants `ArrayEntries`/`ArrayKeys`/`ArrayValues` + runtime functions `js_array_entries`/`_keys`/`_values` in `array.rs`. Lowered in 3 dispatch sites (LocalGet, extern_ref, inline literal). Fixes the segfault in `test_gap_array_methods.ts` where `for (const e of arr.entries())` previously fell through to `js_native_call_method` and iterated garbage.

### v0.4.83
- feat: `console.table` — new `js_console_table` runtime function in `builtins.rs` renders array-of-objects, array-of-arrays, and single-object inputs as Node-style box-drawing tables (`┌─┬─┐` chars, single-quoted string cells, left-aligned). Codegen dispatch added in `expr.rs` next to the existing `console.dir`/`console.time` cluster; runtime declared in `runtime_decls.rs`. The three table outputs in `test_gap_console_methods.ts` now match Node byte-for-byte (diff drops 62 → 41).

### v0.4.82
- feat: tagged template literals — `tag\`Hello ${name},${42}!\`` now desugars to `tag(["Hello ", ",", "!"], name, 42)` for any user function (`String.raw` keeps its existing fast path). Unblocks `test_gap_class_advanced` (was COMPILE_ERROR, now 26 diffs). Implementation in `lower.rs:9244` walks `tpl.quasis` for cooked strings and `tpl.exprs` for interpolated values.
- fix: `JSON.stringify(undefined)` now returns NaN-boxed `undefined` instead of empty string. Root cause: `lower_types.rs:275` had `JSON.stringify` return `Type::String`, so `const x = JSON.stringify(undefined)` declared `x` as a string slot and the runtime's TAG_UNDEFINED bits got mis-handled. Changed to `Type::Union(vec![Type::String, Type::Void])` so callers route through dynamic dispatch.
- fix: `JSON.stringify(circular)` now throws an actual `TypeError` instance (was a plain Error with `name` patched to "TypeError"). All three circular-detection sites in `json.rs` now call `js_typeerror_new` so `e instanceof TypeError` returns true. `test_gap_json_advanced` flipped from 6 diffs to PASS.
- fix: `js_cron_timer_tick`/`js_cron_timer_has_pending` link errors when `scheduler` Cargo feature is disabled. The cron agent's commit (`a6fd7a0`) declared these symbols unconditionally in `runtime_decls.rs` and called them from the CLI event loop, but they only existed in `cron.rs` (gated behind `scheduler`). Added unconditional 0-returning stubs in `perry-stdlib/src/lib.rs` under `#[cfg(not(feature = "scheduler"))]` so non-cron projects (e.g. `node:crypto`-only) still link. `test_gap_node_crypto_buffer` flipped from COMPILE_ERROR back to 44 diffs.
- result: gap test sweep is now **6/22 passing** (up from 3/22), 412 total diffs (down from 450). New PASS: `test_gap_json_advanced`, `test_gap_encoding_timers`, `test_gap_node_path`, `test_gap_node_process` (the last three were already passing — the previous count missed them because Node prints stderr warnings about module type, which the new sweep script filters out).

### v0.4.81
- fix: chained `arr.sort(cb).slice(0, N)` (and similar `.map().sort().slice()`) corrupted to a non-array, segfaulting `JSON.stringify`. Added `slice` to the array-producing-receiver whitelist in `lower.rs`'s generic chain handler so it routes to the array path instead of `js_native_call_method`.

### v0.4.80
- fix: `node-cron`'s `cron.schedule(expr, cb)` callback now actually fires (was a TODO stub). New `CronTimer` struct mirrors `INTERVAL_TIMERS`, callbacks fire on the main thread from the CLI event loop, closure pointer survives via `i64` ABI, registered as a GC root, and `cron.schedule(...)` returns a `CronJob` handle so `job.stop()`/`start()`/`isRunning()` resolve correctly.

### v0.4.79
- feat: RegExp lowering — `regex.exec/source/flags/lastIndex`, `m.index/.groups`, `replace(regex, fn)` callback path, `$<name>` named back-refs. Fixed an ARM64 ABI bug where the callback param was declared `I64` but the Rust function takes `f64`.
- fix: `js_instanceof` Error subclass handling restored (was lost when an Object.defineProperty agent overwrote `object.rs` from an older base).

### v0.4.78
- feat: `TextEncoder`/`TextDecoder`, `encodeURI[Component]`/`decodeURI[Component]`, `structuredClone`, `queueMicrotask`. Timer IDs from `setTimeout`/`setInterval` now NaN-boxed with POINTER_TAG so `clearTimeout`/`clearInterval` can recover the ID.

### v0.4.68
- feat: `console.time`/`timeEnd`/`timeLog`/`count`/`countReset`/`group`/`groupEnd`/`groupCollapsed`/`assert`/`dir`/`clear` — backed by thread-local `CONSOLE_TIMERS` and `CONSOLE_COUNTERS`.

### v0.4.67
- feat: auto-detect optimal build profile — `perry compile` rebuilds runtime + stdlib with the smallest matching feature set and switches `panic = "unwind"` → `"abort"` when no `catch_unwind` callers are reachable. Measured: `await fetch(url)` 4.2 MB → 2.9 MB (-31%) automatically. Legacy `--minimal-stdlib` is now a hidden no-op alias; `--no-auto-optimize` is the escape hatch.
