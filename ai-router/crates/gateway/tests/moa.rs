use std::time::Duration;

use airouter_config::{save_file, ConfigFile};
use airouter_gateway::spawn_test;
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn completion(text: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1000, "completion_tokens": 500}
    })
}

async fn mount_text(mock: &MockServer, upstream: &str, text: &str) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains(format!("\"model\":\"{upstream}\"")))
        .respond_with(ResponseTemplate::new(200).set_body_json(completion(text)))
        .mount(mock)
        .await;
}

async fn boot(mock: &MockServer) -> (airouter_gateway::TestServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.json");
    let data_dir = dir.path().join("state");
    let raw = json!({
        "server": {
            "listen": "127.0.0.1:9",
            "admin_listen": "127.0.0.1:9",
            "data_dir": data_dir,
            "api_keys": [{"key": "test-token", "label": "cursor"}],
            "retry": {"max_retries": 0, "backoff_ms": 1}
        },
        "providers": {
            "openai": {"type": "openai", "base_url": format!("{}/v1", mock.uri()), "api_key": "sk-test"}
        },
        "models": [
            {"id": "openai/cheap", "provider": "openai", "upstream": "cheap", "modality": "chat", "capabilities": ["tools", "vision"], "pricing": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}, "task_quality": {"chat": 0.90, "reasoning": 0.40}, "latency_ms_prior": 200},
            {"id": "openai/mid", "provider": "openai", "upstream": "mid", "modality": "chat", "capabilities": ["tools", "vision"], "pricing": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}, "task_quality": {"chat": 0.70, "reasoning": 0.80}, "latency_ms_prior": 400},
            {"id": "openai/other", "provider": "openai", "upstream": "other", "modality": "chat", "capabilities": ["tools", "vision"], "pricing": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}, "task_quality": {"chat": 0.60, "reasoning": 0.76}, "latency_ms_prior": 500},
            {"id": "openai/agg", "provider": "openai", "upstream": "agg", "modality": "chat", "capabilities": ["tools", "vision"], "pricing": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}, "task_quality": {"chat": 0.70, "reasoning": 0.82}, "latency_ms_prior": 600}
        ],
        "routing": {
            "default_profile": "adaptive",
            "profiles": {
                "adaptive": {
                    "mode": "balanced",
                    "strategy": "moa_on_demand",
                    "moa_profile": "balanced",
                    "safe_model": "openai/mid",
                    "confidence_floor": 0.0,
                    "lambda": 0.0,
                    "mu": 0.0,
                    "required_quality": {"chat": 0.40, "reasoning": 0.60},
                    "difficulty_step": 0.05,
                    "max_cost_usd": 5.0,
                    "tiers": [{"task": "*", "models": ["openai/cheap", "openai/mid", "openai/other", "openai/agg"]}],
                    "moa_triggers": {
                        "tasks": ["reasoning", "code"],
                        "min_difficulty": 2,
                        "quality_margin": 0.10,
                        "max_cost_usd": 5.0
                    }
                }
            }
        },
        "moa": {
            "enabled": true,
            "profiles": {
                "balanced": {
                    "proposers": [
                        {"model": "openai/mid"},
                        {"model": "openai/other"}
                    ],
                    "aggregator": "openai/agg",
                    "worker_model": "openai/agg",
                    "quorum": 2,
                    "max_concurrency": 2,
                    "proposer_max_tokens": 128,
                    "aggregator_max_tokens": 256,
                    "overall_timeout_ms": 5000,
                    "allow_tools": false,
                    "proposer_prompt": "PROPOSER_PROMPT_MARK",
                    "aggregator_prompt": "AGGREGATOR_PROMPT_MARK"
                }
            }
        }
    });
    let file: ConfigFile = serde_json::from_value(raw).unwrap();
    save_file(&cfg_path, &file).unwrap();
    let state = airouter_gateway::AppState::from_file(&cfg_path, file, vec![]).unwrap();
    let server = spawn_test(state).await;
    (server, dir)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap()
}

async fn post(http: &reqwest::Client, base: &str, body: Value, moa: Option<&str>) -> Value {
    let mut req = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&body);
    if let Some(moa) = moa {
        req = req.header("x-airouter-moa", moa);
    }
    let response = req.send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "{status} {text}");
    serde_json::from_str(&text).unwrap()
}

