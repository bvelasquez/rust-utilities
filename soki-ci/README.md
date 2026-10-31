# soki-ci

Multi-project deploy control panel: user-wide YAML registry, parallel deploy jobs across repos (targets of the *same* project are serialized — they share one working tree and build into the same output dir), ratatui dashboard, HTTP broker TTS on success/failure, and an HTTP API to trigger builds remotely (embedded in the TUI by default).

## Quick start

```bash
cd soki-ci
make install
soki-ci config init
# edit ~/Library/Application Support/com.soki-ci.soki-ci/projects.yaml (macOS)
soki-ci config validate
soki-ci   # TUI + HTTP API on 127.0.0.1:9847
```

## Decisions (v1)

- **Config:** user-wide `projects.yaml` only (`soki-ci config path`).
- **Quit:** warns if jobs are running; closing the TUI does **not** stop deploy children (API stops with the TUI).
- **Runners:** `pnpm_script`, `npm_script`, `make` (runs `make <target>` in project or target `cwd`), `shell`.
- **Voice (optional):** broker TTS announces terminal deploy success/error via `POST /v1/tts/speak` on [tts-stt-broker](https://github.com/bvelasquez/tts-stt-broker) — no `broker-cli` binary required. Defaults to `http://127.0.0.1:18888`; omit `voice` to use the broker's configured voice. Silence it globally with `enabled: false` or per target with `speak: false`.
- **HTTP API:** started with the TUI by default (`--bind`, default `127.0.0.1:9847`). Use `--no-api` for TUI-only, or `soki-ci serve` for API-only. `GET /capabilities` returns the full agent manifest (routes, JSON schemas, CLI, env). `GET /projects/builds` lists configured deploy targets as `builds`; `POST /projects/{project_id}/builds/{build_id}` queues a job. Set `SOKI_CI_API_TOKEN` when binding beyond localhost.

## Spoken deploy status (optional)

When the local TTS broker is running, soki-ci can announce how each deploy finished:

```yaml
defaults:
  speak:
    enabled: true                    # false = silent
    base_url: http://127.0.0.1:18888 # tts-stt-broker default
    # voice: Kore                    # omit for the broker's default voice
    on: [success, error]             # terminal states to announce
```

A target can override the global switch:

```yaml
targets:
  testflight:
    speak: false
    runner: { kind: pnpm_script, script: mobile:testflight }
```

Successful deploys say e.g. “Simple Workout Firebase Hosting deploy succeeded.”; failures include the exit
code and the last meaningful output line. Broker failures are logged in the job output as
`speak warning:` and never change the deploy result. Env overrides: `TTS_BROKER_BASE_URL` and
`TTS_BROKER_VOICE`.

## TUI navigation

Layout: header · **deployments** · projects | targets | output · shortcuts · status.

| Keys | Action |
|------|--------|
| `1`–`4` | Focus deployments / projects / targets / log |
| Tab / mouse click | Cycle or jump panes; click rows to select |
| `j`/`k` · wheel | Move selection / scroll log |
| Enter | Run selected target (or open log from deployments) |
| `i` | Type a line into the selected running job (answers `y/N` prompts) |
| `[`/`]` · PgUp/PgDn | Scroll log (when log focused) |
| `g`/`G` · Home/End | Jump to start / end of focused pane |
| `r` | Reload config |
| `x` | Clear finished deployment history |
| `q` | Quit |

## Remote API

With the TUI running (or headless `serve`):

```bash
soki-ci                                    # TUI + API
# or: soki-ci serve                        # API only
# or: soki-ci serve --bind 0.0.0.0:9847 --api-token "$TOKEN"
curl -s http://127.0.0.1:9847/capabilities | jq .
curl -s http://127.0.0.1:9847/projects/builds | jq .
curl -s -X POST http://127.0.0.1:9847/projects/my-app/builds/hosting
curl -s http://127.0.0.1:9847/jobs/<job_id> | jq .
curl -s -X POST http://127.0.0.1:9847/jobs/<job_id>/input \
  -H 'content-type: application/json' \
  -d '{"text":"y"}'
```

`build_id` matches the target key in `projects.yaml` (same as `soki-ci deploy run -t`). Remote starts share the TUI’s in-process job list.

To answer an interactive prompt from another terminal while the TUI is up:

```bash
soki-ci jobs input -i <job_id> --text y --yes
```

## Agents

```bash
soki-ci capabilities --json
soki-ci env schema --json
soki-ci deploy run -p simple-food-track -t hosting --yes --json --wait
```
