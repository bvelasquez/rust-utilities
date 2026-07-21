use anyhow::{Context, Result};
use async_imap::types::{Capability, Fetch};
use async_imap::Session;
use async_native_tls::{TlsConnector, TlsStream};
use async_std::net::TcpStream;
use futures::StreamExt;
use serde::Serialize;
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use crate::agent::schema::{MailAction, MessageDecision};
use crate::config::AccountConfig;
use crate::mail::parser::{self, ParsedMail};

pub type ImapSession = Session<TlsStream<TcpStream>>;

fn capability_name(cap: &Capability) -> String {
    match cap {
        Capability::Imap4rev1 => "IMAP4rev1".into(),
        Capability::Auth(s) => format!("AUTH={s}"),
        Capability::Atom(s) => s.clone(),
    }
}

/// Bound any IMAP await so a stalled server cannot hang the process forever.
async fn imap_timeout<T>(secs: u64, op: &str, fut: impl Future<Output = T>) -> Result<T> {
    let secs = secs.max(1);
    tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .with_context(|| format!("IMAP {op} timed out after {secs}s"))
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountTestResult {
    pub account_id: String,
    pub ok: bool,
    pub imap_ok: bool,
    pub message_count: Option<u32>,
    pub capabilities: Vec<String>,
    pub inbox_folder: String,
    pub error: Option<String>,
}

pub async fn connect(
    account: &AccountConfig,
    password: &str,
    timeout_secs: u64,
) -> Result<ImapSession> {
    let addr = format!("{}:{}", account.imap_host, account.imap_port);
    let tcp = imap_timeout(timeout_secs, &format!("connect to {addr}"), async {
        TcpStream::connect(&addr)
            .await
            .with_context(|| format!("connect to {addr}"))
    })
    .await??;

    let tls = TlsConnector::new();
    let domain = account.imap_host.as_str();
    let tls_stream = imap_timeout(timeout_secs, &format!("TLS handshake with {domain}"), async {
        tls.connect(domain, tcp)
            .await
            .with_context(|| format!("TLS handshake with {domain}"))
    })
    .await??;

    let client = async_imap::Client::new(tls_stream);
    let session = imap_timeout(timeout_secs, &format!("login for {}", account.email), async {
        client
            .login(&account.email, password)
            .await
            .map_err(|(e, _)| e)
            .with_context(|| format!("IMAP login for {}", account.email))
    })
    .await??;

    Ok(session)
}

pub async fn test_account(
    account: &AccountConfig,
    password: &str,
    timeout_secs: u64,
) -> AccountTestResult {
    match connect(account, password, timeout_secs).await {
        Ok(mut session) => {
            let caps: Vec<String> = imap_timeout(timeout_secs, "capabilities", session.capabilities())
                .await
                .ok()
                .and_then(|r| r.ok())
                .map(|c| c.iter().map(capability_name).collect())
                .unwrap_or_default();

            let select =
                imap_timeout(timeout_secs, "select inbox", session.select(&account.inbox_folder))
                    .await;
            match select {
                Ok(Ok(mailbox)) => {
                    let _ = imap_timeout(timeout_secs, "logout", session.logout()).await;
                    AccountTestResult {
                        account_id: account.id.clone(),
                        ok: true,
                        imap_ok: true,
                        message_count: mailbox.exists.checked_sub(0),
                        capabilities: caps,
                        inbox_folder: account.inbox_folder.clone(),
                        error: None,
                    }
                }
                Ok(Err(e)) => AccountTestResult {
                    account_id: account.id.clone(),
                    ok: false,
                    imap_ok: false,
                    message_count: None,
                    capabilities: caps,
                    inbox_folder: account.inbox_folder.clone(),
                    error: Some(e.to_string()),
                },
                Err(e) => AccountTestResult {
                    account_id: account.id.clone(),
                    ok: false,
                    imap_ok: false,
                    message_count: None,
                    capabilities: caps,
                    inbox_folder: account.inbox_folder.clone(),
                    error: Some(e.to_string()),
                },
            }
        }
        Err(e) => AccountTestResult {
            account_id: account.id.clone(),
            ok: false,
            imap_ok: false,
            message_count: None,
            capabilities: vec![],
            inbox_folder: account.inbox_folder.clone(),
            error: Some(e.to_string()),
        },
    }
}

pub async fn fetch_new_messages(
    account: &AccountConfig,
    password: &str,
    last_uid: u32,
    full: bool,
    preview_chars: usize,
    initial_limit: usize,
    full_limit: usize,
    timeout_secs: u64,
) -> Result<(Vec<(u32, ParsedMail, bool, bool)>, u32)> {
    let mut session = connect(account, password, timeout_secs).await?;
    imap_timeout(
        timeout_secs,
        "select inbox",
        session.select(&account.inbox_folder),
    )
    .await?
    .context("select inbox")?;

    let mut uid_set: HashSet<u32> = HashSet::new();

    if full || last_uid == 0 {
        let uids = imap_timeout(timeout_secs, "uid_search ALL", session.uid_search("ALL"))
            .await?
            .context("uid_search ALL")?;
        let mut uid_list: Vec<u32> = uids.into_iter().collect();
        uid_list.sort_unstable();
        // First sync: small recent window. --full: larger backfill. Never the whole mailbox.
        let limit = if full {
            full_limit.max(1)
        } else {
            initial_limit.max(1)
        };
        let start = uid_list.len().saturating_sub(limit);
        for u in &uid_list[start..] {
            uid_set.insert(*u);
        }
    } else {
        // New mail since last sync
        let new_uids = imap_timeout(
            timeout_secs,
            "uid_search new",
            session.uid_search(&format!("UID {}:*", last_uid.saturating_add(1))),
        )
        .await?
        .context("uid_search new")?;
        uid_set.extend(new_uids);

        // Also refresh any currently-unread messages (UIDs may already be cached).
        // Without this, marking old mail unread in Gmail never updates the local cache.
        let unseen = imap_timeout(timeout_secs, "uid_search UNSEEN", session.uid_search("UNSEEN"))
            .await?
            .context("uid_search UNSEEN")?;
        uid_set.extend(unseen);
    }

    let mut uid_list: Vec<u32> = uid_set.into_iter().collect();
    uid_list.sort_unstable();

    if uid_list.is_empty() {
        let _ = imap_timeout(timeout_secs, "logout", session.logout()).await;
        return Ok((vec![], last_uid));
    }

    let max_uid = *uid_list.iter().max().unwrap_or(&last_uid);
    // High-water mark only advances for truly new UIDs (not UNSEEN refreshes of old mail)
    let high_water = max_uid.max(last_uid);
    let uid_set_str = uid_list
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let mut out = Vec::new();
    {
        // BODY.PEEK[] — do NOT use RFC822/BODY[]; those implicitly set \Seen on Gmail.
        let mut fetched = imap_timeout(
            timeout_secs,
            "uid_fetch start",
            session.uid_fetch(&uid_set_str, "(UID FLAGS BODY.PEEK[])"),
        )
        .await?
        .context("uid_fetch")?;
        while let Some(msg) = imap_timeout(timeout_secs, "uid_fetch next", fetched.next()).await? {
            let msg = msg.context("uid_fetch message")?;
            if let Some(parsed) = parse_fetch(msg, preview_chars) {
                // Include: new UIDs, full sync, or UNSEEN refresh of already-seen UIDs
                if parsed.0 > last_uid || full || parsed.2 {
                    out.push(parsed);
                }
            }
        }
    }

    let _ = imap_timeout(timeout_secs, "logout", session.logout()).await;
    Ok((out, high_water))
}

fn parse_fetch(msg: Fetch, preview_chars: usize) -> Option<(u32, ParsedMail, bool, bool)> {
    let uid = msg.uid?;
    let raw = msg.body()?.to_vec();
    let mut flags = msg.flags();
    let is_unread = !flags.any(|f| matches!(f, async_imap::types::Flag::Seen));
    let is_flagged = flags.any(|f| matches!(f, async_imap::types::Flag::Flagged));
    let parsed = parser::parse_raw(&raw, preview_chars);
    Some((uid, parsed, is_unread, is_flagged))
}

pub type ApplyStepCallback<'a> = dyn FnMut(usize, &MessageDecision, &ActionResult) + 'a;

pub async fn apply_decisions(
    account: &AccountConfig,
    password: &str,
    decisions: &[MessageDecision],
    allow_delete: bool,
    dry_run: bool,
    timeout_secs: u64,
    mut on_step: Option<&mut ApplyStepCallback<'_>>,
) -> Result<Vec<ActionResult>> {
    if dry_run {
        let results: Vec<ActionResult> = decisions
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let result = ActionResult {
                    account_id: d.account_id.clone(),
                    uid: d.uid,
                    action: d.action.as_str().into(),
                    ok: true,
                    detail: "dry-run".into(),
                };
                if let Some(ref mut cb) = on_step {
                    cb(i, d, &result);
                }
                result
            })
            .collect();
        return Ok(results);
    }

    let mut session = connect(account, password, timeout_secs).await?;
    imap_timeout(
        timeout_secs,
        "select inbox",
        session.select(&account.inbox_folder),
    )
    .await?
    .context("select inbox")?;
    let mailboxes = list_mailboxes(&mut session, timeout_secs).await?;

    let mut results = Vec::new();
    for (i, d) in decisions.iter().enumerate() {
        if d.account_id != account.id {
            continue;
        }
        let uid = d.uid.to_string();
        let result = match d.action {
            MailAction::Keep => ActionResult::ok(&d.account_id, d.uid, "keep"),
            MailAction::MarkRead => {
                match drain_uid_store(&mut session, &uid, "+FLAGS (\\Seen)", timeout_secs).await {
                    Ok(()) => ActionResult::ok(&d.account_id, d.uid, "mark_read"),
                    Err(e) => ActionResult::err(&d.account_id, d.uid, "mark_read", &e.to_string()),
                }
            }
            MailAction::Flag => {
                match drain_uid_store(&mut session, &uid, "+FLAGS (\\Flagged)", timeout_secs).await
                {
                    Ok(()) => ActionResult::ok(&d.account_id, d.uid, "flag"),
                    Err(e) => ActionResult::err(&d.account_id, d.uid, "flag", &e.to_string()),
                }
            }
            MailAction::Unflag => {
                match drain_uid_store(&mut session, &uid, "-FLAGS (\\Flagged)", timeout_secs).await
                {
                    Ok(()) => ActionResult::ok(&d.account_id, d.uid, "unflag"),
                    Err(e) => ActionResult::err(&d.account_id, d.uid, "unflag", &e.to_string()),
                }
            }
            MailAction::Archive => {
                let (dest, note) = resolve_mailbox(
                    &mailboxes,
                    MailAction::Archive,
                    d.target_folder.as_deref(),
                    &account.archive_folder,
                );
                match move_message(&mut session, &uid, &dest, timeout_secs).await {
                    Ok(()) => ActionResult::ok_detail(
                        &d.account_id,
                        d.uid,
                        "archive",
                        note.as_deref().unwrap_or("ok"),
                    ),
                    Err(e) => ActionResult::err(&d.account_id, d.uid, "archive", &e.to_string()),
                }
            }
            MailAction::Move => {
                let (dest, note) = resolve_mailbox(
                    &mailboxes,
                    MailAction::Move,
                    d.target_folder.as_deref(),
                    &account.archive_folder,
                );
                match move_message(&mut session, &uid, &dest, timeout_secs).await {
                    Ok(()) => ActionResult::ok_detail(
                        &d.account_id,
                        d.uid,
                        "move",
                        note.as_deref().unwrap_or("ok"),
                    ),
                    Err(e) => ActionResult::err(&d.account_id, d.uid, "move", &e.to_string()),
                }
            }
            MailAction::Tag => {
                if let Some(folder) = d.tags.first() {
                    let (dest, note) = resolve_mailbox(
                        &mailboxes,
                        MailAction::Move,
                        Some(folder),
                        &account.archive_folder,
                    );
                    match copy_to_folder(&mut session, &uid, &dest, timeout_secs).await {
                        Ok(()) => ActionResult::ok_detail(
                            &d.account_id,
                            d.uid,
                            "tag",
                            note.as_deref().unwrap_or("ok"),
                        ),
                        Err(e) => ActionResult::err(&d.account_id, d.uid, "tag", &e.to_string()),
                    }
                } else {
                    ActionResult::err(&d.account_id, d.uid, "tag", "no tag folder")
                }
            }
            MailAction::Delete => {
                if !allow_delete {
                    ActionResult::err(&d.account_id, d.uid, "delete", "delete not allowed")
                } else {
                    match async {
                        drain_uid_store(&mut session, &uid, "+FLAGS (\\Deleted)", timeout_secs)
                            .await?;
                        drain_expunge(&mut session, timeout_secs).await
                    }
                    .await
                    {
                        Ok(()) => ActionResult::ok(&d.account_id, d.uid, "delete"),
                        Err(e) => ActionResult::err(&d.account_id, d.uid, "delete", &e.to_string()),
                    }
                }
            }
        };
        if let Some(ref mut cb) = on_step {
            cb(i, d, &result);
        }
        results.push(result);
    }

    let _ = imap_timeout(timeout_secs, "logout", session.logout()).await;
    Ok(results)
}

