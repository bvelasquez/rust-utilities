//! JSON configuration, secret references, and validation.
//!
//! Secrets are stored as strings in the file:
//! - `secret:NAME` reads `secrets.json` beside the config (mode `0600`)
//! - `env:VAR` reads an environment variable
//! - `file:/path` or `file:~/path` reads a trimmed file
//! - any other value is a literal (file mode `0600` is enforced on write)

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use airouter_core::{Modality, ModelConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid config: {0}")]
    Parse(String),
    #[error("config already exists at {0} (pass --force to replace it)")]
    Exists(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Openai,
    OpenaiCompat,
    Anthropic,
    Gemini,
}

impl ProviderType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::OpenaiCompat => "openai_compat",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ApiKeyConfig {
    Token(String),
    Labeled { key: String, label: String },
}

impl ApiKeyConfig {
    pub fn reference(&self) -> &str {
        match self {
            Self::Token(s) => s,
            Self::Labeled { key, .. } => key,
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Token(_) => None,
            Self::Labeled { label, .. } => Some(label),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RetryConfig {
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    #[serde(default = "default_backoff")]
    pub backoff_ms: u64,
}

fn default_retries() -> u32 {
    2
}
fn default_backoff() -> u64 {
    200
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_retries(),
            backoff_ms: default_backoff(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_admin_listen")]
    pub admin_listen: String,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyConfig>,
    /// Override for the SQLite directory. Default is `$XDG_STATE_HOME/airouter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub retry: RetryConfig,
}

fn default_listen() -> String {
    "0.0.0.0:8787".into()
}
fn default_admin_listen() -> String {
    "127.0.0.1:8788".into()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            admin_listen: default_admin_listen(),
            api_keys: Vec::new(),
            data_dir: None,
            retry: RetryConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFile {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub routing: airouter_core::RoutingConfig,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            providers: BTreeMap::new(),
            models: Vec::new(),
            aliases: BTreeMap::new(),
            routing: airouter_core::RoutingConfig::default(),
        }
    }
}

/// Provider after secret references are resolved. The key is never logged.
#[derive(Clone)]
pub struct ResolvedProvider {
    pub name: String,
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Human description of where the key comes from, without the secret.
    pub key_status: String,
    pub headers: BTreeMap<String, String>,
    pub timeout: std::time::Duration,
    pub enabled: bool,
}

impl std::fmt::Debug for ResolvedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedProvider")
            .field("name", &self.name)
            .field("provider_type", &self.provider_type)
            .field("base_url", &self.base_url)
            .field("key_present", &self.api_key.is_some())
            .field("key_status", &self.key_status)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[derive(Clone)]
pub struct ResolvedKey {
    pub key: String,
    pub label: String,
}

#[derive(Clone)]
pub struct ResolvedConfig {
    pub server: ServerConfig,
    pub providers: BTreeMap<String, ResolvedProvider>,
    pub keys: Vec<ResolvedKey>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn default_config_dir() -> PathBuf {
    if let Some(s) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(s).join("airouter");
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/airouter")
}

pub fn default_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("AIROUTER_CONFIG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_config_dir().join("config.json")
}

pub fn default_state_dir(server: &ServerConfig) -> PathBuf {
    if let Some(dir) = &server.data_dir {
        return expand_tilde(dir);
    }
    if let Some(s) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(s).join("airouter");
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/airouter")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

pub fn load_file(path: &Path) -> Result<ConfigFile, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|e| ConfigError::Parse(e.to_string()))
}

pub fn save_file(path: &Path, cfg: &ConfigFile) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let pretty =
        serde_json::to_string_pretty(cfg).map_err(|e| ConfigError::Parse(e.to_string()))?;
    atomic_write(path, pretty.as_bytes())?;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|source| ConfigError::Write {
            path: tmp.display().to_string(),
            source,
        })?;
        f.write_all(bytes).map_err(|source| ConfigError::Write {
            path: tmp.display().to_string(),
            source,
        })?;
        f.write_all(b"\n").map_err(|source| ConfigError::Write {
            path: tmp.display().to_string(),
            source,
        })?;
    }
    set_owner_only(&tmp);
    fs::rename(&tmp, path).map_err(|source| ConfigError::Write {
        path: path.display().to_string(),
        source,
    })?;
    set_owner_only(path);
    Ok(())
}

pub fn set_owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    let _ = path;
}

pub fn json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(ConfigFile))
        .unwrap_or_else(|_| serde_json::json!({}))
}

pub fn secrets_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("secrets.json")
}

