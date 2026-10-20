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

pub struct AnthropicProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
    http: reqwest::Client,
}

impl AnthropicProvider {
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
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, body: Value, ctx: &CallContext) -> Result<ChatResult, ProviderError> {
        let stream = body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut payload = translate_request(&body, &ctx.upstream_model)?;
        payload["stream"] = json!(stream);
        let url = format!("{}/v1/messages", self.base_url);
        let http = self.http.clone();
        let api_key = self.api_key.clone();
        let headers = self.headers.clone();
        let timeout = ctx.timeout;
        let resp = send_with_retry(
            {
                let payload = payload.clone();
                let url = url.clone();
                move || {
                    let mut req = http
                        .post(&url)
                        .header("anthropic-version", "2023-06-01")
                        .json(&payload);
                    if !stream && !timeout.is_zero() {
                        req = req.timeout(timeout);
                    }
                    if let Some(key) = &api_key {
                        req = req.header("x-api-key", key);
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
            let inner = resp.bytes_stream().map_err_stream();
            let translated = AnthropicSse {
                inner: Box::pin(inner),
                decoder: SseDecoder::default(),
                state: AnthropicStreamState::new(&ctx.client_model),
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
}

trait MapErrStream: Stream<Item = Result<Bytes, reqwest::Error>> + Send {
    fn map_err_stream(self) -> impl Stream<Item = Result<Bytes, ProviderError>> + Send;
}

impl<S> MapErrStream for S
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send,
{
    fn map_err_stream(self) -> impl Stream<Item = Result<Bytes, ProviderError>> + Send {
        futures::StreamExt::map(self, |r| {
            r.map_err(|e| ProviderError::Transport(e.to_string()))
        })
    }
}

struct AnthropicSse {
    inner: crate::ByteStream,
    decoder: SseDecoder,
    state: AnthropicStreamState,
    pending: std::collections::VecDeque<Bytes>,
    finished: bool,
}

impl Stream for AnthropicSse {
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
                Poll::Ready(Some(Ok(bytes))) => enqueue(
                    &mut this.decoder,
                    &mut this.state,
                    &bytes,
                    &mut this.pending,
                    &mut this.finished,
                ),
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err))),
                Poll::Ready(None) => {
                    if let Some(ev) = this.decoder.finish() {
                        push_event(
                            &mut this.state,
                            &ev.event,
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

fn enqueue(
    decoder: &mut SseDecoder,
    state: &mut AnthropicStreamState,
    bytes: &[u8],
    pending: &mut std::collections::VecDeque<Bytes>,
    finished: &mut bool,
) {
    for ev in decoder.push(bytes) {
        push_event(state, &ev.event, &ev.data, pending, finished);
    }
}

fn push_event(
    state: &mut AnthropicStreamState,
    event: &str,
    data: &str,
    pending: &mut std::collections::VecDeque<Bytes>,
    finished: &mut bool,
) {
    let parsed: Value = serde_json::from_str(data).unwrap_or(Value::Null);
    let event_name = if event.is_empty() {
        parsed.get("type").and_then(|t| t.as_str()).unwrap_or("")
    } else {
        event
    };
    let out = chunks_for_event(state, event_name, &parsed);
    if !out.chunks.is_empty() || out.done {
        pending.push_back(encode_chunks(&out.chunks, out.done));
    }
    if out.done {
        *finished = true;
    }
}

fn encode_chunks(chunks: &[Value], done: bool) -> Bytes {
    let mut s = String::new();
    for chunk in chunks {
        s.push_str("data: ");
        s.push_str(&serde_json::to_string(chunk).unwrap_or_else(|_| "{}".into()));
        s.push_str("\n\n");
    }
    if done {
        s.push_str("data: [DONE]\n\n");
    }
    Bytes::from(s)
}

pub fn translate_request(body: &Value, upstream_model: &str) -> Result<Value, ProviderError> {
    let messages = body
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or_else(|| ProviderError::Invalid("messages must be an array".into()))?;

    let mut system_parts = Vec::new();
    let mut out_messages: Vec<Value> = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        match role {
            "system" | "developer" => {
                let text = content_to_text(msg.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    system_parts.push(text);
                }
            }
            "user" => {
                let blocks = content_to_blocks(msg.get("content").unwrap_or(&Value::Null))?;
                push_merged(&mut out_messages, "user", blocks);
            }
            "assistant" => {
                let mut blocks = content_to_blocks(msg.get("content").unwrap_or(&Value::Null))?;
                if let Some(calls) = msg.get("tool_calls").and_then(|c| c.as_array()) {
                    for call in calls {
                        let id = call.get("id").and_then(|v| v.as_str()).unwrap_or("tool");
                        let name = call
                            .pointer("/function/name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let arguments = call
                            .pointer("/function/arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        let input = serde_json::from_str::<Value>(arguments)
                            .unwrap_or_else(|_| json!({ "raw": arguments }));
                        blocks.push(
                            json!({"type": "tool_use", "id": id, "name": name, "input": input}),
                        );
                    }
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": ""}));
                }
                push_merged(&mut out_messages, "assistant", blocks);
            }
            "tool" => {
                let id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let content = content_to_text(msg.get("content").unwrap_or(&Value::Null));
                let block = json!({"type": "tool_result", "tool_use_id": id, "content": content});
                push_merged(&mut out_messages, "user", vec![block]);
            }
            other => {
                return Err(ProviderError::Invalid(format!(
                    "unsupported message role '{other}'"
                )));
            }
        }
    }

    if out_messages.is_empty() {
        return Err(ProviderError::Invalid(
            "no user or assistant messages".into(),
        ));
    }

    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| body.get("max_completion_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(4096);

    let mut payload = json!({
        "model": upstream_model,
        "max_tokens": max_tokens,
        "messages": out_messages,
    });
    if !system_parts.is_empty() {
        payload["system"] = json!(system_parts.join("\n\n"));
    }
    if let Some(t) = body.get("temperature") {
        payload["temperature"] = t.clone();
    }
    if let Some(t) = body.get("top_p") {
        payload["top_p"] = t.clone();
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
            payload["stop_sequences"] = json!(sequences);
        }
    }
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let mapped: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()).unwrap_or("function") == "function")
            .map(|t| {
                let f = t.get("function").cloned().unwrap_or_else(|| json!({}));
                let schema = f
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                json!({
                    "name": f.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": f.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "input_schema": schema,
                })
            })
            .collect();
        if !mapped.is_empty() {
            payload["tools"] = json!(mapped);
        }
    }
    if let Some(choice) = body.get("tool_choice") {
        if let Some(mapped) = map_tool_choice(choice) {
            payload["tool_choice"] = mapped;
        }
    }
    Ok(payload)
}

fn map_tool_choice(choice: &Value) -> Option<Value> {
    match choice {
        Value::String(s) if s == "auto" => None,
        Value::String(s) if s == "none" => Some(json!({"type": "none"})),
        Value::String(s) if s == "required" => Some(json!({"type": "any"})),
        Value::Object(_) => {
            let name = choice.pointer("/function/name").and_then(|v| v.as_str())?;
            Some(json!({"type": "tool", "name": name}))
        }
        _ => None,
    }
}

fn push_merged(out: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.get("role").and_then(|r| r.as_str()) == Some(role) {
            if let Some(arr) = last.get_mut("content").and_then(|c| c.as_array_mut()) {
                arr.extend(blocks);
                return;
            }
        }
    }
    out.push(json!({"role": role, "content": blocks}));
}

fn content_to_text(content: &Value) -> String {
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

fn content_to_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(s) => {
            if s.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![json!({"type": "text", "text": s})])
            }
        }
        Value::Array(parts) => {
            let mut blocks = Vec::new();
            for part in parts {
                let kind = part.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                match kind {
                    "text" => {
                        let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if !text.is_empty() {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                    }
                    "image_url" => {
                        let url = part
                            .pointer("/image_url/url")
                            .and_then(|u| u.as_str())
                            .ok_or_else(|| {
                                ProviderError::Invalid("image_url missing url".into())
                            })?;
                        blocks.push(image_block(url));
                    }
                    _ => {}
                }
            }
            Ok(blocks)
        }
        _ => Ok(Vec::new()),
    }
}

fn image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, data)) = rest.split_once(',') {
            let media = meta.split(';').next().unwrap_or("image/png");
            return json!({
                "type": "image",
                "source": {"type": "base64", "media_type": media, "data": data}
            });
        }
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

