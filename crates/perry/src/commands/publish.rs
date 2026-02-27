//! Publish command - build, sign, package and distribute via perry-ship build server

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio_tungstenite::tungstenite::Message;
use walkdir::WalkDir;

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// Build for macOS
    #[arg(long)]
    pub macos: bool,

    /// Build for iOS
    #[arg(long)]
    pub ios: bool,

    /// Build for Android
    #[arg(long)]
    pub android: bool,

    /// Build server URL
    #[arg(long, default_value = "http://localhost:3456")]
    pub server: Option<String>,

    /// License key (or set PERRY_LICENSE_KEY env)
    #[arg(long)]
    pub license_key: Option<String>,

    /// Apple Developer Team ID
    #[arg(long)]
    pub apple_team_id: Option<String>,

    /// Apple signing identity (e.g. "Developer ID Application: ..." or "Apple Distribution: ...")
    #[arg(long)]
    pub apple_identity: Option<String>,

    /// Path to App Store Connect .p8 key file
    #[arg(long)]
    pub apple_p8_key: Option<PathBuf>,

    /// App Store Connect API Key ID
    #[arg(long)]
    pub apple_key_id: Option<String>,

    /// App Store Connect Issuer ID
    #[arg(long)]
    pub apple_issuer_id: Option<String>,

    /// Path to iOS provisioning profile (.mobileprovision)
    #[arg(long)]
    pub provisioning_profile: Option<PathBuf>,

    /// Path to Android keystore (.jks/.keystore) for signing
    #[arg(long)]
    pub android_keystore: Option<PathBuf>,

    /// Android keystore password
    #[arg(long)]
    pub android_keystore_password: Option<String>,

    /// Android key alias within keystore
    #[arg(long)]
    pub android_key_alias: Option<String>,

    /// Android key password (defaults to keystore password)
    #[arg(long)]
    pub android_key_password: Option<String>,

    /// Project directory (default: current)
    #[arg(long, default_value = ".")]
    pub project: PathBuf,

    /// Don't download artifact, just build
    #[arg(long)]
    pub no_download: bool,

    /// Output directory for downloaded artifacts
    #[arg(short, long, default_value = "dist")]
    pub output: PathBuf,

    /// Register a new free license (requires --github-token)
    #[arg(long)]
    pub register: bool,

    /// GitHub personal access token (for --register)
    #[arg(long)]
    pub github_token: Option<String>,

    /// GitHub username (for --register)
    #[arg(long)]
    pub github_username: Option<String>,
}

// --- Config types matching perry.toml ---

