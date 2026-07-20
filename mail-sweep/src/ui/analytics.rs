//! Triage analytics: applied volume over time + action mix (z/g/i/o).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{BarChart, Block, Borders, Paragraph};
use ratatui::Frame;

use crate::store::{ActionCounts, AnalyticsPeriod, AppliedAnalytics};
use super::theme::{panel_block, ACCENT, ERR, MUTED, OK, WARN};

pub fn analytics_height() -> u16 {
    11
}

pub fn render_analytics(f: &mut Frame, area: Rect, stats: &AppliedAnalytics) {
    let total = stats.totals.total();
    let title = format!(
        "Applied {total} · {} · press . to cycle day/week/month",
        stats.period.title()
    );
    let block = panel_block(&title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if total == 0 {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No applied mail in this window yet.",
                    Style::default().fg(MUTED),
                )),
                Line::from(Span::styled(
                    "Press a or enable AUTO — charts fill as plans apply to Gmail.",
                    Style::default().fg(MUTED),
                )),
            ]),
            inner,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(inner);

    render_volume_chart(f, chunks[0], stats);
    render_action_breakdown(f, chunks[1], &stats.totals);
}

fn render_volume_chart(f: &mut Frame, area: Rect, stats: &AppliedAnalytics) {
    let owned: Vec<(String, u64)> = stats
        .buckets
        .iter()
        .map(|b| {
            (
                short_label_owned(&b.label, stats.period),
                b.counts.total() as u64,
            )
        })
        .collect();
    let refs: Vec<(&str, u64)> = owned.iter().map(|(l, n)| (l.as_str(), *n)).collect();
    let max = refs.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
    let bar_width = if refs.len() > 10 { 2 } else { 3 };

    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MUTED))
                .title(Span::styled(
                    format!(" volume/{} (peak {max}) ", stats.period.label()),
                    Style::default().fg(ACCENT),
                )),
        )
        .data(&refs)
        .bar_width(bar_width)
        .bar_gap(1)
        .bar_style(Style::default().fg(ACCENT))
        .value_style(Style::default().fg(OK).add_modifier(Modifier::BOLD))
        .label_style(Style::default().fg(MUTED));

    f.render_widget(chart, area);
}

fn render_action_breakdown(f: &mut Frame, area: Rect, totals: &ActionCounts) {
    let total = totals.total().max(1) as f64;
    let rows = [
        ("z", "delete", totals.delete, ERR),
        ("g", "archive", totals.archive, WARN),
        ("i", "flag", totals.flag, ACCENT),
        ("o", "keep", totals.keep, OK),
    ];

    let mut lines = vec![Line::from(Span::styled(
        "by teach action",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(""));

    for (key, name, n, color) in rows {
        let bar_len = ((n as f64 / total) * 16.0).round() as usize;
        let bar: String = "█".repeat(bar_len.max(if n > 0 { 1 } else { 0 }));
        let pct = (n as f64 / total * 100.0).round() as i64;
        lines.push(Line::from(vec![
            Span::styled(format!("{key} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{name:<7} "), Style::default().fg(MUTED)),
            Span::styled(bar, Style::default().fg(color)),
            Span::styled(format!(" {n} ({pct}%)"), Style::default().fg(OK)),
        ]));
    }

    if totals.other > 0 {
        lines.push(Line::from(Span::styled(
            format!("  other   {} ", totals.other),
            Style::default().fg(MUTED),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MUTED))
                .title(Span::styled(" mix ", Style::default().fg(ACCENT))),
        ),
        area,
    );
}

fn short_label_owned(label: &str, period: AnalyticsPeriod) -> String {
    match period {
        AnalyticsPeriod::Day => {
            if label.len() >= 10 {
                label[5..].to_string()
            } else {
                label.to_string()
            }
        }
        AnalyticsPeriod::Week => {
            // 2026-W28 → W28
            if let Some((_, w)) = label.split_once('-') {
                w.to_string()
            } else {
                label.to_string()
            }
        }
        AnalyticsPeriod::Month => {
            if let Some((_, m)) = label.split_once('-') {
                m.to_string()
            } else {
                label.to_string()
            }
        }
    }
}
