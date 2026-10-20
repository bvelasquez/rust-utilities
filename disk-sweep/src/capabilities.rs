use serde_json::{json, Value};

use crate::targets::{default_targets, host_os};

pub fn capabilities_json() -> Value {
    let default_targets: Vec<Value> = default_targets()
        .into_iter()
        .map(|target| {
            json!({
                "id": target.id,
                "category": target.category,
                "path": target.path,
                "selectedByDefault": target.selected_by_default,
                "description": target.description,
            })
        })
        .collect();

    json!({
        "name": "disk-sweep",
        "version": env!("CARGO_PKG_VERSION"),
        "os": host_os(),
        "description": "Agent-first disk cleanup, duplicate scan, and folder organizer",
        "commands": [
            command("scan", false, false, "Scan default cleanup targets and report sizes", json!([
                {"name": "--detail", "default": false, "description": "Include per-item breakdown"},
                {"name": "--json", "default": false, "description": "JSON envelope"}
            ])),
            command("interactive", false, false, "Interactive TUI. Keys: d duplicate browser, o organizer, c clean after y/n", json!([])),
            command("watch", false, false, "Live disk usage dashboard. Deep scans are manual. d opens duplicates, o opens the organizer.", json!([
                {"name": "--path", "repeatable": true},
                {"name": "--interval", "default": "0"},
                {"name": "--top", "default": 8}
            ])),
            command("analyze", false, false, "Find cleanup candidates. Library paths depend on the host OS.", json!([
                {"name": "--projects-root"},
                {"name": "--stale-days", "default": 180},
                {"name": "--skip-dot"},
                {"name": "--skip-library"}
            ])),
            command("clean", true, true, "Delete selected cache targets. A terminal must type yes. A non-terminal needs --yes or --dry-run. Symlinks are refused.", json!([
                {"name": "--targets", "repeatable": true},
                {"name": "--dry-run", "default": false},
                {"name": "--yes", "default": false}
            ])),
            command("dups", true, true, "Find duplicate files. Read-only unless --apply. Hash gates: size, xxHash64 of the first and last 64KB, full-file xxHash64, optional SHA-256. One copy is always kept. Symlinks and paths outside the scan root are refused.", json!([
                {"name": "path", "required": false, "description": "Omit in a terminal to browse folders"},
                {"name": "--no-recursive", "default": false, "description": "Recursive is the default"},
                {"name": "--sha256", "default": false},
                {"name": "--min-bytes", "default": 1},
                {"name": "--cross-devices", "default": false},
                {"name": "--exclude", "repeatable": true, "description": "Folder to leave out of the scan, including its contents"},
                {"name": "--keep", "default": "shortest", "values": ["shortest", "newest", "oldest"]},
                {"name": "--apply", "default": false, "description": "Delete extra copies. Terminal must type delete. Non-terminal also needs --yes."},
                {"name": "--yes", "default": false}
            ])),
            command("organize", true, false, "Move files from source folders using YAML rules. Date order: EXIF DateTimeOriginal, then DateTimeDigitized, then DateTime, then file created time, then modified time, then organize time. SHA-256 is checked against the organize catalog before filing; a second copy of the same content goes to dest_root/duplicates/. --apply does not re-evaluate rules. --watch cannot be combined with --plan, --dry-run, or --apply.", json!([
                {"name": "--once", "description": "Walk sources once and move immediately"},
                {"name": "--watch", "description": "Headless monitor. Moves as files arrive. No plan file."},
                {"name": "--rules", "default": "~/.config/disk-sweep/rules.yaml"},
                {"name": "--plan", "description": "Write YAML or Markdown (.md) and do not move. Same as --dry-run."},
                {"name": "--dry-run", "description": "Write YAML or Markdown and do not move. Same as --plan."},
                {"name": "--apply", "description": "Move only the recorded plan entries"}
            ])),
            command("review", false, false, "LLM review of a folder for cleanup candidates", json!([
                {"name": "path"},
                {"name": "--limit", "default": 40}
            ])),
            command("targets list", false, false, "List built-in cleanup targets for this OS", json!([])),
            command("targets explain", false, false, "Explain which paths this OS scans and what is never touched", json!([])),
            command("capabilities", false, false, "This catalog. Also available as disk-sweep --capabilities.", json!([])),
            command("env schema", false, false, "Environment variable schema", json!([]))
        ],
        "dups": {
            "hashGates": ["size", "xxh64-partial-64k", "xxh64-full", "sha256-optional"],
            "recursiveDefault": true,
            "deleteRequires": "Type delete in a terminal, or pass --yes when stdout is not a terminal. --apply is required.",
            "alwaysKeepOne": true
        },
        "organize": {
            "rulesFile": "~/.config/disk-sweep/rules.yaml",
            "ruleFields": ["name", "extensions", "filename_regex", "path"],
            "sourceFields": ["path", "dest_root", "settle_secs", "rules", "fallback"],
            "dateOrder": ["exif", "created", "modified", "now"],
            "planFormats": ["yaml", "markdown"],
            "watchIgnoresPlan": true,
            "applyDoesNotReevaluateRules": true,
            "contentCatalog": "~/.local/share/disk-sweep/organize-catalog.sqlite",
            "duplicateContentDest": "dest_root/duplicates/{filename}"
        },
        "defaultTargets": default_targets,
        "agentHints": [
            "Run `disk-sweep --capabilities` or `disk-sweep capabilities --json` before automation",
            "Use `disk-sweep scan --json` to get sizes without the TUI",
            "Use `disk-sweep dups <path> --json` for a read-only duplicate report",
            "Duplicate deletion requires `disk-sweep dups <path> --apply --yes` when stdout is not a terminal",
            "Write an organize plan with `disk-sweep organize --once --plan moves.yaml` and apply it with `disk-sweep organize --apply moves.yaml`",
            "Watch a drop folder with `disk-sweep organize --watch`. Do not combine --watch with --plan or --apply",
            "Mutations require `--yes` or `--dry-run` when stdout is not a TTY"
        ]
    })
}

fn command(id: &str, mutation: bool, tty_prompt: bool, description: &str, flags: Value) -> Value {
    json!({
        "id": id,
        "mutation": mutation,
        "requiresAuth": id == "review",
        "ttyPrompt": tty_prompt,
        "description": description,
        "flags": flags,
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
