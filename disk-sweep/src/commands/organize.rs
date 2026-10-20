use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{bail, Result};

use crate::commands::CommandContext;
use crate::organize::{self, default_rules_path};
use crate::output::Envelope;

pub fn run(
    ctx: &CommandContext,
    once: bool,
    watch: bool,
    rules: Option<PathBuf>,
    plan: Option<PathBuf>,
    dry_run: Option<PathBuf>,
    apply: Option<PathBuf>,
) -> Result<()> {
    if watch && (plan.is_some() || dry_run.is_some() || apply.is_some()) {
        bail!("--watch cannot be combined with --plan, --dry-run, or --apply");
    }
    let plan_out = plan.or(dry_run);

    if let Some(apply_path) = apply {
        let catalog = crate::catalog::Catalog::open_default()?;
        let saved = organize::read_plan(&apply_path)?;
        let report = organize::apply_recorded(&saved, &catalog)?;
        print_apply(ctx, &report)?;
        return Ok(());
    }

    if watch {
        let compiled = organize::load_rules(&rules.unwrap_or_else(default_rules_path))?;
        if !ctx.json {
            println!("Auto mode. Files dropped in the source folder are organized as soon as they arrive.");
            println!("Files already there are organized now. Press Ctrl-C to stop.");
        }
        organize::watch_sources(&compiled, ctx.json, &AtomicBool::new(false))?;
        return Ok(());
    }

    if !once && plan_out.is_none() {
        bail!("pass --once, --watch, --plan FILE, --dry-run FILE, or --apply FILE");
    }

    let compiled = organize::load_rules(&rules.unwrap_or_else(default_rules_path))?;
    let catalog = crate::catalog::Catalog::open_default()?;
    let planned = organize::plan_with_catalog(&compiled, &catalog)?;
    if let Some(path) = plan_out {
        organize::write_plan(&path, &planned)?;
        if ctx.json {
            Envelope::ok("organize plan", &planned)
                .with_next_actions(vec![format!(
                    "disk-sweep organize --apply {}",
                    path.display()
                )])
                .print_json()?;
        } else {
            println!(
                "Wrote {} planned moves to {}",
                planned.moves.len(),
                path.display()
            );
            println!(
                "Review the file, then run: disk-sweep organize --apply {}",
                path.display()
            );
        }
        return Ok(());
    }

    let report = organize::apply_recorded(&planned, &catalog)?;
    print_apply(ctx, &report)?;
    Ok(())
}

fn print_apply(ctx: &CommandContext, report: &organize::ApplyReport) -> Result<()> {
    if ctx.json {
        Envelope::ok("organize", report).print_json()?;
        return Ok(());
    }
    println!("Moved {} files, skipped {}", report.moved, report.skipped);
    for item in &report.results {
        if item.moved {
            println!("  {} -> {}", item.source, item.dest);
        } else if let Some(err) = &item.error {
            println!("  skipped {}: {err}", item.source);
        }
    }
    if let Some(log) = &report.log_path {
        println!("Log: {log}");
    }
    Ok(())
}
