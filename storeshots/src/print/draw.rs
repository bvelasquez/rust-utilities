use crate::background::parse_hex_color;
use crate::config::BrandSection;
use crate::fonts::FontSet;
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anyhow::Result;
use image::{imageops, Rgba, RgbaImage};

#[derive(Clone)]
pub struct PrintTheme {
    pub bg: Rgba<u8>,
    pub bg_soft: Rgba<u8>,
    pub fg: Rgba<u8>,
    pub muted: Rgba<u8>,
    pub accent: Rgba<u8>,
    pub accent_dim: Rgba<u8>,
    pub light_bg: Rgba<u8>,
    pub light_fg: Rgba<u8>,
    pub light_muted: Rgba<u8>,
    pub glow: Rgba<u8>,
}

impl PrintTheme {
    pub fn from_brand(brand: &BrandSection) -> Self {
        let accent = rgba_hex(&brand.accent, 255).unwrap_or(Rgba([91, 124, 250, 255]));
        let bg = rgba_hex(&brand.background, 255).unwrap_or(Rgba([7, 6, 12, 255]));
        let fg = rgba_hex(&brand.foreground, 255).unwrap_or(Rgba([243, 244, 246, 255]));
        let muted_hex = brand.muted.as_deref().unwrap_or("#9ca3af");
        let muted = rgba_hex(muted_hex, 210).unwrap_or(Rgba([196, 181, 253, 210]));
        let accent_dim = darken(accent, 0.45);
        Self {
            bg,
            bg_soft: blend_rgba(bg, accent, 0.08),
            fg,
            muted,
            accent,
            accent_dim,
            light_bg: Rgba([238, 242, 245, 255]),
            light_fg: Rgba([11, 15, 13, 255]),
            light_muted: Rgba([11, 15, 13, 170]),
            glow: Rgba([accent[0], accent[1], accent[2], 90]),
        }
    }
}

pub struct Canvas<'a> {
    pub img: RgbaImage,
    pub fonts: &'a FontSet,
    pub theme: PrintTheme,
}

impl<'a> Canvas<'a> {
    pub fn new(w: u32, h: u32, fonts: &'a FontSet, theme: PrintTheme) -> Self {
        Self {
            img: RgbaImage::new(w, h),
            fonts,
            theme,
        }
    }

    pub fn fill_solid(&mut self, color: Rgba<u8>) {
        for px in self.img.pixels_mut() {
            *px = color;
        }
    }

    pub fn fill_panel(&mut self, x: u32, y: u32, w: u32, h: u32, variant: PanelVariant) {
        let mut panel = RgbaImage::new(w, h);
        match variant {
            PanelVariant::Light => panel.pixels_mut().for_each(|p| *p = self.theme.light_bg),
            PanelVariant::Dark => {
                draw_vertical_gradient(&mut panel, self.theme.bg_soft, self.theme.bg);
                draw_radial_glow(&mut panel, self.theme.glow, 0.75, 0.0, 0.65, 0.45);
            }
            PanelVariant::Brand => {
                draw_vertical_gradient(&mut panel, self.theme.accent_dim, self.theme.bg);
            }
        }
        overlay_image(&mut self.img, &panel, x, y);
    }

    pub fn draw_eyebrow(&mut self, text: &str, x: f32, y: f32, size: f32, dark: bool) -> Result<()> {
        let color = if dark {
            self.theme.accent
        } else {
            self.theme.accent_dim
        };
        self.draw_text_upper(text, x, y, size, color, 0.14)
    }

    pub fn draw_body_width(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        dark: bool,
        max_w: f32,
    ) -> Result<()> {
        let color = if dark { self.theme.muted } else { self.theme.light_muted };
        self.draw_wrapped(text, x, y, size, color, max_w.max(32.0), 1.45)
    }

    pub fn draw_headline_width(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        dark: bool,
        accent: bool,
        max_w: f32,
    ) -> Result<()> {
        let color = if accent {
            self.theme.accent
        } else if dark {
            self.theme.fg
        } else {
            self.theme.light_fg
        };
        let upper = text.to_uppercase();
        self.draw_wrapped(&upper, x, y, size, color, max_w.max(32.0), 1.08)
    }

