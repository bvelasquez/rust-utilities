use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::targets::{expand_path, CleanupTarget};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Delete the path (file or directory contents).
    #[default]
    Remove,
    /// Git project: commit, backup untracked files, push, then remove directory.
    ProjectRemove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanItem {
    pub id: String,
    pub target_id: String,
    /// Parent cleanup folder (e.g. "Archives", "Derived Data").
    pub parent_label: String,
    pub parent_path: PathBuf,
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub exists: bool,
    pub selected: bool,
    pub description: String,
    #[serde(default)]
    pub kind: ItemKind,
    /// `safe_cleanup`, `caution`, or `do_not_delete` (analyze-sourced items).
    #[serde(default)]
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCategory {
    pub id: String,
    pub name: String,
    pub description: String,
    pub items: Vec<ScanItem>,
    pub total_bytes: u64,
    pub selected_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub categories: Vec<ScanCategory>,
    pub total_bytes: u64,
    pub selected_bytes: u64,
    pub item_count: usize,
    pub selected_count: usize,
}

pub fn scan_targets(targets: &[CleanupTarget]) -> Result<ScanReport> {
    let categories_meta = crate::targets::default_categories();
    let mut categories = Vec::new();
    let mut total_bytes = 0u64;
    let mut selected_bytes = 0u64;
    let mut item_count = 0usize;
    let mut selected_count = 0usize;

    for cat in categories_meta {
        let cat_targets: Vec<_> = targets
            .iter()
            .filter(|t| t.category == cat.id)
            .cloned()
            .collect();

        let mut items = Vec::new();
        for target in cat_targets {
            let expanded = expand_path(&target.path);
            if target.expand_children {
                if expanded.is_dir() {
                    if let Ok(entries) = fs::read_dir(&expanded) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if !path.is_dir() && !path.is_file() {
                                continue;
                            }
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let size = dir_size(&path)?;
                            let id = format!("{}::{}", target.id, name);
                            let selected = target.selected_by_default;
                        items.push(ScanItem {
                            id,
                            target_id: target.id.clone(),
                            parent_label: target.name.clone(),
                            parent_path: expanded.clone(),
                            name,
                            path,
                            size_bytes: size,
                            exists: true,
                            selected,
                            description: target.description.clone(),
                            kind: ItemKind::Remove,
                            risk: String::new(),
                        });
                        }
                    }
                } else {
                    items.push(missing_item(&target, &expanded));
                }
            } else {
                let size = if expanded.exists() {
                    dir_size(&expanded)?
                } else {
                    0
                };
                items.push(ScanItem {
                    id: target.id.clone(),
                    target_id: target.id.clone(),
                    parent_label: target.name.clone(),
                    parent_path: expanded.clone(),
                    name: target.name.clone(),
                    path: expanded.clone(),
                    size_bytes: size,
                    exists: expanded.exists(),
                    selected: target.selected_by_default,
                    description: target.description.clone(),
                    kind: ItemKind::Remove,
                    risk: String::new(),
                });
            }
        }

        items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

        let cat_total: u64 = items.iter().map(|i| i.size_bytes).sum();
        let cat_selected: u64 = items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.size_bytes)
            .sum();
        let cat_sel_count = items.iter().filter(|i| i.selected).count();

        total_bytes += cat_total;
        selected_bytes += cat_selected;
        item_count += items.len();
        selected_count += cat_sel_count;

        categories.push(ScanCategory {
            id: cat.id,
            name: cat.name,
            description: cat.description,
            items,
            total_bytes: cat_total,
            selected_bytes: cat_selected,
        });
    }

    Ok(ScanReport {
        categories,
        total_bytes,
        selected_bytes,
        item_count,
        selected_count,
    })
}

fn missing_item(target: &CleanupTarget, path: &Path) -> ScanItem {
    ScanItem {
        id: target.id.clone(),
        target_id: target.id.clone(),
        parent_label: target.name.clone(),
        parent_path: path.to_path_buf(),
        name: target.name.clone(),
        path: path.to_path_buf(),
        size_bytes: 0,
        exists: false,
        selected: false,
        description: target.description.clone(),
        kind: ItemKind::Remove,
        risk: String::new(),
    }
}

pub fn dir_size(path: &Path) -> Result<u64> {
    if path.is_file() {
        return Ok(path.metadata()?.len());
    }
    if !path.is_dir() {
        return Ok(0);
    }

    let total = Arc::new(AtomicU64::new(0));
    let total_clone = Arc::clone(&total);

    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .for_each(|e| {
            if let Ok(meta) = e.metadata() {
                total_clone.fetch_add(meta.len(), Ordering::Relaxed);
            }
        });

    Ok(total.load(Ordering::Relaxed))
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let b = bytes as f64;
    if b >= TB {
        format!("{:.1} TB", b / TB)
    } else if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn dir_size_counts_files() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(b"hello").unwrap();
        assert_eq!(dir_size(dir.path()).unwrap(), 5);
    }

    #[test]
    fn format_bytes_scales() {
        assert_eq!(format_bytes(500), "500 B");
        assert!(format_bytes(1_500_000_000).contains("GB"));
    }
}
