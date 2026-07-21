use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::agent::schema::{ClassificationPlan, MailAction, MessageCategory};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMessage {
    pub id: i64,
    pub account_id: String,
    pub uid: u32,
    pub message_id: Option<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub subject: String,
    pub date: Option<String>,
    pub category: String,
    pub priority: u8,
    pub status: String,
    pub is_unread: bool,
    pub is_flagged: bool,
    pub body_preview: String,
    pub body_text: Option<String>,
    pub list_unsubscribe: Option<String>,
    pub raw_headers_json: Option<String>,
    pub planned_action: Option<String>,
    pub plan_confidence: Option<f32>,
    pub plan_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncState {
    pub account_id: String,
    pub last_uid: u32,
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryStat {
    pub category: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SenderStat {
    pub from_address: String,
    pub count: i64,
    pub dominant_category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DailyStat {
    pub day: String,
    pub count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum AnalyticsPeriod {
    #[default]
    Day,
    Week,
    Month,
}

impl AnalyticsPeriod {
    pub fn next(self) -> Self {
        match self {
            Self::Day => Self::Week,
            Self::Week => Self::Month,
            Self::Month => Self::Day,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Day => "last 14 days",
            Self::Week => "last 8 weeks",
            Self::Month => "last 6 months",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ActionCounts {
    pub delete: i64,
    pub archive: i64,
    pub flag: i64,
    pub keep: i64,
    pub other: i64,
}

impl ActionCounts {
    pub fn add(&mut self, action: &str, n: i64) {
        match action {
            "delete" => self.delete += n,
            "archive" => self.archive += n,
            "flag" => self.flag += n,
            "keep" => self.keep += n,
            _ => self.other += n,
        }
    }

    pub fn total(&self) -> i64 {
        self.delete + self.archive + self.flag + self.keep + self.other
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeBucket {
    pub label: String,
    pub counts: ActionCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppliedAnalytics {
    pub period: AnalyticsPeriod,
    pub buckets: Vec<TimeBucket>,
    pub totals: ActionCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingSenderGroup {
    pub from_address: String,
    pub account_id: String,
    pub message_count: i64,
    pub unread_count: i64,
    pub sample_subject: String,
    pub sample_message_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LearningHint {
    pub sender: String,
    pub action: String,
    pub category: Option<String>,
    pub priority: u8,
    pub weight: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredPlan {
    pub id: i64,
    pub created_at: String,
    pub summary: String,
    pub message_count: usize,
    pub applied_at: Option<String>,
    pub json_plan: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL,
                uid INTEGER NOT NULL,
                message_id TEXT,
                from_address TEXT NOT NULL,
                from_name TEXT,
                subject TEXT NOT NULL,
                date TEXT,
                category TEXT NOT NULL DEFAULT 'unknown',
                priority INTEGER NOT NULL DEFAULT 3,
                status TEXT NOT NULL DEFAULT 'pending',
                is_unread INTEGER NOT NULL DEFAULT 1,
                is_flagged INTEGER NOT NULL DEFAULT 0,
                body_preview TEXT NOT NULL DEFAULT '',
                body_text TEXT,
                list_unsubscribe TEXT,
                raw_headers_json TEXT,
                planned_action TEXT,
                plan_confidence REAL,
                plan_reason TEXT,
                UNIQUE(account_id, uid)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_account_status ON messages(account_id, status);
            CREATE INDEX IF NOT EXISTS idx_messages_priority_date ON messages(priority DESC, date DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_from ON messages(from_address);

            CREATE TABLE IF NOT EXISTS sync_state (
                account_id TEXT PRIMARY KEY,
                last_uid INTEGER NOT NULL DEFAULT 0,
                last_sync_at TEXT
            );

            CREATE TABLE IF NOT EXISTS senders (
                address TEXT PRIMARY KEY,
                message_count INTEGER NOT NULL DEFAULT 0,
                dominant_category TEXT,
                user_override TEXT
            );

            CREATE TABLE IF NOT EXISTS plans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                json_plan TEXT NOT NULL,
                summary TEXT NOT NULL,
                applied_at TEXT
            );

            CREATE TABLE IF NOT EXISTS learning (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender TEXT NOT NULL,
                action TEXT NOT NULL,
                category TEXT,
                priority INTEGER NOT NULL DEFAULT 3,
                source TEXT NOT NULL DEFAULT 'user',
                weight INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_learning_sender ON learning(sender);",
        )?;
        self.ensure_column("messages", "applied_at", "TEXT")?;
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_messages_applied_at ON messages(applied_at);
             UPDATE messages
                SET applied_at = COALESCE(date, datetime('now'))
              WHERE status = 'applied' AND applied_at IS NULL;",
        )?;
        Ok(())
    }

    fn ensure_column(&self, table: &str, column: &str, decl: &str) -> Result<()> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let exists = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .any(|name| name == column);
        if !exists {
            self.conn
                .execute(
                    &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
                    [],
                )
                .with_context(|| format!("add column {table}.{column}"))?;
        }
        Ok(())
    }

    pub fn upsert_message(
        &self,
        account_id: &str,
        uid: u32,
        parsed: &crate::mail::parser::ParsedMail,
        is_unread: bool,
        is_flagged: bool,
    ) -> Result<i64> {
        let date = parsed.date.map(|d| d.to_rfc3339());
        self.conn.execute(
            "INSERT INTO messages (
                account_id, uid, message_id, from_address, from_name, subject, date,
                is_unread, is_flagged, body_preview, body_text, list_unsubscribe, raw_headers_json
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
            ON CONFLICT(account_id, uid) DO UPDATE SET
                message_id = excluded.message_id,
                from_address = excluded.from_address,
                from_name = excluded.from_name,
                subject = excluded.subject,
                date = excluded.date,
                is_unread = excluded.is_unread,
                is_flagged = excluded.is_flagged,
                body_preview = excluded.body_preview,
                body_text = excluded.body_text,
                list_unsubscribe = excluded.list_unsubscribe,
                raw_headers_json = excluded.raw_headers_json,
                -- Re-open applied mail for triage when the user marks it unread in Gmail.
                status = CASE
                    WHEN excluded.is_unread = 1 AND messages.status = 'applied' THEN 'pending'
                    ELSE messages.status
                END,
                planned_action = CASE
                    WHEN excluded.is_unread = 1 AND messages.status = 'applied' THEN NULL
                    ELSE messages.planned_action
                END,
                plan_confidence = CASE
                    WHEN excluded.is_unread = 1 AND messages.status = 'applied' THEN NULL
                    ELSE messages.plan_confidence
                END,
                plan_reason = CASE
                    WHEN excluded.is_unread = 1 AND messages.status = 'applied' THEN NULL
                    ELSE messages.plan_reason
                END",
            params![
                account_id,
                uid,
                parsed.message_id,
                parsed.from_address,
                parsed.from_name,
                parsed.subject,
                date,
                is_unread as i32,
                is_flagged as i32,
                parsed.body_preview,
                parsed.body_text,
                parsed.list_unsubscribe,
                parsed.raw_headers_json,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        if id == 0 {
            let id: i64 = self.conn.query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND uid = ?2",
                params![account_id, uid],
                |row| row.get(0),
            )?;
            Ok(id)
        } else {
            Ok(id)
        }
    }

    pub fn get_sync_state(&self, account_id: &str) -> Result<SyncState> {
        let row = self
            .conn
            .query_row(
                "SELECT account_id, last_uid, last_sync_at FROM sync_state WHERE account_id = ?1",
                params![account_id],
                |row| {
                    Ok(SyncState {
                        account_id: row.get(0)?,
                        last_uid: row.get::<_, i64>(1)? as u32,
                        last_sync_at: row.get(2)?,
                    })
                },
            )
            .optional()?;

        Ok(row.unwrap_or(SyncState {
            account_id: account_id.into(),
            last_uid: 0,
            last_sync_at: None,
        }))
    }

    pub fn set_sync_state(&self, account_id: &str, last_uid: u32) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO sync_state (account_id, last_uid, last_sync_at) VALUES (?1,?2,?3)
             ON CONFLICT(account_id) DO UPDATE SET last_uid = excluded.last_uid, last_sync_at = excluded.last_sync_at",
            params![account_id, last_uid as i64, now],
        )?;
        Ok(())
    }

    pub fn list_messages(
        &self,
        account_id: Option<&str>,
        category: Option<&str>,
        priority: Option<u8>,
        unread_only: bool,
        limit: usize,
    ) -> Result<Vec<CachedMessage>> {
        let mut sql = String::from(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE 1=1",
        );
        let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(a) = account_id {
            sql.push_str(" AND account_id = ?");
            binds.push(Box::new(a.to_string()));
        }
        if let Some(c) = category {
            sql.push_str(" AND category = ?");
            binds.push(Box::new(c.to_string()));
        }
        if let Some(p) = priority {
            sql.push_str(" AND priority = ?");
            binds.push(Box::new(p));
        }
        if unread_only {
            sql.push_str(" AND is_unread = 1");
        }
        sql.push_str(" ORDER BY priority DESC, date DESC LIMIT ?");
        binds.push(Box::new(limit as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(params.as_slice(), row_to_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("list messages")
    }

    pub fn teachable_messages(&self) -> Result<Vec<CachedMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE status IN ('pending', 'planned')
             ORDER BY date DESC",
        )?;
        let rows = stmt.query_map([], row_to_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("teachable messages")
    }

    pub fn get_message(&self, id: i64) -> Result<Option<CachedMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE id = ?1",
        )?;
        stmt.query_row(params![id], row_to_message)
            .optional()
            .context("get message")
    }

    /// Unread keep/flag mail that survived the noise filter (still in the inbox).
    pub fn unread_kept_messages(&self, limit: usize) -> Result<Vec<CachedMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages
             WHERE is_unread = 1
               AND status IN ('applied', 'planned')
               AND planned_action IN ('keep', 'flag')
             ORDER BY COALESCE(date, '') DESC, uid DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("unread kept messages")
    }

    pub fn mark_message_read(&self, account_id: &str, uid: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET is_unread = 0 WHERE account_id = ?1 AND uid = ?2",
            params![account_id, uid],
        )?;
        Ok(())
    }

    pub fn pending_sender_groups(&self, limit: usize) -> Result<Vec<PendingSenderGroup>> {
        let mut stmt = self.conn.prepare(
            "SELECT g.from_address, g.account_id, g.cnt, g.unread, m.id, m.subject
             FROM (
               SELECT from_address, account_id, COUNT(*) AS cnt, SUM(is_unread) AS unread,
                      MAX(id) AS max_id
               FROM messages WHERE status = 'pending'
               GROUP BY from_address, account_id
             ) g
             JOIN messages m ON m.id = g.max_id
             ORDER BY g.cnt DESC, g.from_address
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(PendingSenderGroup {
                from_address: row.get(0)?,
                account_id: row.get(1)?,
                message_count: row.get(2)?,
                unread_count: row.get(3)?,
                sample_message_id: row.get(4)?,
                sample_subject: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("pending sender groups")
    }

    pub fn pending_messages(&self, account_id: Option<&str>, limit: usize) -> Result<Vec<CachedMessage>> {
        let mut sql = String::from(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE status = 'pending'",
        );
        if let Some(a) = account_id {
            sql.push_str(&format!(" AND account_id = '{a}'"));
        }
        sql.push_str(" ORDER BY date DESC LIMIT ?1");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit as i64], row_to_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("pending messages")
    }

    pub fn pending_count_for_sender(&self, from_address: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE status = 'pending' AND from_address = ?1",
                params![from_address],
                |r| r.get(0),
            )
            .context("pending count for sender")
    }

    pub fn pending_from_sender(&self, sender: &str) -> Result<Vec<CachedMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE status = 'pending' AND from_address = ?1",
        )?;
        let rows = stmt.query_map(params![sender], row_to_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("pending from sender")
    }

    pub fn apply_decisions(&self, decisions: &[crate::agent::schema::MessageDecision]) -> Result<()> {
        for d in decisions {
            self.conn.execute(
                "UPDATE messages SET category = ?1, priority = ?2, status = 'planned',
                 planned_action = ?3, plan_confidence = ?4, plan_reason = ?5
                 WHERE account_id = ?6 AND uid = ?7",
                params![
                    d.category.as_str(),
                    d.priority,
                    d.action.as_str(),
                    d.confidence,
                    d.reason,
                    d.account_id,
                    d.uid,
                ],
            )?;
        }
        Ok(())
    }

    /// Soft LLM hint: set category/priority without planning an IMAP action (stays in Triage).
    pub fn apply_category_hints(
        &self,
        decisions: &[crate::agent::schema::MessageDecision],
    ) -> Result<()> {
        for d in decisions {
            self.conn.execute(
                "UPDATE messages SET category = ?1, priority = ?2,
                 plan_reason = ?3
                 WHERE account_id = ?4 AND uid = ?5 AND status = 'pending'",
                params![
                    d.category.as_str(),
                    d.priority,
                    format!("llm hint ({:.0}%): {}", d.confidence * 100.0, d.reason),
                    d.account_id,
                    d.uid,
                ],
            )?;
        }
        Ok(())
    }

    pub fn save_plan(&self, plan: &ClassificationPlan) -> Result<i64> {
        let mut merged = plan.messages.clone();
        let mut seen: HashSet<(String, u32)> = merged
            .iter()
            .map(|m| (m.account_id.clone(), m.uid))
            .collect();

        if let Some(old) = self.latest_pending_plan()? {
            if let Ok(old_plan) = serde_json::from_str::<ClassificationPlan>(&old.json_plan) {
                for m in old_plan.messages {
                    let key = (m.account_id.clone(), m.uid);
                    if seen.insert(key) {
                        merged.push(m);
                    }
                }
            }
        }

        self.supersede_pending_plans()?;

        let summary = if merged.len() > plan.messages.len() {
            format!(
                "{} messages queued (+{} from earlier teaches)",
                merged.len(),
                merged.len() - plan.messages.len()
            )
        } else if plan.summary.is_empty() {
            format!("{} messages queued for apply", merged.len())
        } else {
            plan.summary.clone()
        };

        let keep: Vec<(String, u32)> = merged
            .iter()
            .map(|m| (m.account_id.clone(), m.uid))
            .collect();

        let merged_plan = ClassificationPlan {
            messages: merged,
            summary,
        };

        let now = Utc::now().to_rfc3339();
        let json = serde_json::to_string(&merged_plan)?;
        self.conn.execute(
            "INSERT INTO plans (created_at, json_plan, summary) VALUES (?1,?2,?3)",
            params![now, json, merged_plan.summary],
        )?;
        let id = self.conn.last_insert_rowid();
        self.reset_orphan_planned_messages(&keep)?;
        Ok(id)
    }

    /// Rewrite an open plan's JSON (used after partial apply).
    pub fn replace_plan_messages(&self, plan_id: i64, plan: &ClassificationPlan) -> Result<()> {
        let json = serde_json::to_string(plan)?;
        let n = self.conn.execute(
            "UPDATE plans SET json_plan = ?1, summary = ?2
             WHERE id = ?3 AND applied_at IS NULL",
            params![json, plan.summary, plan_id],
        )?;
        if n == 0 {
            anyhow::bail!("plan {plan_id} is missing or already applied");
        }
        Ok(())
    }

    /// After IMAP apply: close plan if every decision succeeded; otherwise keep
    /// the plan open with only the remaining (failed / unattempted) messages.
    pub fn finalize_apply(
        &self,
        plan_id: i64,
        plan: &ClassificationPlan,
        applied: &HashSet<(String, u32)>,
    ) -> Result<bool> {
        let remaining: Vec<_> = plan
            .messages
            .iter()
            .filter(|m| !applied.contains(&(m.account_id.clone(), m.uid)))
            .cloned()
            .collect();

        if remaining.is_empty() {
            self.mark_plan_applied(plan_id)?;
            self.reset_remaining_planned()?;
            Ok(true)
        } else {
            let remaining_plan = ClassificationPlan {
                summary: format!(
                    "{} message{} still pending apply",
                    remaining.len(),
                    if remaining.len() == 1 { "" } else { "s" }
                ),
                messages: remaining,
            };
            self.replace_plan_messages(plan_id, &remaining_plan)?;
            Ok(false)
        }
    }

    /// Messages in the latest pending plan (full plan size, not just review queue).
    pub fn pending_plan_message_count(&self) -> Result<usize> {
        Ok(self
            .latest_pending_plan()?
            .map(|p| p.message_count)
            .unwrap_or(0))
    }

    pub fn get_plan(&self, plan_id: i64) -> Result<Option<StoredPlan>> {
        self.conn
            .query_row(
                "SELECT id, created_at, json_plan, summary, applied_at FROM plans WHERE id = ?1",
                params![plan_id],
                |row| {
                    let json: String = row.get(2)?;
                    let count = serde_json::from_str::<crate::agent::schema::ClassificationPlan>(&json)
                        .map(|p| p.messages.len())
                        .unwrap_or(0);
                    Ok(StoredPlan {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        json_plan: json,
                        summary: row.get(3)?,
                        applied_at: row.get(4)?,
                        message_count: count,
                    })
                },
            )
            .optional()
            .context("get plan")
    }

    pub fn latest_pending_plan(&self) -> Result<Option<StoredPlan>> {
        self.conn
            .query_row(
                "SELECT id, created_at, json_plan, summary, applied_at FROM plans
                 WHERE applied_at IS NULL ORDER BY id DESC LIMIT 1",
                [],
                |row| {
                    let json: String = row.get(2)?;
                    let count = serde_json::from_str::<crate::agent::schema::ClassificationPlan>(&json)
                        .map(|p| p.messages.len())
                        .unwrap_or(0);
                    Ok(StoredPlan {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        json_plan: json,
                        summary: row.get(3)?,
                        applied_at: row.get(4)?,
                        message_count: count,
                    })
                },
            )
            .optional()
            .context("latest pending plan")
    }

    pub fn mark_plan_applied(&self, plan_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE plans SET applied_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), plan_id],
        )?;
        Ok(())
    }

    /// Close out older unapplied plans when a new plan is saved.
    pub fn supersede_pending_plans(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE plans SET applied_at = ?1 WHERE applied_at IS NULL",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Planned messages not in the active plan go back to pending for re-triage.
    pub fn reset_orphan_planned_messages(&self, keep: &[(String, u32)]) -> Result<usize> {
        let keep_set: HashSet<(String, u32)> = keep.iter().cloned().collect();
        let mut stmt = self
            .conn
            .prepare("SELECT account_id, uid FROM messages WHERE status = 'planned'")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut reset = 0usize;
        for (account_id, uid) in rows {
            if !keep_set.contains(&(account_id.clone(), uid)) {
                self.clear_message_plan(&account_id, uid)?;
                reset += 1;
            }
        }
        Ok(reset)
    }

    /// After apply, any leftover planned rows are stale and should be re-triaged.
    pub fn reset_remaining_planned(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE messages SET status = 'pending', planned_action = NULL,
             plan_confidence = NULL, plan_reason = NULL
             WHERE status = 'planned'",
            [],
        )?;
        Ok(n)
    }

    /// One-time heal for DBs with planned messages but no active plan.
    pub fn reset_remaining_planned_if_no_pending_plan(&self) -> Result<usize> {
        if self.latest_pending_plan()?.is_none() {
            self.reset_remaining_planned()
        } else {
            Ok(0)
        }
    }

    fn clear_message_plan(&self, account_id: &str, uid: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET status = 'pending', planned_action = NULL,
             plan_confidence = NULL, plan_reason = NULL
             WHERE account_id = ?1 AND uid = ?2",
            params![account_id, uid],
        )?;
        Ok(())
    }

    /// Drop one planned message from the open plan and return it to Triage (`pending`).
    pub fn reject_from_plan(&self, account_id: &str, uid: u32) -> Result<bool> {
        self.clear_message_plan(account_id, uid)?;
        let Some(stored) = self.latest_pending_plan()? else {
            return Ok(true);
        };
        let mut plan: crate::agent::schema::ClassificationPlan =
            serde_json::from_str(&stored.json_plan).context("parse plan for reject")?;
        let before = plan.messages.len();
        plan.messages
            .retain(|m| !(m.account_id == account_id && m.uid == uid));
        if plan.messages.len() == before {
            return Ok(true);
        }
        if plan.messages.is_empty() {
            self.mark_plan_applied(stored.id)?;
        } else {
            plan.summary = format!(
                "{} message{} still pending apply",
                plan.messages.len(),
                if plan.messages.len() == 1 { "" } else { "s" }
            );
            self.replace_plan_messages(stored.id, &plan)?;
        }
        Ok(true)
    }

    pub fn mark_messages_applied(&self, account_id: &str, uids: &[u32]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for uid in uids {
            self.conn.execute(
                "UPDATE messages SET status = 'applied', applied_at = COALESCE(applied_at, ?3)
                 WHERE account_id = ?1 AND uid = ?2",
                params![account_id, uid, now],
            )?;
        }
        Ok(())
    }

    /// Applied-mail volume over time + breakdown by planned action (archive/flag/keep/delete).
    pub fn applied_analytics(&self, period: AnalyticsPeriod) -> Result<AppliedAnalytics> {
        let (bucket_expr, lookback, max_buckets) = match period {
            AnalyticsPeriod::Day => ("date(applied_at)", "-14 days", 14usize),
            AnalyticsPeriod::Week => ("strftime('%Y-W%W', applied_at)", "-56 days", 8usize),
            AnalyticsPeriod::Month => ("strftime('%Y-%m', applied_at)", "-180 days", 6usize),
        };

        let sql = format!(
            "SELECT {bucket_expr} AS bucket,
                    COALESCE(planned_action, 'other') AS action,
                    COUNT(*) AS n
             FROM messages
             WHERE status = 'applied'
               AND applied_at IS NOT NULL
               AND applied_at >= datetime('now', '{lookback}')
             GROUP BY bucket, action
             ORDER BY bucket ASC"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        let mut by_bucket: std::collections::BTreeMap<String, ActionCounts> =
            std::collections::BTreeMap::new();
        let mut totals = ActionCounts::default();

        for row in rows {
            let (bucket, action, n) = row?;
            let entry = by_bucket.entry(bucket).or_default();
            entry.add(&action, n);
            totals.add(&action, n);
        }

        // Keep the most recent max_buckets labels (fill empties only for day period).
        let mut buckets: Vec<TimeBucket> = by_bucket
            .into_iter()
            .map(|(label, counts)| TimeBucket { label, counts })
            .collect();
        if buckets.len() > max_buckets {
            buckets = buckets.split_off(buckets.len() - max_buckets);
        }

        Ok(AppliedAnalytics {
            period,
            buckets,
            totals,
        })
    }

    pub fn category_stats(&self, account_id: Option<&str>, days: i64) -> Result<Vec<CategoryStat>> {
        let cutoff = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let (sql, param): (String, Option<String>) = if let Some(a) = account_id {
            (
                "SELECT category, COUNT(*) FROM messages WHERE date >= ?1 AND account_id = ?2 GROUP BY category ORDER BY COUNT(*) DESC".into(),
                Some(a.to_string()),
            )
        } else {
            (
                "SELECT category, COUNT(*) FROM messages WHERE date >= ?1 GROUP BY category ORDER BY COUNT(*) DESC".into(),
                None,
            )
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let mut out = Vec::new();
        if let Some(a) = param {
            let rows = stmt.query_map(params![cutoff, a], |row| {
                Ok(CategoryStat {
                    category: row.get(0)?,
                    count: row.get(1)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let rows = stmt.query_map(params![cutoff], |row| {
                Ok(CategoryStat {
                    category: row.get(0)?,
                    count: row.get(1)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn sender_stats(&self, account_id: Option<&str>, limit: usize) -> Result<Vec<SenderStat>> {
        let sql = if account_id.is_some() {
            "SELECT from_address, COUNT(*), (
                SELECT category FROM messages m2
                WHERE m2.from_address = m.from_address AND m2.account_id = ?1
                GROUP BY category ORDER BY COUNT(*) DESC LIMIT 1
             ) FROM messages m WHERE account_id = ?1 GROUP BY from_address ORDER BY COUNT(*) DESC LIMIT ?2"
        } else {
            "SELECT from_address, COUNT(*), (
                SELECT category FROM messages m2
                WHERE m2.from_address = m.from_address
                GROUP BY category ORDER BY COUNT(*) DESC LIMIT 1
             ) FROM messages m GROUP BY from_address ORDER BY COUNT(*) DESC LIMIT ?1"
        };

        let mut out = Vec::new();
        if let Some(a) = account_id {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![a, limit as i64], |row| {
                Ok(SenderStat {
                    from_address: row.get(0)?,
                    count: row.get(1)?,
                    dominant_category: row.get(2)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![limit as i64], |row| {
                Ok(SenderStat {
                    from_address: row.get(0)?,
                    count: row.get(1)?,
                    dominant_category: row.get(2)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn daily_stats(&self, account_id: Option<&str>, days: i64) -> Result<Vec<DailyStat>> {
        let cutoff = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let sql = if account_id.is_some() {
            "SELECT substr(date, 1, 10) as day, COUNT(*) FROM messages
             WHERE date >= ?1 AND account_id = ?2 GROUP BY day ORDER BY day"
        } else {
            "SELECT substr(date, 1, 10) as day, COUNT(*) FROM messages
             WHERE date >= ?1 GROUP BY day ORDER BY day"
        };

        let mut out = Vec::new();
        if let Some(a) = account_id {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![cutoff, a], |row| {
                Ok(DailyStat {
                    day: row.get(0)?,
                    count: row.get(1)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![cutoff], |row| {
                Ok(DailyStat {
                    day: row.get(0)?,
                    count: row.get(1)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn total_count(&self, account_id: Option<&str>) -> Result<i64> {
        let (sql, has_account) = if account_id.is_some() {
            ("SELECT COUNT(*) FROM messages WHERE account_id = ?1", true)
        } else {
            ("SELECT COUNT(*) FROM messages", false)
        };
        if has_account {
            self.conn
                .query_row(sql, params![account_id.unwrap()], |r| r.get(0))
                .context("total count")
        } else {
            self.conn
                .query_row(sql, [], |r| r.get(0))
                .context("total count")
        }
    }

    pub fn pending_count(&self, account_id: Option<&str>) -> Result<i64> {
        let (sql, has_account) = if account_id.is_some() {
            (
                "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND status = 'pending'",
                true,
            )
        } else {
            ("SELECT COUNT(*) FROM messages WHERE status = 'pending'", false)
        };
        if has_account {
            self.conn
                .query_row(sql, params![account_id.unwrap()], |r| r.get(0))
                .context("pending count")
        } else {
            self.conn
                .query_row(sql, [], |r| r.get(0))
                .context("pending count")
        }
    }

    pub fn review_queue(&self, confidence_threshold: f32) -> Result<Vec<CachedMessage>> {
        let Some(stored) = self.latest_pending_plan()? else {
            return Ok(vec![]);
        };
        let plan: ClassificationPlan =
            serde_json::from_str(&stored.json_plan).context("parse plan for review queue")?;
        let active: HashSet<(&str, u32)> = plan
            .messages
            .iter()
            .map(|m| (m.account_id.as_str(), m.uid))
            .collect();

        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE status = 'planned' AND (
                plan_confidence < ?1 OR planned_action = 'delete'
             ) ORDER BY plan_confidence ASC, priority DESC LIMIT 100",
        )?;
        let rows = stmt.query_map(params![confidence_threshold], row_to_message)?;
        let mut out = Vec::new();
        for row in rows {
            let msg = row?;
            if active.contains(&(msg.account_id.as_str(), msg.uid)) {
                out.push(msg);
            }
        }
        Ok(out)
    }

    pub fn add_learning(
        &self,
        sender: &str,
        action: &str,
        category: Option<&str>,
        priority: u8,
        source: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO learning (sender, action, category, priority, source, weight, created_at)
             VALUES (?1,?2,?3,?4,?5,1,?6)",
            params![sender, action, category, priority, source, Utc::now().to_rfc3339()],
        )?;
        self.conn.execute(
            "INSERT INTO senders (address, message_count, user_override)
             VALUES (?1, 0, ?2)
             ON CONFLICT(address) DO UPDATE SET user_override = excluded.user_override",
            params![sender, action],
        )?;
        Ok(())
    }

    pub fn learning_hints(&self) -> Result<Vec<LearningHint>> {
        let mut stmt = self.conn.prepare(
            "SELECT sender, action, category, priority, SUM(weight) as w
             FROM learning GROUP BY sender, action, category, priority
             ORDER BY w DESC LIMIT 50",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LearningHint {
                sender: row.get(0)?,
                action: row.get(1)?,
                category: row.get(2)?,
                priority: row.get::<_, i64>(3)? as u8,
                weight: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("learning hints")
    }

    pub fn bump_sender(&self, from_address: &str, category: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO senders (address, message_count, dominant_category)
             VALUES (?1, 1, ?2)
             ON CONFLICT(address) DO UPDATE SET
                message_count = message_count + 1,
                dominant_category = excluded.dominant_category",
            params![from_address, category],
        )?;
        Ok(())
    }

    /// Scan cached messages and return those matching `pattern` (up to `max_results`).
    pub fn messages_matching_pattern(
        &self,
        pattern: &str,
        scan_limit: usize,
        max_results: usize,
    ) -> Result<Vec<CachedMessage>> {
        use crate::rules::message_matches_pattern;

        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages ORDER BY date DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![scan_limit as i64], row_to_message)?;
        let mut out = Vec::new();
        for row in rows {
            let msg = row?;
            if message_matches_pattern(pattern, &msg) {
                out.push(msg);
                if out.len() >= max_results {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// All cached messages from a sender (for pattern suggestions).
    pub fn messages_for_sender(
        &self,
        from_address: &str,
        account_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CachedMessage>> {
        let sql = if account_id.is_some() {
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE from_address = ?1 AND account_id = ?2
             ORDER BY date DESC LIMIT ?3"
        } else {
            "SELECT id, account_id, uid, message_id, from_address, from_name, subject, date,
                    category, priority, status, is_unread, is_flagged, body_preview, body_text,
                    list_unsubscribe, raw_headers_json, planned_action, plan_confidence, plan_reason
             FROM messages WHERE from_address = ?1
             ORDER BY date DESC LIMIT ?2"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(aid) = account_id {
            stmt.query_map(params![from_address, aid, limit as i64], row_to_message)?
        } else {
            stmt.query_map(params![from_address, limit as i64], row_to_message)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("messages for sender")
    }

    #[cfg(test)]
    pub fn seed_pending_message(&self, account_id: &str, uid: u32, from: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (
                account_id, uid, from_address, subject, status, category, priority,
                is_unread, is_flagged, body_preview
             ) VALUES (?1, ?2, ?3, 'subj', 'pending', 'unknown', 3, 1, 0, '')",
            params![account_id, uid, from],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn seed_applied_message(
        &self,
        account_id: &str,
        uid: u32,
        from: &str,
        action: &str,
        unread: bool,
        body: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (
                account_id, uid, from_address, subject, status, category, priority,
                is_unread, is_flagged, body_preview, body_text, planned_action, applied_at
             ) VALUES (?1, ?2, ?3, 'subj', 'applied', 'personal', 3, ?4, 0, ?5, ?5, ?6, datetime('now'))",
            params![account_id, uid, from, unread as i32, body, action],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn message_status(&self, account_id: &str, uid: u32) -> Result<String> {
        self.conn
            .query_row(
                "SELECT status FROM messages WHERE account_id = ?1 AND uid = ?2",
                params![account_id, uid],
                |r| r.get(0),
            )
            .context("message status")
    }
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedMessage> {
    Ok(CachedMessage {
        id: row.get(0)?,
        account_id: row.get(1)?,
        uid: row.get::<_, i64>(2)? as u32,
        message_id: row.get(3)?,
        from_address: row.get(4)?,
        from_name: row.get(5)?,
        subject: row.get(6)?,
        date: row.get(7)?,
        category: row.get(8)?,
        priority: row.get::<_, i64>(9)? as u8,
        status: row.get(10)?,
        is_unread: row.get::<_, i64>(11)? != 0,
        is_flagged: row.get::<_, i64>(12)? != 0,
        body_preview: row.get(13)?,
        body_text: row.get(14)?,
        list_unsubscribe: row.get(15)?,
        raw_headers_json: row.get(16)?,
        planned_action: row.get(17)?,
        plan_confidence: row.get(18)?,
        plan_reason: row.get(19)?,
    })
}

pub fn category_from_str(s: &str) -> MessageCategory {
    MessageCategory::parse(s)
}

pub fn action_from_str(s: &str) -> MailAction {
    MailAction::parse(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::schema::{ClassificationPlan, MessageDecision, MessageCategory};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let store = Store::open(&path).unwrap();
        (dir, store)
    }

    fn decision(account: &str, uid: u32, action: &str, confidence: f32) -> MessageDecision {
        MessageDecision {
            account_id: account.into(),
            uid,
            message_id: None,
            action: MailAction::parse(action),
            category: MessageCategory::Newsletter,
            priority: 2,
            target_folder: None,
            tags: vec![],
            confidence,
            reason: "test".into(),
        }
    }

    #[test]
    fn unread_kept_includes_keep_and_flag_excludes_archive() {
        let (_dir, store) = test_store();
        store
            .seed_applied_message("personal", 1, "a@x.com", "keep", true, "hello keep")
            .unwrap();
        store
            .seed_applied_message("personal", 2, "b@x.com", "flag", true, "hello flag")
            .unwrap();
        store
            .seed_applied_message("personal", 3, "c@x.com", "archive", true, "gone")
            .unwrap();
        store
            .seed_applied_message("personal", 4, "d@x.com", "keep", false, "already read")
            .unwrap();

        let kept = store.unread_kept_messages(50).unwrap();
        assert_eq!(kept.len(), 2);
        let actions: Vec<_> = kept
            .iter()
            .map(|m| m.planned_action.as_deref().unwrap_or(""))
            .collect();
        assert!(actions.contains(&"keep"));
        assert!(actions.contains(&"flag"));
    }

    #[test]
    fn mark_message_read_removes_from_unread_kept() {
        let (_dir, store) = test_store();
        store
            .seed_applied_message("personal", 1, "a@x.com", "keep", true, "body")
            .unwrap();
        assert_eq!(store.unread_kept_messages(10).unwrap().len(), 1);
        store.mark_message_read("personal", 1).unwrap();
        assert!(store.unread_kept_messages(10).unwrap().is_empty());
        assert_eq!(store.message_status("personal", 1).unwrap(), "applied");
    }

    #[test]
    fn save_plan_merges_prior_planned_without_resetting() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 1, "a@x.com").unwrap();
        store.seed_pending_message("personal", 2, "b@x.com").unwrap();

        let plan1 = ClassificationPlan {
            messages: vec![decision("personal", 1, "archive", 0.5)],
            summary: "plan 1".into(),
        };
        store.apply_decisions(&plan1.messages).unwrap();
        store.save_plan(&plan1).unwrap();

        let plan2 = ClassificationPlan {
            messages: vec![decision("personal", 2, "delete", 0.4)],
            summary: "plan 2".into(),
        };
        store.apply_decisions(&plan2.messages).unwrap();
        store.save_plan(&plan2).unwrap();

        assert_eq!(store.message_status("personal", 1).unwrap(), "planned");

        let stored = store.latest_pending_plan().unwrap().unwrap();
        let merged: ClassificationPlan = serde_json::from_str(&stored.json_plan).unwrap();
        assert_eq!(merged.messages.len(), 2);

        let queue = store.review_queue(0.85).unwrap();
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn reset_remaining_planned_clears_review_without_plan() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 9, "c@x.com").unwrap();
        let plan = ClassificationPlan {
            messages: vec![decision("personal", 9, "delete", 0.3)],
            summary: "solo".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        let plan_id = store.save_plan(&plan).unwrap();
        store.mark_plan_applied(plan_id).unwrap();
        assert_eq!(store.reset_remaining_planned().unwrap(), 1);
        assert!(store.review_queue(0.85).unwrap().is_empty());
    }

    #[test]
    fn save_plan_resets_orphan_planned_not_in_merged_plan() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 1, "a@x.com").unwrap();
        store.seed_pending_message("personal", 2, "b@x.com").unwrap();

        let orphan = ClassificationPlan {
            messages: vec![decision("personal", 1, "archive", 0.5)],
            summary: "orphan".into(),
        };
        store.apply_decisions(&orphan.messages).unwrap();
        // Mark uid 1 planned without putting it in an active plan JSON merge path:
        // save a plan for uid 2 only after manually planning uid 1.
        store
            .conn
            .execute(
                "UPDATE messages SET status = 'planned', planned_action = 'archive',
                 plan_confidence = 0.5, plan_reason = 'orphan'
                 WHERE account_id = 'personal' AND uid = 1",
                [],
            )
            .unwrap();

        let plan = ClassificationPlan {
            messages: vec![decision("personal", 2, "delete", 0.4)],
            summary: "new".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        store.save_plan(&plan).unwrap();

        assert_eq!(store.message_status("personal", 1).unwrap(), "pending");
        assert_eq!(store.message_status("personal", 2).unwrap(), "planned");
        assert_eq!(store.pending_plan_message_count().unwrap(), 1);
    }

    #[test]
    fn teach_archive_confidence_skips_review_but_plan_exists() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 3, "news@x.com").unwrap();
        let plan = ClassificationPlan {
            messages: vec![decision("personal", 3, "archive", 1.0)],
            summary: "taught archive".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        store.save_plan(&plan).unwrap();

        assert!(store.review_queue(0.85).unwrap().is_empty());
        assert_eq!(store.pending_plan_message_count().unwrap(), 1);
        assert!(store.latest_pending_plan().unwrap().is_some());
    }

    #[test]
    fn finalize_apply_closes_when_all_applied() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 1, "a@x.com").unwrap();
        store.seed_pending_message("personal", 2, "b@x.com").unwrap();
        let plan = ClassificationPlan {
            messages: vec![
                decision("personal", 1, "archive", 1.0),
                decision("personal", 2, "archive", 1.0),
            ],
            summary: "both".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        let plan_id = store.save_plan(&plan).unwrap();
        store
            .mark_messages_applied("personal", &[1, 2])
            .unwrap();

        let mut applied = HashSet::new();
        applied.insert(("personal".into(), 1u32));
        applied.insert(("personal".into(), 2u32));
        assert!(store.finalize_apply(plan_id, &plan, &applied).unwrap());
        assert!(store.latest_pending_plan().unwrap().is_none());
        assert_eq!(store.message_status("personal", 1).unwrap(), "applied");
        assert_eq!(store.message_status("personal", 2).unwrap(), "applied");
    }

    #[test]
    fn finalize_apply_keeps_plan_open_on_partial_failure() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 1, "a@x.com").unwrap();
        store.seed_pending_message("personal", 2, "b@x.com").unwrap();
        let plan = ClassificationPlan {
            messages: vec![
                decision("personal", 1, "archive", 1.0),
                decision("personal", 2, "delete", 0.4),
            ],
            summary: "mixed".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        let plan_id = store.save_plan(&plan).unwrap();
        store.mark_messages_applied("personal", &[1]).unwrap();

        let mut applied = HashSet::new();
        applied.insert(("personal".into(), 1u32));
        assert!(!store.finalize_apply(plan_id, &plan, &applied).unwrap());

        let stored = store.latest_pending_plan().unwrap().unwrap();
        assert_eq!(stored.id, plan_id);
        let remaining: ClassificationPlan = serde_json::from_str(&stored.json_plan).unwrap();
        assert_eq!(remaining.messages.len(), 1);
        assert_eq!(remaining.messages[0].uid, 2);
        assert_eq!(store.message_status("personal", 1).unwrap(), "applied");
        assert_eq!(store.message_status("personal", 2).unwrap(), "planned");
        assert_eq!(store.review_queue(0.85).unwrap().len(), 1);
    }

    #[test]
    fn finalize_apply_mid_abort_leaves_unattempted_in_plan() {
        let (_dir, store) = test_store();
        store.seed_pending_message("a", 1, "a@x.com").unwrap();
        store.seed_pending_message("b", 2, "b@x.com").unwrap();
        let plan = ClassificationPlan {
            messages: vec![
                decision("a", 1, "archive", 1.0),
                decision("b", 2, "archive", 1.0),
            ],
            summary: "two accounts".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        let plan_id = store.save_plan(&plan).unwrap();
        // Simulate account a succeeded, account b never attempted (abort).
        store.mark_messages_applied("a", &[1]).unwrap();
        let mut applied = HashSet::new();
        applied.insert(("a".into(), 1u32));
        assert!(!store.finalize_apply(plan_id, &plan, &applied).unwrap());

        let remaining: ClassificationPlan =
            serde_json::from_str(&store.latest_pending_plan().unwrap().unwrap().json_plan)
                .unwrap();
        assert_eq!(remaining.messages.len(), 1);
        assert_eq!(remaining.messages[0].account_id, "b");
        assert_eq!(store.message_status("a", 1).unwrap(), "applied");
        assert_eq!(store.message_status("b", 2).unwrap(), "planned");
    }

    #[test]
    fn reject_from_plan_returns_message_to_pending() {
        let (_dir, store) = test_store();
        store.seed_pending_message("personal", 1, "a@x.com").unwrap();
        store.seed_pending_message("personal", 2, "b@x.com").unwrap();
        let plan = ClassificationPlan {
            messages: vec![
                decision("personal", 1, "delete", 0.4),
                decision("personal", 2, "archive", 0.5),
            ],
            summary: "two".into(),
        };
        store.apply_decisions(&plan.messages).unwrap();
        store.save_plan(&plan).unwrap();

        assert!(store.reject_from_plan("personal", 1).unwrap());
        assert_eq!(store.message_status("personal", 1).unwrap(), "pending");
        assert_eq!(store.message_status("personal", 2).unwrap(), "planned");
        assert_eq!(store.pending_plan_message_count().unwrap(), 1);

        assert!(store.reject_from_plan("personal", 2).unwrap());
        assert_eq!(store.message_status("personal", 2).unwrap(), "pending");
        assert!(store.latest_pending_plan().unwrap().is_none());
    }
}
