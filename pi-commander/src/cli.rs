use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "pi-commander",
    version,
    about = "Agent coordination hub — spawn and supervise headless pi agents per project",
    after_help = "Agents: run `pi-commander capabilities --json` and `pi-commander env schema --json`.\n\
        Config: user-wide projects.yaml (see `pi-commander config path`).\n\
        Daemon: `pi-commander daemon` keeps agents alive; CLI/TUI talk to it over HTTP."
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "Structured JSON envelope output (machine/agent friendly)"
    )]
    pub json: bool,

    #[arg(
        long,
        global = true,
        env = "PI_COMMANDER_CONFIG",
        help = "Path to projects.yaml (default: user config dir)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        env = "PI_COMMANDER_API",
        default_value = "http://127.0.0.1:9851",
        help = "Daemon API base URL"
    )]
    pub api: String,

    #[arg(
        long,
        global = true,
        env = "PI_COMMANDER_NON_INTERACTIVE",
        help = "Refuse interactive TUI"
    )]
    pub non_interactive: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Interactive agent dashboard (default in TTY)
    Watch,
    /// Start the long-running hub daemon (spawns agents, HTTP API, broker TTS, Telegram)
    Daemon {
        /// API bind address (default from config: 127.0.0.1:9851)
        #[arg(long, env = "PI_COMMANDER_API_BIND")]
        bind: Option<String>,
        /// API auth token; required when binding off loopback
        #[arg(long, env = "PI_COMMANDER_API_TOKEN")]
        api_token: Option<String>,
        /// Spawn configured agents on start (default: yes)
        #[arg(long)]
        no_spawn: bool,
    },
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
    /// Start agents (spawn workers for projects)
    Spawn {
        /// project ids to spawn (default: all configured)
        project: Option<String>,
        /// number of agents to spawn per project (overrides config)
        #[arg(long)]
        agents: Option<usize>,
        /// spawn every configured project
        #[arg(long)]
        all: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Send a prompt (natural routing: "fix bug A on simple-workout")
    Send { text: String },
    /// Send a steering (interrupt) message to a specific agent
    Steer {
        #[arg(long, help = "worker id like simple-workout or simple-workout#1")]
        agent: String,
        text: String,
    },
    /// Queue a follow-up message
    #[command(visible_alias = "fup", alias = "follow")]
    FollowUp {
        #[arg(long, short = 'a')]
        agent: String,
        text: String,
    },
    /// Abort a running agent
    Abort { agent: String },
    /// Switch an agent's model (provider/id, e.g. anthropic/claude-sonnet-4)
    Model { agent: String, model: String },
    /// Cycle to next available model
    CycleModel { agent: String },
    /// Set thinking level (off|minimal|low|medium|high|xhigh|max)
    Thinking { agent: String, level: String },
    /// Compact an agent's context
    Compact { agent: String },
    /// Run a shell command inside an agent's context (result is visible to that agent)
    Bash {
        agent: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Raw JSONL RPC command pass-through to an agent
    Pi {
        agent: String,
        /// JSON command payload, e.g. '{"type":"cycle_model"}'
        payload: String,
    },
    /// Stop an agent (shutdown its pi process)
    Stop { agent: String },
    /// Spawn one extra agent for a project
    New { project: String },
    /// Open an interactive pi on an agent's session (tmux window, or foreground if TTY)
    Hop { agent: String },
    /// Live status of all agents
    Status {
        /// tail N raw lines per agent
        #[arg(long)]
        logs: Option<usize>,
    },
    /// List configured projects (agent-friendly)
    ProjectsList,
    /// Capabilities manifest for driving agents
    Capabilities,
    /// Environment schema for agent automation
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCommands {
    /// List projects from YAML config
    List,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Create user-wide projects.yaml from the example
    Init,
    /// Print the active config path
    Path,
    /// Validate config paths and binaries
    Validate,
}

#[derive(Debug, Subcommand)]
pub enum EnvCommands {
    /// Print the pi-commander environment schema (JSON)
    Schema,
}