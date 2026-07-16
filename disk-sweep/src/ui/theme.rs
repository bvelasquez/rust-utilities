use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::Frame;
use ratatui::widgets::Clear;

pub fn title_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

pub fn highlight_style() -> Style {
    Style::default().bg(Color::DarkGray).fg(Color::White)
}

pub fn checked_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn muted_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub const MODAL_BACKDROP: Color = Color::Rgb(16, 16, 24);
pub const MODAL_SURFACE: Color = Color::Rgb(28, 28, 38);

pub fn modal_surface_style() -> Style {
    Style::default().bg(MODAL_SURFACE).fg(Color::White)
}

pub fn footer_block<'a>(title: &'a str) -> ratatui::widgets::Block<'a> {
    ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::TOP)
        .title(title)
}

pub fn panel_block<'a>(title: &'a str) -> ratatui::widgets::Block<'a> {
    ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .title(title)
        .title_style(title_style())
}

/// Paint every cell in `area` — Blocks alone do not fill backgrounds in ratatui.
pub fn fill_area(f: &mut Frame, area: Rect, style: Style) {
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
                cell.set_char(' ');
                cell.set_style(style);
            }
        }
    }
}

/// Full-screen opaque backdrop for modals.
pub fn draw_modal_backdrop(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
    fill_area(
        f,
        area,
        Style::default().bg(MODAL_BACKDROP).fg(Color::DarkGray),
    );
}

pub fn modal_block<'a>(title: &'a str, border: Color) -> ratatui::widgets::Block<'a> {
    ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .title(format!(" {title} "))
        .title_style(Style::default().fg(border).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(border))
        .style(modal_surface_style())
}

/// Opaque modal panel: filled rect + bordered block.
pub fn draw_modal_panel(f: &mut Frame, area: Rect, title: &str, border: Color) -> Rect {
    fill_area(f, area, modal_surface_style());
    let block = modal_block(title, border);
    let inner = block.inner(area);
    f.render_widget(block, area);
    fill_area(f, inner, modal_surface_style());
    inner
}

/// Re-apply `style` to every sub-rect after layout splits (widgets may clear cell backgrounds).
pub fn fill_rects(f: &mut Frame, rects: &[Rect], style: Style) {
    for rect in rects {
        fill_area(f, *rect, style);
    }
}
