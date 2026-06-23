use crate::sizes::IPHONE_SIZES;
use image::GenericImageView;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

pub fn validate_outputs(out_dir: &Path) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if !out_dir.exists() {
        issues.push(ValidationIssue {
            path: out_dir.display().to_string(),
            message: "output directory does not exist; run storeshots render first".into(),
        });
        return issues;
    }

    for entry in walkdir::WalkDir::new(out_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
    {
        let path = entry.path();
        match validate_png(path) {
            Ok(()) => {}
            Err(msg) => issues.push(ValidationIssue {
                path: path.display().to_string(),
                message: msg,
            }),
        }
    }

    issues
}

fn validate_png(path: &Path) -> Result<(), String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    let known = IPHONE_SIZES.iter().any(|s| s.w == w && s.h == h);
    if !known {
        return Err(format!("unexpected dimensions {w}x{h} for iPhone export"));
    }
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    Ok(())
}
