use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "disk-sweep",
    version,
    about = "Agent-first smart disk cleanup — scan caches, Xcode junk, and review folders with LLM",
    after_help = "Agents: run `disk-sweep capabilities --json` and `disk-sweep env schema --json`.\n\
        Interactive TUI opens by default in a TTY. Use `scan --json` for automation."
)]
pub struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    pub json: bool,

    #[arg(long, global = true, env = "DISK_SWEEP_CONFIG", help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, env = "DISK_SWEEP_OPENROUTER_KEY")]
    pub openrouter_key: Option<String>,

    #[arg(long, global = true, env = "DISK_SWEEP_LLM_MODEL")]
    pub llm_model: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan default cleanup targets and report sizes
    Scan {
        /// Include per-item breakdown inside each target
        #[arg(long)]
        detail: bool,
    },
    /// Interactive TUI to browse, select, and clean
    Interactive,
    /// Live disk usage dashboard (ratatui)
    Watch {
        /// Folders to watch (defaults used when omitted)
        #[arg(long)]
        path: Vec<PathBuf>,

        /// Volume auto-refresh interval; 0 = manual only (deep scan always manual via r)
        #[arg(long, default_value = "0", env = "DISK_SWEEP_WATCH_INTERVAL")]
        interval: String,

        /// Number of largest cleanup items to show
        #[arg(long, default_value_t = 8)]
        top: usize,
    },
    /// Full disk analyze — dot folders, Library, stale projects (nothing selected by default)
    Analyze {
        /// Projects root (default: ~/projects)
        #[arg(long, env = "DISK_SWEEP_PROJECTS_ROOT")]
        projects_root: Option<PathBuf>,

        /// Days without git activity before a project is stale
        #[arg(long, default_value_t = 180, env = "DISK_SWEEP_STALE_DAYS")]
        stale_days: u32,

        /// Minimum size for dot-folder candidates (MB)
        #[arg(long, default_value_t = 100)]
        min_mb: u64,

        /// Minimum size for Library candidates (MB)
        #[arg(long, default_value_t = 200)]
        library_min_mb: u64,

        /// Minimum size for Rust target/incremental dirs under projects (MB)
        #[arg(long, default_value_t = 50)]
        project_build_min_mb: u64,

        /// Skip dot-folder scan (faster; useful with --projects-root for project-only runs)
        #[arg(long)]
        skip_dot: bool,

        /// Skip Library scan (faster; useful with --projects-root for project-only runs)
        #[arg(long)]
        skip_library: bool,
    },
    /// Delete contents of selected targets
    Clean {
        /// Target IDs to clean (default: all selected-by-default targets)
        #[arg(long)]
        targets: Vec<String>,

        /// Show what would be deleted without removing files
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation in non-interactive mode
        #[arg(long)]
        yes: bool,
    },
    /// LLM review of a folder for cleanup candidates
    Review {
        /// Folder to analyze
        path: PathBuf,

        /// Max immediate children to send to the LLM
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// Built-in cleanup targets
    Targets {
        #[command(subcommand)]
        command: TargetsCommands,
    },
    /// Machine-readable command catalog
    Capabilities,
    /// Environment variable schema
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum TargetsCommands {
    /// List default cleanup targets
    List,
    /// Explain what paths are scanned and what is never touched
    Explain,
}

#[derive(Debug, Subcommand)]
pub enum EnvCommands {
    Schema,
}
