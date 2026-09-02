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
                "enum": ["queued", "running", "success", "error", "cancelled"]
            },
            "started_at": { "type": ["string", "null"], "format": "date-time" },
            "finished_at": { "type": ["string", "null"], "format": "date-time" },
            "exit_code": { "type": ["integer", "null"] },
            "log_path": { "type": "string", "description": "Path to log file on disk" }
        },
        "required": [
            "id", "project_id", "project_name", "target_id", "target_label",
            "phase", "started_at", "finished_at", "exit_code", "log_path"
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
                { "status": 409, "description": "Project/target already has a queued or running deploy", "schema": error },
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
            "description": "Mark job cancelled (v1: does not kill child)",
            "flags": ["-i/--id", "--yes", "--config", "--json"]
        },
        {
            "id": "jobs reset",
            "mutation": true,
            "description": "Remove finished deployments and log files from this process",
            "flags": ["--yes", "--config", "--json"]
        },
        {
            "id": "serve",
            "mutation": false,
            "description": "HTTP API only (no TUI) — list and trigger deploys",
            "flags": ["--bind", "--api-token", "--config"]
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
            "startedBy": ["soki-ci (TUI, default)", "soki-ci serve (API only)"],
            "disableFlag": "--no-api",
            "sharedJobStore": "TUI and API in the same process share one in-memory job list",
            "authentication": {
                "type": "bearer",
                "header": "Authorization",
                "format": "Bearer <token>",
                "env": "SOKI_CI_API_TOKEN",
                "flag": "--api-token",
                "whenRequired": "All routes except GET /health and GET /capabilities when a token is configured; binding to non-loopback addresses requires a token at startup",
                "publicRoutes": ["GET /health", "GET /capabilities"]
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
            "health": "GET /health",
            "speak": "POST /v1/tts/speak",
            "note": "No broker-cli binary required; TTS on terminal job success/error only"
        },
        "workflow": {
            "discover": "GET /capabilities",
            "listTargets": "GET /projects/builds",
            "triggerDeploy": "POST /projects/{project_id}/builds/{build_id}",
            "pollJob": "GET /jobs/{job_id}",
            "answerPrompt": "POST /jobs/{job_id}/input with {\"text\":\"y\"}",
            "readLogs": "soki-ci jobs logs -i <job_id> --tail 50 --json (log_path on job record)"
        },
        "agentHints": [
            "Start with GET /capabilities (or `soki-ci capabilities --json`) before automation",
            "Use `soki-ci config validate --json` after editing projects.yaml",
            "Deploy via CLI: `soki-ci deploy run -p <id> -t <target> --yes --json`",
            "build_id in the API equals the target key in projects.yaml",
            "Job stdin: TUI `i` in log pane, POST /jobs/{job_id}/input, or `soki-ci jobs input -i <id> --text y --yes`",
            "Quit TUI with running jobs: warns then detaches (children keep running); API shuts down with the TUI",
            "Speak only on success/error terminal states, not on job start"
        ]
    })
}
