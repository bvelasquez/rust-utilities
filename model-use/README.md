# model-use

Agent-first Rust CLI that aggregates LLM spend from **OpenRouter**, **Anthropic**, **OpenAI**, and **Cursor** into a local cache and displays it in an interactive **ratatui** dashboard with monthly budgets.

## Install

From [crates.io](https://crates.io/crates/model-use):

```bash
cargo install model-use
```

From this repo:

```bash
cd model-use
make setup-sccache   # optional, recommended (sccache + lld)
make install         # build release + install to ~/.cargo/bin
```

Or manually:

```bash
cargo build --release
cargo install --path . --force
```

Binary: `model-use`

## Quick start

```bash
# Configure keys (see Key types below)
model-use providers set openrouter --key "$OPENROUTER_MGMT_KEY"
model-use providers set anthropic --key "$ANTHROPIC_ADMIN_KEY"
model-use providers set openai --key "$OPENAI_ADMIN_KEY"
model-use providers set cursor --key "$CURSOR_ADMIN_KEY"
# optional on teams: filter to your email only
model-use providers set cursor --key "$CURSOR_ADMIN_KEY" --email "you@example.com"

# Validate keys
model-use providers test

# Pull usage (default 90 days)
model-use fetch

# Set budgets
model-use budget set global --monthly 200
model-use budget set openrouter --monthly 75
model-use budget set anthropic --monthly 100
model-use budget set openai --monthly 50
model-use budget set cursor --monthly 100

# Interactive dashboard (default when run in a TTY with no subcommand)
model-use watch

# JSON summary
model-use summary --period month --json
```

## Key types

| Provider | Key type | Notes |
|---|---|---|
| OpenRouter | **Management key** | Regular inference keys cannot access `/analytics/*` (403) |
| Anthropic | **Admin key** (`sk-ant-admin01-...`) | Org admin API for `cost_report` |
| OpenAI | **Organization admin key** | Not a standard `sk-...` inference key |
| Cursor | **Admin API key** (`admin:*` scope) | Enterprise teams — [dashboard → API Keys](https://cursor.com/dashboard) → New API Key |

Do **not** use a Cloud Agents / user API key (the kind that works with `/v1/me` or the Cursor SDK). Those keys start with `crsr_` too but lack `admin:*` scope and return `Invalid Team API Key` on usage endpoints.

Individual Cursor Pro and non-Enterprise team accounts do not have access to the Admin API.

Run `model-use providers test` for actionable errors and documentation links.

## Config

`~/.config/model-use/config.toml`:

```toml
[openrouter]
api_key = "..."
enabled = true

[anthropic]
api_key = "sk-ant-admin01-..."
enabled = true

[openai]
api_key = "..."
enabled = true

[cursor]
api_key = "..."
email = "you@example.com"  # optional team filter
enabled = true

[budgets]
global_monthly_usd = 200.0

[budgets.openrouter]
monthly_usd = 75.0

[budgets.anthropic]
monthly_usd = 100.0

[budgets.openai]
monthly_usd = 50.0

[budgets.cursor]
monthly_usd = 100.0

[tui]
refresh_interval_secs = 900  # 15m; 0 disables auto-refresh
```

Environment overrides: `MODEL_USE_OPENROUTER_KEY`, `MODEL_USE_ANTHROPIC_KEY`, `MODEL_USE_OPENAI_KEY`, `MODEL_USE_CURSOR_KEY`, `MODEL_USE_CURSOR_EMAIL`, `MODEL_USE_CONFIG`.

Cached usage: `~/.config/model-use/cache.db` (SQLite).

## Commands

| Command | Description |
|---|---|
| `watch` | TUI dashboard — tabs, `d`/`w`/`m` period, `r` fetch, `q` quit |
| `fetch [--days N]` | Pull usage from enabled providers |
| `providers list` | Show provider config status |
| `providers set <name> --key <key>` | Save API key |
| `providers test [name]` | Validate key permissions |
| `providers enable/disable <name>` | Toggle provider |
| `budget set global --monthly <usd>` | Global monthly budget |
| `budget set <provider> --monthly <usd>` | Per-provider budget |
| `budget list` | Show budgets |
| `set refresh-interval <duration>` | TUI auto-refresh (default `15m`; `0` disables) |
| `set list` | Show TUI settings |
| `summary [--period day\|week\|month]` | Aggregated spend from cache |
| `capabilities --json` | Agent command catalog |
| `env schema --json` | Environment variable schema |

## TUI

- **Overview** — cost chart with budget red line, global gauge
- **By Provider** — spend breakdown
- **By Model** — top models by cost
- **Budgets** — global + per-provider gauges (green / yellow / red)

## Agent automation

```bash
model-use capabilities --json
model-use env schema --json
model-use fetch --json
model-use summary --period month --json
```

## API lookback limits

- OpenRouter analytics: up to ~365 days
- Anthropic cost report: paginate; ~13 months typical
- OpenAI costs: ~180 days max
- Cursor filtered usage events: paginated; poll at most once per hour per Cursor docs

## Development

```bash
make build
make test
# or: cargo build && cargo test
```
