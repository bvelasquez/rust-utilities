use anyhow::Result;
use serde_json::json;

use crate::commands::AppContext;
use crate::output::Envelope;

pub fn run_list(ctx: &AppContext, json: bool) -> Result<()> {
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
    if json {
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
