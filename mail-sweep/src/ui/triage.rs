use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Cell, Row, StatefulWidget, Table, TableState};
use ratatui::Frame;

use crate::store::{AppliedAnalytics, CachedMessage, PendingSenderGroup};
use crate::ui::analytics::{self, analytics_height};
use crate::ui::keys::{panel_keys_height, render_panel_keys};
use crate::ui::theme::{panel_block, selected_row, ACCENT, MUTED, OK, WARN};
use crate::ui::Tab;

pub fn render_triage(
    f: &mut Frame,
    area: Rect,
    groups: &[PendingSenderGroup],
    leftovers: &[CachedMessage],
    pending_msgs: i64,
    selected: usize,
    table_state: &mut TableState,
    analytics: &AppliedAnalytics,
) {
    let keys_h = panel_keys_height(Tab::Triage);
    let chart_h = analytics_height();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(chart_h),
            Constraint::Min(4),
            Constraint::Length(keys_h),
        ])
        .split(area);

    analytics::render_analytics(f, chunks[0], analytics);
    let content = chunks[1];
    let keys_area = chunks[2];

    if !groups.is_empty() {
        render_sender_groups(f, content, keys_area, groups, pending_msgs, selected, table_state);
        return;
    }

    if !leftovers.is_empty() {
        render_leftovers(f, content, keys_area, leftovers, selected, table_state);
        return;
    }

    let lines = vec![
        ratatui::text::Line::from(ratatui::text::Span::styled(
            "✓ Inbox noise is under control",
            Style::default().fg(OK).add_modifier(Modifier::BOLD),
        )),
        ratatui::text::Line::from(""),
        ratatui::text::Line::from("No unclassified senders and no unread keep/flag mail."),
        ratatui::text::Line::from(""),
        ratatui::text::Line::from("Press s to sync · x to classify · A for AUTO · . for chart period."),
    ];
    f.render_widget(
        ratatui::widgets::Paragraph::new(lines).block(panel_block("Triage")),
        content,
    );
    render_panel_keys(keys_area, f, Tab::Triage);
}

fn render_sender_groups(
    f: &mut Frame,
    content: Rect,
    keys_area: Rect,
    groups: &[PendingSenderGroup],
    pending_msgs: i64,
    selected: usize,
    table_state: &mut TableState,
) {
    let title = format!(
        "Triage — {} senders · {} unclassified msgs",
        groups.len(),
        pending_msgs
    );
    let header = Row::new(vec![
        Cell::from("#"),
        Cell::from("Msgs"),
        Cell::from("New"),
        Cell::from("Sender"),
        Cell::from("Sample subject"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = groups
        .iter()
        .map(|g| {
            Row::new(vec![
                Cell::from(""),
                Cell::from(format!("{}", g.message_count))
                    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from(format!("{}", g.unread_count)).style(Style::default().fg(WARN)),
                Cell::from(truncate(&g.from_address, 28)),
                Cell::from(truncate(&g.sample_subject, 40)),
            ])
        })
        .collect();

    table_state.select(Some(selected));

    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Length(30),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(panel_block(&title))
    .row_highlight_style(selected_row());

    StatefulWidget::render(table, content, f.buffer_mut(), table_state);
    render_panel_keys(keys_area, f, Tab::Triage);
}

fn render_leftovers(
    f: &mut Frame,
    content: Rect,
    keys_area: Rect,
    leftovers: &[CachedMessage],
    selected: usize,
    table_state: &mut TableState,
) {
    let title = format!(
        "Unread — {} kept in inbox (Enter read · m mark read)",
        leftovers.len()
    );
    let header = Row::new(vec![
        Cell::from("Action"),
        Cell::from("From"),
        Cell::from("Subject"),
        Cell::from("Category"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = leftovers
        .iter()
        .map(|m| {
            Row::new(vec![
                Cell::from(m.planned_action.clone().unwrap_or_else(|| "keep".into()))
                    .style(Style::default().fg(WARN)),
                Cell::from(truncate(&m.from_address, 24)),
                Cell::from(truncate(&m.subject, 36)),
                Cell::from(truncate(&m.category, 12)),
            ])
        })
        .collect();

    table_state.select(Some(selected));

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(26),
            Constraint::Min(20),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(panel_block(&title))
    .row_highlight_style(selected_row());

    StatefulWidget::render(table, content, f.buffer_mut(), table_state);
    render_panel_keys(keys_area, f, Tab::Triage);
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
