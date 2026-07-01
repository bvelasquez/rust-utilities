use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::config::{default_credentials_path, legacy_credentials_path};

pub const OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/adwords";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub developer_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_customer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expiry: Option<String>,
}

pub fn save_credentials(path: &Path, creds: &Credentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(creds)? + "\n")?;
    Ok(())
}

fn is_token_expired(creds: &Credentials) -> bool {
    match (&creds.access_token, &creds.token_expiry) {
        (token, Some(expiry)) if !token.is_empty() => {
            chrono::DateTime::parse_from_rfc3339(expiry)
                .map(|t| chrono::Utc::now() >= t - chrono::Duration::seconds(60))
                .unwrap_or(true)
        }
        _ => true,
    }
}

fn can_refresh(creds: &Credentials) -> bool {
    creds.refresh_token.is_some() && creds.client_id.is_some() && creds.client_secret.is_some()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

async fn refresh_access_token(creds: &Credentials) -> Result<(String, String)> {
    let client = reqwest::Client::new();
    let res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", creds.client_id.as_deref().unwrap_or_default()),
            (
                "client_secret",
                creds.client_secret.as_deref().unwrap_or_default(),
            ),
            (
                "refresh_token",
                creds.refresh_token.as_deref().unwrap_or_default(),
            ),
        ])
        .send()
        .await?
        .json::<TokenResponse>()
        .await?;

    if let Some(err) = res.error {
        bail!(
            "token refresh failed: {}",
            res.error_description.unwrap_or(err)
        );
    }

    let expiry = chrono::Utc::now() + chrono::Duration::seconds(res.expires_in as i64);
    Ok((res.access_token, expiry.to_rfc3339()))
}

pub async fn load_credentials(explicit_path: Option<&Path>) -> Result<Credentials> {
    if let (Some(token), Some(dev)) = (
        std::env::var("GOOGLE_ADS_ACCESS_TOKEN")
            .ok()
            .or_else(|| std::env::var("GADS_ACCESS_TOKEN").ok()),
        std::env::var("GOOGLE_ADS_DEVELOPER_TOKEN")
            .ok()
            .or_else(|| std::env::var("GADS_DEVELOPER_TOKEN").ok()),
    ) {
        return Ok(Credentials {
            access_token: token,
            developer_token: dev,
            login_customer_id: std::env::var("GOOGLE_ADS_LOGIN_CUSTOMER_ID")
                .ok()
                .or_else(|| std::env::var("GADS_LOGIN_CUSTOMER_ID").ok()),
            client_id: None,
            client_secret: None,
            refresh_token: None,
            token_expiry: None,
        });
    }

    let path = explicit_path
        .map(Path::to_path_buf)
        .or_else(|| {
            let p = default_credentials_path();
            if p.is_file() {
                Some(p)
            } else {
                let legacy = legacy_credentials_path();
                if legacy.is_file() {
                    Some(legacy)
                } else {
                    None
                }
            }
        })
        .context(
            "no credentials found — run `gads auth login` or set GOOGLE_ADS_ACCESS_TOKEN + GOOGLE_ADS_DEVELOPER_TOKEN",
        )?;

    let mut creds: Credentials =
        serde_json::from_str(&fs::read_to_string(&path)?).context("parse credentials.json")?;

    if can_refresh(&creds) && is_token_expired(&creds) {
        let (access_token, token_expiry) = refresh_access_token(&creds).await?;
        creds.access_token = access_token;
        creds.token_expiry = Some(token_expiry);
        save_credentials(&path, &creds)?;
    }

    if creds.access_token.is_empty() || creds.developer_token.is_empty() {
        bail!("credentials missing access_token or developer_token — run `gads auth login`");
    }

    Ok(creds)
}

pub async fn exchange_auth_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<(String, String, String)> {
    let client = reqwest::Client::new();
    let res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?
        .json::<TokenResponse>()
        .await?;

    if let Some(err) = res.error {
        bail!(
            "token exchange failed: {}",
            res.error_description.unwrap_or(err)
        );
    }

    let refresh = res
        .refresh_token
        .context("no refresh_token — retry auth with prompt=consent")?;
    let expiry = chrono::Utc::now() + chrono::Duration::seconds(res.expires_in as i64);
    Ok((res.access_token, refresh, expiry.to_rfc3339()))
}

pub fn auth_url(client_id: &str, redirect_uri: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent",
        urlencoding_encode(client_id),
        urlencoding_encode(redirect_uri),
        urlencoding_encode(OAUTH_SCOPE),
    )
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

pub async fn run_local_oauth_server(port: u16, timeout: Duration) -> Result<(String, u16)> {
    let server = tiny_http::Server::http(format!("127.0.0.1:{port}"))
        .map_err(|e| anyhow::anyhow!("bind oauth server: {e}"))?;
    let actual_port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(port);

    let deadline = SystemTime::now() + timeout;
    loop {
        if SystemTime::now() > deadline {
            bail!("authorization timed out after {} seconds", timeout.as_secs());
        }
        if let Some(request) = server.recv_timeout(Duration::from_millis(500))? {
            let url = request.url().to_string();
            let parsed = url::Url::parse(&format!("http://127.0.0.1{url}"))
                .or_else(|_| url::Url::parse(&url))?;
            let code = parsed
                .query_pairs()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.to_string());
            let error = parsed
                .query_pairs()
                .find(|(k, _)| k == "error")
                .map(|(_, v)| v.to_string());

            let response = tiny_http::Response::from_string(
                "<html><body><h2>Authorization complete</h2><p>You can close this tab.</p></body></html>",
            )
            .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap());
            let _ = request.respond(response);

            if let Some(err) = error {
                bail!("authorization denied: {err}");
            }
            if let Some(c) = code {
                return Ok((c, actual_port));
            }
        }
    }
}

pub fn open_browser(url: &str) -> Result<()> {
    open::that(url).context("open browser")
}

pub fn auth_status(creds: &Credentials) -> serde_json::Value {
    serde_json::json!({
        "has_access_token": !creds.access_token.is_empty(),
        "has_refresh_token": creds.refresh_token.is_some(),
        "has_developer_token": !creds.developer_token.is_empty(),
        "login_customer_id": creds.login_customer_id,
        "token_expiry": creds.token_expiry,
        "token_expired": is_token_expired(creds),
    })
}
