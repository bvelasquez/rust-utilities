use crate::ads::composite::{render_ad, AdRenderContext};
use crate::ads::formats::{formats_for_groups, AD_FORMAT_GROUPS, AD_FORMATS};
use crate::config::StoreshotsConfig;
use crate::export::write_png_rgb;
use crate::fonts::FontSet;
use crate::text_client::gemini_for_render;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdsRenderOutput {
    pub files: Vec<PathBuf>,
    pub ads_rendered: usize,
    pub formats_per_ad: usize,
}

pub async fn render_ads(
    app_root: &Path,
    cfg: &StoreshotsConfig,
    no_ai: bool,
    only_ids: &[String],
    only_formats: &[String],
    locale: &str,
) -> Result<AdsRenderOutput> {
    if cfg.ads.items.is_empty() {
        bail!("no ads configured; run `storeshots ads suggest --yes` first");
    }

    let use_ai = cfg.ai.backgrounds && !no_ai;
    let client = if use_ai {
        match gemini_for_render(app_root, cfg) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("warning: {e}; rendering with gradient backgrounds instead");
                None
            }
        }
    } else {
        None
    };
    let use_ai = use_ai && client.is_some();

    let fonts = FontSet::load(app_root, cfg.brand.font.as_deref())?;
    let out_base = cfg.ads_out_dir(app_root);
    std::fs::create_dir_all(&out_base)
        .with_context(|| format!("create ads output dir {}", out_base.display()))?;

    let ads: Vec<_> = if only_ids.is_empty() {
        cfg.ads.items.iter().collect()
    } else {
        cfg.ads
            .items
            .iter()
            .filter(|a| only_ids.iter().any(|id| id == &a.id))
            .collect()
    };

    if ads.is_empty() {
        bail!("no ads matched --only filter");
    }

    let mut written = Vec::new();
    let mut formats_count = 0usize;

    for (idx, ad) in ads.iter().enumerate() {
        let groups = if only_formats.is_empty() {
            ad.format_groups.clone()
        } else {
            only_formats.to_vec()
        };
        let formats = formats_for_groups(&groups);
        if formats.is_empty() {
            eprintln!("warning: ad '{}' has no matching formats; skipping", ad.id);
            continue;
        }
        formats_count = formats_count.max(formats.len());

        for format in formats {
            let rendered = render_ad(AdRenderContext {
                app_root,
                cfg,
                ad,
                format,
                use_ai,
                client: client.as_ref(),
                fonts: &fonts,
            })
            .await?;

            let subdir = out_base.join(format.platform).join(format.id);
            std::fs::create_dir_all(&subdir)?;
            let filename = format!(
                "{:02}-{}-{}-{}x{}.png",
                idx + 1,
                ad.id,
                locale,
                format.w,
                format.h
            );
            let out_path = subdir.join(filename);
            write_png_rgb(&out_path, &rendered)?;
            written.push(out_path);
        }
    }

    if written.is_empty() {
        bail!("no ad files were written");
    }

    Ok(AdsRenderOutput {
        files: written,
        ads_rendered: ads.len(),
        formats_per_ad: formats_count,
    })
}

pub fn validate_ads_output(app_root: &Path, cfg: &StoreshotsConfig) -> Vec<AdsValidationIssue> {
    let mut issues = Vec::new();
    let out_dir = cfg.ads_out_dir(app_root);
    if !out_dir.exists() {
        issues.push(AdsValidationIssue {
            path: out_dir.display().to_string(),
            message: "ads output directory does not exist".into(),
        });
        return issues;
    }

    for ad in &cfg.ads.items {
        let formats = formats_for_groups(&ad.format_groups);
        for format in formats {
            let subdir = out_dir.join(format.platform).join(format.id);
            let pattern = format!("-{}-", ad.id);
            let found = std::fs::read_dir(&subdir)
                .ok()
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.file_name()
                            .to_str()
                            .is_some_and(|n| n.contains(&pattern) && n.ends_with(".png"))
                    })
                })
                .unwrap_or(false);
            if !found {
                issues.push(AdsValidationIssue {
                    path: subdir.display().to_string(),
                    message: format!("missing output for ad '{}' at {}×{}", ad.id, format.w, format.h),
                });
            }
        }
    }
    issues
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdsValidationIssue {
    pub path: String,
    pub message: String,
}

pub fn formats_list_json() -> serde_json::Value {
    serde_json::json!({
        "formats": AD_FORMATS.iter().map(|f| serde_json::json!({
            "id": f.id,
            "title": f.title,
            "platform": f.platform,
            "group": f.group,
            "width": f.w,
            "height": f.h,
        })).collect::<Vec<_>>(),
        "groups": AD_FORMAT_GROUPS.iter().map(|(id, desc)| serde_json::json!({
            "id": id,
            "description": desc,
        })).collect::<Vec<_>>(),
        "usage": "storeshots ads render --yes",
    })
}

pub fn format_id_slice() -> Vec<&'static str> {
    crate::ads::formats::format_id_slice()
}
