use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::json;
use uuid::Uuid;

use crate::commands::{require_yes, AppContext};
use crate::config::{find_project, ResolvedProject, ResolvedTarget};
use crate::jobs::{JobPhase, JobRecord};
use crate::output::Envelope;

/// How long to wait for the server's `/health` probe before assuming no server
/// is running and falling back to an in-process deploy.
const HEALTH_TIMEOUT: Duration = Duration::from_millis(800);

#[allow(clippy::too_many_arguments)]
pub async fn run(
    ctx: &AppContext,
    project_id: &str,
    target_id: Option<&str>,
    all: bool,
    yes: bool,
    wait: bool,
    json: bool,
    bind: &str,
    api_token: Option<&str>,
) -> Result<()> {
    require_yes(yes, "start deploy")?;

    let project = find_project(&ctx.config, project_id)
        .ok_or_else(|| anyhow::anyhow!("unknown project `{project_id}`"))?;

    // Prefer the already-running server (TUI / serve / web). Its orchestrator is
    // the one backing the dashboard, so routing the job there is what makes CLI
    // deploys appear in the web UI. Without a server we run in-process.
    let server = ServerClient::detect(bind, api_token).await;

    let mut ids = Vec::new();

    if all {
        for (tid, target) in &project.targets {
            let id = start_job(ctx, server.as_ref(), project, target, tid).await?;
            ids.push((tid.clone(), id));
            wait_for_job(ctx, server.as_ref(), id).await?;
        }
    } else {
        let tid = target_id.ok_or_else(|| anyhow::anyhow!("specify --target <id> or --all"))?;
        let target = project
            .targets
            .get(tid)
            .ok_or_else(|| anyhow::anyhow!("unknown target `{tid}` for project `{project_id}`"))?;
        let id = start_job(ctx, server.as_ref(), project, target, tid).await?;
        ids.push((tid.to_string(), id));
    }

    if wait && !all {
        for (_, id) in &ids {
            wait_for_job(ctx, server.as_ref(), *id).await?;
        }
    }

    let data = json!({
        "jobs": ids.iter().map(|(t, id)| json!({ "target": t, "id": id })).collect::<Vec<_>>(),
    });
    if json {
        Envelope::ok("deploy run", data).print_json()?;
    } else {
        for (t, id) in ids {
            println!("started {project_id}/{t}: {id}");
        }
    }
    Ok(())
}

/// Queue the job on the running server when available, otherwise in this
/// process. `target_id` is passed separately because the API only needs the id.
async fn start_job(
    ctx: &AppContext,
    server: Option<&ServerClient>,
    project: &ResolvedProject,
    target: &ResolvedTarget,
    target_id: &str,
) -> Result<Uuid> {
    match server {
        Some(server) => server.trigger(&project.id, target_id).await,
        None => ctx.jobs.enqueue_with_source(project, target, "cli").await,
    }
}

async fn wait_for_job(ctx: &AppContext, server: Option<&ServerClient>, id: Uuid) -> Result<()> {
    loop {
        let job = match server {
            Some(server) => server
                .get_job(id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("job {id} disappeared"))?,
            None => ctx
                .jobs
                .get_job(id)
                .await
                .ok_or_else(|| anyhow::anyhow!("job {id} disappeared"))?,
        };
        match job.phase {
            JobPhase::Success => {
                // When running in-process, keep the process alive until the
                // failure/success announcement has been sent.
                if server.is_none() && ctx.jobs.speech_pending(id).await {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                return Ok(());
            }
            JobPhase::Error | JobPhase::Cancelled | JobPhase::Interrupted => {
                if server.is_none() && ctx.jobs.speech_pending(id).await {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                bail!("job {id} finished with {:?}", job.phase);
            }
            JobPhase::Queued | JobPhase::Running => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

/// Thin HTTP client for the running soki-ci API. Used by `deploy run` so jobs
/// land in the same orchestrator that the TUI/web dashboard reads from.
struct ServerClient {
    base: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl ServerClient {
    /// Returns `Some` only when a server answers `/health` at `bind`.
    async fn detect(bind: &str, token: Option<&str>) -> Option<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        let base = normalize_base(bind);
        let resp = client
            .get(format!("{base}/health"))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        Some(Self {
            base,
            token: token.map(str::to_string),
            client,
        })
    }

    async fn trigger(&self, project_id: &str, target_id: &str) -> Result<Uuid> {
        let url = format!("{}/projects/{project_id}/builds/{target_id}", self.base);
        let mut req = self.client.post(&url).header("x-soki-source", "cli");
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| {
            anyhow::anyhow!(
                "could not reach soki-ci API at {} ({e}); start the TUI or `soki-ci serve`",
                self.base
            )
        })?;
        let status = resp.status();
        let body = read_json(resp).await;
        if !status.is_success() {
            bail!("API {status}: {}", api_error(&body));
        }
        let job_id = body
            .get("job_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("API response missing job_id"))?;
        Uuid::parse_str(job_id).context("invalid job_id returned by API")
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<JobRecord>> {
        let url = format!("{}/jobs/{id}", self.base);
        let mut req = self.client.get(&url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| {
            anyhow::anyhow!("lost connection to soki-ci API at {} ({e})", self.base)
        })?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = resp.status();
        let body = read_json(resp).await;
        if !status.is_success() {
            bail!("API {status}: {}", api_error(&body));
        }
        let job = body.get("job").cloned().unwrap_or(body);
        Ok(Some(
            serde_json::from_value(job).context("unexpected job shape from API")?,
        ))
    }
}

fn normalize_base(bind: &str) -> String {
    let trimmed = bind.trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

async fn read_json(resp: reqwest::Response) -> serde_json::Value {
    resp.json().await.unwrap_or_else(|_| json!({}))
}

fn api_error(body: &serde_json::Value) -> String {
    body.get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("request failed")
        .to_string()
}
