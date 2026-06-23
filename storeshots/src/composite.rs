use crate::background::{load_or_generate_background, parse_hex_color, BackgroundOptions};
use crate::config::{SlideItem, StoreshotsConfig};
use crate::devices::{hero_phone_placement, Rect, SC_RX, SC_RY};
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

    let caption = measure_caption(&ctx);
    let lum = caption_region_luminance(&canvas, &caption);
    draw_caption_scrim(&mut canvas, &caption, lum, &ctx.cfg.brand.background);
    let caption_bottom = draw_caption_text(&ctx, &mut canvas, &caption, lum)?;

    let placement = hero_phone_placement(ctx.canvas_w, ctx.canvas_h, caption_bottom);
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

    let screen = &placement.screen;
    let bleed = ((screen.w as f32) * 0.003).max(1.5).round() as u32;
    let draw_w = screen.w + bleed * 2;
    let draw_h = screen.h + bleed * 2;
    let draw_x = screen.x.saturating_sub(bleed);
    let draw_y = screen.y.saturating_sub(bleed);

    let fitted = resize_cover_top(&screenshot, draw_w, draw_h);
    let clipped = clip_rounded_rect(&fitted, SC_RX, SC_RY);
    overlay_image_alpha(&mut canvas, &clipped, draw_x, draw_y);

    overlay_mockup_frame(
        &mut canvas,
        &mockup_scaled,
        placement.mockup.x,
        placement.mockup.y,
        screen,
    );

    Ok(canvas)
}

/// Caption layout metrics (computed before drawing).
struct CaptionLayout {
    pad_x: f32,
    top: f32,
    bottom: f32,
    width: f32,
    label_size: f32,
    title_size: f32,
    subtitle_size: f32,
    title_lines: Vec<String>,
    has_label: bool,
    has_subtitle: bool,
}

fn measure_caption(ctx: &RenderContext<'_>) -> CaptionLayout {
    let c_w = ctx.canvas_w as f32;
    let pad_x = c_w * 0.08;
    let label_size = c_w * 0.028;
    let title_size = c_w * 0.095;
    let subtitle_size = c_w * 0.038;
    let top = c_w * 0.10;

    let mut cursor_y = top;
    let has_label = !ctx.slide.label.is_empty();
    if has_label {
        cursor_y += label_size * 1.65;
    }

    let title_lines: Vec<String> = ctx.slide.title.lines().map(str::to_string).collect();
    cursor_y += c_w * 0.042;

    cursor_y += title_size * title_lines.len().max(1) as f32 * 1.05;

    let has_subtitle = !ctx.slide.subtitle.is_empty();
    if has_subtitle {
        cursor_y += title_size * 0.18;
        cursor_y += subtitle_size * 1.25;
    }

    CaptionLayout {
        pad_x,
        top,
        bottom: cursor_y,
        width: c_w * 0.9,
        label_size,
        title_size,
        subtitle_size,
        title_lines,
        has_label,
        has_subtitle,
    }
}

