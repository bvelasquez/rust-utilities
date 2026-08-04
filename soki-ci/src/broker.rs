use anyhow::{Context, Result};
use serde::Serialize;
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
            .timeout(Duration::from_secs(30))
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
        let res = self.client.get(&url).send().await?;
        Ok(res.status().is_success())
    }

    /// Fire-and-forget TTS (does not wait for playback to finish).
    pub async fn speak(&self, text: &str) -> Result<()> {
        let url = format!("{}/v1/tts/speak", self.base_url);
        let body = serde_json::json!({
            "text": text,
            "voice": self.voice,
        });
        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("broker speak request")?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("broker speak HTTP {status}: {body}");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct BrokerStatus {
    pub base_url: String,
    pub reachable: bool,
}
