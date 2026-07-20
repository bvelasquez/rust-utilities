use std::collections::HashSet;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::agent::schema::ClassificationPlan;
use crate::apply_progress::{ApplyProgress, ApplySnapshot};
use crate::commands::CommandContext;
use crate::mail::imap::{self, ActionResult};
use crate::output::Envelope;
use crate::safety::confirm_mutation;
use crate::store::Store;

#[derive(Debug, Serialize)]
struct ApplyReport {
    plan_id: i64,
    results: Vec<imap::ActionResult>,
    applied: usize,
    failed: usize,
    dry_run: bool,
    plan_closed: bool,
    aborted: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApplySummary {
    pub plan_id: i64,
    pub applied: usize,
    pub failed: usize,
    pub results: Vec<imap::ActionResult>,
    /// True when every plan message was applied and the plan was closed.
    pub plan_closed: bool,
    /// Account-level abort (password/IMAP) after some work may already be done.
    pub aborted: Option<String>,
}

pub async fn run(
    ctx: &CommandContext,
    plan_id: Option<i64>,
    dry_run: bool,
    yes: bool,
    allow_delete: bool,
) -> Result<()> {
    let summary = execute_apply(ctx, plan_id, dry_run, yes, allow_delete, None).await?;

    if ctx.json {
        Envelope::ok(
            "apply",
            ApplyReport {
                plan_id: summary.plan_id,
                results: summary.results,
                applied: summary.applied,
                failed: summary.failed,
                dry_run,
                plan_closed: summary.plan_closed,
                aborted: summary.aborted.clone(),
            },
        )
        .print_json()?;
        return Ok(());
    }

    let verb = if dry_run { "Would apply" } else { "Applied" };
    print!(
        "{verb} plan #{} — {} ok, {} failed",
        summary.plan_id, summary.applied, summary.failed
    );
    if !dry_run {
        if summary.plan_closed {
            print!(" — plan closed");
        } else if summary.failed > 0 || summary.aborted.is_some() {
            print!(" — plan kept open for retry");
        }
    }
    if let Some(err) = &summary.aborted {
        print!(" (aborted: {err})");
    }
    println!();
    Ok(())
}

pub async fn execute_apply(
    ctx: &CommandContext,
    plan_id: Option<i64>,
    dry_run: bool,
    yes: bool,
    allow_delete: bool,
    on_progress: Option<&mut dyn FnMut(&ApplySnapshot)>,
) -> Result<ApplySummary> {
    execute_apply_scoped(
        ctx,
        plan_id,
        dry_run,
        yes,
        allow_delete,
        ApplyScope::All,
        on_progress,
    )
    .await
}

#[derive(Debug, Clone, Copy)]
pub enum ApplyScope {
    /// Apply every decision in the plan (manual `a` / CLI).
    All,
    /// AUTO: only high-confidence safe actions; leave deletes + unsure in the plan.
    AutoSafe { min_confidence: f32 },
}

pub async fn execute_apply_scoped(
    ctx: &CommandContext,
    plan_id: Option<i64>,
    dry_run: bool,
    yes: bool,
    allow_delete: bool,
    scope: ApplyScope,
    mut on_progress: Option<&mut dyn FnMut(&ApplySnapshot)>,
) -> Result<ApplySummary> {
    let mut emit = |progress: &ApplyProgress| {
        if let Some(cb) = on_progress.as_mut() {
            cb(&progress.snapshot());
        }
    };

    let store = Store::open(&ctx.app.db_path())?;
    let stored = if let Some(id) = plan_id {
        store
            .get_plan(id)?
            .with_context(|| format!("plan {id} not found"))?
    } else {
        store
            .latest_pending_plan()?
            .context("no pending plan — run `mail-sweep process` first")?
    };

    let plan: ClassificationPlan =
        serde_json::from_str(&stored.json_plan).context("parse stored plan")?;

    let scoped: Vec<_> = match scope {
        ApplyScope::All => plan.messages.clone(),
        ApplyScope::AutoSafe { min_confidence } => plan
            .messages
            .iter()
            .filter(|m| m.is_auto_applicable(min_confidence))
            .cloned()
            .collect(),
    };

    if scoped.is_empty() {
        return Ok(ApplySummary {
            plan_id: stored.id,
            applied: 0,
            failed: 0,
            results: vec![],
            plan_closed: false,
            aborted: None,
        });
    }

    let total = scoped.len();
    let mut progress = ApplyProgress::new(stored.id, total);
    emit(&progress);

    let has_delete = scoped.iter().any(|m| m.action.is_destructive());
    let allow_delete = allow_delete || ctx.app.config.safety.allow_delete;

    if has_delete && !allow_delete && !dry_run {
        bail!("plan contains delete actions — set safety.allow_delete or pass --allow-delete");
    }

    confirm_mutation(
        dry_run,
        yes,
        &format!(
            "Apply plan #{} ({} messages)? Type 'yes' to confirm: ",
            stored.id, total
        ),
    )?;

    progress.set_phase(if dry_run { "Dry run" } else { "Connecting" });
    emit(&progress);

    let mut all_results = Vec::new();
    let mut applied_keys: HashSet<(String, u32)> = HashSet::new();
    let mut aborted: Option<String> = None;

    for account in &ctx.app.config.accounts {
        let account_decisions: Vec<_> = scoped
            .iter()
            .filter(|m| m.account_id == account.id)
            .cloned()
            .collect();
        if account_decisions.is_empty() {
            continue;
        }

        let password = match ctx.app.resolve_password(account) {
            Ok(p) => p,
            Err(e) => {
                aborted = Some(format!("account {}: {e}", account.id));
                break;
            }
        };

        progress.set_account(&account.id);
        emit(&progress);

        let mut step = progress.current_step();
        let results = match imap::apply_decisions(
            account,
            &password,
            &account_decisions,
            allow_delete,
            dry_run,
            Some(&mut |_i, decision, result: &ActionResult| {
                step += 1;
                progress.on_message(
                    step,
                    result.action.as_str(),
                    decision.uid,
                    result.ok,
                    &result.detail,
                );
                emit(&progress);
            }),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                aborted = Some(format!("account {}: {e}", account.id));
                break;
            }
        };

        if !dry_run {
            let applied_uids: Vec<u32> = account_decisions
                .iter()
                .zip(results.iter())
                .filter(|(_, r)| r.ok)
                .map(|(d, _)| d.uid)
                .collect();
            for uid in &applied_uids {
                applied_keys.insert((account.id.clone(), *uid));
            }
            if !applied_uids.is_empty() {
                store.mark_messages_applied(&account.id, &applied_uids)?;
            }
        }
        all_results.extend(results);
    }

    let applied = if dry_run {
        all_results.iter().filter(|r| r.ok).count()
    } else {
        applied_keys.len()
    };
    let failed = total.saturating_sub(applied);

    // Finalize against the full plan so unscoped (delete / low-conf) rows stay queued.
    let plan_closed = if dry_run {
        false
    } else {
        store.finalize_apply(stored.id, &plan, &applied_keys)?
    };

    progress.finish();
    emit(&progress);

    Ok(ApplySummary {
        plan_id: stored.id,
        applied,
        failed,
        results: all_results,
        plan_closed,
        aborted,
    })
}
