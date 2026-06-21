mod archive;
mod backup;
mod config;
mod crypto;
mod discover;
mod filter;
mod git_status;
mod manifest;
mod output;
mod restore;
mod scan;

use anyhow::{bail, Result};
use backup::{collect_scan, run_backup};
use clap::{Parser, Subcommand};
use config::{load_scan_config, GlobalOpts};
use restore::{run_inspect, run_restore};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "secret-sweep",
    about = "Backup local-only secrets and dotfiles across projects into an encrypted archive",
    version
)]
struct Cli {
    #[command(flatten)]
    global: GlobalOpts,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List files that would be backed up (default)
    Scan {
        /// Write committed-secrets report to this path
        #[arg(long)]
        report: Option<PathBuf>,

        /// Exit code 1 if any committed secrets are found
        #[arg(long)]
        strict: bool,
    },
    /// Create an encrypted .svault archive
    Backup {
        /// Output archive path (default: ~/Backups/secret-sweep-<timestamp>.svault)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Committed-secrets report path (default: ~/Backups/secret-sweep-committed-report-<timestamp>.json)
        #[arg(long)]
        report: Option<PathBuf>,

        /// Archive password (avoid; prompts if omitted)
        #[arg(long, env = "SECRET_SWEEP_PASSWORD")]
        password: Option<String>,

        /// Show what would be archived without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Restore files from an encrypted archive
    Restore {
        /// Path to .svault archive
        archive: PathBuf,

        /// Override projects root for restore paths
        #[arg(long)]
        root: Option<PathBuf>,

        /// Restore only this project name
        #[arg(long)]
        project: Option<String>,

        /// Overwrite existing files
        #[arg(long)]
        force: bool,

        #[arg(long, env = "SECRET_SWEEP_PASSWORD")]
        password: Option<String>,

        #[arg(long)]
        dry_run: bool,
    },
    /// List contents of an archive (requires password)
    Inspect {
        archive: PathBuf,

        #[arg(long, env = "SECRET_SWEEP_PASSWORD")]
        password: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_scan_config(&cli.global)?;

    match cli.command {
        Commands::Scan { report, strict } => {
            let scan = collect_scan(&cfg)?;
            if cli.global.json {
                output::print_scan_json(&scan)?;
            } else {
                output::print_scan_human(&scan, true);
                output::print_committed_warnings(&scan);
            }

            if scan.committed_warning_count > 0 {
                let report_path = report.unwrap_or_else(backup::default_report_path);
                output::write_committed_report(&scan, &report_path)?;
                if !cli.global.json {
                    eprintln!(
                        "Committed-secrets report: {}",
                        report_path.display()
                    );
                }
            }

            if strict && scan.committed_warning_count > 0 {
                std::process::exit(1);
            }
        }
        Commands::Backup {
            output,
            report,
            password,
            dry_run,
        } => {
            let scan = collect_scan(&cfg)?;
            run_backup(
                scan,
                output,
                report,
                password,
                dry_run,
                cli.global.yes,
                cli.global.json,
            )?;
        }
        Commands::Restore {
            archive,
            root,
            project,
            force,
            password,
            dry_run,
        } => {
            if !archive.is_file() {
                bail!("archive not found: {}", archive.display());
            }
            run_restore(
                &archive,
                root,
                project,
                force,
                dry_run,
                cli.global.yes,
                password,
                cli.global.json,
            )?;
        }
        Commands::Inspect { archive, password } => {
            if !archive.is_file() {
                bail!("archive not found: {}", archive.display());
            }
            run_inspect(&archive, password, cli.global.json)?;
        }
    }

    Ok(())
}
