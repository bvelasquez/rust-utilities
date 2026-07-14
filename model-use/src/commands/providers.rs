use anyhow::Result;
use serde::Serialize;

use crate::commands::AppContext;
use crate::output::Envelope;
use crate::providers::types::Provider;

#[derive(Serialize)]
struct ProviderListItem {
    provider: String,
    enabled: bool,
    has_key: bool,
    key_type_hint: String,
    docs_url: String,
}

pub async fn run_list(ctx: &AppContext) -> Result<()> {
    let items: Vec<ProviderListItem> = Provider::all()
        .iter()
        .map(|p| ProviderListItem {
            provider: p.to_string(),
            enabled: crate::providers::provider_enabled(&ctx.config, *p),
            has_key: crate::providers::provider_has_key(&ctx.config, *p),
            key_type_hint: p.key_hint().into(),
            docs_url: p.docs_url().into(),
        })
        .collect();

    if ctx.json {
        Envelope::ok("providers list", items).print_json()?;
    } else {
        for item in &items {
            let key = if item.has_key { "configured" } else { "missing" };
            let status = if item.enabled { "enabled" } else { "disabled" };
            println!(
                "{}: {} · key {} · {}",
                item.provider, status, key, item.key_type_hint
            );
        }
    }
    Ok(())
}

pub fn run_set(
    ctx: &mut AppContext,
    provider: &str,
    key: &str,
    email: Option<&str>,
) -> Result<()> {
    let p: Provider = provider.parse()?;
    match p {
        Provider::Openrouter => {
            ctx.config.openrouter.api_key = Some(key.to_string());
            ctx.config.openrouter.enabled = true;
        }
        Provider::Anthropic => {
            ctx.config.anthropic.api_key = Some(key.to_string());
            ctx.config.anthropic.enabled = true;
        }
        Provider::Openai => {
            ctx.config.openai.api_key = Some(key.to_string());
            ctx.config.openai.enabled = true;
        }
        Provider::Cursor => {
            ctx.config.cursor.api_key = Some(key.to_string());
            ctx.config.cursor.enabled = true;
            if let Some(e) = email {
                ctx.config.cursor.email = Some(e.to_string());
            }
        }
    }
    crate::config::ModelUseConfig::save(&ctx.config_path, &ctx.config)?;

    if ctx.json {
        Envelope::ok(
            "providers set",
            serde_json::json!({ "provider": provider, "saved": true }),
        )
        .print_json()?;
    } else {
        println!("Saved {provider} API key to {} (enabled)", ctx.config_path.display());
    }
    Ok(())
}

pub async fn run_test(ctx: &AppContext, provider: Option<&str>) -> Result<()> {
    let providers: Vec<Provider> = match provider {
        Some(p) => vec![p.parse()?],
        None => Provider::all().to_vec(),
    };

    let mut results = Vec::new();
    for p in providers {
        results.push(crate::providers::test_provider(&ctx.config, p).await);
    }

    if ctx.json {
        Envelope::ok("providers test", &results).print_json()?;
    } else {
        for r in &results {
            let mark = if r.ok { "ok" } else { "FAIL" };
            println!("{}: [{mark}] {}", r.provider, r.message);
            if !r.ok {
                println!("  hint: {}", r.key_type_hint);
                println!("  docs: {}", r.docs_url);
            }
        }
    }
    Ok(())
}

pub fn run_enable(ctx: &mut AppContext, provider: &str, enabled: bool) -> Result<()> {
    let p: Provider = provider.parse()?;
    match p {
        Provider::Openrouter => ctx.config.openrouter.enabled = enabled,
        Provider::Anthropic => ctx.config.anthropic.enabled = enabled,
        Provider::Openai => ctx.config.openai.enabled = enabled,
        Provider::Cursor => ctx.config.cursor.enabled = enabled,
    }
    crate::config::ModelUseConfig::save(&ctx.config_path, &ctx.config)?;
    let verb = if enabled { "enabled" } else { "disabled" };
    if ctx.json {
        Envelope::ok(
            if enabled {
                "providers enable"
            } else {
                "providers disable"
            },
            serde_json::json!({ "provider": provider, "enabled": enabled }),
        )
        .print_json()?;
    } else {
        println!("{provider} {verb}");
    }
    Ok(())
}
