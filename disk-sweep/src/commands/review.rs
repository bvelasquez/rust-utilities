use anyhow::Result;

use crate::agent;
use crate::commands::CommandContext;
use crate::output::Envelope;

pub async fn run(ctx: &CommandContext, path: &std::path::Path, limit: usize) -> Result<()> {
    let report = agent::review_folder(&ctx.app, path, limit).await?;

    if ctx.json {
        Envelope::ok("review", &report)
            .with_next_actions(vec![
                "Add safe_cleanup paths to disk-sweep targets config (future)".into(),
                "disk-sweep clean --targets <id> --dry-run --json".into(),
            ])
            .print_json()?;
        return Ok(());
    }

    println!("Review: {}\n", report.root);
    println!("{}\n", report.summary);
    println!("Recommendations:");
    for rec in &report.recommendations {
        println!(
            "  [{:>14}] {:.0}% — {}",
            rec.verdict, rec.confidence * 100.0, rec.path
        );
        println!("    {}", rec.reason);
    }

    Ok(())
}
