mod handlers;
pub mod routes;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::{load_config, ResolvedConfig};
use crate::jobs::JobOrchestrator;

#[derive(Clone)]
pub struct ApiState {
    pub jobs: Arc<JobOrchestrator>,
    pub config_path: PathBuf,
    /// When set, `Authorization: Bearer <token>` is required on all routes except `GET /health`.
    pub api_token: Option<Arc<str>>,
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
        }
    }

    pub fn load_config(&self) -> Result<ResolvedConfig> {
        load_config(&self.config_path)
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
    bind: SocketAddr,
    api_token: Option<String>,
) -> Result<ApiHandle> {
    validate_bind(bind, api_token.as_deref())?;
    let state = ApiState::new(jobs, config_path, api_token);
    let app = routes::router(state);
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind HTTP API on {bind}"))?;
    let bound = listener
        .local_addr()
        .with_context(|| format!("failed to read local address for {bind}"))?;
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
    Ok(ApiHandle {
        bind: bound,
        shutdown_tx: Some(shutdown_tx),
        join,
    })
}

/// Headless API-only mode (`soki-ci serve`).
pub async fn run_server(
    jobs: Arc<JobOrchestrator>,
    config_path: PathBuf,
    bind: SocketAddr,
    api_token: Option<String>,
) -> Result<()> {
    let handle = spawn_api(jobs, config_path, bind, api_token).await?;
    eprintln!(
        "soki-ci API listening on http://{} (GET /projects/builds, POST /projects/{{id}}/builds/{{id}})",
        handle.bind
    );
    tokio::signal::ctrl_c()
        .await
        .context("waiting for Ctrl+C")?;
    eprintln!("soki-ci API shutting down…");
    handle.shutdown().await;
    Ok(())
}

pub(crate) fn projects_builds_json(cfg: &ResolvedConfig) -> serde_json::Value {
    crate::commands::projects::builds_catalog(cfg)
}
