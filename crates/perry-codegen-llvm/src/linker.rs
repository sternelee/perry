//! Driver: write `.ll` text to disk, shell out to `clang -c` to produce an
//! object file, and return its bytes.
//!
//! This is the seam that lets Perry's existing linking pipeline (nm scan +
//! `cc` invocation in `crates/perry/src/commands/compile.rs`) stay unchanged.
//! Both backends produce the same artifact — an object file as `Vec<u8>` —
//! so the rest of the compile pipeline doesn't care which one ran.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

/// Compile LLVM IR text to an object file using the system `clang`, returning
/// the object file bytes.
///
/// We write the `.ll` to a temp file (LLVM text is big and clang reads it
/// more reliably from disk than from stdin), invoke `clang -c`, read the
/// resulting `.o`, and clean up both on success. On failure the temp files
/// are left behind for debugging — the caller can `grep /tmp/perry_llvm_*`.
pub fn compile_ll_to_object(ll_text: &str, target_triple: Option<&str>) -> Result<Vec<u8>> {
    let tmp_dir = env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ll_path = tmp_dir.join(format!("perry_llvm_{}_{}.ll", pid, nonce));
    let obj_path = tmp_dir.join(format!("perry_llvm_{}_{}.o", pid, nonce));

    {
        let mut f = fs::File::create(&ll_path)
            .with_context(|| format!("Failed to create temp .ll file at {}", ll_path.display()))?;
        f.write_all(ll_text.as_bytes())?;
    }

    let clang = find_clang().context("No clang found in PATH (required for --backend llvm)")?;

    let mut cmd = Command::new(&clang);
    cmd.arg("-c")
        .arg("-O2")
        .arg(&ll_path)
        .arg("-o")
        .arg(&obj_path);
    if let Some(triple) = target_triple {
        cmd.arg("-target").arg(triple);
    }

    log::debug!("perry-codegen-llvm: {:?}", cmd);
    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {}", clang.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "clang -c failed (status={}):\n{}\n(LLVM IR left at {})",
            output.status,
            stderr,
            ll_path.display()
        ));
    }

    let bytes = fs::read(&obj_path)
        .with_context(|| format!("Failed to read clang output at {}", obj_path.display()))?;

    // Clean up temp files on success.
    let _ = fs::remove_file(&ll_path);
    let _ = fs::remove_file(&obj_path);

    Ok(bytes)
}

fn find_clang() -> Option<PathBuf> {
    // Honor explicit override first — useful on systems with multiple clang
    // installs (e.g. Homebrew LLVM vs Xcode).
    if let Ok(p) = env::var("PERRY_LLVM_CLANG") {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // Otherwise trust PATH.
    if which("clang") {
        return Some(PathBuf::from("clang"));
    }
    None
}

fn which(name: &str) -> bool {
    let path_var = match env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() && is_executable(&candidate) {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.exists()
}
