//! `perry setup` — guided credential setup wizard for App Store / Google Play distribution

use anyhow::{bail, Result};
use clap::Args;
use console::style;
use dialoguer::{Confirm, Input, Password, Select};

use super::publish::{
    AndroidSavedConfig, AppleSavedConfig, IosSavedConfig, PerryConfig,
    config_path, is_interactive, load_config, save_config,
};

#[derive(Args, Debug)]
pub struct SetupArgs {
    /// Platform to configure: android, ios, macos
    pub platform: Option<String>,
}

pub fn run(args: SetupArgs) -> Result<()> {
    if !is_interactive() {
        bail!("`perry setup` requires an interactive terminal.");
    }

    println!();
    println!("  {} Perry Setup", style("▶").cyan().bold());
    println!();

    let platform = match args.platform.as_deref() {
        Some(p) => p.to_string(),
        None => {
            let options = &["Android", "iOS", "macOS"];
            let selection = Select::new()
                .with_prompt("  Which platform to configure?")
                .items(options)
                .default(0)
                .interact()?;
            match selection {
                0 => "android".to_string(),
                1 => "ios".to_string(),
                _ => "macos".to_string(),
            }
        }
    };

    let mut saved = load_config();

    match platform.as_str() {
        "android" => android_wizard(&mut saved)?,
        "ios" => ios_wizard(&mut saved)?,
        "macos" => macos_wizard(&mut saved)?,
        other => bail!("Unknown platform '{other}'. Use: android, ios, macos"),
    }

    save_config(&saved)?;
    println!();
    println!(
        "  {} Configuration saved to {}",
        style("✓").green().bold(),
        style(config_path().display()).dim()
    );
    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// Android wizard
// ---------------------------------------------------------------------------

fn android_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("Android Setup").bold());
    println!();

    // --- Step 1: Keystore ---
    println!("  {} Keystore", style("Step 1/2 —").cyan().bold());
    println!();

    let has_keystore = Confirm::new()
        .with_prompt("  Do you have an existing Android keystore?")
        .default(true)
        .interact()?;

    let (keystore_path, key_alias) = if has_keystore {
        let path = Input::<String>::new()
            .with_prompt("  Keystore path")
            .interact_text()?;
        let path = expand_tilde(&path);
        let alias = Input::<String>::new()
            .with_prompt("  Key alias")
            .default("key0".to_string())
            .interact_text()?;
        if !std::path::Path::new(&path).exists() {
            bail!("Keystore file not found: {path}");
        }
        (path, alias)
    } else {
        // Check for keytool
        if std::process::Command::new("keytool")
            .arg("-help")
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .is_err()
        {
            bail!(
                "keytool not found — install a JDK first (e.g. brew install --cask temurin) \
                 and try again."
            );
        }

        println!("  Generating a new Android release keystore...");
        println!();

        let path = Input::<String>::new()
            .with_prompt("  Output path (e.g. ~/release-key.keystore)")
            .interact_text()?;
        let path = expand_tilde(&path);
        let alias = Input::<String>::new()
            .with_prompt("  Key alias")
            .default("key0".to_string())
            .interact_text()?;
        let password = Password::new()
            .with_prompt("  Keystore password")
            .with_confirmation("  Confirm password", "Passwords didn't match")
            .interact()?;

        let status = std::process::Command::new("keytool")
            .args([
                "-genkeypair",
                "-v",
                "-keystore",
                &path,
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-alias",
                &alias,
                "-storepass",
                &password,
                "-keypass",
                &password,
                "-dname",
                "CN=Android, O=Android, C=US",
            ])
            .status()?;

        if !status.success() {
            bail!("keytool failed to generate keystore");
        }

        println!();
        println!("  {} Keystore created at {}", style("✓").green(), style(&path).bold());
        (path, alias)
    };

    let android = saved.android.get_or_insert_with(AndroidSavedConfig::default);
    android.keystore_path = Some(keystore_path.clone());
    android.key_alias = Some(key_alias.clone());

    println!("  {} Keystore: {}", style("✓").green(), style(&keystore_path).bold());
    println!("  {} Key alias: {}", style("✓").green(), style(&key_alias).bold());
    println!();

    // --- Step 2: Google Play Service Account ---
    println!("  {} Google Play Service Account", style("Step 2/2 —").cyan().bold());
    println!();
    println!("  Follow these steps to enable automated Play Store uploads:");
    println!();
    println!("  1. Open Play Console → Setup → API access:");
    println!("     https://play.google.com/console/developers/api-access");
    println!("     Link your console to a Google Cloud project.");
    println!();
    println!("  2. Create a service account + download its JSON key:");
    println!("     https://console.cloud.google.com/iam-admin/serviceaccounts");
    println!("     → Create Service Account → Add Key → Create new key → JSON");
    println!();
    println!("  3. Back in Play Console → Users & Permissions → Invite user");
    println!("     Add the service account email with Release Manager permissions.");
    println!();
    println!(
        "  {} The first release MUST be uploaded manually via Play Console before",
        style("!").yellow()
    );
    println!("     automated uploads will work.");
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let json_path = Input::<String>::new()
        .with_prompt("  Path to service account JSON key")
        .interact_text()?;
    let json_path = expand_tilde(&json_path);

    if !std::path::Path::new(&json_path).exists() {
        bail!("Service account JSON not found: {json_path}");
    }

    // Validate JSON content
    let json_content = std::fs::read_to_string(&json_path)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&json_content).map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))?;
    let client_email = parsed["client_email"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'client_email' in service account JSON"))?;
    if parsed["private_key"].as_str().is_none() {
        bail!("Missing 'private_key' in service account JSON");
    }

    println!(
        "  {} Service account: {}",
        style("✓").green(),
        style(client_email).bold()
    );

    let android = saved.android.get_or_insert_with(AndroidSavedConfig::default);
    android.google_play_key_path = Some(json_path);

    println!();
    println!("  Add to your perry.toml:");
    println!();
    println!("  {}", style("[android]").cyan());
    println!("  distribute = \"playstore\"");
    println!();
    println!("  Tip: to target a specific track, use:");
    println!("  distribute = \"playstore:beta\"  {} :internal, :alpha, :beta, :production", style("#").dim());

    Ok(())
}

