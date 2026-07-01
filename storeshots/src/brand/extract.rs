use crate::brand::scan::{format_context, scan_project};
use crate::config::StoreshotsConfig;
use crate::prompts::{assemble_system, PromptOverrides, PromptPhase};
use crate::text_client::{phase_model, TextClient};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

const BRAND_SYSTEM: &str = r##"You are a brand strategist. Analyze the provided source code, CSS, and copy to produce a BRAND.md marketing guide.

Return JSON only with this shape:
{
  "markdown": "# Product Name - Brand Guide\n\n(full markdown document)"
}

The markdown must include these sections (use ## headings):
- Product identity (table: name, website, category, support email if found)
- One-line description
- Taglines (approved list)
- Elevator pitch (~50 words)
- What this product is - and is not
- Target audience (primary + secondary)
- Voice and tone
- Visual brand (colors as hex, fonts, theme notes from CSS/Tailwind)
- Key features (bullets, pain-first not feature dumps)
- Required disclaimers (if health/finance/legal context)
- URLs and contact

Be factual - only claim features evident in the source. Use pain-first messaging for consumer apps. For B2B/studio sites, emphasize capabilities and production focus."##;

#[derive(Debug, Deserialize)]
struct BrandExtractResponse {
    markdown: String,
}

pub async fn extract_brand(
    app_root: &Path,
    cfg: &StoreshotsConfig,
    overrides: &PromptOverrides,
    dry_run: bool,
) -> Result<String> {
    let ctx = scan_project(app_root, cfg)?;
    let assembled = assemble_system(
        app_root,
        cfg,
        PromptPhase::Brand,
        BRAND_SYSTEM,
        overrides,
        None,
    )?;

    let user = format!(
        "App name from config: {}\nApp kind: {}\n\nSource context:{}\n\nWrite the complete BRAND.md markdown.",
        cfg.app.name,
        cfg.app.kind,
        format_context(&ctx)
    );

    let model = phase_model(cfg, "brand", &cfg.ai.text_model);
    let client = TextClient::from_config(app_root, cfg)?;
    let raw = client
        .generate_text(&model, &assembled.system, &user, true)
        .await?;

    let parsed: BrandExtractResponse =
        serde_json::from_str(&raw).with_context(|| format!("parse brand JSON: {raw}"))?;

    if dry_run {
        return Ok(parsed.markdown);
    }

    let brand_path = cfg.brand_path(app_root);
    if let Some(parent) = brand_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&brand_path, &parsed.markdown)
        .with_context(|| format!("write {}", brand_path.display()))?;

    Ok(parsed.markdown)
}
