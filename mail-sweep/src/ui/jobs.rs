//! Background jobs so the TUI keeps polling keys during IMAP / AI work.
//!
//! Work runs on a dedicated thread with its own current-thread Tokio runtime so
//! `!Send` futures (rusqlite `Store`, apply callbacks) stay off the UI thread.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use crate::agent::pattern_suggest::{sender_detail_input, suggest_patterns};
use crate::agent::rules_audit::audit_rules;
use crate::agent::schema::RuleAuditPlan;
use crate::commands::CommandContext;
use crate::store::Store;

use super::actions;
use super::rule_overlays::SuggestItem;

const UNREAD_KEPT_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Sync,
    Classify,
    Apply,
    Auto,
    MarkRead,
    MarkAllRead,
    RulesAudit,
    PatternSuggest,
    PatternFromDesc,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Classify => "classify",
            Self::Apply => "apply",
            Self::Auto => "AUTO",
            Self::MarkRead => "mark read",
            Self::MarkAllRead => "mark all read",
            Self::RulesAudit => "rules audit",
            Self::PatternSuggest => "pattern suggest",
            Self::PatternFromDesc => "pattern generate",
        }
    }
}

pub enum JobEvent {
    /// Mid-job footer update (does not clear inflight).
    Progress(String),
    Done(JobResult),
}

pub enum JobResult {
    Sync {
        result: Result<String, String>,
        has_rules: bool,
    },
    Classify(Result<String, String>),
    Apply(Result<String, String>),
    Auto(Result<String, String>),
    MarkRead(Result<String, String>),
    MarkAllRead(Result<String, String>),
    RulesAudit(Result<RuleAuditPlan, String>),
    PatternSuggest(Result<(Vec<SuggestItem>, String), String>),
    PatternFromDesc(Result<String, String>),
}

pub struct JobRunner {
    tx: Sender<JobEvent>,
    rx: Receiver<JobEvent>,
    inflight: Option<JobKind>,
}

impl JobRunner {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            inflight: None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.inflight.is_some()
    }

    pub fn try_recv(&self) -> Option<JobEvent> {
        match self.rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    fn begin(&mut self, kind: JobKind) -> Option<Sender<JobEvent>> {
        if self.inflight.is_some() {
            return None;
        }
        self.inflight = Some(kind);
        Some(self.tx.clone())
    }

    pub fn clear_inflight(&mut self) {
        self.inflight = None;
    }

    pub fn busy_message(&self) -> String {
        match self.inflight {
            Some(k) => format!("Busy with {} — wait or press q to quit", k.label()),
            None => "Busy".into(),
        }
    }
}

fn err_str(e: impl ToString) -> String {
    e.to_string()
}

fn spawn_worker<F>(name: &str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let _ = thread::Builder::new().name(name.into()).spawn(f);
}

fn block_on_local<Fut>(fut: Fut) -> Fut::Output
where
    Fut: std::future::Future,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("mail-sweep job runtime");
    rt.block_on(fut)
}

pub fn spawn_sync(jobs: &mut JobRunner, ctx: &CommandContext) -> Result<(), String> {
    let tx = jobs.begin(JobKind::Sync).ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    let has_rules = !ctx.app.config.rules.is_empty();
    spawn_worker("mail-sweep-sync", move || {
        block_on_local(async move {
            let result = actions::do_sync(&job_ctx).await.map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::Sync { result, has_rules }));
        });
    });
    Ok(())
}

pub fn spawn_classify(jobs: &mut JobRunner, ctx: &CommandContext) -> Result<(), String> {
    let tx = jobs
        .begin(JobKind::Classify)
        .ok_or_else(|| jobs.busy_message())?;
    let mut job_ctx = ctx.clone();
    spawn_worker("mail-sweep-classify", move || {
        block_on_local(async move {
            let result = actions::do_classify(&mut job_ctx).await.map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::Classify(result)));
        });
    });
    Ok(())
}

pub fn spawn_apply(jobs: &mut JobRunner, ctx: &CommandContext) -> Result<(), String> {
    let tx = jobs.begin(JobKind::Apply).ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    spawn_worker("mail-sweep-apply", move || {
        block_on_local(async move {
            let result = actions::do_apply(&job_ctx, None).await.map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::Apply(result)));
        });
    });
    Ok(())
}

