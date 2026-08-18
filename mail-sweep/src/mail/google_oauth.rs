use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::config::AppContext;
use crate::secrets::{GoogleOAuthAccountTokens, SecretsFile};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Full Gmail IMAP/SMTP access (archive, move, delete, send).
pub const GMAIL_SCOPE: &str = "https://mail.google.com/";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

/// SASL XOAUTH2 initial client response (see Gmail XOAUTH2 protocol).
pub fn xoauth2_initial_response(email: &str, access_token: &str) -> String {
    format!("user={email}\x01auth=Bearer {access_token}\x01\x01")
}

pub fn google_oauth_configured(secrets: &SecretsFile) -> bool {
    secrets
        .google_oauth_client_id
        .as_deref()
        .is_some_and(|s| !s.is_empty())
        && secrets
            .google_oauth_client_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
}

pub async fn ensure_access_token(
    secrets_path: &Path,
    account_id: &str,
) -> Result<String> {
    let mut secrets = SecretsFile::load(secrets_path)?;
    let client_id = secrets
        .google_oauth_client_id
        .clone()
        .filter(|s| !s.is_empty())
        .context(
            "Google OAuth client id missing: run `mail-sweep secrets set-google-oauth --client-id ... --client-secret ...`",
        )?;
    let client_secret = secrets
        .google_oauth_client_secret
        .clone()
        .filter(|s| !s.is_empty())
        .context(
            "Google OAuth client secret missing: run `mail-sweep secrets set-google-oauth --client-id ... --client-secret ...`",
        )?;

    let tokens = secrets
        .google_oauth_tokens
        .get(account_id)
        .cloned()
        .with_context(|| {
            format!(
                "no Google OAuth tokens for account '{account_id}': run `mail-sweep accounts google-login --id {account_id}`"
            )
        })?;

    if let Some(access) = tokens.access_token.as_deref() {
        if token_still_valid(tokens.expires_at) {
            return Ok(access.to_string());
        }
    }

    let refreshed = refresh_access_token(&client_id, &client_secret, &tokens.refresh_token).await?;
    let expires_at = refreshed
        .expires_in
        .map(|secs| now_unix() + secs as i64 - 30);

    let entry = secrets
        .google_oauth_tokens
        .entry(account_id.to_string())
        .or_insert_with(|| GoogleOAuthAccountTokens {
            refresh_token: tokens.refresh_token.clone(),
            access_token: None,
            expires_at: None,
        });
    if let Some(rt) = refreshed.refresh_token {
        entry.refresh_token = rt;
    }
    entry.access_token = Some(refreshed.access_token.clone());
    entry.expires_at = expires_at;
    secrets.save(secrets_path)?;

    Ok(refreshed.access_token)
}

pub async fn access_token_for_account(ctx: &AppContext, account_id: &str) -> Result<String> {
    ensure_access_token(&ctx.secrets_path, account_id).await
}

pub async fn run_browser_login(
    secrets_path: &Path,
    account_id: &str,
    login_hint: Option<&str>,
) -> Result<()> {
    let mut secrets = SecretsFile::load(secrets_path)?;
    let client_id = secrets
        .google_oauth_client_id
        .clone()
        .filter(|s| !s.is_empty())
        .context("set Google OAuth client id/secret first (`mail-sweep secrets set-google-oauth`)")?;
    let client_secret = secrets
        .google_oauth_client_secret
        .clone()
        .filter(|s| !s.is_empty())
        .context("set Google OAuth client id/secret first (`mail-sweep secrets set-google-oauth`)")?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind local OAuth callback server")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = oauth_state();

    let mut auth_url = url_with_query(
        AUTH_URL,
        &[
            ("client_id", client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", GMAIL_SCOPE),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state.as_str()),
        ],
    );
    if let Some(hint) = login_hint.filter(|h| !h.is_empty()) {
        auth_url = url_with_query(
            &auth_url,
            &[("login_hint", hint), ("include_granted_scopes", "true")],
        );
    }

    eprintln!("Opening browser for Google sign-in…");
    eprintln!("If it does not open, visit:\n{auth_url}\n");
    open_browser(&auth_url)?;

    let (code, returned_state) = wait_for_callback(listener, port).await?;
    if returned_state != state {
        bail!("OAuth state mismatch — try again");
    }

    eprintln!("Exchanging authorization code with Google…");
    let token = exchange_code(&client_id, &client_secret, &redirect_uri, &code).await?;
    let existing_refresh = secrets
        .google_oauth_tokens
        .get(account_id)
        .map(|t| t.refresh_token.clone());
    let refresh = token
        .refresh_token
        .filter(|s| !s.is_empty())
        .or(existing_refresh)
        .with_context(|| {
            "Google did not return a refresh token; revoke app access at \
             https://myaccount.google.com/permissions and run google-login again \
             (use prompt=consent is already set)"
        })?;

    let expires_at = token
        .expires_in
        .map(|secs| now_unix() + secs as i64 - 30);

    secrets.google_oauth_tokens.insert(
        account_id.to_string(),
        GoogleOAuthAccountTokens {
            refresh_token: refresh,
            access_token: Some(token.access_token),
            expires_at,
        },
    );
    secrets.save(secrets_path)?;
    eprintln!("Saved Google OAuth tokens for account '{account_id}'.");
    Ok(())
}

async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("build HTTP client")?;
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .context("POST token endpoint")?;
    let status = resp.status();
    let body = resp.text().await.context("read token response")?;
    if !status.is_success() {
        bail!("Google token exchange failed ({status}): {body}");
    }
    eprintln!("Google token exchange OK.");
    serde_json::from_str(&body).context("parse token response")
}

