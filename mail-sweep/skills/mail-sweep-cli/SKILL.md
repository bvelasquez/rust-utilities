---
name: mail-sweep-cli
description: Agent-first Rust email triage CLI — IMAP sync, OpenRouter classification, ratatui inbox, rules and learning. Use for mail-sweep commands, email automation, or configuring IMAP/SMTP accounts.
---

# mail-sweep CLI

## When to use

- Sync and triage email via IMAP with AI batch classification
- Automate `sync` → `process --dry-run` → `apply --yes` flows
- Open the ratatui inbox for priority mail and review queue

## Discovery

```bash
mail-sweep capabilities --json
mail-sweep config schema --json
```

## Setup (no shell env vars)

```bash
mail-sweep accounts add --id personal --email you@gmail.com --gmail
mail-sweep secrets set-openrouter-key --key sk-or-v1-...
mail-sweep secrets set-account --id personal --password '...'
```

Or use `.env` / `~/.config/mail-sweep/secrets.toml` (see `config schema --json`).

## Typical agent flow

```bash
mail-sweep sync --json
mail-sweep process --dry-run --json
mail-sweep apply --yes --json
mail-sweep stats --json
```

## Safety

- Non-TTY mutations need `--yes` or `--dry-run`
- Deletes need `safety.allow_delete` or `--allow-delete`
