use anyhow::{bail, Result};

use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::store::Store;

pub fn run(ctx: &CommandContext, id: i64) -> Result<()> {
    let store = Store::open(&ctx.app.db_path())?;
    let msg = store.get_message(id)?;
    let Some(msg) = msg else {
        bail!("message {id} not found in cache");
    };

    if ctx.json {
        Envelope::ok("show", msg).print_json()?;
        return Ok(());
    }

    println!("From: {}", msg.from_address);
    if let Some(name) = &msg.from_name {
        println!("Name: {name}");
    }
    println!("Subject: {}", msg.subject);
    if let Some(date) = &msg.date {
        println!("Date: {date}");
    }
    println!("Account: {}  UID: {}", msg.account_id, msg.uid);
    println!("Category: {}  Priority: {}  Status: {}", msg.category, msg.priority, msg.status);
    if let Some(action) = &msg.planned_action {
        println!(
            "Planned: {action} (confidence {:.0}%) — {}",
            msg.plan_confidence.unwrap_or(0.0) * 100.0,
            msg.plan_reason.as_deref().unwrap_or("")
        );
    }
    println!();
    if let Some(body) = &msg.body_text {
        println!("{body}");
    } else {
        println!("{}", msg.body_preview);
    }

    Ok(())
}
