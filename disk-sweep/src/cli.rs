use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "disk-sweep",
    version,
    about = "Agent-first disk cleanup, duplicate scan, and folder organizer",
    after_help = "Agents: run `disk-sweep --capabilities` or `disk-sweep capabilities --json`.\n\
        Interactive TUI opens by default in a TTY. Use `scan --json` for automation.\n\
        `dups` finds duplicate files. `organize` moves files using ~/.config/disk-sweep/rules.yaml."
)]
pub struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    pub json: bool,

    #[arg(
        long = "capabilities",
        global = true,
        help = "Print the agent capability catalog as JSON and exit"
    )]
    pub capabilities_flag: bool,

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
    /// Machine-readable command catalog for agents
    Capabilities,
    /// Find duplicate files under a folder. Read-only unless --apply.
    Dups {
        /// Folder to scan. Omit in a terminal to browse and pick a folder.
        path: Option<PathBuf>,

        /// Stay in the top directory instead of walking subdirectories
        #[arg(long)]
        no_recursive: bool,

        /// Confirm full-hash matches with SHA-256
        #[arg(long)]
        sha256: bool,

        /// Skip files smaller than this many bytes (0 includes empty files)
        #[arg(long, default_value_t = 1)]
        min_bytes: u64,

        /// Also scan mounted filesystems under the folder
        #[arg(long)]
        cross_devices: bool,

        /// Folder to leave out of the scan. Repeat for more than one. Everything inside is ignored.
        #[arg(long, value_name = "DIR")]
        exclude: Vec<PathBuf>,

        /// Delete extra copies. The scan stays read-only without this flag.
        /// A terminal must type delete. A non-terminal also needs --yes.
        #[arg(long)]
        apply: bool,

        /// Required with --apply when stdout is not a terminal
        #[arg(long)]
        yes: bool,

        /// Which copy to keep when --apply deletes the others
        #[arg(long, value_enum, default_value_t = DupKeep::Shortest)]
        keep: DupKeep,
    },
    /// Move files from source folders using a YAML rules file.
    /// SHA-256 of each file is checked against ~/.local/share/disk-sweep/organize-catalog.sqlite.
    /// Content already stored is moved to dest_root/duplicates/ instead of the library.
    Organize {
        /// Walk each source once and move matches immediately
        #[arg(long, conflicts_with_all = ["watch", "apply"])]
        once: bool,

        /// Watch source folders and move files as they arrive. Cannot be combined with --plan, --dry-run, or --apply.
        #[arg(long, conflicts_with_all = ["once", "apply", "plan", "dry_run"])]
        watch: bool,

        /// Rules file (default: ~/.config/disk-sweep/rules.yaml)
        #[arg(long)]
        rules: Option<PathBuf>,

        /// Write a YAML or Markdown plan and do not move files. Same as --dry-run. Not valid with --watch.
        #[arg(long, value_name = "FILE", conflicts_with_all = ["watch", "apply", "dry_run"])]
        plan: Option<PathBuf>,

        /// Write a YAML or Markdown plan and do not move files. Same as --plan. Not valid with --watch.
        #[arg(long, value_name = "FILE", conflicts_with_all = ["watch", "apply", "plan"])]
        dry_run: Option<PathBuf>,

        /// Move only the files recorded in a plan from --plan or --dry-run
        #[arg(long, value_name = "FILE", conflicts_with_all = ["watch", "once", "plan", "dry_run"])]
        apply: Option<PathBuf>,
    },
    /// Environment variable schema
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum DupKeep {
    /// Keep the copy with the shortest path
    #[default]
    Shortest,
    /// Keep the copy with the newest modified time
    Newest,
    /// Keep the copy with the oldest modified time
    Oldest,
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
