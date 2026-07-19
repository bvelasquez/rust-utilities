use anyhow::{Context, Result};

pub async fn chat_json(api_key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let client = reqwest::Client::new();
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
