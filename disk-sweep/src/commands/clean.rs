use std::io::{self, IsTerminal, Write};

use anyhow::{bail, Result};

use crate::clean::clean_items;
use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::scan::{format_bytes, scan_targets, ScanItem};
use crate::targets::default_targets;

pub fn run(
    ctx: &CommandContext,
    target_ids: &[String],
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let targets = default_targets();
    let mut report = scan_targets(&targets)?;

    if !target_ids.is_empty() {
        for cat in &mut report.categories {
            for item in &mut cat.items {
                item.selected = target_ids.iter().any(|id| {
                    id == &item.id || id == &item.target_id
                });
            }
        }
    }

    let items: Vec<ScanItem> = report
        .categories
        .iter()
        .flat_map(|c| c.items.clone())
        .filter(|i| i.selected)
        .collect();

    if items.is_empty() {
        bail!("no items selected for cleanup");
    }

    let selected_bytes: u64 = items.iter().map(|i| i.size_bytes).sum();

    if !dry_run && !yes && !io::stdout().is_terminal() {
        bail!("refusing to delete in non-interactive mode without --yes or --dry-run");
    }

    if !dry_run && !yes && io::stdout().is_terminal() {
        println!(
            "About to delete {} items ({})",
            items.len(),
            format_bytes(selected_bytes),
        );
        print!("Type 'yes' to confirm: ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.trim() != "yes" {
            bail!("aborted");
        }
    }

    let clean_report = clean_items(&items, dry_run)?;
    let error_count = clean_report.error_count;

    if ctx.json {
        Envelope::ok("clean", clean_report)
            .with_warnings(if error_count > 0 {
                vec![format!("{error_count} paths failed to delete")]
            } else {
                vec![]
            })
            .print_json()?;
        return Ok(());
    }

    let action = if dry_run { "Would free" } else { "Freed" };
    println!(
        "{action} {} across {} items ({} errors)",
        format_bytes(clean_report.bytes_freed),
        clean_report.results.len(),
        clean_report.error_count,
    );
    for r in &clean_report.results {
        if let Some(err) = &r.error {
            eprintln!("  error {}: {err}", r.path);
        } else if dry_run {
            println!("  would delete {} ({})", r.path, format_bytes(r.bytes_freed));
        } else if r.deleted {
            println!("  deleted {} ({})", r.path, format_bytes(r.bytes_freed));
        }
    }

    Ok(())
}
