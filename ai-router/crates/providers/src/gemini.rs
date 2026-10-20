use std::collections::BTreeMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use airouter_config::ResolvedProvider;
use airouter_core::now_unix;
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::retry::{error_from_response, send_with_retry};
use crate::sse::SseDecoder;
use crate::{CallContext, ChatResult, Provider, ProviderError};

pub struct GeminiProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
    http: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(provider: &ResolvedProvider, http: reqwest::Client) -> Self {
        Self {
            name: provider.name.clone(),
            base_url: provider.base_url.trim_end_matches('/').to_string(),
            api_key: provider.api_key.clone(),
            headers: provider.headers.clone(),
            http,
        }
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, body: Value, ctx: &CallContext) -> Result<ChatResult, ProviderError> {
        let stream = body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let payload = translate_request(&body)?;
        let model = urlencoding::encode(&ctx.upstream_model);
        let url = if stream {
            format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                self.base_url, model
            )
        } else {
            format!("{}/models/{}:generateContent", self.base_url, model)
        };
        let http = self.http.clone();
        let api_key = self.api_key.clone();
        let headers = self.headers.clone();
        let timeout = ctx.timeout;
        let resp = send_with_retry(
            {
                let payload = payload.clone();
                let url = url.clone();
                move || {
                    let mut req = http.post(&url).json(&payload);
                    if !stream && !timeout.is_zero() {
                        req = req.timeout(timeout);
                    }
                    if let Some(key) = &api_key {
                        req = req.header("x-goog-api-key", key);
                    }
                    for (k, v) in &headers {
                        req = req.header(k, v);
                    }
                    req
                }
            },
            ctx.max_retries,
            ctx.backoff_ms,
        )
        .await?;
        if !resp.status().is_success() {
            return Err(error_from_response(resp).await);
        }
        if stream {
            let inner = resp
                .bytes_stream()
                .map(|item| item.map_err(|e| ProviderError::Transport(e.to_string())));
            let translated = GeminiSse {
                inner: Box::pin(inner),
                decoder: SseDecoder::default(),
                state: GeminiStreamState::new(&ctx.client_model),
                pending: std::collections::VecDeque::new(),
                finished: false,
            };
            Ok(ChatResult::Stream(Box::pin(translated)))
        } else {
            let value: Value = resp
                .json()
                .await
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            Ok(ChatResult::Complete(translate_response(
                &value,
                &ctx.client_model,
            )))
        }
    }

    async fn embeddings(&self, body: Value, ctx: &CallContext) -> Result<Value, ProviderError> {
        let inputs = embedding_inputs(&body)?;
        let model = urlencoding::encode(&ctx.upstream_model);
        let url = if inputs.len() == 1 {
            format!("{}/models/{}:embedContent", self.base_url, model)
        } else {
            format!("{}/models/{}:batchEmbedContents", self.base_url, model)
        };
        let payload = if inputs.len() == 1 {
            json!({"content": {"parts": [{"text": inputs[0]}]}})
        } else {
            let requests: Vec<Value> = inputs
                .iter()
                .map(|text| {
                    json!({
                        "model": format!("models/{}", ctx.upstream_model),
                        "content": {"parts": [{"text": text}]}
                    })
                })
                .collect();
            json!({"requests": requests})
        };
        let http = self.http.clone();
        let api_key = self.api_key.clone();
        let headers = self.headers.clone();
        let timeout = ctx.timeout;
        let resp = send_with_retry(
            {
                let url = url.clone();
                let payload = payload.clone();
                move || {
                    let mut req = http.post(&url).json(&payload);
                    if !timeout.is_zero() {
                        req = req.timeout(timeout);
                    }
                    if let Some(key) = &api_key {
                        req = req.header("x-goog-api-key", key);
                    }
                    for (k, v) in &headers {
                        req = req.header(k, v);
                    }
                    req
                }
            },
            ctx.max_retries,
            ctx.backoff_ms,
        )
        .await?;
        if !resp.status().is_success() {
            return Err(error_from_response(resp).await);
        }
        let value: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(translate_embeddings(&value, &ctx.client_model, &inputs))
    }
}

use futures::StreamExt;

fn embedding_inputs(body: &Value) -> Result<Vec<String>, ProviderError> {
    match body.get("input") {
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    _ => {
                        return Err(ProviderError::Invalid(
                            "gemini embeddings accept strings only".into(),
                        ))
                    }
                }
            }
            if out.is_empty() {
                return Err(ProviderError::Invalid("input is empty".into()));
            }
            Ok(out)
        }
        _ => Err(ProviderError::Invalid(
            "input must be a string or array of strings".into(),
        )),
    }
}