pub fn translate_response(body: &Value, client_model: &str) -> Value {
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("msg");
    let (text, tool_calls) = split_content(body.get("content"));
    let finish = map_stop(
        body.get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn"),
    );
    let mut message = json!({"role": "assistant", "content": text});
    if !tool_calls.is_empty() {
        if text.as_ref().is_none_or(|s| s.is_empty()) {
            message["content"] = Value::Null;
        }
        message["tool_calls"] = json!(tool_calls);
    }
    json!({
        "id": format!("chatcmpl-{id}"),
        "object": "chat.completion",
        "created": now_unix(),
        "model": client_model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish
        }],
        "usage": usage_object(body.get("usage"))
    })
}

fn split_content(content: Option<&Value>) -> (Option<String>, Vec<Value>) {
    let Some(parts) = content.and_then(|c| c.as_array()) else {
        return (None, Vec::new());
    };
    let mut text = String::new();
    let mut tools = Vec::new();
    for part in parts {
        match part.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "text" => text.push_str(part.get("text").and_then(|t| t.as_str()).unwrap_or("")),
            "tool_use" => {
                let index = tools.len();
                tools.push(json!({
                    "id": part.get("id").and_then(|v| v.as_str()).unwrap_or("tool"),
                    "type": "function",
                    "index": index,
                    "function": {
                        "name": part.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "arguments": serde_json::to_string(part.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".into())
                    }
                }));
            }
            _ => {}
        }
    }
    let text = if text.is_empty() { None } else { Some(text) };
    (text, tools)
}

