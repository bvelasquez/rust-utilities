use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_projects_dir")]
    pub projects_dir: String,
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    #[serde(default)]
    pub speak: SpeakDefaults,
    #[serde(default)]
    pub confirm_deploys: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            projects_dir: default_projects_dir(),
            max_parallel: default_max_parallel(),
            speak: SpeakDefaults::default(),
            confirm_deploys: true,
        }
    }
}

fn default_projects_dir() -> String {
    "~/projects".into()
}

fn default_max_parallel() -> usize {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakDefaults {
    #[serde(default = "default_speak_enabled")]
    pub enabled: bool,
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default = "default_broker_url")]
    pub base_url: String,
    #[serde(default = "default_speak_on")]
    pub on: Vec<SpeakEvent>,
}

impl Default for SpeakDefaults {
    fn default() -> Self {
        Self {
            enabled: default_speak_enabled(),
            voice: default_voice(),
            base_url: default_broker_url(),
            on: default_speak_on(),
        }
    }
}

fn default_speak_enabled() -> bool {
    true
}

fn default_voice() -> String {
    "Leda".into()
}

fn default_broker_url() -> String {
    "http://127.0.0.1:8787".into()
}

fn default_speak_on() -> Vec<SpeakEvent> {
    vec![SpeakEvent::Success, SpeakEvent::Error]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SpeakEvent {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub path: String,
    #[serde(default)]
    pub package_manager: Option<PackageManager>,
    pub targets: BTreeMap<String, TargetConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Pnpm,
    Npm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    #[serde(default)]
    pub label: Option<String>,
    pub runner: RunnerConfig,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub speak: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunnerConfig {
    PnpmScript { script: String },
    NpmScript { script: String },
    Make { target: String },
    Shell { command: Vec<String> },
}

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
    pub package_manager: PackageManager,
    pub targets: BTreeMap<String, ResolvedTarget>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub id: String,
    pub label: String,
    pub runner: RunnerConfig,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub timeout_secs: Option<u64>,
    pub speak: Option<bool>,
}

pub fn default_config_path() -> Result<PathBuf> {
    if let Some(proj_dirs) = directories::ProjectDirs::from("com.soki-ci", "soki-ci", "soki-ci") {
        let mut p = proj_dirs.config_dir().to_path_buf();
        p.push("projects.yaml");
        return Ok(p);
    }
    let mut home = dirs::home_dir().context("home directory")?;
    home.push(".soki-ci");
    home.push("projects.yaml");
    Ok(home)
}

pub fn resolve_config_path(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }
    default_config_path()
}

pub fn load_config(path: &Path) -> Result<ResolvedConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read config {}", path.display()))?;
    let file: FileConfig =
        serde_yaml::from_str(&raw).with_context(|| format!("parse YAML {}", path.display()))?;
    resolve_file_config(path, file)
}

pub fn resolve_file_config(path: &Path, file: FileConfig) -> Result<ResolvedConfig> {
    if file.version != CONFIG_VERSION {
        bail!(
            "unsupported config version {} (expected {})",
            file.version,
            CONFIG_VERSION
        );
    }

    let projects_dir = expand_path(&file.defaults.projects_dir)?;
    let mut projects = Vec::new();

    for proj in file.projects {
        let pm = proj.package_manager.unwrap_or(PackageManager::Pnpm);
        let name = proj
            .name
            .unwrap_or_else(|| proj.id.replace('-', " "));
        let base_path = expand_with_projects_dir(&proj.path, &projects_dir)?;

        let mut targets = BTreeMap::new();
        for (tid, tcfg) in proj.targets {
            let cwd = if let Some(rel) = &tcfg.cwd {
                base_path.join(rel)
            } else {
                base_path.clone()
            };
            let label = tcfg
                .label
                .unwrap_or_else(|| humanize_id(&tid));
            targets.insert(
                tid.clone(),
                ResolvedTarget {
                    id: tid,
                    label,
                    runner: tcfg.runner,
                    cwd,
                    env: tcfg.env,
                    timeout_secs: tcfg.timeout_secs,
                    speak: tcfg.speak,
                },
            );
        }

        projects.push(ResolvedProject {
            id: proj.id,
            name,
            path: base_path,
            package_manager: pm,
            targets,
        });
    }

    Ok(ResolvedConfig {
        path: path.to_path_buf(),
        defaults: file.defaults,
        projects,
    })
}

