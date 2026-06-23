use crate::background::{load_or_generate_background, parse_hex_color, BackgroundOptions};
use crate::config::{SlideItem, StoreshotsConfig};
use crate::devices::hero_phone_placement;
use crate::fonts::FontSet;
use crate::gemini::GeminiClient;
use crate::sizes::{IPHONE_DESIGN_H, IPHONE_DESIGN_W};
use anyhow::{Context, Result};
use image::{imageops, Rgba, RgbaImage};
use std::path::Path;

pub fn mockup_bytes() -> &'static [u8] {
    include_bytes!("../assets/mockup.png")
}

pub struct RenderContext<'a> {
    pub app_root: &'a Path,
    pub cfg: &'a StoreshotsConfig,
    pub slide: &'a SlideItem,
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub use_ai: bool,
    pub client: Option<&'a GeminiClient>,
    pub fonts: &'a FontSet,
}

pub async fn render_slide(ctx: RenderContext<'_>) -> Result<RgbaImage> {
    let mut canvas = load_or_generate_background(BackgroundOptions {
        app_root: ctx.app_root,
        slide: ctx.slide,
        brand: &ctx.cfg.brand,
        canvas_w: ctx.canvas_w,
        canvas_h: ctx.canvas_h,
        use_ai: ctx.use_ai,
        image_model: &ctx.cfg.ai.image_model,
        client: ctx.client,
    })
    .await?;

    draw_caption(&ctx, &mut canvas)?;

    let placement = hero_phone_placement(ctx.canvas_w, ctx.canvas_h);
    let raw_path = StoreshotsConfig::raw_path(ctx.app_root, &ctx.slide.raw);
    let screenshot = image::open(&raw_path)
        .with_context(|| format!("open screenshot {}", raw_path.display()))?
        .to_rgba8();

    let mockup = image::load_from_memory(mockup_bytes())
        .context("load mockup")?
        .to_rgba8();
    let mockup_scaled = imageops::resize(
        &mockup,
        placement.mockup.w,
        placement.mockup.h,
        imageops::FilterType::Lanczos3,
    );
    overlay_image_alpha(&mut canvas, &mockup_scaled, placement.mockup.x, placement.mockup.y);

    let screen = &placement.screen;
    let fitted = imageops::resize(
        &screenshot,
        screen.w,
        screen.h,
        imageops::FilterType::Lanczos3,
    );
    overlay_image(&mut canvas, &fitted, screen.x, screen.y);

  Ok(canvas)
}

fn draw_caption(ctx: &RenderContext<'_>, canvas: &mut RgbaImage) -> Result<()> {
    let c_w = ctx.canvas_w as f32;
    let fg = parse_hex_color(&ctx.cfg.brand.foreground).unwrap_or([23, 23, 23, 255]);
    let muted = parse_hex_color(
        ctx.cfg
            .brand
            .muted
            .as_deref()
            .unwrap_or("#6B7280"),
    )
    .unwrap_or([107, 114, 128, 255]);
    let accent = parse_hex_color(&ctx.cfg.brand.accent).unwrap_or([91, 124, 250, 255]);

    let pad_x = c_w * 0.08;
    let label_size = c_w * 0.028;
    let title_size = c_w * 0.095;
    let subtitle_size = c_w * 0.038;
    let y = c_w * 0.12;

    if !ctx.slide.label.is_empty() {
        ctx.fonts.draw_multiline(
            canvas,
            &[ctx.slide.label.as_str()],
            pad_x,
            y,
            label_size,
            Rgba(accent),
            1.1,
        )?;
    }

    let title_lines: Vec<&str> = ctx.slide.title.lines().collect();
    let title_y = y + label_size * 2.2;
    ctx.fonts.draw_multiline(
        canvas,
        &title_lines,
        pad_x,
        title_y,
        title_size,
        Rgba(fg),
        1.05,
    )?;

    if !ctx.slide.subtitle.is_empty() {
        let sub_y = title_y + title_size * title_lines.len() as f32 * 1.15 + title_size * 0.25;
        ctx.fonts.draw_multiline(
            canvas,
            &[ctx.slide.subtitle.as_str()],
            pad_x,
            sub_y,
            subtitle_size,
            Rgba(muted),
            1.2,
        )?;
    }

    Ok(())
}

fn overlay_image(canvas: &mut RgbaImage, src: &RgbaImage, x: u32, y: u32) {
    for sy in 0..src.height() {
        for sx in 0..src.width() {
            let dx = x + sx;
            let dy = y + sy;
            if dx < canvas.width() && dy < canvas.height() {
                canvas.put_pixel(dx, dy, *src.get_pixel(sx, sy));
            }
        }
    }
}

fn overlay_image_alpha(canvas: &mut RgbaImage, src: &RgbaImage, x: u32, y: u32) {
    for sy in 0..src.height() {
        for sx in 0..src.width() {
            let dx = x + sx;
            let dy = y + sy;
            if dx >= canvas.width() || dy >= canvas.height() {
                continue;
            }
            let sp = *src.get_pixel(sx, sy);
            if sp[3] == 0 {
                continue;
            }
            let dp = *canvas.get_pixel(dx, dy);
            let blended = alpha_blend(dp, sp);
            canvas.put_pixel(dx, dy, blended);
        }
    }
}

fn alpha_blend(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    if sa >= 1.0 {
        return src;
    }
    if sa <= 0.0 {
        return dst;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    let blend = |s: u8, d: u8| {
        let s = s as f32 / 255.0;
        let d = d as f32 / 255.0;
        ((s * sa + d * da * (1.0 - sa)) / out_a * 255.0).round() as u8
    };
    Rgba([
        blend(src[0], dst[0]),
        blend(src[1], dst[1]),
        blend(src[2], dst[2]),
        (out_a * 255.0).round() as u8,
    ])
}

pub fn design_canvas_for_iphone() -> (u32, u32) {
    (IPHONE_DESIGN_W, IPHONE_DESIGN_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mockup_embedded() {
        assert!(!mockup_bytes().is_empty());
    }
}
