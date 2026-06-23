use crate::config::StoreshotsConfig;
use crate::gemini::GeminiClient;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const COPY_SYSTEM: &str = r#"You write App Store screenshot marketing copy.

Rules:
- One idea per headline. Never join two things with "and".
- Short common words. 3-5 words per line.
- Use intentional line breaks in titles as \n (not more than 2 lines for title).
- Subtitle is one short sentence (outcome or pain killed).
- Label is a short category tag in ALL CAPS (app name or feature area).
- Screenshots are advertisements, not feature lists.

Return JSON only:
{
  "slides": [
    { "id": "hero", "title": "Line one\nline two", "subtitle": "...", "label": "APP NAME" }
  ]
}"#;

#[derive(Debug, Deserialize, Serialize)]
pub struct CopyResponse {
    pub slides: Vec<CopySlide>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CopySlide {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    #[serde(default)]
    pub label: String,
}

pub async fn suggest_copy(
    client: &GeminiClient,
    model: &str,
    cfg: &StoreshotsConfig,
    features: &str,
) -> Result<CopyResponse> {
    let slide_ids: Vec<_> = cfg.slides.items.iter().map(|s| s.id.as_str()).collect();
    let user = format!(
        "App name: {}\nFeatures / context:\n{features}\n\nSlides to write (ids in order): {:?}\n\nWrite compelling copy for each slide id.",
        cfg.app.name, slide_ids
    );

    let text = client.generate_text(model, COPY_SYSTEM, &user).await?;
    let parsed: CopyResponse =
        serde_json::from_str(&text).with_context(|| format!("parse copy JSON: {text}"))?;
    Ok(parsed)
}

pub fn apply_copy(cfg: &mut StoreshotsConfig, copy: &CopyResponse) {
    for item in &mut cfg.slides.items {
        if let Some(s) = copy.slides.iter().find(|c| c.id == item.id) {
            item.title = s.title.clone();
            item.subtitle = s.subtitle.clone();
            if !s.label.is_empty() {
                item.label = s.label.clone();
            }
        }
    }
}

pub fn read_features_hint(app_root: &Path) -> String {
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
