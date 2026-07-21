use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::store::CachedMessage;
use super::theme::{modal_block, ACCENT, MUTED, OK, WARN};

pub fn body_line_count(msg: &CachedMessage) -> usize {
    body_text(msg).lines().count().max(1)
}

fn body_text(msg: &CachedMessage) -> &str {
    msg.body_text
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(msg.body_preview.as_str())
}

pub fn render_message_read(f: &mut Frame, area: Rect, msg: &CachedMessage, scroll: usize) {
    f.render_widget(Clear, area);
    let title = format!(
        " {} — {} ",
        msg.planned_action.as_deref().unwrap_or("keep"),
        truncate(&msg.subject, 48)
    );
    let block = modal_block(&title, ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let from = msg
        .from_name
        .as_deref()
        .map(|n| format!("{n} <{}>", msg.from_address))
        .unwrap_or_else(|| msg.from_address.clone());

    let headers = vec![
        Line::from(vec![
            Span::styled("From: ", Style::default().fg(MUTED)),
            Span::styled(from, Style::default().fg(OK)),
        ]),
        Line::from(vec![
            Span::styled("Subject: ", Style::default().fg(MUTED)),
            Span::raw(msg.subject.clone()),
        ]),
        Line::from(vec![
            Span::styled("Date: ", Style::default().fg(MUTED)),
            Span::raw(msg.date.clone().unwrap_or_else(|| "—".into())),
            Span::raw("  "),
            Span::styled("Category: ", Style::default().fg(MUTED)),
            Span::styled(msg.category.clone(), Style::default().fg(WARN)),
            Span::raw("  "),
            Span::styled("Action: ", Style::default().fg(MUTED)),
            Span::styled(
                msg.planned_action.clone().unwrap_or_else(|| "keep".into()),
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(headers), chunks[0]);

    let body = body_text(msg);
    let lines: Vec<Line> = body.lines().map(|l| Line::from(l.to_string())).collect();
    let max_scroll = lines.len().saturating_sub(chunks[1].height as usize);
    let scroll = scroll.min(max_scroll);
    let visible: Vec<Line> = lines.into_iter().skip(scroll).collect();
    f.render_widget(
        Paragraph::new(visible).wrap(Wrap { trim: false }),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("j/k", Style::default().fg(ACCENT)),
            Span::styled(" scroll  ", Style::default().fg(MUTED)),
            Span::styled("m", Style::default().fg(ACCENT)),
            Span::styled(" mark read  ", Style::default().fg(MUTED)),
            Span::styled("z/g/i/o", Style::default().fg(ACCENT)),
            Span::styled(" re-teach  ", Style::default().fg(MUTED)),
            Span::styled("Esc", Style::default().fg(ACCENT)),
            Span::styled(" close", Style::default().fg(MUTED)),
        ])),
        chunks[2],
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}
