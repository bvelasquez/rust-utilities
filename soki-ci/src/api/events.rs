use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use chrono::Utc;
use futures_util::Stream;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use super::{ApiError, ApiState};
use crate::jobs::ServerEvent;

pub type EventStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

fn make_event<T: Serialize>(name: &'static str, data: &T) -> Option<Event> {
    Event::default().event(name).json_data(data).ok()
}

fn server_event_to_sse(event: ServerEvent) -> Option<Event> {
    match event {
        ServerEvent::Snapshot { jobs, status } => make_event(
            "snapshot",
            &serde_json::json!({ "jobs": jobs, "status": status }),
        ),
        ServerEvent::Job { job } => make_event("job", &job),
        ServerEvent::Removed { job_id } => {
            make_event("removed", &serde_json::json!({ "job_id": job_id }))
        }
        ServerEvent::Config { path } => make_event("config", &serde_json::json!({ "path": path })),
        ServerEvent::Heartbeat { ts } => make_event("heartbeat", &serde_json::json!({ "ts": ts })),
    }
}

pub async fn global_events(State(state): State<ApiState>) -> Sse<EventStream> {
    let (jobs, status) = state.jobs.snapshot().await;
    let mut event_rx = state.jobs.subscribe_events();
    let state = state.clone();

    let stream = async_stream::stream! {
        if let Some(event) = make_event(
            "snapshot",
            &serde_json::json!({ "jobs": jobs, "status": status }),
        ) {
            yield Ok(event);
        }
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                received = event_rx.recv() => {
                    match received {
                        Ok(event) => {
                            if let Some(event) = server_event_to_sse(event) {
                                yield Ok(event);
                            }
                        }
                        Err(RecvError::Lagged(_)) => {
                            let (jobs, status) = state.jobs.snapshot().await;
                            if let Some(event) = make_event(
                                "snapshot",
                                &serde_json::json!({ "jobs": jobs, "status": status }),
                            ) {
                                yield Ok(event);
                            }
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
                _ = heartbeat.tick() => {
                    if let Some(event) = make_event(
                        "heartbeat",
                        &serde_json::json!({ "ts": Utc::now() }),
                    ) {
                        yield Ok(event);
                    }
                }
            }
        }
    };

    Sse::new(Box::pin(stream) as EventStream)
}

pub async fn job_events(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Sse<EventStream>, ApiError> {
    let id = Uuid::parse_str(&job_id).map_err(|_| ApiError::bad_request("invalid job_id"))?;
    let job = state
        .jobs
        .get_job(id)
        .await
        .ok_or_else(|| ApiError::not_found("job not found"))?;
    let state = state.clone();

    if let Some((backlog, mut log_rx)) = state.jobs.subscribe_logs(id).await {
        let mut event_rx = state.jobs.subscribe_events();
        let stream = async_stream::stream! {
            if let Some(event) = make_event(
                "snapshot",
                &serde_json::json!({ "job": job, "lines": backlog }),
            ) {
                yield Ok(event);
            }
            if job.phase.is_terminal() {
                return;
            }
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    line = log_rx.recv() => {
                        match line {
                            Ok(line) => {
                                let event = Event::default()
                                    .event("log")
                                    .id(line.seq.to_string())
                                    .json_data(&line);
                                if let Ok(event) = event {
                                    yield Ok(event);
                                }
                            }
                            Err(RecvError::Lagged(_)) => {
                                let lines = state.jobs.read_log(id, 1_000).await.unwrap_or_default();
                                let job = state.jobs.get_job(id).await;
                                if let Some(event) = make_event(
                                    "resync",
                                    &serde_json::json!({ "job": job, "lines": lines }),
                                ) {
                                    yield Ok(event);
                                }
                            }
                            Err(RecvError::Closed) => break,
                        }
                    }
                    received = event_rx.recv() => {
                        match received {
                            Ok(ServerEvent::Job { job }) if job.id == id => {
                                let terminal = job.phase.is_terminal();
                                if let Some(event) = make_event("job", &job) {
                                    yield Ok(event);
                                }
                                if terminal {
                                    break;
                                }
                            }
                            Ok(ServerEvent::Removed { job_id }) if job_id == id => {
                                if let Some(event) = make_event(
                                    "removed",
                                    &serde_json::json!({ "job_id": job_id }),
                                ) {
                                    yield Ok(event);
                                }
                                break;
                            }
                            Ok(_) => {}
                            Err(RecvError::Lagged(_)) => {
                                if let Some(job) = state.jobs.get_job(id).await {
                                    if let Some(event) = make_event("job", &job) {
                                        yield Ok(event);
                                    }
                                }
                            }
                            Err(RecvError::Closed) => break,
                        }
                    }
                    _ = heartbeat.tick() => {
                        if let Some(event) = make_event(
                            "heartbeat",
                            &serde_json::json!({ "ts": Utc::now() }),
                        ) {
                            yield Ok(event);
                        }
                    }
                }
            }
        };
        return Ok(Sse::new(Box::pin(stream) as EventStream));
    }

    let lines = state.jobs.read_log(id, 2_000).await.unwrap_or_default();
    let stream = async_stream::stream! {
        if let Some(event) = make_event(
            "snapshot",
            &serde_json::json!({ "job": job, "lines": lines, "source": "file" }),
        ) {
            yield Ok(event);
        }
        if let Some(event) = make_event("end", &serde_json::json!({ "job_id": id })) {
            yield Ok(event);
        }
    };
    Ok(Sse::new(Box::pin(stream) as EventStream))
}
