use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::cli::DupKeep;
use crate::commands::CommandContext;
use crate::dups::{self, DupOptions, KeepPolicy};
use crate::output::Envelope;
use crate::scan::format_bytes;

pub fn run(
    ctx: &CommandContext,
    path: Option<PathBuf>,
    no_recursive: bool,
    sha256: bool,
    min_bytes: u64,
    cross_devices: bool,
    exclude: &[PathBuf],
    apply: bool,
    yes: bool,
    keep: DupKeep,
) -> Result<()> {
    let Some(path) = path else {
        if ctx.json || !io::stdout().is_terminal() {
            bail!("path required when stdout is not a terminal; or run `disk-sweep dups` in a terminal to browse");
        }
        return crate::ui::run_dups_standalone();
    };

    let opts = DupOptions {
        root: path,
        recursive: !no_recursive,
        min_bytes,
        confirm_sha256: sha256,
        cross_devices,
        exclude: exclude.to_vec(),
    };
    let (tx, rx) = sync_channel(16);
    let show_progress = !ctx.json && io::stderr().is_terminal();
    let report = if show_progress {
        let opts = opts.clone();
        let handle = std::thread::spawn(move || {
            dups::find_duplicates(&opts, &AtomicBool::new(false), Some(&tx))
        });
        let mut last = Instant::now();
        loop {
            while let Ok(event) = rx.try_recv() {
                if last.elapsed() >= Duration::from_millis(200) {
                    eprint!(
                        "\r{} {}/{} {}   ",
                        event.phase, event.current, event.total, event.detail
                    );
                    let _ = io::stderr().flush();
                    last = Instant::now();
                }
            }
            if handle.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        eprintln!();
        handle.join().map_err(|_| anyhow::anyhow!("duplicate scan panicked"))??
    } else {
        dups::find_duplicates(&opts, &AtomicBool::new(false), None)?
    };

    let policy = match keep {
        DupKeep::Shortest => KeepPolicy::Shortest,
        DupKeep::Newest => KeepPolicy::Newest,
        DupKeep::Oldest => KeepPolicy::Oldest,
    };
    let actions = dups::plan_deletes(&report, policy);
    let prune = if apply {
        confirm_delete(actions.len(), report.reclaimable_bytes(), yes)?;
        dups::apply_deletes(&report.root, &actions, true, None)?
    } else {
        dups::apply_deletes(&report.root, &actions, false, None)?
    };

    if ctx.json {
        Envelope::ok(
            "dups",
            serde_json::json!({
                "root": report.root,
                "recursive": report.recursive,
                "groups": report.groups,
                "stats": report.stats,
                "extra_files": report.extra_files(),
                "reclaimable_bytes": report.reclaimable_bytes(),
                "excluded": report.excluded,
                "prune": prune,
            }),
        )
        .print_json()?;
        return Ok(());
    }

    println!(
        "disk-sweep dups {} ({})",
        report.root.display(),
        if report.recursive { "recursive" } else { "top directory only" }
    );
    if !report.excluded.is_empty() {
        println!("  skipping {} folders", report.excluded.len());
        for path in &report.excluded {
            println!("    {}", path.display());
        }
    }
    println!(
        "  {} files, {} — {} groups, {} extra copies, {} reclaimable",
        report.stats.files_seen,
        format_bytes(report.stats.bytes_seen),
        report.groups.len(),
        report.extra_files(),
        format_bytes(report.reclaimable_bytes())
    );
    println!(
        "  hashed partial {} / full {} / sha256 {}",
        report.stats.partial_hashed, report.stats.full_hashed, report.stats.sha256_hashed
    );
    if !apply {
        println!("\nRead-only. Pass --apply to delete extra copies after typing delete.\n");
    }
    for (index, group) in report.groups.iter().enumerate() {
        println!(
            "group {}  {}  xxh64:{}  ({} copies)",
            index + 1,
            format_bytes(group.size_bytes),
            group.xxh64,
            group.files.len()
        );
        let keep_at = dups::keep_index(&group.files, policy);
        for (file_index, file) in group.files.iter().enumerate() {
            let mark = if file_index == keep_at { "KEEP" } else { "dup " };
            println!("  {mark}  {}", file.path.display());
        }
    }
    if apply {
        println!(
            "\nDeleted {} files ({})",
            prune.deleted,
            format_bytes(prune.bytes_freed)
        );
        if let Some(log) = &prune.log_path {
            println!("Log: {log}");
        }
        for err in &prune.errors {
            println!("error: {err}");
        }
    }
    Ok(())
}

fn confirm_delete(count: usize, bytes: u64, yes: bool) -> Result<()> {
    if count == 0 {
        bail!("no extra copies to delete");
    }
    if !io::stdout().is_terminal() && !yes {
        bail!("refusing to delete duplicates in non-interactive mode without --yes");
    }
    if io::stdout().is_terminal() && !yes {
        println!(
            "About to permanently delete {count} extra copies ({}). One copy of each file is kept.",
            format_bytes(bytes)
        );
        print!("Type 'delete' to confirm: ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.trim() != "delete" {
            bail!("aborted");
        }
    }
    Ok(())
}
