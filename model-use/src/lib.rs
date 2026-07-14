pub mod aggregate;
pub mod budget;
pub mod capabilities;
pub mod cli;
pub mod commands;
pub mod config;
pub mod env_schema;
pub mod output;
pub mod providers;
pub mod store;
pub mod ui;

use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;

use cli::{Cli, Commands, EnvCommands};
use commands::AppContext;
use output::Envelope;

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut ctx = AppContext::from_cli(&cli)?;

    match cli.command {
        None => {
            if std::io::stdout().is_terminal() {
                ui::run_watch(&mut ctx, aggregate::Period::Month).await?;
            } else {
                eprintln!("Usage: model-use watch | fetch | providers | budget | summary");
                eprintln!("Run model-use --help for details");
            }
        }
        Some(Commands::Watch { period }) => {
            ui::run_watch(&mut ctx, period.into_period()).await?;
        }
        Some(Commands::Fetch { days }) => {
            commands::fetch::run(&mut ctx, days, commands::fetch::FetchMode::Cli).await?;
        }
        Some(Commands::Providers { command }) => match command {
            cli::ProvidersCommands::List => commands::providers::run_list(&ctx).await?,
            cli::ProvidersCommands::Set { provider, key, email } => {
                commands::providers::run_set(&mut ctx, &provider, &key, email.as_deref())?;
            }
            cli::ProvidersCommands::Test { provider } => {
                commands::providers::run_test(&ctx, provider.as_deref()).await?;
            }
            cli::ProvidersCommands::Enable { provider } => {
                commands::providers::run_enable(&mut ctx, &provider, true)?;
            }
            cli::ProvidersCommands::Disable { provider } => {
                commands::providers::run_enable(&mut ctx, &provider, false)?;
            }
        },
        Some(Commands::Budget { command }) => match command {
            cli::BudgetCommands::Set { target, monthly } => {
                commands::budget::run_set(&mut ctx, &target, monthly)?;
            }
            cli::BudgetCommands::List => commands::budget::run_list(&ctx)?,
        },
        Some(Commands::Set { command }) => match command {
            cli::SetCommands::RefreshInterval { value } => {
                commands::set::run_refresh_interval(&mut ctx, &value)?;
            }
            cli::SetCommands::List => commands::set::run_list(&ctx)?,
        },
        Some(Commands::Summary { period }) => commands::summary::run(&ctx, period)?,
        Some(Commands::Capabilities) => {
            if cli.json {
                Envelope::ok("capabilities", capabilities::capabilities_json()).print_json()?;
            } else {
                println!("{}", serde_json::to_string_pretty(&capabilities::capabilities_json())?);
            }
        }
        Some(Commands::Env { command }) => match command {
            EnvCommands::Schema => {
                if cli.json {
                    Envelope::ok("env schema", env_schema::env_schema_json()).print_json()?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&env_schema::env_schema_json())?);
                }
            }
        },
    }

    Ok(())
}