    pub fn draw_bullets(
        &mut self,
        items: &[String],
        x: f32,
        y: f32,
        size: f32,
        dark: bool,
        standout_from: usize,
        max_w: f32,
    ) -> Result<f32> {
        let font = self.fonts.font()?;
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let line_h = scaled.height() * 1.32;

        // Fixed gutter column — snap to whole pixels so bullets stay vertically aligned after export scaling.
        let gutter = (size * 1.28).round().max(8.0);
        let bullet_cx = (x.round() + gutter * 0.5).round() as i32;
        let text_left = x.round() + gutter;
        let text_w = (max_w - gutter).max(32.0);
        let dot_r = (size * 0.17).max(3.0);

        let mut cy = y;
        for (i, item) in items.iter().enumerate() {
            let standout = i >= standout_from && standout_from < items.len();
            let color = if standout {
                if dark {
                    self.theme.accent
                } else {
                    self.theme.accent_dim
                }
            } else if dark {
                self.theme.muted
            } else {
                self.theme.light_muted
            };

            let lines = wrap_text(self.fonts, item, size, text_w)?;
            // cy is the baseline for the first line; center the dot on the cap height.
            let dot_cy = (cy - scaled.ascent() * 0.38).round() as i32;
            fill_circle(
                &mut self.img,
                bullet_cx,
                dot_cy,
                dot_r,
                if standout {
                    self.theme.accent
                } else {
                    Rgba([self.theme.accent[0], self.theme.accent[1], self.theme.accent[2], 50])
                },
            );
            stroke_circle(&mut self.img, bullet_cx, dot_cy, dot_r, self.theme.accent);

            draw_wrapped_visual_left(
                &mut self.img,
                &font,
                item,
                text_left,
                cy,
                scale,
                color,
                text_w,
                1.32,
            )?;

            let block_h = lines.len().max(1) as f32 * line_h;
            cy += block_h + size * 0.5;
        }
        Ok(cy)
    }

    pub fn draw_text_upper(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Rgba<u8>,
        letter_em: f32,
    ) -> Result<()> {
        let upper = text.to_uppercase();
        self.draw_text_spaced(&upper, x, y, size, color, letter_em)
    }

    pub fn draw_text_spaced(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Rgba<u8>,
        letter_em: f32,
    ) -> Result<()> {
        let font = self.fonts.font()?;
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let mut cx = x;
        let extra = size * letter_em;
        for ch in text.chars() {
            draw_glyph(&mut self.img, &font, ch, cx, y, scale, color)?;
            cx += scaled.h_advance(font.glyph_id(ch)) + extra;
        }
        Ok(())
    }

    pub fn measure_wrapped(
        &self,
        text: &str,
        size: f32,
        max_w: f32,
        line_height: f32,
    ) -> Result<f32> {
        let lines = wrap_text(self.fonts, text, size, max_w)?;
        let font = self.fonts.font()?;
        let scaled = font.as_scaled(PxScale::from(size));
        Ok(lines.len().max(1) as f32 * scaled.height() * line_height)
    }

