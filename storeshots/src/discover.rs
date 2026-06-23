use crate::config::{default_config, StoreshotsConfig, BRAND_DIR, RAW_DIR};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn resolve_app_root(app: Option<PathBuf>) -> Result<PathBuf> {
    let root = app.unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let root = root.canonicalize().unwrap_or(root);
    Ok(root)
}

pub fn init_app(app_root: &Path, name: Option<String>) -> Result<StoreshotsConfig> {
    let app_name = name.unwrap_or_else(|| infer_app_name(app_root));
    std::fs::create_dir_all(app_root.join(RAW_DIR)).context("create storeshots/raw")?;
    std::fs::create_dir_all(app_root.join(BRAND_DIR)).context("create storeshots/brand")?;

    let cfg = default_config(&app_name);
    if !app_root.join("storeshots.toml").exists() {
        cfg.save(app_root)?;
    }

    try_copy_icon(app_root);

    Ok(cfg)
}

fn infer_app_name(app_root: &Path) -> String {
    app_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("My App")
        .to_string()
}

fn try_copy_icon(app_root: &Path) {
    let dest = app_root.join("storeshots/brand/icon.png");
    if dest.exists() {
        return;
    }
    let candidates = [
        "ios/Runner/Assets.xcassets/AppIcon.appiconset/Icon-App-1024x1024@1x.png",
        "assets/icon.png",
        "assets/app_icon.png",
    ];
    for rel in candidates {
        let src = app_root.join(rel);
        if src.is_file() {
            let _ = std::fs::copy(&src, &dest);
            break;
        }
    }
}
