use serde_json::json;

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "elabs",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-first ElevenLabs CLI — text-to-speech, speech-to-text, sound effects, voice cloning and design",
        "apiBase": "https://api.elevenlabs.io",
        "commands": [
            { "id": "apikey set", "mutation": true, "requiresAuth": false, "description": "Save API key to ~/.config/elabs/config.toml" },
            { "id": "apikey status", "mutation": false, "requiresAuth": false, "description": "Show whether API key is configured (no secret)" },
            { "id": "voices list", "mutation": false, "requiresAuth": true, "description": "List available voices (GET /v2/voices)" },
            { "id": "voices clone", "mutation": true, "requiresAuth": true, "description": "Instant voice clone from audio files (POST /v1/voices/add)" },
            { "id": "voices design", "mutation": true, "requiresAuth": true, "description": "Design voice previews from text prompt (POST /v1/text-to-voice/design)" },
            { "id": "voices save", "mutation": true, "requiresAuth": true, "description": "Save a design preview as a voice (POST /v1/text-to-voice)" },
            { "id": "tts speak", "mutation": false, "requiresAuth": true, "description": "Generate speech audio (POST /v1/text-to-speech/{voice_id})" },
            { "id": "stt transcribe", "mutation": false, "requiresAuth": true, "description": "Transcribe audio file (POST /v1/speech-to-text)" },
            { "id": "sfx create", "mutation": false, "requiresAuth": true, "description": "Generate sound effect from text prompt (POST /v1/sound-generation)" },
            { "id": "sfx list", "mutation": false, "requiresAuth": true, "description": "List generated sound effects from history (GET /v1/history)" },
            { "id": "sfx download", "mutation": false, "requiresAuth": true, "description": "Download sound effect audio from history (GET /v1/history/{id}/audio)" },
            { "id": "models list", "mutation": false, "requiresAuth": true, "description": "List TTS/STT models (GET /v1/models)" },
            { "id": "capabilities", "mutation": false, "requiresAuth": false },
            { "id": "env schema", "mutation": false, "requiresAuth": false },
        ],
        "mutationSafety": "Mutations require --yes, --dry-run, or TTY confirmation",
        "credentials": {
            "defaultPath": "~/.config/elabs/config.toml",
            "env": ["ELEVENLABS_API_KEY", "ELABS_API_KEY"]
        },
        "agentHints": [
            "Run `elabs capabilities --json` and `elabs env schema --json` before automation",
            "Use `elabs voices list --json` to discover voice_id values",
            "TTS writes binary audio to --output path; JSON envelope includes outputPath",
            "SFX flow: sfx create → sfx list → sfx download --id <history_item_id>",
            "Voice design flow: voices design → pick generated_voice_id → voices save"
        ]
    })
}

pub fn env_schema_json() -> serde_json::Value {
    json!({
        "precedence": ["CLI flags", "environment variables", "~/.config/elabs/config.toml"],
        "variables": [
            { "name": "ELEVENLABS_API_KEY", "aliases": ["ELABS_API_KEY"], "required": "or config file", "secret": true, "description": "ElevenLabs API key (xi-api-key header)" },
            { "name": "ELABS_BASE_URL", "aliases": [], "required": false, "secret": false, "description": "Override API base URL (default https://api.elevenlabs.io)" },
            { "name": "RUSTC_WRAPPER", "aliases": [], "required": false, "secret": false, "description": "Set to sccache for faster rebuilds (see AGENT.md)" },
            { "name": "NO_COLOR", "aliases": [], "required": false, "secret": false }
        ],
        "configFile": {
            "path": "~/.config/elabs/config.toml",
            "fields": {
                "api_key": "ElevenLabs API key",
                "base_url": "optional API base URL override"
            }
        }
    })
}
