use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;

use super::types::{Provider, ProviderTestResult, UsageBucket};

pub async fn fetch_anthropic(api_key: &str, days: i64) -> Result<Vec<UsageBucket>> {
    let client = Client::new();
    let end = Utc::now();
    let start = end - Duration::days(days);
    let mut all = Vec::new();
    let mut page: Option<String> = None;

    loop {
        let mut url = format!(
            "https://api.anthropic.com/v1/organizations/cost_report?\
            starting_at={}&ending_at={}&bucket_width=1d&group_by[]=description",
            urlencoding(start),
            urlencoding(end),
        );
        if let Some(ref p) = page {
            url = p.clone();
        }

        let resp = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .context("anthropic cost_report request")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("anthropic cost_report {status}: {text}");
        }

        let parsed: CostReportResponse =
            serde_json::from_str(&text).context("parse anthropic response")?;

        for bucket in &parsed.data {
            let day = DateTime::parse_from_rfc3339(&bucket.starting_at)
                .map(|d| d.with_timezone(&Utc))
                .with_context(|| format!("parse starting_at {}", bucket.starting_at))?;

            for result in &bucket.results {
                let amount_cents: f64 = result.amount.parse().unwrap_or(0.0);
                let cost_usd = amount_cents / 100.0;
                let model = result
                    .model
                    .clone()
                    .or_else(|| result.description.clone());

                all.push(UsageBucket {
                    provider: Provider::Anthropic,
                    bucket_start: day,
                    granularity: "day".into(),
                    model,
                    cost_usd,
                    input_tokens: None,
                    output_tokens: None,
                    request_count: None,
                });
            }
        }

        if parsed.has_more {
            page = parsed.next_page;
        } else {
            break;
        }
    }

    Ok(all)
}

pub async fn test_anthropic(api_key: &str) -> ProviderTestResult {
    if !api_key.starts_with("sk-ant-admin") {
        return ProviderTestResult {
            provider: Provider::Anthropic,
            ok: false,
            message: "key does not look like an admin key (expected sk-ant-admin01-...)".into(),
            key_type_hint: Provider::Anthropic.key_hint().into(),
            docs_url: Provider::Anthropic.docs_url().into(),
        };
    }

    let end = Utc::now();
    let start = end - Duration::days(2);
    let url = format!(
        "https://api.anthropic.com/v1/organizations/cost_report?\
        starting_at={}&ending_at={}&bucket_width=1d",
        urlencoding(start),
        urlencoding(end),
    );

    let client = Client::new();
    match client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => ProviderTestResult {
            provider: Provider::Anthropic,
            ok: true,
            message: "admin key verified — cost_report accessible".into(),
            key_type_hint: Provider::Anthropic.key_hint().into(),
            docs_url: Provider::Anthropic.docs_url().into(),
        },
        Ok(resp) => ProviderTestResult {
            provider: Provider::Anthropic,
            ok: false,
            message: format!("HTTP {}", resp.status()),
            key_type_hint: Provider::Anthropic.key_hint().into(),
            docs_url: Provider::Anthropic.docs_url().into(),
        },
        Err(e) => ProviderTestResult {
            provider: Provider::Anthropic,
            ok: false,
            message: e.to_string(),
            key_type_hint: Provider::Anthropic.key_hint().into(),
            docs_url: Provider::Anthropic.docs_url().into(),
        },
    }
}

fn urlencoding(dt: DateTime<Utc>) -> String {
    urlencoding::encode(&dt.to_rfc3339()).into_owned()
}

#[derive(Debug, Deserialize)]
struct CostReportResponse {
    data: Vec<CostBucket>,
    has_more: bool,
    next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostBucket {
    starting_at: String,
    results: Vec<CostResult>,
}

#[derive(Debug, Deserialize)]
struct CostResult {
    amount: String,
    model: Option<String>,
    description: Option<String>,
}
