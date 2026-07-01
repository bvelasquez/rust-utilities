use crate::config::StoreshotsConfig;
use anyhow::{bail, Result};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct BrandIssue {
    pub severity: String,
    pub message: String,
}

pub fn validate_brand_file(app_root: &Path, cfg: &StoreshotsConfig) -> Result<Vec<BrandIssue>> {
    let path = cfg.brand_path(app_root);
    let mut issues = Vec::new();

    if !path.is_file() {
        issues.push(BrandIssue {
            severity: "error".into(),
            message: format!("missing brand file: {}", path.display()),
        });
        return Ok(issues);
    }

    let text = std::fs::read_to_string(&path)?;
    let lower = text.to_lowercase();

    let required_sections = [
        ("product identity", "## product identity"),
        ("one-line", "one-line"),
        ("tagline", "tagline"),
        ("target audience", "target audience"),
    ];

    for (label, needle) in required_sections {
        if !lower.contains(needle) {
            issues.push(BrandIssue {
                severity: "warning".into(),
                message: format!("BRAND.md may be missing section: {label}"),
            });
        }
    }

    if text.len() < 500 {
        issues.push(BrandIssue {
            severity: "warning".into(),
            message: "BRAND.md seems very short (< 500 chars)".into(),
        });
    }

    Ok(issues)
}

pub fn ensure_valid(issues: &[BrandIssue]) -> Result<()> {
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == "error")
        .collect();
    if !errors.is_empty() {
        bail!(
            "brand validation failed: {}",
            errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(())
}
