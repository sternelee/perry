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

    /// Target platform: ios-simulator, ios, android (default: native host)
    #[arg(long)]
    pub target: Option<String>,

    /// Output type: executable (default) or dylib (shared library plugin)
    #[arg(long, default_value = "executable")]
    pub output_type: String,

    /// Bundle TypeScript extensions from directory.
    /// Scans subdirectories for package.json with openclaw.extensions entries
    /// and compiles them into the binary as static plugins.
    #[arg(long)]
    pub bundle_extensions: Option<PathBuf>,
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
#[derive(Debug)]
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
        Some("ios-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") => Some("aarch64-apple-ios"),
        Some("android") => Some("aarch64-linux-android"),
        Some("linux") => Some("x86_64-unknown-linux-gnu"),
        Some("windows") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
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
    None
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
    None
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
    } else {
        // Host build: search host directories
        candidates.push(PathBuf::from(format!("target/release/{}", name)));
        candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(name));
            }
        }
        candidates.push(PathBuf::from(format!("/usr/local/lib/{}", name)));
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
    find_library("libperry_stdlib.a", target)
}

/// Find the V8 jsruntime library for linking (optional - only needed for JS module support)
fn find_jsruntime_library(target: Option<&str>) -> Option<PathBuf> {
    find_library("libperry_jsruntime.a", target)
}

