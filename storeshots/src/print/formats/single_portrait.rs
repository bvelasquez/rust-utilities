use crate::print::draw::{Canvas, PanelVariant};
use crate::print::formats::{FormatContext, NamedPage};
use crate::print::qr::render_qr;
use anyhow::Result;

pub fn render(ctx: &FormatContext<'_>) -> Result<Vec<NamedPage>> {
    let (w, h) = ctx.layout.portrait_sheet();
    let theme = crate::print::draw::PrintTheme::from_brand(&ctx.cfg.brand);
    let mut canvas = Canvas::new(w, h, ctx.fonts, theme.clone());
    canvas.fill_panel(0, 0, w, h, PanelVariant::Dark);

    let pad = w as f32 * 0.07;
    let text_w = w as f32 - pad * 2.0;
    let footer_h = h as f32 * 0.24;

    let mut cy = pad;
    cy = draw_hero_block(&mut canvas, ctx, pad, cy, w, text_w)?;
    let _ = draw_features_block(&mut canvas, ctx, pad, cy, w, text_w)?;
    draw_footer_block(&mut canvas, ctx, pad, h as f32 - footer_h, w, footer_h)?;

    Ok(vec![NamedPage {
        name: "single-portrait".into(),
        layout: canvas.img,
        width_in: 8.5,
        height_in: 11.0,
    }])
}

fn draw_hero_block(
    c: &mut Canvas<'_>,
    ctx: &FormatContext<'_>,
    pad: f32,
    mut cy: f32,
    w: u32,
    text_w: f32,
) -> Result<f32> {
    let logo_size = w as f32 * 0.12;
    c.draw_logo_or_badge(
        ctx.app_root,
        ctx.cfg.print.copy.logo.as_deref(),
        pad,
        cy,
        logo_size,
        &initials(&ctx.copy.name),
    )?;
    c.draw_eyebrow(
        &ctx.copy.eyebrow,
        pad + logo_size + pad * 0.25,
        cy + logo_size * 0.08,
        w as f32 * 0.022,
        true,
    )?;
    c.draw_headline_width(
        &ctx.copy.name,
        pad + logo_size + pad * 0.25,
        cy + logo_size * 0.34,
        w as f32 * 0.028,
        true,
        false,
        text_w - logo_size - pad * 0.25,
    )?;
    cy += logo_size + pad * 0.45;

    let headline_size = w as f32 * 0.044;
    let headline = ctx.copy.print_headline();
    c.draw_headline_width(&headline, pad, cy, headline_size, true, false, text_w)?;
    cy += c.measure_wrapped(&headline.to_uppercase(), headline_size, text_w, 1.08)? + headline_size * 0.35;

    let pitch_size = w as f32 * 0.027;
    let pitch = ctx.copy.print_pitch();
    c.draw_body_width(&pitch, pad, cy, pitch_size, true, text_w)?;
    cy += c.measure_wrapped(&pitch, pitch_size, text_w, 1.45)? + pitch_size * 0.6;

    Ok(cy)
}

fn draw_features_block(
    c: &mut Canvas<'_>,
    ctx: &FormatContext<'_>,
    pad: f32,
    mut cy: f32,
    w: u32,
    text_w: f32,
) -> Result<f32> {
    c.draw_eyebrow("What you get", pad, cy, w as f32 * 0.02, true)?;
    cy += w as f32 * 0.038;

    let features = ctx.copy.print_features();
    let bullet_size = w as f32 * 0.025;
    cy = c.draw_bullets(
        &features,
        pad,
        cy,
        bullet_size,
        true,
        features.len(),
        text_w,
    )?;
    Ok(cy + bullet_size * 0.5)
}

fn draw_footer_block(
    c: &mut Canvas<'_>,
    ctx: &FormatContext<'_>,
    pad: f32,
    y: f32,
    w: u32,
    footer_h: f32,
) -> Result<()> {
    let qr_size = (footer_h * 0.42).min(w as f32 * 0.3);
    let qr = render_qr(&ctx.copy.qr_url, 8)?;
    let qr_x = pad + (w as f32 - pad * 2.0) * 0.5 - qr_size * 0.5;
    let qr_y = y + footer_h * 0.08;
    c.draw_qr_block(
        &qr,
        qr_x,
        qr_y,
        qr_size,
        "Scan to visit",
        None,
        true,
    )?;

    let label_size = qr_size * 0.11;
    c.draw_text_centered(
        &ctx.copy.website_label,
        pad + (w as f32 - pad * 2.0) * 0.5,
        qr_y + qr_size + label_size * 1.1,
        label_size * 0.95,
        c.theme.muted,
    )?;

    let support = ctx
        .copy
        .support_email
        .as_deref()
        .unwrap_or(ctx.copy.contact_email.as_str());
    c.draw_text_centered(
        &format!("{support} · {}", ctx.copy.contact_email),
        pad + (w as f32 - pad * 2.0) * 0.5,
        qr_y + qr_size + label_size * 2.6,
        w as f32 * 0.019,
        c.theme.muted,
    )?;
    Ok(())
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .take(2)
        .map(|w| w.chars().next().unwrap_or('S').to_ascii_uppercase())
        .collect()
}
