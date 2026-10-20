//! OpenAI-compatible HTTP gateway.

mod observe;
mod signals;

use notify::Watcher;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use airouter_config::{
    is_loopback_bind, load_file, resolve_for as config_resolve, save_file, upsert_secret,
    validate_for, ConfigFile, ResolvedConfig,
};
use airouter_core::{
    compute_cost, now_unix, openai_error_body, DashboardSnapshot, Error, Modality, ModelConfig,
    ModelView, Pricing, ProviderView, RequestRecord, RouteRequest, Target, TokenUsage, UsageMeta,
};
use airouter_providers::{
    build_map, http_client, list_upstream_models, AudioUpload, CallContext, ChatResult, Provider,
    ProviderError,
};
use airouter_registry::Registry;
use airouter_store::{start_of_local_day, start_of_local_month, GroupBy, Store};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Extension, Multipart, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use uuid::Uuid;

pub use airouter_store::Summary;

const BODY_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct ClientId(pub String);

pub struct Finish {
    pub usage: TokenUsage,
    pub ttft_ms: Option<u64>,
    pub status: u16,
    pub error: Option<String>,
    pub latency_ms: u64,
}

struct RuntimeState {
    file: ConfigFile,
    resolved: ResolvedConfig,
    registry: Registry,
    providers: HashMap<String, Arc<dyn Provider>>,
    loaded_at: i64,
    warnings: Vec<String>,
    last_error: Option<String>,
}

pub struct AppInner {
    pub config_path: PathBuf,
    store: Store,
    live: broadcast::Sender<RequestRecord>,
    started: Instant,
    runtime: RwLock<RuntimeState>,
    signals: Mutex<signals::SignalCache>,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppInner>,
}

impl AppState {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let file = load_file(path)?;
        let report = validate_for(path, &file)?;
        if !report.ok() {
            anyhow::bail!("{}", report.errors.join("; "));
        }
        for warning in &report.warnings {
            tracing::warn!(%warning, "config");
        }
        Self::from_file(path, file, report.warnings)
    }

    pub fn from_file(path: &Path, file: ConfigFile, warnings: Vec<String>) -> anyhow::Result<Self> {
        let resolved = config_resolve(path, &file)?;
        let registry = Registry::from_parts(
            file.models.clone(),
            file.aliases.clone(),
            file.routing.clone(),
        );
        let http = http_client();
        let providers = build_map(&resolved, http);
        let data_dir = airouter_config::default_state_dir(&file.server);
        let store = Store::open(&data_dir.join("usage.db"))?;
        let (live, _) = broadcast::channel(1024);
        Ok(Self {
            inner: Arc::new(AppInner {
                config_path: path.to_path_buf(),
                store,
                live,
                started: Instant::now(),
                signals: Mutex::new(signals::SignalCache::new()),
                runtime: RwLock::new(RuntimeState {
                    file,
                    resolved,
                    registry,
                    providers,
                    loaded_at: now_unix(),
                    warnings,
                    last_error: None,
                }),
            }),
        })
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, RuntimeState> {
        self.inner.runtime.read().unwrap_or_else(|e| e.into_inner())
    }

    fn signals(&self) -> &Mutex<signals::SignalCache> {
        &self.inner.signals
    }

    pub fn reload(&self) -> Result<(), String> {
        let file = load_file(&self.inner.config_path).map_err(|e| e.to_string())?;
        let report = validate_for(&self.inner.config_path, &file).map_err(|e| e.to_string())?;
        if !report.ok() {
            let msg = report.errors.join("; ");
            if let Ok(mut guard) = self.inner.runtime.write() {
                guard.last_error = Some(msg.clone());
            }
            return Err(msg);
        }
        let resolved = config_resolve(&self.inner.config_path, &file).map_err(|e| e.to_string())?;
        let registry = Registry::from_parts(
            file.models.clone(),
            file.aliases.clone(),
            file.routing.clone(),
        );
        let providers = build_map(&resolved, http_client());
        let mut guard = self
            .inner
            .runtime
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let listen_changed = guard.file.server.listen != file.server.listen
            || guard.file.server.admin_listen != file.server.admin_listen;
        guard.file = file;
        guard.resolved = resolved;
        guard.registry = registry;
        guard.providers = providers;
        guard.loaded_at = now_unix();
        guard.warnings = report.warnings;
        guard.last_error = listen_changed
            .then(|| "listen addresses changed in config; restart the server to apply them".into());
        Ok(())
    }

    fn record(&self, meta: &UsageMeta, finish: Finish) {
        let cost = compute_cost(&meta.pricing, &finish.usage, meta.modality);
        let known = finish.usage.known
            || finish.usage.images > 0
            || finish.usage.characters > 0
            || finish.usage.audio_seconds > 0.0;
        let rec = RequestRecord {
            id: meta.request_id.clone(),
            ts: now_unix(),
            client_label: meta.client_label.clone(),
            requested_model: meta.requested_model.clone(),
            provider: meta.provider.clone(),
            upstream_model: meta.upstream_model.clone(),
            modality: meta.modality.to_string(),
            status: finish.status,
            error: finish.error.clone(),
            latency_ms: finish.latency_ms,
            ttft_ms: finish.ttft_ms,
            input_tokens: known.then_some(finish.usage.input_tokens),
            output_tokens: known.then_some(finish.usage.output_tokens),
            cached_tokens: known.then_some(finish.usage.cached_tokens),
            cost_usd: cost,
            endpoint: meta.endpoint.clone(),
            route_profile: meta
                .route
                .as_ref()
                .map(|r| r.profile.clone())
                .unwrap_or_default(),
            route_reason: meta
                .route
                .as_ref()
                .map(|r| r.reason.clone())
                .unwrap_or_default(),
            route_confidence: meta.route.as_ref().map(|r| r.confidence),
            route_signals: meta
                .route
                .as_ref()
                .map(|r| r.signals_json())
                .unwrap_or_default(),
            route_rejected: meta
                .route
                .as_ref()
                .map(|r| r.rejected_json())
                .unwrap_or_default(),
            feedback: None,
        };
        tracing::info!(
            request_id = %rec.id,
            model = %rec.requested_model,
            provider = %rec.provider,
            status = rec.status,
            latency_ms = rec.latency_ms,
            cost_usd = rec.cost_usd,
            "request"
        );
        if let Err(err) = self.inner.store.insert(&rec) {
            tracing::warn!(%err, "failed to store usage");
        }
        let _ = self.inner.live.send(rec);
    }

    pub fn snapshot(&self) -> DashboardSnapshot {
        let rt = self.read();
        let recent = self.inner.store.recent(200).unwrap_or_default();
        let now = now_unix();
        let minute = self
            .inner
            .store
            .summary_since(now.saturating_sub(60))
            .unwrap_or_default();
        let hour = self
            .inner
            .store
            .summary_since(now.saturating_sub(3600))
            .unwrap_or_default();
        let today = self
            .inner
            .store
            .summary_since(start_of_local_day())
            .unwrap_or_default();
        let month = self
            .inner
            .store
            .summary_since(start_of_local_month())
            .unwrap_or_default();
        let total = self.inner.store.summary_since(0).unwrap_or_default();
        let error_rate = if hour.requests == 0 {
            0.0
        } else {
            hour.errors as f64 / hour.requests as f64
        };
        let mut last_by_provider: HashMap<String, &RequestRecord> = HashMap::new();
        for rec in &recent {
            last_by_provider.entry(rec.provider.clone()).or_insert(rec);
        }
        let providers = rt
            .resolved
            .providers
            .values()
            .map(|p| {
                let last = last_by_provider.get(&p.name);
                ProviderView {
                    name: p.name.clone(),
                    provider_type: p.provider_type.to_string(),
                    base_url: p.base_url.clone(),
                    enabled: p.enabled,
                    key_present: p.api_key.is_some(),
                    last_status: last.map(|r| r.status),
                    last_error: last.and_then(|r| r.error.clone()),
                }
            })
            .collect();
        let models = rt
            .registry
            .models
            .iter()
            .map(|m| ModelView {
                id: m.id.clone(),
                provider: m.provider.clone(),
                upstream: m.upstream.clone(),
                modality: m.modality.to_string(),
                enabled: m.enabled,
                context: m.context,
                capabilities: m.capabilities.clone(),
                input_per_mtok: m.pricing.input_per_mtok,
                output_per_mtok: m.pricing.output_per_mtok,
                per_image: m.pricing.per_image,
                per_1k_chars: m.pricing.per_1k_chars,
                per_minute: m.pricing.per_minute,
            })
            .collect();
        let since_30d = now.saturating_sub(30 * 86_400);
        DashboardSnapshot {
            listen: rt.file.server.listen.clone(),
            admin_listen: rt.file.server.admin_listen.clone(),
            uptime_secs: self.inner.started.elapsed().as_secs(),
            requests_total: total.requests,
            requests_last_minute: minute.requests,
            error_rate,
            spend_today_usd: today.cost_usd,
            spend_month_usd: month.cost_usd,
            providers,
            models,
            recent,
            usage_by_model: self
                .inner
                .store
                .aggregate(since_30d, GroupBy::Model)
                .unwrap_or_default(),
            usage_by_day: self
                .inner
                .store
                .aggregate(since_30d, GroupBy::Day)
                .unwrap_or_default(),
            config_path: self.inner.config_path.display().to_string(),
            config_loaded_at: rt.loaded_at,
            config_warnings: rt.warnings.clone(),
            config_error: rt.last_error.clone(),
        }
    }
}

