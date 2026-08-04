# soki-ci

Multi-project deploy control panel: user-wide YAML registry, parallel deploy jobs across repos, ratatui dashboard, and HTTP broker TTS on success/failure.

## Quick start

```bash
cd soki-ci
make install
soki-ci config init
# edit ~/Library/Application Support/com.soki-ci.soki-ci/projects.yaml (macOS)
soki-ci config validate
soki-ci   # TUI
```

## Decisions (v1)

- **Config:** user-wide `projects.yaml` only (`soki-ci config path`).
- **Quit:** warns if jobs are running; closing the TUI does **not** stop deploy children.
- **Package managers:** `pnpm` and `npm` script runners (+ `make` / `shell`).
- **Voice:** `POST /v1/tts/speak` on the broker — no `broker-cli` binary required.

## Agents

```bash
soki-ci capabilities --json
soki-ci env schema --json
soki-ci deploy run -p simple-food-track -t hosting --yes --json --wait
```
