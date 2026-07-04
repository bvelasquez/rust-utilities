use crate::ads::formats::{effective_layout_for_format, AdFormat};
use crate::background::gradient_background;
use crate::cache::{background_cache_path, hash_prompt, read_cached_png, write_cached_png};
use crate::composite::mockup_bytes;
use crate::config::{AdItem, BrandSection, StoreshotsConfig};
use crate::devices::{ad_phone_placement, Rect, SC_RX, SC_RY};
use crate::fonts::FontSet;
use crate::gemini::GeminiClient;
use crate::print::draw::PrintTheme;
use anyhow::{Context, Result};
use image::{imageops, Rgba, RgbaImage};
use std::path::Path;

pub struct AdRenderContext<'a> {
    pub app_root: &'a Path,
    pub cfg: &'a StoreshotsConfig,
    pub ad: &'a AdItem,
    pub format: &'static AdFormat,
    pub use_ai: bool,
    pub client: Option<&'a GeminiClient>,
    pub fonts: &'a FontSet,
}

pub async fn render_ad(ctx: AdRenderContext<'_>) -> Result<RgbaImage> {
    let w = ctx.format.w;
    let h = ctx.format.h;
    let layout = effective_layout_for_format(ctx.format);

    let mut canvas = load_ad_background(&ctx, w, h).await?;
    let theme = PrintTheme::from_brand(&ctx.cfg.brand);
    let raw_path = StoreshotsConfig::raw_path(ctx.app_root, &ctx.ad.raw);
    let has_screenshot = raw_path.is_file();

    match layout {
        "text-strip" => draw_text_strip(&ctx, &mut canvas, &theme)?,
        "skyscraper" => {
            if has_screenshot {
                draw_skyscraper(&ctx, &mut canvas, &theme)?;
            } else {
                draw_text_strip(&ctx, &mut canvas, &theme)?;
            }
        }
        "compact" => {
            if has_screenshot {
                draw_compact(&ctx, &mut canvas, &theme)?;
            } else {
                draw_text_strip(&ctx, &mut canvas, &theme)?;
            }
        }
        "landscape-split" => {
            if has_screenshot {
                draw_landscape_split(&ctx, &mut canvas, &theme)?;
            } else {
                draw_text_strip(&ctx, &mut canvas, &theme)?;
            }
        }
        _ => {
            if has_screenshot {
                draw_stacked(&ctx, &mut canvas, &theme)?;
            } else {
                draw_text_only_panel(&ctx, &mut canvas, &theme)?;
            }
        }
    }

    Ok(canvas)
}

async fn load_ad_background(ctx: &AdRenderContext<'_>, w: u32, h: u32) -> Result<RgbaImage> {
    if ctx.use_ai {
        if let Some(client) = ctx.client {
            match generate_ai_background(client, ctx, w, h).await {
                Ok(img) => return Ok(img),
                Err(e) => {
                    eprintln!(
                        "warning: AI background for ad '{}' failed ({e}); using gradient",
                        ctx.ad.id
                    );
                }
            }
        }
    }
    Ok(gradient_background(
        w,
        h,
        &ctx.cfg.brand.background,
        &ctx.cfg.brand.accent,
    ))
}

async fn generate_ai_background(
    client: &GeminiClient,
    ctx: &AdRenderContext<'_>,
    w: u32,
    h: u32,
) -> Result<RgbaImage> {
    let prompt = ad_background_prompt(ctx.ad, &ctx.cfg.brand, ctx.format, w, h);
    let hash = hash_prompt(&[
        ctx.cfg.ai.image_model.as_str(),
        &ctx.cfg.brand.theme,
        &ctx.cfg.brand.accent,
        ctx.format.id,
        &prompt,
    ]);
    let cache_key = format!("{}-{}", ctx.ad.id, ctx.format.id);
    let cache_path =
        background_cache_path(ctx.app_root, &cache_key, &ctx.cfg.brand.theme, &hash);

    if let Some(bytes) = read_cached_png(&cache_path) {
        return decode_and_resize(&bytes, w, h);
    }

    let aspect = ctx.format.gemini_aspect();
    let bytes = client
        .generate_background_image(&ctx.cfg.ai.image_model, &prompt, aspect)
        .await?;

    write_cached_png(&cache_path, &bytes)?;
    decode_and_resize(&bytes, w, h)
}