pub struct ServeOpts {
    pub config_path: PathBuf,
    pub shutdown: Option<Arc<AtomicBool>>,
}

pub async fn serve(opts: ServeOpts) -> anyhow::Result<()> {
    let state = AppState::load(&opts.config_path)?;
    spawn_watcher(state.clone());
    let (public_addr, admin_addr) = {
        let rt = state.read();
        (
            rt.file.server.listen.clone(),
            rt.file.server.admin_listen.clone(),
        )
    };
    if !is_loopback_bind(&admin_addr) {
        anyhow::bail!("admin listen {admin_addr} is not loopback");
    }
    let public_listener = TcpListener::bind(&public_addr).await?;
    let admin_listener = TcpListener::bind(&admin_addr).await?;
    let bound_admin = admin_listener.local_addr()?;
    if !bound_admin.ip().is_loopback() {
        anyhow::bail!("admin listen {bound_admin} is not loopback");
    }
    tracing::info!(listen = %public_addr, admin = %bound_admin, "airouter listening");

    let (tx, mut rx) = tokio::sync::watch::channel(false);
    let flag = opts.shutdown.clone();
    tokio::spawn(async move {
        wait_for_stop(flag).await;
        let _ = tx.send(true);
    });
    let mut rx2 = rx.clone();
    let public = public_router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let admin = admin_router(state).into_make_service_with_connect_info::<SocketAddr>();
    let public_serve = axum::serve(public_listener, public).with_graceful_shutdown(async move {
        let _ = rx.changed().await;
    });
    let admin_serve = axum::serve(admin_listener, admin).with_graceful_shutdown(async move {
        let _ = rx2.changed().await;
    });
    let (a, b) = tokio::join!(public_serve, admin_serve);
    a?;
    b?;
    Ok(())
}

