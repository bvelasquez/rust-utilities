use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;

use crate::api::{self, ApiOptions};
use crate::commands::AppContext;

pub async fn run(
    ctx: AppContext,
    bind: SocketAddr,
    api_token: Option<String>,
    web_dir: Option<PathBuf>,
    allow_config_write: bool,
    open: bool,
) -> Result<()> {
    api::run_server(
        ctx.jobs,
        ctx.config_path,
        ApiOptions {
            bind,
            api_token,
            web_dir,
            allow_config_write,
            open,
        },
    )
    .await
}

pub use api::parse_bind;
