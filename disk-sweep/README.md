# disk-sweep

Agent-first Rust CLI for disk cleanup, duplicate finding, and rules-based file organization. On macOS it scans Xcode junk and `~/Library` caches. On Linux it scans `~/.cache`. An interactive **ratatui** TUI can browse, clean, find duplicates, and organize drop folders.

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

# Find dot folders, Library or cache hogs, and stale projects (nothing pre-selected)
disk-sweep analyze --json

# Dry-run cleanup of default selections
disk-sweep clean --dry-run --json

# Duplicate files under a folder (read-only)
disk-sweep dups ~/photos --json

# Delete extra copies. A terminal asks you to type delete.
disk-sweep dups ~/photos --apply --keep shortest

# Write an organize plan, review it, then apply that file
disk-sweep organize --once --plan moves.yaml
disk-sweep organize --apply moves.yaml

# Headless: move files as they are dropped into source folders
disk-sweep organize --watch

# Agent catalog
disk-sweep --capabilities
disk-sweep capabilities --json

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
| `d` | Duplicate scan — browse to a folder and scan |
| `o` | Organizer — preview moves, write a plan file, apply after confirmation |
| `r` | Rescan sizes |
| `c` | Clean selected (confirmation lists the paths) |
| `q` | Quit |

The footer shows how many items are selected and how much space you can reclaim.

Duplicate browser: `j`/`k` move, Enter opens a folder, Backspace goes up, `s` starts a read-only scan, `r` toggles recursive, `S` toggles SHA-256. Space marks the highlighted folder to skip, `*` skips every folder in the current list, and `n` clears those skips. Skipped folders and everything inside them are not compared. On the results screen, `K` keeps the highlighted copy, Space toggles an extra copy, and `x` asks you to type `delete` before anything is removed.

Organizer screen: `o` lists files already in the drop folder. `a` moves that list after you type `apply`. `A` turns on auto mode: every file dropped in the folder is moved by the rules as soon as it finishes copying, with no plan file and no extra confirmation. `A` again stops auto. From the shell, `disk-sweep organize --watch` is the same auto mode and keeps running until Ctrl-C.

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
| `a` | Analyze — pick projects root, then scan dot folders, Library or caches, stale projects |
| `d` | Duplicate scan |
| `o` | Organizer |
| `v` | Refresh volume gauge only (fast) |
| `Space` | Toggle cleanup item selection |
| `*` / `A` | Select all |
| `n` | Clear selection |
| `c` | Clean selected (confirmation lists paths) |
| `Tab` | Switch focus |
| `q` | Quit (cancels in-progress scan) |

Default `--interval` is **0** (manual). When set (e.g. `30s`), only the volume gauge auto-refreshes — not folder/cleanup scans.

## Analyze mode

`disk-sweep analyze` (or **`a`** in watch) scans three areas for cleanup candidates. **Nothing is selected by default.**

| Area | What it finds |
|------|----------------|
| **Dot folders** | `~/.ollama`, `~/.gradle`, `~/.npm`, `~/.cache`, etc. (≥100 MB) |
| **Library (macOS)** | Large `Application Support`, `Containers`, `Caches`, and `Logs` children (≥200 MB) |
| **Caches (Linux)** | Large children of `~/.cache` and `~/.local/share` (≥200 MB) |
| **Stale projects** | Git repos under `~/projects` with no commits in **180 days** (configurable) |
| **Rust build artifacts** | `target/debug/incremental` and `target/release/incremental` per project (≥50 MB); falls back to whole `target/` |

Stale **clean** projects are tagged `safe_cleanup`. Dirty or unpushed projects are `caution` — cleaning will commit tracked changes, copy untracked files to `~/Documents/disk-sweep-archives/`, push if possible, then remove the folder.

Rust incremental caches are always `safe_cleanup` — deleting them only forces a longer next `cargo build`.

```bash
disk-sweep analyze --json
disk-sweep analyze --stale-days 90 --projects-root ~/projects
disk-sweep analyze --skip-dot --skip-library --projects-root ~/projects   # projects only (fast)
```

## Duplicate scan

`disk-sweep dups [PATH]` walks a folder (recursive by default) and reports files with the same content. The scan is read-only. Nothing is stored in a database.

Hashing uses three gates so unique files are not read in full:

1. Group by size. A size that appears once is skipped.
2. xxHash64 of the first 64KB and the last 64KB.
3. xxHash64 of the whole file, only when the partial hash matches.
4. Optional `--sha256` confirms the remaining groups.

Symlinks are not followed. Hard links of the same inode count as one file. The scan stays on the same filesystem unless you pass `--cross-devices`. `--no-recursive` reads only the top directory. `--min-bytes` defaults to 1. Repeat `--exclude DIR` to leave folders out of the scan.

```bash
disk-sweep dups
disk-sweep dups ~/photos --json
disk-sweep dups ~/photos --sha256
disk-sweep dups ~/photos --exclude ~/photos/originals --exclude ~/photos/archive
disk-sweep dups ~/photos --apply --keep newest
```

`--apply` deletes every extra copy and always leaves one. `--keep` is `shortest` (default), `newest`, or `oldest` based on modified time. In a terminal you must type `delete`. When stdout is not a terminal, also pass `--yes`. Before each unlink, disk-sweep re-hashes the kept file and the copy, refuses symlinks and directories, and refuses a path outside the scan root. A CSV log is written under `~/.local/share/disk-sweep/` before the first deletion.

## Organizer

`disk-sweep organize` moves files from drop folders into destination trees described by `~/.config/disk-sweep/rules.yaml`. Override the file with `--rules`.

