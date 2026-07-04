use crate::config::{AdItem, StoreshotsConfig};
use crate::prompts::{assemble_system, PromptOverrides, PromptPhase};
use crate::text_client::{phase_model, TextClient};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const ADS_SYSTEM: &str = r#"You are a performance marketing creative director specializing in paid social and Google Ads display assets.

Given raw app screenshots, brand guide, and product context, plan DISTINCT marketing ad concepts — not App Store slides, not brochures. Each ad is a conversion-focused creative for paid channels.

Rules:
- One sharp benefit per ad. Never cram two ideas into a headline.
- Headlines: 2-4 words per line, use \n for intentional line breaks (max 2 lines).
- Subtitle: one short outcome sentence (optional for banners).
- CTA: 2-4 words (e.g. "Try free", "Get started", "Download now").
- layout: one of device-bottom | device-center | device-right | device-left | screenshot-hero | text-banner | auto
  - device-bottom: portrait/square — text top, phone mockup bottom
  - device-center: square — centered phone with text above
  - device-right / device-left: landscape — copy on one side, screenshot on other
  - screenshot-hero: full-bleed UI with text overlay (stories, feature graphics)
  - text-banner: wide banners with text + small logo only (no device)
  - auto: renderer picks by export size
- format_groups: array of size categories to render this concept to:
  - google-pmax (required for Performance Max: landscape 1200×628, square, portrait)
  - google-display (IAB banners: 300×250, 728×90, etc.)
  - social (Meta feed square, story 9:16, landscape link)
  - play-feature (1024×500 Play store header)
  Use multiple groups when the concept works across channels. Vary groups across ads.
- raw: filename from storeshots/raw/ (must match an available file exactly).
- Create 4-8 diverse concepts covering different features, angles, and format_groups.
- Match brand voice from BRAND.md. Do not invent features not supported by context.

Return JSON only:
{
  "ads": [
    {
      "id": "hero-benefit",
      "raw": "01-home.png",
      "headline": "Log meals\nin seconds",
      "subtitle": "No guilt. No spreadsheets.",
      "cta": "Try free",
      "layout": "device-bottom",
      "format_groups": ["google-pmax", "social"]
    }
  ]
}"#;

#[derive(Debug, Deserialize, Serialize)]
pub struct AdsResponse {
    pub ads: Vec<AdConcept>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdConcept {
    pub id: String,
    pub raw: String,
    pub headline: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub cta: String,
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default)]
    pub format_groups: Vec<String>,
}

fn default_layout() -> String {
    "auto".into()
}

pub async fn suggest_ads(
    app_root: &Path,
    cfg: &StoreshotsConfig,
    overrides: &PromptOverrides,
) -> Result<AdsResponse> {
    let assembled = assemble_system(
        app_root,
        cfg,
        PromptPhase::Ads,
        ADS_SYSTEM,
        overrides,
        None,
    )?;

    let brand_hint = read_brand_hint(app_root, cfg);
    let raw_files = list_raw_screenshots(app_root);
    let product_hint = read_product_hint(app_root);

    let user = format!(
        "App name: {}\nApp kind: {}\n\nBrand guide excerpt:\n{brand_hint}\n\nProduct / features context:\n{product_hint}\n\nAvailable raw screenshots in storeshots/raw/ (use exact filenames):\n{raw_list}\n\nPlan diverse ad concepts for Google Ads, Meta, and Play Store marketing. Cover multiple format_groups across the set.",
        cfg.app.name,
        cfg.app.kind,
        raw_list = if raw_files.is_empty() {
            "(none found — suggest generic layouts; user will add PNGs later)".into()
        } else {
            raw_files.join("\n")
        }
    );

    let model = phase_model(cfg, "ads", &cfg.ai.text_model);
    let client = TextClient::from_config(app_root, cfg)?;
    let text = client
        .generate_text(&model, &assembled.system, &user, true)
        .await?;

    let parsed: AdsResponse =
        serde_json::from_str(&text).with_context(|| format!("parse ads JSON: {text}"))?;
    Ok(parsed)
}

pub fn apply_ads(cfg: &mut StoreshotsConfig, response: &AdsResponse) {
    cfg.ads.items = response
        .ads
        .iter()
        .map(|a| AdItem {
            id: a.id.clone(),
            raw: a.raw.clone(),
            headline: a.headline.clone(),
            subtitle: a.subtitle.clone(),
            cta: a.cta.clone(),
            layout: a.layout.clone(),
            format_groups: a.format_groups.clone(),
            prompt_append: None,
        })
        .collect();
}

pub fn list_raw_screenshots(app_root: &Path) -> Vec<String> {
    let raw_dir = app_root.join(crate::config::RAW_DIR);
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&raw_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("png")
                        || ext.eq_ignore_ascii_case("jpg")
                        || ext.eq_ignore_ascii_case("jpeg")
                    {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            files.push(name.to_string());
                        }
                    }
                }
            }
        }
    }
    files.sort();
    files
}

fn read_brand_hint(app_root: &Path, cfg: &StoreshotsConfig) -> String {
    let path = cfg.brand_path(app_root);
    if path.is_file() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return text.chars().take(6000).collect();
        }
    }
    String::new()
}

fn read_product_hint(app_root: &Path) -> String {
    let candidates = [
        "README.md",
        "readme.md",
        "APP_STORE.md",
        "docs/app-store.md",
        "fastlane/metadata/en-US/description.txt",
    ];
    for rel in candidates {
        let path = app_root.join(rel);
        if path.is_file() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                return text.chars().take(4000).collect();
            }
        }
    }
    String::new()
}
