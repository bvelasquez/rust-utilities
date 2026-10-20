use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::UNIX_EPOCH;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use xxhash_rust::xxh64::Xxh64;

const PARTIAL_WINDOW: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct DupOptions {
    pub root: PathBuf,
    pub recursive: bool,
    pub min_bytes: u64,
    pub confirm_sha256: bool,
    pub cross_devices: bool,
    /// Directories, and everything inside them, left out of the scan.
    pub exclude: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DupProgress {
    pub phase: String,
    pub detail: String,
    pub current: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepPolicy {
    Shortest,
    Newest,
    Oldest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupFile {
    pub path: PathBuf,
    pub modified_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupGroup {
    pub size_bytes: u64,
    pub xxh64: String,
    pub sha256: Option<String>,
    pub files: Vec<DupFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DupStats {
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub size_candidates: u64,
    pub partial_hashed: u64,
    pub full_hashed: u64,
    pub sha256_hashed: u64,
    pub skipped_hardlinks: u64,
    pub skipped_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupReport {
    pub root: PathBuf,
    pub recursive: bool,
    pub groups: Vec<DupGroup>,
    pub stats: DupStats,
    pub excluded: Vec<PathBuf>,
}

impl DupReport {
    pub fn extra_files(&self) -> u64 {
        self.groups
            .iter()
            .map(|g| g.files.len().saturating_sub(1) as u64)
            .sum()
    }

    pub fn reclaimable_bytes(&self) -> u64 {
        self.groups
            .iter()
            .map(|g| g.size_bytes.saturating_mul(g.files.len().saturating_sub(1) as u64))
            .sum()
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    size: u64,
    modified_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAction {
    pub remove: PathBuf,
    pub keep: PathBuf,
    pub size_bytes: u64,
    pub xxh64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneAction {
    pub path: String,
    pub keep: String,
    pub size_bytes: u64,
    pub xxh64: String,
    pub deleted: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneReport {
    pub apply: bool,
    pub log_path: Option<String>,
    pub planned: usize,
    pub deleted: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
    pub actions: Vec<PruneAction>,
}

pub fn find_duplicates(
    opts: &DupOptions,
    cancel: &AtomicBool,
    progress: Option<&SyncSender<DupProgress>>,
) -> Result<DupReport> {
    let root = opts
        .root
        .canonicalize()
        .with_context(|| format!("cannot read {}", opts.root.display()))?;
    if !root.is_dir() {
        bail!("not a directory: {}", root.display());
    }

    let root_dev = device_id(&fs::metadata(&root)?);
    let exclude = normalize_excludes(&opts.exclude);
    let mut stats = DupStats::default();
    let mut seen_inodes: HashSet<(u64, u64)> = HashSet::new();
    let mut files: Vec<Candidate> = Vec::new();

    let walker = WalkDir::new(&root)
        .follow_links(false)
        .max_depth(if opts.recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.file_type().is_symlink() {
                return false;
            }
            if path_is_excluded(entry.path(), &exclude) {
                return false;
            }
            if opts.cross_devices {
                return true;
            }
            match entry.metadata() {
                Ok(meta) => device_id(&meta) == root_dev,
                Err(_) => false,
            }
        });

    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                stats.skipped_errors += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => {
                stats.skipped_errors += 1;
                continue;
            }
        };
        if let Some(key) = inode_key(&meta) {
            if !seen_inodes.insert(key) {
                stats.skipped_hardlinks += 1;
                continue;
            }
        }
        let size = meta.len();
        stats.files_seen += 1;
        stats.bytes_seen += size;
        if stats.files_seen % 200 == 0 {
            emit(
                progress,
                DupProgress {
                    phase: "walk".into(),
                    detail: entry.path().display().to_string(),
                    current: stats.files_seen,
                    total: 0,
                },
            );
        }
        if size < opts.min_bytes {
            continue;
        }
        files.push(Candidate {
            path: entry.into_path(),
            size,
            modified_secs: modified_secs(&meta),
        });
    }

    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for file in files {
        by_size.entry(file.size).or_default().push(file);
    }
    by_size.retain(|_, group| group.len() >= 2);
    stats.size_candidates = by_size.values().map(|g| g.len() as u64).sum();

    let mut partial_groups: HashMap<(u64, u64), Vec<Candidate>> = HashMap::new();
    let partial_total = stats.size_candidates;
    let mut partial_done = 0u64;
    for (size, group) in by_size {
        let mut survivors = Vec::new();
        for candidate in group {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            partial_done += 1;
            stats.partial_hashed += 1;
            emit(
                progress,
                DupProgress {
                    phase: "partial".into(),
                    detail: candidate.path.display().to_string(),
                    current: partial_done,
                    total: partial_total,
                },
            );
            match partial_xxh64(&candidate.path, candidate.size) {
                Ok(hash) => survivors.push((hash, candidate)),
                Err(_) => stats.skipped_errors += 1,
            }
        }
        let mut buckets: HashMap<u64, Vec<Candidate>> = HashMap::new();
        for (hash, candidate) in survivors {
            buckets.entry(hash).or_default().push(candidate);
        }
        for (hash, bucket) in buckets {
            if bucket.len() >= 2 {
                partial_groups.insert((size, hash), bucket);
            }
        }
    }

    let full_total: u64 = partial_groups.values().map(|g| g.len() as u64).sum();
    let mut full_done = 0u64;
    let mut full_groups: HashMap<(u64, u64), Vec<Candidate>> = HashMap::new();
    for ((size, _), group) in partial_groups {
        let mut buckets: HashMap<u64, Vec<Candidate>> = HashMap::new();
        for candidate in group {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            full_done += 1;
            stats.full_hashed += 1;
            emit(
                progress,
                DupProgress {
                    phase: "full".into(),
                    detail: candidate.path.display().to_string(),
                    current: full_done,
                    total: full_total,
                },
            );
            match full_xxh64(&candidate.path) {
                Ok(hash) => buckets.entry(hash).or_default().push(candidate),
                Err(_) => stats.skipped_errors += 1,
            }
        }
        for (hash, bucket) in buckets {
            if bucket.len() >= 2 {
                full_groups.insert((size, hash), bucket);
            }
        }
    }

    let mut groups = Vec::new();
    for ((size, xxh), mut bucket) in full_groups {
        if opts.confirm_sha256 {
            let mut by_sha: HashMap<String, Vec<Candidate>> = HashMap::new();
            for candidate in bucket {
                if cancel.load(Ordering::Relaxed) {
                    bail!("cancelled");
                }
                stats.sha256_hashed += 1;
                emit(
                    progress,
                    DupProgress {
                        phase: "sha256".into(),
                        detail: candidate.path.display().to_string(),
                        current: stats.sha256_hashed,
                        total: 0,
                    },
                );
                match sha256_file(&candidate.path) {
                    Ok(digest) => by_sha.entry(digest).or_default().push(candidate),
                    Err(_) => stats.skipped_errors += 1,
                }
            }
            for (digest, mut confirmed) in by_sha {
                if confirmed.len() < 2 {
                    continue;
                }
                confirmed.sort_by(|a, b| a.path.cmp(&b.path));
                groups.push(group_from(size, xxh, Some(digest), confirmed));
            }
        } else {
            bucket.sort_by(|a, b| a.path.cmp(&b.path));
            groups.push(group_from(size, xxh, None, bucket));
        }
    }

    groups.sort_by(|a, b| {
        let wa = a.size_bytes * a.files.len().saturating_sub(1) as u64;
        let wb = b.size_bytes * b.files.len().saturating_sub(1) as u64;
        wb.cmp(&wa).then_with(|| a.xxh64.cmp(&b.xxh64))
    });

    Ok(DupReport {
        root,
        recursive: opts.recursive,
        groups,
        stats,
        excluded: exclude,
    })
}

fn normalize_excludes(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if let Ok(canon) = path.canonicalize() {
            if canon.is_dir() {
                out.push(canon);
            }
        }
    }
    out
}

fn path_is_excluded(path: &Path, exclude: &[PathBuf]) -> bool {
    exclude.iter().any(|ex| path == ex || path.starts_with(ex))
}

fn group_from(size: u64, xxh: u64, sha256: Option<String>, files: Vec<Candidate>) -> DupGroup {
    DupGroup {
        size_bytes: size,
        xxh64: format!("{xxh:016x}"),
        sha256,
        files: files
            .into_iter()
            .map(|c| DupFile {
                path: c.path,
                modified_secs: c.modified_secs,
            })
            .collect(),
    }
}

pub fn keep_index(files: &[DupFile], policy: KeepPolicy) -> usize {
    if files.is_empty() {
        return 0;
    }
    match policy {
        KeepPolicy::Shortest => files
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.path
                    .as_os_str()
                    .len()
                    .cmp(&b.path.as_os_str().len())
                    .then_with(|| a.path.cmp(&b.path))
            })
            .map(|(i, _)| i)
            .unwrap_or(0),
        KeepPolicy::Newest => files
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.modified_secs
                    .cmp(&b.modified_secs)
                    .then_with(|| b.path.cmp(&a.path))
            })
            .map(|(i, _)| i)
            .unwrap_or(0),
        KeepPolicy::Oldest => files
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.modified_secs
                    .cmp(&b.modified_secs)
                    .then_with(|| a.path.cmp(&b.path))
            })
            .map(|(i, _)| i)
            .unwrap_or(0),
    }
}

