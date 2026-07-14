use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use super::types::{Provider, ProviderTestResult, UsageBucket};

pub async fn fetch_openrouter(api_key: &str, days: i64) -> Result<Vec<UsageBucket>> {
    let client = Client::new();
    let end = Utc::now();
    let start = end - Duration::days(days);
    let mut all = Vec::new();
    let mut offset = 0i64;

    loop {
        let body = json!({
            "metrics": ["total_usage", "tokens_total", "request_count"],
            "dimensions": ["model"],
            "granularity": "day",
            "time_range": {
                "start": iso8601_z(start),
                "end": iso8601_z(end)
            },
            "limit": 1000,
            "offset": offset
        });

        let resp = client
            .post("https://openrouter.ai/api/v1/analytics/query")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .context("openrouter analytics request")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("openrouter analytics {status}: {text}");
        }

        let parsed: AnalyticsResponse =
            serde_json::from_str(&text).context("parse openrouter response")?;
        let rows = parsed.rows();
        if rows.is_empty() {
            break;
        }

        for row in rows {
            let model = row.model.clone();
            let cost = row.total_usage.unwrap_or(0.0);
            let tokens = row.tokens_total.as_ref().and_then(|v| parse_u64(v));
            let requests = row.request_count.as_ref().and_then(|v| parse_u64(v));
            let day = row
                .date_day
                .as_deref()
                .and_then(parse_day)
                .unwrap_or(start);

            all.push(UsageBucket {
                provider: Provider::Openrouter,
                bucket_start: day,
                granularity: "day".into(),
                model,
                cost_usd: cost,
                input_tokens: tokens,
                output_tokens: None,
                request_count: requests,
            });
        }

        let truncated = parsed
            .inner_metadata()
            .and_then(|m| m.truncated)
            .unwrap_or(false);
        if !truncated {
            break;
        }
        offset += rows.len() as i64;
    }

    Ok(all)
}

pub async fn test_openrouter(api_key: &str) -> ProviderTestResult {
    let client = Client::new();
    let result = client
        .get("https://openrouter.ai/api/v1/analytics/meta")
        .bearer_auth(api_key)
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => ProviderTestResult {
            provider: Provider::Openrouter,
            ok: true,
            message: "management key verified — analytics API accessible".into(),
            key_type_hint: Provider::Openrouter.key_hint().into(),
            docs_url: Provider::Openrouter.docs_url().into(),
        },
        Ok(resp) if resp.status().as_u16() == 403 => ProviderTestResult {
            provider: Provider::Openrouter,
            ok: false,
            message: "403 forbidden — use a management key, not a regular inference key".into(),
            key_type_hint: Provider::Openrouter.key_hint().into(),
            docs_url: Provider::Openrouter.docs_url().into(),
        },
        Ok(resp) => ProviderTestResult {
            provider: Provider::Openrouter,
            ok: false,
            message: format!("HTTP {}", resp.status()),
            key_type_hint: Provider::Openrouter.key_hint().into(),
            docs_url: Provider::Openrouter.docs_url().into(),
        },
        Err(e) => ProviderTestResult {
            provider: Provider::Openrouter,
            ok: false,
            message: e.to_string(),
            key_type_hint: Provider::Openrouter.key_hint().into(),
            docs_url: Provider::Openrouter.docs_url().into(),
        },
    }
}

fn iso8601_z(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[derive(Debug, Deserialize)]
struct AnalyticsResponse {
    data: AnalyticsPayload,
}

#[derive(Debug, Deserialize)]
struct AnalyticsPayload {
    data: Vec<AnalyticsRow>,
    metadata: Option<AnalyticsMeta>,
}

impl AnalyticsResponse {
    fn rows(&self) -> &[AnalyticsRow] {
        &self.data.data
    }

    fn inner_metadata(&self) -> Option<&AnalyticsMeta> {
        self.data.metadata.as_ref()
    }
}

#[derive(Debug, Deserialize)]
struct AnalyticsMeta {
    truncated: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AnalyticsRow {
    model: Option<String>,
    total_usage: Option<f64>,
    tokens_total: Option<String>,
    request_count: Option<String>,
    #[serde(rename = "date__day")]
    date_day: Option<String>,
}

fn parse_u64(s: &str) -> Option<u64> {
    s.parse().ok()
}

fn parse_day(s: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
}
