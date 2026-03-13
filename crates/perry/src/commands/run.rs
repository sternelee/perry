//! Run command - compile and launch a TypeScript file in one step

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use console::style;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::compile::{CompileArgs, CompileResult};
use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Input TypeScript file
    pub input: Option<PathBuf>,

    /// Run on macOS (default on macOS host)
    #[arg(long)]
    pub macos: bool,

    /// Run on iOS (simulator or device)
    #[arg(long)]
    pub ios: bool,

    /// Run on web (opens in browser)
    #[arg(long)]
    pub web: bool,

    /// Run on Android
    #[arg(long)]
    pub android: bool,

    /// Specific iOS simulator UDID to target
    #[arg(long)]
    pub simulator: Option<String>,

    /// Specific iOS device UDID to target
    #[arg(long)]
    pub device: Option<String>,

    /// Enable V8 JavaScript runtime
    #[arg(long)]
    pub enable_js_runtime: bool,

    /// Enable type checking via tsgo
    #[arg(long)]
    pub type_check: bool,

    /// Force local compilation (error if toolchain missing)
    #[arg(long)]
    pub local: bool,

    /// Force remote compilation via Perry Hub build server
    #[arg(long)]
    pub remote: bool,

    /// Arguments passed to the compiled program
    #[arg(last = true)]
    pub program_args: Vec<String>,
}

/// A detected simulator or device
struct DeviceInfo {
    udid: String,
    name: String,
}

pub fn run(args: RunArgs, format: OutputFormat, use_color: bool, verbose: u8) -> Result<()> {
    // 1. Resolve entry file
    let input = resolve_entry_file(args.input.as_deref())?;

    // 2. Resolve target and device
    let (target, device_udid) = resolve_target(&args)?;

    // 3. Decide local vs remote compilation
    let needs_cross = matches!(target.as_deref(), Some("ios-simulator") | Some("ios") | Some("android"));
    let can_local = !needs_cross || can_compile_locally(target.as_deref());

    let use_remote = if args.remote {
        true
    } else if args.local {
        if !can_local {
            bail!(
                "Local compilation for {:?} requires cross-compiled runtime libraries.\n\
                 Build with: cargo build --release -p perry-runtime -p perry-stdlib --target {}\n\
                 Or use --remote to compile via Perry Hub.",
                target.as_deref().unwrap_or("native"),
                rust_target_triple(target.as_deref()).unwrap_or("unknown")
            );
        }
        false
    } else {
        // Auto-detect: use remote when local isn't possible
        needs_cross && !can_local
    };

    if use_remote {
        let target_str = target.as_deref().unwrap_or("native");
        let rt = tokio::runtime::Runtime::new()?;
        let result = rt.block_on(remote_build_and_launch(
            &input,
            target_str,
            device_udid.as_deref(),
            &args.program_args,
            format,
        ));
        return result;
    }

    // Local compile path
    let compile_args = CompileArgs {
        input: input.clone(),
        output: None,
        keep_intermediates: false,
        print_hir: false,
        no_link: false,
        enable_js_runtime: args.enable_js_runtime,
        target: target.clone(),
        app_bundle_id: None,
        output_type: "executable".to_string(),
        bundle_extensions: None,
        type_check: args.type_check,
    };

    let result = super::compile::run(compile_args, format, use_color, verbose)?;
    launch(&result, device_udid.as_deref(), &args.program_args, format)
}

/// Check if we have the cross-compiled runtime libraries for a target
fn can_compile_locally(target: Option<&str>) -> bool {
    let triple = match rust_target_triple(target) {
        Some(t) => t,
        None => return true, // host build, always available
    };
    let runtime_path = format!("target/{triple}/release/libperry_runtime.a");
    Path::new(&runtime_path).exists()
}

/// Map perry target names to Rust target triples
fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("ios-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") => Some("aarch64-apple-ios"),
        Some("android") => Some("aarch64-linux-android"),
        _ => None,
    }
}