fn translate_embeddings(body: &Value, model: &str, inputs: &[String]) -> Value {
    let vectors: Vec<Vec<f64>> =
        if let Some(values) = body.pointer("/embedding/values").and_then(|v| v.as_array()) {
            vec![numbers(values)]
        } else if let Some(list) = body.get("embeddings").and_then(|v| v.as_array()) {
            list.iter()
                .map(|e| {
                    numbers(
                        e.get("values")
                            .and_then(|v| v.as_array())
                            .unwrap_or(&Vec::new()),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
    let data: Vec<Value> = vectors
        .into_iter()
        .enumerate()
        .map(|(i, embedding)| json!({"object": "embedding", "index": i, "embedding": embedding}))
        .collect();
    let tokens: u64 = inputs
        .iter()
        .map(|s| (s.chars().count() as u64 / 4).max(1))
        .sum();
    json!({
        "object": "list",
        "data": data,
        "model": model,
        "usage": {"prompt_tokens": tokens, "total_tokens": tokens}
    })
}

fn numbers(values: &[Value]) -> Vec<f64> {
    values.iter().filter_map(|v| v.as_f64()).collect()
}

pub fn translate_request(body: &Value) -> Result<Value, ProviderError> {
    let messages = body
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or_else(|| ProviderError::Invalid("messages must be an array".into()))?;
    let mut system = Vec::new();
    let mut contents: Vec<Value> = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        match role {
            "system" | "developer" => {
                let text = plain_text(msg.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    system.push(text);
                }
            }
            "user" => push_content(
                &mut contents,
                "user",
                parts_from_content(msg.get("content").unwrap_or(&Value::Null))?,
            ),
            "assistant" => {
                let mut parts = parts_from_content(msg.get("content").unwrap_or(&Value::Null))?;
                if let Some(calls) = msg.get("tool_calls").and_then(|c| c.as_array()) {
                    for call in calls {
                        let name = call
                            .pointer("/function/name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let arguments = call
                            .pointer("/function/arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        let args = serde_json::from_str::<Value>(arguments)
                            .unwrap_or_else(|_| json!({"raw": arguments}));
                        parts.push(json!({"functionCall": {"name": name, "args": args}}));
                    }
                }
                push_content(&mut contents, "model", parts);
            }
            "tool" => {
                let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                let content = plain_text(msg.get("content").unwrap_or(&Value::Null));
                let response = serde_json::from_str::<Value>(&content)
                    .unwrap_or_else(|_| json!({"result": content}));
                push_content(
                    &mut contents,
                    "user",
                    vec![json!({"functionResponse": {"name": name, "response": response}})],
                );
            }
            other => {
                return Err(ProviderError::Invalid(format!(
                    "unsupported message role '{other}'"
                )))
            }
        }
    }
    if contents.is_empty() {
        return Err(ProviderError::Invalid(
            "no user or assistant messages".into(),
        ));
    }
    let mut payload = json!({"contents": contents});
    if !system.is_empty() {
        payload["systemInstruction"] = json!({"parts": [{"text": system.join("\n\n")}]});
    }
    let mut generation = json!({});
    if let Some(t) = body.get("temperature") {
        generation["temperature"] = t.clone();
    }
    if let Some(t) = body.get("top_p") {
        generation["topP"] = t.clone();
    }
    if let Some(n) = body
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| body.get("max_completion_tokens").and_then(|v| v.as_u64()))
    {
        generation["maxOutputTokens"] = json!(n);
    }
    if let Some(stop) = body.get("stop") {
        let sequences = match stop {
            Value::String(s) => vec![s.clone()],
            Value::Array(items) => items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        };
        if !sequences.is_empty() {
            generation["stopSequences"] = json!(sequences);
        }
    }
    if generation.as_object().is_some_and(|o| !o.is_empty()) {
        payload["generationConfig"] = generation;
    }
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let decls: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()).unwrap_or("function") == "function")
            .map(|t| {
                let f = t.get("function").cloned().unwrap_or_else(|| json!({}));
                json!({
                    "name": f.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": f.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "parameters": f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}}))
                })
            })
            .collect();
        if !decls.is_empty() {
            payload["tools"] = json!([{"functionDeclarations": decls}]);
        }
    }
    Ok(payload)
}

fn push_content(out: &mut Vec<Value>, role: &str, parts: Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.get("role").and_then(|r| r.as_str()) == Some(role) {
            if let Some(arr) = last.get_mut("parts").and_then(|p| p.as_array_mut()) {
                arr.extend(parts);
                return;
            }
        }
    }
    out.push(json!({"role": role, "parts": parts}));
}

