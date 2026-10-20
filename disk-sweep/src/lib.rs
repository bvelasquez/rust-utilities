pub mod agent;
pub mod analyze;
pub mod capabilities;
pub mod catalog;
pub mod clean;
pub mod cli;
pub mod commands;
pub mod config;
pub mod dups;
pub mod env_schema;
pub mod organize;
pub mod output;
pub mod scan;
pub mod targets;
pub mod ui;
pub mod volume;
pub mod watch_data;
pub mod interval;

use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;

use cli::{Cli, Commands, EnvCommands, TargetsCommands};
use commands::CommandContext;
use output::Envelope;

pub async fn run() -> Result<()> {
    config::load_dotenv();
    let cli = Cli::parse();
    if cli.capabilities_flag {
        return print_capabilities(true);
    }
    let ctx = CommandContext::from_cli(&cli)?;

    match cli.command {
        None => {
            if std::io::stdout().is_terminal() {
                ui::run_interactive()?;
            } else {
                commands::scan::run(&ctx, false)?;
            }
        }
        Some(Commands::Scan { detail }) => {
            commands::scan::run(&ctx, detail)?;
        }
        Some(Commands::Interactive) => {
            ui::run_interactive()?;
        }
        Some(Commands::Watch {
            path,
            interval,
            top,
        }) => {
            if cli.json || !std::io::stdout().is_terminal() {
                commands::watch::run_cli(&ctx, &path, &interval, top)?;
            } else {
                commands::watch::run_tui(&path, &interval, top).await?;
            }
        }
        Some(Commands::Analyze {
            projects_root,
            stale_days,
            min_mb,
            library_min_mb,
            project_build_min_mb,
            skip_dot,
            skip_library,
        }) => {
            commands::analyze::run(
                &ctx,
                projects_root,
                stale_days,
                min_mb,
                library_min_mb,
                project_build_min_mb,
                skip_dot,
                skip_library,
            )?;
        }
        Some(Commands::Clean {
            targets,
            dry_run,
            yes,
        }) => {
            commands::clean::run(&ctx, &targets, dry_run, yes)?;
        }
        Some(Commands::Review { path, limit }) => {
            commands::review::run(&ctx, &path, limit).await?;
        }
        Some(Commands::Targets { command }) => match command {
            TargetsCommands::List => {
                commands::targets::run_list(&ctx)?;
            }
            TargetsCommands::Explain => {
                commands::targets::run_explain(&ctx)?;
            }
        },
        Some(Commands::Capabilities) => {
            print_capabilities(cli.json)?;
        }
        Some(Commands::Env { command }) => match command {
            EnvCommands::Schema => {
                if cli.json {
                    Envelope::ok("env schema", env_schema::env_schema_json()).print_json()?;
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&env_schema::env_schema_json())?
                    );
                }
            }
        },
        Some(Commands::Dups {
            path,
            no_recursive,
            sha256,
            min_bytes,
            cross_devices,
            exclude,
            apply,
            yes,
            keep,
        }) => {
            commands::dups::run(
                &ctx,
                path,
                no_recursive,
                sha256,
                min_bytes,
                cross_devices,
                &exclude,
                apply,
                yes,
                keep,
            )?;
        }
        Some(Commands::Organize {
            once,
            watch,
            rules,
            plan,
            dry_run,
            apply,
        }) => {
            commands::organize::run(&ctx, once, watch, rules, plan, dry_run, apply)?;
        }
    }

    Ok(())
}

fn print_capabilities(json: bool) -> Result<()> {
    let catalog = capabilities::capabilities_json();
    if json {
        Envelope::ok("capabilities", catalog).print_json()?;
    } else {
        println!("{}", serde_json::to_string_pretty(&catalog)?);
    }
    Ok(())
}
