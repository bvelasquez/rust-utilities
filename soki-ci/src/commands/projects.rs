use anyhow::Result;
use serde_json::json;

use crate::commands::AppContext;
use crate::config::ResolvedConfig;
use crate::output::Envelope;

/// Deploy targets exposed as `builds` for the HTTP API.
pub fn builds_catalog(cfg: &ResolvedConfig) -> serde_json::Value {
    let projects: Vec<_> = cfg
        .projects
        .iter()
        .map(|p| {
            let builds: Vec<_> = p
                .targets
                .iter()
                .map(|(id, t)| {
                    json!({
                        "id": id,
                        "label": t.label,
                    })
                })
                .collect();
            json!({
                "id": p.id,
                "name": p.name,
                "path": p.path,
                "builds": builds,
            })
        })
        .collect();
    json!({ "projects": projects })
}

pub fn run_list(ctx: &AppContext, json: bool) -> Result<()> {
    if json {
        let projects: Vec<_> = ctx
            .config
            .projects
            .iter()
            .map(|p| {
                let targets: Vec<_> = p
                    .targets
                    .iter()
                    .map(|(id, t)| {
                        json!({
                            "id": id,
                            "label": t.label,
                        })
                    })
                    .collect();
                json!({
                    "id": p.id,
                    "name": p.name,
                    "path": p.path,
                    "targets": targets,
                })
            })
            .collect();
        let data = json!({ "projects": projects });
        Envelope::ok("projects list", data).print_json()?;
    } else {
        for p in &ctx.config.projects {
            println!("{} ({})", p.name, p.id);
            println!("  {}", p.path.display());
            for (id, t) in &p.targets {
                println!("  - {id}: {}", t.label);
            }
        }
    }
    Ok(())
}
