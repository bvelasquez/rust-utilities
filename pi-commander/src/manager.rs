//! AgentManager — owns the worker pi processes: spawn, supervise, restart,
//! route commands, and aggregate live status. One tokio task per agent.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::agent::{AgentPhase, AgentRecord};
use crate::config::{ResolvedConfig, ResolvedProject};
use crate::rpc::{self, PiEvent, SpawnedPi};

// ----------------------------------------------------------------- commands

#[derive(Debug, Clone)]
pub enum AgentCommand {
    /// Prompt; `behavior` is "steer" or "followUp" (used when agent is mid-run).
    Prompt { text: String, behavior: String },
    Steer { text: String },
    FollowUp { text: String },
    Abort,
    SetModel { model: String },
    CycleModel,
    SetThinking { level: String },
    Compact,
    Bash { command: String },
    Raw { json: String },
    Stop,
}

#[derive(Debug, Clone)]
pub enum AgentNotify {
    PhaseChanged {
        id: String,
        project: String,
        old: AgentPhase,
        new: AgentPhase,
    },
    Idle {
        id: String,
        project: String,
        text: String,
    },
    Error {
        id: String,
        project: String,
        message: String,
    },
}

// ----------------------------------------------------------------- manager

#[derive(Clone)]
pub struct AgentHandle {
    pub record: Arc<Mutex<AgentRecord>>,
    pub tx: mpsc::Sender<AgentCommand>,
}

pub struct AgentManager {
    pub config: Arc<RwLock<ResolvedConfig>>,
    handles: Arc<RwLock<BTreeMap<String, AgentHandle>>>,
    notify_tx: mpsc::Sender<AgentNotify>,
    notify_rx: Arc<Mutex<Option<mpsc::Receiver<AgentNotify>>>>,
    pub started_at: std::time::Instant,
}

impl AgentManager {
    pub fn new(config: ResolvedConfig) -> Self {
        let (notify_tx, notify_rx) = mpsc::channel(512);
        Self {
            config: Arc::new(RwLock::new(config)),
            handles: Arc::new(RwLock::new(BTreeMap::new())),
            notify_tx,
            notify_rx: Arc::new(Mutex::new(Some(notify_rx))),
            started_at: std::time::Instant::now(),
        }
    }

    /// Take exclusive ownership of the notification stream (daemon wiring).
    pub async fn take_notify_rx(&self) -> Option<mpsc::Receiver<AgentNotify>> {
        self.notify_rx.lock().await.take()
    }

    pub fn notify_tx(&self) -> mpsc::Sender<AgentNotify> {
        self.notify_tx.clone()
    }

    pub async fn spawn_all(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let config = self.config.read().await;
        for p in &config.projects {
            ids.extend(self.spawn_project(p, p.agents).await?);
        }
        Ok(ids)
    }

    /// Re-read projects.yaml from disk, update in-memory config, and spawn any
    /// agents that are configured but not yet running.
    pub async fn reload_config(&self, path: &std::path::Path) -> Result<(usize, Vec<String>)> {
        let new_config = crate::config::load_config(path)?;
        crate::config::validate(&new_config)?;
        let project_count = new_config.projects.len();
        {
            let mut guard = self.config.write().await;
            *guard = new_config;
        }
        let spawned = self.spawn_all().await?;
        Ok((project_count, spawned))
    }

    pub async fn spawn_project(&self, project: &ResolvedProject, count: usize) -> Result<Vec<String>> {
        if count == 0 {
            return Ok(vec![]);
        }
        let existing = self.existing_count(&project.id).await;
        let to_spawn = count.saturating_sub(existing);
        let mut ids = Vec::new();
        for idx in existing..existing + to_spawn {
            let id = format!("{}#{}", project.id, idx);
            self.spawn_agent(project, idx).await?;
            ids.push(id);
        }
        Ok(ids)
    }

    async fn existing_count(&self, project_id: &str) -> usize {
        let prefix = format!("{project_id}#");
        self.handles
            .read()
            .await
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .count()
    }

