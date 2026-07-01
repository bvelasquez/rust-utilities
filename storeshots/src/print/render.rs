use crate::config::StoreshotsConfig;
use crate::export::write_png_rgb;
use crate::fonts::FontSet;
use crate::print::copy::resolve_print_copy;
use crate::print::draw::scale_canvas;
use crate::print::formats::{render_pages, FormatContext};
use crate::print::layout::PrintLayout;
use crate::print::pdf::{write_pdf, PdfPageSpec};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RenderOutput {
    pub format: String,
    pub files: Vec<PathBuf>,
}

pub fn render_format(
    app_root: &Path,
    cfg: &StoreshotsConfig,
    format: &str,
    variant: Option<&str>,
) -> Result<RenderOutput> {
    let copy = resolve_print_copy(app_root, cfg)?;
    let layout = PrintLayout::from_config(cfg);
    let fonts = FontSet::load(app_root, cfg.brand.font.as_deref())?;
    let ctx = FormatContext {
        app_root,
        cfg,
        copy: &copy,
        layout,
        fonts: &fonts,
    };

    let pages = render_pages(&ctx, format, variant)?;
    let out_dir = cfg.print_out_dir(app_root);
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("create print output dir {}", out_dir.display()))?;

    let slug = slugify(&cfg.app.name);
    let mut files = Vec::new();

    let mut pdf_pages = Vec::new();
    for page in &pages {
        let export_img = scale_canvas(&page.layout, ctx.layout.export_scale);
        let png_path = out_dir.join(format!("{}-{}.png", slug, page.name));
        write_png_rgb(&png_path, &export_img)?;
        files.push(png_path);

        pdf_pages.push(PdfPageSpec {
            image: export_img,
            width_in: page.width_in,
            height_in: page.height_in,
        });
    }

    let pdf_name = match format {
        "business-card" => format!("{slug}-business-card.pdf"),
        other => format!("{slug}-{other}.pdf"),
    };
    let pdf_path = out_dir.join(pdf_name);
    write_pdf(&pdf_path, &cfg.app.name, &pdf_pages)?;
    files.push(pdf_path);

    Ok(RenderOutput {
        format: format.into(),
        files,
    })
}

fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
