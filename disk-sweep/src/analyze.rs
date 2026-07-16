use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::scan::{dir_size, format_bytes, ItemKind, ScanItem};
use crate::watch_data::ScanUpdate;

pub const PARENT_DOT: &str = "Dot folders";
pub const PARENT_LIBRARY: &str = "Library";
pub const PARENT_PROJECTS: &str = "Stale projects";
pub const PARENT_PROJECT_BUILD: &str = "Rust build artifacts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeReport {
    pub items: Vec<ScanItem>,
    pub dot_count: usize,
    pub library_count: usize,
    pub project_count: usize,
    pub project_build_count: usize,
    pub total_bytes: u64,
    pub total_human: String,
    pub projects_root: PathBuf,
    pub stale_days: u32,
}

#[derive(Debug, Clone)]
pub struct AnalyzeOptions {
    pub projects_root: PathBuf,
    pub stale_days: u32,
    pub min_bytes: u64,
    pub library_min_bytes: u64,
    /// Minimum size for Rust target/incremental dirs under projects.
    pub project_build_min_bytes: u64,
    pub skip_dot: bool,
    pub skip_library: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            projects_root: home.join("projects"),
            stale_days: 180,
            min_bytes: 100 * 1024 * 1024,
            library_min_bytes: 200 * 1024 * 1024,
            project_build_min_bytes: 50 * 1024 * 1024,
            skip_dot: false,
            skip_library: false,
        }
    }
}

pub fn default_projects_root() -> PathBuf {
    AnalyzeOptions::default().projects_root
}

pub fn run_analyze(options: &AnalyzeOptions) -> Result<AnalyzeReport> {
    let cancel = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(256);
    run_analyze_with_progress(options, &cancel, &tx)
}

pub fn run_analyze_with_progress(
    options: &AnalyzeOptions,
    cancel: &AtomicBool,
    tx: &SyncSender<ScanUpdate>,
) -> Result<AnalyzeReport> {
    let home = dirs::home_dir().context("home directory not found")?;
    let mut items = Vec::new();
    let phases = ["Dot folders", "Library", "Projects"];
    let total_phases = phases.len();

    for (i, phase) in phases.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(ScanUpdate::Cancelled);
            anyhow::bail!("analyze cancelled");
        }

        let _ = tx.send(ScanUpdate::Phase {
            phase: "Analyze",
            detail: format!("{phase}…"),
            current: i + 1,
            total: total_phases,
        });

        match *phase {
            "Dot folders" if !options.skip_dot => {
                let dot = scan_dot_folders(&home, options.min_bytes, cancel, tx)?;
                items.extend(dot);
            }
            "Library" if !options.skip_library => {
                let lib = scan_library(&home, options.library_min_bytes, cancel, tx)?;
                items.extend(lib);
            }
            "Projects" => {
                let builds = scan_rust_build_artifacts(
                    &options.projects_root,
                    options.project_build_min_bytes,
                    cancel,
                    tx,
                )?;
                items.extend(builds);
                let projects =
                    scan_stale_projects(&options.projects_root, options.stale_days, cancel, tx)?;
                items.extend(projects);
            }
            _ => {}
        }
    }

    items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    let total_bytes: u64 = items.iter().map(|i| i.size_bytes).sum();

    let _ = tx.send(ScanUpdate::Phase {
        phase: "Analyze",
        detail: format!(
            "Complete — {} items, {}",
            items.len(),
            format_bytes(total_bytes)
        ),
        current: total_phases,
        total: total_phases,
    });

    Ok(AnalyzeReport {
        dot_count: items.iter().filter(|i| i.parent_label == PARENT_DOT).count(),
        library_count: items.iter().filter(|i| i.parent_label == PARENT_LIBRARY).count(),
        project_count: items.iter().filter(|i| i.parent_label == PARENT_PROJECTS).count(),
        project_build_count: items
            .iter()
            .filter(|i| i.parent_label == PARENT_PROJECT_BUILD)
            .count(),
        total_bytes,
        total_human: format_bytes(total_bytes),
        projects_root: options.projects_root.clone(),
        stale_days: options.stale_days,
        items,
    })
}

