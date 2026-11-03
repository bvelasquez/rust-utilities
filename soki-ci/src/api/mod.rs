mod events;
mod handlers;
pub mod routes;
mod session;
mod web_assets;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::{load_config, ResolvedConfig};
use crate::jobs::JobOrchestrator;

pub use events::{global_events, job_events};
pub use session::{login, logout, session_state};

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub expires_at: DateTime<Utc>,
    pub csrf: String,
}

#[derive(Clone)]
pub struct ApiState {
    pub jobs: Arc<JobOrchestrator>,
    pub config_path: PathBuf,
    /// When set, `Authorization: Bearer <token>` or a browser session cookie is
    /// required on protected routes. `/health`, `/capabilities`, `/api/session`,
    /// and the static web UI stay public.
    pub api_token: Option<Arc<str>>,
    pub started_at: DateTime<Utc>,
    pub bind: Option<SocketAddr>,
    pub sessions: Arc<Mutex<HashMap<String, SessionInfo>>>,
    pub web_dir: Option<PathBuf>,
    pub allow_config_write: bool,
}

impl ApiState {
    pub fn new(
        jobs: Arc<JobOrchestrator>,
        config_path: PathBuf,
        api_token: Option<String>,
    ) -> Self {
        Self {
            jobs,
            config_path,
            api_token: api_token.map(|t| Arc::from(t.into_boxed_str())),
            started_at: Utc::now(),
            bind: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            web_dir: None,
            allow_config_write: false,
        }
    }

    pub fn load_config(&self) -> Result<ResolvedConfig> {
        load_config(&self.config_path)
    }
}

#[derive(Debug, Clone)]
pub struct ApiOptions {
    pub bind: SocketAddr,
    pub api_token: Option<String>,
    pub web_dir: Option<PathBuf>,
    pub allow_config_write: bool,
    pub open: bool,
}

impl ApiOptions {
    pub fn new(bind: SocketAddr) -> Self {
        Self {
            bind,
            api_token: None,
            web_dir: None,
            allow_config_write: false,
            open: false,
        }
    }
}

/// Running HTTP API task (shared job store with TUI or headless `serve`).
pub struct ApiHandle {
    pub bind: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: JoinHandle<Result<()>>,
}

impl ApiHandle {
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        match self.join.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("soki-ci API shutdown error: {e:#}"),
            Err(e) => eprintln!("soki-ci API task join error: {e}"),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.bind)
    }
}

pub fn validate_bind(bind: SocketAddr, api_token: Option<&str>) -> Result<()> {
    if bind.ip().is_unspecified() && api_token.is_none() {
        bail!(
            "refusing to listen on {bind} without SOKI_CI_API_TOKEN (or --api-token); \
             bind to 127.0.0.1 for local-only use"
        );
    }
    Ok(())
}

pub fn parse_bind(s: &str) -> Result<SocketAddr> {
    s.parse()
        .with_context(|| format!("invalid bind address `{s}` (expected host:port)"))
}

/// Bind and spawn the API; call [`ApiHandle::shutdown`] when the host process exits.
pub async fn spawn_api(
    jobs: Arc<JobOrchestrator>,
    config_path: PathBuf,
    options: ApiOptions,
) -> Result<ApiHandle> {
    validate_bind(options.bind, options.api_token.as_deref())?;
    jobs.recover_interrupted().await;
    let listener = TcpListener::bind(options.bind)
        .await
        .with_context(|| format!("failed to bind HTTP API on {}", options.bind))?;
    let bound = listener
        .local_addr()
        .with_context(|| format!("failed to read local address for {}", options.bind))?;

    let mut state = ApiState::new(jobs, config_path, options.api_token);
    state.bind = Some(bound);
    state.web_dir = options.web_dir;
    state.allow_config_write = options.allow_config_write;
    let app = routes::router(state);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .context("HTTP server exited with error")?;
        Ok(())
    });

    if options.open {
        open_browser(&format!("http://{bound}"));
    }

    Ok(ApiHandle {
        bind: bound,
        shutdown_tx: Some(shutdown_tx),
        join,
    })
}

/// Headless API-only mode (`soki-ci serve` / `soki-ci web`).
pub async fn run_server(
    jobs: Arc<JobOrchestrator>,
    config_path: PathBuf,
    options: ApiOptions,
) -> Result<()> {
    let handle = spawn_api(jobs, config_path, options).await?;
    eprintln!(
        "soki-ci API listening on {} (web UI at {}/, GET /capabilities for the manifest)",
        handle.url(),
        handle.url()
    );
    tokio::signal::ctrl_c()
        .await
        .context("waiting for Ctrl+C")?;
    eprintln!("soki-ci API shutting down…");
    handle.shutdown().await;
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "cmd";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    let mut command = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        command.args(["/C", "start", "", url]);
    }
    #[cfg(not(target_os = "windows"))]
    {
        command.arg(url);
    }
    let _ = command.spawn();
}

pub(crate) fn projects_builds_json(cfg: &ResolvedConfig) -> serde_json::Value {
    crate::commands::projects::builds_catalog(cfg)
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}