    pub fn draw_wrapped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Rgba<u8>,
        max_w: f32,
        line_height: f32,
    ) -> Result<()> {
        let lines = wrap_text(self.fonts, text, size, max_w)?;
        let font = self.fonts.font()?;
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let gap = scaled.height() * line_height;
        for (i, line) in lines.iter().enumerate() {
            draw_glyph_line(&mut self.img, &font, line, x, y + i as f32 * gap, scale, color)?;
        }
        Ok(())
    }

    pub fn draw_qr_block(
        &mut self,
        qr: &RgbaImage,
        x: f32,
        y: f32,
        size: f32,
        label: &str,
        caption: Option<&str>,
        dark: bool,
    ) -> Result<()> {
        let qr_u = size.round() as u32;
        let scaled = imageops::resize(qr, qr_u, qr_u, imageops::FilterType::Nearest);
        overlay_image(&mut self.img, &scaled, x.round() as u32, y.round() as u32);
        let label_y = y + size + size * 0.08;
        let label_size = size * 0.11;
        let color = if dark { self.theme.fg } else { self.theme.light_fg };
        self.draw_text_centered(label, x + size * 0.5, label_y, label_size, color)?;
        if let Some(cap) = caption {
            self.draw_text_centered(
                cap,
                x + size * 0.5,
                label_y + label_size * 1.35,
                label_size * 0.85,
                if dark { self.theme.muted } else { self.theme.light_muted },
            )?;
        }
        Ok(())
    }

    pub fn draw_text_centered(
        &mut self,
        text: &str,
        cx: f32,
        y: f32,
        size: f32,
        color: Rgba<u8>,
    ) -> Result<()> {
        let w = text_width(self.fonts, text, size)?;
        draw_glyph_line(
            &mut self.img,
            &self.fonts.font()?,
            text,
            cx - w * 0.5,
            y,
            PxScale::from(size),
            color,
        )
    }

    pub fn draw_logo_or_badge(
        &mut self,
        app_root: &std::path::Path,
        logo_rel: Option<&str>,
        x: f32,
        y: f32,
        size: f32,
        initials: &str,
    ) -> Result<()> {
        let candidates = logo_rel
            .map(|p| vec![app_root.join(p)])
            .unwrap_or_default();
        let mut paths = candidates;
        paths.extend([
            app_root.join("storeshots/assets/icon.png"),
            app_root.join("assets/icon.png"),
            app_root.join(crate::config::BRAND_DIR).join("logo.png"),
            app_root.join("storeshots/assets/logo.png"),
            app_root.join("public/app-icon.png"),
        ]);
        for path in paths {
            if path.is_file() {
                if let Some(img) = open_logo_image(&path) {
                    let rgba = img.to_rgba8();
                    let s = size.round() as u32;
                    let scaled = imageops::resize(&rgba, s, s, imageops::FilterType::Lanczos3);
                    overlay_image(&mut self.img, &scaled, x.round() as u32, y.round() as u32);
                    return Ok(());
                }
            }
        }
        draw_initials_badge(&mut self.img, x, y, size, initials, self.theme.accent, self.theme.fg);
        Ok(())
    }

    /// Extra glow and light streaks for premium business-card backgrounds.
    pub fn draw_card_ambience(&mut self) {
        draw_radial_glow(
            &mut self.img,
            Rgba([
                self.theme.accent[0],
                self.theme.accent[1],
                self.theme.accent[2],
                50,
            ]),
            0.88,
            0.10,
            0.52,
            0.40,
        );
        draw_radial_glow(
            &mut self.img,
            Rgba([186, 70, 220, 38]),
            0.12,
            0.82,
            0.48,
            0.32,
        );
        draw_diagonal_streaks(&mut self.img, self.theme.accent);
    }

    /// Logo on an elevated tile with a micro brand label beneath the mark.
    pub fn draw_logo_tile(
        &mut self,
        app_root: &std::path::Path,
        logo_rel: Option<&str>,
        x: f32,
        y: f32,
        tile: f32,
        micro_label: &str,
        initials: &str,
    ) -> Result<()> {
        let tile_bg = blend_rgba(self.theme.bg, self.theme.accent, 0.16);
        fill_round_rect(&mut self.img, x, y, tile, tile, tile * 0.14, tile_bg);

        let label_band = tile * 0.15;
        let logo_h = tile - label_band;
        self.draw_logo_fitted(
            app_root,
            logo_rel,
            x,
            y,
            tile,
            logo_h,
            tile_bg,
            initials,
        )?;

        let micro_size = (tile * 0.085).max(6.0);
        let label_y = y + logo_h + label_band * 0.38;
        self.draw_text_centered(
            micro_label,
            x + tile * 0.5,
            label_y,
            micro_size,
            self.theme.muted,
        )?;
        Ok(())
    }

    /// Fit logo inside a box, centered on a solid background (hides JPEG fringe).
    pub fn draw_logo_fitted(
        &mut self,
        app_root: &std::path::Path,
        logo_rel: Option<&str>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        bg: Rgba<u8>,
        initials: &str,
    ) -> Result<()> {
        let r = w.min(h) * 0.12;
        fill_round_rect(&mut self.img, x, y, w, h, r, bg);

        let mut paths = logo_rel
            .map(|p| vec![app_root.join(p)])
            .unwrap_or_default();
        paths.extend([
            app_root.join("storeshots/assets/icon.png"),
            app_root.join("assets/icon.png"),
            app_root.join(crate::config::BRAND_DIR).join("logo.png"),
            app_root.join("storeshots/assets/logo.png"),
            app_root.join("public/app-icon.png"),
        ]);

        for path in paths {
            if path.is_file() {
                if let Some(img) = open_logo_image(&path) {
                    let rgba = img.to_rgba8();
                    let iw = rgba.width() as f32;
                    let ih = rgba.height() as f32;
                    let max_w = w * 0.84;
                    let max_h = h * 0.84;
                    let scale = (max_w / iw).min(max_h / ih);
                    let sw = (iw * scale).round().max(1.0) as u32;
                    let sh = (ih * scale).round().max(1.0) as u32;
                    let scaled = imageops::resize(&rgba, sw, sh, imageops::FilterType::Lanczos3);
                    let dx = (x + (w - sw as f32) * 0.5).round() as u32;
                    let dy = (y + (h - sh as f32) * 0.5).round() as u32;
                    overlay_image(&mut self.img, &scaled, dx, dy);
                    return Ok(());
                }
            }
        }

        let s = w.min(h) * 0.72;
        let bx = x + (w - s) * 0.5;
        let by = y + (h - s) * 0.5;
        draw_initials_badge(&mut self.img, bx, by, s, initials, self.theme.accent, self.theme.fg);
        Ok(())
    }

    /// Bold uppercase title lines (business card header).
    pub fn draw_bold_title_lines(
        &mut self,
        lines: &[&str],
        x: f32,
        y: f32,
        size: f32,
        color: Rgba<u8>,
        line_height: f32,
    ) -> Result<f32> {
        let font = self.fonts.font_bold()?;
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let gap = scaled.height() * line_height;
        for (i, line) in lines.iter().enumerate() {
            draw_bold_glyph_line(
                &mut self.img,
                self.fonts,
                &font,
                line,
                x,
                y + i as f32 * gap,
                scale,
                color,
            )?;
        }
        Ok(y + lines.len().max(1) as f32 * gap)
    }

    /// Feature list with filled check icons (business card).
    pub fn draw_check_bullets(
        &mut self,
        items: &[String],
        x: f32,
        y: f32,
        size: f32,
        max_w: f32,
    ) -> Result<f32> {
        let font = self.fonts.font()?;
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let line_h = scaled.height() * 1.28;
        let icon_r = (size * 0.68).max(5.0);
        let gutter = (icon_r * 2.55).round().max(14.0);
        let icon_cx = (x.round() + gutter * 0.5).round() as i32;
        let text_left = x.round() + gutter;
        let text_w = (max_w - gutter).max(32.0);
        let text_color = Rgba([self.theme.fg[0], self.theme.fg[1], self.theme.fg[2], 235]);

        let mut cy = y;
        for item in items {
            let lines = wrap_text(self.fonts, item, size, text_w)?;
            let icon_cy = (cy - scaled.ascent() * 0.34).round() as i32;
            fill_circle(&mut self.img, icon_cx, icon_cy, icon_r, self.theme.accent);
            draw_check_mark(
                &mut self.img,
                icon_cx,
                icon_cy,
                icon_r * 0.55,
            );

            draw_wrapped_visual_left(
                &mut self.img,
                &font,
                item,
                text_left,
                cy,
                scale,
                text_color,
                text_w,
                1.28,
            )?;

            let block_h = lines.len().max(1) as f32 * line_h;
            cy += block_h + size * 0.55;
        }
        Ok(cy)
    }

    /// QR on a white rounded pad with minimal quiet zone.
    pub fn draw_qr_padded(
        &mut self,
        qr: &RgbaImage,
        x: f32,
        y: f32,
        qr_size: f32,
        pad: f32,
    ) -> Result<()> {
        let outer = qr_size + pad * 2.0;
        fill_round_rect(
            &mut self.img,
            x,
            y,
            outer,
            outer,
            pad * 0.45,
            Rgba([255, 255, 255, 255]),
        );
        let qr_u = qr_size.round() as u32;
        let scaled = imageops::resize(qr, qr_u, qr_u, imageops::FilterType::Nearest);
        overlay_image(
            &mut self.img,
            &scaled,
            (x + pad).round() as u32,
            (y + pad).round() as u32,
        );
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum PanelVariant {
    Light,
    Dark,
    Brand,
}

pub fn scale_canvas(src: &RgbaImage, scale: u32) -> RgbaImage {
    if scale <= 1 {
        return src.clone();
    }
    let w = src.width() * scale;
    let h = src.height() * scale;
    imageops::resize(src, w, h, imageops::FilterType::Lanczos3)
}

fn rgba_hex(hex: &str, alpha: u8) -> Option<Rgba<u8>> {
    parse_hex_color(hex).map(|[r, g, b, _]| Rgba([r, g, b, alpha]))
}

fn darken(c: Rgba<u8>, factor: f32) -> Rgba<u8> {
    Rgba([
        (c[0] as f32 * factor) as u8,
        (c[1] as f32 * factor) as u8,
        (c[2] as f32 * factor) as u8,
        c[3],
    ])
}

fn blend_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    Rgba([
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
        255,
    ])
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn draw_vertical_gradient(img: &mut RgbaImage, top: Rgba<u8>, bottom: Rgba<u8>) {
    let h = img.height();
    for y in 0..h {
        let t = y as f32 / h as f32;
        let row = Rgba([
            lerp(top[0], bottom[0], t),
            lerp(top[1], bottom[1], t),
            lerp(top[2], bottom[2], t),
            255,
        ]);
        for x in 0..img.width() {
            img.put_pixel(x, y, row);
        }
    }
}

fn draw_radial_glow(img: &mut RgbaImage, color: Rgba<u8>, cx: f32, cy: f32, rx: f32, ry: f32) {
    let w = img.width() as f32;
    let h = img.height() as f32;
    for y in 0..img.height() {
        for x in 0..img.width() {
            let dx = (x as f32 / w - cx) / rx;
            let dy = (y as f32 / h - cy) / ry;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1.0 {
                let a = ((1.0 - d) * color[3] as f32) as u8;
                if a > 0 {
                    let px = img.get_pixel(x, y);
                    let blended = blend_px(*px, Rgba([color[0], color[1], color[2], a]));
                    img.put_pixel(x, y, blended);
                }
            }
        }
    }
}

fn draw_diagonal_streaks(img: &mut RgbaImage, accent: Rgba<u8>) {
    let w = img.width() as f32;
    let h = img.height() as f32;
    for y in 0..img.height() {
        for x in 0..img.width() {
            let u = x as f32 / w;
            let v = y as f32 / h;
            let d1 = (v - (0.28 - 0.12 * u)).abs();
            let d2 = (v - (0.52 - 0.08 * u)).abs();
            let mut a = 0u8;
            if d1 < 0.10 {
                a = a.saturating_add(((0.10 - d1) / 0.10 * 28.0) as u8);
            }
            if d2 < 0.07 {
                a = a.saturating_add(((0.07 - d2) / 0.07 * 18.0) as u8);
            }
            if a > 0 {
                let px = img.get_pixel(x, y);
                let streak = Rgba([accent[0], accent[1], accent[2], a]);
                img.put_pixel(x, y, blend_px(*px, streak));
            }
        }
    }
}

fn draw_check_mark(img: &mut RgbaImage, cx: i32, cy: i32, arm: f32) {
    let color = Rgba([255, 255, 255, 248]);
    let thickness = (arm * 0.38).max(1.6);
    let fx = cx as f32;
    let fy = cy as f32;
    draw_thick_line(
        img,
        fx - arm * 0.55,
        fy + arm * 0.05,
        fx - arm * 0.08,
        fy + arm * 0.52,
        thickness,
        color,
    );
    draw_thick_line(
        img,
        fx - arm * 0.08,
        fy + arm * 0.52,
        fx + arm * 0.62,
        fy - arm * 0.55,
        thickness,
        color,
    );
}

fn draw_thick_line(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    width: f32,
    color: Rgba<u8>,
) {
    let steps = ((x1 - x0).hypot(y1 - y0) * 2.5).round() as i32;
    let steps = steps.max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = x0 + (x1 - x0) * t;
        let y = y0 + (y1 - y0) * t;
        fill_circle(img, x.round() as i32, y.round() as i32, width * 0.5, color);
    }
}

