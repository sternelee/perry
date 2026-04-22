# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.156

## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 22 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last sweep:** **22/28 passing**, **29 total diff lines** (re-validated at v0.5.152, unchanged since v0.5.142's async closure fix).

| Status | Test | Diffs |
|--------|------|-------|
| ✅ PASS | `array_methods`, `bigint`, `buffer_ops`, `class_advanced`, `closures`, `date_methods`, `encoding_timers`, `error_extensions`, `fetch_response`, `generators`, `json_advanced`, `node_crypto_buffer`, `node_fs`, `node_path`, `node_process`, `number_math`, `object_methods`, `proxy_reflect`, `regexp_advanced`, `symbols`, `typeof_instanceof`, `weakref_finalization` | 0 |
| 🟡 close | `async_advanced` (2), `global_apis` (4), `map_set_extended` (4), `string_methods` (4) | 2–4 |
| 🟡 mid | `typed_arrays` (6), `console_methods` (9) | 6–9 |

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

- **v0.5.156** — Fix the `await-tests` gate added in v0.5.155. First release under the new gate failed publish: Tests (run 24762486365) succeeded at 06:24:35Z and Simulator Tests (run 24762486389) succeeded at 06:05:15Z on the exact tag SHA, but the poller logged `no run found yet for "Tests" on <sha> — waiting` for the entire 45-minute deadline and timed out, skipping every build/publish leg (Homebrew, apt, npm, GH release assets). Root cause: the original script used `gh run list --workflow "Tests" --commit "$SHA" ... 2>/dev/null || echo '[]'`, which does a name→workflow-id resolution that silently returns `[]` in the `release`-event context on CI runners, AND the `2>/dev/null || echo '[]'` fallback made a real `gh` error indistinguishable from a genuinely-empty result. Rewrote to query by workflow *filename* directly: `gh api /repos/$REPO/actions/workflows/{test.yml,simctl-tests.yml}/runs?head_sha=$SHA&per_page=1` — no name resolution, no swallowed errors. A `gh api` non-zero exit now logs stderr, retries once after 10s, then fails the gate loudly instead of burning the 45-min budget. Verified locally that `gh api /repos/PerryTS/perry/actions/workflows/test.yml/runs?head_sha=ed1bde0...&per_page=1` returns the Tests run's `status=completed conclusion=success` — the exact payload the old query swallowed. `workflow_dispatch` bypass preserved for emergency republish. Tag `v0.5.155` stays as cosmetic noise (public tag, GH release body, no binaries); npm/brew/apt catch up on v0.5.156.
- **v0.5.155** — Gate `release-packages.yml` on green `Tests` + `Simulator Tests (iOS)` for the exact tagged commit. New `await-tests` job polls `gh run list --commit <sha> --workflow <name>` every 30s (45-min total deadline) for each gated workflow; fails the release if either workflow failed, succeeds only when both reach `completed`+`success`. `build` now `needs: await-tests` so every downstream publishing leg (Homebrew bottle, apt `.deb`, npm tarballs) transitively waits. Also added `tags: ['v*']` to `test.yml`'s push trigger so the Tests workflow actually runs on the tag that the /release skill pushes — it was previously only on `branches: [main]`, which meant release-packages had no Tests run to gate against on the exact tag-commit SHA. `workflow_dispatch` bypass still works (`if EVENT=workflow_dispatch → exit 0`) for the "first-run / emergency republish" lever that's been in the workflow since v0.5.107. Net effect: `/release` on the dev side is unchanged (still interactive, still drives the tag push), but a bad commit now has to survive all PR-blocking CI *plus* simctl-tests before npm/brew see anything.
- **v0.5.154** — Drop `run_with_timeout` wrapper from the simctl launch path. v0.5.153's trace output localized the `exit 143` to the launch phase itself — 2s after `[+] launch start`, no `[+] launch done` line, clean SIGTERM. Root cause: without `--console-pty` (dropped in v0.5.152), `xcrun simctl launch` returns in <1s, but `run_with_timeout`'s bash-watchdog fallback still starts a `( sleep 30 && kill -TERM <pid> ) &` watcher subshell. On the fast path the main `wait` returns, the function then tries `kill -TERM <watcher>` — the watcher's `sleep 30` is still running, kill succeeds, but the subsequent `wait <watcher>` reaps a 143-exiting process, and with `set -e` in effect at the caller, the whole script aborts with 143. (The `2>/dev/null` suppresses kill's stderr but not its exit code, and bash's `set -e` is unforgiving here.) Wrap the `xcrun simctl launch` call in `set +e … rc=$? … set -e` and skip the timeout wrapper entirely — the whole point of the 30s timeout was to kill hung `--console-pty` sessions, which no longer exist. If launch itself ever hangs we'll catch it at the `timeout-minutes: 45` step level instead.
- **v0.5.153** — Instrument `scripts/run_simctl_tests.sh` with per-phase timestamped trace lines (`[+] compile start/done`, `[+] install start/done`, `[+] launch start/done rc=$rc`, `[+] uninstall start/done`) after run #24759274456 showed the post-v0.5.152 `--console-pty`-drop attempt STILL hung for ~2:04 after `=== counter.ts ===` with no progress output and got SIGTERMed. `counter.run.log` contained `com.perry.doctests.counter: 43320` so simctl launch definitely succeeded; something after launch is silently stalling. Also set explicit `timeout-minutes: 60` job-level and `45` step-level in `.github/workflows/simctl-tests.yml` to rule out any implicit 2-min GitHub runner cutoff (default is 360 min but the consistency of the ~2-min kill across four runs is suspicious enough to verify). Next run will tell us exactly which sub-step hangs — the trace lines force a log flush on every transition so we can narrow to compile vs install vs launch vs uninstall.
- **v0.5.152** — Drop `--console-pty` from `scripts/run_simctl_tests.sh` — the last reason tier-2 iOS runs were being SIGTERMed after the first example. Run #24758790770 with the v0.5.147-150 fix chain in place finally got `counter.run.log` to contain `com.perry.doctests.counter: 30902` (simctl successfully launched the app with test-mode env, PID in stdout), but the step still died at ~1:57 without printing `PASS` or moving to `gallery.ts`. Root cause: `--console-pty` blocks simctl until the app's stdio closes, but when simctl's own stdout is redirected to a file (as the script does), the PTY master never gets EOF even after the app calls `process::exit(0)` — simctl hangs past `LAUNCH_TIMEOUT`, GitHub kills the runner, no next iteration. Dropped the flag; simctl now returns immediately with pid, the test-mode exit timer still self-cleans the app on the device, and the launch exit code still catches bundle/arch/signing errors (which is how we surfaced v0.5.149's Info.plist mismatch in the first place). Trade-off: we verify "bundle launches" not "app cleanly exits" — still strong tier-2 signal for a doc-example smoke gate.
- **v0.5.151** — Closed the last four perry/ui gaps from `GAPS.md` end-to-end. (1) **Alert rich form** — the `PERRY_UI_TABLE` entry for `alert` pointed at the 4-arg `perry_ui_alert(title, message, buttons_ptr, cb)` but was declared 2-arg; `alert("a","b")` was reading buttons/callback from uninitialized registers, which on arm64 happened to work only because `js_nanbox_get_pointer` on the garbage f64 usually yielded a zero-ish pointer that `js_array_get_length` tolerated. Split into `perry_ui_alert_simple(title, message)` (2-arg, NSAlert with OK) and kept the 4-arg `perry_ui_alert` for the new `alertWithButtons(title, message, string[], (i)=>void)` surface. Changed the 4-arg signature's `buttons_ptr: i64` → `buttons: f64` to match the closure convention (codegen passes NaN-boxed JSValue, runtime extracts pointer via `js_nanbox_get_pointer`) — this aligned the macOS/gtk4/Windows implementations which had previously disagreed on calling convention. iOS/tvOS/watchOS stubs updated to match. Added `alertWithButtons` → `UiArgKind::[Str, Str, F64, Closure]` in `PERRY_UI_TABLE` so TS callers get proper dispatch. (2) **String preferences** — `preferencesSet("apiUrl", "https://...")` worked at runtime on all desktop backends (each branches on NaN-box tag `0x7FFF`) but `types/perry/system/index.d.ts` declared `value: number` only; callers had to `as any`-cast. Widened to `string | number` for `preferencesSet`, return type `string | number | undefined` for `preferencesGet` — matches the existing runtime behavior, no codegen or runtime change needed. (3) **Lifecycle hooks** — `perry_ui_app_on_terminate` / `perry_ui_app_on_activate` FFI existed on macOS but the AppDelegate never invoked them: `PerryAppDelegate` at `crates/perry-ui-macos/src/app.rs:1148` had no `applicationWillTerminate:` or `applicationDidBecomeActive:` method overrides. Added both, plus top-level `invoke_terminate_callback` / `invoke_activate_callback` helpers so test-mode exit (which uses `std::process::exit(0)` and bypasses Cocoa's `applicationWillTerminate:`) now also fires the terminate hook for CI coverage. Added `onTerminate` / `onActivate` to `types/perry/ui/index.d.ts` and matching `PERRY_UI_TABLE` entries mapping to the existing FFI. GTK4 already wires `connect_shutdown` → on_terminate and `connect_activate` → on_activate (the original audit was wrong about gtk4); Windows already dispatches via `WM_DESTROY` / `WM_ACTIVATEAPP`. (4) **LazyVStack virtualization** — rewrote `crates/perry-ui-macos/src/widgets/lazyvstack.rs` to back `LazyVStack(count, render)` with `NSTableView` + a `PerryLazyVStackDelegate` (NSTableViewDataSource + NSTableViewDelegate) instead of eager-rendering every row into an NSStackView. Single-column, no header, no grid lines, no selection highlighting — visually matches a plain vertical list. `tableView:viewForTableColumn:row:` invokes the user's render closure lazily, so for a 1000-row list only ~15 rows are realized (just the visible rect). Added `lazyvstackSetRowHeight(handle, height)` since NSTableView virtualization requires uniform row heights (variable-height rows defeat virtualization anyway); default is 44pt. `lazyvstackUpdate(handle, newCount)` triggers `reloadData` which re-fetches only currently-visible rows. The `perry-hir/src/lower.rs:2683` entry registered `LazyVStack` as a native instance type but there was no corresponding `PERRY_UI_TABLE` entry, so `LazyVStack(100, fn)` would have hit the "not a known function" bail path — added that too, plus `lazyvstackUpdate` / `lazyvstackSetRowHeight` dispatch. GTK4/Windows kept their eager-render implementations (their `set_row_height` FFI is advisory) — true virtualization there is a follow-up. Added `LazyVStack` / `lazyvstackUpdate` / `lazyvstackSetRowHeight` to `types/perry/ui/index.d.ts` so the full API is now typed. End-to-end smoke test at `/tmp/test_gaps.ts` exercises all four: prints `stored url: https://api.example.com`, `stored count: 42`, `activate hook fired`, `terminate hook fired` and compiles a 1000-row LazyVStack + simple alert + rich alert in one program.
- **v0.5.150** — `--app-bundle-id` CLI flag is now honored on `--target ios-simulator` / `--target ios` (full-app, not just widget). The iOS branch at `crates/perry/src/commands/compile.rs:6442` resolved `bundle_id` via perry.toml → package.json → `com.perry.{exe_stem}` fallback and ignored `args.app_bundle_id` entirely. v0.5.149 CI run #24758231777 reproduced the real v0.5.148 failure cleanly: perry wrote `CFBundleIdentifier=com.perry.counter` into Info.plist, simctl installed the app under that ID, then our harness ran `simctl launch <UDID> com.perry.doctests.counter` → "app not found" → `FBSOpenApplicationServiceErrorDomain code=4`. Threaded `args.app_bundle_id.clone().or_else(|| <existing-lookup>).unwrap_or_else(|| "com.perry.{stem}")` so CLI wins. End-to-end verified locally: `perry compile --target ios-simulator --app-bundle-id com.perry.doctests.counter docs/examples/ui/counter.ts -o /tmp/counter-test` → `simctl install` → `simctl launch` returns `com.perry.doctests.counter: <pid>` and exits cleanly via the `PERRY_UI_TEST_MODE=1`/`PERRY_UI_TEST_EXIT_AFTER_MS=500` timer from v0.5.135. Combined with v0.5.147 (`--setenv KEY=VAL` space-token) + v0.5.148 (actually `SIMCTL_CHILD_` prefix instead — simctl has no --setenv flag at all) + v0.5.149 (`iPhoneSimulator` plist keys), this is the tier-2 iOS simulator workflow finally running green end-to-end.
- **v0.5.149** — iOS-simulator Info.plist had `CFBundleSupportedPlatforms=[iPhoneOS]` / `DTPlatformName=iphoneos` / `DTSDKName=iphoneos26.4` hardcoded regardless of target, so when we built for `--target ios-simulator` (which produces a Mach-O with `LC_BUILD_VERSION platform=iphonesimulator`) the bundle metadata disagreed with the binary. `simctl launch` caught the mismatch and refused with `FBSOpenApplicationServiceErrorDomain code=4` ("The request to open ... failed") — which is exactly what v0.5.148's simctl workflow run surfaced once the `--setenv` parser errors were out of the way. Branch on `target.as_deref() == Some("ios-simulator")` just before the iOS `info_plist = format!(...)` site in `crates/perry/src/commands/compile.rs:6555` and swap the three strings to `iPhoneSimulator` / `iphonesimulator` / `iphonesimulator26.4`. The tvOS plist (line 7130) does NOT set these three keys at all so it's not blocked on the same bug, but a similar fix will be needed when we wire up tvOS-sim smoke tests.
- **v0.5.148** — `scripts/run_simctl_tests.sh` round three, actually root-caused now. The v0.5.147 `--setenv KEY=VALUE` split didn't fix run #24757255292 — it changed the error from `Invalid device: --setenv=PERRY_UI_TEST_MODE=1` to `Invalid device: --setenv` because `xcrun simctl launch` has **no `--setenv` flag at all** (verified against `xcrun simctl help launch` on macOS 15 + Xcode 16). simctl was parsing `--setenv` as the positional device argument in both prior forms. The documented way to pass env vars into the spawned app is to prefix them with `SIMCTL_CHILD_` in the *calling* shell's env — simctl strips the prefix and forwards the rest to the child. Rewrote the launch call to `SIMCTL_CHILD_PERRY_UI_TEST_MODE=1 SIMCTL_CHILD_PERRY_UI_TEST_EXIT_AFTER_MS=500 run_with_timeout … xcrun simctl launch … "$UDID" "$bundle_id"` — bash propagates those assignments through the function call into xcrun's environment. Comment now explicitly warns future readers that `--setenv` is not a thing, so the fix doesn't get un-fixed a third time.
- **v0.5.147** — `scripts/run_simctl_tests.sh` round two. v0.5.145's GNU-timeout fallback fixed the `command not found` failures but run #24756720691 revealed the next blocker — every example now failed with `Invalid device: --setenv=PERRY_UI_TEST_MODE=1`. Root cause: `xcrun simctl launch` requires `--setenv KEY=VALUE` as two separate argv tokens; `--setenv=KEY=VALUE` (which is how every other POSIX long-option flag works) makes simctl's CLI parser treat the entire `--setenv=...` blob as the device argument. Split both `--setenv` calls into `--setenv PERRY_UI_TEST_MODE=1` + `--setenv PERRY_UI_TEST_EXIT_AFTER_MS=500` and documented the quirk inline so the next reader doesn't un-fix it.
- **v0.5.146** — Add `perry.nativeLibrary.targets.<plat>.metal_sources` sibling to `swift_sources` (closes #124). New `TargetNativeConfig.metal_sources: Vec<PathBuf>` parsed from package.json; new helper `compile_metallib_for_bundle` dedups sources across shared manifests (canonical-path set like `swift_sources` does), shells out to `xcrun -sdk <sdk> metal -c <file> -o <stem>.air` per shader into a pid-scoped tmp dir, then `xcrun -sdk <sdk> metallib -o default.metallib <all .air>` packs them into `<app>.app/default.metallib` — the path SwiftUI's `ShaderLibrary.default` / `MTLDevice.makeDefaultLibrary()` loads at runtime. Wired into all three Apple `.app` branches (iOS/tvOS/watchOS) immediately after `Info.plist` is written. SDK map: watchos→`watchos`, watchos-simulator→`watchsimulator`, ios→`iphoneos`, ios-simulator→`iphonesimulator`, tvos→`appletvos`, tvos-simulator→`appletvsimulator`. Deliberately omits `-target <triple>` — `-sdk` is sufficient for platform selection and avoids hard-coding a Metal min-OS we'd have to keep in sync with Swift's. Early-errors in the native-lib link loop if `metal_sources` is configured on any non-Apple-bundle target so the failure is loud (matches the `swift_sources`/watchOS validation pattern). Unblocks `Bloom-Engine/engine#16` — Bloom's bloom crate can now ship `.metal` shaders for SwiftUI's `.colorEffect(Shader(function: .init(library: .default, name: "...")))` path on `watchos-swift-app`.

Older entries → CHANGELOG.md.
