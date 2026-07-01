use crate::print::draw::{Canvas, PanelVariant};
use crate::print::formats::{FormatContext, NamedPage};
use crate::print::qr::render_qr;
use anyhow::Result;

pub fn render(ctx: &FormatContext<'_>) -> Result<Vec<NamedPage>> {
    let (w, h) = ctx.layout.landscape_spread();
    let panel_w = w / 3;
    let outside = render_outside(ctx, w, h, panel_w)?;
    let inside = render_inside(ctx, w, h, panel_w)?;
    Ok(vec![
        NamedPage {
            name: "trifold-outside".into(),
            layout: outside,
            width_in: 11.0,
            height_in: 8.5,
        },
        NamedPage {
            name: "trifold-inside".into(),
            layout: inside,
            width_in: 11.0,
            height_in: 8.5,
        },
    ])
}

fn render_outside(ctx: &FormatContext<'_>, w: u32, h: u32, panel_w: u32) -> Result<Rgba> {
    let theme = crate::print::draw::PrintTheme::from_brand(&ctx.cfg.brand);
    let mut canvas = Canvas::new(w, h, ctx.fonts, theme.clone());
    canvas.fill_solid(theme.light_bg);

    render_outside_left(&mut canvas, 0, 0, panel_w, h, ctx)?;
    render_outside_center(&mut canvas, panel_w, 0, panel_w, h, ctx)?;
    render_outside_right(&mut canvas, panel_w * 2, 0, panel_w, h, ctx)?;
    Ok(canvas.img)
}

type Rgba = image::RgbaImage;

fn render_inside(ctx: &FormatContext<'_>, w: u32, h: u32, panel_w: u32) -> Result<Rgba> {
    let theme = crate::print::draw::PrintTheme::from_brand(&ctx.cfg.brand);
    let mut canvas = Canvas::new(w, h, ctx.fonts, theme.clone());
    canvas.fill_solid(theme.light_bg);

    render_inside_left(&mut canvas, 0, 0, panel_w, h, ctx)?;
    render_inside_hero(&mut canvas, panel_w, 0, panel_w * 2, h, ctx)?;
    Ok(canvas.img)
}

fn panel_text_w(w: u32, pad: f32) -> f32 {
    w as f32 - pad * 2.0
}

fn render_outside_left(c: &mut Canvas<'_>, x: u32, y: u32, w: u32, h: u32, ctx: &FormatContext<'_>) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Light);
    let pad = w as f32 * 0.065;
    let text_w = panel_text_w(w, pad);
    let mut cy = y as f32 + pad;
    c.draw_eyebrow("Open fully to see", x as f32 + pad, cy, w as f32 * 0.032, false)?;
    cy += h as f32 * 0.06;
    c.draw_headline_width("What we build", x as f32 + pad, cy, w as f32 * 0.072, false, false, text_w)?;
    cy += h as f32 * 0.12;
    c.draw_body_width(
        &format!(
            "Production AI, full-stack web & mobile, and cloud backends — one studio from design through deploy. {}",
            ctx.copy.website_label
        ),
        x as f32 + pad,
        cy,
        w as f32 * 0.038,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.16;
    let bullets: Vec<String> = ctx.copy.print_features().into_iter().take(4).collect();
    c.draw_bullets(
        &bullets,
        x as f32 + pad,
        cy,
        w as f32 * 0.041,
        false,
        bullets.len(),
        text_w,
    )?;
    if let Some(d) = &ctx.copy.disclaimer {
        c.draw_body_width(
            d,
            x as f32 + pad,
            y as f32 + h as f32 - pad - w as f32 * 0.08,
            w as f32 * 0.028,
            false,
            text_w,
        )?;
    }
    Ok(())
}

