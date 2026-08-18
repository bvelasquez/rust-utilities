//! Per-agent runtime record and phase model.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::VecDeque;

pub const LOG_CAP: usize = 500;
pub const TRANSCRIPT_CAP: usize = 500;
pub const TRANSCRIPT_TAIL: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentPhase {
    Spawning,
    Idle,
    Busy,
    Retrying,
    Error,
    Stopped,
}

impl AgentPhase {
    pub fn label(&self) -> &'static str {
        match self {
            AgentPhase::Spawning => "spawning",
            AgentPhase::Idle => "idle",
            AgentPhase::Busy => "busy",
            AgentPhase::Retrying => "retrying",
            AgentPhase::Error => "error",
            AgentPhase::Stopped => "stopped",
        }
    }
    pub fn is_terminal_stop(&self) -> bool {
        matches!(self, AgentPhase::Stopped)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub agent_index: usize,
    pub phase: String,
    pub pid: Option<u32>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub session_file: Option<String>,
    pub session_id: Option<String>,
    pub message_count: Option<u64>,
    pub pending_message_count: u64,
    pub current_tool: Option<String>,
    pub last_text: String,
    pub last_error: Option<String>,
    pub queue_steering: usize,
    pub queue_follow_up: usize,
    pub compacting: bool,
    pub restarts: u32,
    pub started_at: String,
    pub last_activity: String,
    pub uptime_secs: u64,
    pub log_tail: Vec<String>,
    pub transcript_tail: Vec<String>,
    pub stderr_tail: Vec<String>,
}

pub struct AgentRecord {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub agent_index: usize,
    pub phase: AgentPhase,
    pub pid: Option<u32>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub session_file: Option<String>,
    pub session_id: Option<String>,
    pub message_count: Option<u64>,
    pub pending_message_count: u64,
    pub current_tool: Option<String>,
    pub last_text: String,
    pub last_error: Option<String>,
    pub queue_steering: usize,
    pub queue_follow_up: usize,
    pub compacting: bool,
    pub restarts: u32,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
    pub log: VecDeque<String>,
    /// friendly, chat-like transcript lines (not raw JSON)
    pub transcript: VecDeque<String>,
    /// in-progress assistant text while deltas stream
    pub pending_text: String,
    pub stderr_tail: Vec<String>,
    pub exited: bool,
    pub was_busy_on_settle: bool,
    /// Caps how many bash stream lines we echo into the friendly transcript.
    pub bash_stream_lines: usize,
}

impl AgentRecord {
    pub fn new(
        id: String,
        project_id: String,
        project_name: String,
        project_path: String,
        agent_index: usize,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            project_id,
            project_name,
            project_path,
            agent_index,
            phase: AgentPhase::Spawning,
            pid: None,
            model: None,
            thinking_level: None,
            session_file: None,
            session_id: None,
            message_count: None,
            pending_message_count: 0,
            current_tool: None,
            last_text: String::new(),
            last_error: None,
            queue_steering: 0,
            queue_follow_up: 0,
            compacting: false,
            restarts: 0,
            started_at: now,
            last_activity: now,
            settled_at: None,
            log: VecDeque::with_capacity(LOG_CAP),
            transcript: VecDeque::with_capacity(TRANSCRIPT_CAP),
            pending_text: String::new(),
            stderr_tail: Vec::new(),
            exited: false,
            was_busy_on_settle: false,
            bash_stream_lines: 0,
        }
    }

    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    pub fn push_transcript(&mut self, line: String) {
        if self.transcript.len() >= TRANSCRIPT_CAP {
            self.transcript.pop_front();
        }
        self.transcript.push_back(line);
    }

    pub fn flush_pending_text(&mut self) {
        let t = std::mem::take(&mut self.pending_text);
        let trimmed = t.trim();
        if !trimmed.is_empty() {
            // Keep more of the assistant reply so the log is useful.
            let short: String = trimmed.chars().take(280).collect();
            self.push_transcript(format!(
                "[{}] assistant: {}",
                Utc::now().format("%H:%M:%S"),
                if trimmed.chars().count() > 280 {
                    format!("{short}…")
                } else {
                    short
                }
            ));
        }
    }

    pub fn push_log_line(&mut self, line: String) {
        // keep only the tail; prepend a timestamp marker kept in the line by caller if wanted
        if self.log.len() >= LOG_CAP {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    pub fn truncate_text(&mut self) {
        const CAP: usize = 600;
        if self.last_text.chars().count() > CAP {
            let tail: String = self.last_text.chars().rev().take(CAP).collect::<String>().chars().rev().collect();
            self.last_text = format!("…{}", tail);
        }
    }

    pub fn set_phase(&mut self, phase: AgentPhase) -> Option<(AgentPhase, AgentPhase)> {
        if self.phase != phase {
            let old = self.phase;
            self.phase = phase;
            self.touch();
            if phase == AgentPhase::Idle {
                self.settled_at = Some(Utc::now());
            }
            Some((old, phase))
        } else {
            None
        }
    }

    pub fn summary(&self) -> AgentSummary {
        AgentSummary {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            project_name: self.project_name.clone(),
            project_path: self.project_path.clone(),
            agent_index: self.agent_index,
            phase: self.phase.label().to_string(),
            pid: self.pid,
            model: self.model.clone(),
            thinking_level: self.thinking_level.clone(),
            session_file: self.session_file.clone(),
            session_id: self.session_id.clone(),
            message_count: self.message_count,
            pending_message_count: self.pending_message_count,
            current_tool: self.current_tool.clone(),
            last_text: self.last_text.clone(),
            last_error: self.last_error.clone(),
            queue_steering: self.queue_steering,
            queue_follow_up: self.queue_follow_up,
            compacting: self.compacting,
            restarts: self.restarts,
            started_at: self.started_at.to_rfc3339(),
            last_activity: self.last_activity.to_rfc3339(),
            uptime_secs: self
                .settled_at
                .unwrap_or_else(Utc::now)
                .signed_duration_since(self.started_at)
                .num_seconds()
                .max(0) as u64,
            log_tail: self.log.iter().rev().take(80).cloned().collect::<Vec<_>>().into_iter().rev().collect(),
            transcript_tail: self.transcript.iter().rev().take(TRANSCRIPT_TAIL).cloned().collect::<Vec<_>>().into_iter().rev().collect(),
            stderr_tail: self.stderr_tail.iter().rev().take(40).cloned().collect::<Vec<_>>().into_iter().rev().collect(),
        }
    }
}