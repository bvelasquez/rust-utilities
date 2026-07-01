use serde_json::json;

use crate::print;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "storeshots",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Unified marketing asset CLI: brand boards, App Store screenshots, print (coming soon)",
        "commands": [
            { "id": "init", "mutation": true, "requiresAuth": false },
            { "id": "brand extract", "mutation": true, "requiresAuth": "OPENROUTER_API_KEY or GEMINI_API_KEY" },
            { "id": "brand validate", "mutation": false, "requiresAuth": false },
            { "id": "copy suggest", "mutation": true, "requiresAuth": "OPENROUTER_API_KEY or GEMINI_API_KEY" },
            { "id": "mobile render", "mutation": true, "requiresAuth": "optional GEMINI_API_KEY for AI backgrounds" },
            { "id": "mobile validate", "mutation": false, "requiresAuth": false },
            { "id": "print formats", "mutation": false, "requiresAuth": false },
            { "id": "print suggest", "mutation": true, "requiresAuth": "OPENROUTER_API_KEY or GEMINI_API_KEY" },
            { "id": "print render", "mutation": true, "requiresAuth": false },
            { "id": "run", "mutation": true, "requiresAuth": "varies by step" },
            { "id": "interactive", "mutation": true, "requiresAuth": "varies by selection", "human": true },
            { "id": "config schema", "mutation": false },
            { "id": "env schema", "mutation": false },
        ],
        "phases": ["brand", "copy", "mobile_background", "print_copy", "validate"],
        "platforms": {
            "mobile": ["apple_iphone"],
            "print": print::format_id_slice(),
        },
        "promptPrecedence": [
            "CLI --prompt-append / --prompt-file",
            "per-item prompt_append in storeshots.toml",
            "[ai.prompts.{phase}] in storeshots.toml",
            "storeshots/prompts/{phase}.append.md auto-discovered"
        ],
        "apiKeys": {
            "secretsFile": "storeshots/secrets.toml (gitignored)",
            "configKeys": "storeshots config keys --json",
            "resolution": "secrets.toml > [ai.keys].*_env > STORESHOTS_* > legacy global env"
        }
    })
}

pub fn config_schema_json() -> serde_json::Value {
    json!({
        "file": "storeshots.toml",
        "sections": {
            "app": { "name": "string", "kind": "mobile-app|company-site|saas", "bundle_id": "optional string" },
            "paths": { "brand": "docs/BRAND.md", "web_root": ".", "prompts_dir": "storeshots/prompts" },
            "brand": { "accent": "hex", "background": "hex", "foreground": "hex", "theme": "string" },
            "ai": {
                "text_provider": "openrouter|gemini",
                "text_model": "string",
                "image_model": "string",
                "keys": {
                    "secrets_file": "storeshots/secrets.toml (gitignored)",
                    "openrouter_env": "optional env var NAME for project OpenRouter key",
                    "gemini_env": "optional env var NAME for project Gemini key"
                },
                "prompts": { "{phase}": { "model": "optional", "prompt_append": "multiline", "prompt_files": ["paths"] } }
            },
            "pipeline.steps": [{ "id": "string", "phase": "brand|copy|mobile|print", "enabled": "bool", "depends_on": ["ids"] }],
            "slides.items": [{ "id": "string", "raw": "filename in storeshots/raw/", "title": "string", "subtitle": "string", "label": "string", "prompt_append": "optional" }]
        }
    })
}
