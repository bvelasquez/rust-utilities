use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::Cli;
use crate::secrets::{load_dotenv_secrets, SecretsFile};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    pub model: Option<String>,
    pub openrouter_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default = "default_poll_interval")]
    pub poll_interval: String,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_body_preview")]
    pub body_preview_chars: usize,
    /// Max messages to pull on a brand-new account (last_uid == 0). Recent UIDs only.
    #[serde(default = "default_initial_fetch_limit")]
    pub initial_fetch_limit: usize,
    /// Max messages when using `sync --full` / backfill.
    #[serde(default = "default_full_fetch_limit")]
    pub full_fetch_limit: usize,
    #[serde(default)]
    pub auto_process: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            poll_interval: default_poll_interval(),
            batch_size: default_batch_size(),
            body_preview_chars: default_body_preview(),
            initial_fetch_limit: default_initial_fetch_limit(),
            full_fetch_limit: default_full_fetch_limit(),
            auto_process: false,
        }
    }
}

fn default_poll_interval() -> String {
    "5m".into()
}

fn default_batch_size() -> usize {
    25 // sender groups per LLM call, not individual messages
}

fn default_body_preview() -> usize {
    500
}

fn default_initial_fetch_limit() -> usize {
    50
}

fn default_full_fetch_limit() -> usize {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default)]
    pub allow_delete: bool,
    /// Planned messages below this confidence appear in Review (legacy name; usually
    /// matches `auto_apply_min_confidence`).
    #[serde(default = "default_review_threshold")]
    pub require_review_above: f32,
    /// AUTO applies only safe actions (archive/flag/keep/…) at or above this score.
    /// Also the floor for saving LLM patterns as durable rules.
    #[serde(default = "default_auto_apply_confidence")]
    pub auto_apply_min_confidence: f32,
    /// LLM suggestions below this are category-only (stay in Triage); at/above are planned.
    #[serde(default = "default_plan_min_confidence")]
    pub plan_min_confidence: f32,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            allow_delete: false,
            require_review_above: default_review_threshold(),
            auto_apply_min_confidence: default_auto_apply_confidence(),
            plan_min_confidence: default_plan_min_confidence(),
        }
    }
}

impl SafetyConfig {
    /// Review shows planned mail AUTO will not apply (below auto-apply confidence).
    pub fn review_threshold(&self) -> f32 {
        self.auto_apply_min_confidence.clamp(0.0, 1.0)
    }
}

fn default_review_threshold() -> f32 {
    0.88
}

fn default_auto_apply_confidence() -> f32 {
    0.88
}

fn default_plan_min_confidence() -> f32 {
    0.55
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    pub email: String,
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    /// Optional inline password in config.toml (prefer secrets.toml or .env)
    pub password: Option<String>,
    #[serde(default = "default_inbox")]
    pub inbox_folder: String,
    #[serde(default = "default_archive_folder")]
    pub archive_folder: String,
    #[serde(default = "default_spam_folder")]
    pub spam_folder: String,
}

fn default_imap_port() -> u16 {
    993
}

fn default_smtp_port() -> u16 {
    587
}

fn default_inbox() -> String {
    "INBOX".into()
}

fn default_archive_folder() -> String {
    "[Gmail]/All Mail".into()
}

fn default_spam_folder() -> String {
    "[Gmail]/Spam".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    pub id: Option<String>,
    pub r#match: String,
    pub category: Option<String>,
    pub action: String,
    #[serde(default)]
    pub priority: Option<u8>,
    pub target_folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub rules: Vec<RuleConfig>,
}

pub struct AppContext {
    pub config_path: PathBuf,
    pub secrets_path: PathBuf,
    pub data_dir: PathBuf,
    pub config: AppConfig,
    secrets: SecretsFile,
}

impl AppContext {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let config_path = cli
            .config
            .clone()
            .or_else(default_config_path)
            .context("could not resolve config path")?;

        let secrets_path = SecretsFile::path_for_config(&config_path);
        let data_dir = cli
            .data_dir
            .clone()
            .or_else(default_data_dir)
            .context("could not resolve data directory")?;

        let mut config = if config_path.exists() {
            load_config(&config_path)?
        } else {
            AppConfig::default()
        };

        let mut secrets = SecretsFile::load(&secrets_path)?;
        secrets.merge_from(&load_dotenv_secrets());

        apply_secrets(&mut config, &secrets);

        if let Some(k) = &cli.openrouter_key {
            config.llm.openrouter_api_key = Some(k.clone());
        }
        if let Some(m) = &cli.llm_model {
            config.llm.model = Some(m.clone());
        }

