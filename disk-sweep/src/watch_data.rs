use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::scan::{dir_size, format_bytes, scan_targets, ScanItem};
use crate::targets::{default_targets, expand_path};
use crate::volume::{self, VolumeStats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanKind {
    /// Fast statvfs only.
    Volume,
    /// Folder sizes + cleanup target scan.
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderUsage {
    pub label: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub size_human: String,
    pub exists: bool,
    pub pct_of_volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryUsage {
    pub id: String,
    pub name: String,
    pub total_bytes: u64,
    pub total_human: String,
    pub selected_bytes: u64,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSnapshot {
    pub volume: VolumeStats,
    pub folders: Vec<FolderUsage>,
    pub categories: Vec<CategoryUsage>,
    pub cleanup_items: Vec<ScanItem>,
    pub largest_items: Vec<ScanItem>,
    pub reclaimable_bytes: u64,
    pub reclaimable_human: String,
    pub selected_count: usize,
    pub scanned_at: String,
    pub deep_scan_done: bool,
    pub analyze_done: bool,
}

impl WatchSnapshot {
    pub fn with_volume(volume: VolumeStats) -> Self {
        Self {
            volume,
            folders: vec![],
            categories: vec![],
            cleanup_items: vec![],
            largest_items: vec![],
            reclaimable_bytes: 0,
            reclaimable_human: format_bytes(0),
            selected_count: 0,
            scanned_at: String::new(),
            deep_scan_done: false,
            analyze_done: false,
        }
    }

    pub fn recalc_selection(&mut self) {
        self.selected_count = self.cleanup_items.iter().filter(|i| i.selected).count();
        self.reclaimable_bytes = self
            .cleanup_items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.size_bytes)
            .sum();
        self.reclaimable_human = format_bytes(self.reclaimable_bytes);
    }
}

#[derive(Debug, Clone)]
pub enum ScanUpdate {
    Phase {
        phase: &'static str,
        detail: String,
        current: usize,
        total: usize,
    },
    Log(String),
    Snapshot(WatchSnapshot),
    AnalyzeComplete(Vec<crate::scan::ScanItem>),
    Cancelled,
    Failed(String),
}

pub fn default_watch_paths() -> Vec<(String, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let candidates = [
        ("Developer", home.join("Library/Developer")),
        ("Xcode", home.join("Library/Developer/Xcode")),
        ("Caches", home.join("Library/Caches")),
        ("Projects", home.join("projects")),
        ("Downloads", home.join("Downloads")),
    ];

    candidates
        .into_iter()
        .filter(|(_, p)| p.exists())
        .map(|(l, p)| (l.to_string(), p))
        .collect()
}

pub fn resolve_watch_paths(extra: &[PathBuf]) -> Vec<(String, PathBuf)> {
    if extra.is_empty() {
        return default_watch_paths();
    }

    extra
        .iter()
        .map(|p| {
            let expanded = expand_path(p);
            let label = expanded
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| expanded.display().to_string());
            (label, expanded)
        })
        .collect()
}

pub fn refresh_volume(snapshot: &mut WatchSnapshot, anchor: &Path) -> Result<()> {
    snapshot.volume = volume::stats_for_path(anchor)?;
    Ok(())
}

pub fn collect_snapshot(
    watch_paths: &[(String, PathBuf)],
    top_n: usize,
    kind: ScanKind,
) -> Result<WatchSnapshot> {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = std::sync::mpsc::sync_channel(256);
    collect_snapshot_with_progress(watch_paths, top_n, kind, &cancel, &tx)
}

pub fn collect_snapshot_with_progress(
    watch_paths: &[(String, PathBuf)],
    top_n: usize,
    kind: ScanKind,
    cancel: &AtomicBool,
    tx: &SyncSender<ScanUpdate>,
) -> Result<WatchSnapshot> {
    let send = |msg: ScanUpdate| {
        let _ = tx.send(msg);
    };

    if cancel.load(Ordering::Relaxed) {
        send(ScanUpdate::Cancelled);
        anyhow::bail!("scan cancelled");
    }

    let anchor = watch_paths
        .first()
        .map(|(_, p)| p.as_path())
        .unwrap_or(Path::new("/"));

    send(ScanUpdate::Phase {
        phase: "Volume",
        detail: format!("Reading disk capacity for {}", short_path(anchor)),
        current: 0,
        total: if kind == ScanKind::Full {
            watch_paths.len() + 2
        } else {
            1
        },
    });

    let volume = volume::stats_for_path(anchor)?;
    let mut snapshot = WatchSnapshot::with_volume(volume);
    send(ScanUpdate::Snapshot(snapshot.clone()));

    if kind == ScanKind::Volume {
        snapshot.scanned_at = Utc::now().to_rfc3339();
        send(ScanUpdate::Phase {
            phase: "Done",
            detail: "Volume updated".into(),
            current: 1,
            total: 1,
        });
        send(ScanUpdate::Snapshot(snapshot.clone()));
        return Ok(snapshot);
    }

    let folder_total = watch_paths.len();
    for (i, (label, path)) in watch_paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            send(ScanUpdate::Cancelled);
            return Ok(snapshot);
        }

        send(ScanUpdate::Phase {
            phase: "Folders",
            detail: format!("Measuring {} — {}", label, short_path(path)),
            current: i + 1,
            total: folder_total + 2,
        });

        let exists = path.exists();
        let size_bytes = if exists { dir_size(path)? } else { 0 };
        let pct_of_volume = if snapshot.volume.total_bytes > 0 {
            size_bytes as f64 / snapshot.volume.total_bytes as f64
        } else {
            0.0
        };

        let folder = FolderUsage {
            label: label.clone(),
            path: path.clone(),
            size_bytes,
            size_human: format_bytes(size_bytes),
            exists,
            pct_of_volume,
        };

        snapshot.folders.push(folder.clone());
        snapshot.folders.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        send(ScanUpdate::Snapshot(snapshot.clone()));
        send(ScanUpdate::Log(format!("  {} — {}", folder.label, folder.size_human)));
    }

    if cancel.load(Ordering::Relaxed) {
        send(ScanUpdate::Cancelled);
        return Ok(snapshot);
    }

    send(ScanUpdate::Phase {
        phase: "Cleanup",
        detail: "Scanning Xcode junk, caches, and logs…".into(),
        current: folder_total + 1,
        total: folder_total + 2,
    });

    let report = scan_targets(&default_targets())?;
    snapshot.categories = report
        .categories
        .iter()
        .map(|c| CategoryUsage {
            id: c.id.clone(),
            name: c.name.clone(),
            total_bytes: c.total_bytes,
            total_human: format_bytes(c.total_bytes),
            selected_bytes: c.selected_bytes,
            item_count: c.items.len(),
        })
        .collect();

    snapshot.cleanup_items = report
        .categories
        .iter()
        .flat_map(|c| c.items.iter().cloned())
        .filter(|i| i.exists)
        .collect();
    sort_cleanup_items(&mut snapshot.cleanup_items);
    snapshot.recalc_selection();

    let mut largest_items = snapshot.cleanup_items.clone();
    largest_items.truncate(top_n);
    snapshot.largest_items = largest_items;

    for cat in &snapshot.categories {
        send(ScanUpdate::Log(format!("  {} — {}", cat.name, cat.total_human)));
    }

    snapshot.deep_scan_done = true;
    snapshot.scanned_at = Utc::now().to_rfc3339();

    send(ScanUpdate::Phase {
        phase: "Done",
        detail: "Deep scan complete".into(),
        current: folder_total + 2,
        total: folder_total + 2,
    });
    send(ScanUpdate::Snapshot(snapshot.clone()));

    Ok(snapshot)
}

