# mail-sweep

Agent-first Rust CLI for AI-powered email triage. Syncs mail over **IMAP**, classifies batches with **OpenRouter**, applies structured actions (archive, flag, move, delete), and provides a **ratatui** inbox dashboard.

## Quick start

```bash
cd mail-sweep && cargo build --release
# or
make install   # puts mail-sweep on ~/.cargo/bin
cargo install --path .
```

Configure accounts and secrets (no exported env vars required):

```bash
mail-sweep accounts add --id personal --email you@gmail.com --gmail
mail-sweep accounts add --id icloud --email you@icloud.com --icloud
mail-sweep secrets set-openrouter-key --key sk-or-v1-...
mail-sweep secrets set-account --id personal --password 'your-app-password'
mail-sweep accounts test personal --json
```

**iCloud:** use `--icloud` and an [app-specific password](https://appleid.apple.com) (Sign-In and Security → App-Specific Passwords). Your Apple ID password will not work.
Or copy [`.env.example`](.env.example) to `.env` in your project or `~/.config/mail-sweep/.env`:

```env
openrouter_api_key=sk-or-v1-...
account_personal_password=your-app-password
```

Sync and classify:

```bash
mail-sweep sync --json
mail-sweep process --dry-run --json
mail-sweep apply --yes --json
```

Interactive TUI (default in a TTY):

```bash
mail-sweep
# keys: Tab switch view, j/k move, s sync, x process, q quit
```

## Agent discovery

```bash
mail-sweep capabilities --json
mail-sweep config schema --json
```

Every command supports `--json` for the standard envelope (`success`, `command`, `data`, `warnings`, `next_actions`).

## Configuration

| File | Purpose |
|------|---------|
| `~/.config/mail-sweep/config.toml` | Accounts, rules, sync/safety settings |
| `~/.config/mail-sweep/secrets.toml` | API keys and account passwords (via CLI or hand-edited) |
| `.env` or `~/.config/mail-sweep/.env` | Alternative secrets format (parsed directly, not exported to shell) |
| `~/.local/share/mail-sweep/mail-sweep.db` | Message cache |

**Precedence:** CLI flags → `secrets.toml` → `config.toml` → `.env`

### config.toml

```toml
[llm]
model = "openai/gpt-4o-mini"

[sync]
poll_interval = "5m"
batch_size = 25
initial_fetch_limit = 50
full_fetch_limit = 500
imap_timeout_secs = 120

[safety]
allow_delete = false
auto_apply_min_confidence = 0.88
plan_min_confidence = 0.55

[[accounts]]
id = "personal"
email = "you@gmail.com"
imap_host = "imap.gmail.com"
imap_port = 993
smtp_host = "smtp.gmail.com"
smtp_port = 587
```

### secrets.toml

```toml
openrouter_api_key = "sk-or-v1-..."
llm_model = "openai/gpt-4o-mini"

[accounts]
personal = "your-app-password"
```

### CLI secrets

```bash
mail-sweep secrets list
mail-sweep secrets set openrouter-key --key sk-or-v1-...
mail-sweep secrets set llm-model --model openai/gpt-4o-mini
mail-sweep secrets set account --id personal --password '...'
mail-sweep accounts add --id personal --email you@gmail.com --gmail --password '...'
```

## Commands

| Command | Description |
|---------|-------------|
| `sync` | Fetch new mail via IMAP into local cache |
| `process` | Rules + OpenRouter batch classification |
| `apply` | Execute plan on IMAP (`--yes` / `--dry-run`) |
| `list` / `show` | Browse cached messages |
| `stats` | Category and sender breakdown |
| `send` | Send via SMTP |
| `accounts` | list / add / test |
| `secrets` | list / set openrouter-key / set account / set llm-model |
| `rules` | list / add / remove / test |
| `learn feedback` | Record sender correction for future batches |

## Gmail notes

- Use an [App Password](https://support.google.com/accounts/answer/185833) (OAuth deferred to a later version).
- Archive maps to `[Gmail]/All Mail` via IMAP MOVE when supported.

## Safety

- Mutations in non-TTY mode require `--yes` or `--dry-run`.
- IMAP delete requires `safety.allow_delete = true` or `--allow-delete`.
- Low-confidence plans appear in the TUI review queue.