fn ad_background_prompt(ad: &AdItem, brand: &BrandSection, format: &AdFormat, w: u32, h: u32) -> String {
    let layout_hint = if format.h <= 100 {
        "Wide horizontal banner — keep left 70% calm for text; accent energy on right edge only."
    } else if format.aspect_ratio() > 1.2 {
        "Landscape ad — text zone on left 45%, visual interest on right."
    } else if format.aspect_ratio() < 0.85 {
        "Portrait/story ad — top 35% dark and calm for headline; energy in lower portion."
    } else {
        "Square ad — top 40% calm for headline; device will sit in lower half."
    };

    format!(
        r#"Create an abstract paid advertising background ONLY for a digital marketing banner.

CRITICAL RULES:
- Do NOT draw any phone, tablet, device frame, or UI screenshot.
- Do NOT include any text, words, letters, logos, or app icons.
- This is ONLY a decorative background; product UI and copy will be composited on top.

Canvas target: {w}×{h} ({ratio:.2}:1).
{layout_hint}
- Theme: {theme}
- Brand accent: {accent}
- Brand base: {background}
- Ad mood (do not render as text): {headline}

Style: premium paid social / display ad background, modern, high-converting, not stock-photo cheesy."#,
        w = w,
        h = h,
        ratio = format.aspect_ratio(),
        theme = brand.theme,
        accent = brand.accent,
        background = brand.background,
        headline = ad.headline.replace('\n', " "),
    )
}

fn decode_and_resize(bytes: &[u8], w: u32, h: u32) -> Result<RgbaImage> {
    let img = image::load_from_memory(bytes).context("decode ad background")?;
    let rgba = img.to_rgba8();
    if rgba.width() == w && rgba.height() == h {
        return Ok(rgba);
    }
    Ok(imageops::resize(
        &rgba,
        w,
        h,
        imageops::FilterType::Lanczos3,
    ))
}

