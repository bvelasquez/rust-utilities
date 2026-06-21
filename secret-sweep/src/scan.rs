use crate::config::{is_dot_path, is_noise_file, is_skipped_walk_path, ScanConfig};
use crate::git_status::{classify_file, should_backup, GitFileState};
use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct SecretEntry {
    pub project: String,
    pub repo_path: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub sha256: String,
    pub size: u64,
    pub git_status: GitFileState,
    pub is_dotfile: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommittedWarning {
    pub project: String,
    pub repo_path: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub size: u64,
    pub is_dotfile: bool,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectScan {
    pub name: String,
    pub repo_path: String,
    pub backup_entries: Vec<SecretEntry>,
    pub committed_warnings: Vec<CommittedWarning>,
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub root: String,
    pub mine_only: bool,
    pub scanned_repos: usize,
    pub included_repos: usize,
    pub skipped_repos: usize,
    pub backup_count: usize,
    pub committed_warning_count: usize,
    pub projects: Vec<ProjectScan>,
    pub skipped: Vec<crate::filter::SkippedRepo>,
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p)?);
    }
    Ok(builder.build()?)
}

fn file_matches(path: &Path, extra_globs: &GlobSet) -> bool {
    if is_noise_file(path) {
        return false;
    }
    if is_dot_path(path) {
        return true;
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    extra_globs.is_match(&name) || extra_globs.is_match(path.to_string_lossy().as_ref())
}

/// Committed files that look like secrets/credentials (not benign dotfiles like .gitignore).
fn is_likely_secret(rel: &str, path: &Path, secret_globs: &GlobSet) -> bool {
    let lower = rel.to_lowercase();
    let basename = path
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if secret_globs.is_match(&basename) || secret_globs.is_match(rel) {
        return true;
    }
    if basename.starts_with(".env") {
        return !is_safe_env_template(&basename);
    }
    if matches!(
        basename.as_str(),
        ".npmrc" | ".netrc" | ".pypirc" | ".secrets" | ".envrc"
    ) {
        return true;
    }
    if basename.starts_with("id_rsa") || basename.starts_with("id_ed25519") {
        return true;
    }
    if lower.contains("secret")
        || lower.contains("credential")
        || lower.contains("private_key")
        || lower.contains("service-account")
        || lower.contains("firebase-adminsdk")
    {
        return true;
    }
    false
}

fn is_safe_env_template(basename: &str) -> bool {
    let lower = basename.to_lowercase();
    lower.ends_with(".example")
        || lower.ends_with(".template")
        || lower.ends_with(".sample")
        || lower.ends_with(".dist")
        || lower.ends_with(".test")
        || lower == ".env.example"
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let meta = fs::metadata(path)?;
    let size = meta.len();
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

fn scan_repo(
    repo: &Path,
    project_name: &str,
    cfg: &ScanConfig,
    extra_globs: &GlobSet,
) -> Result<ProjectScan> {
    let mut backup_entries = Vec::new();
    let mut committed_warnings = Vec::new();

    for entry in WalkDir::new(repo)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_skipped_walk_path(e.path(), &cfg.skip_dirs))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.depth() > cfg.max_walk_depth {
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.into_path();
        if !file_matches(&path, extra_globs) {
            continue;
        }

        if let Ok(meta) = fs::metadata(&path) {
            if meta.len() > cfg.max_file_size {
                continue;
            }
        }

        let rel = path.strip_prefix(repo).unwrap();
        let rel_str = rel.to_string_lossy().to_string();
        let state = match classify_file(repo, &path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let dot = is_dot_path(&path);

        if should_backup(state) {
            let (sha256, size) = hash_file(&path)?;
            backup_entries.push(SecretEntry {
                project: project_name.to_string(),
                repo_path: repo.display().to_string(),
                relative_path: rel_str.clone(),
                absolute_path: path.display().to_string(),
                sha256,
                size,
                git_status: state,
                is_dotfile: dot,
            });
        } else if state == GitFileState::CommittedClean && is_likely_secret(&rel_str, &path, extra_globs)
        {
            let size = fs::metadata(&path)?.len();
            committed_warnings.push(CommittedWarning {
                project: project_name.to_string(),
                repo_path: repo.display().to_string(),
                relative_path: rel_str,
                absolute_path: path.display().to_string(),
                size,
                is_dotfile: dot,
                recommendation: "Remove from git history, rotate credentials, and add to .gitignore"
                    .into(),
            });
        }
    }

    backup_entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    committed_warnings.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(ProjectScan {
        name: project_name.to_string(),
        repo_path: repo.display().to_string(),
        backup_entries,
        committed_warnings,
    })
}

pub fn run_scan(
    cfg: &ScanConfig,
    included: &[(PathBuf, bool, Option<String>, Option<String>)],
    skipped: Vec<crate::filter::SkippedRepo>,
) -> Result<ScanResult> {
    let extra_globs = build_globset(&cfg.extra_patterns)?;

    let projects: Vec<ProjectScan> = included
        .par_iter()
        .map(|(path, _, _, _)| {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            scan_repo(path, &name, cfg, &extra_globs)
        })
        .collect::<Result<Vec<_>>>()?;

    let backup_count: usize = projects.iter().map(|p| p.backup_entries.len()).sum();
    let committed_warning_count: usize = projects.iter().map(|p| p.committed_warnings.len()).sum();

    Ok(ScanResult {
        root: cfg.root.display().to_string(),
        mine_only: cfg.filter.mine_only,
        scanned_repos: included.len() + skipped.len(),
        included_repos: included.len(),
        skipped_repos: skipped.len(),
        backup_count,
        committed_warning_count,
        projects,
        skipped,
    })
}
