//! HTTP API for driving the commander from agents, scripts, or the TUI.
//! All responses are `{success, data?, error?}` JSON.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;

use crate::broker::BrokerClient;
use crate::manager::{AgentCommand, AgentManager};
use crate::telegram::TelegramClient;

#[derive(Clone)]
pub struct ApiState {
    pub manager: Arc<AgentManager>,
    pub broker: BrokerClient,
    pub telegram: TelegramClient,
    pub config_path: PathBuf,
    pub started_at: Instant,
}

#[derive(Debug, Deserialize)]
pub struct PromptBody {
    pub message: String,
    #[serde(default)]
    pub behavior: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct ModelBody {
    pub model: String,
}
#[derive(Debug, Deserialize)]
pub struct LevelBody {
    pub level: String,
}
#[derive(Debug, Deserialize)]
pub struct BashBody {
    pub command: String,
}
#[derive(Debug, Deserialize)]
pub struct RawBody {
    pub json: String,
}
#[derive(Debug, Deserialize)]
pub struct CountBody {
    #[serde(default)]
    pub count: usize,
}
#[derive(Debug, Deserialize)]
pub struct DispatchBody {
    pub text: String,
}

pub fn build_router(
    manager: Arc<AgentManager>,
    broker: BrokerClient,
    telegram: TelegramClient,
    config_path: PathBuf,
    token: String,
) -> Router {
    let state = ApiState {
        manager,
        broker,
        telegram,
        config_path,
        started_at: Instant::now(),
    };

    let router = Router::new()
        .route("/health", get(health))
        .route("/state", get(snapshot))
        .route("/agents", get(list_agents))
        .route("/agents/{worker}", get(agent_detail))
        .route("/agents/{worker}/prompt", post(agent_prompt))
        .route("/agents/{worker}/steer", post(agent_steer))
        .route("/agents/{worker}/follow_up", post(agent_follow_up))
        .route("/agents/{worker}/abort", post(agent_abort))
        .route("/agents/{worker}/model", post(agent_model))
        .route("/agents/{worker}/thinking", post(agent_thinking))
        .route("/agents/{worker}/compact", post(agent_compact))
        .route("/agents/{worker}/bash", post(agent_bash))
        .route("/agents/{worker}/pi", post(agent_raw))
        .route("/agents/{worker}/stop", post(agent_stop))
        .route("/dispatch", post(dispatch))
        .route("/spawn", post(spawn_all))
        .route("/reload", post(reload_config))
        .route("/projects/{project}/spawn", post(spawn_project))
        .with_state(state.clone());

    let mut router = router;
    if !token.is_empty() {
        router = router.route_layer(middleware::from_fn_with_state(
            state.clone(),
            move |req, next: Next| auth_middleware(req, next, token.clone()),
        ));
    }
    router.layer(axum::middleware::from_fn(cors_layer))
}

async fn auth_middleware(req: axum::extract::Request, next: Next, token: String) -> Response {
    if req.method() == Method::OPTIONS {
        return next.run(req).await;
    }
    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if provided == token {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "success": false,
                "error": "unauthorized — configure PI_COMMANDER_API_TOKEN"
            })),
        )
            .into_response()
    }
}

async fn cors_layer(req: axum::extract::Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|o| o.to_string());
    let mut resp = next.run(req).await;
    if let Some(origin) = origin {
        if let Ok(v) = HeaderValue::from_str(&origin) {
            resp.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
            resp.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, OPTIONS"),
            );
            resp.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("content-type, authorization"),
            );
        }
    }
    resp
}

// --------------------------------------------------------------- helpers

fn ok_json(data: serde_json::Value) -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({"success": true, "data": data}))).into_response()
}

fn err_json(msg: impl Into<String>, code: StatusCode) -> Response {
    (
        code,
        axum::Json(serde_json::json!({"success": false, "error": msg.into()})),
    )
        .into_response()
}

fn missing(worker: &str) -> Response {
    err_json(format!("no agent '{worker}'"), StatusCode::NOT_FOUND)
}

// ------------------------------------------------------------- handlers

async fn health(State(st): State<ApiState>) -> impl IntoResponse {
    let broker = st.broker.health().await.unwrap_or(false);
    axum::Json(serde_json::json!({
        "success": true,
        "daemon": true,
        "uptime_secs": st.started_at.elapsed().as_secs(),
        "broker": broker,
        "telegram": st.telegram.enabled(),
        "agents": st.manager.list_ids().await.len(),
    }))
}