/// Find the UI library for linking (optional - only needed when perry/ui is imported)
fn find_ui_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("ios-simulator") | Some("ios") => "libperry_ui_ios.a",
        Some("android") => "libperry_ui_android.a",
        Some("linux") => "libperry_ui_gtk4.a",
        Some("windows") => "perry_ui_windows.lib",
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

    // Find node_modules starting from importer, then project root
    let search_paths = [importer_path.parent(), Some(project_root)];

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
        || ctx.compile_package_dirs.values().any(|dir| canonical.starts_with(dir));
    let should_use_js_runtime = (is_js_file(&canonical) && !is_in_compiled_pkg)
        || is_declaration_file(&canonical)
        || is_json
        || (enable_js_runtime && is_in_node_modules && !is_perry_native && !is_in_compiled_pkg);

    // Skip JSON files — they're data, not code (imported via `with { type: "json" }`)
    if is_json {
        return Ok(());
    }

    if should_use_js_runtime {
        if !enable_js_runtime {
            return Err(anyhow!(
                "File '{}' requires --enable-js-runtime flag",
                canonical.display()
            ));
        }

        // Skip declaration files - they're just type information
        if is_declaration_file(&canonical) {
            return Ok(());
        }

        // Perry native extension packages (ioredis, ethers, ws, mysql2, dotenv) are handled
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
    let (mut hir_module, new_next_class_id) = perry_hir::lower_module_with_class_id(&ast_module, &module_name, &source_file_path, *next_class_id)?;
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

        if let Some((resolved_path, kind)) = resolve_import(&import.source, &canonical, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
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

                    if !enable_js_runtime {
                        return Err(anyhow!(
                            "Import '{}' resolves to JavaScript file '{}' which requires --enable-js-runtime flag",
                            import.source,
                            resolved_path.display()
                        ));
                    }

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
            if let Some((resolved_path, kind)) = resolve_import(src, &canonical, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
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

/// Compile for web target: emit JavaScript + HTML instead of native code
fn compile_for_web(ctx: &CompilationContext, args: &CompileArgs, format: OutputFormat) -> Result<()> {
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

    let html = perry_codegen_js::compile_modules_to_html(&modules, title)?;

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

    Ok(())
}

pub fn run(args: CompileArgs, format: OutputFormat, _use_color: bool, _verbose: u8) -> Result<()> {
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

    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1; // Start at 1, 0 is reserved for "no parent"
    let skip_transforms = args.target.as_deref() == Some("web");

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

    // --- Web target: emit JavaScript instead of native code ---
    if args.target.as_deref() == Some("web") {
        return compile_for_web(&ctx, &args, format);
    }

    // Transform JS imports into runtime calls
    if ctx.needs_js_runtime {
        for (_, hir_module) in ctx.native_modules.iter_mut() {
            perry_hir::transform_js_imports(hir_module);
        }
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

    // Fix local native instance method calls within each module
    // This handles cases like: const pool = mysql.createPool(); pool.execute();
    for (_, hir_module) in ctx.native_modules.iter_mut() {
        perry_hir::fix_local_native_instances(hir_module);
    }

    // Fix cross-module native instance method calls
    if !exported_instances.is_empty() || !exported_func_return_instances.is_empty() {
        for (_, hir_module) in ctx.native_modules.iter_mut() {
            perry_hir::fix_cross_module_native_instances(hir_module, &exported_instances, &exported_func_return_instances);
        }
    }

    // Run monomorphization pass on all native modules
    for (_, hir_module) in ctx.native_modules.iter_mut() {
        perry_hir::monomorphize_module(hir_module);
    }

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

    // Compile native modules
    let mut failed_modules: Vec<String> = Vec::new();
    for (path, hir_module) in &ctx.native_modules {
        let mut compiler = perry_codegen::Compiler::new(target.as_deref())?;

        // Check if this is the entry module
        let is_entry = path == &entry_path;
        compiler.set_is_entry_module(is_entry);

        // Set output type for dylib support
        compiler.set_output_type(args.output_type.clone());

        // For entry module, add init function calls for all other native modules
        if is_entry {
            for module_name in &non_entry_module_names {
                compiler.add_native_module_init(module_name.clone());
            }

            // Register bundled extensions for static plugin registration in init
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

        // Tell codegen whether stdlib functions are available
        compiler.set_needs_stdlib(ctx.needs_stdlib);

        // Pass external native library FFI functions to codegen
        if !ctx.native_libraries.is_empty() {
            let ffi_functions: Vec<(String, Vec<String>, String)> = ctx.native_libraries.iter()
                .flat_map(|lib| lib.functions.iter().map(|f| {
                    (f.name.clone(), f.params.clone(), f.returns.clone())
                }))
                .collect();
            compiler.set_native_library_functions(ffi_functions);
        }

        // If we need JS runtime, tell the compiler to generate init code
        if ctx.needs_js_runtime {
            compiler.set_needs_js_runtime(true);
            // Pass JS module paths for loading
            for (specifier, _module) in &ctx.js_modules {
                compiler.add_js_module(specifier.clone());
            }
        }

        // Register imported classes from other native modules
        for import in &hir_module.imports {
            // Only process imports from other native TypeScript modules
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
                        // Handle namespace imports: import * as X from './module'
                        // Register the local name so codegen intercepts PropertyGet on it
                        compiler.register_namespace_import(local.clone());
                        // Pre-declare all exports from the target module so PropertyGet resolves them
                        if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                            for (export_name, origin_path) in exports {
                                let origin_prefix = compute_module_prefix(origin_path, &ctx.project_root);
                                let _ = compiler.pre_declare_import_export(export_name, &origin_prefix);

                                // Also handle functions if re-exported
                                let key = (origin_path.clone(), export_name.clone());
                                if let Some(&param_count) = exported_func_param_counts.get(&key) {
                                    compiler.register_imported_func_param_count(export_name.clone(), param_count);
                                    let _ = compiler.pre_declare_import_wrapper(export_name, &origin_prefix, param_count);
                                }

                                // Register imported classes
                                if let Some(class) = exported_classes.get(&key) {
                                    compiler.register_imported_class(class, None, &origin_prefix)?;
                                }

                                // Register imported enums
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
                    perry_hir::ImportSpecifier::Default { local } => (local.clone(), local.clone()),
                    perry_hir::ImportSpecifier::Namespace { .. } => unreachable!(),
                };

                let key = (resolved_path_str.clone(), exported_name.clone());

                // Resolve through re-export chains to find the origin module prefix.
                // When importing from a barrel file (e.g., lib.ts with `export * from "./lib/db.js"`),
                // we need to link to the ORIGIN module's wrapper (___wrapper_lib_db_ts__executeQuery),
                // not the barrel file's wrapper (___wrapper_lib_ts__executeQuery) which would be a stub.
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

                // Check if this import is a class from another module
                if let Some(class) = exported_classes.get(&key) {
                    // Register this class as an import in the current compiler
                    // Pass the local_name as an alias so the class can be found when used with that name
                    // effective_prefix resolves through re-export chains to the origin module
                    compiler.register_imported_class(class, Some(&local_name), &effective_prefix)?;
                }

                // Check if this import is a function from another module
                // Register its param count, return type, and pre-declare the scoped wrapper
                if let Some(&param_count) = exported_func_param_counts.get(&key) {
                    compiler.register_imported_func_param_count(exported_name.clone(), param_count);
                    let _ = compiler.pre_declare_import_wrapper(&exported_name, &effective_prefix, param_count);
                }
                // Register the imported function's return type for await type resolution
                if let Some(return_type) = exported_func_return_types.get(&key) {
                    compiler.register_imported_func_return_type(local_name.clone(), return_type.clone());
                }

                // Pre-declare scoped export global for this import
                let _ = compiler.pre_declare_import_export(&exported_name, &effective_prefix);

                // Check if this import is an enum from another module
                if let Some(members) = exported_enums.get(&key) {
                    compiler.register_imported_enum(&local_name, members);
                }
            }
        }

        let module_name_for_err = hir_module.name.clone();
        let module_path_for_err = path.display().to_string();
        let object_code = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compiler.compile_module(hir_module)
        })) {
            Ok(Ok(code)) => code,
            Ok(Err(e)) => {
                eprintln!("Error compiling module '{}' ({}): {}", module_name_for_err, module_path_for_err, e);
                failed_modules.push(module_name_for_err);
                continue;
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                eprintln!("PANIC compiling module '{}' ({}): {}", module_name_for_err, module_path_for_err, msg);
                failed_modules.push(module_name_for_err);
                continue;
            }
        };

        // Generate a unique object file name using the full sanitized module name.
        // Module names are derived from relative paths and are guaranteed unique,
        // so this avoids collisions like channels/plugins/types.ts vs plugins/types.ts.
        let obj_name = hir_module.name
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
            .trim_matches('_')
            .to_string();
        let obj_path = PathBuf::from(format!("{}.o", obj_name));

        fs::write(&obj_path, &object_code)?;
        match format {
            OutputFormat::Text => println!("Wrote object file: {}", obj_path.display()),
            OutputFormat::Json => {}
        }
        obj_paths.push(obj_path);
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
        for scan_path in &all_scan_paths {
            if let Ok(output) = std::process::Command::new("nm").arg("-g").arg(scan_path).output() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let (st, sn) = if parts.len() == 3 { (parts[1], parts[2]) } else { (parts[0], parts[1]) };
                        let cn = sn.strip_prefix('_').unwrap_or(sn);
                        if st == "U" {
                            // Add export/wrapper symbols to undefined list
                            if cn.starts_with("__export_") || cn.starts_with("__wrapper_") {
                                undefined_syms.insert(cn.to_string());
                            }
                            // Only add jsruntime symbols if jsruntime is NOT being used
                            // (these are defined in libperry_jsruntime.a)
                            else if !use_jsruntime && (cn == "js_call_function" || cn == "js_load_module" || cn == "js_new_from_handle") {
                                undefined_syms.insert(cn.to_string());
                            }
                        } else if matches!(st, "T" | "t" | "D" | "d" | "S" | "s" | "B" | "b") {
                            defined_syms.insert(cn.to_string());
                        }
                    }
                }
            }
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
        return Ok(());
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
    let is_windows = matches!(target.as_deref(), Some("windows"));

    // For dylib output, skip runtime/stdlib linking — symbols resolve from host at dlopen time
    if is_dylib {
        let mut cmd = if is_linux {
            let mut c = Command::new("cc");
            c.arg("-shared");
            c
        } else {
            // macOS
            let mut c = Command::new("cc");
            c.arg("-dynamiclib")
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

        return Ok(());
    }

    let runtime_lib = find_runtime_library(target.as_deref())?;
    let stdlib_lib = find_stdlib_library(target.as_deref());
    let jsruntime_lib = if !is_ios && !is_android && !is_windows && (ctx.needs_js_runtime || args.enable_js_runtime) {
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
    let mut cmd = if is_ios {
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

        let mut c = Command::new(clang);
        c.arg("-target").arg(triple)
         .arg("-isysroot").arg(sysroot);
        c
    } else if is_android {
        // Use Android NDK clang to produce a shared library (.so)
        let ndk_home = std::env::var("ANDROID_NDK_HOME").map_err(|_| {
            anyhow!("ANDROID_NDK_HOME not set. Set it to your NDK path, e.g. $HOME/Library/Android/sdk/ndk/28.0.12433566")
        })?;
        let clang = format!(
            "{}/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android24-clang",
            ndk_home
        );
        if !PathBuf::from(&clang).exists() {
            return Err(anyhow!("Android NDK clang not found at: {}", clang));
        }
        let mut c = Command::new(clang);
        c.arg("-shared")
         .arg("-fPIC")
         .arg("-target").arg("aarch64-linux-android24")
         .arg("-Wl,-z,max-page-size=16384")
         .arg("-Wl,-z,separate-loadable-segments");
        c
    } else if is_linux {
        // Linux target: when running on Linux natively, just use "cc".
        // When cross-compiling from macOS, pass -target for clang.
        let mut c = Command::new("cc");
        #[cfg(not(target_os = "linux"))]
        {
            c.arg("-target").arg("x86_64-unknown-linux-gnu");
        }
        c
    } else if is_windows {
        // Windows target — use MSVC link.exe (native) or lld-link (cross)
        let linker = find_msvc_link_exe().unwrap_or_else(|| PathBuf::from("link.exe"));
        let mut c = Command::new(linker);
        c.arg("/SUBSYSTEM:WINDOWS")
         .arg("/ENTRY:mainCRTStartup")
         .arg("/NOLOGO")
         .arg("/FORCE:UNRESOLVED");
        // Set up MSVC library search paths if LIB env isn't already configured
        if std::env::var("LIB").is_err() {
            if let Some(lib_paths) = find_msvc_lib_paths() {
                c.env("LIB", lib_paths);
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
    // For Android (ELF) and Windows (MSVC), skip the extra runtime when UI provides it.
    let skip_runtime = (is_android || is_windows) && ctx.needs_ui && find_ui_library(target.as_deref()).is_some();
    if !skip_runtime {
        if let Some(ref jsruntime) = jsruntime_lib {
            cmd.arg(jsruntime);
            // Also link runtime to supply symbols DCE'd from jsruntime (e.g. js_register_class_method)
            if !is_android && !is_windows {
                cmd.arg(&runtime_lib);
            }
        } else if ctx.needs_stdlib {
            if let Some(ref stdlib) = stdlib_lib {
                cmd.arg(stdlib);
            } else {
                eprintln!("Warning: stdlib required but libperry_stdlib.a not found, using runtime-only");
                cmd.arg(&runtime_lib);
            }
        } else {
            // Runtime-only linking — no stdlib needed
            cmd.arg(&runtime_lib);
        }
    }

    if is_windows {
        cmd.arg(format!("/OUT:{}", exe_path.display()));
    } else {
        cmd.arg("-o")
            .arg(&exe_path)
            .arg("-lc");
    }

    // For plugin hosts, export symbols so dlopen'd plugins can resolve them
    if ctx.needs_plugins && !is_windows {
        #[cfg(target_os = "macos")]
        {
            cmd.arg("-Wl,-export_dynamic");
        }
        #[cfg(target_os = "linux")]
        {
            cmd.arg("-rdynamic");
        }
        cmd.arg("-ldl"); // needed for dlopen/dlsym/dlclose
    }

    if is_ios {
        // iOS frameworks
        cmd.arg("-framework").arg("UIKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreGraphics")
           .arg("-framework").arg("Security")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("SystemConfiguration")
           .arg("-liconv")
           .arg("-lresolv");
    } else if is_android {
        // Android system libraries
        cmd.arg("-lm")
           .arg("-ldl")
           .arg("-llog");
    } else if is_linux {
        // Linux system libraries (cross-compile target)
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
    } else if is_windows {
        // Windows system libraries
        cmd.arg("user32.lib")
           .arg("gdi32.lib")
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
           .arg("runtimeobject.lib");
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
        if let Some(ui_lib) = find_ui_library(target.as_deref()) {
            cmd.arg(&ui_lib);

            if is_ios {
                // UIKit already linked above
            } else if is_android {
                // Android UI uses JNI - no additional system libs needed
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
            } else if is_windows {
                // Win32 system libs already linked above
            } else {
                #[cfg(target_os = "macos")]
                {
                    cmd.arg("-framework").arg("AppKit");
                    cmd.arg("-framework").arg("QuartzCore"); // CAGradientLayer, CALayer
                }
            }

            match format {
                OutputFormat::Text => println!("Linking perry/ui (native UI)"),
                OutputFormat::Json => {}
            }
        } else {
            let (lib_name, build_cmd) = if is_ios {
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
                    cmd.arg(&lib);
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
                cmd.arg(format!("-l{}", lib));
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

    // For iOS targets, create a .app bundle
    if is_ios {
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        let bundle_id = format!("com.perry.{}", exe_stem);
        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{}</string>
    <key>CFBundleIdentifier</key>
    <string>{}</string>
    <key>CFBundleName</key>
    <string>{}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>UILaunchStoryboardName</key>
    <string></string>
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
            exe_stem, bundle_id, exe_stem
        );
        fs::write(app_dir.join("Info.plist"), info_plist)?;

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

    // Strip debug symbols from the final binary (reduces size significantly)
    if !is_dylib && !is_ios {
        let _ = std::process::Command::new("strip").arg(&exe_path).status();
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

    Ok(())
}
