use axum::extract::{Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use super::{events, handlers, session, web_assets, ApiState};

pub fn router(state: ApiState) -> Router {
    let protected = Router::new()
        // Existing agent/CLI routes.
        .route("/projects/builds", get(handlers::list_projects_builds))
        .route(
            "/projects/{project_id}/builds/{build_id}",
            post(handlers::trigger_build),
        )
        .route("/jobs", get(handlers::list_jobs))
        .route("/jobs/{job_id}", get(handlers::get_job))
        .route("/jobs/{job_id}/input", post(handlers::send_job_input))
        // Web-oriented API.
        .route("/api/status", get(handlers::status))
        .route("/api/projects", get(handlers::list_projects))
        .route("/api/projects/{project_id}", get(handlers::get_project))
        .route(
            "/api/projects/{project_id}/builds/{build_id}",
            post(handlers::trigger_build_web),
        )
        .route(
            "/api/config",
            get(handlers::get_config).put(handlers::put_config),
        )
        .route(
            "/api/config/validate",
            get(handlers::validate_config_endpoint),
        )
        .route("/api/config/reload", post(handlers::reload_config))
        .route("/api/jobs", get(handlers::list_jobs_api))
        .route("/api/jobs/reset", post(handlers::reset_jobs))
        .route("/api/jobs/{job_id}", get(handlers::get_job_api))
        .route("/api/jobs/{job_id}/logs", get(handlers::get_job_logs))
        .route("/api/jobs/{job_id}/events", get(events::job_events))
        .route("/api/jobs/{job_id}/cancel", post(handlers::cancel_job))
        .route("/api/jobs/{job_id}/retry", post(handlers::retry_job))
        .route("/api/jobs/{job_id}/input", post(handlers::send_job_input))
        .route("/api/events", get(events::global_events))
        .route("/api/tts", get(handlers::tts_status))
        .route("/api/tts/test", post(handlers::tts_test))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/health", get(handlers::health))
        .route("/capabilities", get(handlers::capabilities))
        .route(
            "/api/session",
            get(session::session_state)
                .post(session::login)
                .delete(session::logout),
        )
        .merge(protected)
        .fallback(web_assets::spa_fallback)
        .with_state(state)
}

async fn require_auth(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let Some(expected) = state.api_token.as_deref() else {
        return next.run(request).await;
    };

    let bearer = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(provided) = bearer {
        if session::token_matches(expected, provided) {
            return next.run(request).await;
        }
    }

    if let Some(session) = session::current_session(&state, request.headers()) {
        let method = request.method();
        let mutating = !matches!(
            *method,
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        );
        if mutating {
            let csrf = request
                .headers()
                .get("x-soki-csrf")
                .and_then(|value| value.to_str().ok());
            if csrf != Some(session.csrf.as_str()) {
                return (
                    StatusCode::FORBIDDEN,
                    axum::Json(serde_json::json!({
                        "error": "missing or invalid X-Soki-CSRF header"
                    })),
                )
                    .into_response();
            }
        }
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "missing or invalid Authorization: Bearer token or session cookie"
        })),
    )
        .into_response()
}
