use crate::config::{
    default_config, default_pipeline_toml, StoreshotsConfig, ASSETS_DIR, CONFIG_FILE, DEFAULT_BRAND_MD,
    PROMPTS_DIR, RAW_DIR, SECRETS_EXAMPLE_FILE, SECRETS_FILE,
};
use crate::keys;
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
    std::fs::create_dir_all(app_root.join(PROMPTS_DIR)).context("create storeshots/prompts")?;
    std::fs::create_dir_all(app_root.join(ASSETS_DIR)).context("create storeshots/assets")?;

    scaffold_prompt_stubs(app_root)?;
    scaffold_secrets_example(app_root)?;
    ensure_gitignore_secrets(app_root);

    let brand_md = app_root.join(DEFAULT_BRAND_MD);
    if !brand_md.exists() {
        if let Some(parent) = brand_md.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &brand_md,
            format!(
                "# {app_name} — Brand Guide\n\n> Run `storeshots brand extract --yes` to generate from source code.\n"
            ),
        )?;
    }

    let config_path = app_root.join(CONFIG_FILE);
    if !config_path.exists() {
        let mut cfg = default_config(&app_name);
        if app_name.to_lowercase().contains("soki") || infer_app_name(app_root).contains("soki-creative") {
            cfg.app.kind = "company-site".into();
        }
        cfg.save(app_root)?;
    }
    append_pipeline_to_config(app_root)?;

    try_copy_icon(app_root);

    let cfg = if app_root.join(CONFIG_FILE).exists() {
        StoreshotsConfig::load_relaxed(app_root)?
    } else {
        default_config(&app_name)
    };

    Ok(cfg)
}

fn scaffold_prompt_stubs(app_root: &Path) -> Result<()> {
    let stubs = [
        (
            "brand.append.md",
            "# Brand extract instructions\n\n<!-- Add project-specific rules for LLM brand extraction -->\n",
        ),
        (
            "copy.append.md",
            "# Copy suggest instructions\n\n<!-- Pain-first messaging, tone, words to avoid -->\n",
        ),
        (
            "mobile-background.append.md",
            "# Mobile background instructions\n\n<!-- Mood, gradient preferences -->\n",
        ),
        (
            "print.append.md",
            "# Print copy instructions\n\n<!-- Brochure/card tone and CTA rules -->\n",
        ),
    ];
    for (name, content) in stubs {
        let path = app_root.join(PROMPTS_DIR).join(name);
        if !path.exists() {
            std::fs::write(path, content)?;
        }
    }
    Ok(())
}

fn scaffold_secrets_example(app_root: &Path) -> Result<()> {
    let example = app_root.join(SECRETS_EXAMPLE_FILE);
    if !example.exists() {
        std::fs::write(example, keys::secrets_example_toml())?;
    }
    Ok(())
}

fn ensure_gitignore_secrets(app_root: &Path) {
    let gitignore = app_root.join(".gitignore");
    let line = SECRETS_FILE;
    if gitignore.is_file() {
        if let Ok(text) = std::fs::read_to_string(&gitignore) {
            if text.lines().any(|l| l.trim() == line) {
                return;
            }
            let mut updated = text;
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(line);
            updated.push('\n');
            let _ = std::fs::write(gitignore, updated);
        }
    }
}

fn append_pipeline_to_config(app_root: &Path) -> Result<()> {
    let path = app_root.join(CONFIG_FILE);
    let mut text = std::fs::read_to_string(&path)?;
    if !text.contains("[pipeline") {
        text.push('\n');
        text.push_str(&default_pipeline_toml());
        std::fs::write(path, text)?;
    }
    Ok(())
}

fn infer_app_name(app_root: &Path) -> String {
    app_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("My App")
        .to_string()
}

fn try_copy_icon(app_root: &Path) {
    let dest = app_root.join("storeshots/assets/icon.png");
    if dest.exists() {
        return;
    }
    let candidates = [
        "ios/Runner/Assets.xcassets/AppIcon.appiconset/Icon-App-1024x1024@1x.png",
        "assets/icon.png",
        "assets/app_icon.png",
        "public/logo/logo.jpg",
    ];
    for rel in candidates {
        let src = app_root.join(rel);
        if src.is_file() {
            let _ = std::fs::copy(&src, &dest);
            break;
        }
    }
}
