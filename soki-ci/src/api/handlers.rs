use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::{ApiError, ApiState};
use crate::config::{
    find_project, resolve_file_config, validate_config, FileConfig, ResolvedConfig,
    ResolvedProject, ResolvedTarget,
};
use crate::jobs::{stats_for, DeployConflict, JobPhase, JobRecord, JobStats};

#[derive(Serialize)]
pub struct HealthResponse {
    ok: bool,
    service: &'static str,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "soki-ci",
    })
}

pub async fn capabilities() -> Json<serde_json::Value> {
    Json(crate::capabilities::capabilities_json())
}

pub async fn list_projects_builds(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(super::projects_builds_json(&config)))
}

pub async fn list_jobs(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let jobs = state.jobs.list_jobs().await;
    Json(serde_json::json!({ "jobs": jobs }))
}

pub async fn get_job(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = parse_job_id(&job_id)?;
    let job = state
        .jobs
        .get_job(id)
        .await
        .ok_or_else(|| ApiError::not_found("job not found"))?;
    Ok(Json(serde_json::json!({ "job": job })))
}

#[derive(Deserialize)]
pub struct JobInputRequest {
    pub text: String,
}

pub async fn send_job_input(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
    Json(body): Json<JobInputRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = parse_job_id(&job_id)?;
    state
        .jobs
        .send_input(id, &body.text)
        .await
        .map_err(map_job_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "job_id": id,
    })))
}

#[derive(Serialize)]
pub struct TriggerBuildResponse {
    job_id: Uuid,
    project_id: String,
    build_id: String,
    status: &'static str,
}

pub async fn trigger_build(
    State(state): State<ApiState>,
    Path((project_id, build_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<TriggerBuildResponse>), ApiError> {
    let source = headers
        .get("x-soki-source")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("api");
    trigger_build_impl(state, project_id, build_id, source).await
}

pub async fn trigger_build_web(
    State(state): State<ApiState>,
    Path((project_id, build_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<TriggerBuildResponse>), ApiError> {
    trigger_build_impl(state, project_id, build_id, "web").await
}

async fn trigger_build_impl(
    state: ApiState,
    project_id: String,
    build_id: String,
    source: &str,
) -> Result<(StatusCode, Json<TriggerBuildResponse>), ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.jobs.update_speak(&config);
    let project = find_project(&config, &project_id)
        .ok_or_else(|| ApiError::not_found(format!("unknown project `{project_id}`")))?;
    let target = project.targets.get(&build_id).ok_or_else(|| {
        ApiError::not_found(format!(
            "unknown build `{build_id}` for project `{project_id}`"
        ))
    })?;

    let job_id = state
        .jobs
        .enqueue_with_source(project, target, source)
        .await
        .map_err(|e| {
            if e.downcast_ref::<DeployConflict>().is_some() {
                ApiError::conflict(e.to_string())
            } else {
                ApiError::internal(e.to_string())
            }
        })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerBuildResponse {
            job_id,
            project_id,
            build_id,
            status: "queued",
        }),
    ))
}

pub async fn status(State(state): State<ApiState>) -> Json<Value> {
    let orchestrator = state.jobs.status().await;
    let speak = state.jobs.speak_settings();
    let config = state.load_config();
    let config_error = config.as_ref().err().map(|e| e.to_string());
    let max_parallel = config
        .as_ref()
        .map(|cfg| cfg.defaults.max_parallel)
        .unwrap_or(orchestrator.max_parallel);
    let history_limit = config
        .as_ref()
        .map(|cfg| cfg.defaults.history_limit)
        .unwrap_or_else(|_| state.jobs.max_history());

    let reachable = if speak.enabled {
        tokio::time::timeout(Duration::from_secs(2), state.jobs.broker().health())
            .await
            .ok()
            .and_then(|result| result.ok())
            .unwrap_or(false)
    } else {
        false
    };

    Json(json!({
        "service": "soki-ci",
        "version": env!("CARGO_PKG_VERSION"),
        "started_at": state.started_at,
        "uptime_secs": (Utc::now() - state.started_at).num_seconds(),
        "bind": state.bind,
        "config_path": state.config_path,
        "config_error": config_error,
        "max_parallel": max_parallel,
        "history_limit": history_limit,
        "allow_config_write": state.allow_config_write,
        "orchestrator": orchestrator,
        "tts": {
            "enabled": speak.enabled,
            "base_url": speak.base_url,
            "voice": speak.voice,
            "on": speak.on,
            "reachable": reachable,
        }
    }))
}

