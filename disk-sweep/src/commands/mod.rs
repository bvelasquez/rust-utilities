pub mod analyze;
pub mod clean;
pub mod review;
pub mod scan;
pub mod targets;
pub mod watch;

use crate::cli::Cli;
use crate::config::AppContext;

pub struct CommandContext {
    pub app: AppContext,
    pub json: bool,
}

impl CommandContext {
    pub fn from_cli(cli: &Cli) -> anyhow::Result<Self> {
        Ok(Self {
            app: AppContext::from_cli(cli)?,
            json: cli.json,
        })
    }
}
