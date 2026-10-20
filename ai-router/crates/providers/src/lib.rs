//! Upstream adapters. OpenAI-compatible providers are a near passthrough.
//! Anthropic and Gemini translate to and from the OpenAI chat schema so the
//! gateway stays provider-agnostic.
//!
//! `async_job` is the reserved hook for video and other submit/poll APIs.

mod anthropic;
mod gemini;
mod openai;
mod retry;
mod sse;

use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

use airouter_config::{ProviderType, ResolvedConfig, ResolvedProvider};
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use serde_json::Value;

pub use anthropic::{
    chunks_for_event as anthropic_chunks_for_event,
    translate_request as translate_anthropic_request,
    translate_response as translate_anthropic_response, AnthropicStreamState,
};
pub use gemini::{
    translate_request as translate_gemini_request, translate_response as translate_gemini_response,
};

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

#[derive(Debug)]
pub enum ProviderError {
    Unsupported(String),
    Upstream {
        status: u16,
        message: String,
        retry_after: Option<String>,
    },
    Transport(String),
    Invalid(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Upstream {
                status, message, ..
            } => write!(f, "upstream {status}: {message}"),
            Self::Transport(m) => write!(f, "transport: {m}"),
            Self::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

pub enum ChatResult {
    Complete(Value),
    Stream(ByteStream),
}

#[derive(Clone)]
pub struct CallContext {
    pub client_model: String,
    pub upstream_model: String,
    pub timeout: Duration,
    pub max_retries: u32,
    pub backoff_ms: u64,
}

#[derive(Clone)]
pub struct AudioUpload {
    pub file_name: String,
    pub mime: String,
    pub bytes: Bytes,
    pub fields: Vec<(String, String)>,
}

pub struct AudioOut {
    pub content_type: String,
    pub data: Bytes,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    async fn chat(&self, body: Value, ctx: &CallContext) -> Result<ChatResult, ProviderError>;

    async fn embeddings(&self, body: Value, ctx: &CallContext) -> Result<Value, ProviderError> {
        let _ = (body, ctx);
        Err(ProviderError::Unsupported(format!(
            "provider {} does not support embeddings",
            self.name()
        )))
    }

    async fn images(&self, body: Value, ctx: &CallContext) -> Result<Value, ProviderError> {
        let _ = (body, ctx);
        Err(ProviderError::Unsupported(format!(
            "provider {} does not support image generation",
            self.name()
        )))
    }

    async fn speech(&self, body: Value, ctx: &CallContext) -> Result<AudioOut, ProviderError> {
        let _ = (body, ctx);
        Err(ProviderError::Unsupported(format!(
            "provider {} does not support text-to-speech",
            self.name()
        )))
    }

    async fn transcriptions(
        &self,
        upload: AudioUpload,
        ctx: &CallContext,
    ) -> Result<Value, ProviderError> {
        let _ = (upload, ctx);
        Err(ProviderError::Unsupported(format!(
            "provider {} does not support transcription",
            self.name()
        )))
    }

    /// Reserved for video and other asynchronous jobs. v1 has no public route that succeeds.
    async fn async_job(&self, body: Value, ctx: &CallContext) -> Result<Value, ProviderError> {
        let _ = (body, ctx);
        Err(ProviderError::Unsupported(
            "async jobs (video) are not implemented".into(),
        ))
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("airouter/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client")
}

pub fn build_map(
    cfg: &ResolvedConfig,
    http: reqwest::Client,
) -> HashMap<String, std::sync::Arc<dyn Provider>> {
    let mut map = HashMap::new();
    for (name, provider) in &cfg.providers {
        if !provider.enabled {
            continue;
        }
        let built: std::sync::Arc<dyn Provider> = match provider.provider_type {
            ProviderType::Openai | ProviderType::OpenaiCompat => {
                std::sync::Arc::new(openai::OpenAiProvider::new(provider, http.clone()))
            }
            ProviderType::Anthropic => {
                std::sync::Arc::new(anthropic::AnthropicProvider::new(provider, http.clone()))
            }
            ProviderType::Gemini => {
                std::sync::Arc::new(gemini::GeminiProvider::new(provider, http.clone()))
            }
        };
        map.insert(name.clone(), built);
    }
    map
}

pub async fn test_provider(
    provider: &ResolvedProvider,
    http: &reqwest::Client,
    model: Option<&str>,
) -> Result<String, ProviderError> {
    match provider.provider_type {
        ProviderType::Openai | ProviderType::OpenaiCompat => {
            let url = format!("{}/models", provider.base_url);
            let mut req = http.get(url);
            if let Some(key) = &provider.api_key {
                req = req.bearer_auth(key);
            }
            for (k, v) in &provider.headers {
                req = req.header(k, v);
            }
            let resp = req
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(retry::error_from_response(resp).await);
            }
            Ok(format!(
                "{} accepted the key (GET /models -> {})",
                provider.name, status
            ))
        }
        ProviderType::Anthropic => {
            let model = model.unwrap_or("claude-3-5-haiku-latest");
            let url = format!("{}/v1/messages", provider.base_url);
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}]
            });
            let mut req = http
                .post(url)
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            if let Some(key) = &provider.api_key {
                req = req.header("x-api-key", key);
            }
            let resp = req
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(retry::error_from_response(resp).await);
            }
            Ok(format!("{} accepted the key", provider.name))
        }
        ProviderType::Gemini => {
            let url = format!("{}/models", provider.base_url);
            let mut req = http.get(url);
            if let Some(key) = &provider.api_key {
                req = req.header("x-goog-api-key", key);
            }
            let resp = req
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(retry::error_from_response(resp).await);
            }
            Ok(format!(
                "{} accepted the key (GET /models -> {})",
                provider.name,
                resp.status()
            ))
        }
    }
}