fn draw_bold_glyph_line(
    img: &mut RgbaImage,
    fonts: &FontSet,
    font: &FontRef<'_>,
    text: &str,
    x: f32,
    y: f32,
    scale: PxScale,
    color: Rgba<u8>,
) -> Result<()> {
    draw_glyph_line(img, font, text, x, y, scale, color)?;
    if !fonts.has_bold_font() {
        draw_glyph_line(img, font, text, x + 0.75, y, scale, color)?;
    }
    Ok(())
}

fn fill_circle(img: &mut RgbaImage, cx: i32, cy: i32, r: f32, color: Rgba<u8>) {
    let r2 = r * r;
    for y in (cy - r.ceil() as i32)..=(cy + r.ceil() as i32) {
        for x in (cx - r.ceil() as i32)..=(cx + r.ceil() as i32) {
            if x < 0 || y < 0 {
                continue;
            }
            let ux = x as u32;
            let uy = y as u32;
            if ux >= img.width() || uy >= img.height() {
                continue;
            }
            let dx = x as f32 - cx as f32;
            let dy = y as f32 - cy as f32;
            if dx * dx + dy * dy <= r2 {
                img.put_pixel(ux, uy, color);
            }
        }
    }
}

fn stroke_circle(img: &mut RgbaImage, cx: i32, cy: i32, r: f32, color: Rgba<u8>) {
    let inner = (r - 1.2).max(0.0);
    let r2 = r * r;
    let inner2 = inner * inner;
    for y in (cy - r.ceil() as i32)..=(cy + r.ceil() as i32) {
        for x in (cx - r.ceil() as i32)..=(cx + r.ceil() as i32) {
            if x < 0 || y < 0 {
                continue;
            }
            let ux = x as u32;
            let uy = y as u32;
            if ux >= img.width() || uy >= img.height() {
                continue;
            }
            let dx = x as f32 - cx as f32;
            let dy = y as f32 - cy as f32;
            let d2 = dx * dx + dy * dy;
            if d2 <= r2 && d2 >= inner2 {
                img.put_pixel(ux, uy, color);
            }
        }
    }
}

