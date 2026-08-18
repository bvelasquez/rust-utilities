//! BrokerClient — HTTP TTS via tts-stt-broker (mqtt-style fire and forget), the
//! same interface soki-ci talks to. Default: http://127.0.0.1:8787

use anyhow::{Context, Result};
use std::time::Duration;

#[derive(Clone)]
pub struct BrokerClient {
    base_url: String,
    voice: String,
    client: reqwest::Client,
}

impl BrokerClient {
    pub fn new(base_url: impl Into<String>, voice: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            voice: voice.into(),
            client,
        }
    }

    pub async fn health(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        let res = self.client.get(&url).send().await?.error_for_status()?;
        Ok(res.status().is_success())
    }

    /// Fire-and-forget TTS (does not wait for playback).
    pub async fn speak(&self, text: &str) -> Result<()> {
        let url = format!("{}/v1/tts/speak", self.base_url);
        let body = serde_json::json!({
            "text": text,
            "voice": self.voice,
        });
        let _res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("broker speak")?;
        Ok(())
    }

    /// Strict speak: waits for HTTP success.
    pub async fn speak_strict(&self, text: &str) -> Result<()> {
        let url = format!("{}/v1/tts/speak", self.base_url);
        let body = serde_json::json!({ "text": text, "voice": self.voice });
        self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("broker speak")?
            .error_for_status()?;
        Ok(())
    }
}