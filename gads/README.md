# gads

**gads** is an agent-first Google Ads CLI written in Rust. It wraps the official [Google Ads REST API](https://developers.google.com/google-ads/api/rest/overview) (currently **v24**) with:

- GAQL read commands (parity with [google-ads-open-cli](https://github.com/Bin-Huang/google-ads-open-cli), with v24 field fixes)
- REST mutate commands for campaigns, ad groups, ads, keywords, and budgets
- Generic `mutate` / `mutate-batch` for the full API surface
- Shortcuts (`summary`, `conversion-tags`)
- Interactive TUI for humans (`gads interactive`)
- Machine discovery (`capabilities --json`, `env schema --json`)

Works with Cursor, Claude Code, Codex, OpenClaw, shell scripts, and CI.

---

## Table of contents

- [Requirements](#requirements)
- [Install](#install)
- [Authentication](#authentication)
- [Configuration](#configuration)
- [Output modes](#output-modes)
- [Global flags](#global-flags)
- [Entity hierarchy](#entity-hierarchy)
- [Commands reference](#commands-reference)
  - [Discovery & auth](#discovery--auth)
  - [Accounts](#accounts)
  - [Campaigns & budgets](#campaigns--budgets)
  - [Ad groups, ads & keywords](#ad-groups-ads--keywords)
  - [Performance stats](#performance-stats)
  - [Targeting & assets](#targeting--assets)
  - [Shortcuts](#shortcuts)
  - [Mutations](#mutations)
  - [Interactive TUI](#interactive-tui)
- [Agent workflows](#agent-workflows)
- [Examples](#examples)
- [Migrating from google-ads-open-cli](#migrating-from-google-ads-open-cli)
- [Troubleshooting](#troubleshooting)
- [LLM skills](#llm-skills)
- [Development](#development)

---

## Requirements

- [Rust](https://rustup.rs/) 2021 edition
- Google Ads API access:
  - **Developer token** ([API Center](https://ads.google.com/aw/apicenter))
  - **OAuth client ID & secret** (Google Cloud Console → Credentials → Desktop app)
  - Google account with access to the Ads accounts you manage
- Optional: **login customer ID** when accessing child accounts via an MCC manager account

> Google Ads API does **not** support service accounts for user data. Use OAuth user credentials.

---

## Install

From this directory:

```bash
cargo build --release
cargo install --path .
```

Binary name: **`gads`**

Verify:

```bash
gads --version
gads capabilities --json
```

---

## Authentication

### First-time setup

```bash
gads auth login \
  --developer-token "$GOOGLE_ADS_DEVELOPER_TOKEN" \
  --client-id "$GOOGLE_ADS_CLIENT_ID" \
  --client-secret "$GOOGLE_ADS_CLIENT_SECRET"
```

This opens a browser for Google OAuth (scope: `https://www.googleapis.com/auth/adwords`) and saves tokens to:

```
~/.config/gads/credentials.json
```

Check status (no secrets printed):

```bash
gads auth status --json
```

### Credential file format

```json
{
  "developer_token": "...",
  "client_id": "...",
  "client_secret": "...",
  "access_token": "...",
  "refresh_token": "...",
  "token_expiry": "2026-07-01T15:59:20.814Z",
  "login_customer_id": "1234567890"
}
```

`login_customer_id` is optional — add it (10 digits, no dashes) for MCC/manager access.

### Legacy path

If `~/.config/gads/credentials.json` is missing, **gads** automatically reads:

```
~/.config/google-ads-open-cli/credentials.json
```

### Environment variables (CI / automation)

| Variable | Aliases | Purpose |
|----------|---------|---------|
| `GOOGLE_ADS_DEVELOPER_TOKEN` | `GADS_DEVELOPER_TOKEN` | Required |
| `GOOGLE_ADS_ACCESS_TOKEN` | `GADS_ACCESS_TOKEN` | Bearer token (skip file if set with dev token) |
| `GOOGLE_ADS_LOGIN_CUSTOMER_ID` | `GADS_LOGIN_CUSTOMER_ID` | MCC manager ID |

Override credentials file:

```bash
gads --credentials /path/to/credentials.json customers --json
```

---

## Configuration

### Project file: `gads.toml`

Walks **up** from the current working directory to find `gads.toml`:

```toml
default_customer_id = "1234567890"
login_customer_id = "9876543210"   # optional; can also live in credentials.json

[aliases]
workout = "1234567890"
dosier = "1112223333"
```

Use aliases anywhere a customer ID is accepted:

```bash
gads campaigns workout --json
gads --customer workout summary --days 7 --json
```

See `examples/gads.toml`.

### Precedence

1. CLI flags (`--customer`, `--credentials`)
2. Environment variables
3. `~/.config/gads/credentials.json` (or legacy open-cli path)
4. `gads.toml` (`default_customer_id`, `aliases`)

---

## Output modes

| Flag | Use case |
|------|----------|
| `--json` | **Agents** — structured envelope (see below) |
| `--compact` | Scripts — raw API JSON, one line |
| *(default)* | Humans — pretty-printed JSON |

### JSON envelope (`--json`)

```json
{
  "success": true,
  "command": "campaigns",
  "data": { },
  "warnings": [],
  "errors": [],
  "next_actions": [],
  "timestamp": "2026-07-01T12:00:00+00:00"
}
```

Errors go to stderr; exit code is non-zero on failure.

---

## Global flags

Available on every command:

| Flag | Description |
|------|-------------|
| `--json` | Envelope output |
| `--compact` | Raw compact JSON |
| `--credentials <path>` | Credentials JSON file |
| `--customer <id>` | Default customer ID or alias |

---

## Entity hierarchy

```
Manager Account (MCC)
 └── Customer Account (1234567890)
      ├── Campaign
      │    └── Ad Group
      │         ├── Ad (Ad Group Ad)
      │         └── Keyword (Ad Group Criterion)
      ├── Campaign Budget
      ├── Conversion Action
      ├── User List (remarketing)
      └── Asset (images, sitelinks, etc.)
```

- Customer IDs are **10 digits**; dashes are stripped automatically.
- Monetary values use **micros** (1 USD = `1_000_000` micros).

---

## Commands reference

### Discovery & auth

| Command | Description |
|---------|-------------|
| `gads capabilities` | Machine-readable command catalog |
| `gads capabilities --json` | Same, with envelope |
| `gads env schema` | Environment variable schema |
| `gads auth login` | OAuth browser login |
| `gads auth status` | Credential status (no secrets) |

**`auth login` options:** `--developer-token`, `--client-id`, `--client-secret`, `--port`, `--no-browser`

---

### Accounts

| Command | Description |
|---------|-------------|
| `gads customers` | List accessible customer resource names |
| `gads customer [CUSTOMER_ID]` | Account metadata |
| `gads account-hierarchy [CUSTOMER_ID]` | MCC sub-account tree |

---

### Campaigns & budgets

| Command | Description |
|---------|-------------|
| `gads campaigns [CUSTOMER_ID]` | List campaigns |
| `gads campaign get [CUSTOMER_ID] <CAMPAIGN_ID>` | Single campaign |
| `gads campaign-budgets [CUSTOMER_ID]` | List budgets |
| `gads budget create [CUSTOMER_ID] --name ... --amount-micros ...` | Create daily budget |

**`campaigns` options:** `--status ENABLED|PAUSED|REMOVED`, `--limit` (default 100)

**Campaign mutations** (subcommands of `gads campaign`):

| Subcommand | Description |
|------------|-------------|
| `get [CUSTOMER_ID] <CAMPAIGN_ID>` | Read one campaign |
| `set-status [CUSTOMER_ID] <CAMPAIGN_ID> <STATUS>` | `ENABLED`, `PAUSED`, or `REMOVED` |
| `create-search [CUSTOMER_ID] --name ... --budget <resource_name> [--status PAUSED]` | New Search campaign |

```bash
# Pause a campaign (preview first)
gads campaign set-status 1234567890 987654321 PAUSED --dry-run --json
gads campaign set-status 1234567890 987654321 PAUSED --yes --json
```

---

### Ad groups, ads & keywords

| Command | Description |
|---------|-------------|
| `gads ad-groups [CUSTOMER_ID]` | List ad groups |
| `gads ads [CUSTOMER_ID]` | List ads |
| `gads keywords [CUSTOMER_ID]` | List keywords |

**`ad-groups` / `ads` / `keywords` filters:** `--campaign`, `--ad-group`, `--status`, `--limit`

**`gads ad-group` subcommands:**

| Subcommand | Description |
|------------|-------------|
| `get [CUSTOMER_ID] <AD_GROUP_ID>` | Read ad group |
| `create [CUSTOMER_ID] --campaign <resource> --name ...` | Create search ad group |
| `set-status [CUSTOMER_ID] <AD_GROUP_ID> <STATUS>` | Pause / enable / remove |

**`gads ad` subcommands:**

| Subcommand | Description |
|------------|-------------|
| `get [CUSTOMER_ID] <AD_GROUP_ID> <AD_ID>` | Read ad |
| `create-rsa [CUSTOMER_ID] --ad-group <resource> --url ... --headlines "a\|b\|c" --descriptions "x\|y"` | Responsive search ad (min 3 headlines, 2 descriptions) |
| `set-status [CUSTOMER_ID] <AD_GROUP_ID> <AD_ID> <STATUS>` | Pause / enable / remove ad |

**`gads keyword` subcommands:**

| Subcommand | Description |
|------------|-------------|
| `add [CUSTOMER_ID] --ad-group <resource> --text "..." [--match-type PHRASE] [--cpc-bid-micros N]` | Add keyword |

```bash
gads ad create-rsa 1234567890 \
  --ad-group "customers/1234567890/adGroups/111" \
  --url "https://example.com" \
  --headlines "Best App|Download Now|Free Trial" \
  --descriptions "Try it today|No credit card" \
  --dry-run --json
```

---

### Performance stats

All stats commands require `--start YYYY-MM-DD` and `--end YYYY-MM-DD`.

| Command | Description |
|---------|-------------|
| `gads campaign-stats [CUSTOMER_ID] --start ... --end ...` | Campaign metrics by day |
| `gads ad-group-stats [CUSTOMER_ID] --start ... --end ...` | Ad group metrics |
| `gads ad-stats [CUSTOMER_ID] --start ... --end ...` | Ad metrics |
| `gads keyword-stats [CUSTOMER_ID] --start ... --end ...` | Keyword metrics (sorted by impressions) |

**Common filters:** `--campaign`, `--ad-group`, `--limit`

**`campaign-stats` only:** `--segments device,ad_network_type` (comma-separated)

Default metrics include impressions, clicks, cost_micros, conversions, ctr, average_cpc, etc.

---

### Targeting & assets

| Command | Description |
|---------|-------------|
| `gads audiences [CUSTOMER_ID]` | Campaign audience performance |
| `gads user-lists [CUSTOMER_ID]` | Remarketing lists |
| `gads negative-keywords [CUSTOMER_ID]` | Shared negative keyword lists |
| `gads assets [CUSTOMER_ID] [--type SITELINK]` | Account assets |
| `gads extensions [CUSTOMER_ID] [--campaign ID]` | Campaign-level extensions |
| `gads conversion-actions [CUSTOMER_ID]` | Conversion actions |
| `gads billing [CUSTOMER_ID]` | Billing setup |
| `gads change-status [CUSTOMER_ID] [--limit 50]` | Recent change history |

---

### Raw GAQL

```bash
gads query [CUSTOMER_ID] "SELECT campaign.id, campaign.name FROM campaign LIMIT 10" --json
```

Escape quotes in shell as needed. See [GAQL reference](https://developers.google.com/google-ads/api/docs/query/overview).

---

### Shortcuts

| Command | Description |
|---------|-------------|
| `gads summary [CUSTOMER_ID] [--days 30]` | Account-level performance rollup |
| `gads summary [CUSTOMER_ID] --start 2026-01-01 --end 2026-01-31` | Custom date range |
| `gads conversion-tags [CUSTOMER_ID] [--domain example.com]` | Conversion actions + tag snippets, optional domain filter |

---

### Mutations

All write commands require one of:

- `--dry-run` — print payload only
- `--yes` — non-interactive confirm (for agents/CI)
- Interactive **y/n** prompt (TTY only)

#### High-level helpers

Documented above under campaign / ad-group / ad / keyword / budget.

#### Generic single-resource mutate

```bash
gads mutate [CUSTOMER_ID] <RESOURCE> --file ops.json [--partial-failure] [--dry-run|--yes] --json
```

`<RESOURCE>` examples: `campaigns`, `adGroups`, `adGroupAds`, `adGroupCriteria`, `campaignBudgets`, `assets`, `conversionActions`, …

`ops.json` shape:

```json
{
  "operations": [
    {
      "updateMask": "status",
      "update": {
        "resourceName": "customers/1234567890/campaigns/987654321",
        "status": "PAUSED"
      }
    }
  ]
}
```

Template: `examples/mutate-campaign-pause.json`

#### Multi-resource batch (`GoogleAdsService.mutate`)

Create budget + campaign + ad group atomically using temporary negative IDs (`-1`, `-2`, …):

```bash
gads mutate-batch [CUSTOMER_ID] --file batch.json [--validate-only] [--partial-failure] [--dry-run|--yes] --json
```

Template: `examples/mutate-search-campaign-batch.json`

See [mutate best practices](https://developers.google.com/google-ads/api/docs/mutating/best-practices).

---

### Interactive TUI

```bash
gads interactive
# alias:
gads i
```

Menu-driven session: accounts, campaigns, ad groups, stats, conversion tags, raw GAQL, auth status. Requires a terminal (not for piped/CI use).

---

## Agent workflows

### 1. Discover

```bash
gads capabilities --json
gads env schema --json
```

### 2. Pick account

```bash
gads customers --json
gads customer 1234567890 --json
```

### 3. Inspect

```bash
gads campaigns 1234567890 --status ENABLED --json
gads campaign get 1234567890 987654321 --json
gads summary 1234567890 --days 30 --json
```

### 4. Report (ad-hoc)

```bash
gads query 1234567890 "SELECT campaign.name, metrics.clicks FROM campaign WHERE segments.date DURING LAST_7_DAYS" --json
```

### 5. Mutate safely

```bash
# Always dry-run first
gads campaign set-status 1234567890 987654321 PAUSED --dry-run --json

# Apply in automation
gads campaign set-status 1234567890 987654321 PAUSED --yes --json
```

### 6. Full create funnel

```bash
# Option A: one atomic batch file
cp examples/mutate-search-campaign-batch.json /tmp/batch.json
# edit CUSTOMER_ID placeholders
gads mutate-batch 1234567890 --file /tmp/batch.json --dry-run --json
gads mutate-batch 1234567890 --file /tmp/batch.json --yes --json

# Option B: step by step
gads budget create 1234567890 --name "Daily $10" --amount-micros 10000000 --yes --json
# use returned budget resource name in:
gads campaign create-search 1234567890 --name "My Campaign" --budget "customers/.../campaignBudgets/..." --yes --json
```

---

## Examples

| File | Purpose |
|------|---------|
| `examples/gads.toml` | Project config sample |
| `examples/mutate-campaign-pause.json` | Pause campaign via generic mutate |
| `examples/mutate-search-campaign-batch.json` | Budget + campaign + ad group batch |

---

## Migrating from google-ads-open-cli

| Topic | open-cli | gads |
|-------|----------|------|
| Credentials | `~/.config/google-ads-open-cli/credentials.json` | `~/.config/gads/` (auto-fallback to legacy) |
| API version | v23 | **v24** |
| Get campaign | `campaign ID CID` | `campaign get ID CID` |
| Pause campaign | *(not built-in)* | `campaign set-status ID CID PAUSED` |
| Output | `--format json\|compact` | `--json` / `--compact` |
| Agent discovery | — | `capabilities --json`, `env schema --json` |

Your existing credentials file works without changes (copy optional).

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `no credentials found` | Run `gads auth login` or set env vars |
| `USER_PERMISSION_DENIED` / MCC errors | Add `login_customer_id` to credentials or `gads.toml` |
| GAQL field errors after API bump | Pin/check `API_VERSION` in `src/api.rs`; update queries in `src/gaql.rs` |
| Mutation rejected in CI | Pass `--yes` or `--dry-run`; TTY confirm does not work in pipes |
| `unexpected argument` on `campaign set-status` | Use subcommand form: `gads campaign set-status ...` not `gads campaign ID set-status` |
| Old binary behavior | `cargo install --path . --force` |

---

## LLM skills

Agent skill files for Cursor and other tools live in:

```
skills/gads-cli/
├── SKILL.md      # Skill definition (copy to ~/.cursor/skills/ or use npx skills)
└── README.md     # How to install the skill
```

Quick install (Cursor):

```bash
mkdir -p ~/.cursor/skills/gads-cli
cp skills/gads-cli/SKILL.md ~/.cursor/skills/gads-cli/
```

---

## Development

```bash
cargo build
cargo test
cargo build --release
```

API version constant: `src/api.rs` → `API_VERSION`

Smoke tests:

```bash
gads capabilities --json
gads auth status --json
gads customers --json
gads campaigns YOUR_CUSTOMER_ID --json
```

---

## License

Part of the [rust-utilities](../) monorepo — personal utilities collection.
