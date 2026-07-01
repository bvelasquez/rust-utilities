# storeshots

Generate marketing assets from project source, brand boards, and raw captures — brand guides, App Store screenshots, and (soon) print materials.

Pure Rust compositing for mobile slides. OpenRouter or Gemini for copy and brand extraction. Gemini for AI slide backgrounds.

## Install

```bash
cd utilities/storeshots
cargo build --release

# Put `storeshots` on your PATH (~/.cargo/bin):
cargo install --path . --force
```

Verify: `storeshots --version` → should print `0.3.0`.

Without install, run directly: `./target/release/storeshots --help`

## Interactive mode (humans)

From an app repo (or any directory with `storeshots.toml`):

```bash
storeshots interactive
# alias:
storeshots i

# or just run with no subcommand in a terminal:
storeshots
```

The session stays open until you choose **Quit (q)**. Each action shows progress (`→` starting, `✓` done, `✗` on error) and returns to the main menu. Use **Change project folder** to point at a different app repo.

Agents should keep using explicit subcommands with `--json`.

**Where config files live:** `storeshots.toml` and `storeshots/secrets.toml` belong in **each app repo** (e.g. `soki-creative/`, `simple-food-track/`), not inside `utilities/storeshots/` (that directory is only the CLI tool). Run `storeshots init` from your app root to scaffold them.

A reference template ships with the CLI: [`templates/secrets.toml.example`](templates/secrets.toml.example).

## Quick start

```bash
cd ~/projects/my-app
storeshots init --name "My App"
# Add simulator PNGs to storeshots/raw/

storeshots brand extract --yes          # docs/BRAND.md from source
storeshots brand validate
storeshots copy suggest --yes
storeshots mobile render --all-sizes --yes
storeshots mobile validate

# Or run the default pipeline (brand → copy → mobile):
storeshots run --only brand,copy,mobile --yes
```

## Custom LLM instructions (per phase)

Append instructions without forking the CLI:

1. **Auto-discovered files** in `storeshots/prompts/`:
   - `brand.append.md`
   - `copy.append.md`
   - `mobile-background.append.md`
   - `print.append.md`

2. **Inline in `storeshots.toml`**:

```toml
[ai.prompts.copy]
prompt_append = """
Pain-first messaging. Never mention streaks or gamification.
"""

[ai.prompts.brand]
prompt_file = "storeshots/prompts/brand.append.md"
```

3. **One-shot CLI flags**:

```bash
storeshots copy suggest --prompt-append "Keep headlines under 5 words per line." --yes
storeshots brand extract --prompt-file ./brief.md --yes
```

**Precedence:** CLI flags → per-slide `prompt_append` → `[ai.prompts.{phase}]` → auto-discovered `.append.md` files.

## Text vs image models

Keys resolve **per project** (for cost tracking):

| Priority | Source |
|----------|--------|
| 1 | `storeshots/secrets.toml` — gitignored, project-local |
| 2 | Env var **names** in `[ai.keys]` (`openrouter_env`, `gemini_env`) |
| 3 | `STORESHOTS_OPENROUTER_API_KEY` / `STORESHOTS_GEMINI_API_KEY` (global CLI keys) |
| 4 | Legacy `OPENROUTER_API_KEY` / `GEMINI_API_KEY` |

### Per-project secrets file (recommended)

```bash
cp storeshots/secrets.toml.example storeshots/secrets.toml
# Edit storeshots/secrets.toml — never commit
```

```toml
# storeshots/secrets.toml
openrouter = "sk-or-v1-..."
gemini = "..."
```

### Per-project env var names (CI / shell) — Option B

**File:** `storeshots.toml` at the **root of your app repo** (same folder as `package.json` / `Cargo.toml`), not inside `utilities/storeshots`.

```toml
[ai.keys]
secrets_file = "storeshots/secrets.toml"
openrouter_env = "SOKI_CREATIVE_OPENROUTER_API_KEY"
gemini_env = "SOKI_CREATIVE_GEMINI_API_KEY"
```

Then set those env vars in `.env.local` or CI (values never go in committed TOML):