fn bodies_with(requests: &[wiremock::Request], needle: &str) -> Vec<String> {
    requests
        .iter()
        .filter_map(|req| {
            let text = String::from_utf8(req.body.clone()).unwrap_or_default();
            text.contains(needle).then_some(text)
        })
        .collect()
}

#[tokio::test]
async fn on_demand_moa_fans_out_only_when_the_single_model_is_short() {
    let mock = MockServer::start().await;
    mount_text(&mock, "cheap", "cheap-answer").await;
    mount_text(&mock, "mid", "mid-answer-unique").await;
    mount_text(&mock, "other", "other-answer-unique").await;
    mount_text(&mock, "agg", "final-answer").await;
    let (server, dir) = boot(&mock).await;
    let http = client();
    let base = format!("http://{}", server.addr);

    let easy = post(
        &http,
        &base,
        json!({"model": "auto/adaptive", "messages": [{"role": "user", "content": "hi"}]}),
        None,
    )
    .await;
    assert_eq!(easy["choices"][0]["message"]["content"], "cheap-answer");
    assert_eq!(easy["airouter"]["moa"]["mode"], "single");
    assert!(mock.received_requests().await.unwrap().len() == 1);

    let before = mock.received_requests().await.unwrap().len();
    let hard = post(
        &http,
        &base,
        json!({
            "model": "auto/adaptive",
            "messages": [{"role": "user", "content": "UNIQUE_PROMPT_ZZZ prove this step by step"}]
        }),
        None,
    )
    .await;
    let requests = mock.received_requests().await.unwrap();
    let new_requests = &requests[before..];
    assert_eq!(
        new_requests.len(),
        3,
        "expected two proposers and one aggregator"
    );
    let proposer_bodies = bodies_with(new_requests, "PROPOSER_PROMPT_MARK");
    assert_eq!(proposer_bodies.len(), 2);
    assert!(proposer_bodies
        .iter()
        .all(|body| body.contains("UNIQUE_PROMPT_ZZZ")));
    assert!(proposer_bodies
        .iter()
        .all(|body| body.contains("\"stream\":false")));
    let agg_bodies = bodies_with(new_requests, "AGGREGATOR_PROMPT_MARK");
    assert_eq!(agg_bodies.len(), 1);
    assert!(agg_bodies[0].contains("Candidate 1"));
    assert!(agg_bodies[0].contains("mid-answer-unique"));
    assert!(agg_bodies[0].contains("other-answer-unique"));
    assert_eq!(hard["choices"][0]["message"]["content"], "final-answer");
    assert_eq!(hard["airouter"]["moa"]["mode"], "full");
    assert_eq!(hard["airouter"]["moa"]["degraded"], false);
    assert_eq!(
        hard["airouter"]["moa"]["proposers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let before = requests.len();
    let follow = post(
        &http,
        &base,
        json!({
            "model": "auto/adaptive",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": "now make it shorter"}
            ]
        }),
        None,
    )
    .await;
    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len() - before, 1);
    assert_eq!(follow["airouter"]["moa"]["mode"], "worker");
    assert_eq!(follow["choices"][0]["message"]["content"], "final-answer");

    let before = requests.len();
    let hard_follow = post(
        &http,
        &base,
        json!({
            "model": "auto/adaptive",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": "prove this step by step"}
            ]
        }),
        None,
    )
    .await;
    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len() - before, 3);
    assert_eq!(hard_follow["airouter"]["moa"]["mode"], "full");

    let before = requests.len();
    let forced = post(
        &http,
        &base,
        json!({"model": "auto/adaptive", "messages": [{"role": "user", "content": "hi"}]}),
        Some("force"),
    )
    .await;
    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len() - before, 3);
    assert_eq!(forced["airouter"]["moa"]["mode"], "full");

    let before = requests.len();
    let off = post(
        &http,
        &base,
        json!({"model": "auto/adaptive", "messages": [{"role": "user", "content": "prove this step by step"}]}),
        Some("off"),
    )
    .await;
    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len() - before, 1);
    assert_eq!(off["airouter"]["moa"]["mode"], "single");

    let usage: Value = http
        .get(format!("{base}/v1/usage?since=0&by=model"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let summary_cost = usage["summary"]["cost_usd"].as_f64().unwrap();
    let row_cost: f64 = usage["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["cost_usd"].as_f64().unwrap())
        .sum();
    assert!((summary_cost - row_cost).abs() < 1e-6, "{usage}");
    assert!(
        usage["summary"]["requests"].as_u64().unwrap()
            < usage["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["requests"].as_u64().unwrap())
                .sum::<u64>()
    );
    let db = std::fs::read(dir.path().join("state/usage.db")).unwrap_or_default();
    let blob = String::from_utf8_lossy(&db);
    assert!(!blob.contains("UNIQUE_PROMPT_ZZZ"));
    assert!(!blob.contains("mid-answer-unique"));
}

#[tokio::test]
async fn quorum_and_aggregator_failure_degrade() {
    let mock = MockServer::start().await;
    mount_text(&mock, "cheap", "cheap-answer").await;
    mount_text(&mock, "mid", "mid-answer-longer-text").await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"model\":\"other\""))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"model\":\"agg\""))
        .respond_with(|req: &wiremock::Request| {
            let body = String::from_utf8_lossy(&req.body);
            if body.contains("PROPOSER_PROMPT_MARK") {
                ResponseTemplate::new(500).set_body_string("retry failed")
            } else {
                ResponseTemplate::new(200).set_body_json(completion("worker-answer"))
            }
        })
        .mount(&mock)
        .await;
    let (server, _dir) = boot(&mock).await;
    let http = client();
    let base = format!("http://{}", server.addr);
    let response = post(
        &http,
        &base,
        json!({
            "model": "auto/adaptive",
            "messages": [{"role": "user", "content": "prove this step by step"}]
        }),
        None,
    )
    .await;
    assert_eq!(response["airouter"]["moa"]["degraded"], true);
    assert_eq!(
        response["choices"][0]["message"]["content"],
        "worker-answer"
    );

    let mock = MockServer::start().await;
    mount_text(&mock, "mid", "mid-answer-longer-text").await;
    mount_text(&mock, "other", "other-short").await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"model\":\"agg\""))
        .and(body_string_contains("AGGREGATOR_PROMPT_MARK"))
        .respond_with(ResponseTemplate::new(500).set_body_string("agg down"))
        .mount(&mock)
        .await;
    mount_text(&mock, "agg", "worker-answer").await;
    let (server, _dir) = boot(&mock).await;
    let base = format!("http://{}", server.addr);
    let response = post(
        &http,
        &base,
        json!({
            "model": "auto/adaptive",
            "messages": [{"role": "user", "content": "prove this step by step"}]
        }),
        None,
    )
    .await;
    assert_eq!(response["airouter"]["moa"]["degraded"], true);
    assert_eq!(
        response["choices"][0]["message"]["content"],
        "mid-answer-longer-text"
    );
}

