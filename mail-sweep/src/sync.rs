use anyhow::Result;
use serde::Serialize;

use crate::commands::CommandContext;
use crate::config::AccountConfig;
use crate::mail::imap;
use crate::store::Store;

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub accounts: Vec<AccountSyncReport>,
    pub total_fetched: usize,
    pub total_stored: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountSyncReport {
    pub account_id: String,
    pub fetched: usize,
    pub stored: usize,
    pub last_uid: u32,
    pub error: Option<String>,
}

pub async fn sync_all(
    ctx: &CommandContext,
    account_filter: Option<&str>,
    full: bool,
) -> Result<SyncReport> {
    let store = Store::open(&ctx.app.db_path())?;
    let preview = ctx.app.config.sync.body_preview_chars;
    let mut accounts: Vec<&AccountConfig> = ctx.app.config.accounts.iter().collect();

    if let Some(id) = account_filter {
        accounts.retain(|a| a.id == id);
    }

    let mut reports = Vec::new();
    let mut total_fetched = 0usize;
    let mut total_stored = 0usize;

    for account in accounts {
        let password = match ctx.app.resolve_password(account) {
            Ok(p) => p,
            Err(e) => {
                reports.push(AccountSyncReport {
                    account_id: account.id.clone(),
                    fetched: 0,
                    stored: 0,
                    last_uid: 0,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };

        let state = store.get_sync_state(&account.id)?;
        let initial_limit = ctx.app.config.sync.initial_fetch_limit;
        let full_limit = ctx.app.config.sync.full_fetch_limit;
        match imap::fetch_new_messages(
            account,
            &password,
            state.last_uid,
            full,
            preview,
            initial_limit,
            full_limit,
        )
        .await
        {
            Ok((messages, max_uid)) => {
                let fetched = messages.len();
                let mut stored = 0usize;
                for (uid, parsed, is_unread, is_flagged) in messages {
                    store.upsert_message(&account.id, uid, &parsed, is_unread, is_flagged)?;
                    store.bump_sender(&parsed.from_address, "unknown")?;
                    stored += 1;
                }
                if max_uid > state.last_uid {
                    store.set_sync_state(&account.id, max_uid)?;
                }
                total_fetched += fetched;
                total_stored += stored;
                reports.push(AccountSyncReport {
                    account_id: account.id.clone(),
                    fetched,
                    stored,
                    last_uid: max_uid,
                    error: None,
                });
            }
            Err(e) => {
                reports.push(AccountSyncReport {
                    account_id: account.id.clone(),
                    fetched: 0,
                    stored: 0,
                    last_uid: state.last_uid,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(SyncReport {
        accounts: reports,
        total_fetched,
        total_stored,
    })
}
