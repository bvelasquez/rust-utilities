use anyhow::{Context, Result};

use crate::agent::schema::{
    ClassificationPattern, PatternSuggestionPlan, SenderDetailInput,
};
use crate::config::AppContext;
use crate::openrouter::{chat_json, extract_json};
use crate::rules::patterns::header_value;
use crate::store::CachedMessage;

const SYSTEM: &str = "You are an email pattern designer. Propose 2-4 reusable triage match patterns \
for a sender group. Patterns must be durable rules, not one-off message IDs. Output only valid JSON. \
Prefer header and compound patterns when they distinguish noise from important mail.";

pub fn sender_detail_input(sender: &str, messages: &[CachedMessage]) -> SenderDetailInput {
    let domain = sender.split('@').nth(1).unwrap_or("").to_string();
    let mut subjects: Vec<String> = messages
        .iter()
        .map(|m| m.subject.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .take(8)
        .collect();
    subjects.sort();

    let sample_snippet = messages
        .first()
        .map(|m| m.body_preview.chars().take(300).collect())
        .unwrap_or_default();

    let has_list_unsubscribe = messages
        .iter()
        .any(|m| m.list_unsubscribe.as_ref().is_some_and(|s| !s.is_empty()));

    let mut header_hints = Vec::new();
    if let Some(m) = messages.first() {
        for name in ["List-Id", "Precedence", "Auto-Submitted", "List-Unsubscribe"] {
            if let Some(val) = header_value(m, name) {
                if !val.is_empty() {
                    header_hints.push(format!("{name}: {val}"));
                }
            }
        }
    }

    SenderDetailInput {
        sender: sender.to_string(),
        domain,
        message_count: messages.len(),
        subjects,
        sample_snippet,
        has_list_unsubscribe,
        header_hints,
    }
}

pub async fn suggest_patterns(
    ctx: &AppContext,
    detail: &SenderDetailInput,
) -> Result<PatternSuggestionPlan> {
    let api_key = ctx.llm_api_key()?;
    let model = ctx.llm_model();
    let prompt = build_prompt(detail);
    let raw = chat_json(api_key, &model, SYSTEM, &prompt).await?;
    let json = extract_json(&raw);

    let parsed: LlmSuggestResponse =
        serde_json::from_str(&json).with_context(|| format!("parse LLM JSON: {raw}"))?;

    let patterns = parsed
        .patterns
        .into_iter()
        .map(|p| ClassificationPattern {
            match_pattern: p.match_pattern,
            category: p.category,
            priority: p.priority.clamp(1, 5),
            action: p.action,
            target_folder: p.target_folder,
            confidence: p.confidence.clamp(0.0, 1.0),
            reason: p.reason,
        })
        .collect();

    Ok(PatternSuggestionPlan {
        patterns,
        summary: parsed.summary,
    })
}

fn build_prompt(detail: &SenderDetailInput) -> String {
    let detail_json = serde_json::to_string_pretty(detail).unwrap_or_default();
    format!(
        r#"Propose 2-4 ranked triage patterns for this sender group.

Pattern grammar:
- subject:TEXT or subject:REGEX
- from:EMAIL or from:REGEX
- domain:DOMAIN
- header:Header-Name (non-empty) or header:Header-Name:REGEX
- body:TEXT
- has:list-unsubscribe
- all:PART+PART (AND compound, e.g. all:domain:github.com+subject:\[.*\])

Return JSON:
{{
  "patterns": [
    {{
      "match_pattern": "has:list-unsubscribe",
      "category": "newsletter",
      "priority": 2,
      "action": "archive",
      "target_folder": null,
      "confidence": 0.0-1.0,
      "reason": "why this pattern fits"
    }}
  ],
  "summary": "one line"
}}

Prefer patterns more specific than bare from: when subjects/headers show structure.
Do NOT use OR compounds.

Sender group:
{detail_json}
"#
    )
}

#[derive(Debug, serde::Deserialize)]
struct LlmSuggestResponse {
    patterns: Vec<LlmPattern>,
    summary: String,
}

#[derive(Debug, serde::Deserialize)]
struct LlmPattern {
    match_pattern: String,
    category: String,
    priority: u8,
    action: String,
    #[serde(default)]
    target_folder: Option<String>,
    confidence: f32,
    reason: String,
}
