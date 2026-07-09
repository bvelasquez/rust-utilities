use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElabsConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

impl ElabsConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&text).context("parse elabs config")
    }

    pub fn save(path: &Path, config: &Self) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(config)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("elabs")
        .join("config.toml")
}

pub fn default_base_url() -> &'static str {
    "https://api.elevenlabs.io"
}
