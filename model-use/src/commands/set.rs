use anyhow::Result;

use crate::commands::AppContext;
use crate::config::{format_duration_secs, parse_duration_secs, ModelUseConfig};
use crate::output::Envelope;

pub fn run_refresh_interval(ctx: &mut AppContext, value: &str) -> Result<()> {
    let secs = parse_duration_secs(value)?;
    ctx.config.tui.refresh_interval_secs = secs;
    ModelUseConfig::save(&ctx.config_path, &ctx.config)?;

    if ctx.json {
        Envelope::ok(
            "set refresh-interval",
            serde_json::json!({
                "refresh_interval_secs": secs,
                "refresh_interval": format_duration_secs(secs),
            }),
        )
        .print_json()?;
    } else if secs == 0 {
        println!("Disabled TUI auto-refresh");
    } else {
        println!(
            "Set TUI auto-refresh to {} ({} seconds)",
            format_duration_secs(secs),
            secs
        );
    }
    Ok(())
}

pub fn run_list(ctx: &AppContext) -> Result<()> {
    let secs = ctx.config.tui.refresh_interval_secs;
    if ctx.json {
        Envelope::ok(
            "set list",
            serde_json::json!({
                "tui": {
                    "refresh_interval_secs": secs,
                    "refresh_interval": format_duration_secs(secs),
                }
            }),
        )
        .print_json()?;
    } else {
        println!("TUI settings:");
        println!(
            "  refresh-interval: {} ({} seconds)",
            format_duration_secs(secs),
            secs
        );
    }
    Ok(())
}
