use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, StatefulWidget, Wrap};
use ratatui::Frame;

use crate::agent::schema::{ClassificationPattern, RuleAuditPlan, RuleAuditSuggestion};
use super::theme::{modal_block, selected_row, ACCENT, ERR, MUTED, OK, WARN};

pub fn render_rule_test(
    f: &mut Frame,
    area: Rect,
    pattern: &str,
    match_count: usize,
    samples: &[(String, String)],
) {
    f.render_widget(Clear, area);
    let block = modal_block(" rule test ", ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Pattern: ", Style::default().fg(MUTED)),
            Span::styled(pattern, Style::default().fg(OK)),
        ]),
        Line::from(vec![
            Span::styled("Matches: ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{match_count} cached messages"),
                Style::default().fg(OK).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    if samples.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no matches in cache)",
            Style::default().fg(MUTED),
        )));
    } else {
        lines.push(Line::from(Span::styled("Samples:", Style::default().fg(MUTED))));
        for (from, subject) in samples.iter().take(5) {
            lines.push(Line::from(format!("  {from} — {subject}")));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Esc close", Style::default().fg(MUTED))));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

pub fn render_rule_audit(
    f: &mut Frame,
    area: Rect,
    plan: &RuleAuditPlan,
    selected: usize,
    accepted: &[usize],
    list_state: &mut ListState,
) {
    f.render_widget(Clear, area);
    let block = modal_block(" rules audit — review suggestions ", ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(&plan.summary, Style::default().fg(MUTED))),
            Line::from(""),
        ]),
        chunks[0],
    );

    if plan.suggestions.is_empty() {
        f.render_widget(
            Paragraph::new("No suggestions — rules look good."),
            chunks[1],
        );
    } else {
        let items: Vec<ListItem> = plan
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| ListItem::new(audit_suggestion_lines(i, s, accepted.contains(&i))))
            .collect();

        list_state.select(Some(
            selected.min(plan.suggestions.len().saturating_sub(1)),
        ));

        let list = List::new(items)
            .highlight_style(selected_row())
            .highlight_symbol("▸ ");

        StatefulWidget::render(list, chunks[1], f.buffer_mut(), list_state);
    }

    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "j/k select · Space toggle · a apply accepted · Esc cancel",
                Style::default().fg(WARN),
            )),
        ]),
        chunks[2],
    );
}

fn audit_suggestion_lines(i: usize, s: &RuleAuditSuggestion, accepted: bool) -> Vec<Line<'static>> {
    let check = if accepted { "✓" } else { " " };
    let kind_style = if s.kind == "remove" || s.proposed_rules.iter().any(|r| r.action == "delete") {
        Style::default().fg(ERR)
    } else {
        Style::default().fg(OK)
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("[{check}] "), Style::default().fg(OK)),
        Span::styled(format!("[{i}] "), Style::default().fg(MUTED)),
        Span::styled(format!("{} ", s.kind), kind_style),
        Span::styled(
            format!("({:.0}%) ", s.confidence * 100.0),
            Style::default().fg(MUTED),
        ),
        Span::raw(s.reason.clone()),
    ])];
    for r in &s.proposed_rules {
        lines.push(Line::from(format!("      + {} → {}", r.r#match, r.action)));
    }
    if !s.retire_indices.is_empty() {
        lines.push(Line::from(format!("      - retire rules {:?}", s.retire_indices)));
    }
    lines
}

pub fn render_pattern_suggest(
    f: &mut Frame,
    area: Rect,
    patterns: &[SuggestItem],
    selected: usize,
    list_state: &mut ListState,
) {
    f.render_widget(Clear, area);
    let block = modal_block(" AI pattern suggestions ", ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Pick a pattern to teach (1-4 or Enter):",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
        ]),
        chunks[0],
    );

    if patterns.is_empty() {
        f.render_widget(Paragraph::new("No suggestions returned."), chunks[1]);
    } else {
        let items: Vec<ListItem> = patterns
            .iter()
            .enumerate()
            .map(|(i, item)| {
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{} ", i + 1), Style::default().fg(OK)),
                        Span::raw(item.pattern.match_pattern.clone()),
                        Span::styled(
                            format!(
                                " → {} ({:.0}%, ~{} msgs)",
                                item.pattern.action,
                                item.pattern.confidence * 100.0,
                                item.match_count
                            ),
                            Style::default().fg(MUTED),
                        ),
                    ]),
                    Line::from(vec![
                        Span::raw("     "),
                        Span::styled(item.pattern.reason.clone(), Style::default().fg(MUTED)),
                    ]),
                ])
            })
            .collect();

        list_state.select(Some(
            selected.min(patterns.len().saturating_sub(1)),
        ));

        let list = List::new(items)
            .highlight_style(selected_row())
            .highlight_symbol("▸ ");

        StatefulWidget::render(list, chunks[1], f.buffer_mut(), list_state);
    }

    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "j/k select · Enter teach · Esc cancel",
                Style::default().fg(WARN),
            )),
        ]),
        chunks[2],
    );
}

#[derive(Clone, Debug)]
pub struct SuggestItem {
    pub pattern: ClassificationPattern,
    pub match_count: usize,
}

pub fn accepted_suggestions<'a>(
    plan: &'a RuleAuditPlan,
    accepted: &[usize],
) -> Vec<&'a RuleAuditSuggestion> {
    accepted
        .iter()
        .filter_map(|&i| plan.suggestions.get(i))
        .collect()
}