async fn wait_for_stop(flag: Option<Arc<AtomicBool>>) {
    loop {
        if flag.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) {
            return;
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return,
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
}

fn spawn_watcher(state: AppState) {
    let path = state.inner.config_path.clone();
    let Some(parent) = path.parent().map(|p| p.to_path_buf()) else {
        return;
    };
    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(8);
        let config_name = path.file_name().map(|s| s.to_os_string());
        let mut watcher =
            match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let relevant = event.paths.iter().any(|p| {
                        if p.extension().is_some_and(|e| e == "tmp") {
                            return false;
                        }
                        p.file_name() == config_name.as_deref()
                            || p.file_name() == Some("token".as_ref())
                            || p.file_name() == Some("secrets.json".as_ref())
                    });
                    if relevant {
                        let _ = tx.blocking_send(());
                    }
                }
            }) {
                Ok(w) => w,
                Err(err) => {
                    tracing::warn!(%err, "config watcher unavailable");
                    return;
                }
            };
        if let Err(err) = watcher.watch(&parent, notify::RecursiveMode::NonRecursive) {
            tracing::warn!(%err, "config watcher failed to start");
            return;
        }
        while rx.recv().await.is_some() {
            tokio::time::sleep(Duration::from_millis(300)).await;
            while rx.try_recv().is_ok() {}
            match state.reload() {
                Ok(()) => tracing::info!("reloaded config"),
                Err(err) => tracing::warn!(%err, "config reload rejected"),
            }
        }
    });
}

pub fn public_router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/models", get(list_models))
        .route("/models/{*id}", get(get_model))
        .route("/chat/completions", post(chat))
        .route("/embeddings", post(embeddings))
        .route("/images/generations", post(images))
        .route("/audio/speech", post(speech))
        .route("/audio/transcriptions", post(transcriptions))
        .route("/videos", post(videos))
        .route("/usage", get(usage))
        .route("/feedback", post(feedback))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/health", get(health))
        .nest("/v1", v1)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

pub fn admin_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/events", get(events))
        .route("/usage", get(usage))
        .route("/reload", post(reload))
        .route("/models/toggle", post(toggle_model))
        .route("/models/add", post(add_model))
        .route("/providers/toggle", post(toggle_provider))
        .route("/providers/key", post(set_provider_key))
        .route("/providers/models", post(list_provider_models))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

pub struct TestServer {
    pub addr: SocketAddr,
    pub admin: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub async fn spawn_test(state: AppState) -> TestServer {
    let public_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = public_listener.local_addr().unwrap();
    let admin = admin_listener.local_addr().unwrap();
    let public = public_router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let admin_app = admin_router(state).into_make_service_with_connect_info::<SocketAddr>();
    let handle = tokio::spawn(async move {
        let _ = tokio::join!(
            axum::serve(public_listener, public),
            axum::serve(admin_listener, admin_app),
        );
    });
    TestServer {
        addr,
        admin,
        handle,
    }
}

async fn auth(
    State(state): State<AppState>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let keys = {
        let rt = state.read();
        rt.resolved.keys.clone()
    };
    // Non-loopback binds without a key are rejected at startup. An open loopback
    // listener is intentional for local tools.
    let listen_loopback = is_loopback_bind(&state.read().file.server.listen);
    if keys.is_empty() {
        if listen_loopback {
            req.extensions_mut().insert(ClientId("open".into()));
            return next.run(req).await;
        }
        return state.unauthorized();
    }
    let header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header.strip_prefix("Bearer ").unwrap_or("").trim();
    if let Some(found) = keys.into_iter().find(|k| constant_time_eq(&k.key, token)) {
        req.extensions_mut().insert(ClientId(found.label));
        return next.run(req).await;
    }
    state.unauthorized()
}

impl AppState {
    fn unauthorized(&self) -> Response {
        let finish = Finish {
            usage: TokenUsage::default(),
            ttft_ms: None,
            status: 401,
            error: Some("unauthorized".into()),
            latency_ms: 0,
        };
        let meta = UsageMeta {
            request_id: new_id(),
            client_label: "-".into(),
            requested_model: "-".into(),
            provider: "-".into(),
            upstream_model: "-".into(),
            modality: Modality::Chat,
            endpoint: "auth".into(),
            pricing: Default::default(),
            route: None,
        };
        self.record(&meta, finish);
        api_error(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "invalid api key",
            Some("invalid_api_key"),
            None,
        )
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0 && a.len() == b.len()
}

async fn health() -> Response {
    Json(json!({"status": "ok"})).into_response()
}

async fn list_models(State(state): State<AppState>) -> Response {
    let rt = state.read();
    let mut data: Vec<Value> = rt.registry.enabled_models().map(model_json).collect();
    if rt.registry.routing.enabled {
        data.extend(auto_model_entries(&rt.registry.routing));
    }
    Json(json!({"object": "list", "data": data})).into_response()
}

fn auto_model_entries(routing: &airouter_core::RoutingConfig) -> Vec<Value> {
    let mut entries = Vec::new();
    if routing.profiles.contains_key(&routing.default_profile) {
        entries.push(auto_model_json("auto"));
    }
    for name in routing.profiles.keys() {
        entries.push(auto_model_json(&format!("auto/{name}")));
    }
    entries
}

fn auto_model_json(id: &str) -> Value {
    json!({
        "id": id,
        "object": "model",
        "created": 0,
        "owned_by": "airouter",
        "modality": "chat",
        "capabilities": ["tools", "vision", "streaming", "reasoning"],
    })
}

async fn get_model(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    let id = urlencoding_decode(&id);
    let rt = state.read();
    if rt.registry.routing.enabled
        && (id == "auto" || id.starts_with("auto/"))
        && airouter_core::auto_profile(&id, &rt.registry.routing.default_profile)
            .is_some_and(|name| rt.registry.routing.profiles.contains_key(name))
    {
        return Json(auto_model_json(&id)).into_response();
    }
    match rt.registry.find(&id).filter(|m| m.enabled) {
        Some(model) => Json(model_json(model)).into_response(),
        None => api_error(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            &format!("model not found: {id}"),
            Some("model_not_found"),
            None,
        ),
    }
}

fn urlencoding_decode(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let bytes = id.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&id[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn model_json(m: &airouter_core::ModelConfig) -> Value {
    json!({
        "id": m.id,
        "object": "model",
        "created": 0,
        "owned_by": m.provider,
        "modality": m.modality.as_str(),
        "context_length": m.context,
        "pricing": m.pricing,
        "capabilities": m.capabilities,
        "provider": m.provider,
    })
}

async fn chat(
    State(state): State<AppState>,
    Extension(client): Extension<ClientId>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let request_id = new_id();
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return fail(
                &state,
                &client,
                "-",
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
            );
        }
    };
    let requested = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    if requested.is_empty() {
        return fail(
            &state,
            &client,
            &requested,
            Modality::Chat,
            "/v1/chat/completions",
            request_id,
            started,
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "model is required",
            Some("model_required"),
        );
    }
    let target = match resolve_request(&state, &requested, Modality::Chat, &parsed, &client).await {
        Ok(t) => t,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested,
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                err,
            )
        }
    };
    dispatch_chat(state, client, parsed, target, request_id, started).await
}