// --- Remote build ---

/// Build remotely via Perry Hub and launch the result
async fn remote_build_and_launch(
    input: &Path,
    target: &str,
    device_udid: Option<&str>,
    program_args: &[String],
    format: OutputFormat,
) -> Result<()> {
    use super::publish::{
        auto_register_license, create_project_tarball, load_config, save_config,
    };
    use base64::Engine;
    use futures_util::{SinkExt, StreamExt};
    use indicatif::{ProgressBar, ProgressStyle};
    use reqwest::multipart;
    use serde::Deserialize;
    use std::io::Write;
    use tokio_tungstenite::tungstenite::Message;

    let project_dir = input
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));

    // Walk up to find project root (directory containing package.json or perry.toml)
    let project_root = find_project_root(&project_dir);

    // Resolve server URL and license key
    let mut config = load_config();
    let server_url = config
        .server
        .clone()
        .unwrap_or_else(|| "https://hub.perryts.com".into());

    let license_key = match config.license_key.clone() {
        Some(key) => key,
        None => {
            if let OutputFormat::Text = format {
                println!("  Registering with Perry Hub...");
            }
            let key = auto_register_license(&server_url).await?;
            config.license_key = Some(key.clone());
            save_config(&config)?;
            key
        }
    };

    // Determine app name and bundle ID from package.json
    let (app_name, bundle_id) = read_app_metadata(&project_root, input);

    // Determine entry path relative to project root
    let entry = input
        .canonicalize()
        .unwrap_or_else(|_| input.to_path_buf())
        .strip_prefix(&project_root)
        .unwrap_or(input)
        .to_string_lossy()
        .to_string();

    // The build target for the manifest
    let build_target = match target {
        "ios-simulator" | "ios" => "ios",
        other => other,
    };

    if let OutputFormat::Text = format {
        println!();
        println!(
            "  {} Building {} for {} via Perry Hub",
            style("▶").cyan().bold(),
            style(&app_name).bold(),
            style(target).cyan()
        );
        println!();
    }

    // Package project
    if let OutputFormat::Text = format {
        print!("  Packaging project...");
        std::io::stdout().flush().ok();
    }

    let tarball = create_project_tarball(&project_root).context("Failed to create project tarball")?;

    if let OutputFormat::Text = format {
        println!(
            " {} ({:.1} MB)",
            style("done").green(),
            tarball.len() as f64 / 1_048_576.0
        );
    }

    // Build manifest
    let ios_distribute = match target {
        "ios" => "none",          // device build, sign but don't upload to App Store
        "ios-simulator" => "simulator",
        _ => "none",
    };
    let manifest = serde_json::json!({
        "app_name": app_name,
        "bundle_id": bundle_id,
        "version": "0.0.1",
        "entry": entry,
        "targets": [build_target],
        "ios_distribute": ios_distribute,
    });

    // Build credentials — device builds need signing
    let credentials = if target == "ios" {
        build_device_credentials(&config, &bundle_id)?
    } else {
        serde_json::json!({
            "apple_team_id": null,
            "apple_signing_identity": null,
            "apple_key_id": null,
            "apple_issuer_id": null,
            "apple_p8_key": null
        })
    };

    // Upload
    if let OutputFormat::Text = format {
        print!("  Uploading to build server...");
        std::io::stdout().flush().ok();
    }

    let tarball_b64 = base64::engine::general_purpose::STANDARD.encode(&tarball);

    let client = reqwest::Client::new();
    let form = multipart::Form::new()
        .text("license_key", license_key)
        .text("manifest", serde_json::to_string(&manifest)?)
        .text("credentials", serde_json::to_string(&credentials)?);

    let form = form.text("tarball_b64", tarball_b64);

    let resp = client
        .post(format!("{server_url}/api/v1/build"))
        .multipart(form)
        .send()
        .await
        .context("Failed to connect to build server")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Build server returned {status}: {body}");
    }

    #[derive(Deserialize)]
    struct BuildResponse {
        job_id: String,
        ws_url: String,
        position: usize,
    }

    let build_resp: BuildResponse = resp.json().await.context("Invalid build response")?;

    if let OutputFormat::Text = format {
        println!(" {}", style("done").green());
        println!("  Job ID:    {}", style(&build_resp.job_id).dim());
        if build_resp.position > 1 {
            println!("  Position:  {}", build_resp.position);
        }
        println!();
    }

    // WebSocket progress
    let ws_url = if build_resp.ws_url.starts_with("ws://") || build_resp.ws_url.starts_with("wss://") {
        build_resp.ws_url.clone()
    } else if server_url.starts_with("https://") {
        format!("wss://{}{}", &server_url["https://".len()..], build_resp.ws_url)
    } else {
        format!("ws://{}{}", &server_url["http://".len()..], build_resp.ws_url)
    };

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("Failed to connect WebSocket")?;

    let (mut ws_write, mut read) = ws_stream.split();

    ws_write
        .send(Message::Text(
            format!(r#"{{"type":"subscribe","job_id":"{}"}}"#, build_resp.job_id).into(),
        ))
        .await
        .context("Failed to send subscribe message")?;

    let pb = if let OutputFormat::Text = format {
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {spinner:.cyan} [{bar:30.cyan/dim}] {msg}")
                .unwrap()
                .progress_chars("━╸─"),
        );
        pb.set_message("Waiting for build...");
        Some(pb)
    } else {
        None
    };

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum ServerMsg {
        JobCreated {
            #[serde(default)]
            job_id: Option<String>,
        },
        QueueUpdate {
            position: usize,
        },
        Stage {
            #[allow(dead_code)]
            stage: String,
            message: String,
        },
        Log {
            line: String,
            stream: String,
        },
        Progress {
            percent: u8,
        },
        ArtifactReady {
            artifact_name: String,
            download_url: String,
            #[serde(default)]
            download_path: Option<String>,
        },
        Published {
            #[allow(dead_code)]
            platform: String,
            #[allow(dead_code)]
            message: String,
        },
        Error {
            message: String,
        },
        #[serde(other)]
        Unknown,
    }

    let mut download_url: Option<String> = None;
    let mut download_path: Option<String> = None;
    let mut artifact_name: Option<String> = None;
    let mut build_success = false;

    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                if let Some(ref pb) = pb {
                    pb.abandon_with_message(format!("WebSocket error: {e}"));
                }
                bail!("WebSocket error: {e}");
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let server_msg: ServerMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match server_msg {
            ServerMsg::JobCreated { .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message("Build started");
                }
            }
            ServerMsg::QueueUpdate { position, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message(format!("Queue position: {position}"));
                }
            }
            ServerMsg::Stage { message, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message(format!("▶️  {message}"));
                }
            }
            ServerMsg::Log { line, stream, .. } => {
                if let Some(ref pb) = pb {
                    if stream == "stderr" {
                        pb.println(format!("    {}", style(&line).dim()));
                    }
                }
            }
            ServerMsg::Progress { percent, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_position(percent as u64);
                }
            }
            ServerMsg::ArtifactReady {
                artifact_name: name,
                download_url: url,
                download_path: path,
                ..
            } => {
                if let Some(ref pb) = pb {
                    pb.set_position(100);
                    pb.finish_with_message(format!(
                        "✓ Build complete: {}",
                        style(&name).bold()
                    ));
                }
                download_url = Some(url);
                download_path = path;
                artifact_name = Some(name);
                build_success = true;
                break; // artifact ready, proceed to download
            }
            ServerMsg::Published { .. } => {
                build_success = true;
                break;
            }
            ServerMsg::Error { message, .. } => {
                if let Some(ref pb) = pb {
                    pb.abandon_with_message(format!("✗ {message}"));
                }
                bail!("Build failed: {message}");
            }
            ServerMsg::Unknown => {}
        }
    }

    if !build_success {
        bail!("Build failed (no artifact received)");
    }

    // Download artifact
    let (url, name) = match (download_url, artifact_name) {
        (Some(u), Some(n)) => (u, n),
        _ => bail!("Build succeeded but no download URL received"),
    };

    if let OutputFormat::Text = format {
        print!("  Downloading {}...", name);
        std::io::stdout().flush().ok();
    }

    let dist_dir = PathBuf::from("dist");
    std::fs::create_dir_all(&dist_dir)?;
    let dest = dist_dir.join(&name);

    if let Some(ref src_path) = download_path {
        std::fs::copy(src_path, &dest)
            .with_context(|| format!("Failed to copy artifact from {src_path}"))?;
    } else {
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.clone()
        } else {
            format!("{server_url}{url}")
        };
        let resp = client
            .get(&full_url)
            .send()
            .await
            .context("Failed to download artifact")?;

        if !resp.status().is_success() {
            bail!("Download failed: {}", resp.status());
        }

        let bytes = resp.bytes().await?;
        // Detect base64-encoded content
        let data = if bytes.len() > 4
            && bytes.iter().all(|&b| {
                b.is_ascii_alphanumeric()
                    || b == b'+'
                    || b == b'/'
                    || b == b'='
                    || b == b'\n'
                    || b == b'\r'
            })
        {
            base64::engine::general_purpose::STANDARD
                .decode(&bytes)
                .unwrap_or_else(|_| bytes.to_vec())
        } else {
            bytes.to_vec()
        };
        std::fs::write(&dest, &data)?;
    }

    if let OutputFormat::Text = format {
        println!(" {} → {}", style("done").green(), style(dest.display()).bold());
        println!();
    }

    // For iOS: extract .app from .ipa and install
    if target == "ios-simulator" || target == "ios" {
        let app_dir = extract_app_from_ipa(&dest, &dist_dir)?;
        let udid = device_udid.ok_or_else(|| anyhow!("No device UDID for iOS launch"))?;

        if target == "ios-simulator" {
            launch_ios_simulator(&app_dir, &bundle_id, udid, format)
        } else {
            launch_ios_device(&app_dir, &bundle_id, udid, format)
        }
    } else {
        // Native binary
        launch_native(&dest, program_args, format)
    }
}