pub fn plan_deletes(report: &DupReport, policy: KeepPolicy) -> Vec<DeleteAction> {
    let mut actions = Vec::new();
    for group in &report.groups {
        let keep_at = keep_index(&group.files, policy);
        let Some(keep) = group.files.get(keep_at) else {
            continue;
        };
        for (idx, file) in group.files.iter().enumerate() {
            if idx == keep_at {
                continue;
            }
            actions.push(DeleteAction {
                remove: file.path.clone(),
                keep: keep.path.clone(),
                size_bytes: group.size_bytes,
                xxh64: group.xxh64.clone(),
            });
        }
    }
    actions
}

pub fn apply_deletes(
    root: &Path,
    actions: &[DeleteAction],
    apply: bool,
    log_dir: Option<&Path>,
) -> Result<PruneReport> {
    let root = root.canonicalize().context("scan root missing")?;
    let mut results: Vec<PruneAction> = actions
        .iter()
        .map(|action| PruneAction {
            path: action.remove.display().to_string(),
            keep: action.keep.display().to_string(),
            size_bytes: action.size_bytes,
            xxh64: action.xxh64.clone(),
            deleted: false,
            error: None,
        })
        .collect();

    if !apply {
        return Ok(PruneReport {
            apply: false,
            log_path: None,
            planned: results.len(),
            deleted: 0,
            bytes_freed: 0,
            errors: vec![],
            actions: results,
        });
    }

    let dir = match log_dir {
        Some(dir) => dir.to_path_buf(),
        None => dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("disk-sweep"),
    };
    fs::create_dir_all(&dir).context("create duplicate-delete log directory")?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let log_path = dir.join(format!("dup-prune-{stamp}.csv"));
    write_prune_log(&log_path, &results)?;

    let mut deleted = 0usize;
    let mut bytes_freed = 0u64;
    let mut errors = Vec::new();
    let mut blocked_keeps: HashSet<PathBuf> = HashSet::new();

    for (idx, action) in actions.iter().enumerate() {
        if blocked_keeps.contains(&action.keep) {
            let message = format!("skipped because keep file failed: {}", action.keep.display());
            results[idx].error = Some(message.clone());
            errors.push(message);
            continue;
        }
        match delete_one(&root, action) {
            Ok(()) => {
                results[idx].deleted = true;
                deleted += 1;
                bytes_freed += action.size_bytes;
            }
            Err(err) => {
                let message = err.to_string();
                if !action.keep.exists() {
                    blocked_keeps.insert(action.keep.clone());
                }
                results[idx].error = Some(message.clone());
                errors.push(message);
            }
        }
    }

    write_prune_log(&log_path, &results)?;

    Ok(PruneReport {
        apply: true,
        log_path: Some(log_path.display().to_string()),
        planned: results.len(),
        deleted,
        bytes_freed,
        errors,
        actions: results,
    })
}

