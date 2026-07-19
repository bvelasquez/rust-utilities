use anyhow::Result;

use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::sync;

pub async fn run(ctx: &CommandContext, account: Option<String>, full: bool) -> Result<()> {
    let report = sync::sync_all(ctx, account.as_deref(), full).await?;

    if ctx.json {
        Envelope::ok("sync", report)
            .with_next_actions(vec![
                "mail-sweep list --json".into(),
                "mail-sweep process --dry-run --json".into(),
            ])
            .print_json()?;
        return Ok(());
    }

    println!(
        "Synced {} messages ({} stored) across {} accounts",
        report.total_fetched,
        report.total_stored,
        report.accounts.len()
    );
    for a in &report.accounts {
        if let Some(err) = &a.error {
            eprintln!("  {}: error — {err}", a.account_id);
        } else {
            println!(
                "  {}: fetched {} stored {} (last uid {})",
                a.account_id, a.fetched, a.stored, a.last_uid
            );
        }
    }

    Ok(())
}
