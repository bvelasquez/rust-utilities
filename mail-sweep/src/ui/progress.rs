use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Gauge, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::apply_progress::ApplySnapshot;
use super::theme::{modal_block, ACCENT, BG, ERR, MUTED, OK, SURFACE, WARN};

pub fn render_apply_progress(f: &mut Frame, area: Rect, progress: &ApplySnapshot) {
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(BG)),
        area,
    );
    f.render_widget(Clear, area);

    let popup = centered_rect(74, 52, area);
    let block = modal_block(" applying plan ", ACCENT);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner);

    let account = progress
        .account_id
        .as_deref()
        .unwrap_or("—");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Phase: ", Style::default().fg(MUTED)),
            Span::styled(&progress.phase, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(format!("plan #{}", progress.plan_id), Style::default().fg(MUTED)),
        ]))
        .style(Style::default().bg(SURFACE)),
        chunks[0],
    );

    f.render_widget(
        Paragraph::new(format!(
            "Account: {account}  ·  {}/{} messages  ·  ",
            progress.current, progress.total
        ))
        .style(Style::default().fg(MUTED).bg(SURFACE)),
        chunks[1],
    );

    let pct = progress.ratio() * 100.0;
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(ACCENT).bg(Color::Rgb(40, 40, 55)))
            .ratio(progress.ratio())
            .label(format!("{pct:.0}%")),
        chunks[2],
    );

    f.render_widget(
        Paragraph::new(format!(
            "Now: {} uid {}   ·   ✓ {} ok   ·   ✗ {} failed",
            progress.current_action, progress.current_uid, progress.ok_count, progress.fail_count
        ))
        .style(Style::default().fg(WARN).bg(SURFACE)),
        chunks[3],
    );

    let breakdown: String = if progress.action_totals.is_empty() {
        "—".into()
    } else {
        progress
            .action_totals
            .iter()
            .map(|(a, n)| format!("{a}:{n}"))
            .collect::<Vec<_>>()
            .join("  ")
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Actions: ", Style::default().fg(MUTED)),
            Span::styled(breakdown, Style::default().fg(OK)),
        ]))
        .style(Style::default().bg(SURFACE)),
        chunks[4],
    );

    let log_items: Vec<ListItem> = progress
        .log
        .iter()
        .map(|line| {
            let style = if line.starts_with('✗') {
                Style::default().fg(ERR)
            } else {
                Style::default().fg(MUTED)
            };
            ListItem::new(line.as_str()).style(style)
        })
        .collect();
    f.render_widget(
        List::new(log_items).style(Style::default().bg(SURFACE)),
        chunks[5],
    );

    f.render_widget(
        Paragraph::new("Please wait — modifying mailbox via IMAP…")
            .style(Style::default().fg(MUTED).bg(SURFACE)),
        chunks[6],
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
