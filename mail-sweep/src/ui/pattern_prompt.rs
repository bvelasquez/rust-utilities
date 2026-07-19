use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::rules::patterns::{validation_detail, validation_status};
use super::theme::{modal_block, ACCENT, ERR, MUTED, OK, WARN};

pub fn render_pattern_editor(
    f: &mut Frame,
    area: Rect,
    buffer: &str,
    title: &str,
    match_count: Option<usize>,
) {
    f.render_widget(Clear, area);
    let block = modal_block(title, ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let status = validation_status(buffer);
    let status_style = if validation_detail(buffer).is_some() {
        Style::default().fg(ERR)
    } else if status == "valid regex" {
        Style::default().fg(OK)
    } else {
        Style::default().fg(MUTED)
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "Edit match pattern, then Enter:",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                buffer,
                Style::default().fg(OK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▌", Style::default().fg(ACCENT)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(status, status_style),
            Span::styled(
                validation_detail(buffer)
                    .map(|e| format!(" — {e}"))
                    .unwrap_or_default(),
                Style::default().fg(ERR),
            ),
        ]),
    ];

    if let Some(n) = match_count {
        lines.push(Line::from(vec![
            Span::styled("  matches ", Style::default().fg(MUTED)),
            Span::styled(format!("{n}"), Style::default().fg(OK)),
            Span::styled(" cached messages", Style::default().fg(MUTED)),
        ]));
    }

    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            "subject:…  from:…  domain:…  header:…  body:…  has:list-unsubscribe",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "all:domain:x.com+subject:y  (compound AND)",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Enter → pick action · Esc cancel",
            Style::default().fg(WARN),
        )),
    ]);

    f.render_widget(Paragraph::new(lines), inner);
}

pub fn render_pattern_action_picker(
    f: &mut Frame,
    area: Rect,
    pattern: &str,
    editing: bool,
) {
    f.render_widget(Clear, area);
    let title = if editing {
        " save rule "
    } else {
        " rule action "
    };
    let block = modal_block(title, ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(vec![
            Span::styled("Pattern: ", Style::default().fg(MUTED)),
            Span::styled(pattern, Style::default().fg(OK)),
        ]),
        Line::from(""),
        Line::from("  z  junk (delete)"),
        Line::from("  g  archive"),
        Line::from("  i  important (flag)"),
        Line::from("  o  keep"),
        Line::from(""),
        Line::from(Span::styled("Esc to cancel", Style::default().fg(MUTED))),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
