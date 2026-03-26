//! Compile command - compiles TypeScript to native executable

use anyhow::{anyhow, Result};
use clap::Args;
use perry_hir::{Module as HirModule, ModuleKind};
use perry_transform::{inline_functions, transform_generators};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

/// Result of a successful compilation
pub struct CompileResult {
    pub output_path: PathBuf,
    pub target: String,
    pub bundle_id: Option<String>,
    pub is_dylib: bool,
}

#[derive(Args, Debug)]
pub struct CompileArgs {
    /// Input TypeScript file
    pub input: PathBuf,

    /// Output executable name
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Keep intermediate files (for debugging)
    #[arg(long)]
    pub keep_intermediates: bool,

    /// Print the HIR (for debugging)
    #[arg(long)]
    pub print_hir: bool,

    /// Don't link, just produce object file
    #[arg(long)]
    pub no_link: bool,

    /// Enable V8 JavaScript runtime for importing pure JS modules from node_modules.
    /// This is a fallback option when native compilation is not possible.
    /// WARNING: This significantly increases binary size (~10-15MB).
    #[arg(long)]
    pub enable_js_runtime: bool,

    /// Target platform: ios-simulator, ios, android, ios-widget, ios-widget-simulator (default: native host)
    #[arg(long)]
    pub target: Option<String>,

    /// App bundle identifier (required for widget targets)
    #[arg(long)]
    pub app_bundle_id: Option<String>,

    /// Output type: executable (default) or dylib (shared library plugin)
    #[arg(long, default_value = "executable")]
    pub output_type: String,

    /// Bundle TypeScript extensions from directory.
    /// Scans subdirectories for package.json with openclaw.extensions entries
    /// and compiles them into the binary as static plugins.
    #[arg(long)]
    pub bundle_extensions: Option<PathBuf>,

    /// Enable type checking via tsgo (Microsoft's native TypeScript checker).
    /// Resolves cross-file types, interfaces, and generics for better optimization.
    /// Requires: npm install -g @typescript/native-preview
    #[arg(long)]
    pub type_check: bool,

    /// Minify and obfuscate JavaScript output (name mangling + whitespace removal).
    /// Automatically enabled for --target web.
    #[arg(long)]
    pub minify: bool,

    /// Enable compile-time feature flags (comma-separated).
    /// Each feature becomes a `__feature_NAME__` constant (0 or 1) for dead-code elimination.
    /// Example: --features plugins,experimental
    #[arg(long)]
    pub features: Option<String>,

    /// Enable geisterhand in-process input fuzzer (debug/testing).
    /// Starts an HTTP server for programmatic UI interaction.
    #[arg(long)]
    pub enable_geisterhand: bool,

    /// Port for the geisterhand HTTP server (default: 7676).
    /// Implies --enable-geisterhand.
    #[arg(long)]
    pub geisterhand_port: Option<u16>,
}

/// Information about a JavaScript module that will be interpreted at runtime
#[derive(Debug, Clone)]
pub struct JsModule {
    /// Absolute path to the JS file
    pub path: PathBuf,
    /// Source code of the JS module
    pub source: String,
    /// Module specifier used in imports (e.g., "lodash", "./utils.js")
    pub specifier: String,
}

/// Compilation context tracking all modules
pub struct CompilationContext {
    /// Native TypeScript modules to compile
    pub native_modules: BTreeMap<PathBuf, HirModule>,
    /// JavaScript modules to interpret via V8
    pub js_modules: BTreeMap<String, JsModule>,
    /// Mapping from import specifiers to resolved paths
    pub import_map: BTreeMap<String, PathBuf>,
    /// Whether JS runtime is needed
    pub needs_js_runtime: bool,
    /// Whether perry/ui module is imported (needs UI library linking)
    pub needs_ui: bool,
    /// Whether perry/plugin module is imported (needs -rdynamic for symbol export)
    pub needs_plugins: bool,
    /// Whether perry-stdlib is needed (heavy native modules like fastify, mysql2, etc.)
    pub needs_stdlib: bool,
    /// Project root (where we start looking for node_modules)
    pub project_root: PathBuf,
    /// External native libraries discovered from package dependencies
    pub native_libraries: Vec<NativeLibraryManifest>,
    /// Package aliases: maps npm package name → replacement package name (from perry.packageAliases)
    pub package_aliases: HashMap<String, String>,
    /// Packages to compile natively instead of routing to V8 (from perry.compilePackages)
    pub compile_packages: HashSet<String>,
    /// First-resolved directory for each compile package (deduplication across nested node_modules)
    pub compile_package_dirs: HashMap<String, PathBuf>,
    /// Optional tsgo type checker client (when --type-check is enabled)
    pub type_checker: Option<super::typecheck::TsGoClient>,
    /// Cache for resolve_import results: (import_source, importer_dir) -> Option<(resolved_path, kind)>
    pub resolve_cache: HashMap<(String, PathBuf), Option<(PathBuf, ModuleKind)>>,
    /// Cache for find_node_modules results: start_dir -> Option<node_modules_dir>
    pub node_modules_cache: HashMap<PathBuf, Option<PathBuf>>,
    /// Whether geisterhand (in-process input fuzzer) is enabled
    pub needs_geisterhand: bool,
    /// Port for geisterhand HTTP server (default 7676)
    pub geisterhand_port: u16,
}

impl std::fmt::Debug for CompilationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompilationContext")
            .field("native_modules", &self.native_modules.len())
            .field("js_modules", &self.js_modules.len())
            .field("type_checker", &self.type_checker.is_some())
            .finish()
    }
}

impl CompilationContext {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            native_modules: BTreeMap::new(),
            js_modules: BTreeMap::new(),
            import_map: BTreeMap::new(),
            needs_js_runtime: false,
            needs_ui: false,
            needs_plugins: false,
            needs_stdlib: false,
            project_root,
            native_libraries: Vec::new(),
            package_aliases: HashMap::new(),
            compile_packages: HashSet::new(),
            compile_package_dirs: HashMap::new(),
            type_checker: None,
            resolve_cache: HashMap::new(),
            node_modules_cache: HashMap::new(),
            needs_geisterhand: false,
            geisterhand_port: 7676,
        }
    }
}

/// External native library manifest parsed from package.json `perry.nativeLibrary` field
#[derive(Debug, Clone)]
pub struct NativeLibraryManifest {
    /// Package module name (e.g., "@honeide/editor")
    pub module: String,
    /// Resolved package directory path
    pub package_dir: PathBuf,
    /// FFI function declarations
    pub functions: Vec<NativeFunctionDecl>,
    /// Target-specific build configuration
    pub target_config: Option<TargetNativeConfig>,
}

/// An FFI function declaration from a native library manifest
#[derive(Debug, Clone)]
pub struct NativeFunctionDecl {
    pub name: String,
    pub params: Vec<String>,
    pub returns: String,
}

/// Target-specific native library build configuration
#[derive(Debug, Clone)]
pub struct TargetNativeConfig {
    pub crate_path: PathBuf,
    pub lib_name: String,
    pub frameworks: Vec<String>,
    pub libs: Vec<String>,
    pub pkg_config: Vec<String>,
}

/// Get the Rust target triple for a given perry target string
fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("ios-simulator") | Some("ios-widget-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") | Some("ios-widget") => Some("aarch64-apple-ios"),
        Some("watchos-simulator") => Some("aarch64-apple-watchos-sim"),
        Some("watchos") => Some("aarch64-apple-watchos"),
        Some("tvos-simulator") => Some("aarch64-apple-tvos-sim"),
        Some("tvos") => Some("aarch64-apple-tvos"),
        Some("android") => Some("aarch64-linux-android"),
        Some("linux") => Some("x86_64-unknown-linux-gnu"),
        Some("windows") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

/// On Windows, build a trimmed UI lib using the rlib (not staticlib).
///
/// perry-ui-windows builds as both rlib and staticlib. The staticlib bundles
/// ALL transitive deps (std, alloc, core, perry-runtime -- 314 objects).
/// perry-stdlib also bundles these. Linking both causes hundreds of duplicate
/// symbols, and /FORCE:MULTIPLE produces corrupt binaries.
///
/// The rlib contains only the UI crate's own code (1 object). We extract it
/// and combine with UI-only deps (windows, serde, regex...) from the staticlib.
/// All shared deps come from perry-stdlib. No /FORCE:MULTIPLE needed.
fn strip_duplicate_objects_from_lib(lib_path: &PathBuf) -> Result<PathBuf> {
    let llvm_ar = find_llvm_tool("llvm-ar")
        .ok_or_else(|| anyhow::anyhow!("llvm-ar not found (install llvm-tools: rustup component add llvm-tools)"))?;

    // Try to find the rlib alongside the staticlib
    let rlib_name = lib_path.file_name()
        .and_then(|f| f.to_str())
        .map(|f| format!("lib{}", f.replace(".lib", ".rlib")))
        .unwrap_or_default();
    let rlib_path = lib_path.with_file_name(&rlib_name);

    if !rlib_path.exists() {
        eprintln!("Warning: rlib not found at {:?}, using staticlib as-is", rlib_path);
        return Ok(lib_path.clone());
    }

    // Canonicalize paths so they work from any working directory
    let abs_rlib = std::fs::canonicalize(&rlib_path)?;
    let abs_staticlib = std::fs::canonicalize(lib_path)?;

    // List rlib members (skip .rmeta metadata)
    let rlib_out = Command::new(&llvm_ar).arg("t").arg(&abs_rlib).output()?;
    let rlib_objects: Vec<String> = String::from_utf8_lossy(&rlib_out.stdout)
        .lines()
        .filter(|l| l.ends_with(".o"))
        .map(|l| l.to_string())
        .collect();

    // List staticlib members to find UI-only deps
    let staticlib_out = Command::new(&llvm_ar).arg("t").arg(&abs_staticlib).output()?;
    let staticlib_members: Vec<String> = String::from_utf8_lossy(&staticlib_out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();

    // Find perry-stdlib members so we can compute the set difference.
    // Search multiple locations: next to the lib, in target/release/, and via
    // find_stdlib_library() which checks standard Perry search paths.
    let stdlib_path = lib_path.parent()
        .map(|p| p.join("perry_stdlib.lib"))
        .filter(|p| p.exists())
        .or_else(|| find_stdlib_library(Some("windows")));

    let stdlib_members: std::collections::HashSet<String> = if let Some(ref sp) = stdlib_path {
        let abs_sp = std::fs::canonicalize(sp).unwrap_or(sp.clone());
        let out = Command::new(&llvm_ar).arg("t").arg(&abs_sp).output()
            .unwrap_or_else(|_| std::process::Command::new("true").output().unwrap());
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    // Determine the main crate prefix from rlib objects (e.g. "hone_editor_windows-")
    // so we can skip alloc shim objects from the same crate in the staticlib.
    let crate_prefix: Option<String> = rlib_objects.first()
        .and_then(|o| o.split('.').next())
        .and_then(|s| s.split('-').next())
        .map(|s| format!("{}-", s));

    // Keep only staticlib members that are NOT in perry-stdlib, NOT the main
    // crate (already extracted from rlib), and not compiler_builtins.
    let ui_only_deps: Vec<&String> = staticlib_members.iter().filter(|m| {
        if m.ends_with(".dll") { return false; }
        if m.contains("compiler_builtins") { return false; }
        if stdlib_members.contains(m.as_str()) { return false; }
        // Skip objects from the main crate (rlib provides these)
        if let Some(ref prefix) = crate_prefix {
            if m.starts_with(prefix.as_str()) { return false; }
        }
        true
    }).collect();

    let trimmed_lib = abs_staticlib.with_file_name("_perry_ui_trimmed.lib");
    let extract_dir = abs_staticlib.with_file_name("_perry_ui_extract");
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;

    let mut all_objects: Vec<std::path::PathBuf> = Vec::new();

    // Extract UI crate objects from rlib (use absolute paths).
    // Skip allocator shim CGUs — these contain __rust_alloc etc. that are
    // already provided by perry-stdlib. Shim CGUs have random hash names
    // (not the standard "cgu.NN" pattern).
    for member in &rlib_objects {
        // Standard CGUs: "crate-hash.crate.hash-cgu.NN.rcgu.o"
        // Alloc shims:   "crate-hash.RANDOMHASH.rcgu.o" (no "cgu." in name)
        let is_alloc_shim = !member.contains(".cgu.") && !member.contains("-cgu.");
        if is_alloc_shim {
            continue;
        }
        let out = Command::new(&llvm_ar)
            .arg("x").arg(&abs_rlib).arg(member)
            .current_dir(&extract_dir)
            .output()?;
        if out.status.success() {
            let p = extract_dir.join(member);
            if p.exists() { all_objects.push(p); }
        }
    }

    // Extract UI-only deps from staticlib (use absolute paths)
    for member in &ui_only_deps {
        let out = Command::new(&llvm_ar)
            .arg("x").arg(&abs_staticlib).arg(member.as_str())
            .current_dir(&extract_dir)
            .output()?;
        if out.status.success() {
            let p = extract_dir.join(member.as_str());
            if p.exists() { all_objects.push(p); }
        }
    }

    eprintln!("Building trimmed UI lib: {} rlib + {} deps = {} objects (was {})",
        rlib_objects.len(), ui_only_deps.len(), all_objects.len(), staticlib_members.len());

    // Create new archive from just the UI-specific objects
    let mut ar_cmd = Command::new(&llvm_ar);
    ar_cmd.arg("crs").arg(&trimmed_lib);
    for p in &all_objects {
        ar_cmd.arg(p);
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        eprintln!("Warning: archive creation failed: {}", stderr);
        let _ = std::fs::remove_dir_all(&extract_dir);
        return Ok(lib_path.clone());
    }

    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_dir_all("_perry_ui_objects");
    Ok(trimmed_lib)
}


/// Locate an LLVM tool (lld-link, llvm-nm, llvm-ar) from the Rust toolchain or PATH.
/// Search order: env var override (e.g. PERRY_LLD_LINK) → Rust sysroot → PATH.
fn find_llvm_tool(tool_name: &str) -> Option<PathBuf> {
    // 1. Env var override (e.g. PERRY_LLD_LINK for "lld-link")
    let env_key = format!("PERRY_{}", tool_name.to_uppercase().replace('-', "_"));
    if let Ok(path) = std::env::var(&env_key) {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Rust sysroot: lib/rustlib/<host-triple>/bin/<tool>
    if let Ok(output) = Command::new("rustc").arg("--print").arg("sysroot").output() {
        let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !sysroot.is_empty() {
            if let Ok(vv) = Command::new("rustc").arg("-vV").output() {
                let vv_str = String::from_utf8_lossy(&vv.stdout);
                if let Some(host_line) = vv_str.lines().find(|l| l.starts_with("host:")) {
                    let host_triple = host_line.trim_start_matches("host:").trim();
                    let exe_suffix = if cfg!(target_os = "windows") { ".exe" } else { "" };
                    let tool_path = PathBuf::from(&sysroot)
                        .join("lib").join("rustlib").join(host_triple).join("bin")
                        .join(format!("{}{}", tool_name, exe_suffix));
                    if tool_path.exists() {
                        return Some(tool_path);
                    }
                }
            }
        }
    }

    // 3. PATH lookup
    let which_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    if let Ok(output) = Command::new(which_cmd).arg(tool_name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path.lines().next().unwrap_or(&path)));
            }
        }
    }

    None
}

/// Find MSVC link.exe by searching Visual Studio installation directories.
/// On Windows, the PATH may contain a GNU `link` utility (e.g. from Git Bash/MSYS2)
/// which is not the MSVC linker. This function searches for the real MSVC link.exe.
#[cfg(target_os = "windows")]
fn find_msvc_link_exe() -> Option<PathBuf> {
    // Try vswhere.exe first (most reliable)
    let vswhere_paths = [
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ];
    for vswhere in &vswhere_paths {
        if vswhere.exists() {
            if let Ok(output) = Command::new(vswhere)
                .args(["-products", "*", "-latest", "-property", "installationPath", "-nologo"])
                .output()
            {
                let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !install_path.is_empty() {
                    // Search for link.exe under VC/Tools/MSVC/*/bin/Hostx64/x64/
                    let msvc_dir = PathBuf::from(&install_path).join(r"VC\Tools\MSVC");
                    if let Ok(entries) = std::fs::read_dir(&msvc_dir) {
                        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                        for entry in versions {
                            let link = entry.path().join(r"bin\Hostx64\x64\link.exe");
                            if link.exists() {
                                return Some(link);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn find_msvc_link_exe() -> Option<PathBuf> {
    find_llvm_tool("lld-link")
}

/// Find MSVC library search paths (MSVC CRT, Windows SDK um, Windows SDK ucrt).
/// Returns a semicolon-separated string suitable for the LIB environment variable.
#[cfg(target_os = "windows")]
fn find_msvc_lib_paths() -> Option<String> {
    let mut paths = Vec::new();

    // Find MSVC CRT lib path via vswhere
    let vswhere_paths = [
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ];
    for vswhere in &vswhere_paths {
        if vswhere.exists() {
            if let Ok(output) = Command::new(vswhere)
                .args(["-products", "*", "-latest", "-property", "installationPath", "-nologo"])
                .output()
            {
                let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !install_path.is_empty() {
                    let msvc_dir = PathBuf::from(&install_path).join(r"VC\Tools\MSVC");
                    if let Ok(entries) = std::fs::read_dir(&msvc_dir) {
                        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                        if let Some(entry) = versions.first() {
                            let lib_path = entry.path().join(r"lib\x64");
                            if lib_path.exists() {
                                paths.push(lib_path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
            break;
        }
    }

    // Find Windows SDK lib paths
    let sdk_root = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\Lib");
    if let Ok(entries) = std::fs::read_dir(&sdk_root) {
        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        if let Some(entry) = versions.first() {
            let um_path = entry.path().join(r"um\x64");
            let ucrt_path = entry.path().join(r"ucrt\x64");
            if um_path.exists() {
                paths.push(um_path.to_string_lossy().to_string());
            }
            if ucrt_path.exists() {
                paths.push(ucrt_path.to_string_lossy().to_string());
            }
        }
    }

    if paths.is_empty() {
        None
    } else {
        Some(paths.join(";"))
    }
}

#[cfg(not(target_os = "windows"))]
fn find_msvc_lib_paths() -> Option<String> {
    let sysroot = std::env::var("PERRY_WINDOWS_SYSROOT").ok()?;
    let root = PathBuf::from(&sysroot);
    if !root.exists() {
        eprintln!("Warning: PERRY_WINDOWS_SYSROOT={} does not exist", root.display());
        return None;
    }

    let mut paths = Vec::new();

    // Search for xwin-style structured layout (crt/lib/x86_64, sdk/lib/um/x86_64, etc.)
    for (crt_sub, um_sub, ucrt_sub) in &[
        ("crt/lib/x86_64", "sdk/lib/um/x86_64", "sdk/lib/ucrt/x86_64"),
        ("crt/lib/x64", "sdk/lib/um/x64", "sdk/lib/ucrt/x64"),
    ] {
        let crt = root.join(crt_sub);
        let um = root.join(um_sub);
        let ucrt = root.join(ucrt_sub);
        if crt.exists() || um.exists() || ucrt.exists() {
            if crt.exists() { paths.push(crt.to_string_lossy().to_string()); }
            if um.exists() { paths.push(um.to_string_lossy().to_string()); }
            if ucrt.exists() { paths.push(ucrt.to_string_lossy().to_string()); }
            break;
        }
    }

    // Flat lib/ directory
    if paths.is_empty() {
        let flat_lib = root.join("lib");
        if flat_lib.exists() {
            paths.push(flat_lib.to_string_lossy().to_string());
        }
    }

    // Root itself as last resort
    if paths.is_empty() {
        paths.push(root.to_string_lossy().to_string());
    }

    Some(paths.join(";"))
}

/// Find a library by name, optionally searching cross-compilation target directories
fn find_library(name: &str, target: Option<&str>) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    // For cross-compilation targets, ONLY search target-specific directories
    // to avoid linking host-platform libraries into the wrong target
    if let Some(triple) = rust_target_triple(target) {
        candidates.push(PathBuf::from(format!("target/{}/release/{}", triple, name)));
        candidates.push(PathBuf::from(format!("target/{}/debug/{}", triple, name)));
        // When targeting the host platform (e.g. --target windows on Windows),
        // also check the default target/release/ directory since native builds
        // put libraries there without the triple subdirectory.
        #[cfg(target_os = "windows")]
        if matches!(target, Some("windows")) {
            candidates.push(PathBuf::from(format!("target/release/{}", name)));
            candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        }
        #[cfg(target_os = "linux")]
        if matches!(target, Some("linux")) {
            candidates.push(PathBuf::from(format!("target/release/{}", name)));
            candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        }
        // Also check directories relative to the perry executable.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                // For iOS targets, check the exe directory for libs with _ios naming:
                // - Libs already named with _ios (e.g. libperry_ui_ios.a) → direct lookup
                // - Libs using _ios suffix convention (e.g. libperry_runtime.a stored as
                //   libperry_runtime_ios.a next to the binary)
                if matches!(target, Some("ios") | Some("ios-simulator") | Some("ios-widget") | Some("ios-widget-simulator")) {
                    if name.contains("_ios") {
                        candidates.push(dir.join(name));
                    } else {
                        let ios_name = name.replace(".a", "_ios.a");
                        candidates.push(dir.join(&ios_name));
                    }
                }
                if matches!(target, Some("watchos") | Some("watchos-simulator")) {
                    if name.contains("_watchos") {
                        candidates.push(dir.join(name));
                    } else {
                        let watchos_name = name.replace(".a", "_watchos.a");
                        candidates.push(dir.join(&watchos_name));
                    }
                }
                if matches!(target, Some("tvos") | Some("tvos-simulator")) {
                    if name.contains("_tvos") {
                        candidates.push(dir.join(name));
                    } else {
                        let tvos_name = name.replace(".a", "_tvos.a");
                        candidates.push(dir.join(&tvos_name));
                    }
                }
                // Cross-compile targets are in ../../target/<triple>/release/ relative
                // to the perry binary (which is in target/release/)
                if let Some(target_dir) = dir.parent() {
                    candidates.push(target_dir.join(triple).join("release").join(name));
                    candidates.push(target_dir.join(triple).join("debug").join(name));
                }
            }
        }
    } else {
        // Host build: search host directories
        candidates.push(PathBuf::from(format!("target/release/{}", name)));
        candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(name));
                // Homebrew: libs installed in ../lib relative to bin
                if let Some(prefix) = dir.parent() {
                    candidates.push(prefix.join("lib").join(name));
                }
            }
        }
        candidates.push(PathBuf::from(format!("/usr/local/lib/{}", name)));
        // Debian/Ubuntu: libs installed in /usr/lib/perry
        candidates.push(PathBuf::from(format!("/usr/lib/perry/{}", name)));
    }

    for path in &candidates {
        if path.exists() {
            return Some(path.clone());
        }
    }
    None
}

/// Find the runtime library for linking
fn find_runtime_library(target: Option<&str>) -> Result<PathBuf> {
    let lib_name = match target {
        Some("windows") => "perry_runtime.lib",
        #[cfg(target_os = "windows")]
        None => "perry_runtime.lib",
        _ => "libperry_runtime.a",
    };
    find_library(lib_name, target).ok_or_else(|| {
        let extra = if target.is_some() {
            format!(" (for target {:?})", target.unwrap())
        } else {
            String::new()
        };
        anyhow!(
            "Could not find {}{}. Build it with: cargo build --release -p perry-runtime{}",
            lib_name,
            extra,
            rust_target_triple(target).map(|t| format!(" --target {}", t)).unwrap_or_default()
        )
    })
}

/// Find the stdlib library for linking (optional - only needed for native modules)
fn find_stdlib_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("windows") => "perry_stdlib.lib",
        #[cfg(target_os = "windows")]
        None => "perry_stdlib.lib",
        _ => "libperry_stdlib.a",
    };
    find_library(lib_name, target)
}

/// Find the V8 jsruntime library for linking (optional - only needed for JS module support)
fn find_jsruntime_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("windows") => "perry_jsruntime.lib",
        #[cfg(target_os = "windows")]
        None => "perry_jsruntime.lib",
        _ => "libperry_jsruntime.a",
    };
    find_library(lib_name, target)
}

/// Find the UI library for linking (optional - only needed when perry/ui is imported)
fn find_ui_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("ios-simulator") | Some("ios") => "libperry_ui_ios.a",
        Some("android") => "libperry_ui_android.a",
        Some("watchos-simulator") | Some("watchos") => "libperry_ui_watchos.a",
        Some("tvos-simulator") | Some("tvos") => "libperry_ui_tvos.a",
        Some("linux") => "libperry_ui_gtk4.a",
        Some("windows") => "perry_ui_windows.lib",
        #[cfg(target_os = "windows")]
        None => "perry_ui_windows.lib",
        _ => {
            if cfg!(target_os = "linux") {
                "libperry_ui_gtk4.a"
            } else {
                "libperry_ui_macos.a"
            }
        }
    };
    find_library(lib_name, target)
}

/// Search for a geisterhand library by name, checking both cross-compilation
/// target dirs (target/geisterhand/{triple}/release/) and host dir (target/geisterhand/release/).
fn find_geisterhand_lib(name: &str, target: Option<&str>) -> Option<PathBuf> {
    // Search relative to CWD first, then relative to the Perry workspace root.
    // Check both target/geisterhand/ (separate build dir) and target/ (shared build dir)
    // to support both build workflows.
    let search_roots: Vec<PathBuf> = {
        let mut roots = vec![PathBuf::from(".")];
        if let Some(ws) = find_perry_workspace_root() {
            roots.push(ws);
        }
        roots
    };
    for root in &search_roots {
        // Cross-compilation target dir first
        if let Some(triple) = rust_target_triple(target) {
            // Separate geisterhand build dir
            let path = root.join(format!("target/geisterhand/{}/release/{}", triple, name));
            if path.exists() { return Some(path); }
            // Shared release dir (when built with --features geisterhand in normal target)
            let path = root.join(format!("target/{}/release/{}", triple, name));
            if path.exists() { return Some(path); }
        }
        // Host build dir
        let path = root.join(format!("target/geisterhand/release/{}", name));
        if path.exists() { return Some(path); }
        let path = root.join(format!("target/release/{}", name));
        if path.exists() { return Some(path); }
    }
    None
}

fn find_geisterhand_library(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("windows")) || cfg!(target_os = "windows") {
        "perry_ui_geisterhand.lib"
    } else {
        "libperry_ui_geisterhand.a"
    };
    find_geisterhand_lib(name, target)
        .or_else(|| find_library(name, None))
}

fn find_geisterhand_runtime(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("windows")) || cfg!(target_os = "windows") {
        "perry_runtime.lib"
    } else {
        "libperry_runtime.a"
    };
    find_geisterhand_lib(name, target)
}

