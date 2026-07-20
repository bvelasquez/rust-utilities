use anyhow::Result;
use serde::Serialize;

use crate::agent::classify::classify_sender_groups;
use crate::agent::schema::{ClassificationPlan, MailAction, MessageDecision};
use crate::commands::CommandContext;
use crate::config::{save_config_file, RuleConfig, SafetyConfig};
use crate::process::grouping::{
    decision_from_hint, decision_from_teaching, expand_patterns, group_by_sender, hint_for_sender,
    sender_group_input,
};
use crate::rules::apply_rules_to_message;
use crate::rules::message_matches_pattern;
use crate::rules::patterns::{sender_pattern_from, subject_pattern_from};
use crate::store::{CachedMessage, Store};

pub mod grouping;

/// Split LLM decisions into plan-worthy vs category-only hints.
pub fn tier_llm_decisions(
    decisions: Vec<MessageDecision>,
    safety: &SafetyConfig,
) -> (Vec<MessageDecision>, Vec<MessageDecision>) {
    let plan_min = safety.plan_min_confidence.clamp(0.0, 1.0);
    let mut to_plan = Vec::new();
    let mut hints = Vec::new();
    for d in decisions {
        // Deletes always plan into Review; never soft-hint only.
        if d.action.is_destructive() || d.confidence >= plan_min {
            to_plan.push(d);
        } else {
            hints.push(d);
        }
    }
    (to_plan, hints)
}

