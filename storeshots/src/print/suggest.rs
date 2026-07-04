use crate::config::StoreshotsConfig;
use crate::prompts::{assemble_system, PromptOverrides, PromptPhase};
use crate::text_client::{phase_model, TextClient};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const PRINT_COPY_SYSTEM: &str = r#"You are a senior product marketing copywriter for print collateral (brochures, flyers, business cards).

Write scannable, pain-first, outcome-led copy. Every line must earn its space on paper.

Rules:
- User benefits only: features, pain points, outcomes, and solutions — never technical implementation
- NEVER mention: programming languages, frameworks, cloud vendors, APIs, backends, databases, "agentic", "full-stack", devops, or how the product is built
- Business card tagline (`card_tagline`): max 12 words, one sharp outcome — never a paragraph
- Headline: one positioning line (max 18 words)
- Pitch: 2-3 sentences max — problem → solution → result
- Bullets: 4-8 items, verb-led, specific user outcomes (not buzzword stacks)
- Respect brand voice from BRAND.md; do not invent features not in the brand guide
- Single primary CTA: website visit via QR (no App Store unless app.kind is mobile-app)
- Contact email should match brand URLs section when present

Return JSON only:
{
  "eyebrow": "CATEGORY · OFFER",
  "headline": "Main brochure headline",
  "card_tagline": "Short card line under name",
  "pitch": "2-3 sentence elevator pitch for print",
  "bullets": ["outcome 1", "outcome 2"],
  "contact_email": "sales@example.com"
}"#;

#[derive(Debug, Deserialize, Serialize)]
pub struct PrintCopyResponse {
    pub eyebrow: String,
    pub headline: String,
    pub card_tagline: String,
    pub pitch: String,
    pub bullets: Vec<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
}

pub async fn suggest_print_copy(
    app_root: &Path,
    cfg: &StoreshotsConfig,
    overrides: &PromptOverrides,
) -> Result<PrintCopyResponse> {
    let assembled = assemble_system(
        app_root,
        cfg,
        PromptPhase::PrintCopy,
        PRINT_COPY_SYSTEM,
        overrides,
        None,
    )?;

    let brand_path = cfg.brand_path(app_root);
    let brand_text = std::fs::read_to_string(&brand_path)
        .with_context(|| format!("read brand file {}", brand_path.display()))?;

    let user = format!(
        "App name: {}\nApp kind: {}\n\nFull brand guide:\n{}\n\nWrite print copy JSON for tri-fold brochure, single-page flyer, and business card. Optimize for lead generation to the website QR.",
        cfg.app.name,
        cfg.app.kind,
        brand_text.chars().take(8000).collect::<String>()
    );

    let model = phase_model(cfg, "print_copy", &cfg.ai.text_model);
    let client = TextClient::from_config(app_root, cfg)?;
    let text = client
        .generate_text(&model, &assembled.system, &user, true)
        .await?;

    let parsed: PrintCopyResponse =
        serde_json::from_str(&text).with_context(|| format!("parse print copy JSON: {text}"))?;
    Ok(parsed)
}

pub fn apply_print_copy(cfg: &mut StoreshotsConfig, copy: &PrintCopyResponse) {
    cfg.print.copy.eyebrow = Some(copy.eyebrow.clone());
    cfg.print.copy.headline = Some(copy.headline.clone());
    cfg.print.copy.card_tagline = Some(copy.card_tagline.clone());
    cfg.print.copy.pitch = Some(copy.pitch.clone());
    cfg.print.copy.bullets = copy.bullets.clone();
    if let Some(ref email) = copy.contact_email {
        cfg.print.copy.contact_email = Some(email.clone());
    }
}
