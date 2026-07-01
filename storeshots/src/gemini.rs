use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            anyhow::bail!("Gemini API key is empty");
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .context("build HTTP client")?,
            api_key,
        })
    }

    pub async fn generate_text(&self, model: &str, system: &str, user: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Body<'a> {
            #[serde(rename = "systemInstruction")]
            system_instruction: SystemInstruction<'a>,
            contents: Vec<Content<'a>>,
            #[serde(rename = "generationConfig")]
            generation_config: GenerationConfig,
        }

        #[derive(Serialize)]
        struct SystemInstruction<'a> {
            parts: Vec<PartText<'a>>,
        }

        #[derive(Serialize)]
        struct Content<'a> {
            role: &'static str,
            parts: Vec<PartText<'a>>,
        }

        #[derive(Serialize)]
        struct PartText<'a> {
            text: &'a str,
        }

        #[derive(Serialize)]
        struct GenerationConfig {
            #[serde(rename = "responseMimeType")]
            response_mime_type: &'static str,
            temperature: f32,
        }

        let body = Body {
            system_instruction: SystemInstruction {
                parts: vec![PartText { text: system }],
            },
            contents: vec![Content {
                role: "user",
                parts: vec![PartText { text: user }],
            }],
            generation_config: GenerationConfig {
                response_mime_type: "application/json",
                temperature: 0.7,
            },
        };

        let url = format!("{API_BASE}/models/{model}:generateContent");
        let resp = self
            .http
            .post(&url)
            .query(&[("key", self.api_key.as_str())])
            .json(&body)
            .send()
            .await
            .context("Gemini text request")?;

        let status = resp.status();
        let raw: GenerateContentResponse = resp
            .json()
            .await
            .with_context(|| format!("parse Gemini text response (HTTP {status})"))?;

        if let Some(err) = raw.error {
            bail!("Gemini API error: {}", err.message);
        }

        let text = raw
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .and_then(|c| c.parts.into_iter().next())
            .and_then(|p| p.text)
            .context("empty Gemini text response")?;

        Ok(text)
    }

    pub async fn generate_background_image(
        &self,
        model: &str,
        prompt: &str,
        aspect_ratio: &str,
    ) -> Result<Vec<u8>> {
        #[derive(Serialize)]
        struct Body<'a> {
            contents: Vec<Content<'a>>,
            #[serde(rename = "generationConfig")]
            generation_config: ImageGenerationConfig<'a>,
        }

        #[derive(Serialize)]
        struct Content<'a> {
            parts: Vec<Part<'a>>,
        }

        #[derive(Serialize)]
        #[serde(untagged)]
        enum Part<'a> {
            Text { text: &'a str },
        }

        #[derive(Serialize)]
        struct ImageGenerationConfig<'a> {
            #[serde(rename = "responseModalities")]
            response_modalities: Vec<&'static str>,
            #[serde(rename = "imageConfig")]
            image_config: ImageConfig<'a>,
        }

        #[derive(Serialize)]
        struct ImageConfig<'a> {
            #[serde(rename = "aspectRatio")]
            aspect_ratio: &'a str,
        }

        let body = Body {
            contents: vec![Content {
                parts: vec![Part::Text { text: prompt }],
            }],
            generation_config: ImageGenerationConfig {
                response_modalities: vec!["IMAGE"],
                image_config: ImageConfig { aspect_ratio },
            },
        };

        let url = format!("{API_BASE}/models/{model}:generateContent");
        let resp = self
            .http
            .post(&url)
            .query(&[("key", self.api_key.as_str())])
            .json(&body)
            .send()
            .await
            .context("Gemini image request")?;

        let status = resp.status();
        let raw: GenerateContentResponse = resp
            .json()
            .await
            .with_context(|| format!("parse Gemini image response (HTTP {status})"))?;

        if let Some(err) = raw.error {
            bail!("Gemini API error: {}", err.message);
        }

        let parts = raw
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .map(|c| c.parts)
            .context("empty Gemini image response")?;

        for part in parts {
            if let Some(inline) = part.inline_data {
                let bytes = STANDARD
                    .decode(inline.data)
                    .context("decode image base64")?;
                return Ok(bytes);
            }
        }

        bail!("Gemini image response contained no image data")
    }
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<ResponseContent>,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
struct ResponsePart {
    text: Option<String>,
    #[serde(rename = "inlineData")]
    inline_data: Option<InlineData>,
}

#[derive(Debug, Deserialize)]
struct InlineData {
    data: String,
}
