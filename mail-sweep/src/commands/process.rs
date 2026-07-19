use anyhow::Result;

use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::process;

pub async fn run(
    ctx: &mut CommandContext,
    account: Option<String>,
    batch_size: usize,
    dry_run: bool,
) -> Result<()> {
    let report = process::process_pending(ctx, account.as_deref(), batch_size, dry_run).await?;

    if ctx.json {
        Envelope::ok("process", report)
            .with_next_actions(vec![
                "mail-sweep apply --dry-run --json".into(),
                "mail-sweep apply --yes --json".into(),
            ])
            .print_json()?;
        return Ok(());
    }

    println!("{}", report.summary);
    println!(
        "Decisions: {} total ({} rules, {} feedback, {} msgs from {} sender patterns){}",
        report.total_decisions,
        report.rule_matched,
        report.feedback_matched,
        report.llm_classified,
        report.llm_patterns,
        if let Some(id) = report.plan_id {
            format!(" — plan #{id}")
        } else if dry_run {
            " — dry-run".into()
        } else {
            String::new()
        }
    );
    if report.pending_remaining > 0 {
        println!(
            "Pending remaining: {} — run process again for next sender batch",
            report.pending_remaining
        );
    }

    Ok(())
}