/// Models the provider account can call. Ids are the upstream names, sorted.
pub async fn list_upstream_models(
    provider: &ResolvedProvider,
    http: &reqwest::Client,
) -> Result<Vec<String>, ProviderError> {
    if provider.api_key.as_deref().unwrap_or("").is_empty() {
        return Err(ProviderError::Invalid(format!(
            "{} has no API key",
            provider.name
        )));
    }
    let mut ids = match provider.provider_type {
        ProviderType::Openai | ProviderType::OpenaiCompat => {
            let body = provider_get(
                http,
                provider,
                &format!("{}/models", provider.base_url),
                AuthStyle::Bearer,
            )
            .await?;
            openai_model_ids(&body)
        }
        ProviderType::Gemini => {
            let body = provider_get(
                http,
                provider,
                &format!("{}/models", provider.base_url),
                AuthStyle::Google,
            )
            .await?;
            gemini_model_ids(&body)
        }
        ProviderType::Anthropic => anthropic_model_ids(http, provider).await?,
    };
    ids.sort();
    ids.dedup();
    Ok(ids)
}

enum AuthStyle {
    Bearer,
    Google,
    Anthropic,
}

async fn provider_get(
    http: &reqwest::Client,
    provider: &ResolvedProvider,
    url: &str,
    auth: AuthStyle,
) -> Result<Value, ProviderError> {
    let mut req = http.get(url);
    if let Some(key) = &provider.api_key {
        req = match auth {
            AuthStyle::Bearer => req.bearer_auth(key),
            AuthStyle::Google => req.header("x-goog-api-key", key),
            AuthStyle::Anthropic => req
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
        };
    }
    for (k, v) in &provider.headers {
        req = req.header(k, v);
    }
    let resp = req
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| ProviderError::Transport(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(retry::error_from_response(resp).await);
    }
    resp.json()
        .await
        .map_err(|e| ProviderError::Invalid(e.to_string()))
}

fn openai_model_ids(body: &Value) -> Vec<String> {
    body.get("data")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(|id| id.as_str()))
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn gemini_model_ids(body: &Value) -> Vec<String> {
    body.get("models")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("name").and_then(|name| name.as_str()))
                .map(|name| name.trim().trim_start_matches("models/"))
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

async fn anthropic_model_ids(
    http: &reqwest::Client,
    provider: &ResolvedProvider,
) -> Result<Vec<String>, ProviderError> {
    let mut ids = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..10 {
        let mut url = format!("{}/v1/models?limit=100", provider.base_url);
        if let Some(id) = &after {
            url.push_str("&after_id=");
            url.push_str(&urlencoding::encode(id));
        }
        let body = provider_get(http, provider, &url, AuthStyle::Anthropic).await?;
        let page = openai_model_ids(&body);
        if page.is_empty() {
            break;
        }
        after = page.last().cloned();
        let more = body
            .get("has_more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        ids.extend(page);
        if !more {
            break;
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_model_lists() {
        let openai = serde_json::json!({"data":[{"id":"gpt-4o"},{"id":" gpt-4o-mini "},{"id":""}]});
        assert_eq!(
            openai_model_ids(&openai),
            vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
        );
        let gemini = serde_json::json!({"models":[{"name":"models/gemini-2.5-flash"}]});
        assert_eq!(
            gemini_model_ids(&gemini),
            vec!["gemini-2.5-flash".to_string()]
        );
    }
}
