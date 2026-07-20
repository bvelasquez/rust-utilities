//! Generate a single mail-sweep match pattern from a natural-language description.

use anyhow::{bail, Context, Result};

use crate::config::AppContext;
use crate::openrouter::{chat_json, extract_json};
use crate::rules::patterns::validate_pattern;

const SYSTEM: &str = "You write mail-sweep triage match patterns. Output only valid JSON. \
Patterns must use this grammar exactly (no inventing OR compounds):\n\
- subject:TEXT or subject:REGEX — subject line\n\
- from:EMAIL or from:REGEX — sender address\n\
- domain:DOMAIN — sender domain\n\
- body:TEXT — body preview\n\
- header:Name or header:Name:REGEX\n\
- has:list-unsubscribe\n\
- all:PART+PART — AND of sub-patterns (e.g. all:domain:amazon.com+subject:Invoice)\n\
Prefer durable reusable rules. Use regex only when needed; escape carefully. \
For case-insensitive subject matches prefer a simple literal substring when enough, \
else a regex like subject:(?i)invoice.";

pub async fn pattern_from_description(
    ctx: &AppContext,
    description: &str,
    current_pattern: Option<&str>,
) -> Result<String> {
    let description = description.trim();
    if description.is_empty() {
        bail!("description is empty");
    }

    let api_key = ctx.llm_api_key()?;
    let model = ctx.llm_model();
    let prompt = build_prompt(description, current_pattern);
    let raw = chat_json(api_key, &model, SYSTEM, &prompt).await?;
    let json = extract_json(&raw);

    let parsed: LlmPatternResponse =
        serde_json::from_str(&json).with_context(|| format!("parse LLM JSON: {raw}"))?;

    let pattern = parsed.pattern.trim().to_string();
    if pattern.is_empty() {
        bail!("LLM returned an empty pattern");
    }
    if let Some(err) = validate_pattern(&pattern).error {
        bail!("LLM pattern invalid ({err}): {pattern}");
    }
    Ok(pattern)
}

fn build_prompt(description: &str, current: Option<&str>) -> String {
    let current = current
        .map(|c| format!("Current pattern (may refine or replace): {c}\n"))
        .unwrap_or_default();
    format!(
        r#"{current}User wants a triage rule that matches:
"""
{description}
"""

Return JSON only:
{{
  "pattern": "subject:(?i)invoice",
  "reason": "one short sentence"
}}
"#
    )
}

#[derive(Debug, serde::Deserialize)]
struct LlmPatternResponse {
    pattern: String,
    #[serde(default)]
    #[allow(dead_code)]
    reason: String,
}