#[derive(Debug, Deserialize)]
struct PerryToml {
    project: Option<ProjectConfig>,
    app: Option<AppConfig>,
    macos: Option<MacosConfig>,
    ios: Option<IosConfig>,
    android: Option<AndroidConfig>,
    build: Option<BuildConfig>,
    publish: Option<PublishConfig>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    name: Option<String>,
    entry: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppConfig {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    entry: Option<String>,
    icons: Option<IconsConfig>,
}

#[derive(Debug, Deserialize)]
struct IconsConfig {
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MacosConfig {
    bundle_id: Option<String>,
    category: Option<String>,
    minimum_os: Option<String>,
    entitlements: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct IosConfig {
    bundle_id: Option<String>,
    deployment_target: Option<String>,
    device_family: Option<Vec<String>>,
    orientations: Option<Vec<String>>,
    capabilities: Option<Vec<String>>,
    distribute: Option<String>,
    entry: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AndroidConfig {
    package_name: Option<String>,
    min_sdk: Option<String>,
    target_sdk: Option<String>,
    permissions: Option<Vec<String>>,
    distribute: Option<String>,
    entry: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BuildConfig {
    out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PublishConfig {
    server: Option<String>,
}

// --- Server API types ---

#[derive(Debug, Deserialize)]
struct RegisterResponse {
    license_key: String,
    tier: String,
    platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BuildResponse {
    job_id: String,
    ws_url: String,
    position: usize,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    JobCreated {
        job_id: String,
        position: usize,
        estimated_wait_secs: Option<u64>,
    },
    QueueUpdate {
        position: usize,
        estimated_wait_secs: Option<u64>,
    },
    Stage {
        stage: String,
        message: String,
    },
    Log {
        stage: String,
        line: String,
        stream: String,
    },
    Progress {
        stage: String,
        percent: u8,
        message: Option<String>,
    },
    ArtifactReady {
        artifact_name: String,
        artifact_size: u64,
        sha256: String,
        download_url: String,
        expires_in_secs: u64,
    },
    Published {
        platform: String,
        message: String,
        url: Option<String>,
    },
    Error {
        code: String,
        message: String,
        stage: Option<String>,
    },
    Complete {
        job_id: String,
        success: bool,
        duration_secs: f64,
        artifacts: Vec<serde_json::Value>,
    },
}

// --- Manifest sent to the build server ---

#[derive(Debug, Serialize)]
struct BuildManifest {
    app_name: String,
    bundle_id: String,
    version: String,
    short_version: Option<String>,
    entry: String,
    icon: Option<String>,
    targets: Vec<String>,
    category: Option<String>,
    minimum_os_version: Option<String>,
    entitlements: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_device_family: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_orientations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_capabilities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_min_sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_target_sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_permissions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_distribute: Option<String>,
}

#[derive(Debug, Serialize)]
struct CredentialsPayload {
    apple_team_id: Option<String>,
    apple_signing_identity: Option<String>,
    apple_key_id: Option<String>,
    apple_issuer_id: Option<String>,
    apple_p8_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provisioning_profile_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_keystore_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_keystore_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_key_password: Option<String>,
}

pub fn run(args: PublishArgs, format: OutputFormat, use_color: bool, _verbose: u8) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_async(args, format, use_color))
}

async fn run_async(args: PublishArgs, format: OutputFormat, use_color: bool) -> Result<()> {
    let project_dir = args.project.canonicalize().unwrap_or(args.project.clone());

    let server_url = args
        .server
        .clone()
        .unwrap_or_else(|| "http://localhost:3456".into());

    // Handle --register flow
    if args.register {
        return register_license(&args, &server_url, format).await;
    }

    // Need at least one target
    if !args.macos && !args.ios && !args.android {
        bail!("No target specified. Use --macos, --ios, or --android.");
    }

    // Read perry.toml
    let config_path = project_dir.join("perry.toml");
    let config: PerryToml = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .context("Failed to read perry.toml")?;
        toml::from_str(&content)
            .context("Failed to parse perry.toml")?
    } else {
        bail!(
            "No perry.toml found in {}. Run 'perry init' first.",
            project_dir.display()
        );
    };

    // Resolve server URL from config or CLI
    let server_url = args
        .server
        .clone()
        .or_else(|| config.publish.as_ref().and_then(|p| p.server.clone()))
        .unwrap_or_else(|| "http://localhost:3456".into());

    let target_name = if args.android { "android" } else if args.ios { "ios" } else { "macos" };
    let target_display = if args.android { "Android" } else if args.ios { "iOS" } else { "macOS" };

    // Resolve app info
    let app_name = config
        .app
        .as_ref()
        .and_then(|a| a.name.clone())
        .or_else(|| config.project.as_ref().and_then(|p| p.name.clone()))
        .unwrap_or_else(|| "app".into());

    let entry = if args.android {
        config
            .android
            .as_ref()
            .and_then(|a| a.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    } else if args.ios {
        // iOS: prefer [ios].entry, then [app].entry, then default
        config
            .ios
            .as_ref()
            .and_then(|i| i.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_ios.ts".into())
    } else {
        config
            .app
            .as_ref()
            .and_then(|a| a.entry.clone())
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    };

    let version = config
        .app
        .as_ref()
        .and_then(|a| a.version.clone())
        .unwrap_or_else(|| "1.0.0".into());

    let bundle_id = if args.android {
        config
            .android
            .as_ref()
            .and_then(|a| a.package_name.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else if args.ios {
        config
            .ios
            .as_ref()
            .and_then(|i| i.bundle_id.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else {
        config
            .macos
            .as_ref()
            .and_then(|m| m.bundle_id.clone())
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    };

    let icon = config
        .app
        .as_ref()
        .and_then(|a| a.icons.as_ref())
        .and_then(|i| i.source.clone());

    let category = config.macos.as_ref().and_then(|m| m.category.clone());
    let minimum_os = config.macos.as_ref().and_then(|m| m.minimum_os.clone());
    let entitlements = config.macos.as_ref().and_then(|m| m.entitlements.clone());

    // iOS-specific config
    let ios_deployment_target = config.ios.as_ref().and_then(|i| i.deployment_target.clone());
    let ios_device_family = config.ios.as_ref().and_then(|i| i.device_family.clone());
    let ios_orientations = config.ios.as_ref().and_then(|i| i.orientations.clone());
    let ios_capabilities = config.ios.as_ref().and_then(|i| i.capabilities.clone());
    let ios_distribute = config.ios.as_ref().and_then(|i| i.distribute.clone());

    // Android-specific config
    let android_min_sdk = config.android.as_ref().and_then(|a| a.min_sdk.clone());
    let android_target_sdk = config.android.as_ref().and_then(|a| a.target_sdk.clone());
    let android_permissions = config.android.as_ref().and_then(|a| a.permissions.clone());
    let android_distribute = config.android.as_ref().and_then(|a| a.distribute.clone());

    // Resolve license key
    let license_key = args
        .license_key
        .clone()
        .or_else(|| std::env::var("PERRY_LICENSE_KEY").ok())
        .or_else(|| read_saved_license_key());

    let license_key = match license_key {
        Some(k) => k,
        None => {
            bail!(
                "No license key found. Register with:\n  perry publish --register --github-username <user> --github-token <token>\n\nOr set PERRY_LICENSE_KEY environment variable."
            );
        }
    };

    if let OutputFormat::Text = format {
        println!();
        println!(
            "  {} Perry Publish v0.2.162",
            style("▶").cyan().bold()
        );
        println!();
        println!("  App:       {}", style(&app_name).bold());
        println!("  Version:   {version}");
        println!("  Bundle ID: {bundle_id}");
        println!("  Target:    {target_display}");
        println!("  Server:    {server_url}");
        println!();
    }

    // Build manifest
    let manifest = BuildManifest {
        app_name: app_name.clone(),
        bundle_id,
        version,
        short_version: None,
        entry,
        icon: icon.clone(),
        targets: vec![target_name.into()],
        category,
        minimum_os_version: minimum_os,
        entitlements,
        ios_deployment_target: if args.ios { ios_deployment_target } else { None },
        ios_device_family: if args.ios { ios_device_family } else { None },
        ios_orientations: if args.ios { ios_orientations } else { None },
        ios_capabilities: if args.ios { ios_capabilities } else { None },
        ios_distribute: if args.ios { ios_distribute } else { None },
        android_min_sdk: if args.android { android_min_sdk } else { None },
        android_target_sdk: if args.android { android_target_sdk } else { None },
        android_permissions: if args.android { android_permissions } else { None },
        android_distribute: if args.android { android_distribute } else { None },
    };

    // Build credentials
    let p8_key_content = if let Some(ref path) = args.apple_p8_key {
        Some(
            fs::read_to_string(path)
                .with_context(|| format!("Failed to read .p8 key from {}", path.display()))?,
        )
    } else {
        None
    };

    // Read provisioning profile if provided (iOS)
    let provisioning_profile_b64 = if let Some(ref path) = args.provisioning_profile {
        use base64::Engine;
        let data = fs::read(path)
            .with_context(|| format!("Failed to read provisioning profile: {}", path.display()))?;
        Some(base64::engine::general_purpose::STANDARD.encode(&data))
    } else {
        None
    };

    // Read Android keystore if provided
    let android_keystore_b64 = if let Some(ref path) = args.android_keystore {
        use base64::Engine;
        let data = fs::read(path)
            .with_context(|| format!("Failed to read Android keystore: {}", path.display()))?;
        Some(base64::engine::general_purpose::STANDARD.encode(&data))
    } else {
        None
    };

    let credentials = CredentialsPayload {
        apple_team_id: args.apple_team_id.clone(),
        apple_signing_identity: args.apple_identity.clone(),
        apple_key_id: args.apple_key_id.clone(),
        apple_issuer_id: args.apple_issuer_id.clone(),
        apple_p8_key: p8_key_content,
        provisioning_profile_base64: provisioning_profile_b64,
        android_keystore_base64: android_keystore_b64,
        android_keystore_password: args.android_keystore_password.clone(),
        android_key_alias: args.android_key_alias.clone(),
        android_key_password: args.android_key_password.clone(),
    };

    // Create project tarball
    if let OutputFormat::Text = format {
        print!("  Packaging project...");
        std::io::stdout().flush().ok();
    }

    let tarball = create_project_tarball(&project_dir)
        .context("Failed to create project tarball")?;

    let tarball_size = tarball.len();
    if let OutputFormat::Text = format {
        println!(
            " {} ({:.1} MB)",
            style("done").green(),
            tarball_size as f64 / 1_048_576.0
        );
    }

    // Upload to build server
    if let OutputFormat::Text = format {
        print!("  Uploading to build server...");
        std::io::stdout().flush().ok();
    }

    let client = reqwest::Client::new();
    let form = multipart::Form::new()
        .text("license_key", license_key)
        .text("manifest", serde_json::to_string(&manifest)?)
        .text("credentials", serde_json::to_string(&credentials)?)
        .part(
            "project",
            multipart::Part::bytes(tarball)
                .file_name("project.tar.gz")
                .mime_str("application/gzip")?,
        );

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

    let build_resp: BuildResponse = resp.json().await.context("Invalid build response")?;

    if let OutputFormat::Text = format {
        println!(" {}", style("done").green());
        println!(
            "  Job ID:    {}",
            style(&build_resp.job_id).dim()
        );
        println!(
            "  Position:  {}",
            build_resp.position
        );
        println!();
    }

    // Connect WebSocket for progress
    // Hub returns either an absolute WS URL (ws://host:port) or a relative path
    let ws_url = if build_resp.ws_url.starts_with("ws://") || build_resp.ws_url.starts_with("wss://") {
        // Absolute URL from hub
        build_resp.ws_url.clone()
    } else if server_url.starts_with("https://") {
        format!(
            "wss://{}{}",
            &server_url["https://".len()..],
            build_resp.ws_url
        )
    } else {
        format!(
            "ws://{}{}",
            &server_url["http://".len()..],
            build_resp.ws_url
        )
    };

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("Failed to connect WebSocket")?;

    let (mut ws_write, mut read) = ws_stream.split();

    // Send subscribe message to identify as CLI client for this job
    use futures_util::SinkExt;
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

    let mut download_url: Option<String> = None;
    let mut artifact_name: Option<String> = None;
    let mut build_success = false;

    use futures_util::StreamExt;
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

        let server_msg: ServerMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match server_msg {
            ServerMessage::JobCreated { .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message("Build started");
                }
            }
            ServerMessage::QueueUpdate { position, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message(format!("Queue position: {position}"));
                }
            }
            ServerMessage::Stage { stage, message } => {
                if let Some(ref pb) = pb {
                    let icon = match stage.as_str() {
                        "extracting" => "📦",
                        "compiling" => "⚙️ ",
                        "generating_assets" => "🎨",
                        "bundling" => "📁",
                        "signing" => "🔏",
                        "notarizing" => "🍎",
                        "packaging" => "💿",
                        "uploading" => "☁️ ",
                        _ => "▶️ ",
                    };
                    pb.set_message(format!("{icon} {message}"));
                }
            }
            ServerMessage::Log { line, stream, .. } => {
                if let Some(ref pb) = pb {
                    if stream == "stderr" {
                        pb.println(format!("    {}", style(&line).dim()));
                    }
                }
            }
            ServerMessage::Progress { percent, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_position(percent as u64);
                }
            }
            ServerMessage::ArtifactReady {
                artifact_name: name,
                artifact_size,
                sha256,
                download_url: url,
                ..
            } => {
                if let Some(ref pb) = pb {
                    pb.set_position(100);
                    pb.finish_with_message(format!(
                        "{} Artifact ready: {} ({:.1} MB)",
                        style("✓").green().bold(),
                        name,
                        artifact_size as f64 / 1_048_576.0
                    ));
                }
                download_url = Some(url);
                artifact_name = Some(name);

                if let OutputFormat::Text = format {
                    println!("  SHA-256:   {}", style(&sha256).dim());
                }
            }
            ServerMessage::Error {
                code,
                message,
                stage,
            } => {
                if let Some(ref pb) = pb {
                    pb.abandon_with_message(format!(
                        "{} {} ({})",
                        style("✗").red().bold(),
                        message,
                        code
                    ));
                }
                bail!("Build error [{}]: {}", code, message);
            }
            ServerMessage::Complete {
                success,
                duration_secs,
                ..
            } => {
                build_success = success;
                if let OutputFormat::Text = format {
                    println!();
                    if success {
                        println!(
                            "  {} Build completed in {:.1}s",
                            style("✓").green().bold(),
                            duration_secs
                        );
                    } else {
                        println!(
                            "  {} Build failed after {:.1}s",
                            style("✗").red().bold(),
                            duration_secs
                        );
                    }
                }
                break;
            }
            _ => {}
        }
    }

    // Download artifact
    if build_success && !args.no_download {
        if let (Some(url), Some(name)) = (download_url, artifact_name) {
            let full_url = format!("{server_url}{url}");
            if let OutputFormat::Text = format {
                print!("  Downloading {name}...");
                std::io::stdout().flush().ok();
            }

            fs::create_dir_all(&args.output)?;
            let dest = args.output.join(&name);

            let resp = client
                .get(&full_url)
                .send()
                .await
                .context("Failed to download artifact")?;

            if !resp.status().is_success() {
                bail!(
                    "Download failed: {}",
                    resp.status()
                );
            }

            let bytes = resp.bytes().await?;
            fs::write(&dest, &bytes)?;

            if let OutputFormat::Text = format {
                println!(
                    " {} → {}",
                    style("done").green(),
                    style(dest.display()).bold()
                );
                println!();
                println!(
                    "  {} {}",
                    style("Ready!").green().bold(),
                    style(format!("Open with: open {}", dest.display())).dim()
                );
                println!();
            }
        }
    }

    if !build_success {
        bail!("Build failed");
    }

    Ok(())
}

async fn register_license(
    args: &PublishArgs,
    server_url: &str,
    format: OutputFormat,
) -> Result<()> {
    let username = args
        .github_username
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--github-username is required for --register"))?;
    let token = args
        .github_token
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--github-token is required for --register"))?;

    if let OutputFormat::Text = format {
        println!();
        println!(
            "  {} Registering license for @{username}...",
            style("▶").cyan().bold()
        );
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server_url}/api/v1/license/register"))
        .json(&serde_json::json!({
            "github_username": username,
            "github_token": token,
        }))
        .send()
        .await
        .context("Failed to connect to build server")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Registration failed ({status}): {body}");
    }

    let reg: RegisterResponse = resp.json().await?;

    // Save license key
    save_license_key(&reg.license_key)?;

    match format {
        OutputFormat::Text => {
            println!();
            println!(
                "  {} Licensed to @{username} ({} tier)",
                style("✓").green().bold(),
                reg.tier,
            );
            println!(
                "  License key: {}",
                style(&reg.license_key).bold()
            );
            println!(
                "  Platforms:   {}",
                reg.platforms.join(", ")
            );
            println!();
            println!(
                "  Saved to {}",
                style(license_config_path().display()).dim()
            );
            println!();
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "license_key": reg.license_key,
                    "tier": reg.tier,
                    "platforms": reg.platforms,
                }))?
            );
        }
    }

