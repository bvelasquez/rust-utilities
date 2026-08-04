use std::path::Path;

use serde_json::json;

use crate::config::{init_config_at, validate_config};
use crate::output::Envelope;

pub fn run_init(path: &Path, json: bool) -> anyhow::Result<()> {
    let created = init_config_at(path)?;
    let data = serde_json::json!({
        "path": path,
        "created": created,
    });
    if json {
        Envelope::ok("config init", data).print_json()?;
    } else if created {
        println!("Created {}", path.display());
    } else {
        println!("Already exists: {}", path.display());
    }
    Ok(())
}

pub fn run_path(path: &Path, json: bool) -> anyhow::Result<()> {
    let data = json!({ "path": path });
    if json {
        Envelope::ok("config path", data).print_json()?;
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

pub async fn run_validate(ctx: &crate::commands::AppContext, json: bool) -> anyhow::Result<()> {
    let issues = validate_config(&ctx.config);
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.level == "error")
        .map(|i| i.message.clone())
        .collect();
    let warnings: Vec<_> = issues
        .iter()
        .filter(|i| i.level == "warning")
        .map(|i| i.message.clone())
        .collect();
    let mut broker_ok = None;
    if ctx.config.defaults.speak.enabled {
        broker_ok = Some(ctx.jobs.broker().health().await.unwrap_or(false));
    }
    let data = json!({
        "path": ctx.config_path,
        "issue_count": issues.len(),
        "issues": issues,
        "broker_reachable": broker_ok,
    });
    if json {
        let env = Envelope::ok("config validate", data).with_warnings(warnings);
        if errors.is_empty() {
            env.print_json()?;
        } else {
            Envelope::<serde_json::Value>::err(
                "config validate",
                errors,
                vec!["fix projects.yaml and re-run validate".into()],
            )
            .print_json()?;
            std::process::exit(1);
        }
    } else {
        for i in &issues {
            println!("[{}] {}", i.level, i.message);
        }
        if let Some(ok) = broker_ok {
            println!("broker reachable: {ok}");
        }
        if !errors.is_empty() {
            std::process::exit(1);
        }
    }
    Ok(())
}
