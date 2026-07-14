pub mod anthropic;
pub mod cursor;
pub mod openai;
pub mod openrouter;
pub mod types;

use anyhow::{Context, Result};

use crate::config::ModelUseConfig;
use types::{Provider, ProviderTestResult, UsageBucket};

pub async fn fetch_provider(
    config: &ModelUseConfig,
    provider: Provider,
    days: i64,
) -> Result<Vec<UsageBucket>> {
    match provider {
        Provider::Openrouter => {
            let key = config
                .openrouter
                .api_key
                .as_deref()
                .context("openrouter: no API key configured")?;
            openrouter::fetch_openrouter(key, days).await
        }
        Provider::Anthropic => {
            let key = config
                .anthropic
                .api_key
                .as_deref()
                .context("anthropic: no API key configured")?;
            anthropic::fetch_anthropic(key, days).await
        }
        Provider::Openai => {
            let key = config
                .openai
                .api_key
                .as_deref()
                .context("openai: no API key configured")?;
            openai::fetch_openai(key, days).await
        }
        Provider::Cursor => {
            let key = config
                .cursor
                .api_key
                .as_deref()
                .context("cursor: no API key configured")?;
            let options = cursor::CursorFetchOptions {
                email_filter: config.cursor.email.clone(),
            };
            cursor::fetch_cursor(key, days, &options).await
        }
    }
}

pub async fn test_provider(config: &ModelUseConfig, provider: Provider) -> ProviderTestResult {
    match provider {
        Provider::Openrouter => match config.openrouter.api_key.as_deref() {
            Some(k) => openrouter::test_openrouter(k).await,
            None => missing_key(Provider::Openrouter),
        },
        Provider::Anthropic => match config.anthropic.api_key.as_deref() {
            Some(k) => anthropic::test_anthropic(k).await,
            None => missing_key(Provider::Anthropic),
        },
        Provider::Openai => match config.openai.api_key.as_deref() {
            Some(k) => openai::test_openai(k).await,
            None => missing_key(Provider::Openai),
        },
        Provider::Cursor => match config.cursor.api_key.as_deref() {
            Some(k) => cursor::test_cursor(k).await,
            None => missing_key(Provider::Cursor),
        },
    }
}

fn missing_key(provider: Provider) -> ProviderTestResult {
    ProviderTestResult {
        provider,
        ok: false,
        message: "no API key configured".into(),
        key_type_hint: provider.key_hint().into(),
        docs_url: provider.docs_url().into(),
    }
}

pub fn provider_enabled(config: &ModelUseConfig, provider: Provider) -> bool {
    match provider {
        Provider::Openrouter => config.openrouter.enabled,
        Provider::Anthropic => config.anthropic.enabled,
        Provider::Openai => config.openai.enabled,
        Provider::Cursor => config.cursor.enabled,
    }
}

pub fn provider_has_key(config: &ModelUseConfig, provider: Provider) -> bool {
    match provider {
        Provider::Openrouter => config.openrouter.api_key.is_some(),
        Provider::Anthropic => config.anthropic.api_key.is_some(),
        Provider::Openai => config.openai.api_key.is_some(),
        Provider::Cursor => config.cursor.api_key.is_some(),
    }
}
