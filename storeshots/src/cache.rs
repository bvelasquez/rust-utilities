use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn cache_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("resolve cache dir")?;
    Ok(base.join("storeshots"))
}

pub fn background_cache_path(app_root: &Path, slide_id: &str, theme: &str, prompt_hash: &str) -> PathBuf {
    let app_key = hash_path(app_root);
    cache_dir()
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(app_key)
        .join("backgrounds")
        .join(slide_id)
        .join(format!("{theme}-{prompt_hash}.png"))
}

pub fn hash_prompt(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for p in parts {
        hasher.update(p.as_bytes());
    }
    format!("{:x}", hasher.finalize())[..16].to_string()
}

fn hash_path(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

pub fn read_cached_png(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

pub fn write_cached_png(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create cache dir {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("write cache {}", path.display()))?;
    Ok(())
}
