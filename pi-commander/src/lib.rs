pub mod agent;
pub mod api;
pub mod broker;
pub mod capabilities;
pub mod cli;
pub mod commands;
pub mod config;
pub mod dispatch_cmds;
pub mod env_schema;
pub mod manager;
pub mod output;
pub mod router;
pub mod rpc;
pub mod telegram;
pub mod transcript_fmt;
pub mod ui;

use clap::Parser;
use std::io::IsTerminal;
use std::time::Duration;

pub async fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    match &cli.command {
        None | Some(cli::Commands::Watch) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            if std::io::stdout().is_terminal() && !cli.non_interactive {
                ui::run_watch(ctx).await?;
            } else {
                println!("non-interactive: agent status\n");
                commands::agents::run_status(&ctx, None).await?;
            }
        }

        Some(cli::Commands::Daemon { bind, api_token, no_spawn }) => {
            commands::daemon::run(&cli, bind.clone(), api_token.clone(), *no_spawn).await?;
        }

        Some(cli::Commands::Spawn { project, agents, all, yes }) => {
            let _ = yes;
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_spawn(&ctx, project.clone(), *agents, *all).await?;
        }

        Some(cli::Commands::Send { text }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_send(&ctx, text).await?;
        }

        Some(cli::Commands::Steer { agent, text }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "steer", agent, serde_json::json!({ "message": text })).await?;
        }
        Some(cli::Commands::FollowUp { agent, text }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "follow_up", agent, serde_json::json!({ "message": text })).await?;
        }
        Some(cli::Commands::Abort { agent }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "abort", agent, serde_json::json!({})).await?;
        }
        Some(cli::Commands::Model { agent, model }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "model", agent, serde_json::json!({ "model": model })).await?;
        }
        Some(cli::Commands::CycleModel { agent }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "pi", agent, serde_json::json!({ "json": "{\"type\":\"cycle_model\"}" })).await?;
        }
        Some(cli::Commands::Thinking { agent, level }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "thinking", agent, serde_json::json!({ "level": level })).await?;
        }
        Some(cli::Commands::Compact { agent }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "compact", agent, serde_json::json!({})).await?;
        }
        Some(cli::Commands::Bash { agent, command }) => {
            let cmd = command.join(" ");
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "bash", agent, serde_json::json!({ "command": cmd })).await?;
        }
        Some(cli::Commands::Pi { agent, payload }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "pi", agent, serde_json::json!({ "json": payload })).await?;
        }
        Some(cli::Commands::Stop { agent }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_verbose(&ctx, "stop", agent, serde_json::json!({})).await?;
        }
        Some(cli::Commands::New { project }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_new(&ctx, project).await?;
        }
        Some(cli::Commands::Hop { agent }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_hop(&ctx, agent).await?;
        }
        Some(cli::Commands::Status { logs }) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::agents::run_status(&ctx, *logs).await?;
        }

        Some(cli::Commands::ProjectsList) => {
            let ctx = commands::AppContext::from_cli(&cli)?;
            commands::projects::run_list(&ctx, cli.json)?;
        }
        Some(cli::Commands::Projects { command }) => match command {
            cli::ProjectsCommands::List => {
                let ctx = commands::AppContext::from_cli(&cli)?;
                commands::projects::run_list(&ctx, cli.json)?;
            }
        },

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
                    let ctx = commands::AppContext::from_cli(&cli)?;
                    commands::config_cmd::run_validate(&ctx, cli.json)?;
                }
            }
        }

        Some(cli::Commands::Capabilities) => {
            let data = capabilities::capabilities_json();
            if cli.json {
                output::Envelope::ok("capabilities", data).print_json()?;
            } else if std::io::stdout().is_terminal() {
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("{}", serde_json::to_string(&data)?);
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
    }
    Ok(())
}

/// Small helper so callers can await API availability.
pub async fn wait_for_api(base: &str, timeout: Duration) -> bool {
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Ok(res) = client
            .get(format!("{base}/health"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if res.status().is_success() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}