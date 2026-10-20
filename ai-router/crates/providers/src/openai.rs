use std::collections::BTreeMap;
use std::time::Duration;

use airouter_config::{ProviderType, ResolvedProvider};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};

use crate::retry::{error_from_response, send_with_retry};
use crate::{AudioOut, AudioUpload, CallContext, ChatResult, Provider, ProviderError};

pub struct OpenAiProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
    http: reqwest::Client,
    /// Groq gpt-oss rejects any `reasoning_effort` outside low/medium/high.
    clamp_reasoning_effort: bool,
    /// OpenAI-compatible upstreams generally only know the `system` role, while
    /// newer OpenAI clients send `developer`.
    normalize_developer_role: bool,
}

impl OpenAiProvider {
    pub fn new(provider: &ResolvedProvider, http: reqwest::Client) -> Self {
        Self {
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            headers: provider.headers.clone(),
            http,
            clamp_reasoning_effort: groq_reasoning_vocab(&provider.name, &provider.base_url),
            normalize_developer_role: provider.provider_type == ProviderType::OpenaiCompat,
        }
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn post_json(
        &self,
        path: &str,
        body: Value,
        ctx: &CallContext,
        stream: bool,
    ) -> Result<reqwest::Response, ProviderError> {
        let url = self.url(path);
        let timeout = ctx.timeout;
        let http = self.http.clone();
        let api_key = self.api_key.clone();
        let headers = self.headers.clone();
        let body_for = body.clone();
        send_with_retry(
            move || {
                let mut req = http.post(&url).json(&body_for);
                if !stream && !timeout.is_zero() {
                    req = req.timeout(timeout);
                }
                if let Some(key) = &api_key {
                    req = req.bearer_auth(key);
                }
                for (k, v) in &headers {
                    req = req.header(k, v);
                }
                req
            },
            ctx.max_retries,
            ctx.backoff_ms,
        )
        .await
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, mut body: Value, ctx: &CallContext) -> Result<ChatResult, ProviderError> {
        body["model"] = json!(ctx.upstream_model);
        if self.clamp_reasoning_effort {
            clamp_reasoning_effort(&mut body);
        }
        if self.normalize_developer_role {
            normalize_developer_role(&mut body);
        }
        relax_luna_tool_reasoning(&mut body, &ctx.upstream_model);
        let stream = body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if stream {
            // Ask upstream for a usage chunk so we can price the call. Clients tolerate it.
            body["stream_options"]["include_usage"] = json!(true);
        }
        let resp = self
            .post_json("chat/completions", body, ctx, stream)
            .await?;
        if !resp.status().is_success() {
            return Err(error_from_response(resp).await);
        }
        if stream {
            let stream = resp
                .bytes_stream()
                .map(|r| r.map_err(|e| ProviderError::Transport(e.to_string())));
            Ok(ChatResult::Stream(Box::pin(stream)))
        } else {
            let value = resp
                .json()
                .await
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            Ok(ChatResult::Complete(value))
        }
    }

    async fn embeddings(&self, mut body: Value, ctx: &CallContext) -> Result<Value, ProviderError> {
        body["model"] = json!(ctx.upstream_model);
        let resp = self.post_json("embeddings", body, ctx, false).await?;
        finish_json(resp).await
    }

    async fn images(&self, mut body: Value, ctx: &CallContext) -> Result<Value, ProviderError> {
        body["model"] = json!(ctx.upstream_model);
        let resp = self
            .post_json("images/generations", body, ctx, false)
            .await?;
        finish_json(resp).await
    }

    async fn speech(&self, mut body: Value, ctx: &CallContext) -> Result<AudioOut, ProviderError> {
        body["model"] = json!(ctx.upstream_model);
        let resp = self.post_json("audio/speech", body, ctx, false).await?;
        if !resp.status().is_success() {
            return Err(error_from_response(resp).await);
        }
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();
        let data = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(AudioOut { content_type, data })
    }

    async fn transcriptions(
        &self,
        upload: AudioUpload,
        ctx: &CallContext,
    ) -> Result<Value, ProviderError> {
        let url = self.url("audio/transcriptions");
        let http = self.http.clone();
        let api_key = self.api_key.clone();
        let headers = self.headers.clone();
        let timeout = ctx.timeout;
        let upload_name = upload.file_name.clone();
        let upload_mime = upload.mime.clone();
        let upload_bytes = upload.bytes.clone();
        let fields = upload.fields.clone();
        let model = ctx.upstream_model.clone();
        let resp = send_with_retry(
            move || {
                let part = reqwest::multipart::Part::bytes(upload_bytes.to_vec())
                    .file_name(upload_name.clone())
                    .mime_str(&upload_mime)
                    .unwrap_or_else(|_| {
                        reqwest::multipart::Part::bytes(upload_bytes.to_vec())
                            .file_name(upload_name.clone())
                    });
                let mut form = reqwest::multipart::Form::new()
                    .text("model", model.clone())
                    .part("file", part);
                for (k, v) in &fields {
                    if k != "model" && k != "file" {
                        form = form.text(k.clone(), v.clone());
                    }
                }
                let mut req = http.post(&url).multipart(form);
                if !timeout.is_zero() {
                    req = req.timeout(timeout.max(Duration::from_secs(180)));
                }
                if let Some(key) = &api_key {
                    req = req.bearer_auth(key);
                }
                for (k, v) in &headers {
                    req = req.header(k, v);
                }
                req
            },
            ctx.max_retries,
            ctx.backoff_ms,
        )
        .await?;
        finish_json(resp).await
    }
}

fn groq_reasoning_vocab(name: &str, base_url: &str) -> bool {
    name.eq_ignore_ascii_case("groq") || base_url.to_ascii_lowercase().contains("groq.com")
}

/// Map Hermes "thinking off" (`none`) and extended efforts onto Groq's vocabulary.
fn clamp_reasoning_effort(body: &mut Value) {
    let Some(raw) = body.get("reasoning_effort").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(mapped) = map_groq_effort(raw) else {
        return;
    };
    body["reasoning_effort"] = json!(mapped);
}

/// Fold OpenAI's `developer` role back into `system` before an OpenAI-compatible
/// upstream sees it. Clients built for newer OpenAI models emit `developer` for
/// the instruction message; upstreams such as DeepSeek only understand `system`
/// and reject anything else with HTTP 422 ("unknown variant `developer`").
/// `system` is accepted everywhere, so the rewrite is safe.
fn normalize_developer_role(body: &mut Value) {
    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for message in messages {
        let Some(role) = message.get_mut("role") else {
            continue;
        };
        if role.as_str() == Some("developer") {
            *role = json!("system");
        }
    }
}

/// Luna's chat endpoint rejects function tools unless reasoning is `none`.
fn relax_luna_tool_reasoning(body: &mut Value, upstream: &str) {
    if upstream != "gpt-6-luna" {
        return;
    }
    let has_tools = body
        .get("tools")
        .and_then(|t| t.as_array())
        .is_some_and(|tools| !tools.is_empty());
    if !has_tools {
        return;
    }
    let effort = body
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if effort.is_empty() || effort.eq_ignore_ascii_case("none") {
        return;
    }
    body["reasoning_effort"] = json!("none");
}

fn map_groq_effort(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" | "medium" | "high" => None,
        "xhigh" | "max" | "ultra" => Some("high"),
        _ => Some("low"),
    }
}

