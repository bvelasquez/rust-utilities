use crate::config::RepoFilter;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize)]
pub struct SkippedRepo {
    pub name: String,
    pub path: String,
    pub remote: Option<String>,
    pub remote_owner: Option<String>,
    pub reason: String,
}

fn normalize_name(s: &str) -> String {
    s.trim().to_lowercase()
}

fn parse_remote_owner(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            if host.contains("github") || host.contains("gitlab") || host.contains("bitbucket") {
                return path.split('/').next().map(|s| s.to_string());
            }
        }
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        if let Some(path_start) = url.find("://").and_then(|i| url[i + 3..].find('/')) {
            let path = &url[url.find("://").unwrap() + 3 + path_start + 1..];
            return path.split('/').next().map(|s| s.to_string());
        }
    }

    if let Some(idx) = url.find("://") {
        let after = &url[idx + 3..];
        if let Some(slash) = after.find('/') {
            let path = &after[slash + 1..];
            return path.split('/').next().map(|s| s.to_string());
        }
    }

    None
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
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
        bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            stderr.trim()
        )
    }
}

pub fn remote_origin(repo: &Path) -> Option<String> {
    run_git(repo, &["config", "--get", "remote.origin.url"]).ok()
}

pub fn classify_repo(
    name: &str,
    remote: Option<&str>,
    filter: &RepoFilter,
) -> (bool, Option<String>, String) {
    let name_key = normalize_name(name);

    if filter.exclude.contains(&name_key) {
        return (
            false,
            remote.and_then(|u| parse_remote_owner(u)),
            "excluded by name".into(),
        );
    }

    if filter.include.contains(&name_key) {
        return (
            true,
            remote.and_then(|u| parse_remote_owner(u)),
            "included by name".into(),
        );
    }

    if !filter.mine_only {
        return (
            true,
            remote.and_then(|u| parse_remote_owner(u)),
            "all repos mode".into(),
        );
    }

    let owner = remote.and_then(|u| parse_remote_owner(u));
    match (&owner, remote) {
        (Some(o), _) if filter.owners.contains(&normalize_name(o)) => {
            (true, owner, "owned remote".into())
        }
        (None, None) => (true, None, "no remote (local project)".into()),
        (Some(o), Some(_)) => (
            false,
            owner.clone(),
            format!("third-party remote ({o})"),
        ),
        (Some(_), None) => (false, owner, "remote url missing".into()),
        (None, Some(_)) => (false, None, "unrecognized remote".into()),
    }
}

pub fn partition_repos(
    repo_paths: Vec<std::path::PathBuf>,
    filter: &RepoFilter,
) -> (
    Vec<(
        std::path::PathBuf,
        bool,
        Option<String>,
        Option<String>,
    )>,
    Vec<SkippedRepo>,
) {
    let mut included = Vec::new();
    let mut skipped = Vec::new();

    for path in repo_paths {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let remote = remote_origin(&path);
        let (mine, owner, reason) = classify_repo(&name, remote.as_deref(), filter);

        if mine {
            included.push((path, mine, remote, owner));
        } else {
            skipped.push(SkippedRepo {
                name,
                path: path.display().to_string(),
                remote,
                remote_owner: owner,
                reason,
            });
        }
    }

    (included, skipped)
}
