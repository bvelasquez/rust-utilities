use anyhow::Result;

use crate::aggregate::{build_summary, Period};
use crate::cli::PeriodArg;
use crate::commands::AppContext;
use crate::output::Envelope;
use crate::store::Store;

pub fn run(ctx: &AppContext, period: PeriodArg) -> Result<()> {
    let store = Store::open(&ctx.cache_path)?;
    let rows = store.daily_rows()?;
    let period = period.into_period();
    let summary = build_summary(&rows, &ctx.config, period);

    if ctx.json {
        let mut envelope = Envelope::ok("summary", &summary);
        let warnings: Vec<String> = summary
            .budgets
            .iter()
            .filter(|b| b.over_budget)
            .map(|b| format!("{} over budget (${:.2} / ${:.2?})", b.label, b.spent_usd, b.budget_usd))
            .collect();
        if !warnings.is_empty() {
            envelope = envelope.with_warnings(warnings);
        }
        envelope.print_json()?;
    } else {
        print_summary(&summary, period);
    }
    Ok(())
}

fn print_summary(summary: &crate::aggregate::SummaryData, period: Period) {
    println!(
        "Summary ({}) — period total ${:.2} · MTD ${:.2}",
        period.label(),
        summary.total_usd,
        summary.mtd_usd
    );
    for (p, cost) in &summary.by_provider {
        println!("  {p}: ${cost:.2}");
    }
    println!("\nBudgets (MTD):");
    for b in &summary.budgets {
        let budget = b
            .budget_usd
            .map(|v| format!("${v:.2}"))
            .unwrap_or_else(|| "—".into());
        let flag = if b.over_budget { " OVER" } else { "" };
        println!("  {}: ${:.2} / {}{}", b.label, b.spent_usd, budget, flag);
    }
}
