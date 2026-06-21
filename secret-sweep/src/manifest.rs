use crate::scan::ScanResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub tool_version: String,
    pub root: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub project: String,
    pub repo_path: String,
    pub relative_path: String,
    pub archive_path: String,
    pub sha256: String,
    pub size: u64,
    pub git_status: String,
    pub is_dotfile: bool,
}

#[derive(Debug, Serialize)]
pub struct CommittedReport {
    pub generated_at: DateTime<Utc>,
    pub tool_version: String,
    pub root: String,
    pub warning_count: usize,
    pub warnings: Vec<CommittedReportEntry>,
}

#[derive(Debug, Serialize)]
pub struct CommittedReportEntry {
    pub project: String,
    pub repo_path: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub size: u64,
    pub is_dotfile: bool,
    pub recommendation: String,
}

pub fn manifest_from_scan(scan: &ScanResult) -> Manifest {
    let mut entries = Vec::new();
    for project in &scan.projects {
        for e in &project.backup_entries {
            entries.push(ManifestEntry {
                project: e.project.clone(),
                repo_path: e.repo_path.clone(),
                relative_path: e.relative_path.clone(),
                archive_path: format!("projects/{}/{}", e.project, e.relative_path),
                sha256: e.sha256.clone(),
                size: e.size,
                git_status: format!("{:?}", e.git_status).to_lowercase(),
                is_dotfile: e.is_dotfile,
            });
        }
    }
    Manifest {
        version: MANIFEST_VERSION,
        created_at: Utc::now(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        root: scan.root.clone(),
        entries,
    }
}

pub fn committed_report_from_scan(scan: &ScanResult) -> CommittedReport {
    let mut warnings = Vec::new();
    for project in &scan.projects {
        for w in &project.committed_warnings {
            warnings.push(CommittedReportEntry {
                project: w.project.clone(),
                repo_path: w.repo_path.clone(),
                relative_path: w.relative_path.clone(),
                absolute_path: w.absolute_path.clone(),
                size: w.size,
                is_dotfile: w.is_dotfile,
                recommendation: w.recommendation.clone(),
            });
        }
    }
    CommittedReport {
        generated_at: Utc::now(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        root: scan.root.clone(),
        warning_count: warnings.len(),
        warnings,
    }
}
