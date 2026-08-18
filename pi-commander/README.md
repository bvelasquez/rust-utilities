# pi-commander

Agent coordination hub: spawn and supervise headless **pi agents per project**,
from a single TUI. The "one pane all day" dashboard for parallel coding agents.

```
┌──────────────────────── COMMANDER TUI — one pane ────────────────────────┐
│  agent roster │ live phase/model/tool | log tail │ command input          │
└──────────────────────┬─────────────────────────────────────┬──────────────┘
                       │ owns (stdin/stdout JSONL)           │ speaks:
                       ▼                                     ▼
        pi --mode rpc --cwd <project> …        tts-stt-broker :8787 (voice)
        pi --mode rpc --cwd <project> …        Telegram (status + remote cmds)
```

Everything is a stock `pi --mode rpc` process in each project — the commander
just owns the other side of the pipe and turns its JSON events into status.
No herdr panes, no craning your neck.

## Architecture

- **daemon** (`pi-commander daemon`) — the hub. Spawns/restarts workers (one
  task per agent), supervises crash/restart policy, aggregates live status,
  talks to the broker (`:8787/v1/tts/speak`) and Telegram, serves the REST API
  (default `127.0.0.1:9851`).
- **watch** (`pi-commander watch`) — ratatui dashboard. Auto-starts the daemon if
  it's down, then polls `/state` and shows every agent's live status.
- **cli** — everything else goes through the daemon API, so commands work from
  any terminal/session and remote agents can drive the commander too.

## Install

```bash
make install        # cargo build --release + cp to ~/.cargo/bin
```

## Quick start

```bash
pi-commander config init        # copy projects.example.yaml to user config
# edit ~/Library/Application Support/pi-commander/projects.yaml
pi-commander config validate
pi-commander watch              # single pane — spawned agents + live status
```

## Commands

```bash
pi-commander watch                              # TUI (auto-starts daemon)
pi-commander daemon                             # hub (run in one tmux/panel)
pi-commander spawn [project] [--agents N]       # start agents
pi-commander status [--logs 10]                 # live status text view
pi-commander send "list the top-level files on simple-workout"   # natural dispatch
pi-commander steer  -a simple-workout#0 "stop, do X instead"     # interrupt
pi-commander fup     -a simple-workout#0 "then also fix Y"       # follow-up
pi-commander abort   -a simple-workout#0
pi-commander model   -a simple-workout#0 anthropic/claude-sonnet-4
pi-commander cycle-model -a simple-workout#0
pi-commander thinking -a simple-workout#0 high
pi-commander compact -a simple-workout#0        # manual context compaction
pi-commander bash     -a simple-workout#0 "pnpm test -- --runInBand"  # runs in the agent's context
pi-commander pi       -a simple-workout#0 '{"type":"get_messages"}'   # raw JSON-RPC
pi-commander stop     -a simple-workout#0
pi-commander new      simple-workout            # extra parallel agent
pi-commander hop      simple-workout#0          # interactive pi on that session
```

Worker ids are `project_id` or `project_id#agent_index` (`simple-workout#1`).
Dispatch routing: "…on <project>" picks the least-busy agent of that project.

## Watch TUI (power-user)

`pi-commander watch` is the day-long pane. Layout: header · **activity inbox** · projects | agents | log · input.

**Dispatch rules**

- Free text → prompt the **selected** agent (daemon auto steer vs follow-up by phase).
- Text with `on|in|for <project>` → natural `/dispatch` (least-busy agent of that project).
- `/dispatch …` or `/d …` → always natural dispatch.
- Slash verbs work with or without `/`; worker is optional when an agent is selected (`/steer fix it`, `/fup then also Y`).
- `/` opens the command menu; **↑↓** highlight, **Tab** complete, **Enter** run.

**Navigation**

| Keys | Action |
|------|--------|
| `1`–`4` | Focus projects / agents / log / input |
| Tab / mouse click | Cycle or jump panes; click rows to select |
| `j`/`k` · wheel | Move selection / scroll log |
| `n`/`N` | Next / prev activity (completion or error) |
| `f` / `s` / `a` / `x` | Follow-up / steer / abort / stop (selected agent) |
| `b` / `!` | Filter busy-only / errors-only |
| `z` | Dense vs preview agent rows |
| `G` | Jump log to end (resume follow) |
| Ctrl+R | Reload daemon config |

When any agent goes **idle** or **error**, the activity strip updates (and the status line announces it). Press `n` to select that agent, then `f` + message for a fast follow-up.

## Config reference

```yaml
version: 1
defaults:
  projects_dir: ~/projects
  agents_per_project: 1
  pi: pi
  auto_restart: true
  max_restarts: 3
  speak:            # tts-stt-broker
    enabled: true
    voice: Leda
    base_url: http://127.0.0.1:8787
    on: [routed, idle, error]
  telegram:         # optional outbound status + inbound commands
    enabled: false
    bot_token: ""          # or TELEGRAM_BOT_TOKEN
    chat_id: ""            # or TELEGRAM_CHAT_ID
    inbound: true
    notify: [routed, idle, error]
  api:
    bind: 127.0.0.1:9851   # or PI_COMMANDER_API_BIND
    token: ""              # or PI_COMMANDER_API_TOKEN (Bearer)
projects:
  - id: simple-workout
    path: ${projects_dir}/simple-health/simple-workout
    agents: 1
    model: anthropic/claude-sonnet-4   # optional per-worker default model
    env: { PI_MODE: worker }           # optional per-worker env
```

## Agent-friendly / automation

- `pi-commander capabilities --json` — capabilities manifest
- `pi-commander env schema --json` — env var schema
- REST API on the daemon: `/health`, `/state`, `/agents`, `/agents/{w}`,
  `POST /agents/{w}/prompt|steer|follow_up|abort|model|thinking|compact|bash|pi|stop`,
  `POST /dispatch` (natural routing), `POST /projects/{id}/spawn`
- CLI commands mirror the API; all return `{success,command,data,…}` envelopes
  with `--json`

## Example flow

```bash
pi-commander daemon &
pi-commander send "fix the failing night test on simple-workout"
# ...broker says "routed to simple-workout#0", watch shows ⏳ busy…
# ...when idle: voice + telegram "simple-workout done: 3 tests fixed, push in PR"
pi-commander hop simple-workout#0   # drop into that session interactively if needed
```

Known niceties: worker ids (`#`) are URL-safe in the API client; phases
spawning→idle→busy→retrying→error; auto-restart on crash with backoff.