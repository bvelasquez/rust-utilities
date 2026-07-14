use anyhow::{Context, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

use super::types::{Provider, ProviderTestResult, UsageBucket};

#[derive(Debug, Clone, Default)]
pub struct CursorFetchOptions {
    pub email_filter: Option<String>,
}

pub async fn fetch_cursor(
    api_key: &str,
    days: i64,
    options: &CursorFetchOptions,
) -> Result<Vec<UsageBucket>> {
    let client = Client::new();
    let end = Utc::now();
    let start = end - Duration::days(days);
    let mut accum: HashMap<(DateTime<Utc>, String), BucketAccum> = HashMap::new();

    let mut chunk_start = start;
    while chunk_start < end {
        let chunk_end = (chunk_start + Duration::days(30)).min(end);
        fetch_chunk(
            &client,
            api_key,
            chunk_start,
            chunk_end,
            options,
            &mut accum,
        )
        .await?;
        chunk_start = chunk_end + Duration::milliseconds(1);
    }

    let mut buckets: Vec<UsageBucket> = accum
        .into_iter()
        .map(|((day, model), acc)| UsageBucket {
            provider: Provider::Cursor,
            bucket_start: day,
            granularity: "day".into(),
            model: Some(model),
            cost_usd: acc.cost_usd,
            input_tokens: if acc.input_tokens > 0 {
                Some(acc.input_tokens)
            } else {
                None
            },
            output_tokens: if acc.output_tokens > 0 {
                Some(acc.output_tokens)
            } else {
                None
            },
            request_count: if acc.requests > 0 {
                Some(acc.requests)
            } else {
                None
            },
        })
        .collect();
    buckets.sort_by(|a, b| a.bucket_start.cmp(&b.bucket_start));
    Ok(buckets)
}

#[derive(Default)]
struct BucketAccum {
    cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
    requests: u64,
}

async fn fetch_chunk(
    client: &Client,
    api_key: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    options: &CursorFetchOptions,
    accum: &mut HashMap<(DateTime<Utc>, String), BucketAccum>,
) -> Result<()> {
    let mut page = 1u32;
    loop {
        let mut body = json!({
            "startDate": start.timestamp_millis(),
            "endDate": end.timestamp_millis(),
            "page": page,
            "pageSize": 100
        });
        if let Some(email) = &options.email_filter {
            body["email"] = json!(email);
        }

        let resp = client
            .post("https://api.cursor.com/teams/filtered-usage-events")
            .basic_auth(api_key, Some(""))
            .json(&body)
            .send()
            .await
            .context("cursor filtered-usage-events request")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("cursor usage events {status}: {text}");
        }

        let parsed: UsageEventsResponse =
            serde_json::from_str(&text).context("parse cursor usage response")?;

        for event in &parsed.usage_events {
            let ts_ms: i64 = event
                .timestamp
                .parse()
                .context("parse cursor event timestamp")?;
            let ts = Utc
                .timestamp_millis_opt(ts_ms)
                .single()
                .context("cursor event timestamp out of range")?;
            let day = ts.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
            let model = event.model.clone().unwrap_or_else(|| "unknown".into());
            let entry = accum.entry((day, model)).or_default();
            entry.cost_usd += event.charged_cents.unwrap_or(0.0) / 100.0;
            entry.requests += 1;
            if let Some(tokens) = &event.token_usage {
                entry.input_tokens += tokens.input_tokens.unwrap_or(0);
                entry.output_tokens += tokens.output_tokens.unwrap_or(0);
            }
        }

        if parsed.pagination.as_ref().map(|p| p.has_next_page).unwrap_or(false) {
            page += 1;
        } else {
            break;
        }
    }
    Ok(())
}

async fn is_cloud_agents_key(client: &Client, api_key: &str) -> bool {
    client
        .get("https://api.cursor.com/v1/me")
        .bearer_auth(api_key)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

pub async fn test_cursor(api_key: &str) -> ProviderTestResult {
    let client = Client::new();
    match client
        .get("https://api.cursor.com/teams/members")
        .basic_auth(api_key, Some(""))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => ProviderTestResult {
            provider: Provider::Cursor,
            ok: true,
            message: "admin API key verified — team members accessible".into(),
            key_type_hint: Provider::Cursor.key_hint().into(),
            docs_url: Provider::Cursor.docs_url().into(),
        },
        Ok(resp) if resp.status().as_u16() == 401 => {
            let message = if is_cloud_agents_key(&client, api_key).await {
                "wrong key type — this key works for Cloud Agents (/v1/me) but not usage APIs. \
                 Create a new key at cursor.com/dashboard → API Keys with admin:* scope \
                 (Admin API requires Enterprise)"
                    .into()
            } else {
                "401 unauthorized — create an Admin API key with admin:* scope at \
                 cursor.com/dashboard → API Keys (Enterprise teams only)"
                    .into()
            };
            ProviderTestResult {
                provider: Provider::Cursor,
                ok: false,
                message,
                key_type_hint: Provider::Cursor.key_hint().into(),
                docs_url: Provider::Cursor.docs_url().into(),
            }
        }
        Ok(resp) if resp.status().as_u16() == 403 => ProviderTestResult {
            provider: Provider::Cursor,
            ok: false,
            message: "403 forbidden — Admin API requires a Team or Enterprise plan".into(),
            key_type_hint: Provider::Cursor.key_hint().into(),
            docs_url: Provider::Cursor.docs_url().into(),
        },
        Ok(resp) => ProviderTestResult {
            provider: Provider::Cursor,
            ok: false,
            message: format!("HTTP {}", resp.status()),
            key_type_hint: Provider::Cursor.key_hint().into(),
            docs_url: Provider::Cursor.docs_url().into(),
        },
        Err(e) => ProviderTestResult {
            provider: Provider::Cursor,
            ok: false,
            message: e.to_string(),
            key_type_hint: Provider::Cursor.key_hint().into(),
            docs_url: Provider::Cursor.docs_url().into(),
        },
    }
}

#[derive(Debug, Deserialize)]
struct UsageEventsResponse {
    #[serde(rename = "usageEvents")]
    usage_events: Vec<UsageEvent>,
    pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

#[derive(Debug, Deserialize)]
struct UsageEvent {
    timestamp: String,
    model: Option<String>,
    #[serde(rename = "chargedCents")]
    charged_cents: Option<f64>,
    #[serde(rename = "tokenUsage")]
    token_usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
struct TokenUsage {
    #[serde(rename = "inputTokens")]
    input_tokens: Option<u64>,
    #[serde(rename = "outputTokens")]
    output_tokens: Option<u64>,
}
