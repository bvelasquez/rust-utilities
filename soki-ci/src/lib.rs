pub mod api;
pub mod broker;
pub mod capabilities;
pub mod cli;
pub mod commands;
pub mod config;
pub mod env_schema;
pub mod jobs;
pub mod output;
pub mod runner;
pub mod ui;

use clap::Parser;
use std::io::IsTerminal;

pub async fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    match &cli.command {
        None | Some(cli::Commands::Watch) => {
            let ctx = commands::AppContext::from_cli(&cli).await?;
            if std::io::stdout().is_terminal() && !cli.non_interactive {
                let api = if cli.no_api {
                    None
                } else {
                    let addr = api::parse_bind(&cli.bind)?;
                    let handle = api::spawn_api(
                        ctx.jobs.clone(),
                        ctx.config_path.clone(),
                        addr,
                        cli.api_token.clone(),
                    )
                    .await?;
                    eprintln!(
                        "soki-ci API listening on http://{} (shared with TUI)",
                        handle.bind
                    );
                    Some(handle)
                };
                ui::run_dashboard(ctx, api).await?;
            } else {
                anyhow::bail!(
                    "Interactive TUI requires a TTY. Run `soki-ci watch` or use subcommands (e.g. `deploy run --yes`)."
                );
            }
        }
        Some(cli::Commands::Config { command }) => {
            let config_path = config::resolve_config_path(cli.config.as_deref())?;
            match command {
                cli::ConfigCommands::Init => {
                    commands::config_cmd::run_init(&config_path, cli.json)?
                }
                cli::ConfigCommands::Path => {
                    commands::config_cmd::run_path(&config_path, cli.json)?
                }
                cli::ConfigCommands::Validate => {
                    let ctx = commands::AppContext::from_cli(&cli).await?;
                    commands::config_cmd::run_validate(&ctx, cli.json).await?;
                }
            }
        }
        Some(cli::Commands::Capabilities) => {
            let data = capabilities::capabilities_json();
            if cli.json {
                output::Envelope::ok("capabilities", data).print_json()?;
            } else {
                println!("{}", serde_json::to_string_pretty(&data)?);
            }
        }
        Some(cli::Commands::Env { command }) => match command {
            cli::EnvCommands::Schema => {
                let data = env_schema::env_schema_json();
                if cli.json {
                    output::Envelope::ok("env schema", data).print_json()?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&data)?);
                }
            }
        },
        Some(cli::Commands::Projects { command }) => {
            let ctx = commands::AppContext::from_cli(&cli).await?;
            match command {
                cli::ProjectsCommands::List => commands::projects::run_list(&ctx, cli.json)?,
            }
        }
        Some(cli::Commands::Deploy { command }) => {
            let ctx = commands::AppContext::from_cli(&cli).await?;
            match command {
                cli::DeployCommands::Run {
                    project,
                    target,
                    all,
                    yes,
                    wait,
                } => {
                    commands::deploy::run(
                        &ctx,
                        project,
                        target.as_deref(),
                        *all,
                        *yes,
                        *wait,
                        cli.json,
                    )
                    .await?;
                }
            }
        }
        Some(cli::Commands::Jobs { command }) => {
            let ctx = commands::AppContext::from_cli(&cli).await?;
            match command {
                cli::JobsCommands::List => commands::jobs_cmd::run_list(&ctx, cli.json)?,
                cli::JobsCommands::Logs { id, tail } => {
                    commands::jobs_cmd::run_logs(&ctx, id, *tail, cli.json)?
                }
                cli::JobsCommands::Cancel { id, yes } => {
                    commands::jobs_cmd::run_cancel(&ctx, id, *yes, cli.json)?;
                }
                cli::JobsCommands::Reset { yes } => {
                    commands::jobs_cmd::run_reset(&ctx, *yes, cli.json)?;
                }
            }
        }
        Some(cli::Commands::Serve) => {
            let ctx = commands::AppContext::from_cli(&cli).await?;
            let addr = commands::serve::parse_bind(&cli.bind)?;
            commands::serve::run(ctx, addr, cli.api_token.clone()).await?;
        }
    }

    Ok(())
}