pub fn load_secrets(path: &Path) -> Result<BTreeMap<String, String>, ConfigError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|e| ConfigError::Parse(e.to_string()))
}

pub fn save_secrets(path: &Path, secrets: &BTreeMap<String, String>) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let pretty =
        serde_json::to_string_pretty(secrets).map_err(|e| ConfigError::Parse(e.to_string()))?;
    atomic_write(path, pretty.as_bytes())
}

pub fn upsert_secret(config_path: &Path, name: &str, value: &str) -> Result<(), ConfigError> {
    let path = secrets_path(config_path);
    let mut secrets = load_secrets(&path)?;
    secrets.insert(name.to_string(), value.trim().to_string());
    save_secrets(&path, &secrets)
}

pub fn remove_secret(config_path: &Path, name: &str) -> Result<bool, ConfigError> {
    let path = secrets_path(config_path);
    let mut secrets = load_secrets(&path)?;
    let removed = secrets.remove(name).is_some();
    if removed {
        save_secrets(&path, &secrets)?;
    }
    Ok(removed)
}

/// Resolve secret references. Missing secrets become `None` plus a status string.
/// `secrets` is the contents of `secrets.json`; pass an empty map when there is no file.
pub fn resolve(cfg: &ConfigFile) -> Result<ResolvedConfig, ConfigError> {
    resolve_with(cfg, &BTreeMap::new())
}

pub fn resolve_for(config_path: &Path, cfg: &ConfigFile) -> Result<ResolvedConfig, ConfigError> {
    let secrets = load_secrets(&secrets_path(config_path))?;
    resolve_with(cfg, &secrets)
}

pub fn resolve_with(
    cfg: &ConfigFile,
    secrets: &BTreeMap<String, String>,
) -> Result<ResolvedConfig, ConfigError> {
    let mut providers = BTreeMap::new();
    for (name, p) in &cfg.providers {
        let (api_key, key_status) = resolve_provider_key(name, p.api_key.as_deref(), secrets);
        let base_url = default_base_url(p.provider_type, p.base_url.as_deref());
        let timeout = std::time::Duration::from_secs(p.timeout_secs.unwrap_or(180));
        providers.insert(
            name.clone(),
            ResolvedProvider {
                name: name.clone(),
                provider_type: p.provider_type,
                base_url,
                api_key,
                key_status,
                headers: p.headers.clone(),
                timeout,
                enabled: p.enabled,
            },
        );
    }

    let mut keys = Vec::new();
    for (i, entry) in cfg.server.api_keys.iter().enumerate() {
        let (value, status) = resolve_secret(entry.reference(), secrets);
        if let Some(key) = value {
            let label = entry
                .label()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("key-{}", i + 1));
            keys.push(ResolvedKey { key, label });
        } else if !status.contains("missing") && !status.contains("not set") {
            // literal empty already handled; keep going
        }
    }

    Ok(ResolvedConfig {
        server: cfg.server.clone(),
        providers,
        keys,
    })
}

fn resolve_provider_key(
    provider: &str,
    spec: Option<&str>,
    secrets: &BTreeMap<String, String>,
) -> (Option<String>, String) {
    let Some(spec) = spec else {
        return secret_or_env(provider, secrets);
    };
    let (value, status) = resolve_secret(spec, secrets);
    if value.is_some() {
        return (value, status);
    }
    if spec.starts_with("env:") || spec.starts_with("secret:") {
        if let (Some(value), status) = secret_or_env(provider, secrets) {
            return (Some(value), status);
        }
    }
    (None, status)
}

/// File entry wins. A conventional environment variable is only a fallback so an
/// existing `export` keeps working until the key is stored with `airouter secret set`.
fn secret_or_env(provider: &str, secrets: &BTreeMap<String, String>) -> (Option<String>, String) {
    if let Some(value) = secrets
        .get(provider)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return (Some(value.to_string()), format!("secret:{provider}"));
    }
    if let Some(var) = conventional_env(provider) {
        if let Ok(value) = std::env::var(var) {
            if !value.is_empty() {
                return (Some(value), format!("env:{var}"));
            }
        }
    }
    (None, format!("secret:{provider} (missing)"))
}

fn conventional_env(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "gemini" => Some("GEMINI_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "xai" => Some("XAI_API_KEY"),
        "deepinfra" => Some("DEEPINFRA_API_KEY"),
        "cheaperinference" => Some("CHEAPERINFERENCE_API_KEY"),
        _ => None,
    }
}

