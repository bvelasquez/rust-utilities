use crate::print::draw::{Canvas, PanelVariant};
use crate::print::formats::{FormatContext, NamedPage};
use crate::print::qr::render_qr_quiet;
use anyhow::Result;

pub fn render(ctx: &FormatContext<'_>, variant: &str) -> Result<Vec<NamedPage>> {
    let w_in = ctx
        .cfg
        .print
        .business_card
        .as_ref()
        .map(|b| b.bleed_w_in)
        .unwrap_or(3.625);
    let h_in = ctx
        .cfg
        .print
        .business_card
        .as_ref()
        .map(|b| b.bleed_h_in)
        .unwrap_or(2.125);
    let (w, h) = ctx.layout.business_card_bleed(w_in, h_in);

    let mut pages = Vec::new();
    if variant == "front" || variant == "both" {
        pages.push(NamedPage {
            name: "business-card-front".into(),
            layout: render_front(ctx, w, h)?,
            width_in: w_in,
            height_in: h_in,
        });
    }
    if variant == "back" || variant == "both" {
        pages.push(NamedPage {
            name: "business-card-back".into(),
            layout: render_back(ctx, w, h)?,
            width_in: w_in,
            height_in: h_in,
        });
    }
    if pages.is_empty() {
        anyhow::bail!("business-card variant must be front, back, or both");
    }
    Ok(pages)
}

fn render_front(ctx: &FormatContext<'_>, w: u32, h: u32) -> Result<image::RgbaImage> {
    let theme = crate::print::draw::PrintTheme::from_brand(&ctx.cfg.brand);
    let mut canvas = Canvas::new(w, h, ctx.fonts, theme.clone());
    canvas.fill_panel(0, 0, w, h, PanelVariant::Dark);
    canvas.draw_card_ambience();

    let pad = w as f32 * 0.065;
    let col_gap = w as f32 * 0.04;

    // Footer stack (bottom-anchored): email under QR — layout upward from bottom edge.
    let email_size = h as f32 * 0.042;
    let email_band = email_size * 1.55;
    let qr_pad = h as f32 * 0.008;
    let qr_inner = h as f32 * 0.30;
    let qr_outer = qr_inner + qr_pad * 2.0;
    let qr_x = w as f32 - pad - qr_outer;
    let email_baseline = h as f32 - pad - email_size * 0.15;
    let qr_y = email_baseline - email_band - qr_outer;

    let content_w = qr_x - pad - col_gap;

    // Header — logo tile + bold title + tagline (tight stack)
    let tile = h as f32 * 0.34;
    canvas.draw_logo_tile(
        ctx.app_root,
        ctx.cfg.print.copy.logo.as_deref(),
        pad,
        pad,
        tile,
        &brand_short_name(&ctx.copy.name),
        &initials(&ctx.copy.name),
    )?;

    let name_x = pad + tile + col_gap;
    let name_y = pad;
    let name_size = h as f32 * 0.104;
    let name_w = w as f32 - name_x - pad;
    let title_lines = card_title_lines(&ctx.copy.name);
    let title_refs: Vec<&str> = title_lines.iter().map(String::as_str).collect();
    let title_bottom = canvas.draw_bold_title_lines(
        &title_refs,
        name_x,
        name_y,
        name_size,
        theme.fg,
        1.02,
    )?;

    let tagline_size = h as f32 * 0.058;
    let tagline_y = title_bottom + tagline_size * 0.18;
    canvas.draw_wrapped(
        ctx.copy.card_line(),
        name_x,
        tagline_y,
        tagline_size,
        theme.accent,
        name_w,
        1.22,
    )?;

    // Body — checkmark bullets (left column only)
    let header_bottom = pad + tile + h as f32 * 0.028;
    let body_y = header_bottom;
    let body_w = content_w;
    let bullets = ctx.copy.card_bullets();
    if !bullets.is_empty() {
        let bullet_size = h as f32 * 0.048;
        canvas.draw_check_bullets(&bullets, pad, body_y, bullet_size, body_w)?;
    }

    // QR + email
    let qr = render_qr_quiet(&ctx.copy.qr_url, 8, 2)?;
    canvas.draw_qr_padded(&qr, qr_x, qr_y, qr_inner, qr_pad)?;

    let footer_email = ctx
        .copy
        .support_email
        .as_deref()
        .unwrap_or(&ctx.copy.contact_email);
    canvas.draw_text_centered(
        footer_email,
        qr_x + qr_outer * 0.5,
        email_baseline,
        email_size,
        theme.muted,
    )?;

    Ok(canvas.img)
}

fn render_back(ctx: &FormatContext<'_>, w: u32, h: u32) -> Result<image::RgbaImage> {
    let theme = crate::print::draw::PrintTheme::from_brand(&ctx.cfg.brand);
    let mut canvas = Canvas::new(w, h, ctx.fonts, theme.clone());
    canvas.fill_panel(0, 0, w, h, PanelVariant::Brand);
    canvas.draw_card_ambience();

    let pad = w as f32 * 0.08;
    let text_w = w as f32 - pad * 2.0;
    canvas.draw_eyebrow(&ctx.copy.eyebrow, pad, pad, h as f32 * 0.07, true)?;
    canvas.draw_headline_width(
        &ctx.copy.website_label,
        pad,
        h as f32 * 0.28,
        h as f32 * 0.12,
        true,
        true,
        text_w,
    )?;
    canvas.draw_body_width(
        &ctx.copy.contact_email,
        pad,
        h as f32 * 0.48,
        h as f32 * 0.075,
        true,
        text_w,
    )?;
    if let Some(support) = &ctx.copy.support_email {
        if support != &ctx.copy.contact_email {
            canvas.draw_body_width(support, pad, h as f32 * 0.58, h as f32 * 0.065, true, text_w)?;
        }
    }
    canvas.draw_body_width(
        &ctx.copy.eyebrow,
        pad,
        h as f32 - pad - h as f32 * 0.08,
        h as f32 * 0.06,
        true,
        text_w,
    )?;
    Ok(canvas.img)
}

fn card_title_lines(name: &str) -> Vec<String> {
    let upper = name.to_uppercase();
    for suffix in [" LLC", " INC.", " INC", " LTD.", " LTD"] {
        if let Some(base) = upper.strip_suffix(suffix) {
            let legal = suffix.trim();
            if !base.trim().is_empty() {
                return vec![base.trim().to_string(), legal.to_string()];
            }
        }
    }
    vec![upper]
}

fn brand_short_name(name: &str) -> String {
    name.to_uppercase()
        .replace(" LLC", "")
        .replace(" INC.", "")
        .replace(" INC", "")
        .replace(" LTD.", "")
        .replace(" LTD", "")
        .trim()
        .to_string()
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .take(2)
        .map(|w| w.chars().next().unwrap_or('S').to_ascii_uppercase())
        .collect()
}
