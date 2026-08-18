use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "soki-ci",
    version,
    about = "Multi-project deploy control panel — parallel deploys, TUI, broker TTS",
    after_help = "Agents: run `soki-ci capabilities --json` and `soki-ci env schema --json`.\n\
        Config: user-wide projects.yaml (see `soki-ci config path`).\n\
        TUI starts the HTTP API on --bind (default 127.0.0.1:9847); use --no-api to disable."
)]
pub struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    pub json: bool,

    #[arg(
        long,
        global = true,
        env = "SOKI_CI_CONFIG",
        help = "Path to projects.yaml (default: user config dir)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        env = "SOKI_CI_NON_INTERACTIVE",
        help = "Refuse interactive TUI"
    )]
    pub non_interactive: bool,

    #[arg(
        long,
        global = true,
        env = "SOKI_CI_API_BIND",
        default_value = "127.0.0.1:9847",
        help = "HTTP API listen address (host:port); used by TUI and `serve`"
    )]
    pub bind: String,

    #[arg(
        long,
        global = true,
        env = "SOKI_CI_API_TOKEN",
        help = "Bearer token for HTTP API (required when binding beyond loopback)"
    )]
    pub api_token: Option<String>,

    #[arg(
        long,
        global = true,
        default_value_t = false,
        help = "Do not start the HTTP API with the TUI"
    )]
    pub no_api: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Interactive deploy dashboard (default in TTY); also serves the HTTP API unless --no-api
    Watch,
    /// Project registry
    Projects {
        #[command(subcommand)]
        command: ProjectsCommands,
    },
    /// User-wide YAML config
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Start deploy jobs
    Deploy {
        #[command(subcommand)]
        command: DeployCommands,
    },
    /// Running and recent jobs
    Jobs {
        #[command(subcommand)]
        command: JobsCommands,
    },
    /// HTTP API only (no TUI) — list and trigger deploy builds
    Serve,
    /// Machine-readable command catalog
    Capabilities,
    /// Environment variable schema
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCommands {
    /// List configured projects and deploy targets
    List,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Copy projects.example.yaml to the user config path
    Init,
    /// Validate YAML, paths, and script names
    Validate,
    /// Print resolved config file path
    Path,
}

#[derive(Debug, Subcommand)]
pub enum DeployCommands {
    /// Run one or all targets for a project
    Run {
        #[arg(short, long)]
        project: String,
        #[arg(short, long)]
        target: Option<String>,
        #[arg(long, help = "Run all targets sequentially for this project")]
        all: bool,
        #[arg(long, help = "Required for non-interactive deploy")]
        yes: bool,
        #[arg(long, help = "Block until the job finishes")]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum JobsCommands {
    /// Active and recent jobs
    List,
    /// Show log file (optionally tail)
    Logs {
        #[arg(short, long)]
        id: String,
        #[arg(long, default_value_t = 0)]
        tail: usize,
    },
    /// Send SIGTERM to a running job
    Cancel {
        #[arg(short, long)]
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Drop finished deployments and log files (keeps active jobs)
    Reset {
        #[arg(long, help = "Required for non-interactive reset")]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum EnvCommands {
    /// JSON schema for env vars
    Schema,
}
