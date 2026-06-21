use anyhow::{bail, Context, Result};
use clap::Parser;
use colored::Colorize;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Parser, Debug)]
#[command(
    name = "git-sweep",
    about = "Scan projects for git repos with uncommitted or unpushed work",
    version
)]
struct Cli {
    /// Root folder containing project directories (default: ~/projects)
    #[arg(long, env = "GIT_SWEEP_ROOT")]
    root: Option<PathBuf>,

    /// How deep to search for .git directories (1 = immediate children only)
    #[arg(long, default_value_t = 1)]
    depth: usize,

    /// Only repos you own (remote owner matches your GitHub orgs/users, or no remote)
    #[arg(long, default_value_t = true, env = "GIT_SWEEP_MINE")]
    mine: bool,

    /// Include all git repos, including third-party / open-source clones
    #[arg(long, conflicts_with = "mine")]
    all: bool,

    /// GitHub/GitLab owner to treat as yours (repeatable). Default: bvelasquez, mighty45
    #[arg(long = "owner", env = "GIT_SWEEP_OWNERS", value_delimiter = ',')]
    owners: Vec<String>,

    /// Always include these project folder names
    #[arg(long = "include", value_delimiter = ',')]
    include: Vec<String>,

    /// Always skip these project folder names
    #[arg(long = "exclude", value_delimiter = ',')]
    exclude: Vec<String>,

    /// Config file (default: ~/.config/git-sweep/config.toml)
    #[arg(long, env = "GIT_SWEEP_CONFIG")]
    config: Option<PathBuf>,

    /// Report only — list repos with issues (default mode)
    #[arg(long, conflicts_with = "commit")]
    report: bool,

    /// Stage all changes and commit in each dirty repo
    #[arg(long)]
    commit: bool,

    /// After commit, push to the tracked upstream branch
    #[arg(long, requires = "commit")]
    push: bool,

    /// Commit message when using --commit
    #[arg(long, default_value = "chore: sync uncommitted work")]
    message: String,

    /// Show what --commit/--push would do without changing anything
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation prompts (use with --commit/--push)
    #[arg(short = 'y', long)]
    yes: bool,

    /// Emit machine-readable JSON on stdout
    #[arg(long)]
    json: bool,

    /// Also list clean repos (and skipped third-party repos when filtering)
    #[arg(long)]
    verbose: bool,

    /// Exit code 1 if any repo has local changes or is ahead of remote
    #[arg(long)]
    strict: bool,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    owners: Option<Vec<String>>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct RepoFilter {
    mine_only: bool,
    owners: HashSet<String>,
    include: HashSet<String>,
    exclude: HashSet<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepoReport {
    path: String,
    name: String,
    remote: Option<String>,
    remote_owner: Option<String>,
    mine: bool,
    branch: Option<String>,
    dirty: bool,
    staged: u32,
    modified: u32,
    untracked: u32,
    deleted: u32,
    ahead: Option<u32>,
    behind: Option<u32>,
    no_upstream: bool,
    detached: bool,
    porcelain: Vec<String>,
    issues: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SkippedRepo {
    name: String,
    path: String,
    remote: Option<String>,
    remote_owner: Option<String>,
    reason: String,
}

#[derive(Debug, Serialize)]
struct SweepOutput {
    root: String,
    mine_only: bool,
    scanned: usize,
    repos: usize,
    skipped: usize,
    dirty_count: usize,
    ahead_count: usize,
    reports: Vec<RepoReport>,
    skipped_repos: Vec<SkippedRepo>,
}

fn default_root() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("projects"))
        .unwrap_or_else(|| PathBuf::from("projects"))
}

fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("git-sweep").join("config.toml"))
}

fn default_owners() -> Vec<String> {
    vec!["bvelasquez".into(), "mighty45".into()]
}

fn normalize_name(s: &str) -> String {
    s.trim().to_lowercase()
}

fn load_repo_filter(cli: &Cli) -> Result<RepoFilter> {
    let mut file_cfg = FileConfig::default();
    let config_path = cli.config.clone().or_else(default_config_path);
    if let Some(path) = config_path {
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read config {}", path.display()))?;
            file_cfg = toml::from_str(&text)
                .with_context(|| format!("parse config {}", path.display()))?;
        }
    }

    let mut owners: Vec<String> = if cli.owners.is_empty() {
        file_cfg
            .owners
            .clone()
            .unwrap_or_else(default_owners)
    } else {
        cli.owners.clone()
    };

    if owners.is_empty() {
        owners = default_owners();
    }

    let include: HashSet<String> = cli
        .include
        .iter()
        .chain(file_cfg.include.iter().flatten())
        .map(|s| normalize_name(s))
        .collect();

    let exclude: HashSet<String> = cli
        .exclude
        .iter()
        .chain(file_cfg.exclude.iter().flatten())
        .map(|s| normalize_name(s))
        .collect();

    Ok(RepoFilter {
        mine_only: cli.mine && !cli.all,
        owners: owners.into_iter().map(|s| normalize_name(&s)).collect(),
        include,
        exclude,
    })
}

