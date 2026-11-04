# soki-ci

Multi-project deploy control panel: user-wide YAML registry, parallel deploy jobs across repos (targets of the *same* project are serialized — they share one working tree and build into the same output dir), ratatui dashboard, HTTP broker TTS on success/failure, and an HTTP API to trigger builds remotely (embedded in the TUI by default).

## Quick start

```bash
cd soki-ci
make install
soki-ci config init
# edit ~/Library/Application Support/com.soki-ci.soki-ci/projects.yaml (macOS)
soki-ci config validate
soki-ci   # TUI + HTTP API + web UI on 127.0.0.1:9847
# or headless:
soki-ci serve --bind 127.0.0.1:9847
# open http://127.0.0.1:9847/ for the web dashboard
```

## Decisions (v1)

- **Config:** user-wide `projects.yaml` only (`soki-ci config path`).
- **Quit:** warns if jobs are running; closing the TUI does **not** stop deploy children (API stops with the TUI).
- **Runners:** `pnpm_script`, `npm_script`, `make` (runs `make <target>` in project or target `cwd`), `shell`.
- **Voice (optional):** broker TTS announces terminal deploy success/error via `POST /v1/tts/speak` on [tts-stt-broker](https://github.com/bvelasquez/tts-stt-broker) — no `broker-cli` binary required. Defaults to `http://127.0.0.1:18888`; omit `voice` to use the broker's configured voice. Silence it globally with `enabled: false` or per target with `speak: false`.
- **HTTP API:** started with the TUI by default (`--bind`, default `127.0.0.1:9847`). Use `--no-api` for TUI-only, or `soki-ci serve` / `soki-ci web` for API + web UI without the TUI. `GET /capabilities` returns the full agent manifest (routes, JSON schemas, CLI, env). `GET /projects/builds` lists configured deploy targets as `builds`; `POST /projects/{project_id}/builds/{build_id}` queues a job. Set `SOKI_CI_API_TOKEN` when binding beyond localhost.

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

## Web dashboard

The service also serves a React/TypeScript dashboard at `/`:

```bash
soki-ci serve                 # API + web UI, no TUI
soki-ci web --open            # same, and open the browser
soki-ci                       # TUI + API + web UI (open / in a browser)
```

The web app is API-first and same-origin. It talks to the running service over
`/api/*`, using SSE for live job state and log lines. Features beyond the TUI:

- Persistent deployment history (`runs/{id}.json`) with search, filters, sorting, and pagination.
- Live log viewer with stream badges, search, follow/pause, ANSI stripping, copy, and download.
- Project/target metadata: runner command, cwd, env keys, timeout, speak override, danger flags, tags, groups.
- Real cancellation (process-group `SIGTERM`, then `SIGKILL`), retry, and clear-history actions.
- Queue view with per-project locks and global parallel slots.
- Config viewer, validation issues, reload, and optional YAML editing.
- Overview stats, success rate, average duration, recent failures, TTS broker status.
- Browser notifications and keyboard-friendly navigation.

### Web development

```bash
make web-install
make web-dev        # Vite on :5173, proxying /api to :9847
# in another terminal:
soki-ci serve
```

`make build` and `make install` build the web app first and embed `web/dist`
into the Rust binary with `rust-embed`. Set `SOKI_CI_WEB_DIR` (or `--web-dir`)
to serve assets from disk instead.

### Browser auth

When `SOKI_CI_API_TOKEN` is set, the static UI is still public, but data routes
require auth. The browser login exchanges the token for an HttpOnly
`SameSite=Strict` session cookie and returns a CSRF token for mutations.
`Authorization: Bearer <token>` continues to work for agents and CLI clients.

### Web API additions

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/status` | version, uptime, queue, locks, TTS |
| GET | `/api/projects` | enriched project/target catalog + stats |
| GET | `/api/projects/{id}` | one project |
| POST | `/api/projects/{id}/builds/{build}` | queue a deploy (source=web) |
| GET | `/api/jobs` | paginated/filterable history |
| GET | `/api/jobs/{id}` | job detail |
| GET | `/api/jobs/{id}/logs` | log lines, text, or download |
| GET | `/api/jobs/{id}/events` | SSE log + phase stream |
| GET | `/api/events` | SSE global job/config stream |
| POST | `/api/jobs/{id}/cancel` | real cancel |
| POST | `/api/jobs/{id}/retry` | re-enqueue |
| POST | `/api/jobs/{id}/input` | stdin for prompts |
| POST | `/api/jobs/reset` | clear finished jobs/logs |
| GET/POST | `/api/config`, `/api/config/reload` | read/reload config |
| PUT | `/api/config` | write YAML (requires `--allow-config-write`) |
| GET/POST | `/api/tts`, `/api/tts/test` | TTS status/test |
| GET/POST/DELETE | `/api/session` | browser login/session |

The legacy agent routes (`/projects/builds`, `/jobs`, `/jobs/{id}/input`) are
unchanged.

### History and data

Jobs now persist as sidecars next to their log:

```text
~/.local/share/soki-ci/runs/{job_id}.log
~/.local/share/soki-ci/runs/{job_id}.json
```

On startup, jobs that were running/queued when the previous process exited are
marked `interrupted`. Set `SOKI_CI_DATA_DIR` to isolate data, and
`SOKI_CI_HISTORY_LIMIT` / `defaults.history_limit` to bound loaded history
(default 500).

### Optional project/target metadata

```yaml
projects:
  - id: simple-food-track
    group: Firebase Apps
    tags: [web, firebase]
    description: Food tracking app
    targets:
      all:
        label: DANGER: Deploy everything
        danger: true
        description: Runs all Firebase deploy steps
        tags: [danger, full]
```

All fields are optional. `danger: true` (or a label starting with `DANGER`)
forces the web confirmation dialog.

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

`soki-ci deploy run` also submits to the running server at `--bind` when one is reachable, so CLI-started jobs appear in the TUI and web UI and can be streamed/cancelled there. It falls back to a standalone in-process run when no server answers `/health`.

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