fn delete_one(root: &Path, action: &DeleteAction) -> Result<()> {
    if action.remove == action.keep {
        bail!("refusing to delete the kept copy");
    }
    let remove_meta = fs::symlink_metadata(&action.remove)
        .with_context(|| format!("missing {}", action.remove.display()))?;
    if remove_meta.file_type().is_symlink() {
        bail!("refusing to delete symlink {}", action.remove.display());
    }
    if !remove_meta.is_file() {
        bail!("refusing to delete non-file {}", action.remove.display());
    }
    let keep_meta = fs::symlink_metadata(&action.keep)
        .with_context(|| format!("kept file missing: {}", action.keep.display()))?;
    if keep_meta.file_type().is_symlink() || !keep_meta.is_file() {
        bail!("kept path is not a regular file: {}", action.keep.display());
    }

    let remove = action
        .remove
        .canonicalize()
        .with_context(|| format!("canonicalize {}", action.remove.display()))?;
    let keep = action
        .keep
        .canonicalize()
        .with_context(|| format!("canonicalize {}", action.keep.display()))?;
    if !remove.starts_with(root) || !keep.starts_with(root) {
        bail!("path is outside the scan root");
    }
    if remove_meta.len() != action.size_bytes || keep_meta.len() != action.size_bytes {
        bail!("size changed since the scan");
    }
    let expected = u64::from_str_radix(&action.xxh64, 16).context("bad xxh64")?;
    let remove_hash = full_xxh64(&remove)?;
    let keep_hash = full_xxh64(&keep)?;
    if remove_hash != expected || keep_hash != expected {
        bail!("hash changed since the scan");
    }
    fs::remove_file(&remove)?;
    Ok(())
}