/// Extract github.com/OWNER/... or git@github.com:OWNER/...
fn parse_remote_owner(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            if host.contains("github") || host.contains("gitlab") || host.contains("bitbucket") {
                return path.split('/').next().map(|s| s.to_string());
            }
        }
    }

    // https://github.com/owner/repo
    if url.starts_with("http://") || url.starts_with("https://") {
        if let Some(path_start) = url.find("://").and_then(|i| url[i + 3..].find('/')) {
            let path = &url[url.find("://").unwrap() + 3 + path_start + 1..];
            return path.split('/').next().map(|s| s.to_string());
        }
    }

    // ssh://git@github.com/owner/repo
    if let Some(idx) = url.find("://") {
        let after = &url[idx + 3..];
        if let Some(slash) = after.find('/') {
            let path = &after[slash + 1..];
            return path.split('/').next().map(|s| s.to_string());
        }
    }

    None
}

fn remote_origin(repo: &Path) -> Option<String> {
    run_git(repo, &["config", "--get", "remote.origin.url"]).ok()
}

fn classify_repo(name: &str, remote: Option<&str>, filter: &RepoFilter) -> (bool, Option<String>, String) {
    let name_key = normalize_name(name);

    if filter.exclude.contains(&name_key) {
        return (
            false,
            remote.and_then(|u| parse_remote_owner(u)),
            "excluded by name".into(),
        );
    }

    if filter.include.contains(&name_key) {
        return (true, remote.and_then(|u| parse_remote_owner(u)), "included by name".into());
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
        (Some(o), _) if filter.owners.contains(&normalize_name(o)) => (true, owner, "owned remote".into()),
        (None, None) | (_, None) => (true, None, "no remote (local project)".into()),
        (Some(o), Some(_)) => (
            false,
            owner.clone(),
            format!("third-party remote ({o})"),
        ),
        (None, Some(_)) => (false, None, "unrecognized remote".into()),
    }
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
        );
    }
}

fn find_git_repos(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    if !root.is_dir() {
        return repos;
    }

    fn walk(dir: &Path, depth: usize, max_depth: usize, repos: &mut Vec<PathBuf>) {
        if depth > max_depth {
            return;
        }
        let git = dir.join(".git");
        if git.exists() {
            repos.push(dir.to_path_buf());
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            walk(&path, depth + 1, max_depth, repos);
        }
    }

    if root.join(".git").exists() {
        repos.push(root.to_path_buf());
        return repos;
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return repos;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        walk(&path, 1, max_depth, &mut repos);
    }

    repos.sort();
    repos.dedup();
    repos
}

fn parse_porcelain(porcelain: &str) -> (bool, u32, u32, u32, u32, Vec<String>) {
    let mut staged = 0;
    let mut modified = 0;
    let mut untracked = 0;
    let mut deleted = 0;
    let mut lines = Vec::new();

    for line in porcelain.lines() {
        if line.is_empty() {
            continue;
        }
        lines.push(line.to_string());
        let x = line.chars().next().unwrap_or(' ');
        let y = line.chars().nth(1).unwrap_or(' ');
        match (x, y) {
            ('?', '?') => untracked += 1,
            _ if x == 'D' || y == 'D' => deleted += 1,
            _ => {}
        }
        if x != ' ' && x != '?' {
            staged += 1;
        }
        if y != ' ' && y != '?' {
            modified += 1;
        }
    }

    let dirty = !lines.is_empty();
    (dirty, staged, modified, untracked, deleted, lines)
}

fn branch_info(repo: &Path) -> (Option<String>, bool, bool) {
    let branch = run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).ok();
    let detached = branch.as_deref() == Some("HEAD");
    let no_upstream = if detached {
        true
    } else {
        run_git(
            repo,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .is_err()
    };
    (branch, detached, no_upstream)
}