fn draw_initials_badge(
    img: &mut RgbaImage,
    x: f32,
    y: f32,
    size: f32,
    initials: &str,
    bg: Rgba<u8>,
    fg: Rgba<u8>,
) {
    let s = size;
    fill_round_rect(img, x, y, s, s, s * 0.18, bg);
    let _ = initials;
    // Simple centered block letters via filled rects is overkill; use font if available later.
    // For now draw accent ring.
    stroke_round_rect(img, x + 2.0, y + 2.0, s - 4.0, s - 4.0, s * 0.16, fg);
}

fn fill_round_rect(img: &mut RgbaImage, x: f32, y: f32, w: f32, h: f32, r: f32, color: Rgba<u8>) {
    for py in y.round() as i32..(y + h).round() as i32 {
        for px in x.round() as i32..(x + w).round() as i32 {
            if px < 0 || py < 0 {
                continue;
            }
            let ux = px as u32;
            let uy = py as u32;
            if ux >= img.width() || uy >= img.height() {
                continue;
            }
            if inside_round_rect(px as f32, py as f32, x, y, w, h, r) {
                img.put_pixel(ux, uy, color);
            }
        }
    }
}

fn stroke_round_rect(img: &mut RgbaImage, x: f32, y: f32, w: f32, h: f32, r: f32, color: Rgba<u8>) {
    for py in y.round() as i32..(y + h).round() as i32 {
        for px in x.round() as i32..(x + w).round() as i32 {
            if px < 0 || py < 0 {
                continue;
            }
            let ux = px as u32;
            let uy = py as u32;
            if ux >= img.width() || uy >= img.height() {
                continue;
            }
            if inside_round_rect(px as f32, py as f32, x, y, w, h, r)
                && !inside_round_rect(px as f32, py as f32, x + 2.0, y + 2.0, w - 4.0, h - 4.0, (r - 2.0).max(0.0))
            {
                img.put_pixel(ux, uy, color);
            }
        }
    }
}

