use serde_json::json;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "disk-sweep",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-first smart disk cleanup — scan caches, Xcode junk, and review folders with LLM",
        "commands": [
            { "id": "scan", "mutation": false, "requiresAuth": false, "description": "Scan default cleanup targets and report sizes" },
            { "id": "interactive", "mutation": false, "requiresAuth": false, "description": "Interactive TUI to browse, select, and clean (default in TTY)" },
            { "id": "watch", "mutation": false, "requiresAuth": false, "description": "Live disk usage dashboard with folder bars, category breakdown, and usage trend" },
            { "id": "analyze", "mutation": false, "requiresAuth": false, "description": "Find cleanup candidates in dot folders, Library, and stale git projects" },
            { "id": "clean", "mutation": true, "requiresAuth": false, "description": "Delete selected targets (requires --yes or --dry-run in non-interactive mode)" },
            { "id": "review", "mutation": false, "requiresAuth": true, "description": "LLM review of a folder for cleanup candidates" },
            { "id": "targets list", "mutation": false, "requiresAuth": false, "description": "List built-in cleanup targets" },
            { "id": "capabilities", "mutation": false, "requiresAuth": false },
            { "id": "env schema", "mutation": false, "requiresAuth": false },
        ],
        "defaultCategories": [
            "Xcode Junk",
            "User Cache Files",
            "User Log Files"
        ],
        "agentHints": [
            "Run `disk-sweep capabilities --json` and `disk-sweep env schema --json` before automation",
            "Use `disk-sweep scan --json` to get sizes without the TUI",
            "Use `disk-sweep analyze --json` for dot folders, Library, and stale projects",
            "Use `disk-sweep watch` for live volume and folder usage charts",
            "Use `disk-sweep review <path> --json` to classify unknown folders",
            "Mutations require `--yes` or `--dry-run` when stdout is not a TTY"
        ]
    })
}

pub fn env_schema_json() -> serde_json::Value {
    json!({
        "precedence": ["CLI flags", "environment variables", "~/.config/disk-sweep/config.toml"],
        "variables": [
            { "name": "DISK_SWEEP_OPENROUTER_KEY", "aliases": ["OPENROUTER_API_KEY"], "required": false, "secret": true, "description": "OpenRouter API key for `review` command" },
            { "name": "DISK_SWEEP_LLM_MODEL", "required": false, "secret": false, "description": "OpenRouter model override (default: openai/gpt-4o-mini)" },
            { "name": "DISK_SWEEP_CONFIG", "required": false, "secret": false, "description": "Override config file path" },
            { "name": "DISK_SWEEP_WATCH_INTERVAL", "required": false, "secret": false, "description": "Default refresh interval for watch mode (e.g. 30s, 5m)" },
            { "name": "DISK_SWEEP_PROJECTS_ROOT", "required": false, "secret": false, "description": "Projects root for analyze (default: ~/projects)" },
            { "name": "DISK_SWEEP_STALE_DAYS", "required": false, "secret": false, "description": "Days without git activity before a project is stale (default: 180)" },
            { "name": "NO_COLOR", "required": false, "secret": false, "description": "Disable ANSI colors in pretty output" },
        ],
        "configFile": {
            "path": "~/.config/disk-sweep/config.toml",
            "fields": {
                "llm.model": "OpenRouter model override for review",
                "llm.openrouter_api_key": "OpenRouter API key"
            }
        }
    })
}
