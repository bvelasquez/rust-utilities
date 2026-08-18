use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::config::find_project;

use super::ApiState;

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
    let id = Uuid::parse_str(&job_id).map_err(|_| ApiError::bad_request("invalid job_id"))?;
    let job = state
        .jobs
        .get_job(id)
        .await
        .ok_or_else(|| ApiError::not_found("job not found"))?;
    Ok(Json(serde_json::json!({ "job": job })))
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
) -> Result<(StatusCode, Json<TriggerBuildResponse>), ApiError> {
    let config = state
        .load_config()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let project = find_project(&config, &project_id)
        .ok_or_else(|| ApiError::not_found(format!("unknown project `{project_id}`")))?;
    let target = project.targets.get(&build_id).ok_or_else(|| {
        ApiError::not_found(format!(
            "unknown build `{build_id}` for project `{project_id}`"
        ))
    })?;

    let job_id = state.jobs.enqueue(project, target).await.map_err(|e| {
        if e.to_string().contains("already has a running or queued deploy") {
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

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}
