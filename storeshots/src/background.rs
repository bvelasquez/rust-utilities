use crate::cache::{background_cache_path, hash_prompt, read_cached_png, write_cached_png};
use crate::config::{BrandSection, SlideItem};
use crate::gemini::GeminiClient;
use anyhow::{Context, Result};
use image::RgbaImage;
use std::path::Path;

pub struct BackgroundOptions<'a> {
    pub app_root: &'a Path,
    pub slide: &'a SlideItem,
    pub brand: &'a BrandSection,
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub use_ai: bool,
    pub image_model: &'a str,
    pub client: Option<&'a GeminiClient>,
}

pub async fn load_or_generate_background(opts: BackgroundOptions<'_>) -> Result<RgbaImage> {
    if opts.use_ai {
        if let Some(client) = opts.client {
            match generate_ai_background(client, &opts).await {
                Ok(img) => return Ok(img),
                Err(e) => {
                    eprintln!(
                        "warning: AI background for slide '{}' failed ({e}); using gradient fallback",
                        opts.slide.id
                    );
                }
            }
        } else {
            eprintln!(
                "warning: AI backgrounds enabled but no API key; using gradient for '{}'",
                opts.slide.id
            );
        }
    }

    Ok(gradient_background(
        opts.canvas_w,
        opts.canvas_h,
        &opts.brand.background,
        &opts.brand.accent,
    ))
}

async fn generate_ai_background(
    client: &GeminiClient,
    opts: &BackgroundOptions<'_>,
) -> Result<RgbaImage> {
    let prompt = background_prompt(opts.slide, opts.brand, opts.canvas_w, opts.canvas_h);
    let hash = hash_prompt(&[
        opts.image_model,
        &opts.brand.theme,
        &opts.brand.accent,
        &opts.brand.background,
        &prompt,
    ]);
    let cache_path = background_cache_path(opts.app_root, &opts.slide.id, &opts.brand.theme, &hash);

    if let Some(bytes) = read_cached_png(&cache_path) {
        return decode_rgba(&bytes, opts.canvas_w, opts.canvas_h);
    }

    let bytes = client
        .generate_background_image(opts.image_model, &prompt, "9:16")
        .await?;

    write_cached_png(&cache_path, &bytes)?;
    decode_rgba(&bytes, opts.canvas_w, opts.canvas_h)
}

fn background_prompt(slide: &SlideItem, brand: &BrandSection, w: u32, h: u32) -> String {
    format!(
        r#"Create an abstract App Store marketing background ONLY for a mobile app screenshot.

CRITICAL RULES:
- Do NOT draw any phone, tablet, device frame, or UI screenshot.
- Do NOT include any text, words, letters, logos, or app icons.
- This is ONLY a decorative background plate; a real app screenshot will be composited on top later.

Composition:
- Portrait canvas aspect ratio 9:16 ({w}x{h} target).
- Keep the lower 55% relatively calm and uncluttered (space for a centered phone mockup).
- The entire top-left headline zone (top 38%, left 90% of width) must stay VERY DARK — no bright blooms, flares, clouds, or light streaks behind text. Use only deep shadows and subtle accent tones there.
- Visual interest and brighter elements belong in the upper-right and mid-right edges only, away from the text column.
- Theme preset: {theme}
- Brand accent color: {accent}
- Brand base color: {background}
- Slide mood hint (do not render as text): {label} — {subtitle}

Style: premium app-store advertisement background, photoreal lighting, clean and modern."#,
        w = w,
        h = h,
        theme = brand.theme,
        accent = brand.accent,
        background = brand.background,
        label = slide.label,
        subtitle = slide.subtitle,
    )
}

fn decode_rgba(bytes: &[u8], target_w: u32, target_h: u32) -> Result<RgbaImage> {
    let img = image::load_from_memory(bytes).context("decode background image")?;
    let rgba = img.to_rgba8();
    if rgba.width() == target_w && rgba.height() == target_h {
        return Ok(rgba);
    }
    let resized = image::imageops::resize(
        &rgba,
        target_w,
        target_h,
        image::imageops::FilterType::Lanczos3,
    );
    Ok(resized)
}

pub fn gradient_background(w: u32, h: u32, base_hex: &str, accent_hex: &str) -> RgbaImage {
    let base = parse_hex_color(base_hex).unwrap_or([246, 241, 234, 255]);
    let accent = parse_hex_color(accent_hex).unwrap_or([91, 124, 250, 255]);

    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t_y = y as f32 / h as f32;
            let t_x = x as f32 / w as f32;
            let blend = (t_y * 0.55 + t_x * 0.15).min(1.0);
            let soft = blend * blend * (3.0 - 2.0 * blend);
            let r = lerp(base[0], accent[0], soft * 0.35) as u8;
            let g = lerp(base[1], accent[1], soft * 0.35) as u8;
            let b = lerp(base[2], accent[2], soft * 0.35) as u8;
            img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
        }
    }
    img
}

fn lerp(a: u8, b: u8, t: f32) -> f32 {
    a as f32 + (b as f32 - a as f32) * t
}

pub fn parse_hex_color(hex: &str) -> Option<[u8; 4]> {
    let s = hex.trim().trim_start_matches('#');
    match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some([r, g, b, 255])
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(parse_hex_color("#5B7CFA"), Some([91, 124, 250, 255]));
    }
}
