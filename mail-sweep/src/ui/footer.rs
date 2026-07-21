use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::keys::{footer_height as tab_footer_height, footer_key_row_count, keys_for_footer};
use super::theme::{self, footer_block, label_style};
use super::Tab;

#[derive(Clone, Debug)]
pub enum Activity {
    Ready,
    Applying,
    /// Generic in-progress work — shown inline in the footer status line.
    Busy(String),
    AutoIdle,
    Success(String),
    Error(String),
}

impl Activity {
    pub fn spans(&self) -> Vec<Span<'static>> {
        match self {
            Self::Ready => vec![
                Span::styled("● ", Style::default().fg(theme::OK)),
                Span::styled(
                    "Ready — triage unclassified senders or enable AUTO",
                    Style::default().fg(theme::OK),
                ),
            ],
            Self::Applying => vec![
                Span::styled("◌ ", Style::default().fg(theme::WARN)),
                Span::styled(
                    "Applying to Gmail…",
                    Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD),
                ),
            ],
            Self::Busy(msg) => vec![
                Span::styled("◌ ", Style::default().fg(theme::ACCENT)),
                Span::styled(
                    format!("{msg}…"),
                    Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
                ),
            ],
            Self::AutoIdle => vec![
                Span::styled("◉ ", Style::default().fg(theme::ACCENT2)),
                Span::styled(
                    "AUTO on — press A to stop",
                    Style::default().fg(theme::ACCENT2).add_modifier(Modifier::BOLD),
                ),
            ],
            Self::Success(msg) => vec![
                Span::styled("✓ ", Style::default().fg(theme::OK)),
                Span::styled(msg.clone(), Style::default().fg(theme::OK)),
            ],
            Self::Error(msg) => vec![
                Span::styled("✗ ", Style::default().fg(theme::ERR)),
                Span::styled(msg.clone(), Style::default().fg(theme::ERR)),
            ],
        }
    }
}

pub fn render_footer(f: &mut Frame, area: Rect, tab: Tab, activity: &Activity, auto_on: bool) {
    let rows = footer_key_row_count(tab);
    let constraints = vec![ratatui::layout::Constraint::Length(1); 1 + rows];
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let auto_tag = if auto_on {
        Span::styled(
            " AUTO ",
            Style::default()
                .fg(theme::BG)
                .bg(theme::ACCENT2)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" manual ", label_style())
    };

    let mut status_spans = activity.spans();
    status_spans.push(Span::raw("  "));
    status_spans.push(auto_tag);
    f.render_widget(Paragraph::new(Line::from(status_spans)), chunks[0]);

    for row in 0..rows {
        f.render_widget(
            Paragraph::new(keys_for_footer(tab, row as u8)).block(footer_block()),
            chunks[1 + row],
        );
    }
}

pub fn footer_height(tab: Tab) -> u16 {
    tab_footer_height(tab)
}
