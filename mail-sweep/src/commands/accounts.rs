use anyhow::{Context, Result};

use crate::cli::AccountsCommands;
use crate::commands::CommandContext;
use crate::config::{
    gmail_account, icloud_account, save_config_file, AccountAuthMethod, AccountConfig,
};
use crate::mail::google_oauth;
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
            google_oauth,
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
            *google_oauth,
            *icloud,
            password.as_deref(),
        ),
        AccountsCommands::Test { id } => run_test(ctx, id).await,
        AccountsCommands::GoogleLogin { id, email, add, skip_test } => {
            run_google_login(ctx, id, email.as_deref(), *add, *skip_test).await
        }
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
                "auth": auth_label(a.auth),
                "credentials_set": ctx.app.account_auth_ready(a),
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
        let creds = if ctx.app.account_auth_ready(a) {
            "credentials set"
        } else if a.auth == AccountAuthMethod::GoogleOauth {
            "Google sign-in needed"
        } else {
            "password missing"
        };
        println!(
            "{} — {} (imap {}:{}, smtp {}:{}, {}) · {}",
            a.id,
            a.email,
            a.imap_host,
            a.imap_port,
            a.smtp_host,
            a.smtp_port,
            auth_label(a.auth),
            creds
        );
    }

    Ok(())
}

fn auth_label(auth: AccountAuthMethod) -> &'static str {
    match auth {
        AccountAuthMethod::Password => "password",
        AccountAuthMethod::GoogleOauth => "google_oauth",
    }
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
    google_oauth: bool,
    icloud: bool,
    password: Option<&str>,
) -> Result<()> {
    if gmail && icloud {
        anyhow::bail!("use either --gmail or --icloud, not both");
    }
    if google_oauth && !gmail {
        anyhow::bail!("--google-oauth requires --gmail");
    }
    if google_oauth && password.is_some() {
        anyhow::bail!("use either --google-oauth or --password, not both");
    }

    let mut config = ctx.app.config.clone();
    if config.accounts.iter().any(|a| a.id == id) {
        anyhow::bail!("account id '{id}' already exists");
    }

    let auth = if google_oauth {
        AccountAuthMethod::GoogleOauth
    } else {
        AccountAuthMethod::Password
    };

    let mut account = if gmail {
        gmail_account(id, email, auth)
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
            auth,
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
                "auth": auth_label(auth),
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
        if google_oauth {
            println!(
                "Next: mail-sweep secrets set-google-oauth --client-id ... --client-secret ... \
                 (Google Cloud → APIs & Services → Credentials → Desktop app)"
            );
            println!("Then: mail-sweep accounts google-login --id {id}");
        } else if password.is_none() {
            println!("Set password: mail-sweep secrets set-account --id {id} --password <pass>");
        }
    }

    Ok(())
}

async fn run_google_login(
    ctx: &mut CommandContext,
    id: &str,
    email: Option<&str>,
    add: bool,
    skip_test: bool,
) -> Result<()> {
    let login_hint = ensure_gmail_oauth_account(ctx, id, email, add)?;
    google_oauth::run_browser_login(&ctx.app.secrets_path, id, login_hint.as_deref()).await?;
    ctx.app.reload_config()?;

    let account = ctx.app.account_by_id(id)?;

    if skip_test {
        if ctx.json {
            Envelope::ok(
                "accounts google-login",
                serde_json::json!({
                    "id": id,
                    "email": account.email,
                    "imap_ok": null,
                    "skipped_imap_test": true,
                }),
            )
            .print_json()?;
        } else {
            println!(
                "Google sign-in saved for '{id}' ({}). Run `mail-sweep accounts test {id}` to verify IMAP.",
                account.email
            );
        }
        return Ok(());
    }

    eprintln!("Testing IMAP connection (up to {}s)…", ctx.app.config.sync.imap_timeout_secs);
    let timeout_secs = ctx.app.config.sync.imap_timeout_secs;
    let credentials = ctx.app.resolve_mail_credentials(account).await?;
    let result = imap::test_account(account, &credentials, timeout_secs).await;

    if ctx.json {
        Envelope::ok(
            "accounts google-login",
            serde_json::json!({
                "id": id,
                "email": account.email,
                "imap_ok": result.ok,
                "message_count": result.message_count,
            }),
        )
        .print_json()?;
    } else if result.ok {
        println!(
            "Google sign-in OK for '{id}' ({}) — inbox {} messages",
            account.email,
            result.message_count.unwrap_or(0)
        );
    } else {
        println!(
            "Signed in, but IMAP test failed: {}",
            result.error.unwrap_or_else(|| "unknown error".into())
        );
    }

    Ok(())
}

fn ensure_gmail_oauth_account(
    ctx: &mut CommandContext,
    id: &str,
    email: Option<&str>,
    add: bool,
) -> Result<Option<String>> {
    if let Ok(existing) = ctx.app.account_by_id(id) {
        if existing.auth != AccountAuthMethod::GoogleOauth {
            anyhow::bail!(
                "account '{id}' is not configured for Google OAuth; add with --gmail --google-oauth or set auth = \"google_oauth\" in config.toml"
            );
        }
        return Ok(Some(existing.email.clone()));
    }

    if !add {
        anyhow::bail!(
            "unknown account id '{id}'; add first with `mail-sweep accounts add --id {id} --email you@gmail.com --gmail --google-oauth` or pass --add --email"
        );
    }

    let email = email
        .filter(|e| !e.is_empty())
        .context("--email is required with --add when the account does not exist")?;

    run_add(
        ctx,
        id,
        email,
        "imap.gmail.com",
        993,
        "smtp.gmail.com",
        587,
        true,
        true,
        false,
        None,
    )?;
    Ok(Some(email.to_string()))
}

async fn run_test(ctx: &CommandContext, id: &str) -> Result<()> {
    let account = ctx.app.account_by_id(id)?;
    let timeout_secs = ctx.app.config.sync.imap_timeout_secs;
    if !ctx.json {
        eprintln!(
            "Testing IMAP {}:{} as {} (timeout {}s)…",
            account.imap_host, account.imap_port, account.email, timeout_secs
        );
    }
    let credentials = ctx.app.resolve_mail_credentials(account).await?;
    let result = imap::test_account(account, &credentials, timeout_secs).await;

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
