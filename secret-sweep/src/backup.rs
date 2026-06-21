use crate::archive::create_inner_zip;
use crate::config::{default_backups_dir, ScanConfig};
use crate::crypto::write_vault;
use crate::manifest::manifest_from_scan;
use crate::output::{print_committed_warnings, print_scan_human, write_committed_report};
use crate::scan::{run_scan, ScanResult};
use anyhow::{bail, Context, Result};
use chrono::Local;
use colored::Colorize;
use std::path::PathBuf;

pub fn default_archive_path() -> PathBuf {
    let stamp = Local::now().format("%Y-%m-%d_%H%M%S");
    default_backups_dir().join(format!("secret-sweep-{stamp}.svault"))
}

pub fn default_report_path() -> PathBuf {
    let stamp = Local::now().format("%Y-%m-%d_%H%M%S");
    default_backups_dir().join(format!("secret-sweep-committed-report-{stamp}.json"))
}

pub fn prompt_password_confirm() -> Result<String> {
    let p1 = rpassword::prompt_password("Archive password: ")?;
    let p2 = rpassword::prompt_password("Confirm password: ")?;
    if p1 != p2 {
        bail!("passwords do not match");
    }
    if p1.len() < 8 {
        bail!("password must be at least 8 characters");
    }
    Ok(p1)
}

pub fn prompt_password() -> Result<String> {
    rpassword::prompt_password("Archive password: ").context("read password")
}

pub fn run_backup(
    scan: ScanResult,
    output: Option<PathBuf>,
    report_path: Option<PathBuf>,
    password: Option<String>,
    dry_run: bool,
    yes: bool,
    json: bool,
) -> Result<PathBuf> {
    if !json {
        print_scan_human(&scan, true);
        print_committed_warnings(&scan);
    }

    if scan.backup_count == 0 && scan.committed_warning_count == 0 {
        if !json {
            println!("Nothing to back up.");
        }
        return Ok(output.unwrap_or_else(default_archive_path));
    }

    let report_out = report_path.unwrap_or_else(default_report_path);
    if scan.committed_warning_count > 0 {
        write_committed_report(&scan, &report_out)?;
        if !json {
            eprintln!(
                "\n{} Committed-secrets report written to {}",
                "REPORT".red().bold(),
                report_out.display()
            );
        }
    }

    if scan.backup_count == 0 {
        if !json {
            println!("No local-only files to archive (see committed-secrets report).");
        }
        return Ok(output.unwrap_or_else(default_archive_path));
    }

    let out_path = output.unwrap_or_else(default_archive_path);

    if dry_run {
        if !json {
            println!("\n[dry-run] would write archive to {}", out_path.display());
        }
        return Ok(out_path);
    }

    if !yes && !json {
        if !crate::output::confirm(&format!(
            "Back up {} file(s) to {}?",
            scan.backup_count,
            out_path.display()
        ))? {
            bail!("aborted");
        }
    }

    let password = match password {
        Some(p) => p,
        None => prompt_password_confirm()?,
    };

    let manifest = manifest_from_scan(&scan);
    let mut files = Vec::new();
    for project in &scan.projects {
        for entry in &project.backup_entries {
            files.push((
                format!("projects/{}/{}", entry.project, entry.relative_path),
                entry.absolute_path.clone(),
            ));
        }
    }

    let inner = create_inner_zip(&manifest, &files)?;
    write_vault(&out_path, &inner, &password)?;

    if json {
        let report = serde_json::json!({
            "archive": out_path.display().to_string(),
            "files": scan.backup_count,
            "committed_warnings": scan.committed_warning_count,
            "committed_report": if scan.committed_warning_count > 0 {
                Some(report_out.display().to_string())
            } else {
                None
            },
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "\n{} Archive written to {}",
            "✓".green().bold(),
            out_path.display()
        );
    }

    Ok(out_path)
}

pub fn collect_scan(cfg: &ScanConfig) -> Result<ScanResult> {
    let all_repos = crate::discover::find_git_repos(&cfg.root, cfg.depth);
    let (included, skipped) = crate::filter::partition_repos(all_repos, &cfg.filter);
    run_scan(cfg, &included, skipped)
}
