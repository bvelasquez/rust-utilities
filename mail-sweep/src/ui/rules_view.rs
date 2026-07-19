use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, StatefulWidget};
use ratatui::Frame;

use crate::config::RuleConfig;
use super::keys::{panel_keys_height, render_panel_keys};
use super::theme::{modal_block, panel_block, selected_row, ACCENT, ACCENT2, MUTED, OK, WARN};
use super::Tab;

pub const RULE_CATEGORIES: &[&str] = &[
    "priority",
    "personal",
    "work",
    "newsletter",
    "marketing",
    "notification",
    "receipt",
    "spam",
    "unknown",
];

const UNCATEGORIZED: &str = "uncategorized";

pub fn category_options() -> Vec<&'static str> {
    let mut options: Vec<&'static str> = RULE_CATEGORIES.to_vec();
    options.push(UNCATEGORIZED);
    options
}

pub fn category_label(key: &str) -> &str {
    match key {
        UNCATEGORIZED => "Uncategorized",
        other => other,
    }
}

pub fn category_sort_key(cat: &str) -> usize {
    if cat == UNCATEGORIZED {
        return usize::MAX;
    }
    RULE_CATEGORIES
        .iter()
        .position(|&c| c == cat)
        .unwrap_or(RULE_CATEGORIES.len())
}

pub fn selected_category_index(rule: &RuleConfig) -> usize {
    let current = rule.category.as_deref().unwrap_or(UNCATEGORIZED);
    category_options()
        .iter()
        .position(|&c| c == current)
        .unwrap_or(0)
}

/// Rule indices in grouped display order (top to bottom).
pub fn ordered_rule_indices(rules: &[RuleConfig]) -> Vec<usize> {
    build_grouped_list(rules).visual_to_rule
}

/// Map list selection position to config rule index.
pub fn resolve_rule_index(rules: &[RuleConfig], visual_selected: usize) -> usize {
    ordered_rule_indices(rules)
        .get(visual_selected)
        .copied()
        .unwrap_or(0)
}

/// Map config rule index to list selection position after reordering.
pub fn visual_index_for_rule(rules: &[RuleConfig], rule_index: usize) -> usize {
    ordered_rule_indices(rules)
        .iter()
        .position(|&i| i == rule_index)
        .unwrap_or(0)
}

struct GroupedRulesList {
    items: Vec<ListItem<'static>>,
    visual_to_rule: Vec<usize>,
    visual_to_list_row: Vec<usize>,
    category_count: usize,
}

fn build_grouped_list(rules: &[RuleConfig]) -> GroupedRulesList {
    use std::collections::HashMap;

    let mut by_cat: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, rule) in rules.iter().enumerate() {
        let key = rule
            .category
            .as_deref()
            .unwrap_or(UNCATEGORIZED)
            .to_string();
        by_cat.entry(key).or_default().push(i);
    }

    let mut cat_keys: Vec<String> = by_cat.keys().cloned().collect();
    cat_keys.sort_by_key(|k| category_sort_key(k));

    let mut items = Vec::new();
    let mut visual_to_rule = Vec::with_capacity(rules.len());
    let mut visual_to_list_row = Vec::with_capacity(rules.len());

    for cat in &cat_keys {
        let indices = &by_cat[cat];
        items.push(category_header_item(cat, indices.len()));
        for &rule_index in indices {
            visual_to_list_row.push(items.len());
            visual_to_rule.push(rule_index);
            items.push(rule_item(rule_index, &rules[rule_index]));
        }
    }

    GroupedRulesList {
        items,
        visual_to_rule,
        visual_to_list_row,
        category_count: cat_keys.len(),
    }
}

fn category_header_item(category: &str, count: usize) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(
            format!(" {} ", category_label(category)),
            Style::default()
                .fg(ACCENT2)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("({count})"),
            Style::default().fg(MUTED),
        ),
    ]))
}

fn rule_item(index: usize, rule: &RuleConfig) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(format!("[{index}] "), Style::default().fg(MUTED)),
        Span::raw(rule.r#match.clone()),
        Span::styled(" → ", Style::default().fg(ACCENT2)),
        Span::styled(rule.action.clone(), Style::default().fg(OK)),
    ]))
}

