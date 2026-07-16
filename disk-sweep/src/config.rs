use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::Cli;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    pub model: Option<String>,
    pub openrouter_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub llm: LlmConfig,
}

pub struct AppContext {
    pub config_path: PathBuf,
    pub config: AppConfig,
}

impl AppContext {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let config_path = cli
            .config
            .clone()
            .or_else(default_config_path)
            .context("could not resolve config path")?;

        let mut config = if config_path.exists() {
            load_config(&config_path)?
        } else {
            AppConfig::default()
        };

        if let Some(k) = &cli.openrouter_key {
            config.llm.openrouter_api_key = Some(k.clone());
        }
        if let Some(m) = &cli.llm_model {
            config.llm.model = Some(m.clone());
        }

        if config.llm.openrouter_api_key.is_none() {
            config.llm.openrouter_api_key = std::env::var("DISK_SWEEP_OPENROUTER_KEY")
                .ok()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok());
        }

        Ok(Self {
            config_path,
            config,
        })
    }

    pub fn llm_api_key(&self) -> Result<&str> {
        self.config
            .llm
            .openrouter_api_key
            .as_deref()
            .context("OpenRouter API key required: set DISK_SWEEP_OPENROUTER_KEY or OPENROUTER_API_KEY")
    }

    pub fn llm_model(&self) -> String {
        self.config
            .llm
            .model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o-mini".into())
    }
}

fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("disk-sweep").join("config.toml"))
}

fn load_config(path: &PathBuf) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let cfg: AppConfig = toml::from_str(&raw).context("parse config.toml")?;
    Ok(cfg)
}

/// Load `.env` from the current directory when vars are not already set.
pub fn load_dotenv() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let path = cwd.join(".env");
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if std::env::var_os(key).is_none() {
            // SAFETY: called before threads spawn, during single-threaded startup.
            unsafe { std::env::set_var(key, value) };
        }
    }
}
