//! Publish command - build, sign, package and distribute via perry-ship build server

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use dialoguer::{Confirm, Input, Select};
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

    /// Build for Linux
    #[arg(long)]
    pub linux: bool,

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

    /// Path to Google Play service account JSON key file
    #[arg(long)]
    pub google_play_key: Option<PathBuf>,

    /// Project directory (default: current)
    #[arg(long, default_value = ".")]
    pub project: PathBuf,

    /// Don't download artifact, just build
    #[arg(long)]
    pub no_download: bool,

    /// Output directory for downloaded artifacts
    #[arg(short, long, default_value = "dist")]
    pub output: PathBuf,

}

// --- Config types matching perry.toml ---

#[derive(Debug, Deserialize)]
struct PerryToml {
    project: Option<ProjectConfig>,
    app: Option<AppConfig>,
    macos: Option<MacosConfig>,
    ios: Option<IosConfig>,
    android: Option<AndroidConfig>,
    linux: Option<LinuxConfig>,
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
struct LinuxConfig {
    format: Option<String>,
    category: Option<String>,
    description: Option<String>,
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
        #[serde(default)]
        download_path: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_description: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    google_play_service_account_json: Option<String>,
}

pub fn run(args: PublishArgs, format: OutputFormat, use_color: bool, _verbose: u8) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_async(args, format, use_color))
}

