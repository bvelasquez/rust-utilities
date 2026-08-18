use axum::extract::Request;
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use super::handlers;
use super::ApiState;

pub fn router(state: ApiState) -> Router {
    let public = Router::new()
        .route("/health", get(handlers::health))
        .with_state(state.clone());

    let protected = Router::new()
        .route("/projects/builds", get(handlers::list_projects_builds))
        .route(
            "/projects/{project_id}/builds/{build_id}",
            post(handlers::trigger_build),
        )
        .route("/jobs", get(handlers::list_jobs))
        .route("/jobs/{job_id}", get(handlers::get_job))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state);

    public.merge(protected)
}

async fn require_auth(
    axum::extract::State(state): axum::extract::State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.api_token.as_deref() else {
        return next.run(request).await;
    };
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| h == format!("Bearer {expected}"));
    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "missing or invalid Authorization: Bearer token"
            })),
        )
            .into_response()
    }
}
