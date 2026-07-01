use crate::openrouter::TextProvider;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "storeshots.toml";
pub const RAW_DIR: &str = "storeshots/raw";
pub const BRAND_DIR: &str = "storeshots/brand";
pub const PROMPTS_DIR: &str = "storeshots/prompts";
pub const ASSETS_DIR: &str = "storeshots/assets";
pub const OUT_DIR: &str = "storeshots/out";
pub const DEFAULT_BRAND_MD: &str = "docs/BRAND.md";
pub const SECRETS_FILE: &str = "storeshots/secrets.toml";
pub const SECRETS_EXAMPLE_FILE: &str = "storeshots/secrets.toml.example";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreshotsConfig {
    pub app: AppSection,
    #[serde(default)]
    pub paths: PathsSection,
    #[serde(default)]
    pub brand: BrandSection,
    #[serde(default)]
    pub stores: StoresSection,
    #[serde(default)]
    pub ai: AiSection,
    #[serde(default)]
    pub pipeline: PipelineSection,
    #[serde(default)]
    pub print: PrintSection,
    pub slides: SlidesSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSection {
    pub name: String,
    #[serde(default)]
    pub bundle_id: Option<String>,
    #[serde(default = "default_app_kind")]
    pub kind: String,
}

fn default_app_kind() -> String {
    "mobile-app".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsSection {
    #[serde(default = "default_brand_path")]
    pub brand: String,
    #[serde(default = "default_web_root")]
    pub web_root: String,
    #[serde(default)]
    pub screenshots: Option<String>,
    #[serde(default = "default_prompts_dir")]
    pub prompts_dir: Option<String>,
}

impl Default for PathsSection {
    fn default() -> Self {
        Self {
            brand: default_brand_path(),
            web_root: default_web_root(),
            screenshots: None,
            prompts_dir: default_prompts_dir(),
        }
    }
}

fn default_brand_path() -> String {
    DEFAULT_BRAND_MD.into()
}

fn default_web_root() -> String {
    ".".into()
}

fn default_prompts_dir() -> Option<String> {
    Some(PROMPTS_DIR.into())
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
    #[serde(default = "default_true")]
    pub backgrounds: bool,
    #[serde(default)]
    pub text_provider: TextProvider,
    #[serde(default = "default_image_model")]
    pub image_model: String,
    #[serde(default = "default_text_model")]
    pub text_model: String,
    #[serde(default)]
    pub prompts: HashMap<String, AiPromptSection>,
    #[serde(default)]
    pub keys: AiKeysSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiKeysSection {
    #[serde(default = "default_secrets_file")]
    pub secrets_file: String,
    /// Name of an env var holding this project's OpenRouter key (cost tracking).
    #[serde(default)]
    pub openrouter_env: Option<String>,
    /// Name of an env var holding this project's Gemini / Google AI key.
    #[serde(default)]
    pub gemini_env: Option<String>,
}

fn default_secrets_file() -> String {
    "storeshots/secrets.toml".into()
}

impl Default for AiKeysSection {
    fn default() -> Self {
        Self {
            secrets_file: default_secrets_file(),
            openrouter_env: None,
            gemini_env: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiPromptSection {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_append: Option<String>,
    #[serde(default)]
    pub prompt_files: Vec<String>,
}

fn default_image_model() -> String {
    "gemini-2.5-flash-image".into()
}

fn default_text_model() -> String {
    "google/gemini-2.5-flash".into()
}

impl Default for AiSection {
    fn default() -> Self {
        Self {
            backgrounds: true,
            text_provider: TextProvider::Openrouter,
            image_model: default_image_model(),
            text_model: default_text_model(),
            prompts: HashMap::new(),
            keys: AiKeysSection::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineSection {
    #[serde(default)]
    pub steps: Vec<PipelineStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub id: String,
    pub phase: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrintSection {
    #[serde(default = "default_print_out")]
    pub output_dir: String,
    #[serde(default = "default_print_dpi")]
    pub dpi: u32,
    #[serde(default = "default_print_export_scale")]
    pub export_scale: u32,
    #[serde(default)]
    pub copy: PrintCopySection,
    #[serde(default)]
    pub brochure: Option<PrintBrochureSection>,
    #[serde(default)]
    pub business_card: Option<PrintBusinessCardSection>,
}

fn default_print_out() -> String {
    "storeshots/out/print".into()
}

fn default_print_dpi() -> u32 {
    300
}

fn default_print_export_scale() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrintCopySection {
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub qr_url: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub headline: Option<String>,
    #[serde(default)]
    pub eyebrow: Option<String>,
    #[serde(default)]
    pub card_tagline: Option<String>,
    #[serde(default)]
    pub pitch: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub bullets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintBrochureSection {
    #[serde(default)]
    pub formats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintBusinessCardSection {
    #[serde(default = "default_bleed_w")]
    pub bleed_w_in: f64,
    #[serde(default = "default_bleed_h")]
    pub bleed_h_in: f64,
    #[serde(default)]
    pub variants: Vec<String>,
}

fn default_bleed_w() -> f64 {
    3.625
}
fn default_bleed_h() -> f64 {
    2.125
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
    #[serde(default)]
    pub prompt_append: Option<String>,
}

fn default_layout() -> String {
    "hero-center".into()
}

impl StoreshotsConfig {
    pub fn load(app_root: &Path) -> Result<Self> {
        let cfg = Self::load_parsed(app_root)?;
        cfg.validate_for_render(app_root)?;
        Ok(cfg)
    }

    pub fn load_relaxed(app_root: &Path) -> Result<Self> {
        let cfg = Self::load_parsed(app_root)?;
        cfg.validate_basic()?;
        Ok(cfg)
    }

    fn load_parsed(app_root: &Path) -> Result<Self> {
        let path = app_root.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&text).context("parse storeshots.toml")
    }

    pub fn save(&self, app_root: &Path) -> Result<()> {
        let path = app_root.join(CONFIG_FILE);
        let text = toml::to_string_pretty(self).context("serialize config")?;
        std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn validate_basic(&self) -> Result<()> {
        if self.app.name.trim().is_empty() {
            bail!("storeshots.toml: app.name must not be empty");
        }
        if self.app.kind != "company-site" && self.slides.items.is_empty() {
            bail!("storeshots.toml: slides.items must not be empty");
        }
        Ok(())
    }

    pub fn validate_for_render(&self, app_root: &Path) -> Result<()> {
        self.validate_basic()?;
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

    pub fn brand_path(&self, app_root: &Path) -> PathBuf {
        let rel = if self.paths.brand.is_empty() {
            DEFAULT_BRAND_MD
        } else {
            self.paths.brand.as_str()
        };
        app_root.join(rel)
    }

    pub fn web_root_path(&self, app_root: &Path) -> PathBuf {
        let rel = if self.paths.web_root.is_empty() {
            "."
        } else {
            self.paths.web_root.as_str()
        };
        app_root.join(rel)
    }

    pub fn raw_path(app_root: &Path, filename: &str) -> PathBuf {
        app_root.join(RAW_DIR).join(filename)
    }

    pub fn out_dir(app_root: &Path, platform: &str, size_label: &str) -> PathBuf {
        app_root.join(OUT_DIR).join("mobile").join(platform).join(size_label)
    }

    pub fn print_out_dir(&self, app_root: &Path) -> PathBuf {
        app_root.join(&self.print.output_dir)
    }

    pub fn default_pipeline(&self) -> Vec<PipelineStep> {
        if !self.pipeline.steps.is_empty() {
            return self.pipeline.steps.clone();
        }
        vec![
            PipelineStep {
                id: "brand".into(),
                phase: "brand".into(),
                enabled: true,
                depends_on: vec![],
            },
            PipelineStep {
                id: "copy".into(),
                phase: "copy".into(),
                enabled: true,
                depends_on: vec!["brand".into()],
            },
            PipelineStep {
                id: "mobile".into(),
                phase: "mobile".into(),
                enabled: true,
                depends_on: vec!["copy".into()],
            },
            PipelineStep {
                id: "print".into(),
                phase: "print".into(),
                enabled: false,
                depends_on: vec!["copy".into()],
            },
        ]
    }
}

pub fn default_config(app_name: &str) -> StoreshotsConfig {
    StoreshotsConfig {
        app: AppSection {
            name: app_name.into(),
            bundle_id: None,
            kind: default_app_kind(),
        },
        paths: PathsSection::default(),
        brand: BrandSection::default(),
        stores: StoresSection::default(),
        ai: AiSection::default(),
        pipeline: PipelineSection::default(),
        print: PrintSection::default(),
        slides: SlidesSection {
            items: vec![
                SlideItem {
                    id: "hero".into(),
                    raw: "01-home.png".into(),
                    title: "Your main\nbenefit here".into(),
                    subtitle: "One line that sells the outcome".into(),
                    label: app_name.to_uppercase(),
                    layout: "hero-center".into(),
                    prompt_append: None,
                },
                SlideItem {
                    id: "feature".into(),
                    raw: "02-feature.png".into(),
                    title: "One idea\nper slide".into(),
                    subtitle: "Never join two features with and".into(),
                    label: "FEATURE".into(),
                    layout: "hero-center".into(),
                    prompt_append: None,
                },
            ],
        },
    }
}

pub fn default_pipeline_toml() -> String {
    r#"# Declarative pipeline for `storeshots run`
[[pipeline.steps]]
id = "brand"
phase = "brand"
enabled = true

[[pipeline.steps]]
id = "copy"
phase = "copy"
depends_on = ["brand"]

[[pipeline.steps]]
id = "mobile"
phase = "mobile"
depends_on = ["copy"]

[[pipeline.steps]]
id = "print"
phase = "print"
depends_on = ["copy"]
enabled = false
"#
    .into()
}
