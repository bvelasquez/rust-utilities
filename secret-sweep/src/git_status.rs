use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFileState {
    Untracked,
    Ignored,
    Modified,
    Staged,
    CommittedClean,
}

pub fn classify_file(repo: &Path, file: &Path) -> Result<GitFileState> {
    let rel = file
        .strip_prefix(repo)
        .with_context(|| format!("{} not under {}", file.display(), repo.display()))?;
    let rel_str = rel.to_string_lossy();

    if git_check_ignore(repo, &rel_str)? {
        return Ok(GitFileState::Ignored);
    }

    let tracked = git_tracked(repo, &rel_str)?;
    if !tracked {
        return Ok(GitFileState::Untracked);
    }

    let staged_dirty = !git_diff_quiet(repo, &["diff", "--cached", "--quiet", "HEAD", "--", &rel_str])?;
    let worktree_dirty = !git_diff_quiet(repo, &["diff", "--quiet", "HEAD", "--", &rel_str])?;

    if staged_dirty && !worktree_dirty {
        return Ok(GitFileState::Staged);
    }
    if staged_dirty || worktree_dirty {
        return Ok(GitFileState::Modified);
    }

    Ok(GitFileState::CommittedClean)
}

fn git_check_ignore(repo: &Path, rel: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["check-ignore", "-q", "--", rel])
        .current_dir(repo)
        .status()
        .with_context(|| format!("git check-ignore in {}", repo.display()))?;
    Ok(status.success())
}

fn git_tracked(repo: &Path, rel: &str) -> Result<bool> {
    let out = Command::new("git")
        .args(["ls-files", "--error-unmatch", "--", rel])
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("git ls-files in {}", repo.display()))?;

    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("submodule") {
        return Ok(false);
    }
    Ok(false)
}

fn git_diff_quiet(repo: &Path, args: &[&str]) -> Result<bool> {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("git diff in {}", repo.display()))?;
    Ok(status.success())
}

pub fn should_backup(state: GitFileState) -> bool {
    matches!(
        state,
        GitFileState::Untracked
            | GitFileState::Ignored
            | GitFileState::Modified
            | GitFileState::Staged
    )
}