// ---------------------------------------------------------------------------
// iOS wizard
// ---------------------------------------------------------------------------

fn ios_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("iOS Setup").bold());
    println!();

    // --- Step 1: App Store Connect API Key ---
    println!("  {} App Store Connect API Key", style("Step 1/3 —").cyan().bold());
    println!();
    println!("  1. Go to App Store Connect → Users and Access → Integrations → API:");
    println!("     https://appstoreconnect.apple.com/access/integrations/api");
    println!("  2. Click '+' and create a key with App Manager role.");
    println!("  3. Download the .p8 file immediately — it can only be downloaded once.");
    println!("  4. Note the Key ID and Issuer ID shown on the page.");
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let p8_path = prompt_file_path("  Path to .p8 key file", ".p8")?;
    let p8_content = std::fs::read_to_string(&p8_path)?;
    if !p8_content.trim_start().starts_with("-----BEGIN") {
        bail!("Invalid .p8 file — expected PEM format starting with '-----BEGIN'");
    }

    let key_id = Input::<String>::new()
        .with_prompt("  Key ID (e.g. ABC123XYZ)")
        .interact_text()?;
    let issuer_id = Input::<String>::new()
        .with_prompt("  Issuer ID (UUID format, e.g. a1b2c3d4-...)")
        .interact_text()?;
    let team_id = Input::<String>::new()
        .with_prompt("  Apple Developer Team ID (10 characters)")
        .interact_text()?;

    let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
    apple.p8_key_path = Some(p8_path.clone());
    apple.key_id = Some(key_id.clone());
    apple.issuer_id = Some(issuer_id.clone());
    apple.team_id = Some(team_id.clone());

    println!();
    println!("  {} Key ID: {}", style("✓").green(), style(&key_id).bold());
    println!("  {} Issuer ID: {}", style("✓").green(), style(&issuer_id).bold());
    println!("  {} Team ID: {}", style("✓").green(), style(&team_id).bold());
    println!();

    // --- Step 2: Signing Certificate (.p12) ---
    println!("  {} Signing Certificate (.p12)", style("Step 2/3 —").cyan().bold());
    println!();
    println!("  1. Open Xcode → Settings → Accounts → select your Apple ID.");
    println!("  2. Click 'Manage Certificates'.");
    println!("  3. Create an 'Apple Distribution' certificate if you don't have one.");
    println!("  4. Right-click the certificate → Export Certificate → save as .p12.");
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let cert_path = prompt_file_path("  Path to .p12 certificate", ".p12")?;

    let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
    apple.certificate_path = Some(cert_path.clone());

    println!("  {} Certificate saved: {}", style("✓").green(), style(&cert_path).bold());
    println!(
        "  {} Certificate password is NOT saved — set PERRY_APPLE_CERTIFICATE_PASSWORD",
        style("ℹ").blue()
    );
    println!("     or you will be prompted each time you run `perry publish`.");
    println!();

    // --- Step 3: Provisioning Profile ---
    println!("  {} Provisioning Profile", style("Step 3/3 —").cyan().bold());
    println!();
    println!("  1. Go to Apple Developer Portal → Profiles:");
    println!("     https://developer.apple.com/account/resources/profiles/list");
    println!("  2. Click '+' and choose 'App Store Connect' distribution type.");
    println!("  3. Select your App ID and the distribution certificate, then download.");
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let profile_path = prompt_file_path("  Path to .mobileprovision file", ".mobileprovision")?;

    let ios = saved.ios.get_or_insert_with(IosSavedConfig::default);
    ios.provisioning_profile_path = Some(profile_path.clone());

    println!("  {} Provisioning profile saved.", style("✓").green());
    println!();
    println!("  Add to your perry.toml:");
    println!();
    println!("  {}", style("[ios]").cyan());
    println!("  distribute = \"appstore\"  {} or \"testflight\"", style("#").dim());

    Ok(())
}

