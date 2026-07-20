use anyhow::Result;

use crate::cli::AccountsCommands;
use crate::commands::CommandContext;
use crate::config::{gmail_account, icloud_account, save_config_file, AccountConfig};
use crate::mail::imap;
use crate::output::Envelope;

pub async fn run(ctx: &mut CommandContext, command: &AccountsCommands) -> Result<()> {
    match command {
        AccountsCommands::List => run_list(ctx),
        AccountsCommands::Add {
            id,
            email,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            gmail,
            icloud,
            password,
        } => run_add(
            ctx,
            id,
            email,
            imap_host,
            *imap_port,
            smtp_host,
            *smtp_port,
            *gmail,
            *icloud,
            password.as_deref(),
        ),
        AccountsCommands::Test { id } => run_test(ctx, id).await,
    }
}

fn run_list(ctx: &CommandContext) -> Result<()> {
    let accounts: Vec<_> = ctx
        .app
        .config
        .accounts
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "email": a.email,
                "imap": format!("{}:{}", a.imap_host, a.imap_port),
                "smtp": format!("{}:{}", a.smtp_host, a.smtp_port),
                "inbox_folder": a.inbox_folder,
                "password_set": ctx.app.resolve_password(a).is_ok(),
            })
        })
        .collect();

    if ctx.json {
        Envelope::ok("accounts list", accounts).print_json()?;
        return Ok(());
    }

    if accounts.is_empty() {
        println!("No accounts configured. Add one with `mail-sweep accounts add`.");
        return Ok(());
    }

    for a in &ctx.app.config.accounts {
        let pw = if ctx.app.resolve_password(a).is_ok() {
            "password set"
        } else {
            "password missing"
        };
        println!(
            "{} — {} (imap {}:{}, smtp {}:{}) · {}",
            a.id, a.email, a.imap_host, a.imap_port, a.smtp_host, a.smtp_port, pw
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_add(
    ctx: &mut CommandContext,
    id: &str,
    email: &str,
    imap_host: &str,
    imap_port: u16,
    smtp_host: &str,
    smtp_port: u16,
    gmail: bool,
    icloud: bool,
    password: Option<&str>,
) -> Result<()> {
    if gmail && icloud {
        anyhow::bail!("use either --gmail or --icloud, not both");
    }

    let mut config = ctx.app.config.clone();
    if config.accounts.iter().any(|a| a.id == id) {
        anyhow::bail!("account id '{id}' already exists");
    }

    let mut account = if gmail {
        gmail_account(id, email)
    } else if icloud {
        icloud_account(id, email)
    } else {
        AccountConfig {
            id: id.into(),
            email: email.into(),
            imap_host: imap_host.into(),
            imap_port,
            smtp_host: smtp_host.into(),
            smtp_port,
            password: None,
            inbox_folder: "INBOX".into(),
            archive_folder: "Archive".into(),
            spam_folder: "Spam".into(),
        }
    };

    if !gmail && !icloud {
        account.imap_host = imap_host.into();
        account.imap_port = imap_port;
        account.smtp_host = smtp_host.into();
        account.smtp_port = smtp_port;
    }

    config.accounts.push(account);
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;

    if let Some(password) = password {
        ctx.app.set_account_password(id, password.to_string())?;
    }

    if ctx.json {
        Envelope::ok(
            "accounts add",
            serde_json::json!({
                "id": id,
                "email": email,
                "provider": if gmail { "gmail" } else if icloud { "icloud" } else { "custom" },
            }),
        )
        .print_json()?;
    } else {
        println!("Added account '{id}' ({email})");
        if icloud {
            println!(
                "iCloud requires an app-specific password from https://appleid.apple.com \
                 (Sign-In and Security → App-Specific Passwords)."
            );
        }
        if password.is_none() {
            println!("Set password: mail-sweep secrets set-account --id {id} --password <pass>");
        }
    }

    Ok(())
}

async fn run_test(ctx: &CommandContext, id: &str) -> Result<()> {
    let account = ctx.app.account_by_id(id)?;
    let password = ctx.app.resolve_password(account)?;
    let result = imap::test_account(account, &password).await;

    if ctx.json {
        Envelope::ok("accounts test", result).print_json()?;
    } else if result.ok {
        println!(
            "OK — {} inbox {} messages, capabilities: {}",
            id,
            result.message_count.unwrap_or(0),
            result.capabilities.join(", ")
        );
    } else {
        eprintln!(
            "FAIL — {}: {}",
            id,
            result.error.unwrap_or_else(|| "unknown error".into())
        );
    }

    Ok(())
}
