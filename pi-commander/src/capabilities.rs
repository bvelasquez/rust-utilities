use serde_json::json;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "pi-commander",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent coordination hub — spawn and supervise headless pi agents per project, with TUI, broker TTS, Telegram, and REST API",
        "transport": {
            "workers": "pi --mode rpc (JSONL over stdin/stdout), spawned as children by the daemon",
            "daemon": "pi-commander daemon — long-running hub owning agents",
            "api": "HTTP REST on the daemon (default 127.0.0.1:9851)"
        },
        "commands": [
            { "id": "watch", "mutation": false, "description": "Interactive agent dashboard (auto-starts daemon)" },
            { "id": "spawn", "mutation": true, "description": "Start agent(s); all projects or one project (-p <id> --agents N)" },
            { "id": "send", "mutation": true, "description": "Dispatch a task via natural routing: 'fix bug A on simple-workout' -> that project's least-busy agent" },
            { "id": "steer", "mutation": true, "description": "Interrupt an agent with a steering message (-a <worker>)" },
            { "id": "follow-up", "mutation": true, "description": "Queue a follow-up message for an agent (-a <worker>)" },
            { "id": "abort", "mutation": true, "description": "Abort a running agent (-a <worker>)" },
            { "id": "model", "mutation": true, "description": "Switch an agent's model provider/id (-a <worker> <model>)" },
            { "id": "cycle-model", "mutation": true, "description": "Cycle to next available model for an agent" },
            { "id": "thinking", "mutation": true, "description": "Set thinking level off|minimal|low|medium|high|xhigh|max" },
            { "id": "compact", "mutation": true, "description": "Compact an agent's context" },
            { "id": "bash", "mutation": true, "description": "Run a shell command inside an agent's context (result visible to that agent)" },
            { "id": "pi", "mutation": true, "description": "Raw JSONL RPC pass-through to an agent (-a <worker> '<json>')" },
            { "id": "stop", "mutation": true, "description": "Shut down a worker's pi process" },
            { "id": "new", "mutation": true, "description": "Spawn an extra agent for a project" },
            { "id": "hop", "mutation": true, "description": "Open an interactive pi on an agent's session (tmux window)" },
            { "id": "status", "mutation": false, "description": "Live status of all agents (--logs N for tail)" },
            { "id": "projects list", "mutation": false, "description": "List configured projects" },
            { "id": "config init|path|validate", "mutation": false, "description": "Config management" },
            { "id": "capabilities", "mutation": false, "description": "This manifest" },
            { "id": "env schema", "mutation": false, "description": "Environment schema for automation" },
        ],
        "workerIds": "project_id or project_id#agent_index — e.g. simple-workout or simple-workout#1",
        "phases": ["spawning", "idle", "busy", "retrying", "error", "stopped"],
        "configFile": {
            "scope": "user-wide only",
            "macOS": "~/Library/Application Support/pi-commander/projects.yaml",
            "linux": "~/.config/pi-commander/projects.yaml",
            "override": "PI_COMMANDER_CONFIG or --config"
        },
        "broker": {
            "protocol": "HTTP",
            "health": "GET /health",
            "speak": "POST /v1/tts/speak",
            "note": "Same broker as soki-ci (tts-stt-broker, default http://127.0.0.1:8787)"
        },
        "telegram": {
            "note": "Optional outbound status + inbound command channel via long-polling; configure in projects.yaml"
        },
        "api": {
            "bind": "PI_COMMANDER_API_BIND or --bind (default 127.0.0.1:9851)",
            "auth": "PI_COMMANDER_API_TOKEN or config api.token (Bearer); required when not binding to loopback",
            "routes": [
                "GET /health",
                "GET /state",
                "GET /agents",
                "GET /agents/{worker}",
                "POST /agents/{worker}/prompt",
                "POST /agents/{worker}/steer",
                "POST /agents/{worker}/follow_up",
                "POST /agents/{worker}/abort",
                "POST /agents/{worker}/model",
                "POST /agents/{worker}/thinking",
                "POST /agents/{worker}/compact",
                "POST /agents/{worker}/bash",
                "POST /agents/{worker}/pi",
                "POST /agents/{worker}/stop",
                "POST /dispatch",
                "POST /projects/{project}/spawn",
                "POST /projects/{project}/spawn_extra"
            ]
        },
        "agentHints": [
            "Run `pi-commander capabilities --json` and `pi-commander env schema --json` before automation",
            "Daemon must be running: `pi-commander daemon` (or `pi-commander watch` auto-starts it)",
            "Dispatch: `pi-commander send 'fix <X> on <project>' --json` — commander picks the least-busy agent of that project",
            "Specific worker: use project#index e.g. `pi-commander send --agent login-api#1 '...'`",
            "Interrupt: `pi-commander steer -a <worker> 'stop and do X instead'`",
            "Models: `pi-commander model -a <worker> anthropic/claude-sonnet-4`",
            "Remote control: REST API routes above (`curl -X POST /dispatch`)",
            "Quit TUI with busy agents: warns, then detaches — daemon + children keep running"
        ]
    })
}