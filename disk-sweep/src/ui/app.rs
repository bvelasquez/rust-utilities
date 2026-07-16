use std::io;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::clean::clean_items;
use crate::scan::{format_bytes, scan_targets, ScanItem, ScanReport};
use crate::targets::default_targets;

use super::theme::{checked_style, draw_modal_backdrop, draw_modal_panel, fill_rects, footer_block, highlight_style, modal_surface_style, muted_style, panel_block, selected_style};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Categories = 0,
    Items = 1,
    Detail = 2,
}

enum Mode {
    Browse,
    ConfirmClean,
    Done(()),
}

pub fn run_interactive() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut report = scan_targets(&default_targets())?;
    let mut cat_state = ListState::default();
    cat_state.select(Some(0));
    let mut item_state = ListState::default();
    item_state.select(Some(0));
    let mut active_panel = Panel::Categories;
    let mut mode = Mode::Browse;
    let mut status = "Scan complete".to_string();

    loop {
        terminal.draw(|f| {
            draw(
                f,
                f.area(),
                &report,
                &mut cat_state,
                &mut item_state,
                active_panel,
                &mode,
                &status,
            );
        })?;

        if matches!(mode, Mode::Done(_)) {
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => break,
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match &mut mode {
                    Mode::ConfirmClean => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            status = "Cleaning...".into();
                            terminal.draw(|f| {
                                draw(
                                    f,
                                    f.area(),
                                    &report,
                                    &mut cat_state,
                                    &mut item_state,
                                    active_panel,
                                    &mode,
                                    &status,
                                );
                            })?;

                            let items = all_selected_items(&report);
                            match clean_items(&items, false) {
                                Ok(r) => {
                                    status = format!(
                                        "Freed {} ({} deleted, {} errors). Press q to exit.",
                                        format_bytes(r.bytes_freed),
                                        r.deleted_count,
                                        r.error_count,
                                    );
                                    mode = Mode::Done(());
                                    report = scan_targets(&default_targets())?;
                                }
                                Err(e) => {
                                    status = format!("Clean failed: {e:#}");
                                    mode = Mode::Browse;
                                }
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            mode = Mode::Browse;
                            status = "Clean cancelled".into();
                        }
                        _ => {}
                    },
                    Mode::Browse => {
                        let cat_idx = cat_state.selected().unwrap_or(0);
                        let item_count = report
                            .categories
                            .get(cat_idx)
                            .map(|c| c.items.len())
                            .unwrap_or(0);

                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('c')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                break
                            }
                            KeyCode::Esc => break,
                            KeyCode::Tab => {
                                active_panel = match active_panel {
                                    Panel::Categories => Panel::Items,
                                    Panel::Items => Panel::Detail,
                                    Panel::Detail => Panel::Categories,
                                };
                            }
                            KeyCode::BackTab => {
                                active_panel = match active_panel {
                                    Panel::Categories => Panel::Detail,
                                    Panel::Items => Panel::Categories,
                                    Panel::Detail => Panel::Items,
                                };
                            }
                            KeyCode::Right => {
                                if active_panel == Panel::Categories {
                                    active_panel = Panel::Items;
                                }
                            }
                            KeyCode::Left => {
                                if active_panel == Panel::Items {
                                    active_panel = Panel::Categories;
                                } else if active_panel == Panel::Detail {
                                    active_panel = Panel::Items;
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => match active_panel {
                                Panel::Categories => {
                                    let i = cat_state.selected().unwrap_or(0);
                                    cat_state.select(Some(i.saturating_sub(1)));
                                    item_state.select(Some(0));
                                }
                                Panel::Items | Panel::Detail => {
                                    let i = item_state.selected().unwrap_or(0);
                                    item_state.select(Some(i.saturating_sub(1)));
                                }
                            },
                            KeyCode::Down | KeyCode::Char('j') => match active_panel {
                                Panel::Categories => {
                                    let i = cat_state.selected().unwrap_or(0);
                                    let max = report.categories.len().saturating_sub(1);
                                    cat_state.select(Some((i + 1).min(max)));
                                    item_state.select(Some(0));
                                }
                                Panel::Items | Panel::Detail => {
                                    let i = item_state.selected().unwrap_or(0);
                                    let max = item_count.saturating_sub(1);
                                    item_state.select(Some((i + 1).min(max)));
                                }
                            },
                            KeyCode::Char(' ') => {
                                if active_panel == Panel::Items {
                                    if let Some(item) = selected_item_mut(&mut report, cat_idx, item_state.selected()) {
                                        item.selected = !item.selected;
                                        recalc_totals(&mut report);
                                    }
                                } else if active_panel == Panel::Categories {
                                    if let Some(cat) = report.categories.get_mut(cat_idx) {
                                        let all_selected = cat.items.iter().all(|i| i.selected);
                                        for item in &mut cat.items {
                                            item.selected = !all_selected;
                                        }
                                        recalc_totals(&mut report);
                                    }
                                }
                            }
                            KeyCode::Char('a') => {
                                for cat in &mut report.categories {
                                    for item in &mut cat.items {
                                        if item.exists {
                                            item.selected = true;
                                        }
                                    }
                                }
                                recalc_totals(&mut report);
                                status = "Selected all existing items".into();
                            }
                            KeyCode::Char('n') => {
                                for cat in &mut report.categories {
                                    for item in &mut cat.items {
                                        item.selected = false;
                                    }
                                }
                                recalc_totals(&mut report);
                                status = "Cleared selection".into();
                            }
                            KeyCode::Char('r') => {
                                status = "Rescanning...".into();
                                terminal.draw(|f| {
                                    draw(
                                        f,
                                        f.area(),
                                        &report,
                                        &mut cat_state,
                                        &mut item_state,
                                        active_panel,
                                        &mode,
                                        &status,
                                    );
                                })?;
                                report = scan_targets(&default_targets())?;
                                status = "Rescan complete".into();
                            }
                            KeyCode::Char('c') => {
                                let selected = all_selected_items(&report);
                                if selected.is_empty() {
                                    status = "Nothing selected".into();
                                } else {
                                    mode = Mode::ConfirmClean;
                                    status = format!(
                                        "Confirm clean {} items ({})? [y/N]",
                                        selected.len(),
                                        format_bytes(selected.iter().map(|i| i.size_bytes).sum()),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn all_selected_items(report: &ScanReport) -> Vec<ScanItem> {
    report
        .categories
        .iter()
        .flat_map(|c| c.items.iter().cloned())
        .filter(|i| i.selected && i.exists)
        .collect()
}

fn selected_item_mut<'a>(
    report: &'a mut ScanReport,
    cat_idx: usize,
    item_idx: Option<usize>,
) -> Option<&'a mut ScanItem> {
    let idx = item_idx?;
    report.categories.get_mut(cat_idx)?.items.get_mut(idx)
}

fn recalc_totals(report: &mut ScanReport) {
    let mut total = 0u64;
    let mut selected = 0u64;
    let mut sel_count = 0usize;
    for cat in &mut report.categories {
        cat.total_bytes = cat.items.iter().map(|i| i.size_bytes).sum();
        cat.selected_bytes = cat
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.size_bytes)
            .sum();
        total += cat.total_bytes;
        selected += cat.selected_bytes;
        sel_count += cat.items.iter().filter(|i| i.selected).count();
    }
    report.total_bytes = total;
    report.selected_bytes = selected;
    report.selected_count = sel_count;
}

fn draw(
    f: &mut ratatui::Frame,
    area: Rect,
    report: &ScanReport,
    cat_state: &mut ListState,
    item_state: &mut ListState,
    active_panel: Panel,
    mode: &Mode,
    status: &str,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("disk-sweep", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" — Smart disk cleanup"),
    ]));
    f.render_widget(header, chunks[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(38),
            Constraint::Percentage(40),
        ])
        .split(chunks[1]);

    // Categories panel
    let cat_items: Vec<ListItem> = report
        .categories
        .iter()
        .map(|c| {
            let line = format!(
                "{}  {}",
                truncate(&c.name, 16),
                format_bytes(c.total_bytes)
            );
            ListItem::new(line)
        })
        .collect();
    let cat_block = panel_block("Categories");
    let cat_list = List::new(cat_items)
        .block(cat_block)
        .highlight_style(if active_panel == Panel::Categories {
            highlight_style()
        } else {
            Style::default()
        })
        .highlight_symbol("› ");
    f.render_stateful_widget(cat_list, cols[0], cat_state);

    let cat_idx = cat_state.selected().unwrap_or(0);
    let category = report.categories.get(cat_idx);

    // Items panel
    let item_rows: Vec<ListItem> = category
        .map(|c| {
            c.items
                .iter()
                .map(|item| {
                    let check = if item.selected { "☑" } else { "☐" };
                    let size = format_bytes(item.size_bytes);
                    let name = truncate(&item.name, 28);
                    let style = if item.selected {
                        checked_style()
                    } else if !item.exists {
                        muted_style()
                    } else {
                        Style::default()
                    };
                    ListItem::new(format!("{check} {name}  {size}")).style(style)
                })
                .collect()
        })
        .unwrap_or_default();

    let cat_title = category.map(|c| c.name.as_str()).unwrap_or("Items");
    let items_block = panel_block(cat_title);
    let items_list = List::new(item_rows)
        .block(items_block)
        .highlight_style(if active_panel == Panel::Items {
            highlight_style()
        } else {
            Style::default()
        })
        .highlight_symbol("› ");
    f.render_stateful_widget(items_list, cols[1], item_state);

    // Detail panel
    let detail = if let Some(cat) = category {
        if let Some(item) = item_state
            .selected()
            .and_then(|i| cat.items.get(i))
        {
            let desc = item.description.clone();

            format!(
                "Name: {}\nPath: {}\nSize: {}\nSelected: {}\nExists: {}\n\n{}\n",
                item.name,
                item.path.display(),
                format_bytes(item.size_bytes),
                if item.selected { "yes" } else { "no" },
                if item.exists { "yes" } else { "no" },
                desc,
            )
        } else {
            format!("{}\n\nNo items in this category.", cat.description)
        }
    } else {
        "Select a category.".into()
    };

    let detail_block = panel_block("Details");
    let detail_para = Paragraph::new(detail)
        .block(detail_block)
        .wrap(Wrap { trim: true })
        .style(if active_panel == Panel::Detail {
            highlight_style()
        } else {
            Style::default()
        });
    f.render_widget(detail_para, cols[2]);

    // Footer
    let keys = match mode {
        Mode::ConfirmClean => "y confirm · n cancel",
        Mode::Done(_) => "q exit",
        _ => "Tab panel · Space toggle · a all · n none · r rescan · c clean · q quit",
    };

    let footer_text = format!(
        "{} items selected · {} to reclaim\n{} | {}",
        report.selected_count,
        format_bytes(report.selected_bytes),
        status,
        keys,
    );
    let footer = Paragraph::new(footer_text)
        .block(footer_block(""))
        .style(selected_style());
    f.render_widget(footer, chunks[2]);

    if matches!(mode, Mode::ConfirmClean) {
        draw_modal_backdrop(f, area);
        let popup = centered_rect(60, 20, area);
        let inner = draw_modal_panel(f, popup, "Confirm cleanup", Color::Red);
        fill_rects(f, &[inner], modal_surface_style());
        let prompt = Paragraph::new(format!(
            "Delete {} selected items?\nReclaim ~{}\n\nThis cannot be undone.\n\n[y] yes  [n] no",
            report.selected_count,
            format_bytes(report.selected_bytes),
        ))
        .style(modal_surface_style())
        .wrap(Wrap { trim: true });
        f.render_widget(prompt, inner);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw + 1 > max {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
