use crate::agent::schema::{MailAction, MessageCategory, MessageDecision};
use crate::config::RuleConfig;
use crate::mail::parser::ParsedMail;
use crate::rules::patterns::{header_value, pattern_specificity, regex_match};
use crate::store::CachedMessage;

pub mod patterns;
pub mod subsume;

pub use subsume::{find_subsumed_rules, remove_rules_at, SubsumedRule};

pub fn apply_rules_to_message(
    msg: &CachedMessage,
    rules: &[RuleConfig],
) -> Option<MessageDecision> {
    let mut ordered: Vec<&RuleConfig> = rules.iter().collect();
    ordered.sort_by_key(|r| pattern_specificity(&r.r#match));

    for rule in ordered {
        if rule_matches(msg, rule) {
            return Some(MessageDecision {
                account_id: msg.account_id.clone(),
                uid: msg.uid,
                message_id: Some(msg.id),
                category: rule
                    .category
                    .as_deref()
                    .map(MessageCategory::parse)
                    .unwrap_or(MessageCategory::Unknown),
                priority: rule.priority.unwrap_or(3),
                action: MailAction::parse(&rule.action),
                target_folder: rule.target_folder.clone(),
                tags: vec![],
                confidence: 1.0,
                reason: format!("matched rule: {}", rule.r#match),
            });
        }
    }
    builtin_rule(msg)
}

fn rule_matches(msg: &CachedMessage, rule: &RuleConfig) -> bool {
    message_matches_pattern(&rule.r#match, msg)
}

/// Match a rule-style pattern against a cached message.
pub fn message_matches_pattern(pattern: &str, msg: &CachedMessage) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }

    if pattern == "has:list-unsubscribe" {
        return msg.list_unsubscribe.as_ref().is_some_and(|s| !s.is_empty());
    }

    if let Some(rest) = pattern.strip_prefix("all:") {
        return rest
            .split('+')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .all(|part| message_matches_pattern(part, msg));
    }

    if let Some(rest) = pattern.strip_prefix("from:") {
        return regex_match(rest, &msg.from_address);
    }
    if let Some(rest) = pattern.strip_prefix("subject:") {
        return regex_match(rest, &msg.subject);
    }
    if let Some(rest) = pattern.strip_prefix("domain:") {
        let domain = msg.from_address.split('@').nth(1).unwrap_or("");
        return regex_match(rest, domain);
    }
    if let Some(rest) = pattern.strip_prefix("body:") {
        return regex_match(rest, &msg.body_preview);
    }
    if let Some(rest) = pattern.strip_prefix("header:") {
        let (name, value_pat) = if let Some((name, value)) = rest.split_once(':') {
            (name, Some(value))
        } else {
            (rest, None)
        };
        let header_val = header_value(msg, name);
        return match value_pat {
            None => header_val.is_some_and(|v| !v.is_empty()),
            Some(pat) => regex_match(pat, &header_val.unwrap_or_default()),
        };
    }

    regex_match(pattern, &format!("{} {}", msg.from_address, msg.subject))
}

pub fn builtin_rule(msg: &CachedMessage) -> Option<MessageDecision> {
    if let Some(ref lu) = msg.list_unsubscribe {
        if !lu.is_empty() {
            return Some(decision(
                msg,
                MessageCategory::Newsletter,
                2,
                MailAction::Archive,
                0.9,
                "List-Unsubscribe header present",
            ));
        }
    }

    if msg.from_address.contains("noreply")
        || msg.from_address.contains("no-reply")
        || msg.from_address.contains("donotreply")
    {
        return Some(decision(
            msg,
            MessageCategory::Notification,
            2,
            MailAction::Archive,
            0.85,
            "noreply sender address",
        ));
    }

    None
}

pub fn builtin_rule_parsed(parsed: &ParsedMail) -> Option<(MessageCategory, u8, MailAction, f32, String)> {
    if parsed.list_unsubscribe.as_ref().is_some_and(|s| !s.is_empty()) {
        return Some((
            MessageCategory::Newsletter,
            2,
            MailAction::Archive,
            0.9,
            "List-Unsubscribe header present".into(),
        ));
    }
    if let Some(ref prec) = parsed.precedence {
        if prec.to_lowercase().contains("bulk") || prec.to_lowercase().contains("list") {
            return Some((
                MessageCategory::Newsletter,
                2,
                MailAction::Archive,
                0.88,
                "Precedence bulk/list".into(),
            ));
        }
    }
    if let Some(ref auto) = parsed.auto_submitted {
        if auto.to_lowercase() != "no" {
            return Some((
                MessageCategory::Notification,
                2,
                MailAction::Archive,
                0.85,
                "Auto-Submitted header".into(),
            ));
        }
    }
    None
}

fn decision(
    msg: &CachedMessage,
    category: MessageCategory,
    priority: u8,
    action: MailAction,
    confidence: f32,
    reason: &str,
) -> MessageDecision {
    MessageDecision {
        account_id: msg.account_id.clone(),
        uid: msg.uid,
        message_id: Some(msg.id),
        category,
        priority,
        action,
        target_folder: None,
        tags: vec![],
        confidence,
        reason: reason.into(),
    }
}

pub fn test_rule(from: &str, subject: &str, headers: &str, rule: &RuleConfig) -> bool {
    let msg = CachedMessage {
        id: 0,
        account_id: "test".into(),
        uid: 0,
        message_id: None,
        from_address: from.into(),
        from_name: None,
        subject: subject.into(),
        date: None,
        category: "unknown".into(),
        priority: 3,
        status: "pending".into(),
        is_unread: true,
        is_flagged: false,
        body_preview: String::new(),
        body_text: None,
        list_unsubscribe: headers
            .lines()
            .find(|l| l.to_lowercase().starts_with("list-unsubscribe:"))
            .map(|l| l.split_once(':').map(|(_, v)| v.trim().into()).unwrap_or_default()),
        raw_headers_json: None,
        planned_action: None,
        plan_confidence: None,
        plan_reason: None,
    };
    rule_matches(&msg, rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuleConfig;

    fn sample() -> CachedMessage {
        CachedMessage {
            id: 1,
            account_id: "test".into(),
            uid: 1,
            message_id: None,
            from_address: "orders@amazon.com".into(),
            from_name: None,
            subject: "Your order has shipped".into(),
            date: None,
            category: "unknown".into(),
            priority: 3,
            status: "pending".into(),
            is_unread: true,
            is_flagged: false,
            body_preview: "track your package".into(),
            body_text: None,
            list_unsubscribe: None,
            raw_headers_json: Some(r#"[["List-Id","store.list"]]"#.into()),
            planned_action: None,
            plan_confidence: None,
            plan_reason: None,
        }
    }

    #[test]
    fn compound_and_header() {
        let m = sample();
        assert!(message_matches_pattern(
            "all:domain:amazon.com+subject:Your order",
            &m
        ));
        assert!(message_matches_pattern("header:List-Id", &m));
        assert!(message_matches_pattern("body:package", &m));
        assert!(!message_matches_pattern("has:list-unsubscribe", &m));
    }

    #[test]
    fn subject_regex() {
        let m = sample();
        assert!(message_matches_pattern(r"subject:^Your order", &m));
    }

    #[test]
    fn rule_order_specificity() {
        let rules = vec![
            RuleConfig {
                id: None,
                r#match: "domain:amazon.com".into(),
                category: None,
                action: "keep".into(),
                priority: None,
                target_folder: None,
            },
            RuleConfig {
                id: None,
                r#match: "subject:Your order".into(),
                category: None,
                action: "archive".into(),
                priority: None,
                target_folder: None,
            },
        ];
        let decision = apply_rules_to_message(&sample(), &rules).unwrap();
        assert_eq!(decision.action, MailAction::Archive);
    }
}
