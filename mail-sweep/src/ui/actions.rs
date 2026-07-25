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

/// Mark every leftover unread keep/flag message as read (all accounts).
pub async fn do_mark_all_read(ctx: &CommandContext, limit: usize) -> Result<String> {
    use std::collections::BTreeMap;

    let store = crate::store::Store::open(&ctx.app.db_path())?;
    let leftovers = store.unread_kept_messages(limit)?;
    if leftovers.is_empty() {
        return Ok("No unread keep/flag mail to mark".into());
    }

    let mut by_account: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut items: Vec<(String, u32)> = Vec::with_capacity(leftovers.len());
    for m in &leftovers {
        by_account
            .entry(m.account_id.clone())
            .or_default()
            .push(m.uid);
        items.push((m.account_id.clone(), m.uid));
    }

    let timeout = ctx.app.config.sync.imap_timeout_secs;
    for (account_id, uids) in &by_account {
        let account = ctx.app.account_by_id(account_id)?;
        let password = ctx.app.resolve_password(account)?;
        crate::mail::imap::mark_seen_uids(account, &password, uids, timeout).await?;
    }

    let marked = store.mark_messages_read(&items)?;
    let accounts = by_account.len();
    Ok(format!(
        "Marked read — {marked} message{} across {accounts} account{}",
        if marked == 1 { "" } else { "s" },
        if accounts == 1 { "" } else { "s" }
    ))
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

