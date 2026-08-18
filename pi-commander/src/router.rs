//! Natural-language-ish routing: "fix bug A on simple-workout" -> target
//! project / optional explicit worker index.

use regex::Regex;

use std::sync::OnceLock;

fn target_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:on|in|for|against)\s+([A-Za-z0-9._-]+)(?:#(\d+))?")
            .expect("target regex")
    })
}

/// Parse a dispatch line.
/// Returns (project_id, optional agent index, remaining text with the target removed).
/// If no explicit target is found, returns ("", None, original) so callers can
/// decide to broadcast or reject.
pub fn parse_target(text: &str, known_projects: &[String]) -> (String, Option<usize>, String) {
    // 1) explicit "proj#idx" at start
    let trimmed = text.trim();
    let start_re = Regex::new(r"^(?i)([A-Za-z0-9_-]+)(?:#(\d+))[: ]").unwrap();
    if let Some(caps) = start_re.captures(trimmed) {
        let id = caps.get(1).unwrap().as_str();
        if known_projects.iter().any(|p| p.eq_ignore_ascii_case(id)) {
            let idx = caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
            let rest = trimmed[caps.get(0).unwrap().end()..].trim().to_string();
            return (id.to_string(), idx, rest);
        }
    }

    // 2) "on|for|in <project>" anywhere
    if let Some(caps) = target_re().captures(text) {
        let cand = caps.get(1).unwrap().as_str();
        if let Some(matched) = known_projects
            .iter()
            .find(|p| p.eq_ignore_ascii_case(cand))
        {
            let idx = caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
            let start = caps.get(0).unwrap().start();
            let end = caps.get(0).unwrap().end();
            let prefix = text[..start].trim_end();
            let suffix = text[end..].trim_start();
            let rest = format!("{prefix} {suffix}").trim().to_string();
            return (matched.clone(), idx, rest);
        }
    }

    // 3) trailing "on <project>" (no punctuation after word)
    let trailing_re = Regex::new(r"(?i)\b(on|for|in|against)\s+([A-Za-z0-9_-]+?)\s*$").unwrap();
    if let Some(caps) = trailing_re.captures(text) {
        let cand = caps.get(2).unwrap().as_str();
        if let Some(matched) = known_projects
            .iter()
            .find(|p| p.eq_ignore_ascii_case(cand))
        {
            let start = caps.get(0).unwrap().start();
            let rest = text[..start].trim().to_string();
            return (matched.clone(), None, rest);
        }
    }

    ("".to_string(), None, text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Vec<String> {
        vec![
            "simple-workout".into(),
            "simple-sleep".into(),
            "paperdoll".into(),
            "mighty45-web".into(),
        ]
    }

    #[test]
    fn routes_on_phrase() {
        let (p, idx, rest) = parse_target("fix bug A on simple-workout", &known());
        assert_eq!(p, "simple-workout");
        assert!(idx.is_none());
        assert_eq!(rest, "fix bug A");
    }

    #[test]
    fn routes_explicit_index() {
        let (p, idx, rest) = parse_target("login-api#1 add a retry", &known());
        assert_eq!(p, "");
        let (p2, i2, _) = parse_target("simple-workout#2 fix the bug", &known());
        assert_eq!(p2, "simple-workout");
        assert_eq!(i2, Some(2));
        let _ = (p, idx, rest);
    }

    #[test]
    fn unknown_project_untouched() {
        let (p, idx, rest) = parse_target("fix the landing page", &known());
        assert_eq!(p, "");
        assert!(idx.is_none());
        assert_eq!(rest, "fix the landing page");
    }
}