/// Mark a single message \\Seen on the server (inbox survivors / leftover unread).
pub async fn mark_seen(
    account: &AccountConfig,
    password: &str,
    uid: u32,
    timeout_secs: u64,
) -> Result<()> {
    let mut session = connect(account, password, timeout_secs).await?;
    imap_timeout(
        timeout_secs,
        "select inbox",
        session.select(&account.inbox_folder),
    )
    .await?
    .context("select inbox")?;
    drain_uid_store(
        &mut session,
        &uid.to_string(),
        "+FLAGS (\\Seen)",
        timeout_secs,
    )
    .await?;
    let _ = imap_timeout(timeout_secs, "logout", session.logout()).await;
    Ok(())
}

async fn drain_uid_store(
    session: &mut ImapSession,
    uid: &str,
    query: &str,
    timeout_secs: u64,
) -> Result<()> {
    let stream = imap_timeout(timeout_secs, "uid_store", session.uid_store(uid, query))
        .await?
        .context("uid_store")?;
    futures::pin_mut!(stream);
    while imap_timeout(timeout_secs, "uid_store next", stream.next())
        .await?
        .transpose()?
        .is_some()
    {}
    Ok(())
}

async fn drain_expunge(session: &mut ImapSession, timeout_secs: u64) -> Result<()> {
    let stream = imap_timeout(timeout_secs, "expunge", session.expunge())
        .await?
        .context("expunge")?;
    futures::pin_mut!(stream);
    while imap_timeout(timeout_secs, "expunge next", stream.next())
        .await?
        .transpose()?
        .is_some()
    {}
    Ok(())
}

