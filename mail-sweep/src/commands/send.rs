use anyhow::Result;

use crate::commands::CommandContext;
use crate::mail::smtp;
use crate::output::Envelope;
use crate::safety::confirm_mutation;

pub async fn run(
    ctx: &CommandContext,
    account_id: &str,
    to: &str,
    subject: &str,
    body: &str,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let account = ctx.app.account_by_id(account_id)?;
    let credentials = ctx.app.resolve_mail_credentials(account).await?;

    confirm_mutation(
        dry_run,
        yes,
        &format!("Send email to {to}? Type 'yes' to confirm: "),
    )?;

    let result = smtp::send_mail(account, &credentials, to, subject, body, dry_run)?;

    if ctx.json {
        Envelope::ok("send", result).print_json()?;
        return Ok(());
    }

    if result.dry_run {
        println!("Would send to {to}: {subject}");
    } else {
        println!("Sent to {to}");
    }

    Ok(())
}
