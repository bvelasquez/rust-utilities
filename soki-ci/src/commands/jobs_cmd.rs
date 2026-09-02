use anyhow::Result;
use serde_json::json;
use std::fs;
use uuid::Uuid;

use crate::commands::{require_yes, AppContext};
use crate::output::Envelope;

pub fn run_list(ctx: &AppContext, json: bool) -> Result<()> {
    let jobs = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(ctx.jobs.list_jobs())
    });
    let data = json!({ "jobs": jobs });
    if json {
        Envelope::ok("jobs list", data).print_json()?;
    } else {
        for j in jobs {
            println!(
                "{} {} {} {:?} exit={:?}",
                j.id, j.project_id, j.target_id, j.phase, j.exit_code
            );
        }
    }
    Ok(())
}

pub fn run_logs(ctx: &AppContext, id: &str, tail: usize, json: bool) -> Result<()> {
    let uuid = Uuid::parse_str(id)?;
    let jobs = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(ctx.jobs.list_jobs())
    });
    let job = jobs
        .into_iter()
        .find(|j| j.id == uuid)
        .ok_or_else(|| anyhow::anyhow!("job not found"))?;
    let content = fs::read_to_string(&job.log_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let text = if tail > 0 && lines.len() > tail {
        lines[lines.len() - tail..].join("\n")
    } else {
        content
    };
    let data = json!({
        "id": id,
        "path": job.log_path,
        "content": text,
    });
    if json {
        Envelope::ok("jobs logs", data).print_json()?;
    } else {
        println!("{text}");
    }
    Ok(())
}

pub async fn run_input(
    bind: &str,
    api_token: Option<&str>,
    id: &str,
    text: &str,
    yes: bool,
    json: bool,
) -> Result<()> {
    require_yes(yes, "send job stdin")?;
    let _ = Uuid::parse_str(id)?;
    let url = format!("http://{bind}/jobs/{id}/input");
    let mut req = reqwest::Client::new()
        .post(&url)
        .json(&json!({ "text": text }));
    if let Some(token) = api_token {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| {
        anyhow::anyhow!(
            "could not reach soki-ci API at {bind} ({e}); start the TUI or `soki-ci serve`"
        )
    })?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        anyhow::bail!("API {status}: {err}");
    }
    let data = json!({ "id": id, "sent": true, "text": text });
    if json {
        Envelope::ok("jobs input", data).print_json()?;
    } else {
        println!("sent stdin to {id}");
    }
    Ok(())
}

pub fn run_reset(ctx: &AppContext, yes: bool, json: bool) -> Result<()> {
    require_yes(yes, "reset job history")?;
    let removed = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(ctx.jobs.clear_history())
    })?;
    let data = json!({ "removed": removed });
    if json {
        Envelope::ok("jobs reset", data).print_json()?;
    } else {
        println!("cleared {removed} finished deployment(s)");
    }
    Ok(())
}

pub fn run_cancel(ctx: &AppContext, id: &str, yes: bool, json: bool) -> Result<()> {
    require_yes(yes, "cancel job")?;
    let uuid = Uuid::parse_str(id)?;
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(ctx.jobs.cancel(uuid))
    })?;
    let data = json!({ "id": id, "cancelled": true });
    if json {
        Envelope::ok("jobs cancel", data).print_json()?;
    } else {
        println!("cancelled {id} (process may still run until v1.1 kill support)");
    }
    Ok(())
}
