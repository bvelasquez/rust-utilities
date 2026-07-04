use crate::print::draw::{Canvas, PanelVariant};
use crate::print::formats::{FormatContext, NamedPage};
use crate::print::qr::render_qr;
use anyhow::Result;

pub fn render(ctx: &FormatContext<'_>) -> Result<Vec<NamedPage>> {
    let (w, h) = ctx.layout.landscape_spread();
    let theme = crate::print::draw::PrintTheme::from_brand(&ctx.cfg.brand);
    let mut canvas = Canvas::new(w, h, ctx.fonts, theme.clone());
    canvas.fill_solid(theme.light_bg);

    let left_w = (w as f32 * 0.4) as u32;
    let right_w = w - left_w;

    render_copy_column(&mut canvas, 0, 0, left_w, h, ctx)?;
    render_hero_column(&mut canvas, left_w, 0, right_w, h, ctx)?;

    Ok(vec![NamedPage {
        name: "single-landscape".into(),
        layout: canvas.img,
        width_in: 11.0,
        height_in: 8.5,
    }])
}

fn render_copy_column(
    c: &mut Canvas<'_>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    ctx: &FormatContext<'_>,
) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Light);
    let pad = w as f32 * 0.09;
    let text_w = w as f32 - pad * 2.0;
    let mut cy = y as f32 + pad;

    let logo_size = w as f32 * 0.16;
    c.draw_logo_or_badge(
        ctx.app_root,
        ctx.cfg.print.copy.logo.as_deref(),
        x as f32 + pad,
        cy,
        logo_size,
        &initials(&ctx.copy.name),
    )?;
    c.draw_eyebrow(
        &ctx.copy.eyebrow,
        x as f32 + pad + logo_size + pad * 0.35,
        cy + logo_size * 0.12,
        w as f32 * 0.028,
        false,
    )?;
    c.draw_headline_width(
        &ctx.copy.name,
        x as f32 + pad + logo_size + pad * 0.35,
        cy + logo_size * 0.38,
        w as f32 * 0.034,
        false,
        false,
        text_w - logo_size - pad * 0.35,
    )?;
    cy += logo_size + pad * 0.5;

    c.draw_eyebrow(
        "What you get",
        x as f32 + pad,
        cy,
        w as f32 * 0.026,
        false,
    )?;
    cy += h as f32 * 0.045;

    c.draw_headline_width(
        &ctx.copy.print_headline(),
        x as f32 + pad,
        cy,
        w as f32 * 0.052,
        false,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.11;

    c.draw_body_width(
        &ctx.copy.print_pitch(),
        x as f32 + pad,
        cy,
        w as f32 * 0.034,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.17;

    let features = ctx.copy.print_features();
    c.draw_bullets(
        &features,
        x as f32 + pad,
        cy,
        w as f32 * 0.032,
        false,
        features.len(),
        text_w,
    )?;

    Ok(())
}

fn render_hero_column(
    c: &mut Canvas<'_>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    ctx: &FormatContext<'_>,
) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Dark);
    let pad = w as f32 * 0.08;
    let text_w = w as f32 - pad * 2.0;
    let mut cy = y as f32 + h as f32 * 0.08;

    c.draw_eyebrow(
        &ctx.copy.name.to_uppercase(),
        x as f32 + pad,
        cy,
        w as f32 * 0.024,
        true,
    )?;
    cy += h as f32 * 0.07;

    for line in ctx.copy.hero_headline() {
        c.draw_headline_width(&line, x as f32 + pad, cy, w as f32 * 0.062, true, true, text_w)?;
        cy += h as f32 * 0.1;
    }
    cy += h as f32 * 0.04;

    c.draw_body_width(
        &ctx.copy.hero_subline(),
        x as f32 + pad,
        cy,
        w as f32 * 0.03,
        true,
        text_w * 0.85,
    )?;
    cy += h as f32 * 0.12;

    c.draw_headline_width(
        ctx.copy.hero_cta(),
        x as f32 + pad,
        cy,
        w as f32 * 0.04,
        true,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.055;
    c.draw_body_width(
        &ctx.copy.contact_email,
        x as f32 + pad,
        cy,
        w as f32 * 0.034,
        true,
        text_w,
    )?;
    cy += h as f32 * 0.045;
    c.draw_body_width(
        &ctx.copy.website_label,
        x as f32 + pad,
        cy,
        w as f32 * 0.03,
        true,
        text_w,
    )?;

    let qr_size = (h as f32 * 0.22).min(w as f32 * 0.28);
    let qr = render_qr(&ctx.copy.qr_url, 8)?;
    c.draw_qr_block(
        &qr,
        x as f32 + w as f32 - pad - qr_size,
        y as f32 + h as f32 - pad - qr_size - h as f32 * 0.04,
        qr_size,
        "Scan to visit",
        Some(&ctx.copy.website_label),
        true,
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