/// Extract .app bundle from an .ipa file
fn extract_app_from_ipa(ipa_path: &Path, dest_dir: &Path) -> Result<PathBuf> {
    use std::io::Read;

    let file = std::fs::File::open(ipa_path).context("Failed to open .ipa")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read .ipa as ZIP")?;

    // .ipa structure: Payload/<AppName>.app/...
    let mut app_name = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.starts_with("Payload/") && name.ends_with(".app/") {
            // Extract the .app directory name
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() >= 2 {
                app_name = Some(parts[1].to_string());
                break;
            }
        }
    }

    let app_name = app_name.ok_or_else(|| anyhow!("No .app found in .ipa"))?;
    let app_dir = dest_dir.join(&app_name);
    let _ = std::fs::remove_dir_all(&app_dir); // clean previous

    // Extract all files under Payload/<app_name>/
    let prefix = format!("Payload/{}/", app_name);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if let Some(rel) = name.strip_prefix(&prefix) {
            if rel.is_empty() {
                continue;
            }
            let out_path = app_dir.join(rel);
            if name.ends_with('/') {
                std::fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out_file = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out_file)?;
            }
        }
    }

    // Make the main executable... executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Find the executable inside the .app (same name as app, without .app)
        let exe_name = app_name.strip_suffix(".app").unwrap_or(&app_name);
        let exe_path = app_dir.join(exe_name);
        if exe_path.exists() {
            let _ = std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755));
        }
    }

    Ok(app_dir)
}

