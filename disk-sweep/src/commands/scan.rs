use anyhow::Result;

use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::scan::{format_bytes, scan_targets};
use crate::targets::default_targets;

pub fn run(ctx: &CommandContext, detail: bool) -> Result<()> {
    let targets = default_targets();
    let report = scan_targets(&targets)?;

    if ctx.json {
        let data = if detail {
            serde_json::json!(report)
        } else {
            serde_json::json!({
                "total_bytes": report.total_bytes,
                "total_human": format_bytes(report.total_bytes),
                "selected_bytes": report.selected_bytes,
                "selected_human": format_bytes(report.selected_bytes),
                "item_count": report.item_count,
                "selected_count": report.selected_count,
                "categories": report.categories.iter().map(|c| serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "total_bytes": c.total_bytes,
                    "total_human": format_bytes(c.total_bytes),
                    "selected_bytes": c.selected_bytes,
                    "item_count": c.items.len(),
                })).collect::<Vec<_>>(),
            })
        };
        Envelope::ok("scan", data)
            .with_next_actions(vec![
                "disk-sweep interactive".into(),
                "disk-sweep clean --dry-run --json".into(),
            ])
            .print_json()?;
        return Ok(());
    }

    println!("Disk sweep scan\n");
    for cat in &report.categories {
        println!(
            "  {} — {} ({} selected: {})",
            cat.name,
            format_bytes(cat.total_bytes),
            cat.items.iter().filter(|i| i.selected).count(),
            format_bytes(cat.selected_bytes),
        );
        if detail {
            for item in &cat.items {
                let mark = if item.selected { "[x]" } else { "[ ]" };
                let exists = if item.exists { "" } else { " (missing)" };
                println!(
                    "    {} {} — {}{}",
                    mark,
                    item.name,
                    format_bytes(item.size_bytes),
                    exists,
                );
            }
        }
    }
    println!(
        "\nTotal: {} | Selected: {} ({} items)",
        format_bytes(report.total_bytes),
        format_bytes(report.selected_bytes),
        report.selected_count,
    );

    Ok(())
}