async fn snapshot(State(st): State<ApiState>) -> impl IntoResponse {
    ok_json(st.manager.snapshot().await)
}

async fn list_agents(State(st): State<ApiState>) -> impl IntoResponse {
    let v = st.manager.snapshot().await;
    let agents = v.get("agents").cloned().unwrap_or_else(|| serde_json::Value::Array(vec![]));
    ok_json(serde_json::json!({"agents": agents}))
}

async fn agent_detail(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
) -> impl IntoResponse {
    match st.manager.detail(&worker).await {
        Some(d) => ok_json(d),
        None => missing(&worker),
    }
}

async fn agent_prompt(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<PromptBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    let message = body.message;
    let requested = body.behavior;
    let behavior = match requested.as_deref() {
        Some(b) => b.to_string(),
        None => {
            let r = h.record.lock().await;
            match r.phase {
                crate::agent::AgentPhase::Busy | crate::agent::AgentPhase::Retrying => {
                    "steer".to_string()
                }
                _ => "followUp".to_string(),
            }
        }
    };
    match h.tx.send(AgentCommand::Prompt { text: message, behavior: behavior.clone() }).await
    {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "queued": true, "behavior": behavior})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_steer(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<PromptBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h.tx.send(AgentCommand::Steer { text: body.message }).await {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "queued": true})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_follow_up(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<PromptBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h.tx.send(AgentCommand::FollowUp { text: body.message }).await {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "queued": true})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_abort(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h.tx.send(AgentCommand::Abort).await {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "aborted": true})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_model(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<ModelBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h
        .tx
        .send(AgentCommand::SetModel { model: body.model.clone() })
        .await
    {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "model": body.model})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_thinking(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<LevelBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h
        .tx
        .send(AgentCommand::SetThinking { level: body.level.clone() })
        .await
    {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "level": body.level})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_compact(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h.tx.send(AgentCommand::Compact).await {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "compacting": true})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_bash(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<BashBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    match h.tx.send(AgentCommand::Bash { command: body.command }).await {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "bash": true})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_raw(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
    axum::extract::Json(body): axum::extract::Json<RawBody>,
) -> impl IntoResponse {
    let Some(h) = st.manager.get_any(&worker).await else {
        return missing(&worker);
    };
    // validate JSON before queueing
    if serde_json::from_str::<serde_json::Value>(&body.json).is_err() {
        return err_json("json field must be valid JSON", StatusCode::BAD_REQUEST);
    }
    match h.tx.send(AgentCommand::Raw { json: body.json }).await {
        Ok(()) => ok_json(serde_json::json!({"worker": worker, "raw": true})),
        Err(e) => err_json(format!("send failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn agent_stop(
    State(st): State<ApiState>,
    Path(worker): Path<String>,
) -> impl IntoResponse {
    match st.manager.stop(&worker).await {
        Ok(true) => ok_json(serde_json::json!({"worker": worker, "stopping": true})),
        Ok(false) => missing(&worker),
        Err(e) => err_json(format!("stop failed: {e}"), StatusCode::BAD_REQUEST),
    }
}

async fn dispatch(
    State(st): State<ApiState>,
    axum::extract::Json(body): axum::extract::Json<DispatchBody>,
) -> impl IntoResponse {
    match st.manager.dispatch(&body.text).await {
        Ok((id, text)) => ok_json(serde_json::json!({
            "worker": id,
            "text": text,
            "note": "task dispatched"
        })),
        Err(e) => err_json(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn spawn_all(State(st): State<ApiState>) -> impl IntoResponse {
    match st.manager.spawn_all().await {
        Ok(ids) => ok_json(serde_json::json!({"spawned": ids})),
        Err(e) => err_json(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn reload_config(State(st): State<ApiState>) -> impl IntoResponse {
    match st.manager.reload_config(&st.config_path).await {
        Ok((project_count, spawned)) => ok_json(serde_json::json!({
            "project_count": project_count,
            "spawned": spawned,
            "config": st.config_path.display().to_string(),
        })),
        Err(e) => err_json(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn spawn_project(
    State(st): State<ApiState>,
    Path(project): Path<String>,
    axum::extract::Json(body): axum::extract::Json<CountBody>,
) -> impl IntoResponse {
    let count = if body.count == 0 { 1 } else { body.count };
    match st.manager.spawn_project_extra(&project, count).await {
        Ok(ids) => ok_json(serde_json::json!({"spawned": ids})),
        Err(e) => err_json(e.to_string(), StatusCode::BAD_REQUEST),
    }
}