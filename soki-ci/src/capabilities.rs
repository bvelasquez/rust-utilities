use serde_json::{json, Value};

use crate::env_schema;

fn error_body_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "error": { "type": "string" }
        },
        "required": ["error"],
        "additionalProperties": false
    })
}

fn job_schema() -> Value {
    json!({
        "type": "object",
        "description": "Deploy job in the local soki-ci process (TUI and API share one store)",
        "properties": {
            "id": { "type": "string", "format": "uuid", "description": "Job id" },
            "project_id": { "type": "string" },
            "project_name": { "type": "string" },
            "target_id": { "type": "string", "description": "Deploy target key from projects.yaml (API build_id)" },
            "target_label": { "type": "string" },
            "phase": {
                "type": "string",
                "enum": ["queued", "running", "success", "error", "cancelled", "interrupted"]
            },
            "created_at": { "type": "string", "format": "date-time" },
            "started_at": { "type": ["string", "null"], "format": "date-time" },
            "finished_at": { "type": ["string", "null"], "format": "date-time" },
            "exit_code": { "type": ["integer", "null"] },
            "duration_ms": { "type": ["integer", "null"] },
            "source": { "type": "string", "description": "tui | cli | api | web" },
            "runner": { "type": "string" },
            "cwd": { "type": "string" },
            "log_path": { "type": "string", "description": "Path to log file on disk" },
            "interrupted": { "type": "boolean" },
            "cancel_requested": { "type": "boolean" },
            "can_cancel": { "type": "boolean" },
            "can_input": { "type": "boolean" },
            "log_size_bytes": { "type": "integer" }
        },
        "required": [
            "id", "project_id", "project_name", "target_id", "target_label",
            "phase", "created_at", "started_at", "finished_at", "exit_code",
            "duration_ms", "source", "runner", "cwd", "log_path", "interrupted",
            "cancel_requested", "can_cancel", "can_input", "log_size_bytes"
        ],
        "additionalProperties": false
    })
}