pub async fn collect_snapshot_async(
    watch_paths: &[(String, PathBuf)],
    top_n: usize,
    kind: ScanKind,
    cancel: Arc<AtomicBool>,
) -> Result<WatchSnapshot> {
    let paths = watch_paths.to_vec();
    tokio::task::spawn_blocking(move || {
        let (tx, _rx) = std::sync::mpsc::sync_channel(256);
        collect_snapshot_with_progress(&paths, top_n, kind, &cancel, &tx)
    })
    .await?
}

fn short_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}

fn sort_cleanup_items(items: &mut [ScanItem]) {
    let order: Vec<String> = default_targets()
        .into_iter()
        .map(|t| t.name)
        .chain([
            crate::analyze::PARENT_DOT.to_string(),
            crate::analyze::PARENT_LIBRARY.to_string(),
            crate::analyze::PARENT_PROJECT_BUILD.to_string(),
            crate::analyze::PARENT_PROJECTS.to_string(),
        ])
        .collect();
    items.sort_by(|a, b| {
        let ai = order
            .iter()
            .position(|n| n == &a.parent_label)
            .unwrap_or(usize::MAX);
        let bi = order
            .iter()
            .position(|n| n == &b.parent_label)
            .unwrap_or(usize::MAX);
        ai.cmp(&bi).then(b.size_bytes.cmp(&a.size_bytes))
    });
}

pub fn apply_analyze_results(snapshot: &mut WatchSnapshot, items: Vec<ScanItem>, top_n: usize) {
    crate::analyze::merge_analyze_items(&mut snapshot.cleanup_items, items);
    sort_cleanup_items(&mut snapshot.cleanup_items);
    snapshot.recalc_selection();
    let mut largest = snapshot.cleanup_items.clone();
    largest.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    largest.truncate(top_n);
    snapshot.largest_items = largest;
    snapshot.analyze_done = true;
}