/// Default OpenAI-compatible base URL for a provider name. `None` means the caller must pass one.
pub fn known_compat_base_url(name: &str) -> Option<&'static str> {
    match name {
        "groq" => Some("https://api.groq.com/openai/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "xai" => Some("https://api.x.ai/v1"),
        "deepinfra" => Some("https://api.deepinfra.com/v1/openai"),
        "cheaperinference" => Some("https://api.cheaperinference.com/v1"),
        _ => None,
    }
}

pub fn resolve_secret(spec: &str, secrets: &BTreeMap<String, String>) -> (Option<String>, String) {
    if let Some(name) = spec.strip_prefix("secret:") {
        match secrets
            .get(name)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            Some(value) => (Some(value.to_string()), format!("secret:{name}")),
            None => (None, format!("secret:{name} (missing)")),
        }
    } else if let Some(var) = spec.strip_prefix("env:") {
        match std::env::var(var) {
            Ok(v) if !v.is_empty() => (Some(v), format!("env:{var}")),
            _ => (None, format!("env:{var} (missing)")),
        }
    } else if let Some(path) = spec.strip_prefix("file:") {
        let full = expand_tilde(path);
        match fs::read_to_string(&full) {
            Ok(v) => {
                let v = v.trim().to_string();
                if v.is_empty() {
                    (None, format!("file:{} (empty)", full.display()))
                } else {
                    (Some(v), format!("file:{}", full.display()))
                }
            }
            Err(_) => (None, format!("file:{} (missing)", full.display())),
        }
    } else if spec.is_empty() {
        (None, "empty".into())
    } else {
        (Some(spec.to_string()), "literal".into())
    }
}

pub fn default_base_url(kind: ProviderType, configured: Option<&str>) -> String {
    if let Some(url) = configured {
        if !url.is_empty() {
            return url.trim_end_matches('/').to_string();
        }
    }
    match kind {
        ProviderType::Openai | ProviderType::OpenaiCompat => "https://api.openai.com/v1".into(),
        ProviderType::Anthropic => "https://api.anthropic.com".into(),
        ProviderType::Gemini => "https://generativelanguage.googleapis.com/v1beta".into(),
    }
}

pub fn validate(cfg: &ConfigFile) -> Report {
    validate_with(cfg, &BTreeMap::new())
}

pub fn validate_for(config_path: &Path, cfg: &ConfigFile) -> Result<Report, ConfigError> {
    let secrets = load_secrets(&secrets_path(config_path))?;
    Ok(validate_with(cfg, &secrets))
}

pub fn validate_with(cfg: &ConfigFile, secrets: &BTreeMap<String, String>) -> Report {
    let mut report = Report::default();
    if let Err(msg) = check_bind("server.listen", &cfg.server.listen, false) {
        report.errors.push(msg);
    }
    if let Err(msg) = check_bind("server.admin_listen", &cfg.server.admin_listen, true) {
        report.errors.push(msg);
    }

    let loopback = is_loopback_bind(&cfg.server.listen);
    let resolved = resolve_with(cfg, secrets).ok();
    let key_count = resolved.as_ref().map(|r| r.keys.len()).unwrap_or(0);
    if !loopback && key_count == 0 {
        report.errors.push(
            "refusing a non-loopback listen address without at least one resolvable server.api_keys entry"
                .into(),
        );
    }
    if cfg.server.api_keys.is_empty() {
        report.warnings.push(
            "no server.api_keys configured; the public API accepts any caller who can reach it"
                .into(),
        );
    } else if key_count == 0 {
        report
            .warnings
            .push("server.api_keys are set but none currently resolve".into());
    }

    let mut seen_models = BTreeMap::<String, usize>::new();
    for model in &cfg.models {
        *seen_models.entry(model.id.clone()).or_default() += 1;
        if model.provider.is_empty() || model.upstream.is_empty() || model.id.is_empty() {
            report.errors.push(format!(
                "model '{}' needs id, provider, and upstream",
                model.id
            ));
        }
        if !cfg.providers.contains_key(&model.provider) {
            report.warnings.push(format!(
                "model '{}' references unknown provider '{}'",
                model.id, model.provider
            ));
        }
        if model.modality == Modality::Video {
            report.warnings.push(format!(
                "model '{}' is modality video, which has no public route in v1",
                model.id
            ));
        }
    }
    for (id, n) in seen_models {
        if n > 1 {
            report.errors.push(format!("duplicate model id '{id}'"));
        }
    }

    for (name, p) in &cfg.providers {
        if p.provider_type == ProviderType::OpenaiCompat
            && p.base_url.as_deref().unwrap_or("").is_empty()
        {
            report.errors.push(format!(
                "provider '{name}' is openai_compat and needs base_url"
            ));
        }
        if let Some(resolved_map) = resolved.as_ref().map(|r| &r.providers) {
            if let Some(rp) = resolved_map.get(name) {
                if p.enabled && rp.api_key.is_none() {
                    report.warnings.push(format!(
                        "provider '{name}' has no API key ({})",
                        rp.key_status
                    ));
                }
            }
        }
        if rp_literal(p) {
            report.warnings.push(format!(
                "provider '{name}' stores a literal API key; prefer env: or file: references"
            ));
        }
    }

    for (alias, target) in &cfg.aliases {
        if alias == "auto" || alias.starts_with("auto/") {
            report.errors.push(format!(
                "alias '{alias}' is reserved for routing profiles; configure routing.profiles instead"
            ));
        }
        let resolved_target = resolve_alias_target(&cfg.aliases, target);
        if !cfg.models.iter().any(|m| m.id == resolved_target) {
            report.warnings.push(format!(
                "alias '{alias}' points at '{target}', which is not a configured model"
            ));
        }
    }

    validate_routing(cfg, secrets, &mut report);

    if !cfg.models.iter().any(|m| m.enabled) {
        report
            .warnings
            .push("no models are enabled; GET /v1/models will be empty until you add some".into());
    }

    report
}

