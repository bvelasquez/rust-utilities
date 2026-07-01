use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub default_customer_id: Option<String>,
    #[serde(default)]
    pub login_customer_id: Option<String>,
    #[serde(default)]
    pub aliases: std::collections::HashMap<String, String>,
}

impl ProjectConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&text).context("parse gads.toml")
    }

    pub fn resolve_customer<'a>(&'a self, id_or_alias: &'a str) -> &'a str {
        self.aliases
            .get(id_or_alias)
            .map(String::as_str)
            .unwrap_or(id_or_alias)
    }
}

pub fn default_credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("gads")
        .join("credentials.json")
}

pub fn legacy_credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("google-ads-open-cli")
        .join("credentials.json")
}

pub fn find_project_config(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let candidate = d.join("gads.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}