fn scan_dot_folders(
    home: &Path,
    min_bytes: u64,
    cancel: &AtomicBool,
    tx: &SyncSender<ScanUpdate>,
) -> Result<Vec<ScanItem>> {
    let catalog = dot_folder_catalog();
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(home) else {
        return Ok(items);
    };

    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(items);
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') {
            continue;
        }

        let _ = tx.send(ScanUpdate::Log(format!("  measuring {name}")));
        let size = dir_size(&path)?;
        if size < min_bytes {
            continue;
        }

        let (risk, description) = if let Some((desc, risk)) = catalog.get(name.as_str()) {
            (*risk, *desc)
        } else {
            (
                "caution",
                "Unknown dot-folder — review before deleting",
            )
        };

        items.push(make_analyze_item(
            &format!("dot::{name}"),
            "analyze-dot",
            PARENT_DOT,
            home,
            name.clone(),
            path,
            size,
            description,
            ItemKind::Remove,
            risk,
        ));
    }

    Ok(items)
}

fn scan_library(
    home: &Path,
    min_bytes: u64,
    cancel: &AtomicBool,
    tx: &SyncSender<ScanUpdate>,
) -> Result<Vec<ScanItem>> {
    let library = home.join("Library");
    let mut items = Vec::new();

    let zones = [
        ("Application Support", library.join("Application Support")),
        ("Containers", library.join("Containers")),
        ("Caches", library.join("Caches")),
        ("Logs", library.join("Logs")),
    ];

    for (zone, root) in zones {
        if cancel.load(Ordering::Relaxed) {
            return Ok(items);
        }
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };

        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(items);
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }

            let _ = tx.send(ScanUpdate::Log(format!("  {zone}/{name}")));
            let size = dir_size(&path)?;
            if size < min_bytes {
                continue;
            }

            let (risk, description) = library_entry_hint(zone, &name);
            items.push(make_analyze_item(
                &format!("library::{zone}::{name}"),
                "analyze-library",
                PARENT_LIBRARY,
                &root,
                format!("{zone}/{name}"),
                path,
                size,
                description,
                ItemKind::Remove,
                risk,
            ));
        }
    }

    Ok(items)
}

fn scan_rust_build_artifacts(
    root: &Path,
    min_bytes: u64,
    cancel: &AtomicBool,
    tx: &SyncSender<ScanUpdate>,
) -> Result<Vec<ScanItem>> {
    let mut items = Vec::new();
    if !root.is_dir() {
        return Ok(items);
    }

    let Ok(entries) = fs::read_dir(root) else {
        return Ok(items);
    };

    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(items);
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let _ = tx.send(ScanUpdate::Log(format!("  rust target {name}")));
        items.extend(scan_project_rust_targets(&path, &name, root, min_bytes)?);
    }

    Ok(items)
}

fn scan_project_rust_targets(
    project: &Path,
    name: &str,
    projects_root: &Path,
    min_bytes: u64,
) -> Result<Vec<ScanItem>> {
    let target = project.join("target");
    if !target.is_dir() {
        return Ok(vec![]);
    }

    let is_rust =
        project.join("Cargo.toml").exists() || target.join(".rustc_info.json").exists();
    if !is_rust {
        return Ok(vec![]);
    }

    let mut items = Vec::new();
    for profile in ["debug", "release"] {
        let incremental = target.join(profile).join("incremental");
        if !incremental.is_dir() {
            continue;
        }
        let size = dir_size(&incremental)?;
        if size < min_bytes {
            continue;
        }
        let label = format!("{name}/target/{profile}/incremental");
        items.push(make_analyze_item(
            &format!("rust-incremental::{name}::{profile}"),
            "analyze-rust-incremental",
            PARENT_PROJECT_BUILD,
            projects_root,
            label,
            incremental,
            size,
            "Rust incremental build cache — cargo rebuilds on next compile",
            ItemKind::Remove,
            "safe_cleanup",
        ));
    }

    if items.is_empty() {
        let size = dir_size(&target)?;
        if size >= min_bytes {
            items.push(make_analyze_item(
                &format!("rust-target::{name}"),
                "analyze-rust-target",
                PARENT_PROJECT_BUILD,
                projects_root,
                format!("{name}/target"),
                target,
                size,
                "Rust cargo target/ — safe to delete; cargo clean regenerates",
                ItemKind::Remove,
                "safe_cleanup",
            ));
        }
    }

    Ok(items)
}