async fn dispatch_chat(
    state: AppState,
    client: ClientId,
    body: Value,
    target: Target,
    request_id: String,
    started: Instant,
) -> Response {
    let mut chain = std::iter::once(target.clone())
        .chain(target.fallbacks.iter().map(|fb| Target {
            provider: fb.provider.clone(),
            upstream_model: fb.upstream_model.clone(),
            client_model: fb.client_model.clone(),
            modality: fb.modality,
            pricing: fb.pricing.clone(),
            capabilities: fb.capabilities.clone(),
            route: target.route.clone(),
            fallbacks: Vec::new(),
        }))
        .collect::<Vec<_>>();
    let mut last_err = None;
    while let Some(mut current) = chain.first().cloned() {
        chain.remove(0);
        let provider = match provider_for(&state, &current) {
            Ok(p) => p,
            Err(err) if !chain.is_empty() => {
                tracing::warn!(
                    request_id = %request_id,
                    model = %current.client_model,
                    %err,
                    "routed provider unavailable; trying the next model"
                );
                continue;
            }
            Err(err) => {
                return fail_core(
                    &state,
                    &client,
                    &current.client_model,
                    Modality::Chat,
                    "/v1/chat/completions",
                    request_id,
                    started,
                    err,
                );
            }
        };
        if last_err.is_some() {
            if let Some(route) = current.route.as_mut() {
                route.reason = format!("{}; failover", route.reason);
            }
        }
        let ctx = call_context(&state, &current);
        let meta = usage_meta(
            &request_id,
            &client,
            &body,
            &current,
            "/v1/chat/completions",
        );
        match provider.chat(body.clone(), &ctx).await {
            Ok(ChatResult::Complete(mut value)) => {
                let usage = TokenUsage::from_openai_chat(&value);
                state.record(
                    &meta,
                    Finish {
                        usage,
                        ttft_ms: None,
                        status: 200,
                        error: None,
                        latency_ms: started.elapsed().as_millis() as u64,
                    },
                );
                attach_route(&mut value, &current);
                return json_response(StatusCode::OK, value, &request_id, Some(&current));
            }
            Ok(ChatResult::Stream(stream)) => {
                let mapped = stream.map(|item| item.map_err(|e| e.to_string()));
                let state2 = state.clone();
                let meta2 = meta.clone();
                let observed = observe::Observe::new(
                    mapped,
                    Box::new(move |finish| state2.record(&meta2, finish)),
                );
                return sse_response(observed, &request_id, &current);
            }
            Err(err) if failover(&err) && !chain.is_empty() => {
                tracing::warn!(
                    request_id = %request_id,
                    model = %current.client_model,
                    "upstream failed; trying the next routed model"
                );
                last_err = Some(err);
            }
            Err(err) => return provider_fail(&state, &meta, started, err),
        }
    }
    provider_fail(
        &state,
        &usage_meta(&request_id, &client, &body, &target, "/v1/chat/completions"),
        started,
        last_err.unwrap_or_else(|| ProviderError::Transport("no routed model".into())),
    )
}

fn failover(err: &ProviderError) -> bool {
    match err {
        ProviderError::Transport(_) => true,
        ProviderError::Upstream { status, .. } => {
            matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504)
        }
        _ => false,
    }
}

fn attach_route(body: &mut Value, target: &Target) {
    let Some(route) = &target.route else {
        return;
    };
    body["airouter"] = json!({
        "profile": route.profile,
        "model": target.client_model,
        "reason": route.reason,
        "confidence": route.confidence,
        "task": route.task,
        "rejected": route.rejected,
    });
}

async fn embeddings(
    State(state): State<AppState>,
    Extension(client): Extension<ClientId>,
    body: Bytes,
) -> Response {
    json_endpoint(
        state,
        client,
        body,
        Modality::Embedding,
        "/v1/embeddings",
        "embeddings",
    )
    .await
}

async fn images(
    State(state): State<AppState>,
    Extension(client): Extension<ClientId>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let request_id = new_id();
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return fail(
                &state,
                &client,
                "-",
                Modality::Image,
                "/v1/images/generations",
                request_id,
                started,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
            )
        }
    };
    let requested = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let target = match resolve_request(&state, &requested, Modality::Image, &parsed, &client).await
    {
        Ok(t) => t,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested,
                Modality::Image,
                "/v1/images/generations",
                request_id,
                started,
                err,
            )
        }
    };
    let n = parsed.get("n").and_then(|v| v.as_u64()).unwrap_or(1);
    let provider = match provider_for(&state, &target) {
        Ok(p) => p,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested,
                Modality::Image,
                "/v1/images/generations",
                request_id,
                started,
                err,
            )
        }
    };
    let ctx = call_context(&state, &target);
    let meta = usage_meta(
        &request_id,
        &client,
        &parsed,
        &target,
        "/v1/images/generations",
    );
    match provider.images(parsed, &ctx).await {
        Ok(value) => {
            let mut usage = TokenUsage::default();
            usage.images = n.max(1);
            state.record(
                &meta,
                Finish {
                    usage,
                    ttft_ms: None,
                    status: 200,
                    error: None,
                    latency_ms: started.elapsed().as_millis() as u64,
                },
            );
            json_response(StatusCode::OK, value, &request_id, Some(&target))
        }
        Err(err) => provider_fail(&state, &meta, started, err),
    }
}

