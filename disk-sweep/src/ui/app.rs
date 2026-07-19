use std::io;
use std::time::Duration;

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

use crate::analyze::AnalyzeOptions;
use crate::scan::{format_bytes, merge_analyze_into_report, ScanItem, ScanReport};

use super::active_analyze::{ActiveAnalyze, AnalyzePoll};
use super::active_clean::{ActiveClean, CleanPoll};
use super::active_scan::{ActiveScan, ScanPoll};
use super::progress::{centered_rect, draw_clean_overlay, draw_progress_overlay, CleanProgressView, ProgressView};
use super::project_picker::{draw_project_picker, ProjectRootPicker};
use super::theme::{checked_style, draw_modal_backdrop, draw_modal_panel, fill_rects, footer_block, highlight_style, modal_surface_style, muted_style, panel_block, selected_style};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Categories = 0,
    Items = 1,
    Detail = 2,
}

enum Mode {
    Browse,
    PickProjectRoot,
    Analyzing,
    ConfirmClean,
    Cleaning,
    Scanning,
}

pub fn run_interactive() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut report = ScanReport::empty();
    let mut cat_state = ListState::default();
    cat_state.select(Some(0));
    let mut item_state = ListState::default();
    item_state.select(Some(0));
    let mut active_panel = Panel::Categories;
    let mut mode = Mode::Scanning;
    let mut status = "Starting up — scanning cleanup targets…".to_string();
    let mut project_picker = ProjectRootPicker::new(None);
    let mut active_analyze: Option<ActiveAnalyze> = None;
    let mut active_scan: Option<ActiveScan> = Some(ActiveScan::start());
    let mut active_clean: Option<ActiveClean> = None;
    let mut scan_progress: Option<ProgressView> = Some(ProgressView::new(
        "Scanning cleanup targets…",
        1,
    ));
    let mut analyze_progress: Option<ProgressView> = None;
    let mut clean_progress: Option<CleanProgressView> = None;

    loop {
        let busy = active_scan.is_some() || active_analyze.is_some() || active_clean.is_some();

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
                if matches!(mode, Mode::PickProjectRoot) {
                    Some(&mut project_picker)
                } else {
                    None
                },
                scan_progress.as_ref().or(analyze_progress.as_ref()),
                clean_progress.as_ref(),
                busy,
            );
        })?;

        if let Some(scan) = active_scan.as_mut() {
            match scan.poll(&mut scan_progress) {
                ScanPoll::Running => {}
                ScanPoll::Done(result) => {
                    active_scan = None;
                    scan_progress = None;
                    match result {
                        Ok(r) => {
                            report = r;
                            if report.categories.is_empty() {
                                cat_state.select(None);
                            } else {
                                cat_state.select(Some(0));
                            }
                            item_state.select(Some(0));
                            status = format!(
                                "Scan complete — {} items, {} selected by default",
                                report.item_count, report.selected_count
                            );
                        }
                        Err(e) => status = format!("Scan failed: {e:#}"),
                    }
                    mode = Mode::Browse;
                }
            }
        }

        if let Some(analyze) = active_analyze.as_mut() {
            match analyze.poll(&mut analyze_progress) {
                AnalyzePoll::Complete(items) => {
                    merge_analyze_into_report(&mut report, items);
                    recalc_totals(&mut report);
                    status = format!(
                        "Analyze complete — {} items (none selected). Select stale projects and press c to clean.",
                        report.item_count
                    );
                    active_analyze = None;
                    analyze_progress = None;
                    mode = Mode::Browse;
                }
                AnalyzePoll::Done => {
                    active_analyze = None;
                    analyze_progress = None;
                    mode = Mode::Browse;
                    if !status.starts_with("Analyze failed") {
                        status = "Analyze finished with errors".into();
                    }
                }
                AnalyzePoll::Cancelled => {
                    active_analyze = None;
                    analyze_progress = None;
                    mode = Mode::Browse;
                    status = "Analyze cancelled".into();
                }
                AnalyzePoll::Running => {}
            }
        }

        if let Some(clean) = active_clean.as_mut() {
            if let Some(cp) = clean_progress.as_mut() {
                match clean.poll(cp) {
                    CleanPoll::Running => {}
                    CleanPoll::Done => {
                        let clean = active_clean.take().unwrap();
                        match clean.finish() {
                            Ok(r) => {
                                status = format!(
                                    "Freed {} ({} deleted, {} errors)",
                                    format_bytes(r.bytes_freed),
                                    r.deleted_count,
                                    r.error_count,
                                );
                                mode = Mode::Scanning;
                                status = format!("{status} — rescanning…");
                                scan_progress = Some(ProgressView::new("Rescanning cleanup targets…", 1));
                                active_scan = Some(ActiveScan::start());
                            }
                            Err(e) => {
                                status = format!("Clean failed: {e:#}");
                                mode = Mode::Browse;
                            }
                        }
                        clean_progress = None;
                    }
                }
            }
            if active_clean.is_some() {
                drain_keys(Duration::from_millis(80))?;
                continue;
            }
        }

        if matches!(mode, Mode::Analyzing) {
            if event::poll(Duration::from_millis(80))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                if let Some(a) = active_analyze.take() {
                                    a.abort();
                                }
                                analyze_progress = None;
                                mode = Mode::Browse;
                                status = "Analyze cancelled".into();
                            }
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if matches!(mode, Mode::PickProjectRoot) {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                mode = Mode::Browse;
                                status = "Analyze cancelled".into();
                            }
                            KeyCode::Up | KeyCode::Char('k') => project_picker.move_selection(-1),
                            KeyCode::Down | KeyCode::Char('j') => project_picker.move_selection(1),
                            KeyCode::Enter => {
                                if let Some(root) = project_picker.selected().cloned() {
                                    status = format!(
                                        "Analyzing {} — dot folders, Library, stale projects…",
                                        short_path(&root)
                                    );
                                    analyze_progress = Some(ProgressView::new(
                                        "Analyze: dot folders, Library, stale projects…",
                                        3,
                                    ));
                                    active_analyze = Some(ActiveAnalyze::start(AnalyzeOptions {
                                        projects_root: root,
                                        ..AnalyzeOptions::default()
                                    }));
                                    mode = Mode::Analyzing;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if matches!(mode, Mode::ConfirmClean) {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                let items = all_selected_items(&report);
                                if items.is_empty() {
                                    mode = Mode::Browse;
                                    status = "Nothing to clean".into();
                                } else {
                                    mode = Mode::Cleaning;
                                    status = "Cleaning…".into();
                                    clean_progress = Some(CleanProgressView {
                                        current: 0,
                                        total: items.len(),
                                        path: String::new(),
                                        log: vec![],
                                    });
                                    active_clean = Some(ActiveClean::start(items));
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Esc => {
                                mode = Mode::Browse;
                                status = "Clean cancelled".into();
                            }
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if active_scan.is_some() {
            if event::poll(Duration::from_millis(80))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match mode {
                    Mode::Browse => {
                        let cat_idx = cat_state.selected().unwrap_or(0);
                        let item_count = report
                            .categories
                            .get(cat_idx)
                            .map(|c| c.items.len())
                            .unwrap_or(0);

                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('c')
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
                                    if let Some(item) =
                                        selected_item_mut(&mut report, cat_idx, item_state.selected())
                                    {
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
                            KeyCode::Char('*') | KeyCode::Char('A') => {
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
                                mode = Mode::Scanning;
                                status = "Rescanning…".into();
                                scan_progress =
                                    Some(ProgressView::new("Rescanning cleanup targets…", 1));
                                active_scan = Some(ActiveScan::start());
                            }
                            KeyCode::Char('a') => {
                                project_picker = ProjectRootPicker::new(None);
                                mode = Mode::PickProjectRoot;
                                status = "Select projects root for analyze".into();
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
                    Mode::Scanning | Mode::Cleaning | Mode::Analyzing | Mode::ConfirmClean
                    | Mode::PickProjectRoot => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn drain_keys(timeout: Duration) -> Result<()> {
    if event::poll(timeout)? {
        while event::poll(Duration::from_millis(0))? {
            let _ = event::read()?;
        }
    }
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
    report.item_count = report.categories.iter().map(|c| c.items.len()).sum();
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
    project_picker: Option<&mut ProjectRootPicker>,
    operation_progress: Option<&ProgressView>,
    clean_progress: Option<&CleanProgressView>,
    _busy: bool,
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

    if report.categories.is_empty() {
        f.render_widget(
            Paragraph::new(if matches!(mode, Mode::Scanning) {
                "Scanning cleanup targets — sizes will appear shortly…"
            } else {
                "No categories loaded — press r to rescan"
            })
            .block(panel_block("Loading"))
            .style(muted_style()),
            chunks[1],
        );
    } else {
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
    }

    let keys = match mode {
        Mode::ConfirmClean => "y confirm · n cancel",
        Mode::PickProjectRoot => "↑↓ select root · Enter start · Esc cancel",
        Mode::Analyzing => "q cancel analyze",
        Mode::Cleaning => "cleaning…",
        Mode::Scanning => "scanning…",
        Mode::Browse => {
            "Tab · Space toggle · * all · n none · a analyze · r rescan · c clean · q quit"
        }
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

    let show_backdrop = operation_progress.is_some()
        || clean_progress.is_some()
        || matches!(
            mode,
            Mode::ConfirmClean | Mode::PickProjectRoot | Mode::Analyzing | Mode::Cleaning
        );

    if show_backdrop {
        draw_modal_backdrop(f, area);
    }

    if let Some(p) = operation_progress {
        let title = if p.phase == "Analyze" || p.detail.contains("Analyze") {
            "Analyzing"
        } else {
            "Scanning"
        };
        draw_progress_overlay(f, area, title, p, "please wait…");
    }

    if matches!(mode, Mode::ConfirmClean) {
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

    if matches!(mode, Mode::PickProjectRoot) {
        if let Some(picker) = project_picker {
            draw_project_picker(f, area, picker);
        }
    }

    if let Some(cp) = clean_progress {
        draw_clean_overlay(f, area, cp);
    }
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
