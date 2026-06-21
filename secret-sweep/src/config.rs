use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const DEFAULT_OWNERS: &[&str] = &["bvelasquez", "mighty45"];

pub const DEFAULT_SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".turbo",
    "Pods",
    "DerivedData",
    ".gradle",
];

pub const DEFAULT_EXTRA_PATTERNS: &[&str] = &[
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "credentials.json",
    "service-account*.json",
    "firebase-adminsdk*.json",
];

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    owners: Option<Vec<String>>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    patterns: Option<Vec<String>>,
    skip_dirs: Option<Vec<String>>,
    max_file_size: Option<u64>,
    max_walk_depth: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RepoFilter {
    pub mine_only: bool,
    pub owners: HashSet<String>,
    pub include: HashSet<String>,
    pub exclude: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub root: PathBuf,
    pub depth: usize,
    pub filter: RepoFilter,
    pub extra_patterns: Vec<String>,
    pub skip_dirs: HashSet<String>,
    pub max_file_size: u64,
    pub max_walk_depth: usize,
}

#[derive(Debug, clap::Parser)]
pub struct GlobalOpts {
    /// Root folder containing project directories (default: ~/projects)
    #[arg(long, env = "SECRET_SWEEP_ROOT", global = true)]
    pub root: Option<PathBuf>,

    /// How deep to search for git repos (1 = immediate children only)
    #[arg(long, default_value_t = 1, global = true)]
    pub depth: usize,

    /// Only repos you own (remote owner matches your GitHub orgs/users, or no remote)
    #[arg(long, default_value_t = true, env = "SECRET_SWEEP_MINE", global = true)]
    pub mine: bool,

    /// Include all git repos, including third-party / open-source clones
    #[arg(long, conflicts_with = "mine", global = true)]
    pub all: bool,

    /// GitHub/GitLab owner to treat as yours (repeatable)
    #[arg(long = "owner", env = "SECRET_SWEEP_OWNERS", value_delimiter = ',', global = true)]
    pub owners: Vec<String>,

    /// Always include these project folder names
    #[arg(long = "include", value_delimiter = ',', global = true)]
    pub include: Vec<String>,

    /// Always skip these project folder names
    #[arg(long = "exclude", value_delimiter = ',', global = true)]
    pub exclude: Vec<String>,

    /// Config file (default: ~/.config/secret-sweep/config.toml)
    #[arg(long, env = "SECRET_SWEEP_CONFIG", global = true)]
    pub config: Option<PathBuf>,

    /// Extra glob patterns beyond dotfiles and defaults
    #[arg(long = "pattern", value_delimiter = ',', global = true)]
    pub patterns: Vec<String>,

    /// Emit machine-readable JSON on stdout
    #[arg(long, global = true)]
    pub json: bool,

    /// Show additional detail
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Skip confirmation prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
}

pub fn default_root() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("projects"))
        .unwrap_or_else(|| PathBuf::from("projects"))
}

pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("secret-sweep").join("config.toml"))
}

pub fn default_backups_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("Backups"))
        .unwrap_or_else(|| PathBuf::from("Backups"))
}

fn normalize_name(s: &str) -> String {
    s.trim().to_lowercase()
}

pub fn load_scan_config(opts: &GlobalOpts) -> Result<ScanConfig> {
    let mut file_cfg = FileConfig::default();
    let config_path = opts.config.clone().or_else(default_config_path);
    if let Some(path) = config_path {
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read config {}", path.display()))?;
            file_cfg = toml::from_str(&text)
                .with_context(|| format!("parse config {}", path.display()))?;
        }
    }

    let owners: Vec<String> = if opts.owners.is_empty() {
        file_cfg
            .owners
            .clone()
            .unwrap_or_else(|| DEFAULT_OWNERS.iter().map(|s| s.to_string()).collect())
    } else {
        opts.owners.clone()
    };

    let include: HashSet<String> = opts
        .include
        .iter()
        .chain(file_cfg.include.iter().flatten())
        .map(|s| normalize_name(s))
        .collect();

    let exclude: HashSet<String> = opts
        .exclude
        .iter()
        .chain(file_cfg.exclude.iter().flatten())
        .map(|s| normalize_name(s))
        .collect();

    let mut extra_patterns: Vec<String> = DEFAULT_EXTRA_PATTERNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    extra_patterns.extend(file_cfg.patterns.clone().unwrap_or_default());
    extra_patterns.extend(opts.patterns.clone());

    let mut skip_dirs: HashSet<String> = DEFAULT_SKIP_DIRS
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(extra) = &file_cfg.skip_dirs {
        skip_dirs.extend(extra.iter().cloned());
    }

    let root = opts.root.clone().unwrap_or_else(default_root);
    let root = root
        .canonicalize()
        .with_context(|| format!("projects root not found: {}", root.display()))?;

    Ok(ScanConfig {
        root,
        depth: opts.depth,
        filter: RepoFilter {
            mine_only: opts.mine && !opts.all,
            owners: owners.into_iter().map(|s| normalize_name(&s)).collect(),
            include,
            exclude,
        },
        extra_patterns,
        skip_dirs,
        max_file_size: file_cfg.max_file_size.unwrap_or(1_048_576),
        max_walk_depth: file_cfg.max_walk_depth.unwrap_or(8),
    })
}

pub fn is_skipped_dir_name(name: &str, skip_dirs: &HashSet<String>) -> bool {
    skip_dirs.contains(name)
}

pub fn is_skipped_walk_path(path: &Path, skip_dirs: &HashSet<String>) -> bool {
    let normalized = path.to_string_lossy();
    if normalized.contains("/.yarn/cache/") {
        return true;
    }
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|name| is_skipped_dir_name(name, skip_dirs))
            .unwrap_or(false)
    })
}

pub fn is_noise_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some(".DS_Store" | "Thumbs.db")
    )
}

pub fn is_dot_path(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .starts_with('.')
            && c.as_os_str().to_string_lossy() != "."
            && c.as_os_str().to_string_lossy() != ".."
    })
}
