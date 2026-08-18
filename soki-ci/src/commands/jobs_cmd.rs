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
