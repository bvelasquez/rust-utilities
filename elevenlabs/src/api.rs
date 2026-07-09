use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::multipart::{Form, Part};
use serde_json::{json, Value};
use std::path::Path;

use crate::auth::AuthState;
use crate::config::{default_base_url, ElabsConfig};

pub const DEFAULT_TTS_MODEL: &str = "eleven_multilingual_v2";
pub const DEFAULT_STT_MODEL: &str = "scribe_v2";
pub const DEFAULT_SFX_MODEL: &str = "eleven_text_to_sound_v2";
pub const DEFAULT_OUTPUT_FORMAT: &str = "mp3_44100_128";

pub struct ElevenLabsClient {
    http: reqwest::Client,
    base_url: String,
}

impl ElevenLabsClient {
    pub fn new(auth: &AuthState, config_path: Option<&std::path::PathBuf>) -> Result<Self> {
        let base_url = resolve_base_url(config_path)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "xi-api-key",
            HeaderValue::from_str(&auth.api_key).context("invalid api key header")?,
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self {
            http,
            base_url,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    async fn api_error(&self, context: &str, resp: reqwest::Response) -> anyhow::Error {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::anyhow!("{context}: HTTP {status} — {body}")
    }

    pub async fn list_voices(
        &self,
        search: Option<&str>,
        page_size: u32,
        next_page_token: Option<&str>,
    ) -> Result<Value> {
        let mut url = reqwest::Url::parse(&self.url("/v2/voices"))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("page_size", &page_size.to_string());
            if let Some(s) = search {
                qp.append_pair("search", s);
            }
            if let Some(t) = next_page_token {
                qp.append_pair("next_page_token", t);
            }
        }
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            bail!(self.api_error("list voices", resp).await);
        }
        Ok(resp.json().await?)
    }

    pub async fn list_models(&self) -> Result<Value> {
        let resp = self.http.get(self.url("/v1/models")).send().await?;
        if !resp.status().is_success() {
            bail!(self.api_error("list models", resp).await);
        }
        Ok(resp.json().await?)
    }