/// Patterns durable enough to save as rules (never auto-save deletes).
pub fn pattern_eligible_for_rule_save(
    confidence: f32,
    action: &str,
    min_confidence: f32,
) -> bool {
    if MailAction::parse(action).is_destructive() {
        return false;
    }
    confidence >= min_confidence
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessReport {
    pub plan_id: Option<i64>,
    pub rule_matched: usize,
    pub feedback_matched: usize,
    pub llm_senders: usize,
    pub llm_patterns: usize,
    pub llm_classified: usize,
    pub total_decisions: usize,
    pub pending_remaining: i64,
    pub summary: String,
    pub dry_run: bool,
    pub decisions: Vec<MessageDecision>,
}

pub async fn process_pending(
    ctx: &mut CommandContext,
    account_filter: Option<&str>,
    batch_size: usize,
    dry_run: bool,
) -> Result<ProcessReport> {
    let store = Store::open(&ctx.app.db_path())?;
    let sender_batch_limit = if batch_size == 0 {
        ctx.app.config.sync.batch_size
    } else {
        batch_size
    };

    let pending = store.pending_messages(account_filter, 10_000)?;
    let rules = &ctx.app.config.rules;
    let hints = store.learning_hints()?;

    let mut rule_decisions = Vec::new();
    let mut needs_llm = Vec::new();

    for msg in pending {
        if let Some(decision) = apply_rules_to_message(&msg, rules) {
            rule_decisions.push(decision);
        } else {
            needs_llm.push(msg);
        }
    }

    let mut sender_batches = group_by_sender(needs_llm);
    let mut feedback_decisions = Vec::new();
    let mut llm_batches = Vec::new();

    for batch in sender_batches.drain(..) {
        if let Some(hint) = hint_for_sender(&hints, &batch.sender) {
            for msg in &batch.messages {
                feedback_decisions.push(decision_from_hint(msg, hint));
            }
        } else {
            llm_batches.push(batch);
        }
    }

    llm_batches.truncate(sender_batch_limit);
    let llm_sender_count = llm_batches.len();

    let mut llm_decisions = Vec::new();
    let mut llm_hints = Vec::new();
    let mut patterns_proposed = 0usize;
    let mut summary = String::new();
    let mut rules_saved = 0usize;

    if !llm_batches.is_empty() {
        let inputs: Vec<_> = llm_batches.iter().map(sender_group_input).collect();
        let plan = classify_sender_groups(&ctx.app, &inputs, &hints).await?;
        patterns_proposed = plan.patterns.len();
        summary = plan.summary.clone();
        let expanded = expand_patterns(&plan.patterns, &llm_batches);
        let (planned, soft) = tier_llm_decisions(expanded, &ctx.app.config.safety);
        llm_decisions = planned;
        llm_hints = soft;

        if !dry_run {
            rules_saved = maybe_save_patterns(ctx, &plan.patterns)?;
            if !llm_hints.is_empty() {
                store.apply_category_hints(&llm_hints)?;
            }
        }
    }

    if summary.is_empty() {
        summary = format!(
            "{} msgs via rules, {} via your feedback, {} senders → {} patterns ({} planned, {} soft hints)",
            rule_decisions.len(),
            feedback_decisions.len(),
            llm_sender_count,
            patterns_proposed,
            llm_decisions.len(),
            llm_hints.len()
        );
    } else if rules_saved > 0 || !llm_hints.is_empty() {
        summary = format!(
            "{summary} · saved {rules_saved} rules · {} soft category hints",
            llm_hints.len()
        );
    }

    let rule_count = rule_decisions.len();
    let feedback_count = feedback_decisions.len();
    let llm_msg_count = llm_decisions.len() + llm_hints.len();

    let mut all_decisions = rule_decisions;
    all_decisions.append(&mut feedback_decisions);
    all_decisions.append(&mut llm_decisions);

    let plan_id = if dry_run {
        None
    } else if !all_decisions.is_empty() {
        let plan = ClassificationPlan {
            messages: all_decisions.clone(),
            summary: summary.clone(),
        };
        store.apply_decisions(&all_decisions)?;
        Some(store.save_plan(&plan)?)
    } else {
        None
    };

    let pending_remaining = store.pending_count(account_filter)?;

    Ok(ProcessReport {
        plan_id,
        rule_matched: rule_count,
        feedback_matched: feedback_count,
        llm_senders: llm_sender_count,
        llm_patterns: patterns_proposed,
        llm_classified: llm_msg_count,
        total_decisions: all_decisions.len(),
        pending_remaining,
        summary,
        dry_run,
        decisions: all_decisions,
    })
}

/// Junk delete rule for entire sender (`from:…`).
pub fn teach_junk_sender(ctx: &mut CommandContext, sender: &str) -> Result<TeachReport> {
    ensure_delete_enabled(ctx)?;
    teach_pattern(
        ctx,
        &sender_pattern_from(sender),
        "delete",
        Some("spam"),
        1,
        None,
    )
}

/// Junk delete for selected message — subject pattern by default, or whole sender.
pub fn teach_junk_message(
    ctx: &mut CommandContext,
    msg: &CachedMessage,
    whole_sender: bool,
) -> Result<TeachReport> {
    ensure_delete_enabled(ctx)?;
    if whole_sender {
        teach_message_sender(ctx, msg, "delete", Some("spam"), 1)
    } else {
        teach_message_subject(ctx, msg, "delete", Some("spam"), 1)
    }
}

pub fn teach_message_subject(
    ctx: &mut CommandContext,
    msg: &CachedMessage,
    action: &str,
    category: Option<&str>,
    priority: u8,
) -> Result<TeachReport> {
    let pattern = subject_pattern_from(&msg.subject);
    teach_pattern(ctx, &pattern, action, category, priority, Some(msg))
}

pub fn teach_message_sender(
    ctx: &mut CommandContext,
    msg: &CachedMessage,
    action: &str,
    category: Option<&str>,
    priority: u8,
) -> Result<TeachReport> {
    let pattern = sender_pattern_from(&msg.from_address);
    teach_pattern(ctx, &pattern, action, category, priority, Some(msg))
}

/// Apply a match pattern rule and plan actions for all matching teachable mail.
pub fn teach_pattern(
    ctx: &mut CommandContext,
    match_pattern: &str,
    action: &str,
    category: Option<&str>,
    priority: u8,
    focus: Option<&CachedMessage>,
) -> Result<TeachReport> {
    if action == "delete" {
        ensure_delete_enabled(ctx)?;
    }

    let store = Store::open(&ctx.app.db_path())?;
    store.add_learning(match_pattern, action, category, priority, "tui")?;
    ensure_rule(ctx, match_pattern, action, category, Some(priority), None)?;

    let rules = ctx.app.config.rules.clone();
    let mut matching: Vec<CachedMessage> = store
        .teachable_messages()?
        .into_iter()
        .filter(|m| message_matches_pattern(match_pattern, m))
        .collect();

    if let Some(msg) = focus {
        if !matching.iter().any(|m| m.id == msg.id) {
            matching.push(msg.clone());
        }
        if let Ok(pending_same) = store.pending_from_sender(&msg.from_address) {
            for m in pending_same {
                if matching.iter().any(|x| x.id == m.id) {
                    continue;
                }
                if message_matches_pattern(match_pattern, &m)
                    || apply_rules_to_message(&m, &rules).is_some()
                {
                    matching.push(m);
                }
            }
        }
    }

    let reason = format!("you taught: {match_pattern} → {action}");
    let decisions: Vec<_> = matching
        .iter()
        .map(|msg| decision_from_teaching(msg, action, category, priority, &reason))
        .collect();
    let affected = decisions.len();

    if !decisions.is_empty() {
        store.apply_decisions(&decisions)?;
        let plan = ClassificationPlan {
            messages: decisions,
            summary: format!("Rule {match_pattern} → {affected} messages"),
        };
        store.save_plan(&plan)?;
    }

    let sender_pending_remaining = focus
        .and_then(|m| store.pending_count_for_sender(&m.from_address).ok())
        .filter(|&n| n > 0);

    Ok(TeachReport {
        pattern: match_pattern.into(),
        messages_affected: affected,
        sender_pending_remaining,
    })
}

/// Record user feedback for a sender (legacy — prefer `teach_message_sender`).
pub fn teach_sender(
    ctx: &mut CommandContext,
    sender: &str,
    action: &str,
    category: Option<&str>,
    priority: u8,
) -> Result<TeachReport> {
    teach_pattern(
        ctx,
        &sender_pattern_from(sender),
        action,
        category,
        priority,
        None,
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct TeachReport {
    pub pattern: String,
    pub messages_affected: usize,
    /// Pending messages still unclassified from the taught sender (subject rules may not cover all).
    pub sender_pending_remaining: Option<i64>,
}

fn ensure_delete_enabled(ctx: &mut CommandContext) -> Result<()> {
    if ctx.app.config.safety.allow_delete {
        return Ok(());
    }
    let mut config = ctx.app.config.clone();
    config.safety.allow_delete = true;
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;
    Ok(())
}

fn ensure_rule(
    ctx: &mut CommandContext,
    pattern: &str,
    action: &str,
    category: Option<&str>,
    priority: Option<u8>,
    target_folder: Option<&str>,
) -> Result<()> {
    let mut config = ctx.app.config.clone();
    if let Some(rule) = config.rules.iter_mut().find(|r| r.r#match == pattern) {
        rule.action = action.into();
        rule.category = category.map(|s| s.into());
        rule.priority = priority;
        rule.target_folder = target_folder.map(|s| s.into());
    } else {
        config.rules.push(RuleConfig {
            id: None,
            r#match: pattern.into(),
            category: category.map(|s| s.into()),
            action: action.into(),
            priority,
            target_folder: target_folder.map(|s| s.into()),
        });
    }
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;
    Ok(())
}

fn maybe_save_patterns(
    ctx: &mut CommandContext,
    patterns: &[crate::agent::schema::ClassificationPattern],
) -> Result<usize> {
    let min = ctx.app.config.safety.auto_apply_min_confidence;
    let mut config = ctx.app.config.clone();
    let mut saved = 0usize;

    for p in patterns {
        if !pattern_eligible_for_rule_save(p.confidence, &p.action, min) {
            continue;
        }
        if config.rules.iter().any(|r| r.r#match == p.match_pattern) {
            continue;
        }
        config.rules.push(RuleConfig {
            id: None,
            r#match: p.match_pattern.clone(),
            category: Some(p.category.clone()),
            action: p.action.clone(),
            priority: Some(p.priority),
            target_folder: p.target_folder.clone(),
        });
        saved += 1;
    }

    if saved > 0 {
        save_config_file(&ctx.app.config_path, &config)?;
        ctx.app.config = config;
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::schema::{MailAction, MessageCategory};

    fn decision(action: MailAction, confidence: f32) -> MessageDecision {
        MessageDecision {
            account_id: "a".into(),
            uid: 1,
            message_id: None,
            category: MessageCategory::Newsletter,
            priority: 2,
            action,
            target_folder: None,
            tags: vec![],
            confidence,
            reason: "test".into(),
        }
    }

    #[test]
    fn tiers_soft_hint_low_confidence_safe_actions() {
        let safety = SafetyConfig::default();
        let (plan, hints) = tier_llm_decisions(
            vec![
                decision(MailAction::Archive, 0.4),
                decision(MailAction::Archive, 0.7),
                decision(MailAction::Delete, 0.3),
            ],
            &safety,
        );
        assert_eq!(hints.len(), 1);
        assert_eq!(plan.len(), 2);
        assert!(plan.iter().any(|d| d.action.is_destructive()));
    }

    #[test]
    fn never_auto_saves_delete_patterns() {
        assert!(!pattern_eligible_for_rule_save(0.99, "delete", 0.88));
        assert!(pattern_eligible_for_rule_save(0.90, "archive", 0.88));
        assert!(!pattern_eligible_for_rule_save(0.80, "archive", 0.88));
    }

    #[test]
    fn auto_applicable_requires_safe_and_high_conf() {
        let d = decision(MailAction::Archive, 0.9);
        assert!(d.is_auto_applicable(0.88));
        let d = decision(MailAction::Delete, 0.99);
        assert!(!d.is_auto_applicable(0.88));
        let d = decision(MailAction::Flag, 0.7);
        assert!(!d.is_auto_applicable(0.88));
    }
}
