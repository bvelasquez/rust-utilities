# storeshots

Generate App Store marketing screenshots from raw iOS simulator captures — headlines, AI-generated backgrounds, and a photoreal iPhone frame — without a browser or manual design work.

Raw PNGs stay pixel-perfect under the bezel. Copy and backgrounds are driven by a `storeshots.toml` config in your app repo, so renders are repeatable and automatable.

## What it does

- Scaffolds `storeshots.toml`, `storeshots/raw/`, and output folders in an app project
- Composites each slide: caption (label, title, subtitle) + device mockup + screenshot
- Generates AI backgrounds via Gemini (gradient fallback when no API key)
- Suggests marketing copy with `copy suggest`
- Exports all required iPhone display sizes for App Store Connect
- Validates output PNG dimensions

**v0.1:** iPhone only. iPad, Android, and Play Store feature graphics are planned.

## Install

```bash
cargo build --release
# binary: target/release/storeshots
```

## Workflow

**1. Initialize in your app repo:**

```bash
cd ~/projects/my-app
storeshots init --name "My App"
```

**2. Add raw simulator screenshots** (no bezel) to `storeshots/raw/`.

**3. Edit `storeshots.toml`** — map slides to raw files, set brand colors, write or generate copy:

```bash
storeshots copy suggest --yes --features "workout tracking, muscle filters, analytics"
```

**4. Render:**

```bash
export GEMINI_API_KEY=...   # optional; uses gradients if missing
storeshots render --all-sizes --yes
```

**5. Validate:**

```bash
storeshots validate
```

Outputs go to `storeshots/out/apple/iphone/<size>/`.

## Example `storeshots.toml`

```toml
[app]
name = "Simple Workout"

[brand]
accent = "#3b82f6"
background = "#0b0d12"
foreground = "#ffffff"
muted = "#9ca3af"
theme = "dark"

[stores]
apple_iphone = true

[ai]
backgrounds = true
image_model = "gemini-2.5-flash-image"
text_model = "gemini-2.5-flash"

[[slides.items]]
id = "hero"
label = "TRAINING"
title = "Your workout,\ntracked"
subtitle = "Log sets, reps, and progress in seconds"
raw = "01-hero.png"
```

## Commands

| Command | Description |
|---------|-------------|
| `init` | Create config and folder scaffold |
| `copy suggest` | Generate slide copy via Gemini; use `--yes` to apply |
| `render` | Composite all slides; `--all-sizes` for every iPhone size |
| `validate` | Check output PNG dimensions |
| `env schema` | List environment variables for automation |

## iPhone export sizes

| Label | Dimensions |
|-------|------------|
| 6.9" | 1320 × 2868 |
| 6.5" | 1284 × 2778 |
| 6.3" | 1206 × 2622 |
| 6.1" | 1125 × 2436 |

Renders at 6.9" design resolution, then scales down for smaller sizes.

## Environment variables

| Variable | Purpose |
|----------|---------|
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | AI backgrounds and copy suggestions |
| `STORESHOTS_MODEL_IMAGE` | Image model override |
| `STORESHOTS_MODEL_TEXT` | Text model override |

```bash
storeshots env schema
storeshots env schema --json
```

## Flags for automation

- `--yes` — non-interactive (required for `render` and `copy suggest`)
- `--json` — structured output envelope on supported commands
- `--no-ai` — skip Gemini backgrounds, use brand gradient
- `--only 1,3,6` — render specific slides (1-based indices)

## Project layout (in your app repo)

```
my-app/
├── storeshots.toml
└── storeshots/
    ├── raw/           # simulator PNGs (gitignored or committed — your choice)
    ├── brand/         # optional custom font
    └── out/           # generated marketing screenshots
        └── apple/iphone/6.9"/...
```

## Tips

- Capture screenshots on the largest simulator (iPhone 16 Pro Max / 17 Pro Max) for best downscale quality
- Keep the top-left of backgrounds dark — storeshots adds an adaptive scrim for subtitle readability on bright AI backgrounds
- Re-run `render --all-sizes --yes` after changing copy, raw captures, or brand colors