fn http_endpoints() -> Value {
    let error = error_body_schema();
    let job = job_schema();
    json!([
        {
            "method": "GET",
            "path": "/health",
            "authentication": false,
            "description": "Liveness check; no auth even when SOKI_CI_API_TOKEN is set",
            "request": {
                "pathParameters": [],
                "queryParameters": [],
                "headers": [],
                "body": null
            },
            "responses": [
                {
                    "status": 200,
                    "description": "Service is up",
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "ok": { "type": "boolean", "const": true },
                            "service": { "type": "string", "const": "soki-ci" }
                        },
                        "required": ["ok", "service"],
                        "additionalProperties": false
                    },
                    "example": { "ok": true, "service": "soki-ci" }
                }
            ]
        },
        {
            "method": "GET",
            "path": "/capabilities",
            "authentication": false,
            "description": "Agent manifest — HTTP routes, JSON schemas, CLI, env, and workflow hints (this document)",
            "request": {
                "pathParameters": [],
                "queryParameters": [],
                "headers": [],
                "body": null
            },
            "responses": [
                {
                    "status": 200,
                    "description": "Full capabilities manifest",
                    "contentType": "application/json",
                    "schema": { "type": "object", "description": "Same structure as this capabilities document" }
                }
            ]
        },
        {
            "method": "GET",
            "path": "/projects/builds",
            "authentication": true,
            "description": "List configured projects and deploy targets (builds) from projects.yaml",
            "request": {
                "pathParameters": [],
                "queryParameters": [],
                "headers": [],
                "body": null
            },
            "responses": [
                {
                    "status": 200,
                    "description": "Project catalog",
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "projects": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "name": { "type": "string" },
                                        "path": { "type": "string" },
                                        "builds": {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "id": { "type": "string", "description": "Target key — use as build_id in POST" },
                                                    "label": { "type": "string" }
                                                },
                                                "required": ["id", "label"],
                                                "additionalProperties": false
                                            }
                                        }
                                    },
                                    "required": ["id", "name", "path", "builds"],
                                    "additionalProperties": false
                                }
                            }
                        },
                        "required": ["projects"],
                        "additionalProperties": false
                    },
                    "example": {
                        "projects": [
                            {
                                "id": "my-app",
                                "name": "My App",
                                "path": "/path/to/repo",
                                "builds": [
                                    { "id": "hosting", "label": "Firebase Hosting" }
                                ]
                            }
                        ]
                    }
                },
                { "status": 401, "description": "Missing or invalid Bearer token", "schema": error },
                { "status": 500, "description": "Config load failure", "schema": error }
            ]
        },
        {
            "method": "POST",
            "path": "/projects/{project_id}/builds/{build_id}",
            "authentication": true,
            "description": "Queue a deploy job for project_id and target build_id (YAML target key)",
            "request": {
                "pathParameters": [
                    { "name": "project_id", "type": "string", "description": "Project id from projects.yaml" },
                    { "name": "build_id", "type": "string", "description": "Deploy target id (same as soki-ci deploy run -t)" }
                ],
                "queryParameters": [],
                "headers": [],
                "body": null
            },
            "responses": [
                {
                    "status": 202,
                    "description": "Job queued",
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "job_id": { "type": "string", "format": "uuid" },
                            "project_id": { "type": "string" },
                            "build_id": { "type": "string" },
                            "status": { "type": "string", "const": "queued" }
                        },
                        "required": ["job_id", "project_id", "build_id", "status"],
                        "additionalProperties": false
                    },
                    "example": {
                        "job_id": "550e8400-e29b-41d4-a716-446655440000",
                        "project_id": "my-app",
                        "build_id": "hosting",
                        "status": "queued"
                    }
                },
                { "status": 401, "description": "Missing or invalid Bearer token", "schema": error },
                { "status": 404, "description": "Unknown project_id or build_id", "schema": error },
                { "status": 409, "description": "Project already has a queued or running deploy (deploys are serialized per project, since targets of one project share a working tree)", "schema": error },
                { "status": 500, "description": "Enqueue or config error", "schema": error }
            ]
        },
        {
            "method": "GET",
            "path": "/jobs",
            "authentication": true,
            "description": "List active and recent jobs in this process",
            "request": {
                "pathParameters": [],
                "queryParameters": [],
                "headers": [],
                "body": null
            },
            "responses": [
                {
                    "status": 200,
                    "description": "Job list",
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "jobs": { "type": "array", "items": job }
                        },
                        "required": ["jobs"],
                        "additionalProperties": false
                    }
                },
                { "status": 401, "description": "Missing or invalid Bearer token", "schema": error }
            ]
        },
        {
            "method": "GET",
            "path": "/jobs/{job_id}",
            "authentication": true,
            "description": "Get one job by UUID",
            "request": {
                "pathParameters": [
                    { "name": "job_id", "type": "string", "format": "uuid" }
                ],
                "queryParameters": [],
                "headers": [],
                "body": null
            },
            "responses": [
                {
                    "status": 200,
                    "description": "Job record",
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "job": job
                        },
                        "required": ["job"],
                        "additionalProperties": false
                    }
                },
                { "status": 400, "description": "Invalid job_id", "schema": error },
                { "status": 401, "description": "Missing or invalid Bearer token", "schema": error },
                { "status": 404, "description": "Job not found in this process", "schema": error }
            ]
        },
        {
            "method": "POST",
            "path": "/jobs/{job_id}/input",
            "authentication": true,
            "description": "Write text to a running job's stdin (interactive prompts, e.g. y/N)",
            "request": {
                "pathParameters": [
                    { "name": "job_id", "type": "string", "format": "uuid" }
                ],
                "queryParameters": [],
                "headers": [
                    { "name": "Content-Type", "required": true, "value": "application/json" }
                ],
                "body": {
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string", "description": "Line to write to stdin (newline appended)" }
                        },
                        "required": ["text"],
                        "additionalProperties": false
                    },
                    "example": { "text": "y" }
                }
            },
            "responses": [
                {
                    "status": 200,
                    "description": "Input accepted",
                    "contentType": "application/json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "ok": { "type": "boolean", "const": true },
                            "job_id": { "type": "string", "format": "uuid" }
                        },
                        "required": ["ok", "job_id"],
                        "additionalProperties": false
                    },
                    "example": { "ok": true, "job_id": "550e8400-e29b-41d4-a716-446655440000" }
                },
                { "status": 400, "description": "Invalid job_id", "schema": error },
                { "status": 401, "description": "Missing or invalid Bearer token", "schema": error },
                { "status": 404, "description": "Job not found", "schema": error },
                { "status": 409, "description": "Job not running or stdin not available", "schema": error },
                { "status": 500, "description": "Internal error", "schema": error }
            ]
        },
        {
            "method": "GET",
            "path": "/api/status",
            "authentication": true,
            "description": "Rich service status: version, uptime, bind, config path, queue limits, project locks, TTS health",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Service status", "contentType": "application/json", "schema": { "type": "object" } }]
        },
        {
            "method": "GET",
            "path": "/api/projects",
            "authentication": true,
            "description": "Project/target catalog with runner metadata, env keys, danger flags, stats, and active/last jobs",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Project catalog", "contentType": "application/json", "schema": { "type": "object" } }]
        },
        {
            "method": "GET",
            "path": "/api/projects/{project_id}",
            "authentication": true,
            "description": "One enriched project record",
            "request": { "pathParameters": [{ "name": "project_id", "type": "string" }], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Project", "schema": { "type": "object" } }, { "status": 404, "description": "Unknown project", "schema": error }]
        },
        {
            "method": "POST",
            "path": "/api/projects/{project_id}/builds/{build_id}",
            "authentication": true,
            "description": "Queue a deploy from the web UI (source=web)",
            "request": { "pathParameters": [{ "name": "project_id", "type": "string" }, { "name": "build_id", "type": "string" }], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 202, "description": "Queued", "schema": { "type": "object" } }, { "status": 409, "description": "Project already busy", "schema": error }]
        },
        {
            "method": "GET",
            "path": "/api/jobs",
            "authentication": true,
            "description": "Paginated, filterable job history (project, target, phase, source, q, limit, offset)",
            "request": { "pathParameters": [], "queryParameters": [{ "name": "project" }, { "name": "target" }, { "name": "phase" }, { "name": "source" }, { "name": "q" }, { "name": "limit" }, { "name": "offset" }], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Jobs page + stats", "schema": { "type": "object" } }]
        },
        {
            "method": "POST",
            "path": "/api/jobs/reset",
            "authentication": true,
            "description": "Delete finished jobs and log files; active jobs are kept",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Removed count", "schema": { "type": "object" } }]
        },
        {
            "method": "GET",
            "path": "/api/jobs/{job_id}",
            "authentication": true,
            "description": "One job with computed can_cancel/can_input/log_size_bytes",
            "request": { "pathParameters": [{ "name": "job_id", "type": "string", "format": "uuid" }], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Job", "schema": { "type": "object" } }, { "status": 404, "description": "Not found", "schema": error }]
        },
        {
            "method": "GET",
            "path": "/api/jobs/{job_id}/logs",
            "authentication": true,
            "description": "Log lines (JSON or text). Query: tail, format=text, download=true",
            "request": { "pathParameters": [{ "name": "job_id", "type": "string" }], "queryParameters": [{ "name": "tail" }, { "name": "format" }, { "name": "download" }], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Log lines or raw file", "schema": { "type": "object" } }, { "status": 404, "description": "Not found", "schema": error }]
        },
        {
            "method": "GET",
            "path": "/api/jobs/{job_id}/events",
            "authentication": true,
            "description": "Server-Sent Events stream: snapshot, log, resync, job, removed, heartbeat",
            "request": { "pathParameters": [{ "name": "job_id", "type": "string" }], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "text/event-stream" }]
        },
        {
            "method": "POST",
            "path": "/api/jobs/{job_id}/cancel",
            "authentication": true,
            "description": "Cancel a queued/running job; running jobs get SIGTERM then SIGKILL after a grace period",
            "request": { "pathParameters": [{ "name": "job_id", "type": "string" }], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 202, "description": "Cancellation requested", "schema": { "type": "object" } }, { "status": 409, "description": "Job not running", "schema": error }]
        },
        {
            "method": "POST",
            "path": "/api/jobs/{job_id}/retry",
            "authentication": true,
            "description": "Re-enqueue the same project/target, returning a new job id",
            "request": { "pathParameters": [{ "name": "job_id", "type": "string" }], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 202, "description": "Retry queued", "schema": { "type": "object" } }, { "status": 409, "description": "Project busy", "schema": error }]
        },
        {
            "method": "POST",
            "path": "/api/jobs/{job_id}/input",
            "authentication": true,
            "description": "Write text to a running job's stdin (same as the existing /jobs route)",
            "request": { "pathParameters": [{ "name": "job_id", "type": "string" }], "queryParameters": [], "headers": [{ "name": "Content-Type", "required": true, "value": "application/json" }], "body": { "contentType": "application/json", "schema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] } } },
            "responses": [{ "status": 200, "description": "Input accepted", "schema": { "type": "object" } }, { "status": 409, "description": "Not running", "schema": error }]
        },
        {
            "method": "GET",
            "path": "/api/events",
            "authentication": true,
            "description": "Global Server-Sent Events stream: snapshot, job, removed, config, heartbeat",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "text/event-stream" }]
        },
        {
            "method": "GET",
            "path": "/api/config",
            "authentication": true,
            "description": "Resolved config, validation issues, and optional raw YAML (?raw=true)",
            "request": { "pathParameters": [], "queryParameters": [{ "name": "raw" }], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Config", "schema": { "type": "object" } }]
        },
        {
            "method": "GET",
            "path": "/api/config/validate",
            "authentication": true,
            "description": "Validate config paths, runners, and package scripts",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Validation issues", "schema": { "type": "object" } }]
        },
        {
            "method": "POST",
            "path": "/api/config/reload",
            "authentication": true,
            "description": "Reload projects.yaml and broadcast a config event",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Reloaded config", "schema": { "type": "object" } }]
        },
        {
            "method": "PUT",
            "path": "/api/config",
            "authentication": true,
            "description": "Write YAML (requires --allow-config-write); validates, backs up, and reloads",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": { "contentType": "application/json", "schema": { "type": "object", "properties": { "yaml": { "type": "string" } }, "required": ["yaml"] } } },
            "responses": [{ "status": 200, "description": "Saved or validation errors", "schema": { "type": "object" } }, { "status": 403, "description": "Writes disabled", "schema": error }]
        },
        {
            "method": "GET",
            "path": "/api/tts",
            "authentication": true,
            "description": "TTS broker settings and reachability",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "TTS status", "schema": { "type": "object" } }]
        },
        {
            "method": "POST",
            "path": "/api/tts/test",
            "authentication": true,
            "description": "Speak a test phrase through the broker",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": { "contentType": "application/json", "schema": { "type": "object", "properties": { "text": { "type": "string" } } } } },
            "responses": [{ "status": 200, "description": "Spoken", "schema": { "type": "object" } }, { "status": 502, "description": "Broker error", "schema": error }]
        },
        {
            "method": "GET",
            "path": "/api/session",
            "authentication": false,
            "description": "Browser auth state; returns CSRF token for cookie sessions",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "Session state", "schema": { "type": "object" } }]
        },
        {
            "method": "POST",
            "path": "/api/session",
            "authentication": false,
            "description": "Exchange SOKI_CI_API_TOKEN for an HttpOnly session cookie",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": { "contentType": "application/json", "schema": { "type": "object", "properties": { "token": { "type": "string" } }, "required": ["token"] } } },
            "responses": [{ "status": 200, "description": "Logged in", "schema": { "type": "object" } }, { "status": 401, "description": "Invalid token", "schema": error }]
        },
        {
            "method": "DELETE",
            "path": "/api/session",
            "authentication": false,
            "description": "Log out and clear the browser session cookie",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 204, "description": "Logged out" }]
        },
        {
            "method": "GET",
            "path": "/",
            "authentication": false,
            "description": "Embedded React web dashboard (SPA). Data routes require auth when a token is configured.",
            "request": { "pathParameters": [], "queryParameters": [], "headers": [], "body": null },
            "responses": [{ "status": 200, "description": "text/html" }]
        }
    ])
}