fn find_geisterhand_ui(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("ios-simulator") | Some("ios")) {
        "libperry_ui_ios.a"
    } else if matches!(target, Some("android")) {
        "libperry_ui_android.a"
    } else if matches!(target, Some("linux")) || cfg!(target_os = "linux") {
        "libperry_ui_gtk4.a"
    } else if matches!(target, Some("windows")) || cfg!(target_os = "windows") {
        "perry_ui_windows.lib"
    } else {
        "libperry_ui_macos.a"
    };
    find_geisterhand_lib(name, target)
}

/// Auto-build geisterhand-enabled libraries when they're missing.
/// Uses a separate target dir (target/geisterhand/) to avoid mixing with normal builds.
fn build_geisterhand_libs(target: Option<&str>, format: OutputFormat) -> Result<()> {
    // Determine which UI crate to build based on target platform
    let ui_crate = match target {
        Some("ios-simulator") | Some("ios") => "perry-ui-ios",
        Some("android") => "perry-ui-android",
        Some("linux") => "perry-ui-gtk4",
        Some("windows") => "perry-ui-windows",
        _ if cfg!(target_os = "linux") => "perry-ui-gtk4",
        _ if cfg!(target_os = "windows") => "perry-ui-windows",
        _ => "perry-ui-macos",
    };

    match format {
        OutputFormat::Text => println!("Building geisterhand libraries ({}, {})...", ui_crate,
            rust_target_triple(target).unwrap_or("host")),
        OutputFormat::Json => {}
    }

    // Find the Perry workspace root by looking for Cargo.toml with [workspace]
    // relative to the perry executable
    let workspace_root = find_perry_workspace_root()
        .ok_or_else(|| anyhow!(
            "Cannot auto-build geisterhand libraries: Perry workspace not found.\n\
            Build manually from the Perry source directory:\n  \
            CARGO_TARGET_DIR=target/geisterhand cargo build --release \\\n    \
            -p perry-runtime --features geisterhand \\\n    \
            -p {} --features geisterhand \\\n    \
            -p perry-ui-geisterhand", ui_crate
        ))?;

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", workspace_root.join("target/geisterhand"))
        .arg("build")
        .arg("--release")
        .arg("-p").arg("perry-runtime").arg("--features").arg("perry-runtime/geisterhand")
        .arg("-p").arg(ui_crate).arg("--features").arg(format!("{}/geisterhand", ui_crate))
        .arg("-p").arg("perry-ui-geisterhand");

    // Add cross-compilation target if needed
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }

    let status = cargo_cmd.status()
        .map_err(|e| anyhow!("Failed to run cargo: {}", e))?;

    if !status.success() {
        return Err(anyhow!("Failed to build geisterhand libraries (cargo exited with {})", status));
    }

    match format {
        OutputFormat::Text => println!("Geisterhand libraries built successfully"),
        OutputFormat::Json => {}
    }
    Ok(())
}

/// Find the Perry workspace root by searching upward from the executable location.
pub fn find_perry_workspace_root() -> Option<PathBuf> {
    // First try: relative to the perry executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Binary in target/release/ → workspace is ../../
            for ancestor in [dir, &dir.join(".."), &dir.join("../.."), &dir.join("../../..")] {
                let candidate = std::fs::canonicalize(ancestor).ok()?;
                if candidate.join("crates/perry-runtime").is_dir()
                    && candidate.join("crates/perry-ui-geisterhand").is_dir()
                {
                    return Some(candidate);
                }
            }
        }
    }
    // Second try: current working directory or its ancestors
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            if dir.join("crates/perry-runtime").is_dir()
                && dir.join("crates/perry-ui-geisterhand").is_dir()
            {
                return Some(dir.to_path_buf());
            }
            dir = dir.parent()?;
        }
    }
    None
}

/// Check if a package directory has a perry.nativeLibrary field in its package.json
fn has_perry_native_library(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg.get("perry")
                .and_then(|p| p.get("nativeLibrary"))
                .is_some();
        }
    }
    false
}

/// Check if a package directory has `perry.nativeModule: true` in its package.json.
///
/// Packages that set this flag contain Perry-compatible TypeScript source code
/// and should be compiled natively (NativeCompiled) rather than interpreted via V8.
/// This is the mechanism used by `perry-react`, `perry-react-dom`, and similar
/// first-party TypeScript packages that rely on `perry/ui` or other native modules.
fn has_perry_native_module(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg.get("perry")
                .and_then(|p| p.get("nativeModule"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
    }
    false
}

/// Parse a native library manifest from a package's package.json
fn parse_native_library_manifest(
    package_dir: &Path,
    module_name: &str,
    target: Option<&str>,
) -> Option<NativeLibraryManifest> {
    let package_json = package_dir.join("package.json");
    let content = fs::read_to_string(&package_json).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    let native_lib = pkg.get("perry")?.get("nativeLibrary")?;

    // Parse functions
    let functions: Vec<NativeFunctionDecl> = native_lib.get("functions")?
        .as_array()?
        .iter()
        .filter_map(|f| {
            Some(NativeFunctionDecl {
                name: f.get("name")?.as_str()?.to_string(),
                params: f.get("params")?
                    .as_array()?
                    .iter()
                    .filter_map(|p| p.as_str().map(|s| s.to_string()))
                    .collect(),
                returns: f.get("returns")?.as_str()?.to_string(),
            })
        })
        .collect();

    // Parse target config
    let target_key = match target {
        Some("ios-simulator") | Some("ios") => "ios",
        Some("android") => "android",
        Some("tvos-simulator") | Some("tvos") => "tvos",
        Some("linux") => "linux",
        Some("windows") => "windows",
        Some("web") => "web",
        None if cfg!(target_os = "linux") => "linux",
        None if cfg!(target_os = "windows") => "windows",
        _ => "macos",
    };

    let target_config = native_lib.get("targets")
        .and_then(|t| t.get(target_key))
        .map(|tc| {
            TargetNativeConfig {
                crate_path: package_dir.join(
                    tc.get("crate").and_then(|c| c.as_str()).unwrap_or("")
                ),
                lib_name: tc.get("lib").and_then(|l| l.as_str())
                    .unwrap_or("").to_string(),
                frameworks: tc.get("frameworks")
                    .and_then(|f| f.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                libs: tc.get("libs")
                    .and_then(|l| l.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                pkg_config: tc.get("pkgConfig")
                    .and_then(|p| p.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            }
        });

    Some(NativeLibraryManifest {
        module: module_name.to_string(),
        package_dir: package_dir.to_path_buf(),
        functions,
        target_config,
    })
}

/// Packages that Perry provides built-in native extensions for.
/// These must never be loaded into V8 — Perry's codegen intercepts all imports
/// from these packages and replaces them with native calls.
const PERRY_NATIVE_EXTENSION_PACKAGES: &[&str] = &[
    "ioredis", "ethers", "mysql2", "ws", "dotenv",
];

/// Check if a file path is inside a Perry native extension package (has built-in stdlib support)
/// or a package that has perry.nativeLibrary in its package.json.
fn is_in_perry_native_package(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    // Check hardcoded native extension packages first (fast path)
    for pkg_name in PERRY_NATIVE_EXTENSION_PACKAGES {
        let needle_slash = format!("node_modules/{}/", pkg_name);
        let needle_end = format!("node_modules/{}", pkg_name);
        if path_str.contains(&needle_slash) || path_str.ends_with(&needle_end) {
            return true;
        }
    }
    // Fall back to package.json perry.nativeLibrary check
    let mut current = path.parent();
    while let Some(dir) = current {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            return has_perry_native_library(dir);
        }
        // Stop at node_modules boundary
        if dir.file_name().map(|n| n == "node_modules").unwrap_or(false) {
            break;
        }
        current = dir.parent();
    }
    false
}

/// Extract the package directory from a resolved path for a given package name.
/// E.g., for path "/project/node_modules/@noble/curves/node_modules/@noble/hashes/src/sha256.ts"
/// and package_name "@noble/hashes", returns "/project/node_modules/@noble/curves/node_modules/@noble/hashes"
fn extract_compile_package_dir(resolved_path: &Path, package_name: &str) -> Option<PathBuf> {
    let path_str = resolved_path.to_string_lossy();
    let needle = format!("node_modules/{}", package_name);
    // Use rfind to handle deeply nested node_modules
    if let Some(idx) = path_str.rfind(&needle) {
        Some(PathBuf::from(&path_str[..idx + needle.len()]))
    } else {
        None
    }
}

/// Check if a file path is inside a package listed in compile_packages
fn is_in_compile_package(path: &Path, compile_packages: &HashSet<String>) -> bool {
    let path_str = path.to_string_lossy();
    for pkg_name in compile_packages {
        let pattern = format!("node_modules/{}/", pkg_name);
        if path_str.contains(&pattern) {
            return true;
        }
    }
    false
}

/// Find node_modules directory starting from a given path
fn find_node_modules(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let node_modules = current.join("node_modules");
        if node_modules.is_dir() {
            return Some(node_modules);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Parse a package specifier into (package_name, subpath)
fn parse_package_specifier(specifier: &str) -> (String, Option<String>) {
    if specifier.starts_with('@') {
        // Scoped package: @scope/package or @scope/package/subpath
        let parts: Vec<&str> = specifier.splitn(3, '/').collect();
        if parts.len() >= 2 {
            let package_name = format!("{}/{}", parts[0], parts[1]);
            let subpath = if parts.len() > 2 {
                Some(parts[2].to_string())
            } else {
                None
            };
            return (package_name, subpath);
        }
    } else {
        // Regular package: package or package/subpath
        let parts: Vec<&str> = specifier.splitn(2, '/').collect();
        let package_name = parts[0].to_string();
        let subpath = if parts.len() > 1 {
            Some(parts[1].to_string())
        } else {
            None
        };
        return (package_name, subpath);
    }

    (specifier.to_string(), None)
}

/// Try to resolve a path with common extensions
/// Prefers TypeScript source files over JavaScript for native compilation
fn resolve_with_extensions(base: &Path) -> Option<PathBuf> {
    // TypeScript extensions to try (in order of preference)
    let ts_extensions = [".ts", ".tsx", ".mts"];
    // JavaScript extensions (fallback)
    let js_extensions = [".js", ".mjs", ".cjs"];
    // All extensions in order of preference
    let all_extensions = [".ts", ".tsx", ".mts", ".js", ".mjs", ".cjs", ".json"];

    // Check if the path has an explicit JS extension - if so, try TS equivalents first
    if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
        if matches!(ext, "js" | "mjs" | "cjs") {
            // Strip the JS extension and try TS extensions first
            let stem = base.with_extension("");
            for ts_ext in ts_extensions {
                let ts_path = stem.with_extension(ts_ext.trim_start_matches('.'));
                if ts_path.exists() && ts_path.is_file() {
                    return Some(ts_path);
                }
            }
            // If no TS file found, fall back to the original JS file
            if base.exists() && base.is_file() {
                return Some(base.to_path_buf());
            }
        }
    }

    // If it already exists as-is (and not a JS file that we already handled above)
    if base.exists() && base.is_file() {
        // Even if it exists, check for TS version first
        if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "js" | "mjs" | "cjs") {
                let stem = base.with_extension("");
                for ts_ext in ts_extensions {
                    let ts_path = stem.with_extension(ts_ext.trim_start_matches('.'));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
        }
        return Some(base.to_path_buf());
    }

    // Try with extensions in order of preference (TS before JS)
    for ext in all_extensions {
        let with_ext = base.with_extension(ext.trim_start_matches('.'));
        if with_ext.exists() && with_ext.is_file() {
            return Some(with_ext);
        }

        // Also try adding extension to full path (for paths like ./foo.js)
        let path_str = base.to_string_lossy();
        let with_ext = PathBuf::from(format!("{}{}", path_str, ext));
        if with_ext.exists() && with_ext.is_file() {
            // If we found a JS file, check for TS equivalent first
            if matches!(ext, ".js" | ".mjs" | ".cjs") {
                let stem_str = path_str.to_string();
                for ts_ext in ts_extensions {
                    let ts_path = PathBuf::from(format!("{}{}", stem_str, ts_ext));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
            return Some(with_ext);
        }
    }

    // Try index files in directory
    if base.is_dir() {
        for ext in all_extensions {
            let index = base.join(format!("index{}", ext));
            if index.exists() {
                return Some(index);
            }
        }
    }

    None
}

/// Resolve package.json entry point
fn resolve_package_entry(package_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
    let package_json = package_dir.join("package.json");
    if !package_json.exists() {
        // Fall back to index.js
        return resolve_with_extensions(&package_dir.join("index"));
    }

    let content = fs::read_to_string(&package_json).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try "exports" field first (modern packages), for both main and subpaths
    let export_key = if let Some(sub) = subpath {
        format!("./{}", sub)
    } else {
        ".".to_string()
    };

    if let Some(exports) = pkg.get("exports") {
        if let Some(entry) = resolve_exports(exports, &export_key) {
            let entry_path = package_dir.join(&entry);
            if entry_path.exists() {
                return Some(entry_path);
            }
        }
    }

    // If there's a subpath and exports didn't match, resolve it directly
    if let Some(sub) = subpath {
        let subpath_resolved = package_dir.join(sub);
        return resolve_with_extensions(&subpath_resolved);
    }

    // Try "types" or "typings" field for TypeScript
    for field in ["types", "typings"] {
        if let Some(types_path) = pkg.get(field).and_then(|v| v.as_str()) {
            // Look for corresponding .ts file
            let types_file = package_dir.join(types_path);
            let ts_file = types_file.with_extension("ts");
            // Skip .d.ts declaration files - they're type-only, not real source
            if ts_file.exists() && !ts_file.to_string_lossy().ends_with(".d.ts") {
                return Some(ts_file);
            }
        }
    }

    // Try "module" field (ESM)
    if let Some(module) = pkg.get("module").and_then(|v| v.as_str()) {
        let module_path = package_dir.join(module);
        if module_path.exists() {
            return Some(module_path);
        }
    }

    // Try "main" field (CommonJS)
    if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
        let main_path = package_dir.join(main);
        return resolve_with_extensions(&main_path);
    }

    // Fall back to index files
    resolve_with_extensions(&package_dir.join("index"))
}

/// Resolve package entry preferring TypeScript source over compiled JS output.
/// Used for compile_packages where we want to compile from TS source, not bundled JS.
fn resolve_package_source_entry(package_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
    // For subpaths, try src/<subpath>.ts
    if let Some(sub) = subpath {
        let src_path = package_dir.join("src").join(sub);
        if let Some(resolved) = resolve_with_extensions(&src_path) {
            if !is_js_file(&resolved) {
                return Some(resolved);
            }
        }
    }

    // Try src/index.ts (most common TS source entry)
    let src_index = package_dir.join("src").join("index");
    if let Some(resolved) = resolve_with_extensions(&src_index) {
        if !is_js_file(&resolved) {
            return Some(resolved);
        }
    }

    // Try using normal entry resolution but prefer TS over JS
    let normal_entry = resolve_package_entry(package_dir, subpath)?;
    if is_js_file(&normal_entry) {
        // Try .ts equivalent of the .js entry
        let ts_path = normal_entry.with_extension("ts");
        if ts_path.exists() {
            return Some(ts_path);
        }
        // Check src/ directory mirror of lib/ or dist/ path
        if let Ok(rel) = normal_entry.strip_prefix(package_dir) {
            let rel_str = rel.to_string_lossy();
            if rel_str.starts_with("lib") || rel_str.starts_with("dist") {
                let stripped = if rel_str.starts_with("lib") {
                    rel.strip_prefix("lib")
                } else {
                    rel.strip_prefix("dist")
                };
                if let Some(rest) = stripped.ok() {
                    let src_equiv = package_dir.join("src").join(rest).with_extension("ts");
                    if src_equiv.exists() {
                        return Some(src_equiv);
                    }
                }
            }
        }
    }

    None
}

/// Resolve exports field from package.json
fn resolve_exports(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    match exports {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => {
            // Try the specific subpath first
            if let Some(entry) = map.get(subpath) {
                return resolve_exports(entry, subpath);
            }

            // Try wildcard patterns (e.g., "./*" -> "./src/*.ts")
            for (key, value) in map.iter() {
                if key.contains('*') {
                    // Convert "./*" to a prefix/suffix match
                    let parts: Vec<&str> = key.splitn(2, '*').collect();
                    if parts.len() == 2 {
                        let prefix = parts[0];
                        let suffix = parts[1];
                        if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                            let matched = &subpath[prefix.len()..subpath.len() - suffix.len()];
                            if let Some(template) = resolve_exports(value, subpath) {
                                return Some(template.replace('*', matched));
                            }
                        }
                    }
                }
            }

            // Try common conditions (for both main entry and subpath entries)
            // This handles the case where we've matched a subpath and now need to resolve the conditions
            for condition in ["import", "module", "default", "require", "node"] {
                if let Some(entry) = map.get(condition) {
                    return resolve_exports(entry, subpath);
                }
            }

            None
        }
        _ => None,
    }
}

/// Determine if a file is a JavaScript file (not TypeScript)
fn is_js_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "js" | "mjs" | "cjs")
    } else {
        false
    }
}

/// Determine if a file is a TypeScript declaration file (.d.ts)
fn is_declaration_file(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".d.ts")
}

