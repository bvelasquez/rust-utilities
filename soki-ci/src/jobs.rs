use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::{broadcast, mpsc, Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::broker::BrokerClient;
use crate::config::{ResolvedConfig, ResolvedProject, ResolvedTarget, SpeakDefaults, SpeakEvent};
use crate::history;
use crate::runner::{append_log_stream, build_spawn_spec, pump_lines, spawn_deploy, SpawnSpec};

const LOG_BUFFER_LINES: usize = 5_000;
const EVENT_CHANNEL_CAPACITY: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobPhase {
    Queued,
    Running,
    Success,
    Error,
    Cancelled,
    Interrupted,
}

impl JobPhase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobPhase::Success | JobPhase::Error | JobPhase::Cancelled | JobPhase::Interrupted
        )
    }

    pub fn is_active(self) -> bool {
        matches!(self, JobPhase::Queued | JobPhase::Running)
    }
}

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: Uuid,
    pub project_id: String,
    pub project_name: String,
    pub target_id: String,
    pub target_label: String,
    pub phase: JobPhase,
    #[serde(default = "utc_now")]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub runner: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub log_path: String,
    #[serde(default)]
    pub interrupted: bool,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub batch_id: Option<Uuid>,
    #[serde(default)]
    pub parent_job_id: Option<Uuid>,
    #[serde(default)]
    pub can_cancel: bool,
    #[serde(default)]
    pub can_input: bool,
    #[serde(default)]
    pub log_size_bytes: u64,
    #[serde(skip, default)]
    pub log_tail: VecDeque<String>,
}

