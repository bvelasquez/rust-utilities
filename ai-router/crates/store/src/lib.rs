//! SQLite usage log. Prompt and response bodies are not stored.

use std::path::Path;
use std::sync::{Arc, Mutex};

use airouter_core::{RequestRecord, UsageRow};
use chrono::Datelike;
use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Copy)]
pub enum GroupBy {
    Model,
    Provider,
    Day,
    Client,
    Route,
}

impl GroupBy {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "model" => Some(Self::Model),
            "provider" => Some(Self::Provider),
            "day" => Some(Self::Day),
            "client" => Some(Self::Client),
            "route" => Some(Self::Route),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub requests: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS requests (
                id TEXT PRIMARY KEY,
                ts INTEGER NOT NULL,
                client_label TEXT,
                requested_model TEXT NOT NULL,
                provider TEXT,
                upstream_model TEXT,
                modality TEXT NOT NULL,
                status INTEGER NOT NULL,
                error TEXT,
                latency_ms INTEGER,
                ttft_ms INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cached_tokens INTEGER,
                cost_usd REAL,
                endpoint TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_requests_ts ON requests(ts);
             CREATE INDEX IF NOT EXISTS idx_requests_provider ON requests(provider);
             CREATE INDEX IF NOT EXISTS idx_requests_model ON requests(requested_model);",
        )?;
        migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn insert(&self, rec: &RequestRecord) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT OR REPLACE INTO requests (
                id, ts, client_label, requested_model, provider, upstream_model, modality,
                status, error, latency_ms, ttft_ms, input_tokens, output_tokens, cached_tokens,
                cost_usd, endpoint, route_profile, route_reason, route_confidence,
                route_signals, route_rejected, feedback
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
            params![
                rec.id,
                rec.ts,
                rec.client_label,
                rec.requested_model,
                rec.provider,
                rec.upstream_model,
                rec.modality,
                rec.status,
                rec.error,
                rec.latency_ms,
                rec.ttft_ms,
                rec.input_tokens,
                rec.output_tokens,
                rec.cached_tokens,
                rec.cost_usd,
                rec.endpoint,
                rec.route_profile,
                rec.route_reason,
                rec.route_confidence,
                rec.route_signals,
                rec.route_rejected,
                rec.feedback,
            ],
        )?;
        Ok(())
    }

    pub fn recent(&self, limit: u64) -> Result<Vec<RequestRecord>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, ts, client_label, requested_model, provider, upstream_model, modality,
                    status, error, latency_ms, ttft_ms, input_tokens, output_tokens, cached_tokens,
                    cost_usd, endpoint, route_profile, route_reason, route_confidence,
                    route_signals, route_rejected, feedback
             FROM requests ORDER BY ts DESC, rowid DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_record)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn summary_since(&self, since: i64) -> Result<Summary, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT COUNT(*),
                    SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0)
             FROM requests WHERE ts >= ?1",
        )?;
        let summary = stmt.query_row(params![since], |row| {
            Ok(Summary {
                requests: row.get::<_, i64>(0)? as u64,
                errors: row.get::<_, i64>(1).unwrap_or(0).max(0) as u64,
                input_tokens: row.get::<_, i64>(2).unwrap_or(0).max(0) as u64,
                output_tokens: row.get::<_, i64>(3).unwrap_or(0).max(0) as u64,
                cost_usd: row.get::<_, f64>(4).unwrap_or(0.0),
            })
        })?;
        Ok(summary)
    }

    pub fn count_since(&self, since: i64) -> Result<u64, StoreError> {
        Ok(self.summary_since(since)?.requests)
    }

    pub fn aggregate(&self, since: i64, by: GroupBy) -> Result<Vec<UsageRow>, StoreError> {
        let key_expr = match by {
            GroupBy::Model => "requested_model",
            GroupBy::Provider => "COALESCE(provider, '')",
            GroupBy::Client => "COALESCE(client_label, '')",
            GroupBy::Day => "date(ts, 'unixepoch')",
            GroupBy::Route => {
                "CASE WHEN route_profile IS NULL OR route_profile = '' THEN '(explicit)' ELSE route_profile END"
            }
        };
        let sql = format!(
            "SELECT {key_expr} AS k,
                    COUNT(*),
                    SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0)
             FROM requests WHERE ts >= ?1
             GROUP BY k
             ORDER BY 6 DESC"
        );
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![since], |row| {
            Ok(UsageRow {
                key: row.get::<_, String>(0)?,
                requests: row.get::<_, i64>(1)? as u64,
                errors: row.get::<_, i64>(2).unwrap_or(0).max(0) as u64,
                input_tokens: row.get::<_, i64>(3).unwrap_or(0).max(0) as u64,
                output_tokens: row.get::<_, i64>(4).unwrap_or(0).max(0) as u64,
                cost_usd: row.get::<_, f64>(5).unwrap_or(0.0),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestRecord> {
    Ok(RequestRecord {
        id: row.get(0)?,
        ts: row.get(1)?,
        client_label: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        requested_model: row.get(3)?,
        provider: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        upstream_model: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        modality: row.get(6)?,
        status: row.get::<_, i64>(7)? as u16,
        error: row.get(8)?,
        latency_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
        ttft_ms: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
        input_tokens: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
        output_tokens: row.get::<_, Option<i64>>(12)?.map(|v| v as u64),
        cached_tokens: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
        cost_usd: row.get::<_, Option<f64>>(14)?.unwrap_or(0.0),
        endpoint: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
        route_profile: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
        route_reason: row.get::<_, Option<String>>(17)?.unwrap_or_default(),
        route_confidence: row.get(18)?,
        route_signals: row.get::<_, Option<String>>(19)?.unwrap_or_default(),
        route_rejected: row.get::<_, Option<String>>(20)?.unwrap_or_default(),
        feedback: row.get::<_, Option<i64>>(21)?.map(|v| v as i8),
    })
}

fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let mut existing = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare("PRAGMA table_info(requests)")?;
        let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for name in names {
            existing.insert(name?);
        }
    }
    for (column, ty) in [
        ("route_profile", "TEXT"),
        ("route_reason", "TEXT"),
        ("route_confidence", "REAL"),
        ("route_signals", "TEXT"),
        ("route_rejected", "TEXT"),
        ("feedback", "INTEGER"),
    ] {
        if !existing.contains(column) {
            conn.execute(
                &format!("ALTER TABLE requests ADD COLUMN {column} {ty}"),
                [],
            )?;
        }
    }
    Ok(())
}

impl Store {
    pub fn set_feedback(&self, id: &str, value: i8) -> Result<bool, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.execute(
            "UPDATE requests SET feedback = ?1 WHERE id = ?2",
            params![value, id],
        )?;
        Ok(n > 0)
    }

    pub fn scan_since(&self, since: i64) -> Result<Vec<RequestRecord>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, ts, client_label, requested_model, provider, upstream_model, modality,
                    status, error, latency_ms, ttft_ms, input_tokens, output_tokens, cached_tokens,
                    cost_usd, endpoint, route_profile, route_reason, route_confidence,
                    route_signals, route_rejected, feedback
             FROM requests WHERE ts >= ?1 ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![since], row_to_record)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

