use anyhow::Result;

use crate::cli::OutputCategory;
use crate::commands::CommandContext;
use crate::output::Envelope;
use crate::store::Store;

pub fn run(
    ctx: &CommandContext,
    account: Option<String>,
    category: Option<OutputCategory>,
    priority: Option<u8>,
    unread: bool,
    limit: usize,
) -> Result<()> {
    let store = Store::open(&ctx.app.db_path())?;
    let messages = store.list_messages(
        account.as_deref(),
        category.as_ref().map(|c| c.as_str()),
        priority,
        unread,
        limit,
    )?;

    if ctx.json {
        Envelope::ok("list", messages)
            .with_next_actions(vec!["mail-sweep show <id> --json".into()])
            .print_json()?;
        return Ok(());
    }

    if messages.is_empty() {
        println!("No messages in cache. Run `mail-sweep sync` first.");
        return Ok(());
    }

    for m in &messages {
        let flag = if m.is_flagged { "*" } else { " " };
        let unread_mark = if m.is_unread { "U" } else { " " };
        println!(
            "{flag}{unread_mark} [{:>1}] P{} {:<12} {:<20} {}",
            m.id,
            m.priority,
            m.category,
            truncate(&m.from_address, 20),
            truncate(&m.subject, 50),
        );
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