fn inside_round_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
    if px < x || py < y || px > x + w || py > y + h {
        return false;
    }
    let corners = [
        (x + r, y + r),
        (x + w - r, y + r),
        (x + r, y + h - r),
        (x + w - r, y + h - r),
    ];
    for (cx, cy) in corners {
        if (px < x + r && px < cx) || (px > x + w - r && px > cx) {
            if (py < y + r && py < cy) || (py > y + h - r && py > cy) {
                let dx = px - cx;
                let dy = py - cy;
                if dx * dx + dy * dy > r * r {
                    return false;
                }
            }
        }
    }
    true
}

pub fn overlay_image(dst: &mut RgbaImage, src: &RgbaImage, x: u32, y: u32) {
    for sy in 0..src.height() {
        let dy = y + sy;
        if dy >= dst.height() {
            break;
        }
        for sx in 0..src.width() {
            let dx = x + sx;
            if dx >= dst.width() {
                break;
            }
            let sp = src.get_pixel(sx, sy);
            if sp[3] == 0 {
                continue;
            }
            let dp = dst.get_pixel(dx, dy);
            dst.put_pixel(dx, dy, blend_px(*dp, *sp));
        }
    }
}

fn blend_px(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return dst;
    }
    if sa >= 1.0 {
        return src;
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

fn draw_glyph(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    ch: char,
    x: f32,
    y: f32,
    scale: PxScale,
    color: Rgba<u8>,
) -> Result<()> {
    draw_glyph_line(img, font, &ch.to_string(), x, y, scale, color)
}

fn draw_glyph_line(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    x: f32,
    y: f32,
    scale: PxScale,
    color: Rgba<u8>,
) -> Result<()> {
    let scaled = font.as_scaled(scale);
    let mut cx = x;
    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        if let Some(glyph) = font.outline_glyph(glyph_id.with_scale(scale)) {
            let bounds = glyph.px_bounds();
            glyph.draw(|gx, gy, v| {
                let px = cx + bounds.min.x + gx as f32;
                let py = y + bounds.min.y + gy as f32;
                let ix = px.round() as i32;
                let iy = py.round() as i32;
                if ix >= 0 && iy >= 0 {
                    let ux = ix as u32;
                    let uy = iy as u32;
                    if ux < img.width() && uy < img.height() {
                        let alpha = (v * color[3] as f32) as u8;
                        if alpha > 0 {
                            let dst = *img.get_pixel(ux, uy);
                            img.put_pixel(
                                ux,
                                uy,
                                blend_px(dst, Rgba([color[0], color[1], color[2], alpha])),
                            );
                        }
                    }
                }
            });
        }
        cx += scaled.h_advance(glyph_id);
    }
    Ok(())
}