fn plain_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn parts_from_content(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(s) if s.is_empty() => Ok(Vec::new()),
        Value::String(s) => Ok(vec![json!({"text": s})]),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                match part.get("type").and_then(|t| t.as_str()).unwrap_or("text") {
                    "text" => {
                        let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if !text.is_empty() {
                            out.push(json!({"text": text}));
                        }
                    }
                    "image_url" => {
                        let url = part
                            .pointer("/image_url/url")
                            .and_then(|u| u.as_str())
                            .unwrap_or("");
                        if let Some(block) = gemini_image(url) {
                            out.push(block);
                        }
                    }
                    _ => {}
                }
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

fn gemini_image(url: &str) -> Option<Value> {
    if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        let mime = meta.split(';').next().unwrap_or("image/png");
        return Some(json!({"inlineData": {"mimeType": mime, "data": data}}));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(json!({"fileData": {"mimeType": "image/jpeg", "fileUri": url}}));
    }
    None
}

pub fn translate_response(body: &Value, client_model: &str) -> Value {
    let parts = body
        .pointer("/candidates/0/content/parts")
        .and_then(|p| p.as_array());
    let mut text = String::new();
    let mut tools = Vec::new();
    if let Some(parts) = parts {
        for part in parts {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
            if let Some(call) = part.get("functionCall") {
                let index = tools.len();
                tools.push(json!({
                    "id": format!("call_{index}"),
                    "type": "function",
                    "function": {
                        "name": call.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "arguments": serde_json::to_string(call.get("args").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".into())
                    }
                }));
            }
        }
    }
    let finish = map_finish(
        body.pointer("/candidates/0/finishReason")
            .and_then(|v| v.as_str())
            .unwrap_or("STOP"),
    );
    let mut message = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
    if !tools.is_empty() {
        message["tool_calls"] = json!(tools);
    }
    let usage = body
        .get("usageMetadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let prompt = usage
        .get("promptTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("candidatesTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached = usage
        .get("cachedContentTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": now_unix(),
        "model": client_model,
        "choices": [{"index": 0, "message": message, "finish_reason": finish}],
        "usage": {
            "prompt_tokens": prompt,
            "completion_tokens": output,
            "total_tokens": prompt + output,
            "prompt_tokens_details": {"cached_tokens": cached}
        }
    })
}

fn map_finish(reason: &str) -> &'static str {
    match reason {
        "MAX_TOKENS" => "length",
        "SAFETY" | "RECITATION" | "BLOCKLIST" => "content_filter",
        "STOP" | "FINISH_REASON_UNSPECIFIED" | _ => "stop",
    }
}

struct GeminiStreamState {
    id: String,
    model: String,
    created: i64,
    started: bool,
    tool_index: u64,
}

impl GeminiStreamState {
    fn new(model: &str) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4().simple()),
            model: model.to_string(),
            created: now_unix(),
            started: false,
            tool_index: 0,
        }
    }

    fn frame(&self, delta: Value, finish: Option<&str>, usage: Option<Value>) -> Value {
        let mut chunk = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]
        });
        if finish.is_none() {
            chunk["choices"][0]["finish_reason"] = Value::Null;
        }
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        chunk
    }
}

struct GeminiSse {
    inner: crate::ByteStream,
    decoder: SseDecoder,
    state: GeminiStreamState,
    pending: std::collections::VecDeque<Bytes>,
    finished: bool,
}