    Ok(())
}

fn create_project_tarball(project_dir: &Path) -> Result<Vec<u8>> {
    let buf = Vec::new();
    let encoder = GzEncoder::new(buf, Compression::default());
    let mut ar = tar::Builder::new(encoder);

    let exclude_dirs = [
        "node_modules",
        ".git",
        "dist",
        "build",
        "target",
        ".perry",
        "xcode",
    ];

    let exclude_extensions = [
        "o", "a", "dylib", "so", "dll", "exe", "dmg", "ipa", "apk", "aab",
    ];

    for entry in WalkDir::new(project_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Exclude known directory names
            if e.file_type().is_dir() {
                if exclude_dirs.iter().any(|ex| name == *ex) {
                    return false;
                }
                // Exclude .app bundles
                if name.ends_with(".app") {
                    return false;
                }
            }
            true
        })
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(project_dir)?;

        if relative.as_os_str().is_empty() {
            continue;
        }

        if path.is_file() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();

            // Skip build artifacts by extension
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if exclude_extensions.contains(&ext) {
                    continue;
                }
            }

            // Skip files that start with _ and are large (e.g. _perry_ui_stripped.a)
            if name.starts_with('_') && path.metadata().map(|m| m.len() > 1_000_000).unwrap_or(false) {
                continue;
            }

            // Skip executables without extension that are large (compiled binaries)
            if path.extension().is_none() && path.metadata().map(|m| m.len() > 1_000_000).unwrap_or(false) {
                continue;
            }

            // Skip .DS_Store
            if name == ".DS_Store" {
                continue;
            }

            ar.append_path_with_name(path, relative)?;
        } else if path.is_dir() {
            ar.append_dir(relative, path)?;
        }
    }

    ar.finish()?;
    let encoder = ar.into_inner()?;
    Ok(encoder.finish()?)
}

fn license_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".perry")
        .join("config.toml")
}

fn save_license_key(key: &str) -> Result<()> {
    let config_path = license_config_path();
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read existing config or create new
    let mut content = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    // Simple TOML append/replace for license_key
    if content.contains("license_key") {
        // Replace existing
        let lines: Vec<&str> = content.lines().collect();
        let new_lines: Vec<String> = lines
            .iter()
            .map(|l| {
                if l.trim_start().starts_with("license_key") {
                    format!("license_key = \"{}\"", key)
                } else {
                    l.to_string()
                }
            })
            .collect();
        content = new_lines.join("\n");
    } else {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&format!("license_key = \"{}\"\n", key));
    }

    fs::write(&config_path, content)?;
    Ok(())
}

fn read_saved_license_key() -> Option<String> {
    let config_path = license_config_path();
    let content = fs::read_to_string(&config_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("license_key") {
            if let Some(val) = trimmed.split('=').nth(1) {
                return Some(val.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}