fn cli_commands() -> Value {
    json!([
        {
            "id": "watch",
            "mutation": false,
            "description": "Interactive deploy dashboard (default in TTY); also serves HTTP API unless --no-api",
            "flags": ["--bind", "--api-token", "--no-api", "--config", "--json"]
        },
        {
            "id": "projects list",
            "mutation": false,
            "description": "List projects and deploy targets from YAML",
            "flags": ["--config", "--json"]
        },
        {
            "id": "config init",
            "mutation": true,
            "description": "Create user-wide projects.yaml from example",
            "flags": ["--config", "--json"]
        },
        {
            "id": "config validate",
            "mutation": false,
            "description": "Validate paths and package.json scripts",
            "flags": ["--config", "--json"]
        },
        {
            "id": "config path",
            "mutation": false,
            "description": "Print active config path",
            "flags": ["--config", "--json"]
        },
        {
            "id": "deploy run",
            "mutation": true,
            "description": "Start deploy job(s); requires --yes in automation",
            "flags": ["-p/--project", "-t/--target", "--all", "--yes", "--wait", "--config", "--json"]
        },
        {
            "id": "jobs list",
            "mutation": false,
            "description": "Active and recent jobs in this process",
            "flags": ["--config", "--json"]
        },
        {
            "id": "jobs logs",
            "mutation": false,
            "description": "Read job log file",
            "flags": ["-i/--id", "--tail", "--config", "--json"]
        },
        {
            "id": "jobs input",
            "mutation": true,
            "description": "Write a line to a running job's stdin; uses local API when not in same process",
            "flags": ["-i/--id", "--text", "--yes", "--bind", "--api-token", "--json"]
        },
        {
            "id": "jobs cancel",
            "mutation": true,
            "description": "Cancel a queued/running job; running process groups get SIGTERM then SIGKILL",
            "flags": ["-i/--id", "--yes", "--config", "--json"]
        },
        {
            "id": "jobs reset",
            "mutation": true,
            "description": "Remove finished deployments and log files from persistent history",
            "flags": ["--yes", "--config", "--json"]
        },
        {
            "id": "serve",
            "mutation": false,
            "description": "HTTP API + web dashboard (no TUI) — list and trigger deploys",
            "flags": ["--bind", "--api-token", "--config", "--web-dir", "--allow-config-write", "--open"]
        },
        {
            "id": "web",
            "mutation": false,
            "description": "Alias for serve: web dashboard + API, optionally opening the browser",
            "flags": ["--bind", "--api-token", "--config", "--web-dir", "--allow-config-write", "--open"]
        },
        {
            "id": "capabilities",
            "mutation": false,
            "description": "Print this manifest (CLI); same data as GET /capabilities",
            "flags": ["--json"]
        },
        {
            "id": "env schema",
            "mutation": false,
            "description": "Environment variables and config field reference",
            "flags": ["--json"]
        }
    ])
}