impl Stream for GeminiSse {
    type Item = Result<Bytes, ProviderError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(chunk) = this.pending.pop_front() {
                return Poll::Ready(Some(Ok(chunk)));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    for ev in this.decoder.push(&bytes) {
                        emit_gemini(
                            &mut this.state,
                            &ev.data,
                            &mut this.pending,
                            &mut this.finished,
                        );
                    }
                }
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err))),
                Poll::Ready(None) => {
                    if let Some(ev) = this.decoder.finish() {
                        emit_gemini(
                            &mut this.state,
                            &ev.data,
                            &mut this.pending,
                            &mut this.finished,
                        );
                    }
                    if !this.finished {
                        this.pending
                            .push_back(Bytes::from_static(b"data: [DONE]\n\n"));
                        this.finished = true;
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn emit_gemini(
    state: &mut GeminiStreamState,
    data: &str,
    pending: &mut std::collections::VecDeque<Bytes>,
    finished: &mut bool,
) {
    if data.trim() == "[DONE]" {
        pending.push_back(Bytes::from_static(b"data: [DONE]\n\n"));
        *finished = true;
        return;
    }
    let Ok(body) = serde_json::from_str::<Value>(data) else {
        return;
    };
    let mut frames = Vec::new();
    if !state.started {
        frames.push(state.frame(json!({"role": "assistant"}), None, None));
        state.started = true;
    }
    if let Some(parts) = body
        .pointer("/candidates/0/content/parts")
        .and_then(|p| p.as_array())
    {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    frames.push(state.frame(json!({"content": text}), None, None));
                }
            }
            if let Some(call) = part.get("functionCall") {
                let index = state.tool_index;
                state.tool_index += 1;
                let args = serde_json::to_string(call.get("args").unwrap_or(&json!({})))
                    .unwrap_or_else(|_| "{}".into());
                frames.push(state.frame(
                    json!({"tool_calls": [{
                        "index": index,
                        "id": format!("call_{index}"),
                        "type": "function",
                        "function": {"name": call.get("name").and_then(|v| v.as_str()).unwrap_or(""), "arguments": args}
                    }]}),
                    None,
                    None,
                ));
            }
        }
    }
    let finish = body
        .pointer("/candidates/0/finishReason")
        .and_then(|v| v.as_str());
    let usage = body.get("usageMetadata").map(|usage| {
        let prompt = usage
            .get("promptTokenCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("candidatesTokenCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cached = usage
            .get("cachedContentTokenCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        json!({
            "prompt_tokens": prompt,
            "completion_tokens": output,
            "total_tokens": prompt + output,
            "prompt_tokens_details": {"cached_tokens": cached}
        })
    });
    if finish.is_some() || usage.is_some() {
        frames.push(state.frame(json!({}), finish.map(map_finish), usage));
    }
    if !frames.is_empty() {
        let mut s = String::new();
        for frame in frames {
            s.push_str("data: ");
            s.push_str(&serde_json::to_string(&frame).unwrap_or_else(|_| "{}".into()));
            s.push_str("\n\n");
        }
        pending.push_back(Bytes::from(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_maps_system_tools_and_image() {
        let body = json!({
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": [
                    {"type": "text", "text": "see"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,QQ=="}}
                ]}
            ],
            "temperature": 0.2,
            "max_tokens": 32,
            "tools": [{"type": "function", "function": {"name": "lookup", "description": "d", "parameters": {"type": "object"}}}]
        });
        let out = translate_request(&body).unwrap();
        assert_eq!(out["systemInstruction"]["parts"][0]["text"], "be brief");
        assert_eq!(out["contents"][0]["role"], "user");
        assert_eq!(
            out["contents"][0]["parts"][1]["inlineData"]["mimeType"],
            "image/png"
        );
        assert_eq!(out["generationConfig"]["maxOutputTokens"], 32);
        assert_eq!(out["tools"][0]["functionDeclarations"][0]["name"], "lookup");
    }

    #[test]
    fn response_maps_function_call_and_usage() {
        let body = json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "done"},
                    {"functionCall": {"name": "lookup", "args": {"q": "x"}}}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 8, "candidatesTokenCount": 3, "cachedContentTokenCount": 2}
        });
        let out = translate_response(&body, "gemini/gemini-2.5-flash");
        assert_eq!(out["choices"][0]["message"]["content"], "done");
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(out["usage"]["prompt_tokens"], 8);
        assert_eq!(out["usage"]["prompt_tokens_details"]["cached_tokens"], 2);
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn embeddings_preserve_order() {
        let body = json!({"embeddings": [{"values": [0.1, 0.2]}, {"values": [0.3]}]});
        let out = translate_embeddings(
            &body,
            "gemini/text-embedding-004",
            &["ab".into(), "cd".into()],
        );
        assert_eq!(out["data"][1]["embedding"][0], 0.3);
        assert_eq!(out["data"][0]["index"], 0);
        assert!(out["usage"]["prompt_tokens"].as_u64().unwrap() >= 2);
    }
}
