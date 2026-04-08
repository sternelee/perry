//! HIR → LLVM IR compilation entry point.
//!
//! This is the public API surface mirrored from `perry-codegen`. The contract:
//!
//! ```ignore
//! let opts = CompileOptions { target: None, is_entry_module: true };
//! let object_bytes: Vec<u8> = perry_codegen_llvm::compile_module(&hir, opts)?;
//! ```
//!
//! The returned bytes are a regular object file (`.o` on macOS/Linux, `.obj`
//! on Windows) produced by `clang -c`. Perry's existing linking stage in
//! `crates/perry/src/commands/compile.rs` picks them up identically to the
//! Cranelift output — no linker changes needed.
//!
//! Phase 1 scope: this returns an unimplemented error. Phase 1's Task #8
//! replaces the stub with a minimal HIR walker that can emit a `main`
//! function calling `js_console_log_dynamic(42.0)`.

use anyhow::{bail, Result};
use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::runtime_decls;

/// Options mirrored from the Cranelift backend's setter API — only the
/// handful Phase 1 needs. More fields get added as later phases require them.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Target triple override. `None` uses the host default
    /// (`arm64-apple-macosx15.0.0` on Apple Silicon macOS).
    pub target: Option<String>,
    /// Whether this module is the program entry point. When true, codegen
    /// emits a `main` function that calls `js_gc_init` and then the module's
    /// top-level statements.
    pub is_entry_module: bool,
}

/// Compile a Perry HIR module to an object file via LLVM IR.
pub fn compile_module(hir: &HirModule, opts: CompileOptions) -> Result<Vec<u8>> {
    let triple = opts
        .target
        .clone()
        .unwrap_or_else(default_target_triple);

    let mut llmod = LlModule::new(&triple);
    runtime_decls::declare_phase1(&mut llmod);

    // Phase 1 stub: the walker that lowers `hir.functions` / top-level stmts
    // doesn't exist yet. Task #8 fills this in. Bail with a clear message so
    // anyone who wires up `--backend llvm` before Phase 1 lands gets a
    // actionable error instead of a silent empty object file.
    let _ = hir; // silence unused warning until Phase 1 walker is added
    bail!(
        "perry-codegen-llvm: compile_module is a Phase 1 scaffold stub. \
         Implement Task #8 (minimal expr/stmt) before using --backend llvm."
    );

    // Unreachable until Task #8 replaces the bail above:
    // #[allow(unreachable_code)]
    // {
    //     let ll_text = llmod.to_ir();
    //     crate::linker::compile_ll_to_object(&ll_text, opts.target.as_deref())
    // }
}

/// Host default triple. Mirrors anvil's hardcoded value for macOS; later
/// phases plumb Perry's existing cross-target table here.
fn default_target_triple() -> String {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "arm64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu".to_string()
    } else {
        // Conservative fallback — clang will reject this on unsupported hosts
        // and the error message will be obvious.
        "arm64-apple-macosx15.0.0".to_string()
    }
}
