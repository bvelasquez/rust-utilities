use anyhow::{bail, Result};
use serde_json::json;
use uuid::Uuid;

use crate::commands::{require_yes, AppContext};
use crate::config::find_project;
use crate::jobs::JobPhase;
use crate::output::Envelope;

pub async fn run(
    ctx: &AppContext,
    project_id: &str,
    target_id: Option<&str>,
    all: bool,
    yes: bool,
    wait: bool,
    json: bool,
) -> Result<()> {
    require_yes(yes, "start deploy")?;

    let project = find_project(&ctx.config, project_id)
        .ok_or_else(|| anyhow::anyhow!("unknown project `{project_id}`"))?;

    let mut ids = Vec::new();

    if all {
        for (tid, target) in &project.targets {
            let id = ctx.jobs.enqueue(project, target).await?;
            ids.push((tid.clone(), id));
            wait_for_job(ctx, id).await?;
        }
    } else {
        let tid = target_id.ok_or_else(|| {
            anyhow::anyhow!("specify --target <id> or --all")
        })?;
        let target = project
            .targets
            .get(tid)
            .ok_or_else(|| anyhow::anyhow!("unknown target `{tid}` for project `{project_id}`"))?;
        let id = ctx.jobs.enqueue(project, target).await?;
        ids.push((tid.to_string(), id));
    }

    if wait && !all {
        for (_, id) in &ids {
            wait_for_job(ctx, *id).await?;
        }
    }

    let data = json!({
        "jobs": ids.iter().map(|(t, id)| json!({ "target": t, "id": id })).collect::<Vec<_>>(),
    });
    if json {
        Envelope::ok("deploy run", data).print_json()?;
    } else {
        for (t, id) in ids {
            println!("started {project_id}/{t}: {id}");
        }
    }
    Ok(())
}

async fn wait_for_job(ctx: &AppContext, id: Uuid) -> Result<()> {
    loop {
        if let Some(job) = ctx.jobs.get_job(id).await {
            match job.phase {
                JobPhase::Success => return Ok(()),
                JobPhase::Error | JobPhase::Cancelled => {
                    bail!("job {id} finished with {:?}", job.phase);
                }
                JobPhase::Queued | JobPhase::Running => {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        } else {
            bail!("job {id} disappeared");
        }
    }
}
