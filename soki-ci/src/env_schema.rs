use serde_json::json;

pub fn env_schema_json() -> serde_json::Value {
    json!({
        "precedence": ["CLI flags", "environment variables", "user-wide projects.yaml"],
        "variables": [
            { "name": "SOKI_CI_CONFIG", "required": false, "secret": false, "description": "Override path to projects.yaml" },
            { "name": "SOKI_CI_NON_INTERACTIVE", "required": false, "secret": false, "description": "Refuse bare TUI" },
            { "name": "SOKI_CI_API_BIND", "required": false, "secret": false, "description": "HTTP API listen address (host:port); default 127.0.0.1:9847; used by TUI and serve" },
            { "name": "SOKI_CI_API_TOKEN", "required": false, "secret": true, "description": "Bearer token for HTTP API when exposed beyond loopback" },
            { "name": "SOKI_CI_MAX_PARALLEL", "required": false, "secret": false, "description": "Override defaults.max_parallel in YAML" },
            { "name": "TTS_BROKER_BASE_URL", "required": false, "secret": false, "description": "Broker base URL (YAML defaults.speak.base_url)" },
            { "name": "TTS_BROKER_VOICE", "required": false, "secret": false, "description": "TTS voice (default Leda in example YAML)" },
        ],
        "flags": {
            "--no-api": "Disable HTTP API when starting the TUI",
            "--bind": "Same as SOKI_CI_API_BIND",
            "--api-token": "Same as SOKI_CI_API_TOKEN"
        },
        "configFile": {
            "fields": {
                "defaults.projects_dir": "Base path; expands ${projects_dir} in project paths",
                "defaults.max_parallel": "Max concurrent deploy jobs across projects",
                "defaults.speak": "HTTP broker TTS on terminal job states",
                "projects[].targets": "Named deploy actions with runner kind pnpm_script | npm_script | make | shell"
            }
        }
    })
}
