use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::cli::Cli;
use crate::config::{load_config, resolve_config_path, ResolvedConfig};

pub mod agents;
pub mod config_cmd;
pub mod daemon;
pub mod projects;

pub struct AppContext {
    pub config_path: PathBuf,
    pub config: Option<ResolvedConfig>,
    pub api_base: String,
    pub json: bool,
}

impl AppContext {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let config_path = resolve_config_path(cli.config.as_deref())?;
        let config = if config_path.is_file() {
            Some(load_config(&config_path)?)
        } else {
            None
        };
        Ok(Self {
            config_path,
            config,
            api_base: cli.api.trim_end_matches('/').to_string(),
            json: cli.json,
        })
    }

    pub fn require_config(&self) -> Result<&ResolvedConfig> {
        self.config.as_ref().with_context(|| {
            format!(
                "config not found at {}. Run `pi-commander config init` first.",
                self.config_path.display()
            )
        })
    }
}

/// Thin HTTP client for the daemon API.
#[derive(Clone)]
pub struct ApiClient {
    pub base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base: impl Into<String>) -> Self {
        let token = std::env::var("PI_COMMANDER_API_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            token,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        // worker ids contain '#' which would be parsed as a URL fragment
        let escaped = path.replace('#', "%23");
        let mut rb = self.http.request(method, format!("{}{}", self.base, escaped));
        if let Some(t) = &self.token {
            rb = rb.bearer_auth(t);
        }
        rb
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        let res = self
            .req(reqwest::Method::GET, path)
            .send()
            .await
            .with_context(|| format!("GET {}", path))?;
        parse_response(res).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let res = self
            .req(reqwest::Method::POST, path)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        parse_response(res).await
    }

    pub async fn health(&self) -> bool {
        self.get("/health").await.is_ok()
    }
}

async fn parse_response(res: reqwest::Response) -> Result<Value> {
    let status = res.status();
    let v: Value = res
        .json()
        .await
        .context("daemon returned non-JSON response")?;
    if status.is_success() {
        let ok = v.get("success").and_then(|s| s.as_bool()).unwrap_or(true);
        if ok {
            Ok(v)
        } else {
            let err = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown daemon error");
            bail!("{err}");
        }
    } else {
        let fallback = status.to_string();
        let err = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or(&fallback);
        bail!("{err}");
    }
}

pub fn print_result(json: bool, command: &str, v: Value) -> Result<()> {
    if json {
        crate::output::Envelope::ok(command, v).print_json()
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
        Ok(())
    }
}

pub fn ensure_daemon(_api: &ApiClient, _config_path: &PathBuf) -> Result<()> {
    // Non-blocking check; watch.rs performs auto-start.
    Ok(())
}

/// Try to auto-start the daemon (used by watch when API is down).
pub fn start_daemon_detached(config_path: Option<&PathBuf>) -> Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().context("resolve current executable")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon");
    if let Some(cp) = config_path {
        cmd.arg("--config").arg(cp);
    }
    let log_dir = crate::config::config_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("daemon.log"))?;
    cmd.stdout(std::process::Stdio::from(log_file.try_clone()?))
        .stderr(std::process::Stdio::from(log_file))
        .stdin(std::process::Stdio::null());
    // detach: new session, no controlling tty -> survives the launching TUI
    // exiting and ignores terminal SIGHUP/SIGINT
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    Ok(cmd.spawn().context("spawn daemon")?)
}

