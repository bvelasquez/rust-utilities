//! Telegram outbound notifications + inbound command polling.
//! Optional: only active when bot_token and chat_id are configured.

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct TelegramClient {
    bot_token: String,
    chat_id: Option<String>,
    client: reqwest::Client,
}

impl TelegramClient {
    pub fn new(bot_token: String, chat_id: Option<String>) -> Self {
        Self {
            bot_token,
            chat_id,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        !self.bot_token.is_empty()
    }

    fn api(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.bot_token, method)
    }

    /// Send a text message. Ok(true) if sent, Ok(false) if disabled/unconfigured.
    pub async fn send(&self, text: &str) -> Result<bool> {
        if !self.enabled() {
            return Ok(false);
        }
        let Some(chat) = &self.chat_id else {
            return Ok(false);
        };
        let url = self.api("sendMessage");
        self.client
            .post(&url)
            .json(&serde_json::json!({ "chat_id": chat, "text": text }))
            .send()
            .await
            .context("telegram sendMessage")?
            .error_for_status()?;
        Ok(true)
    }

    /// Long-poll inbound messages. Each text message is passed to `handler(chat_id, text)`.
    /// Returns when the stop channel closes.
    pub async fn poll_inbound(
        &self,
        allowed_chat: Option<String>,
        mut handler: impl FnMut(String, String) + Send + 'static,
        mut stop_rx: mpsc::Receiver<()>,
    ) {
        if !self.enabled() {
            while stop_rx.recv().await.is_some() {}
            return;
        }
        let mut offset: i64 = 0;
        loop {
            tokio::select! {
                _ = stop_rx.recv() => break,
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
            }
            let url = self.api("getUpdates");
            let body = async {
                let res = self
                    .client
                    .post(&url)
                    .json(&serde_json::json!({
                        "offset": offset,
                        "timeout": 25,
                        "allowed_updates": ["message"],
                    }))
                    .send()
                    .await?;
                let res = res.error_for_status()?;
                res.json::<serde_json::Value>().await
            }
            .await;
            let body = match body {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("telegram poll error: {e}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            let updates = body
                .get("result")
                .and_then(|r| r.as_array())
                .cloned()
                .unwrap_or_default();
            for u in updates {
                if let Some(id) = u.get("update_id").and_then(|v| v.as_i64()) {
                    offset = id + 1;
                }
                let Some(chat_id) = u.pointer("/message/chat/id").and_then(|v| v.as_i64()) else {
                    continue;
                };
                let Some(txt) = u.pointer("/message/text").and_then(|v| v.as_str()) else {
                    continue;
                };
                if let Some(allowed) = &allowed_chat {
                    if &chat_id.to_string() != allowed {
                        continue;
                    }
                }
                handler(chat_id.to_string(), txt.to_string());
            }
        }
    }
}

/// Resolve chat id with env fallback (TELEGRAM_CHAT_ID).
pub fn chat_id(cfg: &str) -> Option<String> {
    if !cfg.is_empty() {
        return Some(cfg.to_string());
    }
    std::env::var("TELEGRAM_CHAT_ID").ok().filter(|s| !s.is_empty())
}

/// Resolve bot token with env fallback (TELEGRAM_BOT_TOKEN).
pub fn bot_token(cfg: &str) -> String {
    if !cfg.is_empty() {
        return cfg.to_string();
    }
    std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default()
}