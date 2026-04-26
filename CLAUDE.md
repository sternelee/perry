# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.296

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

Generational mark-sweep GC in `crates/perry-runtime/src/gc.rs` (default since v0.5.237 / Phase D). Two regions in the per-thread arena: nursery (`ARENA`, fills with new allocations, swept on minor GC) and old-gen (`OLD_ARENA`, holds tenured/evacuated objects). Conservative stack scan + precise shadow-stack roots + 9 registered scanners. Write barriers populate a remembered set so minor GC can avoid retracing the old-gen. Two-bit aging (`HAS_SURVIVED` / `TENURED`) promotes nursery survivors after 2 minor cycles; the C4b evacuation pass moves non-pinned tenured objects into old-gen with full reference rewriting. Idle nursery blocks observed empty for 2 GC cycles are `dealloc`'d back to the OS (C4b-δ, v0.5.235), and the next-trigger calc is hard-capped at the initial threshold (64 MB) so >90%-freed step-doubling can't blow up peak occupancy (C4b-δ-tune, v0.5.236). Triggers on arena block allocation (1 MB blocks since v0.5.196), malloc count threshold, or explicit `gc()` call. 8-byte GcHeader per allocation.

**Escape hatches**: `PERRY_GEN_GC=0`/`off`/`false` reverts to full mark-sweep (bisection only). `PERRY_GEN_GC_EVACUATE=1` enables the copying evacuation pass (default OFF — complete and correctness-safe but adds work that's a no-op on workloads where nothing tenures). `PERRY_WRITE_BARRIERS=1` opts into codegen-emitted write barriers (default OFF — barrier emission has its own perf cost; the runtime barrier always exists). `PERRY_GC_DIAG=1` prints per-cycle diagnostics.

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

- **v0.5.296** — Add `.github/FUNDING.yml` with `ko_fi: perryts` so the repo's Sponsor button routes to Ko-fi while the GitHub Sponsors application is in review.
- **v0.5.295** — Linux build fix: `find_clang()` / `find_llvm_tool()` (`crates/perry-codegen/src/linker.rs`) now search common Linux LLVM install prefixes (`/usr/lib64/rocm/llvm/bin`, `/usr/lib/llvm-{17,18,19}/bin`) alongside the existing Homebrew/Windows paths, so `.ll` → `.o` works without `PERRY_LLVM_CLANG`. Removed 3 AOT stubs (`js_sqlite_transaction`, `_commit`, `_rollback`) from `perry-runtime/src/closure.rs` — they collided with the real implementations in `perry-stdlib/src/sqlite.rs` and broke `cargo test --workspace` with duplicate-symbol linker errors under GNU ld (origin's macos-only main test job didn't catch it).
- **v0.5.294** — Release-blocker fix surfaced by v0.5.293's failed publish: `_js_stdlib_process_pending` link error on macOS doc-tests + iOS simulator tests. Root cause was Cargo feature unification — when CI's auto-optimize runs `cargo build -p perry-runtime -p perry-stdlib` together, perry-stdlib's `Cargo.toml` declares `perry-runtime = { features = ["stdlib"] }`, which activates `stdlib` on perry-runtime. That activates the `#[cfg(not(feature = "stdlib"))]` gate at `crates/perry-runtime/src/lib.rs:65` and excludes `mod stdlib_stubs;`, removing `_js_stdlib_process_pending` from `libperry_runtime.a`. Perry's compile then enters runtime-only link mode (libperry_runtime.a + libperry_ui_macos.a, no libperry_stdlib.a) and the symbol is undefined. Local builds didn't catch it because `cargo build -p perry-runtime` alone doesn't unify with stdlib's feature requirements. Fix: 18 files across `perry-ui-{macos,ios,tvos,visionos,gtk4}` switched from hard-linking `js_stdlib_process_pending` to calling `js_run_stdlib_pump`, the existing always-exported trampoline at `lib.rs:121` that dispatches via the registered-callback pattern (same shape `js_stdlib_has_active_handles` already uses). Also re-added `test_gap_console_methods` to `test-parity/known_failures.json` as `ci-env` — the v0.5.290 drop was premature; it passes locally through `normalize_output` but the CI runner produces a variant that escapes it. Re-tagging for `release-packages.yml` since v0.5.293's GH release shipped no binaries.
- **v0.5.293** — Repo hygiene: untrack 465 Android Gradle cache files (`android-build/.gradle/`, `android-build/app/build/`, `android-build/build/`) that were churning on every Gradle invocation, and add the matching `.gitignore` rules. Also gitignored: `docs/examples/_reports/` (CI-generated doc-test report), `/assets/` + `benchmarks/suite/assets/` (external game-project assets the user keeps adjacent for perry-ui-* manual testing — never source), and stray repro binaries `enum_repro`/`no_pragma_test`. Bench methodology: `json_polyglot/run.sh` precompiles Node TS to `.mjs` (esbuild → npx-esbuild → tsc fallback chain) as untimed setup so Node isn't charged for `--experimental-strip-types`'s per-launch parse on every run — Perry is AOT and Bun strips natively, so neither pays this; falls back to the old `--experimental-strip-types` invocation with a banner if no stripper is available. `polyglot/bench.rs` gains an FP-contract caveat block on `bench_loop_data_dependent` documenting the FMA-contract (Apple Clang, Go) vs no-contract (Rust, Swift, Perry, Node, Bun, Java) clustering. Plus `tests/test_array_index_loop.sh` runner companion to the existing `.ts` regression test.
- **v0.5.292** — CLAUDE.md hygiene: migrated 124 verbose Recent Changes entries (~242 KB) to CHANGELOG.md verbatim, condensed the section to the last 22 versions at 1-2 lines each. CLAUDE.md 254 KB → 12 KB (95% reduction). Save the always-loaded context budget for actual project guidance.
- **v0.5.291** — Land the actual workflow code for v0.5.289's CI disk-space fix.
- **v0.5.290** — Stub audit: `test_gap_console_methods` removed from `known_failures.json` — passes through the parity-runner's `normalize_output` despite the raw diff showing different timer values.
- **v0.5.289** — CI hygiene: stop the `Tests` workflow's macos-14 jobs from OOM'ing on disk space.
- **v0.5.288** — Stub audit: `test_json` removed from `known_failures.json`, incidentally fixed by v0.5.286's `JSON.stringify(<plain f64>)` segfault fix.
- **v0.5.286** — Stub audit: `JSON.stringify(<plain f64>)` segfaulted.
- **v0.5.285** — Bench docs prose pass on `benchmarks/README.md`.
- **v0.5.284** — Stub audit: two correctness bugs in the Promise microtask runner.
- **v0.5.283** — Bench docs: rewrote the f64 bullet in `benchmarks/README.md` §Strengths so it doesn't carry contradictory framing.
- **v0.5.281** — Stub audit: two distinct bugs in the NaN/number-formatting family.
- **v0.5.280** — Stub audit: NaN/Infinity ToInt32 coercion.
- **v0.5.279** — #187 follow-up (stub audit): SSO + property-read NaN bug.
- **v0.5.278** — Stub audit: `is_inlinable` in `crates/perry-transform/src/inline.rs:213` was inlining functions with rest parameters even though the inliner's `param_map` mechanism only handles 1:1 formal-to-actual arg mapping — so `function…
- **v0.5.277** — Stub audit: `fs.readFileSync(path)` (no encoding) now returns a real Buffer, matching Node.
- **v0.5.276** — Bench docs: footnote on `04_array_read`'s 211 MB peak RSS row + new `benchmarks/polyglot/ARRAY_READ_NOTES.md` with analytic working-set math (10M f64 doubling fill, 8M-cap + 16M-cap coexist mid-grow → 192 MB arena peak + ~13 MB overhead),…
- **v0.5.275** — #187 follow-up: async-factory pattern for `pg`'s `Client`/`Pool` and `mongodb`'s `MongoClient` — the npm-compatible `new T(config); await t.connect()` shape.
- **v0.5.274** — Bench credibility: add the two comparison rows the page was missing.
- **v0.5.273** — Stub audit: closure-null family fix.
- **v0.5.272** — Bench refactor (code landing): the v0.5.271 entry below described two new benchmarks and a README restructure, but only metadata changes (CLAUDE.md, Cargo.toml) actually shipped under v0.5.271 due to a race during commit.
- **v0.5.271** — Bench refactor: add the two benchmarks that address the most-likely skeptic objections to this README within 30 seconds of reading it.
- **v0.5.270** — #187 follow-up: `Redis` (ioredis) end-to-end + fixes a pre-existing dispatch-table-symbol-mismatch bug.

Older entries → CHANGELOG.md.
