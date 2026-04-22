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

    let clang = find_clang().context(if cfg!(windows) {
        "clang not found. Install LLVM from https://github.com/llvm/llvm-project/releases \
         or set PERRY_LLVM_CLANG to the path of clang.exe"
    } else {
        "No clang found in PATH. Install LLVM/clang or set PERRY_LLVM_CLANG"
    })?;

    let mut cmd = Command::new(&clang);
    cmd.arg("-c")
        // -O3 unlocks LLVM's auto-vectorizer, aggressive inlining, and
        // better SLP / loop unrolling. The compile-time cost vs -O2 is
        // small for typical user programs (<1s of overhead) compared
        // to the runtime perf wins on tight loops.
        .arg("-O3")
        // Include DWARF debug info so crash symbolicators can map
        // addresses back to function names. Only enabled when
        // PERRY_DEBUG_SYMBOLS=1 is set — otherwise omitted to keep
        // binaries small.
        .args(if std::env::var("PERRY_DEBUG_SYMBOLS").is_ok() { vec!["-g"] } else { vec![] })
        // We want LLVM to reassociate f64 ops (for loop unrolling)
        // but NOT to assume NaN never occurs — Perry's NaN-boxing uses
        // NaN bit patterns for ALL non-number values (strings, objects,
        // null, undefined, booleans). -ffast-math includes
        // -ffinite-math-only which tells LLVM NaN never happens,
        // causing it to replace NaN-boxed constants (TAG_NULL, etc.)
        // with 0.0. Use individual flags instead:
        // -funsafe-math-optimizations: allows reassociation + reciprocal
        // -fno-math-errno: skip errno checks on math functions
        // (Do NOT use -ffinite-math-only or -ffast-math)
        .arg("-fno-math-errno")
        // Use native CPU features for better codegen on the build machine.
        // ARM uses -mcpu=native; x86 uses -march=native.
        // Cross-compilation overrides this via -target.
        .arg(if cfg!(target_arch = "aarch64") { "-mcpu=native" } else { "-march=native" })
        .arg(&ll_path)
        .arg("-o")
        .arg(&obj_path);
    if let Some(triple) = target_triple {
        cmd.arg("-target").arg(triple);
    }

    log::debug!("perry-codegen: {:?}", cmd);
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

    // Clean up temp files on success — unless PERRY_LLVM_KEEP_IR is set, in
    // which case we leave the .ll around for debugging and print the path.
    let keep = env::var_os("PERRY_LLVM_KEEP_IR").is_some();
    if keep {
        eprintln!("[perry-codegen] kept LLVM IR: {}", ll_path.display());
        eprintln!("[perry-codegen] kept object:  {}", obj_path.display());
    } else {
        let _ = fs::remove_file(&ll_path);
        let _ = fs::remove_file(&obj_path);
    }

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
    // Check PATH (with .exe extension handling on Windows).
    if which("clang") {
        return Some(PathBuf::from("clang"));
    }
    // Check well-known install locations.
    #[cfg(windows)]
    {
        // Standalone LLVM installer (llvm.org)
        let standalone = PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe");
        if standalone.exists() {
            return Some(standalone);
        }
        // MSVC Build Tools bundled clang (via "C++ Clang Compiler" component)
        if let Some(path) = find_msvc_bundled_clang() {
            return Some(path);
        }
    }
    #[cfg(not(windows))]
    {
        for prefix in &["/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin"] {
            let candidate = PathBuf::from(prefix).join("clang");
            if candidate.exists() && is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Search for clang.exe bundled with Visual Studio Build Tools / Community.
/// The "C++ Clang Compiler for Windows" workload component installs it at:
///   <VS install>/VC/Tools/Llvm/x64/bin/clang.exe
#[cfg(windows)]
fn msvc_vswhere_installation_path_args() -> [&'static str; 8] {
    [
        "-products",
        "*",
        // Without the VC tools filter, `-latest` can select Management Studio.
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-latest",
        "-property",
        "installationPath",
        "-nologo",
    ]
}

#[cfg(windows)]
fn find_msvc_bundled_clang() -> Option<PathBuf> {
    let vswhere_paths = [
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ];
    for vswhere in &vswhere_paths {
        if !vswhere.exists() { continue; }
        let output = std::process::Command::new(vswhere)
            .args(msvc_vswhere_installation_path_args())
            .output()
            .ok()?;
        let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if install_path.is_empty() { continue; }
        // Check x64 first, then ARM64
        for arch in &["x64", "ARM64"] {
            let candidate = PathBuf::from(&install_path)
                .join("VC").join("Tools").join("Llvm").join(arch).join("bin").join("clang.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
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
        // On Windows, executables have .exe extension
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{}.exe", name));
            if with_exe.exists() && is_executable(&with_exe) {
                return true;
            }
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

// ---------------------------------------------------------------------------
// Bitcode link pipeline (Phase J)
// ---------------------------------------------------------------------------

/// Find an LLVM tool (llvm-link, opt, llc, llvm-as) on the system.
fn find_llvm_tool(tool: &str) -> Option<PathBuf> {
    let env_key = format!("PERRY_LLVM_{}", tool.to_uppercase().replace('-', "_"));
    if let Ok(p) = env::var(&env_key) {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    for prefix in &["/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin"] {
        let candidate = PathBuf::from(prefix).join(tool);
        if candidate.exists() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    if which(tool) {
        return Some(PathBuf::from(tool));
    }
    None
}

/// Whole-program bitcode link pipeline.
///
/// Converts user `.ll` files to `.bc`, merges them with the runtime/stdlib
/// bitcode via `llvm-link`, runs `opt -O3`, then `llc -filetype=obj` to
/// produce a single object file. Returns the path to that `.o`.
pub fn bitcode_link_pipeline(
    user_ll_files: &[PathBuf],
    runtime_bc: &Path,
    stdlib_bc: Option<&Path>,
    extra_bc: &[PathBuf],
    target_triple: Option<&str>,
) -> Result<PathBuf> {
    let llvm_as = find_llvm_tool("llvm-as")
        .ok_or_else(|| anyhow!("llvm-as not found (required for bitcode link)"))?;
    let llvm_link = find_llvm_tool("llvm-link")
        .ok_or_else(|| anyhow!("llvm-link not found (required for bitcode link)"))?;
    let opt_tool = find_llvm_tool("opt")
        .ok_or_else(|| anyhow!("opt not found (required for bitcode link)"))?;
    let llc = find_llvm_tool("llc")
        .ok_or_else(|| anyhow!("llc not found (required for bitcode link)"))?;

    let tmp_dir = env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let prefix = format!("perry_bc_{}_{}", pid, nonce);
    let keep = env::var_os("PERRY_LLVM_KEEP_IR").is_some();
    let mut intermediates: Vec<PathBuf> = Vec::new();

    // Step 1: llvm-as each .ll → .bc
    let mut user_bc_files: Vec<PathBuf> = Vec::new();
    for (i, ll_file) in user_ll_files.iter().enumerate() {
        let bc_path = tmp_dir.join(format!("{}_{}.bc", prefix, i));
        let output = Command::new(&llvm_as)
            .arg(ll_file)
            .arg("-o")
            .arg(&bc_path)
            .output()
            .with_context(|| format!("Failed to invoke llvm-as on {}", ll_file.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "llvm-as failed on {} (status={}):\n{}",
                ll_file.display(), output.status, stderr
            ));
        }
        intermediates.push(bc_path.clone());
        user_bc_files.push(bc_path);
    }

    // Step 2: llvm-link all bitcode into one module.
    // perry-stdlib re-exports/wraps some perry-runtime symbols, so we
    // pass the stdlib as `--override` to let its definitions win.
    let linked_bc = tmp_dir.join(format!("{}_linked.bc", prefix));
    {
        let mut cmd = Command::new(&llvm_link);
        for bc in &user_bc_files {
            cmd.arg(bc);
        }
        cmd.arg(runtime_bc);
        if let Some(stdlib) = stdlib_bc {
            cmd.arg("--override").arg(stdlib);
        }
        for bc in extra_bc {
            cmd.arg(bc);
        }
        cmd.arg("-o").arg(&linked_bc);
        log::debug!("perry-codegen bitcode-link: {:?}", cmd);
        let output = cmd.output().context("Failed to invoke llvm-link")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("llvm-link failed (status={}):\n{}", output.status, stderr));
        }
    }
    intermediates.push(linked_bc.clone());

    // Step 3: opt -O3
    let opt_bc = tmp_dir.join(format!("{}_opt.bc", prefix));
    {
        let mut cmd = Command::new(&opt_tool);
        cmd.arg("-O3").arg(&linked_bc).arg("-o").arg(&opt_bc);
        log::debug!("perry-codegen opt: {:?}", cmd);
        let output = cmd.output().context("Failed to invoke opt")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("opt -O3 failed (status={}):\n{}", output.status, stderr));
        }
    }
    intermediates.push(opt_bc.clone());

    // Step 4: llc -filetype=obj → .o
    let linked_obj = PathBuf::from(format!("{}_linked.o", prefix));
    {
        let mut cmd = Command::new(&llc);
        cmd.arg("-filetype=obj").arg("-O3").arg(&opt_bc).arg("-o").arg(&linked_obj);
        if let Some(triple) = target_triple {
            cmd.arg("-mtriple").arg(triple);
        }
        log::debug!("perry-codegen llc: {:?}", cmd);
        let output = cmd.output().context("Failed to invoke llc")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("llc failed (status={}):\n{}", output.status, stderr));
        }
    }

    if keep {
        eprintln!("[perry-codegen] bitcode-link intermediates kept:");
        for f in &intermediates {
            eprintln!("  {}", f.display());
        }
        eprintln!("  → {}", linked_obj.display());
    } else {
        for f in &intermediates {
            let _ = fs::remove_file(f);
        }
    }

    Ok(linked_obj)
}