    pub async fn spawn_one_extra(&self, project_id: &str) -> Result<String> {
        let config = self.config.read().await;
        let project = config
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .with_context(|| format!("unknown project '{project_id}'"))?
            .clone();
        drop(config);
        let idx = self.existing_count(project_id).await;
        let id = format!("{project_id}#{idx}");
        self.spawn_agent(&project, idx).await?;
        Ok(id)
    }

    /// Spawn N additional agents for a project, returing their ids.
    pub async fn spawn_project_extra(&self, project_id: &str, count: usize) -> Result<Vec<String>> {
        let config = self.config.read().await;
        let project = config
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .with_context(|| format!("unknown project '{project_id}'"))?
            .clone();
        drop(config);
        self.spawn_project(&project, count).await
    }

    async fn spawn_agent(&self, project: &ResolvedProject, idx: usize) -> Result<String> {
        let id = format!("{}#{}", project.id, idx);
        let record = Arc::new(Mutex::new(AgentRecord::new(
            id.clone(),
            project.id.clone(),
            project.display_name().to_string(),
            project.path.display().to_string(),
            idx,
        )));
        let (tx, rx) = mpsc::channel(64);
        let cfg = Arc::new(self.config.read().await.clone());
        let notify_tx = self.notify_tx.clone();
        let proj = project.clone();

        tokio::spawn(agent_supervisor(cfg, proj, idx, record.clone(), rx, notify_tx));

        self.handles
            .write()
            .await
            .insert(id.clone(), AgentHandle { record, tx });
        Ok(id)
    }

    pub async fn list_ids(&self) -> Vec<String> {
        self.handles.read().await.keys().cloned().collect()
    }

    pub async fn get(&self, id: &str) -> Option<AgentHandle> {
        self.handles.read().await.get(id).cloned()
    }

    /// Accepts "proj" or "proj#idx"; bare project id resolves to its first agent.
    pub async fn get_any(&self, id_or_project: &str) -> Option<AgentHandle> {
        let guards = self.handles.read().await;
        if let Some(h) = guards.get(id_or_project) {
            return Some(h.clone());
        }
        let prefix = format!("{id_or_project}#");
        guards
            .iter()
            .find(|(k, _)| k.starts_with(&prefix))
            .map(|(_, h)| h.clone())
    }

    pub async fn project_agents(&self, project_id: &str) -> Vec<String> {
        let prefix = format!("{project_id}#");
        self.handles
            .read()
            .await
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect()
    }