pub async fn list_projects(State(state): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let jobs = state.jobs.list_jobs().await;
    let projects: Vec<Value> = config
        .projects
        .iter()
        .map(|project| project_json(project, &jobs))
        .collect();
    Ok(Json(json!({ "projects": projects })))
}

pub async fn get_project(
    State(state): State<ApiState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let jobs = state.jobs.list_jobs().await;
    let project = find_project(&config, &project_id)
        .ok_or_else(|| ApiError::not_found(format!("unknown project `{project_id}`")))?;
    Ok(Json(project_json(project, &jobs)))
}

fn project_json(project: &ResolvedProject, jobs: &[JobRecord]) -> Value {
    let targets: Vec<Value> = project
        .targets
        .values()
        .map(|target| target_json(project, target, jobs))
        .collect();
    json!({
        "id": project.id,
        "name": project.name,
        "path": project.path,
        "package_manager": project.package_manager,
        "group": project.group,
        "tags": project.tags,
        "description": project.description,
        "stats": stats_for(jobs, Some(&project.id), None),
        "targets": targets,
    })
}

fn target_json(project: &ResolvedProject, target: &ResolvedTarget, jobs: &[JobRecord]) -> Value {
    let active = jobs.iter().find(|job| {
        job.project_id == project.id && job.target_id == target.id && job.phase.is_active()
    });
    let last = jobs
        .iter()
        .find(|job| job.project_id == project.id && job.target_id == target.id);
    json!({
        "id": target.id,
        "label": target.label,
        "runner": target.runner,
        "runner_summary": crate::jobs::runner_summary(target),
        "cwd": target.cwd,
        "env_keys": target.env.keys().cloned().collect::<Vec<_>>(),
        "timeout_secs": target.timeout_secs,
        "speak": target.speak,
        "description": target.description,
        "tags": target.tags,
        "danger": target.danger,
        "stats": stats_for(jobs, Some(&project.id), Some(&target.id)),
        "active_job": active.map(job_json),
        "last_job": last.map(job_json),
    })
}

fn parse_job_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::bad_request("invalid job_id"))
}

fn map_job_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("not found") {
        ApiError::not_found("job not found")
    } else if message.contains("not running") || message.contains("stdin") {
        ApiError::conflict(message)
    } else {
        ApiError::internal(message)
    }
}