/// Wide leaderboard / mobile banner — text only, no device.
fn draw_text_strip(ctx: &AdRenderContext<'_>, canvas: &mut RgbaImage, theme: &PrintTheme) -> Result<()> {
    let w = ctx.format.w as f32;
    let h = ctx.format.h as f32;
    let pad_x = w * 0.04;
    let mut title_size = (h * 0.38).min(w * 0.05).max(9.0);
    let mut cta_size = (h * 0.28).min(title_size * 0.85).max(8.0);

    let headline = if h <= 60.0 {
        banner_headline(&ctx.ad.headline)
    } else {
        strip_headline(&ctx.ad.headline, true)
    };

    let mut cta_pad_y = cta_size * 0.35;
    let mut cta_pill_h = cta_size + cta_pad_y * 2.0;
    let mut cta_w = if !ctx.ad.cta.is_empty() {
        ctx.fonts.measure_line(&ctx.ad.cta, cta_size)? + cta_size * 2.2
    } else {
        0.0
    };

    if h <= 55.0 && cta_w > 0.0 {
        while pad_x + ctx.fonts.measure_line(&headline, title_size)? + cta_w + pad_x * 2.0 > w
            && title_size > 7.0
        {
            title_size *= 0.9;
            cta_size = (cta_size * 0.9).max(7.0);
            cta_pad_y = cta_size * 0.35;
            cta_pill_h = cta_size + cta_pad_y * 2.0;
            cta_w = ctx.fonts.measure_line(&ctx.ad.cta, cta_size)? + cta_size * 2.2;
        }
    }

    let title_metrics = ctx.fonts.line_metrics(title_size)?;
    let headline_w = ctx.fonts.measure_line(&headline, title_size)?;
    let side_by_side = cta_w > 0.0 && pad_x + headline_w + cta_w + pad_x * 2.0 <= w;

    if side_by_side {
        let block_h = title_metrics.height.max(cta_pill_h);
        let block_top = (h - block_h) * 0.5;
        let headline_top = block_top + (block_h - title_metrics.height) * 0.5;
        ctx.fonts.draw_line_from_top(
            canvas,
            &headline,
            pad_x,
            headline_top,
            title_size,
            theme.fg,
        )?;

        if !ctx.ad.cta.is_empty() {
            let pill_x = w - pad_x - cta_w;
            let pill_y = block_top + (block_h - cta_pill_h) * 0.5;
            draw_cta_pill_at(ctx, canvas, theme, pill_x, pill_y, cta_w, cta_pill_h, cta_size)?;
        }
    } else {
        let mut block_h = title_metrics.height;
        if !ctx.ad.cta.is_empty() {
            block_h += title_size * 0.2 + cta_pill_h;
        }
        let mut block_top = (h - block_h) * 0.5;

        // Shrink type if the block still clips vertically.
        while block_top < h * 0.08 && title_size > 8.0 {
            title_size *= 0.92;
            cta_size = (cta_size * 0.92).max(8.0);
            let _tm = ctx.fonts.line_metrics(title_size)?;
            let cph = cta_size + cta_pad_y * 2.0;
            block_h = _tm.height + if ctx.ad.cta.is_empty() { 0.0 } else { title_size * 0.2 + cph };
            block_top = (h - block_h) * 0.5;
        }

        let bottom = ctx.fonts.draw_line_from_top(
            canvas,
            &headline,
            pad_x,
            block_top.max(h * 0.08),
            title_size,
            theme.fg,
        )?;

        if !ctx.ad.cta.is_empty() {
            let cph = cta_size + cta_pad_y * 2.0;
            let cw = ctx.fonts.measure_line(&ctx.ad.cta, cta_size)? + cta_size * 2.2;
            let pill_y = bottom + title_size * 0.15;
            draw_cta_pill_at(ctx, canvas, theme, pad_x, pill_y, cw, cph, cta_size)?;
        }
    }

    if ctx.ad.cta.is_empty() {
        if let Some(icon) = load_app_icon(ctx) {
            let ih = (h * 0.72) as u32;
            let iw = (ih as f32 * icon.width() as f32 / icon.height().max(1) as f32) as u32;
            let icon_rgba = imageops::resize(&icon, iw.max(1), ih.max(1), imageops::FilterType::Lanczos3);
            let x = ctx.format.w - iw - (w * 0.03) as u32;
            let y = ((h - ih as f32) * 0.5) as u32;
            overlay_image_alpha(canvas, &icon_rgba, x, y);
        }
    }

    Ok(())
}

/// Tall narrow skyscraper — text top, screenshot middle, CTA bottom.
fn draw_skyscraper(ctx: &AdRenderContext<'_>, canvas: &mut RgbaImage, theme: &PrintTheme) -> Result<()> {
    let w = ctx.format.w as f32;
    let h = ctx.format.h as f32;
    let pad = w * 0.08;
    let title_size = (w * 0.11).min(h * 0.038).max(9.0);
    let subtitle_size = title_size * 0.42;
    let label_size = title_size * 0.38;

    let cta_font = title_size * 0.38;
    let cta_pad_y = cta_font * 0.35;
    let cta_pill_h = cta_font + cta_pad_y * 2.0;
    let cta_zone = if ctx.ad.cta.is_empty() {
        h * 0.04
    } else {
        cta_pill_h + h * 0.05
    };

    let mut cursor_y = h * 0.04;
    let text_bottom = draw_headline_block(
        ctx,
        canvas,
        theme,
        pad,
        &mut cursor_y,
        label_size,
        title_size,
        subtitle_size,
        false,
        2,
    )?;

    let shot_top = (text_bottom + h * 0.04).max(h * 0.26);
    let shot_bottom = h - cta_zone;
    let shot_h = (shot_bottom - shot_top).max(40.0) as u32;
    let shot_w = (w * 0.84) as u32;
    let shot_x = ((w - shot_w as f32) * 0.5) as u32;
    draw_rounded_screenshot(ctx, canvas, shot_x, shot_top as u32, shot_w, shot_h, true)?;

    if !ctx.ad.cta.is_empty() {
        let cta_w = ctx.fonts.measure_line(&ctx.ad.cta, cta_font)? + cta_font * 2.2;
        let pill_x = ((w - cta_w) * 0.5).max(pad);
        let pill_y = h - cta_zone + (cta_zone - cta_pill_h) * 0.5;
        draw_cta_pill_at(ctx, canvas, theme, pill_x, pill_y, cta_w, cta_pill_h, cta_font)?;
    }
    Ok(())
}

