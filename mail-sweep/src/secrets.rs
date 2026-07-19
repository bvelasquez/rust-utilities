use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretsFile {
    pub openrouter_api_key: Option<String>,
    pub llm_model: Option<String>,
    #[serde(default)]
    pub accounts: HashMap<String, String>,
}

impl SecretsFile {
    pub fn path_for_config(config_path: &Path) -> PathBuf {
        config_path
            .parent()
            .map(|d| d.join("secrets.toml"))
            .unwrap_or_else(|| PathBuf::from("secrets.toml"))
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let secrets: Self = toml::from_str(&raw).context("parse secrets.toml")?;
        Ok(secrets)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self).context("serialize secrets.toml")?;
        fs::write(path, raw).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn merge_from(&mut self, other: &Self) {
        if other.openrouter_api_key.is_some() {
            self.openrouter_api_key = other.openrouter_api_key.clone();
        }
        if other.llm_model.is_some() {
            self.llm_model = other.llm_model.clone();
        }
        for (id, password) in &other.accounts {
            self.accounts.insert(id.clone(), password.clone());
        }
    }

    pub fn account_password(&self, account_id: &str) -> Option<&str> {
        self.accounts.get(account_id).map(|s| s.as_str())
    }
}

/// Load `.env` files into a secrets struct (does not set process environment variables).
pub fn load_dotenv_secrets() -> SecretsFile {
    let mut secrets = SecretsFile::default();
    for path in dotenv_paths() {
        if let Ok(raw) = fs::read_to_string(&path) {
            merge_dotenv_text(&mut secrets, &raw);
        }
    }
    secrets
}

fn dotenv_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
    }
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join("mail-sweep").join(".env"));
    }
    paths
}

fn merge_dotenv_text(secrets: &mut SecretsFile, raw: &str) {
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        apply_dotenv_pair(secrets, key.trim(), value.trim().trim_matches('"').trim_matches('\''));
    }
}

fn apply_dotenv_pair(secrets: &mut SecretsFile, key: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    match key {
        "openrouter_api_key" | "MAIL_SWEEP_OPENROUTER_KEY" | "OPENROUTER_API_KEY" => {
            secrets.openrouter_api_key = Some(value.into());
        }
        "llm_model" | "MAIL_SWEEP_LLM_MODEL" => {
            secrets.llm_model = Some(value.into());
        }
        k if k.starts_with("account_") && k.ends_with("_password") => {
            let id = k
                .trim_start_matches("account_")
                .trim_end_matches("_password")
                .replace('_', "-");
            secrets.accounts.insert(id, value.into());
        }
        k if k.starts_with("MAIL_SWEEP_ACCOUNT_") && k.ends_with("_PASSWORD") => {
            let id = k
                .trim_start_matches("MAIL_SWEEP_ACCOUNT_")
                .trim_end_matches("_PASSWORD")
                .to_lowercase()
                .replace('_', "-");
            secrets.accounts.insert(id, value.into());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotenv_keys() {
        let mut secrets = SecretsFile::default();
        merge_dotenv_text(
            &mut secrets,
            "openrouter_api_key=sk-test\naccount_personal_password=secret\n",
        );
        assert_eq!(secrets.openrouter_api_key.as_deref(), Some("sk-test"));
        assert_eq!(secrets.account_password("personal"), Some("secret"));
    }
}