fn render_outside_center(c: &mut Canvas<'_>, x: u32, y: u32, w: u32, h: u32, ctx: &FormatContext<'_>) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Light);
    let pad = w as f32 * 0.065;
    let text_w = panel_text_w(w, pad);
    let mut cy = y as f32 + pad;
    c.draw_eyebrow("Work with us", x as f32 + pad, cy, w as f32 * 0.032, false)?;
    cy += h as f32 * 0.06;
    c.draw_headline_width(
        "Soup to nuts delivery",
        x as f32 + pad,
        cy,
        w as f32 * 0.068,
        false,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.12;
    c.draw_body_width(
        &format!(
            "From React and React Native clients to Firebase and Python services — we ship coherent features across surfaces. {}",
            ctx.copy.contact_email
        ),
        x as f32 + pad,
        cy,
        w as f32 * 0.037,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.14;
    let items = vec![
        "Production AI & agentic systems".into(),
        "Web + mobile + cloud".into(),
        "Fast iteration, real engineering".into(),
        ctx.copy.contact_email.clone(),
    ];
    c.draw_bullets(
        &items,
        x as f32 + pad,
        cy,
        w as f32 * 0.04,
        false,
        items.len(),
        text_w,
    )?;
    c.draw_text_centered(
        &ctx.copy.website_label,
        x as f32 + w as f32 * 0.5,
        y as f32 + h as f32 - pad - w as f32 * 0.06,
        w as f32 * 0.042,
        c.theme.light_fg,
    )?;
    Ok(())
}

fn render_outside_right(c: &mut Canvas<'_>, x: u32, y: u32, w: u32, h: u32, ctx: &FormatContext<'_>) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Dark);
    let pad = w as f32 * 0.06;
    let text_w = panel_text_w(w, pad);
    let cx = x as f32 + w as f32 * 0.5;
    let mut cy = y as f32 + pad;

    c.draw_logo_or_badge(
        ctx.app_root,
        ctx.cfg.print.copy.logo.as_deref(),
        cx - w as f32 * 0.055,
        cy,
        w as f32 * 0.11,
        &initials(&ctx.copy.name),
    )?;
    cy += w as f32 * 0.14;
    c.draw_eyebrow(&ctx.copy.eyebrow, x as f32 + pad, cy, w as f32 * 0.022, true)?;
    cy += h as f32 * 0.045;
    c.draw_headline_width(
        &ctx.copy.print_headline(),
        x as f32 + pad,
        cy,
        w as f32 * 0.038,
        true,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.14;

    let qr_size = w as f32 * 0.54;
    let qr = render_qr(&ctx.copy.qr_url, 8)?;
    c.draw_qr_block(
        &qr,
        cx - qr_size * 0.5,
        cy,
        qr_size,
        "Scan to visit",
        Some(&ctx.copy.website_label),
        true,
    )?;

    c.draw_text_upper(
        "Open inside →",
        cx - w as f32 * 0.22,
        y as f32 + h as f32 - pad - w as f32 * 0.04,
        w as f32 * 0.028,
        c.theme.accent,
        0.1,
    )?;
    Ok(())
}

fn render_inside_left(c: &mut Canvas<'_>, x: u32, y: u32, w: u32, h: u32, ctx: &FormatContext<'_>) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Light);
    let pad = w as f32 * 0.07;
    let text_w = panel_text_w(w, pad);
    let mut cy = y as f32 + pad;
    c.draw_eyebrow("Why teams call us", x as f32 + pad, cy, w as f32 * 0.03, false)?;
    cy += h as f32 * 0.055;
    c.draw_headline_width(
        "Demo chatbots aren't production.",
        x as f32 + pad,
        cy,
        w as f32 * 0.056,
        false,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.14;

    let pains = vec![
        ("Prototype stall", "Ideas stuck in pilots without a path to ship."),
        ("Fragmented vendors", "Design, mobile, backend, and AI owned by different teams."),
        ("Ops blind spots", "No guardrails, memory, or operator controls in agentic systems."),
    ];
    for (title, body) in pains {
        c.draw_headline_width(title, x as f32 + pad, cy, w as f32 * 0.042, false, false, text_w)?;
        cy += w as f32 * 0.05;
        c.draw_body_width(body, x as f32 + pad, cy, w as f32 * 0.035, false, text_w)?;
        cy += h as f32 * 0.11;
    }

    c.draw_body_width(
        &ctx.copy.print_pitch(),
        x as f32 + pad,
        y as f32 + h as f32 - pad - w as f32 * 0.12,
        w as f32 * 0.032,
        false,
        text_w,
    )?;
    Ok(())
}

fn render_inside_hero(c: &mut Canvas<'_>, x: u32, y: u32, w: u32, h: u32, ctx: &FormatContext<'_>) -> Result<()> {
    c.fill_panel(x, y, w, h, PanelVariant::Dark);
    let pad_x = w as f32 * 0.035;
    let pad_top = h as f32 * 0.045;
    let text_w = w as f32 - pad_x * 2.0;
    let mut cy = y as f32 + pad_top;

    c.draw_eyebrow(
        &format!("Full stack · {}", ctx.copy.website_label),
        x as f32 + pad_x,
        cy,
        h as f32 * 0.024,
        true,
    )?;
    cy += h as f32 * 0.05;
    c.draw_headline_width(
        "AI, web, mobile & cloud — one team",
        x as f32 + pad_x,
        cy,
        h as f32 * 0.052,
        true,
        false,
        text_w,
    )?;
    cy += h as f32 * 0.09;
    c.draw_body_width(
        &ctx.copy.print_pitch(),
        x as f32 + pad_x,
        cy,
        h as f32 * 0.026,
        true,
        text_w,
    )?;
    cy += h as f32 * 0.12;

    let bullets = ctx.copy.print_features();
    let standout = bullets.len().saturating_sub(2);
    c.draw_bullets(
        &bullets,
        x as f32 + pad_x,
        cy,
        h as f32 * 0.028,
        true,
        standout,
        text_w,
    )?;

    let qr = render_qr(&ctx.copy.qr_url, 6)?;
    let qr_size = h as f32 * 0.22;
    c.draw_qr_block(
        &qr,
        x as f32 + w as f32 - pad_x - qr_size,
        y as f32 + h as f32 - qr_size * 1.35,
        qr_size,
        &ctx.copy.website_label,
        Some(&ctx.copy.contact_email),
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
