use anyhow::Result;

use crate::cli::LearnCommands;
use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::store::Store;

pub fn run(ctx: &mut CommandContext, command: &LearnCommands) -> Result<()> {
    match command {
        LearnCommands::Feedback {
            sender,
            action,
            category,
            priority,
        } => run_feedback(ctx, sender, action, category.as_deref(), *priority),
    }
}

fn run_feedback(
    ctx: &mut CommandContext,
    sender: &str,
    action: &str,
    category: Option<&str>,
    priority: u8,
) -> Result<()> {
    if action == "delete" {
        crate::process::teach_junk_sender(ctx, sender)?;
    } else {
        let store = Store::open(&ctx.app.db_path())?;
        store.add_learning(sender, action, category, priority, "cli")?;
    }

    if ctx.json {
        Envelope::ok(
            "learn feedback",
            serde_json::json!({ "sender": sender, "action": action, "category": category, "priority": priority }),
        )
        .print_json()?;
    } else {
        println!("Recorded feedback for {sender}: {action}");
    }

    Ok(())
}
