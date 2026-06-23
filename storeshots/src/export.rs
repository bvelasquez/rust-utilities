use crate::sizes::ExportSize;
use anyhow::{Context, Result};
use image::{imageops, RgbaImage};
use std::path::Path;

pub fn write_png_rgb(path: &Path, img: &RgbaImage) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output dir {}", parent.display()))?;
    }
    let rgb = flatten_rgba(img);
    rgb.save(path)
        .with_context(|| format!("write PNG {}", path.display()))?;
    Ok(())
}

pub fn resize_to(img: &RgbaImage, size: &ExportSize) -> RgbaImage {
    if img.width() == size.w && img.height() == size.h {
        return img.clone();
    }
    imageops::resize(img, size.w, size.h, imageops::FilterType::Lanczos3)
}

fn flatten_rgba(img: &RgbaImage) -> image::RgbImage {
    let mut out = image::RgbImage::new(img.width(), img.height());
    for (x, y, pixel) in img.enumerate_pixels() {
        let a = pixel[3] as f32 / 255.0;
        let bg = 255.0;
        let r = (pixel[0] as f32 * a + bg * (1.0 - a)).round() as u8;
        let g = (pixel[1] as f32 * a + bg * (1.0 - a)).round() as u8;
        let b = (pixel[2] as f32 * a + bg * (1.0 - a)).round() as u8;
        out.put_pixel(x, y, image::Rgb([r, g, b]));
    }
    out
}

pub fn export_filename(
    index: usize,
    slide_id: &str,
    locale: &str,
    size: &ExportSize,
) -> String {
    format!(
        "{:02}-{slide_id}-{locale}-{}x{}.png",
        index + 1,
        size.w,
        size.h
    )
}