fn ahead_behind(repo: &Path) -> (Option<u32>, Option<u32>) {
    let Ok(out) = run_git(repo, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]) else {
        return (None, None);
    };
    let mut parts = out.split_whitespace();
    let behind = parts.next().and_then(|s| s.parse().ok());
    let ahead = parts.next().and_then(|s| s.parse().ok());
    (ahead, behind)
}

fn inspect_repo(path: PathBuf, mine: bool, remote: Option<String>, remote_owner: Option<String>) -> Result<RepoReport> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    let porcelain = run_git(&path, &["status", "--porcelain"]).unwrap_or_default();
    let (dirty, staged, modified, untracked, deleted, porcelain_lines) =
        parse_porcelain(&porcelain);
    let (branch, detached, no_upstream) = branch_info(&path);
    let (ahead, behind) = if no_upstream || detached {
        (None, None)
    } else {
        ahead_behind(&path)
    };

    let mut issues = Vec::new();
    if dirty {
        issues.push("dirty".into());
    }
    if detached {
        issues.push("detached".into());
    }
    if no_upstream && !detached {
        issues.push("no_upstream".into());
    }
    if ahead.unwrap_or(0) > 0 {
        issues.push("ahead".into());
    }
    if behind.unwrap_or(0) > 0 {
        issues.push("behind".into());
    }

    Ok(RepoReport {
        path: path.display().to_string(),
        name,
        remote,
        remote_owner,
        mine,
        branch,
        dirty,
        staged,
        modified,
        untracked,
        deleted,
        ahead,
        behind,
        no_upstream,
        detached,
        porcelain: porcelain_lines,
        issues,
    })
}

fn has_actionable_issue(r: &RepoReport) -> bool {
    r.dirty
        || r.ahead.unwrap_or(0) > 0
        || r.behind.unwrap_or(0) > 0
        || r.no_upstream
        || r.detached
}

fn print_human(reports: &[RepoReport], skipped: &[SkippedRepo], verbose: bool) {
    let mut issue_count = 0;
    for r in reports {
        if !has_actionable_issue(r) {
            if verbose {
                println!("{} {}", "OK".green().bold(), r.name);
            }
            continue;
        }
        issue_count += 1;
        let flags: Vec<String> = r.issues.iter().map(|s| format!("[{s}]")).collect();
        println!(
            "{} {} {}",
            "!!".red().bold(),
            r.name.bold(),
            flags.join(" ")
        );
        println!("    {}", r.path.dimmed());
        if let Some(ref b) = r.branch {
            println!("    branch: {b}");
        }
        if let Some(ref remote) = r.remote {
            println!("    remote: {remote}");
        }
        if r.dirty {
            println!(
                "    changes: {} staged, {} modified, {} untracked, {} deleted",
                r.staged, r.modified, r.untracked, r.deleted
            );
            for line in r.porcelain.iter().take(8) {
                println!("      {line}");
            }
            if r.porcelain.len() > 8 {
                println!("      ... +{} more", r.porcelain.len() - 8);
            }
        }
        if let Some(a) = r.ahead {
            if a > 0 {
                println!("    ahead of upstream: {a} commit(s)");
            }
        }
        if let Some(b) = r.behind {
            if b > 0 {
                println!("    behind upstream: {b} commit(s)");
            }
        }
        if r.no_upstream {
            println!("    {}", "no upstream branch configured".yellow());
        }
        if r.detached {
            println!("    {}", "detached HEAD".yellow());
        }
        println!();
    }

    if verbose && !skipped.is_empty() {
        println!("{}", "Skipped (third-party / not yours):".dimmed());
        for s in skipped {
            let remote = s.remote.as_deref().unwrap_or("(no remote)");
            println!(
                "  {} — {} — {}",
                s.name.dimmed(),
                remote.dimmed(),
                s.reason.dimmed()
            );
        }
        println!();
    }

    let clean = reports.len().saturating_sub(issue_count);
    println!(
        "{}",
        format!(
            "Summary: {} repo(s), {} with issues, {} clean, {} skipped",
            reports.len(),
            issue_count,
            clean,
            skipped.len()
        )
        .bold()
    );
}

