use crate::config::StoreshotsConfig;
use crate::gemini::GeminiClient;
use crate::keys::{self, ResolvedKeys};
use crate::openrouter::{OpenRouterClient, TextProvider};
use anyhow::Result;
use std::path::Path;

pub enum TextClient {
    Gemini(GeminiClient),
    OpenRouter(OpenRouterClient),
}

impl TextClient {
    pub fn from_config(app_root: &Path, cfg: &StoreshotsConfig) -> Result<Self> {
        Self::from_config_keys(app_root, cfg, &keys::resolve_all(app_root, cfg)?)
    }

    pub fn from_config_keys(
        app_root: &Path,
        cfg: &StoreshotsConfig,
        resolved: &ResolvedKeys,
    ) -> Result<Self> {
        match cfg.ai.text_provider {
            TextProvider::Gemini => {
                let key = resolved
                    .gemini
                    .clone()
                    .or_else(|| keys::resolve_gemini(app_root, &cfg.ai.keys).ok())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Gemini key required for text_provider = gemini")
                    })?;
                Ok(Self::Gemini(GeminiClient::new(key)?))
            }
            TextProvider::Openrouter => {
                let key = resolved
                    .openrouter
                    .clone()
                    .or_else(|| keys::resolve_openrouter(app_root, &cfg.ai.keys).ok())
                    .ok_or_else(|| {
                        anyhow::anyhow!("OpenRouter key required for text_provider = openrouter")
                    })?;
                Ok(Self::OpenRouter(OpenRouterClient::new(key)?))
            }
        }
    }

    pub async fn generate_text(
        &self,
        model: &str,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String> {
        match self {
            Self::Gemini(client) => client.generate_text(model, system, user).await,
            Self::OpenRouter(client) => {
                client
                    .generate_text(model, system, user, json_mode)
                    .await
            }
        }
    }

    pub fn resolve_text_model(cfg_model: &str, provider: TextProvider) -> String {
        if !cfg_model.is_empty() {
            return cfg_model.to_string();
        }
        match provider {
            TextProvider::Gemini => std::env::var("STORESHOTS_MODEL_TEXT")
                .unwrap_or_else(|_| "gemini-2.5-flash".into()),
            TextProvider::Openrouter => std::env::var("STORESHOTS_MODEL_TEXT")
                .unwrap_or_else(|_| "google/gemini-2.5-flash".into()),
        }
    }
}

pub fn phase_model(cfg: &StoreshotsConfig, phase: &str, fallback: &str) -> String {
    cfg.ai
        .prompts
        .get(phase)
        .and_then(|p| p.model.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| TextClient::resolve_text_model(fallback, cfg.ai.text_provider))
}

pub fn gemini_for_render(app_root: &Path, cfg: &StoreshotsConfig) -> Result<GeminiClient> {
    let key = keys::resolve_gemini(app_root, &cfg.ai.keys)?;
    GeminiClient::new(key)
}
