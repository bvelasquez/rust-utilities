# airouter

A single-binary OpenAI-compatible router for your own provider accounts. Point Cursor, pi, or any OpenAI SDK at it, pick a model you have enabled, and the process forwards the call with that provider's API key.

v1 routes the model you name. It does not choose one for you. Model ids that are `auto` or start with `auto/` are reserved for a later rules router.

## Run

```bash
cargo build --release
./target/release/airouter init
export OPENAI_API_KEY=sk-...
./target/release/airouter models add openai/gpt-4o-mini
./target/release/airouter alias set fast openai/gpt-4o-mini
./target/release/airouter serve
```

`init` writes `~/.config/airouter/config.json` and a token file, both mode `0600`. The token is the bearer clients send to airouter. Provider keys stay in env vars or files referenced from the config.

```bash
curl http://127.0.0.1:8787/v1/models \
  -H "Authorization: Bearer $(tr -d '[:space:]' < ~/.config/airouter/token)"
```

In Cursor or another OpenAI-compatible client:

- Base URL: `http://127.0.0.1:8787/v1` on this machine, or `http://<tailscale-ip>:8787/v1` from another device on the tailnet
- API key: the local token from `init`
- Model: an id from `GET /v1/models`, or an alias such as `fast`

`airouter serve --tui` opens the dashboard in the same terminal. `airouter tui` attaches to a server that is already running. The dashboard reads the admin API on `127.0.0.1:8788` (requests, spend, providers, models). It never shows API keys.

## Config

Override the path with `--config` or `AIROUTER_CONFIG`. Print the schema with `airouter schema`.

Secrets are strings:

- `env:OPENAI_API_KEY`
- `file:~/.secrets/anthropic` (leading `~` is your home directory)
- a literal value (avoid this; `doctor` warns)

`openai_compat` plus `base_url` covers Groq, Together, DeepSeek, Mistral, xAI, Fireworks, OpenRouter, Ollama, vLLM, and LM Studio without new code.

| Provider | type | base_url |
| --- | --- | --- |
| OpenAI | `openai` | default `https://api.openai.com/v1` |
| Anthropic | `anthropic` | default `https://api.anthropic.com` |
| Gemini | `gemini` | default `https://generativelanguage.googleapis.com/v1beta` |
| Groq | `openai_compat` | `https://api.groq.com/openai/v1` |
| Together | `openai_compat` | `https://api.together.xyz/v1` |
| DeepSeek | `openai_compat` | `https://api.deepseek.com/v1` |
| Mistral | `openai_compat` | `https://api.mistral.ai/v1` |
| xAI | `openai_compat` | `https://api.x.ai/v1` |
| DeepInfra | `openai_compat` | `https://api.deepinfra.com/v1/openai` |
| Cheaper Inference | `openai_compat` | `https://api.cheaperinference.com/v1` |
| OpenRouter | `openai_compat` | `https://openrouter.ai/api/v1` |
| Ollama | `openai_compat` | `http://127.0.0.1:11434/v1` |

Anthropic and Gemini chat (including tools, images, and streaming) are translated to the OpenAI chat schema. Embeddings, image generation, speech, and transcription pass through OpenAI-compatible providers. Gemini embeddings are translated. Video is not served; `POST /v1/videos` returns 501 so the later async-job hook has a visible seam.

Only models with `"enabled": true` appear on `GET /v1/models`. The built-in catalog supplies default context, capabilities, and prices:

```bash
airouter models search claude
airouter models add anthropic/claude-sonnet-4-5
airouter models price openai/gpt-4o-mini
```

Catalog prices are defaults you can override. They are not a live price feed. `models add deepinfra/<org>/<model>` and `models add cheaperinference/<id>` are the exceptions: both read the provider's public model list, and Cheaper Inference returns the customer-facing discounted rates for the configured key.

`server.listen` defaults to `0.0.0.0:8787` so clients on Tailscale can reach it. That bind is refused unless at least one `server.api_keys` entry resolves. `server.admin_listen` stays on loopback. The admin API is unauthenticated.

Editing the config file reloads providers, models, and keys. Listen addresses apply on restart.

## Usage and cost

Every request is stored in SQLite (`$XDG_STATE_HOME/airouter/usage.db`, or `~/.local/state/airouter/usage.db`). Prompt and response bodies are not stored. Cost uses the model's pricing and the upstream usage report. Cache reads fall back to the input price when no cache price is set, so cached tokens are not treated as free by accident.

```bash
airouter usage --since 7d --by model
airouter usage --since 24h --by provider --json
```

`GET /v1/usage?since=7d&by=model` returns the same summary to an authenticated client.

## Commands

`airouter --help` lists them. Every command accepts `--json` and exits non-zero on failure. `tui`, `serve --tui`, and `config edit` need a terminal.

- `init`, `serve [--tui]`, `tui`
- `provider list|add|remove|test`
- `models list|search|add|enable|disable|price`
- `alias list|set|remove`
- `usage`, `doctor [--offline]`, `schema`
- `config path|validate|schema|edit`

`provider test` makes a short authenticated call (list models, or a 1-token Anthropic message).

## Deploy

```bash
make
sudo make install
```

That builds a release binary and installs it to `/usr/local/bin/airouter`. `sudo make install` compiles as your user (so `cargo` from rustup is found and `target/` stays yours) and only the copy into `/usr/local` runs as root. `make install PREFIX=$HOME/.local` installs into your home directory instead. `make uninstall` removes it.

`make install-service` installs `deploy/airouter.user.service` as a systemd user unit and enables it. With linger (`loginctl enable-linger`), that unit starts at boot without a login. `make redeploy` stops the unit, does a clean release build, installs the binary, and starts the unit again. `airouter tui` attaches to the running server.

`deploy/airouter.service` is a sample system unit (dedicated `airouter` user, `/etc/airouter/config.json`) for a machine that should not run the router as your login. For a VM without the Rust toolchain:

```bash
./scripts/build-musl.sh
```

That needs `musl-tools` (`musl-gcc`) because SQLite is bundled. The script selects `x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl` from the host. The binary lands in `target/<target>/release/airouter`.

## Layout

- `crates/core` shared types, pricing, cost
- `crates/config` JSON config, secret references, schema
- `crates/registry` catalog, aliases, explicit router (`RulesRouter` is the later plug-in)
- `crates/providers` OpenAI passthrough, Anthropic, Gemini
- `crates/gateway` `/v1` and the loopback admin API
- `crates/store` SQLite usage log
- `crates/tui` dashboard
- `crates/cli` the `airouter` binary