        Ok(Self {
            config_path,
            secrets_path,
            data_dir,
            config,
            secrets,
        })
    }

    pub fn reload_config(&mut self) -> Result<()> {
        if self.config_path.exists() {
            self.config = load_config(&self.config_path)?;
        }
        self.secrets = SecretsFile::load(&self.secrets_path)?;
        self.secrets.merge_from(&load_dotenv_secrets());
        apply_secrets(&mut self.config, &self.secrets);
        Ok(())
    }

    pub fn llm_api_key(&self) -> Result<&str> {
        self.config
            .llm
            .openrouter_api_key
            .as_deref()
            .context(
                "OpenRouter API key required: run `mail-sweep secrets set-openrouter-key --key ...` \
                 or add openrouter_api_key to ~/.config/mail-sweep/secrets.toml or .env",
            )
    }

    pub fn llm_model(&self) -> String {
        self.config
            .llm
            .model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o-mini".into())
    }

    pub fn account_by_id(&self, id: &str) -> Result<&AccountConfig> {
        self.config
            .accounts
            .iter()
            .find(|a| a.id == id)
            .with_context(|| format!("unknown account id: {id}"))
    }

    pub fn resolve_password(&self, account: &AccountConfig) -> Result<String> {
        if let Some(p) = &account.password {
            if !p.is_empty() {
                return Ok(p.clone());
            }
        }
        if let Some(p) = self.secrets.account_password(&account.id) {
            if !p.is_empty() {
                return Ok(p.to_string());
            }
        }
        anyhow::bail!(
            "password for account '{}' not found: run `mail-sweep secrets set-account --id {} --password ...`",
            account.id,
            account.id
        )
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("mail-sweep.db")
    }

    pub fn save_config(&self) -> Result<()> {
        save_config_file(&self.config_path, &self.config)
    }

    pub fn save_secrets(&self) -> Result<()> {
        self.secrets.save(&self.secrets_path)
    }

    pub fn set_openrouter_key(&mut self, key: String) -> Result<()> {
        self.secrets.openrouter_api_key = Some(key.clone());
        self.config.llm.openrouter_api_key = Some(key);
        self.save_secrets()
    }

    pub fn set_llm_model(&mut self, model: String) -> Result<()> {
        self.secrets.llm_model = Some(model.clone());
        self.config.llm.model = Some(model);
        self.save_secrets()
    }

    pub fn set_account_password(&mut self, account_id: &str, password: String) -> Result<()> {
        if !self
            .config
            .accounts
            .iter()
            .any(|a| a.id == account_id)
        {
            anyhow::bail!("unknown account id: {account_id}");
        }
        self.secrets
            .accounts
            .insert(account_id.to_string(), password.clone());
        if let Some(account) = self.config.accounts.iter_mut().find(|a| a.id == account_id) {
            account.password = Some(password);
        }
        self.save_secrets()
    }

    pub fn secrets_status(&self) -> SecretsStatus {
        SecretsStatus {
            openrouter_key_set: self.config.llm.openrouter_api_key.is_some(),
            llm_model: self.config.llm.model.clone(),
            accounts: self
                .config
                .accounts
                .iter()
                .map(|a| AccountSecretStatus {
                    id: a.id.clone(),
                    password_set: self.resolve_password(a).is_ok(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretsStatus {
    pub openrouter_key_set: bool,
    pub llm_model: Option<String>,
    pub accounts: Vec<AccountSecretStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountSecretStatus {
    pub id: String,
    pub password_set: bool,
}

fn apply_secrets(config: &mut AppConfig, secrets: &SecretsFile) {
    if let Some(key) = &secrets.openrouter_api_key {
        config.llm.openrouter_api_key = Some(key.clone());
    }
    if let Some(model) = &secrets.llm_model {
        config.llm.model = Some(model.clone());
    }
    for account in &mut config.accounts {
        if let Some(password) = secrets.account_password(&account.id) {
            account.password = Some(password.to_string());
        }
    }
}

pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("mail-sweep").join("config.toml"))
}

pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("mail-sweep"))
}

pub fn load_config(path: &PathBuf) -> Result<AppConfig> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let cfg: AppConfig = toml::from_str(&raw).context("parse config.toml")?;
    Ok(cfg)
}

pub fn save_config_file(path: &PathBuf, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(config).context("serialize config.toml")?;
    fs::write(path, raw).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn gmail_account(id: &str, email: &str) -> AccountConfig {
    AccountConfig {
        id: id.into(),
        email: email.into(),
        imap_host: "imap.gmail.com".into(),
        imap_port: 993,
        smtp_host: "smtp.gmail.com".into(),
        smtp_port: 587,
        password: None,
        inbox_folder: "INBOX".into(),
        archive_folder: "[Gmail]/All Mail".into(),
        spam_folder: "[Gmail]/Spam".into(),
    }
}

/// iCloud Mail (Apple) — requires an app-specific password from appleid.apple.com.
pub fn icloud_account(id: &str, email: &str) -> AccountConfig {
    AccountConfig {
        id: id.into(),
        email: email.into(),
        imap_host: "imap.mail.me.com".into(),
        imap_port: 993,
        smtp_host: "smtp.mail.me.com".into(),
        smtp_port: 587,
        password: None,
        inbox_folder: "INBOX".into(),
        archive_folder: "Archive".into(),
        spam_folder: "Junk".into(),
    }
}