    pub async fn text_to_speech(
        &self,
        voice_id: &str,
        text: &str,
        model_id: &str,
        output_format: &str,
    ) -> Result<Vec<u8>> {
        let url = format!(
            "{}?output_format={}",
            self.url(&format!("/v1/text-to-speech/{voice_id}")),
            urlencoding_query_value(output_format)
        );
        let body = json!({
            "text": text,
            "model_id": model_id,
        });
        let resp = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "audio/mpeg")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("text-to-speech", resp).await);
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn speech_to_text(
        &self,
        file_path: &Path,
        model_id: &str,
        language_code: Option<&str>,
        diarize: bool,
    ) -> Result<Value> {
        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("read {}", file_path.display()))?;
        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio");
        let mime = mime_guess::from_path(file_path)
            .first_or_octet_stream()
            .to_string();

        let file_part = Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(&mime)?;

        let mut form = Form::new()
            .part("file", file_part)
            .text("model_id", model_id.to_string())
            .text("diarize", diarize.to_string());

        if let Some(lang) = language_code {
            form = form.text("language_code", lang.to_string());
        }

        let resp = self
            .http
            .post(self.url("/v1/speech-to-text"))
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("speech-to-text", resp).await);
        }
        Ok(resp.json().await?)
    }

    pub async fn clone_voice(
        &self,
        name: &str,
        files: &[std::path::PathBuf],
        description: Option<&str>,
        remove_background_noise: bool,
    ) -> Result<Value> {
        let mut form = Form::new()
            .text("name", name.to_string())
            .text(
                "remove_background_noise",
                remove_background_noise.to_string(),
            );

        if let Some(desc) = description {
            form = form.text("description", desc.to_string());
        }

        for path in files {
            let bytes = tokio::fs::read(path)
                .await
                .with_context(|| format!("read {}", path.display()))?;
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sample");
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            let part = Part::bytes(bytes)
                .file_name(filename.to_string())
                .mime_str(&mime)?;
            form = form.part("files", part);
        }

        let resp = self
            .http
            .post(self.url("/v1/voices/add"))
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("clone voice", resp).await);
        }
        Ok(resp.json().await?)
    }

    pub async fn design_voice(
        &self,
        voice_description: &str,
        text: Option<&str>,
        auto_generate_text: bool,
        model_id: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "voice_description": voice_description,
            "auto_generate_text": auto_generate_text,
        });
        if let Some(t) = text {
            body["text"] = json!(t);
        }
        if let Some(m) = model_id {
            body["model_id"] = json!(m);
        }

        let resp = self
            .http
            .post(self.url("/v1/text-to-voice/design"))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("design voice", resp).await);
        }
        Ok(resp.json().await?)
    }

    pub async fn create_sound_effect(
        &self,
        text: &str,
        model_id: &str,
        output_format: &str,
        duration_seconds: Option<f64>,
        loop_audio: bool,
        prompt_influence: Option<f64>,
    ) -> Result<Vec<u8>> {
        let url = format!(
            "{}?output_format={}",
            self.url("/v1/sound-generation"),
            urlencoding_query_value(output_format)
        );
        let mut body = json!({
            "text": text,
            "model_id": model_id,
            "loop": loop_audio,
        });
        if let Some(d) = duration_seconds {
            body["duration_seconds"] = json!(d);
        }
        if let Some(p) = prompt_influence {
            body["prompt_influence"] = json!(p);
        }

        let resp = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "audio/mpeg")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("sound effect generation", resp).await);
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn list_history(
        &self,
        page_size: u32,
        start_after_history_item_id: Option<&str>,
        search: Option<&str>,
        model_id: Option<&str>,
    ) -> Result<Value> {
        let mut url = reqwest::Url::parse(&self.url("/v1/history"))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("page_size", &page_size.to_string());
            if let Some(id) = start_after_history_item_id {
                qp.append_pair("start_after_history_item_id", id);
            }
            if let Some(s) = search {
                qp.append_pair("search", s);
            }
            if let Some(m) = model_id {
                qp.append_pair("model_id", m);
            }
        }
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            bail!(self.api_error("list history", resp).await);
        }
        Ok(resp.json().await?)
    }

    pub async fn download_history_audio(&self, history_item_id: &str) -> Result<Vec<u8>> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/history/{history_item_id}/audio")))
            .header(ACCEPT, "audio/mpeg")
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("download history audio", resp).await);
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn save_voice_from_preview(
        &self,
        generated_voice_id: &str,
        voice_name: &str,
        voice_description: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "generated_voice_id": generated_voice_id,
            "voice_name": voice_name,
        });
        if let Some(desc) = voice_description {
            body["voice_description"] = json!(desc);
        }

        let resp = self
            .http
            .post(self.url("/v1/text-to-voice"))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!(self.api_error("save voice", resp).await);
        }
        Ok(resp.json().await?)
    }
}

fn resolve_base_url(config_path: Option<&std::path::PathBuf>) -> Result<String> {
    if let Ok(url) = std::env::var("ELABS_BASE_URL") {
        if !url.is_empty() {
            return Ok(url);
        }
    }
    if let Some(path) = config_path {
        if path.is_file() {
            if let Ok(cfg) = ElabsConfig::load(path) {
                if let Some(url) = cfg.base_url.filter(|u| !u.is_empty()) {
                    return Ok(url);
                }
            }
        }
    }
    let default_path = crate::config::default_config_path();
    if default_path.is_file() {
        if let Ok(cfg) = ElabsConfig::load(&default_path) {
            if let Some(url) = cfg.base_url.filter(|u| !u.is_empty()) {
                return Ok(url);
            }
        }
    }
    Ok(default_base_url().to_string())
}

fn urlencoding_query_value(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}