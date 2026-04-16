# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.63

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