fn map_stop(reason: &str) -> &'static str {
    match reason {
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

fn usage_object(usage: Option<&Value>) -> Value {
    let usage = usage.cloned().unwrap_or_else(|| json!({}));
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    json!({
        "prompt_tokens": input + cache_read,
        "completion_tokens": output,
        "total_tokens": input + cache_read + cache_write + output,
        "prompt_tokens_details": {"cached_tokens": cache_read},
        "cache_creation_input_tokens": cache_write
    })
}

#[derive(Debug)]
pub struct AnthropicStreamState {
    id: String,
    model: String,
    created: i64,
    next_tool: u64,
    input_tokens: u64,
    cache_read: u64,
    cache_write: u64,
    output_tokens: u64,
    started: bool,
}

impl AnthropicStreamState {
    pub fn new(model: &str) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4().simple()),
            model: model.to_string(),
            created: now_unix(),
            next_tool: 0,
            input_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            output_tokens: 0,
            started: false,
        }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>, with_usage: bool) -> Value {
        let mut choice = json!({"index": 0, "delta": delta, "finish_reason": finish});
        if finish.is_none() {
            choice["finish_reason"] = Value::Null;
        }
        let mut chunk = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [choice]
        });
        if with_usage {
            chunk["usage"] = json!({
                "prompt_tokens": self.input_tokens + self.cache_read,
                "completion_tokens": self.output_tokens,
                "total_tokens": self.input_tokens + self.cache_read + self.cache_write + self.output_tokens,
                "prompt_tokens_details": {"cached_tokens": self.cache_read},
                "cache_creation_input_tokens": self.cache_write
            });
        }
        chunk
    }
}

pub struct StreamPieces {
    pub chunks: Vec<Value>,
    pub done: bool,
}

