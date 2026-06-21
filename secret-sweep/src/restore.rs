use crate::archive::extract_inner_zip;
use crate::crypto::read_vault;
use crate::manifest::Manifest;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, serde::Serialize)]
pub struct RestoreResult {
    pub restored: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

pub fn run_restore(
    archive: &Path,
    root_override: Option<PathBuf>,
    project_filter: Option<String>,
    force: bool,
    dry_run: bool,
    yes: bool,
    password: Option<String>,
    json: bool,
) -> Result<RestoreResult> {
    let password = match password {
        Some(p) => p,
        None => crate::backup::prompt_password()?,
    };

    let inner = read_vault(archive, &password)?;
    let temp = tempfile::tempdir().context("temp dir")?;
    let manifest = extract_inner_zip(&inner, temp.path())?;

    let entries: Vec<_> = manifest
        .entries
        .iter()
        .filter(|e| {
            project_filter
                .as_ref()
                .map(|p| &e.project == p)
                .unwrap_or(true)
        })
        .collect();

    if entries.is_empty() {
        bail!("no matching entries in archive");
    }

    if !json {
        println!(
            "Restore {} file(s) from archive created {}",
            entries.len(),
            manifest.created_at
        );
        for e in &entries {
            let dest = resolve_dest(&manifest, e, root_override.as_deref())?;
            println!("  {} -> {}", e.relative_path, dest.display());
        }
    }

    if dry_run {
        return Ok(RestoreResult {
            restored: entries.len(),
            skipped: 0,
            errors: vec!["dry-run".into()],
        });
    }

    if !yes && !json {
        if !crate::output::confirm("Proceed with restore?")? {
            bail!("aborted");
        }
    }

    let mut restored = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    for entry in entries {
        let dest = match resolve_dest(&manifest, entry, root_override.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("{}: {e:#}", entry.relative_path));
                continue;
            }
        };

        if dest.exists() && !force {
            skipped += 1;
            errors.push(format!("{}: exists (use --force)", dest.display()));
            continue;
        }

        let src = temp.path().join(&entry.archive_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        match restore_file(&src, &dest, &entry.sha256) {
            Ok(()) => restored += 1,
            Err(e) => errors.push(format!("{}: {e:#}", entry.relative_path)),
        }
    }

    let result = RestoreResult {
        restored,
        skipped,
        errors,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "\nDone: {} restored, {} skipped, {} error(s)",
            result.restored, result.skipped, result.errors.len()
        );
    }

    Ok(result)
}

pub fn run_inspect(archive: &Path, password: Option<String>, json: bool) -> Result<Manifest> {
    let password = match password {
        Some(p) => p,
        None => crate::backup::prompt_password()?,
    };
    let inner = read_vault(archive, &password)?;
    let temp = tempfile::tempdir()?;
    let manifest = extract_inner_zip(&inner, temp.path())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
    } else {
        println!(
            "Archive from {} ({} file(s))",
            manifest.created_at,
            manifest.entries.len()
        );
        for e in &manifest.entries {
            println!(
                "  [{}] {} ({}, {})",
                e.project, e.relative_path, e.size, e.git_status
            );
        }
    }

    Ok(manifest)
}

fn resolve_dest(
    manifest: &Manifest,
    entry: &crate::manifest::ManifestEntry,
    root_override: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(root) = root_override {
        return Ok(root.join(&entry.project).join(&entry.relative_path));
    }
    let repo = PathBuf::from(&entry.repo_path);
    if repo.exists() {
        return Ok(repo.join(&entry.relative_path));
    }
    let manifest_root = PathBuf::from(&manifest.root);
    Ok(manifest_root.join(&entry.project).join(&entry.relative_path))
}

fn restore_file(src: &Path, dest: &Path, expected_sha: &str) -> Result<()> {
    let mut file = fs::File::open(src)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut data = Vec::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        data.extend_from_slice(&buf[..n]);
    }
    let hash = format!("{:x}", hasher.finalize());
    if hash != expected_sha {
        bail!("checksum mismatch (expected {expected_sha}, got {hash})");
    }

    fs::write(dest, &data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dest, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
