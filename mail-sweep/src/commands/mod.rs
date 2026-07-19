pub mod accounts;
pub mod apply;
pub mod learn;
pub mod list;
pub mod process;
pub mod rules;
pub mod secrets;
pub mod send;
pub mod show;
pub mod stats;
pub mod sync;

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
