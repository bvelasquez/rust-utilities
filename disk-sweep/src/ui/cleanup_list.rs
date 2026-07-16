use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

use crate::scan::{format_bytes, ScanItem};

use super::theme::{checked_style, muted_style};

/// Maps each rendered list row to `cleanup_items` index (`None` = group header).
pub struct GroupedCleanupList {
    pub rows: Vec<ListItem<'static>>,
    pub row_to_item: Vec<Option<usize>>,
}

pub fn build_grouped_cleanup_list(items: &[ScanItem]) -> GroupedCleanupList {
    let mut rows = Vec::new();
    let mut row_to_item = Vec::new();

    let mut groups: Vec<(String, String, Vec<usize>)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(g) = groups.iter_mut().find(|g| g.0 == item.parent_label) {
            g.2.push(i);
        } else {
            groups.push((
                item.parent_label.clone(),
                short_path(&item.parent_path),
                vec![i],
            ));
        }
    }

    for (label, parent_path, indices) in groups {
        let total: u64 = indices.iter().map(|i| items[*i].size_bytes).sum();
        let selected = indices.iter().filter(|i| items[**i].selected).count();
        rows.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("▸ {label}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {parent_path}  ")),
            Span::styled(
                format!("{} · {selected}/{} selected", format_bytes(total), indices.len()),
                muted_style(),
            ),
        ])));
        row_to_item.push(None);

        for i in indices {
            let item = &items[i];
            let check = if item.selected { "☑" } else { "☐" };
            let style = if item.selected {
                checked_style()
            } else {
                Style::default()
            };
            rows.push(ListItem::new(format!(
                "    {check} {}  {}{}",
                truncate(&item.name, 22),
                format_bytes(item.size_bytes),
                risk_suffix(&item.risk)
            ))
            .style(style));
            row_to_item.push(Some(i));
        }
    }

    GroupedCleanupList { rows, row_to_item }
}

pub fn first_selectable_row(row_to_item: &[Option<usize>]) -> usize {
    row_to_item
        .iter()
        .position(|r| r.is_some())
        .unwrap_or(0)
}

pub fn move_selectable_row(
    row_to_item: &[Option<usize>],
    current_row: usize,
    delta: i32,
) -> usize {
    if row_to_item.is_empty() {
        return 0;
    }
    let mut next = current_row;
    for _ in 0..row_to_item.len() {
        next = ((next as i32 + delta).clamp(0, row_to_item.len() as i32 - 1)) as usize;
        if row_to_item[next].is_some() {
            return next;
        }
        if next == current_row {
            break;
        }
    }
    current_row
}

pub fn item_index_at_row(row_to_item: &[Option<usize>], row: usize) -> Option<usize> {
    row_to_item.get(row).copied().flatten()
}

fn short_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..max.saturating_sub(1)])
}

fn risk_suffix(risk: &str) -> String {
    match risk {
        "safe_cleanup" => "  safe".to_string(),
        "caution" => "  !".to_string(),
        _ => String::new(),
    }
}
