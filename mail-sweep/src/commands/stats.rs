use anyhow::Result;
use serde::Serialize;

use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::store::{CategoryStat, DailyStat, SenderStat, Store};

#[derive(Debug, Serialize)]
struct StatsReport {
    total: i64,
    days: i64,
    by_category: Vec<CategoryStat>,
    top_senders: Vec<SenderStat>,
    daily_volume: Vec<DailyStat>,
}

pub fn run(ctx: &CommandContext, account: Option<String>, days: i64) -> Result<()> {
    let store = Store::open(&ctx.app.db_path())?;
    let report = StatsReport {
        total: store.total_count(account.as_deref())?,
        days,
        by_category: store.category_stats(account.as_deref(), days)?,
        top_senders: store.sender_stats(account.as_deref(), 10)?,
        daily_volume: store.daily_stats(account.as_deref(), days)?,
    };

    if ctx.json {
        Envelope::ok("stats", report).print_json()?;
        return Ok(());
    }

    println!("Total cached: {}", report.total);
    println!("\nBy category (last {days} days):");
    for c in &report.by_category {
        println!("  {:<14} {}", c.category, c.count);
    }
    println!("\nTop senders:");
    for s in &report.top_senders {
        println!(
            "  {:<30} {} ({})",
            truncate(&s.from_address, 30),
            s.count,
            s.dominant_category.as_deref().unwrap_or("-")
        );
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
