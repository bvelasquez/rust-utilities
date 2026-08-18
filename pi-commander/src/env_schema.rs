use serde_json::json;

/// Machine-readable schema of every env var pi-commander reads, plus per-worker
/// passthrough notes (PI_COMMANDER_* is consumed by the commander; everything
/// else is inherited by spawned pi workers).
pub fn env_schema_json() -> serde_json::Value {
    json!({
        "variables": [
            {
                "name": "PI_COMMANDER_CONFIG",
                "scope": "commander",
                "description": "Path to projects.yaml (default: user config dir)",
                "example": "/path/to/projects.yaml",
                "override": "--config"
            },
            {
                "name": "PI_COMMANDER_API",
                "scope": "commander",
                "description": "Daemon API base URL used by CLI/TUI",
                "default": "http://127.0.0.1:9851",
                "override": "--api"
            },
            {
                "name": "PI_COMMANDER_API_BIND",
                "scope": "daemon",
                "description": "Address the daemon HTTP API binds",
                "default": "127.0.0.1:9851",
                "override": "daemon --bind"
            },
            {
                "name": "PI_COMMANDER_API_TOKEN",
                "scope": "daemon",
                "description": "Bearer token required for API access when binding off loopback",
                "default": "(empty — loopback open)"
            },
            {
                "name": "PI_COMMANDER_NON_INTERACTIVE",
                "scope": "commander",
                "description": "Refuse interactive TUI",
                "default": "0"
            }
        ],
        "notes": [
            "Per-project `env:` entries in projects.yaml are merged into that worker's pi process environment",
            "pi reads its own config/auth from the usual locations (~/.pi) and inherits commander env",
            "TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID are honored as fallbacks for telegram.* config"
        ]
    })
}