/// Find project root by walking up from a directory
fn find_project_root(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    for _ in 0..10 {
        if dir.join("package.json").exists() || dir.join("perry.toml").exists() {
            return dir;
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    start.to_path_buf()
}

/// Read app name and bundle ID from package.json
fn read_app_metadata(project_root: &Path, input: &Path) -> (String, String) {
    let pkg_path = project_root.join("package.json");
    if let Ok(content) = std::fs::read_to_string(&pkg_path) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            let name = pkg
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("app")
                .to_string();
            let bundle_id = pkg
                .get("bundleId")
                .or_else(|| pkg.get("perry").and_then(|p| p.get("bundleId")))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("com.perry.{}", name));
            return (name, bundle_id);
        }
    }
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app");
    (stem.to_string(), format!("com.perry.{}", stem))
}

/// Build signing credentials for physical iOS device builds.
/// Loads from saved config (~/.perry/config.toml), auto-exports .p12 from Keychain,
/// and finds provisioning profile in ~/.perry/.
fn build_device_credentials(
    config: &super::publish::PerryConfig,
    bundle_id: &str,
) -> Result<serde_json::Value> {
    use base64::Engine;

    let apple = config.apple.as_ref();
    let team_id = apple.and_then(|a| a.team_id.clone());
    let key_id = apple.and_then(|a| a.key_id.clone());
    let issuer_id = apple.and_then(|a| a.issuer_id.clone());

    // Read .p8 key if path is saved
    let p8_key = apple
        .and_then(|a| a.p8_key_path.as_ref())
        .and_then(|p| std::fs::read_to_string(p).ok());

    // Auto-detect signing identity from Keychain
    let signing_identity = detect_signing_identity();

    // Auto-export .p12 from Keychain
    let (cert_b64, cert_password) = if let Some(ref identity) = signing_identity {
        auto_export_p12(identity)
    } else {
        (None, None)
    };

    // Find provisioning profile in ~/.perry/
    let profile_b64 = find_provisioning_profile(bundle_id);

    if signing_identity.is_none() {
        bail!(
            "No code signing identity found for device builds.\n\
             Run `perry setup ios` first, or use `perry run --ios --simulator <UDID>` for unsigned builds."
        );
    }

    Ok(serde_json::json!({
        "apple_team_id": team_id,
        "apple_signing_identity": signing_identity,
        "apple_key_id": key_id,
        "apple_issuer_id": issuer_id,
        "apple_p8_key": p8_key,
        "provisioning_profile_base64": profile_b64,
        "apple_certificate_p12_base64": cert_b64,
        "apple_certificate_password": cert_password,
    }))
}

