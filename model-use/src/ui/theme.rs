use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

use crate::providers::types::Provider;

/// Muted palette — avoids loud terminal primaries on dark backgrounds.
pub const ACCENT: Color = Color::Rgb(108, 158, 178);
pub const MUTED: Color = Color::Rgb(92, 98, 112);
pub const PROFIT: Color = Color::Rgb(108, 158, 118);
pub const LOSS: Color = Color::Rgb(188, 108, 108);
pub const WARN: Color = Color::Rgb(188, 158, 96);

pub fn panel_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Rgb(60, 66, 82)))
}

pub fn chrome_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Rgb(45, 50, 64)))
}

pub fn footer_block() -> Block<'static> {
    Block::default()
        .borders(Borders::TOP)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::Rgb(45, 50, 64)))
}

pub fn gauge_color(ratio: f64) -> Color {
    if ratio >= 1.0 {
        LOSS
    } else if ratio > 0.8 {
        LOSS
    } else if ratio > 0.5 {
        WARN
    } else {
        PROFIT
    }
}

pub fn label_style() -> Style {
    Style::default().fg(MUTED)
}

pub fn key_style() -> Style {
    Style::default()
        .fg(Color::Rgb(130, 170, 255))
        .add_modifier(Modifier::BOLD)
}

/// Distinct, muted colors for stacked provider segments in the cost chart.
pub fn provider_color(provider: Provider) -> Color {
    match provider {
        Provider::Openrouter => Color::Rgb(196, 142, 88),
        Provider::Anthropic => Color::Rgb(196, 128, 108),
        Provider::Openai => Color::Rgb(108, 168, 138),
        Provider::Cursor => Color::Rgb(148, 128, 188),
    }
}

pub fn provider_short_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Openrouter => "or",
        Provider::Anthropic => "an",
        Provider::Openai => "oa",
        Provider::Cursor => "cu",
    }
}

/// Global budgets use ratio-based traffic-light colors; provider budgets match the cost chart.
pub fn budget_gauge_fill_color(label: &str, ratio: f64) -> Color {
    if label == "global" {
        return gauge_color(ratio);
    }
    Provider::from_str(label)
        .map(provider_color)
        .unwrap_or_else(|_| gauge_color(ratio))
}
