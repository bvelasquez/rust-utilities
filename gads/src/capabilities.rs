use serde_json::json;

use crate::api::API_VERSION;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "gads",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-first Google Ads CLI — GAQL reads, REST mutates, shortcuts, interactive TUI",
        "apiVersion": API_VERSION,
        "commands": [
            { "id": "auth login", "mutation": true, "requiresAuth": false },
            { "id": "auth status", "mutation": false, "requiresAuth": false },
            { "id": "customers", "mutation": false, "requiresAuth": true },
            { "id": "customer", "mutation": false, "requiresAuth": true },
            { "id": "account-hierarchy", "mutation": false, "requiresAuth": true },
            { "id": "campaigns", "mutation": false, "requiresAuth": true },
            { "id": "campaign", "mutation": false, "requiresAuth": true },
            { "id": "campaign-budgets", "mutation": false, "requiresAuth": true },
            { "id": "ad-groups", "mutation": false, "requiresAuth": true },
            { "id": "ad-group", "mutation": false, "requiresAuth": true },
            { "id": "ads", "mutation": false, "requiresAuth": true },
            { "id": "ad", "mutation": false, "requiresAuth": true },
            { "id": "campaign-stats", "mutation": false, "requiresAuth": true },
            { "id": "ad-group-stats", "mutation": false, "requiresAuth": true },
            { "id": "ad-stats", "mutation": false, "requiresAuth": true },
            { "id": "keyword-stats", "mutation": false, "requiresAuth": true },
            { "id": "keywords", "mutation": false, "requiresAuth": true },
            { "id": "audiences", "mutation": false, "requiresAuth": true },
            { "id": "user-lists", "mutation": false, "requiresAuth": true },
            { "id": "negative-keywords", "mutation": false, "requiresAuth": true },
            { "id": "assets", "mutation": false, "requiresAuth": true },
            { "id": "extensions", "mutation": false, "requiresAuth": true },
            { "id": "conversion-actions", "mutation": false, "requiresAuth": true },
            { "id": "query", "mutation": false, "requiresAuth": true },
            { "id": "billing", "mutation": false, "requiresAuth": true },
            { "id": "change-status", "mutation": false, "requiresAuth": true },
            { "id": "summary", "mutation": false, "requiresAuth": true, "shortcut": true },
            { "id": "conversion-tags", "mutation": false, "requiresAuth": true, "shortcut": true },
            { "id": "mutate", "mutation": true, "requiresAuth": true },
            { "id": "mutate-batch", "mutation": true, "requiresAuth": true },
            { "id": "campaign get", "mutation": false, "requiresAuth": true },
            { "id": "campaign set-status", "mutation": true, "requiresAuth": true },
            { "id": "campaign create-search", "mutation": true, "requiresAuth": true },
            { "id": "ad-group get", "mutation": false, "requiresAuth": true },
            { "id": "ad-group create", "mutation": true, "requiresAuth": true },
            { "id": "ad-group set-status", "mutation": true, "requiresAuth": true },
            { "id": "ad get", "mutation": false, "requiresAuth": true },
            { "id": "ad create-rsa", "mutation": true, "requiresAuth": true },
            { "id": "ad set-status", "mutation": true, "requiresAuth": true },
            { "id": "budget create", "mutation": true, "requiresAuth": true },
            { "id": "keyword add", "mutation": true, "requiresAuth": true },
            { "id": "interactive", "mutation": true, "requiresAuth": true, "human": true },
            { "id": "capabilities", "mutation": false },
            { "id": "env schema", "mutation": false },
        ],
        "mutationSafety": "Mutations require --yes, --dry-run, or TTY confirmation",
        "credentials": {
            "defaultPath": "~/.config/gads/credentials.json",
            "legacyPath": "~/.config/google-ads-open-cli/credentials.json",
            "env": ["GOOGLE_ADS_ACCESS_TOKEN", "GOOGLE_ADS_DEVELOPER_TOKEN", "GOOGLE_ADS_LOGIN_CUSTOMER_ID", "GADS_* aliases"]
        },
        "parity": "Read commands match google-ads-open-cli with v24 GAQL field fixes"
    })
}

pub fn env_schema_json() -> serde_json::Value {
    json!({
        "precedence": ["CLI flags", "environment variables", "credentials.json", "gads.toml project aliases"],
        "variables": [
            { "name": "GOOGLE_ADS_ACCESS_TOKEN", "aliases": ["GADS_ACCESS_TOKEN"], "required": "or credentials.json", "secret": true },
            { "name": "GOOGLE_ADS_DEVELOPER_TOKEN", "aliases": ["GADS_DEVELOPER_TOKEN"], "required": true, "secret": true },
            { "name": "GOOGLE_ADS_LOGIN_CUSTOMER_ID", "aliases": ["GADS_LOGIN_CUSTOMER_ID"], "required": "MCC access only", "secret": false },
            { "name": "NO_COLOR", "aliases": [], "required": false, "secret": false }
        ],
        "projectFile": {
            "path": "gads.toml (walk up from cwd)",
            "fields": {
                "default_customer_id": "optional default for commands",
                "login_customer_id": "optional MCC login customer",
                "aliases": "map short names to customer IDs"
            }
        }
    })
}
