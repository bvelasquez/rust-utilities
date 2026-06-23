use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anyhow::{Context, Result};
use image::{Rgba, RgbaImage};
use std::path::Path;

pub struct FontSet {
    regular: Vec<u8>,
}

impl FontSet {
    pub fn load(app_root: &Path, font_config: Option<&str>) -> Result<Self> {
        if let Some(spec) = font_config {
            let path = if spec.contains('/') || spec.contains('\\') {
                app_root.join(spec)
            } else {
                app_root.join("storeshots/brand").join(format!("{spec}.ttf"))
            };
            if path.is_file() {
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("read font {}", path.display()))?;
                return Ok(Self { regular: bytes });
            }
            let alt = app_root.join("storeshots/brand/font.ttf");
            if alt.is_file() {
                let bytes = std::fs::read(&alt)
                    .with_context(|| format!("read font {}", alt.display()))?;
                return Ok(Self { regular: bytes });
            }
        }

        let bundled = system_fallback_font()?;
        Ok(Self { regular: bundled })
    }

    fn font(&self) -> Result<FontRef<'_>> {
        FontRef::try_from_slice(&self.regular).context("parse font")
    }

    pub fn draw_multiline(
        &self,
        canvas: &mut RgbaImage,
        lines: &[&str],
        x: f32,
        y: f32,
        size_px: f32,
        color: Rgba<u8>,
        line_height: f32,
    ) -> Result<()> {
        let font = self.font()?;
        let scale = PxScale::from(size_px);
        let scaled = font.as_scaled(scale);
        let line_gap = scaled.height() * line_height;

        for (i, line) in lines.iter().enumerate() {
            let mut cursor_x = x;
            let line_y = y + i as f32 * line_gap;
            for ch in line.chars() {
                let glyph_id = font.glyph_id(ch);
                if let Some(glyph) = font.outline_glyph(glyph_id.with_scale(scale)) {
                    let bounds = glyph.px_bounds();
                    glyph.draw(|gx, gy, v| {
                        let px = cursor_x + bounds.min.x + gx as f32;
                        let py = line_y + bounds.min.y + gy as f32;
                        let ix = px.round() as i32;
                        let iy = py.round() as i32;
                        if ix >= 0 && iy >= 0 {
                            let ux = ix as u32;
                            let uy = iy as u32;
                            if ux < canvas.width() && uy < canvas.height() {
                                let src = color;
                                let dst = *canvas.get_pixel(ux, uy);
                                let alpha = (v * src[3] as f32) as u8;
                                if alpha > 0 {
                                    let blended = blend_rgba(dst, Rgba([src[0], src[1], src[2], alpha]));
                                    canvas.put_pixel(ux, uy, blended);
                                }
                            }
                        }
                    });
                }
                cursor_x += scaled.h_advance(glyph_id);
            }
        }
        Ok(())
    }
}

fn blend_rgba(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
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

fn system_fallback_font() -> Result<Vec<u8>> {
    let candidates = [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    for path in candidates {
        if Path::new(path).is_file() {
            return std::fs::read(path).with_context(|| format!("read font {path}"));
        }
    }
    anyhow::bail!("no font found; add storeshots/brand/font.ttf or set brand.font in storeshots.toml")
}
