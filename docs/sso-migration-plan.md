# Plan: Small String Optimization (SSO) rollout

**Status:** infrastructure landed in v0.5.213. No creation sites migrated yet —
see "Why infrastructure-only first" below.

**Goal:** strings of length 0..=5 bytes encode inline in the NaN-boxed f64
payload (48 bits: 8-bit length + 5 bytes of data + the SHORT_STRING_TAG
band). Zero heap allocation for short strings. Tier 1 #2 per
`docs/memory-perf-roadmap.md`.

## Current state (v0.5.213)

Landed:

- `SHORT_STRING_TAG = 0x7FF9_0000_0000_0000`, constants at
  `crates/perry-runtime/src/value.rs:44-56`.
- `JSValue::try_short_string(&[u8])` — returns `Some(Self)` for
  `bytes.len() <= 5`, `None` otherwise. `value.rs`.
- `JSValue::short_string_to_buf(&mut [u8; 5]) -> usize` — decoder.
- `JSValue::is_short_string()`, `JSValue::is_any_string()` (matches
  both heap STRING_TAG and inline SSO), `JSValue::is_string()`
  (legacy strict heap predicate — unchanged).
- `js_string_new_sso(ptr, len) -> f64` in `string.rs` — SSO-aware
  construction that returns an SSO value for short inputs and falls
  through to `js_string_from_bytes` + `JSValue::string_ptr` for
  long inputs.
- `str_bytes_from_jsvalue(value, &mut scratch) -> Option<(*const u8, u32)>`
  in `string.rs` — central decoder that produces a `(ptr, len)` view
  for both heap and SSO forms (SSO decodes into caller-owned
  scratch).
- `js_string_materialize_to_heap(value) -> *mut StringHeader` in
  `string.rs` — compatibility shim that allocates a heap
  StringHeader from an SSO value. For call sites that can't easily
  be migrated.
- `typeof` (builtins.rs) accepts both tags — returns `"string"` for
  SSO values.
- `js_jsvalue_equals` + `js_jsvalue_compare` (value.rs) handle SSO
  on both sides, with a bitwise fast path when both operands are
  SSO (canonical encoding: same bytes ⇒ same bits).
- `js_value_length_f64` (value.rs) returns length directly from the
  SSO tag byte for SHORT_STRING_TAG values, no heap access.
- Three stringify arms in `json.rs` (top-level value, object field
  inline dispatch, array element inline dispatch) decode SSO into
  escaped output.
- Six unit tests in `value::tests` cover roundtrip / rejection /
  embedded-NUL / tag-distinctness / empty / byte-order.

## Why infrastructure-only first

Attempting to emit SSO from `DirectParser::parse_string_value` in
the same commit produced immediate regressions: three
`test_json_lazy_*.ts` tests diffed from Node because stringify
walkers for objects + arrays in json.rs have **many** inline
dispatch sites (verified: `grep "== STRING_TAG" crates/perry-runtime/src/json.rs`
returns 20+ hits) and each needs a parallel SSO arm before the
creation site can ship safely. Beyond json.rs, consumer sites include:

- `crates/perry-runtime/src/object.rs` — property-get helpers,
  field-key lookups, `Object.keys/values/entries`, proxy handlers.
- `crates/perry-runtime/src/string.rs` — every string method
  (`split`, `replace`, `slice`, `indexOf`, `includes`, `startsWith`,
  etc.).
- `crates/perry-runtime/src/regex.rs` — match result string
  extraction, `replace` substitution.
- `crates/perry-runtime/src/set.rs` + `map.rs` — key comparison
  when keys are strings.
- `crates/perry-runtime/src/builtins.rs` — `console.log` argument
  stringification, `String(x)` coercion, the handful of `is_string()`
  type guards.
- `crates/perry-stdlib/src/fastify/`, `mysql2/`, `pg/`, `redis/` —
  request/response body paths use `js_string_from_bytes` + assume
  heap-pointer semantics.
- `crates/perry-codegen/src/expr.rs` — string-literal emission,
  template-literal concat, tag function calls.

Flipping `DirectParser::parse_string_value` to SSO immediately
without the consumer audit breaks each of those paths in a
different way. Landing the infrastructure without producers is
safe (the new tag value is allocated but unused) and unblocks
incremental per-site migration without coordinating a single giant
commit.

## Migration roll-out

Each step is self-contained: it picks one consumer cluster, adds
SSO-aware dispatch to every site in that cluster, tests against the
full regression suite + targeted new tests, then ships.

### Step 1 — stringify consumers (json.rs)

Add SSO arm parallel to every `== STRING_TAG` dispatch in:

- `stringify_value` (done in v0.5.213)
- `stringify_value_depth`
- `stringify_object_inner` — both the "inline value dispatch" block
  at ~line 1866 and the replacer path at ~line 2060.
- `stringify_array_depth` — inline value dispatch at ~line 2180.
- `extract_string_array` at ~line 3320.
- Spacer / replacer / toJSON paths: lines 2660, 2752, 2801, 2878,
  2972, 3266.
- Top-level `js_json_stringify_full` inline string-handle paths at
  ~line 3425.

Tests: rerun `test_json_*` suite under forced-on SSO (to be added as
`PERRY_SSO_FORCE=1` env var) — all must match Node byte-for-byte.

### Step 2 — DirectParser emits SSO

Flip `parse_string_value` to call `JSValue::try_short_string(b)`
first, fall back to heap on `None`. Verify test_json_* regression
suite still passes after Step 1's stringify arms.

Expected win: for the bench_json_roundtrip shape, many keys (`id`,
`name` ≤ 5 bytes fits: "alpha"=5, "beta"=4, "gamma"=5 fit) but most
values in the bench (`"item_" + i` ≥ 6 bytes) don't. Measured
improvement will be small on that specific bench; larger on
string-heavy synthetic workloads.

### Step 3 — object key storage (object.rs)

Field keys are currently stored as a JSValue array. If keys are
often short (typically 2-6 chars) and typing SSO-aware key lookup
to accept both forms, we can skip key-header allocation entirely.
Requires audit of:

- `js_object_set_field_by_name` + `js_object_get_field_by_name` —
  convert key pointer to normalized form.
- Shape cache + transition cache — keys are currently interned via
  PARSE_KEY_CACHE into `*const StringHeader`. If we switch to SSO
  for short keys, the cache becomes `Vec<u8> -> JSValue`.
- FNV-1a hashing — must hash equivalent bytes regardless of
  representation.

Biggest win target: the `"id"`, `"v"`, `"k"` keys in the JSON
benches would all become zero-allocation. Reduces PARSE_KEY_CACHE
hot-path cost further.

### Step 4 — string methods (string.rs)

For each of `js_string_length`, `js_string_concat`,
`js_string_equals`, `js_string_substring`, `js_string_indexof`,
`js_string_split`, etc., accept JSValue in place of
`*const StringHeader` and dispatch on tag. Alternative: keep
signatures, add `js_string_*_f64` variants that take a NaN-boxed
value, and migrate callers incrementally.

### Step 5 — codegen string literals

TypeScript source like `const s = "ok";` currently lowers to an
`Expr::String("ok")` which codegen emits as a runtime
`js_string_from_bytes` call at every evaluation. Short string
literals can instead compile to a constant `f64` (the NaN-boxed
SSO encoding), loaded via `double_literal` — zero runtime cost.

### Step 6 — stdlib HTTP / DB paths

Audit perry-stdlib crates for `js_string_from_bytes` call sites
that construct short strings (status codes, short header values,
short DB cell values). Migrate to `js_string_new_sso`.

## Measurement gates

After each step lands, rerun:

- `cargo test --release -p perry-runtime --lib` — must stay green.
- `run_gap_tests.sh` + `run_parity_tests.sh` — no regressions.
- `bench_json_{roundtrip,readonly,readonly_indexed}` — should improve
  or hold steady. Any regression blocks the step.
- A new `bench_string_heavy.ts` that allocates many short strings
  — should show the targeted RSS + time win.

## Expected aggregate win (all steps)

Projected on `bench_json_roundtrip`: ~5-10 MB RSS reduction + small
time improvement from skipping StringHeader alloc overhead on short
strings.

Projected on string-heavy synthetics (many short-string allocations):
~20-40 MB RSS reduction + 10-20% time improvement.

Not as large as tier 2/3 wins, but fully additive with them and a
much smaller scope per step.

## Decision gate

After Step 2 ships, re-evaluate whether Steps 3-6 are worth the
effort. If the measured win on the JSON benches is <3 MB RSS and
<5% time, the remaining steps should be deferred and effort shifted
to tier 2/3 (escape analysis + precise root tracking +
generational GC) which give 10-50× larger wins.

If Step 2 gives the projected win, Steps 3-6 proceed in priority
order — codegen string literals (Step 5) is the cheapest next and
compounds with Step 2 without additional migration risk.

## Reference

- Tag encoding: `docs/audit-lazy-json.md` §4 has a parallel
  discussion of NaN-boxing tag layout that applies here.
- Tier classification: `docs/memory-perf-roadmap.md` §#2.