fn expand_path(s: &str) -> Result<PathBuf> {
    let s = s.trim();
    let expanded = if let Some(rest) = s.strip_prefix("~/") {
        let home = dirs::home_dir().context("home directory for ~ expansion")?;
        home.join(rest)
    } else if s == "~" {
        dirs::home_dir().context("home directory")?
    } else {
        PathBuf::from(s)
    };
    Ok(expanded)
}

fn expand_with_projects_dir(path_template: &str, projects_dir: &Path) -> Result<PathBuf> {
    let substituted = path_template.replace("${projects_dir}", &projects_dir.to_string_lossy());
    expand_path(&substituted)
}

fn humanize_id(id: &str) -> String {
    id.replace(['-', '_'], " ")
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub level: &'static str,
    pub message: String,
}

pub fn validate_config(cfg: &ResolvedConfig) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut ids = HashSet::new();

    for p in &cfg.projects {
        if !ids.insert(p.id.clone()) {
            issues.push(ValidationIssue {
                level: "error",
                message: format!("duplicate project id `{}`", p.id),
            });
        }
        if !p.path.is_dir() {
            issues.push(ValidationIssue {
                level: "error",
                message: format!(
                    "project `{}`: path not found: {}",
                    p.id,
                    p.path.display()
                ),
            });
        }
        if p.targets.is_empty() {
            issues.push(ValidationIssue {
                level: "warning",
                message: format!("project `{}` has no targets", p.id),
            });
        }
        for (tid, target) in &p.targets {
            if !target.cwd.is_dir() {
                issues.push(ValidationIssue {
                    level: "error",
                    message: format!(
                        "project `{}` target `{}`: cwd not found: {}",
                        p.id,
                        tid,
                        target.cwd.display()
                    ),
                });
            }
            match &target.runner {
                RunnerConfig::PnpmScript { script }
                | RunnerConfig::NpmScript { script } => {
                    if let Ok(scripts) = read_package_scripts(&target.cwd) {
                        if !scripts.contains(script) {
                            issues.push(ValidationIssue {
                                level: "warning",
                                message: format!(
                                    "project `{}` target `{}`: script `{}` not in package.json",
                                    p.id, tid, script
                                ),
                            });
                        }
                    } else {
                        issues.push(ValidationIssue {
                            level: "warning",
                            message: format!(
                                "project `{}` target `{}`: no package.json in {}",
                                p.id,
                                tid,
                                target.cwd.display()
                            ),
                        });
                    }
                }
                RunnerConfig::Make { .. } => {
                    if !target.cwd.join("Makefile").is_file()
                        && !target.cwd.join("makefile").is_file()
                    {
                        issues.push(ValidationIssue {
                            level: "warning",
                            message: format!(
                                "project `{}` target `{}`: no Makefile in {}",
                                p.id,
                                tid,
                                target.cwd.display()
                            ),
                        });
                    }
                }
                RunnerConfig::Shell { command } if command.is_empty() => {
                    issues.push(ValidationIssue {
                        level: "error",
                        message: format!(
                            "project `{}` target `{}`: shell command is empty",
                            p.id, tid
                        ),
                    });
                }
                RunnerConfig::Shell { .. } => {}
            }
        }
    }

    issues
}

fn read_package_scripts(dir: &Path) -> Result<HashSet<String>> {
    let pkg_path = dir.join("package.json");
    let raw = fs::read_to_string(&pkg_path)?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let mut set = HashSet::new();
    if let Some(scripts) = v.get("scripts").and_then(|s| s.as_object()) {
        for k in scripts.keys() {
            set.insert(k.clone());
        }
    }
    Ok(set)
}

pub fn example_yaml() -> &'static str {
    include_str!("../projects.example.yaml")
}

pub fn init_config_at(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, example_yaml())?;
    Ok(true)
}

pub fn find_project<'a>(cfg: &'a ResolvedConfig, id: &str) -> Option<&'a ResolvedProject> {
    cfg.projects.iter().find(|p| p.id == id)
}
