use anyhow::Result;

use crate::budget::{budgets_list_data, set_global_budget, set_provider_budget};
use crate::commands::AppContext;
use crate::output::Envelope;
use crate::providers::types::Provider;

pub fn run_set(ctx: &mut AppContext, target: &str, monthly: f64) -> Result<()> {
    if target.eq_ignore_ascii_case("global") {
        set_global_budget(&mut ctx.config, monthly);
    } else {
        let provider: Provider = target.parse()?;
        set_provider_budget(&mut ctx.config, provider, monthly);
    }
    crate::config::ModelUseConfig::save(&ctx.config_path, &ctx.config)?;

    if ctx.json {
        Envelope::ok(
            "budget set",
            serde_json::json!({ "target": target, "monthly_usd": monthly }),
        )
        .print_json()?;
    } else {
        println!("Set {target} monthly budget to ${monthly:.2}");
    }
    Ok(())
}

pub fn run_list(ctx: &AppContext) -> Result<()> {
    let data = budgets_list_data(&ctx.config);
    if ctx.json {
        Envelope::ok("budget list", data).print_json()?;
    } else {
        print_budgets(&data);
    }
    Ok(())
}

fn print_budgets(b: &crate::config::BudgetsConfig) {
    println!("Budgets (monthly USD):");
    match b.global_monthly_usd {
        Some(v) => println!("  global:   ${v:.2}"),
        None => println!("  global:   (not set)"),
    }
    for (name, val) in [
        ("openrouter", b.openrouter.monthly_usd),
        ("anthropic", b.anthropic.monthly_usd),
        ("openai", b.openai.monthly_usd),
        ("cursor", b.cursor.monthly_usd),
    ] {
        match val {
            Some(v) => println!("  {name}: ${v:.2}"),
            None => println!("  {name}: (not set)"),
        }
    }
}