    pub async fn send_cmd(&self, id: &str, cmd: AgentCommand) -> Result<bool> {
        if let Some(h) = self.get(id).await {
            h.tx
                .send(cmd)
                .await
                .map_err(|_| anyhow::anyhow!("agent {id} is not accepting commands"))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn stop(&self, id: &str) -> Result<bool> {
        if let Some(h) = self.get(id).await {
            h.tx.send(AgentCommand::Stop).await.ok();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn shutdown_all(&self) {
        for id in self.list_ids().await {
            if let Some(h) = self.get(&id).await {
                h.tx.send(AgentCommand::Stop).await.ok();
            }
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    /// Natural-language dispatch: "fix bug A on simple-workout" -> that project's
    /// least-busy agent. Returns (worker id, effective text).
    pub async fn dispatch(&self, text: &str) -> Result<(String, String)> {
        let known: Vec<String> = self
            .config
            .read()
            .await
            .projects
            .iter()
            .map(|p| p.id.clone())
            .collect();
        let (project_id, explicit_idx, rest) = crate::router::parse_target(text, &known);
        let effective = if rest.trim().is_empty() { text } else { &rest };
        if project_id.is_empty() {
            bail!(
                "no project matched in '{}' — name one: '... on <project>' or target a worker '<project>#<idx>'",
                text.trim()
            );
        }

        let handle: AgentHandle = if let Some(idx) = explicit_idx {
            let id = format!("{project_id}#{idx}");
            self.get(&id)
                .await
                .with_context(|| format!("no agent {id}"))?
        } else {
            let agent_ids = self.project_agents(&project_id).await;
            if agent_ids.is_empty() {
                // auto-spawn a worker for the target project so dispatch just works
                let spawned = self.spawn_project_extra(&project_id, 1).await?;
                let id = spawned
                    .first()
                    .cloned()
                    .context("auto-spawn produced no agent")?;
                let h = self.get(&id).await.context("auto-spawned agent missing")?;
                let (wid, behavior) = {
                    let r = h.record.lock().await;
                    (r.id.clone(), "followUp".to_string())
                };
                let text = effective.trim().to_string();
                h.tx
                    .send(AgentCommand::Prompt { text: text.clone(), behavior })
                    .await
                    .map_err(|_| anyhow::anyhow!("agent '{wid}' not accepting commands"))?;
                return Ok((wid, text));
            }
            let mut best: Option<AgentHandle> = None;
            let mut best_score = (i32::MAX, u64::MAX);
            for id in agent_ids {
                if let Some(h) = self.get(&id).await {
                    let r = h.record.lock().await;
                    let score = (m_priority(r.phase), r.queue_steering as u64
                        + r.queue_follow_up as u64
                        + r.pending_message_count);
                    if score < best_score {
                        best_score = score;
                        best = Some(h.clone());
                    }
                }
            }
            best.context("no viable agent to dispatch to")?
        };

        let (id, behavior) = {
            let r = handle.record.lock().await;
            (
                r.id.clone(),
                if matches!(r.phase, AgentPhase::Busy | AgentPhase::Retrying) {
                    "steer".to_string()
                } else {
                    "followUp".to_string()
                },
            )
        };
        let text = effective.trim().to_string();
        handle
            .tx
            .send(AgentCommand::Prompt { text: text.clone(), behavior })
            .await
            .map_err(|_| anyhow::anyhow!("agent '{id}' not accepting commands"))?;
        Ok((id, text))
    }

    /// Snapshot for TUI/API: projects + agent summaries + meta.
    pub async fn snapshot(&self) -> Value {
        let mut agents: Vec<Value> = Vec::new();
        let mut by_project: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        {
            let guards = self.handles.read().await;
            for h in guards.values() {
                let r = h.record.lock().await;
                let s = serde_json::to_value(r.summary()).unwrap_or(Value::Null);
                by_project
                    .entry(r.project_id.clone())
                    .or_default()
                    .push(s);
            }
        }
        let config = self.config.read().await;
        let projects: Vec<Value> = config
            .projects
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.display_name(),
                    "path": p.path.display().to_string(),
                    "configured_agents": p.agents,
                    "running_agents": by_project.get(&p.id).map(|v| v.len()).unwrap_or(0),
                    "model": p.model,
                    "agents": by_project.get(&p.id).cloned().unwrap_or_default(),
                })
            })
            .collect();
        for v in by_project.values() {
            agents.extend(v.iter().cloned());
        }
        json!({
            "daemon_uptime_secs": self.started_at.elapsed().as_secs(),
            "project_count": config.projects.len(),
            "projects": projects,
            "agents": agents,
        })
    }

    pub async fn detail(&self, id_or_project: &str) -> Option<Value> {
        let h = self.get_any(id_or_project).await?;
        let r = h.record.lock().await;
        Some(serde_json::to_value(r.summary()).unwrap_or(Value::Null))
    }
}

fn m_priority(p: AgentPhase) -> i32 {
    match p {
        AgentPhase::Idle => 0,
        AgentPhase::Spawning => 2,
        AgentPhase::Busy | AgentPhase::Retrying => 1,
        _ => 3,
    }
}

// ------------------------------------------------------------ agent task

enum RunResult {
    Stopped,
    Crashed { reason: String },
}

async fn agent_supervisor(
    cfg: Arc<ResolvedConfig>,
    project: ResolvedProject,
    idx: usize,
    record: Arc<Mutex<AgentRecord>>,
    mut rx: mpsc::Receiver<AgentCommand>,
    notify_tx: mpsc::Sender<AgentNotify>,
) {
    let max_restarts = cfg.defaults.max_restarts.max(1).min(10);
    loop {
        match run_agent_process(cfg.clone(), &project, &record, &mut rx, &notify_tx).await {
            RunResult::Stopped => break,
            RunResult::Crashed { reason } => {
                let mut rec = record.lock().await;
                rec.exited = true;
                rec.last_error = Some(format!("agent crashed: {reason}"));
                if let Some((old, new)) = rec.set_phase(AgentPhase::Error) {
                    let _ = notify_tx
                        .send(AgentNotify::PhaseChanged {
                            id: rec.id.clone(),
                            project: rec.project_id.clone(),
                            old,
                            new,
                        })
                        .await;
                }
                rec.restarts += 1;
                if !cfg.defaults.auto_restart || rec.restarts as usize > max_restarts {
                    drop(rec);
                    break;
                }
                drop(rec);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    // terminal: mark stopped
    let mut rec = record.lock().await;
    if let Some((old, new)) = rec.set_phase(AgentPhase::Stopped) {
        let _ = notify_tx
            .send(AgentNotify::PhaseChanged {
                id: rec.id.clone(),
                project: rec.project_id.clone(),
                old,
                new,
            })
            .await;
    }
}

/// One full lifetime of a pi subprocess for an agent.
async fn run_agent_process(
    cfg: Arc<ResolvedConfig>,
    project: &ResolvedProject,
    record: &Arc<Mutex<AgentRecord>>,
    rx: &mut mpsc::Receiver<AgentCommand>,
    notify_tx: &mpsc::Sender<AgentNotify>,
) -> RunResult {
    let spawned = match rpc::spawn_pi(
        &cfg.defaults.pi,
        &project.path,
        &project.env,
        project.model.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return RunResult::Crashed { reason: e.to_string() },
    };
    let SpawnedPi {
        mut child,
        mut stdin,
        stdout,
        stderr,
    } = spawned;

    {
        let mut rec = record.lock().await;
        rec.pid = child.id();
        rec.exited = false;
        rec.last_error = None;
        rec.touch();
        if let Some((old, new)) = rec.set_phase(AgentPhase::Spawning) {
            let _ = notify_tx
                .send(AgentNotify::PhaseChanged {
                    id: rec.id.clone(),
                    project: rec.project_id.clone(),
                    old,
                    new,
                })
                .await;
        }
    }

    // stderr pump
    let stderr_record = record.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let trimmed = line.trim_end().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let mut rec = stderr_record.lock().await;
                    if rec.stderr_tail.len() >= 100 {
                        rec.stderr_tail.remove(0);
                    }
                    rec.stderr_tail.push(trimmed.clone());
                    rec.last_error = Some(format!("stderr: {trimmed}"));
                    rec.touch();
                }
            }
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut refresh = tokio::time::sleep(Duration::from_millis(1500));
    tokio::pin!(refresh);

    // initial state probe
    let _ = rpc::write_command(&mut stdin, &json!({"type": "get_state"})).await;
    {
        let mut rec = record.lock().await;
        rec.touch();
    }

    loop {
        tokio::select! {
            line = async { lines.next_line().await } => {
                match line {
                    Ok(Some(l)) => {
                        let raw = format!("[{}] {}", Utc::now().format("%H:%M:%S"), l);
                        let mut rec = record.lock().await;
                        rec.push_log_line(raw);
                        if let Some(ev) = rpc::parse_line(&l) {
                            apply_event(&mut rec, ev, notify_tx).await;
                        }
                        rec.touch();
                    }
                    Ok(None) => {
                        // stdout closed -> worker exiting
                        break;
                    }
                    Err(e) => {
                        let mut rec = record.lock().await;
                        rec.last_error = Some(format!("stdout error: {e}"));
                        break;
                    }
                }
            }
            cmd = async { rx.recv().await } => {
                let Some(cmd) = cmd else { break };
                if matches!(cmd, AgentCommand::Stop) {
                    break;
                }
                if let Err(e) = write_agent_command(&mut stdin, cmd, record).await {
                    let mut rec = record.lock().await;
                    rec.last_error = Some(format!("command failed: {e}"));
                }
            }
            _ = &mut refresh => {
                let acceptable = {
                    let rec = record.lock().await;
                    matches!(
                        rec.phase,
                        AgentPhase::Spawning | AgentPhase::Idle | AgentPhase::Busy | AgentPhase::Retrying
                    )
                };
                if acceptable {
                    let _ = rpc::write_command(&mut stdin, &json!({"type": "get_state"})).await;
                }
                refresh.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(20));
            }
        }
    }

    drop(stdin);
    let _ = child.kill().await;
    let _ = child.wait().await;
    RunResult::Stopped
}

async fn write_agent_command(
    stdin: &mut tokio::process::ChildStdin,
    cmd: AgentCommand,
    record: &Arc<Mutex<AgentRecord>>,
) -> Result<()> {
    let (json, note) = {
        let rec = record.lock().await;
        let json = build_command(cmd.clone(), rec.model.as_deref());
        let note = match &cmd {
            AgentCommand::Prompt { text, .. } => Some(format!("▶ {}", short(text, 140))),
            AgentCommand::Steer { text } => Some(format!("✎ steer: {}", short(text, 140))),
            AgentCommand::FollowUp { text } => Some(format!("✎ follow-up: {}", short(text, 140))),
            AgentCommand::Abort => Some("■ abort".to_string()),
            AgentCommand::SetModel { model } => Some(format!("⚙ model → {model}")),
            AgentCommand::CycleModel => Some("⚙ cycle model".to_string()),
            AgentCommand::SetThinking { level } => Some(format!("⚙ thinking → {level}")),
            AgentCommand::Compact => Some("◇ compact (manual)".to_string()),
            AgentCommand::Bash { command } => Some(format!("⌁ bash: {}", short(command, 140))),
            AgentCommand::Raw { json } => Some(format!("⚙ raw: {}", short(json, 120))),
            AgentCommand::Stop => Some("■ stop".to_string()),
        };
        (json, note)
    };
    if let Some(n) = note {
        let mut rec = record.lock().await;
        rec.push_transcript(format!("[{}] {n}", Utc::now().format("%H:%M:%S")));
    }
    rpc::write_command(stdin, &json).await
}

fn short(s: &str, cap: usize) -> String {
    let trimmed = s.trim();
    let t: String = trimmed.chars().take(cap).collect();
    if trimmed.chars().count() > cap { format!("{t}…") } else { t }
}

fn build_command(cmd: AgentCommand, current_model: Option<&str>) -> Value {
    match cmd {
        AgentCommand::Prompt { text, behavior } => {
            json!({"type": "prompt", "message": text, "streamingBehavior": behavior})
        }
        AgentCommand::Steer { text } => json!({"type": "steer", "message": text}),
        AgentCommand::FollowUp { text } => json!({"type": "follow_up", "message": text}),
        AgentCommand::Abort => json!({"type": "abort"}),
        AgentCommand::Compact => json!({"type": "compact"}),
        AgentCommand::CycleModel => json!({"type": "cycle_model"}),
        AgentCommand::SetThinking { level } => {
            json!({"type": "set_thinking_level", "level": level})
        }
        AgentCommand::SetModel { model } => {
            let (provider, model_id) = split_model(&model, current_model);
            json!({"type": "set_model", "provider": provider, "modelId": model_id})
        }
        AgentCommand::Bash { command } => json!({"type": "bash", "command": command}),
        AgentCommand::Raw { json } => {
            serde_json::from_str(&json).unwrap_or_else(|_| json!({"type": "unknown"}))
        }
        AgentCommand::Stop => json!({"type": "abort"}),
    }
}

fn split_model(model: &str, current: Option<&str>) -> (String, String) {
    match model.split_once('/') {
        Some((p, m)) => (p.to_string(), m.to_string()),
        None => {
            let provider = current
                .and_then(|c| c.split_once('/'))
                .map(|(p, _)| p.to_string())
                .unwrap_or_else(|| "default".to_string());
            (provider, model.to_string())
        }
    }
}

// ----------------------------------------------------------- event handling

async fn apply_event(
    rec: &mut AgentRecord,
    ev: PiEvent,
    notify_tx: &mpsc::Sender<AgentNotify>,
) {
    use PiEvent::*;
    let id = rec.id.clone();
    let project = rec.project_id.clone();
    match ev {
        AgentStart => {
            if let Some((old, new)) = rec.set_phase(AgentPhase::Busy) {
                let _ = notify_tx
                    .send(AgentNotify::PhaseChanged { id: id.clone(), project: project.clone(), old, new })
                    .await;
            }
            rec.was_busy_on_settle = true;
            rec.bash_stream_lines = 0;
            rec.push_transcript(format!("[{}] ▶ run started", Utc::now().format("%H:%M:%S")));
        }
        AgentSettled => {
            let was = rec.was_busy_on_settle;
            if let Some((old, new)) = rec.set_phase(AgentPhase::Idle) {
                let _ = notify_tx
                    .send(AgentNotify::PhaseChanged { id: id.clone(), project: project.clone(), old, new })
                    .await;
            }
            if was {
                let text = summary_text(rec);
                let _ = notify_tx
                    .send(AgentNotify::Idle { id: id.clone(), project: project.clone(), text })
                    .await;
            }
            rec.was_busy_on_settle = false;
            rec.bash_stream_lines = 0;
            rec.flush_pending_text();
            rec.push_transcript(format!("[{}] ✓ idle", Utc::now().format("%H:%M:%S")));
        }
        AutoRetryStart { attempt, max_attempts, delay_ms, error_message } => {
            rec.last_error = Some(format!("retry {attempt}/{max_attempts} in {delay_ms}ms: {error_message}"));
            rec.push_transcript(format!(
                "[{}] ↻ retry {attempt}/{max_attempts} in {delay_ms}ms — {}",
                Utc::now().format("%H:%M:%S"),
                short(&error_message, 80)
            ));
            if let Some((old, new)) = rec.set_phase(AgentPhase::Retrying) {
                let _ = notify_tx
                    .send(AgentNotify::PhaseChanged { id: id.clone(), project: project.clone(), old, new })
                    .await;
            }
        }
        AutoRetryEnd { success, final_error, .. } => {
            if !success {
                rec.last_error = final_error.clone();
            }
            // run resumes (or settles soon); we keep Busy if we were Busy before
            if rec.phase == AgentPhase::Retrying {
                rec.set_phase(AgentPhase::Busy);
            }
        }
        MessageUpdate { delta_type, text, .. } => {
            match delta_type.as_str() {
                "text_delta" => {
                    if let Some(t) = text {
                        rec.last_text.push_str(&t);
                        rec.truncate_text();
                        rec.pending_text.push_str(&t);
                    }
                }
                "text_end" => {
                    if let Some(t) = text {
                        rec.last_text = t;
                        rec.truncate_text();
                    }
                    rec.flush_pending_text();
                }
                _ => {}
            }
        }
        MessageEnd { message } => {
            if let Some(m) = message.get("content").and_then(|c| c.as_array()) {
                let mut parts: Vec<String> = Vec::new();
                for block in m {
                    match (block.get("type").and_then(|t| t.as_str()), block.get("text").and_then(|t| t.as_str())) {
                        (Some("text"), Some(t)) => parts.push(t.to_string()),
                        _ => {}
                    }
                }
                if !parts.is_empty() {
                    rec.last_text = parts.join("\n");
                    rec.truncate_text();
                }
            }
            rec.flush_pending_text();
        }
        ToolExecStart { name, args, .. } => {
            let label = crate::transcript_fmt::tool_short_label(&name, &args);
            rec.current_tool = Some(label);
            let ts = Utc::now().format("%H:%M:%S");
            for line in crate::transcript_fmt::format_tool_start(&name, &args) {
                rec.push_transcript(format!("[{ts}] {line}"));
            }
            rec.touch();
        }
        ToolExecUpdate { .. } => {
            // Streaming partials are noisy; final summary comes from ToolExecEnd.
            rec.touch();
        }
        ToolExecEnd { name, is_error, result, .. } => {
            let ts = Utc::now().format("%H:%M:%S");
            for line in crate::transcript_fmt::format_tool_end(&name, is_error, &result) {
                rec.push_transcript(format!("[{ts}] {line}"));
            }
            rec.current_tool = None;
            rec.touch();
        }
        QueueUpdate { steering, follow_up } => {
            rec.queue_steering = steering.len();
            rec.queue_follow_up = follow_up.len();
            rec.touch();
        }
        CompactionStart { reason } => {
            rec.compacting = true;
            rec.push_transcript(format!(
                "[{}] ◇ compacting ({reason})",
                Utc::now().format("%H:%M:%S")
            ));
        }
        CompactionEnd { .. } => {
            rec.compacting = false;
            rec.push_transcript(format!("[{}] ◇ compaction done", Utc::now().format("%H:%M:%S")));
        }
        ExtensionError { error } => {
            rec.last_error = Some(format!("extension error: {error}"));
            rec.push_transcript(format!("[{}] ⚠ {}", Utc::now().format("%H:%M:%S"), short(&error, 120)));
            let _ = notify_tx
                .send(AgentNotify::Error { id: id.clone(), project: project.clone(), message: error })
                .await;
        }
        Response { command, success, data, error } => {
            if !success {
                let err = error.clone().unwrap_or_else(|| "command failed".into());
                rec.last_error = error.clone();
                rec.push_transcript(format!(
                    "[{}] ✗ {command}: {}",
                    Utc::now().format("%H:%M:%S"),
                    short(&err, 120)
                ));
                let _ = notify_tx
                    .send(AgentNotify::Error { id: id.clone(), project: project.clone(), message: err })
                    .await;
                return;
            }
            match command.as_str() {
                "get_state" => {
                    if let Some(model) = data.get("model") {
                        if !model.is_null() {
                            let provider = model.get("provider").and_then(|p| p.as_str()).unwrap_or("?");
                            let mid = model.get("id").and_then(|m| m.as_str()).unwrap_or("?");
                            rec.model = Some(format!("{provider}/{mid}"));
                        }
                    }
                    if let Some(level) = data.get("thinkingLevel").and_then(|l| l.as_str()) {
                        rec.thinking_level = Some(level.to_string());
                    }
                    if let Some(f) = data.get("sessionFile").and_then(|s| s.as_str()) {
                        rec.session_file = Some(f.to_string());
                    }
                    if let Some(s) = data.get("sessionId").and_then(|s| s.as_str()) {
                        rec.session_id = Some(s.to_string());
                    }
                    if let Some(mc) = data.get("messageCount").and_then(|m| m.as_u64()) {
                        rec.message_count = Some(mc);
                    }
                    if let Some(pc) = data.get("pendingMessageCount").and_then(|p| p.as_u64()) {
                        rec.pending_message_count = pc;
                    }
                    if rec.phase == AgentPhase::Spawning {
                        rec.set_phase(AgentPhase::Idle);
                    }
                }
                "set_model" | "cycle_model" => {
                    if let Some(model) = data.get("model") {
                        if !model.is_null() {
                            let provider = model.get("provider").and_then(|p| p.as_str()).unwrap_or("");
                            let mid = model.get("id").and_then(|m| m.as_str()).unwrap_or("?");
                            rec.model = Some(format!("{provider}/{mid}"));
                        }
                        if let Some(level) = data.get("thinkingLevel").and_then(|l| l.as_str()) {
                            rec.thinking_level = Some(level.to_string());
                        }
                    }
                }
                "set_thinking_level" => {
                    // response has no data; the level we sent is in the log
                    rec.touch();
                }
                _ => {}
            }
        }
        BashExecUpdate { delta } => {
            // Direct RPC `bash` command output — keep raw log full; friendly transcript capped.
            if !delta.is_empty() {
                rec.push_log_line(format!("[bash] {}", delta.trim_end()));
                const CAP: usize = 10;
                if rec.bash_stream_lines < CAP {
                    let ts = Utc::now().format("%H:%M:%S");
                    for line in delta.lines().filter(|l| !l.trim().is_empty()) {
                        if rec.bash_stream_lines >= CAP {
                            rec.push_transcript(format!(
                                "[{ts}]   ⌁ … (further bash output in raw log)"
                            ));
                            rec.bash_stream_lines = CAP + 1;
                            break;
                        }
                        let first: String = line.chars().take(110).collect();
                        rec.push_transcript(format!("[{ts}]   ⌁ {first}"));
                        rec.bash_stream_lines += 1;
                    }
                }
                rec.touch();
            }
        }
        TurnStart | TurnEnd | AgentEnd { .. } | Unknown(_) => {}
    }
}

fn summary_text(rec: &AgentRecord) -> String {
    let text = rec.last_text.trim();
    if text.is_empty() {
        "done".to_string()
    } else {
        let shortened: String = text.chars().take(220).collect();
        if text.chars().count() > 220 {
            format!("{shortened}…")
        } else {
            shortened
        }
    }
}
