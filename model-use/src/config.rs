use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default TUI auto-refresh interval (15 minutes).
pub const DEFAULT_TUI_REFRESH_INTERVAL_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CursorConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional: filter usage events to one team member email.
    #[serde(default)]
    pub email: Option<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderBudget {
    #[serde(default)]
    pub monthly_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiConfig {
    /// Auto-refresh interval in seconds. `0` disables periodic refresh.
    #[serde(default = "default_tui_refresh_interval_secs")]
    pub refresh_interval_secs: u64,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: default_tui_refresh_interval_secs(),
        }
    }
}

impl TuiConfig {
    pub fn refresh_interval(&self) -> Option<Duration> {
        if self.refresh_interval_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(self.refresh_interval_secs))
        }
    }
}

fn default_tui_refresh_interval_secs() -> u64 {
    DEFAULT_TUI_REFRESH_INTERVAL_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetsConfig {
    #[serde(default)]
    pub global_monthly_usd: Option<f64>,
    #[serde(default)]
    pub openrouter: ProviderBudget,
    #[serde(default)]
    pub anthropic: ProviderBudget,
    #[serde(default)]
    pub openai: ProviderBudget,
    #[serde(default)]
    pub cursor: ProviderBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelUseConfig {
    #[serde(default)]
    pub openrouter: ProviderConfig,
    #[serde(default)]
    pub anthropic: ProviderConfig,
    #[serde(default)]
    pub openai: ProviderConfig,
    #[serde(default)]
    pub cursor: CursorConfig,
    #[serde(default)]
    pub budgets: BudgetsConfig,
    #[serde(default)]
    pub tui: TuiConfig,
}

impl ModelUseConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&text).context("parse model-use config")
    }

    pub fn save(path: &Path, config: &Self) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(config)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn apply_env_overrides(&mut self) {
        if let Ok(key) = std::env::var("MODEL_USE_OPENROUTER_KEY") {
            if !key.is_empty() {
                self.openrouter.api_key = Some(key);
                self.openrouter.enabled = true;
            }
        }
        if let Ok(key) = std::env::var("MODEL_USE_ANTHROPIC_KEY") {
            if !key.is_empty() {
                self.anthropic.api_key = Some(key);
                self.anthropic.enabled = true;
            }
        }
        if let Ok(key) = std::env::var("MODEL_USE_OPENAI_KEY") {
            if !key.is_empty() {
                self.openai.api_key = Some(key);
                self.openai.enabled = true;
            }
        }
        if let Ok(key) = std::env::var("MODEL_USE_CURSOR_KEY") {
            if !key.is_empty() {
                self.cursor.api_key = Some(key);
                self.cursor.enabled = true;
            }
        }
        if let Ok(email) = std::env::var("MODEL_USE_CURSOR_EMAIL") {
            if !email.is_empty() {
                self.cursor.email = Some(email);
            }
        }
    }
}

pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("model-use")
        .join("config.toml")
}

pub fn default_cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("model-use")
        .join("cache.db")
}

/// Parse a duration string into seconds. Bare numbers are seconds; suffixes `s`, `m`, `h` supported.
pub fn parse_duration_secs(input: &str) -> Result<u64> {
    let s = input.trim();
    if s.is_empty() {
        bail!("duration cannot be empty");
    }

    if let Ok(secs) = s.parse::<u64>() {
        return Ok(secs);
    }

    let (num, unit) = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| s.split_at(i))
        .unwrap_or((s, ""));

    if num.is_empty() {
        bail!("invalid duration: {input}");
    }

    let value: u64 = num.parse().with_context(|| format!("invalid duration: {input}"))?;
    match unit.to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => Ok(value),
        "m" | "min" | "mins" | "minute" | "minutes" => Ok(value.saturating_mul(60)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Ok(value.saturating_mul(60 * 60)),
        _ => bail!("invalid duration unit in {input:?}; use s, m, or h"),
    }
}

pub fn format_duration_secs(secs: u64) -> String {
    if secs == 0 {
        return "disabled".into();
    }
    if secs % 3600 == 0 {
        let h = secs / 3600;
        return format!("{h}h");
    }
    if secs % 60 == 0 {
        let m = secs / 60;
        return format!("{m}m");
    }
    format!("{secs}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_variants() {
        assert_eq!(parse_duration_secs("900").unwrap(), 900);
        assert_eq!(parse_duration_secs("15m").unwrap(), 900);
        assert_eq!(parse_duration_secs("1h").unwrap(), 3600);
        assert_eq!(parse_duration_secs("30s").unwrap(), 30);
        assert_eq!(parse_duration_secs("0").unwrap(), 0);
    }

    #[test]
    fn default_tui_refresh_is_15_minutes() {
        let cfg = ModelUseConfig::default();
        assert_eq!(cfg.tui.refresh_interval_secs, 900);
    }
}
