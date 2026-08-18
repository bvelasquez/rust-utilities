use anyhow::Result;
use serde_json::json;

use crate::commands::AppContext;

pub fn run_list(ctx: &AppContext, json: bool) -> Result<()> {
    let config = ctx.require_config()?;
    let projects: Vec<_> = config
        .projects
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.display_name(),
                "path": p.path.display().to_string(),
                "agents": p.agents,
                "model": p.model,
            })
        })
        .collect();
    if json {
        crate::output::Envelope::ok("projects list", json!({ "projects": projects })).print_json()?;
    } else {
        println!(
            "{:<20} {:<6} {:<28} {}",
            "id", "agents", "name", "path"
        );
        for p in &config.projects {
            println!(
                "{:<20} {:<6} {:<28} {}",
                p.id,
                p.agents,
                p.display_name(),
                p.path.display()
            );
        }
        println!(
            "\n{} project(s), {} configured agent(s)",
            config.projects.len(),
            config.projects.iter().map(|p| p.agents).sum::<usize>()
        );
    }
    Ok(())
}