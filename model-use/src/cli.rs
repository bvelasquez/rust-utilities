use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "model-use",
    version,
    about = "Agent-first LLM cost aggregator — OpenRouter, Anthropic, OpenAI, Cursor",
    after_help = "Agents: run `model-use capabilities --json` and `model-use env schema --json`.\n\
        Keys: OpenRouter management, Anthropic admin, OpenAI org admin, Cursor Admin API (Teams/Enterprise)."
)]
pub struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    pub json: bool,

    #[arg(long, global = true, env = "MODEL_USE_CONFIG", help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, env = "MODEL_USE_OPENROUTER_KEY")]
    pub openrouter_key: Option<String>,

    #[arg(long, global = true, env = "MODEL_USE_ANTHROPIC_KEY")]
    pub anthropic_key: Option<String>,

    #[arg(long, global = true, env = "MODEL_USE_OPENAI_KEY")]
    pub openai_key: Option<String>,

    #[arg(long, global = true, env = "MODEL_USE_CURSOR_KEY")]
    pub cursor_key: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Interactive TUI dashboard
    Watch {
        #[arg(long, default_value = "day")]
        period: PeriodArg,
    },
    /// Pull usage from enabled providers
    Fetch {
        #[arg(long, default_value_t = 90)]
        days: i64,
    },
    /// Manage provider API keys
    Providers {
        #[command(subcommand)]
        command: ProvidersCommands,
    },
    /// Manage monthly budgets
    Budget {
        #[command(subcommand)]
        command: BudgetCommands,
    },
    /// Manage TUI and app settings
    Set {
        #[command(subcommand)]
        command: SetCommands,
    },
    /// Aggregated spend from local cache
    Summary {
        #[arg(long, default_value = "month")]
        period: PeriodArg,
    },
    /// Machine-readable command catalog
    Capabilities,
    /// Environment variable schema
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PeriodArg {
    Day,
    Week,
    Month,
}

impl PeriodArg {
    pub fn into_period(self) -> crate::aggregate::Period {
        match self {
            PeriodArg::Day => crate::aggregate::Period::Day,
            PeriodArg::Week => crate::aggregate::Period::Week,
            PeriodArg::Month => crate::aggregate::Period::Month,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ProvidersCommands {
    /// List configured providers
    List,
    /// Save provider API key to config
    Set {
        provider: String,
        #[arg(long)]
        key: String,
        /// Cursor only: filter usage to this team member email
        #[arg(long)]
        email: Option<String>,
    },
    /// Validate API key type and permissions
    Test {
        provider: Option<String>,
    },
    /// Enable a provider
    Enable { provider: String },
    /// Disable a provider
    Disable { provider: String },
}

#[derive(Debug, Subcommand)]
pub enum BudgetCommands {
    /// Set global or per-provider monthly budget
    Set {
        /// Provider name or "global"
        target: String,
        #[arg(long)]
        monthly: f64,
    },
    /// Show configured budgets
    List,
}

#[derive(Debug, Subcommand)]
pub enum SetCommands {
    /// Set TUI auto-refresh interval (e.g. 15m, 1h, 900, 0 to disable)
    RefreshInterval {
        /// Duration: bare seconds, or with s/m/h suffix (default 15m)
        value: String,
    },
    /// Show current settings
    List,
}

#[derive(Debug, Subcommand)]
pub enum EnvCommands {
    Schema,
}