impl JobRecord {
    pub fn push_log(&mut self, line: String) {
        self.log_tail.push_back(line);
        while self.log_tail.len() > 200 {
            self.log_tail.pop_front();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
    Stdin,
}

impl LogStream {
    pub fn file_prefix(self) -> &'static str {
        match self {
            LogStream::Stderr => "[stderr] ",
            LogStream::Stdin => "[stdin] ",
            LogStream::Stdout | LogStream::System => "",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub seq: u64,
    pub stream: LogStream,
    pub line: String,
    pub ts: DateTime<Utc>,
}

/// Events broadcast to desktop/API SSE clients. Log lines travel on a
/// separate per-job channel so a busy deploy cannot evict state events.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum ServerEvent {
    Snapshot {
        jobs: Vec<JobRecord>,
        status: OrchestratorStatus,
    },
    Job {
        job: JobRecord,
    },
    Removed {
        job_id: Uuid,
    },
    Config {
        path: String,
    },
    Heartbeat {
        ts: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectLock {
    pub project_id: String,
    pub job_id: Uuid,
    pub target_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrchestratorStatus {
    pub max_parallel: usize,
    pub available_parallelism: usize,
    pub running: usize,
    pub queued: usize,
    pub active: usize,
    pub project_locks: Vec<ProjectLock>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct JobStats {
    pub total: u64,
    pub success: u64,
    pub error: u64,
    pub cancelled: u64,
    pub interrupted: u64,
    pub running: u64,
    pub queued: u64,
    pub success_rate: f64,
    pub avg_duration_ms: Option<f64>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_finished_at: Option<DateTime<Utc>>,
    pub last_phase: Option<JobPhase>,
}

impl JobStats {
    pub fn from_jobs(jobs: &[JobRecord]) -> Self {
        let mut stats = JobStats::default();
        let mut durations = Vec::new();
        for job in jobs {
            stats.total += 1;
            match job.phase {
                JobPhase::Queued => stats.queued += 1,
                JobPhase::Running => stats.running += 1,
                JobPhase::Success => stats.success += 1,
                JobPhase::Error => stats.error += 1,
                JobPhase::Cancelled => stats.cancelled += 1,
                JobPhase::Interrupted => stats.interrupted += 1,
            }
            if let Some(ms) = job.duration_ms {
                durations.push(ms as f64);
            }
            if let Some(started) = job.started_at {
                stats.last_started_at = Some(
                    stats
                        .last_started_at
                        .map(|prev| prev.max(started))
                        .unwrap_or(started),
                );
            }
            if let Some(finished) = job.finished_at {
                stats.last_finished_at = Some(
                    stats
                        .last_finished_at
                        .map(|prev| prev.max(finished))
                        .unwrap_or(finished),
                );
            }
            if stats.last_phase.is_none() {
                stats.last_phase = Some(job.phase);
            }
        }
        let terminal = stats.success + stats.error + stats.cancelled + stats.interrupted;
        stats.success_rate = if terminal == 0 {
            0.0
        } else {
            (stats.success as f64 / terminal as f64) * 100.0
        };
        stats.avg_duration_ms = if durations.is_empty() {
            None
        } else {
            Some(durations.iter().sum::<f64>() / durations.len() as f64)
        };
        stats
    }
}

pub fn stats_for(
    jobs: &[JobRecord],
    project_id: Option<&str>,
    target_id: Option<&str>,
) -> JobStats {
    let filtered: Vec<JobRecord> = jobs
        .iter()
        .filter(|j| project_id.is_none_or(|p| j.project_id == p))
        .filter(|j| target_id.is_none_or(|t| j.target_id == t))
        .cloned()
        .collect();
    JobStats::from_jobs(&filtered)
}

/// Returned by [`JobOrchestrator::enqueue`] when the project already has an
/// active deploy. Callers map this to HTTP 409.
#[derive(Debug)]
pub struct DeployConflict(pub String);

impl std::fmt::Display for DeployConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeployConflict {}

fn project_slot_key(project_id: &str) -> String {
    project_id.to_string()
}

struct JobLogBuffer {
    lines: VecDeque<LogLine>,
    next_seq: u64,
    tx: broadcast::Sender<LogLine>,
}

impl JobLogBuffer {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(LOG_BUFFER_LINES.min(2_048));
        Self {
            lines: VecDeque::new(),
            next_seq: 1,
            tx,
        }
    }

    fn push(&mut self, stream: LogStream, line: String) -> LogLine {
        let entry = LogLine {
            seq: self.next_seq,
            stream,
            line,
            ts: Utc::now(),
        };
        self.next_seq += 1;
        self.lines.push_back(entry.clone());
        while self.lines.len() > LOG_BUFFER_LINES {
            self.lines.pop_front();
        }
        let _ = self.tx.send(entry.clone());
        entry
    }

    fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.tx.subscribe()
    }

    fn tail(&self, tail: usize) -> Vec<LogLine> {
        let n = tail.max(1);
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip).cloned().collect()
    }
}

struct JobControl {
    cancel: CancellationToken,
}

struct OrchestratorInner {
    jobs: HashMap<Uuid, JobRecord>,
    order: Vec<Uuid>,
    project_active: HashMap<String, Uuid>,
    stdin: HashMap<Uuid, mpsc::Sender<Vec<u8>>>,
    speech_pending: HashSet<Uuid>,
    controls: HashMap<Uuid, JobControl>,
    logs: HashMap<Uuid, JobLogBuffer>,
    events: broadcast::Sender<ServerEvent>,
}

impl OrchestratorInner {
    fn trim_history(&mut self, max_history: usize) {
        while self.order.len() > max_history {
            let Some(old) = self.order.pop() else {
                break;
            };
            let removable = self
                .jobs
                .get(&old)
                .is_some_and(|j| j.phase != JobPhase::Running && j.phase != JobPhase::Queued);
            if removable {
                self.jobs.remove(&old);
                self.logs.remove(&old);
                self.stdin.remove(&old);
                self.controls.remove(&old);
                continue;
            }
            self.order.push(old);
            break;
        }
    }

    fn clear_project_slot(&mut self, project_id: &str, job_id: Uuid) {
        let key = project_slot_key(project_id);
        if self.project_active.get(&key) == Some(&job_id) {
            self.project_active.remove(&key);
        }
    }

    fn push_log(&mut self, id: Uuid, stream: LogStream, line: String) {
        if let Some(job) = self.jobs.get_mut(&id) {
            let tui_line = format!("{}{}", stream.file_prefix(), line);
            job.push_log(tui_line);
        }
        if let Some(buffer) = self.logs.get_mut(&id) {
            buffer.push(stream, line);
        }
    }
}

pub fn runner_summary(target: &ResolvedTarget) -> String {
    use crate::config::RunnerConfig;
    match &target.runner {
        RunnerConfig::PnpmScript { script } => format!("pnpm run {script}"),
        RunnerConfig::NpmScript { script } => format!("npm run {script}"),
        RunnerConfig::Make { target } => format!("make {target}"),
        RunnerConfig::Shell { command } => command.join(" "),
    }
}

pub struct JobOrchestrator {
    inner: Arc<Mutex<OrchestratorInner>>,
    semaphore: Arc<Semaphore>,
    speak: Arc<RwLock<SpeakDefaults>>,
    broker: Arc<RwLock<BrokerClient>>,
    events: broadcast::Sender<ServerEvent>,
    max_history: usize,
    max_parallel: usize,
    data_dir: PathBuf,
}

impl JobOrchestrator {
    pub fn new(cfg: &ResolvedConfig) -> Self {
        let data_dir =
            history::data_dir().unwrap_or_else(|_| std::env::temp_dir().join("soki-ci-data"));
        Self::with_data_dir(cfg, data_dir, true)
    }

    /// Isolated constructor for tests and embedded instances.
    pub fn new_with_data_dir(cfg: &ResolvedConfig, data_dir: PathBuf) -> Self {
        Self::with_data_dir(cfg, data_dir, true)
    }

    pub fn new_in_memory(cfg: &ResolvedConfig) -> Self {
        let data_dir = std::env::temp_dir().join(format!("soki-ci-memory-{}", Uuid::new_v4()));
        Self::with_data_dir(cfg, data_dir, false)
    }

    fn with_data_dir(cfg: &ResolvedConfig, data_dir: PathBuf, load_history: bool) -> Self {
        let speak = cfg.defaults.speak.clone();
        let broker = BrokerClient::new(speak.base_url.clone(), speak.voice.clone());
        let max_parallel = cfg.defaults.max_parallel.max(1);
        let semaphore = Arc::new(Semaphore::new(max_parallel));
        let max_history = cfg.defaults.history_limit.max(1);
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let mut inner = OrchestratorInner {
            jobs: HashMap::new(),
            order: Vec::new(),
            project_active: HashMap::new(),
            stdin: HashMap::new(),
            speech_pending: HashSet::new(),
            controls: HashMap::new(),
            logs: HashMap::new(),
            events: events.clone(),
        };

        if load_history {
            if let Ok(jobs) = history::load_jobs_in(&data_dir, max_history) {
                for job in jobs {
                    inner.order.push(job.id);
                    inner.jobs.insert(job.id, job);
                }
            }
        }

        Self {
            inner: Arc::new(Mutex::new(inner)),
            semaphore,
            speak: Arc::new(RwLock::new(speak)),
            broker: Arc::new(RwLock::new(broker)),
            events,
            max_history,
            max_parallel,
            data_dir,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn max_history(&self) -> usize {
        self.max_history
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ServerEvent> {
        self.events.subscribe()
    }

    pub fn notify_config_changed(&self, path: String) {
        let _ = self.events.send(ServerEvent::Config { path });
    }

    pub fn update_speak(&self, cfg: &ResolvedConfig) {
        let speak = cfg.defaults.speak.clone();
        let broker = BrokerClient::new(speak.base_url.clone(), speak.voice.clone());
        *self.speak.write().expect("tts settings lock poisoned") = speak;
        *self.broker.write().expect("tts broker lock poisoned") = broker;
    }

    pub fn broker(&self) -> BrokerClient {
        self.broker
            .read()
            .expect("tts broker lock poisoned")
            .clone()
    }

    pub fn speak_settings(&self) -> SpeakDefaults {
        self.speak
            .read()
            .expect("tts settings lock poisoned")
            .clone()
    }

    pub fn tts_enabled(&self) -> bool {
        self.speak
            .read()
            .expect("tts settings lock poisoned")
            .enabled
    }

    pub async fn speech_pending(&self, id: Uuid) -> bool {
        self.inner.lock().await.speech_pending.contains(&id)
    }

    /// Called once by the owning service (TUI/serve/web) after history load.
    /// Jobs that were queued/running when the previous process exited are
    /// marked interrupted. Transient CLI commands do not call this, so running
    /// a `jobs list` next to a live TUI cannot corrupt its sidecars.
    pub async fn recover_interrupted(&self) {
        let mut updates = Vec::new();
        let events = {
            let mut g = self.inner.lock().await;
            let ids: Vec<Uuid> = g
                .jobs
                .values()
                .filter(|job| job.phase.is_active())
                .map(|job| job.id)
                .collect();
            for id in ids {
                let project_id = {
                    let Some(job) = g.jobs.get_mut(&id) else {
                        continue;
                    };
                    job.phase = JobPhase::Interrupted;
                    job.interrupted = true;
                    job.cancel_requested = false;
                    job.can_cancel = false;
                    job.can_input = false;
                    job.exit_code = None;
                    job.finished_at = Some(Utc::now());
                    job.duration_ms = job
                        .started_at
                        .map(|started| (Utc::now() - started).num_milliseconds());
                    job.project_id.clone()
                };
                g.push_log(
                    id,
                    LogStream::System,
                    "interrupted: previous soki-ci process exited".to_string(),
                );
                g.stdin.remove(&id);
                g.controls.remove(&id);
                g.clear_project_slot(&project_id, id);
                if let Some(job) = g.jobs.get(&id).cloned() {
                    updates.push(job);
                }
            }
            g.events.clone()
        };
        for job in updates {
            let _ = append_log_stream(
                Path::new(&job.log_path),
                LogStream::System,
                "interrupted: previous soki-ci process exited",
            )
            .await;
            let _ = history::write_job_in(&self.data_dir, &job);
            let _ = events.send(ServerEvent::Job { job });
        }
    }

    pub async fn list_jobs(&self) -> Vec<JobRecord> {
        let g = self.inner.lock().await;
        g.order
            .iter()
            .filter_map(|id| g.jobs.get(id).cloned())
            .collect()
    }

    pub async fn get_job(&self, id: Uuid) -> Option<JobRecord> {
        let g = self.inner.lock().await;
        g.jobs.get(&id).cloned()
    }

    pub async fn running_count(&self) -> usize {
        let g = self.inner.lock().await;
        g.jobs.values().filter(|j| j.phase.is_active()).count()
    }

    pub async fn status(&self) -> OrchestratorStatus {
        let g = self.inner.lock().await;
        let running = g
            .jobs
            .values()
            .filter(|j| j.phase == JobPhase::Running)
            .count();
        let queued = g
            .jobs
            .values()
            .filter(|j| j.phase == JobPhase::Queued)
            .count();
        let project_locks = g
            .project_active
            .iter()
            .filter_map(|(project_id, job_id)| {
                g.jobs.get(job_id).map(|job| ProjectLock {
                    project_id: project_id.clone(),
                    job_id: *job_id,
                    target_id: job.target_id.clone(),
                })
            })
            .collect();
        OrchestratorStatus {
            max_parallel: self.max_parallel,
            available_parallelism: self.semaphore.available_permits(),
            running,
            queued,
            active: running + queued,
            project_locks,
        }
    }

    pub async fn snapshot(&self) -> (Vec<JobRecord>, OrchestratorStatus) {
        let jobs = self.list_jobs().await;
        let status = self.status().await;
        (jobs, status)
    }

    pub async fn subscribe_logs(
        &self,
        id: Uuid,
    ) -> Option<(Vec<LogLine>, broadcast::Receiver<LogLine>)> {
        let g = self.inner.lock().await;
        let buffer = g.logs.get(&id)?;
        let rx = buffer.subscribe();
        Some((buffer.lines.iter().cloned().collect(), rx))
    }

    pub async fn read_log(&self, id: Uuid, tail: usize) -> Result<Vec<LogLine>> {
        let (log_path, buffered) = {
            let g = self.inner.lock().await;
            let job = g.jobs.get(&id).ok_or_else(|| anyhow!("job not found"))?;
            let buffered = g.logs.get(&id).map(|b| b.tail(tail));
            (job.log_path.clone(), buffered)
        };
        if let Some(lines) = buffered {
            return Ok(lines);
        }
        if log_path.is_empty() {
            return Ok(Vec::new());
        }
        let content = tokio::fs::read_to_string(&log_path)
            .await
            .unwrap_or_default();
        Ok(parse_log_file(&content, tail))
    }

    pub async fn enqueue(
        &self,
        project: &ResolvedProject,
        target: &ResolvedTarget,
    ) -> Result<Uuid> {
        self.enqueue_with_source(project, target, "unknown").await
    }

    pub async fn enqueue_with_source(
        &self,
        project: &ResolvedProject,
        target: &ResolvedTarget,
        source: &str,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let log_dir = history::ensure_runs_dir(&self.data_dir)?;
        let log_path = log_dir.join(format!("{id}.log"));
        let slot = project_slot_key(&project.id);
        let record = JobRecord {
            id,
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            target_id: target.id.clone(),
            target_label: target.label.clone(),
            phase: JobPhase::Queued,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            exit_code: None,
            duration_ms: None,
            source: source.to_string(),
            runner: runner_summary(target),
            cwd: target.cwd.to_string_lossy().into_owned(),
            log_path: log_path.to_string_lossy().into_owned(),
            interrupted: false,
            cancel_requested: false,
            batch_id: None,
            parent_job_id: None,
            can_cancel: true,
            can_input: true,
            log_size_bytes: 0,
            log_tail: VecDeque::new(),
        };

        let spec = build_spawn_spec(project, target)?;
        let cancel = CancellationToken::new();
        let pid = Arc::new(AtomicU32::new(0));
        let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(32);
        let control = JobControl {
            cancel: cancel.clone(),
        };

        let (record_clone, events) = {
            let mut g = self.inner.lock().await;
            if let Some(active) = g.project_active.get(&slot) {
                let active_target = g
                    .jobs
                    .get(active)
                    .map(|j| j.target_id.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let msg = format!(
                    "project `{}` already has a `{}` deploy running or queued. Targets of one project share a working tree and cannot build concurrently, so wait for it to finish and then deploy `{}`.",
                    project.id, active_target, target.id
                );
                return Err(anyhow::Error::new(DeployConflict(msg)));
            }
            g.jobs.insert(id, record);
            g.order.insert(0, id);
            g.project_active.insert(slot, id);
            g.stdin.insert(id, in_tx);
            g.controls.insert(id, control);
            g.logs.insert(id, JobLogBuffer::new());
            g.push_log(
                id,
                LogStream::System,
                format!("queued: {} / {}", project.id, target.id),
            );
            let record = g.jobs.get(&id).cloned().expect("job inserted");
            let events = g.events.clone();
            g.trim_history(self.max_history);
            (record, events)
        };

        let _ = append_log_stream(
            &log_path,
            LogStream::System,
            &format!("queued: {} / {}", project.id, target.id),
        )
        .await;
        let _ = history::write_job_in(&self.data_dir, &record_clone);
        let _ = events.send(ServerEvent::Job {
            job: record_clone.clone(),
        });

        let inner = self.inner.clone();
        let semaphore = self.semaphore.clone();
        let speak = self
            .speak
            .read()
            .expect("tts settings lock poisoned")
            .clone();
        let broker = self
            .broker
            .read()
            .expect("tts broker lock poisoned")
            .clone();
        let project_id = project.id.clone();
        let project_name = project.name.clone();
        let target_label = target.label.clone();
        let target_speak = target.speak;
        let log_path = log_path.clone();
        let task_cancel = cancel.clone();
        let task_pid = pid.clone();
        let data_dir = self.data_dir.clone();

        tokio::spawn(async move {
            // The permit must be dropped before TTS. `broker.speak` waits for
            // playback (up to the client timeout) and a broker that accepts the
            // connection but never answers holds it for that whole time. The
            // permit is the global parallelism budget, so a few stuck
            // announcements fill it and every later job stays `queued` with no
            // child. Project slots are already cleared in `finalize_job`.
            enum AfterPermit {
                Speak(String),
                Failed(String),
                Done,
            }
            let step = async {
                let permit = tokio::select! {
                    res = semaphore.acquire_owned() => res.ok(),
                    _ = task_cancel.cancelled() => None,
                };
                let Some(permit) = permit else {
                    cleanup_prequeue(&inner, &data_dir, id).await;
                    return AfterPermit::Done;
                };
                let still_queued = {
                    let g = inner.lock().await;
                    g.jobs.get(&id).is_some_and(|j| j.phase == JobPhase::Queued)
                };
                if !still_queued {
                    drop(permit);
                    cleanup_prequeue(&inner, &data_dir, id).await;
                    return AfterPermit::Done;
                }

                let announcement = std::panic::AssertUnwindSafe(run_job(
                    id,
                    spec,
                    log_path.clone(),
                    inner.clone(),
                    data_dir.clone(),
                    in_rx,
                    task_cancel,
                    task_pid,
                    project_id,
                    project_name,
                    target_label,
                    target_speak,
                    speak,
                ))
                .catch_unwind()
                .await;
                drop(permit);
                match announcement {
                    Ok(Ok(Some(msg))) => AfterPermit::Speak(msg),
                    Ok(Ok(None)) => AfterPermit::Done,
                    Ok(Err(e)) => AfterPermit::Failed(format!("orchestrator error: {e}")),
                    Err(_) => AfterPermit::Failed("dispatcher task panicked".to_string()),
                }
            };
            match step.await {
                AfterPermit::Speak(msg) => {
                    let speak_err = broker.speak(&msg).await.err();
                    let mut g = inner.lock().await;
                    g.speech_pending.remove(&id);
                    if let Some(e) = speak_err {
                        if let Some(job) = g.jobs.get_mut(&id) {
                            job.push_log(format!("speak warning: {e}"));
                        }
                    }
                }
                AfterPermit::Done => {}
                AfterPermit::Failed(message) => {
                    fail_active_job(&inner, &data_dir, id, message).await;
                }
            }
        });

        Ok(id)
    }

    pub async fn clear_history(&self) -> Result<usize> {
        let (to_remove, events) = {
            let mut g = self.inner.lock().await;
            let to_remove: Vec<Uuid> = g
                .order
                .iter()
                .filter(|id| g.jobs.get(id).is_some_and(|j| !j.phase.is_active()))
                .copied()
                .collect();
            for id in &to_remove {
                g.jobs.remove(id);
                g.logs.remove(id);
                g.speech_pending.remove(id);
                g.stdin.remove(id);
                g.controls.remove(id);
            }
            g.order.retain(|id| !to_remove.contains(id));
            (to_remove, g.events.clone())
        };
        let n = to_remove.len();
        for id in &to_remove {
            let _ = history::delete_job_in(&self.data_dir, id, None);
            let _ = events.send(ServerEvent::Removed { job_id: *id });
        }
        Ok(n)
    }

    pub async fn cancel(&self, id: Uuid) -> Result<()> {
        let (control, phase, log_path) = {
            let g = self.inner.lock().await;
            let job = g.jobs.get(&id).ok_or_else(|| anyhow!("job not found"))?;
            if job.phase.is_terminal() {
                bail!("job is not running");
            }
            let control = g
                .controls
                .get(&id)
                .ok_or_else(|| anyhow!("job control is not available"))?
                .cancel
                .clone();
            (control, job.phase, job.log_path.clone())
        };

        if phase == JobPhase::Queued {
            let (job, events) = {
                let mut g = self.inner.lock().await;
                let Some(job) = g.jobs.get_mut(&id) else {
                    bail!("job not found");
                };
                job.phase = JobPhase::Cancelled;
                job.finished_at = Some(Utc::now());
                job.duration_ms = job.started_at.map(|s| (Utc::now() - s).num_milliseconds());
                job.cancel_requested = false;
                job.can_cancel = false;
                job.can_input = false;
                let project_id = job.project_id.clone();
                g.push_log(id, LogStream::System, "cancelled before start".to_string());
                g.stdin.remove(&id);
                g.controls.remove(&id);
                g.clear_project_slot(&project_id, id);
                let job = g.jobs.get(&id).cloned().expect("job exists");
                (job, g.events.clone())
            };
            let _ = append_log_stream(
                Path::new(&log_path),
                LogStream::System,
                "cancelled before start",
            )
            .await;
            let _ = history::write_job_in(&self.data_dir, &job);
            let _ = events.send(ServerEvent::Job { job });
            control.cancel();
            return Ok(());
        }

        {
            let mut g = self.inner.lock().await;
            if let Some(job) = g.jobs.get_mut(&id) {
                job.cancel_requested = true;
            }
            g.push_log(id, LogStream::System, "cancel requested".to_string());
            let job = g.jobs.get(&id).cloned();
            let events = g.events.clone();
            drop(g);
            if let Some(job) = job {
                let _ =
                    append_log_stream(Path::new(&log_path), LogStream::System, "cancel requested")
                        .await;
                let _ = history::write_job_in(&self.data_dir, &job);
                let _ = events.send(ServerEvent::Job { job });
            }
        }
        control.cancel();
        Ok(())
    }

    pub async fn send_input(&self, id: Uuid, text: &str) -> Result<()> {
        let payload = if text.ends_with('\n') {
            text.to_string()
        } else {
            format!("{text}\n")
        };
        let (tx, log_path) = {
            let g = self.inner.lock().await;
            let job = g.jobs.get(&id).ok_or_else(|| anyhow!("job not found"))?;
            if job.phase.is_terminal() {
                bail!("job is not running");
            }
            let tx = g
                .stdin
                .get(&id)
                .cloned()
                .ok_or_else(|| anyhow!("job stdin is not available"))?;
            (tx, job.log_path.clone())
        };
        tx.send(payload.as_bytes().to_vec())
            .await
            .map_err(|_| anyhow!("failed to write job stdin (process may have exited)"))?;

        let echo = payload.trim_end_matches(['\r', '\n']).to_string();
        record_log(
            &self.inner,
            id,
            Path::new(&log_path),
            LogStream::Stdin,
            echo,
        )
        .await?;
        Ok(())
    }

    pub async fn retry(&self, cfg: &ResolvedConfig, id: Uuid, source: &str) -> Result<Uuid> {
        let job = self
            .get_job(id)
            .await
            .ok_or_else(|| anyhow!("job not found"))?;
        let project = crate::config::find_project(cfg, &job.project_id)
            .ok_or_else(|| anyhow!("project `{}` is no longer configured", job.project_id))?;
        let target = project
            .targets
            .get(&job.target_id)
            .ok_or_else(|| anyhow!("target `{}` is no longer configured", job.target_id))?;
        self.enqueue_with_source(project, target, source).await
    }
}

async fn record_log(
    inner: &Arc<Mutex<OrchestratorInner>>,
    id: Uuid,
    log_path: &Path,
    stream: LogStream,
    line: String,
) -> Result<()> {
    {
        let mut g = inner.lock().await;
        g.push_log(id, stream, line.clone());
    }
    append_log_stream(log_path, stream, &line).await
}

async fn cleanup_prequeue(inner: &Arc<Mutex<OrchestratorInner>>, data_dir: &Path, id: Uuid) {
    let still_queued = {
        let g = inner.lock().await;
        g.jobs.get(&id).is_some_and(|j| j.phase == JobPhase::Queued)
    };
    if still_queued {
        let _ = finalize_job(
            inner,
            data_dir,
            id,
            JobPhase::Cancelled,
            None,
            Some("cancelled before start".to_string()),
        )
        .await;
    }
}

async fn finalize_job(
    inner: &Arc<Mutex<OrchestratorInner>>,
    data_dir: &Path,
    id: Uuid,
    phase: JobPhase,
    exit_code: Option<i32>,
    message: Option<String>,
) -> Option<JobRecord> {
    let (job, events) = {
        let mut g = inner.lock().await;
        let project_id = {
            let job = g.jobs.get_mut(&id)?;
            job.phase = phase;
            job.finished_at = Some(Utc::now());
            job.exit_code = exit_code;
            job.cancel_requested = false;
            job.can_cancel = false;
            job.can_input = false;
            job.duration_ms = job
                .started_at
                .map(|started| (Utc::now() - started).num_milliseconds());
            if phase == JobPhase::Interrupted {
                job.interrupted = true;
            }
            job.project_id.clone()
        };
        if let Some(message) = &message {
            g.push_log(id, LogStream::System, message.clone());
        }
        g.stdin.remove(&id);
        g.controls.remove(&id);
        g.clear_project_slot(&project_id, id);
        let job = g.jobs.get(&id).cloned().expect("job exists");
        (job, g.events.clone())
    };
    if let Some(message) = &message {
        let _ = append_log_stream(Path::new(&job.log_path), LogStream::System, message).await;
    }
    let mut job = job;
    job.log_size_bytes = std::fs::metadata(&job.log_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let _ = history::write_job_in(data_dir, &job);
    let _ = events.send(ServerEvent::Job { job: job.clone() });
    Some(job)
}

#[allow(clippy::too_many_arguments)]
async fn run_job(
    id: Uuid,
    spec: SpawnSpec,
    log_path: PathBuf,
    inner: Arc<Mutex<OrchestratorInner>>,
    data_dir: PathBuf,
    mut in_rx: mpsc::Receiver<Vec<u8>>,
    cancel: CancellationToken,
    pid_slot: Arc<AtomicU32>,
    project_id: String,
    project_name: String,
    target_label: String,
    target_speak: Option<bool>,
    speak_defaults: SpeakDefaults,
) -> Result<Option<String>> {
    let (events, job) = {
        let mut g = inner.lock().await;
        let Some(job) = g.jobs.get_mut(&id) else {
            g.clear_project_slot(&project_id, id);
            return Ok(None);
        };
        if job.phase != JobPhase::Queued {
            return Ok(None);
        }
        job.phase = JobPhase::Running;
        job.started_at = Some(Utc::now());
        job.cancel_requested = false;
        job.can_cancel = true;
        job.can_input = true;
        g.push_log(
            id,
            LogStream::System,
            format!("running: {} {:?}", spec.program, spec.args),
        );
        let job = g.jobs.get(&id).cloned().expect("job exists");
        (g.events.clone(), job)
    };
    let _ = append_log_stream(
        &log_path,
        LogStream::System,
        &format!("running: {} {:?}", spec.program, spec.args),
    )
    .await;
    let _ = history::write_job_in(&data_dir, &job);
    let _ = events.send(ServerEvent::Job { job });

    let mut child = match spawn_deploy(&spec).await {
        Ok(child) => child,
        Err(e) => {
            let _ = record_log(
                &inner,
                id,
                &log_path,
                LogStream::System,
                format!("spawn error: {e}"),
            )
            .await;
            let _ = finalize_job(
                &inner,
                &data_dir,
                id,
                JobPhase::Error,
                None,
                Some(format!("spawn error: {e}")),
            )
            .await;
            return Ok(None);
        }
    };

    if let Some(pid) = child.id() {
        pid_slot.store(pid, Ordering::SeqCst);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(bytes) = in_rx.recv().await {
                if stdin.write_all(&bytes).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });
    }

    let (tx, mut rx) = mpsc::channel::<(LogStream, String)>(256);
    if let Some(out) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            pump_lines(out, tx, LogStream::Stdout).await;
        });
    }
    if let Some(err) = stderr {
        let tx = tx.clone();
        tokio::spawn(async move {
            pump_lines(err, tx, LogStream::Stderr).await;
        });
    }
    drop(tx);

    let inner_log = inner.clone();
    let log_path_log = log_path.clone();
    let pump = tokio::spawn(async move {
        while let Some((stream, line)) = rx.recv().await {
            let _ = record_log(&inner_log, id, &log_path_log, stream, line).await;
        }
    });

    let mut cancelled = false;
    let status = tokio::select! {
        res = child.wait() => Some(res),
        _ = cancel.cancelled() => {
            cancelled = true;
            let pid = pid_slot.load(Ordering::SeqCst);
            kill_process_group(pid, libc::SIGTERM);
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(res) => Some(res),
                Err(_) => {
                    kill_process_group(pid, libc::SIGKILL);
                    Some(child.wait().await)
                }
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), pump).await;

    let status = match status {
        Some(Ok(status)) => status,
        Some(Err(e)) => {
            let _ = record_log(
                &inner,
                id,
                &log_path,
                LogStream::System,
                format!("wait error: {e}"),
            )
            .await;
            let _ = finalize_job(
                &inner,
                &data_dir,
                id,
                JobPhase::Error,
                None,
                Some(format!("wait error: {e}")),
            )
            .await;
            return Ok(None);
        }
        None => {
            let _ = finalize_job(
                &inner,
                &data_dir,
                id,
                JobPhase::Cancelled,
                None,
                Some("cancelled".to_string()),
            )
            .await;
            return Ok(None);
        }
    };

    let code = status.code().unwrap_or(-1);
    let success = status.success();
    let phase = if cancelled {
        JobPhase::Cancelled
    } else if success {
        JobPhase::Success
    } else {
        JobPhase::Error
    };

    let speak_enabled = target_speak.unwrap_or(speak_defaults.enabled);
    let should_speak = !cancelled
        && speak_enabled
        && ((success && speak_defaults.on.contains(&SpeakEvent::Success))
            || (!success && speak_defaults.on.contains(&SpeakEvent::Error)));

    let last_output_line = if should_speak && !success {
        let g = inner.lock().await;
        g.jobs.get(&id).and_then(last_error_line)
    } else {
        None
    };

    let message = if cancelled {
        "cancelled".to_string()
    } else {
        format!("finished: exit {code}")
    };
    // Mark the announcement pending before the phase becomes terminal so
    // in-process `--wait` cannot exit in the gap and cancel playback.
    if should_speak {
        inner.lock().await.speech_pending.insert(id);
    }
    let _ = finalize_job(&inner, &data_dir, id, phase, Some(code), Some(message)).await;

    if !should_speak {
        return Ok(None);
    }
    let msg = if success {
        format!("{project_name} {target_label} deploy succeeded.")
    } else if let Some(line) = last_output_line {
        format!("{project_name} {target_label} deploy failed with exit code {code}: {line}")
    } else {
        format!("{project_name} {target_label} deploy failed with exit code {code}.")
    };
    // Spoken after the caller drops the parallelism permit.
    Ok(Some(msg))
}

async fn fail_active_job(
    inner: &Arc<Mutex<OrchestratorInner>>,
    data_dir: &Path,
    id: Uuid,
    message: String,
) {
    let active = {
        let g = inner.lock().await;
        g.jobs.get(&id).is_some_and(|job| job.phase.is_active())
    };
    if !active {
        return;
    }
    let _ = finalize_job(
        inner,
        data_dir,
        id,
        JobPhase::Error,
        None,
        Some(message),
    )
    .await;
}

fn parse_log_file(content: &str, tail: usize) -> Vec<LogLine> {
    let mut lines: Vec<LogLine> = content
        .lines()
        .enumerate()
        .map(|(index, raw)| {
            let (stream, line) = if let Some(rest) = raw.strip_prefix("[stderr] ") {
                (LogStream::Stderr, rest)
            } else if let Some(rest) = raw.strip_prefix("[stdin] ") {
                (LogStream::Stdin, rest)
            } else if is_system_line(raw) {
                (LogStream::System, raw)
            } else {
                (LogStream::Stdout, raw)
            };
            LogLine {
                seq: index as u64 + 1,
                stream,
                line: line.to_string(),
                ts: Utc::now(),
            }
        })
        .collect();
    if tail > 0 && lines.len() > tail {
        lines = lines.split_off(lines.len() - tail);
    }
    lines
}

fn is_system_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    line.starts_with("queued:")
        || line.starts_with("running:")
        || line.starts_with("finished:")
        || line.starts_with("cancel")
        || lower.contains("orchestrator error")
        || lower.contains("dispatcher task panicked")
        || lower.contains("spawn error")
        || lower.contains("wait error")
        || lower.contains("speak warning")
}

#[cfg(unix)]
fn kill_process_group(pid: u32, signal: i32) {
    if pid == 0 {
        return;
    }
    // Negative pid targets the process group created with process_group(0).
    unsafe {
        libc::kill(-(pid as i32), signal);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32, _signal: i32) {}

/// Pick the last meaningful process-output line for the failure announcement.
fn last_error_line(job: &JobRecord) -> Option<String> {
    let line = job
        .log_tail
        .iter()
        .rev()
        .map(|l| l.trim())
        .map(|l| l.strip_prefix("[stderr] ").unwrap_or(l))
        .find(|l| {
            !l.is_empty()
                && !l.starts_with("queued:")
                && !l.starts_with("running:")
                && !l.starts_with("finished:")
                && !l.starts_with("[stdin]")
                && !l.starts_with("cancel")
                && !l.to_lowercase().contains("speak warning")
        })?;
    Some(truncate_for_speech(line, 200))
}

fn truncate_for_speech(text: &str, max_chars: usize) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut out: String = clean.chars().take(max_chars).collect();
    if clean.chars().count() > max_chars {
        out.push('…');
    }
    out.trim().to_string()
}
