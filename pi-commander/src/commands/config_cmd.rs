use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::commands::AppContext;
use crate::config::validate;

pub fn run_init(config_path: &Path, json: bool) -> Result<()> {
    if config_path.exists() {
        bail!(
            "config already exists at {} — refusing to overwrite",
            config_path.display()
        );
    }
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let example = include_str!("../../projects.example.yaml");
    fs::write(config_path, example).with_context(|| format!("write {}", config_path.display()))?;
    if json {
        crate::output::Envelope::ok(
            "config init",
            serde_json::json!({ "path": config_path.display().to_string() }),
        )
        .print_json()?;
    } else {
        println!("wrote {}", config_path.display());
    }
    Ok(())
}

pub fn run_path(config_path: &Path, json: bool) -> Result<()> {
    if json {
        crate::output::Envelope::ok(
            "config path",
            serde_json::json!({ "path": config_path.display().to_string() }),
        )
        .print_json()?;
    } else {
        println!("{}", config_path.display());
    }
    Ok(())
}

pub fn run_validate(ctx: &AppContext, json: bool) -> Result<()> {
    let config = ctx.require_config()?;
    let warnings = validate(config)?;
    let project_count = config.projects.len();
    let agents_total: usize = config.projects.iter().map(|p| p.agents).sum();
    let data = serde_json::json!({
        "ok": true,
        "config": ctx.config_path.display().to_string(),
        "projects": project_count,
        "configured_agents": agents_total,
        "pi": config.defaults.pi,
        "broker": config.defaults.speak.base_url,
        "telegram_enabled": config.defaults.telegram.enabled,
        "warnings": warnings,
    });
    if json {
        crate::output::Envelope::ok("config validate", data).print_json()
    } else {
        println!("{}", serde_json::to_string_pretty(&data)?);
        for w in &warnings {
            eprintln!("warning: {w}");
        }
        println!(
            "config OK — {} projects, {} configured agents",
            config.projects.len(),
            agents_total
        );
        Ok(())
    }
}