/// Detect first available Apple Distribution / Developer signing identity
fn detect_signing_identity() -> Option<String> {
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Prefer "Apple Distribution" for device, then "iPhone Distribution", then first available
    let mut identities: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(q1) = line.find('"') {
            if let Some(q2) = line.rfind('"') {
                if q2 > q1 {
                    identities.push(line[q1 + 1..q2].to_string());
                }
            }
        }
    }

    identities
        .iter()
        .find(|n| n.starts_with("Apple Distribution"))
        .or_else(|| identities.iter().find(|n| n.starts_with("iPhone Distribution")))
        .or_else(|| identities.first())
        .cloned()
}

/// Auto-export a .p12 from Keychain for the given identity
fn auto_export_p12(identity: &str) -> (Option<String>, Option<String>) {
    use base64::Engine;

    let password = "perry-run-auto";
    let tmp_path = std::env::temp_dir().join("perry_run_auto.p12");

    // Find the identity hash
    let output = match Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return (None, None),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut hash = None;
    for line in stdout.lines() {
        if line.contains(identity) {
            let trimmed = line.trim();
            let after_paren = trimmed.find(") ").map(|i| i + 2).unwrap_or(0);
            let hash_end = trimmed.find(" \"").unwrap_or(trimmed.len());
            if hash_end > after_paren {
                hash = Some(trimmed[after_paren..hash_end].trim().to_string());
                break;
            }
        }
    }

    let hash = match hash {
        Some(h) => h,
        None => return (None, None),
    };

    // Export .p12
    let status = Command::new("security")
        .args([
            "export",
            "-k",
            "login.keychain-db",
            "-t",
            "identities",
            "-f",
            "pkcs12",
            "-P",
            password,
            "-o",
        ])
        .arg(&tmp_path)
        .status();

    if status.map(|s| s.success()).unwrap_or(false) {
        if let Ok(data) = std::fs::read(&tmp_path) {
            let _ = std::fs::remove_file(&tmp_path);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            return (Some(b64), Some(password.to_string()));
        }
    }
    let _ = std::fs::remove_file(&tmp_path);
    (None, None)
}

