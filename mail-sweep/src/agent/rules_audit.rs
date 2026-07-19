use anyhow::{Context, Result};

use crate::agent::schema::{
    ProposedRule, RuleAuditInput, RuleAuditPlan, RuleAuditSuggestion,
};
use crate::config::{AppContext, RuleConfig};
use crate::openrouter::{chat_json, extract_json};
use crate::store::Store;

const SYSTEM: &str = "You are an email rules auditor. Review existing triage rules and suggest \
consolidations, generalizations, and gap-filling patterns. Output only valid JSON. Never invent \
senders not present in the samples. Prefer broader domain/subject/header patterns over many \
narrow from: rules when action and category agree.";

pub fn build_audit_inputs(rules: &[RuleConfig], store: &Store) -> Result<Vec<RuleAuditInput>> {
    let mut inputs = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let matches = store.messages_matching_pattern(&rule.r#match, 5000, 5)?;
        inputs.push(RuleAuditInput {
            index,
            r#match: rule.r#match.clone(),
            action: rule.action.clone(),
            category: rule.category.clone(),
            priority: rule.priority,
            sample_subjects: matches.iter().map(|m| m.subject.clone()).collect(),
            sample_senders: matches
                .iter()
                .map(|m| m.from_address.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .take(3)
                .collect(),
            match_count: store
                .messages_matching_pattern(&rule.r#match, 5000, usize::MAX)?
                .len(),
        });
    }
    Ok(inputs)
}

pub fn find_local_duplicates(rules: &[RuleConfig]) -> Vec<RuleAuditSuggestion> {
    let mut suggestions = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, rule) in rules.iter().enumerate() {
        let key = format!("{}|{}|{}", rule.r#match, rule.action, rule.category.as_deref().unwrap_or(""));
        if let Some(&first) = seen.get(&key) {
            suggestions.push(RuleAuditSuggestion {
                kind: "remove".into(),
                confidence: 1.0,
                reason: format!("duplicate of rule [{first}]"),
                proposed_rules: vec![],
                retire_indices: vec![i],
                example_subjects: vec![],
            });
        } else {
            seen.insert(key, i);
        }
    }
    suggestions
}

pub async fn audit_rules(ctx: &AppContext, rules: &[RuleConfig], store: &Store) -> Result<RuleAuditPlan> {
    if rules.is_empty() {
        return Ok(RuleAuditPlan {
            suggestions: vec![],
            summary: "No rules to audit".into(),
        });
    }

    let inputs = build_audit_inputs(rules, store)?;
    let mut local = find_local_duplicates(rules);

    let api_key = ctx.llm_api_key()?;
    let model = ctx.llm_model();
    let prompt = build_prompt(&inputs);
    let raw = chat_json(api_key, &model, SYSTEM, &prompt).await?;
    let json = extract_json(&raw);

    let parsed: LlmAuditResponse =
        serde_json::from_str(&json).with_context(|| format!("parse LLM JSON: {raw}"))?;

    let mut suggestions: Vec<RuleAuditSuggestion> = parsed
        .suggestions
        .into_iter()
        .map(|s| RuleAuditSuggestion {
            kind: s.kind,
            confidence: s.confidence.clamp(0.0, 1.0),
            reason: s.reason,
            proposed_rules: s
                .proposed_rules
                .into_iter()
                .map(|r| ProposedRule {
                    r#match: r.r#match,
                    action: r.action,
                    category: r.category,
                    priority: r.priority,
                    target_folder: r.target_folder,
                })
                .collect(),
            retire_indices: s.retire_indices,
            example_subjects: s.example_subjects,
        })
        .collect();

    suggestions.append(&mut local);

    Ok(RuleAuditPlan {
        summary: parsed.summary,
        suggestions,
    })
}

fn build_prompt(inputs: &[RuleAuditInput]) -> String {
    let rules_json = serde_json::to_string_pretty(inputs).unwrap_or_default();
    format!(
        r#"Review these email triage rules and suggest improvements.

Pattern grammar:
- subject:TEXT or subject:REGEX — match subject line
- from:EMAIL or from:REGEX — match sender address
- domain:DOMAIN — match sender domain
- header:Header-Name — header present and non-empty
- header:Header-Name:REGEX — match header value
- body:TEXT — match body preview
- has:list-unsubscribe — List-Unsubscribe header present
- all:PART+PART — all sub-patterns must match (e.g. all:domain:amazon.com+subject:Your order)

Return JSON:
{{
  "suggestions": [
    {{
      "kind": "merge|replace|add|remove|conflict",
      "confidence": 0.0-1.0,
      "reason": "short explanation",
      "proposed_rules": [
        {{ "match": "domain:example.com", "action": "archive", "category": "newsletter", "priority": 2, "target_folder": null }}
      ],
      "retire_indices": [2, 5],
      "example_subjects": ["sample subject lines"]
    }}
  ],
  "summary": "one line overview"
}}

Rules:
- merge/replace: retire_indices lists rule indexes to remove; proposed_rules are replacements
- add: new rules with empty retire_indices
- remove: retire_indices only, empty proposed_rules
- conflict: highlight contradictory rules, do not auto-resolve
- Prefer domain: or subject: regex over many from: rules with same action
- Do NOT use OR compounds (not supported)

Current rules with coverage samples:
{rules_json}
"#
    )
}

#[derive(Debug, serde::Deserialize)]
struct LlmAuditResponse {
    suggestions: Vec<LlmAuditSuggestion>,
    summary: String,
}

#[derive(Debug, serde::Deserialize)]
struct LlmAuditSuggestion {
    kind: String,
    confidence: f32,
    reason: String,
    proposed_rules: Vec<LlmProposedRule>,
    #[serde(default)]
    retire_indices: Vec<usize>,
    #[serde(default)]
    example_subjects: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct LlmProposedRule {
    r#match: String,
    action: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    target_folder: Option<String>,
}

/// Apply accepted audit suggestions to a rules vec (returns new rules list).
pub fn apply_audit_suggestions(
    rules: &[RuleConfig],
    suggestions: &[RuleAuditSuggestion],
) -> Vec<RuleConfig> {
    let mut out = rules.to_vec();
    let mut to_retire: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for suggestion in suggestions {
        for &idx in &suggestion.retire_indices {
            to_retire.insert(idx);
        }
        for proposed in &suggestion.proposed_rules {
            out.push(RuleConfig {
                id: None,
                r#match: proposed.r#match.clone(),
                category: proposed.category.clone(),
                action: proposed.action.clone(),
                priority: proposed.priority,
                target_folder: proposed.target_folder.clone(),
            });
        }
    }

    let mut final_rules: Vec<RuleConfig> = out
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| if to_retire.contains(&i) { None } else { Some(r) })
        .collect();

    // Deduplicate by match string (keep first)
    let mut seen = std::collections::HashSet::new();
    final_rules.retain(|r| seen.insert(r.r#match.clone()));
    final_rules
}

/// Preview how many messages a proposed rule would match.
pub fn preview_rule_matches(store: &Store, pattern: &str) -> usize {
    store
        .messages_matching_pattern(pattern, 5000, usize::MAX)
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Check if any messages match a pattern (utility for tests).
pub fn pattern_matches_any(store: &Store, pattern: &str) -> bool {
    preview_rule_matches(store, pattern) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_retires_and_adds() {
        let rules = vec![
            RuleConfig {
                id: None,
                r#match: "from:a@x.com".into(),
                category: None,
                action: "archive".into(),
                priority: None,
                target_folder: None,
            },
            RuleConfig {
                id: None,
                r#match: "from:b@x.com".into(),
                category: None,
                action: "archive".into(),
                priority: None,
                target_folder: None,
            },
        ];
        let suggestions = vec![RuleAuditSuggestion {
            kind: "merge".into(),
            confidence: 0.9,
            reason: "same domain".into(),
            proposed_rules: vec![ProposedRule {
                r#match: "domain:x.com".into(),
                action: "archive".into(),
                category: None,
                priority: None,
                target_folder: None,
            }],
            retire_indices: vec![0, 1],
            example_subjects: vec![],
        }];
        let merged = apply_audit_suggestions(&rules, &suggestions);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].r#match, "domain:x.com");
    }
}