/// Small IAB rectangles — text + CTA top, screenshot bottom.
fn draw_compact(ctx: &AdRenderContext<'_>, canvas: &mut RgbaImage, theme: &PrintTheme) -> Result<()> {
    let w = ctx.format.w as f32;
    let h = ctx.format.h as f32;
    let pad = w * 0.07;
    let title_size = (w * 0.08).min(h * 0.12).max(11.0);
    let subtitle_size = title_size * 0.4;
    let label_size = title_size * 0.34;

    let mut cursor_y = h * 0.05;
    let text_bottom = draw_headline_block(
        ctx,
        canvas,
        theme,
        pad,
        &mut cursor_y,
        label_size,
        title_size,
        subtitle_size,
        true,
        2,
    )?;

    let shot_top = text_bottom + h * 0.04;
    let shot_h = ((h * 0.96) - shot_top).max(36.0) as u32;
    let shot_w = (w * 0.88) as u32;
    let shot_x = ((w - shot_w as f32) * 0.5) as u32;
    draw_rounded_screenshot(ctx, canvas, shot_x, shot_top as u32, shot_w, shot_h, false)?;
    Ok(())
}

/// Landscape / billboard / play feature — copy left, screenshot right.
fn draw_landscape_split(ctx: &AdRenderContext<'_>, canvas: &mut RgbaImage, theme: &PrintTheme) -> Result<()> {
    let w = ctx.format.w;
    let h = ctx.format.h;
    let wf = w as f32;
    let hf = h as f32;

    let text_w_frac = if wf / hf > 3.0 { 0.58 } else { 0.48 };
    let text_w = wf * text_w_frac;
    let pad = wf * 0.05;
    let title_size = (text_w * 0.11).min(hf * 0.15).max(12.0);
    let subtitle_size = title_size * 0.38;
    let label_size = title_size * 0.32;

    let mut cursor_y = hf * 0.1;
    draw_headline_block(
        ctx,
        canvas,
        theme,
        pad,
        &mut cursor_y,
        label_size,
        title_size,
        subtitle_size,
        true,
        2,
    )?;

    let shot_w = (wf * (1.0 - text_w_frac - 0.05)) as u32;
    let zone_left = w - shot_w - (wf * 0.035) as u32;
    let zone_top = (hf * 0.06) as f32;
    let zone_bottom = hf * 0.94;
    place_phone_mockup_in_zone(ctx, canvas, zone_left, zone_top, shot_w, zone_bottom, 0.92)?;
    Ok(())
}

fn place_phone_mockup_in_zone(
    ctx: &AdRenderContext<'_>,
    canvas: &mut RgbaImage,
    zone_x: u32,
    zone_top: f32,
    zone_w: u32,
    zone_bottom: f32,
    max_width_frac: f32,
) -> Result<()> {
    let canvas_w = ctx.format.w;
    let available_h = (zone_bottom - zone_top).max(8.0);
    let max_w = zone_w as f32 * max_width_frac;

    let mut mock_w = max_w;
    let mut mock_h = mock_w * (crate::devices::MK_H / crate::devices::MK_W);
    if mock_h > available_h {
        mock_h = available_h;
        mock_w = mock_h * (crate::devices::MK_W / crate::devices::MK_H);
    }

    let mock_w = mock_w.max(24.0).round() as u32;
    let mock_h = mock_h.max(48.0).round() as u32;
    let x = zone_x + zone_w.saturating_sub(mock_w) / 2;
    let y = (zone_top + (available_h - mock_h as f32) * 0.5)
        .max(zone_top)
        .round() as u32;

    let raw_path = StoreshotsConfig::raw_path(ctx.app_root, &ctx.ad.raw);
    let screenshot = image::open(&raw_path)
        .with_context(|| format!("open screenshot {}", raw_path.display()))?
        .to_rgba8();
    let mockup = image::load_from_memory(mockup_bytes())
        .context("load mockup")?
        .to_rgba8();
    let mockup_scaled = imageops::resize(
        &mockup,
        mock_w,
        mock_h,
        imageops::FilterType::Lanczos3,
    );
    let screen = crate::devices::screen_rect_in_mockup(x, y, mock_w, mock_h);
    let bleed = ((screen.w as f32) * 0.003).max(1.0).round() as u32;
    let draw_w = screen.w + bleed * 2;
    let draw_h = screen.h + bleed * 2;
    let draw_x = screen.x.saturating_sub(bleed);
    let draw_y = screen.y.saturating_sub(bleed);
    let fitted = resize_contain(&screenshot, draw_w, draw_h);
    let clipped = clip_rounded_rect(&fitted, SC_RX, SC_RY);
    overlay_image_alpha(canvas, &clipped, draw_x, draw_y);
    overlay_mockup_frame(canvas, &mockup_scaled, x, y, &screen);
    let _ = canvas_w;
    Ok(())
}

