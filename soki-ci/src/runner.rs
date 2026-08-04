use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::config::{PackageManager, ResolvedProject, ResolvedTarget, RunnerConfig};

pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub env: Vec<(String, String)>,
}

pub fn build_spawn_spec(project: &ResolvedProject, target: &ResolvedTarget) -> Result<SpawnSpec> {
    let pm = project.package_manager;
    let (program, args) = match &target.runner {
        RunnerConfig::PnpmScript { script } => {
            if pm != PackageManager::Pnpm {
                // honor explicit runner kind over project default
            }
            ("pnpm".into(), vec!["run".into(), script.clone()])
        }
        RunnerConfig::NpmScript { script } => ("npm".into(), vec!["run".into(), script.clone()]),
        RunnerConfig::Make { target: make_target } => {
            ("make".into(), vec![make_target.clone()])
        }
        RunnerConfig::Shell { command } => {
            if command.is_empty() {
                bail!("empty shell command");
            }
            let program = command[0].clone();
            let args = command[1..].to_vec();
            (program, args)
        }
    };

    Ok(SpawnSpec {
        program,
        args,
        cwd: target.cwd.clone(),
        env: target
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    })
}

pub async fn spawn_deploy(spec: &SpawnSpec) -> Result<tokio::process::Child> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd.spawn()
        .with_context(|| format!("spawn {} in {}", spec.program, spec.cwd.display()))
}

pub async fn pump_lines<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    tx: tokio::sync::mpsc::Sender<String>,
    prefix: &'static str,
) {
    let mut lines = BufReader::new(&mut reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let _ = tx.send(format!("{prefix}{line}")).await;
    }
}

pub fn log_dir() -> Result<std::path::PathBuf> {
    let base = if let Some(proj) = directories::ProjectDirs::from("com.soki-ci", "soki-ci", "soki-ci")
    {
        proj.data_dir().to_path_buf()
    } else {
        let mut p = dirs::home_dir().context("home")?;
        p.push(".soki-ci");
        p
    };
    std::fs::create_dir_all(&base)?;
    Ok(base.join("runs"))
}

pub fn log_path_for(job_id: &uuid::Uuid) -> Result<std::path::PathBuf> {
    let dir = log_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{job_id}.log")))
}

pub async fn append_log(path: &Path, line: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    Ok(())
}
