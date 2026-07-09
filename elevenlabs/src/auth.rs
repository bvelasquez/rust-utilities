use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::PathBuf;

use crate::config::{default_config_path, ElabsConfig};

#[derive(Debug, Clone)]
pub struct AuthState {
    pub api_key: String,
    #[allow(dead_code)]
    pub config_path: PathBuf,
}

pub fn api_key_status(config_path: Option<&PathBuf>) -> serde_json::Value {
    let path = config_path.cloned().unwrap_or_else(default_config_path);
    let from_env = std::env::var("ELEVENLABS_API_KEY")
        .ok()
        .or_else(|| std::env::var("ELABS_API_KEY").ok());
    let from_file = if path.is_file() {
        ElabsConfig::load(&path)
            .ok()
            .and_then(|c| c.api_key)
    } else {
        None
    };

    serde_json::json!({
        "configPath": path.display().to_string(),
        "configExists": path.is_file(),
        "hasEnvKey": from_env.is_some(),
        "hasFileKey": from_file.is_some(),
        "envVars": ["ELEVENLABS_API_KEY", "ELABS_API_KEY"],
        "configured": from_env.is_some() || from_file.as_ref().is_some_and(|k| !k.is_empty()),
    })
}

pub fn load_api_key(config_path: Option<&PathBuf>) -> Result<AuthState> {
    if let Ok(key) = std::env::var("ELEVENLABS_API_KEY") {
        if !key.is_empty() {
            return Ok(AuthState {
                api_key: key,
                config_path: config_path.cloned().unwrap_or_else(default_config_path),
            });
        }
    }
    if let Ok(key) = std::env::var("ELABS_API_KEY") {
        if !key.is_empty() {
            return Ok(AuthState {
                api_key: key,
                config_path: config_path.cloned().unwrap_or_else(default_config_path),
            });
        }
    }

    let path = config_path.cloned().unwrap_or_else(default_config_path);
    if path.is_file() {
        let cfg = ElabsConfig::load(&path)?;
        if let Some(key) = cfg.api_key.filter(|k| !k.is_empty()) {
            return Ok(AuthState {
                api_key: key,
                config_path: path,
            });
        }
    }

    bail!(
        "ElevenLabs API key not configured — run `elabs apikey set <key>` or set ELEVENLABS_API_KEY"
    )
}

pub fn save_api_key(config_path: Option<&PathBuf>, api_key: &str) -> Result<PathBuf> {
    let path = config_path.cloned().unwrap_or_else(default_config_path);
    let mut cfg = if path.is_file() {
        ElabsConfig::load(&path).unwrap_or_default()
    } else {
        ElabsConfig::default()
    };
    cfg.api_key = Some(api_key.to_string());
    ElabsConfig::save(&path, &cfg)?;
    Ok(path)
}

pub fn set_api_key_interactive(config_path: Option<&PathBuf>, key: Option<String>, from_env: bool) -> Result<PathBuf> {
    let api_key = if from_env {
        std::env::var("ELEVENLABS_API_KEY")
            .or_else(|_| std::env::var("ELABS_API_KEY"))
            .context("--from-env requires ELEVENLABS_API_KEY or ELABS_API_KEY")?
    } else if let Some(k) = key {
        k
    } else {
        use dialoguer::Password;
        Password::new()
            .with_prompt("ElevenLabs API key")
            .interact()?
    };

    if api_key.trim().is_empty() {
        bail!("API key cannot be empty");
    }

    let path = save_api_key(config_path, api_key.trim())?;
    eprintln!(
        "{} {}",
        "API key saved to".green().bold(),
        path.display()
    );
    Ok(path)
}