fn scan_stale_projects(
    root: &Path,
    stale_days: u32,
    cancel: &AtomicBool,
    tx: &SyncSender<ScanUpdate>,
) -> Result<Vec<ScanItem>> {
    let mut items = Vec::new();
    if !root.is_dir() {
        return Ok(items);
    }

    let stale = Duration::days(i64::from(stale_days));
    let cutoff = Utc::now() - stale;

    let Ok(entries) = fs::read_dir(root) else {
        return Ok(items);
    };

    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(items);
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let _ = tx.send(ScanUpdate::Log(format!("  project {name}")));

        let info = inspect_project(&path, &name)?;
        if info.last_activity > cutoff {
            continue;
        }

        let size = dir_size(&path)?;
        if size == 0 {
            continue;
        }

        let (risk, description, kind) = project_verdict(&info, stale_days);
        items.push(make_analyze_item(
            &format!("project::{}", info.name),
            "analyze-project",
            PARENT_PROJECTS,
            root,
            info.name.clone(),
            path,
            size,
            &description,
            kind,
            risk,
        ));
    }

    Ok(items)
}

#[derive(Debug)]
struct ProjectInfo {
    name: String,
    is_git: bool,
    last_activity: DateTime<Utc>,
    dirty: bool,
    untracked: u32,
    ahead: u32,
    #[allow(dead_code)]
    issues: Vec<String>,
}

fn inspect_project(path: &Path, name: &str) -> Result<ProjectInfo> {
    let is_git = path.join(".git").exists();
    let mut issues = Vec::new();
    let mut dirty = false;
    let mut untracked = 0u32;
    let mut ahead = 0u32;
    let mut last_activity = dir_mtime(path)?;

    if is_git {
        if let Ok(iso) = run_git(path, &["log", "-1", "--format=%cI"]) {
            if let Ok(dt) = DateTime::parse_from_rfc3339(&iso) {
                last_activity = dt.with_timezone(&Utc);
            }
        }

        let porcelain = run_git(path, &["status", "--porcelain"]).unwrap_or_default();
        for line in porcelain.lines() {
            if line.is_empty() {
                continue;
            }
            dirty = true;
            if line.starts_with("??") {
                untracked += 1;
            }
        }
        if dirty {
            issues.push("dirty".into());
        }

        if let Ok(count) = run_git(path, &["rev-list", "--count", "@{u}..HEAD"]) {
            ahead = count.parse().unwrap_or(0);
            if ahead > 0 {
                issues.push(format!("ahead:{ahead}"));
            }
        }
    }

    Ok(ProjectInfo {
        name: name.to_string(),
        is_git,
        last_activity,
        dirty,
        untracked,
        ahead,
        issues,
    })
}

fn project_verdict(info: &ProjectInfo, stale_days: u32) -> (&'static str, String, ItemKind) {
    let idle = (Utc::now() - info.last_activity).num_days();
    let last = info.last_activity.format("%Y-%m-%d").to_string();

    if !info.is_git {
        return (
            "caution",
            format!("No git repo · last activity {last} ({idle}d idle)"),
            ItemKind::Remove,
        );
    }

    if info.dirty || info.ahead > 0 {
        let mut parts = vec![format!("Last commit {last} ({idle}d idle, threshold {stale_days}d)")];
        if info.dirty {
            parts.push(format!(
                "uncommitted changes ({} untracked)",
                info.untracked
            ));
        }
        if info.ahead > 0 {
            parts.push(format!("{0} unpushed commit(s)", info.ahead));
        }
        parts.push("clean will commit, backup untracked, push, then remove".into());
        return ("caution", parts.join(" · "), ItemKind::ProjectRemove);
    }

    (
        "safe_cleanup",
        format!("Clean git repo · last commit {last} ({idle}d idle)"),
        ItemKind::ProjectRemove,
    )
}

fn make_analyze_item(
    id: &str,
    target_id: &str,
    parent_label: &str,
    parent_path: &Path,
    name: String,
    path: PathBuf,
    size_bytes: u64,
    description: &str,
    kind: ItemKind,
    risk: &str,
) -> ScanItem {
    ScanItem {
        id: id.to_string(),
        target_id: target_id.to_string(),
        parent_label: parent_label.to_string(),
        parent_path: parent_path.to_path_buf(),
        name,
        path,
        size_bytes,
        exists: true,
        selected: false,
        description: description.to_string(),
        kind,
        risk: risk.to_string(),
    }
}