/// Relative luminance 0–1 in the caption text region.
fn caption_region_luminance(canvas: &RgbaImage, cap: &CaptionLayout) -> f32 {
    let x0 = cap.pad_x.floor().max(0.0) as u32;
    let y0 = (cap.top - cap.label_size).floor().max(0.0) as u32;
    let x1 = (cap.pad_x + cap.width).min(canvas.width() as f32) as u32;
    let y1 = (cap.bottom + cap.subtitle_size * 0.4)
        .min(canvas.height() as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }

    let mut sum = 0.0f64;
    let mut n = 0u64;
    let step = ((x1 - x0) / 24).max(1);
    let ystep = ((y1 - y0) / 16).max(1);

    for y in (y0..y1).step_by(ystep as usize) {
        for x in (x0..x1).step_by(step as usize) {
            let p = canvas.get_pixel(x, y);
            let r = p[0] as f64 / 255.0;
            let g = p[1] as f64 / 255.0;
            let b = p[2] as f64 / 255.0;
            sum += 0.2126 * r + 0.7152 * g + 0.0722 * b;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        (sum / n as f64) as f32
    }
}

/// Darken the caption column when the background is bright behind text.
fn draw_caption_scrim(canvas: &mut RgbaImage, cap: &CaptionLayout, luminance: f32, brand_bg: &str) {
    let tint = parse_hex_color(brand_bg).unwrap_or([11, 13, 18, 255]);
    let base = 0.38f32;
    let extra = ((luminance - 0.28) * 1.35).max(0.0);
    let strength = (base + extra).min(0.92);

    let x0 = 0u32;
    let y0 = cap.top.floor().max(0.0) as u32;
    let x1 = (cap.pad_x + cap.width + cap.pad_x * 0.5)
        .min(canvas.width() as f32) as u32;
    let y1 = (cap.bottom + cap.subtitle_size * 0.55)
        .min(canvas.height() as f32) as u32;
    let h = (y1.saturating_sub(y0)).max(1) as f32;

    for y in y0..y1 {
        let t = (y - y0) as f32 / h;
        // Strongest at top; fade out toward phone.
        let fade = (1.0 - t).powf(1.35);
        let alpha = (strength * (0.65 + 0.35 * fade) * 255.0).round() as u8;
        if alpha == 0 {
            continue;
        }
        let scrim = Rgba([tint[0], tint[1], tint[2], alpha]);
        for x in x0..x1 {
            let dst = *canvas.get_pixel(x, y);
            canvas.put_pixel(x, y, alpha_blend(dst, scrim));
        }
    }
}

fn adapt_subtitle_color(muted: [u8; 4], fg: [u8; 4], luminance: f32) -> Rgba<u8> {
    let t = ((luminance - 0.32) * 1.6).clamp(0.0, 1.0);
    let blend = |m: u8, f: u8| -> u8 {
        let t = t * 0.75;
        (m as f32 + (f as f32 - m as f32) * t).round() as u8
    };
    Rgba([
        blend(muted[0], fg[0]),
        blend(muted[1], fg[1]),
        blend(muted[2], fg[2]),
        255,
    ])
}

/// Returns the Y coordinate (px) just below the caption block.
fn draw_caption_text(
    ctx: &RenderContext<'_>,
    canvas: &mut RgbaImage,
    cap: &CaptionLayout,
    luminance: f32,
) -> Result<f32> {
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
    let subtitle_color = adapt_subtitle_color(muted, fg, luminance);

    let mut cursor_y = cap.top;

    if cap.has_label {
        ctx.fonts.draw_multiline(
            canvas,
            &[ctx.slide.label.as_str()],
            cap.pad_x,
            cursor_y,
            cap.label_size,
            Rgba(accent),
            1.1,
        )?;
        cursor_y += cap.label_size * 1.65;
    }

    cursor_y += ctx.canvas_w as f32 * 0.042;

    let title_line_refs: Vec<&str> = cap.title_lines.iter().map(String::as_str).collect();
    ctx.fonts.draw_multiline(
        canvas,
        &title_line_refs,
        cap.pad_x,
        cursor_y,
        cap.title_size,
        Rgba(fg),
        1.05,
    )?;
    cursor_y += cap.title_size * cap.title_lines.len().max(1) as f32 * 1.05;

    if cap.has_subtitle {
        cursor_y += cap.title_size * 0.18;
        let shadow = Rgba([0, 0, 0, (120.0 + luminance * 80.0) as u8]);
        let shadow_off = (ctx.canvas_w as f32 * 0.003).max(2.0);
        ctx.fonts.draw_multiline(
            canvas,
            &[ctx.slide.subtitle.as_str()],
            cap.pad_x + shadow_off,
            cursor_y + shadow_off,
            cap.subtitle_size,
            shadow,
            1.2,
        )?;
        ctx.fonts.draw_multiline(
            canvas,
            &[ctx.slide.subtitle.as_str()],
            cap.pad_x,
            cursor_y,
            cap.subtitle_size,
            subtitle_color,
            1.2,
        )?;
        cursor_y += cap.subtitle_size * 1.25;
    }

    Ok(cursor_y)
}

/// Draw the device bezel over the screenshot; skip opaque screen-fill pixels so UI shows through.
fn overlay_mockup_frame(
    canvas: &mut RgbaImage,
    mockup: &RgbaImage,
    mx: u32,
    my: u32,
    screen: &Rect,
) {
    for sy in 0..mockup.height() {
        for sx in 0..mockup.width() {
            let dx = mx + sx;
            let dy = my + sy;
            if dx >= canvas.width() || dy >= canvas.height() {
                continue;
            }
            let sp = *mockup.get_pixel(sx, sy);
            if sp[3] == 0 {
                continue;
            }

            let in_screen = dx >= screen.x
                && dx < screen.x + screen.w
                && dy >= screen.y
                && dy < screen.y + screen.h;

            if in_screen && is_screen_fill(sp) {
                continue;
            }

            let dp = *canvas.get_pixel(dx, dy);
            canvas.put_pixel(dx, dy, alpha_blend(dp, sp));
        }
    }
}

fn is_screen_fill(p: Rgba<u8>) -> bool {
    p[3] > 160 && p[0] < 48 && p[1] < 48 && p[2] < 48
}

/// Scale and crop to exactly fill the target rect (no letterboxing).
fn resize_cover_top(img: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let sw = img.width().max(1) as f32;
    let sh = img.height().max(1) as f32;
    let scale = (tw as f32 / sw).max(th as f32 / sh);
    let nw = (sw * scale).ceil() as u32;
    let nh = (sh * scale).ceil() as u32;
    let resized = imageops::resize(img, nw, nh, imageops::FilterType::Lanczos3);
    let x0 = nw.saturating_sub(tw) / 2;
    // Match skill `object-position: top` so status bar stays under the bezel.
    let y0 = 0;
    imageops::crop_imm(&resized, x0, y0, tw, th).to_image()
}

/// Rounded-rect alpha mask so screenshot corners sit cleanly under the bezel.
fn clip_rounded_rect(img: &RgbaImage, rx_frac: f32, ry_frac: f32) -> RgbaImage {
    let w = img.width();
    let h = img.height();
    let rx = (w as f32 * rx_frac).max(1.0);
    let ry = (h as f32 * ry_frac).max(1.0);
    let mut out = img.clone();

    for y in 0..h {
        for x in 0..w {
            let xf = x as f32 + 0.5;
            let yf = y as f32 + 0.5;
            if !inside_rounded_rect(xf, yf, w as f32, h as f32, rx, ry) {
                out.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }
    out
}

fn inside_rounded_rect(x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) -> bool {
    let qx = if x < rx {
        rx - x
    } else if x > w - rx {
        x - (w - rx)
    } else {
        0.0
    };
    let qy = if y < ry {
        ry - y
    } else if y > h - ry {
        y - (h - ry)
    } else {
        0.0
    };
    if qx == 0.0 || qy == 0.0 {
        return true;
    }
    (qx * qx) / (rx * rx) + (qy * qy) / (ry * ry) <= 1.0
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

    #[test]
    fn rounded_clip_preserves_center() {
        let img = RgbaImage::from_pixel(100, 100, Rgba([255, 0, 0, 255]));
        let clipped = clip_rounded_rect(&img, 0.1, 0.1);
        assert_eq!(*clipped.get_pixel(50, 50), Rgba([255, 0, 0, 255]));
        assert_eq!(clipped.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn bright_region_gets_stronger_scrim() {
        let mut bright = RgbaImage::from_pixel(400, 400, Rgba([240, 240, 250, 255]));
        let cap = CaptionLayout {
            pad_x: 32.0,
            top: 40.0,
            bottom: 200.0,
            width: 320.0,
            label_size: 12.0,
            title_size: 40.0,
            subtitle_size: 16.0,
            title_lines: vec!["Hi".into()],
            has_label: true,
            has_subtitle: true,
        };
        let lum = caption_region_luminance(&bright, &cap);
        assert!(lum > 0.8);
        draw_caption_scrim(&mut bright, &cap, lum, "#0b0d12");
        let after = bright.get_pixel(50, 60);
        assert!(after[0] < 200);
    }
}