fn validate_routing(cfg: &ConfigFile, secrets: &BTreeMap<String, String>, report: &mut Report) {
    let routing = &cfg.routing;
    for (name, profile) in &routing.profiles {
        if name.is_empty() || name.contains('/') || name == "auto" {
            report.errors.push(format!(
                "routing profile '{name}' must be a single path segment"
            ));
        }
        if profile.safe_model.is_empty() {
            report
                .warnings
                .push(format!("routing profile '{name}' has no safe default"));
        } else if !cfg.models.iter().any(|m| m.id == profile.safe_model) {
            report.errors.push(format!(
                "routing profile '{name}' safe_model '{}' is not a configured model",
                profile.safe_model
            ));
        }
        for tier in &profile.tiers {
            for id in &tier.models {
                if !cfg.models.iter().any(|m| m.id == id.as_str()) {
                    report.warnings.push(format!(
                        "routing profile '{name}' tier lists '{id}', which is not a configured model"
                    ));
                }
            }
        }
        if profile.confidence_floor < 0.0 || profile.confidence_floor > 1.0 {
            report.errors.push(format!(
                "routing profile '{name}' confidence_floor must be between 0 and 1"
            ));
        }
    }
    if routing.enabled
        && !routing.profiles.is_empty()
        && !routing.profiles.contains_key(&routing.default_profile)
    {
        report.warnings.push(format!(
            "routing.default_profile '{}' is not defined; a bare 'auto' model name will fail",
            routing.default_profile
        ));
    }
    if routing.signals.provider == airouter_core::SignalKind::Jev {
        match routing.signals.jev.api_key.as_deref() {
            None => report.warnings.push(
                "routing.signals.provider is jev but routing.signals.jev.api_key is unset".into(),
            ),
            Some(spec) => {
                let (key, status) = resolve_secret(spec, secrets);
                if key.is_none() {
                    report.warnings.push(format!(
                        "routing.signals.jev.api_key does not resolve ({status})"
                    ));
                }
                if !spec.starts_with("env:")
                    && !spec.starts_with("file:")
                    && !spec.starts_with("secret:")
                    && !spec.is_empty()
                {
                    report.warnings.push(
                        "routing.signals.jev.api_key is a literal; prefer env:TYPESAFE_API_KEY"
                            .into(),
                    );
                }
            }
        }
        let any_opt_in = routing
            .profiles
            .values()
            .any(|p| !p.hosted_signal_clients.is_empty());
        if !any_opt_in {
            report.warnings.push(
                "jev is configured but no profile lists hosted_signal_clients; prompts stay local"
                    .into(),
            );
        }
    }
}

fn rp_literal(p: &ProviderConfig) -> bool {
    matches!(&p.api_key, Some(s) if !s.starts_with("env:") && !s.starts_with("file:") && !s.starts_with("secret:") && !s.is_empty())
}

fn resolve_alias_target(aliases: &BTreeMap<String, String>, target: &str) -> String {
    let mut current = target.to_string();
    for _ in 0..4 {
        match aliases.get(&current) {
            Some(next) if next != &current => current = next.clone(),
            _ => break,
        }
    }
    current
}