async fn list_mailboxes(session: &mut ImapSession, timeout_secs: u64) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    let mut stream = imap_timeout(timeout_secs, "list mailboxes", session.list(None, Some("*")))
        .await?
        .context("list mailboxes")?;
    while let Some(mb) = imap_timeout(timeout_secs, "list next", stream.next()).await? {
        let mb = mb.context("list mailbox")?;
        names.insert(mb.name().to_string());
    }
    Ok(names)
}

/// Pick a destination folder that exists on the server. Falls back to `archive_folder`.
pub fn resolve_mailbox(
    mailboxes: &HashSet<String>,
    action: MailAction,
    requested: Option<&str>,
    archive_folder: &str,
) -> (String, Option<String>) {
    let exists = |name: &str| mailboxes.contains(name);

    match action {
        MailAction::Archive | MailAction::Move => {
            if let Some(req) = requested {
                if exists(req) {
                    return (req.to_string(), None);
                }
                let note = format!("folder '{req}' not found; used {archive_folder}");
                return (archive_folder.to_string(), Some(note));
            }
            (archive_folder.to_string(), None)
        }
        _ => (archive_folder.to_string(), None),
    }
}

async fn move_message(
    session: &mut ImapSession,
    uid: &str,
    dest: &str,
    timeout_secs: u64,
) -> Result<()> {
    let caps = imap_timeout(timeout_secs, "capabilities", session.capabilities())
        .await?
        .context("capabilities")?;
    if caps.has_str("MOVE") {
        imap_timeout(timeout_secs, "uid_mv", session.uid_mv(uid, dest))
            .await?
            .context("uid_mv")?;
    } else {
        imap_timeout(timeout_secs, "uid_copy", session.uid_copy(uid, dest))
            .await?
            .context("uid_copy")?;
        drain_uid_store(session, uid, "+FLAGS (\\Deleted)", timeout_secs).await?;
        drain_expunge(session, timeout_secs).await?;
    }
    Ok(())
}