```yaml
sources:
  - path: ~/Inbox
    dest_root: ~/Sorted
    settle_secs: 2
    rules:
      - name: photos
        extensions: [jpg, jpeg, png]
        path: "{ext}/{yyyy}-{mm}-{dd}/{filename}"
      - name: pdfs
        extensions: [pdf]
        path: "{ext}/{yyyy}/{filename}"
      - name: invoices
        filename_regex: '(?i)^invoice-(?P<client>[a-z0-9]+)[-_].+\.pdf$'
        path: "invoices/{client}/{yyyy}/{filename}"
    fallback: unsorted/{filename}
```

A rule matches `extensions`, `filename_regex`, or both (both means both must match). The regex is applied to the file name only. Named captures become path tokens (`{client}`). A capture cannot reuse `{ext}`, `{filename}`, `{yyyy}`, `{mm}`, or `{dd}`. An invalid pattern fails when the file is loaded, before any move.

`{yyyy}`, `{mm}`, and `{dd}` come from the first date that exists: EXIF `DateTimeOriginal`, then `DateTimeDigitized`, then `DateTime` (JPEG and TIFF), then the file created/birth time, then modified time, then the moment of organizing. The move log records which source was used.

Only the files sitting directly in each source folder are considered. Hidden files and symlinks are ignored. A file is moved once its modified time is at least `settle_secs` old, so a copy still in progress stays in the drop folder. Destinations that already exist get a numeric suffix (`name (2).ext`). A template containing `..` is rejected. The destination must stay inside `dest_root`.

Before a file is filed, disk-sweep hashes it with SHA-256 and checks `~/.local/share/disk-sweep/organize-catalog.sqlite`. The catalog is filled from each `dest_root` the first time it is used (the drop folder and `duplicates/` are not treated as the library). If that content is already stored and the recorded file is still there, the new file is moved to `dest_root/duplicates/` so auto mode does not keep retrying it. A name clash still gets a numeric suffix. If the recorded file is gone, the new file is filed and the catalog path is updated.

```bash
disk-sweep organize --once
disk-sweep organize --once --plan moves.yaml
disk-sweep organize --dry-run moves.md
disk-sweep organize --apply moves.yaml
disk-sweep organize --watch
```

`--plan` and `--dry-run` are the same option. They write the planned moves and do not touch files. `.yaml` / `.yml` is the move list. `.md` is a table plus the same list in a fenced YAML block, and `--apply` reads that block. `--apply` does not run the rules again. It skips a source that disappeared or a destination that already exists, and it does not apply later entries after a failed check.

`--watch` is the full auto mode. It organizes files already in the drop folder, then moves new files as soon as they arrive. It does not write or read a plan. Combining `--watch` with `--plan`, `--dry-run`, or `--apply` is an error. Press Ctrl-C to stop. Press `A` on the organize screen for the same behavior.

Applied moves are appended to `~/.local/share/disk-sweep/organize-YYYYMMDD.csv`.

## Default cleanup targets

Targets depend on the host OS. `disk-sweep targets explain` prints which set is active.

### macOS — Xcode Junk (selected by default)

| ID | Path |
|----|------|
| `xcode-archives` | `~/Library/Developer/Xcode/Archives` |
| `xcode-ios-device-support` | `~/Library/Developer/Xcode/iOS DeviceSupport` |
| `xcode-derived-data` | `~/Library/Developer/Xcode/DerivedData` |
| `xcode-macos-device-support` | `~/Library/Developer/Xcode/macOS DeviceSupport` |
| `xcode-watchos-device-support` | `~/Library/Developer/Xcode/watchOS DeviceSupport` |
| `xcode-documentation-cache` | `~/Library/Developer/Xcode/DocumentationCache` |
| `xcode-simulator-caches` | `~/Library/Developer/CoreSimulator/Caches` (opt-in; temp caches only, not sim devices) |

### macOS — User Cache Files and User Log Files (opt-in)

`~/Library/Caches` and `~/Library/Logs`, expanded per app.

### Linux

| ID | Path | Default |
|----|------|---------|
| `thumbnail-cache` | `~/.cache/thumbnails` | selected |
| `user-caches` | `$XDG_CACHE_HOME` or `~/.cache` | opt-in, per app |

Trash, `/var/cache`, `/var/log`, and the systemd journal are never targets. Other operating systems use this user-cache set and `targets explain` says the host is unsupported.

List all targets:

```bash
disk-sweep targets list --json
disk-sweep targets explain
```

### What is never deleted

disk-sweep only scans the cache and build paths for the current OS. It does **not** touch:

- Source code, git repos, or `~/projects` (except the explicit stale-project clean)
- Installed simulators (`CoreSimulator/Devices`) or their apps/data
- Photos, Documents, Downloads (unless you add a custom `--path` for watch, or an organize rule you wrote)
- System files outside the configured targets

Run `disk-sweep targets explain` for the full list with descriptions.

## Agent / automation

```bash
disk-sweep --capabilities
disk-sweep capabilities --json
disk-sweep env schema --json
```

`--capabilities` and `capabilities --json` return the host OS, command flags, duplicate-scan safety rules, organizer rule fields, and the cleanup target ids for this machine.

Mutations (`clean`, `dups --apply`, `organize --once`, `organize --apply`) require a confirmation when stdout is not a TTY: `--yes` for deletes, and a plan file for organize apply.

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
- User caches are **not** selected by default — opt in via the TUI or `clean --targets`. Thumbnail cache on Linux is the exception, because it is recreated.
- Cleanup confirmation in the TUI lists the paths. Symlinks are never deleted.
- Duplicate deletion re-hashes both copies and keeps one.
- Organize plan mode does not move files. Watch mode never writes a plan.
- Always run `clean --dry-run` or `organize --plan` before automation.

## License

MIT