fn check_bind(field: &str, value: &str, admin: bool) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} is empty"));
    }
    if admin && !is_loopback_bind(value) {
        return Err(format!(
            "{field} must be a loopback address (the admin API is unauthenticated)"
        ));
    }
    Ok(())
}

pub fn is_loopback_bind(addr: &str) -> bool {
    let addr = addr.trim();
    if let Ok(sa) = addr.parse::<SocketAddr>() {
        return sa.ip().is_loopback();
    }
    let host = if let Some(rest) = addr.strip_prefix('[') {
        rest.split(']').next().unwrap_or(addr)
    } else {
        addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub fn file_mode(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return fs::metadata(path)
            .ok()
            .map(|m| m.permissions().mode() & 0o777);
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

pub fn template_config(token_file: &Path) -> ConfigFile {
    let mut providers = BTreeMap::new();
    providers.insert(
        "openai".into(),
        ProviderConfig {
            provider_type: ProviderType::Openai,
            api_key: Some("env:OPENAI_API_KEY".into()),
            base_url: None,
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "anthropic".into(),
        ProviderConfig {
            provider_type: ProviderType::Anthropic,
            api_key: Some("env:ANTHROPIC_API_KEY".into()),
            base_url: None,
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "gemini".into(),
        ProviderConfig {
            provider_type: ProviderType::Gemini,
            api_key: Some("env:GEMINI_API_KEY".into()),
            base_url: None,
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "groq".into(),
        ProviderConfig {
            provider_type: ProviderType::OpenaiCompat,
            api_key: Some("env:GROQ_API_KEY".into()),
            base_url: Some("https://api.groq.com/openai/v1".into()),
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "deepseek".into(),
        ProviderConfig {
            provider_type: ProviderType::OpenaiCompat,
            api_key: Some("env:DEEPSEEK_API_KEY".into()),
            base_url: Some("https://api.deepseek.com/v1".into()),
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "xai".into(),
        ProviderConfig {
            provider_type: ProviderType::OpenaiCompat,
            api_key: Some("env:XAI_API_KEY".into()),
            base_url: Some("https://api.x.ai/v1".into()),
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "deepinfra".into(),
        ProviderConfig {
            provider_type: ProviderType::OpenaiCompat,
            api_key: Some("env:DEEPINFRA_API_KEY".into()),
            base_url: Some("https://api.deepinfra.com/v1/openai".into()),
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    providers.insert(
        "cheaperinference".into(),
        ProviderConfig {
            provider_type: ProviderType::OpenaiCompat,
            api_key: Some("env:CHEAPERINFERENCE_API_KEY".into()),
            base_url: Some("https://api.cheaperinference.com/v1".into()),
            headers: BTreeMap::new(),
            timeout_secs: None,
            enabled: true,
        },
    );
    ConfigFile {
        server: ServerConfig {
            api_keys: vec![ApiKeyConfig::Labeled {
                key: format!("file:{}", token_file.display()),
                label: "local".into(),
            }],
            ..ServerConfig::default()
        },
        providers,
        models: Vec::new(),
        aliases: BTreeMap::new(),
        routing: airouter_core::RoutingConfig::default(),
    }
}

pub fn init_at(path: &Path, force: bool) -> Result<InitResult, ConfigError> {
    if path.exists() && !force {
        return Err(ConfigError::Exists(path.display().to_string()));
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(|source| ConfigError::Write {
        path: dir.display().to_string(),
        source,
    })?;
    let token_path = dir.join("token");
    let token = generate_token();
    fs::write(&token_path, format!("{token}\n")).map_err(|source| ConfigError::Write {
        path: token_path.display().to_string(),
        source,
    })?;
    set_owner_only(&token_path);
    let cfg = template_config(&token_path);
    save_file(path, &cfg)?;
    Ok(InitResult {
        config_path: path.to_path_buf(),
        token_path,
        token,
        listen: cfg.server.listen.clone(),
    })
}

pub struct InitResult {
    pub config_path: PathBuf,
    pub token_path: PathBuf,
    pub token: String,
    pub listen: String,
}

pub fn generate_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).expect("getrandom");
    hex_encode(&buf)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub fn static_doctor(path: &Path) -> Result<Report, ConfigError> {
    let cfg = load_file(path)?;
    let mut report = validate_for(path, &cfg)?;
    match file_mode(path) {
        Some(mode) if mode & 0o077 != 0 => report.warnings.push(format!(
            "config {path} is mode {mode:03o}; expected 0600 so keys stay private",
            path = path.display()
        )),
        Some(_) => {}
        None => {}
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_env_and_file_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("key");
        fs::write(&secret, "from-file\n").unwrap();
        let var = format!("AIROUTER_TEST_{}", std::process::id());
        std::env::set_var(&var, "from-env");
        let empty = BTreeMap::new();
        let (env_v, env_s) = resolve_secret(&format!("env:{var}"), &empty);
        assert_eq!(env_v.as_deref(), Some("from-env"));
        assert!(!env_s.contains("missing"));
        let (file_v, _) = resolve_secret(&format!("file:{}", secret.display()), &empty);
        assert_eq!(file_v.as_deref(), Some("from-file"));
        let (missing, status) = resolve_secret("env:AIROUTER_DEFINITELY_UNSET_XYZ", &empty);
        assert!(missing.is_none());
        assert!(status.contains("missing"));
        std::env::remove_var(&var);
    }

    #[test]
    fn secret_file_resolves_and_status_omits_the_value() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.json");
        let mut cfg = ConfigFile::default();
        cfg.providers.insert(
            "openai".into(),
            ProviderConfig {
                provider_type: ProviderType::Openai,
                api_key: Some("secret:openai".into()),
                base_url: None,
                headers: BTreeMap::new(),
                timeout_secs: None,
                enabled: true,
            },
        );
        upsert_secret(&cfg_path, "openai", "sk-hidden\n").unwrap();
        assert_eq!(file_mode(&secrets_path(&cfg_path)), Some(0o600));
        let resolved = resolve_for(&cfg_path, &cfg).unwrap();
        let provider = resolved.providers.get("openai").unwrap();
        assert_eq!(provider.api_key.as_deref(), Some("sk-hidden"));
        assert_eq!(provider.key_status, "secret:openai");
        assert!(!provider.key_status.contains("sk-hidden"));
    }

    #[test]
    fn rejects_public_bind_without_a_key_and_public_admin() {
        let mut cfg = ConfigFile::default();
        cfg.server.listen = "0.0.0.0:8787".into();
        let report = validate(&cfg);
        assert!(report.errors.iter().any(|e| e.contains("non-loopback")));
        cfg.server.admin_listen = "0.0.0.0:8788".into();
        let report = validate(&cfg);
        assert!(report.errors.iter().any(|e| e.contains("admin")));
    }

    #[test]
    fn openai_compat_requires_base_url_and_duplicate_models_fail() {
        let mut cfg = ConfigFile::default();
        cfg.providers.insert(
            "local".into(),
            ProviderConfig {
                provider_type: ProviderType::OpenaiCompat,
                api_key: None,
                base_url: None,
                headers: BTreeMap::new(),
                timeout_secs: None,
                enabled: true,
            },
        );
        cfg.models.push(ModelConfig {
            id: "local/x".into(),
            provider: "local".into(),
            upstream: "x".into(),
            modality: Modality::Chat,
            enabled: true,
            context: None,
            capabilities: vec![],
            pricing: Default::default(),
            quality: None,
            task_quality: BTreeMap::new(),
            latency_ms_prior: None,
        });
        cfg.models.push(cfg.models[0].clone());
        let report = validate(&cfg);
        assert!(report.errors.iter().any(|e| e.contains("base_url")));
        assert!(report.errors.iter().any(|e| e.contains("duplicate")));
    }

    #[test]
    fn deepinfra_has_a_known_openai_base_url() {
        assert_eq!(
            known_compat_base_url("deepinfra"),
            Some("https://api.deepinfra.com/v1/openai")
        );
    }

    #[test]
    fn cheaperinference_has_a_known_openai_base_url_and_env() {
        assert_eq!(
            known_compat_base_url("cheaperinference"),
            Some("https://api.cheaperinference.com/v1")
        );
        assert_eq!(
            conventional_env("cheaperinference"),
            Some("CHEAPERINFERENCE_API_KEY")
        );
    }

    #[test]
    fn save_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        save_file(&path, &ConfigFile::default()).unwrap();
        let mode = file_mode(&path).unwrap();
        assert_eq!(mode, 0o600, "mode was {mode:03o}");
    }

    #[test]
    fn schema_is_json_object() {
        let schema = json_schema();
        assert!(schema.is_object());
        assert!(
            schema.get("properties").is_some()
                || schema.get("definitions").is_some()
                || schema.get("$schema").is_some()
        );
    }
}
