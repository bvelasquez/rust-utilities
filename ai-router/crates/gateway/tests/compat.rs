use std::time::Duration;

use airouter_config::{save_file, ConfigFile};
use airouter_gateway::{spawn_test, AppState};
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
            "openai": {"type": "openai", "base_url": format!("{}/v1", mock.uri()), "api_key": "sk-test"},
            "anthropic": {"type": "anthropic", "base_url": mock.uri(), "api_key": "ant-test"}
        },
        "models": [
            {"id": "openai/gpt-4o-mini", "provider": "openai", "upstream": "gpt-4o-mini", "modality": "chat", "pricing": {"input_per_mtok": 1.0, "output_per_mtok": 2.0}},
            {"id": "openai/text-embedding-3-small", "provider": "openai", "upstream": "text-embedding-3-small", "modality": "embedding", "pricing": {"embedding_per_mtok": 0.02}},
            {"id": "anthropic/claude", "provider": "anthropic", "upstream": "claude-sonnet-4-5", "modality": "chat", "pricing": {"input_per_mtok": 3.0, "output_per_mtok": 15.0}},
            {"id": "openai/disabled", "provider": "openai", "upstream": "nope", "modality": "chat", "enabled": false}
        ],
        "aliases": {"fast": "openai/gpt-4o-mini"}
    });
    let file: ConfigFile = serde_json::from_value(raw).unwrap();
    save_file(&cfg_path, &file).unwrap();
    let state = AppState::from_file(&cfg_path, file, vec![]).unwrap();
    let server = spawn_test(state).await;
    (server, dir)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn auth_models_chat_usage_and_anthropic() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .and(body_string_contains("\"model\":\"gpt-4o-mini\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1000, "completion_tokens": 500}
        })))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "ant-test"))
        .and(body_string_contains("\"model\":\"claude-sonnet-4-5\""))
        .and(body_string_contains("be helpful"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "hello from claude"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 11, "output_tokens": 4, "cache_read_input_tokens": 5}
        })))
        .mount(&mock)
        .await;

    let (server, _dir) = boot(&mock).await;
    let http = client();
    let base = format!("http://{}", server.addr);

    let open = http.get(format!("{base}/v1/models")).send().await.unwrap();
    assert_eq!(open.status(), 401);

    let models: Value = http
        .get(format!("{base}/v1/models"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(ids.contains(&"openai/gpt-4o-mini"));
    assert!(!ids.contains(&"openai/disabled"));
    assert!(models["data"][0].get("pricing").is_some());

    let chat: Value = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&json!({"model": "fast", "messages": [{"role": "user", "content": "ping"}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(chat["choices"][0]["message"]["content"], "pong");

    let missing = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&json!({"model": "nope", "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    let mismatch = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&json!({"model": "openai/text-embedding-3-small", "messages": [{"role": "user", "content": "x"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(mismatch.status(), 400);

    let claude: Value = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&json!({
            "model": "anthropic/claude",
            "messages": [
                {"role": "system", "content": "be helpful"},
                {"role": "user", "content": "hi"}
            ]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        claude["choices"][0]["message"]["content"],
        "hello from claude"
    );
    assert_eq!(claude["model"], "anthropic/claude");
    assert_eq!(claude["usage"]["prompt_tokens"], 16);

    let usage: Value = http
        .get(format!("{base}/v1/usage?since=0&by=model"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(usage["summary"]["requests"].as_u64().unwrap() >= 2);
    let mini = usage["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "fast")
        .unwrap();
    assert!((mini["cost_usd"].as_f64().unwrap() - 0.002).abs() < 1e-9);

    let videos = http
        .post(format!("{base}/v1/videos"))
        .bearer_auth("test-token")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(videos.status(), 501);

    let snap = http
        .get(format!("http://{}/snapshot", server.admin))
        .send()
        .await
        .unwrap();
    let snap_body = snap.text().await.unwrap();
    assert!(!snap_body.contains("sk-test"));
    assert!(!snap_body.contains("ant-test"));
    assert!(!snap_body.contains("test-token"));
}

#[tokio::test]
async fn streams_openai_sse_and_records_usage() {
    let mock = MockServer::start().await;
    let sse = "data: {\"id\":\"chatcmpl-s\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"chatcmpl-s\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":1}}\n\n\
data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"stream\":true"))
        .and(body_string_contains("include_usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&mock)
        .await;
    let (server, _dir) = boot(&mock).await;
    let http = client();
    let resp = http
        .post(format!("http://{}/v1/chat/completions", server.addr))
        .bearer_auth("test-token")
        .json(&json!({"model": "openai/gpt-4o-mini", "stream": true, "messages": [{"role": "user", "content": "hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(text.contains("Hi"));
    assert!(text.contains("[DONE]"));

    let http = client();
    let mut seen = false;
    for _ in 0..20 {
        let usage: Value = http
            .get(format!("http://{}/v1/usage?since=0&by=model", server.addr))
            .bearer_auth("test-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if usage["summary"]["requests"].as_u64().unwrap_or(0) >= 1 {
            seen = true;
            let row = &usage["rows"][0];
            assert_eq!(row["input_tokens"], 4, "usage body: {usage}");
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(seen, "stream usage was not recorded");
}

#[tokio::test]
async fn embeddings_images_and_speech_passthrough() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .and(body_string_contains("\"model\":\"text-embedding-3-small\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.5, 0.25]}],
            "usage": {"prompt_tokens": 500000, "total_tokens": 500000}
        })))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/images/generations"))
        .and(body_string_contains("\"model\":\"dall-e-3\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"b64_json": "aGk="}]
        })))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .and(body_string_contains("\"model\":\"tts-1\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .set_body_bytes(b"FAKEAUDIO".to_vec()),
        )
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.json");
    let raw = json!({
        "server": {
            "listen": "127.0.0.1:9",
            "admin_listen": "127.0.0.1:9",
            "data_dir": dir.path().join("state"),
            "api_keys": ["secret"],
            "retry": {"max_retries": 0, "backoff_ms": 1}
        },
        "providers": {
            "openai": {"type": "openai", "base_url": format!("{}/v1", mock.uri()), "api_key": "sk-test"}
        },
        "models": [
            {"id": "openai/text-embedding-3-small", "provider": "openai", "upstream": "text-embedding-3-small", "modality": "embedding", "pricing": {"embedding_per_mtok": 0.02}},
            {"id": "openai/dall-e-3", "provider": "openai", "upstream": "dall-e-3", "modality": "image", "pricing": {"per_image": 0.04}},
            {"id": "openai/tts-1", "provider": "openai", "upstream": "tts-1", "modality": "audio_tts", "pricing": {"per_1k_chars": 0.015}}
        ]
    });
    let file: ConfigFile = serde_json::from_value(raw).unwrap();
    save_file(&cfg_path, &file).unwrap();
    let server = spawn_test(AppState::from_file(&cfg_path, file, vec![]).unwrap()).await;
    let http = client();
    let base = format!("http://{}", server.addr);

    let embed: Value = http
        .post(format!("{base}/v1/embeddings"))
        .bearer_auth("secret")
        .json(&json!({"model": "openai/text-embedding-3-small", "input": "hello"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(embed["data"][0]["embedding"][0], 0.5);

    let image: Value = http
        .post(format!("{base}/v1/images/generations"))
        .bearer_auth("secret")
        .json(&json!({"model": "openai/dall-e-3", "prompt": "a cat", "n": 2}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(image["data"][0]["b64_json"], "aGk=");

    let speech = http
        .post(format!("{base}/v1/audio/speech"))
        .bearer_auth("secret")
        .json(&json!({"model": "openai/tts-1", "input": "hi", "voice": "alloy"}))
        .send()
        .await
        .unwrap();
    assert_eq!(speech.headers().get("content-type").unwrap(), "audio/mpeg");
    assert_eq!(speech.bytes().await.unwrap().as_ref(), b"FAKEAUDIO");

    let usage: Value = http
        .get(format!("{base}/v1/usage?since=0&by=model"))
        .bearer_auth("secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let cost = usage["summary"]["cost_usd"].as_f64().unwrap();
    // 500k embed tokens * $0.02 / 1M = 0.01; 2 images * 0.04 = 0.08; 2 chars * 0.015 / 1000 = 0.00003
    assert!((cost - 0.09003).abs() < 1e-6, "cost {cost}");
}

#[tokio::test]
async fn provider_key_is_stored_and_not_echoed() {
    let mock = MockServer::start().await;
    let secret = "sk-from-tui";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {secret}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        })))
        .mount(&mock)
        .await;

    let (server, dir) = boot(&mock).await;
    let http = client();
    let saved = http
        .post(format!("http://{}/providers/key", server.admin))
        .json(&json!({"name": "openai", "key": secret}))
        .send()
        .await
        .unwrap();
    assert!(saved.status().is_success());
    let body = saved.text().await.unwrap();
    assert!(!body.contains(secret));
    assert!(body.contains("key_present"));

    let snap = http
        .get(format!("http://{}/snapshot", server.admin))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!snap.contains(secret));
    assert!(snap.contains("\"key_present\":true"));

    let stored = std::fs::read_to_string(dir.path().join("secrets.json")).unwrap();
    assert!(stored.contains(secret));
    let cfg = std::fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(cfg.contains("secret:openai"));
    assert!(!cfg.contains(secret));

    let chat = http
        .post(format!("http://{}/v1/chat/completions", server.addr))
        .bearer_auth("test-token")
        .json(&json!({
            "model": "openai/gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert!(chat.status().is_success());
}

#[tokio::test]
async fn lists_upstream_models_and_adds_one() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "gpt-4o"}, {"id": "gpt-4o-mini"}]
        })))
        .mount(&mock)
        .await;

    let (server, dir) = boot(&mock).await;
    let http = client();
    let listed: Value = http
        .post(format!("http://{}/providers/models", server.admin))
        .json(&json!({"name": "openai"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = listed["models"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
    assert!(!listed.to_string().contains("sk-test"));

    let added: Value = http
        .post(format!("http://{}/models/add", server.admin))
        .json(&json!({"provider": "openai", "upstream": "gpt-4o"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(added["id"], "openai/gpt-4o");

    let cfg = std::fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(cfg.contains("openai/gpt-4o"));
}

#[tokio::test]
async fn auto_profile_fails_over_and_accepts_feedback() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": {"message": "busy", "type": "server_error"}
        })))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_auto",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "routed"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 20, "output_tokens": 4}
        })))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.json");
    let raw = json!({
        "server": {
            "listen": "127.0.0.1:9",
            "admin_listen": "127.0.0.1:9",
            "data_dir": dir.path().join("state"),
            "api_keys": [{"key": "test-token", "label": "cursor"}],
            "retry": {"max_retries": 0, "backoff_ms": 1}
        },
        "providers": {
            "openai": {"type": "openai", "base_url": format!("{}/v1", mock.uri()), "api_key": "sk-test"},
            "anthropic": {"type": "anthropic", "base_url": mock.uri(), "api_key": "ant-test"}
        },
        "models": [
            {"id": "openai/gpt-4o-mini", "provider": "openai", "upstream": "gpt-4o-mini", "modality": "chat", "pricing": {"input_per_mtok": 0.15, "output_per_mtok": 0.6}},
            {"id": "anthropic/claude", "provider": "anthropic", "upstream": "claude-sonnet-4-5", "modality": "chat", "pricing": {"input_per_mtok": 3.0, "output_per_mtok": 15.0}}
        ],
        "routing": {
            "default_profile": "cheap",
            "profiles": {
                "cheap": {
                    "safe_model": "anthropic/claude",
                    "lambda": 50.0,
                    "tiers": [{"task": "chat", "models": ["openai/gpt-4o-mini", "anthropic/claude"]}]
                }
            }
        }
    });
    let file: ConfigFile = serde_json::from_value(raw).unwrap();
    let profile = file.routing.profiles.get("cheap").expect("profile");
    assert_eq!(profile.tiers[0].models.len(), 2);
    save_file(&cfg_path, &file).unwrap();
    let state = AppState::from_file(&cfg_path, file, vec![]).unwrap();
    let server = spawn_test(state).await;
    let http = client();
    let base = format!("http://{}", server.addr);

    let models: Value = http
        .get(format!("{base}/v1/models"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(ids.contains(&"auto"));
    assert!(ids.contains(&"auto/cheap"));

    let resp = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth("test-token")
        .json(&json!({"model": "auto/cheap", "messages": [{"role": "user", "content": "hello"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let route = resp
        .headers()
        .get("x-airouter-route")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(route.contains("profile=cheap"));
    let request_id = resp
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let chat: Value = resp.json().await.unwrap();
    assert_eq!(chat["airouter"]["model"], "anthropic/claude");
    assert!(chat["airouter"]["reason"]
        .as_str()
        .unwrap()
        .contains("failover"));

    let rated = http
        .post(format!("{base}/v1/feedback"))
        .bearer_auth("test-token")
        .json(&json!({"request_id": request_id, "rating": "up"}))
        .send()
        .await
        .unwrap();
    assert_eq!(rated.status(), 200);

    let usage: Value = http
        .get(format!("{base}/v1/usage?since=0&by=route"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let keys: Vec<&str> = usage["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["key"].as_str())
        .collect();
    assert!(keys.contains(&"cheap"));
}
