# secret-sweep

Back up **local-only** secrets and dotfiles across all your git projects into a single encrypted archive — and flag credentials that were accidentally **committed** to git.

When you maintain dozens of repos, each with its own `.env`, API keys, and config dotfiles, secret-sweep gives you one place to audit and backup what would otherwise be lost on a disk failure or machine swap.

## What it does

- Scans git repos under a projects root (same discovery model as git-sweep)
- Collects dotfiles and secret-like files (`.env*`, `*.pem`, `credentials.json`, etc.)
- **Backs up only local-only files** — untracked, gitignored, or modified/staged — never committed clean copies
- **Warns** when secret-looking files are already committed (with remediation hints)
- Writes encrypted `.svault` archives (AES-256-GCM, Argon2id key derivation)
- Supports restore and inspect of archives

## Install

```bash
cargo build --release
# binary: target/release/secret-sweep
```

## Usage

**Scan (default) — see what would be backed up:**

```bash
secret-sweep scan
secret-sweep scan --verbose
secret-sweep scan --strict          # exit 1 if committed secrets found
secret-sweep scan --json
```

**Create an encrypted backup:**

```bash
secret-sweep backup --yes
secret-sweep backup --output ~/Backups/my-secrets.svault --yes
secret-sweep backup --dry-run
```

**Restore after setting up a new machine:**

```bash
secret-sweep restore ~/Backups/secret-sweep-2026-06-22.svault --yes
secret-sweep restore archive.svault --project my-app --force --yes
```

**List archive contents:**

```bash
secret-sweep inspect ~/Backups/secret-sweep-2026-06-22.svault
```

## What gets backed up

Files matching:

- Any dotfile or dot-directory path (e.g. `.env`, `.npmrc`, `.config/...`)
- Extra globs: `*.pem`, `*.key`, `*.p12`, `credentials.json`, `service-account*.json`, etc.

Only if git status is **untracked**, **ignored**, **modified**, or **staged** — not if the file is committed and clean.

## Committed-secrets warnings

If a file looks like a secret but is committed to git, secret-sweep does **not** include it in the backup. Instead it reports it so you can rotate the credential and remove it from history.

Run `scan --strict` in CI or a pre-backup hook to catch mistakes early.

## Configuration

`~/.config/secret-sweep/config.toml`:

```toml
owners = ["your-github-user"]
include = ["legacy-project"]
exclude = ["upstream-clone"]
patterns = ["secrets/*.yaml"]
max_file_size = 1048576   # 1 MiB per file
max_walk_depth = 8
```

## Environment variables

| Variable | Purpose |
|----------|---------|
| `SECRET_SWEEP_ROOT` | Projects root (default `~/projects`) |
| `SECRET_SWEEP_CONFIG` | Config file path |
| `SECRET_SWEEP_OWNERS` | Comma-separated remote owners |
| `SECRET_SWEEP_PASSWORD` | Archive password (prefer interactive prompt) |

## Archive format

`.svault` files use a custom encrypted format (`SVAULT` magic, version 1). Keep passwords safe — there is no recovery without them.

Default backup location: `~/Backups/secret-sweep-<timestamp>.svault`

## Security notes

- Archives contain plaintext secrets after decryption — store `.svault` files securely
- `.svault` files are gitignored in this repo; never commit them
- Prefer `secret-sweep scan --strict` regularly to catch committed credentials before they spread