async fn speech(
    State(state): State<AppState>,
    Extension(client): Extension<ClientId>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let request_id = new_id();
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return fail(
                &state,
                &client,
                "-",
                Modality::AudioTts,
                "/v1/audio/speech",
                request_id,
                started,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
            )
        }
    };
    let requested = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let target =
        match resolve_request(&state, &requested, Modality::AudioTts, &parsed, &client).await {
            Ok(t) => t,
            Err(err) => {
                return fail_core(
                    &state,
                    &client,
                    &requested,
                    Modality::AudioTts,
                    "/v1/audio/speech",
                    request_id,
                    started,
                    err,
                )
            }
        };
    let chars = parsed
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .count() as u64;
    let provider = match provider_for(&state, &target) {
        Ok(p) => p,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested,
                Modality::AudioTts,
                "/v1/audio/speech",
                request_id,
                started,
                err,
            )
        }
    };
    let ctx = call_context(&state, &target);
    let meta = usage_meta(&request_id, &client, &parsed, &target, "/v1/audio/speech");
    match provider.speech(parsed, &ctx).await {
        Ok(audio) => {
            let mut usage = TokenUsage::default();
            usage.characters = chars;
            state.record(
                &meta,
                Finish {
                    usage,
                    ttft_ms: None,
                    status: 200,
                    error: None,
                    latency_ms: started.elapsed().as_millis() as u64,
                },
            );
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, audio.content_type)
                .header("x-request-id", request_id)
                .header("x-airouter-provider", target.provider)
                .body(axum::body::Body::from(audio.data))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            let _ = response;
            response
        }
        Err(err) => provider_fail(&state, &meta, started, err),
    }
}

async fn transcriptions(
    State(state): State<AppState>,
    Extension(client): Extension<ClientId>,
    multipart: Result<Multipart, axum::extract::multipart::MultipartRejection>,
) -> Response {
    let started = Instant::now();
    let request_id = new_id();
    let mut multipart = match multipart {
        Ok(m) => m,
        Err(err) => {
            return fail(
                &state,
                &client,
                "-",
                Modality::AudioStt,
                "/v1/audio/transcriptions",
                request_id,
                started,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
            )
        }
    };
    let mut file_name = "audio".to_string();
    let mut mime = "application/octet-stream".to_string();
    let mut bytes = Bytes::new();
    let mut fields = Vec::new();
    let mut model = String::new();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(err) => {
                return fail(
                    &state,
                    &client,
                    &model,
                    Modality::AudioStt,
                    "/v1/audio/transcriptions",
                    request_id,
                    started,
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &err.to_string(),
                    None,
                )
            }
        };
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            file_name = field.file_name().unwrap_or("audio").to_string();
            mime = field
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_string();
            bytes = match field.bytes().await {
                Ok(b) => b,
                Err(err) => {
                    return fail(
                        &state,
                        &client,
                        &model,
                        Modality::AudioStt,
                        "/v1/audio/transcriptions",
                        request_id,
                        started,
                        StatusCode::BAD_REQUEST,
                        "invalid_request_error",
                        &err.to_string(),
                        None,
                    )
                }
            };
        } else {
            let value = field.text().await.unwrap_or_default();
            if name == "model" {
                model = value.clone();
            }
            fields.push((name, value));
        }
    }
    let prompt = fields
        .iter()
        .find(|(name, _)| name == "prompt")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let summary_body = json!({"model": &model, "prompt": prompt});
    let target =
        match resolve_request(&state, &model, Modality::AudioStt, &summary_body, &client).await {
            Ok(t) => t,
            Err(err) => {
                return fail_core(
                    &state,
                    &client,
                    &model,
                    Modality::AudioStt,
                    "/v1/audio/transcriptions",
                    request_id,
                    started,
                    err,
                )
            }
        };
    let provider = match provider_for(&state, &target) {
        Ok(p) => p,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &model,
                Modality::AudioStt,
                "/v1/audio/transcriptions",
                request_id,
                started,
                err,
            )
        }
    };
    let ctx = call_context(&state, &target);
    let meta = UsageMeta {
        request_id: request_id.clone(),
        client_label: client.0.clone(),
        requested_model: model,
        provider: target.provider.clone(),
        upstream_model: target.upstream_model.clone(),
        modality: Modality::AudioStt,
        endpoint: "/v1/audio/transcriptions".into(),
        pricing: target.pricing.clone(),
        route: target.route.clone(),
    };
    let upload = AudioUpload {
        file_name,
        mime,
        bytes,
        fields,
    };
    match provider.transcriptions(upload, &ctx).await {
        Ok(value) => {
            let mut usage = TokenUsage::default();
            if let Some(secs) = value.get("duration").and_then(|v| v.as_f64()) {
                usage.audio_seconds = secs;
            }
            state.record(
                &meta,
                Finish {
                    usage,
                    ttft_ms: None,
                    status: 200,
                    error: None,
                    latency_ms: started.elapsed().as_millis() as u64,
                },
            );
            json_response(StatusCode::OK, value, &request_id, Some(&target))
        }
        Err(err) => provider_fail(&state, &meta, started, err),
    }
}

async fn videos(State(state): State<AppState>, Extension(client): Extension<ClientId>) -> Response {
    let _ = (state, client);
    api_error(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "video jobs are reserved for a later release; the provider async_job hook is not exposed yet",
        Some("video_not_implemented"),
        None,
    )
}

async fn json_endpoint(
    state: AppState,
    client: ClientId,
    body: Bytes,
    modality: Modality,
    endpoint: &str,
    kind: &str,
) -> Response {
    let started = Instant::now();
    let request_id = new_id();
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return fail(
                &state,
                &client,
                "-",
                modality,
                endpoint,
                request_id,
                started,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
            )
        }
    };
    let requested = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let target = match resolve_request(&state, &requested, modality, &parsed, &client).await {
        Ok(t) => t,
        Err(err) => {
            return fail_core(
                &state, &client, &requested, modality, endpoint, request_id, started, err,
            )
        }
    };
    let provider = match provider_for(&state, &target) {
        Ok(p) => p,
        Err(err) => {
            return fail_core(
                &state, &client, &requested, modality, endpoint, request_id, started, err,
            )
        }
    };
    let ctx = call_context(&state, &target);
    let meta = usage_meta(&request_id, &client, &parsed, &target, endpoint);
    let result = match kind {
        "embeddings" => provider.embeddings(parsed, &ctx).await,
        _ => Err(ProviderError::Unsupported(kind.into())),
    };
    match result {
        Ok(value) => {
            let usage = TokenUsage::from_openai_chat(&value);
            state.record(
                &meta,
                Finish {
                    usage,
                    ttft_ms: None,
                    status: 200,
                    error: None,
                    latency_ms: started.elapsed().as_millis() as u64,
                },
            );
            json_response(StatusCode::OK, value, &request_id, Some(&target))
        }
        Err(err) => provider_fail(&state, &meta, started, err),
    }
}

