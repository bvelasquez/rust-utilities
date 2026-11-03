//! Persistent job metadata.
//!
//! Every job gets a sidecar JSON file next to its log:
//! `{data_dir}/runs/{job_id}.json` and `{data_dir}/runs/{job_id}.log`.
//! Sidecars are written atomically so a crash cannot leave a half-written
//! record behind. Log files remain plain text for compatibility with the TUI,
//! CLI, and anyone tailing them by hand.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::jobs::JobRecord;

/// Override with `SOKI_CI_DATA_DIR` (useful for tests and isolated instances).
pub fn data_dir() -> Result<PathBuf> {
    if let Ok(value) = std::env::var("SOKI_CI_DATA_DIR") {
        let value = value.trim();
        if !value.is_empty() {
            let path = PathBuf::from(value);
            std::fs::create_dir_all(&path)
                .with_context(|| format!("create data dir {}", path.display()))?;
            return Ok(path);
        }
    }
    let base =
        if let Some(proj) = directories::ProjectDirs::from("com.soki-ci", "soki-ci", "soki-ci") {
            proj.data_dir().to_path_buf()
        } else {
            let mut p = dirs::home_dir().context("home")?;
            p.push(".soki-ci");
            p
        };
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

pub fn runs_dir_in(data_dir: &Path) -> PathBuf {
    data_dir.join("runs")
}

pub fn log_path_in(data_dir: &Path, id: &Uuid) -> PathBuf {
    runs_dir_in(data_dir).join(format!("{id}.log"))
}

pub fn meta_path_in(data_dir: &Path, id: &Uuid) -> PathBuf {
    runs_dir_in(data_dir).join(format!("{id}.json"))
}

pub fn ensure_runs_dir(data_dir: &Path) -> Result<PathBuf> {
    let dir = runs_dir_in(data_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("create runs dir {}", dir.display()))?;
    Ok(dir)
}

pub fn write_job_in(data_dir: &Path, job: &JobRecord) -> Result<()> {
    let dir = ensure_runs_dir(data_dir)?;
    let path = dir.join(format!("{}.json", job.id));
    let tmp = dir.join(format!("{}.json.tmp", job.id));
    let bytes = serde_json::to_vec_pretty(job).context("serialize job metadata")?;
    std::fs::write(&tmp, bytes).with_context(|| format!("write job metadata {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("commit job metadata {}", path.display()))?;
    Ok(())
}

/// Load persisted jobs, newest first. Jobs that were queued/running when the
/// previous server exited are marked `Interrupted` and written back.
pub fn load_jobs_in(data_dir: &Path, limit: usize) -> Result<Vec<JobRecord>> {
    let dir = runs_dir_in(data_dir);
    let mut jobs = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(jobs);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let mut job: JobRecord = match serde_json::from_str(&raw) {
            Ok(job) => job,
            Err(_) => continue,
        };

        // Older records may not carry a log path; keep the sidecar useful.
        if job.log_path.is_empty() {
            job.log_path = log_path_in(data_dir, &job.id).to_string_lossy().into();
        }
        if !Path::new(&job.log_path).is_file() {
            let candidate = log_path_in(data_dir, &job.id);
            if candidate.is_file() {
                job.log_path = candidate.to_string_lossy().into();
            }
        }

        job.can_cancel = false;
        job.can_input = false;
        jobs.push(job);
    }

    jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at));
    jobs.truncate(limit);
    Ok(jobs)
}

pub fn delete_job_in(data_dir: &Path, id: &Uuid, log_path: Option<&str>) -> Result<()> {
    let meta = meta_path_in(data_dir, id);
    let _ = std::fs::remove_file(&meta);
    let log = log_path
        .map(PathBuf::from)
        .unwrap_or_else(|| log_path_in(data_dir, id));
    let _ = std::fs::remove_file(log);
    Ok(())
}