fn text_width(fonts: &FontSet, text: &str, size: f32) -> Result<f32> {
    let font = fonts.font()?;
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    Ok(text
        .chars()
        .map(|ch| scaled.h_advance(font.glyph_id(ch)))
        .sum())
}

fn pen_x_for_visual_left(font: &FontRef<'_>, text: &str, scale: PxScale, visual_left: f32) -> f32 {
    if let Some(ch) = text.chars().next() {
        if let Some(glyph) = font.outline_glyph(font.glyph_id(ch).with_scale(scale)) {
            return visual_left - glyph.px_bounds().min.x;
        }
    }
    visual_left
}

fn draw_wrapped_visual_left(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    visual_left: f32,
    y: f32,
    scale: PxScale,
    color: Rgba<u8>,
    max_w: f32,
    line_height: f32,
) -> Result<()> {
    let lines = wrap_text_for_font(font, text, scale, max_w)?;
    let scaled = font.as_scaled(scale);
    let gap = scaled.height() * line_height;
    for (i, line) in lines.iter().enumerate() {
        let pen_x = pen_x_for_visual_left(font, line, scale, visual_left);
        draw_glyph_line(
            img,
            font,
            line,
            pen_x,
            y + i as f32 * gap,
            scale,
            color,
        )?;
    }
    Ok(())
}

fn wrap_text_for_font(font: &FontRef<'_>, text: &str, scale: PxScale, max_w: f32) -> Result<Vec<String>> {
    let scaled = font.as_scaled(scale);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_w = 0.0f32;
    for word in text.split_whitespace() {
        let word_w: f32 = word
            .chars()
            .map(|ch| scaled.h_advance(font.glyph_id(ch)))
            .sum();
        let space_w = scaled.h_advance(font.glyph_id(' '));
        let add_w = if current.is_empty() {
            word_w
        } else {
            space_w + word_w
        };
        if !current.is_empty() && current_w + add_w > max_w {
            lines.push(current.trim().to_string());
            current = word.to_string();
            current_w = word_w;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_w += space_w;
            }
            current.push_str(word);
            current_w += word_w;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    Ok(lines)
}

fn wrap_text(fonts: &FontSet, text: &str, size: f32, max_w: f32) -> Result<Vec<String>> {
    let font = fonts.font()?;
    wrap_text_for_font(&font, text, PxScale::from(size), max_w)
}

/// Sniff format from file contents (not extension) so JPEG data saved as `.png` still loads.
fn open_logo_image(path: &std::path::Path) -> Option<image::DynamicImage> {
    use image::ImageReader;
    ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
}
