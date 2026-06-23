use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "storeshots.toml";
pub const RAW_DIR: &str = "storeshots/raw";
pub const BRAND_DIR: &str = "storeshots/brand";
pub const OUT_DIR: &str = "storeshots/out";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreshotsConfig {
    pub app: AppSection,
    #[serde(default)]
    pub brand: BrandSection,
    #[serde(default)]
    pub stores: StoresSection,
    #[serde(default)]
    pub ai: AiSection,
    pub slides: SlidesSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSection {
    pub name: String,
    #[serde(default)]
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandSection {
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_foreground")]
    pub foreground: String,
    #[serde(default)]
    pub muted: Option<String>,
    /// Path relative to app root, or a system font family name.
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_accent() -> String {
    "#5B7CFA".into()
}
fn default_background() -> String {
    "#F6F1EA".into()
}
fn default_foreground() -> String {
    "#171717".into()
}
fn default_theme() -> String {
    "clean-light".into()
}

impl Default for BrandSection {
    fn default() -> Self {
        Self {
            accent: default_accent(),
            background: default_background(),
            foreground: default_foreground(),
            muted: Some("#6B7280".into()),
            font: None,
            theme: default_theme(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoresSection {
    #[serde(default = "default_true")]
    pub apple_iphone: bool,
    #[serde(default)]
    pub apple_ipad: bool,
    #[serde(default)]
    pub google_phone: bool,
    #[serde(default)]
    pub feature_graphic: bool,
}

fn default_true() -> bool {
    true
}

impl Default for StoresSection {
    fn default() -> Self {
        Self {
            apple_iphone: true,
            apple_ipad: false,
            google_phone: false,
            feature_graphic: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSection {
    /// Generate AI backgrounds (default on). Use `storeshots render --no-ai` to skip.
    #[serde(default = "default_true")]
    pub backgrounds: bool,
    #[serde(default = "default_image_model")]
    pub image_model: String,
    #[serde(default = "default_text_model")]
    pub text_model: String,
}

fn default_image_model() -> String {
    "gemini-2.5-flash-image".into()
}

fn default_text_model() -> String {
    "gemini-2.5-flash".into()
}

impl Default for AiSection {
    fn default() -> Self {
        Self {
            backgrounds: true,
            image_model: default_image_model(),
            text_model: default_text_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlidesSection {
    pub items: Vec<SlideItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideItem {
    pub id: String,
    pub raw: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_layout")]
    pub layout: String,
}

fn default_layout() -> String {
    "hero-center".into()
}

impl StoreshotsConfig {
    pub fn load(app_root: &Path) -> Result<Self> {
        let path = app_root.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let cfg: Self = toml::from_str(&text).context("parse storeshots.toml")?;
        cfg.validate(app_root)?;
        Ok(cfg)
    }

    pub fn save(&self, app_root: &Path) -> Result<()> {
        let path = app_root.join(CONFIG_FILE);
        let text = toml::to_string_pretty(self).context("serialize config")?;
        std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn validate(&self, app_root: &Path) -> Result<()> {
        if self.slides.items.is_empty() {
            bail!("storeshots.toml: slides.items must not be empty");
        }
        for slide in &self.slides.items {
            let raw_path = app_root.join(RAW_DIR).join(&slide.raw);
            if !raw_path.is_file() {
                bail!(
                    "missing raw screenshot: {} (expected at {})",
                    slide.raw,
                    raw_path.display()
                );
            }
        }
        Ok(())
    }

    pub fn raw_path(app_root: &Path, filename: &str) -> PathBuf {
        app_root.join(RAW_DIR).join(filename)
    }

    pub fn out_dir(app_root: &Path, platform: &str, size_label: &str) -> PathBuf {
        app_root.join(OUT_DIR).join(platform).join(size_label)
    }
}

pub fn default_config(app_name: &str) -> StoreshotsConfig {
    StoreshotsConfig {
        app: AppSection {
            name: app_name.into(),
            bundle_id: None,
        },
        brand: BrandSection::default(),
        stores: StoresSection::default(),
        ai: AiSection::default(),
        slides: SlidesSection {
            items: vec![
                SlideItem {
                    id: "hero".into(),
                    raw: "01-home.png".into(),
                    title: "Your main\nbenefit here".into(),
                    subtitle: "One line that sells the outcome".into(),
                    label: app_name.to_uppercase(),
                    layout: "hero-center".into(),
                },
                SlideItem {
                    id: "feature".into(),
                    raw: "02-feature.png".into(),
                    title: "One idea\nper slide".into(),
                    subtitle: "Never join two features with and".into(),
                    label: "FEATURE".into(),
                    layout: "hero-center".into(),
                },
            ],
        },
    }
}
