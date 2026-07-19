use serde_json::json;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "mail-sweep",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-first AI email triage — IMAP sync, OpenRouter classification, ratatui inbox",
        "commands": [
            { "id": "sync", "mutation": false, "requiresAuth": true, "description": "Fetch new mail into local cache via IMAP" },
            { "id": "process", "mutation": false, "requiresAuth": true, "description": "Classify pending messages with rules + OpenRouter" },
            { "id": "apply", "mutation": true, "requiresAuth": true, "description": "Execute a classification plan on IMAP (requires --yes or --dry-run in non-TTY)" },
            { "id": "list", "mutation": false, "requiresAuth": false, "description": "List cached messages" },
            { "id": "show", "mutation": false, "requiresAuth": false, "description": "Show a cached message" },
            { "id": "stats", "mutation": false, "requiresAuth": false, "description": "Email volume and category stats from cache" },
            { "id": "send", "mutation": true, "requiresAuth": true, "description": "Send email via SMTP" },
            { "id": "accounts list", "mutation": false, "requiresAuth": false },
            { "id": "accounts add", "mutation": true, "requiresAuth": false },
            { "id": "accounts test", "mutation": false, "requiresAuth": true },
            { "id": "secrets list", "mutation": false, "requiresAuth": false },
            { "id": "secrets set openrouter-key", "mutation": true, "requiresAuth": false },
            { "id": "secrets set account", "mutation": true, "requiresAuth": false },
            { "id": "rules list", "mutation": false, "requiresAuth": false },
            { "id": "rules add", "mutation": true, "requiresAuth": false },
            { "id": "rules update", "mutation": true, "requiresAuth": false },
            { "id": "rules remove", "mutation": true, "requiresAuth": false },
            { "id": "rules test", "mutation": false, "requiresAuth": false },
            { "id": "rules audit", "mutation": false, "requiresAuth": true, "description": "AI review of rules — suggests merges and generalizations (--yes to apply)" },
            { "id": "learn feedback", "mutation": true, "requiresAuth": false },
            { "id": "interactive", "mutation": false, "requiresAuth": false, "description": "Interactive ratatui TUI (default in TTY)" },
            { "id": "capabilities", "mutation": false, "requiresAuth": false },
            { "id": "config schema", "mutation": false, "requiresAuth": false },
        ],
        "categories": [
            "priority", "personal", "work", "newsletter", "marketing",
            "notification", "receipt", "spam", "unknown"
        ],
        "actions": [
            "keep", "mark_read", "flag", "unflag", "archive", "move", "delete", "tag"
        ],
        "agentHints": [
            "Run `mail-sweep capabilities --json` and `mail-sweep config schema --json` before automation",
            "Configure accounts in ~/.config/mail-sweep/config.toml",
            "Store API keys and passwords with `mail-sweep secrets set ...` or secrets.toml / .env",
            "Use `mail-sweep sync --json` then `mail-sweep process --dry-run --json` before `apply --yes --json`",
            "IMAP reads/syncs; SMTP sends. Both configured per account.",
            "Mutations require `--yes` or `--dry-run` when stdout is not a TTY",
            "Deletes require `safety.allow_delete = true` or `--allow-delete`",
            "Rule patterns: subject:/from:/domain:/header:/body:/has:list-unsubscribe/all:A+B",
            "Run `mail-sweep rules audit --json` to review AI merge suggestions before `--yes`"
        ]
    })
}

pub fn config_schema_json() -> serde_json::Value {
    json!({
        "precedence": ["CLI flags", "secrets.toml", "config.toml", ".env files"],
        "configFile": {
            "path": "~/.config/mail-sweep/config.toml",
            "fields": {
                "llm.model": "OpenRouter model override (non-secret)",
                "sync.poll_interval": "Background sync interval",
                "sync.batch_size": "Sender groups per AI pattern batch (not per-email)",
                "safety.allow_delete": "Allow IMAP delete actions",
                "accounts": "Multi-account IMAP/SMTP settings (hosts, email, folders)"
            }
        },
        "secretsFile": {
            "path": "~/.config/mail-sweep/secrets.toml",
            "fields": {
                "openrouter_api_key": "OpenRouter API key for classification",
                "llm_model": "Default OpenRouter model",
                "accounts.<id>": "Per-account IMAP/SMTP password"
            },
            "example": {
                "openrouter_api_key": "sk-or-v1-...",
                "llm_model": "openai/gpt-4o-mini",
                "accounts": { "personal": "your-app-password" }
            }
        },
        "dotenvFiles": [
            "./.env",
            "~/.config/mail-sweep/.env"
        ],
        "dotenvKeys": [
            { "key": "openrouter_api_key", "secret": true, "description": "OpenRouter API key" },
            { "key": "llm_model", "secret": false, "description": "LLM model override" },
            { "key": "account_<id>_password", "secret": true, "description": "Account password (id uses underscores, e.g. account_personal_password)" }
        ],
        "cliSecrets": [
            "mail-sweep secrets set-openrouter-key --key <key>",
            "mail-sweep secrets set-llm-model --model <model>",
            "mail-sweep secrets set-account --id <id> --password <pass>",
            "mail-sweep accounts add --id <id> --email <email> --password <pass> --gmail"
        ]
    })
}
