use serde_json::json;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "soki-ci",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Multi-project deploy control panel — parallel deploys, TUI, broker HTTP TTS",
        "commands": [
            { "id": "watch", "mutation": false, "description": "Interactive deploy dashboard (default in TTY); also serves HTTP API unless --no-api" },
            { "id": "projects list", "mutation": false, "description": "List projects and deploy targets from YAML" },
            { "id": "config init", "mutation": true, "description": "Create user-wide projects.yaml from example" },
            { "id": "config validate", "mutation": false, "description": "Validate paths and package.json scripts" },
            { "id": "config path", "mutation": false, "description": "Print active config path" },
            { "id": "deploy run", "mutation": true, "description": "Start deploy job(s); requires --yes in automation" },
            { "id": "jobs list", "mutation": false, "description": "Active and recent jobs in this process" },
            { "id": "jobs logs", "mutation": false, "description": "Read job log file" },
            { "id": "jobs cancel", "mutation": true, "description": "Mark job cancelled (v1: does not kill child)" },
            { "id": "jobs reset", "mutation": true, "description": "Remove finished deployments and log files from this process" },
            { "id": "serve", "mutation": false, "description": "HTTP API only (no TUI) — list and trigger deploys" },
            { "id": "capabilities", "mutation": false },
            { "id": "env schema", "mutation": false },
        ],
        "runners": ["pnpm_script", "npm_script", "make", "shell"],
        "configFile": {
            "scope": "user-wide only",
            "macOS": "~/Library/Application Support/com.soki-ci.soki-ci/projects.yaml",
            "linux": "~/.config/soki-ci/projects.yaml",
            "override": "SOKI_CI_CONFIG or --config"
        },
        "broker": {
            "protocol": "HTTP",
            "health": "GET /health",
            "speak": "POST /v1/tts/speak",
            "note": "No broker-cli binary required"
        },
        "api": {
            "command": "soki-ci (TUI) or soki-ci serve",
            "bind": "SOKI_CI_API_BIND or --bind (default 127.0.0.1:9847)",
            "auth": "SOKI_CI_API_TOKEN or --api-token (Bearer); required when not binding to loopback",
            "disable": "--no-api (TUI only)",
            "routes": [
                "GET /health",
                "GET /projects/builds",
                "POST /projects/{project_id}/builds/{build_id}",
                "GET /jobs",
                "GET /jobs/{job_id}"
            ],
            "note": "build_id is the deploy target id from projects.yaml; TUI and API share the same in-process job store"
        },
        "agentHints": [
            "Run `soki-ci capabilities --json` and `soki-ci env schema --json` before automation",
            "Use `soki-ci config validate --json` after editing projects.yaml",
            "Deploy: `soki-ci deploy run -p <id> -t <target> --yes --json`",
            "Remote API: with TUI running (or `soki-ci serve`), GET /projects/builds and POST /projects/{id}/builds/{build} on --bind (default 127.0.0.1:9847)",
            "Quit TUI with running jobs: warns then detaches (children keep running); API shuts down with the TUI",
            "Speak only on success/error terminal states, not on job start"
        ]
    })
}
