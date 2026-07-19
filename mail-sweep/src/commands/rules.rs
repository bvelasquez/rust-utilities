use anyhow::Result;
use crate::config::save_config_file;

use crate::agent::rules_audit::{apply_audit_suggestions, audit_rules};
use crate::cli::RulesCommands;
use crate::commands::CommandContext;
use crate::config::RuleConfig;
use crate::output::Envelope;
use crate::rules::test_rule;
use crate::store::Store;

pub async fn run(ctx: &mut CommandContext, command: &RulesCommands) -> Result<()> {
    match command {
        RulesCommands::List => run_list(ctx),
        RulesCommands::Add {
            r#match,
            action,
            category,
            priority,
            target_folder,
        } => run_add(ctx, r#match, action, category.as_deref(), *priority, target_folder.as_deref()),
        RulesCommands::Update {
            index,
            r#match,
            action,
            category,
            priority,
            target_folder,
        } => run_update(
            ctx,
            *index,
            r#match.as_deref(),
            action.as_deref(),
            category.as_deref(),
            *priority,
            target_folder.as_deref(),
        ),
        RulesCommands::Remove { index } => run_remove(ctx, *index),
        RulesCommands::Test {
            from,
            subject,
            headers,
        } => run_test(ctx, from, subject, headers),
        RulesCommands::Audit { yes } => run_audit(ctx, *yes).await,
    }
}

fn run_list(ctx: &CommandContext) -> Result<()> {
    if ctx.json {
        Envelope::ok("rules list", &ctx.app.config.rules).print_json()?;
        return Ok(());
    }

    if ctx.app.config.rules.is_empty() {
        println!("No rules configured.");
        return Ok(());
    }

    for (i, r) in ctx.app.config.rules.iter().enumerate() {
        println!(
            "[{i}] match={} action={} category={}",
            r.r#match,
            r.action,
            r.category.as_deref().unwrap_or("-")
        );
    }

    Ok(())
}

fn run_add(
    ctx: &mut CommandContext,
    pattern: &str,
    action: &str,
    category: Option<&str>,
    priority: Option<u8>,
    target_folder: Option<&str>,
) -> Result<()> {
    let mut config = ctx.app.config.clone();
    config.rules.push(RuleConfig {
        id: None,
        r#match: pattern.into(),
        category: category.map(|s| s.into()),
        action: action.into(),
        priority,
        target_folder: target_folder.map(|s| s.into()),
    });
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;

    if ctx.json {
        Envelope::ok("rules add", serde_json::json!({ "match": pattern, "action": action })).print_json()?;
    } else {
        println!("Added rule: {pattern} → {action}");
    }

    Ok(())
}

fn run_update(
    ctx: &mut CommandContext,
    index: usize,
    pattern: Option<&str>,
    action: Option<&str>,
    category: Option<&str>,
    priority: Option<u8>,
    target_folder: Option<&str>,
) -> Result<()> {
    let mut config = ctx.app.config.clone();
    if index >= config.rules.len() {
        anyhow::bail!("rule index {index} out of range");
    }
    let rule = &mut config.rules[index];
    if let Some(m) = pattern {
        rule.r#match = m.into();
    }
    if let Some(a) = action {
        rule.action = a.into();
    }
    if let Some(c) = category {
        rule.category = Some(c.into());
    }
    if let Some(p) = priority {
        rule.priority = Some(p);
    }
    if let Some(f) = target_folder {
        rule.target_folder = Some(f.into());
    }
    let updated = rule.clone();
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;

    if ctx.json {
        Envelope::ok("rules update", updated).print_json()?;
    } else {
        println!("Updated rule [{index}]");
    }

    Ok(())
}

fn run_remove(ctx: &mut CommandContext, index: usize) -> Result<()> {
    let mut config = ctx.app.config.clone();
    if index >= config.rules.len() {
        anyhow::bail!("rule index {index} out of range");
    }
    let removed = config.rules.remove(index);
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;

    if ctx.json {
        Envelope::ok("rules remove", removed).print_json()?;
    } else {
        println!("Removed rule [{index}]");
    }

    Ok(())
}

fn run_test(ctx: &CommandContext, from: &str, subject: &str, headers: &str) -> Result<()> {
    let mut matches = Vec::new();
    for rule in &ctx.app.config.rules {
        if test_rule(from, subject, headers, rule) {
            matches.push(rule);
        }
    }

    if ctx.json {
        Envelope::ok("rules test", matches).print_json()?;
        return Ok(());
    }

    if matches.is_empty() {
        println!("No rules matched.");
    } else {
        println!("Matched {} rules:", matches.len());
        for r in matches {
            println!("  {} → {}", r.r#match, r.action);
        }
    }

    Ok(())
}

async fn run_audit(ctx: &mut CommandContext, yes: bool) -> Result<()> {
    let store = Store::open(&ctx.app.db_path())?;
    let plan = audit_rules(&ctx.app, &ctx.app.config.rules, &store).await?;

    if yes {
        let new_rules = apply_audit_suggestions(&ctx.app.config.rules, &plan.suggestions);
        let mut config = ctx.app.config.clone();
        config.rules = new_rules;
        save_config_file(&ctx.app.config_path, &config)?;
        ctx.app.config = config;
        if ctx.json {
            Envelope::ok(
                "rules audit applied",
                serde_json::json!({
                    "summary": plan.summary,
                    "applied": plan.suggestions.len(),
                    "rules": ctx.app.config.rules,
                }),
            )
            .print_json()?;
        } else {
            println!("Applied {} suggestions: {}", plan.suggestions.len(), plan.summary);
        }
        return Ok(());
    }

    if ctx.json {
        Envelope::ok("rules audit", &plan).print_json()?;
    } else {
        println!("{}", plan.summary);
        for (i, s) in plan.suggestions.iter().enumerate() {
            println!(
                "[{i}] {} ({:.0}%): {}",
                s.kind,
                s.confidence * 100.0,
                s.reason
            );
            for r in &s.proposed_rules {
                println!("    + {} → {}", r.r#match, r.action);
            }
            if !s.retire_indices.is_empty() {
                println!("    - retire: {:?}", s.retire_indices);
            }
        }
        println!("\nRun with --yes to apply all suggestions.");
    }

    Ok(())
}
