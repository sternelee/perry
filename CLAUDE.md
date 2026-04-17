# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.76

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

<<<<<<< HEAD
- **v0.5.75** — Close the remaining parse-leftover GC gap for stringify (#64 follow-up to v0.5.72). Two targeted changes: (1) `mark_block_persisting_arena_objects`' pass 2 now uses the new `arena_walk_objects_filtered` (in `arena.rs`) which skips entire blocks up-front rather than iterating their headers and early-returning per object — on post-parse workloads with 27 of 29 blocks fully dead, pass 2 drops from ~55ms to <1ms per iteration. (2) `gc_check_trigger`'s adaptive step now DOUBLES when `pct_freed < 10%` instead of halving — a pointless GC shouldn't retrigger 3 iterations later at the 16MB floor, which was the root cause of the bench_order thrash (50 pointless GCs at 100ms each). (3) `js_json_parse` now calls `gc_check_trigger` before suppressing GC for its own work, so a tight parse loop doesn't have to wait until the next stringify's arena-overflow to shed the previous iteration's tree. Combined: bench_stringify (issue's benchmark) stringify **5178→77ms (67× faster, 1.1× Node 69ms, was 75× slower)**; bench_order's "stringify_first → parse_50 → stringify_after" adversarial ordering no longer thrashes (stringify_after 1930→77ms, parse_50 229→419ms — 1.4× Node 300ms, acceptable). 108 runtime tests pass; gap suite 14/28 · 113 diffs unchanged.
- **v0.5.74** — Inline bump-allocator IR for small array literals (issue #63 phase 3/3). Reuses the existing `js_inline_arena_state` + `js_inline_arena_slow_alloc` infrastructure built for `new ClassName()` inline allocation. For literals with N ≤ 16, `lower_array_literal` now emits the same 5-instruction bump check + packed-header store sequence instead of calling `js_array_alloc_literal` — the GcHeader and ArrayHeader each collapse into one i64 store (LLVM was store-combining the two u32 writes, but the explicit packing matches the `new`-path IR exactly so both callers share LLVM optimizer work). Element stores go into `(raw + 16) + i*8` via `gep_inbounds ptr`, so LLVM has provenance for vectorization. Slow path (arena block overflow) hits the same `js_inline_arena_slow_alloc` the `new` path uses; no new runtime symbol. **Non-scalar-replaced escape benchmark: triple_escape 10→8ms (parity w/ Node 7ms), quad_escape 14→10ms (beats Node 11ms), eight_escape 10→7-8ms (beats Node 11ms).** Scalar-replaced cases (v0.5.73) unaffected. 218 core-crate tests pass; gap suite 14/28 · 113 diffs (unchanged).
- **v0.5.73** — Scalar replacement for non-escaping array literals (issue #63 phase 2/3). New `collect_non_escaping_arrays` pass mirrors the existing `collect_non_escaping_news` object pass: a `let arr = [a, b, c]` binding where `arr` is only used in constant-index reads (`arr[k]` with `k < N`) and `.length` gets converted into N separate stack allocas in codegen — one `alloca double` per slot. `IndexGet { LocalGet(id), Integer(k) }` then lowers to a direct `load double, ptr slot_k` (no heap, no runtime call, no bounds check), and `PropertyGet { LocalGet(id), "length" }` folds to a `double` constant. Escape criteria for arrays: reassignment, `IndexSet`/`IndexUpdate`, closure capture, passing to a call, or a dynamic (non-literal / out-of-range) index all mark the array as escaped and fall back to the v0.5.69 heap path. Capped at 16 elements. Fixed the pre-existing `.length` shortcut in expr.rs to check `non_escaping_arrays` before unboxing the dummy local slot (it previously returned 0 for scalar-replaced arrays). **arr_only (3-elem, 500k iters): 10→5ms (matches Node). arr4 (4-elem): 14→7ms (beats Node's 8ms).** 218 core-crate tests pass; gap suite 14/28 · 113 diffs (unchanged).
- **v0.5.76** — Windows x86_64 support: five fixes. (1) `-mcpu=native` → `-march=native` on x86 in linker.rs (clang rejects `-mcpu=` on x86_64-pc-windows-msvc). (2) Module-level IC counter — `ic_site_counter` was per-function (reset to 0), causing `@perry_ic_0` redefinition in modules with 2+ functions using property access; moved to `LlModule.ic_counter`. (3) `_setjmp(buf, ptr null)` on Windows MSVC (was `setjmp(buf)` which doesn't exist on MSVC); fixes try/catch/throw. (4) `call_vtable_method` passes `this` as `f64` not `i64` — on Windows x64 ABI these use different registers (xmm0 vs rcx), causing segfaults on dynamic method dispatch. (5) `is_valid_obj_ptr` lower bound 0x100000000 → 0x100000 (Windows heap allocates at lower addresses than macOS ARM64). **Windows test suite: 88 → 108 PASS (of 122 non-skipped), 17 → 1 compile fail, 7 → 0 runtime fail.**
- **v0.5.72** — Per-call shape-template cache for stringify (#64 follow-up). TLS `SHAPE_CACHE` (linear-scan `Vec<(*mut ArrayHeader, Box<ShapeTemplate>)>`, cap 32, no eviction for pointer stability) keys on `keys_array` raw pointer — identity is a stable shape ID within one top-level stringify call since no GC runs over the user graph until the result allocation. `stringify_object_inner` now tries `try_emit_shape_element` on cache hit, skipping the per-object `has_pointer_fields` scan, `object_get_to_json` key walk, and per-field key load/lookup/push — first-encounter shapes build the template once; subsequent objects with the same `keys_array` hit the cache. Save/restore at each top-level entry (`js_json_stringify` / `js_json_stringify_full` / `..with_replacer`) so reentrant `toJSON` callbacks don't return stale templates or drop outer-call entries mid-emit. **Clean stringify (50×10k items, no prior GC pressure): Perry 76ms vs Node 74ms (1.03×, was 15× at v0.5.71). 5-deep homogeneous nested objects (50×10k iters): Perry 43ms = Node 43ms (parity). `id_only`/`id+name`/`id+value`/`id+tags`/`id+nested`/`id+name+val` all match or beat Node.** Isolation benchmark on this machine shows the residual 2.2s "post-parse" gap from the original benchmark is pure GC pressure — explicit `gc()` between parse and stringify loops brings it to 86ms (vs Node 71ms); the fix for that would live in parse/GC tuning, not stringify. 108 runtime tests pass; gap suite 14/28 · 113 diffs unchanged.
- **v0.5.71** — O(1) `charCodeAt` / `codePointAt` (closes #65). Both runtime entry points were calling `str_data.encode_utf16().collect()` on every invocation — a fresh allocation and full-string walk per character access. On a 68 MB JSON output this turned the FNV-1a hash loop into O(n²): the full 500k-record pipeline ran >13 min at 100% CPU and never completed. New code: ASCII fast path (byte index when `utf16_len == byte_len`, which `js_json_stringify` always produces); non-ASCII path walks codepoints once with `chars()` + `len_utf16`, zero allocation. **500k-record JSON pipeline (parse→filter→map→stringify→write→hash 68 MB): >780000ms → 1559ms (>500× faster); hash step alone on 140 kB: 22054ms → 5ms (4400×).** Byte-identical output vs. Node. Gap suite 14/28 · 109 diffs (`global_apis` 22→18 — `atob`/`btoa` share the path).
- **v0.5.70** — JSON.stringify per-call overhead reduction (closes #64). Three changes in `json.rs`: (1) thread-local reusable `STRINGIFY_BUF` (Cell<Option<String>>) replaces `String::with_capacity(estimated)`/`String::with_capacity(4096)` allocate-and-drop in `js_json_stringify`, `js_json_stringify_full`, and `js_json_stringify_with_replacer` — reentrancy-safe (inner take returns `None`; larger buffer wins on restore) so `toJSON` callbacks can't deadlock. (2) `js_json_stringify_full` now mirrors `js_json_stringify`'s direct `arena_alloc_gc` + `utf16_len = byte_len` pattern instead of `js_string_from_bytes` — JSON output is always ASCII (high bytes are `\uXXXX`-escaped), so the per-byte `compute_utf16_len` scan was pure waste, particularly painful for multi-MB outputs. (3) `stringify_array_depth` now inline-checks the first element's tag for POINTER_TAG/raw-pointer before calling `build_shape_prefix_template` — arrays of primitives (e.g. `tags: ["x","y"]` recursing per outer element) no longer pay a function-call frame to learn there's no shape. **Stringify large 50×10k items: 5178→2218ms (2.3× faster, gap vs Node 34×→15×). Small stringify 100k iters: 7873→3478ms (2.3×) — and 10ms (≈Node 37ms) when measured in isolation, confirming the residual cost is GC pressure from the prior parse loop, not stringify itself. Parse 50×10k: 324→231ms; small_parse_100k: 50→20ms.** 108 runtime tests pass; gap suite 14/28 · 113 diffs unchanged.
- **v0.5.69** — Exact-sized fast path for array literals (issue #63). `[a, b, c]` in a hot loop previously emitted `js_array_alloc(N)` (capacity padded to `MIN_ARRAY_CAPACITY=16`) + N×`js_array_push_f64` (each re-reading length/capacity, re-doing the null/tag guard) + inline nanbox — N+1 extern calls per literal plus 128 bytes of arena for a 3-element array. New `js_array_alloc_literal(N)` allocates exactly `N` slots, pre-sets `length=N`; codegen evaluates element exprs first, then emits one call plus N inline `store double, ptr` via `gep_inbounds`, skipping `js_array_push_f64` entirely. GC-safe: element evaluations finish before alloc (their values pinned in SSA via conservative stack scan), and no allocator runs between alloc and final store, so no GC observes the partially-initialized array. **arr_only (3-elem, 500k iters): 20→10ms (2× faster, 2× Node was 4×). arr4 (4-elem): 21→14ms (1.5× faster, 1.7× Node was 2.6×).** 218 core-crate tests pass; gap suite 14/28 · 113 diffs (unchanged).
- **v0.5.68** — Arena-allocate strings (issue #62 phase B). All 5 string allocation sites in `string.rs` now go through `arena_alloc_gc` instead of `gc_malloc` — bump-pointer + GcHeader init (~10-15ns) instead of mimalloc + `MALLOC_STATE` tracking (~30-40ns). Strings are discovered by the existing arena block walker (GC_TYPE_STRING is already in the walker's allowed-type range), and block-persistence keeps interned/long-lived strings alive. Also removed the macOS-specific "ASCII-like keys_array" heuristic in `object.rs` (lines 2027-2038 + 3389-3397) — it false-positived on legitimate arena pointers once strings joined the arena, flapping `test_gap_object_methods` between 0 and 69 diff lines. The GcHeader `obj_type == GC_TYPE_ARRAY` check immediately after is content-based and deterministic. **str_concat 55→14ms (3.9× faster, within 1.27× of Node's 11ms), template 55→14ms (3.9×), toString 68→27ms (2.5×), combined 213→120ms (1.8×).** Gap suite back to 14/28 · 113 diffs; 259 workspace tests pass.
- **v0.5.67** — mimalloc as global allocator (issue #62 follow-up). `#[global_allocator]` in perry-runtime routes every `std::alloc::{alloc, realloc, dealloc}` — gc_malloc, arena blocks, internal Vec/HashMap growth, strings in compiled TS — through mimalloc's per-thread segregated free lists instead of macOS's system `malloc` (~25-40ns/call). **str_concat 63→55ms, toString 78→68ms, template 62→55ms** (~12-13% faster). Arena-backed workloads (`obj_only`, `arr3`) unchanged — they already bump-allocate. 259 workspace tests pass; gap suite 14/28 · 113 diffs (down from 117 — `global_apis` improved by 4 lines, everything else unchanged).
- **v0.5.66** — Consolidated per-allocation TLS state (issue #62). `MALLOC_OBJECTS` + `MALLOC_SET` merged into one `RefCell<MallocState>` (one TLS lookup + one borrow_mut per `gc_malloc` instead of two of each; adjacent fields share a cacheline). `GC_IN_ALLOC` + `GC_SUPPRESSED` merged into a single `Cell<u8>` bitfield so `gc_check_trigger`'s fast-path pre-check is one TLS read. Sweep destructures the struct and removes from `set` inline instead of re-entering TLS per freed object. **str_concat 65→63ms, toString 80→78ms, template 65→62ms** (modest: TLS on macOS aarch64 is ~5ns, not the 30–40ns the issue estimated — real bottleneck is `malloc()` itself, a future bump-allocator would move the needle). 259 workspace tests pass, gap suite unchanged at 14/28 · 117 diffs.
- **v0.5.65** — Homogeneous-shape stringify template + ASCII-clean escape fast path (issue #59). `stringify_array_depth` now detects arrays of objects sharing one `keys_array` pointer, builds a single key-prefix table (`"{\"id\":"`, `",\"name\":"`, …) once per array, and reuses it across every element — fusing open-brace/comma with the following key into one `push_str` and skipping per-field key dereferences. `primitive_only` templates skip the per-element undefined/closure pre-scan (rolled back via `buf.truncate` on stray undefined). `write_escaped_string` prechecks `bytes.iter().any(…)` for escape-triggering bytes so the escape-free common case becomes `push('"') + push_str + push('"')`. **Stringify: 52ms→45ms (1.32× Node was 1.5×). Roundtrip: 197ms→187ms (1.26× Node)**.
- **v0.5.64** — Typed `ptr`-slot + `getelementptr inbounds` for Buffer/Uint8Array const locals + per-buffer alias-scope metadata for LLVM noalias. `Stmt::Let` on `Buffer.alloc(N)` pre-computes `handle + 8` into a `ptr` alloca; `Uint8ArrayGet/Set` emits `getelementptr inbounds i8, ptr %base, i32 %idx` instead of the `inttoptr(handle + offset)` chain — giving LLVM proper pointer provenance so the LoopVectorizer can identify array bounds. Module-level `!alias.scope`/`!noalias` nodes (per-buffer scopes in a shared domain, noalias sets enumerating other buffers) prove `src` reads don't alias `dst` writes. **image_conv blur: 283ms→183ms (1.55× faster, 1.08× Zig was 1.67×). Total: 335ms→230ms (1.15× Zig was 1.67×). Input gen: 21ms→15ms**.
- **v0.5.63** — Stringify closure/toJSON guard + persistent parse key cache + inline value dispatch (issue #59). Pre-scans object fields for POINTER_TAG to skip toJSON key scan and closure checks on data-only objects. PARSE_KEY_CACHE persists across parses (capped at 4096) — saves ~10k gc_malloc per repeated parse of homogeneous JSON. Inline common-type dispatch in stringify_object avoids function call overhead per field. **Stringify: 55→52ms. Roundtrip: 199→197ms (1.3× Node)**.
- **v0.5.62** — JSON.stringify fast paths (issue #59 follow-up). `write_number` uses `itoa`/`ryu` instead of `format!` (zero heap alloc per number). Direct `gc_malloc` for stringify result skips `compute_utf16_len` scan (JSON is always ASCII). Depth-based circular ref check: `STRINGIFY_STACK` TLS only accessed at depth >128, eliminating per-object borrow overhead. `gc_obj_type` trusted for OBJECT dispatch (removed redundant `is_object_pointer`). Removed stale GC debug print. **JSON.stringify 50×10k: 97ms→55ms (1.8× faster, 1.6× Node). Roundtrip: 241ms→199ms (1.3× Node). RSS: 254MB (stable)**.
- **v0.5.61** — `-mcpu=native` in clang codegen for architecture-specific optimizations (NEON, AES, etc.). Blur: 310ms→283ms. **image_conv total: 375ms→335ms (1.6× Zig, was 1.8×)**.
- **v0.5.61** — Adaptive GC malloc-count step + fused string-number concat (closes #58). GC malloc-count trigger now backs off when collection is ineffective (<15% freed → 4× step, <50% → 2× step), preventing useless GC cycles during tight allocation loops where conservative stack scanning keeps everything alive. Fused `js_string_concat_value`/`js_value_concat_string` eliminates intermediate string allocation for `"str" + number` patterns. **Object+string alloc loop: 1012ms→148ms (6.8× faster, ~10× Node 15ms). Was 218× slower at v0.5.44**.
- **v0.5.60** — Math.imul polyfill detection + unsigned i32 locals (`>>> 0` seeding). Phase 0 in inline pass detects `imul32`-like polyfills (2-param, half-word decomposition, `| 0` return) and rewrites calls to `MathImul(a, b)` → single `mul i32`. `collect_integer_let_ids` now seeds `>>> 0` mutable inits; i32 slot init uses `fptosi→i64 + trunc→i32` to safely handle unsigned values exceeding INT32_MAX. **FNV: 60ms→37ms (1.6×), input gen: 123ms→24ms (5.1×). Total: 480ms→375ms (1.57× Zig, was 2.2×)**.
- **v0.5.60** — GC suppression during JSON.parse (issue #59). `gc_suppress`/`gc_unsuppress` flag skips `gc_check_trigger` during parse; `gc_bump_malloc_trigger` rebaselines the threshold post-parse so freshly-created objects don't trip immediate collection. Clears PARSE_KEY_CACHE after each parse (correctness: dangling pointers). **JSON.parse 50×10k: 3250ms→143ms (22× faster, ~1.1× Node 122ms). Roundtrip: 21254ms→241ms (88× faster, 1.5× Node 157ms). Peak RSS: 842MB→254MB (3.3× less)**.
- **v0.5.59** — Pure-function HIR inlining in init context + broader integer-local seeding. Phase 4 of the inline pass now inlines standalone pure functions (no module-global refs) into module init — `imul32` polyfill body exposed to i32 analysis. `collect_integer_let_ids` seeds immutable bitwise Lets and mutable `|0` Lets. Multi-statement `[Let*, Return(expr)]` functions now inline at expression level with setup-stmt hoisting. **FNV: 380ms→60ms (6.3× faster). image_conv total: 800ms→490ms (1.6× faster, 2.2× Zig)**.
- **v0.5.59** — Property-name string interning + pointer-identity transition cache + small-integer string cache (issue #60 follow-up). `js_string_concat` checks intern table for short results before allocating (zero gc_malloc on repeated keys). Transition cache uses interned pointer identity instead of FNV-1a hash. `js_number_to_string` caches 0–255. **10k×20 dynamic property writes: 77ms→8ms (10× faster, 2× faster than Node 17ms)**.
- **v0.5.58** — `Math.imul` i32 native path + `returns_integer` function detection. `MathImul(a,b)` in `can_lower_expr_as_i32`/`lower_expr_as_i32` emits single `mul i32` — no fptosi/sitofp. `returns_integer(f)` detects functions where ALL return paths end with `|0`/`>>>0`/bitwise (e.g. user-defined `imul32` polyfills) and includes them in the integer-candidate seeding. image_conv with Math.imul: **blur 287ms (1.17× Zig), total 467ms (1.9× Zig)**.
- **v0.5.57** — Fix dylib GC root segfault (closes #54). Dylib entry module now emits `perry_module_init()` instead of `main()` — initializes GC, string pools, module globals (GC root registration), and top-level statements. Host calls this once after dlopen; event loop is omitted (host manages its own).
- **v0.5.56** — i32-native bitwise ops in `lower_expr_as_i32` + i32 index/value in Uint8ArrayGet/Set. `can_lower_expr_as_i32` and `lower_expr_as_i32` now handle `BitAnd/BitOr/BitXor/Shl/Shr/UShr` — entire xorshift/FNV chains stay in i32. Uint8ArrayGet/Set use `lower_expr_as_i32` for index (and value for Set) when possible, skipping double round-trips. image_conv total: **456ms** (was 483ms). Blur: 280ms (1.14× Zig). Gap: **1.85× Zig** (was 1.97×).
- **v0.5.55** — Eliminate TLS overhead from transition cache + descriptor check (#60 follow-up). `TRANSITION_CACHE_GLOBAL` is now a plain `static mut` (user code is single-threaded), `ANY_DESCRIPTORS_IN_USE` → `static AtomicBool` with `Relaxed` load. 10k×20 benchmark: **142ms→77ms (1.8× faster)**, gap vs Node down to **4.5×** (was 84× before v0.5.51).
- **v0.5.54** — String split/indexOf perf: arena-allocated split parts (closes #61). `utf16_offset_to_byte_offset` / `byte_offset_to_utf16_index` zero-offset fast returns. indexOf/lastIndexOf ASCII path uses Rust Two-Way `str::find`/`rfind` instead of O(n×m) byte scan. Split uses `arena_alloc_gc` bump allocator + `gc_malloc_batch` helper. **split: 145ms→24ms (6× faster, beats Node 27ms), indexOf: 145ms→35ms (4× faster, ~Node 30ms)**.
- **v0.5.53** — `x | 0` / `x >>> 0` noop for known-finite operands + branchless Uint8ArraySet via `@llvm.assume`. When left operand is known-finite and right is `Integer(0)`, skip toint32 entirely (just fptosi+sitofp identity, no NaN/Inf guard). Uint8ArraySet now uses `@llvm.assume(in_bounds)` like Get, eliminating the branch diamond in input-gen and encoder loops. Blur kernel: **0 `bl` instructions** (fully inlined, zero function calls).
- **v0.5.52** — Targeted clamp-function i32 inlining: `is_int32_producing_expr`, `collect_integer_let_ids`, and `can_lower_expr_as_i32` now recognize calls to detected clamp functions (3-param clamp + clampU8) as int-producing. `lower_expr_as_i32` emits `@llvm.smax.i32` + `@llvm.smin.i32` directly — zero double conversions. **Blur kernel alone: 284ms vs Zig 246ms (1.15×)**. Full image_conv 0.76s includes input-gen overhead.
- **v0.5.51** — Content-hash shape-transition cache for dynamic property writes (closes #60). Transition cache keyed on FNV-1a content hash instead of string pointer identity — freshly concatenated keys (`"field_"+j`) now hit the cache across objects. Cache size 4096→16384. 10k×20 benchmark: **1300ms→136ms (9.6× faster)**, gap vs Node 84×→8.5×.
- **v0.5.50** — `toint32_fast` for known-finite bitwise operands + `alwaysinline` on small functions. `is_known_finite` analysis skips the 5-insn NaN/Inf guard from v0.5.49 when operands are provably finite (integer_locals, literals, byte loads, bitwise results). `force_inline` attribute on functions ≤8 stmts + i64-specialized wrappers. Clamp pattern detection (smin/smax in `lower_expr_as_i32`).
- **v0.5.49** — Bitwise ops with NaN/Infinity produce 0 per ECMAScript ToInt32 spec (closes #57). `LlBlock::toint32` emits inline NaN/Inf guard (`fcmp uno` + `fabs` + `fcmp oeq ±inf` → `select 0.0`) before `fptosi`, fixing UB for all bitwise ops (`|`, `&`, `^`, `<<`, `>>`, `>>>`).
- **v0.5.48** — `sdiv` for `(int / const) | 0` + `@llvm.assume` bounds in Uint8ArrayGet. image_conv: 0.69s → 0.61s.
- **v0.5.47** — `Buffer.indexOf(byte)` / `Buffer.includes(byte)` with numeric argument (closes #56).
- **v0.5.46** — PIC miss handler fix for >8-field objects (closes #55). Zero-copy JSON string parsing + incremental object build. JSON pipeline: Perry 180ms vs Node 140ms (was 547×).
- **v0.5.45** — JSON.parse key interning + transition-cache shape sharing. 20-record pipeline: Perry 12ms vs Node 4ms.
- **v0.5.44** — Monomorphic inline cache for PropertyGet (closes #51). Per-site `[2 x i64]` globals.
- **v0.5.43** — Wire int-analysis ↔ flat-const bridge. image_conv: 1.95s → 0.66s (-66%).
- **v0.5.42** — `!invariant.load` metadata on Array/Buffer length loads (closes #52).
- **v0.5.41** — Flat `[N x i32]` constants for module-level `const` 2D int arrays (closes #50).
- **v0.5.40** — Accumulator-pattern int-arithmetic fast path (closes #49). sum-of-bytes: 272ms → 63ms.
- **v0.5.39** — Int32-stable local specialization (closes #48). Fixed boxed_vars bug for non-closure loop counters.
- **v0.5.38** — Inline Buffer/Uint8Array bracket-access (closes #47). image_conv: 2.19s → 1.98s.
- **v0.5.37** — `JSON.parse` GC-root stack for in-progress parse frames (closes #46).