fn write_prune_log(path: &Path, actions: &[PruneAction]) -> Result<()> {
    let mut file = File::create(path)?;
    writeln!(file, "status,path,keep,bytes,xxh64,error")?;
    for action in actions {
        let status = if action.deleted { "deleted" } else { "planned" };
        writeln!(
            file,
            "{status},{},{},{},{},{}",
            csv_cell(&action.path),
            csv_cell(&action.keep),
            action.size_bytes,
            action.xxh64,
            csv_cell(action.error.as_deref().unwrap_or(""))
        )?;
    }
    file.flush()?;
    Ok(())
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn emit(progress: Option<&SyncSender<DupProgress>>, event: DupProgress) {
    if let Some(tx) = progress {
        let _ = tx.try_send(event);
    }
}

fn partial_xxh64(path: &Path, size: u64) -> Result<u64> {
    let mut file = File::open(path)?;
    let mut hasher = Xxh64::new(0);
    let mut buf = vec![0u8; PARTIAL_WINDOW as usize];
    let head = size.min(PARTIAL_WINDOW) as usize;
    file.read_exact(&mut buf[..head])?;
    hasher.update(&buf[..head]);
    if size > PARTIAL_WINDOW {
        let tail_start = size - PARTIAL_WINDOW;
        file.seek(SeekFrom::Start(tail_start))?;
        let tail_len = PARTIAL_WINDOW.min(size - tail_start) as usize;
        file.read_exact(&mut buf[..tail_len])?;
        hasher.update(&buf[..tail_len]);
    }
    Ok(hasher.digest())
}

fn full_xxh64(path: &Path) -> Result<u64> {
    let mut file = File::open(path)?;
    let mut hasher = Xxh64::new(0);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.digest())
}