/// Square / portrait — headline + CTA top, phone mockup below (fully inside canvas).
fn draw_stacked(ctx: &AdRenderContext<'_>, canvas: &mut RgbaImage, theme: &PrintTheme) -> Result<()> {
    let w = ctx.format.w as f32;
    let h = ctx.format.h as f32;
    let pad = w * 0.08;
    let title_size = (w * 0.075).min(h * 0.048).max(14.0);
    let subtitle_size = title_size * 0.38;
    let label_size = title_size * 0.32;

    let mut cursor_y = h * 0.06;
    draw_headline_block(
        ctx,
        canvas,
        theme,
        pad,
        &mut cursor_y,
        label_size,
        title_size,
        subtitle_size,
        true,
        2,
    )?;

    let zone_top = cursor_y + h * 0.03;
    let bottom_margin = h * 0.04;
    place_phone_mockup(ctx, canvas, zone_top, h - bottom_margin, 0.72)?;
    Ok(())
}

fn draw_text_only_panel(ctx: &AdRenderContext<'_>, canvas: &mut RgbaImage, theme: &PrintTheme) -> Result<()> {
    let wf = ctx.format.w as f32;
    let hf = ctx.format.h as f32;
    let pad = wf * 0.08;
    let title_size = (wf * 0.075).min(hf * 0.08).max(14.0);
    let subtitle_size = title_size * 0.4;
    let mut cursor_y = hf * 0.2;
    draw_headline_block(
        ctx,
        canvas,
        theme,
        pad,
        &mut cursor_y,
        title_size * 0.3,
        title_size,
        subtitle_size,
        true,
        3,
    )?;
    Ok(())
}

fn draw_headline_block(
    ctx: &AdRenderContext<'_>,
    canvas: &mut RgbaImage,
    theme: &PrintTheme,
    pad: f32,
    cursor_y: &mut f32,
    label_size: f32,
    title_size: f32,
    subtitle_size: f32,
    include_cta: bool,
    max_title_lines: usize,
) -> Result<f32> {
    let accent = theme.accent;
    let fg = theme.fg;

    if !ctx.cfg.app.name.is_empty() && ctx.format.h > 120 {
        let label_bottom = ctx.fonts.draw_line_from_top(
            canvas,
            &ctx.cfg.app.name.to_uppercase(),
            pad,
            *cursor_y,
            label_size,
            accent,
        )?;
        *cursor_y = label_bottom + label_size * 0.35;
    }

    let lines: Vec<&str> = ctx
        .ad
        .headline
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(max_title_lines)
        .collect();
    *cursor_y = ctx.fonts.draw_lines_from_top(
        canvas,
        &lines,
        pad,
        *cursor_y,
        title_size,
        fg,
        1.08,
    )?;

    if !ctx.ad.subtitle.is_empty() && ctx.format.h > 280 {
        *cursor_y += title_size * 0.12;
        *cursor_y = ctx.fonts.draw_line_from_top(
            canvas,
            &ctx.ad.subtitle,
            pad,
            *cursor_y,
            subtitle_size,
            theme.muted,
        )?;
        *cursor_y += subtitle_size * 0.2;
    }

    if include_cta && !ctx.ad.cta.is_empty() {
        *cursor_y += title_size * 0.1;
        let cta_font = subtitle_size * 1.05;
        let cta_pad_y = cta_font * 0.35;
        let cta_pill_h = cta_font + cta_pad_y * 2.0;
        let cta_w = ctx.fonts.measure_line(&ctx.ad.cta, cta_font)? + cta_font * 2.2;
        draw_cta_pill_at(ctx, canvas, theme, pad, *cursor_y, cta_w, cta_pill_h, cta_font)?;
        *cursor_y += cta_pill_h + title_size * 0.12;
    }

    Ok(*cursor_y)
}

