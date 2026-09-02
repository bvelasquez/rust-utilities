use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
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
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd.spawn()
        .with_context(|| format!("spawn {} in {}", spec.program, spec.cwd.display()))
}

/// Read stdout/stderr, emitting complete lines and flushing prompt fragments
/// that never get a newline (typical `y/N` questions) after a short idle.
pub async fn pump_lines<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    tx: tokio::sync::mpsc::Sender<String>,
    prefix: &'static str,
) {
    use tokio::io::AsyncReadExt;
    use tokio::time::{timeout, Duration};

    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match timeout(Duration::from_millis(150), reader.read(&mut tmp)).await {
            Ok(Ok(0)) => {
                if !buf.is_empty() {
                    let line = String::from_utf8_lossy(&buf).into_owned();
                    let _ = tx.send(format!("{prefix}{line}")).await;
                }
                break;
            }
            Ok(Ok(n)) => {
                buf.extend_from_slice(&tmp[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let mut line: Vec<u8> = buf.drain(..=pos).collect();
                    if line.last() == Some(&b'\n') {
                        line.pop();
                    }
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    let s = String::from_utf8_lossy(&line).into_owned();
                    if tx.send(format!("{prefix}{s}")).await.is_err() {
                        return;
                    }
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {
                if !buf.is_empty() {
                    let s = String::from_utf8_lossy(&buf).into_owned();
                    buf.clear();
                    if tx.send(format!("{prefix}{s}")).await.is_err() {
                        return;
                    }
                }
            }
        }
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
