// Issue #257 — when perry-jsruntime is NOT linked, the closure.rs V8 stubs run.
// Pre-fix they returned f64 0.0 with a wrong return type (e.g. js_load_module declared
// I64 in codegen but f64 in stub → caller read garbage from rax/x0). Post-fix the stubs
// return NaN-boxed TAG_UNDEFINED with codegen-matching signatures, so `typeof` on
// stub-returned values is `"undefined"` rather than `"number"`. This test runs WITHOUT
// V8 (no JS imports), so it just exercises plain perry-runtime code paths and pins the
// existing pre-#257 behavior down — it doesn't itself trigger the V8 stub path.
//
// The actual stub-path correctness is verified at link time via the new weak-symbol
// emission (visible via `nm`-style introspection) — see CLAUDE.md note for v0.5.363.

const x = 42;
console.log("typeof x:", typeof x);
const s = "hello";
console.log("typeof s:", typeof s);
const o = { a: 1 };
console.log("typeof o:", typeof o);
const f = () => 1;
console.log("typeof f:", typeof f);
