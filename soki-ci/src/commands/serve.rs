use std::net::SocketAddr;

use anyhow::Result;

use crate::api;
use crate::commands::AppContext;

pub async fn run(ctx: AppContext, bind: SocketAddr, api_token: Option<String>) -> Result<()> {
    api::run_server(ctx.jobs, ctx.config_path, bind, api_token).await
}

pub use api::parse_bind;
