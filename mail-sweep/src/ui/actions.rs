use anyhow::Result;

use crate::apply_progress::ApplySnapshot;
use crate::commands::apply;
use crate::commands::CommandContext;
use crate::process;
use crate::sync;

pub async fn do_sync(ctx: &CommandContext) -> Result<String> {
    let report = sync::sync_all(ctx, None, false).await?;
    Ok(format!(
        "Synced {} new/stored ({} fetched)",
        report.total_stored, report.total_fetched
    ))
}

pub async fn do_classify(ctx: &mut CommandContext) -> Result<String> {
    let report = process::process_pending(ctx, None, 0, false).await?;
    Ok(format!(
        "AI: {} patterns → {} msgs classified ({} senders left) — {}",
        report.llm_patterns,
        report.llm_classified,
        report.pending_remaining,
        report.summary
    ))
}

pub async fn do_apply(
    ctx: &CommandContext,
    on_progress: Option<&mut dyn FnMut(&ApplySnapshot)>,
) -> Result<String> {
    let summary = apply::execute_apply(
        ctx,
        None,
        false,
        true,
        ctx.app.config.safety.allow_delete,
        on_progress,
    )
    .await?;
    let mut msg = format!(
        "Applied plan #{} — {} ok, {} failed",
        summary.plan_id, summary.applied, summary.failed
    );
    if summary.plan_closed {
        msg.push_str(" — plan closed");
    } else if summary.failed > 0 || summary.aborted.is_some() {
        msg.push_str(" — plan kept open for retry");
    }
    if let Some(err) = &summary.aborted {
        msg.push_str(&format!(" ({err})"));
    }
    Ok(msg)
}

