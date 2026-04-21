//! Perceptual screenshot diff for the widget gallery.
//!
//! Uses `dssim-core`'s multi-scale SSIM so we tolerate the small
//! anti-aliasing differences between dev boxes and CI runners but still
//! catch real widget regressions. Thresholds are per-(baseline, OS) and
//! live in `docs/examples/_baselines/thresholds.json`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

const DEFAULT_THRESHOLD: f64 = 0.010;

/// Diff outcome. `distance` is dssim's raw SSIM distance (0 = identical).
/// `threshold` is what was compared against.
pub struct DiffOutcome {
    pub distance: f64,
    pub threshold: f64,
}

impl DiffOutcome {
    pub fn passed(&self) -> bool {
        self.distance <= self.threshold
    }
}

/// Diff `actual_png` against `baseline_png` using SSIM.
/// Returns Err if either image is missing or malformed.
pub fn diff(actual_png: &Path, baseline_png: &Path, threshold: f64) -> Result<DiffOutcome> {
    let actual = load(actual_png)
        .with_context(|| format!("loading actual screenshot {}", actual_png.display()))?;
    let baseline = load(baseline_png)
        .with_context(|| format!("loading baseline {}", baseline_png.display()))?;

    if actual.width() != baseline.width() || actual.height() != baseline.height() {
        return Err(anyhow!(
            "size mismatch: actual {}x{} vs baseline {}x{}",
            actual.width(),
            actual.height(),
            baseline.width(),
            baseline.height()
        ));
    }

    let attr = dssim_core::Dssim::new();
    let actual_img = to_dssim(&actual, &attr)?;
    let baseline_img = to_dssim(&baseline, &attr)?;
    let (val, _maps) = attr.compare(&baseline_img, &actual_img);
    Ok(DiffOutcome {
        distance: val.into(),
        threshold,
    })
}

/// Look up the threshold for a given baseline name + host OS.
/// Falls back to `DEFAULT_THRESHOLD` if not specified. Unknown keys at the top
/// level (`_comment`, anything else) are ignored, so the JSON file can carry
/// human-readable notes alongside real entries.
pub fn threshold_for(thresholds_file: &Path, baseline_name: &str, host_os: &str) -> f64 {
    let Ok(text) = std::fs::read_to_string(thresholds_file) else {
        return DEFAULT_THRESHOLD;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return DEFAULT_THRESHOLD;
    };
    root.get(baseline_name)
        .and_then(|v| v.get(host_os))
        .and_then(|v| v.as_f64())
        .unwrap_or(DEFAULT_THRESHOLD)
}

fn load(path: &Path) -> Result<image::RgbaImage> {
    let img = image::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(img.to_rgba8())
}

fn to_dssim(
    img: &image::RgbaImage,
    attr: &dssim_core::Dssim,
) -> Result<dssim_core::DssimImage<f32>> {
    let width = img.width() as usize;
    let height = img.height() as usize;
    let pixels: Vec<rgb::RGBA8> = img
        .pixels()
        .map(|p| rgb::RGBA8 {
            r: p[0],
            g: p[1],
            b: p[2],
            a: p[3],
        })
        .collect();
    attr.create_image_rgba(&pixels, width, height)
        .ok_or_else(|| anyhow!("dssim failed to ingest image"))
}
