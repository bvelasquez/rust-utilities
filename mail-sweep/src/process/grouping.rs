use std::collections::HashMap;

use crate::agent::schema::{
    ClassificationPattern, MailAction, MessageCategory, MessageDecision, SenderGroupInput,
};
use crate::rules::message_matches_pattern;
use crate::store::{CachedMessage, LearningHint};

pub struct SenderBatch {
    pub sender: String,
    pub messages: Vec<CachedMessage>,
}

pub fn group_by_sender(messages: Vec<CachedMessage>) -> Vec<SenderBatch> {
    let mut map: HashMap<String, Vec<CachedMessage>> = HashMap::new();
    for msg in messages {
        map.entry(msg.from_address.clone())
            .or_default()
            .push(msg);
    }
    let mut batches: Vec<SenderBatch> = map
        .into_iter()
        .map(|(sender, messages)| SenderBatch { sender, messages })
        .collect();
    batches.sort_by_key(|b| std::cmp::Reverse(b.messages.len()));
    batches
}

pub fn sender_group_input(batch: &SenderBatch) -> SenderGroupInput {
    let domain = batch
        .sender
        .split('@')
        .nth(1)
        .unwrap_or("")
        .to_string();
    let sample_subjects: Vec<String> = batch
        .messages
        .iter()
        .take(3)
        .map(|m| m.subject.clone())
        .collect();
    let sample_snippet = batch
        .messages
        .first()
        .map(|m| m.body_preview.clone())
        .unwrap_or_default();
    let has_list_unsubscribe = batch
        .messages
        .iter()
        .any(|m| m.list_unsubscribe.as_ref().is_some_and(|s| !s.is_empty()));

    SenderGroupInput {
        sender: batch.sender.clone(),
        domain,
        message_count: batch.messages.len(),
        sample_subjects,
        sample_snippet,
        has_list_unsubscribe,
    }
}

pub fn hint_for_sender<'a>(hints: &'a [LearningHint], sender: &str) -> Option<&'a LearningHint> {
    hints
        .iter()
        .find(|h| sender == h.sender || sender.contains(&h.sender) || h.sender.contains(sender))
}

pub fn decision_from_hint(msg: &CachedMessage, hint: &LearningHint) -> MessageDecision {
    MessageDecision {
        account_id: msg.account_id.clone(),
        uid: msg.uid,
        message_id: Some(msg.id),
        category: hint
            .category
            .as_deref()
            .map(MessageCategory::parse)
            .unwrap_or(MessageCategory::Unknown),
        priority: hint.priority,
        action: MailAction::parse(&hint.action),
        target_folder: None,
        tags: vec![],
        confidence: 1.0,
        reason: format!("user feedback for {}", hint.sender),
    }
}

pub fn expand_patterns(
    patterns: &[ClassificationPattern],
    batches: &[SenderBatch],
) -> Vec<MessageDecision> {
    let mut decisions = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for pattern in patterns {
        for batch in batches {
            for msg in &batch.messages {
                let key = (msg.account_id.clone(), msg.uid);
                if seen.contains(&key) {
                    continue;
                }
                if message_matches_pattern(&pattern.match_pattern, msg) {
                    seen.insert(key);
                    decisions.push(MessageDecision {
                        account_id: msg.account_id.clone(),
                        uid: msg.uid,
                        message_id: Some(msg.id),
                        category: MessageCategory::parse(&pattern.category),
                        priority: pattern.priority.clamp(1, 5),
                        action: MailAction::parse(&pattern.action),
                        target_folder: if pattern.action.eq_ignore_ascii_case("archive") {
                            None
                        } else {
                            pattern.target_folder.clone()
                        },
                        tags: vec![],
                        confidence: pattern.confidence.clamp(0.0, 1.0),
                        reason: pattern.reason.clone(),
                    });
                }
            }
        }
    }
    decisions
}

pub fn decision_from_teaching(
    msg: &CachedMessage,
    action: &str,
    category: Option<&str>,
    priority: u8,
    reason: &str,
) -> MessageDecision {
    MessageDecision {
        account_id: msg.account_id.clone(),
        uid: msg.uid,
        message_id: Some(msg.id),
        category: category
            .map(MessageCategory::parse)
            .unwrap_or(MessageCategory::Unknown),
        priority,
        action: MailAction::parse(action),
        target_folder: None,
        tags: vec![],
        confidence: 1.0,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_msg(sender: &str, uid: u32) -> CachedMessage {
        CachedMessage {
            id: uid as i64,
            account_id: "personal".into(),
            uid,
            message_id: None,
            from_address: sender.into(),
            from_name: None,
            subject: "Weekly deals".into(),
            date: None,
            category: "unknown".into(),
            priority: 3,
            status: "pending".into(),
            is_unread: true,
            is_flagged: false,
            body_preview: "snippet".into(),
            body_text: None,
            list_unsubscribe: None,
            raw_headers_json: None,
            planned_action: None,
            plan_confidence: None,
            plan_reason: None,
        }
    }

    #[test]
    fn groups_by_sender() {
        let msgs = vec![
            sample_msg("a@x.com", 1),
            sample_msg("a@x.com", 2),
            sample_msg("b@y.com", 3),
        ];
        let groups = group_by_sender(msgs);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].messages.len(), 2);
    }

    #[test]
    fn expands_from_pattern_to_all_sender_messages() {
        let batches = group_by_sender(vec![
            sample_msg("shop@store.com", 1),
            sample_msg("shop@store.com", 2),
        ]);
        let patterns = vec![ClassificationPattern {
            match_pattern: "from:shop@store.com".into(),
            category: "marketing".into(),
            priority: 2,
            action: "archive".into(),
            target_folder: None,
            confidence: 0.95,
            reason: "retail promos".into(),
        }];
        let decisions = expand_patterns(&patterns, &batches);
        assert_eq!(decisions.len(), 2);
    }
}
