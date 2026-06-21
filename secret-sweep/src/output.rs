use crate::manifest::committed_report_from_scan;
use crate::scan::ScanResult;
use anyhow::Result;
use colored::Colorize;
use std::io::{self, Write};
use std::path::Path;

pub fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

pub fn print_scan_human(scan: &ScanResult, show_files: bool) {
    let mode = if scan.mine_only {
        "mine only".to_string()
    } else {
        "all repos".to_string()
    };
    println!(
        "Scanning {} ({}, {} repos, {} skipped)\n",
        scan.root, mode, scan.included_repos, scan.skipped_repos
    );

    for project in &scan.projects {
        let n = project.backup_entries.len();
        let w = project.committed_warnings.len();
        if n == 0 && w == 0 {
            continue;
        }
        println!("{}", project.name.bold());
        println!("  {}", project.repo_path.dimmed());

        if show_files {
            for e in &project.backup_entries {
                println!(
                    "  {} {} ({:?}, {} bytes)",
                    "backup".green(),
                    e.relative_path,
                    e.git_status,
                    e.size
                );
            }
        } else if n > 0 {
            println!("  {} file(s) to back up", n);
        }

        for w in &project.committed_warnings {
            println!(
                "  {} {} (COMMITTED — not backed up)",
                "!!".red().bold(),
                w.relative_path.red().bold()
            );
        }
        println!();
    }

    println!(
        "{}",
        format!(
            "Summary: {} file(s) to back up, {} committed-secret warning(s)",
            scan.backup_count, scan.committed_warning_count
        )
        .bold()
    );
}

pub fn print_committed_warnings(scan: &ScanResult) {
    if scan.committed_warning_count == 0 {
        return;
    }

    eprintln!("\n{}", "═".repeat(72).red());
    eprintln!(
        "{}",
        "  WARNING: SECRETS COMMITTED TO GIT — NOT INCLUDED IN BACKUP"
            .red()
            .bold()
    );
    eprintln!(
        "{}",
        "  Rotate these credentials and remove them from git history."
            .red()
    );
    eprintln!("{}\n", "═".repeat(72).red());

    for project in &scan.projects {
        for w in &project.committed_warnings {
            eprintln!(
                "  {} / {}",
                project.name.red().bold(),
                w.relative_path.red().bold()
            );
            eprintln!("    {}", w.absolute_path.dimmed());
            eprintln!("    {}", w.recommendation.yellow());
        }
    }
    eprintln!();
}

pub fn write_committed_report(scan: &ScanResult, path: &Path) -> Result<()> {
    let report = committed_report_from_scan(scan);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn print_scan_json(scan: &ScanResult) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(scan)?);
    Ok(())
}