fn job_json(job: &JobRecord) -> Value {
    let mut value = serde_json::to_value(job).unwrap_or_else(|_| json!({}));
    let log_size = std::fs::metadata(&job.log_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if let Some(object) = value.as_object_mut() {
        object.insert("log_size_bytes".into(), json!(log_size));
        object.insert("can_cancel".into(), json!(job.phase.is_active()));
        object.insert("can_input".into(), json!(job.phase.is_active()));
    }
    value
}

#[derive(Deserialize)]
pub struct JobsQuery {
    pub project: Option<String>,
    pub target: Option<String>,
    pub phase: Option<String>,
    pub source: Option<String>,
    pub q: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn list_jobs_api(
    State(state): State<ApiState>,
    Query(query): Query<JobsQuery>,
) -> Json<Value> {
    let all = state.jobs.list_jobs().await;
    let total_all = all.len();
    let search = query.q.as_deref().map(str::to_lowercase);
    let filtered: Vec<JobRecord> = all
        .into_iter()
        .filter(|job| {
            query
                .project
                .as_deref()
                .is_none_or(|project| job.project_id == project)
        })
        .filter(|job| {
            query
                .target
                .as_deref()
                .is_none_or(|target| job.target_id == target)
        })
        .filter(|job| {
            query
                .phase
                .as_deref()
                .is_none_or(|phase| phase_name(job.phase) == phase.to_lowercase())
        })
        .filter(|job| {
            query
                .source
                .as_deref()
                .is_none_or(|source| job.source == source)
        })
        .filter(|job| {
            search.as_deref().is_none_or(|needle| {
                job.id.to_string().contains(needle)
                    || job.project_id.to_lowercase().contains(needle)
                    || job.project_name.to_lowercase().contains(needle)
                    || job.target_id.to_lowercase().contains(needle)
                    || job.target_label.to_lowercase().contains(needle)
            })
        })
        .collect();

    let total = filtered.len();
    let limit = query.limit.unwrap_or(100).clamp(1, 1_000);
    let offset = query.offset.unwrap_or(0);
    let page: Vec<Value> = filtered
        .iter()
        .skip(offset)
        .take(limit)
        .map(job_json)
        .collect();
    let stats = JobStats::from_jobs(&filtered);

    Json(json!({
        "jobs": page,
        "total": total,
        "total_all": total_all,
        "count": page.len(),
        "limit": limit,
        "offset": offset,
        "has_more": offset.saturating_add(limit) < total,
        "stats": stats,
    }))
}

pub async fn get_job_api(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_job_id(&job_id)?;
    let job = state
        .jobs
        .get_job(id)
        .await
        .ok_or_else(|| ApiError::not_found("job not found"))?;
    Ok(Json(json!({ "job": job_json(&job) })))
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub tail: Option<usize>,
    pub format: Option<String>,
    pub download: Option<bool>,
}

pub async fn get_job_logs(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<Response, ApiError> {
    let id = parse_job_id(&job_id)?;
    let job = state
        .jobs
        .get_job(id)
        .await
        .ok_or_else(|| ApiError::not_found("job not found"))?;

    if query.download.unwrap_or(false) {
        let content = tokio::fs::read_to_string(&job.log_path)
            .await
            .unwrap_or_default();
        let filename = format!("soki-ci-{id}.log");
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            )
            .body(content.into())
            .map_err(|_| ApiError::internal("failed to build download response"));
    }

    let tail = query.tail.unwrap_or(500).clamp(1, 20_000);
    let lines = state
        .jobs
        .read_log(id, tail)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if query.format.as_deref() == Some("text") {
        let body = lines
            .iter()
            .map(|line| line.line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(body.into())
            .map_err(|_| ApiError::internal("failed to build log response"));
    }

    Ok(Json(json!({
        "job_id": id,
        "lines": lines,
        "count": lines.len(),
    }))
    .into_response())
}

pub async fn cancel_job(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let id = parse_job_id(&job_id)?;
    state.jobs.cancel(id).await.map_err(map_job_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "ok": true, "job_id": id, "status": "cancelling" })),
    ))
}

pub async fn retry_job(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let id = parse_job_id(&job_id)?;
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let new_id = state.jobs.retry(&config, id, "web").await.map_err(|e| {
        if e.downcast_ref::<DeployConflict>().is_some() {
            ApiError::conflict(e.to_string())
        } else if e.to_string().contains("not found") {
            ApiError::not_found(e.to_string())
        } else {
            ApiError::internal(e.to_string())
        }
    })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "ok": true, "job_id": new_id, "retried_from": id })),
    ))
}

pub async fn reset_jobs(State(state): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let removed = state
        .jobs
        .clear_history()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "removed": removed })))
}

#[derive(Deserialize)]
pub struct ConfigQuery {
    pub raw: Option<bool>,
}

pub async fn get_config(
    State(state): State<ApiState>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<Value>, ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let jobs = state.jobs.list_jobs().await;
    let issues = validate_config(&config);
    let raw = if query.raw.unwrap_or(false) {
        tokio::fs::read_to_string(&state.config_path).await.ok()
    } else {
        None
    };
    Ok(Json(config_json(&config, &jobs, issues, raw)))
}

pub async fn validate_config_endpoint(
    State(state): State<ApiState>,
) -> Result<Json<Value>, ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let issues = validate_config(&config);
    Ok(Json(json!({
        "path": config.path,
        "issue_count": issues.len(),
        "errors": issues.iter().filter(|issue| issue.level == "error").count(),
        "warnings": issues.iter().filter(|issue| issue.level == "warning").count(),
        "issues": issues,
    })))
}

