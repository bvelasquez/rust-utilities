use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_projects_dir")]
    pub projects_dir: String,
    #[serde(default = "default_agents_per_project")]
    pub agents_per_project: usize,
    #[serde(default = "default_pi_path")]
    pub pi: String,
    #[serde(default = "default_auto_restart")]
    pub auto_restart: bool,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: usize,
    #[serde(default)]
    pub speak: SpeakDefaults,
    #[serde(default)]
    pub telegram: TelegramDefaults,
    #[serde(default)]
    pub api: ApiDefaults,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            projects_dir: default_projects_dir(),
            agents_per_project: default_agents_per_project(),
            pi: default_pi_path(),
            auto_restart: default_auto_restart(),
            max_restarts: default_max_restarts(),
            speak: SpeakDefaults::default(),
            telegram: TelegramDefaults::default(),
            api: ApiDefaults::default(),
        }
    }
}

fn default_projects_dir() -> String {
    "~/projects".into()
}
fn default_agents_per_project() -> usize {
    1
}
fn default_pi_path() -> String {
    "pi".into()
}
fn default_auto_restart() -> bool {
    true
}
fn default_max_restarts() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakDefaults {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default = "default_broker_url")]
    pub base_url: String,
    #[serde(default = "default_speak_on")]
    pub on: Vec<String>,
}

impl Default for SpeakDefaults {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            voice: default_voice(),
            base_url: default_broker_url(),
            on: default_speak_on(),
        }
    }
}
fn default_true() -> bool {
    true
}
fn default_voice() -> String {
    "Leda".into()
}
fn default_broker_url() -> String {
    "http://127.0.0.1:8787".into()
}
fn default_speak_on() -> Vec<String> {
    vec!["routed".into(), "idle".into(), "error".into()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramDefaults {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default = "default_true")]
    pub inbound: bool,
    #[serde(default = "default_notify_on")]
    pub notify: Vec<String>,
}

impl Default for TelegramDefaults {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            chat_id: String::new(),
            inbound: default_true(),
            notify: default_notify_on(),
        }
    }
}
fn default_notify_on() -> Vec<String> {
    vec!["routed".into(), "idle".into(), "error".into()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiDefaults {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub token: String,
}

impl Default for ApiDefaults {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            token: String::new(),
        }
    }
}
fn default_bind() -> String {
    "127.0.0.1:9851".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub agents: Option<usize>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl ProjectConfig {
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
    pub fn agent_count(&self, defaults: &Defaults) -> usize {
        self.agents.unwrap_or(defaults.agents_per_project)
    }
}

/// Fully-resolved config with expanded paths and defaults applied.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub path: PathBuf,
    pub defaults: Defaults,
    pub projects: Vec<ResolvedProject>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub model: Option<String>,
    pub env: BTreeMap<String, String>,
    pub agents: usize,
}

impl ResolvedProject {
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config dir")?;
    Ok(base.join("pi-commander"))
}

pub fn resolve_config_path(cli_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = cli_path {
        return Ok(p.to_path_buf());
    }
    if let Ok(envp) = std::env::var("PI_COMMANDER_CONFIG") {
        return Ok(PathBuf::from(envp));
    }
    Ok(config_dir()?.join("projects.yaml"))
}


/// Load and resolve a config file from disk.
pub fn load_config(path: &Path) -> Result<ResolvedConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read config {}", path.display()))?;
    let parsed: FileConfig = serde_yaml::from_str(&raw)
        .with_context(|| format!("parse config {}", path.display()))?;
    if parsed.version != CONFIG_VERSION {
        bail!(
            format!(
                "unsupported config version {} (expected {}) — {}",
                parsed.version,
                CONFIG_VERSION,
                path.display()
            )
        );
    }
    resolve(parsed, path)
}

/// Resolve a FileConfig into a ResolvedConfig, applying defaults and path interpolation.
pub fn resolve(file: FileConfig, path: &Path) -> Result<ResolvedConfig> {
    let defaults = file.defaults;
    let projects = file
        .projects
        .into_iter()
        .map(|p| -> Result<ResolvedProject> {
            let path_str = interpolate(&p.path, &defaults);
            let resolved_path = expand_tilde(PathBuf::from(&path_str));
            if !resolved_path.exists() {
                bail!(
                    "project '{}' path does not exist: {}",
                    p.id,
                    resolved_path.display()
                );
            }
            let pname = p.display_name().to_string();
            let agents = p.agent_count(&defaults);
            let model = p.model.filter(|m| !m.is_empty());
            let env = p.env;
            let id = if p.id.is_empty() {
                resolved_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| resolved_path.display().to_string())
            } else {
                p.id
            };
            Ok(ResolvedProject {
                id,
                name: pname,
                path: resolved_path,
                model,
                env,
                agents,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ResolvedConfig {
        path: path.to_path_buf(),
        defaults,
        projects,
    })
}

pub fn interpolate(s: &str, defaults: &Defaults) -> String {
    s.replace("${projects_dir}", &defaults.projects_dir)
        .replace("$projects_dir", &defaults.projects_dir)
}

pub fn expand_tilde(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    p
}

pub fn validate(resolved: &ResolvedConfig) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for p in &resolved.projects {
        if !p.path.exists() {
            bail!("project '{}' path missing: {}", p.id, p.path.display());
        }
        if p.agents == 0 {
            warnings.push(format!("project '{}' has agents: 0 (never spawned)", p.id));
        }
    }
    // pi binary reachable?
    let pi_ok = which(&resolved.defaults.pi);
    if !pi_ok {
        warnings.push(format!(
            "pi binary '{}' not found on PATH",
            resolved.defaults.pi
        ));
    }
    Ok(warnings)
}

fn which(prog: &str) -> bool {
    if prog.contains('/') {
        return Path::new(prog).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join(prog))
                .any(|f| f.is_file())
        })
        .unwrap_or(false)
}