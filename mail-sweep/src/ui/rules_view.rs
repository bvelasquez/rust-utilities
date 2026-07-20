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

/// Filter Rules list by teach action (same keys as Triage: z/g/i/o).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionFilter {
    #[default]
    All,
    Delete,
    Archive,
    Flag,
    Keep,
}

impl ActionFilter {
    pub fn from_key(c: char) -> Option<Self> {
        match c {
            'z' => Some(Self::Delete),
            'g' => Some(Self::Archive),
            'i' => Some(Self::Flag),
            'o' => Some(Self::Keep),
            '0' | '*' | '.' => Some(Self::All),
            _ => None,
        }
    }

    pub fn matches(self, action: &str) -> bool {
        match self {
            Self::All => true,
            Self::Delete => action == "delete",
            Self::Archive => action == "archive",
            Self::Flag => action == "flag",
            Self::Keep => action == "keep",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Delete => "delete (z)",
            Self::Archive => "archive (g)",
            Self::Flag => "flag (i)",
            Self::Keep => "keep (o)",
        }
    }

    pub fn action_name(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Delete => Some("delete"),
            Self::Archive => Some("archive"),
            Self::Flag => Some("flag"),
            Self::Keep => Some("keep"),
        }
    }
}

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

/// Rule indices in grouped display order (top to bottom), optionally filtered by action.
pub fn ordered_rule_indices(rules: &[RuleConfig], filter: ActionFilter) -> Vec<usize> {
    build_grouped_list(rules, filter).visual_to_rule
}

pub fn visible_rule_count(rules: &[RuleConfig], filter: ActionFilter) -> usize {
    ordered_rule_indices(rules, filter).len()
}

/// Map list selection position to config rule index.
pub fn resolve_rule_index(
    rules: &[RuleConfig],
    visual_selected: usize,
    filter: ActionFilter,
) -> usize {
    ordered_rule_indices(rules, filter)
        .get(visual_selected)
        .copied()
        .unwrap_or(0)
}

/// Map config rule index to list selection position after reordering/filtering.
pub fn visual_index_for_rule(
    rules: &[RuleConfig],
    rule_index: usize,
    filter: ActionFilter,
) -> usize {
    ordered_rule_indices(rules, filter)
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

fn build_grouped_list(rules: &[RuleConfig], filter: ActionFilter) -> GroupedRulesList {
    use std::collections::HashMap;

    let mut by_cat: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, rule) in rules.iter().enumerate() {
        if !filter.matches(&rule.action) {
            continue;
        }
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
    let mut visual_to_rule = Vec::new();
    let mut visual_to_list_row = Vec::new();

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
        Span::styled(format!("({count})"), Style::default().fg(MUTED)),
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
    filter: ActionFilter,
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

    let grouped = build_grouped_list(rules, filter);
    if grouped.visual_to_rule.is_empty() {
        let label = filter.label();
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("No {label} rules."),
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from("Press z/g/i/o to filter by action, 0 to show all."),
            ])
            .block(panel_block(&format!(
                "Rules — filter: {label} · {} total",
                rules.len()
            ))),
            chunks[0],
        );
        render_panel_keys(chunks[1], f, Tab::Rules);
        return;
    }

    let visual_selected = visual_selected.min(grouped.visual_to_rule.len().saturating_sub(1));
    let list_row = grouped.visual_to_list_row[visual_selected];

    list_state.select(Some(list_row));

    let shown = grouped.visual_to_rule.len();
    let title = if filter == ActionFilter::All {
        format!(
            "Rules — {shown} rules · {} categories",
            grouped.category_count
        )
    } else {
        format!(
            "Rules — {}/{} {} · filter: {} · 0=all",
            shown,
            rules.len(),
            filter.action_name().unwrap_or(""),
            filter.label(),
        )
    };

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

    fn rule(cat: Option<&str>, action: &str) -> RuleConfig {
        RuleConfig {
            id: None,
            r#match: "subject:test".into(),
            category: cat.map(|s| s.into()),
            action: action.into(),
            priority: None,
            target_folder: None,
        }
    }

    #[test]
    fn groups_by_category_in_order() {
        let rules = vec![
            rule(Some("newsletter"), "archive"),
            rule(None, "archive"),
            rule(Some("priority"), "flag"),
            rule(Some("newsletter"), "archive"),
        ];
        let grouped = build_grouped_list(&rules, ActionFilter::All);
        assert_eq!(grouped.category_count, 3);
        assert_eq!(grouped.visual_to_rule, vec![2, 0, 3, 1]);
        assert_eq!(grouped.visual_to_list_row, vec![1, 3, 4, 6]);
    }

    #[test]
    fn visual_navigation_matches_display_order() {
        let rules = vec![
            rule(Some("newsletter"), "archive"),
            rule(None, "archive"),
            rule(Some("priority"), "flag"),
            rule(Some("newsletter"), "archive"),
        ];
        assert_eq!(resolve_rule_index(&rules, 0, ActionFilter::All), 2);
        assert_eq!(resolve_rule_index(&rules, 1, ActionFilter::All), 0);
        assert_eq!(resolve_rule_index(&rules, 2, ActionFilter::All), 3);
        assert_eq!(resolve_rule_index(&rules, 3, ActionFilter::All), 1);
        assert_eq!(visual_index_for_rule(&rules, 0, ActionFilter::All), 1);
        assert_eq!(visual_index_for_rule(&rules, 1, ActionFilter::All), 3);
    }

    #[test]
    fn action_filter_shows_only_matching() {
        let rules = vec![
            rule(Some("spam"), "delete"),
            rule(Some("newsletter"), "archive"),
            rule(Some("priority"), "flag"),
            rule(Some("personal"), "keep"),
            rule(Some("spam"), "delete"),
        ];
        assert_eq!(
            ordered_rule_indices(&rules, ActionFilter::Delete),
            vec![0, 4]
        );
        assert_eq!(ordered_rule_indices(&rules, ActionFilter::Keep), vec![3]);
        assert_eq!(visible_rule_count(&rules, ActionFilter::Flag), 1);
        assert_eq!(visible_rule_count(&rules, ActionFilter::All), 5);
    }
}
