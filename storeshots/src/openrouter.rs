use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub struct OpenRouterClient {
    http: reqwest::Client,
    api_key: String,
}

impl OpenRouterClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            anyhow::bail!("OpenRouter API key is empty");
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .build()
                .context("build HTTP client")?,
            api_key,
        })
    }

    pub async fn generate_text(
        &self,
        model: &str,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String> {
        let mut body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": 0.7,
        });

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let resp = self
            .http
            .post(OPENROUTER_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://soki-creative.com")
            .header("X-Title", "storeshots")
            .json(&body)
            .send()
            .await
            .context("OpenRouter request")?;

        let status = resp.status();
        let raw: ChatResponse = resp
            .json()
            .await
            .with_context(|| format!("parse OpenRouter response (HTTP {status})"))?;

        if let Some(err) = raw.error {
            bail!("OpenRouter API error: {}", err.message);
        }

        let text = raw
            .choices
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.message)
            .and_then(|m| m.content)
            .context("empty OpenRouter response")?;

        Ok(strip_json_fences(&text))
    }
}

fn strip_json_fences(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("```") {
        let inner = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        return inner.to_string();
    }
    trimmed.to_string()
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Option<Vec<Choice>>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextProvider {
    #[default]
    Openrouter,
    Gemini,
}
