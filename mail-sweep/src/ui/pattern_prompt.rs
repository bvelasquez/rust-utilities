use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::rules::patterns::{validation_detail, validation_status};
use super::theme::{modal_block, ACCENT, ERR, MUTED, OK, WARN};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatternEditFocus {
    #[default]
    Pattern,
    Desc,
}

pub fn render_pattern_editor(
    f: &mut Frame,
    area: Rect,
    buffer: &str,
    desc: &str,
    focus: PatternEditFocus,
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

    let pattern_focused = focus == PatternEditFocus::Pattern;
    let desc_focused = focus == PatternEditFocus::Desc;

    let mut lines = vec![
        Line::from(Span::styled(
            "Pattern (Tab → description · F5 generate · Enter on pattern → action):",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if pattern_focused { "▸ " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                if buffer.is_empty() && !pattern_focused {
                    "(empty)".into()
                } else {
                    buffer.to_string()
                },
                if pattern_focused {
                    Style::default().fg(OK).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
            ),
            if pattern_focused {
                Span::styled("▌", Style::default().fg(ACCENT))
            } else {
                Span::raw("")
            },
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

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            if desc_focused { "▸ " } else { "  " },
            Style::default().fg(ACCENT),
        ),
        Span::styled(
            "Describe match for AI",
            if desc_focused {
                Style::default().fg(WARN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        ),
        Span::styled(" (multiline):", Style::default().fg(MUTED)),
    ]));

    if desc.is_empty() && !desc_focused {
        lines.push(Line::from(Span::styled(
            "    e.g. invoices from Stripe mentioning \"paid\" in the subject",
            Style::default().fg(MUTED),
        )));
    } else {
        for (i, row) in desc.split('\n').enumerate() {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    row.to_string(),
                    if desc_focused {
                        Style::default().fg(OK)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                if desc_focused && i + 1 == desc.split('\n').count() {
                    Span::styled("▌", Style::default().fg(ACCENT))
                } else {
                    Span::raw("")
                },
            ]));
        }
        if desc_focused && desc.ends_with('\n') {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("▌", Style::default().fg(ACCENT)),
            ]));
        }
    }

    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            "subject:…  from:…  domain:…  header:…  body:…  has:list-unsubscribe",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Tab focus · F5 AI fill pattern · Enter = newline in description · Enter on pattern → action",
            Style::default().fg(WARN),
        )),
    ]);

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
