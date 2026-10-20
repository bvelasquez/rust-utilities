# Agents working with airouter

This repo is a local OpenAI-compatible proxy. Configure it by editing JSON or by running `airouter` with `--json`. Do not put provider secrets in the config as literals; use `env:NAME` or `file:PATH`.

Read [docs/agent-usage.md](docs/agent-usage.md) before changing config or calling the API.

- Config path: `airouter config path`
- Schema: `airouter schema`
- Validate: `airouter config validate`
- Health of a running server: `GET /health` on the listen address, no auth
- Model list clients see: `GET /v1/models` with the local bearer token
- `auto` and `auto/<profile>` resolve through `routing.profiles` in the config. Aliases must not use those names. Hosted signal providers (Jev) send prompt text only for clients listed in the profile's `hosted_signal_clients`.

Do not log or echo API keys, the local token, or `Authorization` headers. `doctor` and the TUI report only whether a key is present.