pub fn spawn_auto(jobs: &mut JobRunner, ctx: &CommandContext) -> Result<(), String> {
    let tx = jobs.begin(JobKind::Auto).ok_or_else(|| jobs.busy_message())?;
    let mut job_ctx = ctx.clone();
    spawn_worker("mail-sweep-auto", move || {
        block_on_local(async move {
            let _ = tx.send(JobEvent::Progress("AUTO sync".into()));
            let sync_result = actions::do_sync(&job_ctx).await;
            if let Err(e) = sync_result {
                let _ = tx.send(JobEvent::Done(JobResult::Auto(Err(format!(
                    "Auto sync failed: {e}"
                )))));
                return;
            }
            let sync_msg = sync_result.unwrap();

            let _ = tx.send(JobEvent::Progress("AUTO classify".into()));
            let classify_result = actions::do_classify(&mut job_ctx).await;
            if let Err(e) = classify_result {
                let _ = tx.send(JobEvent::Done(JobResult::Auto(Err(format!(
                    "Auto classify failed: {e}"
                )))));
                return;
            }
            let classify_msg = classify_result.unwrap();

            let apply_msg = match Store::open(&job_ctx.app.db_path()) {
                Ok(store) => match store.latest_pending_plan() {
                    Ok(None) => "no plan to apply".into(),
                    Ok(Some(_)) => {
                        let _ = tx.send(JobEvent::Progress("AUTO apply".into()));
                        match actions::do_apply_auto(&job_ctx).await {
                            Ok(s) => s,
                            Err(e) => {
                                let _ = tx.send(JobEvent::Done(JobResult::Auto(Err(format!(
                                    "Auto apply failed: {e}"
                                )))));
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(JobEvent::Done(JobResult::Auto(Err(format!(
                            "Auto apply failed: {e}"
                        )))));
                        return;
                    }
                },
                Err(e) => {
                    let _ = tx.send(JobEvent::Done(JobResult::Auto(Err(format!(
                        "Auto apply failed: {e}"
                    )))));
                    return;
                }
            };

            let _ = tx.send(JobEvent::Done(JobResult::Auto(Ok(format!(
                "AUTO: {sync_msg} · {classify_msg} · {apply_msg}"
            )))));
        });
    });
    Ok(())
}

pub fn spawn_mark_read(
    jobs: &mut JobRunner,
    ctx: &CommandContext,
    account_id: String,
    uid: u32,
) -> Result<(), String> {
    let tx = jobs
        .begin(JobKind::MarkRead)
        .ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    spawn_worker("mail-sweep-mark-read", move || {
        block_on_local(async move {
            let result = actions::do_mark_read(&job_ctx, &account_id, uid)
                .await
                .map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::MarkRead(result)));
        });
    });
    Ok(())
}

pub fn spawn_mark_all_read(jobs: &mut JobRunner, ctx: &CommandContext) -> Result<(), String> {
    let tx = jobs
        .begin(JobKind::MarkAllRead)
        .ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    spawn_worker("mail-sweep-mark-all", move || {
        block_on_local(async move {
            let result = actions::do_mark_all_read(&job_ctx, UNREAD_KEPT_LIMIT)
                .await
                .map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::MarkAllRead(result)));
        });
    });
    Ok(())
}

pub fn spawn_rules_audit(jobs: &mut JobRunner, ctx: &CommandContext) -> Result<(), String> {
    let tx = jobs
        .begin(JobKind::RulesAudit)
        .ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    spawn_worker("mail-sweep-audit", move || {
        block_on_local(async move {
            let result = async {
                let store = Store::open(&job_ctx.app.db_path())?;
                audit_rules(&job_ctx.app, &job_ctx.app.config.rules, &store).await
            }
            .await
            .map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::RulesAudit(result)));
        });
    });
    Ok(())
}

pub fn spawn_pattern_suggest(
    jobs: &mut JobRunner,
    ctx: &CommandContext,
    from: String,
    account_id: String,
) -> Result<(), String> {
    let tx = jobs
        .begin(JobKind::PatternSuggest)
        .ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    spawn_worker("mail-sweep-suggest", move || {
        block_on_local(async move {
            let result = async {
                let store = Store::open(&job_ctx.app.db_path())?;
                let messages = store.messages_for_sender(&from, Some(&account_id), 50)?;
                let detail = sender_detail_input(&from, &messages);
                let plan = suggest_patterns(&job_ctx.app, &detail).await?;
                let items: Vec<SuggestItem> = plan
                    .patterns
                    .into_iter()
                    .map(|p| {
                        let match_count = store
                            .messages_matching_pattern(&p.match_pattern, 5000, usize::MAX)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        SuggestItem {
                            pattern: p,
                            match_count,
                        }
                    })
                    .collect();
                Ok::<_, anyhow::Error>((items, plan.summary))
            }
            .await
            .map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::PatternSuggest(result)));
        });
    });
    Ok(())
}

pub fn spawn_pattern_from_desc(
    jobs: &mut JobRunner,
    ctx: &CommandContext,
    description: String,
    current: String,
) -> Result<(), String> {
    let tx = jobs
        .begin(JobKind::PatternFromDesc)
        .ok_or_else(|| jobs.busy_message())?;
    let job_ctx = ctx.clone();
    spawn_worker("mail-sweep-pattern", move || {
        block_on_local(async move {
            let result = crate::agent::pattern_from_desc::pattern_from_description(
                &job_ctx.app,
                &description,
                if current.trim().is_empty() {
                    None
                } else {
                    Some(current.as_str())
                },
            )
            .await
            .map_err(err_str);
            let _ = tx.send(JobEvent::Done(JobResult::PatternFromDesc(result)));
        });
    });
    Ok(())
}
