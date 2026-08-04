use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Semaphore};
use uuid::Uuid;

use crate::broker::BrokerClient;
use crate::config::{ResolvedConfig, ResolvedProject, ResolvedTarget, SpeakEvent, SpeakDefaults};
use crate::runner::{append_log, build_spawn_spec, log_path_for, pump_lines, spawn_deploy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobPhase {
    Queued,
    Running,
    Success,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobRecord {
    pub id: Uuid,
    pub project_id: String,
    pub project_name: String,
    pub target_id: String,
    pub target_label: String,
    pub phase: JobPhase,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub log_path: String,
    #[serde(skip)]
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

pub struct JobOrchestrator {
    inner: Arc<Mutex<OrchestratorInner>>,
    semaphore: Arc<Semaphore>,
    speak: SpeakDefaults,
    broker: BrokerClient,
    max_history: usize,
}

struct OrchestratorInner {
    jobs: HashMap<Uuid, JobRecord>,
    order: Vec<Uuid>,
    project_running: HashMap<String, Uuid>,
}

impl JobOrchestrator {
    pub fn new(cfg: &ResolvedConfig) -> Self {
        let speak = cfg.defaults.speak.clone();
        let broker = BrokerClient::new(speak.base_url.clone(), speak.voice.clone());
        let permits = cfg.defaults.max_parallel.max(1);
        Self {
            inner: Arc::new(Mutex::new(OrchestratorInner {
                jobs: HashMap::new(),
                order: Vec::new(),
                project_running: HashMap::new(),
            })),
            semaphore: Arc::new(Semaphore::new(permits)),
            speak,
            broker,
            max_history: 50,
        }
    }

    pub fn broker(&self) -> &BrokerClient {
        &self.broker
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
        g.jobs
            .values()
            .filter(|j| j.phase == JobPhase::Running || j.phase == JobPhase::Queued)
            .count()
    }

    pub async fn enqueue(
        &self,
        project: &ResolvedProject,
        target: &ResolvedTarget,
    ) -> Result<Uuid> {
        {
            let g = self.inner.lock().await;
            if g.project_running.contains_key(&project.id) {
                bail!(
                    "project `{}` already has a running or queued deploy",
                    project.id
                );
            }
        }

        let id = Uuid::new_v4();
        let log_path = log_path_for(&id)?;
        let mut record = JobRecord {
            id,
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            target_id: target.id.clone(),
            target_label: target.label.clone(),
            phase: JobPhase::Queued,
            started_at: None,
            finished_at: None,
            exit_code: None,
            log_path: log_path.to_string_lossy().into(),
            log_tail: VecDeque::new(),
        };
        record.push_log(format!(
            "queued: {} / {}",
            project.id, target.id
        ));

        {
            let mut g = self.inner.lock().await;
            g.jobs.insert(id, record);
            g.order.insert(0, id);
            g.project_running.insert(project.id.clone(), id);
            while g.order.len() > self.max_history {
                if let Some(old) = g.order.pop() {
                    g.jobs.remove(&old);
                }
            }
        }

        let spec = build_spawn_spec(project, target)?;
        let inner = self.inner.clone();
        let semaphore = self.semaphore.clone();
        let speak = self.speak.clone();
        let broker = self.broker.clone();
        let project_id = project.id.clone();
        let project_name = project.name.clone();
        let target_label = target.label.clone();
        let target_speak = target.speak;
        let log_path = log_path.clone();

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            if let Err(e) = run_job(
                id,
                spec,
                log_path,
                inner.clone(),
                project_id.clone(),
                project_name,
                target_label,
                target_speak,
                speak,
                broker,
            )
            .await
            {
                let mut g = inner.lock().await;
                if let Some(j) = g.jobs.get_mut(&id) {
                    j.phase = JobPhase::Error;
                    j.finished_at = Some(Utc::now());
                    j.push_log(format!("orchestrator error: {e}"));
                }
                g.project_running.remove(&project_id);
            }
        });

        Ok(id)
    }

    pub async fn cancel(&self, id: Uuid) -> Result<()> {
        // v1: mark cancelled; child kill wired via JobHandle map in future
        let mut g = self.inner.lock().await;
        let job = g.jobs.get_mut(&id).ok_or_else(|| anyhow!("job not found"))?;
        if job.phase != JobPhase::Running && job.phase != JobPhase::Queued {
            bail!("job is not running");
        }
        let project_id = job.project_id.clone();
        job.phase = JobPhase::Cancelled;
        job.finished_at = Some(Utc::now());
        g.project_running.remove(&project_id);
        Ok(())
    }
}

async fn run_job(
    id: Uuid,
    spec: crate::runner::SpawnSpec,
    log_path: std::path::PathBuf,
    inner: Arc<Mutex<OrchestratorInner>>,
    project_id: String,
    project_name: String,
    target_label: String,
    target_speak: Option<bool>,
    speak_defaults: SpeakDefaults,
    broker: BrokerClient,
) -> Result<()> {
    {
        let mut g = inner.lock().await;
        let job = g.jobs.get_mut(&id).expect("job");
        job.phase = JobPhase::Running;
        job.started_at = Some(Utc::now());
        job.push_log(format!("running: {} {:?}", spec.program, spec.args));
    }

    let mut child = spawn_deploy(&spec).await?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, mut rx) = mpsc::channel::<String>(256);
    let log_path_stdout = log_path.clone();
    let log_path_stderr = log_path.clone();
    let inner_log = inner.clone();
    let job_id = id;

    if let Some(out) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            pump_lines(out, tx, "").await;
        });
    }
    if let Some(err) = stderr {
        tokio::spawn(async move {
            pump_lines(err, tx, "[stderr] ").await;
        });
    }

    let pump = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            let _ = append_log(&log_path_stdout, &line).await;
            let mut g = inner_log.lock().await;
            if let Some(j) = g.jobs.get_mut(&job_id) {
                j.push_log(line);
            }
        }
    });

    let status = child.wait().await?;
    pump.await.ok();
    let code = status.code().unwrap_or(-1);
    let success = status.success();

    let phase = if success {
        JobPhase::Success
    } else {
        JobPhase::Error
    };

    let speak_enabled = target_speak.unwrap_or(speak_defaults.enabled);
    let should_speak = speak_enabled
        && ((success && speak_defaults.on.contains(&SpeakEvent::Success))
            || (!success && speak_defaults.on.contains(&SpeakEvent::Error)));

    {
        let mut g = inner.lock().await;
        let job = g.jobs.get_mut(&id).expect("job");
        job.phase = phase;
        job.finished_at = Some(Utc::now());
        job.exit_code = Some(code);
        job.push_log(format!("finished: exit {code}"));
        g.project_running.remove(&project_id);
    }

    if should_speak {
        let msg = if success {
            format!("{project_name} {target_label} deploy succeeded.")
        } else {
            format!("{project_name} {target_label} deploy failed with exit code {code}.")
        };
        if let Err(e) = broker.speak(&msg).await {
            let mut g = inner.lock().await;
            if let Some(j) = g.jobs.get_mut(&id) {
                j.push_log(format!("speak warning: {e}"));
            }
        }
    }

    let _ = log_path_stderr;
    Ok(())
}
