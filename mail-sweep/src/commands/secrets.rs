use anyhow::Result;

use crate::cli::SecretsCommands;
use crate::config::AccountAuthMethod;
use crate::commands::CommandContext;
use crate::output::Envelope;

pub fn run(ctx: &mut CommandContext, command: &SecretsCommands) -> Result<()> {
    match command {
        SecretsCommands::List => run_list(ctx),
        SecretsCommands::SetOpenrouterKey { key } => run_set_openrouter_key(ctx, key),
        SecretsCommands::SetLlmModel { model } => run_set_llm_model(ctx, model),
        SecretsCommands::SetAccount { id, password } => run_set_account(ctx, id, password),
        SecretsCommands::SetGoogleOauth {
            client_id,
            client_secret,
        } => run_set_google_oauth(ctx, client_id, client_secret),
    }
}

fn run_set_openrouter_key(ctx: &mut CommandContext, key: &str) -> Result<()> {
    ctx.app.set_openrouter_key(key.to_string())?;
    if ctx.json {
        Envelope::ok(
            "secrets set openrouter-key",
            serde_json::json!({ "saved": true }),
        )
        .print_json()?;
    } else {
        println!("Saved OpenRouter key to {}", ctx.app.secrets_path.display());
    }
    Ok(())
}

fn run_set_llm_model(ctx: &mut CommandContext, model: &str) -> Result<()> {
    ctx.app.set_llm_model(model.to_string())?;
    if ctx.json {
        Envelope::ok(
            "secrets set llm-model",
            serde_json::json!({ "model": model, "saved": true }),
        )
        .print_json()?;
    } else {
        println!(
            "Saved LLM model '{model}' to {}",
            ctx.app.secrets_path.display()
        );
    }
    Ok(())
}

fn run_set_account(ctx: &mut CommandContext, id: &str, password: &str) -> Result<()> {
    ctx.app
        .set_account_password(id, password.to_string())?;
    if ctx.json {
        Envelope::ok(
            "secrets set account",
            serde_json::json!({ "id": id, "saved": true }),
        )
        .print_json()?;
    } else {
        println!(
            "Saved password for account '{id}' to {}",
            ctx.app.secrets_path.display()
        );
    }
    Ok(())
}

fn run_set_google_oauth(
    ctx: &mut CommandContext,
    client_id: &str,
    client_secret: &str,
) -> Result<()> {
    ctx.app
        .set_google_oauth_client(client_id.to_string(), client_secret.to_string())?;
    if ctx.json {
        Envelope::ok(
            "secrets set google-oauth",
            serde_json::json!({ "saved": true }),
        )
        .print_json()?;
    } else {
        println!(
            "Saved Google OAuth client to {} — add redirect URI http://127.0.0.1 in Google Cloud if prompted",
            ctx.app.secrets_path.display()
        );
    }
    Ok(())
}

fn run_list(ctx: &CommandContext) -> Result<()> {
    let status = ctx.app.secrets_status();
    if ctx.json {
        Envelope::ok("secrets list", status).print_json()?;
        return Ok(());
    }

    println!("Secrets file: {}", ctx.app.secrets_path.display());
    println!(
        "OpenRouter key: {}",
        if status.openrouter_key_set {
            "configured"
        } else {
            "missing"
        }
    );
    if let Some(model) = &status.llm_model {
        println!("LLM model: {model}");
    }
    println!(
        "Google OAuth client: {}",
        if status.google_oauth_client_set {
            "configured"
        } else {
            "missing"
        }
    );
    for account in &status.accounts {
        let label = match account.auth {
            AccountAuthMethod::GoogleOauth => "Google sign-in",
            AccountAuthMethod::Password => "password",
        };
        let pw = if account.password_set {
            "configured"
        } else {
            "missing"
        };
        println!("Account {} {label}: {pw}", account.id);
    }
    if !status.openrouter_key_set
        || !status.google_oauth_client_set && status.accounts.iter().any(|a| a.auth == AccountAuthMethod::GoogleOauth)
        || status.accounts.iter().any(|a| !a.password_set)
    {
        println!();
        println!("Set secrets with:");
        println!("  mail-sweep secrets set-openrouter-key --key <key>");
        println!("  mail-sweep secrets set-account --id <id> --password <pass>");
        println!("  mail-sweep secrets set-google-oauth --client-id ... --client-secret ...");
        println!("  mail-sweep accounts google-login --id <id>");
        println!("Or add keys to .env (see `mail-sweep config schema --json`).");
    }

    Ok(())
}