async fn copy_to_folder(
    session: &mut ImapSession,
    uid: &str,
    dest: &str,
    timeout_secs: u64,
) -> Result<()> {
    imap_timeout(timeout_secs, "uid_copy", session.uid_copy(uid, dest))
        .await?
        .context("uid_copy")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub account_id: String,
    pub uid: u32,
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

impl ActionResult {
    fn ok(account_id: &str, uid: u32, action: &str) -> Self {
        Self::ok_detail(account_id, uid, action, "ok")
    }

    fn ok_detail(account_id: &str, uid: u32, action: &str, detail: &str) -> Self {
        Self {
            account_id: account_id.into(),
            uid,
            action: action.into(),
            ok: true,
            detail: detail.into(),
        }
    }

    fn err(account_id: &str, uid: u32, action: &str, detail: &str) -> Self {
        Self {
            account_id: account_id.into(),
            uid,
            action: action.into(),
            ok: false,
            detail: detail.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_when_folder_missing() {
        let mailboxes: HashSet<String> = ["[Gmail]/All Mail".into()].into_iter().collect();
        let (dest, note) = resolve_mailbox(
            &mailboxes,
            MailAction::Archive,
            Some("walmart_newsletters"),
            "[Gmail]/All Mail",
        );
        assert_eq!(dest, "[Gmail]/All Mail");
        assert!(note.unwrap().contains("walmart_newsletters"));
    }

    #[test]
    fn uses_existing_folder() {
        let mailboxes: HashSet<String> = ["Promotions".into()].into_iter().collect();
        let (dest, note) = resolve_mailbox(
            &mailboxes,
            MailAction::Move,
            Some("Promotions"),
            "[Gmail]/All Mail",
        );
        assert_eq!(dest, "Promotions");
        assert!(note.is_none());
    }

    #[tokio::test]
    async fn imap_timeout_fires() {
        let err = imap_timeout(1, "sleep", async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            42
        })
        .await
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("timed out"), "{msg}");
        assert!(msg.contains("sleep"), "{msg}");
    }
}
