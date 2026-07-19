use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::Serialize;

use crate::config::AccountConfig;

#[derive(Debug, Clone, Serialize)]
pub struct SendResult {
    pub account_id: String,
    pub to: String,
    pub subject: String,
    pub sent: bool,
    pub dry_run: bool,
}

pub fn send_mail(
    account: &AccountConfig,
    password: &str,
    to: &str,
    subject: &str,
    body: &str,
    dry_run: bool,
) -> Result<SendResult> {
    let from: Mailbox = account
        .email
        .parse()
        .with_context(|| format!("parse from address {}", account.email))?;
    let to_mailbox: Mailbox = to
        .parse()
        .with_context(|| format!("parse to address {to}"))?;

    if dry_run {
        return Ok(SendResult {
            account_id: account.id.clone(),
            to: to.into(),
            subject: subject.into(),
            sent: false,
            dry_run: true,
        });
    }

    let email = Message::builder()
        .from(from)
        .to(to_mailbox)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())?;

    let creds = Credentials::new(account.email.clone(), password.to_string());
    let mailer = SmtpTransport::relay(&account.smtp_host)?
        .port(account.smtp_port)
        .credentials(creds)
        .build();

    mailer.send(&email)?;

    Ok(SendResult {
        account_id: account.id.clone(),
        to: to.into(),
        subject: subject.into(),
        sent: true,
        dry_run: false,
    })
}
