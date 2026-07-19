use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputCategory {
    Priority,
    Personal,
    Work,
    Newsletter,
    Marketing,
    Notification,
    Receipt,
    Spam,
    Unknown,
}

impl OutputCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::Personal => "personal",
            Self::Work => "work",
            Self::Newsletter => "newsletter",
            Self::Marketing => "marketing",
            Self::Notification => "notification",
            Self::Receipt => "receipt",
            Self::Spam => "spam",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "mail-sweep",
    version,
    about = "Agent-first AI email triage — IMAP sync, OpenRouter classification, ratatui inbox",
    after_help = "Agents: run `mail-sweep capabilities --json` and `mail-sweep config schema --json`.\n\
        Store secrets via `mail-sweep secrets set ...` or ~/.config/mail-sweep/secrets.toml or .env.\n\
        Interactive TUI opens by default in a TTY. Use `sync --json` and `process --dry-run --json` for automation."
)]
pub struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    pub json: bool,

    #[arg(long, global = true, help = "Config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, help = "SQLite cache directory")]
    pub data_dir: Option<PathBuf>,

    #[arg(long, global = true, help = "OpenRouter API key (overrides secrets.toml / .env)")]
    pub openrouter_key: Option<String>,

    #[arg(long, global = true, help = "LLM model override")]
    pub llm_model: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Fetch new mail into local cache via IMAP
    Sync {
        #[arg(long, help = "Sync only this account id")]
        account: Option<String>,
        #[arg(long, help = "Re-fetch recent UIDs (last 500)")]
        full: bool,
    },
    /// Classify pending messages with rules + OpenRouter
    Process {
        #[arg(long)]
        account: Option<String>,
        #[arg(long, default_value_t = 0, help = "Batch size (0 = config default)")]
        batch_size: usize,
        #[arg(long, help = "Show plan without saving")]
        dry_run: bool,
    },
    /// Execute a classification plan on IMAP
    Apply {
        #[arg(long, help = "Plan id from process output (latest pending if omitted)")]
        plan_id: Option<i64>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long, help = "Allow delete actions")]
        allow_delete: bool,
    },
    /// List cached messages
    List {
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        category: Option<OutputCategory>,
        #[arg(long)]
        priority: Option<u8>,
        #[arg(long)]
        unread: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Show a cached message by internal id
    Show {
        id: i64,
    },
    /// Email volume and category stats from cache
    Stats {
        #[arg(long)]
        account: Option<String>,
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
    /// Send email via SMTP
    Send {
        #[arg(long)]
        account: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Manage mail accounts
    Accounts {
        #[command(subcommand)]
        command: AccountsCommands,
    },
    /// Deterministic pre-AI rules
    Rules {
        #[command(subcommand)]
        command: RulesCommands,
    },
    /// Record user correction for sender/pattern
    Learn {
        #[command(subcommand)]
        command: LearnCommands,
    },
    /// Interactive ratatui TUI
    Interactive,
    /// Machine-readable command catalog
    Capabilities,
    /// Manage API keys and account passwords
    Secrets {
        #[command(subcommand)]
        command: SecretsCommands,
    },
    /// Configuration and secrets file schema
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum AccountsCommands {
    List,
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "imap.gmail.com")]
        imap_host: String,
        #[arg(long, default_value_t = 993)]
        imap_port: u16,
        #[arg(long, default_value = "smtp.gmail.com")]
        smtp_host: String,
        #[arg(long, default_value_t = 587)]
        smtp_port: u16,
        #[arg(long, help = "Use Gmail folder defaults")]
        gmail: bool,
        #[arg(long, help = "IMAP/SMTP password (saved to secrets.toml)")]
        password: Option<String>,
    },
    Test {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum RulesCommands {
    List,
    Add {
        #[arg(long)]
        r#match: String,
        #[arg(long)]
        action: String,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        priority: Option<u8>,
        #[arg(long)]
        target_folder: Option<String>,
    },
    Update {
        index: usize,
        #[arg(long)]
        r#match: Option<String>,
        #[arg(long)]
        action: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        priority: Option<u8>,
        #[arg(long)]
        target_folder: Option<String>,
    },
    Remove {
        index: usize,
    },
    Test {
        #[arg(long)]
        from: String,
        #[arg(long)]
        subject: String,
        #[arg(long, default_value = "")]
        headers: String,
    },
    /// AI review of existing rules — suggests merges and generalizations
    Audit {
        #[arg(long, help = "Apply all suggestions to config (non-interactive)")]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum LearnCommands {
    Feedback {
        #[arg(long)]
        sender: String,
        #[arg(long)]
        action: String,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, default_value_t = 5)]
        priority: u8,
    },
}

#[derive(Debug, Subcommand)]
pub enum SecretsCommands {
    /// Show which secrets are configured (values redacted)
    List,
    /// Save OpenRouter API key to secrets.toml
    #[command(name = "set-openrouter-key")]
    SetOpenrouterKey {
        #[arg(long)]
        key: String,
    },
    /// Save default LLM model to secrets.toml
    #[command(name = "set-llm-model")]
    SetLlmModel {
        #[arg(long)]
        model: String,
    },
    /// Save account IMAP/SMTP password to secrets.toml
    #[command(name = "set-account")]
    SetAccount {
        #[arg(long)]
        id: String,
        #[arg(long)]
        password: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    Schema,
}
