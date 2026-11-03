use axum::extract::State;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ApiError, ApiState, SessionInfo};

const SESSION_COOKIE: &str = "soki_session";
const SESSION_TTL_HOURS: i64 = 12;

#[derive(Serialize)]
pub struct SessionResponse {
    pub auth_required: bool,
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub token: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub authenticated: bool,
    pub csrf_token: String,
}

pub fn token_matches(expected: &str, provided: &str) -> bool {
    let a = expected.as_bytes();
    let b = provided.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn parse_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let cookie = cookie.trim();
                let (key, value) = cookie.split_once('=')?;
                (key == name).then(|| value.to_string())
            })
        })
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn current_session(state: &ApiState, headers: &HeaderMap) -> Option<SessionInfo> {
    let id = parse_cookie(headers, SESSION_COOKIE)?;
    let sessions = state.sessions.lock().ok()?;
    let info = sessions.get(&id)?;
    if info.expires_at <= Utc::now() {
        return None;
    }
    Some(info.clone())
}

fn cookie_header(value: &str, max_age: i64) -> String {
    format!("{SESSION_COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}")
}

fn prune_sessions(state: &ApiState) {
    if let Ok(mut sessions) = state.sessions.lock() {
        let now = Utc::now();
        sessions.retain(|_, info| info.expires_at > now);
    }
}

pub async fn session_state(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<SessionResponse> {
    if state.api_token.is_none() {
        return Json(SessionResponse {
            auth_required: false,
            authenticated: true,
            csrf_token: None,
            method: Some("local".into()),
        });
    }
    if let Some(provided) = bearer(&headers) {
        if let Some(expected) = state.api_token.as_deref() {
            if token_matches(expected, provided) {
                return Json(SessionResponse {
                    auth_required: true,
                    authenticated: true,
                    csrf_token: None,
                    method: Some("bearer".into()),
                });
            }
        }
    }
    if let Some(session) = current_session(&state, &headers) {
        return Json(SessionResponse {
            auth_required: true,
            authenticated: true,
            csrf_token: Some(session.csrf),
            method: Some("cookie".into()),
        });
    }
    Json(SessionResponse {
        auth_required: true,
        authenticated: false,
        csrf_token: None,
        method: None,
    })
}

pub async fn login(
    State(state): State<ApiState>,
    Json(body): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let Some(expected) = state.api_token.as_deref() else {
        return Ok(Json(LoginResponse {
            authenticated: true,
            csrf_token: String::new(),
        })
        .into_response());
    };
    if !token_matches(expected, body.token.trim()) {
        return Err(ApiError::unauthorized("invalid API token"));
    }
    prune_sessions(&state);
    let id = Uuid::new_v4().to_string();
    let csrf = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::hours(SESSION_TTL_HOURS);
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.insert(
            id.clone(),
            SessionInfo {
                expires_at,
                csrf: csrf.clone(),
            },
        );
    }
    let cookie = cookie_header(&id, SESSION_TTL_HOURS * 3600);
    let mut response = Json(LoginResponse {
        authenticated: true,
        csrf_token: csrf,
    })
    .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| ApiError::internal("failed to set session cookie"))?,
    );
    Ok(response)
}

pub async fn logout(State(state): State<ApiState>, headers: HeaderMap) -> Response {
    if let Some(id) = parse_cookie(&headers, SESSION_COOKIE) {
        if let Ok(mut sessions) = state.sessions.lock() {
            sessions.remove(&id);
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        cookie_header("", 0)
            .parse()
            .expect("static cookie header is valid"),
    );
    response
}