#[tokio::test]
async fn tools_skip_fanout_and_streaming_synthesizes_sse() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"model\":\"agg\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-tools",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "lookup", "arguments": "{}"}}]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        })))
        .mount(&mock)
        .await;
    mount_text(&mock, "cheap", "cheap-answer").await;
    mount_text(&mock, "mid", "mid-answer").await;
    mount_text(&mock, "other", "other-answer").await;
    let (server, _dir) = boot(&mock).await;
    let http = client();
    let base = format!("http://{}", server.addr);
    let response = post(
        &http,
        &base,
        json!({
            "model": "auto/adaptive",
            "messages": [{"role": "user", "content": "fix the bug in ```rust fn main() {}"}],
            "tools": [{"type": "function", "function": {"name": "lookup"}}]
        }),
        None,
    )
    .await;
    assert_eq!(response["airouter"]["moa"]["mode"], "worker");
    assert_eq!(
        response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "lookup"
    );
    assert_eq!(mock.received_requests().await.unwrap().len(), 1);

    let mock = MockServer::start().await;
    mount_text(&mock, "mid", "mid-answer").await;
    mount_text(&mock, "other", "other-answer").await;
    mount_text(&mock, "agg", "streamed-final").await;
    let (server, _dir) = boot(&mock).await;
    let base = format!("http://{}", server.addr);
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&json!({
            "model": "auto/adaptive",
            "stream": true,
            "messages": [{"role": "user", "content": "prove this step by step"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    assert_eq!(
        response
            .headers()
            .get("x-airouter-moa")
            .and_then(|v| v.to_str().ok()),
        Some("full")
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("streamed-final"), "{body}");
    assert!(body.contains("[DONE]"), "{body}");
}