#[derive(Deserialize)]
struct UsageQuery {
    since: Option<String>,
    by: Option<String>,
}

async fn usage(State(state): State<AppState>, Query(q): Query<UsageQuery>) -> Response {
    let now = now_unix();
    let since = match q.since.as_deref() {
        Some(s) => match parse_since(s, now) {
            Ok(v) => v,
            Err(err) => {
                return api_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &err,
                    None,
                    None,
                )
            }
        },
        None => now.saturating_sub(30 * 86_400),
    };
    let by = match q.by.as_deref().unwrap_or("model") {
        s => match GroupBy::parse(s) {
            Some(g) => g,
            None => {
                return api_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "by must be model, provider, day, client, or route",
                    None,
                    None,
                )
            }
        },
    };
    let summary = state.inner.store.summary_since(since).unwrap_or_default();
    let rows = state.inner.store.aggregate(since, by).unwrap_or_default();
    Json(json!({
        "object": "usage",
        "since": since,
        "summary": {
            "requests": summary.requests,
            "errors": summary.errors,
            "input_tokens": summary.input_tokens,
            "output_tokens": summary.output_tokens,
            "cost_usd": summary.cost_usd,
        },
        "rows": rows,
    }))
    .into_response()
}

async fn snapshot(State(state): State<AppState>) -> Response {
    Json(state.snapshot()).into_response()
}

async fn events(State(state): State<AppState>) -> Response {
    let mut rx = state.inner.live.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let line = format!("data: {}\n\n", serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into()));
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(line));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(axum::body::Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn reload(State(state): State<AppState>) -> Response {
    match state.reload() {
        Ok(()) => {
            let warnings = state.read().warnings.clone();
            Json(json!({"ok": true, "warnings": warnings})).into_response()
        }
        Err(err) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            &err,
            None,
            None,
        ),
    }
}

async fn toggle_model(State(state): State<AppState>, body: Bytes) -> Response {
    let id = match serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("id").and_then(|s| s.as_str()).map(|s| s.to_string()))
    {
        Some(id) => id,
        None => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "body must be {\"id\":\"...\"}",
                None,
                None,
            )
        }
    };
    toggle_config(&state, |cfg| {
        let Some(model) = cfg.models.iter_mut().find(|m| m.id == id) else {
            return Err(format!("model not found: {id}"));
        };
        model.enabled = !model.enabled;
        Ok(json!({"id": model.id, "enabled": model.enabled}))
    })
}

async fn toggle_provider(State(state): State<AppState>, body: Bytes) -> Response {
    let name = match serde_json::from_slice::<Value>(&body).ok().and_then(|v| {
        v.get("name")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
    }) {
        Some(name) => name,
        None => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "body must be {\"name\":\"...\"}",
                None,
                None,
            )
        }
    };
    toggle_config(&state, |cfg| {
        let Some(provider) = cfg.providers.get_mut(&name) else {
            return Err(format!("provider not found: {name}"));
        };
        provider.enabled = !provider.enabled;
        Ok(json!({"name": name, "enabled": provider.enabled}))
    })
}

async fn list_provider_models(State(state): State<AppState>, body: Bytes) -> Response {
    let name = match json_name(&body) {
        Some(name) => name,
        None => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "body must be {\"name\":\"...\"}",
                None,
                None,
            )
        }
    };
    let provider = {
        let rt = state.read();
        rt.resolved.providers.get(&name).cloned()
    };
    let Some(provider) = provider else {
        return api_error(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            &format!("provider not found: {name}"),
            None,
            None,
        );
    };
    match list_upstream_models(&provider, &http_client()).await {
        Ok(models) => Json(json!({"provider": name, "models": models})).into_response(),
        Err(err) => api_error(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            &err.to_string(),
            None,
            None,
        ),
    }
}

async fn add_model(State(state): State<AppState>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).ok();
    let provider = value.as_ref().and_then(|v| text_field(v, "provider"));
    let upstream = value.as_ref().and_then(|v| text_field(v, "upstream"));
    let (Some(provider), Some(upstream)) = (provider, upstream) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "body must be {\"provider\":\"...\",\"upstream\":\"...\"}",
            None,
            None,
        );
    };
    if !state.read().file.providers.contains_key(&provider) {
        return api_error(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            &format!("provider not found: {provider}"),
            None,
            None,
        );
    }
    toggle_config(&state, |cfg| {
        let id = format!("{provider}/{upstream}");
        if let Some(existing) = cfg.models.iter_mut().find(|m| m.id == id) {
            existing.enabled = true;
            return Ok(json!({"id": id, "enabled": true}));
        }
        let model = airouter_registry::find_catalog(&id)
            .map(|mut found| {
                found.enabled = true;
                found.provider = provider.clone();
                found.upstream = upstream.clone();
                found
            })
            .unwrap_or_else(|| ModelConfig {
                id: id.clone(),
                provider: provider.clone(),
                upstream: upstream.clone(),
                modality: Modality::Chat,
                enabled: true,
                context: None,
                capabilities: vec!["streaming".into()],
                pricing: Pricing::default(),
                quality: None,
                task_quality: std::collections::BTreeMap::new(),
                latency_ms_prior: None,
            });
        cfg.models.push(model);
        Ok(json!({"id": id, "enabled": true}))
    })
}

fn json_name(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|v| text_field(v, "name"))
}

fn text_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

