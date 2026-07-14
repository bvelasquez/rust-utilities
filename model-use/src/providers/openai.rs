use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;

use super::types::{Provider, ProviderTestResult, UsageBucket};

pub async fn fetch_openai(api_key: &str, days: i64) -> Result<Vec<UsageBucket>> {
    let client = Client::new();
    let end = Utc::now();
    let start = end - Duration::days(days);
    let start_time = start.timestamp();
    let end_time = end.timestamp();
    let mut all = Vec::new();
    let mut page: Option<String> = None;

    loop {
        let mut url = format!(
            "https://api.openai.com/v1/organization/costs?\
            start_time={start_time}&end_time={end_time}&bucket_width=1d&group_by=line_item&limit=180"
        );
        if let Some(ref p) = page {
            url = p.clone();
        }

        let resp = client
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await
            .context("openai costs request")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("openai costs {status}: {text}");
        }

        let parsed: CostsPage = serde_json::from_str(&text).context("parse openai response")?;

        for bucket in &parsed.data {
            let day = DateTime::from_timestamp(bucket.start_time, 0)
                .unwrap_or(start)
                .with_timezone(&Utc);

            for result in &bucket.results {
                let cost_usd = result.amount.value;
                let model = result
                    .line_item
                    .as_ref()
                    .map(|li| li.split(',').next().unwrap_or(li).trim().to_string());

                all.push(UsageBucket {
                    provider: Provider::Openai,
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

pub async fn test_openai(api_key: &str) -> ProviderTestResult {
    let end = Utc::now();
    let start = end - Duration::days(2);
    let url = format!(
        "https://api.openai.com/v1/organization/costs?\
        start_time={}&end_time={}&bucket_width=1d&limit=1",
        start.timestamp(),
        end.timestamp(),
    );

    let client = Client::new();
    match client.get(&url).bearer_auth(api_key).send().await {
        Ok(resp) if resp.status().is_success() => ProviderTestResult {
            provider: Provider::Openai,
            ok: true,
            message: "admin key verified — organization costs API accessible".into(),
            key_type_hint: Provider::Openai.key_hint().into(),
            docs_url: Provider::Openai.docs_url().into(),
        },
        Ok(resp) if resp.status().as_u16() == 401 => ProviderTestResult {
            provider: Provider::Openai,
            ok: false,
            message: "401 unauthorized — use an organization admin API key".into(),
            key_type_hint: Provider::Openai.key_hint().into(),
            docs_url: Provider::Openai.docs_url().into(),
        },
        Ok(resp) => ProviderTestResult {
            provider: Provider::Openai,
            ok: false,
            message: format!("HTTP {}", resp.status()),
            key_type_hint: Provider::Openai.key_hint().into(),
            docs_url: Provider::Openai.docs_url().into(),
        },
        Err(e) => ProviderTestResult {
            provider: Provider::Openai,
            ok: false,
            message: e.to_string(),
            key_type_hint: Provider::Openai.key_hint().into(),
            docs_url: Provider::Openai.docs_url().into(),
        },
    }
}

#[derive(Debug, Deserialize)]
struct CostsPage {
    data: Vec<CostBucket>,
    has_more: bool,
    next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostBucket {
    start_time: i64,
    results: Vec<CostResult>,
}

#[derive(Debug, Deserialize)]
struct CostResult {
    amount: Amount,
    line_item: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Amount {
    value: f64,
}