```bash
export SOKI_CREATIVE_OPENROUTER_API_KEY=sk-or-v1-...
export SOKI_CREATIVE_GEMINI_API_KEY=...
```

### Global storeshots CLI keys (optional fallback)

```bash
export STORESHOTS_OPENROUTER_API_KEY=sk-or-v1-...
export STORESHOTS_GEMINI_API_KEY=...
```

```bash
storeshots config keys --json   # full resolution order
storeshots env schema --json
```

| Task | Provider | Key source |
|------|----------|------------|
| Brand extract, copy suggest | OpenRouter (default) | Project secrets / env |
| Slide backgrounds | Gemini | Project secrets / env |

## Commands

| Command | Description |
|---------|-------------|
| `init` | Scaffold `storeshots.toml`, `storeshots/prompts/`, `docs/BRAND.md` |
| `brand extract` | LLM → `docs/BRAND.md` from web source/CSS/copy |
| `brand validate` | Check brand guide completeness |
| `copy suggest` | Generate slide copy → `storeshots.toml` |
| `mobile render` | Composite App Store screenshots |
| `mobile validate` | Check output PNG dimensions |
| `print render` | Tri-fold, single-page, business card → PNG + PDF |
| `run` | Execute `[[pipeline.steps]]` from manifest |
| `capabilities --json` | Machine discovery for agents |
| `config schema --json` | TOML contract for agents |
| `config keys --json` | API key resolution order and secrets file format |
| `env schema --json` | Environment variables |

Legacy aliases: `storeshots render` → `mobile render`, `storeshots validate` → `mobile validate`.

## Project layout

```
my-app/
├── storeshots.toml
├── docs/BRAND.md
└── storeshots/
    ├── prompts/       # optional .append.md per phase
    ├── raw/           # simulator PNGs (no bezel)
    ├── assets/        # logo, web captures
    └── out/
        ├── mobile/apple/iphone/6.9"/...
        └── print/           # brochures, cards (PNG + PDF)
```

## Print materials (`storeshots print render`)

Pure Rust compositor at 300 DPI layout, exported at 2× (600 DPI PNG embedded in PDF). Copy is pulled from `docs/BRAND.md` with optional overrides in `[print.copy]`.

```bash
storeshots print render --format trifold --yes
storeshots print render --format single-landscape --yes
storeshots print render --format single-portrait --yes
storeshots print render --format business-card --yes
# business-card sides: --variant front|back|both (default both)
```

Formats: `trifold`, `single-landscape`, `single-portrait`, `business-card`.

Optional in `storeshots.toml`:

```toml
[print]
output_dir = "storeshots/out/print"
dpi = 300
export_scale = 2

[print.copy]
website = "https://soki-creative.com"
qr_url = "https://soki-creative.com"
contact_email = "sales@soki-creative.com"
eyebrow = "B2B software studio"
logo = "storeshots/assets/logo.png"   # optional PNG
bullets = ["Custom bullet override"]
```

## Pipeline (`storeshots run`)

Define steps in `storeshots.toml`:

```toml
[[pipeline.steps]]
id = "brand"
phase = "brand"
enabled = true

[[pipeline.steps]]
id = "copy"
phase = "copy"
depends_on = ["brand"]

[[pipeline.steps]]
id = "mobile"
phase = "mobile"
depends_on = ["copy"]
```

```bash
storeshots run --only brand,copy,mobile --yes
```

## Agent automation

```bash
storeshots capabilities --json
storeshots config schema --json
storeshots env schema --json
storeshots brand extract --yes --json
```

All mutating commands require `--yes` (or `--dry-run` where supported) unless `--json` is passed in non-interactive contexts.

## iPhone export sizes

| Label | Dimensions |
|-------|------------|
| 6.9" | 1320 × 2868 |
| 6.5" | 1284 × 2778 |
| 6.3" | 1206 × 2622 |
| 6.1" | 1125 × 2436 |

## Roadmap

- **v0.2:** brand extract, prompt appends, pipeline, OpenRouter, mobile alias, interactive mode
- **v0.3:** Pure Rust print compositor (tri-fold, single-page, business cards)
- **v0.4:** Android sizes, iPad, Play feature graphic
