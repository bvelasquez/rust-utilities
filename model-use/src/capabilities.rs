use serde_json::json;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "model-use",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-first LLM cost aggregator — OpenRouter, Anthropic, OpenAI, Cursor usage and budgets",
        "commands": [
            { "id": "watch", "mutation": false, "requiresAuth": false, "description": "Interactive TUI dashboard (default in TTY)" },
            { "id": "fetch", "mutation": false, "requiresAuth": true, "description": "Pull usage from enabled providers into local cache" },
            { "id": "providers list", "mutation": false, "requiresAuth": false, "description": "List configured providers and key status" },
            { "id": "providers set", "mutation": true, "requiresAuth": false, "description": "Save provider API key to config" },
            { "id": "providers test", "mutation": false, "requiresAuth": true, "description": "Validate API key type and permissions" },
            { "id": "providers enable", "mutation": true, "requiresAuth": false, "description": "Enable a provider" },
            { "id": "providers disable", "mutation": true, "requiresAuth": false, "description": "Disable a provider" },
            { "id": "budget set", "mutation": true, "requiresAuth": false, "description": "Set global or per-provider monthly budget" },
            { "id": "budget list", "mutation": false, "requiresAuth": false, "description": "Show configured budgets" },
            { "id": "set refresh-interval", "mutation": true, "requiresAuth": false, "description": "Set TUI auto-refresh interval (default 15m; 0 disables)" },
            { "id": "set list", "mutation": false, "requiresAuth": false, "description": "Show TUI and app settings" },
            { "id": "summary", "mutation": false, "requiresAuth": false, "description": "Aggregated spend from cache" },
            { "id": "capabilities", "mutation": false, "requiresAuth": false },
            { "id": "env schema", "mutation": false, "requiresAuth": false },
        ],
        "providers": {
            "openrouter": {
                "keyType": "management",
                "docs": "https://openrouter.ai/docs/api/api-reference/analytics"
            },
            "anthropic": {
                "keyType": "admin (sk-ant-admin01-...)",
                "docs": "https://platform.claude.com/docs/en/manage-claude/usage-cost-api"
            },
            "openai": {
                "keyType": "organization admin",
                "docs": "https://developers.openai.com/api/docs/guides/admin-apis"
            },
            "cursor": {
                "keyType": "Admin API key (Teams/Enterprise)",
                "docs": "https://cursor.com/docs/account/teams/admin-api"
            }
        },
        "credentials": {
            "defaultPath": "~/.config/model-use/config.toml",
            "cachePath": "~/.config/model-use/cache.db"
        },
        "agentHints": [
            "Run `model-use capabilities --json` and `model-use env schema --json` before automation",
            "Use `model-use providers test --json` to verify admin/management keys",
            "Run `model-use fetch` before `summary` or `watch` for fresh data",
            "OpenRouter requires a management key for analytics; regular keys return 403"
        ]
    })
}

pub fn env_schema_json() -> serde_json::Value {
    json!({
        "precedence": ["CLI flags", "environment variables", "~/.config/model-use/config.toml"],
        "variables": [
            { "name": "MODEL_USE_OPENROUTER_KEY", "required": false, "secret": true, "description": "OpenRouter management API key" },
            { "name": "MODEL_USE_ANTHROPIC_KEY", "required": false, "secret": true, "description": "Anthropic admin API key (sk-ant-admin01-...)" },
            { "name": "MODEL_USE_OPENAI_KEY", "required": false, "secret": true, "description": "OpenAI organization admin API key" },
            { "name": "MODEL_USE_CURSOR_KEY", "required": false, "secret": true, "description": "Cursor Admin API key (Teams/Enterprise)" },
            { "name": "MODEL_USE_CURSOR_EMAIL", "required": false, "secret": false, "description": "Filter Cursor usage events to one team member email" },
            { "name": "MODEL_USE_CONFIG", "required": false, "secret": false, "description": "Override config file path" },
        ],
        "configFile": {
            "path": "~/.config/model-use/config.toml",
            "fields": {
                "openrouter.api_key": "OpenRouter management key",
                "anthropic.api_key": "Anthropic admin key",
                "openai.api_key": "OpenAI admin key",
                "cursor.api_key": "Cursor Admin API key",
                "cursor.email": "Optional email filter for team usage events",
                "budgets.global_monthly_usd": "Global monthly budget in USD",
                "budgets.<provider>.monthly_usd": "Per-provider monthly budget",
                "tui.refresh_interval_secs": "TUI auto-refresh interval in seconds (default 900; 0 disables)"
            }
        }
    })
}
