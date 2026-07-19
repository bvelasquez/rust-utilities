//! Helpers for building, validating, and ranking rule match patterns.

use crate::store::{CachedMessage, Store};

/// Build a `subject:…` pattern from a message subject (strips Re:/Fwd:, literal match).
pub fn subject_pattern_from(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        if s.len() >= 4 && s[..4].eq_ignore_ascii_case("re: ") {
            s = s[4..].trim();
        } else if s.len() >= 5 && s[..5].eq_ignore_ascii_case("fwd: ") {
            s = s[5..].trim();
        } else if s.len() >= 4 && s[..4].eq_ignore_ascii_case("fw: ") {
            s = s[4..].trim();
        } else {
            break;
        }
    }
    let needle: String = s.chars().take(56).collect();
    if needle.is_empty() {
        "subject:(no subject)".into()
    } else {
        format!("subject:{needle}")
    }
}

pub fn sender_pattern_from(address: &str) -> String {
    format!("from:{address}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternKind {
    All(Vec<String>),
    Subject(String),
    From(String),
    Domain(String),
    Header { name: String, value: Option<String> },
    Body(String),
    HasListUnsubscribe,
    Bare(String),
}

#[derive(Debug, Clone)]
pub struct PatternValidation {
    pub kind: PatternKind,
    pub uses_regex: bool,
    pub error: Option<String>,
}

/// Classify a pattern string and validate regex segments where applicable.
pub fn validate_pattern(pattern: &str) -> PatternValidation {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return PatternValidation {
            kind: PatternKind::Bare(String::new()),
            uses_regex: false,
            error: Some("pattern is empty".into()),
        };
    }

    if pattern == "has:list-unsubscribe" {
        return PatternValidation {
            kind: PatternKind::HasListUnsubscribe,
            uses_regex: false,
            error: None,
        };
    }

    if let Some(rest) = pattern.strip_prefix("all:") {
        let parts: Vec<String> = rest.split('+').map(|p| p.trim().to_string()).collect();
        if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
            return PatternValidation {
                kind: PatternKind::All(parts),
                uses_regex: false,
                error: Some("all: needs at least one non-empty sub-pattern joined with +".into()),
            };
        }
        for part in &parts {
            let sub = validate_pattern(part);
            if sub.error.is_some() {
                return PatternValidation {
                    kind: PatternKind::All(parts.clone()),
                    uses_regex: parts.iter().any(|p| segment_uses_regex(p)),
                    error: sub.error,
                };
            }
        }
        return PatternValidation {
            kind: PatternKind::All(parts.clone()),
            uses_regex: parts.iter().any(|p| segment_uses_regex(p)),
            error: None,
        };
    }

    if let Some(rest) = pattern.strip_prefix("subject:") {
        return segment_validation(PatternKind::Subject(rest.to_string()), rest);
    }
    if let Some(rest) = pattern.strip_prefix("from:") {
        return segment_validation(PatternKind::From(rest.to_string()), rest);
    }
    if let Some(rest) = pattern.strip_prefix("domain:") {
        return segment_validation(PatternKind::Domain(rest.to_string()), rest);
    }
    if let Some(rest) = pattern.strip_prefix("body:") {
        return segment_validation(PatternKind::Body(rest.to_string()), rest);
    }
    if let Some(rest) = pattern.strip_prefix("header:") {
        let (name, value) = if let Some((name, value)) = rest.split_once(':') {
            (name.to_string(), Some(value.to_string()))
        } else {
            (rest.to_string(), None)
        };
        if name.is_empty() {
            return PatternValidation {
                kind: PatternKind::Header {
                    name,
                    value: value.clone(),
                },
                uses_regex: false,
                error: Some("header: needs a header name".into()),
            };
        }
        if let Some(ref v) = value {
            return segment_validation(
                PatternKind::Header {
                    name: name.clone(),
                    value: value.clone(),
                },
                v,
            );
        }
        return PatternValidation {
            kind: PatternKind::Header { name, value: None },
            uses_regex: false,
            error: None,
        };
    }

    segment_validation(PatternKind::Bare(pattern.to_string()), pattern)
}

fn segment_validation(kind: PatternKind, segment: &str) -> PatternValidation {
    if segment.is_empty() {
        return PatternValidation {
            kind,
            uses_regex: false,
            error: Some("pattern segment is empty".into()),
        };
    }
    let uses_regex = regex::Regex::new(segment).is_ok();
    PatternValidation {
        kind,
        uses_regex,
        error: None,
    }
}

fn segment_uses_regex(segment: &str) -> bool {
    let v = validate_pattern(segment);
    v.uses_regex
}

/// Lower = more specific = evaluated first.
pub fn pattern_specificity(pattern: &str) -> u8 {
    if pattern.starts_with("all:") {
        0
    } else if pattern.starts_with("subject:") {
        1
    } else if pattern.starts_with("header:")
        || pattern.starts_with("body:")
        || pattern == "has:list-unsubscribe"
    {
        2
    } else if pattern.starts_with("from:") {
        3
    } else if pattern.starts_with("domain:") {
        4
    } else {
        5
    }
}

pub fn regex_match(pattern: &str, haystack: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(haystack))
        .unwrap_or_else(|_| haystack.contains(pattern))
}

pub fn header_value(msg: &CachedMessage, name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if lower == "list-unsubscribe" {
        return msg.list_unsubscribe.clone();
    }
    let raw = msg.raw_headers_json.as_ref()?;
    let pairs: Vec<(String, String)> = serde_json::from_str(raw).ok()?;
    pairs
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// Count cached messages matching a pattern (scans up to `scan_limit`).
pub fn pattern_match_preview(store: &Store, pattern: &str, scan_limit: usize) -> usize {
    store
        .messages_matching_pattern(pattern, scan_limit, usize::MAX)
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Human-readable validation status for the pattern editor.
pub fn validation_status(pattern: &str) -> &'static str {
    let v = validate_pattern(pattern);
    if let Some(err) = v.error {
        return match err.as_str() {
            "pattern is empty" => "enter a pattern",
            _ => "invalid pattern",
        };
    }
    if v.uses_regex {
        "valid regex"
    } else {
        "literal substring"
    }
}

pub fn validation_detail(pattern: &str) -> Option<String> {
    validate_pattern(pattern).error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_re_prefix() {
        let p = subject_pattern_from("Re: Weekly deals");
        assert!(p.contains("Weekly deals"));
        assert!(!p.to_lowercase().contains("re:"));
    }

    #[test]
    fn sender_pattern() {
        assert_eq!(sender_pattern_from("a@b.com"), "from:a@b.com");
    }

    #[test]
    fn validates_compound() {
        let v = validate_pattern("all:domain:amazon.com+subject:Your order");
        assert!(v.error.is_none());
        assert!(matches!(v.kind, PatternKind::All(_)));
    }

    #[test]
    fn specificity_order() {
        assert!(pattern_specificity("all:x") < pattern_specificity("subject:x"));
        assert!(pattern_specificity("subject:x") < pattern_specificity("from:x"));
    }
}
