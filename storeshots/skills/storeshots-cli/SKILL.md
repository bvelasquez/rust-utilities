---
name: storeshots-cli
description: >-
  Agent-first storeshots CLI for marketing assets — brand boards, App Store
  screenshots, print collateral, and paid ads (Google PMax, Display, Meta, Play).
  Use when initializing storeshots in an app repo, extracting BRAND.md, suggesting
  copy, rendering mobile screenshots, print materials, or ads from storeshots/raw/
  captures, running the storeshots pipeline, or automating marketing asset generation.
---

# storeshots CLI (agent skill)

## When to use this skill

Invoke **storeshots** when the user or task involves:

- Generating App Store / marketing screenshots from raw simulator captures
- Extracting or validating `docs/BRAND.md` from project source
- LLM-suggested slide copy, print copy, or ad layouts
- Print collateral (tri-fold, flyers, business cards)
- Paid ad creatives (Google PMax, Display, Meta, Play feature graphic)
- Running a declarative marketing pipeline in an app repo

Do **not** confuse the **CLI tool repo** (`utilities/storeshots/`) with **per-project config** (`storeshots.toml` at each app root).

---

## Install (human one-time)

```bash
cd /path/to/utilities/storeshots
cargo install --path . --force
storeshots --version   # expect 0.4.x
```

Without install: `./target/release/storeshots --help`

Humans can use menu mode: `storeshots interactive` (or bare `storeshots` in a TTY).

---

## Critical: where files live

| Location | What |
|----------|------|
| `utilities/storeshots/` | Rust CLI source only |
| `<app-repo>/storeshots.toml` | Project manifest |
| `<app-repo>/storeshots/secrets.toml` | API keys (gitignored) |
| `<app-repo>/storeshots/raw/` | Simulator PNGs (no bezel) |
| `<app-repo>/storeshots/prompts/*.append.md` | Per-phase LLM instructions |
| `<app-repo>/storeshots/out/` | Generated assets |
| `<app-repo>/docs/BRAND.md` | Brand guide (LLM-generated or hand-edited) |

**Always `cd` to the app repo root** (where `package.json`, `Cargo.toml`, or `storeshots.toml` lives) before running commands.

---

## Agent discovery (run first every session)

```bash
storeshots capabilities --json
storeshots config schema --json
storeshots config keys --json
storeshots env schema --json
```

**Agents:** use explicit subcommands with `--json`. Mutating commands need `--yes` (or `--dry-run` where supported) in non-interactive mode.

---

## Bootstrap a new project

```bash
cd ~/projects/my-app
storeshots init --name "My App"
# User adds PNG captures → storeshots/raw/ (01-home.png, 02-feature.png, …)
cp storeshots/secrets.toml.example storeshots/secrets.toml
# Edit secrets: openrouter + gemini keys
```

Optional brand font: `storeshots/brand/font.ttf` or `brand.font` in TOML.

---

## End-to-end workflows

### App Store screenshots (mobile)

```bash
storeshots brand extract --yes
storeshots brand validate
storeshots copy suggest --yes
storeshots mobile render --all-sizes --yes
storeshots mobile validate
```

Or pipeline:

```bash
storeshots run --only brand,copy,mobile --yes
```

Output: `storeshots/out/mobile/apple/iphone/{6.9",…}/`

### Print collateral

```bash
storeshots print suggest --yes          # optional: fills [print.copy]
storeshots print formats                # trifold, single-landscape, …
storeshots print render --format trifold --yes
```

### Paid marketing ads

```bash
storeshots ads formats                  # list sizes & groups
storeshots ads suggest --yes            # LLM → [[ads.items]] in TOML
storeshots ads render --yes
storeshots ads validate
```

Filter render:

```bash
storeshots ads render --only hero-benefit --formats google-pmax --yes
```

Output: `storeshots/out/ads/{google-ads|meta|google-play}/{format-id}/`

### Full marketing pass

Enable in `storeshots.toml`:

```toml
[[pipeline.steps]]
id = "ads"
phase = "ads"
depends_on = ["copy"]
enabled = true
```

```bash
storeshots run --only brand,copy,mobile,print,ads --yes
```

---

## Command map

| Phase | Suggest (LLM) | Render | Validate |
|-------|---------------|--------|----------|
| Brand | `brand extract` | — | `brand validate` |
| Copy | `copy suggest` | — | — |
| Mobile | — | `mobile render` | `mobile validate` |
| Print | `print suggest` | `print render --format <id>` | — |
| Ads | `ads suggest` | `ads render` | `ads validate` |

| Utility | Command |
|---------|---------|
| Init | `storeshots init --name "…"` |
| Pipeline | `storeshots run --only … --yes` |
| Discovery | `capabilities`, `config schema`, `config keys`, `env schema` |

Legacy aliases: `render` → `mobile render`, `validate` → `mobile validate`.