pub fn start_of_local_day() -> i64 {
    let now = chrono::Local::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| now.timestamp() - 86_400)
}

pub fn start_of_local_month() -> i64 {
    let now = chrono::Local::now();
    now.date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| now.timestamp() - 31 * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use airouter_core::now_unix;

    #[test]
    fn insert_aggregate_and_recent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("usage.db")).unwrap();
        let rec = RequestRecord {
            id: "r1".into(),
            ts: now_unix(),
            client_label: "local".into(),
            requested_model: "openai/gpt-4o-mini".into(),
            provider: "openai".into(),
            upstream_model: "gpt-4o-mini".into(),
            modality: "chat".into(),
            status: 200,
            error: None,
            latency_ms: 12,
            ttft_ms: Some(4),
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cached_tokens: Some(0),
            cost_usd: 0.002,
            endpoint: "/v1/chat/completions".into(),
            route_profile: "cheap".into(),
            route_reason: "tier:chat".into(),
            route_confidence: Some(0.6),
            route_signals: "{}".into(),
            route_rejected: "[]".into(),
            feedback: None,
        };
        store.insert(&rec).unwrap();
        let mut failed = rec.clone();
        failed.id = "r2".into();
        failed.status = 500;
        failed.error = Some("boom".into());
        failed.cost_usd = 0.0;
        store.insert(&failed).unwrap();

        let summary = store.summary_since(0).unwrap();
        assert_eq!(summary.requests, 2);
        assert_eq!(summary.errors, 1);
        assert!((summary.cost_usd - 0.002).abs() < 1e-9);

        let by_model = store.aggregate(0, GroupBy::Model).unwrap();
        assert_eq!(by_model[0].key, "openai/gpt-4o-mini");
        assert_eq!(by_model[0].requests, 2);

        let recent = store.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].route_profile, "cheap");

        let by_route = store.aggregate(0, GroupBy::Route).unwrap();
        assert_eq!(by_route[0].key, "cheap");
        assert!(store.set_feedback("r1", 1).unwrap());
        let rated = store.recent(10).unwrap();
        let r1 = rated.iter().find(|r| r.id == "r1").unwrap();
        assert_eq!(r1.feedback, Some(1));
    }
}
