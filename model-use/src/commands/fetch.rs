use anyhow::Result;

use crate::commands::AppContext;
use crate::output::Envelope;
use crate::providers::{fetch_provider, provider_enabled, provider_has_key};
use crate::providers::types::Provider;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchMode {
    /// Print progress to stdout/stderr (CLI).
    Cli,
    /// No stdout/stderr; caller formats status (TUI).
    Quiet,
}

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub buckets_upserted: usize,
    pub providers_fetched: Vec<String>,
    /// Actionable problems (fetch failures, enabled-but-no-key).
    pub errors: Vec<String>,
    /// Non-fatal hints for CLI `--json` only.
    pub hints: Vec<String>,
}

impl FetchOutcome {
    pub fn tui_status(&self) -> String {
        if let Some(err) = self.errors.first() {
            return truncate_status(err, 72);
        }
        if self.buckets_upserted == 0 && self.providers_fetched.is_empty() {
            return "no data — configure and enable a provider".into();
        }
        let providers = if self.providers_fetched.is_empty() {
            "0 providers".into()
        } else {
            self.providers_fetched.join(", ")
        };
        format!(
            "refreshed · {} buckets from {}",
            self.buckets_upserted, providers
        )
    }
}

pub async fn run(ctx: &mut AppContext, days: i64, mode: FetchMode) -> Result<FetchOutcome> {
    let store = Store::open(&ctx.cache_path)?;
    let mut all_buckets = Vec::new();
    let mut fetched = Vec::new();
    let mut errors = Vec::new();
    let mut hints = Vec::new();

    for provider in Provider::all() {
        if !provider_enabled(&ctx.config, provider) {
            // Disabled without a key is normal — not worth warning about.
            if provider_has_key(&ctx.config, provider) {
                hints.push(format!(
                    "{provider}: disabled but key configured — run `model-use providers enable {provider}`"
                ));
            }
            continue;
        }
        if !provider_has_key(&ctx.config, provider) {
            errors.push(format!("{provider}: enabled but no API key configured"));
            continue;
        }
        match fetch_provider(&ctx.config, provider, days).await {
            Ok(buckets) => {
                fetched.push(provider.to_string());
                all_buckets.extend(buckets);
            }
            Err(e) => errors.push(format!("{provider}: {e:#}")),
        }
    }

    let count = store.upsert_buckets(&all_buckets)?;
    let outcome = FetchOutcome {
        buckets_upserted: count,
        providers_fetched: fetched,
        errors: errors.clone(),
        hints: hints.clone(),
    };

    if mode == FetchMode::Cli {
        let data = FetchResult {
            providers_fetched: outcome.providers_fetched.clone(),
            buckets_upserted: count,
            warnings: errors
                .iter()
                .chain(hints.iter())
                .cloned()
                .collect(),
        };

        if ctx.json {
            let warnings: Vec<String> = errors
                .iter()
                .chain(hints.iter())
                .cloned()
                .collect();
            Envelope::ok("fetch", data)
                .with_warnings(warnings)
                .print_json()?;
        } else {
            println!(
                "Fetched {} buckets from {} provider(s)",
                count,
                outcome.providers_fetched.len()
            );
            for w in &errors {
                eprintln!("error: {w}");
            }
            for h in &hints {
                eprintln!("hint: {h}");
            }
        }
    }

    Ok(outcome)
}

fn truncate_status(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[derive(serde::Serialize)]
struct FetchResult {
    pub providers_fetched: Vec<String>,
    pub buckets_upserted: usize,
    pub warnings: Vec<String>,
}