fn draw_cta_pill_at(
    ctx: &AdRenderContext<'_>,
    canvas: &mut RgbaImage,
    theme: &PrintTheme,
    x: f32,
    y: f32,
    pill_w: f32,
    pill_h: f32,
    font_size: f32,
) -> Result<()> {
    if ctx.ad.cta.is_empty() {
        return Ok(());
    }
    let pill_w_u = pill_w.ceil() as u32;
    let pill_h_u = pill_h.ceil() as u32;
    let rx = pill_h * 0.45;

    let x0 = x.max(0.0).min((canvas.width() as f32 - pill_w).max(0.0)) as u32;
    let y0 = y.max(0.0).min((canvas.height() as f32 - pill_h).max(0.0)) as u32;
    fill_rounded_rect(canvas, x0, y0, pill_w_u, pill_h_u, rx as u32, theme.accent);

    ctx.fonts.draw_line_centered_in_rect(
        canvas,
        &ctx.ad.cta,
        x0 as f32,
        y0 as f32,
        pill_w,
        pill_h,
        font_size,
        Rgba([255, 255, 255, 255]),
    )?;
    Ok(())
}

fn place_phone_mockup(
    ctx: &AdRenderContext<'_>,
    canvas: &mut RgbaImage,
    zone_top: f32,
    zone_bottom: f32,
    max_width_frac: f32,
) -> Result<()> {
    let placement = ad_phone_placement(
        ctx.format.w,
        ctx.format.h,
        zone_top,
        zone_bottom,
        max_width_frac,
    );
    let raw_path = StoreshotsConfig::raw_path(ctx.app_root, &ctx.ad.raw);
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
    let bleed = ((screen.w as f32) * 0.003).max(1.0).round() as u32;
    let draw_w = screen.w + bleed * 2;
    let draw_h = screen.h + bleed * 2;
    let draw_x = screen.x.saturating_sub(bleed);
    let draw_y = screen.y.saturating_sub(bleed);

    let fitted = resize_contain(&screenshot, draw_w, draw_h);
    let clipped = clip_rounded_rect(&fitted, SC_RX, SC_RY);
    overlay_image_alpha(canvas, &clipped, draw_x, draw_y);
    overlay_mockup_frame(canvas, &mockup_scaled, placement.mockup.x, placement.mockup.y, screen);
    Ok(())
}

fn draw_rounded_screenshot(
    ctx: &AdRenderContext<'_>,
    canvas: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    contain: bool,
) -> Result<()> {
    if w < 8 || h < 8 {
        return Ok(());
    }
    let raw_path = StoreshotsConfig::raw_path(ctx.app_root, &ctx.ad.raw);
    let screenshot = image::open(&raw_path)
        .with_context(|| format!("open screenshot {}", raw_path.display()))?
        .to_rgba8();
    let fitted = if contain {
        resize_contain_on_dark(&screenshot, w, h)
    } else {
        resize_cover_top(&screenshot, w, h)
    };
    let clipped = clip_rounded_rect(&fitted, 0.08, 0.06);
    overlay_image_alpha(canvas, &clipped, x, y);
    Ok(())
}

fn resize_contain_on_dark(img: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let mut panel = RgbaImage::from_pixel(tw, th, Rgba([12, 14, 22, 255]));
    let contained = resize_contain(img, tw, th);
    let x0 = (tw.saturating_sub(contained.width())) / 2;
    let y0 = (th.saturating_sub(contained.height())) / 2;
    for py in 0..contained.height() {
        for px in 0..contained.width() {
            let sp = *contained.get_pixel(px, py);
            if sp[3] > 0 {
                panel.put_pixel(x0 + px, y0 + py, sp);
            }
        }
    }
    panel
}

