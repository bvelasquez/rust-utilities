use std::sync::Arc;

use axum::body::Body;
use http_body_util::BodyExt;
use soki_ci::api::{self, routes};
use soki_ci::config::load_config;
use soki_ci::jobs::{JobOrchestrator, JobPhase};
use tower::ServiceExt;

fn write_test_config(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("projects.yaml");
    std::fs::write(
        &path,
        r#"
version: 1
defaults:
  max_parallel: 2
  speak:
    enabled: false
projects:
  - id: api-test
    path: "."
    targets:
      ok:
        label: OK
        runner:
          kind: shell
          command: ["true"]
"#,
    )
    .unwrap();
    path
}

fn test_state(config_path: &std::path::Path) -> api::ApiState {
    let config = load_config(config_path).unwrap();
    api::ApiState::new(
        Arc::new(JobOrchestrator::new(&config)),
        config_path.to_path_buf(),
        None,
    )
}

#[tokio::test]
async fn api_list_and_trigger_build() {
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let state = test_state(&config_path);
    let app = routes::router(state.clone());

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/projects/builds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["projects"][0]["id"], "api-test");
    assert_eq!(v["projects"][0]["builds"][0]["id"], "ok");

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/projects/api-test/builds/ok")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = v["job_id"].as_str().unwrap();

    for _ in 0..40 {
        if let Some(job) = state
            .jobs
            .get_job(uuid::Uuid::parse_str(job_id).unwrap())
            .await
        {
            if job.phase == JobPhase::Success {
                return;
            }
            if matches!(job.phase, JobPhase::Error | JobPhase::Cancelled) {
                panic!("job failed: {:?}", job.phase);
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job did not finish in time");
}

#[tokio::test]
async fn spawn_api_health_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let config = load_config(&config_path).unwrap();
    let jobs = Arc::new(JobOrchestrator::new(&config));
    let handle = api::spawn_api(jobs, config_path, "127.0.0.1:0".parse().unwrap(), None)
        .await
        .expect("spawn_api");
    let addr = handle.bind;

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/health");
    let mut ok = false;
    for _ in 0..40 {
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                ok = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(ok, "health endpoint not reachable at {url}");
    handle.shutdown().await;
}