/// Full agent manifest for `GET /capabilities` and `soki-ci capabilities`.
pub fn capabilities_json() -> serde_json::Value {
    json!({
        "name": "soki-ci",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Multi-project deploy control panel — parallel deploys, TUI, broker HTTP TTS",
        "http": {
            "defaultBind": "127.0.0.1:9847",
            "bindEnv": "SOKI_CI_API_BIND",
            "bindFlag": "--bind",
            "startedBy": ["soki-ci (TUI, default)", "soki-ci serve (API + web)", "soki-ci web (API + web, optional --open)"],
            "disableFlag": "--no-api",
            "sharedJobStore": "TUI and API in the same process share one in-memory job list",
            "authentication": {
                "type": "bearer",
                "header": "Authorization",
                "format": "Bearer <token>",
                "env": "SOKI_CI_API_TOKEN",
                "flag": "--api-token",
                "whenRequired": "All data routes when a token is configured; /health, /capabilities, /api/session, and the static web UI are public. Browser sessions use an HttpOnly cookie plus X-Soki-CSRF on mutations.",
                "publicRoutes": ["GET /health", "GET /capabilities", "GET/POST/DELETE /api/session", "GET / (static web UI)"]
            },
            "endpoints": http_endpoints()
        },
        "cli": {
            "binary": "soki-ci",
            "jsonEnvelope": {
                "description": "CLI --json responses wrap data in a standard envelope",
                "schema": {
                    "type": "object",
                    "properties": {
                        "success": { "type": "boolean" },
                        "command": { "type": "string" },
                        "inputs": { "type": ["object", "null"] },
                        "data": { "description": "Command-specific payload" },
                        "warnings": { "type": "array", "items": { "type": "string" } },
                        "errors": { "type": "array", "items": { "type": "string" } },
                        "next_actions": { "type": "array", "items": { "type": "string" } },
                        "timestamp": { "type": "string", "format": "date-time" }
                    },
                    "required": ["success", "command", "warnings", "errors", "next_actions", "timestamp"]
                }
            },
            "commands": cli_commands()
        },
        "environment": env_schema::env_schema_json(),
        "runners": ["pnpm_script", "npm_script", "make", "shell"],
        "configFile": {
            "scope": "user-wide only",
            "macOS": "~/Library/Application Support/com.soki-ci.soki-ci/projects.yaml",
            "linux": "~/.config/soki-ci/projects.yaml",
            "override": "SOKI_CI_CONFIG or --config"
        },
        "broker": {
            "protocol": "HTTP",
            "defaultBaseUrl": "http://127.0.0.1:18888",
            "config": "defaults.speak { enabled, base_url, voice, on } in projects.yaml",
            "envOverrides": ["TTS_BROKER_BASE_URL", "TTS_BROKER_VOICE"],
            "voice": "optional; omit to use the broker's configured default voice",
            "health": "GET /health",
            "speak": "POST /v1/tts/speak",
            "note": "Controlled by defaults.speak.enabled (set false to silence) or per-target speak: true/false. No broker-cli binary required; TTS on terminal job success/error only; broker errors are logged and do not affect job status."
        },
        "web": {
            "url": "/",
            "stack": "React 19 + TypeScript + Vite",
            "auth": "Static UI is public; data routes require the bearer token or a browser session cookie. POST /api/session exchanges the token for an HttpOnly cookie and returns a CSRF token for mutations.",
            "streaming": "SSE at /api/events (job state) and /api/jobs/{job_id}/events (logs)",
            "history": "Sidecar JSON + log files in {SOKI_CI_DATA_DIR}/runs; active jobs are marked interrupted on startup",
            "configWrite": "PUT /api/config is disabled unless --allow-config-write is set"
        },
        "workflow": {
            "discover": "GET /capabilities or GET /api/status",
            "legacyListTargets": "GET /projects/builds",
            "listTargets": "GET /api/projects",
            "triggerDeploy": "POST /api/projects/{project_id}/builds/{build_id}",
            "pollJob": "GET /api/jobs/{job_id}",
            "streamJob": "GET /api/jobs/{job_id}/events (SSE)",
            "readLogs": "GET /api/jobs/{job_id}/logs?tail=500 (or format=text)",
            "cancelJob": "POST /api/jobs/{job_id}/cancel",
            "retryJob": "POST /api/jobs/{job_id}/retry",
            "answerPrompt": "POST /api/jobs/{job_id}/input with {\"text\":\"y\"}",
            "reloadConfig": "POST /api/config/reload"
        },
        "agentHints": [
            "Start with GET /capabilities (or `soki-ci capabilities --json`) before automation",
            "Use `soki-ci config validate --json` after editing projects.yaml",
            "Deploy via CLI: `soki-ci deploy run -p <id> -t <target> --yes --json`",
            "build_id in the API equals the target key in projects.yaml",
            "Job stdin: TUI `i` in log pane, POST /api/jobs/{job_id}/input, POST /jobs/{job_id}/input, or `soki-ci jobs input -i <id> --text y --yes`",
            "Quit TUI with running jobs: warns then detaches (children keep running); API shuts down with the TUI",
            "Speak only on success/error terminal states, not on job start",
            "TTS is optional: set defaults.speak.enabled false (or per-target speak: false); use TTS_BROKER_BASE_URL for a non-default broker",
            "Web UI: open / after `soki-ci serve` or `soki-ci web`; use /api/session for browser auth when SOKI_CI_API_TOKEN is set",
            "History: persisted in runs/{job_id}.json + .log; use GET /api/jobs for paginated history and POST /api/jobs/reset to clear finished jobs"
        ]
    })
}
