# git-sweep

Scan a projects folder for git repositories that need attention — uncommitted changes, unpushed commits, missing upstream branches, or detached HEAD — and optionally fix them in bulk.

Useful when you work across many repos under `~/projects` and want a single command to answer “what did I leave dirty?” before shutting down for the day, or to sync everything with one scripted pass.

## What it does

- Discovers git repos under a root directory (default `~/projects`, depth 1)
- Filters to **your** repos by default (remote owner matches configured GitHub/GitLab users, or no remote)
- Reports dirty state with porcelain details, ahead/behind counts, and branch info
- Optionally **stage all**, **commit**, and **push** across every repo that needs it
- Emits JSON for scripts and CI (`--json`, `--strict`)

## Install

```bash
cargo build --release
# binary: target/release/git-sweep
```

## Usage

**Report only (default):**

```bash
git-sweep
git-sweep --root ~/code --verbose
git-sweep --json --strict   # exit 1 if any repo has issues
```

**Commit and push across dirty repos:**

```bash
git-sweep --commit --push --yes --message "chore: sync uncommitted work"
git-sweep --commit --dry-run   # preview without changes
```

**Include third-party clones:**

```bash
git-sweep --all
```

## Filtering

By default, git-sweep only scans repos you “own”:

- Remote `origin` owner is in your owner list (default: `bvelasquez`, `mighty45`)
- No remote configured (local-only projects)
- Explicitly included by folder name

Repos with third-party remotes are skipped unless you pass `--all`, use `--include`, or add them in config.

## Configuration

`~/.config/git-sweep/config.toml`:

```toml
owners = ["your-github-user", "your-org"]
include = ["special-fork"]
exclude = ["vendor-mirror"]
```

## Environment variables

| Variable | Purpose |
|----------|---------|
| `GIT_SWEEP_ROOT` | Projects root (default `~/projects`) |
| `GIT_SWEEP_CONFIG` | Config file path |
| `GIT_SWEEP_OWNERS` | Comma-separated remote owners |
| `GIT_SWEEP_MINE` | `true`/`false` — mine-only filter (default true) |

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success (or `--strict` with no issues) |
| 1 | `--strict` and at least one repo has actionable issues |
| 2 | One or more repos failed during `--commit` / `--push` |

## Example output

```
Scanning /Users/you/projects (depth=1, mine only (owners: bvelasquez, mighty45), 12 git repos, 3 skipped)

!! my-app [dirty] [ahead]
    /Users/you/projects/my-app
    branch: main
    changes: 0 staged, 2 modified, 1 untracked, 0 deleted
    ahead of upstream: 1 commit(s)

Summary: 12 repo(s), 1 with issues, 11 clean, 3 skipped
```
