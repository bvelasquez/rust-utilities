use anyhow::{Context, Result};

use crate::agent::schema::{PatternClassificationPlan, SenderGroupInput};
use crate::config::AppContext;
use crate::openrouter::{chat_json, extract_json};
use crate::store::LearningHint;

const SYSTEM: &str = "You are an email triage classifier. You do NOT classify individual messages. \
Instead, propose reusable match patterns (from:, domain:, subject:) that apply to groups of mail. \
Output only valid JSON. Prefer archiving newsletters/marketing noise so the inbox stays clean, \
flagging real human/priority mail, and only suggesting delete for clear spam. \
Be honest about confidence: ≥0.88 only when the pattern is clearly reusable; \
0.55–0.87 when plausible but uncertain; below 0.55 when guessing. \
Use priority 5 for urgent human mail, 1 for bulk noise.";

pub async fn classify_sender_groups(
    ctx: &AppContext,
    groups: &[SenderGroupInput],
    hints: &[LearningHint],
) -> Result<PatternClassificationPlan> {
    if groups.is_empty() {
        return Ok(PatternClassificationPlan {
            patterns: vec![],
            summary: "No sender groups to classify".into(),
        });
    }

    let api_key = ctx.llm_api_key()?;
    let model = ctx.llm_model();
    let prompt = build_prompt(groups, hints);
    let raw = chat_json(api_key, &model, SYSTEM, &prompt).await?;
    let json = extract_json(&raw);

    let parsed: LlmPatternResponse =
        serde_json::from_str(&json).with_context(|| format!("parse LLM JSON: {raw}"))?;

    let patterns = parsed
        .patterns
        .into_iter()
        .map(|p| {
            let archive = p.action.eq_ignore_ascii_case("archive");
            crate::agent::schema::ClassificationPattern {
                match_pattern: p.match_pattern,
                category: p.category,
                priority: p.priority.clamp(1, 5),
                action: p.action,
                target_folder: if archive { None } else { p.target_folder },
                confidence: p.confidence.clamp(0.0, 1.0),
                reason: p.reason,
            }
        })
        .collect();

    Ok(PatternClassificationPlan {
        patterns,
        summary: parsed.summary,
    })
}

fn build_prompt(groups: &[SenderGroupInput], hints: &[LearningHint]) -> String {
    let groups_json = serde_json::to_string_pretty(groups).unwrap_or_default();
    let hints_json = serde_json::to_string_pretty(hints).unwrap_or_default();

    format!(
        r#"Propose triage patterns for these sender groups. Each group may represent many emails.
Return JSON:
{{
  "patterns": [
    {{
      "match_pattern": "from:sender@example.com or domain:example.com or all:domain:example.com+subject:weekly",
      "category": "priority|personal|work|newsletter|marketing|notification|receipt|spam|unknown",
      "priority": 1-5,
      "action": "keep|mark_read|flag|unflag|archive|move|delete|tag",
      "target_folder": "optional folder for move/archive",
      "confidence": 0.0-1.0,
      "reason": "short explanation"
    }}
  ],
  "summary": "one line — mention how many senders/messages covered"
}}

Rules:
- Prefer `from:` for single senders; use `domain:` when the whole domain is similar noise.
- Use `header:`, `body:`, `has:list-unsubscribe`, or `all:PART+PART` when they better capture the pattern.
- One pattern can cover a sender group; do NOT return per-message entries.
- Respect user learning hints when the sender matches.
- Prefer `archive` (or `mark_read`) over `delete` for newsletters/marketing — deletes need human Review.
- Set confidence honestly: high (≥0.88) only for clear noise or clear priority patterns.
- For action=archive, always leave target_folder null (uses account archive folder, e.g. Gmail All Mail).
- Only set target_folder when action=move AND the folder is a real Gmail label the user already has.
- Never invent folder names (no walmart_newsletters, no made-up labels).

User learning hints (always prefer these):
{hints_json}

Sender groups (classify these, not individual emails):
{groups_json}
"#
    )
}

#[derive(Debug, serde::Deserialize)]
struct LlmPatternResponse {
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
