//! The long-running hub: owns agents, speaks via broker, polls Telegram,
//! serves the HTTP API.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::json;
use tokio::sync::mpsc;

use crate::broker::BrokerClient;
use crate::cli::Cli;
use crate::commands::{config_cmd, AppContext};
use crate::config::{SpeakDefaults, TelegramDefaults};
use crate::manager::{AgentManager, AgentNotify};
use crate::telegram::{self, TelegramClient};

pub async fn run(cli: &Cli, bind: Option<String>, api_token: Option<String>, no_spawn: bool) -> Result<()> {
    // ignore SIGHUP when running detached (no controlling tty after setsid)
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let ctx = AppContext::from_cli(cli)?;
    let config = ctx.require_config()?.clone();

    // validate pi binary early
    crate::config::validate(&config)?;

    let manager = Arc::new(AgentManager::new(config.clone()));
    let notify_rx = manager.take_notify_rx().await;

    if !no_spawn {
        let ids = manager.spawn_all().await?;
        println!(
            "pi-commander daemon: spawned {} agent(s): {}",
            ids.len(),
            ids.join(", ")
        );
    } else {
        println!("pi-commander daemon: agent spawning disabled (--no-spawn)");
    }

    let broker = BrokerClient::new(
        &config.defaults.speak.base_url,
        &config.defaults.speak.voice,
    );
    let telegram_client = TelegramClient::new(
        telegram::bot_token(&config.defaults.telegram.bot_token),
        telegram::chat_id(&config.defaults.telegram.chat_id),
    );

    if let Some(rx) = notify_rx {
        let broker_n = broker.clone();
        let tg_n = telegram_client.clone();
        let speak = config.defaults.speak.clone();
        let tg_cfg = config.defaults.telegram.clone();
        tokio::spawn(notifier_loop(rx, broker_n, tg_n, speak, tg_cfg));
    }

    // telegram inbound -> dispatch to agents
    if telegram_client.enabled() && config.defaults.telegram.inbound {
        let tg_in = telegram_client.clone();
        let mgr = manager.clone();
        let allowed = telegram::chat_id(&config.defaults.telegram.chat_id);
        let (_stop_tx, stop_rx) = mpsc::channel(1);
        tokio::spawn(async move {
            tg_in
                .poll_inbound(allowed, move |_chat, text| {
                    let mgr = mgr.clone();
                    tokio::spawn(async move {
                        match mgr.dispatch(&text).await {
                            Ok((id, _)) => println!("telegram -> dispatched to {id}"),
                            Err(e) => eprintln!("telegram dispatch failed: {e}"),
                        }
                    });
                }, stop_rx)
                .await;
        });
    }

    // API server
    let token = api_token
        .filter(|t| !t.is_empty())
        .or_else(|| {
            let t = config.defaults.api.token.clone();
            if t.is_empty() { None } else { Some(t) }
        })
        .unwrap_or_default();
    let router = crate::api::build_router(
        manager.clone(),
        broker.clone(),
        telegram_client,
        ctx.config_path.clone(),
        token,
    );
    let addr = bind.unwrap_or_else(|| config.defaults.api.bind.clone());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind API {addr}"))?;
    println!("pi-commander daemon: HTTP API on http://{addr}");
    println!(
        "pi-commander daemon: broker={} voice={} telegram={}",
        config.defaults.speak.base_url,
        config.defaults.speak.voice,
        telegram_enabled_label(&config.defaults.telegram)
    );

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("axum serve")?;

    // graceful shutdown: stop agents
    manager.shutdown_all().await;
    println!("pi-commander daemon stopped");
    Ok(())
}

fn telegram_enabled_label(cfg: &TelegramDefaults) -> &'static str {
    if !cfg.bot_token.is_empty() || std::env::var("TELEGRAM_BOT_TOKEN").is_ok() {
        "enabled"
    } else {
        "disabled"
    }
}

async fn notifier_loop(
    mut rx: mpsc::Receiver<AgentNotify>,
    broker: BrokerClient,
    telegram: TelegramClient,
    speak: SpeakDefaults,
    tg_cfg: TelegramDefaults,
) {
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentNotify::Idle { id, project, text } => {
                let msg = format!("{project} {id} done: {text}");
                if speak.enabled && speak.on.iter().any(|e| e == "idle") {
                    if let Err(e) = broker.speak(&msg).await {
                        eprintln!("broker speak: {e}");
                    }
                }
                if tg_cfg.notify.iter().any(|e| e == "idle") {
                    let _ = telegram.send(&msg).await;
                }
            }
            AgentNotify::Error { id, project, message } => {
                let msg = format!("⚠ {project} {id} error: {message}");
                if speak.enabled && speak.on.iter().any(|e| e == "error") {
                    if let Err(e) = broker.speak(&msg).await {
                        eprintln!("broker speak: {e}");
                    }
                }
                if tg_cfg.notify.iter().any(|e| e == "error") {
                    let _ = telegram.send(&msg).await;
                }
            }
            AgentNotify::PhaseChanged { id, project, old, new } => {
                // routed events (dispatch) arrive as a synthetic Idle->Busy
                if new == crate::agent::AgentPhase::Busy && old == crate::agent::AgentPhase::Idle {
                    if speak.enabled && speak.on.iter().any(|e| e == "routed") {
                        let msg = format!("routed to {project} {id}");
                        if let Err(e) = broker.speak(&msg).await {
                            eprintln!("broker speak: {e}");
                        }
                    }
                    if tg_cfg.notify.iter().any(|e| e == "routed") {
                        let msg = format!("▶ routed: {project} {id}");
                        let _ = telegram.send(&msg).await;
                    }
                }
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// Helper used by tests/scripts: validate then print config status.
pub async fn run_validate_json(cli: &Cli) -> Result<()> {
    let ctx = AppContext::from_cli(cli)?;
    config_cmd::run_validate(&ctx, true)
}

// re-export for main
pub fn daemon_help() -> &'static str {
    "pi-commander daemon — long-running agent hub (spawns pi --mode rpc workers, API, broker TTS, Telegram)"
}

// silence unused import warning for json! in some builds
#[allow(dead_code)]
fn _unused(_v: serde_json::Value) {
    let _ = json!(null);
}