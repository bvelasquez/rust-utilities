# rust-utilities

A collection of small, focused Rust CLIs for day-to-day development workflows: keeping git repos tidy, backing up local secrets safely, and generating App Store marketing screenshots.

Each tool lives in its own crate, builds independently, and is designed for both interactive use and automation (`--json`, `--strict`, non-interactive flags).

## Tools

| Tool | What it does |
|------|----------------|
| [**git-sweep**](./git-sweep/) | Scan a projects folder for git repos with uncommitted or unpushed work. Optionally stage, commit, and push across many repos at once. |
| [**secret-sweep**](./secret-sweep/) | Find local-only secrets and dotfiles across your projects, warn about credentials committed to git, and back everything up into an encrypted `.svault` archive. |
| [**storeshots**](./storeshots/) | Turn raw iOS simulator captures into polished App Store screenshots with headlines, AI backgrounds, and a photoreal iPhone frame. |
| [**gads**](./gads/) | Agent-first Google Ads CLI — GAQL reads, campaign mutates, shortcuts, and interactive TUI over the REST API. |
| [**model-use**](./model-use/) | Agent-first LLM cost aggregator — OpenRouter, Anthropic, OpenAI, and Cursor spend with budgets and a ratatui dashboard. |
| [**disk-sweep**](./disk-sweep/) | Smart macOS disk cleanup — scan Xcode junk and caches in a ratatui TUI, clean with confirmation, and LLM-review unknown folders. |

## Requirements

- [Rust](https://rustup.rs/) (2021 edition)
- `git` on your `PATH` (git-sweep, secret-sweep)
- Optional: `GEMINI_API_KEY` or `GOOGLE_API_KEY` for storeshots AI backgrounds and copy suggestions
- Google Ads API credentials for **gads** (developer token + OAuth client)

## Build

Build a single tool:

```bash
cd git-sweep && cargo build --release
cd secret-sweep && cargo build --release
cd storeshots && cargo build --release
cd gads && cargo build --release
cd model-use && cargo build --release
cd disk-sweep && cargo build --release
```

Binaries land in each crate’s `target/release/` directory. Add them to your `PATH`, or install with:

```bash
cargo install --path git-sweep
cargo install --path secret-sweep
cargo install --path storeshots
cargo install --path gads
cargo install --path model-use
cargo install --path disk-sweep
# or from crates.io:
cargo install model-use
```

## Quick start

**See what needs attention across ~/projects:**

```bash
git-sweep
```

**Back up local `.env` files and keys before a machine migration:**

```bash
secret-sweep scan
secret-sweep backup --yes
```

**Generate App Store screenshots for an app:**

```bash
cd ~/projects/my-app
storeshots init --name "My App"
# add PNGs to storeshots/raw/, edit storeshots.toml
storeshots render --all-sizes --yes
```

**Manage Google Ads (agent or interactive):**

```bash
gads auth login --developer-token "$GOOGLE_ADS_DEVELOPER_TOKEN" ...
gads capabilities --json
gads campaigns 1234567890 --json
gads interactive
```

**Free disk space from Xcode caches and junk:**

```bash
disk-sweep
disk-sweep watch
disk-sweep scan --json
disk-sweep review ~/Library/Developer --json
```

Full docs: [gads/README.md](./gads/README.md). Agent skill: [gads/skills/gads-cli/](./gads/skills/gads-cli/).

## Configuration

| Tool | Default config location |
|------|-------------------------|
| git-sweep | `~/.config/git-sweep/config.toml` |
| secret-sweep | `~/.config/secret-sweep/config.toml` |
| storeshots | `<app-root>/storeshots.toml` |
| gads | `~/.config/gads/credentials.json` + optional `gads.toml` |
| model-use | `~/.config/model-use/config.toml` |
| disk-sweep | `~/.config/disk-sweep/config.toml` |

All tools also accept CLI flags and environment variables — see each tool’s README for details.

## License

See individual crates for license information if present; otherwise treat as personal utilities in this repository.