pub async fn reload_config(State(state): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.jobs.update_speak(&config);
    state
        .jobs
        .notify_config_changed(state.config_path.to_string_lossy().into_owned());
    let jobs = state.jobs.list_jobs().await;
    let issues = validate_config(&config);
    Ok(Json(config_json(&config, &jobs, issues, None)))
}

#[derive(Deserialize)]
pub struct ConfigWriteRequest {
    pub yaml: String,
}

pub async fn put_config(
    State(state): State<ApiState>,
    Json(body): Json<ConfigWriteRequest>,
) -> Result<Json<Value>, ApiError> {
    if !state.allow_config_write {
        return Err(ApiError::forbidden(
            "config writes are disabled; restart with --allow-config-write",
        ));
    }
    let file: FileConfig = serde_yaml::from_str(&body.yaml)
        .map_err(|e| ApiError::bad_request(format!("invalid YAML: {e}")))?;
    let config = resolve_file_config(&state.config_path, file)
        .map_err(|e| ApiError::bad_request(format!("invalid config: {e}")))?;
    let issues = validate_config(&config);
    let errors: Vec<&crate::config::ValidationIssue> = issues
        .iter()
        .filter(|issue| issue.level == "error")
        .collect();
    if !errors.is_empty() {
        return Ok(Json(json!({
            "ok": false,
            "issues": issues,
            "error": "config has validation errors",
        })));
    }

    if let Ok(previous) = tokio::fs::read_to_string(&state.config_path).await {
        let backup = state.config_path.with_extension("yaml.bak");
        let _ = tokio::fs::write(&backup, previous).await;
    }
    let tmp = state.config_path.with_extension("yaml.tmp");
    tokio::fs::write(&tmp, &body.yaml)
        .await
        .map_err(|e| ApiError::internal(format!("write temp config: {e}")))?;
    tokio::fs::rename(&tmp, &state.config_path)
        .await
        .map_err(|e| ApiError::internal(format!("commit config: {e}")))?;

    let reloaded = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.jobs.update_speak(&reloaded);
    state
        .jobs
        .notify_config_changed(state.config_path.to_string_lossy().into_owned());
    let jobs = state.jobs.list_jobs().await;
    let issues = validate_config(&reloaded);
    Ok(Json(json!({
        "ok": true,
        "config": config_json(&reloaded, &jobs, issues, Some(body.yaml)),
    })))
}

fn config_json(
    config: &ResolvedConfig,
    jobs: &[JobRecord],
    issues: Vec<crate::config::ValidationIssue>,
    raw: Option<String>,
) -> Value {
    json!({
        "path": config.path,
        "version": crate::config::CONFIG_VERSION,
        "defaults": config.defaults,
        "projects": config.projects.iter().map(|project| project_json(project, jobs)).collect::<Vec<_>>(),
        "issues": issues,
        "raw": raw,
    })
}

pub async fn tts_status(State(state): State<ApiState>) -> Json<Value> {
    let speak = state.jobs.speak_settings();
    let reachable = if speak.enabled {
        tokio::time::timeout(Duration::from_secs(2), state.jobs.broker().health())
            .await
            .ok()
            .and_then(|result| result.ok())
            .unwrap_or(false)
    } else {
        false
    };
    Json(json!({
        "enabled": speak.enabled,
        "base_url": speak.base_url,
        "voice": speak.voice,
        "on": speak.on,
        "reachable": reachable,
    }))
}

#[derive(Deserialize)]
pub struct TtsTestRequest {
    pub text: Option<String>,
}

pub async fn tts_test(
    State(state): State<ApiState>,
    Json(body): Json<TtsTestRequest>,
) -> Result<Json<Value>, ApiError> {
    let text = body
        .text
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "soki-ci test announcement.".to_string());
    state
        .jobs
        .broker()
        .speak(&text)
        .await
        .map_err(|e| ApiError::bad_gateway(e.to_string()))?;
    Ok(Json(json!({ "ok": true, "spoken": text })))
}

fn phase_name(phase: JobPhase) -> String {
    match phase {
        JobPhase::Queued => "queued",
        JobPhase::Running => "running",
        JobPhase::Success => "success",
        JobPhase::Error => "error",
        JobPhase::Cancelled => "cancelled",
        JobPhase::Interrupted => "interrupted",
    }
    .to_string()
}
