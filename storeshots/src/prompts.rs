use crate::config::StoreshotsConfig;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// LLM phases that accept supplementary prompt appends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptPhase {
    Brand,
    Copy,
    #[allow(dead_code)]
    MobileBackground,
    PrintCopy,
    #[allow(dead_code)]
    Validate,
}

impl PromptPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Brand => "brand",
            Self::Copy => "copy",
            Self::MobileBackground => "mobile_background",
            Self::PrintCopy => "print_copy",
            Self::Validate => "validate",
        }
    }

    pub fn append_filename(self) -> &'static str {
        match self {
            Self::Brand => "brand.append.md",
            Self::Copy => "copy.append.md",
            Self::MobileBackground => "mobile-background.append.md",
            Self::PrintCopy => "print.append.md",
            Self::Validate => "validate.append.md",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PromptOverrides {
    pub append: Vec<String>,
    pub files: Vec<PathBuf>,
}

/// Resolved system prompt for one LLM call.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    pub system: String,
}

pub fn assemble_system(
    app_root: &Path,
    cfg: &StoreshotsConfig,
    phase: PromptPhase,
    default_system: &str,
    overrides: &PromptOverrides,
    item_append: Option<&str>,
) -> Result<AssembledPrompt> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(section) = cfg.ai.prompts.get(phase.as_str()) {
        if let Some(ref inline) = section.prompt_append {
            let trimmed = inline.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
        for rel in &section.prompt_files {
            parts.push(read_prompt_file(app_root, Path::new(rel.as_str()))?);
        }
    }

    let prompts_dir = cfg
        .paths
        .prompts_dir
        .as_deref()
        .unwrap_or("storeshots/prompts");
    let auto_path = app_root.join(prompts_dir).join(phase.append_filename());
    if auto_path.is_file() {
        parts.push(
            std::fs::read_to_string(&auto_path)
                .with_context(|| format!("read {}", auto_path.display()))?,
        );
    }

    for path in &overrides.files {
        parts.push(read_prompt_file(app_root, path)?);
    }

    for append in &overrides.append {
        let trimmed = append.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    if let Some(item) = item_append {
        let trimmed = item.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    let user_append = parts.join("\n\n");
    let system = if user_append.is_empty() {
        default_system.to_string()
    } else {
        format!("{default_system}\n\n--- Project-specific instructions ---\n{user_append}")
    };

    Ok(AssembledPrompt { system })
}

fn read_prompt_file(app_root: &Path, rel: &Path) -> Result<String> {
    let path = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        app_root.join(rel)
    };
    std::fs::read_to_string(&path).with_context(|| format!("read prompt file {}", path.display()))
}

pub fn overrides_from_cli(prompt_append: &[String], prompt_file: &[PathBuf]) -> PromptOverrides {
    PromptOverrides {
        append: prompt_append.to_vec(),
        files: prompt_file.to_vec(),
    }
}
