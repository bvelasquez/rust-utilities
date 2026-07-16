use std::path::PathBuf;

use anyhow::Result;

use crate::analyze::{AnalyzeOptions, AnalyzeReport, default_projects_root};
use crate::commands::CommandContext;
use crate::output::Envelope;

pub fn run(
    ctx: &CommandContext,
    projects_root: Option<PathBuf>,
    stale_days: u32,
    min_mb: u64,
    library_min_mb: u64,
    project_build_min_mb: u64,
    skip_dot: bool,
    skip_library: bool,
) -> Result<()> {
    let options = AnalyzeOptions {
        projects_root: projects_root.unwrap_or_else(default_projects_root),
        stale_days,
        min_bytes: min_mb * 1024 * 1024,
        library_min_bytes: library_min_mb * 1024 * 1024,
        project_build_min_bytes: project_build_min_mb * 1024 * 1024,
        skip_dot,
        skip_library,
    };

    let report = crate::analyze::run_analyze(&options)?;

    if ctx.json {
        Envelope::ok("analyze", summarize(&report))
            .with_next_actions(vec![
                "disk-sweep watch".into(),
                "disk-sweep clean --dry-run --json".into(),
            ])
            .print_json()?;
        return Ok(());
    }

    print_human(&report);
    Ok(())
}

fn summarize(report: &AnalyzeReport) -> serde_json::Value {
    serde_json::json!({
        "projects_root": report.projects_root,
        "stale_days": report.stale_days,
        "dot_count": report.dot_count,
        "library_count": report.library_count,
        "project_count": report.project_count,
        "project_build_count": report.project_build_count,
        "total_bytes": report.total_bytes,
        "total_human": report.total_human,
        "items": report.items,
    })
}

fn print_human(report: &AnalyzeReport) {
    println!("disk-sweep analyze");
    println!(
        "  projects: {} (stale ≥ {} days)",
        report.projects_root.display(),
        report.stale_days
    );
    println!(
        "  found {} items — {} (dot: {}, library: {}, rust: {}, stale: {})",
        report.items.len(),
        report.total_human,
        report.dot_count,
        report.library_count,
        report.project_build_count,
        report.project_count,
    );
    println!("\nNothing selected by default. Use watch (`a`) or clean after reviewing.\n");

    let mut current_parent = String::new();
    for item in &report.items {
        if item.parent_label != current_parent {
            current_parent = item.parent_label.clone();
            println!("## {}", current_parent);
        }
        let tag = if item.risk.is_empty() {
            String::new()
        } else {
            format!(" [{}]", item.risk)
        };
        println!(
            "  {:>10}  {}{}",
            crate::scan::format_bytes(item.size_bytes),
            item.name,
            tag
        );
        if !item.description.is_empty() {
            println!("             {}", item.description);
        }
    }
}
