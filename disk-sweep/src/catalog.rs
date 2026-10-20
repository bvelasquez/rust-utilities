use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

pub struct IndexStatus {
    pub done: bool,
    pub dest: String,
    pub checked: u64,
    pub hashed: u64,
}

pub struct Catalog {
    conn: Connection,
    walk: RefCell<Option<WalkCursor>>,
}

struct WalkCursor {
    key: String,
    dest: String,
    iter: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    checked: u64,
    hashed: u64,
}

struct Hit {
    size: u64,
    path: PathBuf,
}

impl Catalog {
    pub fn open_default() -> Result<Self> {
        let path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("disk-sweep")
            .join("organize-catalog.sqlite");
        Self::open(&path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open catalog {}", path.display()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                sha256 TEXT PRIMARY KEY,
                size INTEGER NOT NULL,
                path TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS seen (
                path TEXT PRIMARY KEY,
                sha256 TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            conn,
            walk: RefCell::new(None),
        })
    }

    /// Hash files already stored under each destination once, so later drops of the same bytes are recognized.
    pub fn ensure_backfill(&self, rules: &crate::organize::CompiledRules) -> Result<()> {
        loop {
            let status = self.index_slice(rules, Duration::from_millis(400))?;
            if status.done {
                return Ok(());
            }
            eprintln!(
                "Indexing {}: checked {}, hashed {} so far",
                status.dest, status.checked, status.hashed
            );
        }
    }

    /// Index for a short slice so a caller can redraw and handle keys between batches.
    pub fn index_slice(
        &self,
        rules: &crate::organize::CompiledRules,
        budget: Duration,
    ) -> Result<IndexStatus> {
        let deadline = Instant::now() + budget;
        for source in &rules.sources {
            let key = format!("backfill:{}", source.dest_root.display());
            if self.meta(&key)?.is_some() || !source.dest_root.is_dir() {
                continue;
            }
            self.ensure_cursor(&key, &source.dest_root, &source.path)?;
            let finished = {
                let mut slot = self.walk.borrow_mut();
                let cursor = slot.as_mut().expect("cursor");
                pump(&self.conn, cursor, deadline)?
            };
            let status = self.snapshot(false);
            if finished {
                self.set_meta(&key, &chrono::Utc::now().to_rfc3339())?;
                *self.walk.borrow_mut() = None;
                if Instant::now() >= deadline {
                    return Ok(status);
                }
                continue;
            }
            return Ok(status);
        }
        Ok(IndexStatus {
            done: true,
            dest: String::new(),
            checked: 0,
            hashed: 0,
        })
    }

    fn snapshot(&self, done: bool) -> IndexStatus {
        let slot = self.walk.borrow();
        match slot.as_ref() {
            Some(cursor) => IndexStatus {
                done,
                dest: cursor.dest.clone(),
                checked: cursor.checked,
                hashed: cursor.hashed,
            },
            None => IndexStatus {
                done,
                dest: String::new(),
                checked: 0,
                hashed: 0,
            },
        }
    }

    fn ensure_cursor(&self, key: &str, root: &Path, drop_dir: &Path) -> Result<()> {
        let mut slot = self.walk.borrow_mut();
        if slot.as_ref().is_some_and(|cursor| cursor.key == key) {
            return Ok(());
        }
        let drop_dir = drop_dir.to_path_buf();
        let duplicates = root.join("duplicates");
        let iter = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(move |entry| {
                if entry.depth() == 0 {
                    return true;
                }
                if entry.file_type().is_symlink() {
                    return false;
                }
                let path = entry.path();
                !path.starts_with(&drop_dir) && !path.starts_with(&duplicates)
            });
        *slot = Some(WalkCursor {
            key: key.to_string(),
            dest: root.display().to_string(),
            iter: Box::new(iter),
            checked: 0,
            hashed: 0,
        });
        Ok(())
    }

    /// Path of an existing file with this content, when that file is still present and unchanged.
    pub fn kept_copy(&self, sha: &str) -> Result<Option<PathBuf>> {
        let Some(hit) = self.lookup(sha)? else {
            return Ok(None);
        };
        if !hit.path.is_file() {
            return Ok(None);
        }
        let meta = fs::metadata(&hit.path)?;
        if meta.len() != hit.size {
            return Ok(None);
        }
        if sha256_file(&hit.path)? != sha {
            return Ok(None);
        }
        Ok(Some(hit.path))
    }

    pub fn remember(&self, sha: &str, size: u64, path: &Path) -> Result<()> {
        let path_str = path.display().to_string();
        self.conn.execute(
            "INSERT INTO files (sha256, size, path) VALUES (?1, ?2, ?3)
             ON CONFLICT(sha256) DO UPDATE SET size = excluded.size, path = excluded.path",
            params![sha, size as i64, path_str],
        )?;
        let mtime = file_mtime(path).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO seen (path, sha256, size, mtime) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET sha256 = excluded.sha256, size = excluded.size, mtime = excluded.mtime",
            params![path.display().to_string(), sha, size as i64, mtime],
        )?;
        Ok(())
    }

    fn lookup(&self, sha: &str) -> Result<Option<Hit>> {
        let mut stmt = self
            .conn
            .prepare("SELECT size, path FROM files WHERE sha256 = ?1")?;
        let mut rows = stmt.query(params![sha])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(Hit {
            size: row.get::<_, i64>(0)? as u64,
            path: PathBuf::from(row.get::<_, String>(1)?),
        }))
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(row.get(0)?))
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn pump(conn: &Connection, cursor: &mut WalkCursor, deadline: Instant) -> Result<bool> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let finished = pump_tx(conn, cursor, deadline);
    if finished.is_ok() {
        conn.execute_batch("COMMIT")?;
    } else {
        let _ = conn.execute_batch("ROLLBACK");
    }
    finished
}

fn pump_tx(conn: &Connection, cursor: &mut WalkCursor, deadline: Instant) -> Result<bool> {
    let mut seen = conn.prepare("SELECT size, mtime FROM seen WHERE path = ?1")?;
    let mut insert_file = conn.prepare(
        "INSERT OR IGNORE INTO files (sha256, size, path) VALUES (?1, ?2, ?3)",
    )?;
    let mut insert_seen = conn.prepare(
        "INSERT INTO seen (path, sha256, size, mtime) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(path) DO UPDATE SET sha256 = excluded.sha256, size = excluded.size, mtime = excluded.mtime",
    )?;
    let mut steps = 0u64;
    loop {
        if steps > 0 && Instant::now() >= deadline {
            return Ok(false);
        }
        let Some(entry) = cursor.iter.next() else {
            return Ok(true);
        };
        steps += 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        cursor.checked += 1;
        let size = meta.len();
        let mtime = file_mtime(path).unwrap_or(0);
        let path_str = path.display().to_string();
        let unchanged = seen
            .query_row(params![path_str], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .ok()
            .is_some_and(|(seen_size, seen_mtime)| seen_size as u64 == size && seen_mtime == mtime);
        if unchanged {
            continue;
        }
        let sha = match sha256_file(path) {
            Ok(sha) => sha,
            Err(_) => continue,
        };
        insert_file.execute(params![sha, size as i64, path_str])?;
        insert_seen.execute(params![path_str, sha, size as i64, mtime])?;
        cursor.hashed += 1;
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn file_mtime(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