fn sha256_file(path: &Path) -> Result<String> {
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

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn modified_secs(meta: &fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

#[cfg(unix)]
fn device_id(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
fn device_id(_meta: &fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn inode_key(meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn inode_key(_meta: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn opts(root: &Path, recursive: bool) -> DupOptions {
        DupOptions {
            root: root.to_path_buf(),
            recursive,
            min_bytes: 1,
            confirm_sha256: false,
            cross_devices: false,
            exclude: Vec::new(),
        }
    }

    fn write_bytes(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(bytes).unwrap();
    }

    fn write_edges(path: &Path, head: u8, mid: u8, tail: u8) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(&vec![head; PARTIAL_WINDOW as usize]).unwrap();
        file.write_all(&vec![mid; 80 * 1024]).unwrap();
        file.write_all(&vec![tail; PARTIAL_WINDOW as usize]).unwrap();
    }

    #[test]
    fn identical_nested_files_match() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("a.txt"), b"same-bytes");
        write_bytes(&dir.path().join("sub/b.txt"), b"same-bytes");
        write_bytes(&dir.path().join("unique.bin"), b"nope");

        let report = find_duplicates(&opts(dir.path(), true), &AtomicBool::new(false), None).unwrap();
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].files.len(), 2);
        assert_eq!(report.extra_files(), 1);
        assert!(report.stats.partial_hashed >= 2);
        assert!(report.stats.full_hashed >= 2);
    }

    #[test]
    fn non_recursive_misses_nested_copy() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("a.txt"), b"same-bytes");
        write_bytes(&dir.path().join("sub/b.txt"), b"same-bytes");
        let report =
            find_duplicates(&opts(dir.path(), false), &AtomicBool::new(false), None).unwrap();
        assert!(report.groups.is_empty());
    }

    #[test]
    fn same_size_different_bytes_are_not_duplicates() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("a.txt"), b"aaaa");
        write_bytes(&dir.path().join("b.txt"), b"bbbb");
        let report = find_duplicates(&opts(dir.path(), true), &AtomicBool::new(false), None).unwrap();
        assert!(report.groups.is_empty());
        assert_eq!(report.stats.partial_hashed, 2);
        assert_eq!(report.stats.full_hashed, 0);
    }

    #[test]
    fn same_edges_different_middle_is_not_a_duplicate() {
        let dir = tempdir().unwrap();
        write_edges(&dir.path().join("a.bin"), 1, 2, 3);
        write_edges(&dir.path().join("b.bin"), 1, 9, 3);
        write_bytes(&dir.path().join("tiny.txt"), b"x");
        let report = find_duplicates(&opts(dir.path(), true), &AtomicBool::new(false), None).unwrap();
        assert!(report.groups.is_empty());
        assert_eq!(report.stats.partial_hashed, 2);
        assert_eq!(report.stats.full_hashed, 2);
    }

    #[test]
    fn hard_links_are_not_extra_copies() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.txt");
        write_bytes(&a, b"linked");
        fs::hard_link(&a, dir.path().join("b.txt")).unwrap();
        let report = find_duplicates(&opts(dir.path(), true), &AtomicBool::new(false), None).unwrap();
        assert!(report.groups.is_empty());
        assert_eq!(report.stats.skipped_hardlinks, 1);
    }

    #[test]
    fn symlink_is_not_scanned() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("a.txt"), b"same-bytes");
        write_bytes(&dir.path().join("b.txt"), b"same-bytes");
        std::os::unix::fs::symlink(dir.path().join("a.txt"), dir.path().join("link.txt")).unwrap();
        let report = find_duplicates(&opts(dir.path(), true), &AtomicBool::new(false), None).unwrap();
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].files.len(), 2);
    }

    #[test]
    fn excluded_folder_is_not_deduped() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("keep/a.txt"), b"same-bytes");
        write_bytes(&dir.path().join("skip/b.txt"), b"same-bytes");
        write_bytes(&dir.path().join("keep/c.txt"), b"pair");
        write_bytes(&dir.path().join("keep/d.txt"), b"pair");
        let mut options = opts(dir.path(), true);
        options.exclude = vec![dir.path().join("skip")];
        let report = find_duplicates(&options, &AtomicBool::new(false), None).unwrap();
        assert_eq!(report.groups.len(), 1);
        assert!(report.groups[0].files.iter().all(|f| !f.path.starts_with(dir.path().join("skip"))));
        assert_eq!(report.excluded.len(), 1);
    }

    #[test]
    fn apply_keeps_one_file_and_refuses_outside_root() {
        let dir = tempdir().unwrap();
        write_bytes(&dir.path().join("a.txt"), b"same-bytes");
        write_bytes(&dir.path().join("nested/a.txt"), b"same-bytes");
        let report = find_duplicates(&opts(dir.path(), true), &AtomicBool::new(false), None).unwrap();
        let actions = plan_deletes(&report, KeepPolicy::Shortest);
        assert_eq!(actions.len(), 1);
        let preview = apply_deletes(dir.path(), &actions, false, None).unwrap();
        assert_eq!(preview.deleted, 0);
        assert!(dir.path().join("nested/a.txt").exists());

        let log = tempdir().unwrap();
        let applied = apply_deletes(dir.path(), &actions, true, Some(log.path())).unwrap();
        assert_eq!(applied.deleted, 1);
        assert!(dir.path().join("a.txt").exists());
        assert!(!dir.path().join("nested/a.txt").exists());
        assert!(applied.log_path.is_some());

        let outside = tempdir().unwrap();
        let victim = outside.path().join("keep-me.txt");
        write_bytes(&victim, b"same-bytes");
        let bad = vec![DeleteAction {
            remove: victim.clone(),
            keep: dir.path().join("a.txt"),
            size_bytes: 10,
            xxh64: report.groups[0].xxh64.clone(),
        }];
        let refused = apply_deletes(dir.path(), &bad, true, Some(log.path())).unwrap();
        assert_eq!(refused.deleted, 0);
        assert!(victim.exists());
        assert!(!refused.errors.is_empty());
    }
}
