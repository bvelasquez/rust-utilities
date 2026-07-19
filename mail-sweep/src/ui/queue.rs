use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, StatefulWidget, Table, TableState};
use ratatui::Frame;

use crate::store::CachedMessage;
use crate::ui::keys::{panel_keys_height, render_panel_keys};
use crate::ui::theme::{panel_block, selected_row, ERR, MUTED, OK, WARN};
use crate::ui::Tab;

pub fn render_queue(
    f: &mut Frame,
    area: Rect,
    items: &[CachedMessage],
    selected: usize,
    table_state: &mut TableState,
    plan_total: usize,
) {
    let keys_h = panel_keys_height(Tab::Review);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(keys_h)])
        .split(area);
    let content = chunks[0];
    let keys_area = chunks[1];

    let ready = plan_total.saturating_sub(items.len());

    if items.is_empty() {
        let lines = if plan_total == 0 {
            vec![
                Line::from(Span::styled(
                    "✓ Nothing pending to apply",
                    Style::default().fg(OK).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Teach on Triage or press x to classify, then a to apply."),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    format!("✓ Nothing needs inspection — {plan_total} ready to apply"),
                    Style::default().fg(OK).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!(
                    "{plan_total} high-confidence action{} in the pending plan.",
                    if plan_total == 1 { "" } else { "s" }
                )),
                Line::from("Press a to apply the full plan to Gmail."),
                Line::from(""),
                Line::from("This tab only lists:"),
                Line::from("  • planned deletes (always reviewed)"),
                Line::from("  • low-confidence archive/flag/keep decisions"),
            ]
        };
        f.render_widget(
            Paragraph::new(lines).block(panel_block("Review")),
            content,
        );
        render_panel_keys(keys_area, f, Tab::Review);
        return;
    }

    let title = if ready > 0 {
        format!(
            "Review — {} need inspection · {} ready · a applies all {}",
            items.len(),
            ready,
            plan_total
        )
    } else {
        format!(
            "Review — {} need inspection before apply (a applies all {})",
            items.len(),
            plan_total.max(items.len())
        )
    };
    let header = Row::new(vec![
        Cell::from("Conf"),
        Cell::from("Action"),
        Cell::from("From"),
        Cell::from("Reason"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = items
        .iter()
        .map(|m| {
            let conf = m.plan_confidence.unwrap_or(0.0);
            let conf_style = if conf < 0.7 {
                Style::default().fg(ERR)
            } else {
                Style::default().fg(OK)
            };
            Row::new(vec![
                Cell::from(format!("{:.0}%", conf * 100.0)).style(conf_style),
                Cell::from(m.planned_action.clone().unwrap_or_default())
                    .style(Style::default().fg(WARN)),
                Cell::from(truncate(&m.from_address, 22)),
                Cell::from(truncate(m.plan_reason.as_deref().unwrap_or(""), 32)),
            ])
        })
        .collect();

    table_state.select(Some(selected));

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(24),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .block(panel_block(&title))
    .row_highlight_style(selected_row());

    StatefulWidget::render(table, content, f.buffer_mut(), table_state);
    render_panel_keys(keys_area, f, Tab::Review);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