/// Find a provisioning profile for the given bundle ID in ~/.perry/
fn find_provisioning_profile(bundle_id: &str) -> Option<String> {
    use base64::Engine;

    let perry_dir = dirs::home_dir()?.join(".perry");
    if !perry_dir.exists() {
        return None;
    }

    // Look for {bundle_id_underscored}.mobileprovision or generic perry.mobileprovision
    let underscored = bundle_id.replace('.', "_");
    let candidates = [
        perry_dir.join(format!("{underscored}.mobileprovision")),
        perry_dir.join("perry.mobileprovision"),
    ];

    for path in &candidates {
        if path.exists() {
            if let Ok(data) = std::fs::read(path) {
                return Some(base64::engine::general_purpose::STANDARD.encode(&data));
            }
        }
    }

    // Also check any .mobileprovision file in ~/.perry/
    if let Ok(entries) = std::fs::read_dir(&perry_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("mobileprovision") {
                if let Ok(data) = std::fs::read(entry.path()) {
                    return Some(base64::engine::general_purpose::STANDARD.encode(&data));
                }
            }
        }
    }

    None
}

// --- Local compilation helpers ---

/// Resolve the entry TypeScript file
fn resolve_entry_file(input: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = input {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(anyhow!("File not found: {}", path.display()));
    }

    // Try perry.toml
    if let Some(entry) = read_perry_toml_entry() {
        if entry.exists() {
            return Ok(entry);
        }
    }

    // Fallback: src/main.ts, then main.ts
    for candidate in &["src/main.ts", "main.ts"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(anyhow!(
        "No input file specified and no main.ts found.\n\
         Usage: perry run <file.ts>\n\
         Or create src/main.ts or main.ts, or set entry in perry.toml"
    ))
}

/// Read entry point from perry.toml if present
fn read_perry_toml_entry() -> Option<PathBuf> {
    let toml_str = std::fs::read_to_string("perry.toml").ok()?;
    for line in toml_str.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("entry") {
            if let Some(eq_pos) = trimmed.find('=') {
                let value = trimmed[eq_pos + 1..].trim().trim_matches('"');
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

/// Resolve the compilation target and optional device UDID
fn resolve_target(args: &RunArgs) -> Result<(Option<String>, Option<String>)> {
    if args.web {
        return Ok((Some("web".to_string()), None));
    }

    if args.android {
        let devices = detect_android_devices()?;
        if devices.is_empty() {
            return Err(anyhow!(
                "No Android devices found. Connect a device or start an emulator, then try again."
            ));
        }
        let serial = if devices.len() == 1 {
            devices[0].udid.clone()
        } else {
            pick_device(&devices, "Android device")?
        };
        return Ok((Some("android".to_string()), Some(serial)));
    }

    if args.ios {
        if let Some(ref udid) = args.simulator {
            return Ok((Some("ios-simulator".to_string()), Some(udid.clone())));
        }
        if let Some(ref udid) = args.device {
            return Ok((Some("ios".to_string()), Some(udid.clone())));
        }

        // Auto-detect: booted simulators + connected devices
        let simulators = detect_booted_simulators().unwrap_or_default();
        let devices = detect_ios_devices().unwrap_or_default();

        let mut all: Vec<(DeviceInfo, &str)> = Vec::new();
        for s in simulators {
            all.push((s, "ios-simulator"));
        }
        for d in devices {
            all.push((d, "ios"));
        }

        if all.is_empty() {
            return Err(anyhow!(
                "No iOS simulators or devices found.\n\
                 Boot a simulator:  xcrun simctl boot <UDID>\n\
                 Or specify one:    perry run --ios --simulator <UDID>"
            ));
        }

        if all.len() == 1 {
            let (dev, target) = all.remove(0);
            return Ok((Some(target.to_string()), Some(dev.udid)));
        }

        // Multiple options: prompt
        let names: Vec<String> = all
            .iter()
            .map(|(d, t)| format!("{} ({})", d.name, t))
            .collect();
        let selection = pick_from_list(&names, "Select iOS target")?;
        let (dev, target) = all.remove(selection);
        return Ok((Some(target.to_string()), Some(dev.udid)));
    }

    if args.macos {
        return Ok((None, None));
    }

    // Default: native (no target flag)
    Ok((None, None))
}

/// Launch the compiled output based on target
fn launch(
    result: &CompileResult,
    device_udid: Option<&str>,
    program_args: &[String],
    format: OutputFormat,
) -> Result<()> {
    match result.target.as_str() {
        "web" => launch_web(&result.output_path, format),
        "ios-simulator" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No simulator UDID — use --simulator <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for iOS app"))?;
            launch_ios_simulator(&result.output_path, bundle_id, udid, format)
        }
        "ios" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No device UDID — use --device <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for iOS app"))?;
            launch_ios_device(&result.output_path, bundle_id, udid, format)
        }
        "android" => {
            if let OutputFormat::Text = format {
                println!();
                println!("Android .so compiled. Perry produces native libraries, not APKs.");
                println!("To test, integrate the .so into an Android project.");
            }
            Ok(())
        }
        _ => launch_native(&result.output_path, program_args, format),
    }
}

// --- Launch functions ---

/// Launch a native executable
fn launch_native(exe_path: &Path, program_args: &[String], format: OutputFormat) -> Result<()> {
    let exe = if exe_path.is_absolute() {
        exe_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(exe_path)
    };

    if !exe.exists() {
        return Err(anyhow!(
            "Compiled executable not found: {}",
            exe.display()
        ));
    }

    if let OutputFormat::Text = format {
        println!();
        println!("Running {}...", exe_path.display());
        println!();
    }

    let status = Command::new(&exe)
        .args(program_args)
        .status()
        .map_err(|e| anyhow!("Failed to launch {}: {}", exe.display(), e))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Launch on iOS Simulator: install + launch
fn launch_ios_simulator(
    app_dir: &Path,
    bundle_id: &str,
    udid: &str,
    format: OutputFormat,
) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Installing on simulator {}...", udid);
    }

    let install = Command::new("xcrun")
        .args(["simctl", "install", udid])
        .arg(app_dir)
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun simctl install: {}", e))?;

    if !install.success() {
        return Err(anyhow!("Failed to install app on simulator {}", udid));
    }

    if let OutputFormat::Text = format {
        println!("Launching {}...", bundle_id);
        println!();
    }

    let launch = Command::new("xcrun")
        .args(["simctl", "launch", "--console-pty", udid, bundle_id])
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun simctl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on simulator"));
    }
    Ok(())
}

