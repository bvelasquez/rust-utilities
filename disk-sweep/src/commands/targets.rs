use anyhow::Result;

use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::targets::{default_categories, default_targets};

pub fn run_list(ctx: &CommandContext) -> Result<()> {
    let targets = default_targets();

    if ctx.json {
        Envelope::ok("targets list", targets).print_json()?;
        return Ok(());
    }

    for t in targets {
        println!(
            "{} [{}] {} — {}",
            t.id,
            t.category,
            t.name,
            t.path.display()
        );
        println!("    {}", t.description);
    }

    Ok(())
}

pub fn run_explain(ctx: &CommandContext) -> Result<()> {
    let categories = default_categories();
    let targets = default_targets();

    if ctx.json {
        Envelope::ok(
            "targets explain",
            serde_json::json!({
                "categories": categories,
                "targets": targets,
                "notes": [
                    "Only paths listed here are scanned for cleanup",
                    "expand_children targets list each immediate child as a separate item",
                    "Simulator temp caches does NOT touch CoreSimulator/Devices (installed sims/apps)",
                    "User caches and logs are opt-in (not selected by default)"
                ]
            }),
        )
        .print_json()?;
        return Ok(());
    }

    println!("disk-sweep cleanup targets\n");
    println!("Only regenerable caches, build artifacts, and logs are included.");
    println!("Source code, git repos, documents, and simulator devices are never scanned.\n");

    for cat in &categories {
        println!("## {}", cat.name);
        println!("{}\n", cat.description);
        for t in targets.iter().filter(|t| t.category == cat.id) {
            let default = if t.selected_by_default {
                "selected by default"
            } else {
                "opt-in"
            };
            let mode = if t.expand_children {
                "each child folder"
            } else {
                "whole folder"
            };
            println!("  {} ({default}, {mode})", t.name);
            println!("    Path: {}", t.path.display());
            println!("    {}", t.description);
            println!();
        }
    }

    println!("Run `disk-sweep targets list --json` for machine-readable IDs.");

    Ok(())
}