/// Determine if a file is a TypeScript file (but not a declaration file)
fn is_ts_file(path: &Path) -> bool {
    if is_declaration_file(path) {
        return false;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "ts" | "tsx")
    } else {
        false
    }
}

/// Resolve an import specifier to a file path
fn resolve_import(
    import_source: &str,
    importer_path: &Path,
    project_root: &Path,
    compile_packages: &HashSet<String>,
    compile_package_dirs: &HashMap<String, PathBuf>,
) -> Option<(PathBuf, ModuleKind)> {
    // Check if it's a native Rust stdlib module
    if perry_hir::is_native_module(import_source) {
        return None; // Native modules are handled by stdlib, not file imports
    }

    // Handle relative imports (./ or ../)
    if import_source.starts_with("./") || import_source.starts_with("../") {
        let parent = importer_path.parent()?;
        let resolved = parent.join(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let kind = if is_js_file(&path) && !is_in_compile_package(&path, compile_packages) {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((path.canonicalize().ok()?, kind));
        }
        return None;
    }

    // Handle absolute paths
    if import_source.starts_with('/') {
        let resolved = PathBuf::from(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let kind = if is_js_file(&path) {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((path.canonicalize().ok()?, kind));
        }
        return None;
    }

    // Handle node_modules (bare specifiers)
    let (package_name, subpath) = parse_package_specifier(import_source);

    // For compile_packages, search project root first to prefer ESM versions
    // over nested CJS copies (e.g., @solana/web3.js/node_modules/bs58 is CJS,
    // but the top-level node_modules/bs58 has ESM support)
    let search_paths = if compile_packages.contains(&package_name) {
        [Some(project_root), importer_path.parent()]
    } else {
        [importer_path.parent(), Some(project_root)]
    };

    for start in search_paths.iter().flatten() {
        if let Some(node_modules) = find_node_modules(start) {
            let package_dir = node_modules.join(&package_name);
            if package_dir.is_dir() {
                if let Some(entry) = resolve_package_entry(&package_dir, subpath.as_deref()) {
                    // Packages with perry.nativeLibrary are compiled natively (Rust FFI)
                    if has_perry_native_library(&package_dir) {
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // Packages with perry.nativeModule: true contain Perry-compatible
                    // TypeScript that must be compiled natively (e.g. perry-react).
                    if has_perry_native_module(&package_dir) {
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // Packages listed in perry.compilePackages are compiled natively
                    if compile_packages.contains(&package_name) {
                        // Deduplicate: if we've already resolved this package from a
                        // different node_modules location, use the first-found directory
                        // to avoid duplicate symbols from identical package copies
                        let effective_dir = compile_package_dirs
                            .get(&package_name)
                            .unwrap_or(&package_dir);
                        // Prefer TypeScript source over compiled JS
                        if let Some(src_entry) = resolve_package_source_entry(effective_dir, subpath.as_deref()) {
                            return Some((src_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                        }
                        // Fall back to normal resolution but still mark as NativeCompiled
                        if let Some(fallback_entry) = resolve_package_entry(effective_dir, subpath.as_deref()) {
                            return Some((fallback_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                        }
                        // If effective_dir failed (shouldn't happen), try the local dir
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // For other node_modules packages, treat as Interpreted
                    // Even .ts files in node_modules are library source code,
                    // not user code to be compiled. V8 will handle them at runtime.
                    return Some((entry.canonicalize().ok()?, ModuleKind::Interpreted));
                }
            }
        }
    }

    None
}

/// Discover extension entry points from a directory of plugins.
/// Each subdirectory is checked for a package.json with an `openclaw.extensions` array.
/// Returns Vec<(entry_path, plugin_id)> — e.g., ("extensions/telegram/index.ts", "telegram").
fn discover_extension_entries(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = Vec::new();

    if !dir.is_dir() {
        return Err(anyhow!("--bundle-extensions path is not a directory: {}", dir.display()));
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let subdir = entry.path();
        if !subdir.is_dir() {
            continue;
        }

        let plugin_id = subdir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let pkg_json_path = subdir.join("package.json");
        if pkg_json_path.exists() {
            // Read package.json and look for openclaw.extensions
            let pkg_contents = fs::read_to_string(&pkg_json_path)
                .map_err(|e| anyhow!("Failed to read {}: {}", pkg_json_path.display(), e))?;
            let pkg: serde_json::Value = serde_json::from_str(&pkg_contents)
                .map_err(|e| anyhow!("Failed to parse {}: {}", pkg_json_path.display(), e))?;

            let extensions = pkg.get("openclaw")
                .and_then(|oc| oc.get("extensions"))
                .and_then(|ext| ext.as_array());

            if let Some(ext_array) = extensions {
                for ext_entry in ext_array {
                    if let Some(rel_path) = ext_entry.as_str() {
                        let entry_path = subdir.join(rel_path.trim_start_matches("./"));
                        if entry_path.exists() {
                            entries.push((entry_path, plugin_id.clone()));
                        }
                    }
                }
            } else {
                // Fallback: look for index.ts
                let index_path = subdir.join("index.ts");
                if index_path.exists() {
                    entries.push((index_path, plugin_id));
                }
            }
        } else {
            // No package.json — try index.ts directly
            let index_path = subdir.join("index.ts");
            if index_path.exists() {
                entries.push((index_path, plugin_id));
            }
        }
    }

    // Sort for deterministic ordering
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Compute a sanitized module prefix from a resolved path for scoped cross-module symbols
fn compute_module_prefix(resolved_path: &str, project_root: &Path) -> String {
    let source_path = PathBuf::from(resolved_path);
    let source_module_name = source_path
        .strip_prefix(project_root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| source_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("module")
            .to_string());
    source_module_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
}

/// Cached wrapper around resolve_import to avoid redundant I/O
fn cached_resolve_import(
    import_source: &str,
    importer_path: &Path,
    ctx: &mut CompilationContext,
) -> Option<(PathBuf, ModuleKind)> {
    let importer_dir = importer_path.parent().unwrap_or(importer_path).to_path_buf();
    let cache_key = (import_source.to_string(), importer_dir);
    if let Some(cached) = ctx.resolve_cache.get(&cache_key) {
        return cached.clone();
    }
    let result = resolve_import(import_source, importer_path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs);
    ctx.resolve_cache.insert(cache_key, result.clone());
    result
}

/// Collect all modules to compile (transitive closure of imports)
fn collect_modules(
    entry_path: &PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    enable_js_runtime: bool,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
) -> Result<()> {
    let canonical = entry_path
        .canonicalize()
        .map_err(|e| anyhow!("Failed to canonicalize {}: {}", entry_path.display(), e))?;

    if visited.contains(&canonical) {
        return Ok(());
    }
    visited.insert(canonical.clone());

    // Check if this file should be handled by JS runtime instead of native compilation
    // This includes: JS files, declaration files (.d.ts), JSON files, or any file in node_modules when JS runtime is enabled
    let is_json = canonical.extension().and_then(|e| e.to_str()) == Some("json");
    let is_in_node_modules = canonical.to_string_lossy().contains("node_modules");
    let is_perry_native = is_in_node_modules && is_in_perry_native_package(&canonical);
    let is_in_compiled_pkg = (is_in_node_modules && is_in_compile_package(&canonical, &ctx.compile_packages))
        || ctx.compile_package_dirs.values().any(|dir| {
            if canonical.starts_with(dir) {
                // Exclude nested node_modules/ inside the compiled package
                // (e.g., @solana/web3.js/node_modules/bs58/ is NOT part of @solana/web3.js)
                let relative = canonical.strip_prefix(dir).unwrap_or(canonical.as_ref());
                !relative.to_string_lossy().contains("node_modules/")
            } else {
                false
            }
        });
    let should_use_js_runtime = (is_js_file(&canonical) && !is_in_compiled_pkg)
        || is_declaration_file(&canonical)
        || is_json
        || (enable_js_runtime && is_in_node_modules && !is_perry_native && !is_in_compiled_pkg);

    // Skip JSON files — they're data, not code (imported via `with { type: "json" }`)
    if is_json {
        return Ok(());
    }

    if should_use_js_runtime {

        // Skip declaration files - they're just type information
        if is_declaration_file(&canonical) {
            return Ok(());
        }

        // Perry native extension packages (ioredis, ethers, mysql2, ws, dotenv) are handled
        // entirely by Perry's built-in stdlib — they must NOT be loaded into V8.
        if is_perry_native {
            return Ok(());
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;

        let specifier = canonical.to_string_lossy().to_string();
        ctx.js_modules.insert(specifier.clone(), JsModule {
            path: canonical.clone(),
            source,
            specifier,
        });
        ctx.needs_js_runtime = true;

        // We don't parse JS/node_modules files for their imports (V8 will handle that at runtime)
        return Ok(());
    }

    // It's a TypeScript file to compile natively
    let source = fs::read_to_string(&canonical)
        .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;

    let filename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.ts");

    // Use a relative path from project root for unique module names
    // This ensures files like "routes/auth.ts" and "middleware/auth.ts" have different names
    let module_name = canonical
        .strip_prefix(&ctx.project_root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| filename.to_string());

    let ast_module = perry_parser::parse_typescript(&source, filename)
        .map_err(|e| anyhow!("Failed to parse {}: {}", canonical.display(), e))?;
    let source_file_path = canonical.to_string_lossy().to_string();

    // If type checking is enabled, resolve types from tsgo before lowering
    let resolved_types = if ctx.type_checker.is_some() {
        let positions = super::typecheck::collect_untyped_positions(&ast_module);
        if !positions.is_empty() {
            let client = ctx.type_checker.as_mut().unwrap();
            match super::typecheck::resolve_types_for_file(client, &source_file_path, &positions) {
                Ok(types) => {
                    if !types.is_empty() {
                        Some(types)
                    } else {
                        None
                    }
                }
                Err(_) => None, // Silently continue without resolved types on error
            }
        } else {
            None
        }
    } else {
        None
    };

    let (mut hir_module, new_next_class_id) = perry_hir::lower_module_with_class_id_and_types(
        &ast_module, &module_name, &source_file_path, *next_class_id, resolved_types
    )?;
    *next_class_id = new_next_class_id; // Update the global class_id counter

    if !skip_transforms {
        // Apply function inlining optimization
        inline_functions(&mut hir_module);

        // Transform generator functions into state machines
        transform_generators(&mut hir_module);
    }

    // Process imports and update their resolved paths and module kinds
    for import in &mut hir_module.imports {
        // Apply package alias (e.g., @parse/node-apn → perry-push from perry.packageAliases)
        if let Some(alias) = ctx.package_aliases.get(import.source.as_str()).cloned() {
            import.source = alias;
            import.is_native = perry_hir::is_native_module(&import.source);
        }

        if import.is_native {
            import.module_kind = ModuleKind::NativeRust;
            if import.source == "perry/ui" {
                ctx.needs_ui = true;
            }
            if import.source == "perry/plugin" {
                ctx.needs_plugins = true;
            }
            if perry_hir::requires_stdlib(&import.source) {
                ctx.needs_stdlib = true;
            }
            continue;
        }

        if let Some((resolved_path, kind)) = cached_resolve_import(&import.source, &canonical, ctx) {
            import.resolved_path = Some(resolved_path.to_string_lossy().to_string());
            import.module_kind = kind;

            match kind {
                ModuleKind::NativeCompiled => {
                    // Record compile package directory for dedup (first-found wins).
                    // When the same package exists in multiple nested node_modules/,
                    // we always resolve to the first-found copy to avoid duplicate symbols.
                    let module_name = &import.source;
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        let (pkg_name, _) = parse_package_specifier(module_name);
                        if ctx.compile_packages.contains(&pkg_name) && !ctx.compile_package_dirs.contains_key(&pkg_name) {
                            if let Some(pkg_dir) = extract_compile_package_dir(&resolved_path, &pkg_name) {
                                ctx.compile_package_dirs.insert(pkg_name, pkg_dir);
                            } else {
                                // Symlinked local package: canonical path is outside node_modules.
                                // Walk up from resolved_path to find the package root (dir with package.json).
                                let mut dir = resolved_path.parent();
                                while let Some(d) = dir {
                                    if d.join("package.json").exists() {
                                        ctx.compile_package_dirs.insert(pkg_name, d.to_path_buf());
                                        break;
                                    }
                                    dir = d.parent();
                                }
                            }
                        }
                    }
                    // Collect native library manifest (FFI functions, build config)
                    // Only for package imports (not relative imports within the same package)
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        if !ctx.native_libraries.iter().any(|nl| nl.module == *module_name) {
                            // Walk up to find the package directory with perry.nativeLibrary
                            // Works for both node_modules packages and symlinked local packages
                            let mut pkg_dir = resolved_path.parent();
                            while let Some(dir) = pkg_dir {
                                if dir.join("package.json").exists() && has_perry_native_library(dir) {
                                    if let Some(manifest) = parse_native_library_manifest(dir, module_name, target) {
                                        match format {
                                            OutputFormat::Text => println!("  Native library: {} ({} FFI functions)", manifest.module, manifest.functions.len()),
                                            OutputFormat::Json => {}
                                        }
                                        ctx.native_libraries.push(manifest);
                                    }
                                    break;
                                }
                                pkg_dir = dir.parent();
                            }
                        }
                    }
                    // Recursively collect TypeScript modules
                    collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms)?;
                }
                ModuleKind::Interpreted => {
                    // Perry native extension packages (ioredis, ethers, ws, mysql2, dotenv)
                    // are handled entirely by Perry's built-in stdlib at codegen time.
                    // They must NOT be loaded into V8 — skip them entirely.
                    if is_in_perry_native_package(&resolved_path) {
                        continue;
                    }

                    // Skip declaration files (.d.ts) - they only contain type information
                    if is_declaration_file(&resolved_path) {
                        continue;
                    }

                    // Auto-enable JS runtime for JavaScript imports

                    // Even for Interpreted imports, collect native library manifest if
                    // the resolved package has perry.nativeLibrary (handles symlinked packages
                    // where has_perry_native_library returns false for the symlink path but the
                    // canonical resolved path walks up to the correct package.json).
                    let module_name = &import.source;
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        if !ctx.native_libraries.iter().any(|nl| nl.module == *module_name) {
                            let mut pkg_dir = resolved_path.parent();
                            while let Some(dir) = pkg_dir {
                                if dir.join("package.json").exists() && has_perry_native_library(dir) {
                                    if let Some(manifest) = parse_native_library_manifest(dir, module_name, target) {
                                        match format {
                                            OutputFormat::Text => println!("  Native library: {} ({} FFI functions)", manifest.module, manifest.functions.len()),
                                            OutputFormat::Json => {}
                                        }
                                        ctx.native_libraries.push(manifest);
                                    }
                                    break;
                                }
                                pkg_dir = dir.parent();
                            }
                        }
                    }

                    match format {
                        OutputFormat::Text => {
                            println!("  JS module: {} -> {}", import.source, resolved_path.display());
                        }
                        OutputFormat::Json => {}
                    }

                    // Collect JS module
                    collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms)?;
                }
                ModuleKind::NativeRust => {
                    // Native Rust modules are handled by stdlib
                }
            }
        } else {
            // Could not resolve - might be a Node.js builtin or missing module
            // For now, treat unresolved non-native imports as errors
            if !import.is_native {
                match format {
                    OutputFormat::Text => {
                        println!("  Warning: Could not resolve import '{}' from {}", import.source, filename);
                    }
                    OutputFormat::Json => {}
                }
            }
        }
    }

    // Process re-exports
    for export in &hir_module.exports {
        let source = match export {
            perry_hir::Export::ReExport { source, .. } => Some(source),
            perry_hir::Export::ExportAll { source } => Some(source),
            perry_hir::Export::Named { .. } => None,
        };
        if let Some(src) = source {
            if let Some((resolved_path, kind)) = cached_resolve_import(src, &canonical, ctx) {
                match kind {
                    ModuleKind::NativeCompiled => {
                        collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms)?;
                    }
                    ModuleKind::Interpreted => {
                        if enable_js_runtime {
                            collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms)?;
                        }
                    }
                    ModuleKind::NativeRust => {}
                }
            }
        }
    }

    // Detect fetch() usage — js_fetch_with_options lives in perry-stdlib
    if !ctx.needs_stdlib && hir_module.uses_fetch {
        ctx.needs_stdlib = true;
    }

    // Detect ioredis usage (detected by class name, not import path)
    if !ctx.needs_stdlib {
        for (_, module_name, _) in &hir_module.exported_native_instances {
            if module_name == "ioredis" {
                ctx.needs_stdlib = true;
                break;
            }
        }
        if !ctx.needs_stdlib {
            for (_, module_name, _) in &hir_module.exported_func_return_native_instances {
                if module_name == "ioredis" {
                    ctx.needs_stdlib = true;
                    break;
                }
            }
        }
    }

    ctx.native_modules.insert(canonical, hir_module);
    Ok(())
}

/// Generate a JS bundle file containing all JS modules
fn generate_js_bundle(ctx: &CompilationContext, output_dir: &Path) -> Result<PathBuf> {
    let bundle_path = output_dir.join("__perry_js_bundle.js");

    let mut bundle = String::new();
    bundle.push_str("// Auto-generated JS bundle by Perry\n");
    bundle.push_str("// This file contains all JavaScript modules needed at runtime\n\n");

    bundle.push_str("globalThis.__COMPILETS_MODULES = {};\n\n");

    for (specifier, module) in &ctx.js_modules {
        // Escape the source code for embedding
        let escaped_source = module.source
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace("${", "\\${");

        bundle.push_str(&format!(
            "globalThis.__COMPILETS_MODULES[{:?}] = `{}`;\n",
            specifier, escaped_source
        ));
    }

    fs::write(&bundle_path, &bundle)?;
    Ok(bundle_path)
}

/// Compile for iOS widget target: emit SwiftUI source for WidgetKit extension
fn compile_for_ios_widget(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<CompileResult> {
    let app_bundle_id = args.app_bundle_id.as_deref()
        .ok_or_else(|| anyhow!("--app-bundle-id is required for ios-widget target"))?;

    // Collect all widget declarations from all modules
    let mut widgets: Vec<&perry_hir::ir::WidgetDecl> = Vec::new();
    for (_, hir_module) in &ctx.native_modules {
        for widget in &hir_module.widgets {
            widgets.push(widget);
        }
    }

    if widgets.is_empty() {
        return Err(anyhow!("No Widget() declarations found. Import {{ Widget }} from 'perry/widget' and call Widget({{...}})."));
    }

    match format {
        OutputFormat::Text => println!("Generating WidgetKit extension ({} widget{})...",
            widgets.len(), if widgets.len() == 1 { "" } else { "s" }),
        OutputFormat::Json => {}
    }

    // Determine output directory
    let output_dir = if let Some(ref out) = args.output {
        out.clone()
    } else {
        let stem = args.input.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("widget");
        PathBuf::from(format!("{}_widget", stem))
    };

    // Create output directory
    fs::create_dir_all(&output_dir)?;

    // Generate SwiftUI for each widget
    let mut all_swift_files: Vec<(String, String)> = Vec::new();
    let mut all_info_plists: Vec<(String, String)> = Vec::new();

    for widget in &widgets {
        let bundle = perry_codegen_swiftui::compile_widget(widget, app_bundle_id)?;

        for (filename, source) in &bundle.swift_files {
            let swift_path = output_dir.join(filename);
            fs::write(&swift_path, source)?;
            all_swift_files.push((filename.clone(), source.clone()));
        }

        // Write Info.plist
        let plist_path = output_dir.join("Info.plist");
        fs::write(&plist_path, &bundle.info_plist)?;
        all_info_plists.push(("Info.plist".to_string(), bundle.info_plist.clone()));
    }

    // Report results
    let total_size: usize = all_swift_files.iter()
        .map(|(_, s)| s.len())
        .sum();

    match format {
        OutputFormat::Text => {
            println!("Widget extension generated: {}/", output_dir.display());
            for (name, source) in &all_swift_files {
                println!("  {} ({:.1} KB)", name, source.len() as f64 / 1024.0);
            }
            println!("  Info.plist");
            println!("Total: {:.1} KB SwiftUI source", total_size as f64 / 1024.0);
            println!();
            println!("To build the widget extension:");
            let sdk = if args.target.as_deref() == Some("ios-widget-simulator") {
                "iphonesimulator"
            } else {
                "iphoneos"
            };
            println!("  xcrun --sdk {} swiftc -target arm64-apple-ios17.0 \\", sdk);
            for (name, _) in &all_swift_files {
                println!("    {}/{} \\", output_dir.display(), name);
            }
            println!("    -framework WidgetKit -framework SwiftUI \\");
            println!("    -o {}/WidgetExtension", output_dir.display());
        }
        OutputFormat::Json => {
            println!("{{\"output\": \"{}\", \"widgets\": {}, \"size\": {}, \"target\": \"ios-widget\"}}",
                output_dir.display(), widgets.len(), total_size);
        }
    }

    let target_str = args.target.as_deref().unwrap_or("ios-widget").to_string();
    Ok(CompileResult {
        output_path: output_dir,
        target: target_str,
        bundle_id: Some(app_bundle_id.to_string()),
        is_dylib: false,
    })
}

/// Compile for watchOS widget target: emit SwiftUI + native timeline (accessory families)
fn compile_for_watchos_widget(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<CompileResult> {
    let app_bundle_id = args.app_bundle_id.as_deref()
        .ok_or_else(|| anyhow!("--app-bundle-id is required for watchos-widget target"))?;

    let mut widgets: Vec<&perry_hir::ir::WidgetDecl> = Vec::new();
    for (_, hir_module) in &ctx.native_modules {
        for widget in &hir_module.widgets {
            widgets.push(widget);
        }
    }

    if widgets.is_empty() {
        return Err(anyhow!("No Widget() declarations found. Import {{ Widget }} from 'perry/widget' and call Widget({{...}})."));
    }

    match format {
        OutputFormat::Text => println!("Generating watchOS WidgetKit extension ({} complication{})...",
            widgets.len(), if widgets.len() == 1 { "" } else { "s" }),
        OutputFormat::Json => {}
    }

    let output_dir = if let Some(ref out) = args.output {
        out.clone()
    } else {
        let stem = args.input.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("widget");
        PathBuf::from(format!("{}_watchos_widget", stem))
    };

    fs::create_dir_all(&output_dir)?;

    let mut all_swift_files: Vec<(String, String)> = Vec::new();

    for widget in &widgets {
        let bundle = perry_codegen_swiftui::compile_widget(widget, app_bundle_id)?;

        for (filename, source) in &bundle.swift_files {
            let swift_path = output_dir.join(filename);
            fs::write(&swift_path, source)?;
            all_swift_files.push((filename.clone(), source.clone()));
        }

        let plist_path = output_dir.join("Info.plist");
        fs::write(&plist_path, &bundle.info_plist)?;
    }

    let total_size: usize = all_swift_files.iter().map(|(_, s)| s.len()).sum();

    match format {
        OutputFormat::Text => {
            println!("watchOS complication generated: {}/", output_dir.display());
            for (name, source) in &all_swift_files {
                println!("  {} ({:.1} KB)", name, source.len() as f64 / 1024.0);
            }
            println!("  Info.plist");
            println!("Total: {:.1} KB SwiftUI source", total_size as f64 / 1024.0);
            println!();
            println!("To build the watchOS widget extension:");
            let sdk = if args.target.as_deref() == Some("watchos-widget-simulator") {
                "watchsimulator"
            } else {
                "watchos"
            };
            println!("  xcrun --sdk {} swiftc -target arm64-apple-watchos9.0 \\", sdk);
            for (name, _) in &all_swift_files {
                println!("    {}/{} \\", output_dir.display(), name);
            }
            println!("    -framework WidgetKit -framework SwiftUI \\");
            println!("    -o {}/WidgetExtension", output_dir.display());
        }
        OutputFormat::Json => {
            println!("{{\"output\": \"{}\", \"widgets\": {}, \"size\": {}, \"target\": \"watchos-widget\"}}",
                output_dir.display(), widgets.len(), total_size);
        }
    }

    let target_str = args.target.as_deref().unwrap_or("watchos-widget").to_string();
    Ok(CompileResult {
        output_path: output_dir,
        target: target_str,
        bundle_id: Some(app_bundle_id.to_string()),
        is_dylib: false,
    })
}

/// Find the PerryWatchApp.swift runtime file.
fn find_watchos_swift_runtime() -> Option<PathBuf> {
    // 1. Check next to the perry binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("swift").join("PerryWatchApp.swift");
            if candidate.exists() {
                return Some(candidate);
            }
            // Also check ../lib/perry/swift/
            if let Some(prefix) = dir.parent() {
                let candidate = prefix.join("lib").join("perry").join("swift").join("PerryWatchApp.swift");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 2. Check in the source tree (development builds)
    let source_candidate = PathBuf::from("crates/perry-ui-watchos/swift/PerryWatchApp.swift");
    if source_candidate.exists() {
        return Some(source_candidate);
    }

    None
}

/// Look up bundle_id from perry.toml for a specific section (e.g., "watchos", "ios", "app")
fn lookup_bundle_id_from_toml(input: &std::path::Path, section: &str) -> Option<String> {
    let mut dir = input.canonicalize().ok()?;
    for _ in 0..5 {
        dir = dir.parent()?.to_path_buf();
        let toml_path = dir.join("perry.toml");
        if toml_path.exists() {
            let data = fs::read_to_string(&toml_path).ok()?;
            let doc: toml::Table = data.parse().ok()?;
            let bid = doc.get(section)
                .and_then(|s| s.get("bundle_id"))
                .or_else(|| doc.get("bundle_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if bid.is_some() {
                return bid;
            }
        }
    }
    None
}

/// Compile for Android widget target: emit Kotlin/Glance source + JNI bridge
fn compile_for_android_widget(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<CompileResult> {
    let mut widgets: Vec<&perry_hir::ir::WidgetDecl> = Vec::new();
    for (_, hir_module) in &ctx.native_modules {
        for widget in &hir_module.widgets {
            widgets.push(widget);
        }
    }

    if widgets.is_empty() {
        return Err(anyhow!("No Widget() declarations found. Import {{ Widget }} from 'perry/widget' and call Widget({{...}})."));
    }

    let app_package = args.app_bundle_id.as_deref().unwrap_or("com.perry.widget");

    match format {
        OutputFormat::Text => println!("Generating Android Glance widget ({} widget{})...",
            widgets.len(), if widgets.len() == 1 { "" } else { "s" }),
        OutputFormat::Json => {}
    }

    let output_dir = if let Some(ref out) = args.output {
        out.clone()
    } else {
        let stem = args.input.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("widget");
        PathBuf::from(format!("{}_android_widget", stem))
    };

    fs::create_dir_all(&output_dir)?;
    fs::create_dir_all(output_dir.join("xml"))?;

    let mut all_kotlin_files: Vec<(String, String)> = Vec::new();

    for widget in &widgets {
        let bundle = perry_codegen_glance::compile_widget_glance(widget, app_package)?;

        for (filename, source) in &bundle.kotlin_files {
            let kt_path = output_dir.join(filename);
            fs::write(&kt_path, source)?;
            all_kotlin_files.push((filename.clone(), source.clone()));
        }

        // Write widget_info XML
        let safe_name = widget.kind.rsplit('.').next().unwrap_or("widget").to_lowercase();
        let xml_path = output_dir.join("xml").join(format!("widget_info_{}.xml", safe_name));
        fs::write(&xml_path, &bundle.widget_info_xml)?;

        // Write manifest snippet
        let manifest_path = output_dir.join("AndroidManifest_snippet.xml");
        fs::write(&manifest_path, &bundle.manifest_snippet)?;
    }

    let total_size: usize = all_kotlin_files.iter().map(|(_, s)| s.len()).sum();

    match format {
        OutputFormat::Text => {
            println!("Android Glance widget generated: {}/", output_dir.display());
            for (name, source) in &all_kotlin_files {
                println!("  {} ({:.1} KB)", name, source.len() as f64 / 1024.0);
            }
            println!("  xml/widget_info_*.xml");
            println!("  AndroidManifest_snippet.xml");
            println!("Total: {:.1} KB Kotlin source", total_size as f64 / 1024.0);
            println!();
            println!("Add the generated files to your Android/Gradle project:");
            println!("  1. Copy *.kt files to app/src/main/java/{}/", app_package.replace('.', "/"));
            println!("  2. Copy xml/ to app/src/main/res/xml/");
            println!("  3. Merge AndroidManifest_snippet.xml into your AndroidManifest.xml");
            println!("  4. Add Glance dependency: implementation \"androidx.glance:glance-appwidget:1.1.0\"");
        }
        OutputFormat::Json => {
            println!("{{\"output\": \"{}\", \"widgets\": {}, \"size\": {}, \"target\": \"android-widget\"}}",
                output_dir.display(), widgets.len(), total_size);
        }
    }

    Ok(CompileResult {
        output_path: output_dir,
        target: "android-widget".to_string(),
        bundle_id: Some(app_package.to_string()),
        is_dylib: false,
    })
}

/// Compile for Wear OS tile target: emit Kotlin Tiles source + JNI bridge
fn compile_for_wearos_tile(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<CompileResult> {
    let mut widgets: Vec<&perry_hir::ir::WidgetDecl> = Vec::new();
    for (_, hir_module) in &ctx.native_modules {
        for widget in &hir_module.widgets {
            widgets.push(widget);
        }
    }

    if widgets.is_empty() {
        return Err(anyhow!("No Widget() declarations found. Import {{ Widget }} from 'perry/widget' and call Widget({{...}})."));
    }

    let app_package = args.app_bundle_id.as_deref().unwrap_or("com.perry.tile");

    match format {
        OutputFormat::Text => println!("Generating Wear OS tile ({} tile{})...",
            widgets.len(), if widgets.len() == 1 { "" } else { "s" }),
        OutputFormat::Json => {}
    }

    let output_dir = if let Some(ref out) = args.output {
        out.clone()
    } else {
        let stem = args.input.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("tile");
        PathBuf::from(format!("{}_wearos_tile", stem))
    };

    fs::create_dir_all(&output_dir)?;

    let mut all_kotlin_files: Vec<(String, String)> = Vec::new();

    for widget in &widgets {
        let bundle = perry_codegen_wear_tiles::compile_widget_wear_tile(widget, app_package)?;

        for (filename, source) in &bundle.kotlin_files {
            let kt_path = output_dir.join(filename);
            fs::write(&kt_path, source)?;
            all_kotlin_files.push((filename.clone(), source.clone()));
        }

        let manifest_path = output_dir.join("AndroidManifest_snippet.xml");
        fs::write(&manifest_path, &bundle.manifest_snippet)?;
    }

    let total_size: usize = all_kotlin_files.iter().map(|(_, s)| s.len()).sum();

    match format {
        OutputFormat::Text => {
            println!("Wear OS tile generated: {}/", output_dir.display());
            for (name, source) in &all_kotlin_files {
                println!("  {} ({:.1} KB)", name, source.len() as f64 / 1024.0);
            }
            println!("  AndroidManifest_snippet.xml");
            println!("Total: {:.1} KB Kotlin source", total_size as f64 / 1024.0);
            println!();
            println!("Add the generated files to your Wear OS/Gradle project:");
            println!("  1. Copy *.kt files to app/src/main/java/{}/", app_package.replace('.', "/"));
            println!("  2. Merge AndroidManifest_snippet.xml into your AndroidManifest.xml");
            println!("  3. Add dependencies:");
            println!("     implementation \"com.google.android.horologist:horologist-tiles:0.6.5\"");
            println!("     implementation \"androidx.wear.tiles:tiles-material:1.4.0\"");
        }
        OutputFormat::Json => {
            println!("{{\"output\": \"{}\", \"widgets\": {}, \"size\": {}, \"target\": \"wearos-tile\"}}",
                output_dir.display(), widgets.len(), total_size);
        }
    }

    Ok(CompileResult {
        output_path: output_dir,
        target: "wearos-tile".to_string(),
        bundle_id: Some(app_package.to_string()),
        is_dylib: false,
    })
}

/// Compile for web target: emit JavaScript + HTML instead of native code
fn compile_for_web(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<CompileResult> {
    match format {
        OutputFormat::Text => println!("Generating JavaScript for web target..."),
        OutputFormat::Json => {}
    }

    let entry_path = args.input.canonicalize().unwrap_or_else(|_| args.input.clone());

    // Build topologically sorted module list (dependencies before dependents)
    let mut sorted_paths: Vec<PathBuf> = Vec::new();
    {
        let mut path_to_deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for (path, hir_module) in &ctx.native_modules {
            let mut deps = Vec::new();
            for import in &hir_module.imports {
                if let Some(ref resolved) = import.resolved_path {
                    let resolved_path = PathBuf::from(resolved);
                    if ctx.native_modules.contains_key(&resolved_path) {
                        deps.push(resolved_path);
                    }
                }
            }
            path_to_deps.insert(path.clone(), deps);
        }

        let mut visited_set: HashSet<PathBuf> = HashSet::new();
        let mut visiting_set: HashSet<PathBuf> = HashSet::new();

        fn topo_visit(
            path: &PathBuf,
            deps: &HashMap<PathBuf, Vec<PathBuf>>,
            visited: &mut HashSet<PathBuf>,
            visiting: &mut HashSet<PathBuf>,
            sorted: &mut Vec<PathBuf>,
        ) {
            if visited.contains(path) || visiting.contains(path) { return; }
            visiting.insert(path.clone());
            if let Some(module_deps) = deps.get(path) {
                for dep in module_deps {
                    topo_visit(dep, deps, visited, visiting, sorted);
                }
            }
            visiting.remove(path);
            visited.insert(path.clone());
            sorted.push(path.clone());
        }

        let mut all: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
        all.sort();
        for path in &all {
            topo_visit(path, &path_to_deps, &mut visited_set, &mut visiting_set, &mut sorted_paths);
        }
    }

    // Ensure entry module is last
    if let Some(pos) = sorted_paths.iter().position(|p| *p == entry_path) {
        sorted_paths.remove(pos);
    }
    sorted_paths.push(entry_path.clone());

    // Build module list for JS codegen
    let modules: Vec<(String, perry_hir::Module)> = sorted_paths.iter()
        .filter_map(|path| {
            ctx.native_modules.get(path).map(|m| (m.name.clone(), m.clone()))
        })
        .collect();

    // Determine output title from entry filename
    let title = args.input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Perry App");

    // Minify by default for web target (--minify flag is auto-enabled)
    let minify = true;
    let html = perry_codegen_js::compile_modules_to_html(&modules, title, minify)?;

    // Determine output path
    let output_path = if let Some(ref out) = args.output {
        if out.extension().is_none() {
            out.with_extension("html")
        } else {
            out.clone()
        }
    } else {
        let stem = args.input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        PathBuf::from(format!("{}.html", stem))
    };

    fs::write(&output_path, &html)?;

    let file_size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
    match format {
        OutputFormat::Text => {
            println!("Web output: {} ({:.1} KB)", output_path.display(), file_size as f64 / 1024.0);
        }
        OutputFormat::Json => {
            println!("{{\"output\": \"{}\", \"size\": {}, \"target\": \"web\"}}",
                output_path.display(), file_size);
        }
    }

    Ok(CompileResult {
        output_path,
        target: "web".to_string(),
        bundle_id: None,
        is_dylib: false,
    })
}

/// Compile for WebAssembly target: emit WASM binary + JS runtime bridge
fn compile_for_wasm(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<CompileResult> {
    match format {
        OutputFormat::Text => println!("Generating WebAssembly..."),
        OutputFormat::Json => {}
    }

    let entry_path = args.input.canonicalize().unwrap_or_else(|_| args.input.clone());

    // Build topologically sorted module list (same as web target)
    let mut sorted_paths: Vec<PathBuf> = Vec::new();
    {
        let mut path_to_deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for (path, hir_module) in &ctx.native_modules {
            let mut deps = Vec::new();
            for import in &hir_module.imports {
                if let Some(ref resolved) = import.resolved_path {
                    let resolved_path = PathBuf::from(resolved);
                    if ctx.native_modules.contains_key(&resolved_path) {
                        deps.push(resolved_path);
                    }
                }
            }
            path_to_deps.insert(path.clone(), deps);
        }

        let mut visited_set: HashSet<PathBuf> = HashSet::new();
        let mut visiting_set: HashSet<PathBuf> = HashSet::new();

        fn topo_visit_wasm(
            path: &PathBuf,
            deps: &HashMap<PathBuf, Vec<PathBuf>>,
            visited: &mut HashSet<PathBuf>,
            visiting: &mut HashSet<PathBuf>,
            sorted: &mut Vec<PathBuf>,
        ) {
            if visited.contains(path) || visiting.contains(path) { return; }
            visiting.insert(path.clone());
            if let Some(module_deps) = deps.get(path) {
                for dep in module_deps {
                    topo_visit_wasm(dep, deps, visited, visiting, sorted);
                }
            }
            visiting.remove(path);
            visited.insert(path.clone());
            sorted.push(path.clone());
        }

        let mut all: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
        all.sort();
        for path in &all {
            topo_visit_wasm(path, &path_to_deps, &mut visited_set, &mut visiting_set, &mut sorted_paths);
        }
    }

    // Ensure entry module is last
    if let Some(pos) = sorted_paths.iter().position(|p| *p == entry_path) {
        sorted_paths.remove(pos);
    }
    sorted_paths.push(entry_path.clone());

    let modules: Vec<(String, perry_hir::Module)> = sorted_paths.iter()
        .filter_map(|path| {
            ctx.native_modules.get(path).map(|m| (m.name.clone(), m.clone()))
        })
        .collect();

    let title = args.input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Perry App");

    let minify = args.minify;

    // Determine output format: .html (default) or .wasm (raw binary)
    let output_path = if let Some(ref out) = args.output {
        if out.extension().map_or(false, |e| e == "wasm") {
            out.clone()
        } else if out.extension().is_none() {
            out.with_extension("html")
        } else {
            out.clone()
        }
    } else {
        let stem = args.input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        PathBuf::from(format!("{}.html", stem))
    };

    if output_path.extension().map_or(false, |e| e == "wasm") {
        // Raw WASM binary output
        let wasm = perry_codegen_wasm::compile_modules_to_wasm(&modules)?;
        fs::write(&output_path, &wasm)?;
    } else {
        // HTML with embedded WASM
        let html = perry_codegen_wasm::compile_modules_to_wasm_html(&modules, title, minify)?;
        fs::write(&output_path, &html)?;
    }

    let file_size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
    match format {
        OutputFormat::Text => {
            println!("WASM output: {} ({:.1} KB)", output_path.display(), file_size as f64 / 1024.0);
        }
        OutputFormat::Json => {
            println!("{{\"output\": \"{}\", \"size\": {}, \"target\": \"wasm\"}}",
                output_path.display(), file_size);
        }
    }

    Ok(CompileResult {
        output_path,
        target: "wasm".to_string(),
        bundle_id: None,
        is_dylib: false,
    })
}

pub fn run(args: CompileArgs, format: OutputFormat, _use_color: bool, _verbose: u8) -> Result<CompileResult> {
    match format {
        OutputFormat::Text => println!("Collecting modules..."),
        OutputFormat::Json => {}
    }

    let project_root = args.input
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));

    let mut ctx = CompilationContext::new(project_root.clone());

    // Read perry.packageAliases from the project's package.json (if present)
    // This allows mapping npm package imports to native Perry packages at compile time.
    // Example: { "@parse/node-apn": "perry-push", "@prisma/client": "perry-prisma" }
    // Walk up from project_root (which is the parent of the entry file) to find package.json.
    let pkg_json_path = {
        let mut dir = project_root.clone();
        let mut found = None;
        loop {
            let candidate = dir.join("package.json");
            if candidate.exists() {
                found = Some(candidate);
                break;
            }
            if !dir.pop() {
                break;
            }
        }
        found
    };
    if let Some(pkg_json_path) = pkg_json_path {
        if let Ok(content) = fs::read_to_string(&pkg_json_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(aliases) = pkg.get("perry").and_then(|p| p.get("packageAliases")).and_then(|a| a.as_object()) {
                    for (from, to) in aliases {
                        if let Some(to_str) = to.as_str() {
                            match format {
                                OutputFormat::Text => println!("  Package alias: {} → {}", from, to_str),
                                OutputFormat::Json => {}
                            }
                            ctx.package_aliases.insert(from.clone(), to_str.to_string());
                        }
                    }
                }
                if let Some(compile_pkgs) = pkg.get("perry").and_then(|p| p.get("compilePackages")).and_then(|a| a.as_array()) {
                    for pkg_name in compile_pkgs {
                        if let Some(name) = pkg_name.as_str() {
                            match format {
                                OutputFormat::Text => println!("  Compile package: {}", name),
                                OutputFormat::Json => {}
                            }
                            ctx.compile_packages.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // --- i18n: parse [i18n] config from perry.toml and load locale files ---
    let mut i18n_config: Option<perry_transform::i18n::I18nConfig> = None;
    let mut i18n_translations: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

    {
        let toml_path = project_root.join("perry.toml");
        if toml_path.exists() {
            if let Ok(content) = fs::read_to_string(&toml_path) {
                if let Ok(doc) = content.parse::<toml::Table>() {
                    if let Some(i18n) = doc.get("i18n").and_then(|v| v.as_table()) {
                        let locales: Vec<String> = i18n.get("locales")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                            .unwrap_or_default();
                        let default_locale = i18n.get("default_locale")
                            .and_then(|v| v.as_str())
                            .unwrap_or("en")
                            .to_string();
                        let dynamic = i18n.get("dynamic")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        // Parse [i18n.currencies] — locale → currency code
                        let mut currencies = HashMap::new();
                        if let Some(curr_table) = i18n.get("currencies").and_then(|v| v.as_table()) {
                            for (locale, code) in curr_table {
                                if let Some(code_str) = code.as_str() {
                                    currencies.insert(locale.clone(), code_str.to_string());
                                }
                            }
                        }

                        if !locales.is_empty() {
                            match format {
                                OutputFormat::Text => println!("  i18n: {} locale(s) [{}], default: {}",
                                    locales.len(), locales.join(", "), default_locale),
                                OutputFormat::Json => {}
                            }

                            // Load locale files
                            let locales_dir = project_root.join("locales");
                            for locale in &locales {
                                let locale_file = locales_dir.join(format!("{}.json", locale));
                                if locale_file.exists() {
                                    if let Ok(json_content) = fs::read_to_string(&locale_file) {
                                        match serde_json::from_str::<BTreeMap<String, String>>(&json_content) {
                                            Ok(translations) => {
                                                match format {
                                                    OutputFormat::Text => println!("    Loaded locales/{}.json ({} keys)", locale, translations.len()),
                                                    OutputFormat::Json => {}
                                                }
                                                i18n_translations.insert(locale.clone(), translations);
                                            }
                                            Err(e) => {
                                                eprintln!("  Warning: Failed to parse locales/{}.json: {}", locale, e);
                                            }
                                        }
                                    }
                                } else {
                                    eprintln!("  Warning: Locale file locales/{}.json not found", locale);
                                }
                            }

                            i18n_config = Some(perry_transform::i18n::I18nConfig {
                                locales,
                                default_locale,
                                dynamic,
                                currencies,
                            });
                        }
                    }
                }
            }
        }
    }

    // Initialize tsgo type checker if --type-check is enabled
    if args.type_check {
        match super::typecheck::TsGoClient::spawn(&project_root) {
            Ok(mut client) => {
                // Try to load the project's tsconfig.json
                if let Some(tsconfig) = super::typecheck::find_tsconfig(&project_root) {
                    match format {
                        OutputFormat::Text => println!("  Type checking enabled (tsgo)"),
                        OutputFormat::Json => {}
                    }
                    if let Err(e) = client.load_project(&tsconfig) {
                        match format {
                            OutputFormat::Text => eprintln!("  Warning: tsgo project load failed: {}. Continuing without type checking.", e),
                            OutputFormat::Json => {}
                        }
                    } else {
                        ctx.type_checker = Some(client);
                    }
                } else {
                    match format {
                        OutputFormat::Text => eprintln!("  Warning: No tsconfig.json found. Type checking disabled."),
                        OutputFormat::Json => {}
                    }
                }
            }
            Err(e) => {
                match format {
                    OutputFormat::Text => eprintln!("  Warning: {}", e),
                    OutputFormat::Json => {}
                }
            }
        }
    }

    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1; // Start at 1, 0 is reserved for "no parent"
    let skip_transforms = matches!(args.target.as_deref(), Some("web") | Some("wasm"));

    collect_modules(&args.input, &mut ctx, &mut visited, args.enable_js_runtime, format, args.target.as_deref(), &mut next_class_id, skip_transforms)?;

    // Bundle extensions if --bundle-extensions specified
    let mut bundled_extensions: Vec<(PathBuf, String)> = Vec::new();
    if let Some(ext_dir) = &args.bundle_extensions {
        let ext_entries = discover_extension_entries(ext_dir)?;
        match format {
            OutputFormat::Text => println!("Bundling {} extension(s)...", ext_entries.len()),
            OutputFormat::Json => {}
        }
        for (entry_path, plugin_id) in &ext_entries {
            match format {
                OutputFormat::Text => println!("  Extension: {} ({})", plugin_id, entry_path.display()),
                OutputFormat::Json => {}
            }
            collect_modules(entry_path, &mut ctx, &mut visited,
                           args.enable_js_runtime, format, args.target.as_deref(), &mut next_class_id, skip_transforms)?;
            bundled_extensions.push((entry_path.canonicalize()?, plugin_id.clone()));
        }
    }

    // Recompute project_root as the common ancestor of all module paths.
    // The initial project_root is the parent of the entry file, but modules may be in sibling
    // directories (e.g., entry in workers/, modules in lib/). This ensures unique module names.
    if ctx.native_modules.len() > 1 {
        let mut common: Option<PathBuf> = None;
        for path in ctx.native_modules.keys() {
            if let Some(parent) = path.parent() {
                match &common {
                    None => common = Some(parent.to_path_buf()),
                    Some(prev) => {
                        // Find common prefix of prev and parent
                        let mut new_common = PathBuf::new();
                        for (a, b) in prev.components().zip(parent.components()) {
                            if a == b {
                                new_common.push(a);
                            } else {
                                break;
                            }
                        }
                        common = Some(new_common);
                    }
                }
            }
        }
        if let Some(new_root) = common {
            if !new_root.as_os_str().is_empty() {
                ctx.project_root = new_root;
                // Re-set module names based on the new project root
                let paths: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
                for path in paths {
                    if let Some(module) = ctx.native_modules.get_mut(&path) {
                        let filename = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("module.ts");
                        module.name = path
                            .strip_prefix(&ctx.project_root)
                            .ok()
                            .and_then(|p| p.to_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| filename.to_string());
                    }
                }
            }
        }
    }

    let total_modules = ctx.native_modules.len() + ctx.js_modules.len();
    match format {
        OutputFormat::Text => {
            println!("Found {} module(s): {} native, {} JavaScript",
                total_modules,
                ctx.native_modules.len(),
                ctx.js_modules.len()
            );
        }
        OutputFormat::Json => {}
    }

    if args.enable_geisterhand || args.geisterhand_port.is_some() {
        ctx.needs_geisterhand = true;
        if let Some(port) = args.geisterhand_port {
            ctx.geisterhand_port = port;
        }
    }

    // --- Web target: emit JavaScript instead of native code ---
    if args.target.as_deref() == Some("web") {
        return compile_for_web(&ctx, &args, format);
    }

    // --- WebAssembly target: emit WASM binary + JS runtime bridge ---
    if args.target.as_deref() == Some("wasm") {
        return compile_for_wasm(&ctx, &args, format);
    }

    // --- Widget targets: emit platform-specific source + optional native provider ---
    if matches!(args.target.as_deref(), Some("ios-widget") | Some("ios-widget-simulator")) {
        return compile_for_ios_widget(&ctx, &args, format);
    }
    if matches!(args.target.as_deref(), Some("watchos-widget") | Some("watchos-widget-simulator")) {
        return compile_for_watchos_widget(&ctx, &args, format);
    }
    if args.target.as_deref() == Some("android-widget") {
        return compile_for_android_widget(&ctx, &args, format);
    }
    if args.target.as_deref() == Some("wearos-tile") {
        return compile_for_wearos_tile(&ctx, &args, format);
    }

    // Transform JS imports into runtime calls (parallel)
    use rayon::prelude::*;
    if ctx.needs_js_runtime {
        ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
            perry_hir::transform_js_imports(hir_module);
        });
    }

    // Build map of exported native instances from all modules
    let mut exported_instances: BTreeMap<(String, String), perry_hir::ExportedNativeInstance> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for (export_name, native_module, native_class) in &hir_module.exported_native_instances {
            exported_instances.insert(
                (path_str.clone(), export_name.clone()),
                perry_hir::ExportedNativeInstance {
                    native_module: native_module.clone(),
                    native_class: native_class.clone(),
                },
            );
        }
    }

    // Build map of exported functions that return native instances
    let mut exported_func_return_instances: BTreeMap<(String, String), perry_hir::ExportedNativeInstance> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for (func_name, native_module, native_class) in &hir_module.exported_func_return_native_instances {
            exported_func_return_instances.insert(
                (path_str.clone(), func_name.clone()),
                perry_hir::ExportedNativeInstance {
                    native_module: native_module.clone(),
                    native_class: native_class.clone(),
                },
            );
        }
    }

    // Fix local native instance method calls within each module (parallel)
    ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
        perry_hir::fix_local_native_instances(hir_module);
    });

    // Fix cross-module native instance method calls (parallel — reads immutable maps)
    if !exported_instances.is_empty() || !exported_func_return_instances.is_empty() {
        ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
            perry_hir::fix_cross_module_native_instances(hir_module, &exported_instances, &exported_func_return_instances);
        });
    }

    // Re-run local native instance fix after cross-module fixes (parallel)
    ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
        perry_hir::fix_local_native_instances(hir_module);
    });

    // Run monomorphization pass on all native modules (parallel)
    ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
        perry_hir::monomorphize_module(hir_module);
    });

    // --- i18n: apply i18n transform pass ---
    let i18n_table = if let Some(ref config) = i18n_config {
        let table = perry_transform::i18n::apply_i18n(
            &mut ctx.native_modules, config, &i18n_translations
        );
        // Report diagnostics
        for diag in &table.diagnostics {
            match diag.severity {
                perry_transform::i18n::I18nSeverity::Warning => {
                    match format {
                        OutputFormat::Text => eprintln!("  i18n warning: {}", diag.message),
                        OutputFormat::Json => {}
                    }
                }
                perry_transform::i18n::I18nSeverity::Error => {
                    match format {
                        OutputFormat::Text => eprintln!("  i18n error: {}", diag.message),
                        OutputFormat::Json => {}
                    }
                }
            }
        }
        match format {
            OutputFormat::Text => if !table.keys.is_empty() {
                println!("  i18n: {} localizable string(s) detected", table.keys.len());
            },
            OutputFormat::Json => {}
        }
        // Set the thread-local i18n table for codegen
        perry_codegen::set_i18n_table(
            table.translations.clone(),
            table.keys.len(),
            table.locale_count,
            table.locale_codes.clone(),
        );
        Some(table)
    } else {
        None
    };

    if args.print_hir {
        for (path, hir_module) in &ctx.native_modules {
            println!("\n=== HIR (after monomorphization): {} ===", path.display());
            println!("Module: {}", hir_module.name);
            println!("Imports: {}", hir_module.imports.len());
            for import in &hir_module.imports {
                println!(
                    "  - {} ({} specifiers, kind: {:?})",
                    import.source,
                    import.specifiers.len(),
                    import.module_kind
                );
            }
            println!("Exports: {}", hir_module.exports.len());
            println!("Functions: {}", hir_module.functions.len());
            for func in &hir_module.functions {
                println!(
                    "  - {} (params: {}, type_params: {}, async: {}, exported: {})",
                    func.name,
                    func.params.len(),
                    func.type_params.len(),
                    func.is_async,
                    func.is_exported
                );

            }
            println!("Init statements: {}", hir_module.init.len());
            for (i, stmt) in hir_module.init.iter().enumerate() {
                println!("  [{}] {:?}", i, stmt);
            }
            println!("===========\n");
        }

        if !ctx.js_modules.is_empty() {
            println!("\n=== JavaScript Modules (interpreted) ===");
            for (specifier, module) in &ctx.js_modules {
                println!("  {} -> {}", specifier, module.path.display());
            }
            println!("===========\n");
        }
    }

    // --- i18n: write key registry ---
    if let Some(ref table) = i18n_table {
        if !table.keys.is_empty() {
            let perry_dir = ctx.project_root.join(".perry");
            let _ = fs::create_dir_all(&perry_dir);
            let registry: Vec<serde_json::Value> = table.keys.iter().enumerate().map(|(i, key)| {
                serde_json::json!({
                    "key": key,
                    "string_idx": i,
                })
            }).collect();
            let registry_json = serde_json::json!({ "keys": registry });
            let _ = fs::write(perry_dir.join("i18n-keys.json"),
                serde_json::to_string_pretty(&registry_json).unwrap_or_default());
        }
    }

    match format {
        OutputFormat::Text => println!("Generating code..."),
        OutputFormat::Json => {}
    }

    let mut obj_paths = Vec::new();

    // Get canonical path of entry module
    let entry_path = args.input.canonicalize().unwrap_or_else(|_| args.input.clone());

    // Collect non-entry module names for init function calls
    // Topologically sort by import dependencies so that if module A imports from module B,
    // module B is initialized first. This ensures module-level variables (e.g., Maps) are
    // allocated before other modules try to use them via imported functions.
    let non_entry_module_names: Vec<String> = {
        // Build path->name mapping and dependency graph
        let mut path_to_name: HashMap<PathBuf, String> = HashMap::new();
        let mut name_to_path: HashMap<String, PathBuf> = HashMap::new();
        let mut deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        for (path, hir_module) in &ctx.native_modules {
            if *path == entry_path {
                continue;
            }
            path_to_name.insert(path.clone(), hir_module.name.clone());
            name_to_path.insert(hir_module.name.clone(), path.clone());

            let mut module_deps = Vec::new();
            for import in &hir_module.imports {
                if let Some(ref resolved) = import.resolved_path {
                    let resolved_path = PathBuf::from(resolved);
                    if resolved_path != entry_path && ctx.native_modules.contains_key(&resolved_path) {
                        module_deps.push(resolved_path);
                    }
                }
            }
            // Also treat ExportAll/ReExport sources as dependencies.
            // If module A does `export * from './B'`, then B must be initialized before A
            // so that B's export globals are set before any consumer of A reads them.
            for export in &hir_module.exports {
                let source = match export {
                    perry_hir::Export::ExportAll { source } => Some(source),
                    perry_hir::Export::ReExport { source, .. } => Some(source),
                    perry_hir::Export::Named { .. } => None,
                };
                if let Some(src) = source {
                    if let Some((resolved_path, _)) = resolve_import(src, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                        if resolved_path != entry_path && ctx.native_modules.contains_key(&resolved_path) {
                            module_deps.push(resolved_path);
                        }
                    }
                }
            }
            deps.insert(path.clone(), module_deps);
        }

        // DFS-based topological sort (handles circular dependencies gracefully)
        // Dependencies are visited before the module itself. Cycles are broken
        // at the back-edge (module already being visited), ensuring the best
        // possible ordering even with circular imports.
        let mut sorted = Vec::new();
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut visiting: HashSet<PathBuf> = HashSet::new(); // cycle detection

        fn dfs_visit(
            path: &PathBuf,
            deps: &HashMap<PathBuf, Vec<PathBuf>>,
            path_to_name: &HashMap<PathBuf, String>,
            visited: &mut HashSet<PathBuf>,
            visiting: &mut HashSet<PathBuf>,
            sorted: &mut Vec<String>,
        ) {
            if visited.contains(path) || visiting.contains(path) {
                return; // already done or cycle back-edge
            }
            visiting.insert(path.clone());

            // Visit dependencies first (so they get initialized before us)
            if let Some(module_deps) = deps.get(path) {
                // Sort deps for deterministic order
                let mut sorted_deps = module_deps.clone();
                sorted_deps.sort();
                for dep in &sorted_deps {
                    dfs_visit(dep, deps, path_to_name, visited, visiting, sorted);
                }
            }

            visiting.remove(path);
            visited.insert(path.clone());
            if let Some(name) = path_to_name.get(path) {
                sorted.push(name.clone());
            }
        }

        // Sort starting nodes for deterministic iteration order
        let mut all_paths: Vec<PathBuf> = path_to_name.keys().cloned().collect();
        all_paths.sort();

        for path in &all_paths {
            dfs_visit(path, &deps, &path_to_name, &mut visited, &mut visiting, &mut sorted);
        }

        sorted
    };

    // Debug: print init order for crash diagnosis
    if let OutputFormat::Text = format {
        eprintln!("\nModule init order ({} modules):", non_entry_module_names.len());
        for (i, name) in non_entry_module_names.iter().enumerate() {
            eprintln!("  [{}] {}", i, name);
        }
        eprintln!();
    }

    // Build a map of all exported enums from all modules (owned data, no borrows)
    // Key: (resolved_path, enum_name) -> Vec<(member_name, EnumValue)>
    let mut exported_enums: BTreeMap<(String, String), Vec<(String, perry_hir::EnumValue)>> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for en in &hir_module.enums {
            if en.is_exported {
                let members: Vec<(String, perry_hir::EnumValue)> = en.members.iter()
                    .map(|m| (m.name.clone(), m.value.clone()))
                    .collect();
                exported_enums.insert((path_str.clone(), en.name.clone()), members);
            }
        }
    }

    // Propagate enum re-exports: when module A has `export * from "./B"`,
    // all enums exported from B should also be accessible via A's path.
    loop {
        let mut new_enum_entries: Vec<((String, String), Vec<(String, perry_hir::EnumValue)>)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                let source_str = match export {
                    perry_hir::Export::ExportAll { source } => Some((source.as_str(), None)),
                    perry_hir::Export::ReExport { source, imported, exported } => Some((source.as_str(), Some((imported.as_str(), exported.as_str())))),
                    _ => None,
                };
                if let Some((source, re_export_names)) = source_str {
                    if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                        let source_path_str = resolved_source.to_string_lossy().to_string();
                        for ((src_path, enum_name), members) in &exported_enums {
                            if src_path == &source_path_str {
                                let (propagate, exported_name) = match re_export_names {
                                    Some((imported, exported)) => (enum_name == imported, exported.to_string()),
                                    None => (true, enum_name.clone()),
                                };
                                if propagate {
                                    let key = (path_str.clone(), exported_name);
                                    if !exported_enums.contains_key(&key) {
                                        new_enum_entries.push((key, members.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if new_enum_entries.is_empty() { break; }
        for (key, members) in new_enum_entries {
            exported_enums.insert(key, members);
        }
    }

    // Fix imported enum references in all modules BEFORE building exported_classes
    // (exported_classes holds references into ctx.native_modules, so we need to do
    // the mutable fixup pass first)
    {
        let mut module_enums: BTreeMap<PathBuf, BTreeMap<String, Vec<(String, perry_hir::EnumValue)>>> = BTreeMap::new();
        for (path, hir_module) in &ctx.native_modules {
            let mut imported_enums_for_module: BTreeMap<String, Vec<(String, perry_hir::EnumValue)>> = BTreeMap::new();
            for import in &hir_module.imports {
                if import.module_kind != perry_hir::ModuleKind::NativeCompiled { continue; }
                let resolved_path = match &import.resolved_path {
                    Some(p) => p.clone(),
                    None => continue,
                };
                for spec in &import.specifiers {
                    let (local_name, exported_name) = match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => (local.clone(), imported.clone()),
                        perry_hir::ImportSpecifier::Default { local } => (local.clone(), local.clone()),
                        perry_hir::ImportSpecifier::Namespace { .. } => continue,
                    };
                    let key = (resolved_path.clone(), exported_name.clone());
                    if let Some(members) = exported_enums.get(&key) {
                        imported_enums_for_module.insert(local_name, members.clone());
                    }
                }
            }
            if !imported_enums_for_module.is_empty() {
                module_enums.insert(path.clone(), imported_enums_for_module);
            }
        }
        for (path, imported_enums_for_module) in &module_enums {
            if let Some(hir_module) = ctx.native_modules.get_mut(path) {
                perry_hir::fix_imported_enums(hir_module, imported_enums_for_module);
            }
        }
    }

    // Build a map of all exported classes from all modules
    // Key: (resolved_path, class_name) -> Class reference
    let mut exported_classes: BTreeMap<(String, String), &perry_hir::Class> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for class in &hir_module.classes {
            if class.is_exported {
                exported_classes.insert((path_str.clone(), class.name.clone()), class);
            }
        }
    }

    // Build a map of all exported functions with their param counts from all modules
    let mut exported_func_param_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    // Build a map of all exported functions with their return types from all modules
    let mut exported_func_return_types: BTreeMap<(String, String), perry_types::Type> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for func in &hir_module.functions {
            if func.is_exported {
                exported_func_param_counts.insert((path_str.clone(), func.name.clone()), func.params.len());
                exported_func_return_types.insert((path_str.clone(), func.name.clone()), func.return_type.clone());
            }
        }
        // Also register exported_functions aliases (e.g., "default" → actual function)
        // This handles `export default funcName` where the export name differs from the function name
        for (export_name, func_id) in &hir_module.exported_functions {
            if let Some(func) = hir_module.functions.iter().find(|f| f.id == *func_id) {
                let key = (path_str.clone(), export_name.clone());
                exported_func_param_counts.entry(key.clone()).or_insert(func.params.len());
                exported_func_return_types.entry(key).or_insert_with(|| func.return_type.clone());
            }
        }
        // Debug: print superstruct exports
        if path_str.contains("superstruct") {
            eprintln!("[DEBUG] superstruct: {} functions ({} exported), {} exported_functions entries",
                hir_module.functions.len(),
                hir_module.functions.iter().filter(|f| f.is_exported).count(),
                hir_module.exported_functions.len());
            for (name, _fid) in &hir_module.exported_functions {
                eprintln!("[DEBUG]   exported_function: {}", name);
            }
        }

        // Also scan init statements for exported closures (arrow functions assigned to const)
        // These are in exported_objects but not in functions, so they need param counts too
        let exported_set: std::collections::HashSet<&String> = hir_module.exported_objects.iter().collect();
        for stmt in &hir_module.init {
            if let perry_hir::ir::Stmt::Let { name, init: Some(expr), .. } = stmt {
                if exported_set.contains(name) {
                    if let perry_hir::ir::Expr::Closure { params, return_type, .. } = expr {
                        exported_func_param_counts.insert((path_str.clone(), name.clone()), params.len());
                        exported_func_return_types.insert((path_str.clone(), name.clone()), return_type.clone());
                    }
                }
            }
        }
    }

    // Build a map of all exports from all modules: module_path -> HashMap<export_name, origin_module_path>
    // This is used for namespace imports (`import * as X from './module'`) to resolve all exports
    let mut all_module_exports: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        let exports = all_module_exports.entry(path_str.clone()).or_insert_with(BTreeMap::new);
        // Exported functions
        for func in &hir_module.functions {
            if func.is_exported {
                exports.insert(func.name.clone(), path_str.clone());
            }
        }
        // Exported objects (export const x = { ... })
        for obj_name in &hir_module.exported_objects {
            exports.insert(obj_name.clone(), path_str.clone());
        }
        // Exported classes
        for class in &hir_module.classes {
            if class.is_exported {
                exports.insert(class.name.clone(), path_str.clone());
            }
        }
        // Exported enums
        for en in &hir_module.enums {
            if en.is_exported {
                exports.insert(en.name.clone(), path_str.clone());
            }
        }
        // Named exports (export { foo, bar as baz })
        for export in &hir_module.exports {
            if let perry_hir::Export::Named { exported, .. } = export {
                exports.insert(exported.clone(), path_str.clone());
            }
            // ReExport is handled in the propagation loop below (avoids borrow issues)
        }
    }

    // Propagate exports through ExportAll and ReExport chains
    loop {
        let mut new_export_entries: Vec<(String, String, String)> = Vec::new(); // (module_path, export_name, origin_path)
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                let current_exports = all_module_exports.get(&path_str);
                                for (name, origin) in source_exports {
                                    let already_exists = current_exports
                                        .map(|e| e.contains_key(name))
                                        .unwrap_or(false);
                                    if !already_exists {
                                        new_export_entries.push((path_str.clone(), name.clone(), origin.clone()));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                if let Some(origin) = source_exports.get(imported) {
                                    let current_exports = all_module_exports.get(&path_str);
                                    let already_correct = current_exports
                                        .and_then(|e| e.get(exported.as_str()))
                                        .map(|v| v == origin)
                                        .unwrap_or(false);
                                    if !already_correct {
                                        new_export_entries.push((path_str.clone(), exported.clone(), origin.clone()));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        // Check if this local was imported from another module
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                            if let Some(origin) = source_exports.get(&imported_name) {
                                                let current_exports = all_module_exports.get(&path_str);
                                                let already_correct = current_exports
                                                    .and_then(|e| e.get(exported.as_str()))
                                                    .map(|v| v == origin)
                                                    .unwrap_or(false);
                                                if !already_correct {
                                                    new_export_entries.push((path_str.clone(), exported.clone(), origin.clone()));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_export_entries.is_empty() { break; }
        for (module_path, name, origin) in new_export_entries {
            all_module_exports.entry(module_path).or_insert_with(BTreeMap::new).insert(name, origin);
        }
    }

    // Also propagate exported_func_param_counts through ExportAll/ReExport/Named chains
    loop {
        let mut new_func_entries: Vec<((String, String), usize)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), &param_count) in &exported_func_param_counts {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), func_name.clone());
                                    if !exported_func_param_counts.contains_key(&key) {
                                        new_func_entries.push((key, param_count));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), &param_count) in &exported_func_param_counts {
                                if src_path == &source_path_str && func_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_func_param_counts.contains_key(&key) {
                                        new_func_entries.push((key, param_count));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(&param_count) = exported_func_param_counts.get(&key_src) {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_func_param_counts.contains_key(&key) {
                                                new_func_entries.push((key, param_count));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_func_entries.is_empty() { break; }
        for (key, param_count) in new_func_entries {
            exported_func_param_counts.insert(key, param_count);
        }
    }

    // Propagate exported_func_return_types through ExportAll/ReExport/Named chains
    loop {
        let mut new_func_entries: Vec<((String, String), perry_types::Type)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), return_type) in &exported_func_return_types {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), func_name.clone());
                                    if !exported_func_return_types.contains_key(&key) {
                                        new_func_entries.push((key, return_type.clone()));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), return_type) in &exported_func_return_types {
                                if src_path == &source_path_str && func_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_func_return_types.contains_key(&key) {
                                        new_func_entries.push((key, return_type.clone()));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(return_type) = exported_func_return_types.get(&key_src) {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_func_return_types.contains_key(&key) {
                                                new_func_entries.push((key, return_type.clone()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_func_entries.is_empty() { break; }
        for (key, return_type) in new_func_entries {
            exported_func_return_types.insert(key, return_type);
        }
    }

    // Propagate class re-exports through ExportAll/ReExport/Named chains
    loop {
        let mut new_entries: Vec<((String, String), &perry_hir::Class)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, class_name), class) in &exported_classes {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), class_name.clone());
                                    if !exported_classes.contains_key(&key) {
                                        new_entries.push((key, *class));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, class_name), class) in &exported_classes {
                                if src_path == &source_path_str && class_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_classes.contains_key(&key) {
                                        new_entries.push((key, *class));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(class) = exported_classes.get(&key_src) {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_classes.contains_key(&key) {
                                                new_entries.push((key, *class));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_entries.is_empty() { break; }
        for (key, class) in new_entries {
            exported_classes.insert(key, class);
        }
    }

    let target = args.target.clone();

    // Pre-compute feature flags (moved out of parallel loop to avoid ctx mutation)
    let compiled_features: Vec<String> = if let Some(ref features_str) = args.features {
        let mut features: Vec<String> = features_str.split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        let is_mobile = matches!(target.as_deref(), Some("ios") | Some("ios-simulator") | Some("android") | Some("watchos") | Some("watchos-simulator") | Some("tvos") | Some("tvos-simulator"));
        if is_mobile {
            features.retain(|f| f != "plugins");
        }
        if features.iter().any(|f| f == "plugins") {
            ctx.needs_plugins = true;
        }
        features
    } else {
        Vec::new()
    };

    // Pre-compute native library FFI functions
    let ffi_functions: Vec<(String, Vec<String>, String)> = ctx.native_libraries.iter()
        .flat_map(|lib| lib.functions.iter().map(|f| {
            (f.name.clone(), f.params.clone(), f.returns.clone())
        }))
        .collect();

    // Pre-compute JS module specifiers
    let js_module_specifiers: Vec<String> = ctx.js_modules.keys().cloned().collect();
    let needs_js_runtime = ctx.needs_js_runtime || args.enable_js_runtime;

    // Compile native modules in parallel using rayon
    use rayon::prelude::*;

    let compile_results: Vec<Result<(PathBuf, Vec<u8>), String>> = ctx.native_modules.par_iter()
        .map(|(path, hir_module)| {
            let mut compiler = perry_codegen::Compiler::new(target.as_deref())
                .map_err(|e| format!("Failed to create compiler: {}", e))?;

            let is_entry = path == &entry_path;
            compiler.set_is_entry_module(is_entry);
            compiler.set_output_type(args.output_type.clone());

            if !compiled_features.is_empty() {
                compiler.set_enabled_features(compiled_features.clone());
            }

            if is_entry {
                for module_name in &non_entry_module_names {
                    compiler.add_native_module_init(module_name.clone());
                }
                if !bundled_extensions.is_empty() {
                    for (ext_path, _plugin_id) in &bundled_extensions {
                        let ext_prefix = compute_module_prefix(
                            &ext_path.to_string_lossy(),
                            &ctx.project_root,
                        );
                        compiler.add_bundled_extension(
                            ext_path.to_string_lossy().to_string(),
                            ext_prefix,
                        );
                    }
                }
            }

            compiler.set_needs_stdlib(ctx.needs_stdlib);
            compiler.set_needs_ui(ctx.needs_ui);
            compiler.set_needs_geisterhand(ctx.needs_geisterhand);
            compiler.set_geisterhand_port(ctx.geisterhand_port);

            if !ffi_functions.is_empty() {
                compiler.set_native_library_functions(ffi_functions.clone());
            }

            if needs_js_runtime {
                compiler.set_needs_js_runtime(true);
                for specifier in &js_module_specifiers {
                    compiler.add_js_module(specifier.clone());
                }
            }

            // Register imported classes from other native modules
            for import in &hir_module.imports {
                if import.module_kind != perry_hir::ModuleKind::NativeCompiled {
                    continue;
                }

                let resolved_path = match &import.resolved_path {
                    Some(p) => p.clone(),
                    None => continue,
                };

                let resolved_path_str = resolved_path.clone();
                let source_module_prefix = compute_module_prefix(&resolved_path_str, &ctx.project_root);

                for spec in &import.specifiers {
                    match spec {
                        perry_hir::ImportSpecifier::Namespace { local } => {
                            compiler.register_namespace_import(local.clone());
                            if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                                for (export_name, origin_path) in exports {
                                    let origin_prefix = compute_module_prefix(origin_path, &ctx.project_root);
                                    let _ = compiler.pre_declare_import_export(export_name, export_name, &origin_prefix);

                                    let key = (origin_path.clone(), export_name.clone());
                                    if let Some(&param_count) = exported_func_param_counts.get(&key) {
                                        compiler.register_imported_func_param_count(export_name.clone(), param_count);
                                        let _ = compiler.pre_declare_import_wrapper(export_name, &origin_prefix, param_count);
                                    }

                                    if let Some(class) = exported_classes.get(&key) {
                                        let _ = compiler.register_imported_class(class, None, &origin_prefix);
                                    }

                                    if let Some(members) = exported_enums.get(&key) {
                                        compiler.register_imported_enum(export_name, members);
                                    }
                                }
                            }
                            continue;
                        }
                        _ => {}
                    }

                    let (local_name, exported_name) = match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => (local.clone(), imported.clone()),
                        perry_hir::ImportSpecifier::Default { local } => (local.clone(), "default".to_string()),
                        perry_hir::ImportSpecifier::Namespace { .. } => unreachable!(),
                    };

                    let key = (resolved_path_str.clone(), exported_name.clone());

                    let effective_prefix = if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                        if let Some(origin_path) = exports.get(&exported_name) {
                            if origin_path != &resolved_path_str {
                                compute_module_prefix(origin_path, &ctx.project_root)
                            } else {
                                source_module_prefix.clone()
                            }
                        } else {
                            source_module_prefix.clone()
                        }
                    } else {
                        source_module_prefix.clone()
                    };

                    if let Some(class) = exported_classes.get(&key) {
                        let _ = compiler.register_imported_class(class, Some(&local_name), &effective_prefix);
                    }

                    if let Some(&param_count) = exported_func_param_counts.get(&key) {
                        compiler.register_imported_func_param_count(exported_name.clone(), param_count);
                        let _ = compiler.pre_declare_import_wrapper(&exported_name, &effective_prefix, param_count);
                        if local_name != exported_name {
                            compiler.register_imported_func_param_count(local_name.clone(), param_count);
                            compiler.register_import_wrapper_alias(&local_name, &exported_name, param_count);
                        }
                    }
                    if let Some(return_type) = exported_func_return_types.get(&key) {
                        compiler.register_imported_func_return_type(local_name.clone(), return_type.clone());
                    }

                    let _ = compiler.pre_declare_import_export(&exported_name, &local_name, &effective_prefix);

                    if let Some(members) = exported_enums.get(&key) {
                        compiler.register_imported_enum(&local_name, members);
                    }
                }
            }

            let module_name = hir_module.name.clone();
            let module_path = path.display().to_string();
            let object_code = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compiler.compile_module(hir_module)
            })) {
                Ok(Ok(code)) => code,
                Ok(Err(e)) => {
                    return Err(format!("Error compiling module '{}' ({}): {}", module_name, module_path, e));
                }
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "unknown panic".to_string()
                    };
                    return Err(format!("PANIC compiling module '{}' ({}): {}", module_name, module_path, msg));
                }
            };

            let obj_name = hir_module.name
                .replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
                .trim_matches('_')
                .to_string();
            let obj_path = PathBuf::from(format!("{}.o", obj_name));

            Ok((obj_path, object_code))
        })
        .collect();

    // Write object files and collect results (sequential — I/O + error reporting)
    let mut failed_modules: Vec<String> = Vec::new();
    for result in compile_results {
        match result {
            Ok((obj_path, object_code)) => {
                fs::write(&obj_path, &object_code)?;
                match format {
                    OutputFormat::Text => println!("Wrote object file: {}", obj_path.display()),
                    OutputFormat::Json => {}
                }
                obj_paths.push(obj_path);
            }
            Err(msg) => {
                eprintln!("{}", msg);
                // Extract module name from error message for failed_modules
                if let Some(name) = msg.split('\'').nth(1) {
                    failed_modules.push(name.to_string());
                }
            }
        }
    }

    // Generate stubs for missing symbols from unresolved imports (npm packages etc.)
    {
        use std::collections::HashSet;
        let mut undefined_syms: HashSet<String> = HashSet::new();
        let mut defined_syms: HashSet<String> = HashSet::new();
        let runtime_lib_path = find_runtime_library(target.as_deref()).ok();
        let stdlib_lib_path = find_stdlib_library(target.as_deref());
        // Check if jsruntime will be used - if so, don't generate stubs for its symbols
        let use_jsruntime = ctx.needs_js_runtime || args.enable_js_runtime;
        let jsruntime_lib_path = if use_jsruntime {
            find_jsruntime_library(target.as_deref())
        } else {
            None
        };
        let mut all_scan_paths: Vec<PathBuf> = obj_paths.clone();
        if let Some(ref p) = runtime_lib_path { all_scan_paths.push(p.clone()); }
        if ctx.needs_stdlib {
            if let Some(ref p) = stdlib_lib_path { all_scan_paths.push(p.clone()); }
        }
        if let Some(ref p) = jsruntime_lib_path { all_scan_paths.push(p.clone()); }
        // Scan UI library for defined symbols so we don't generate stubs for
        // functions that exist in the platform UI library (e.g. screen detection FFI)
        if ctx.needs_ui {
            if let Some(ui_lib) = find_ui_library(target.as_deref()) {
                all_scan_paths.push(ui_lib);
            }
        }
        // Mark native library FFI functions as defined so we don't generate stubs
        // that would shadow the real implementations in the native library .a/.so
        for native_lib in &ctx.native_libraries {
            for func in &native_lib.functions {
                defined_syms.insert(func.name.clone());
            }
        }
        // Platform detection for nm tool and symbol prefix
        let is_ios = matches!(target.as_deref(), Some("ios-simulator") | Some("ios"));
        let is_android = matches!(target.as_deref(), Some("android"));
        let is_linux = matches!(target.as_deref(), Some("linux")) || (!cfg!(target_os = "macos") && !cfg!(target_os = "windows") && target.is_none());
        let is_windows = matches!(target.as_deref(), Some("windows")) || (cfg!(target_os = "windows") && target.is_none());
        // Find the nm tool: for Windows targets use llvm-nm (reads COFF); otherwise system nm
        let nm_cmd = if is_windows {
            find_llvm_tool("llvm-nm")
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "nm".to_string())
        } else {
            "nm".to_string()
        };
        // Symbol prefix depends on object format:
        // Mach-O (native macOS build, no --target): nm adds `_` prefix
        // COFF (Windows targets): no prefix
        // ELF (Linux/Android targets): no prefix
        let is_macho = !is_windows && !is_linux && !is_android && !is_ios && cfg!(target_os = "macos");
        // Scan object files in parallel for symbol resolution
        let scan_results: Vec<(HashSet<String>, HashSet<String>)> = all_scan_paths.par_iter()
            .map(|scan_path| {
                let mut local_undef = HashSet::new();
                let mut local_def = HashSet::new();
                if let Ok(output) = std::process::Command::new(&nm_cmd).arg("-g").arg(scan_path).output() {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let (st, sn) = if parts.len() == 3 { (parts[1], parts[2]) } else { (parts[0], parts[1]) };
                            let cn = if is_macho {
                                sn.strip_prefix('_').unwrap_or(sn)
                            } else {
                                sn
                            };
                            if st == "U" {
                                if cn.starts_with("__export_") || cn.starts_with("__wrapper_") {
                                    local_undef.insert(cn.to_string());
                                } else if !use_jsruntime && (cn == "js_call_function" || cn == "js_load_module" || cn == "js_new_from_handle"
                                    || cn == "js_new_instance" || cn == "js_create_callback" || cn == "js_runtime_init"
                                    || cn == "js_set_property" || cn == "js_get_export" || cn == "js_await_js_promise") {
                                    local_undef.insert(cn.to_string());
                                } else if is_windows && (
                                    cn.starts_with("perry_ui_") || cn.starts_with("perry_system_") ||
                                    cn.starts_with("perry_plugin_") || cn.starts_with("perry_get_")
                                ) {
                                    local_undef.insert(cn.to_string());
                                }
                            } else if matches!(st, "T" | "t" | "D" | "d" | "S" | "s" | "B" | "b") {
                                local_def.insert(cn.to_string());
                            }
                        }
                    }
                }
                (local_undef, local_def)
            })
            .collect();

        // Merge parallel scan results
        for (local_undef, local_def) in scan_results {
            undefined_syms.extend(local_undef);
            defined_syms.extend(local_def);
        }
        let missing: Vec<String> = undefined_syms.difference(&defined_syms).cloned().collect();
        if !missing.is_empty() {
            let (mut md, mut mf, mut mi) = (Vec::new(), Vec::new(), Vec::new());
            for s in &missing {
                if s.starts_with("__export_") {
                    md.push(s.clone());
                } else if s == "js_await_any_promise" {
                    // Identity stub: takes f64, returns it as-is (pass-through for standalone builds)
                    mi.push(s.clone());
                } else {
                    mf.push(s.clone());
                }
            }
            if let OutputFormat::Text = format { eprintln!("  Generating stubs for {} missing symbols ({} data, {} functions, {} identity)", missing.len(), md.len(), mf.len(), mi.len()); for s in &missing { eprintln!("    - {}", s); } }
            let stub_bytes = perry_codegen::generate_stub_object(&md, &mf, &mi, target.as_deref())?;
            let stub_path = PathBuf::from("_perry_stubs.o");
            fs::write(&stub_path, &stub_bytes)?;
            obj_paths.push(stub_path);
        }
    }

    // Generate JS bundle if needed
    let _js_bundle_path = if ctx.needs_js_runtime && !ctx.js_modules.is_empty() {
        let bundle_path = generate_js_bundle(&ctx, Path::new("."))?;
        match format {
            OutputFormat::Text => println!("Generated JS bundle: {}", bundle_path.display()),
            OutputFormat::Json => {}
        }
        Some(bundle_path)
    } else {
        None
    };

    let stem = args
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let is_dylib = args.output_type == "dylib";
    let exe_path = args.output.unwrap_or_else(|| {
        if is_dylib {
            #[cfg(target_os = "macos")]
            { PathBuf::from(format!("{}.dylib", stem)) }
            #[cfg(not(target_os = "macos"))]
            { PathBuf::from(format!("{}.so", stem)) }
        } else if matches!(target.as_deref(), Some("windows"))
            || (target.is_none() && cfg!(target_os = "windows"))
        {
            PathBuf::from(format!("{}.exe", stem))
        } else {
            PathBuf::from(stem)
        }
    });

    if !failed_modules.is_empty() {
        eprintln!("\n{} module(s) failed to compile:", failed_modules.len());
        for m in &failed_modules {
            eprintln!("  - {}", m);
        }
        eprintln!("Continuing with linking despite failed modules...");

        // Generate stub init functions for failed modules so the binary still links
        let stub_init_names: Vec<String> = failed_modules.iter().map(|m| {
            let sanitized = m.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
            format!("_perry_init_{}", sanitized)
        }).collect();
        if !stub_init_names.is_empty() {
            let stub_bytes = perry_codegen::generate_stub_object(&[], &stub_init_names, &[], target.as_deref())?;
            let stub_path = PathBuf::from("_perry_failed_stubs.o");
            fs::write(&stub_path, &stub_bytes)?;
            obj_paths.push(stub_path);
            eprintln!("Generated {} stub init functions for failed modules", stub_init_names.len());
        }
    }

    if args.no_link {
        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            is_dylib,
        });
    }

    match format {
        OutputFormat::Text => {
            if ctx.needs_stdlib {
                println!("Linking (with stdlib)...");
            } else {
                println!("Linking (runtime-only)...");
            }
        }
        OutputFormat::Json => {}
    }

    let is_ios = matches!(target.as_deref(), Some("ios-simulator") | Some("ios"));
    let is_android = matches!(target.as_deref(), Some("android"));
    let is_linux = matches!(target.as_deref(), Some("linux"))
        || (target.is_none() && cfg!(target_os = "linux"));
    let is_windows = matches!(target.as_deref(), Some("windows"))
        || (target.is_none() && cfg!(target_os = "windows"));
    let is_cross_windows = is_windows && !cfg!(target_os = "windows");
    // Note: is_watchos and is_tvos were already defined earlier near jsruntime_lib

    // For dylib output, skip runtime/stdlib linking — symbols resolve from host at dlopen time
    if is_dylib {
        let mut cmd = if is_linux {
            let mut c = Command::new("cc");
            c.arg("-shared");
            c
        } else {
            // macOS — use flat_namespace so plugins can resolve symbols from the host
            let mut c = Command::new("cc");
            c.arg("-dynamiclib")
             .arg("-flat_namespace")
             .arg("-undefined").arg("dynamic_lookup");
            c
        };

        for obj_path in &obj_paths {
            cmd.arg(obj_path);
        }

        cmd.arg("-o").arg(&exe_path);

        let status = cmd.status()?;
        if !status.success() {
            return Err(anyhow!("Linking dylib failed"));
        }

        match format {
            OutputFormat::Text => println!("Wrote shared library: {}", exe_path.display()),
            OutputFormat::Json => {
                println!("{{\"output\": \"{}\"}}", exe_path.display());
            }
        }

        // Clean up intermediate files
        if !args.keep_intermediates {
            for obj_path in &obj_paths {
                let _ = fs::remove_file(obj_path);
            }
        }

        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            is_dylib: true,
        });
    }

    // When geisterhand is enabled, prefer the geisterhand-enabled runtime
    // (has the registry, dispatch queue, and pump functions)
    let runtime_lib = if ctx.needs_geisterhand {
        if let Some(gh_rt) = find_geisterhand_runtime(target.as_deref()) {
            gh_rt
        } else {
            find_runtime_library(target.as_deref())?
        }
    } else {
        find_runtime_library(target.as_deref())?
    };
    let stdlib_lib = find_stdlib_library(target.as_deref());
    let is_watchos = matches!(target.as_deref(), Some("watchos") | Some("watchos-simulator"));
    let is_tvos = matches!(target.as_deref(), Some("tvos") | Some("tvos-simulator"));
    let jsruntime_lib = if !is_ios && !is_android && !is_watchos && !is_tvos && (ctx.needs_js_runtime || args.enable_js_runtime) {
        match find_jsruntime_library(target.as_deref()) {
            Some(lib) => {
                match format {
                    OutputFormat::Text => println!("Using V8 JavaScript runtime for JS module support"),
                    OutputFormat::Json => {}
                }
                Some(lib)
            }
            None => {
                if ctx.needs_js_runtime {
                    return Err(anyhow!(
                        "JavaScript modules found but libperry_jsruntime.a not found. Build it with: cargo build --release -p perry-jsruntime"
                    ));
                }
                None
            }
        }
    } else {
        None
    };

    // For cross-compilation targets, use the appropriate toolchain
    let mut cmd = if is_watchos {
        // watchOS uses swiftc to compile the PerryWatchApp.swift runtime alongside linking
        let sdk = if target.as_deref() == Some("watchos-simulator") { "watchsimulator" } else { "watchos" };
        let swiftc = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--find", "swiftc"]).output()?.stdout
        )?.trim().to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let triple = if target.as_deref() == Some("watchos-simulator") {
            "arm64-apple-watchos10.0-simulator"
        } else {
            "arm64-apple-watchos10.0"
        };

        let swift_runtime = find_watchos_swift_runtime()
            .ok_or_else(|| anyhow!(
                "PerryWatchApp.swift not found. Expected next to perry binary or in source tree."
            ))?;

        // Rename _main to _perry_main_init in the entry object file so it doesn't
        // conflict with the SwiftUI @main entry point in PerryWatchApp.swift.
        // The Swift runtime calls perry_main_init() to initialize the compiled TS code.
        if let Some(entry_obj) = obj_paths.iter().find(|f| f.to_string_lossy().contains("main_ts")) {
            // Use rustup's llvm-objcopy to rename _main to _perry_main_init
            let objcopy = std::env::var("HOME").ok()
                .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-objcopy"))
                .filter(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from("llvm-objcopy"));
            let _ = Command::new(&objcopy)
                .args(["--redefine-sym", "_main=_perry_main_init"])
                .arg(entry_obj)
                .status();
        }

        let mut c = Command::new(swiftc);
        c.arg("-target").arg(triple)
         .arg("-sdk").arg(&sysroot)
         .arg("-parse-as-library")
         .arg(&swift_runtime);
        c
    } else if is_ios {
        let sdk = if target.as_deref() == Some("ios-simulator") { "iphonesimulator" } else { "iphoneos" };
        let clang = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--find", "clang"]).output()?.stdout
        )?.trim().to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let triple = if target.as_deref() == Some("ios-simulator") {
            "arm64-apple-ios17.0-simulator"
        } else {
            "arm64-apple-ios17.0"
        };

        // Discover Xcode developer directory for Swift standard library paths.
        // Swift libs live in the toolchain, not the SDK sysroot, so the linker
        // needs explicit -L flags to resolve auto-linked libs like swiftCore.
        let developer_dir = String::from_utf8(
            Command::new("xcode-select").arg("-p").output()?.stdout
        )?.trim().to_string();

        let mut c = Command::new(clang);
        c.arg("-target").arg(triple)
         .arg("-isysroot").arg(&sysroot)
         // Swift standard library .tbd stubs in the SDK (swiftCore, swift_Concurrency, etc.)
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         // Swift compatibility static archives in the toolchain
         .arg("-L").arg(format!("{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/{}", developer_dir, sdk));
        c
    } else if is_tvos {
        let sdk = if target.as_deref() == Some("tvos-simulator") { "appletvsimulator" } else { "appletvos" };
        let clang = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--find", "clang"]).output()?.stdout
        )?.trim().to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let triple = if target.as_deref() == Some("tvos-simulator") {
            "arm64-apple-tvos17.0-simulator"
        } else {
            "arm64-apple-tvos17.0"
        };

        let developer_dir = String::from_utf8(
            Command::new("xcode-select").arg("-p").output()?.stdout
        )?.trim().to_string();

        let mut c = Command::new(clang);
        c.arg("-target").arg(triple)
         .arg("-isysroot").arg(&sysroot)
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         .arg("-L").arg(format!("{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/{}", developer_dir, sdk));
        c
    } else if is_android {
        // Use Android NDK clang to produce a shared library (.so)
        let ndk_home = std::env::var("ANDROID_NDK_HOME").map_err(|_| {
            anyhow!("ANDROID_NDK_HOME not set. Set it to your NDK path, e.g. $HOME/Library/Android/sdk/ndk/28.0.12433566")
        })?;
        let host_tag = if cfg!(target_os = "macos") { "darwin-x86_64" } else { "linux-x86_64" };
        let clang = format!(
            "{}/toolchains/llvm/prebuilt/{}/bin/aarch64-linux-android24-clang",
            ndk_home, host_tag
        );
        if !PathBuf::from(&clang).exists() {
            return Err(anyhow!("Android NDK clang not found at: {}", clang));
        }
        let mut c = Command::new(clang);
        c.arg("-shared")
         .arg("-fPIC")
         .arg("-target").arg("aarch64-linux-android24")
         .arg("-Wl,-z,max-page-size=16384")
         .arg("-Wl,-z,separate-loadable-segments")
         // Allow unresolved symbols from namespace imports (import * as X).
         // The codegen emits short-name extern refs (__export_X) for namespace
         // imports that may not have a corresponding definition when the module
         // only exports individually-scoped symbols.
         .arg("-Wl,--warn-unresolved-symbols");
        c
    } else if is_linux {
        // Linux target: when running on Linux natively, just use "cc".
        // When cross-compiling from macOS, pass -target for clang.
        let mut c = Command::new("cc");
        #[cfg(not(target_os = "linux"))]
        {
            c.arg("-target").arg("x86_64-unknown-linux-gnu");
        }
        // Allow unresolved symbols — native libs and UI libs may not implement
        // all functions on Linux. Matches /FORCE:UNRESOLVED on Windows.
        c.arg("-Wl,--warn-unresolved-symbols");
        c
    } else if is_windows {
        // Windows target — use MSVC link.exe (native) or lld-link (cross)
        // Check for PERRY_LLD_LINK override to use lld-link instead of MSVC link.exe.
        // lld-link may handle large COFF objects differently than MSVC's linker.
        let linker = if let Ok(lld) = std::env::var("PERRY_LLD_LINK") {
            PathBuf::from(lld)
        } else {
            find_msvc_link_exe().unwrap_or_else(|| {
                if is_cross_windows {
                    eprintln!("Warning: lld-link not found for cross-compilation. Install: rustup component add llvm-tools");
                }
                PathBuf::from("link.exe")
            })
        };
        let mut c = Command::new(linker);
        c.arg("/SUBSYSTEM:WINDOWS")
         .arg("/ENTRY:mainCRTStartup")
         .arg("/NOLOGO")
         // Perry generates large init functions for TS modules (one function
         // per module). Large codebases (100+ modules) can overflow the
         // default 1MB stack. Reserve 8MB.
         .arg("/STACK:67108864");
        // Set up MSVC library search paths if LIB env isn't already configured
        if std::env::var("LIB").is_err() {
            if let Some(lib_paths) = find_msvc_lib_paths() {
                c.env("LIB", lib_paths);
            } else if is_cross_windows {
                eprintln!("Warning: No Windows SDK library paths found. Set PERRY_WINDOWS_SYSROOT to your xwin sysroot.");
            }
        }
        c
    } else {
        Command::new("cc")
    };

    for obj_path in &obj_paths {
        cmd.arg(obj_path);
    }

    // Dead code stripping — safe because compile_init() emits func_addr
    // calls for every class method/getter during vtable registration. These
    // serve as linker roots that keep dynamically-dispatched methods alive.
    if !is_windows {
        if is_android || is_linux {
            cmd.arg("-Wl,--gc-sections");
        } else if is_watchos {
            cmd.arg("-Xlinker").arg("-dead_strip");
        } else {
            cmd.arg("-Wl,-dead_strip");
        }
    }

    // Link libraries - jsruntime bundles V8 + stdlib; runtime provides base FFI symbols.
    // Note: libperry_jsruntime.a omits some runtime symbols (js_register_class_method,
    // js_register_class_getter, etc.) due to Rust DCE on rlib dependencies. We always
    // link libperry_runtime.a as a fallback to fill these gaps. On macOS/Linux/ELF the
    // linker uses first-definition-wins for archives, so no duplicate symbol errors arise.
    // When UI lib is also linked, it bundles its own copy of perry-runtime.
    // For Android (ELF), skip the extra runtime when UI provides it.
    // On Windows (MSVC), always link the runtime — the UI lib's rlib dependency on
    // perry-runtime may not include all symbols (e.g., perry_init_guard_check_and_set).
    // watchOS: swiftc treats duplicate symbols as errors (not warnings like clang),
    // so skip the standalone runtime when the UI lib already bundles it.
    let skip_runtime = (is_android || is_watchos) && ctx.needs_ui && find_ui_library(target.as_deref()).is_some();
    if !skip_runtime {
        if let Some(ref jsruntime) = jsruntime_lib {
            cmd.arg(jsruntime);
            // Also link runtime to supply symbols DCE'd from jsruntime (e.g. js_register_class_method)
            if !is_android && !is_windows {
                cmd.arg(&runtime_lib);
            }
        } else if ctx.needs_stdlib || is_windows {
            // On Windows/MSVC, always try to link stdlib because codegen unconditionally
            // declares all stdlib extern functions, creating import references that MSVC
            // won't dead-strip. On macOS/Linux, the linker ignores unreferenced archives.
            if let Some(ref stdlib) = stdlib_lib {
                cmd.arg(stdlib);
            } else {
                if ctx.needs_stdlib {
                    eprintln!("Warning: stdlib required but {} not found, using runtime-only",
                        if is_windows { "perry_stdlib.lib" } else { "libperry_stdlib.a" });
                }
                cmd.arg(&runtime_lib);
            }
        } else {
            // Runtime-only linking — no stdlib needed
            cmd.arg(&runtime_lib);
        }
    } else if ctx.needs_stdlib {
        // Android + UI: runtime is provided by UI lib, but stdlib must still be linked
        // separately (UI lib does not bundle perry-stdlib).
        if let Some(ref stdlib) = stdlib_lib {
            cmd.arg(stdlib);
        } else {
            eprintln!("Warning: stdlib required but libperry_stdlib.a not found");
        }
    }

    if is_windows {
        cmd.arg(format!("/OUT:{}", exe_path.display()));
        // V8/deno_core needs additional Windows system libraries
        if jsruntime_lib.is_some() {
            cmd.arg("winmm.lib");
            cmd.arg("dbghelp.lib");
            cmd.arg("msvcprt.lib"); // C++ runtime for exception_ptr
        }
    } else {
        cmd.arg("-o")
            .arg(&exe_path)
            .arg("-lc");
    }

    // For plugin hosts, export symbols so dlopen'd plugins can resolve them.
    // Plugins are dylibs loaded via dlopen — they need to resolve:
    //   1. hone_host_api_* (plugin→host calls)
    //   2. js_*/perry_* (Perry runtime used by compiled plugin code)
    // We use -u to prevent dead_strip from removing these, keeping binary size small.
    if ctx.needs_plugins && !is_windows {
        #[cfg(target_os = "macos")]
        {
            // Force-keep all functions from plugin-related native libraries
            for native_lib in &ctx.native_libraries {
                if native_lib.module.contains("plugin") {
                    for func in &native_lib.functions {
                        cmd.arg(format!("-Wl,-u,_{}", func.name));
                    }
                }
            }
            // Force-keep Perry runtime symbols that plugin dylibs reference.
            // These are collected from the Perry runtime's public API.
            // Using -u tells the linker "treat as referenced" so dead_strip keeps them.
            let runtime_syms = [
                "js_array_alloc",
                "js_array_from_f64", "js_array_push_f64",
                "js_bigint_is_zero",
                "js_closure_alloc",
                "js_console_log_spread",
                "js_dynamic_object_get_property",
                "js_dynamic_string_equals",
                "js_gc_register_global_root",
                "js_is_truthy",
                "js_jsvalue_compare", "js_jsvalue_equals",
                "js_nanbox_get_pointer", "js_nanbox_pointer", "js_nanbox_string",
                "js_native_call_method",
                "js_object_alloc_class_with_keys", "js_object_alloc_with_shape",
                "js_register_class_method",
                "js_string_char_code_at", "js_string_from_bytes", "js_string_length",
                "perry_debug_trace_init", "perry_debug_trace_init_done",
                "perry_init_guard_check_and_set",
            ];
            for sym in &runtime_syms {
                cmd.arg(format!("-Wl,-u,_{}", sym));
            }
        }
        #[cfg(target_os = "linux")]
        {
            cmd.arg("-rdynamic");
        }
    }

    if is_watchos {
        // watchOS frameworks (swiftc auto-links Swift stdlib)
        cmd.arg("-framework").arg("SwiftUI")
           .arg("-framework").arg("WatchKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("Security")
           .arg("-lSystem")
           .arg("-lresolv");
    } else if is_ios {
        // iOS frameworks
        cmd.arg("-framework").arg("UIKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreGraphics")
           .arg("-framework").arg("Security")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("SystemConfiguration")
           .arg("-framework").arg("QuartzCore")
           .arg("-framework").arg("AVFAudio") // AVAudioEngine for audio capture
           .arg("-framework").arg("AVFoundation") // Camera capture (AVCaptureSession)
           .arg("-framework").arg("CoreMedia") // CMSampleBuffer
           .arg("-framework").arg("CoreVideo") // CVPixelBuffer
           .arg("-liconv")
           .arg("-lresolv")
           .arg("-lSystem");
    } else if is_tvos {
        // tvOS frameworks (UIKit-based, like iOS)
        cmd.arg("-framework").arg("UIKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreGraphics")
           .arg("-framework").arg("Security")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("SystemConfiguration")
           .arg("-framework").arg("QuartzCore")
           .arg("-framework").arg("AVFoundation")
           .arg("-framework").arg("GameController")
           .arg("-liconv")
           .arg("-lresolv")
           .arg("-lSystem");
    } else if is_android {
        // Android system libraries
        cmd.arg("-Wl,--allow-multiple-definition")
           .arg("-lm")
           .arg("-ldl")
           .arg("-llog");
    } else if is_linux {
        // Linux system libraries (cross-compile target)
        // Allow multiple definitions: perry-jsruntime embeds perry-runtime symbols,
        // and we also link perry-runtime directly for symbols DCE'd from jsruntime.
        // macOS Mach-O uses first-definition-wins natively; ELF linkers need this flag.
        cmd.arg("-Wl,--allow-multiple-definition")
           .arg("-lm")
           .arg("-lpthread")
           .arg("-ldl");

        if ctx.needs_stdlib || jsruntime_lib.is_some() {
            cmd.arg("-lssl")
               .arg("-lcrypto");
        }

        if jsruntime_lib.is_some() {
            cmd.arg("-lstdc++");
        }
    } else if is_windows {
        // Windows system libraries
        cmd.arg("user32.lib")
           .arg("gdi32.lib")
           .arg("gdiplus.lib")
           .arg("msimg32.lib")
           .arg("kernel32.lib")
           .arg("shell32.lib")
           .arg("ole32.lib")
           .arg("comctl32.lib")
           .arg("advapi32.lib")
           .arg("comdlg32.lib")
           .arg("ws2_32.lib");
        // MSVC CRT (dynamic) and additional Windows API libraries needed by the Rust runtime
        cmd.arg("msvcrt.lib")
           .arg("vcruntime.lib")
           .arg("ucrt.lib")
           .arg("bcrypt.lib")
           .arg("ntdll.lib")
           .arg("userenv.lib")
           .arg("oleaut32.lib")
           .arg("propsys.lib")
           .arg("runtimeobject.lib")
           .arg("iphlpapi.lib");
    } else {
        // On macOS, we need additional frameworks for the runtime (sysinfo, etc.) and V8
        #[cfg(target_os = "macos")]
        {
            cmd.arg("-framework").arg("Security")
               .arg("-framework").arg("CoreFoundation")
               .arg("-framework").arg("SystemConfiguration")
               .arg("-liconv")
               .arg("-lresolv");

            if jsruntime_lib.is_some() {
                cmd.arg("-lc++");
            }
        }

        // On Linux, link against system libraries
        #[cfg(target_os = "linux")]
        {
            cmd.arg("-lm")
               .arg("-lpthread")
               .arg("-ldl");

            if ctx.needs_stdlib || jsruntime_lib.is_some() {
                cmd.arg("-lssl")
                   .arg("-lcrypto");
            }

            if jsruntime_lib.is_some() {
                cmd.arg("-lstdc++");
            }
        }
    }

    // Link perry/ui library and platform frameworks if needed
    if ctx.needs_ui {
        // When geisterhand is enabled, prefer the geisterhand-enabled UI lib
        // (it contains widget registration calls that the normal lib doesn't have)
        let ui_lib_option = if ctx.needs_geisterhand {
            find_geisterhand_ui(target.as_deref()).or_else(|| find_ui_library(target.as_deref()))
        } else {
            find_ui_library(target.as_deref())
        };
        if let Some(ui_lib) = ui_lib_option {
            // On Windows, the UI staticlib bundles its own copies of perry-runtime and
            // Rust std. When perry-stdlib is also linked (which bundles the same), the
            // duplicate Rust std CRT init causes a pre-main crash. Fix: extract only the
            // UI-specific objects from the .lib and link them individually.
            let ui_lib = if is_windows {
                strip_duplicate_objects_from_lib(&ui_lib)
                    .unwrap_or(ui_lib)
            } else {
                ui_lib
            };
            cmd.arg(&ui_lib);

            if is_watchos {
                // SwiftUI/WatchKit already linked above
            } else if is_ios || is_tvos {
                // UIKit already linked above
            } else if is_android {
                // Allow multiple definitions from perry-runtime in both UI lib and native libs
                cmd.arg("-Wl,--allow-multiple-definition");
            } else if is_linux {
                // Allow multiple definitions from perry-runtime in both stdlib and UI lib
                cmd.arg("-Wl,--allow-multiple-definition");
                // GTK4 libraries via pkg-config
                if let Ok(output) = Command::new("pkg-config").args(["--libs", "gtk4"]).output() {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                    }
                } else {
                    // Fallback: link GTK4 libraries directly
                    cmd.arg("-lgtk-4")
                       .arg("-lgobject-2.0")
                       .arg("-lglib-2.0")
                       .arg("-lgio-2.0");
                }
                // PulseAudio for audio capture (only needed with UI)
                cmd.arg("-lpulse-simple")
                   .arg("-lpulse");
            } else if is_windows {
                // Win32 system libs already linked above
            } else {
                #[cfg(target_os = "macos")]
                {
                    cmd.arg("-framework").arg("AppKit");
                    cmd.arg("-framework").arg("QuartzCore"); // CAGradientLayer, CALayer
                    cmd.arg("-framework").arg("AVFoundation"); // AVAudioEngine for audio capture
                }
            }

            match format {
                OutputFormat::Text => println!("Linking perry/ui (native UI) from {}", ui_lib.display()),
                OutputFormat::Json => {}
            }
        } else {
            let (lib_name, build_cmd) = if is_watchos {
                ("libperry_ui_watchos.a", "cargo build --release -p perry-ui-watchos --target aarch64-apple-watchos")
            } else if is_tvos {
                ("libperry_ui_tvos.a", "cargo build --release -p perry-ui-tvos --target aarch64-apple-tvos")
            } else if is_ios {
                ("libperry_ui_ios.a", "cargo build --release -p perry-ui-ios --target aarch64-apple-ios-sim")
            } else if is_android {
                ("libperry_ui_android.a", "cargo build --release -p perry-ui-android --target aarch64-linux-android")
            } else if is_linux {
                ("libperry_ui_gtk4.a", "cargo build --release -p perry-ui-gtk4 --target x86_64-unknown-linux-gnu")
            } else if is_windows {
                ("perry_ui_windows.lib", "cargo build --release -p perry-ui-windows --target x86_64-pc-windows-msvc")
            } else {
                ("libperry_ui_macos.a", "cargo build --release -p perry-ui-macos")
            };
            return Err(anyhow!(
                "perry/ui imported but {} not found. Build with: {}", lib_name, build_cmd
            ));
        }
    }

    // Link geisterhand libraries if enabled
    if ctx.needs_geisterhand {
        // Auto-build geisterhand libraries if any are missing
        let gh_missing = find_geisterhand_library(target.as_deref()).is_none()
            || find_geisterhand_runtime(target.as_deref()).is_none()
            || (ctx.needs_ui && find_geisterhand_ui(target.as_deref()).is_none());
        if gh_missing {
            build_geisterhand_libs(target.as_deref(), format)?;
        }

        if let Some(gh_lib) = find_geisterhand_library(target.as_deref()) {
            cmd.arg(&gh_lib);
            // Link geisterhand-enabled runtime (has the registry + pump functions)
            if let Some(gh_runtime) = find_geisterhand_runtime(target.as_deref()) {
                cmd.arg(&gh_runtime);
                // ELF linkers need --allow-multiple-definition; macOS Mach-O uses first-wins natively
                if is_linux || is_android {
                    cmd.arg("-Wl,--allow-multiple-definition");
                }
            }
            // On Windows, re-link the stdlib after geisterhand to resolve
            // forward references to geisterhand registry functions.
            // lld-link scans archives left-to-right once, so the stdlib
            // must appear after the geisterhand lib that references it.
            // On Windows, force-include geisterhand registry symbols from stdlib.
            // lld-link scans archives left-to-right once, so the stdlib's
            // geisterhand objects are skipped on first scan (no references yet).
            // /INCLUDE forces the linker to pull in the specific symbols.
            if is_windows {
                cmd.arg("/INCLUDE:perry_geisterhand_queue_action");
                cmd.arg("/INCLUDE:perry_geisterhand_queue_action1");
                cmd.arg("/INCLUDE:perry_geisterhand_queue_state_set");
                cmd.arg("/INCLUDE:perry_geisterhand_request_screenshot");
                cmd.arg("/INCLUDE:perry_geisterhand_register");
                cmd.arg("/INCLUDE:perry_geisterhand_pump");
                cmd.arg("/INCLUDE:perry_geisterhand_start");
                cmd.arg("/INCLUDE:perry_geisterhand_free_string");
                cmd.arg("/INCLUDE:perry_geisterhand_get_closure");
                cmd.arg("/INCLUDE:perry_geisterhand_get_registry_json");
                // Allow duplicate symbols from re-linked stdlib objects
                cmd.arg("/FORCE:MULTIPLE");
            }
            match format {
                OutputFormat::Text => println!("Linking geisterhand (in-process fuzzer)"),
                OutputFormat::Json => {}
            }
        } else {
            return Err(anyhow!(
                "Failed to build geisterhand libraries. Check that Perry source crates are available."
            ));
        }
    }

    // Build and link external native libraries from perry.nativeLibrary manifests
    for native_lib in &ctx.native_libraries {
        if let Some(ref target_config) = native_lib.target_config {
            match format {
                OutputFormat::Text => println!("Building native library: {} ...", native_lib.module),
                OutputFormat::Json => {}
            }

            // Build the Rust crate
            let cargo_toml = target_config.crate_path.join("Cargo.toml");
            if cargo_toml.exists() {
                let mut cargo_cmd = Command::new("cargo");
                cargo_cmd.arg("build").arg("--release")
                    .arg("--manifest-path").arg(&cargo_toml);

                if let Some(triple) = rust_target_triple(target.as_deref()) {
                    cargo_cmd.arg("--target").arg(triple);
                }

                // For Android, ensure 16 KB page size alignment (required by Google Play)
                if is_android {
                    cargo_cmd.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
                        "-C link-arg=-Wl,-z,max-page-size=16384");
                }

                let cargo_status = cargo_cmd.status()?;
                if !cargo_status.success() {
                    return Err(anyhow!(
                        "Failed to build native library crate for {}: {}",
                        native_lib.module,
                        target_config.crate_path.display()
                    ));
                }
            }

            // Find and link the static library
            let lib_name = &target_config.lib_name;
            if !lib_name.is_empty() {
                // Search in the crate's target directory first, then standard paths
                let mut lib_path = None;
                let crate_target_dir = target_config.crate_path.join("target");
                if let Some(triple) = rust_target_triple(target.as_deref()) {
                    let candidate = crate_target_dir.join(triple).join("release").join(lib_name);
                    if candidate.exists() {
                        lib_path = Some(candidate);
                    }
                } else {
                    let candidate = crate_target_dir.join("release").join(lib_name);
                    if candidate.exists() {
                        lib_path = Some(candidate);
                    }
                }

                if let Some(lib) = lib_path {
                    // For shared libraries (.so) on Android, use -L/-l so the linker
                    // records just the soname (not the full build path) in DT_NEEDED.
                    if is_android && lib_name.ends_with(".so") {
                        if let Some(dir) = lib.parent() {
                            cmd.arg(format!("-L{}", dir.display()));
                        }
                        // Strip "lib" prefix and ".so" suffix for -l flag
                        let stem = lib_name.strip_prefix("lib").unwrap_or(lib_name);
                        let stem = stem.strip_suffix(".so").unwrap_or(stem);
                        cmd.arg(format!("-l{}", stem));
                    } else {
                        // When building a plugin host on macOS, force-load plugin-related native
                        // libraries so their symbols are available for dlopen'd plugin dylibs.
                        let force_load = cfg!(target_os = "macos")
                            && ctx.needs_plugins
                            && native_lib.module.contains("plugin");
                        if force_load {
                            cmd.arg(format!("-Wl,-force_load,{}", lib.display()));
                        } else if is_windows && lib.extension().map_or(false, |e| e == "lib") {
                            // On Windows, native libs may be staticlibs that bundle
                            // std/alloc/core, causing duplicate CRT init. Dedup them
                            // the same way we dedup the UI lib.
                            let deduped = strip_duplicate_objects_from_lib(&lib)
                                .unwrap_or_else(|_| lib.clone());
                            cmd.arg(&deduped);
                        } else {
                            cmd.arg(&lib);
                        }
                    }
                    match format {
                        OutputFormat::Text => println!("Linking native library: {}", lib.display()),
                        OutputFormat::Json => {}
                    }
                } else {
                    return Err(anyhow!(
                        "Native library {} not found after building {} crate",
                        lib_name, native_lib.module
                    ));
                }
            }

            // Add platform frameworks
            for framework in &target_config.frameworks {
                cmd.arg("-framework").arg(framework);
            }

            // Add platform libraries
            for lib in &target_config.libs {
                if is_windows {
                    cmd.arg(format!("{}.lib", lib));
                } else {
                    cmd.arg(format!("-l{}", lib));
                }
            }

            // Add pkg-config libraries
            for pkg in &target_config.pkg_config {
                if let Ok(output) = Command::new("pkg-config").args(["--libs", pkg]).output() {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                    }
                }
            }
        }
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(anyhow!("Linking failed"));
    }

    // For Android, copy companion shared libraries (.so) next to the output binary
    // so that perry-builder can pick them up and include them in the APK/AAB.
    if is_android {
        if let Some(output_dir) = exe_path.parent() {
            for native_lib in &ctx.native_libraries {
                if let Some(ref target_config) = native_lib.target_config {
                    let lib_name = &target_config.lib_name;
                    if lib_name.ends_with(".so") {
                        let crate_target_dir = target_config.crate_path.join("target");
                        let candidate = if let Some(triple) = rust_target_triple(target.as_deref()) {
                            crate_target_dir.join(triple).join("release").join(lib_name)
                        } else {
                            crate_target_dir.join("release").join(lib_name)
                        };
                        if candidate.exists() {
                            let dest = output_dir.join(lib_name);
                            if let Err(e) = fs::copy(&candidate, &dest) {
                                eprintln!("Warning: failed to copy companion library {}: {}", lib_name, e);
                            } else {
                                match format {
                                    OutputFormat::Text => println!("Copied companion library: {}", lib_name),
                                    OutputFormat::Json => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Track iOS bundle info for CompileResult
    let mut result_bundle_id: Option<String> = None;
    let mut result_app_dir: Option<PathBuf> = None;

    // For iOS targets, create a .app bundle
    if is_ios {
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        // Check perry.toml, then package.json for a custom bundleId, fall back to com.perry.{name}
        // Search relative to the source file, walking up directories
        let bundle_id = (|| -> Option<String> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                // Check perry.toml first: [ios].bundle_id, then top-level bundle_id
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    if let Ok(data) = fs::read_to_string(&toml_path) {
                        if let Ok(doc) = data.parse::<toml::Table>() {
                            let toml_bid = doc.get("ios")
                                .and_then(|i| i.get("bundle_id"))
                                .or_else(|| doc.get("app").and_then(|a| a.get("bundle_id")))
                                .or_else(|| doc.get("project").and_then(|p| p.get("bundle_id")))
                                .or_else(|| doc.get("bundle_id"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            if toml_bid.is_some() {
                                return toml_bid;
                            }
                        }
                    }
                }
                // Then check package.json
                let pkg = dir.join("package.json");
                if pkg.exists() {
                    let data = fs::read_to_string(pkg).ok()?;
                    let idx = data.find("\"bundleId\"")?;
                    let colon = data[idx..].find(':')?;
                    let q1 = data[idx + colon..].find('"')? + idx + colon + 1;
                    let q2 = data[q1..].find('"')? + q1;
                    return Some(data[q1..q2].to_string());
                }
            }
            None
        })().unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        // Check perry.toml for iOS-specific settings (e.g. encryption_exempt)
        let encryption_exempt_plist = (|| -> Option<String> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    let data = fs::read_to_string(toml_path).ok()?;
                    let doc: toml::Table = data.parse().ok()?;
                    let ios = doc.get("ios")?.as_table()?;
                    let exempt = ios.get("encryption_exempt")?.as_bool()?;
                    if exempt {
                        return Some(
                            "    <key>ITSAppUsesNonExemptEncryption</key>\n    <false/>".into()
                        );
                    } else {
                        return Some(
                            "    <key>ITSAppUsesNonExemptEncryption</key>\n    <true/>".into()
                        );
                    }
                }
            }
            None
        })().unwrap_or_default();

        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UILaunchStoryboardName</key>
    <string>LaunchScreen</string>
    <key>UIRequiresFullScreen</key>
    <true/>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
    </array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UIApplicationSupportsMultipleScenes</key>
        <false/>
        <key>UISceneConfigurations</key>
        <dict>
            <key>UIWindowSceneSessionRoleApplication</key>
            <array>
                <dict>
                    <key>UISceneConfigurationName</key>
                    <string>Default Configuration</string>
                    <key>UISceneDelegateClassName</key>
                    <string>PerrySceneDelegate</string>
                </dict>
            </array>
        </dict>
    </dict>
</dict>
</plist>"#,
        );

        // Append usage descriptions for camera and microphone
        let usage_descriptions = concat!(
            "    <key>NSCameraUsageDescription</key>\n",
            "    <string>This app uses the camera to identify colors.</string>\n",
            "    <key>NSMicrophoneUsageDescription</key>\n",
            "    <string>This app uses the microphone to measure sound levels.</string>",
        );
        let info_plist = info_plist.replace(
            "</dict>\n</plist>",
            &format!("{}\n</dict>\n</plist>", usage_descriptions),
        );

        // Append ITSAppUsesNonExemptEncryption if configured in perry.toml
        let info_plist = if !encryption_exempt_plist.is_empty() {
            info_plist.replace(
                "</dict>\n</plist>",
                &format!("{}\n</dict>\n</plist>", encryption_exempt_plist),
            )
        } else {
            info_plist
        };

        // Append custom Info.plist entries from [ios.info_plist] in perry.toml
        let custom_plist_entries = (|| -> Option<String> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    let data = fs::read_to_string(&toml_path).ok()?;
                    let doc: toml::Table = data.parse().ok()?;
                    let ios = doc.get("ios")?.as_table()?;
                    let info_plist_table = ios.get("info_plist")?.as_table()?;
                    let mut entries = String::new();
                    for (key, value) in info_plist_table {
                        if let Some(s) = value.as_str() {
                            entries.push_str(&format!(
                                "    <key>{}</key>\n    <string>{}</string>\n",
                                key, s
                            ));
                        } else if let Some(b) = value.as_bool() {
                            entries.push_str(&format!(
                                "    <key>{}</key>\n    <{}/>",
                                key, if b { "true" } else { "false" }
                            ));
                        }
                    }
                    if !entries.is_empty() {
                        return Some(entries);
                    }
                }
            }
            None
        })().unwrap_or_default();
        let info_plist = if !custom_plist_entries.is_empty() {
            info_plist.replace(
                "</dict>\n</plist>",
                &format!("{}</dict>\n</plist>", custom_plist_entries),
            )
        } else {
            info_plist
        };

        fs::write(app_dir.join("Info.plist"), info_plist)?;

        // Read splash screen config from package.json perry.splash section
        let splash_config: Option<(Option<std::path::PathBuf>, String, Option<std::path::PathBuf>)> = (|| -> Option<(Option<std::path::PathBuf>, String, Option<std::path::PathBuf>)> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let pkg = dir.join("package.json");
                if pkg.exists() {
                    let data = fs::read_to_string(&pkg).ok()?;
                    let pkg_val: serde_json::Value = serde_json::from_str(&data).ok()?;
                    let splash = pkg_val.get("perry")?.get("splash")?;

                    // Check for custom storyboard override first
                    if let Some(sb_path) = splash.get("ios").and_then(|i| i.get("storyboard")).and_then(|v| v.as_str()) {
                        let abs = dir.join(sb_path);
                        if abs.exists() {
                            return Some((None, "#FFFFFF".into(), Some(abs)));
                        }
                    }

                    // Resolve image: splash.ios.image -> splash.image
                    let image_path = splash.get("ios").and_then(|i| i.get("image")).and_then(|v| v.as_str())
                        .or_else(|| splash.get("image").and_then(|v| v.as_str()))
                        .map(|p| dir.join(p))
                        .filter(|p| p.exists());

                    // Resolve background: splash.ios.background -> splash.background -> "#FFFFFF"
                    let background = splash.get("ios").and_then(|i| i.get("background")).and_then(|v| v.as_str())
                        .or_else(|| splash.get("background").and_then(|v| v.as_str()))
                        .unwrap_or("#FFFFFF")
                        .to_string();

                    if image_path.is_some() || background != "#FFFFFF" {
                        return Some((image_path, background, None));
                    }
                    return None;
                }
            }
            None
        })();

        // Write a compiled LaunchScreen storyboard — with splash image if configured,
        // otherwise a minimal blank storyboard so iPadOS treats the app as native iPad.
        let launch_sb_xml = if let Some((ref image_path, ref bg_hex, ref custom_sb)) = splash_config {
            if let Some(custom) = custom_sb {
                // Custom storyboard: copy as-is
                fs::read_to_string(custom).unwrap_or_default()
            } else {
                // Copy splash image into bundle
                if let Some(img) = image_path {
                    let _ = fs::copy(img, app_dir.join("splash_image.png"));
                }

                // Parse hex color to RGB floats
                let hex = bg_hex.trim_start_matches('#');
                let (r, g, b) = if hex.len() == 6 {
                    let rv = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255) as f64 / 255.0;
                    let gv = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255) as f64 / 255.0;
                    let bv = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255) as f64 / 255.0;
                    (rv, gv, bv)
                } else {
                    (1.0, 1.0, 1.0)
                };

                let image_views = if image_path.is_some() {
                    format!(r#"
                        <subviews>
                            <imageView clipsSubviews="YES" userInteractionEnabled="NO" contentMode="scaleAspectFit" image="splash_image" translatesAutoresizingMaskIntoConstraints="NO" id="img-splash-1">
                                <rect key="frame" x="132.5" y="362" width="128" height="128"/>
                                <constraints>
                                    <constraint firstAttribute="width" constant="128" id="img-w-1"/>
                                    <constraint firstAttribute="height" constant="128" id="img-h-1"/>
                                </constraints>
                            </imageView>
                        </subviews>
                        <constraints>
                            <constraint firstItem="img-splash-1" firstAttribute="centerX" secondItem="Ze5-6b-2t3" secondAttribute="centerX" id="cx-1"/>
                            <constraint firstItem="img-splash-1" firstAttribute="centerY" secondItem="Ze5-6b-2t3" secondAttribute="centerY" id="cy-1"/>
                        </constraints>"#)
                } else {
                    String::new()
                };

                let resources = if image_path.is_some() {
                    r#"
    <resources>
        <image name="splash_image" width="128" height="128"/>
    </resources>"#.to_string()
                } else {
                    String::new()
                };

                format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" toolsVersion="21701" targetRuntime="iOS.CocoaTouch" propertyAccessControl="none" useAutolayout="YES" launchScreen="YES" useTraitCollections="YES" useSafeAreas="YES" colorMatched="YES" initialViewController="01J-lp-oVM">
    <scenes>
        <scene sceneID="EHf-IW-A2E">
            <objects>
                <viewController id="01J-lp-oVM" sceneMemberID="viewController">
                    <view key="view" contentMode="scaleToFill" id="Ze5-6b-2t3">
                        <rect key="frame" x="0.0" y="0.0" width="393" height="852"/>
                        <autoresizingMask key="autoresizingMask" widthSizable="YES" heightSizable="YES"/>
                        <color key="backgroundColor" red="{r}" green="{g}" blue="{b}" alpha="1" colorSpace="custom" customColorSpace="sRGB"/>{image_views}
                    </view>
                </viewController>
                <placeholder placeholderIdentifier="IBFirstResponder" id="iYj-Kq-Ea1" userLabel="First Responder" sceneMemberID="firstResponder"/>
            </objects>
            <point key="canvasLocation" x="0" y="0"/>
        </scene>
    </scenes>{resources}
</document>"#)
            }
        } else {
            // No splash config — minimal blank storyboard for iPadOS compatibility
            r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" toolsVersion="21701" targetRuntime="iOS.CocoaTouch" propertyAccessControl="none" useAutolayout="YES" launchScreen="YES" useTraitCollections="YES" useSafeAreas="YES" colorMatched="YES" initialViewController="01J-lp-oVM">
    <scenes>
        <scene sceneID="EHf-IW-A2E">
            <objects>
                <viewController id="01J-lp-oVM" sceneMemberID="viewController">
                    <view key="view" contentMode="scaleToFill" id="Ze5-6b-2t3">
                        <rect key="frame" x="0.0" y="0.0" width="393" height="852"/>
                        <autoresizingMask key="autoresizingMask" widthSizable="YES" heightSizable="YES"/>
                        <color key="backgroundColor" systemColor="systemBackgroundColor"/>
                    </view>
                </viewController>
                <placeholder placeholderIdentifier="IBFirstResponder" id="iYj-Kq-Ea1" userLabel="First Responder" sceneMemberID="firstResponder"/>
            </objects>
            <point key="canvasLocation" x="0" y="0"/>
        </scene>
    </scenes>
</document>"#.to_string()
        };

        let sb_source = app_dir.join("_LaunchScreen.storyboard");
        fs::write(&sb_source, launch_sb_xml)?;
        let storyboardc = app_dir.join("Base.lproj").join("LaunchScreen.storyboardc");
        let _ = fs::create_dir_all(app_dir.join("Base.lproj"));
        let _ = fs::remove_dir_all(&storyboardc);
        let ibt_result = std::process::Command::new("ibtool")
            .arg("--compile")
            .arg(storyboardc.as_os_str())
            .arg(sb_source.as_os_str())
            .output();
        let _ = fs::remove_file(&sb_source);
        if ibt_result.is_err() || !ibt_result.as_ref().unwrap().status.success() {
            eprintln!("Warning: ibtool failed to compile LaunchScreen.storyboard");
        }

        // Bundle resource files: scan source for ImageFile('...') calls and copy referenced files
        // Also copy any directories named 'logo', 'assets', 'resources', 'images' from the project root
        let source_dir = args.input.canonicalize().ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        if let Some(src_dir) = &source_dir {
            // Walk up to find project root (where package.json is)
            let mut project_root = src_dir.clone();
            for _ in 0..5 {
                if project_root.join("package.json").exists() { break; }
                if let Some(parent) = project_root.parent() {
                    project_root = parent.to_path_buf();
                } else { break; }
            }
            // Copy common resource directories into the bundle
            for dir_name in &["logo", "assets", "resources", "images"] {
                let resource_dir = project_root.join(dir_name);
                if resource_dir.is_dir() {
                    let dest = app_dir.join(dir_name);
                    fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                        fs::create_dir_all(dst)?;
                        for entry in fs::read_dir(src)? {
                            let entry = entry?;
                            let ty = entry.file_type()?;
                            let dest_path = dst.join(entry.file_name());
                            if ty.is_dir() {
                                copy_dir_recursive(&entry.path(), &dest_path)?;
                            } else {
                                fs::copy(entry.path(), &dest_path)?;
                            }
                        }
                        Ok(())
                    }
                    let _ = copy_dir_recursive(&resource_dir, &dest);
                }
            }
        }

        // --- i18n: generate .lproj bundles for iOS/macOS ---
        if let (Some(ref table), Some(ref config)) = (&i18n_table, &i18n_config) {
            if !table.keys.is_empty() {
                for (locale_idx, locale) in config.locales.iter().enumerate() {
                    let lproj_dir = app_dir.join(format!("{}.lproj", locale));
                    let _ = fs::create_dir_all(&lproj_dir);
                    let mut strings_content = String::new();
                    for (key_idx, key) in table.keys.iter().enumerate() {
                        let flat_idx = locale_idx * table.keys.len() + key_idx;
                        let value = table.translations.get(flat_idx).cloned().unwrap_or_else(|| key.clone());
                        // Escape for .strings format
                        let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
                        let escaped_val = value.replace('\\', "\\\\").replace('"', "\\\"");
                        strings_content.push_str(&format!("\"{}\" = \"{}\";\n", escaped_key, escaped_val));
                    }
                    let _ = fs::write(lproj_dir.join("Localizable.strings"), &strings_content);
                }
                match format {
                    OutputFormat::Text => println!("  Generated {}.lproj bundles for {} locale(s)",
                        config.locales.join(", "), config.locales.len()),
                    OutputFormat::Json => {}
                }
            }
        }

        match format {
            OutputFormat::Text => {
                println!("Wrote iOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on iOS Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else if is_watchos {
        // Create watchOS .app bundle
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        let bundle_id = lookup_bundle_id_from_toml(&args.input, "watchos")
            .or_else(|| lookup_bundle_id_from_toml(&args.input, "app"))
            .unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>10.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>4</integer>
    </array>
    <key>WKApplication</key>
    <true/>
    <key>WKWatchOnly</key>
    <true/>
</dict>
</plist>"#
        );
        fs::write(app_dir.join("Info.plist"), info_plist)?;

        match format {
            OutputFormat::Text => {
                println!("Wrote watchOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on Apple Watch Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else if is_tvos {
        // Create tvOS .app bundle
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        let bundle_id = lookup_bundle_id_from_toml(&args.input, "tvos")
            .or_else(|| lookup_bundle_id_from_toml(&args.input, "app"))
            .unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>3</integer>
    </array>
    <key>UILaunchStoryboardName</key>
    <string>LaunchScreen</string>
    <key>UIRequiresFullScreen</key>
    <true/>
</dict>
</plist>"#
        );
        fs::write(app_dir.join("Info.plist"), info_plist)?;

        match format {
            OutputFormat::Text => {
                println!("Wrote tvOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on Apple TV Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else {
        match format {
            OutputFormat::Text => println!("Wrote executable: {}", exe_path.display()),
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": exe_path.to_string_lossy(),
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    }

    // --- i18n: generate Android values-xx/ resources ---
    if is_android {
        if let (Some(ref table), Some(ref config)) = (&i18n_table, &i18n_config) {
            if !table.keys.is_empty() {
                let output_dir = exe_path.parent().unwrap_or(Path::new("."));
                let res_dir = output_dir.join("res");
                for (locale_idx, locale) in config.locales.iter().enumerate() {
                    let values_dir = if locale_idx == 0 {
                        res_dir.join("values") // default locale
                    } else {
                        res_dir.join(format!("values-{}", locale))
                    };
                    let _ = fs::create_dir_all(&values_dir);
                    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n");
                    for (key_idx, key) in table.keys.iter().enumerate() {
                        let flat_idx = locale_idx * table.keys.len() + key_idx;
                        let value = table.translations.get(flat_idx).cloned().unwrap_or_else(|| key.clone());
                        // Sanitize key for Android resource name (alphanumeric + underscore)
                        let res_name: String = key.chars().map(|c| {
                            if c.is_alphanumeric() || c == '_' { c } else { '_' }
                        }).collect();
                        // Escape XML special chars
                        let escaped = value.replace('&', "&amp;").replace('<', "&lt;")
                            .replace('>', "&gt;").replace('"', "&quot;").replace('\'', "\\'");
                        xml.push_str(&format!("    <string name=\"{}\">{}</string>\n", res_name, escaped));
                    }
                    xml.push_str("</resources>\n");
                    let _ = fs::write(values_dir.join("strings.xml"), &xml);
                }
                match format {
                    OutputFormat::Text => println!("  Generated res/values-*/strings.xml for {} locale(s)", config.locales.len()),
                    OutputFormat::Json => {}
                }
            }
        }
    }

    // Strip debug symbols from the final binary (reduces size significantly)
    // Skip for iOS/Android cross-compilation — host strip can't handle foreign architectures
    if !is_dylib && !is_ios && !is_tvos && target.as_deref() != Some("android") {
        if ctx.needs_plugins {
            // When plugins are enabled, use strip -x to keep exported symbols
            // (dlopen'd plugins need to resolve hone_host_api_* from the main executable)
            let _ = std::process::Command::new("strip").arg("-x").arg(&exe_path).status();
        } else {
            let _ = std::process::Command::new("strip").arg(&exe_path).status();
        }
    }

    // Print binary size
    if let OutputFormat::Text = format {
        if let Ok(meta) = fs::metadata(&exe_path) {
            let size_mb = meta.len() as f64 / 1_048_576.0;
            println!("Binary size: {:.1}MB", size_mb);
        }
    }

    if !args.keep_intermediates {
        for obj_path in &obj_paths {
            let _ = fs::remove_file(obj_path);
        }
    }

    let final_output_path = result_app_dir.unwrap_or(exe_path);

    Ok(CompileResult {
        output_path: final_output_path,
        target: target.unwrap_or_else(|| "native".to_string()),
        bundle_id: result_bundle_id,
        is_dylib,
    })
}
