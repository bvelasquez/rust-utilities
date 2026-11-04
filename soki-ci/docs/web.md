# soki-ci web dashboard

The service embeds a React/TypeScript SPA and serves it at `/`. It is
API-first: the browser talks to the same axum process over `/api/*` and uses
SSE for live updates.

## Run it

```bash
soki-ci serve                 # API + web UI
soki-ci web --open            # same, and open the browser
soki-ci                       # TUI + API + web UI
```

## Architecture

```text
Browser (React SPA)
  │  same-origin REST + SSE
  ▼
axum API (TUI or serve/web)
  ├── JobOrchestrator (shared in-process store)
  ├── EventBus (broadcast) ──► SSE
  ├── runs/{id}.json        ──► persistent job history
  ├── runs/{id}.log         ──► full log archive
  └── runner → pnpm / npm / make / shell
```

- **Frontend:** React 19, TypeScript, Vite, Tailwind CSS, TanStack Query.
- **Backend:** axum handlers in `src/api/`, SSE in `src/api/events.rs`,
  embedded assets in `src/api/web_assets.rs`, persistence in `src/history.rs`.
- **Live updates:** `/api/events` for job/config state, `/api/jobs/{id}/events`
  for log lines and phase changes.
- **Cancellation:** jobs are spawned in their own Unix process group and get
  `SIGTERM`, then `SIGKILL` after a grace period.
- **Persistence:** sidecar JSON written atomically on every job transition.
  Active jobs found at startup are marked `interrupted`.

## API surface

See `GET /capabilities` for the machine-readable manifest. Web-oriented routes:

- `GET /api/status`
- `GET /api/projects`, `GET /api/projects/{id}`
- `POST /api/projects/{id}/builds/{build}`
- `GET /api/jobs`, `GET /api/jobs/{id}`
- `GET /api/jobs/{id}/logs`, `GET /api/jobs/{id}/events`
- `POST /api/jobs/{id}/cancel`, `/retry`, `/input`
- `POST /api/jobs/reset`
- `GET /api/events`
- `GET /api/config`, `POST /api/config/reload`, `PUT /api/config`
- `GET /api/tts`, `POST /api/tts/test`
- `GET/POST/DELETE /api/session`

Legacy agent routes (`/projects/builds`, `/jobs`, `/jobs/{id}/input`) are
unchanged.

## Development

```bash
make web-install
make web-dev        # Vite on :5173, proxying /api to :9847
# another terminal:
soki-ci serve
```

`make build` / `make install` run the web build first, then embed `web/dist`
with `rust-embed`. Set `SOKI_CI_WEB_DIR` or `--web-dir` to serve from disk.

## Auth

- No `SOKI_CI_API_TOKEN`: local mode, no login.
- With a token: `POST /api/session` exchanges the token for an HttpOnly,
  `SameSite=Strict` cookie and returns a CSRF token. Mutations authenticated by
  cookie must send `X-Soki-CSRF`.
- `Authorization: Bearer <token>` still works for agents and CLI clients.
- Static assets stay public; data routes are protected.

## Favorite commands

Star any deploy target on a project detail page. Favorites are stored in the
browser (`localStorage`) and:

- Sort to the **top** of that project’s target list (global order across projects).
- Appear on the **Overview** with **Run** and reorder (↑↓) controls.

Reload the config after editing `projects.yaml`; favorites are independent of the server.

## Config editing

`PUT /api/config` is disabled by default. Start with `--allow-config-write` (or
`SOKI_CI_ALLOW_CONFIG_WRITE=1`) to enable the browser editor. It validates,
writes a `.bak`, then reloads. Validation errors are returned and nothing is
written.
