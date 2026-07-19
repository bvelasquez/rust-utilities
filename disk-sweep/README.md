# disk-sweep

Agent-first Rust CLI for smart disk cleanup on macOS. Scans common cache and Xcode junk folders, shows sizes in an interactive **ratatui** TUI, and supports LLM-powered folder review for discovering new cleanup candidates.

Inspired by cleanup managers that break down **System Junk** into Xcode archives, device support, derived data, user caches, and logs.

## Install

From this repo:

```bash
cd disk-sweep
cargo build --release
cargo install --path . --force
```

Binary: `disk-sweep`

## Quick start

```bash
# Interactive cleanup TUI (default in a TTY)
disk-sweep

# Live disk usage dashboard
disk-sweep watch

# Watch only specific folders (skips default set)
disk-sweep watch --path ~/projects --path ~/Library/Developer --interval 15s

# One-shot watch snapshot for agents
disk-sweep watch --json

# Scan sizes as JSON (automation)
disk-sweep scan --json

# Detailed per-item breakdown
disk-sweep scan --detail --json

# Find dot folders, Library hogs, and stale projects (nothing pre-selected)
disk-sweep analyze --json

# Dry-run cleanup of default Xcode selections
disk-sweep clean --dry-run --json

# LLM review of an unknown folder (reads DISK_SWEEP_OPENROUTER_KEY from .env when present)
cp .env.example .env   # add your OpenRouter key
disk-sweep review ~/Library/Developer --json
```

## TUI controls

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Move between category, item, and detail panels |
| `↑`/`↓` or `j`/`k` | Navigate lists |
| `Space` | Toggle selection (items) or all in category |
| `*` / `A` | Select all existing items |
| `n` | Clear selection |
| `a` | Analyze — pick projects root, scan stale projects |
| `r` | Rescan sizes |
| `c` | Clean selected (with confirmation) |
| `q` | Quit |

The footer shows how many items are selected and how much space you can reclaim.

## Watch mode

Live ratatui dashboard for disk health. **Deep scans are manual** (`r`) — no more full rescans on a timer.

```bash
disk-sweep watch
disk-sweep watch --interval 30s   # auto-refresh volume gauge only
disk-sweep watch --path ~/projects
```

| Key | Action |
|-----|--------|
| `r` | Deep scan (folders + cleanup targets) |
| `a` | Analyze — pick projects root, then scan dot folders, Library, stale projects |
| `v` | Refresh volume gauge only (fast) |
| `Space` | Toggle cleanup item selection |
| `*` / `A` | Select all |
| `n` | Clear selection |
| `c` | Clean selected (with confirmation) |
| `Tab` | Switch focus |
| `q` | Quit (cancels in-progress scan) |

Default `--interval` is **0** (manual). When set (e.g. `30s`), only the volume gauge auto-refreshes — not folder/cleanup scans.

## Analyze mode

`disk-sweep analyze` (or **`a`** in watch) scans three areas for cleanup candidates. **Nothing is selected by default.**

| Area | What it finds |
|------|----------------|
| **Dot folders** | `~/.ollama`, `~/.gradle`, `~/.npm`, `~/.cache`, etc. (≥100 MB) |
| **Library** | Large `Application Support`, `Containers`, `Caches`, and `Logs` children (≥200 MB) |
| **Stale projects** | Git repos under `~/projects` with no commits in **180 days** (configurable) |
| **Rust build artifacts** | `target/debug/incremental` and `target/release/incremental` per project (≥50 MB); falls back to whole `target/` |

Stale **clean** projects are tagged `safe_cleanup`. Dirty or unpushed projects are `caution` — cleaning will commit tracked changes, copy untracked files to `~/Documents/disk-sweep-archives/`, push if possible, then remove the folder.

Rust incremental caches are always `safe_cleanup` — deleting them only forces a longer next `cargo build`.

```bash
disk-sweep analyze --json
disk-sweep analyze --stale-days 90 --projects-root ~/projects
disk-sweep analyze --skip-dot --skip-library --projects-root ~/projects   # projects only (fast)
```

## Default cleanup targets

### Xcode Junk (selected by default)

| ID | Path |
|----|------|
| `xcode-archives` | `~/Library/Developer/Xcode/Archives` |
| `xcode-ios-device-support` | `~/Library/Developer/Xcode/iOS DeviceSupport` |
| `xcode-derived-data` | `~/Library/Developer/Xcode/DerivedData` |
| `xcode-macos-device-support` | `~/Library/Developer/Xcode/macOS DeviceSupport` |
| `xcode-watchos-device-support` | `~/Library/Developer/Xcode/watchOS DeviceSupport` |
| `xcode-documentation-cache` | `~/Library/Developer/Xcode/DocumentationCache` |
| `xcode-simulator-caches` | `~/Library/Developer/CoreSimulator/Caches` (opt-in; temp caches only, not sim devices) |

### User Cache Files (opt-in)

Expands `~/Library/Caches` into per-app cache folders.

### User Log Files (opt-in)

Expands `~/Library/Logs` into per-app log folders.

List all targets:

```bash
disk-sweep targets list --json
disk-sweep targets explain
```

### What is never deleted

disk-sweep only scans known cache/build/log paths under `~/Library`. It does **not** touch:

- Source code, git repos, or `~/projects`
- Installed simulators (`CoreSimulator/Devices`) or their apps/data
- Photos, Documents, Downloads (unless you add custom `--path` for watch only)
- System files outside the configured targets

Run `disk-sweep targets explain` for the full list with descriptions.

## Agent / automation

```bash
disk-sweep capabilities --json
disk-sweep env schema --json
```

Mutations (`clean`) require `--yes` or `--dry-run` when stdout is not a TTY.

### LLM review

The `review` command sends immediate child folder names and sizes to an LLM and returns structured verdicts:

- `safe_cleanup` — caches, build artifacts, regenerable data
- `caution` — review before deleting
- `do_not_delete` — source, documents, irreplaceable data

| Variable | Purpose |
|----------|---------|
| `DISK_SWEEP_OPENROUTER_KEY` / `OPENROUTER_API_KEY` | OpenRouter API key |
| `DISK_SWEEP_LLM_MODEL` | OpenRouter model override (default: `openai/gpt-4o-mini`) |

## Safety

- Xcode archives and derived data are **regenerable** but deleting archives removes old App Store build snapshots.
- User caches/logs are **not** selected by default — opt in via the TUI or `clean --targets`.
- Always run `clean --dry-run` before automation.

## License

MIT
