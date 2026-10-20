use std::time::Duration;

use airouter_core::redact;
use reqwest::{RequestBuilder, Response};
use serde_json::Value;

use crate::ProviderError;

pub fn should_retry(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504)
}

pub fn retry_delay(resp: &Response, backoff_ms: u64, attempt: u32) -> Duration {
    let header = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_retry_after);
    let base = header.unwrap_or_else(|| {
        let factor = 1u64 << attempt.min(4);
        Duration::from_millis(backoff_ms.saturating_mul(factor))
    });
    base.min(Duration::from_secs(2))
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    if let Ok(secs) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    None
}

pub async fn send_with_retry<F>(
    mut build: F,
    max_retries: u32,
    backoff_ms: u64,
) -> Result<Response, ProviderError>
where
    F: FnMut() -> RequestBuilder,
{
    let mut attempt = 0u32;
    loop {
        let resp = match build().send().await {
            Ok(resp) => resp,
            Err(err) if err.is_timeout() || err.is_connect() || err.is_request() => {
                if attempt < max_retries {
                    let wait =
                        Duration::from_millis(backoff_ms.saturating_mul(1u64 << attempt.min(4)))
                            .min(Duration::from_secs(2));
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                    continue;
                }
                return Err(ProviderError::Transport(err.to_string()));
            }
            Err(err) => return Err(ProviderError::Transport(err.to_string())),
        };
        if should_retry(resp.status().as_u16()) && attempt < max_retries {
            let wait = retry_delay(&resp, backoff_ms, attempt);
            attempt += 1;
            tokio::time::sleep(wait).await;
            continue;
        }
        return Ok(resp);
    }
}

pub async fn error_from_response(resp: Response) -> ProviderError {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let text = resp.text().await.unwrap_or_default();
    let message = if let Ok(v) = serde_json::from_str::<Value>(&text) {
        v.pointer("/error/message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| text.clone())
    } else {
        text
    };
    let message = if message.trim().is_empty() {
        format!("upstream status {status}")
    } else {
        redact(&message)
    };
    ProviderError::Upstream {
        status,
        message,
        retry_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_gateway_and_rate_limit_only() {
        assert!(should_retry(429));
        assert!(should_retry(503));
        assert!(!should_retry(400));
        assert!(!should_retry(401));
        assert!(!should_retry(200));
    }
}