async fn run_async(args: PublishArgs, format: OutputFormat, use_color: bool) -> Result<()> {
    let project_dir = args.project.canonicalize().unwrap_or(args.project.clone());

    // Load saved config
    let mut saved = load_config();
    let interactive = is_interactive() && matches!(format, OutputFormat::Text);

    // Read perry.toml
    let perry_toml_path = project_dir.join("perry.toml");
    let config: PerryToml = if perry_toml_path.exists() {
        let content = fs::read_to_string(&perry_toml_path)
            .context("Failed to read perry.toml")?;
        toml::from_str(&content)
            .context("Failed to parse perry.toml")?
    } else {
        bail!(
            "No perry.toml found in {}. Run 'perry init' first.",
            project_dir.display()
        );
    };

    // Resolve app info (always from perry.toml)
    let app_name = config
        .app
        .as_ref()
        .and_then(|a| a.name.clone())
        .or_else(|| config.project.as_ref().and_then(|p| p.name.clone()))
        .unwrap_or_else(|| "app".into());

    let toml_version = config
        .app
        .as_ref()
        .and_then(|a| a.version.clone())
        .unwrap_or_else(|| "1.0.0".into());

    if let OutputFormat::Text = format {
        println!();
        println!(
            "  {} Perry Publish v0.2.162",
            style("▶").cyan().bold()
        );
        println!();
        println!("  App:       {}", style(&app_name).bold());
    }

    // --- Resolve target platform ---
    let target_name = if args.macos {
        "macos".to_string()
    } else if args.ios {
        "ios".to_string()
    } else if args.android {
        "android".to_string()
    } else if args.linux {
        "linux".to_string()
    } else if let Some(ref t) = saved.default_target {
        // Have a saved default — use it (user can change via prompt below)
        t.clone()
    } else if interactive {
        prompt_target(saved.default_target.as_deref())
    } else {
        bail!("No target specified. Use --macos, --ios, --android, or --linux.");
    };

    let target_display = match target_name.as_str() {
        "ios" => "iOS",
        "android" => "Android",
        "linux" => "Linux",
        _ => "macOS",
    };
    let is_ios = target_name == "ios";
    let is_android = target_name == "android";
    let is_linux = target_name == "linux";

    // --- Resolve server URL ---
    let server_url = args
        .server
        .clone()
        .or_else(|| saved.server.clone())
        .or_else(|| config.publish.as_ref().and_then(|p| p.server.clone()))
        .unwrap_or_else(|| "http://localhost:3456".into());

    // --- Resolve entry point ---
    let entry = if is_android {
        config.android.as_ref().and_then(|a| a.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    } else if is_ios {
        config.ios.as_ref().and_then(|i| i.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_ios.ts".into())
    } else {
        config.app.as_ref().and_then(|a| a.entry.clone())
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    };

    // --- Resolve version (allow override) ---
    let version = if interactive {
        let v = prompt_input(
            &format!("  Version [{}]", toml_version),
            Some(&toml_version),
        );
        v.unwrap_or(toml_version.clone())
    } else {
        toml_version.clone()
    };

    // Update perry.toml if version changed
    if version != toml_version {
        if let Ok(content) = fs::read_to_string(&perry_toml_path) {
            let updated = content.replace(
                &format!("version = \"{}\"", toml_version),
                &format!("version = \"{}\"", version),
            );
            if updated != content {
                fs::write(&perry_toml_path, &updated).ok();
            }
        }
    }

    let bundle_id = if is_android {
        config.android.as_ref().and_then(|a| a.package_name.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else if is_ios {
        config.ios.as_ref().and_then(|i| i.bundle_id.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else {
        config.macos.as_ref().and_then(|m| m.bundle_id.clone())
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    };

    let icon = config.app.as_ref().and_then(|a| a.icons.as_ref()).and_then(|i| i.source.clone());
    let category = config.macos.as_ref().and_then(|m| m.category.clone());
    let minimum_os = config.macos.as_ref().and_then(|m| m.minimum_os.clone());
    let entitlements = config.macos.as_ref().and_then(|m| m.entitlements.clone());

    // iOS-specific config from perry.toml
    let ios_deployment_target = config.ios.as_ref().and_then(|i| i.deployment_target.clone());
    let ios_device_family = config.ios.as_ref().and_then(|i| i.device_family.clone());
    let ios_orientations = config.ios.as_ref().and_then(|i| i.orientations.clone());
    let ios_capabilities = config.ios.as_ref().and_then(|i| i.capabilities.clone());
    let ios_distribute = config.ios.as_ref().and_then(|i| i.distribute.clone());

    // Android-specific config from perry.toml
    let android_min_sdk = config.android.as_ref().and_then(|a| a.min_sdk.clone());
    let android_target_sdk = config.android.as_ref().and_then(|a| a.target_sdk.clone());
    let android_permissions = config.android.as_ref().and_then(|a| a.permissions.clone());
    let android_distribute = config.android.as_ref().and_then(|a| a.distribute.clone());

    // --- Resolve license key ---
    let license_key = args
        .license_key
        .clone()
        .or_else(|| std::env::var("PERRY_LICENSE_KEY").ok())
        .or_else(|| saved.license_key.clone());

    let license_key = match license_key {
        Some(k) => k,
        None => {
            // Auto-register a free license
            if let OutputFormat::Text = format {
                print!("  No license key found. Registering free license...");
                std::io::stdout().flush().ok();
            }
            let key = auto_register_license(&server_url).await?;
            if let OutputFormat::Text = format {
                println!(" {}", style("done").green());
                println!("  {} License: {}", style("✓").green().bold(), style(&key).bold());
            }
            // Save immediately
            saved.license_key = Some(key.clone());
            save_config(&saved).ok();
            key
        }
    };

    // --- Resolve credentials using CLI → env → saved config → interactive prompt ---

    // Apple credentials (for macOS and iOS)
    let apple_team_id = if !is_android {
        resolve_credential(
            args.apple_team_id.as_deref(),
            "PERRY_APPLE_TEAM_ID",
            saved.apple.as_ref().and_then(|a| a.team_id.as_deref()),
            "  Apple Team ID",
            false,
            interactive,
        )
    } else {
        args.apple_team_id.clone()
    };

    let apple_identity = if !is_android {
        resolve_credential(
            args.apple_identity.as_deref(),
            "PERRY_APPLE_IDENTITY",
            saved.apple.as_ref().and_then(|a| a.signing_identity.as_deref()),
            "  Signing Identity",
            false,
            interactive,
        )
    } else {
        args.apple_identity.clone()
    };

    let apple_p8_key_path = if !is_android {
        resolve_path_credential(
            args.apple_p8_key.as_deref(),
            "PERRY_APPLE_P8_KEY",
            saved.apple.as_ref().and_then(|a| a.p8_key_path.as_deref()),
            "  App Store Connect .p8 key path",
            interactive,
        )
    } else {
        args.apple_p8_key.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    let apple_key_id = if !is_android {
        resolve_credential(
            args.apple_key_id.as_deref(),
            "PERRY_APPLE_KEY_ID",
            saved.apple.as_ref().and_then(|a| a.key_id.as_deref()),
            "  App Store Connect Key ID",
            false,
            interactive,
        )
    } else {
        args.apple_key_id.clone()
    };

    let apple_issuer_id = if !is_android {
        resolve_credential(
            args.apple_issuer_id.as_deref(),
            "PERRY_APPLE_ISSUER_ID",
            saved.apple.as_ref().and_then(|a| a.issuer_id.as_deref()),
            "  App Store Connect Issuer ID",
            false,
            interactive,
        )
    } else {
        args.apple_issuer_id.clone()
    };

    // iOS provisioning profile
    let provisioning_profile_path = if is_ios {
        resolve_path_credential(
            args.provisioning_profile.as_deref(),
            "PERRY_PROVISIONING_PROFILE",
            saved.ios.as_ref().and_then(|i| i.provisioning_profile_path.as_deref()),
            "  Provisioning profile path",
            interactive,
        )
    } else {
        args.provisioning_profile.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    // Android credentials
    let android_keystore_path = if is_android {
        resolve_path_credential(
            args.android_keystore.as_deref(),
            "PERRY_ANDROID_KEYSTORE",
            saved.android.as_ref().and_then(|a| a.keystore_path.as_deref()),
            "  Android keystore path",
            interactive,
        )
    } else {
        args.android_keystore.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    let android_key_alias = if is_android {
        resolve_credential(
            args.android_key_alias.as_deref(),
            "PERRY_ANDROID_KEY_ALIAS",
            saved.android.as_ref().and_then(|a| a.key_alias.as_deref()),
            "  Android key alias",
            false,
            interactive,
        )
    } else {
        args.android_key_alias.clone()
    };

    // Passwords are NEVER saved — always from CLI, env, or prompt
    let android_keystore_password = args
        .android_keystore_password
        .clone()
        .or_else(|| std::env::var("PERRY_ANDROID_KEYSTORE_PASSWORD").ok());
    let android_keystore_password = if android_keystore_password.is_none() && is_android && android_keystore_path.is_some() && interactive {
        prompt_input("  Android keystore password", None)
    } else {
        android_keystore_password
    };

    let android_key_password = args
        .android_key_password
        .clone()
        .or_else(|| std::env::var("PERRY_ANDROID_KEY_PASSWORD").ok());

    // Google Play service account JSON
    let google_play_key_path = if is_android {
        resolve_path_credential(
            args.google_play_key.as_deref(),
            "PERRY_GOOGLE_PLAY_KEY_PATH",
            saved.android.as_ref().and_then(|a| a.google_play_key_path.as_deref()),
            "  Google Play service account JSON path",
            interactive && android_distribute.as_deref() == Some("playstore"),
        )
    } else {
        args.google_play_key.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    // Read file contents for credentials that need to be sent as content
    let p8_key_content = if let Some(ref path_str) = apple_p8_key_path {
        let path = Path::new(path_str);
        if path.exists() {
            Some(fs::read_to_string(path)
                .with_context(|| format!("Failed to read .p8 key from {path_str}"))?)
        } else {
            None
        }
    } else {
        None
    };

    let provisioning_profile_b64 = if let Some(ref path_str) = provisioning_profile_path {
        let path = Path::new(path_str);
        if path.exists() {
            use base64::Engine;
            let data = fs::read(path)
                .with_context(|| format!("Failed to read provisioning profile: {path_str}"))?;
            Some(base64::engine::general_purpose::STANDARD.encode(&data))
        } else {
            None
        }
    } else {
        None
    };

    let android_keystore_b64 = if let Some(ref path_str) = android_keystore_path {
        let path = Path::new(path_str);
        if path.exists() {
            use base64::Engine;
            let data = fs::read(path)
                .with_context(|| format!("Failed to read Android keystore: {path_str}"))?;
            Some(base64::engine::general_purpose::STANDARD.encode(&data))
        } else {
            None
        }
    } else {
        None
    };

    let google_play_json = if let Some(ref path_str) = google_play_key_path {
        let path = Path::new(path_str);
        if path.exists() {
            Some(fs::read_to_string(path)
                .with_context(|| format!("Failed to read Google Play key: {path_str}"))?)
        } else {
            None
        }
    } else {
        None
    };

    // --- Show summary and confirm ---
    if let OutputFormat::Text = format {
        println!("  Version:   {version}");
        println!("  Bundle ID: {bundle_id}");
        println!("  Target:    {target_display}");
        println!("  Server:    {server_url}");
        if let Some(ref id) = apple_identity {
            println!("  Signing:   {id}");
        }
        if android_distribute.as_deref() == Some("playstore") {
            println!("  Distribute: Google Play");
        } else if ios_distribute.as_deref() == Some("appstore") || ios_distribute.as_deref() == Some("testflight") {
            println!("  Distribute: App Store Connect");
        }
        println!();
    }

    if interactive {
        let confirm = Confirm::new()
            .with_prompt("  Confirm and publish?")
            .default(true)
            .interact()
            .unwrap_or(false);
        if !confirm {
            bail!("Publish cancelled.");
        }
        println!();
    }

    // --- Save non-sensitive config ---
    saved.license_key = Some(license_key.clone());
    saved.default_target = Some(target_name.clone());
    if server_url != "http://localhost:3456" {
        saved.server = Some(server_url.clone());
    }
    if !is_android {
        let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
        if apple_team_id.is_some() { apple.team_id = apple_team_id.clone(); }
        if apple_identity.is_some() { apple.signing_identity = apple_identity.clone(); }
        if apple_p8_key_path.is_some() { apple.p8_key_path = apple_p8_key_path.clone(); }
        if apple_key_id.is_some() { apple.key_id = apple_key_id.clone(); }
        if apple_issuer_id.is_some() { apple.issuer_id = apple_issuer_id.clone(); }
    }
    if is_ios {
        let ios_saved = saved.ios.get_or_insert_with(IosSavedConfig::default);
        if provisioning_profile_path.is_some() { ios_saved.provisioning_profile_path = provisioning_profile_path.clone(); }
    }
    if is_android {
        let android_saved = saved.android.get_or_insert_with(AndroidSavedConfig::default);
        if android_keystore_path.is_some() { android_saved.keystore_path = android_keystore_path.clone(); }
        if android_key_alias.is_some() { android_saved.key_alias = android_key_alias.clone(); }
        if google_play_key_path.is_some() { android_saved.google_play_key_path = google_play_key_path.clone(); }
    }
    if let Err(e) = save_config(&saved) {
        if let OutputFormat::Text = format {
            println!("  {} Could not save config: {e}", style("!").yellow());
        }
    } else if interactive {
        println!("  Saved settings to {}", style(config_path().display()).dim());
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
        targets: vec![target_name.clone()],
        category,
        minimum_os_version: minimum_os,
        entitlements,
        ios_deployment_target: if is_ios { ios_deployment_target } else { None },
        ios_device_family: if is_ios { ios_device_family } else { None },
        ios_orientations: if is_ios { ios_orientations } else { None },
        ios_capabilities: if is_ios { ios_capabilities } else { None },
        ios_distribute: if is_ios { ios_distribute } else { None },
        android_min_sdk: if is_android { android_min_sdk } else { None },
        android_target_sdk: if is_android { android_target_sdk } else { None },
        android_permissions: if is_android { android_permissions } else { None },
        android_distribute: if is_android { android_distribute } else { None },
        linux_format: if is_linux { config.linux.as_ref().and_then(|l| l.format.clone()) } else { None },
        linux_category: if is_linux { config.linux.as_ref().and_then(|l| l.category.clone()) } else { None },
        linux_description: if is_linux {
            config.linux.as_ref().and_then(|l| l.description.clone())
                .or_else(|| config.app.as_ref().and_then(|a| a.description.clone()))
        } else { None },
    };

    let credentials = CredentialsPayload {
        apple_team_id,
        apple_signing_identity: apple_identity,
        apple_key_id,
        apple_issuer_id,
        apple_p8_key: p8_key_content,
        provisioning_profile_base64: provisioning_profile_b64,
        android_keystore_base64: android_keystore_b64,
        android_keystore_password,
        android_key_alias,
        android_key_password,
        google_play_service_account_json: google_play_json,
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

    // Write tarball to temp file and send path (avoids binary corruption in hub's text-based body parsing)
    let upload_id = format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let tarball_tmp_dir = std::env::temp_dir().join("perry-uploads");
    std::fs::create_dir_all(&tarball_tmp_dir)
        .context("Failed to create upload temp directory")?;
    let tarball_tmp_path = tarball_tmp_dir.join(format!("{upload_id}.tar.gz"));
    std::fs::write(&tarball_tmp_path, &tarball)
        .context("Failed to write tarball to temp file")?;

    let client = reqwest::Client::new();
    let form = multipart::Form::new()
        .text("license_key", license_key)
        .text("manifest", serde_json::to_string(&manifest)?)
        .text("credentials", serde_json::to_string(&credentials)?)
        .text("project_path", tarball_tmp_path.to_string_lossy().to_string());

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
    let mut download_path: Option<String> = None;
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
            Err(e) => {
                // Unknown message type from hub, skip it
                continue;
            }
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
                download_path: dl_path,
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
                download_path = dl_path;
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
            if let OutputFormat::Text = format {
                print!("  Downloading {name}...");
                std::io::stdout().flush().ok();
            }

            fs::create_dir_all(&args.output)?;
            let dest = args.output.join(&name);

            if let Some(ref src_path) = download_path {
                // Local path available (self-hosted hub) - copy directly
                fs::copy(src_path, &dest)
                    .with_context(|| format!("Failed to copy artifact from {src_path}"))?;
            } else {
                // Remote hub - download via HTTP
                let full_url = format!("{server_url}{url}");
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
            }

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

async fn auto_register_license(server_url: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server_url}/api/v1/license/register"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .context("Failed to register license")?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("License registration failed: {body}");
    }
    let reg: RegisterResponse = resp.json().await?;
    Ok(reg.license_key)
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

// --- Saved config (~/.perry/config.toml) ---

#[derive(Default, Debug, Serialize, Deserialize)]
struct PerryConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    license_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apple: Option<AppleSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios: Option<IosSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android: Option<AndroidSavedConfig>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct AppleSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    p8_key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issuer_id: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct IosSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    provisioning_profile_path: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct AndroidSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    keystore_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_play_key_path: Option<String>,
}

fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".perry")
        .join("config.toml")
}

fn load_config() -> PerryConfig {
    let path = config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        toml::from_str(&content).unwrap_or_default()
    } else {
        PerryConfig::default()
    }
}

fn save_config(config: &PerryConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)
        .context("Failed to serialize config")?;
    fs::write(&path, content)?;
    Ok(())
}

fn is_interactive() -> bool {
    atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout)
}

/// Prompt user for text input with an optional default value.
/// Returns None if the user enters empty string.
fn prompt_input(prompt: &str, default: Option<&str>) -> Option<String> {
    let mut builder = Input::<String>::new().with_prompt(prompt);
    if let Some(d) = default {
        builder = builder.default(d.to_string());
    }
    builder = builder.allow_empty(true);
    match builder.interact_text() {
        Ok(val) if val.is_empty() => None,
        Ok(val) => Some(val),
        Err(_) => None,
    }
}

/// Prompt for target platform selection. Returns "macos", "ios", "android", or "linux".
fn prompt_target(default: Option<&str>) -> String {
    let options = &["macOS", "iOS", "Android", "Linux"];
    let default_idx = match default {
        Some("ios") => 1,
        Some("android") => 2,
        Some("linux") => 3,
        _ => 0,
    };
    let selection = Select::new()
        .with_prompt("Target platform")
        .items(options)
        .default(default_idx)
        .interact()
        .unwrap_or(0);
    match selection {
        1 => "ios".into(),
        2 => "android".into(),
        3 => "linux".into(),
        _ => "macos".into(),
    }
}

/// Resolve a credential value using priority: CLI flag → env var → saved config → interactive prompt.
/// Returns None only if the field is optional and the user skips it.
fn resolve_credential(
    cli_value: Option<&str>,
    env_var: &str,
    saved_value: Option<&str>,
    prompt_label: &str,
    required: bool,
    interactive: bool,
) -> Option<String> {
    // 1. CLI flag
    if let Some(v) = cli_value {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    // 2. Environment variable
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    // 3. Saved config
    if let Some(v) = saved_value {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    // 4. Interactive prompt
    if interactive {
        let val = prompt_input(prompt_label, saved_value);
        if val.is_some() {
            return val;
        }
        if required {
            // Re-prompt once if required
            return prompt_input(&format!("{prompt_label} (required)"), None);
        }
    }
    None
}

/// Resolve a file path credential: CLI → env → saved config → interactive prompt.
/// Returns the path string (not validated here).
fn resolve_path_credential(
    cli_value: Option<&Path>,
    env_var: &str,
    saved_value: Option<&str>,
    prompt_label: &str,
    interactive: bool,
) -> Option<String> {
    if let Some(v) = cli_value {
        return Some(v.to_string_lossy().to_string());
    }
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    if let Some(v) = saved_value {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    if interactive {
        return prompt_input(prompt_label, saved_value);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perry_config_roundtrip() {
        let config = PerryConfig {
            license_key: Some("FREE-abc123".into()),
            server: Some("https://build.example.com".into()),
            default_target: Some("macos".into()),
            apple: Some(AppleSavedConfig {
                team_id: Some("ABC123DEF".into()),
                signing_identity: Some("Developer ID Application: Test (ABC123DEF)".into()),
                p8_key_path: Some("/Users/me/AuthKey_XXX.p8".into()),
                key_id: Some("XXX".into()),
                issuer_id: Some("abc-def-ghi".into()),
            }),
            ios: Some(IosSavedConfig {
                provisioning_profile_path: Some("/Users/me/profile.mobileprovision".into()),
            }),
            android: Some(AndroidSavedConfig {
                keystore_path: Some("/Users/me/release.keystore".into()),
                key_alias: Some("key0".into()),
                google_play_key_path: Some("/Users/me/play-sa.json".into()),
            }),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: PerryConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.license_key, config.license_key);
        assert_eq!(parsed.server, config.server);
        assert_eq!(parsed.default_target, config.default_target);
        assert_eq!(parsed.apple.as_ref().unwrap().team_id, config.apple.as_ref().unwrap().team_id);
        assert_eq!(parsed.apple.as_ref().unwrap().signing_identity, config.apple.as_ref().unwrap().signing_identity);
        assert_eq!(parsed.android.as_ref().unwrap().google_play_key_path, config.android.as_ref().unwrap().google_play_key_path);
    }

    #[test]
    fn test_perry_config_minimal() {
        let config = PerryConfig {
            license_key: Some("FREE-test".into()),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("license_key"));
        assert!(!toml_str.contains("[apple]"), "empty sections should be omitted");
        assert!(!toml_str.contains("[android]"));
    }

    #[test]
    fn test_perry_config_parse_legacy_format() {
        // Old format was just license_key = "..." — should still parse
        let legacy = r#"license_key = "FREE-legacy-key""#;
        let config: PerryConfig = toml::from_str(legacy).unwrap();
        assert_eq!(config.license_key.as_deref(), Some("FREE-legacy-key"));
        assert!(config.apple.is_none());
        assert!(config.default_target.is_none());
    }

    #[test]
    fn test_perry_config_no_passwords_in_toml() {
        let config = PerryConfig {
            license_key: Some("FREE-test".into()),
            android: Some(AndroidSavedConfig {
                keystore_path: Some("/path/to/keystore".into()),
                key_alias: Some("key0".into()),
                google_play_key_path: None,
            }),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        // Config struct intentionally has no password fields
        assert!(!toml_str.contains("password"));
        assert!(toml_str.contains("keystore_path"));
    }

    #[test]
    fn test_perry_config_default_is_empty() {
        let config = PerryConfig::default();
        assert!(config.license_key.is_none());
        assert!(config.server.is_none());
        assert!(config.default_target.is_none());
        assert!(config.apple.is_none());
        assert!(config.ios.is_none());
        assert!(config.android.is_none());
    }

    #[test]
    fn test_credentials_payload_with_google_play() {
        let creds = CredentialsPayload {
            apple_team_id: None,
            apple_signing_identity: None,
            apple_key_id: None,
            apple_issuer_id: None,
            apple_p8_key: None,
            provisioning_profile_base64: None,
            android_keystore_base64: Some("dGVzdA==".into()),
            android_keystore_password: Some("pass".into()),
            android_key_alias: Some("key0".into()),
            android_key_password: None,
            google_play_service_account_json: Some("{\"client_email\":\"test@gcp\"}".into()),
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert!(json.contains("google_play_service_account_json"));
        assert!(json.contains("client_email"));
    }

    #[test]
    fn test_credentials_payload_omits_none() {
        let creds = CredentialsPayload {
            apple_team_id: Some("ABC".into()),
            apple_signing_identity: Some("Dev ID".into()),
            apple_key_id: None,
            apple_issuer_id: None,
            apple_p8_key: None,
            provisioning_profile_base64: None,
            android_keystore_base64: None,
            android_keystore_password: None,
            android_key_alias: None,
            android_key_password: None,
            google_play_service_account_json: None,
        };
        let json = serde_json::to_string(&creds).unwrap();
        // Fields with skip_serializing_if should be absent
        assert!(!json.contains("android_keystore_base64"));
        assert!(!json.contains("google_play_service_account_json"));
        // Non-skip fields are always present (even as null)
        assert!(json.contains("apple_team_id"));
    }

    #[test]
    fn test_resolve_credential_cli_wins() {
        let result = resolve_credential(
            Some("from-cli"),
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("from-saved"),
            "test",
            false,
            false, // not interactive
        );
        assert_eq!(result.as_deref(), Some("from-cli"));
    }

    #[test]
    fn test_resolve_credential_saved_fallback() {
        let result = resolve_credential(
            None,
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("from-saved"),
            "test",
            false,
            false,
        );
        assert_eq!(result.as_deref(), Some("from-saved"));
    }

    #[test]
    fn test_resolve_credential_none_when_missing() {
        let result = resolve_credential(
            None,
            "NONEXISTENT_ENV_VAR_XYZ",
            None,
            "test",
            false,
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_credential_skips_empty() {
        let result = resolve_credential(
            Some(""),
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("saved"),
            "test",
            false,
            false,
        );
        assert_eq!(result.as_deref(), Some("saved"));
    }

    #[test]
    fn test_resolve_path_credential() {
        let result = resolve_path_credential(
            Some(Path::new("/path/to/file")),
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("/saved/path"),
            "test",
            false,
        );
        assert_eq!(result.as_deref(), Some("/path/to/file"));
    }

    #[test]
    fn test_resolve_path_credential_saved_fallback() {
        let result = resolve_path_credential(
            None,
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("/saved/path"),
            "test",
            false,
        );
        assert_eq!(result.as_deref(), Some("/saved/path"));
    }

    #[test]
    fn test_config_file_write_and_read() {
        // Test writing to a temp location and reading back
        let dir = std::env::temp_dir().join("perry-test-config");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("config.toml");

        let config = PerryConfig {
            license_key: Some("TEST-KEY-123".into()),
            default_target: Some("ios".into()),
            apple: Some(AppleSavedConfig {
                team_id: Some("TEAM123".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        let read_back = fs::read_to_string(&path).unwrap();
        let parsed: PerryConfig = toml::from_str(&read_back).unwrap();
        assert_eq!(parsed.license_key.as_deref(), Some("TEST-KEY-123"));
        assert_eq!(parsed.default_target.as_deref(), Some("ios"));
        assert_eq!(parsed.apple.unwrap().team_id.as_deref(), Some("TEAM123"));

        // Cleanup
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