async fn set_provider_key(State(state): State<AppState>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).ok();
    let name = value.as_ref().and_then(|v| {
        v.get("name")
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    let key = value.as_ref().and_then(|v| {
        v.get("key")
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    let (Some(name), Some(key)) = (name, key) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "body must be {\"name\":\"...\",\"key\":\"...\"}",
            None,
            None,
        );
    };
    if !state.read().file.providers.contains_key(&name) {
        return api_error(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            &format!("provider not found: {name}"),
            None,
            None,
        );
    }
    if let Err(err) = upsert_secret(&state.inner.config_path, &name, &key) {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            &err.to_string(),
            None,
            None,
        );
    }
    tracing::info!(provider = %name, "stored provider api key");
    toggle_config(&state, |cfg| {
        let Some(provider) = cfg.providers.get_mut(&name) else {
            return Err(format!("provider not found: {name}"));
        };
        provider.api_key = Some(format!("secret:{name}"));
        Ok(json!({"name": name, "key_present": true}))
    })
}

fn toggle_config(
    state: &AppState,
    edit: impl FnOnce(&mut ConfigFile) -> Result<Value, String>,
) -> Response {
    let mut file = match load_file(&state.inner.config_path) {
        Ok(f) => f,
        Err(err) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
                None,
            )
        }
    };
    let body = match edit(&mut file) {
        Ok(v) => v,
        Err(err) => {
            return api_error(
                StatusCode::NOT_FOUND,
                "invalid_request_error",
                &err,
                None,
                None,
            )
        }
    };
    if let Err(err) = save_file(&state.inner.config_path, &file) {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            &err.to_string(),
            None,
            None,
        );
    }
    if let Err(err) = state.reload() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            &err,
            None,
            None,
        );
    }
    Json(body).into_response()
}

async fn resolve_request(
    state: &AppState,
    model: &str,
    modality: Modality,
    body: &Value,
    client: &ClientId,
) -> Result<Target, Error> {
    if model.is_empty() {
        return Err(Error::Invalid("model is required".into()));
    }
    let summary = summarize(body, &client.0);
    let signals = if model == "auto" || model.starts_with("auto/") {
        Some(signals::extract(state, model, &summary).await)
    } else {
        None
    };
    let rt = state.read();
    airouter_registry::resolve(
        &rt.registry,
        &RouteRequest {
            model: model.to_string(),
            modality,
            summary,
            signals,
        },
    )
}

fn summarize(body: &Value, client: &str) -> airouter_core::RequestSummary {
    let mut chars = 0usize;
    let mut has_images = false;
    let mut last_user = String::new();
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            let text = content_text(message.get("content"), &mut has_images);
            chars += text.len();
            if message.get("role").and_then(|r| r.as_str()) == Some("user") {
                last_user = text;
            }
        }
    } else if let Some(input) = body.get("input").and_then(|v| v.as_str()) {
        chars += input.len();
        last_user = input.to_string();
    } else if let Some(prompt) = body.get("prompt").and_then(|v| v.as_str()) {
        chars += prompt.len();
        last_user = prompt.to_string();
    }
    airouter_core::RequestSummary {
        token_estimate: (chars / 4).max(1) as u64,
        has_tools: body
            .get("tools")
            .and_then(|t| t.as_array())
            .is_some_and(|tools| !tools.is_empty()),
        has_images,
        needs_json: body.get("response_format").is_some(),
        stream: body
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false),
        last_user_text: trim_chars(&last_user, 8_000),
        client_label: client.to_string(),
        reasoning_effort: body
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

fn content_text(content: Option<&Value>, has_images: &mut bool) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                let kind = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if kind == "image_url" || kind == "input_image" {
                    *has_images = true;
                }
                if let Some(bit) = part.get("text").and_then(|t| t.as_str()) {
                    text.push_str(bit);
                    text.push('\n');
                }
            }
            text
        }
        _ => String::new(),
    }
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn provider_for(state: &AppState, target: &Target) -> Result<Arc<dyn Provider>, Error> {
    let rt = state.read();
    if let Some(p) = rt.providers.get(&target.provider) {
        return Ok(p.clone());
    }
    if rt
        .resolved
        .providers
        .get(&target.provider)
        .is_some_and(|p| !p.enabled)
    {
        Err(Error::ProviderDisabled(target.provider.clone()))
    } else {
        Err(Error::ProviderMissing(target.provider.clone()))
    }
}

fn call_context(state: &AppState, target: &Target) -> CallContext {
    let rt = state.read();
    let provider = rt.resolved.providers.get(&target.provider);
    let (timeout, max_retries, backoff_ms) = match provider {
        Some(p) => (
            p.timeout,
            rt.file.server.retry.max_retries,
            rt.file.server.retry.backoff_ms,
        ),
        None => (
            Duration::from_secs(180),
            rt.file.server.retry.max_retries,
            rt.file.server.retry.backoff_ms,
        ),
    };
    CallContext {
        client_model: target.client_model.clone(),
        upstream_model: target.upstream_model.clone(),
        timeout,
        max_retries,
        backoff_ms,
    }
}

fn usage_meta(
    request_id: &str,
    client: &ClientId,
    body: &Value,
    target: &Target,
    endpoint: &str,
) -> UsageMeta {
    UsageMeta {
        request_id: request_id.to_string(),
        client_label: client.0.clone(),
        requested_model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&target.client_model)
            .to_string(),
        provider: target.provider.clone(),
        upstream_model: target.upstream_model.clone(),
        modality: target.modality,
        endpoint: endpoint.to_string(),
        pricing: target.pricing.clone(),
        route: target.route.clone(),
    }
}

