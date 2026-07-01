use crate::config::StoreshotsConfig;
use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

const MAX_FILE_BYTES: usize = 48_000;
const MAX_TOTAL_BYTES: usize = 120_000;

#[derive(Debug, Clone)]
pub struct CodeContext {
    pub files: Vec<(String, String)>,
    pub total_chars: usize,
}

pub fn scan_project(app_root: &Path, cfg: &StoreshotsConfig) -> Result<CodeContext> {
    let web_root = cfg.web_root_path(app_root);
    let scan_root = if web_root.is_dir() {
        web_root
    } else {
        app_root.to_path_buf()
    };

    let priority_patterns = [
        "tailwind.config.js",
        "tailwind.config.ts",
        "index.css",
        "src/index.css",
        "constants.ts",
        "src/constants.ts",
        "marketing.ts",
        "src/constants/marketing.ts",
        "README.md",
        "docs/BRAND.md",
    ];

    let content_patterns = [
        "LandingPage",
        "HomePage",
        "App.tsx",
        "layout.tsx",
        "index.html",
    ];

    let mut files: Vec<(String, String)> = Vec::new();
    let mut total = 0usize;

    for pat in priority_patterns {
        let path = scan_root.join(pat);
        if path.is_file() {
            try_add_file(&mut files, &mut total, &path, pat)?;
        }
        let alt = app_root.join(pat);
        if alt.is_file() && alt != path {
            try_add_file(&mut files, &mut total, &alt, pat)?;
        }
    }

    if total < MAX_TOTAL_BYTES {
        for entry in WalkDir::new(&scan_root)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.ends_with(".tsx")
                && !name.ends_with(".ts")
                && !name.ends_with(".css")
                && !name.ends_with(".html")
            {
                continue;
            }
            if path.to_string_lossy().contains("node_modules")
                || path.to_string_lossy().contains(".next")
                || path.to_string_lossy().contains("dist/")
            {
                continue;
            }
            let rel = path
                .strip_prefix(app_root)
                .unwrap_or(path)
                .display()
                .to_string();
            if files.iter().any(|(r, _)| r == &rel) {
                continue;
            }
            let interesting = content_patterns.iter().any(|p| rel.contains(p));
            if !interesting && !name.contains("Page") && !name.contains("Landing") {
                continue;
            }
            if try_add_file(&mut files, &mut total, path, &rel).is_err() {
                break;
            }
            if total >= MAX_TOTAL_BYTES {
                break;
            }
        }
    }

    Ok(CodeContext {
        files,
        total_chars: total,
    })
}

fn try_add_file(
    files: &mut Vec<(String, String)>,
    total: &mut usize,
    path: &Path,
    label: &str,
) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let truncated: String = text.chars().take(MAX_FILE_BYTES).collect();
    *total += truncated.len();
    files.push((label.to_string(), truncated));
    Ok(())
}

pub fn format_context(ctx: &CodeContext) -> String {
    let mut out = format!("Approx. {} characters of project context.\n", ctx.total_chars);
    for (path, content) in &ctx.files {
        out.push_str(&format!("\n\n### File: {path}\n```\n{content}\n```"));
    }
    out
}
