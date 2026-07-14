use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::providers::types::{Provider, UsageBucket};

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
            "CREATE TABLE IF NOT EXISTS usage_buckets (
                provider TEXT NOT NULL,
                bucket_start TEXT NOT NULL,
                granularity TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                cost_usd REAL NOT NULL,
                input_tokens INTEGER,
                output_tokens INTEGER,
                request_count INTEGER,
                fetched_at TEXT NOT NULL,
                PRIMARY KEY (provider, bucket_start, model)
            );
            CREATE INDEX IF NOT EXISTS idx_usage_bucket_start ON usage_buckets(bucket_start);",
        )?;
        Ok(())
    }

    pub fn upsert_buckets(&self, buckets: &[UsageBucket]) -> Result<usize> {
        let fetched_at = Utc::now().to_rfc3339();
        let mut count = 0usize;
        for b in buckets {
            self.conn.execute(
                "INSERT INTO usage_buckets
                (provider, bucket_start, granularity, model, cost_usd, input_tokens, output_tokens, request_count, fetched_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(provider, bucket_start, model) DO UPDATE SET
                    cost_usd = excluded.cost_usd,
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    request_count = excluded.request_count,
                    fetched_at = excluded.fetched_at",
                params![
                    b.provider.to_string(),
                    b.bucket_start.to_rfc3339(),
                    b.granularity,
                    b.model.clone().unwrap_or_default(),
                    b.cost_usd,
                    b.input_tokens,
                    b.output_tokens,
                    b.request_count,
                    fetched_at,
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn daily_rows(
        &self,
    ) -> Result<Vec<(Provider, DateTime<Utc>, Option<String>, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT provider, bucket_start, model, cost_usd FROM usage_buckets WHERE granularity = 'day'",
        )?;
        let rows = stmt.query_map([], |row| {
            let provider: String = row.get(0)?;
            let bucket_start: String = row.get(1)?;
            let model: String = row.get(2)?;
            let cost: f64 = row.get(3)?;
            Ok((provider, bucket_start, model, cost))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (provider_s, bucket_start_s, model, cost) = row?;
            let provider: Provider = provider_s.parse()?;
            let bucket_start = DateTime::parse_from_rfc3339(&bucket_start_s)
                .map(|d| d.with_timezone(&Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(&bucket_start_s, "%Y-%m-%d %H:%M:%S")
                        .map(|d| d.and_utc())
                })
                .with_context(|| format!("parse bucket_start {bucket_start_s}"))?;
            let model_opt = if model.is_empty() {
                None
            } else {
                Some(model)
            };
            out.push((provider, bucket_start, model_opt, cost));
        }
        Ok(out)
    }

    pub fn last_fetched_at(&self) -> Result<Option<DateTime<Utc>>> {
        let mut stmt = self.conn.prepare("SELECT MAX(fetched_at) FROM usage_buckets")?;
        let val: Option<String> = stmt.query_row([], |row| row.get(0))?;
        match val {
            Some(s) => Ok(Some(
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .context("parse fetched_at")?,
            )),
            None => Ok(None),
        }
    }
}
