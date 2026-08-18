# soki-ci

Multi-project deploy control panel: user-wide YAML registry, parallel deploy jobs across repos, ratatui dashboard, HTTP broker TTS on success/failure, and an HTTP API to trigger builds remotely (embedded in the TUI by default).

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
- **Voice:** `POST /v1/tts/speak` on the broker — no `broker-cli` binary required.
- **HTTP API:** started with the TUI by default (`--bind`, default `127.0.0.1:9847`). Use `--no-api` for TUI-only, or `soki-ci serve` for API-only. `GET /projects/builds` lists configured deploy targets as `builds`; `POST /projects/{project_id}/builds/{build_id}` queues a job. Set `SOKI_CI_API_TOKEN` when binding beyond localhost.

## TUI navigation

Layout: header · **deployments** · projects | targets | output · shortcuts · status.

| Keys | Action |
|------|--------|
| `1`–`4` | Focus deployments / projects / targets / log |
| Tab / mouse click | Cycle or jump panes; click rows to select |
| `j`/`k` · wheel | Move selection / scroll log |
| Enter | Run selected target (or open log from deployments) |
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
curl -s http://127.0.0.1:9847/projects/builds | jq .
curl -s -X POST http://127.0.0.1:9847/projects/my-app/builds/hosting
curl -s http://127.0.0.1:9847/jobs/<job_id> | jq .
```

`build_id` matches the target key in `projects.yaml` (same as `soki-ci deploy run -t`). Remote starts share the TUI’s in-process job list.

## Agents

```bash
soki-ci capabilities --json
soki-ci env schema --json
soki-ci deploy run -p simple-food-track -t hosting --yes --json --wait
```
