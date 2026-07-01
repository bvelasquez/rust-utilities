mod business_card;
pub mod registry;
mod single_landscape;
mod single_portrait;
mod trifold;

pub use registry::{format_id_slice, unknown_format_message, PRINT_FORMATS};

use crate::config::StoreshotsConfig;
use crate::fonts::FontSet;
use crate::print::copy::PrintCopy;
use crate::print::layout::PrintLayout;
use anyhow::Result;
use image::RgbaImage;
use std::path::Path;

pub struct FormatContext<'a> {
    pub app_root: &'a Path,
    pub cfg: &'a StoreshotsConfig,
    pub copy: &'a PrintCopy,
    pub layout: PrintLayout,
    pub fonts: &'a FontSet,
}

pub fn render_pages(ctx: &FormatContext<'_>, format: &str, variant: Option<&str>) -> Result<Vec<NamedPage>> {
    match format {
        "trifold" => trifold::render(ctx),
        "single-landscape" => single_landscape::render(ctx),
        "single-portrait" => single_portrait::render(ctx),
        "business-card" => business_card::render(ctx, variant.unwrap_or("both")),
        other => anyhow::bail!("{}", unknown_format_message(other)),
    }
}

pub struct NamedPage {
    pub name: String,
    pub layout: RgbaImage,
    pub width_in: f64,
    pub height_in: f64,
}
