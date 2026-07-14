pub mod budget;
pub mod fetch;
pub mod providers;
pub mod set;
pub mod summary;

use std::path::PathBuf;

use anyhow::Result;

use crate::cli::Cli;
use crate::config::{default_cache_path, default_config_path, ModelUseConfig};

#[derive(Clone)]
pub struct AppContext {
    pub config_path: PathBuf,
    pub cache_path: PathBuf,
    pub config: ModelUseConfig,
    pub json: bool,
}

impl AppContext {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let config_path = cli
            .config
            .clone()
            .unwrap_or_else(default_config_path);
        let mut config = ModelUseConfig::load(&config_path)?;
        config.apply_env_overrides();
        if let Some(k) = &cli.openrouter_key {
            config.openrouter.api_key = Some(k.clone());
            config.openrouter.enabled = true;
        }
        if let Some(k) = &cli.anthropic_key {
            config.anthropic.api_key = Some(k.clone());
            config.anthropic.enabled = true;
        }
        if let Some(k) = &cli.openai_key {
            config.openai.api_key = Some(k.clone());
            config.openai.enabled = true;
        }
        if let Some(k) = &cli.cursor_key {
            config.cursor.api_key = Some(k.clone());
            config.cursor.enabled = true;
        }
        Ok(Self {
            config_path,
            cache_path: default_cache_path(),
            config,
            json: cli.json,
        })
    }

    pub fn reload_config(&mut self) -> Result<()> {
        let mut config = ModelUseConfig::load(&self.config_path)?;
        config.apply_env_overrides();
        self.config = config;
        Ok(())
    }
}
