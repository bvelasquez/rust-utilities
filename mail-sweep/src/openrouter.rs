use anyhow::{Context, Result};
use std::time::Duration;

pub async fn chat_json(api_key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("build HTTP client")?;
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/barryvelasquez/utilities")
        .header("X-Title", "mail-sweep")
        .json(&body)
        .send()
        .await
        .context("OpenRouter request failed (check network / API key)")?
        .error_for_status()
        .context("OpenRouter HTTP error")?
        .json::<serde_json::Value>()
        .await
        .context("decode OpenRouter JSON")?;

    if let Some(err) = resp["error"]["message"].as_str() {
        anyhow::bail!("OpenRouter API error: {err}");
    }

    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .context("missing OpenRouter response content")
}

pub fn extract_json(raw: &str) -> String {
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