async fn finish_json(resp: reqwest::Response) -> Result<Value, ProviderError> {
    if !resp.status().is_success() {
        return Err(error_from_response(resp).await);
    }
    resp.json()
        .await
        .map_err(|e| ProviderError::Transport(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use airouter_config::ProviderType;
    use std::time::Duration;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(base: &str) -> OpenAiProvider {
        OpenAiProvider::new(
            &ResolvedProvider {
                name: "openai".into(),
                provider_type: ProviderType::Openai,
                base_url: base.trim_end_matches('/').to_string(),
                api_key: Some("sk-test".into()),
                key_status: "literal".into(),
                headers: BTreeMap::new(),
                timeout: Duration::from_secs(10),
                enabled: true,
            },
            crate::http_client(),
        )
    }

    #[test]
    fn luna_tools_force_reasoning_effort_none() {
        let mut body = json!({
            "tools": [{"type": "function", "function": {"name": "x"}}],
            "reasoning_effort": "low"
        });
        relax_luna_tool_reasoning(&mut body, "gpt-6-luna");
        assert_eq!(body["reasoning_effort"], "none");
        body["reasoning_effort"] = json!("low");
        relax_luna_tool_reasoning(&mut body, "gpt-6-sol");
        assert_eq!(body["reasoning_effort"], "low");
    }

    #[test]
    fn groq_effort_maps_none_to_low_and_keeps_medium() {
        assert_eq!(map_groq_effort("none"), Some("low"));
        assert_eq!(map_groq_effort("minimal"), Some("low"));
        assert_eq!(map_groq_effort("max"), Some("high"));
        assert_eq!(map_groq_effort("medium"), None);
    }

    #[tokio::test]
    async fn groq_clamps_reasoning_effort_before_upstream() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("\"reasoning_effort\":\"low\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}]
            })))
            .mount(&server)
            .await;

        let p = OpenAiProvider::new(
            &ResolvedProvider {
                name: "groq".into(),
                provider_type: ProviderType::OpenaiCompat,
                base_url: format!("{}/v1", server.uri()),
                api_key: Some("gsk-test".into()),
                key_status: "literal".into(),
                headers: BTreeMap::new(),
                timeout: Duration::from_secs(10),
                enabled: true,
            },
            crate::http_client(),
        );
        let ctx = CallContext {
            client_model: "groq/openai/gpt-oss-120b".into(),
            upstream_model: "openai/gpt-oss-120b".into(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            backoff_ms: 10,
        };
        p.chat(
            json!({
                "model": "groq/openai/gpt-oss-120b",
                "reasoning_effort": "none",
                "messages": [{"role": "user", "content": "title"}]
            }),
            &ctx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn passthrough_rewrites_model_and_keeps_messages() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_string_contains("\"model\":\"gpt-4o-mini\""))
            .and(body_string_contains("hello"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-1",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 3, "completion_tokens": 1}
            })))
            .mount(&server)
            .await;

        let p = provider(&format!("{}/v1", server.uri()));
        let ctx = CallContext {
            client_model: "openai/gpt-4o-mini".into(),
            upstream_model: "gpt-4o-mini".into(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            backoff_ms: 10,
        };
        let result = p
            .chat(
                json!({"model": "openai/gpt-4o-mini", "messages": [{"role": "user", "content": "hello"}]}),
                &ctx,
            )
            .await
            .unwrap();
        match result {
            ChatResult::Complete(v) => assert_eq!(v["choices"][0]["message"]["content"], "hi"),
            ChatResult::Stream(_) => panic!("expected json"),
        }
    }

    #[tokio::test]
    async fn retries_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"embedding": [0.1], "index": 0}],
                "usage": {"prompt_tokens": 1, "total_tokens": 1}
            })))
            .mount(&server)
            .await;

        let p = provider(&format!("{}/v1", server.uri()));
        let ctx = CallContext {
            client_model: "openai/text-embedding-3-small".into(),
            upstream_model: "text-embedding-3-small".into(),
            timeout: Duration::from_secs(5),
            max_retries: 2,
            backoff_ms: 1,
        };
        let body = p
            .embeddings(json!({"model": "x", "input": "a"}), &ctx)
            .await
            .unwrap();
        assert_eq!(body["data"][0]["embedding"][0], 0.1);
    }

    #[test]
    fn developer_role_folds_to_system() {
        let mut body = json!({
            "messages": [
                {"role": "developer", "content": "be brief"},
                {"role": "user", "content": "hi"},
                {"role": "developer", "content": "mid convo"}
            ]
        });
        normalize_developer_role(&mut body);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][2]["role"], "system");
    }

    fn compat_provider(base: &str) -> OpenAiProvider {
        OpenAiProvider::new(
            &ResolvedProvider {
                name: "deepseek".into(),
                provider_type: ProviderType::OpenaiCompat,
                base_url: base.trim_end_matches('/').to_string(),
                api_key: Some("sk-test".into()),
                key_status: "literal".into(),
                headers: BTreeMap::new(),
                timeout: Duration::from_secs(10),
                enabled: true,
            },
            crate::http_client(),
        )
    }

    #[tokio::test]
    async fn openai_compat_rewrites_developer_role_before_upstream() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("\"role\":\"system\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}]
            })))
            .mount(&server)
            .await;

        let p = compat_provider(&format!("{}/v1", server.uri()));
        let ctx = CallContext {
            client_model: "deepseek/deepseek-flash".into(),
            upstream_model: "deepseek-flash".into(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            backoff_ms: 10,
        };
        p.chat(
            json!({
                "model": "deepseek/deepseek-flash",
                "messages": [
                    {"role": "developer", "content": "be brief"},
                    {"role": "user", "content": "hi"}
                ]
            }),
            &ctx,
        )
        .await
        .unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body["messages"][1]["role"] == "user");
    }

    #[tokio::test]
    async fn native_openai_keeps_developer_role() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("\"role\":\"developer\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}]
            })))
            .mount(&server)
            .await;

        let p = provider(&format!("{}/v1", server.uri()));
        let ctx = CallContext {
            client_model: "openai/gpt-6-sol".into(),
            upstream_model: "gpt-6-sol".into(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            backoff_ms: 10,
        };
        p.chat(
            json!({
                "model": "openai/gpt-6-sol",
                "messages": [{"role": "developer", "content": "be brief"}]
            }),
            &ctx,
        )
        .await
        .unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["messages"][0]["role"], "developer");
    }
}