pub fn chunks_for_event(
    state: &mut AnthropicStreamState,
    event: &str,
    data: &Value,
) -> StreamPieces {
    let mut chunks = Vec::new();
    let mut done = false;
    match event {
        "message_start" => {
            if let Some(usage) = data.pointer("/message/usage") {
                state.input_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                state.cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                state.cache_write = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
            if let Some(id) = data.pointer("/message/id").and_then(|v| v.as_str()) {
                state.id = format!("chatcmpl-{id}");
            }
            chunks.push(state.chunk(json!({"role": "assistant"}), None, false));
            state.started = true;
        }
        "content_block_start" => {
            let block = data.get("content_block").cloned().unwrap_or(Value::Null);
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                let index = state.next_tool;
                state.next_tool += 1;
                chunks.push(state.chunk(
                    json!({"tool_calls": [{
                        "index": index,
                        "id": block.get("id").and_then(|v| v.as_str()).unwrap_or("tool"),
                        "type": "function",
                        "function": {"name": block.get("name").and_then(|v| v.as_str()).unwrap_or(""), "arguments": ""}
                    }]}),
                    None,
                    false,
                ));
            }
        }
        "content_block_delta" => {
            let delta = data.get("delta").cloned().unwrap_or(Value::Null);
            match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "text_delta" => {
                    let text = delta.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    if !text.is_empty() {
                        chunks.push(state.chunk(json!({"content": text}), None, false));
                    }
                }
                "input_json_delta" => {
                    let partial = delta
                        .get("partial_json")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    let index = state.next_tool.saturating_sub(1);
                    chunks.push(state.chunk(
                        json!({"tool_calls": [{"index": index, "function": {"arguments": partial}}]}),
                        None,
                        false,
                    ));
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(usage) = data.get("usage") {
                if let Some(n) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                    state.output_tokens = n;
                }
            }
            let reason = data
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("end_turn");
            chunks.push(state.chunk(json!({}), Some(map_stop(reason)), true));
        }
        "message_stop" => done = true,
        "error" => {
            let message = data
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("upstream stream error");
            chunks.push(state.chunk(
                json!({"content": format!("\n[{message}]")}),
                Some("stop"),
                false,
            ));
            done = true;
        }
        _ => {}
    }
    StreamPieces { chunks, done }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_system_tools_images_and_tool_results() {
        let body = json!({
            "model": "anthropic/claude-sonnet-4-5",
            "max_tokens": 128,
            "messages": [
                {"role": "system", "content": "be helpful"},
                {"role": "user", "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
                ]},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"q\":\"sf\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
            ],
            "tools": [{"type": "function", "function": {"name": "get_weather", "description": "weather", "parameters": {"type": "object"}}}],
            "tool_choice": "required"
        });
        let out = translate_request(&body, "claude-sonnet-4-5").unwrap();
        assert_eq!(out["model"], "claude-sonnet-4-5");
        assert_eq!(out["system"], "be helpful");
        assert_eq!(out["messages"][0]["role"], "user");
        assert_eq!(
            out["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(out["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(out["messages"][1]["content"][0]["input"]["q"], "sf");
        assert_eq!(out["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(out["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(out["tool_choice"]["type"], "any");
    }

    #[test]
    fn response_maps_tool_use_and_cache_usage() {
        let body = json!({
            "id": "msg_1",
            "content": [
                {"type": "text", "text": "ok"},
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"q": "sf"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 4, "cache_read_input_tokens": 6, "cache_creation_input_tokens": 2}
        });
        let out = translate_response(&body, "anthropic/claude-sonnet-4-5");
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out["choices"][0]["message"]["content"], "ok");
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
        assert_eq!(out["usage"]["prompt_tokens"], 16);
        assert_eq!(out["usage"]["prompt_tokens_details"]["cached_tokens"], 6);
        assert_eq!(out["usage"]["cache_creation_input_tokens"], 2);
        assert_eq!(out["model"], "anthropic/claude-sonnet-4-5");
    }

    #[test]
    fn stream_events_become_openai_chunks() {
        let mut state = AnthropicStreamState::new("anthropic/claude-sonnet-4-5");
        let start = chunks_for_event(
            &mut state,
            "message_start",
            &json!({"message": {"id": "msg_9", "usage": {"input_tokens": 3}}}),
        );
        assert_eq!(start.chunks[0]["choices"][0]["delta"]["role"], "assistant");
        let text = chunks_for_event(
            &mut state,
            "content_block_delta",
            &json!({"delta": {"type": "text_delta", "text": "Hi"}}),
        );
        assert_eq!(text.chunks[0]["choices"][0]["delta"]["content"], "Hi");
        let tool = chunks_for_event(
            &mut state,
            "content_block_start",
            &json!({"content_block": {"type": "tool_use", "id": "toolu_1", "name": "ping"}}),
        );
        assert_eq!(
            tool.chunks[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"],
            "ping"
        );
        let args = chunks_for_event(
            &mut state,
            "content_block_delta",
            &json!({"delta": {"type": "input_json_delta", "partial_json": "{\"a\":1}"}}),
        );
        assert_eq!(
            args.chunks[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
            "{\"a\":1}"
        );
        let end = chunks_for_event(
            &mut state,
            "message_delta",
            &json!({"delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
        );
        assert_eq!(end.chunks[0]["choices"][0]["finish_reason"], "stop");
        assert_eq!(end.chunks[0]["usage"]["prompt_tokens"], 3);
        assert_eq!(end.chunks[0]["usage"]["completion_tokens"], 2);
        let stop = chunks_for_event(&mut state, "message_stop", &json!({}));
        assert!(stop.done);
    }
}
