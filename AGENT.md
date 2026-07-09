# Agent guide — utilities Rust projects

All new Rust CLIs in this repo should use the **fast compiler toolchain** from [flatland3](../flatland3): **sccache** (compile caching) + **lld** (faster linking on macOS).

## One-time setup

```bash
# From any Rust project in utilities (or flatland3):
./scripts/setup-sccache.sh
```

Requires Homebrew. Installs `sccache` and `lld` if missing, warms the cache, and prints stats.

## Per-project requirements

Every new Rust CLI should include:

1. **`.cargo/config.toml`** — lld linker (copy from `elevenlabs/.cargo/config.toml` or `flatland3/.cargo/config.toml`)
2. **`scripts/_dev-common.sh`** and **`scripts/setup-sccache.sh`** — optional but recommended for `dev_cargo` / sccache auto-enable

## Building

Prefer project scripts or explicit env when building:

```bash
export RUSTC_WRAPPER=sccache
export CARGO_INCREMENTAL=0   # required for sccache to cache crates effectively
cargo build --release
```

Or from a project with scripts:

```bash
./scripts/setup-sccache.sh   # once
source scripts/_dev-common.sh && dev_cargo build --release
```

## Why

| Tool | Effect |
|------|--------|
| **sccache** | Caches compiled crates across clean builds and branches |
| **lld** (`ld64.lld`) | Faster incremental linking on Apple Silicon / Intel Mac |
| **CARGO_INCREMENTAL=0** | Lets sccache own artifact caching (incremental + sccache fight each other) |

## Agent-first CLI conventions

Match existing CLIs (`gads`, `elabs`):

- `--json` / `--compact` global flags with structured envelope output
- `capabilities --json` — machine-readable command catalog
- `env schema --json` — env var precedence and secrets map
- Mutations require `--yes`, `--dry-run`, or TTY confirmation
- API keys via `apikey` subcommand + `ELEVENLABS_API_KEY` / project-specific env vars

## Existing Rust CLIs

| Directory | Binary | Purpose |
|-----------|--------|---------|
| `gads/` | `gads` | Google Ads API |
| `elevenlabs/` | `elabs` | ElevenLabs TTS/STT/voices |
| `storeshots/` | `storeshots` | Marketing assets |
| `git-sweep/` | `git-sweep` | Git cleanup |
| `secret-sweep/` | `secret-sweep` | Secret scanning |

When adding a new CLI, follow `elevenlabs/` or `gads/` layout and add a row here.
