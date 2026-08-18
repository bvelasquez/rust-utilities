//! JSONL transport for spawning and driving `pi --mode rpc` agents.
//!
//! The daemon owns one task per agent: the task holds the child process and its
//! stdin/stdout, parses JSONL events into [`PiEvent`], and serializes outgoing
//! RPC commands through stdin. This mirrors the protocol documented in
//! pi's docs/rpc.md.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;

pub struct SpawnedPi {
    pub child: Child,
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
    pub stderr: tokio::process::ChildStderr,
}

/// Spawn `pi --mode rpc` with the given cwd/env. Model is passed via CLI flag
/// (keeps the worker's session state independent of commander-side guessing).
pub async fn spawn_pi(
    pi_bin: &str,
    cwd: &Path,
    extra_env: &BTreeMap<String, String>,
    model: Option<&str>,
) -> Result<SpawnedPi> {
    let mut cmd = tokio::process::Command::new(pi_bin);
    cmd.arg("--mode")
        .arg("rpc")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(m) = model {
        if !m.is_empty() {
            cmd.arg("--model").arg(m);
        }
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().context("spawn pi --mode rpc")?;
    let stdin = child.stdin.take().context("pi stdin")?;
    let stdout = child.stdout.take().context("pi stdout")?;
    let stderr = child.stderr.take().context("pi stderr")?;
    Ok(SpawnedPi {
        child,
        stdin,
        stdout,
        stderr,
    })
}

/// A single parsed JSON line from an agent's stdout.
#[derive(Debug, Clone)]
pub enum PiEvent {
    Response {
        command: String,
        success: bool,
        data: Value,
        error: Option<String>,
    },
    AgentStart,
    AgentEnd {
        will_retry: bool,
    },
    AgentSettled,
    TurnStart,
    TurnEnd,
    MessageUpdate {
        delta_type: String,
        text: Option<String>,
        tool_name: Option<String>,
    },
    MessageEnd {
        message: Value,
    },
    ToolExecStart {
        call_id: String,
        name: String,
        args: Value,
    },
    ToolExecUpdate {
        call_id: String,
        name: String,
        /// Accumulated partial result (not a delta) — used sparingly in the transcript.
        partial: Value,
    },
    ToolExecEnd {
        call_id: String,
        name: String,
        is_error: bool,
        result: Value,
    },
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    CompactionStart {
        reason: String,
    },
    CompactionEnd {
        reason: String,
        aborted: bool,
        will_retry: bool,
    },
    AutoRetryStart {
        attempt: u64,
        max_attempts: u64,
        delay_ms: u64,
        error_message: String,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u64,
        final_error: Option<String>,
    },
    ExtensionError {
        error: String,
    },
    BashExecUpdate {
        delta: String,
    },
    Unknown(Value),
}

pub fn parse_line(line: &str) -> Option<PiEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let ty = v.get("type")?.as_str()?;
    use PiEvent::*;
    let event = match ty {
        "response" => Response {
            command: v
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("?")
                .to_string(),
            success: v
                .get("success")
                .and_then(|s| s.as_bool())
                .unwrap_or(false),
            data: v.get("data").cloned().unwrap_or(Value::Null),
            error: v
                .get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string()),
        },
        "agent_start" => AgentStart,
        "agent_end" => AgentEnd {
            will_retry: v
                .get("willRetry")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
        },
        "agent_settled" => AgentSettled,
        "turn_start" => TurnStart,
        "turn_end" => TurnEnd,
        "message_update" => {
            // assistantMessageEvent: {type, contentIndex, delta?, content?, toolName?}
            let ev = v
                .get("assistantMessageEvent")
                .cloned()
                .unwrap_or(Value::Null);
            let delta_type = ev
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("?")
                .to_string();
            let text = delta_text(&delta_type, &ev);
            MessageUpdate {
                delta_type,
                text: if text.is_empty() {
                    None
                } else {
                    Some(text)
                },
                tool_name: ev.get("toolName").and_then(|t| t.as_str()).map(|s| s.to_string()),
            }
        }
        "message_end" => MessageEnd {
            message: v.get("message").cloned().unwrap_or(Value::Null),
        },
        "tool_execution_start" => ToolExecStart {
            call_id: v
                .get("toolCallId")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            name: v
                .get("toolName")
                .and_then(|n| n.as_str())
                .unwrap_or("?")
                .to_string(),
            args: v.get("args").cloned().unwrap_or(Value::Null),
        },
        "tool_execution_update" => ToolExecUpdate {
            call_id: v
                .get("toolCallId")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            name: v
                .get("toolName")
                .and_then(|n| n.as_str())
                .unwrap_or("?")
                .to_string(),
            partial: v
                .get("partialResult")
                .cloned()
                .unwrap_or(Value::Null),
        },
        "tool_execution_end" => ToolExecEnd {
            call_id: v
                .get("toolCallId")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            name: v
                .get("toolName")
                .and_then(|n| n.as_str())
                .unwrap_or("?")
                .to_string(),
            is_error: v.get("isError").and_then(|b| b.as_bool()).unwrap_or(false),
            result: v.get("result").cloned().unwrap_or(Value::Null),
        },
        "queue_update" => QueueUpdate {
            steering: v
                .get("steering")
                .and_then(|s| s.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            follow_up: v
                .get("followUp")
                .and_then(|s| s.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        },
        "compaction_start" => CompactionStart {
            reason: v
                .get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("?")
                .to_string(),
        },
        "compaction_end" => CompactionEnd {
            reason: v
                .get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("?")
                .to_string(),
            aborted: v.get("aborted").and_then(|b| b.as_bool()).unwrap_or(false),
            will_retry: v
                .get("willRetry")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
        },
        "auto_retry_start" => AutoRetryStart {
            attempt: v.get("attempt").and_then(|a| a.as_u64()).unwrap_or(0),
            max_attempts: v
                .get("maxAttempts")
                .and_then(|a| a.as_u64())
                .unwrap_or(0),
            delay_ms: v.get("delayMs").and_then(|d| d.as_u64()).unwrap_or(0),
            error_message: v
                .get("errorMessage")
                .and_then(|e| e.as_str())
                .unwrap_or("?")
                .to_string(),
        },
        "auto_retry_end" => AutoRetryEnd {
            success: v.get("success").and_then(|b| b.as_bool()).unwrap_or(false),
            attempt: v.get("attempt").and_then(|a| a.as_u64()).unwrap_or(0),
            final_error: v
                .get("finalError")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string()),
        },
        "extension_error" => ExtensionError {
            error: v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("?")
                .to_string(),
        },
        "bash_execution_update" => BashExecUpdate {
            delta: v.get("delta").and_then(|d| d.as_str()).unwrap_or("").to_string(),
        },
        other => Unknown(v),
    };
    Some(event)
}

fn delta_text(delta_type: &str, ev: &Value) -> String {
    if delta_type == "text_delta" {
        ev.get("delta")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string()
    } else if delta_type == "text_end" {
        ev.get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    }
}

/// Write a command line to an agent's stdin. Commands are JSONL.
pub async fn write_command(writer: &mut tokio::process::ChildStdin, cmd: &Value) -> Result<()> {
    let mut line = serde_json::to_string(cmd)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

/// Read stderr into a buffer (kept separately for diagnostics).
pub async fn drain_stderr(reader: tokio::process::ChildStderr, buf: &mut Vec<String>) {
    let mut r = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match r.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let l = line.trim_end().to_string();
                if !l.is_empty() {
                    buf.push(l);
                    if buf.len() > 200 {
                        buf.remove(0);
                    }
                }
            }
        }
    }
}