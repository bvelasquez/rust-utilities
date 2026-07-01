---
name: gads-cli
description: >-
  Agent-first Google Ads CLI (gads) — GAQL reads, REST mutates, conversion-tag
  shortcuts, interactive TUI. Use when managing Google Ads campaigns, running
  GAQL queries, pausing or creating campaigns/ad groups/ads/keywords, checking
  conversion tracking for a domain, or automating Google Ads via shell from
  Cursor, Claude Code, Codex, or CI.
---

# gads — Google Ads CLI (agent skill)

## When to use this skill

Invoke **gads** when the user or task involves:

- Listing or inspecting Google Ads accounts, campaigns, ad groups, ads, keywords
- Performance reporting (campaign/ad/keyword stats, account summary)
- Conversion action / tag audit for a website domain
- Pausing, enabling, or creating campaigns, ad groups, RSA ads, keywords, budgets
- Raw GAQL reporting not covered by built-in commands
- Any Google Ads REST mutate via JSON operation files

Do **not** use for Meta/Microsoft/Amazon ads — this is Google Ads only.

---

## Install (human one-time)

```bash
cd /path/to/utilities/gads
cargo install --path .
gads auth login \
  --developer-token "$GOOGLE_ADS_DEVELOPER_TOKEN" \
  --client-id "$GOOGLE_ADS_CLIENT_ID" \
  --client-secret "$GOOGLE_ADS_CLIENT_SECRET"
```

Credentials: `~/.config/gads/credentials.json` (also reads legacy `~/.config/google-ads-open-cli/credentials.json`).

---

## Agent discovery (run first every session)

```bash
gads capabilities --json
gads env schema --json
```

Plain `gads --help` lists top-level commands. Nested groups: `gads campaign --help`, `gads ad --help`, `gads ad-group --help`, `gads budget --help`, `gads keyword --help`.

**Always prefer `--json`** for parseable output.

---

## Output contract

With `--json`, responses use this envelope:

```json
{
  "success": true,
  "command": "campaigns",
  "data": {},
  "warnings": [],
  "errors": [],
  "next_actions": [],
  "timestamp": "..."
}
```

Use `--compact` for raw Google API JSON (no envelope).

---

## Configuration precedence

1. CLI flags (`--customer`, `--credentials`)
2. Environment variables (`GOOGLE_ADS_*` or `GADS_*`)
3. `~/.config/gads/credentials.json`
4. Project `gads.toml` (walk up from cwd): `default_customer_id`, `[aliases]`

```toml
# gads.toml
default_customer_id = "1234567890"
[aliases]
workout = "1234567890"
```

---

## Auth

| Item | Location |
|------|----------|
| Login | `gads auth login --developer-token ... --client-id ... --client-secret ...` |
| Status | `gads auth status --json` |
| MCC | `login_customer_id` in credentials or `gads.toml` (10 digits, no dashes) |

Env-only (CI): `GOOGLE_ADS_DEVELOPER_TOKEN` + `GOOGLE_ADS_ACCESS_TOKEN` (+ optional `GOOGLE_ADS_LOGIN_CUSTOMER_ID`).

---

## Read commands (quick reference)

| Task | Command |
|------|---------|
| List accounts | `gads customers --json` |
| Account info | `gads customer CUSTOMER_ID --json` |
| MCC tree | `gads account-hierarchy CUSTOMER_ID --json` |
| Campaigns | `gads campaigns CUSTOMER_ID [--status ENABLED] --json` |
| One campaign | `gads campaign get CUSTOMER_ID CAMPAIGN_ID --json` |
| Budgets | `gads campaign-budgets CUSTOMER_ID --json` |
| Ad groups | `gads ad-groups CUSTOMER_ID [--campaign ID] --json` |
| Ads | `gads ads CUSTOMER_ID [--ad-group ID] --json` |
| Keywords | `gads keywords CUSTOMER_ID --json` |
| Campaign stats | `gads campaign-stats CUSTOMER_ID --start YYYY-MM-DD --end YYYY-MM-DD --json` |
| Raw GAQL | `gads query CUSTOMER_ID "SELECT ..." --json` |
| 30-day summary | `gads summary CUSTOMER_ID --days 30 --json` |
| Conversion tags | `gads conversion-tags CUSTOMER_ID --domain example.com --json` |

Customer ID: 10 digits; dashes stripped. Optional if `--customer` or `gads.toml` default set.

**API version:** v24. Campaign date fields use `start_date_time` / `end_date_time`.

**Money:** amounts in **micros** (1 USD = 1_000_000).

---

## Mutations (critical safety rules)

Every write requires **one of**:

- `--dry-run` — show JSON payload, no API call
- `--yes` — non-interactive approval (agents/CI)
- TTY confirmation prompt

**Agent workflow:** always `--dry-run` first, then `--yes` to apply.

| Task | Command |
|------|---------|
| Pause campaign | `gads campaign set-status CID CAMPAIGN_ID PAUSED --dry-run\|--yes --json` |
| Enable campaign | `... ENABLED ...` |
| Create budget | `gads budget create CID --name "..." --amount-micros 5000000 --yes --json` |
| Create Search campaign | `gads campaign create-search CID --name "..." --budget customers/CID/campaignBudgets/BID --yes --json` |
| Create ad group | `gads ad-group create CID --campaign customers/CID/campaigns/ID --name "..." --yes --json` |
| Create RSA | `gads ad create-rsa CID --ad-group customers/CID/adGroups/AG --url https://... --headlines "a\|b\|c" --descriptions "x\|y" --yes --json` |
| Pause ad | `gads ad set-status CID AD_GROUP_ID AD_ID PAUSED --yes --json` |
| Add keyword | `gads keyword add CID --ad-group customers/CID/adGroups/AG --text "phrase" --match-type PHRASE --yes --json` |
| Generic mutate | `gads mutate CID campaigns --file ops.json --yes --json` |
| Atomic multi-create | `gads mutate-batch CID --file batch.json --yes --json` |

JSON templates: repo `examples/mutate-campaign-pause.json`, `examples/mutate-search-campaign-batch.json`.

**Resource names** for mutate files: `customers/{customer_id}/campaigns/{id}`, etc. Batch creates use negative temp IDs (`-1`, `-2`) per [Google grouped mutate docs](https://developers.google.com/google-ads/api/docs/mutating/best-practices).

---

## Recommended agent workflow

```
1. gads capabilities --json
2. gads customers --json                    → pick CUSTOMER_ID
3. gads campaigns CUSTOMER_ID --json        → inspect
4. gads summary CUSTOMER_ID --days 30 --json → performance context
5. gads query CUSTOMER_ID "..." --json       → ad-hoc if needed
6. mutation --dry-run --json                 → verify payload
7. mutation --yes --json                     → apply
```

For new Search funnel: prefer `mutate-batch` with `examples/mutate-search-campaign-batch.json` (replace `CUSTOMER_ID` placeholders).

---

## Humans

```bash
gads interactive   # alias: gads i
```

Read-focused menu (accounts, campaigns, stats, GAQL). Mutations via CLI commands above.

---

## Errors

- stderr: JSON `{"error": "message"}` or anyhow text
- Non-zero exit code on failure
- Common: missing credentials, MCC `login_customer_id`, GAQL field drift after API version bump

---

## Full documentation

See repo `README.md` in the `gads` crate root for complete command reference, troubleshooting, and migration from google-ads-open-cli.
