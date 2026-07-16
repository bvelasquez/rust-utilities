use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::AppContext;
use crate::scan::{dir_size, format_bytes};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub size_human: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecommendation {
    pub path: String,
    pub verdict: String,
    pub confidence: f32,
    pub reason: String,
    pub reclaimable_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub root: String,
    pub total_bytes: u64,
    pub entries: Vec<FolderEntry>,
    pub recommendations: Vec<ReviewRecommendation>,
    pub summary: String,
}

pub async fn review_folder(ctx: &AppContext, path: &Path, limit: usize) -> Result<ReviewReport> {
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve path {}", path.display()))?;

    if !path.is_dir() {
        anyhow::bail!("{} is not a directory", path.display());
    }

    let total_bytes = dir_size(&path)?;
    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&path)?.flatten().take(limit) {
        let p = entry.path();
        let size = dir_size(&p)?;
        entries.push(FolderEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: p.display().to_string(),
            size_bytes: size,
            size_human: format_bytes(size),
        });
    }

    entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    let api_key = ctx.llm_api_key()?;
    let model = ctx.llm_model();

    let prompt = build_prompt(&path, total_bytes, &entries);
    let raw = call_openrouter(api_key, &model, &prompt).await?;

    let parsed: LlmReviewResponse = serde_json::from_str(&extract_json(&raw))
        .with_context(|| format!("parse LLM JSON response: {raw}"))?;

    let recommendations: Vec<ReviewRecommendation> = parsed
        .recommendations
        .into_iter()
        .map(|r| ReviewRecommendation {
            path: r.path,
            verdict: r.verdict,
            confidence: r.confidence,
            reason: r.reason,
            reclaimable_bytes: r.reclaimable_bytes,
        })
        .collect();

    Ok(ReviewReport {
        root: path.display().to_string(),
        total_bytes,
        entries,
        summary: parsed.summary,
        recommendations,
    })
}

fn build_prompt(path: &Path, total_bytes: u64, entries: &[FolderEntry]) -> String {
    let listing: String = entries
        .iter()
        .map(|e| format!("- {} ({}) — {}", e.name, e.size_human, e.path))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You are a macOS disk cleanup advisor. Analyze this folder and its immediate children.

Root: {root}
Total size: {total} bytes ({total_human})

Children:
{listing}

Classify each child (and the root if appropriate) for disk cleanup on a developer Mac.
Verdicts must be one of: safe_cleanup, caution, do_not_delete.

Rules:
- Caches, derived data, build artifacts, old archives, device support symbols, logs → usually safe_cleanup
- Source code, git repos, documents, photos, databases with user data → do_not_delete
- Package manager stores (node_modules can be caution; Homebrew caches often safe_cleanup)
- When unsure, use caution

Respond with ONLY valid JSON (no markdown fences):
{{
  "summary": "one paragraph overview",
  "recommendations": [
    {{
      "path": "/full/path",
      "verdict": "safe_cleanup|caution|do_not_delete",
      "confidence": 0.0,
      "reason": "short explanation",
      "reclaimable_bytes": null
    }}
  ]
}}"#,
        root = path.display(),
        total = total_bytes,
        total_human = format_bytes(total_bytes),
        listing = listing,
    )
}

#[derive(Debug, Deserialize)]
struct LlmReviewResponse {
    summary: String,
    recommendations: Vec<LlmRec>,
}

#[derive(Debug, Deserialize)]
struct LlmRec {
    path: String,
    verdict: String,
    confidence: f32,
    reason: String,
    #[serde(default)]
    reclaimable_bytes: Option<u64>,
}

async fn call_openrouter(api_key: &str, model: &str, prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": "You output only JSON for disk cleanup analysis." },
            { "role": "user", "content": prompt }
        ]
    });

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/barryvelasquez/utilities")
        .header("X-Title", "disk-sweep")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;

    if let Some(err) = resp["error"]["message"].as_str() {
        anyhow::bail!("OpenRouter API error: {err}");
    }

    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .context("missing OpenRouter response content")
}

fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    }
}
