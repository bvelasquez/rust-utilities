pub mod apply_progress;
pub mod agent;
pub mod capabilities;
pub mod cli;
pub mod commands;
pub mod config;
pub mod mail;
pub mod openrouter;
pub mod output;
pub mod process;
pub mod rules;
pub mod secrets;
pub mod safety;
pub mod store;
pub mod sync;
pub mod ui;

use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;

use cli::{Cli, Commands, ConfigCommands};
use commands::CommandContext;
use output::Envelope;

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut ctx = CommandContext::from_cli(&cli)?;

    match cli.command {
        None => {
            if std::io::stdout().is_terminal() {
                ui::run(&mut ctx).await?;
            } else {
                commands::stats::run(&ctx, None, 30)?;
            }
        }
        Some(Commands::Sync { account, full }) => {
            commands::sync::run(&ctx, account, full).await?;
        }
        Some(Commands::Process {
            account,
            batch_size,
            dry_run,
        }) => {
            commands::process::run(&mut ctx, account, batch_size, dry_run).await?;
        }
        Some(Commands::Apply {
            plan_id,
            dry_run,
            yes,
            allow_delete,
        }) => {
            commands::apply::run(&ctx, plan_id, dry_run, yes, allow_delete).await?;
        }
        Some(Commands::List {
            account,
            category,
            priority,
            unread,
            limit,
        }) => {
            commands::list::run(&ctx, account, category, priority, unread, limit)?;
        }
        Some(Commands::Show { id }) => {
            commands::show::run(&ctx, id)?;
        }
        Some(Commands::Stats { account, days }) => {
            commands::stats::run(&ctx, account, days)?;
        }
        Some(Commands::Send {
            account,
            to,
            subject,
            body,
            dry_run,
            yes,
        }) => {
            commands::send::run(&ctx, &account, &to, &subject, &body, dry_run, yes)?;
        }
        Some(Commands::Accounts { command }) => {
            commands::accounts::run(&mut ctx, &command).await?;
        }
        Some(Commands::Rules { command }) => {
            commands::rules::run(&mut ctx, &command).await?;
        }
        Some(Commands::Learn { command }) => {
            commands::learn::run(&mut ctx, &command)?;
        }
        Some(Commands::Secrets { command }) => {
            commands::secrets::run(&mut ctx, &command)?;
        }
        Some(Commands::Interactive) => {
            ui::run(&mut ctx).await?;
        }
        Some(Commands::Capabilities) => {
            if cli.json {
                Envelope::ok("capabilities", capabilities::capabilities_json()).print_json()?;
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&capabilities::capabilities_json())?
                );
            }
        }
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Schema => {
                if cli.json {
                    Envelope::ok("config schema", capabilities::config_schema_json()).print_json()?;
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&capabilities::config_schema_json())?
                    );
                }
            }
        },
    }

    Ok(())
}
