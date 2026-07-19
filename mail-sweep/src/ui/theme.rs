use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};

pub const BG: Color = Color::Rgb(18, 18, 26);
pub const SURFACE: Color = Color::Rgb(28, 28, 40);
pub const ACCENT: Color = Color::Rgb(120, 200, 255);
pub const ACCENT2: Color = Color::Rgb(180, 140, 255);
pub const MUTED: Color = Color::Rgb(110, 110, 130);
pub const OK: Color = Color::Rgb(100, 220, 150);
pub const WARN: Color = Color::Rgb(255, 200, 100);
pub const ERR: Color = Color::Rgb(255, 110, 110);
pub const AI: Color = Color::Rgb(196, 142, 88);

pub fn modal_block(title: &str, border: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(border).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(SURFACE))
}

pub fn chrome_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(SURFACE))
}

pub fn panel_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(ACCENT2))
        .style(Style::default().bg(SURFACE))
}

pub fn footer_block() -> Block<'static> {
    Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(MUTED))
        .style(Style::default().bg(BG))
}

pub fn key_style() -> Style {
    Style::default()
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn label_style() -> Style {
    Style::default().fg(MUTED)
}

pub fn selected_row() -> Style {
    Style::default()
        .bg(Color::Rgb(45, 55, 80))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}
