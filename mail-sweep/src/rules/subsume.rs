//! Find rules made redundant by a broader keeper pattern (same action).

use crate::config::RuleConfig;
use crate::rules::message_matches_pattern;
use crate::rules::patterns::{validate_pattern, PatternKind};
use crate::store::{CachedMessage, Store};

#[derive(Debug, Clone)]
pub struct SubsumedRule {
    pub index: usize,
    pub r#match: String,
    pub action: String,
    pub reason: String,
}

/// Rules (other than `keeper_index`) with the same action that the keeper pattern covers.
pub fn find_subsumed_rules(
    rules: &[RuleConfig],
    keeper_index: usize,
    store: &Store,
) -> Vec<SubsumedRule> {
    let Some(keeper) = rules.get(keeper_index) else {
        return vec![];
    };
    let keeper_pat = keeper.r#match.as_str();
    if validate_pattern(keeper_pat).error.is_some() {
        return vec![];
    }

    let mut out = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        if i == keeper_index {
            continue;
        }
        if rule.action != keeper.action {
            continue;
        }
        if rule.r#match == keeper.r#match {
            out.push(SubsumedRule {
                index: i,
                r#match: rule.r#match.clone(),
                action: rule.action.clone(),
                reason: "exact duplicate pattern".into(),
            });
            continue;
        }
        if let Some(reason) = coverage_reason(keeper_pat, &rule.r#match, store) {
            out.push(SubsumedRule {
                index: i,
                r#match: rule.r#match.clone(),
                action: rule.action.clone(),
                reason,
            });
        }
    }
    out
}

fn coverage_reason(keeper: &str, other: &str, store: &Store) -> Option<String> {
    if pattern_covers_pattern(keeper, other) {
        return Some("pattern covered by broader rule".into());
    }
    if empirically_covered(keeper, other, store) {
        return Some("all cached matches also hit broader rule".into());
    }
    None
}

/// True when every message that would match `other` also matches `keeper`,
/// judged by treating `other`'s subject/from/domain needle as a synthetic message,
/// and/or by cache evidence.
fn pattern_covers_pattern(keeper: &str, other: &str) -> bool {
    let keeper_v = validate_pattern(keeper);
    let other_v = validate_pattern(other);
    if keeper_v.error.is_some() || other_v.error.is_some() {
        return false;
    }

    // Fast path: broader subject/from/domain needle contained in narrower (case-insensitive),
    // when the keeper side is a simple literal (not a regex metachar pattern).
    if let (PatternKind::Subject(k), PatternKind::Subject(o)) = (&keeper_v.kind, &other_v.kind) {
        if is_simple_literal(k) && o.to_lowercase().contains(&k.to_lowercase()) {
            return true;
        }
    }
    if let (PatternKind::From(k), PatternKind::From(o)) = (&keeper_v.kind, &other_v.kind) {
        if is_simple_literal(k) && o.eq_ignore_ascii_case(k) {
            return true;
        }
    }
    if let (PatternKind::Domain(k), PatternKind::From(o)) = (&keeper_v.kind, &other_v.kind) {
        let domain = o.split('@').nth(1).unwrap_or("");
        if is_simple_literal(k)
            && (domain.eq_ignore_ascii_case(k)
                || domain.to_lowercase().ends_with(&format!(".{}", k.to_lowercase())))
        {
            return true;
        }
    }

    match &other_v.kind {
        PatternKind::Subject(subj) => {
            let msg = synthetic(subj, "probe@example.com");
            message_matches_pattern(keeper, &msg)
        }
        PatternKind::From(addr) => {
            let msg = synthetic("probe subject", addr);
            message_matches_pattern(keeper, &msg)
        }
        PatternKind::Domain(domain) => {
            let msg = synthetic("probe subject", &format!("user@{domain}"));
            message_matches_pattern(keeper, &msg)
        }
        PatternKind::Bare(s) => {
            let msg = synthetic(s, "probe@example.com");
            message_matches_pattern(keeper, &msg)
        }
        PatternKind::All(parts) => parts.iter().all(|p| pattern_covers_pattern(keeper, p)),
        PatternKind::Body(_) | PatternKind::Header { .. } | PatternKind::HasListUnsubscribe => {
            false
        }
    }
}

fn is_simple_literal(s: &str) -> bool {
    !s.chars()
        .any(|c| matches!(c, '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '|' | '\\' | '^' | '$'))
}

fn empirically_covered(keeper: &str, other: &str, store: &Store) -> bool {
    let Ok(matches) = store.messages_matching_pattern(other, 5000, 50) else {
        return false;
    };
    if matches.is_empty() {
        return false;
    }
    matches
        .iter()
        .all(|m| message_matches_pattern(keeper, m))
}

fn synthetic(subject: &str, from: &str) -> CachedMessage {
    CachedMessage {
        id: 0,
        account_id: "probe".into(),
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
        list_unsubscribe: None,
        raw_headers_json: None,
        planned_action: None,
        plan_confidence: None,
        plan_reason: None,
    }
}

/// Remove rules at the given config indices (descending order).
pub fn remove_rules_at(rules: &mut Vec<RuleConfig>, indices: &[usize]) {
    let mut sorted: Vec<usize> = indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    for i in sorted.into_iter().rev() {
        if i < rules.len() {
            rules.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rule(m: &str, action: &str) -> RuleConfig {
        RuleConfig {
            id: None,
            r#match: m.into(),
            category: Some("newsletter".into()),
            action: action.into(),
            priority: Some(2),
            target_folder: None,
        }
    }

    #[test]
    fn broader_subject_covers_narrower() {
        let rules = vec![
            rule("subject:Invoice", "archive"),
            rule("subject:Your Apple Ads invoice.", "archive"),
            rule("subject:Amazon Web Services Invoice Available", "archive"),
            rule("subject:Invoice", "flag"), // different action — keep
        ];
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("t.db")).unwrap();
        let covered = find_subsumed_rules(&rules, 0, &store);
        let idxs: Vec<_> = covered.iter().map(|c| c.index).collect();
        assert!(idxs.contains(&1));
        assert!(idxs.contains(&2));
        assert!(!idxs.contains(&3));
    }

    #[test]
    fn domain_covers_from() {
        let rules = vec![
            rule("domain:stripe.com", "flag"),
            rule("from:notifications@stripe.com", "flag"),
            rule("from:updates@e.stripe.com", "delete"),
        ];
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("t.db")).unwrap();
        let covered = find_subsumed_rules(&rules, 0, &store);
        assert_eq!(covered.len(), 1);
        assert_eq!(covered[0].index, 1);
    }

    #[test]
    fn remove_rules_at_descending() {
        let mut rules = vec![
            rule("a", "keep"),
            rule("b", "keep"),
            rule("c", "keep"),
            rule("d", "keep"),
        ];
        remove_rules_at(&mut rules, &[1, 3]);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].r#match, "a");
        assert_eq!(rules[1].r#match, "c");
    }
}