async fn refresh_access_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("build HTTP client")?;
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .context("POST token refresh")?;
    let status = resp.status();
    let body = resp.text().await.context("read refresh response")?;
    if !status.is_success() {
        bail!("Google token refresh failed ({status}): {body}");
    }
    serde_json::from_str(&body).context("parse refresh response")
}

async fn wait_for_callback(listener: TcpListener, port: u16) -> Result<(String, String)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            bail!("timed out waiting for OAuth callback (10 minutes)");
        }
        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .context("timed out waiting for OAuth callback")?
            .context("accept OAuth callback connection")?;

        match read_callback_request(&mut stream).await? {
            CallbackRead::OAuth { code, state, error } => {
                send_callback_html(&mut stream, error.as_deref()).await?;
                if let Some(err) = error {
                    bail!("Google OAuth error: {err}");
                }
                let code = code.context("missing code in OAuth callback")?;
                eprintln!("Received OAuth callback on port {port}");
                return Ok((code, state.unwrap_or_default()));
            }
            CallbackRead::Ignore => {
                send_simple_response(&mut stream, 404, "Not found").await.ok();
                continue;
            }
        }
    }
}

enum CallbackRead {
    OAuth {
        code: Option<String>,
        state: Option<String>,
        error: Option<String>,
    },
    Ignore,
}

async fn read_callback_request(
    stream: &mut tokio::net::TcpStream,
) -> Result<CallbackRead> {
    let mut buf = vec![0u8; 16384];
    let n = stream
        .read(&mut buf)
        .await
        .context("read OAuth callback request")?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or_default();
    let Some(path) = first_line.split_whitespace().nth(1) else {
        return Ok(CallbackRead::Ignore);
    };
    if !path.starts_with("/callback") {
        return Ok(CallbackRead::Ignore);
    }
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    if query.is_empty() {
        return Ok(CallbackRead::Ignore);
    }
    let params = parse_query(query);
    Ok(CallbackRead::OAuth {
        code: params.get("code").cloned(),
        state: params.get("state").cloned(),
        error: params.get("error").cloned(),
    })
}

async fn send_callback_html(stream: &mut tokio::net::TcpStream, error: Option<&str>) -> Result<()> {
    let body = if let Some(err) = error {
        let desc = err;
        format!("OAuth failed: {desc}. You can close this tab.")
    } else {
        "Signed in — you can close this tab and return to mail-sweep.".into()
    };
    send_simple_response(stream, 200, &body).await
}

async fn send_simple_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    message: &str,
) -> Result<()> {
    let escaped = html_escape(message);
    let html = format!("<html><body><p>{escaped}</p></body></html>");
    let status_text = if status == 200 { "OK" } else { "Not Found" };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{html}",
        html.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .context("write OAuth callback response")?;
    stream.shutdown().await.ok();
    Ok(())
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        map.insert(
            percent_decode(k),
            percent_decode(v),
        );
    }
    map
}

fn percent_decode(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            // RFC 3986 query strings use %20; do not map '+' for OAuth codes.
            out.push('+');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn url_with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    let mut url = base.to_string();
    for (i, (k, v)) in pairs.iter().enumerate() {
        let sep = if i == 0 && !base.contains('?') { '?' } else { '&' };
        url.push(sep);
        url.push_str(&url_encode(k));
        url.push('=');
        url.push_str(&url_encode(v));
    }
    url
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn oauth_state() -> String {
    format!(
        "{:x}{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn token_still_valid(expires_at: Option<i64>) -> bool {
    expires_at.is_some_and(|exp| exp > now_unix())
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("open browser (macOS)")?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("open browser (xdg-open)")?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .context("open browser (Windows)")?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoauth2_format_matches_google() {
        let s = xoauth2_initial_response("me@gmail.com", "ya29.token");
        assert_eq!(s, "user=me@gmail.com\x01auth=Bearer ya29.token\x01\x01");
    }

    #[test]
    fn parses_callback_query_with_slashes_in_code() {
        let q = "code=4%2F0Aabc%2Fxyz&state=deadbeef";
        let params = parse_query(q);
        assert_eq!(params.get("code").map(|s| s.as_str()), Some("4/0Aabc/xyz"));
        assert_eq!(params.get("state").map(|s| s.as_str()), Some("deadbeef"));
    }
}