/// Launch on a physical iOS device via devicectl (Xcode 15+)
fn launch_ios_device(
    app_dir: &Path,
    bundle_id: &str,
    udid: &str,
    format: OutputFormat,
) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Installing on device {}...", udid);
    }

    let install = Command::new("xcrun")
        .args(["devicectl", "device", "install", "app", "--device", udid])
        .arg(app_dir)
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun devicectl install: {}", e))?;

    if !install.success() {
        return Err(anyhow!("Failed to install app on device {}", udid));
    }

    if let OutputFormat::Text = format {
        println!("Launching {}...", bundle_id);
        println!();
    }

    let launch = Command::new("xcrun")
        .args([
            "devicectl",
            "device",
            "process",
            "launch",
            "--device",
            udid,
            bundle_id,
        ])
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun devicectl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on device"));
    }
    Ok(())
}

/// Launch a web build: open HTML in browser
fn launch_web(html_path: &Path, format: OutputFormat) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Opening {} in browser...", html_path.display());
    }

    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "start"
    } else {
        "xdg-open"
    };

    Command::new(cmd)
        .arg(html_path)
        .status()
        .map_err(|e| anyhow!("Failed to open browser: {}", e))?;

    Ok(())
}

// --- Device detection ---

/// Detect booted iOS simulators via `xcrun simctl list`
fn detect_booted_simulators() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "--json"])
        .output()
        .map_err(|e| anyhow!("Failed to run xcrun simctl: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(device_map) = json.get("devices").and_then(|d| d.as_object()) {
        for (_runtime, device_list) in device_map {
            if let Some(arr) = device_list.as_array() {
                for dev in arr {
                    let state = dev.get("state").and_then(|s| s.as_str()).unwrap_or("");
                    if state == "Booted" {
                        if let (Some(udid), Some(name)) = (
                            dev.get("udid").and_then(|s| s.as_str()),
                            dev.get("name").and_then(|s| s.as_str()),
                        ) {
                            devices.push(DeviceInfo {
                                udid: udid.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(devices)
}

/// Detect connected iOS devices via `xcrun devicectl` (Xcode 15+)
fn detect_ios_devices() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["devicectl", "list", "devices", "--json-output", "-"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Ok(Vec::new()),
    };

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(arr) = json
        .get("result")
        .and_then(|r| r.get("devices"))
        .and_then(|d| d.as_array())
    {
        for dev in arr {
            let connected = dev
                .get("connectionProperties")
                .and_then(|c| c.get("transportType"))
                .and_then(|t| t.as_str())
                .is_some();
            if connected {
                if let Some(udid) = dev.get("identifier").and_then(|s| s.as_str()) {
                    let name = dev
                        .get("deviceProperties")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("iOS Device");
                    devices.push(DeviceInfo {
                        udid: udid.to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
    }

    Ok(devices)
}

/// Detect connected Android devices via `adb devices`
fn detect_android_devices() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("adb")
        .args(["devices", "-l"])
        .output()
        .map_err(|_| anyhow!("adb not found. Install Android SDK platform-tools."))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "device" {
            let serial = parts[0].to_string();
            let name = parts
                .iter()
                .find(|p| p.starts_with("model:"))
                .map(|p| p.trim_start_matches("model:").to_string())
                .unwrap_or_else(|| serial.clone());
            devices.push(DeviceInfo {
                udid: serial,
                name,
            });
        }
    }

    Ok(devices)
}

// --- Interactive prompts ---

/// Pick a device from a list using dialoguer, or auto-select if non-interactive
fn pick_device(devices: &[DeviceInfo], label: &str) -> Result<String> {
    let names: Vec<String> = devices
        .iter()
        .map(|d| format!("{} ({})", d.name, d.udid))
        .collect();
    let idx = pick_from_list(&names, &format!("Select {}", label))?;
    Ok(devices[idx].udid.clone())
}

/// Interactive selection from a list of options
fn pick_from_list(items: &[String], prompt: &str) -> Result<usize> {
    if items.is_empty() {
        return Err(anyhow!("No options available"));
    }

    // Non-interactive: pick first
    if !atty::is(atty::Stream::Stdin) {
        eprintln!("Non-interactive terminal, selecting: {}", items[0]);
        return Ok(0);
    }

    let selection = dialoguer::Select::new()
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()
        .map_err(|e| anyhow!("Selection cancelled: {}", e))?;

    Ok(selection)
}