pub fn render_rules(
    f: &mut Frame,
    area: Rect,
    rules: &[RuleConfig],
    visual_selected: usize,
    list_state: &mut ListState,
) {
    let keys_h = panel_keys_height(Tab::Rules);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(keys_h)])
        .split(area);

    if rules.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("No rules yet.", Style::default().fg(MUTED))),
                Line::from(""),
                Line::from("Teach rules on the Triage tab (z/g/i/o or /),"),
                Line::from("or press n here for a newsletter preset."),
            ])
            .block(panel_block("Rules")),
            chunks[0],
        );
        render_panel_keys(chunks[1], f, Tab::Rules);
        return;
    }

    let grouped = build_grouped_list(rules);
    let visual_selected = visual_selected.min(grouped.visual_to_rule.len().saturating_sub(1));
    let list_row = grouped.visual_to_list_row[visual_selected];

    list_state.select(Some(list_row));

    let title = format!(
        "Rules — {} rules · {} categories",
        rules.len(),
        grouped.category_count
    );

    let list = List::new(grouped.items)
        .block(panel_block(&title))
        .highlight_style(selected_row())
        .highlight_symbol("▸ ");

    StatefulWidget::render(list, chunks[0], f.buffer_mut(), list_state);
    render_panel_keys(chunks[1], f, Tab::Rules);
}

pub fn render_category_picker(
    f: &mut Frame,
    area: Rect,
    rule: &RuleConfig,
    selected: usize,
    list_state: &mut ListState,
) {
    f.render_widget(Clear, area);
    let block = modal_block(" change category ", ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let current = rule.category.as_deref().unwrap_or(UNCATEGORIZED);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Rule: ", Style::default().fg(MUTED)),
                Span::styled(&rule.r#match, Style::default().fg(OK)),
            ]),
            Line::from(vec![
                Span::styled("Current: ", Style::default().fg(MUTED)),
                Span::styled(category_label(current), Style::default().fg(ACCENT2)),
            ]),
            Line::from(""),
        ]),
        chunks[0],
    );

    let options = category_options();
    let items: Vec<ListItem> = options
        .iter()
        .map(|cat| {
            ListItem::new(Line::from(Span::styled(
                category_label(cat),
                Style::default().fg(OK),
            )))
        })
        .collect();

    let pick = selected.min(options.len().saturating_sub(1));
    list_state.select(Some(pick));

    let list = List::new(items)
        .highlight_style(selected_row())
        .highlight_symbol("▸ ");

    StatefulWidget::render(list, chunks[1], f.buffer_mut(), list_state);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "j/k select · Enter save · Esc cancel",
                Style::default().fg(WARN),
            )),
        ]),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(cat: Option<&str>) -> RuleConfig {
        RuleConfig {
            id: None,
            r#match: "subject:test".into(),
            category: cat.map(|s| s.into()),
            action: "archive".into(),
            priority: None,
            target_folder: None,
        }
    }

    #[test]
    fn groups_by_category_in_order() {
        let rules = vec![
            rule(Some("newsletter")),
            rule(None),
            rule(Some("priority")),
            rule(Some("newsletter")),
        ];
        let grouped = build_grouped_list(&rules);
        assert_eq!(grouped.category_count, 3);
        assert_eq!(grouped.visual_to_rule, vec![2, 0, 3, 1]);
        assert_eq!(grouped.visual_to_list_row, vec![1, 3, 4, 6]);
    }

    #[test]
    fn visual_navigation_matches_display_order() {
        let rules = vec![
            rule(Some("newsletter")),
            rule(None),
            rule(Some("priority")),
            rule(Some("newsletter")),
        ];
        assert_eq!(resolve_rule_index(&rules, 0), 2);
        assert_eq!(resolve_rule_index(&rules, 1), 0);
        assert_eq!(resolve_rule_index(&rules, 2), 3);
        assert_eq!(resolve_rule_index(&rules, 3), 1);
        assert_eq!(visual_index_for_rule(&rules, 0), 1);
        assert_eq!(visual_index_for_rule(&rules, 1), 3);
    }
}