---

## API keys (per project)

**Resolution order:** `storeshots/secrets.toml` → `[ai.keys].*_env` → `STORESHOTS_*` → legacy `OPENROUTER_API_KEY` / `GEMINI_API_KEY`.

```toml
# storeshots/secrets.toml (gitignored)
openrouter = "sk-or-v1-..."
gemini = "..."
```

| Task | Provider |
|------|----------|
| Brand extract, copy/print/ads suggest | OpenRouter (default) or Gemini (`text_provider`) |
| Mobile + ad AI backgrounds | Gemini (`image_model`, default `gemini-2.5-flash-image`) |

Render without AI backgrounds: `--no-ai` on `mobile render` / `ads render`.

---

## Custom LLM instructions

**Precedence:** CLI `--prompt-append` / `--prompt-file` → per-item TOML → `[ai.prompts.{phase}]` → auto `storeshots/prompts/{phase}.append.md`.

| Phase | Append file |
|-------|-------------|
| brand | `brand.append.md` |
| copy | `copy.append.md` |
| mobile backgrounds | `mobile-background.append.md` |
| print | `print.append.md` |
| ads | `ads.append.md` |

```bash
storeshots copy suggest --prompt-append "Pain-first. No gamification." --yes
storeshots ads suggest --prompt-file ./campaign-brief.md --yes
```

---

## Config essentials (`storeshots.toml`)

### Slides (App Store)

```toml
[[slides.items]]
id = "hero"
raw = "01-home.png"        # must exist in storeshots/raw/
title = "Main benefit\nhere"
subtitle = "One outcome line"
label = "APP NAME"
layout = "hero-center"
```

### Ads

```toml
[ads]
output_dir = "storeshots/out/ads"

[[ads.items]]
id = "hero-benefit"
raw = "01-home.png"
headline = "Log meals\nin seconds"
subtitle = "No guilt."
cta = "Try free"
layout = "auto"              # or device-bottom, device-right, screenshot-hero, text-banner
format_groups = ["google-pmax", "social"]
```

**Ad format groups:** `google-pmax`, `google-display`, `social`, `play-feature`, `all`

### Print

```toml
[print.copy]
website = "https://example.com"
headline = "…"
bullets = ["…"]
logo = "storeshots/assets/logo.png"
```

---

## Copy rules (built into LLM prompts)

- Screenshots and ads are **advertisements**, not feature lists
- **One idea per headline** — never join with "and"
- **3–5 words per line**; use `\n` for intentional breaks
- Pain-first / outcome-first messaging

Agents should run `copy suggest` / `ads suggest` then **review** TOML before render.

---

## Raw captures checklist

Before `mobile render` or `ads render`:

- [ ] PNGs in `storeshots/raw/` match `slides.items[].raw` and `ads.items[].raw`
- [ ] Captures are **without device bezel** (CLI composites mockup)
- [ ] RGB preferred (RGBA can cause export issues on App Store assets)

List available raws: inspect `storeshots/raw/` or run `ads suggest --dry-run --yes` to see LLM context.

---

## iPhone export sizes

| Label | Dimensions |
|-------|------------|
| 6.9" | 1320 × 2868 |
| 6.5" | 1284 × 2778 |
| 6.3" | 1206 × 2622 |
| 6.1" | 1125 × 2436 |

Default render uses 6.9" only; `--all-sizes` exports all four.

---

## Ad sizes (reference)

| Group | Key sizes |
|-------|-----------|
| `google-pmax` | 1200×628, 1200×1200, 960×1200 |
| `google-display` | 300×250, 728×90, 970×250, 320×50, … |
| `social` | 1080×1080, 1080×1920, 1200×628 |
| `play-feature` | 1024×500 |

Full list: `storeshots ads formats --json`

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `missing raw screenshot` | Add PNG to `storeshots/raw/` or fix filename in TOML |
| `use --yes to proceed` | Add `--yes` (agents) or use `--dry-run` to preview |
| AI backgrounds skipped | Set `gemini` in secrets; or use `--no-ai` |
| `no ads configured` | Run `storeshots ads suggest --yes` first |
| Wrong project picked | `storeshots init` / `--app /path/to/repo` / interactive "Change project folder" |
| Brand extract thin | Add `storeshots/prompts/brand.append.md`; ensure `paths.web_root` points at site source |

---

## Security

- Never commit `storeshots/secrets.toml` or API keys in TOML
- `init` appends secrets path to `.gitignore` when possible
- Use per-project env var **names** in `[ai.keys]` for CI (not literal keys)

---

## Related skills

- **app-store-screenshots** — Next.js/html-to-image approach when storeshots isn't set up
- **google-ads-campaign-builder** — upload PMax assets created by `storeshots ads render`
- **gads-cli** — manage Google Ads campaigns after creatives exist