fn provider_fail(
    state: &AppState,
    meta: &UsageMeta,
    started: Instant,
    err: ProviderError,
) -> Response {
    let (status, typ, code, retry) = match &err {
        ProviderError::Unsupported(_) => (
            StatusCode::NOT_IMPLEMENTED,
            "not_implemented",
            Some("unsupported"),
            None,
        ),
        ProviderError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request_error", None, None),
        ProviderError::Transport(_) => (
            StatusCode::BAD_GATEWAY,
            "server_error",
            Some("upstream_transport"),
            None,
        ),
        ProviderError::Upstream {
            status,
            retry_after,
            ..
        } => (
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
            "upstream_error",
            Some("upstream"),
            retry_after.clone(),
        ),
    };
    state.record(
        meta,
        Finish {
            usage: TokenUsage::default(),
            ttft_ms: None,
            status: status.as_u16(),
            error: Some(err.to_string()),
            latency_ms: started.elapsed().as_millis() as u64,
        },
    );
    api_error(status, typ, &err.to_string(), code, retry)
}

fn fail_core(
    state: &AppState,
    client: &ClientId,
    model: &str,
    modality: Modality,
    endpoint: &str,
    request_id: String,
    started: Instant,
    err: Error,
) -> Response {
    let (status, typ, code) = match &err {
        Error::ModelNotFound(_) => (
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            Some("model_not_found"),
        ),
        Error::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            Some("invalid_api_key"),
        ),
        Error::Unsupported(_) | Error::AutoRoutingDisabled => {
            (StatusCode::NOT_IMPLEMENTED, "not_implemented", None)
        }
        _ => (StatusCode::BAD_REQUEST, "invalid_request_error", None),
    };
    fail(
        state,
        client,
        model,
        modality,
        endpoint,
        request_id,
        started,
        status,
        typ,
        &err.to_string(),
        code,
    )
}

fn fail(
    state: &AppState,
    client: &ClientId,
    model: &str,
    modality: Modality,
    endpoint: &str,
    request_id: String,
    started: Instant,
    status: StatusCode,
    typ: &str,
    message: &str,
    code: Option<&str>,
) -> Response {
    let meta = UsageMeta {
        request_id,
        client_label: client.0.clone(),
        requested_model: model.to_string(),
        provider: "-".into(),
        upstream_model: "-".into(),
        modality,
        endpoint: endpoint.to_string(),
        pricing: Default::default(),
        route: None,
    };
    state.record(
        &meta,
        Finish {
            usage: TokenUsage::default(),
            ttft_ms: None,
            status: status.as_u16(),
            error: Some(message.to_string()),
            latency_ms: started.elapsed().as_millis() as u64,
        },
    );
    api_error(status, typ, message, code, None)
}

fn api_error(
    status: StatusCode,
    typ: &str,
    message: &str,
    code: Option<&str>,
    retry_after: Option<String>,
) -> Response {
    let mut response = (status, Json(openai_error_body(typ, message, code))).into_response();
    if let Some(retry) = retry_after {
        if let Ok(value) = HeaderValue::from_str(&retry) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

fn json_response(
    status: StatusCode,
    body: Value,
    request_id: &str,
    target: Option<&Target>,
) -> Response {
    let mut response = (status, Json(body)).into_response();
    insert_debug_headers(response.headers_mut(), request_id, target);
    response
}

fn sse_response<S>(stream: S, request_id: &str, target: &Target) -> Response
where
    S: futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(axum::body::Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    insert_debug_headers(response.headers_mut(), request_id, Some(target));
    response
}

fn insert_debug_headers(headers: &mut HeaderMap, request_id: &str, target: Option<&Target>) {
    if let Ok(v) = HeaderValue::from_str(request_id) {
        headers.insert("x-request-id", v);
    }
    if let Some(target) = target {
        if let Ok(v) = HeaderValue::from_str(&target.provider) {
            headers.insert("x-airouter-provider", v);
        }
        if let Ok(v) = HeaderValue::from_str(&target.client_model) {
            headers.insert("x-airouter-model", v);
        }
        if let Some(route) = &target.route {
            let value = format!(
                "profile={};model={};reason={};confidence={:.2}",
                route.profile,
                target.client_model,
                route.reason.replace([';', '\n', '\r'], " "),
                route.confidence
            );
            if let Ok(v) = HeaderValue::from_str(&value) {
                headers.insert("x-airouter-route", v);
            }
        }
    }
}

async fn feedback(State(state): State<AppState>, body: Bytes) -> Response {
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &err.to_string(),
                None,
                None,
            )
        }
    };
    let Some(id) = parsed.get("request_id").and_then(|v| v.as_str()) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "request_id is required",
            None,
            None,
        );
    };
    let rating = parsed.get("rating").and_then(|v| v.as_str()).unwrap_or("");
    let value: i8 = match rating {
        "up" | "1" => 1,
        "down" | "-1" => -1,
        _ => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "rating must be up or down",
                None,
                None,
            )
        }
    };
    match state.inner.store.set_feedback(id, value) {
        Ok(true) => Json(json!({"request_id": id, "rating": rating})).into_response(),
        Ok(false) => api_error(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            "request not found",
            None,
            None,
        ),
        Err(err) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            &err.to_string(),
            None,
            None,
        ),
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

struct Json<T>(T);

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        match serde_json::to_vec(&self.0) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

pub fn parse_since(spec: &str, now: i64) -> Result<i64, String> {
    if spec.is_empty() {
        return Err("since is empty".into());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(spec) {
        return Ok(dt.timestamp());
    }
    // A bare integer is a unix timestamp, including 0 (the start of the log).
    if let Ok(n) = spec.parse::<i64>() {
        return Ok(n);
    }
    let split = spec
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or_else(|| format!("invalid since '{spec}'"))?;
    let (num, unit) = spec.split_at(split);
    let n: i64 = num.parse().map_err(|_| format!("invalid since '{spec}'"))?;
    let secs = match unit {
        "s" => n,
        "m" => n.saturating_mul(60),
        "h" => n.saturating_mul(3600),
        "d" => n.saturating_mul(86_400),
        _ => return Err(format!("invalid since unit in '{spec}'")),
    };
    Ok(now.saturating_sub(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_and_unix_since() {
        assert_eq!(parse_since("30m", 1_000_000).unwrap(), 1_000_000 - 1800);
        assert_eq!(
            parse_since("1700000000", 1_800_000_000).unwrap(),
            1_700_000_000
        );
    }
}
