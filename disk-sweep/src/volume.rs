use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeStats {
    pub mount_path: PathBuf,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub used_ratio: f64,
}

/// Filesystem capacity for the volume containing `path` (macOS / Unix via statvfs).
pub fn stats_for_path(path: &Path) -> Result<VolumeStats> {
    let path = if path.exists() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
        .context("invalid path for statvfs")?;

    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        anyhow::bail!("statvfs failed for {}", path.display());
    }

    let block_size = stat.f_frsize as u128;
    let total_bytes = (stat.f_blocks as u128)
        .checked_mul(block_size)
        .context("total bytes overflow")?;
    let available_bytes = (stat.f_bavail as u128)
        .checked_mul(block_size)
        .context("available bytes overflow")?;
    let used_bytes = total_bytes.saturating_sub(available_bytes);

    let used_ratio = if total_bytes > 0 {
        used_bytes as f64 / total_bytes as f64
    } else {
        0.0
    };

    Ok(VolumeStats {
        mount_path: path,
        total_bytes: total_bytes.min(u64::MAX as u128) as u64,
        available_bytes: available_bytes.min(u64::MAX as u128) as u64,
        used_bytes: used_bytes.min(u64::MAX as u128) as u64,
        used_ratio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_for_root() {
        let stats = stats_for_path(Path::new("/")).expect("statvfs on /");
        assert!(stats.total_bytes > 0);
        assert!(stats.used_ratio >= 0.0 && stats.used_ratio <= 1.0);
    }
}
