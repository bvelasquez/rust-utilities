//! Agent-facing CLI operations. Everything goes through the daemon API so the
//! commander process owns the workers regardless of where commands come from.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::commands::{ApiClient, AppContext};

fn api(ctx: &AppContext) -> ApiClient {
    ApiClient::new(&ctx.api_base)
}

async fn require_daemon(client: &ApiClient) -> Result<()> {
    if client.health().await {
        Ok(())
    } else {
        bail!("commander daemon is not running. Start it: `pi-commander daemon` (or `pi-commander watch` auto-starts it)")
    }
}

fn extract_data(v: Value) -> Value {
    v.get("data").cloned().unwrap_or(v)
}

async fn emit(ctx: &AppContext, command: &str, v: Value) -> Result<()> {
    if ctx.json {
        crate::output::Envelope::ok(command, v).print_json()
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
        Ok(())
    }
}

pub async fn run_spawn(
    ctx: &AppContext,
    project: Option<String>,
    agents: Option<usize>,
    _all: bool,
) -> Result<()> {
    let client = api(ctx);
    require_daemon(&client).await?;
    let req = json!({ "count": agents.unwrap_or(1) });
    let v = match &project {
        Some(p) => client.post(&format!("/projects/{p}/spawn"), req).await,
        None => client.post("/spawn", json!({})).await,
    }?;
    let v = extract_data(v);
    if ctx.json {
        crate::output::Envelope::ok("spawn", v).print_json()
    } else {
        let spawned = v.get("spawned").cloned().unwrap_or(Value::Array(vec![]));
        let ids = spawned
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        println!("spawned {} agent(s): {}", ids.len(), ids.join(", "));
        if ids.is_empty() {
            println!("  (no projects configured, or all already spawned — try `pi-commander config init`)");
        }
        Ok(())
    }
}

pub async fn run_send(ctx: &AppContext, text: &str) -> Result<()> {
    if crate::dispatch_cmds::is_help_command(text) {
        let help = crate::dispatch_cmds::format_help(None);
        if ctx.json {
            crate::output::Envelope::ok("send", serde_json::json!({ "help": help })).print_json()
        } else {
            println!("{help}");
            Ok(())
        }
    } else {
        let client = api(ctx);
        require_daemon(&client).await?;
        let v = client.post("/dispatch", json!({ "text": text })).await?;
        emit(ctx, "send", v).await
    }
}

pub async fn run_verbose(
    ctx: &AppContext,
    verb: &str,
    worker: &str,
    body: Value,
) -> Result<()> {
    let client = api(ctx);
    require_daemon(&client).await?;
    let path = match verb {
        "steer" => format!("/agents/{worker}/steer"),
        "follow_up" => format!("/agents/{worker}/follow_up"),
        "abort" => format!("/agents/{worker}/abort"),
        "model" => format!("/agents/{worker}/model"),
        "thinking" => format!("/agents/{worker}/thinking"),
        "compact" => format!("/agents/{worker}/compact"),
        "bash" => format!("/agents/{worker}/bash"),
        "pi" => format!("/agents/{worker}/pi"),
        "stop" => format!("/agents/{worker}/stop"),
        _ => bail!("unsupported verb {verb}"),
    };
    let v = if body.is_null() {
        client.post(&path, json!({})).await
    } else {
        client.post(&path, body).await
    }?;
    emit(ctx, verb, v).await
}

pub async fn run_status(ctx: &AppContext, logs: Option<usize>) -> Result<()> {
    let client = api(ctx);
    require_daemon(&client).await?;
    let v = client.get("/state").await?;
    let data = extract_data(v);
    if ctx.json {
        crate::output::Envelope::ok("status", data).print_json()?;
        return Ok(());
    }
    let agents = data
        .get("agents")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    if agents.is_empty() {
        println!(
            "no agents running — start with `pi-commander spawn` (daemon: {}).",
            client.base
        );
        return Ok(());
    }
    for a in &agents {
        let id = a.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let phase = a.get("phase").and_then(|v| v.as_str()).unwrap_or("?");
        let model = a.get("model").and_then(|v| v.as_str()).unwrap_or("-");
        let tool = a.get("current_tool").and_then(|v| v.as_str()).unwrap_or("-");
        let text = a.get("last_text").and_then(|v| v.as_str()).unwrap_or("");
        let err = a.get("last_error").and_then(|v| v.as_str()).unwrap_or("");
        let msgs = a.get("message_count").and_then(|v| v.as_u64()).unwrap_or(0);
        println!(
            "{id:<24} {phase:<9} msgs={msgs:<4} {model:<34} tool={tool:<20} {}",
            text.chars().take(50).collect::<String>()
        );
        if !err.is_empty() {
            println!("    ⚠ {err}");
        }
        if let Some(n) = logs {
            if let Some(tail) = a.get("log_tail").and_then(|t| t.as_array()) {
                for line in tail.iter().rev().take(n) {
                    println!("    {}", line.as_str().unwrap_or(""));
                }
            }
        }
    }
    Ok(())
}

pub async fn run_hop(ctx: &AppContext, worker: &str) -> Result<()> {
    let client = api(ctx);
    require_daemon(&client).await?;
    let v = client.get(&format!("/agents/{worker}")).await?;
    let data = extract_data(v);
    let session_file = data.get("session_file").and_then(|s| s.as_str());
    let project_path = data.get("project_path").and_then(|s| s.as_str());
    let Some(sf) = session_file else {
        bail!("agent {worker} has no session file yet (agents get one after first prompt)");
    };
    let cwd = project_path.unwrap_or(".");
    println!("opening pi on session {sf} (cwd {cwd})");

    let in_tmux = std::env::var("TMUX").is_ok();
    if in_tmux {
        std::process::Command::new("tmux")
            .args([
                "new-window",
                "-c",
                cwd,
                &format!("pi --session {}", shell_quote(sf)),
            ])
            .status()
            .context("tmux new-window")?;
        return Ok(());
    }
    let _ = std::process::Command::new("pi")
        .args(["--session", &sf])
        .current_dir(cwd)
        .spawn()
        .context("spawn pi")?;
    Ok(())
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Convenience shared with watch: resolve a worker's fields.
pub async fn agent_data(ctx: &AppContext, worker: &str) -> Result<Value> {
    let client = api(ctx);
    let v = client.get(&format!("/agents/{worker}")).await?;
    Ok(extract_data(v))
}

pub async fn run_new(ctx: &AppContext, project: &str) -> Result<()> {
    run_spawn(ctx, Some(project.to_string()), Some(1), false).await
}