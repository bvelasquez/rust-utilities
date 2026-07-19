use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::keys::{panel_keys_height, render_panel_keys};
use super::theme::{panel_block, ACCENT, ACCENT2, AI, MUTED, OK, WARN};
use super::Tab;

pub fn render_setup(
    f: &mut Frame,
    area: Rect,
    auto_on: bool,
    poll_label: &str,
    account_lines: Vec<Line<'static>>,
) {
    let keys_h = panel_keys_height(Tab::Setup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(keys_h)])
        .split(area);
    let content = chunks[0];
    let keys_area = chunks[1];

    let auto_line = if auto_on {
        Span::styled(
            format!("ON — every {poll_label}: sync → AI patterns → safe apply"),
            Style::default().fg(OK).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "OFF — press A on any tab to enable hands-free triage",
            Style::default().fg(WARN),
        )
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "Automation",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![Span::styled("AUTO: ", Style::default().fg(MUTED)), auto_line]),
        Line::from(""),
        Line::from(Span::styled("What AUTO does", Style::default().fg(AI))),
        Line::from("  1. Sync new/unread mail from Gmail"),
        Line::from("  2. Match your rules + AI sender patterns"),
        Line::from("  3. Apply safe actions (archive, mark read) — deletes stay in Review"),
        Line::from(""),
        Line::from(Span::styled(
            "Accounts",
            Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(account_lines);

    f.render_widget(Paragraph::new(lines).block(panel_block("Setup")), content);
    render_panel_keys(keys_area, f, Tab::Setup);
}
