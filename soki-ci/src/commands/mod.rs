use anyhow::{bail, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use crate::cli::Cli;
use crate::config::{load_config, resolve_config_path, ResolvedConfig};
use crate::jobs::JobOrchestrator;

pub mod config_cmd;
pub mod deploy;
pub mod jobs_cmd;
pub mod projects;

pub struct AppContext {
    pub config_path: PathBuf,
    pub config: ResolvedConfig,
    pub jobs: Arc<JobOrchestrator>,
}

impl AppContext {
    pub async fn from_cli(cli: &Cli) -> Result<Self> {
        let config_path = resolve_config_path(cli.config.as_deref())?;
        let config = if config_path.is_file() {
            load_config(&config_path)?
        } else {
            bail!(
                "config not found at {}. Run `soki-ci config init` first.",
                config_path.display()
            );
        };
        let jobs = Arc::new(JobOrchestrator::new(&config));
        Ok(Self {
            config_path,
            config,
            jobs,
        })
    }

    pub fn reload_config(&mut self) -> Result<()> {
        self.config = load_config(&self.config_path)?;
        Ok(())
    }
}

pub fn require_yes(yes: bool, action: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if std::io::stdout().is_terminal() {
        bail!("confirmation required; re-run with --yes to {action}");
    }
    bail!("non-interactive mode requires --yes to {action}");
}