fn confirm(prompt: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    use std::io::{self, Write};
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn commit_repo(repo: &Path, message: &str, push: bool, dry_run: bool, yes: bool) -> Result<()> {
    let porcelain = run_git(repo, &["status", "--porcelain"])?;
    if porcelain.is_empty() {
        let ahead = ahead_behind(repo).0.unwrap_or(0);
        if push && ahead > 0 {
            if !confirm(
                &format!("Push {} unpushed commit(s) in {}?", ahead, repo.display()),
                yes,
            )? {
                return Ok(());
            }
            if dry_run {
                println!("[dry-run] would push {}", repo.display());
                return Ok(());
            }
            run_git(repo, &["push"])?;
            println!("{} pushed {}", "->".green(), repo.display());
        }
        return Ok(());
    }

    if !confirm(
        &format!("Stage and commit all changes in {}?", repo.display()),
        yes,
    )? {
        return Ok(());
    }

    if dry_run {
        println!("[dry-run] would: git add -A && git commit in {}", repo.display());
        if push {
            println!("[dry-run] would: git push in {}", repo.display());
        }
        return Ok(());
    }

    run_git(repo, &["add", "-A"])?;
    let status = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo)
        .status()
        .context("git commit failed")?;

    if !status.success() {
        bail!("git commit failed in {}", repo.display());
    }
    println!("{} committed {}", "✓".green(), repo.display());

    if push {
        run_git(repo, &["push"])?;
        println!("{} pushed {}", "->".green(), repo.display());
    }

    Ok(())
}

fn partition_repos(
    repo_paths: Vec<PathBuf>,
    filter: &RepoFilter,
) -> (Vec<(PathBuf, bool, Option<String>, Option<String>)>, Vec<SkippedRepo>) {
    let mut included = Vec::new();
    let mut skipped = Vec::new();

    for path in repo_paths {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let remote = remote_origin(&path);
        let (mine, owner, reason) =
            classify_repo(&name, remote.as_deref(), filter);

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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.clone().unwrap_or_else(default_root);
    let root = root
        .canonicalize()
        .with_context(|| format!("projects root not found: {}", root.display()))?;

    let filter = load_repo_filter(&cli)?;

    let report_only = !cli.commit;
    if report_only && cli.push {
        bail!("--push requires --commit");
    }

    let all_repo_paths = find_git_repos(&root, cli.depth);
    let scanned = all_repo_paths.len();
    let (included_paths, skipped) = partition_repos(all_repo_paths, &filter);

    let reports: Vec<RepoReport> = included_paths
        .par_iter()
        .map(|(p, mine, remote, owner)| {
            inspect_repo(p.clone(), *mine, remote.clone(), owner.clone())
        })
        .collect::<Result<Vec<_>>>()?;

    let dirty_count = reports.iter().filter(|r| r.dirty).count();
    let ahead_count = reports
        .iter()
        .filter(|r| r.ahead.unwrap_or(0) > 0)
        .count();

    if cli.json {
        let out = SweepOutput {
            root: root.display().to_string(),
            mine_only: filter.mine_only,
            scanned,
            repos: reports.len(),
            skipped: skipped.len(),
            dirty_count,
            ahead_count,
            reports: reports.clone(),
            skipped_repos: skipped,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if report_only {
        let mode = if filter.mine_only {
            format!(
                "mine only (owners: {})",
                filter.owners.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        } else {
            "all repos".into()
        };
        println!(
            "Scanning {} (depth={}, {}, {} git repos, {} skipped)\n",
            root.display(),
            cli.depth,
            mode,
            reports.len(),
            skipped.len()
        );
        print_human(&reports, &skipped, cli.verbose);
    }

    if cli.commit {
        let to_process: Vec<_> = reports
            .iter()
            .filter(|r| r.dirty || r.ahead.unwrap_or(0) > 0)
            .collect();

        if to_process.is_empty() {
            println!("Nothing to commit or push.");
        } else {
            println!(
                "\n{} repo(s) with local changes or unpushed commits\n",
                to_process.len()
            );
        }

        let ok = AtomicUsize::new(0);
        let err = AtomicUsize::new(0);
        for r in to_process {
            match commit_repo(
                Path::new(&r.path),
                &cli.message,
                cli.push,
                cli.dry_run,
                cli.yes,
            ) {
                Ok(()) => {
                    ok.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    err.fetch_add(1, Ordering::Relaxed);
                    eprintln!("{} {}: {e:#}", "error".red(), r.name);
                }
            }
        }
        if cli.commit && !cli.json {
            println!(
                "\nDone: {} processed, {} error(s)",
                ok.load(Ordering::Relaxed),
                err.load(Ordering::Relaxed)
            );
        }
        if err.load(Ordering::Relaxed) > 0 {
            std::process::exit(2);
        }
    }

    if cli.strict && reports.iter().any(has_actionable_issue) {
        std::process::exit(1);
    }

    Ok(())
}
