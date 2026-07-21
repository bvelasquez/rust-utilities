use anyhow::Result;

use crate::apply_progress::ApplySnapshot;
use crate::commands::apply;
use crate::commands::CommandContext;
use crate::process;
use crate::sync;

pub async fn do_sync(ctx: &CommandContext) -> Result<String> {
    let report = sync::sync_all(ctx, None, false).await?;
    Ok(format!(
        "Synced {} new/stored ({} fetched) — UNSEEN refreshes re-open applied mail for Triage",
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
    Ok(format_apply_summary(&summary))
}

/// AUTO path: only high-confidence safe actions; deletes/unsure stay in the plan for Review.
pub async fn do_apply_auto(ctx: &CommandContext) -> Result<String> {
    let min = ctx.app.config.safety.auto_apply_min_confidence;
    let summary = apply::execute_apply_scoped(
        ctx,
        None,
        false,
        true,
        false,
        apply::ApplyScope::AutoSafe {
            min_confidence: min,
        },
        None,
    )
    .await?;
    if summary.applied == 0 && summary.failed == 0 && summary.aborted.is_none() {
        return Ok(format!(
            "no high-confidence safe actions (≥{:.0}%) — unsure/deletes wait in Review",
            min * 100.0
        ));
    }
    Ok(format_apply_summary(&summary))
}

pub async fn do_mark_read(ctx: &CommandContext, account_id: &str, uid: u32) -> Result<String> {
    let account = ctx.app.account_by_id(account_id)?;
    let password = ctx.app.resolve_password(account)?;
    let timeout = ctx.app.config.sync.imap_timeout_secs;
    crate::mail::imap::mark_seen(account, &password, uid, timeout).await?;
    let store = crate::store::Store::open(&ctx.app.db_path())?;
    store.mark_message_read(account_id, uid)?;
    Ok(format!("Marked read — {account_id} uid {uid}"))
}

fn format_apply_summary(summary: &apply::ApplySummary) -> String {
    let mut msg = format!(
        "Applied plan #{} — {} ok, {} failed",
        summary.plan_id, summary.applied, summary.failed
    );
    if summary.plan_closed {
        msg.push_str(" — plan closed");
    } else if summary.failed > 0 || summary.aborted.is_some() || summary.applied > 0 {
        msg.push_str(" — plan kept open for retry/Review");
    }
    if let Some(err) = &summary.aborted {
        msg.push_str(&format!(" ({err})"));
    }
    msg
}

