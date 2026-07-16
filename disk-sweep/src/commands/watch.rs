use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use crate::commands::CommandContext;
use crate::interval::parse_interval;
use crate::output::Envelope;
use crate::watch_data::{collect_snapshot, resolve_watch_paths, ScanKind};

pub fn run_cli(
    ctx: &CommandContext,
    extra_paths: &[PathBuf],
    interval_str: &str,
    top_n: usize,
) -> Result<()> {
    let interval = parse_interval(interval_str)?;
    let paths = resolve_watch_paths(extra_paths);
    let snapshot = collect_snapshot(&paths, top_n, ScanKind::Full)?;

    if ctx.json {
        Envelope::ok("watch", snapshot)
            .with_inputs(serde_json::json!({
                "paths": paths.iter().map(|(l, p)| serde_json::json!({"label": l, "path": p})).collect::<Vec<_>>(),
                "interval": interval_str,
                "top": top_n,
            }))
            .with_next_actions(vec![
                "disk-sweep watch".into(),
                "disk-sweep interactive".into(),
            ])
            .print_json()?;
        return Ok(());
    }

    let vol = &snapshot.volume;
    println!("Disk watch snapshot");
    println!(
        "  Volume: {} — {:.1}% used",
        vol.mount_path.display(),
        vol.used_ratio * 100.0
    );
    println!(
        "  {} used / {} total ({} free)",
        crate::scan::format_bytes(vol.used_bytes),
        crate::scan::format_bytes(vol.total_bytes),
        crate::scan::format_bytes(vol.available_bytes),
    );
    println!("\nWatched folders:");
    for f in &snapshot.folders {
        println!("  {} — {} ({:.1}% of volume)", f.label, f.size_human, f.pct_of_volume * 100.0);
    }
    println!("\nCleanup categories:");
    for c in &snapshot.categories {
        println!("  {} — {} ({} items)", c.name, c.total_human, c.item_count);
    }
    println!(
        "\nReclaimable (selected): {} ({} items)",
        snapshot.reclaimable_human, snapshot.selected_count
    );
    let vol_note = if interval.is_zero() {
        "volume manual (v); deep scan manual (r)".to_string()
    } else {
        format!("volume every {}; deep scan manual (r)", format_duration(interval))
    };
    println!("\nRun `disk-sweep watch` for live TUI ({vol_note}).");

    Ok(())
}

pub async fn run_tui(extra_paths: &[PathBuf], interval_str: &str, top_n: usize) -> Result<()> {
    let interval = parse_interval(interval_str)?;
    crate::ui::run_watch(extra_paths, interval, top_n).await
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}
