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
    let data_dir = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("data");
    api::ApiState::new(
        Arc::new(JobOrchestrator::new_with_data_dir(&config, data_dir)),
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
async fn api_send_stdin_answers_prompt() {
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("projects.yaml");
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
      prompt:
        label: Prompt
        runner:
          kind: shell
          command: ["sh", "-c", "printf 'Continue? [y/N] '; read ans; test \"$ans\" = y"]
"#,
    )
    .unwrap();
    let state = test_state(&path);
    let app = routes::router(state.clone());

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/projects/api-test/builds/prompt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = v["job_id"].as_str().unwrap().to_string();

    let mut saw_running = false;
    for _ in 0..40 {
        if let Some(job) = state
            .jobs
            .get_job(uuid::Uuid::parse_str(&job_id).unwrap())
            .await
        {
            if job.phase == JobPhase::Running || job.phase == JobPhase::Queued {
                saw_running = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(saw_running, "job never became queued/running");

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/jobs/{job_id}/input"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text":"y"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "stdin post failed");

    for _ in 0..40 {
        if let Some(job) = state
            .jobs
            .get_job(uuid::Uuid::parse_str(&job_id).unwrap())
            .await
        {
            if job.phase == JobPhase::Success {
                return;
            }
            if matches!(job.phase, JobPhase::Error | JobPhase::Cancelled) {
                panic!("job failed: {:?} tail={:?}", job.phase, job.log_tail);
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("prompt job did not finish in time");
}

#[tokio::test]
async fn spawn_api_health_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let config = load_config(&config_path).unwrap();
    let data_dir = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("data");
    let jobs = Arc::new(JobOrchestrator::new_with_data_dir(&config, data_dir));
    let handle = api::spawn_api(
        jobs,
        config_path,
        api::ApiOptions::new("127.0.0.1:0".parse().unwrap()),
    )
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

#[tokio::test]
async fn api_capabilities_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let state = test_state(&config_path);
    let app = routes::router(state);

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["name"], "soki-ci");
    assert!(v["http"]["endpoints"].as_array().unwrap().len() >= 6);
    let paths: Vec<_> = v["http"]["endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"/capabilities"));
    assert!(paths.contains(&"/projects/builds"));
}

#[tokio::test]
async fn api_web_status_projects_logs_and_reset() {
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let state = test_state(&config_path);
    let app = routes::router(state.clone());

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["service"], "soki-ci");
    assert_eq!(v["max_parallel"], 2);

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["projects"][0]["id"], "api-test");
    assert_eq!(v["projects"][0]["targets"][0]["id"], "ok");
    assert!(v["projects"][0]["targets"][0]["runner_summary"]
        .as_str()
        .unwrap()
        .contains("true"));

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/projects/api-test/builds/ok")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = v["job_id"].as_str().unwrap().to_string();

    for _ in 0..60 {
        if let Some(job) = state
            .jobs
            .get_job(uuid::Uuid::parse_str(&job_id).unwrap())
            .await
        {
            if job.phase == JobPhase::Success {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/jobs/{job_id}/logs?tail=100"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["lines"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| { line["line"].as_str().unwrap_or("").contains("queued:") }));

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/jobs/reset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["removed"], 1);
}

#[tokio::test]
async fn api_cancel_stops_running_process() {
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("projects.yaml");
    std::fs::write(
        &path,
        r#"
version: 1
defaults:
  max_parallel: 1
  speak:
    enabled: false
projects:
  - id: cancel-test
    path: "."
    targets:
      sleep:
        runner:
          kind: shell
          command: ["sh", "-c", "sleep 30"]
"#,
    )
    .unwrap();
    let state = test_state(&path);
    let app = routes::router(state.clone());

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/projects/cancel-test/builds/sleep")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let job_id = v["job_id"].as_str().unwrap().to_string();

    for _ in 0..80 {
        if let Some(job) = state
            .jobs
            .get_job(uuid::Uuid::parse_str(&job_id).unwrap())
            .await
        {
            if job.phase == JobPhase::Running {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/jobs/{job_id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 202);

    for _ in 0..120 {
        if let Some(job) = state
            .jobs
            .get_job(uuid::Uuid::parse_str(&job_id).unwrap())
            .await
        {
            if job.phase == JobPhase::Cancelled {
                assert!(!job.can_cancel);
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job was not cancelled in time");
}

#[tokio::test]
async fn api_history_survives_restart_and_marks_active_jobs_interrupted() {
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("projects.yaml");
    std::fs::write(
        &path,
        r#"
version: 1
defaults:
  max_parallel: 1
  speak:
    enabled: false
projects:
  - id: history-test
    path: "."
    targets:
      sleep:
        runner:
          kind: shell
          command: ["sh", "-c", "sleep 2"]
"#,
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    let data_dir = temp.path().join("data");
    let jobs = Arc::new(JobOrchestrator::new_with_data_dir(
        &config,
        data_dir.clone(),
    ));
    let project = config.projects.first().unwrap();
    let target = project.targets.get("sleep").unwrap();
    let id = jobs.enqueue(project, target).await.unwrap();

    for _ in 0..80 {
        if let Some(job) = jobs.get_job(id).await {
            if job.phase == JobPhase::Running {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let reloaded = JobOrchestrator::new_with_data_dir(&config, data_dir);
    reloaded.recover_interrupted().await;
    let loaded = reloaded.list_jobs().await;
    let job = loaded.iter().find(|job| job.id == id).expect("job loaded");
    assert_eq!(job.phase, JobPhase::Interrupted);
    assert!(job.interrupted);
    assert!(!job.can_cancel);

    jobs.cancel(id).await.unwrap();
}

#[tokio::test]
async fn api_session_auth_and_csrf() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let config = load_config(&config_path).unwrap();
    let data_dir = config_path.parent().unwrap().join("data");
    let state = api::ApiState::new(
        Arc::new(JobOrchestrator::new_with_data_dir(&config, data_dir)),
        config_path,
        Some("secret".into()),
    );
    let app = routes::router(state);

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/session")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"token":"secret"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let csrf = v["csrf_token"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/status")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/jobs/reset")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 403);

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/jobs/reset")
                .header("cookie", &cookie)
                .header("x-soki-csrf", csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn api_config_write_is_gated_and_validates() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let mut state = test_state(&config_path);
    state.allow_config_write = true;
    let app = routes::router(state);

    let yaml = r#"version: 1
defaults:
  max_parallel: 1
  speak:
    enabled: false
projects:
  - id: api-test
    path: "."
    targets:
      ok:
        runner:
          kind: shell
          command: ["true"]
"#;
    let body = serde_json::json!({ "yaml": yaml }).to_string();
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/config")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["ok"], true);
    assert!(std::fs::read_to_string(&config_path)
        .unwrap()
        .contains("max_parallel: 1"));

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"yaml":"not: [valid"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn cli_deploy_run_routes_through_running_server() {
    use soki_ci::commands::AppContext;

    let temp = tempfile::tempdir().unwrap();
    let config_path = write_test_config(&temp);
    let config = load_config(&config_path).unwrap();

    // Separate stores on purpose: the CLI's own orchestrator must stay empty,
    // while the server's orchestrator receives the job (that is what the web
    // UI reads from).
    let server_jobs = Arc::new(JobOrchestrator::new_with_data_dir(
        &config,
        temp.path().join("server-data"),
    ));
    let handle = api::spawn_api(
        server_jobs.clone(),
        config_path.clone(),
        api::ApiOptions::new("127.0.0.1:0".parse().unwrap()),
    )
    .await
    .expect("spawn_api");
    let bind = handle.bind.to_string();

    let ctx = AppContext {
        config_path: config_path.clone(),
        config: config.clone(),
        jobs: Arc::new(JobOrchestrator::new_with_data_dir(
            &config,
            temp.path().join("cli-data"),
        )),
    };

    soki_ci::commands::deploy::run(
        &ctx,
        "api-test",
        Some("ok"),
        false,
        true,
        true,
        true,
        &bind,
        None,
    )
    .await
    .expect("deploy run via server");

    let server_job_list = server_jobs.list_jobs().await;
    assert_eq!(
        server_job_list.len(),
        1,
        "job should be registered with the server"
    );
    assert_eq!(server_job_list[0].source, "cli");
    assert_eq!(server_job_list[0].phase, JobPhase::Success);

    let local_job_list = ctx.jobs.list_jobs().await;
    assert!(
        local_job_list.is_empty(),
        "CLI must not run the job in its own process when a server is up"
    );

    handle.shutdown().await;
}
