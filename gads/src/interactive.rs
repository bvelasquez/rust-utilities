use anyhow::Result;
use chrono::{Duration, Utc};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use std::path::PathBuf;

use crate::api::AdsClient;
use crate::auth;
use crate::config::{default_credentials_path, find_project_config, ProjectConfig};
use crate::ops::ReadContext;
use crate::output::Envelope;
use crate::util::normalize_customer_id;

pub struct Session {
    pub customer_id: Option<String>,
    pub credentials: Option<PathBuf>,
    pub project: Option<ProjectConfig>,
}

impl Session {
    pub fn resolve_customer(&self, explicit: Option<&str>) -> Result<String> {
        if let Some(id) = explicit {
            if let Some(proj) = &self.project {
                return Ok(normalize_customer_id(proj.resolve_customer(id)));
            }
            return Ok(normalize_customer_id(id));
        }
        if let Some(id) = &self.customer_id {
            return Ok(normalize_customer_id(id));
        }
        if let Some(proj) = &self.project {
            if let Some(id) = &proj.default_customer_id {
                return Ok(normalize_customer_id(id));
            }
        }
        anyhow::bail!("no customer ID — set one in the session or pass --customer")
    }
}

pub async fn run(initial_customer: Option<String>, credentials: Option<PathBuf>) -> Result<()> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        anyhow::bail!("interactive mode requires a terminal (try: gads interactive)");
    }

    let project = find_project_config(&std::env::current_dir()?)
        .and_then(|p| ProjectConfig::load(&p).ok());

    let mut session = Session {
        customer_id: initial_customer,
        credentials,
        project,
    };

    print_banner();

    loop {
        println!();
        let label = session
            .customer_id
            .as_deref()
            .unwrap_or("(not set)");
        println!("{} {}", "Customer:".bold(), label.dimmed());
        println!("{}", "─".repeat(48).dimmed());

        let items = [
            "Accounts & customers",
            "Campaigns & budgets",
            "Ad groups, ads & keywords",
            "Performance & stats",
            "Conversion tags & assets",
            "Raw GAQL query",
            "Auth status",
            "Set customer ID",
            "Quit",
        ];

        let choice = Select::new()
            .with_prompt("What would you like to do?")
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(choice) = choice else {
            break;
        };

        if choice == items.len() - 1 {
            break;
        }

        let result = match choice {
            0 => menu_accounts(&session).await,
            1 => menu_campaigns(&session).await,
            2 => menu_ads(&session).await,
            3 => menu_stats(&session).await,
            4 => menu_conversions(&session).await,
            5 => menu_query(&session).await,
            6 => menu_auth(&session).await,
            7 => menu_set_customer(&mut session),
            _ => Ok(()),
        };

        if let Err(e) = result {
            eprintln!("{} {e:#}", "Error:".red().bold());
            let cont = Confirm::new()
                .with_prompt("Continue?")
                .default(true)
                .interact()?;
            if !cont {
                break;
            }
        }
    }

    Ok(())
}

fn print_banner() {
    println!();
    println!("{}", "gads — Google Ads".bold().cyan());
    println!("{}", "Interactive mode for humans. Agents: use --json subcommands.".dimmed());
}

async fn client_for(session: &Session) -> Result<AdsClient> {
    let creds = auth::load_credentials(session.credentials.as_deref()).await?;
    Ok(AdsClient::new(creds))
}

async fn menu_accounts(session: &Session) -> Result<()> {
    let client = client_for(session).await?;
    let data = client.list_accessible_customers().await?;
    print_pretty_json(&data)?;
    Ok(())
}

async fn menu_campaigns(session: &Session) -> Result<()> {
    let cid = session.resolve_customer(None)?;
    let client = client_for(session).await?;
    let ctx = ReadContext {
        client: &client,
        customer_id: &cid,
    };
    let data = ctx.campaigns(None, 50).await?;
    print_pretty_json(&data)?;
    Ok(())
}

async fn menu_ads(session: &Session) -> Result<()> {
    let cid = session.resolve_customer(None)?;
    let client = client_for(session).await?;
    let ctx = ReadContext {
        client: &client,
        customer_id: &cid,
    };
    let data = ctx.ad_groups(None, None, 50).await?;
    print_pretty_json(&data)?;
    Ok(())
}

async fn menu_stats(session: &Session) -> Result<()> {
    let cid = session.resolve_customer(None)?;
    let end = Utc::now().date_naive();
    let start = end - Duration::days(30);
    let client = client_for(session).await?;
    let ctx = ReadContext {
        client: &client,
        customer_id: &cid,
    };
    let data = ctx
        .performance_summary(&start.to_string(), &end.to_string())
        .await?;
    print_pretty_json(&data)?;
    Ok(())
}

async fn menu_conversions(session: &Session) -> Result<()> {
    let domain: String = Input::new()
        .with_prompt("Filter by domain (empty = all)")
        .allow_empty(true)
        .interact_text()?;
    let cid = session.resolve_customer(None)?;
    let client = client_for(session).await?;
    let ctx = ReadContext {
        client: &client,
        customer_id: &cid,
    };
    let domain_opt = if domain.trim().is_empty() {
        None
    } else {
        Some(domain.trim())
    };
    let data = ctx.conversion_tags(domain_opt).await?;
    print_pretty_json(&data)?;
    Ok(())
}

async fn menu_query(session: &Session) -> Result<()> {
    let gaql: String = Input::new()
        .with_prompt("GAQL query")
        .interact_text()?;
    let cid = session.resolve_customer(None)?;
    let client = client_for(session).await?;
    let ctx = ReadContext {
        client: &client,
        customer_id: &cid,
    };
    let data = ctx.raw_query(&gaql).await?;
    print_pretty_json(&data)?;
    Ok(())
}

async fn menu_auth(session: &Session) -> Result<()> {
    let creds = auth::load_credentials(session.credentials.as_deref()).await?;
    let status = auth::auth_status(&creds);
    print_pretty_json(&status)?;
    println!(
        "{}",
        format!("Credentials: {}", default_credentials_path().display()).dimmed()
    );
    Ok(())
}

fn menu_set_customer(session: &mut Session) -> Result<()> {
    let id: String = Input::new()
        .with_prompt("Customer ID")
        .interact_text()?;
    session.customer_id = Some(normalize_customer_id(&id));
    Ok(())
}

fn print_pretty_json(data: &serde_json::Value) -> Result<()> {
    let env = Envelope::ok("interactive", data);
    env.print_json()
}
