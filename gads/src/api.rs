use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

use crate::auth::Credentials;

/// Pin Google Ads API version in one place.
pub const API_VERSION: &str = "v24";
pub const BASE_URL: &str = "https://googleads.googleapis.com/v24";

pub struct AdsClient {
    http: reqwest::Client,
    creds: Credentials,
}

impl AdsClient {
    pub fn new(creds: Credentials) -> Self {
        Self {
            http: reqwest::Client::new(),
            creds,
        }
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.creds.access_token))
                .context("invalid access token header")?,
        );
        headers.insert(
            "developer-token",
            HeaderValue::from_str(&self.creds.developer_token)
                .context("invalid developer token header")?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(login) = &self.creds.login_customer_id {
            if !login.is_empty() {
                headers.insert(
                    "login-customer-id",
                    HeaderValue::from_str(&login.replace('-', ""))
                        .context("invalid login-customer-id")?,
                );
            }
        }
        Ok(headers)
    }

    pub async fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let mut url = format!("{BASE_URL}/{path}");
        if !params.is_empty() {
            let qs: Vec<String> = params
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| format!("{k}={}", urlencoding(v)))
                .collect();
            if !qs.is_empty() {
                url.push('?');
                url.push_str(&qs.join("&"));
            }
        }

        let res = self.http.get(&url).headers(self.headers()?).send().await?;
        parse_response(res).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{BASE_URL}/{path}");
        let res = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(body)
            .send()
            .await?;
        parse_response(res).await
    }

    pub async fn search_stream(&self, customer_id: &str, query: &str) -> Result<Value> {
        let path = format!("customers/{customer_id}/googleAds:searchStream");
        self.post(&path, &serde_json::json!({ "query": query }))
            .await
    }

    pub async fn mutate_resource(
        &self,
        customer_id: &str,
        resource: &str,
        body: &Value,
    ) -> Result<Value> {
        let path = format!("customers/{customer_id}/{resource}:mutate");
        self.post(&path, body).await
    }

    pub async fn mutate_google_ads(&self, customer_id: &str, body: &Value) -> Result<Value> {
        let path = format!("customers/{customer_id}/googleAds:mutate");
        self.post(&path, body).await
    }

    pub async fn list_accessible_customers(&self) -> Result<Value> {
        self.get("customers:listAccessibleCustomers", &[]).await
    }
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

async fn parse_response(res: reqwest::Response) -> Result<Value> {
    let status = res.status();
    let text = res.text().await?;
    let data: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
        serde_json::json!({ "rawResponse": text })
    });

    if status.is_success() {
        return Ok(data);
    }

    let msg = extract_error_message(&data).unwrap_or_else(|| format!("HTTP {status}"));
    bail!(msg)
}

fn extract_error_message(data: &Value) -> Option<String> {
    if let Some(arr) = data.as_array() {
        if let Some(first) = arr.first() {
            if let Some(msg) = first.pointer("/error/message").and_then(|v| v.as_str()) {
                return Some(msg.to_string());
            }
        }
    }
    data.pointer("/error/message")
        .and_then(|v| v.as_str())
        .map(String::from)
}