// ---------------------------------------------------------------------------
// macOS wizard
// ---------------------------------------------------------------------------

fn macos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("macOS Setup").bold());
    println!();

    // --- Step 1: App Store Connect API Key ---
    println!("  {} App Store Connect API Key", style("Step 1/2 —").cyan().bold());
    println!();
    println!("  1. Go to App Store Connect → Users and Access → Integrations → API:");
    println!("     https://appstoreconnect.apple.com/access/integrations/api");
    println!("  2. Click '+' and create a key with App Manager role.");
    println!("  3. Download the .p8 file immediately — it can only be downloaded once.");
    println!("  4. Note the Key ID and Issuer ID shown on the page.");
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let p8_path = prompt_file_path("  Path to .p8 key file", ".p8")?;
    let p8_content = std::fs::read_to_string(&p8_path)?;
    if !p8_content.trim_start().starts_with("-----BEGIN") {
        bail!("Invalid .p8 file — expected PEM format starting with '-----BEGIN'");
    }

    let key_id = Input::<String>::new()
        .with_prompt("  Key ID (e.g. ABC123XYZ)")
        .interact_text()?;
    let issuer_id = Input::<String>::new()
        .with_prompt("  Issuer ID (UUID format, e.g. a1b2c3d4-...)")
        .interact_text()?;
    let team_id = Input::<String>::new()
        .with_prompt("  Apple Developer Team ID (10 characters)")
        .interact_text()?;

    let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
    apple.p8_key_path = Some(p8_path.clone());
    apple.key_id = Some(key_id.clone());
    apple.issuer_id = Some(issuer_id.clone());
    apple.team_id = Some(team_id.clone());

    println!();
    println!("  {} Key ID: {}", style("✓").green(), style(&key_id).bold());
    println!("  {} Issuer ID: {}", style("✓").green(), style(&issuer_id).bold());
    println!("  {} Team ID: {}", style("✓").green(), style(&team_id).bold());
    println!();

    // --- Step 2: Mac Distribution Certificate ---
    println!("  {} Mac Distribution Certificate", style("Step 2/2 —").cyan().bold());
    println!();

    let cert_types = &["Mac App Store (submit to App Store)", "Developer ID (direct distribution / notarize)"];
    let cert_type_idx = Select::new()
        .with_prompt("  Distribution method")
        .items(cert_types)
        .default(0)
        .interact()?;
    let is_appstore = cert_type_idx == 0;

    println!();
    if is_appstore {
        println!("  To create a Mac App Store distribution certificate:");
        println!("  1. Open Xcode → Settings → Accounts → select your Apple ID.");
        println!("  2. Click 'Manage Certificates'.");
        println!("  3. Create a 'Mac App Distribution' certificate.");
        println!("  4. Right-click → Export Certificate → save as .p12.");
    } else {
        println!("  To create a Developer ID Application certificate:");
        println!("  1. Open Xcode → Settings → Accounts → select your Apple ID.");
        println!("  2. Click 'Manage Certificates'.");
        println!("  3. Create a 'Developer ID Application' certificate.");
        println!("  4. Right-click → Export Certificate → save as .p12.");
    }
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let cert_path = prompt_file_path("  Path to .p12 certificate", ".p12")?;

    let signing_identity = Input::<String>::new()
        .with_prompt("  Signing identity string (optional, e.g. 'Developer ID Application: ...')")
        .allow_empty(true)
        .interact_text()?;

    let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
    apple.certificate_path = Some(cert_path.clone());
    if !signing_identity.is_empty() {
        apple.signing_identity = Some(signing_identity);
    }

    println!("  {} Certificate saved: {}", style("✓").green(), style(&cert_path).bold());
    println!(
        "  {} Certificate password is NOT saved — set PERRY_APPLE_CERTIFICATE_PASSWORD",
        style("ℹ").blue()
    );
    println!();

    let distribute_value = if is_appstore { "appstore" } else { "notarize" };
    println!("  Add to your perry.toml:");
    println!();
    println!("  {}", style("[macos]").cyan());
    println!("  distribute = \"{distribute_value}\"");

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Expand leading `~/` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        dirs::home_dir()
            .map(|h| h.join(&path[2..]).to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    }
}

/// Prompt for a file path, validate it exists and has the expected extension.
fn prompt_file_path(prompt: &str, expected_ext: &str) -> Result<String> {
    let path = Input::<String>::new()
        .with_prompt(prompt)
        .interact_text()?;
    let path = expand_tilde(&path);
    if !std::path::Path::new(&path).exists() {
        bail!("File not found: {path}");
    }
    if !path.ends_with(expected_ext) {
        bail!("Expected a {expected_ext} file, got: {path}");
    }
    Ok(path)
}

/// Display a "Press Enter to continue" prompt.
fn press_enter_to_continue(prompt: &str) {
    let _ = Input::<String>::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text();
}