fn first_headline_line(headline: &str) -> String {
    headline
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Simple Workout")
        .trim_end_matches(&['.', ','][..])
        .to_string()
}

fn banner_headline(headline: &str) -> String {
    let lines: Vec<&str> = headline
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match lines.as_slice() {
        [] => "Simple Workout".into(),
        [one] => one.trim_end_matches(&['.', ','][..]).to_string(),
        [first, second, ..] => {
            format!(
                "{} {}",
                first.trim_end_matches(&['.', ','][..]),
                second.trim_end_matches(&['.', ','][..])
            )
        }
    }
}

fn strip_headline(headline: &str, compact: bool) -> String {
    let lines: Vec<&str> = headline
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if compact {
        if lines.len() >= 2 {
            return format!("{} {}", lines[0].trim_end_matches('.'), lines[1]);
        }
        return lines.first().copied().unwrap_or("Simple Workout").to_string();
    }
    lines
        .into_iter()
        .take(2)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn resize_cover_top(img: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let sw = img.width().max(1) as f32;
    let sh = img.height().max(1) as f32;
    let scale = (tw as f32 / sw).max(th as f32 / sh);
    let nw = (sw * scale).ceil() as u32;
    let nh = (sh * scale).ceil() as u32;
    let resized = imageops::resize(img, nw, nh, imageops::FilterType::Lanczos3);
    let x0 = nw.saturating_sub(tw) / 2;
    imageops::crop_imm(&resized, x0, 0, tw, th).to_image()
}

fn load_app_icon(ctx: &AdRenderContext<'_>) -> Option<RgbaImage> {
    for rel in [
        "storeshots/brand/icon.png",
        "storeshots/assets/icon.png",
    ] {
        let path = ctx.app_root.join(rel);
        if path.is_file() {
            if let Ok(img) = image::open(&path) {
                return Some(img.to_rgba8());
            }
        }
    }
    None
}

fn fill_rounded_rect(canvas: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, r: u32, color: Rgba<u8>) {
    let r = r.min(w / 2).min(h / 2).max(1);
    for py in 0..h {
        for px in 0..w {
            let dx = x + px;
            let dy = y + py;
            if dx >= canvas.width() || dy >= canvas.height() {
                continue;
            }
            let xf = px as f32 + 0.5;
            let yf = py as f32 + 0.5;
            if inside_rounded_rect(xf, yf, w as f32, h as f32, r as f32, r as f32) {
                canvas.put_pixel(dx, dy, color);
            }
        }
    }
}

fn resize_contain(img: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let sw = img.width().max(1) as f32;
    let sh = img.height().max(1) as f32;
    let scale = (tw as f32 / sw).min(th as f32 / sh);
    let nw = (sw * scale).round().max(1.0) as u32;
    let nh = (sh * scale).round().max(1.0) as u32;
    let resized = imageops::resize(img, nw, nh, imageops::FilterType::Lanczos3);
    let mut out = RgbaImage::from_pixel(tw, th, Rgba([0, 0, 0, 255]));
    let x0 = (tw - nw) / 2;
    let y0 = (th - nh) / 2;
    for y in 0..nh {
        for x in 0..nw {
            out.put_pixel(x0 + x, y0 + y, *resized.get_pixel(x, y));
        }
    }
    out
}

fn clip_rounded_rect(img: &RgbaImage, rx_frac: f32, ry_frac: f32) -> RgbaImage {
    let w = img.width();
    let h = img.height();
    let rx = (w as f32 * rx_frac).max(1.0);
    let ry = (h as f32 * ry_frac).max(1.0);
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            if !inside_rounded_rect(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32, rx, ry) {
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
            canvas.put_pixel(dx, dy, alpha_blend(dp, sp));
        }
    }
}

fn overlay_mockup_frame(canvas: &mut RgbaImage, mockup: &RgbaImage, mx: u32, my: u32, screen: &Rect) {
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
