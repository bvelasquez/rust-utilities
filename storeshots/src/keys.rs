use crate::config::{AiKeysSection, StoreshotsConfig};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

pub const GLOBAL_OPENROUTER_ENV: &str = "STORESHOTS_OPENROUTER_API_KEY";
pub const GLOBAL_GEMINI_ENV: &str = "STORESHOTS_GEMINI_API_KEY";
pub const LEGACY_OPENROUTER_ENV: &str = "OPENROUTER_API_KEY";
pub const LEGACY_GEMINI_ENV: &str = "GEMINI_API_KEY";
pub const LEGACY_GOOGLE_ENV: &str = "GOOGLE_API_KEY";

#[derive(Debug, Clone, Default)]
pub struct ResolvedKeys {
    pub openrouter: Option<String>,
    pub gemini: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SecretsFile {
    openrouter: Option<String>,
    gemini: Option<String>,
}

pub fn resolve_all(app_root: &Path, cfg: &StoreshotsConfig) -> Result<ResolvedKeys> {
    Ok(ResolvedKeys {
        openrouter: resolve_openrouter(app_root, &cfg.ai.keys).ok(),
        gemini: resolve_gemini(app_root, &cfg.ai.keys).ok(),
    })
}

pub fn resolve_openrouter(app_root: &Path, keys: &AiKeysSection) -> Result<String> {
    resolve_key(
        app_root,
        keys,
        KeyKind::OpenRouter,
        "OpenRouter",
        &[GLOBAL_OPENROUTER_ENV, LEGACY_OPENROUTER_ENV],
    )
}

pub fn resolve_gemini(app_root: &Path, keys: &AiKeysSection) -> Result<String> {
    resolve_key(
        app_root,
        keys,
        KeyKind::Gemini,
        "Gemini",
        &[GLOBAL_GEMINI_ENV, LEGACY_GEMINI_ENV, LEGACY_GOOGLE_ENV],
    )
}

enum KeyKind {
    OpenRouter,
    Gemini,
}

fn resolve_key(
    app_root: &Path,
    keys: &AiKeysSection,
    kind: KeyKind,
    label: &str,
    global_envs: &[&str],
) -> Result<String> {
    let secrets = load_secrets(app_root, keys)?;

    let from_secrets = match kind {
        KeyKind::OpenRouter => secrets.openrouter,
        KeyKind::Gemini => secrets.gemini,
    };
    if let Some(key) = from_secrets.filter(|k| !k.trim().is_empty()) {
        return Ok(key.trim().to_string());
    }

    let project_env = match kind {
        KeyKind::OpenRouter => keys.openrouter_env.as_deref(),
        KeyKind::Gemini => keys.gemini_env.as_deref(),
    };
    if let Some(var) = project_env {
        if let Ok(val) = std::env::var(var) {
            if !val.trim().is_empty() {
                return Ok(val.trim().to_string());
            }
        }
    }

    for var in global_envs {
        if let Ok(val) = std::env::var(var) {
            if !val.trim().is_empty() {
                return Ok(val.trim().to_string());
            }
        }
    }

    let secrets_path = app_root.join(&keys.secrets_file);
    let mut hint = format!(
        "no {label} API key found. Set one of:\n  1. {} (gitignored project secrets)\n",
        secrets_path.display()
    );
    if let Some(var) = project_env {
        hint.push_str(&format!("  2. env var {var} (per-project, set in storeshots.toml [ai.keys])\n"));
    }
    hint.push_str(&format!(
        "  3. {} (global storeshots CLI key)\n",
        global_envs[0]
    ));
    if global_envs.len() > 1 {
        hint.push_str(&format!(
            "  4. Legacy env: {}\n",
            global_envs[1..].join(", ")
        ));
    }
    bail!(hint)
}

fn load_secrets(app_root: &Path, keys: &AiKeysSection) -> Result<SecretsFile> {
    let path = app_root.join(&keys.secrets_file);
    if !path.is_file() {
        return Ok(SecretsFile::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read secrets file {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn secrets_example_toml() -> &'static str {
    r#"# Copy to storeshots/secrets.toml — NEVER commit this file.
# Per-project keys for cost tracking on OpenRouter / Google AI.

openrouter = "sk-or-v1-..."
gemini = "..."
"#
}

pub fn keys_schema_json() -> serde_json::Value {
    serde_json::json!({
        "secrets_file": {
            "default": "storeshots/secrets.toml",
            "gitignore": true,
            "fields": {
                "openrouter": "OpenRouter API key for this project",
                "gemini": "Google AI / Gemini API key for this project"
            }
        },
        "storeshots_toml_ai_keys": {
            "secrets_file": "path to gitignored secrets file (default storeshots/secrets.toml)",
            "openrouter_env": "optional env var NAME for per-project OpenRouter key",
            "gemini_env": "optional env var NAME for per-project Gemini key"
        },
        "resolution_order": [
            "storeshots/secrets.toml literal keys",
            "env var named in [ai.keys].openrouter_env / gemini_env",
            "STORESHOTS_OPENROUTER_API_KEY / STORESHOTS_GEMINI_API_KEY",
            "OPENROUTER_API_KEY / GEMINI_API_KEY / GOOGLE_API_KEY"
        ]
    })
}
