use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::SyncSender;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::scan::{ItemKind, ScanItem};

#[derive(Debug, Clone)]
pub enum CleanUpdate {
    Progress { current: usize, total: usize, path: String },
    Log(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanResult {
    pub path: String,
    pub bytes_freed: u64,
    pub deleted: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanReport {
    pub dry_run: bool,
    pub results: Vec<CleanResult>,
    pub bytes_freed: u64,
    pub deleted_count: usize,
    pub error_count: usize,
}

pub fn clean_items(items: &[ScanItem], dry_run: bool) -> Result<CleanReport> {
    // Buffer progress messages; non-TUI callers do not drain the channel.
    let (tx, _rx) = std::sync::mpsc::sync_channel(256);
    clean_items_with_progress(items, dry_run, &tx)
}

pub fn clean_items_with_progress(
    items: &[ScanItem],
    dry_run: bool,
    tx: &SyncSender<CleanUpdate>,
) -> Result<CleanReport> {
    let selected: Vec<_> = items.iter().filter(|i| i.selected && i.exists).collect();
    if selected.is_empty() {
        bail!("no selected items to clean");
    }

    let total = selected.len();
    let mut results = Vec::new();
    let mut bytes_freed = 0u64;
    let mut deleted_count = 0usize;
    let mut error_count = 0usize;

    for (i, item) in selected.iter().enumerate() {
        let path_str = item.path.display().to_string();
        let _ = tx.send(CleanUpdate::Progress {
            current: i + 1,
            total,
            path: path_str.clone(),
        });

        let size = item.size_bytes;
        if dry_run {
            results.push(CleanResult {
                path: path_str,
                bytes_freed: size,
                deleted: false,
                error: None,
            });
            bytes_freed += size;
            continue;
        }

        match item.kind {
            ItemKind::ProjectRemove => match prepare_and_remove_project(&item.path) {
                Ok(freed) => {
                    let _ = tx.send(CleanUpdate::Log(format!("archived + removed {path_str}")));
                    results.push(CleanResult {
                        path: path_str,
                        bytes_freed: freed,
                        deleted: true,
                        error: None,
                    });
                    bytes_freed += freed;
                    deleted_count += 1;
                }
                Err(e) => {
                    let _ = tx.send(CleanUpdate::Log(format!("error {path_str}: {e}")));
                    results.push(CleanResult {
                        path: path_str,
                        bytes_freed: 0,
                        deleted: false,
                        error: Some(e.to_string()),
                    });
                    error_count += 1;
                }
            },
            ItemKind::Remove => match remove_path(&item.path) {
                Ok(()) => {
                    let _ = tx.send(CleanUpdate::Log(format!("deleted {path_str}")));
                    results.push(CleanResult {
                        path: path_str,
                        bytes_freed: size,
                        deleted: true,
                        error: None,
                    });
                    bytes_freed += size;
                    deleted_count += 1;
                }
                Err(e) => {
                    let _ = tx.send(CleanUpdate::Log(format!("error {path_str}: {e}")));
                    results.push(CleanResult {
                        path: path_str,
                        bytes_freed: 0,
                        deleted: false,
                        error: Some(e.to_string()),
                    });
                    error_count += 1;
                }
            },
        }
    }

    Ok(CleanReport {
        dry_run,
        results,
        bytes_freed,
        deleted_count,
        error_count,
    })
}

fn remove_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.is_file() {
        fs::remove_file(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Commit dirty work, backup untracked files, push if ahead, then remove the project directory.
pub fn prepare_and_remove_project(path: &Path) -> Result<u64> {
    use chrono::Utc;

    if !path.is_dir() {
        bail!("not a directory: {}", path.display());
    }

    let size = crate::scan::dir_size(path)?;
    let is_git = path.join(".git").exists();

    if is_git {
        let porcelain = crate::analyze::run_git(path, &["status", "--porcelain"]).unwrap_or_default();
        let untracked: Vec<String> = porcelain
            .lines()
            .filter(|l| l.starts_with("??"))
            .filter_map(|l| l.get(3..).map(str::trim).map(str::to_string))
            .collect();

        if !untracked.is_empty() {
            let archive_root = crate::analyze::archive_dir();
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into());
            let stamp = Utc::now().format("%Y%m%d-%H%M%S");
            let dest = archive_root.join(format!("{name}-{stamp}"));
            fs::create_dir_all(&dest)?;
            for rel in &untracked {
                let src = path.join(rel);
                let target = dest.join(rel);
                if src.is_dir() {
                    copy_dir_recursive(&src, &target)?;
                } else if src.is_file() {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(&src, &target)?;
                }
            }
        }

        let has_tracked_changes = porcelain
            .lines()
            .any(|l| !l.is_empty() && !l.starts_with("??"));
        if has_tracked_changes {
            crate::analyze::run_git(path, &["add", "-A"])?;
            let msg = format!(
                "chore: disk-sweep archive before removal ({})",
                Utc::now().format("%Y-%m-%d")
            );
            let status = Command::new("git")
                .args(["commit", "-m", &msg])
                .current_dir(path)
                .status()
                .context("git commit failed")?;
            if !status.success() {
                bail!("git commit failed in {}", path.display());
            }
        }

        if let Ok(ahead) = crate::analyze::run_git(path, &["rev-list", "--count", "@{u}..HEAD"]) {
            if ahead.parse::<u32>().unwrap_or(0) > 0 {
                let _ = crate::analyze::run_git(path, &["push"]);
            }
        }
    }

    remove_path(path)?;
    Ok(size)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn test_item(path: std::path::PathBuf) -> ScanItem {
        ScanItem {
            id: "t".into(),
            target_id: "t".into(),
            parent_label: "Test".into(),
            parent_path: path.parent().unwrap_or(&path).to_path_buf(),
            name: "cache.bin".into(),
            path,
            size_bytes: 4,
            exists: true,
            selected: true,
            description: "test".into(),
            kind: ItemKind::Remove,
            risk: String::new(),
        }
    }

    #[test]
    fn dry_run_does_not_delete() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("cache.bin");
        let mut f = fs::File::create(&file).unwrap();
        f.write_all(b"data").unwrap();

        let report = clean_items(&[test_item(file.clone())], true).unwrap();
        assert!(file.exists());
        assert_eq!(report.bytes_freed, 4);
        assert_eq!(report.deleted_count, 0);
    }

    #[test]
    fn clean_removes_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("cache.bin");
        let mut f = fs::File::create(&file).unwrap();
        f.write_all(b"data").unwrap();

        let report = clean_items(&[test_item(file.clone())], false).unwrap();
        assert!(!file.exists());
        assert_eq!(report.deleted_count, 1);
    }
}