fn dot_folder_catalog() -> std::collections::HashMap<&'static str, (&'static str, &'static str)> {
    [
        (
            ".ollama",
            (
                "LLM model weights — remove unused models with ollama rm",
                "caution",
            ),
        ),
        (
            ".android",
            ("Android SDK/emulator images — regenerate from Android Studio", "caution"),
        ),
        (
            ".gradle",
            ("Gradle dependency caches — safe to delete, re-downloads on build", "safe_cleanup"),
        ),
        (
            ".nvm",
            ("Node versions installed by nvm — keep versions you still use", "caution"),
        ),
        (
            ".npm",
            ("npm cache — safe to clear with npm cache clean --force", "safe_cleanup"),
        ),
        (
            ".cache",
            ("User-level tool caches — usually safe to delete", "safe_cleanup"),
        ),
        (
            ".cargo",
            ("Rust registry/git caches — cargo re-fetches as needed", "caution"),
        ),
        (
            ".pyenv",
            ("Python versions — keep interpreters you still need", "caution"),
        ),
        (
            ".cursor",
            ("Cursor IDE caches and indexes — may slow next launch", "caution"),
        ),
        (
            ".console-logs",
            ("Saved console log exports — safe if already reviewed", "safe_cleanup"),
        ),
        (
            ".local",
            ("pipx, uv, and other tool data — review contents first", "caution"),
        ),
        (
            ".gem",
            ("Ruby gem caches — safe to delete, bundler re-installs", "safe_cleanup"),
        ),
        (
            ".sdkman",
            ("SDKMAN-managed SDKs — keep versions you still use", "caution"),
        ),
        (
            ".vscode",
            ("VS Code extensions and caches", "caution"),
        ),
    ]
    .into_iter()
    .collect()
}

fn library_entry_hint(zone: &str, name: &str) -> (&'static str, &'static str) {
    let lower = name.to_lowercase();
    let safe_names = [
        "cache", "caches", "logs", "crashpad", "shipit", "updates", "temp",
    ];
    if zone == "Caches" || zone == "Logs" {
        return (
            "safe_cleanup",
            "Library cache/log data — apps recreate as needed",
        );
    }
    if safe_names.iter().any(|s| lower.contains(s)) {
        return (
            "safe_cleanup",
            "Likely cache or log data under Application Support",
        );
    }
    (
        "caution",
        "Application data — verify before deleting (may include settings or databases)",
    )
}

fn dir_mtime(path: &Path) -> Result<DateTime<Utc>> {
    let meta = fs::metadata(path)?;
    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let dt: DateTime<Utc> = modified.into();
    Ok(dt)
}

pub fn merge_analyze_items(existing: &mut Vec<ScanItem>, new_items: Vec<ScanItem>) {
    let paths: HashSet<_> = existing.iter().map(|i| i.path.clone()).collect();
    for item in new_items {
        if !paths.contains(&item.path) {
            existing.push(item);
        }
    }
    existing.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then(a.parent_label.cmp(&b.parent_label))
    });
}

pub fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to run git {} in {}", args.join(" "), repo.display()))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            stderr.trim()
        );
    }
}

pub fn archive_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("Documents").join("disk-sweep-archives"))
        .unwrap_or_else(|| PathBuf::from("disk-sweep-archives"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_verdict_clean_stale() {
        let info = ProjectInfo {
            name: "old-app".into(),
            is_git: true,
            last_activity: Utc::now() - Duration::days(200),
            dirty: false,
            untracked: 0,
            ahead: 0,
            issues: vec![],
        };
        let (risk, _, kind) = project_verdict(&info, 180);
        assert_eq!(risk, "safe_cleanup");
        assert_eq!(kind, ItemKind::ProjectRemove);
    }

    #[test]
    fn project_verdict_dirty_stale() {
        let info = ProjectInfo {
            name: "dirty-app".into(),
            is_git: true,
            last_activity: Utc::now() - Duration::days(200),
            dirty: true,
            untracked: 3,
            ahead: 2,
            issues: vec!["dirty".into()],
        };
        let (risk, desc, kind) = project_verdict(&info, 180);
        assert_eq!(risk, "caution");
        assert_eq!(kind, ItemKind::ProjectRemove);
        assert!(desc.contains("uncommitted"));
    }

    #[test]
    fn rust_incremental_detected() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("demo-app");
        let incremental = project
            .join("target")
            .join("debug")
            .join("incremental");
        fs::create_dir_all(&incremental).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        fs::write(incremental.join("cache.bin"), vec![0u8; 60 * 1024 * 1024]).unwrap();

        let items = scan_project_rust_targets(
            &project,
            "demo-app",
            root.path(),
            50 * 1024 * 1024,
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].parent_label, PARENT_PROJECT_BUILD);
        assert!(items[0].name.contains("incremental"));
        assert_eq!(items[0].risk, "safe_cleanup");
    }
}
