use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Gauge, List, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::analyze::AnalyzeOptions;
use super::active_clean::{ActiveClean, CleanPoll};
use super::progress::CleanProgressView;
use crate::scan::format_bytes;
use crate::volume;
use crate::watch_data::{
    apply_analyze_results, refresh_volume, resolve_watch_paths, ScanKind, ScanUpdate,
    WatchSnapshot,
};

use super::active_analyze::{ActiveAnalyze, AnalyzePoll};
use super::charts::{bar_color, usage_color, Sparkline};
use super::cleanup_list::{
    build_grouped_cleanup_list, first_selectable_row, item_index_at_row, move_selectable_row,
};
use super::progress::{centered_rect, draw_clean_overlay, draw_progress_overlay, ProgressView};
use super::project_picker::{draw_project_picker, ProjectRootPicker};
use super::theme::{
    draw_modal_backdrop, draw_modal_panel, fill_rects, footer_block, highlight_style,
    modal_surface_style, muted_style, panel_block, selected_style, title_style,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    CleanupList,
    Overview,
}

enum WatchMode {
    Normal,
    PickProjectRoot,
    ConfirmClean,
}

struct ActiveScan {
    rx: Receiver<ScanUpdate>,
    cancel: Arc<AtomicBool>,
    finished: bool,
    saw_done: bool,
}

enum ScanPoll {
    Running,
    Done,
    Cancelled,
}

impl ActiveScan {
    fn start(watch_paths: &[(String, PathBuf)], top_n: usize, kind: ScanKind) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ScanUpdate>(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        let paths = watch_paths.to_vec();
        std::thread::spawn(move || {
            let _ = crate::watch_data::collect_snapshot_with_progress(
                &paths,
                top_n,
                kind,
                &cancel_worker,
                &tx,
            );
        });
        Self {
            rx,
            cancel,
            finished: false,
            saw_done: false,
        }
    }

    fn abort(mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.finished = true;
    }

    fn poll(
        &mut self,
        snapshot: &mut WatchSnapshot,
        progress: &mut Option<ProgressView>,
    ) -> ScanPoll {
        let mut got_cancelled = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ScanUpdate::Snapshot(s) => *snapshot = s,
                ScanUpdate::Cancelled => got_cancelled = true,
                ScanUpdate::Failed(e) => {
                    if let Some(p) = progress.as_mut() {
                        p.apply(&ScanUpdate::Failed(e));
                    }
                    self.finished = true;
                    return ScanPoll::Done;
                }
                ScanUpdate::Phase {
                    phase,
                    detail,
                    current,
                    total,
                } => {
                    if phase == "Done" {
                        self.saw_done = true;
                    }
                    if progress.is_none() {
                        *progress = Some(ProgressView::new(&detail, total));
                        if let Some(p) = progress.as_mut() {
                            p.phase = (*phase).to_string();
                            p.current = current;
                        }
                    } else if let Some(p) = progress.as_mut() {
                        p.apply(&ScanUpdate::Phase {
                            phase,
                            detail,
                            current,
                            total,
                        });
                    }
                }
                ScanUpdate::Log(line) => {
                    if progress.is_none() {
                        *progress = Some(ProgressView::new("Scanning", 1));
                    }
                    if let Some(p) = progress.as_mut() {
                        p.apply(&ScanUpdate::Log(line));
                    }
                }
                ScanUpdate::AnalyzeComplete(_) => {}
            }
        }

        if got_cancelled {
            self.finished = true;
            return ScanPoll::Cancelled;
        }

        if self.saw_done {
            self.finished = true;
            return ScanPoll::Done;
        }

        ScanPoll::Running
    }
}

pub async fn run_watch(extra_paths: &[PathBuf], volume_interval: Duration, top_n: usize) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let watch_paths = resolve_watch_paths(extra_paths);
    let anchor = watch_paths
        .first()
        .map(|(_, p)| p.as_path())
        .unwrap_or(std::path::Path::new("/"));

    let mut snapshot = WatchSnapshot::with_volume(volume::stats_for_path(anchor)?);
    let mut item_state = ListState::default();
    item_state.select(Some(0));
    let mut focus = Focus::CleanupList;
    let mut mode = WatchMode::Normal;
    let preferred_root = watch_paths
        .iter()
        .find(|(label, _)| label == "Projects")
        .map(|(_, p)| p.as_path());
    let mut project_picker = ProjectRootPicker::new(preferred_root);
    let mut active_scan: Option<ActiveScan> = None;
    let mut active_analyze: Option<ActiveAnalyze> = None;
    let mut active_clean: Option<ActiveClean> = None;
    let mut progress: Option<ProgressView> = None;
    let mut clean_progress: Option<CleanProgressView> = None;
    let mut last_volume_refresh = Instant::now();
    let mut last_deep_scan = Instant::now();
    let mut status = "Starting deep scan…".to_string();
    let mut history: Vec<f64> = vec![snapshot.volume.used_ratio];

    start_scan(
        &mut active_scan,
        &mut progress,
        &watch_paths,
        top_n,
        ScanKind::Full,
        &mut status,
    );

    let restore = || -> Result<()> {
        disable_raw_mode()?;
        let mut out = io::stdout();
        out.execute(LeaveAlternateScreen)?;
        Ok(())
    };

    loop {
        if let Some(scan) = active_scan.as_mut() {
            match scan.poll(&mut snapshot, &mut progress) {
                ScanPoll::Done => {
                    last_deep_scan = Instant::now();
                    history.push(snapshot.volume.used_ratio);
                    trim_history(&mut history);
                    status = if snapshot.deep_scan_done {
                        "deep scan complete".into()
                    } else {
                        "volume updated".into()
                    };
                    active_scan = None;
                    progress = None;
                }
                ScanPoll::Cancelled => {
                    active_scan = None;
                    progress = None;
                    status = "scan cancelled".into();
                }
                ScanPoll::Running => {}
            }
        }

        if let Some(analyze) = active_analyze.as_mut() {
            match analyze.poll(&mut progress) {
                AnalyzePoll::Complete(items) => {
                    apply_analyze_results(&mut snapshot, items, top_n);
                    history.push(snapshot.volume.used_ratio);
                    trim_history(&mut history);
                    status = format!(
                        "analyze complete — {} candidates (none selected)",
                        snapshot.cleanup_items.len()
                    );
                    active_analyze = None;
                    progress = None;
                }
                AnalyzePoll::Done => {
                    active_analyze = None;
                    progress = None;
                    status = "analyze finished with errors".into();
                }
                AnalyzePoll::Cancelled => {
                    active_analyze = None;
                    progress = None;
                    status = "analyze cancelled".into();
                }
                AnalyzePoll::Running => {}
            }
        }

        let scanning = active_scan.is_some() || active_analyze.is_some();
        let cleaning = active_clean.is_some();
        terminal.draw(|f| {
            draw_watch(
                f,
                f.area(),
                &snapshot,
                &mut item_state,
                focus,
                &mode,
                &history,
                volume_interval,
                last_deep_scan.elapsed(),
                &status,
                progress.as_ref(),
                clean_progress.as_ref(),
                scanning,
                cleaning,
                if matches!(mode, WatchMode::PickProjectRoot) {
                    Some(&mut project_picker)
                } else {
                    None
                },
            );
        })?;

        if let Some(clean) = active_clean.as_mut() {
            if let Some(cp) = clean_progress.as_mut() {
                match clean.poll(cp) {
                    CleanPoll::Running => {}
                    CleanPoll::Done => {
                        let clean = active_clean.take().unwrap();
                        match clean.finish() {
                            Ok(report) => {
                                status = format!(
                                    "freed {} ({} deleted, {} errors)",
                                    format_bytes(report.bytes_freed),
                                    report.deleted_count,
                                    report.error_count,
                                );
                                snapshot.cleanup_items.retain(|i| i.path.exists());
                                let mut largest = snapshot.cleanup_items.clone();
                                largest.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
                                largest.truncate(top_n);
                                snapshot.largest_items = largest;
                                snapshot.recalc_selection();
                                let _ = refresh_volume(&mut snapshot, anchor);
                                history.push(snapshot.volume.used_ratio);
                                trim_history(&mut history);
                            }
                            Err(e) => status = format!("clean failed: {e:#}"),
                        }
                        clean_progress = None;
                        mode = WatchMode::Normal;
                    }
                }
            }
            if active_clean.is_some() {
                if event::poll(Duration::from_millis(80))? {
                    while event::poll(Duration::from_millis(0))? {
                        let _ = event::read()?;
                    }
                }
                continue;
            }
        }

        if matches!(mode, WatchMode::ConfirmClean) {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                let selected: Vec<_> = snapshot
                                    .cleanup_items
                                    .iter()
                                    .filter(|i| i.selected && i.exists)
                                    .cloned()
                                    .collect();
                                if selected.is_empty() {
                                    mode = WatchMode::Normal;
                                    status = "nothing to clean".into();
                                } else {
                                    mode = WatchMode::Normal;
                                    status = "cleaning…".into();
                                    clean_progress = Some(CleanProgressView {
                                        current: 0,
                                        total: selected.len(),
                                        path: String::new(),
                                        log: vec![],
                                    });
                                    active_clean = Some(ActiveClean::start(selected));
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Esc => {
                                mode = WatchMode::Normal;
                                status = "clean cancelled".into();
                            }
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        if matches!(mode, WatchMode::PickProjectRoot) {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                mode = WatchMode::Normal;
                                status = "analyze cancelled".into();
                            }
                            KeyCode::Up | KeyCode::Char('k') => project_picker.move_selection(-1),
                            KeyCode::Down | KeyCode::Char('j') => project_picker.move_selection(1),
                            KeyCode::Enter => {
                                if let Some(root) = project_picker.selected().cloned() {
                                    start_analyze_with_root(
                                        &mut active_analyze,
                                        &mut progress,
                                        &mut status,
                                        root,
                                    );
                                    mode = WatchMode::Normal;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        let timeout = if scanning {
            Duration::from_millis(80)
        } else if volume_interval > Duration::ZERO {
            volume_interval
                .saturating_sub(last_volume_refresh.elapsed())
                .max(Duration::from_millis(100))
        } else {
            Duration::from_millis(200)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if let Some(scan) = active_scan.take() {
                            scan.abort();
                        }
                        if let Some(analyze) = active_analyze.take() {
                            analyze.abort();
                        }
                        restore()?;
                        terminal.show_cursor()?;
                        return Ok(());
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(scan) = active_scan.take() {
                            scan.abort();
                        }
                        if let Some(analyze) = active_analyze.take() {
                            analyze.abort();
                        }
                        restore()?;
                        terminal.show_cursor()?;
                        return Ok(());
                    }
                    KeyCode::Char('r') if active_scan.is_none() && active_analyze.is_none() => start_scan(
                        &mut active_scan,
                        &mut progress,
                        &watch_paths,
                        top_n,
                        ScanKind::Full,
                        &mut status,
                    ),
                    KeyCode::Char('v') if active_scan.is_none() && active_analyze.is_none() => {
                        if let Err(e) = refresh_volume(&mut snapshot, anchor) {
                            status = format!("volume refresh failed: {e:#}");
                        } else {
                            history.push(snapshot.volume.used_ratio);
                            trim_history(&mut history);
                            last_volume_refresh = Instant::now();
                            status = "volume refreshed".into();
                        }
                    }
                    KeyCode::Tab => {
                        focus = if focus == Focus::CleanupList {
                            Focus::Overview
                        } else {
                            Focus::CleanupList
                        };
                    }
                    KeyCode::Up | KeyCode::Char('k') if focus == Focus::CleanupList => {
                        let grouped = build_grouped_cleanup_list(&snapshot.cleanup_items);
                        let row = item_state.selected().unwrap_or(0);
                        let next = move_selectable_row(&grouped.row_to_item, row, -1);
                        item_state.select(Some(next));
                    }
                    KeyCode::Down | KeyCode::Char('j') if focus == Focus::CleanupList => {
                        let grouped = build_grouped_cleanup_list(&snapshot.cleanup_items);
                        let row = item_state.selected().unwrap_or(0);
                        let next = move_selectable_row(&grouped.row_to_item, row, 1);
                        item_state.select(Some(next));
                    }
                    KeyCode::Char(' ') if focus == Focus::CleanupList && active_scan.is_none() && active_analyze.is_none() => {
                        if let Some(row) = item_state.selected() {
                            let grouped = build_grouped_cleanup_list(&snapshot.cleanup_items);
                            if let Some(idx) = item_index_at_row(&grouped.row_to_item, row) {
                                if let Some(item) = snapshot.cleanup_items.get_mut(idx) {
                                    item.selected = !item.selected;
                                    snapshot.recalc_selection();
                                }
                            }
                        }
                    }
                    KeyCode::Char('a') if active_scan.is_none() && active_analyze.is_none() => {
                        project_picker = ProjectRootPicker::new(preferred_root);
                        mode = WatchMode::PickProjectRoot;
                        status = "select projects root for analyze".into();
                    }
                    KeyCode::Char('*') | KeyCode::Char('A') if active_scan.is_none() && active_analyze.is_none() => {
                        for item in &mut snapshot.cleanup_items {
                            if item.exists {
                                item.selected = true;
                            }
                        }
                        snapshot.recalc_selection();
                        status = format!("{} items selected", snapshot.selected_count);
                    }
                    KeyCode::Char('n') if active_scan.is_none() && active_analyze.is_none() => {
                        for item in &mut snapshot.cleanup_items {
                            item.selected = false;
                        }
                        snapshot.recalc_selection();
                        status = "selection cleared".into();
                    }
                    KeyCode::Char('?') => {
                        status = "see: disk-sweep targets explain".into();
                    }
                    KeyCode::Char('c') if active_scan.is_none() && active_analyze.is_none() && active_clean.is_none() => {
                        if snapshot.selected_count == 0 {
                            status = "nothing selected — run r to scan first".into();
                        } else {
                            mode = WatchMode::ConfirmClean;
                        }
                    }
                    _ => {}
                }
            }
        } else if active_scan.is_none()
            && active_analyze.is_none()
            && volume_interval > Duration::ZERO
            && last_volume_refresh.elapsed() >= volume_interval
        {
            if refresh_volume(&mut snapshot, anchor).is_ok() {
                history.push(snapshot.volume.used_ratio);
                trim_history(&mut history);
                last_volume_refresh = Instant::now();
            }
        }
    }
}

fn start_analyze_with_root(
    active_analyze: &mut Option<ActiveAnalyze>,
    progress: &mut Option<ProgressView>,
    status: &mut String,
    projects_root: PathBuf,
) {
    *status = format!(
        "analyzing {} — dot folders, Library, stale projects…",
        short_path(&projects_root)
    );
    *progress = Some(ProgressView::new(
        "Analyze: dot folders, Library, stale projects…",
        3,
    ));
    *active_analyze = Some(ActiveAnalyze::start(AnalyzeOptions {
        projects_root,
        ..AnalyzeOptions::default()
    }));
}

fn start_scan(
    active_scan: &mut Option<ActiveScan>,
    progress: &mut Option<ProgressView>,
    watch_paths: &[(String, PathBuf)],
    top_n: usize,
    kind: ScanKind,
    status: &mut String,
) {
    *status = match kind {
        ScanKind::Full => "deep scanning…".into(),
        ScanKind::Volume => "refreshing volume…".into(),
    };
    *progress = Some(ProgressView::new(
        match kind {
            ScanKind::Full => "Deep scan: folders + cleanup targets…",
            ScanKind::Volume => "Reading volume stats…",
        },
        1,
    ));
    *active_scan = Some(ActiveScan::start(watch_paths, top_n, kind));
}

fn trim_history(history: &mut Vec<f64>) {
    if history.len() > 120 {
        history.remove(0);
    }
}

fn draw_watch(
    f: &mut ratatui::Frame,
    area: Rect,
    snap: &WatchSnapshot,
    item_state: &mut ListState,
    focus: Focus,
    mode: &WatchMode,
    history: &[f64],
    volume_interval: Duration,
    since_deep_scan: Duration,
    status: &str,
    progress: Option<&ProgressView>,
    clean_progress: Option<&CleanProgressView>,
    scanning: bool,
    cleaning: bool,
    project_picker: Option<&mut ProjectRootPicker>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(area);

    draw_volume_header(f, chunks[0], snap, scanning);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    let mid_left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(mid[0]);

    draw_folder_bars(f, mid_left[0], snap, scanning);
    draw_category_bars(f, mid_left[1], snap, scanning);

    draw_cleanup_list(f, mid[1], snap, item_state, focus, scanning);

    draw_sparkline(f, chunks[2], history, snap.volume.used_ratio);
    draw_footer(f, chunks[3], volume_interval, since_deep_scan, snap, status, scanning, cleaning);

    if progress.is_some() || matches!(mode, WatchMode::ConfirmClean | WatchMode::PickProjectRoot) || cleaning {
        draw_modal_backdrop(f, area);
    }

    if let Some(p) = progress {
        let title = if p.phase == "Analyze" { "Analyzing" } else { "Scanning" };
        draw_progress_overlay(f, area, title, p, "q quit (cancels scan)");
    }

    if matches!(mode, WatchMode::PickProjectRoot) {
        if let Some(picker) = project_picker {
            draw_project_picker(f, area, picker);
        }
    }

    if matches!(mode, WatchMode::ConfirmClean) {
        draw_confirm(f, area, snap);
    }

    if let Some(cp) = clean_progress {
        draw_clean_overlay(f, area, cp);
    }
}

fn draw_cleanup_list(
    f: &mut ratatui::Frame,
    area: Rect,
    snap: &WatchSnapshot,
    item_state: &mut ListState,
    focus: Focus,
    scanning: bool,
) {
    let title = format!(
        "Cleanup items [{} selected · {}]",
        snap.selected_count, snap.reclaimable_human
    );

    if snap.cleanup_items.is_empty() {
        let msg = if scanning {
            "Scanning cleanup targets…"
        } else {
            "Press r to run a deep scan and list cleanup targets"
        };
        f.render_widget(Paragraph::new(msg).block(panel_block(&title)), area);
        return;
    }

    let grouped = build_grouped_cleanup_list(&snap.cleanup_items);
    if grouped.rows.is_empty() {
        return;
    }

    if item_state.selected().is_none() {
        item_state.select(Some(first_selectable_row(&grouped.row_to_item)));
    }

    let highlight = if focus == Focus::CleanupList {
        highlight_style()
    } else {
        Style::default()
    };

    let list = List::new(grouped.rows)
        .block(panel_block(&title))
        .highlight_style(highlight)
        .highlight_symbol("› ");

    f.render_stateful_widget(list, area, item_state);
}

fn draw_confirm(f: &mut ratatui::Frame, area: Rect, snap: &WatchSnapshot) {
    let popup = centered_rect(62, 55, area);
    let inner = draw_modal_panel(f, popup, "Confirm cleanup", Color::Red);
    let text_style = modal_surface_style();
    fill_rects(f, &[inner], text_style);

    let mut lines = vec![
        Line::from(format!(
            "Delete {} selected items? Reclaim ~{}",
            snap.selected_count, snap.reclaimable_human
        )),
        Line::from(""),
    ];

    let mut parents: Vec<&str> = Vec::new();
    for item in snap.cleanup_items.iter().filter(|i| i.selected) {
        if !parents.contains(&item.parent_label.as_str()) {
            parents.push(&item.parent_label);
        }
    }
    lines.push(Line::from(Span::styled("Parent folders:", muted_style())));
    for p in parents.iter().take(6) {
        lines.push(Line::from(format!("  • {p}")));
    }
    if parents.len() > 6 {
        lines.push(Line::from(format!("  … and {} more", parents.len() - 6)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("[y] delete   [n] cancel"));

    f.render_widget(
        Paragraph::new(lines).style(text_style).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_volume_header(f: &mut ratatui::Frame, area: Rect, snap: &WatchSnapshot, scanning: bool) {
    let vol = &snap.volume;
    let ratio = vol.used_ratio.clamp(0.0, 1.0);
    let scan_tag = if scanning { " · scanning…" } else { "" };

    let title = format!(
        "Volume: {} — Used {} / {} (free {}){}",
        short_path(&vol.mount_path),
        format_bytes(vol.used_bytes),
        format_bytes(vol.total_bytes),
        format_bytes(vol.available_bytes),
        scan_tag,
    );

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title("Disk usage")
                .title_style(title_style())
                .borders(ratatui::widgets::Borders::ALL),
        )
        .gauge_style(Style::default().fg(usage_color(ratio)).bg(Color::DarkGray))
        .ratio(ratio)
        .label(format!("{:.1}% used", ratio * 100.0));

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(2)])
        .split(area);

    f.render_widget(Paragraph::new(title), inner[0]);
    f.render_widget(gauge, inner[1]);
}

fn draw_folder_bars(f: &mut ratatui::Frame, area: Rect, snap: &WatchSnapshot, loading: bool) {
    if snap.folders.is_empty() {
        let msg = if loading {
            "Measuring folders…"
        } else {
            "r — deep scan for folder sizes"
        };
        f.render_widget(Paragraph::new(msg).block(panel_block("Watched folders")), area);
        return;
    }
    render_bars(
        f,
        area,
        "Watched folders",
        snap.folders
            .iter()
            .map(|f| (format!("{} {}", f.label, f.size_human), f.size_bytes))
            .collect(),
        0,
    );
}

fn draw_category_bars(f: &mut ratatui::Frame, area: Rect, snap: &WatchSnapshot, loading: bool) {
    if snap.categories.is_empty() {
        let msg = if loading {
            "Scanning categories…"
        } else {
            "r — deep scan for categories"
        };
        f.render_widget(Paragraph::new(msg).block(panel_block("Cleanup categories")), area);
        return;
    }
    render_bars(
        f,
        area,
        "Cleanup categories",
        snap.categories
            .iter()
            .map(|c| (format!("{} {}", c.name, c.total_human), c.total_bytes))
            .collect(),
        2,
    );
}

fn render_bars(
    f: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    entries: Vec<(String, u64)>,
    color_offset: usize,
) {
    let max = entries.iter().map(|(_, s)| *s).max().unwrap_or(1).max(1);
    let bars: Vec<Bar> = entries
        .iter()
        .enumerate()
        .map(|(i, (label, size))| {
            let value = ((*size as f64 / max as f64) * 100.0) as u64;
            Bar::default()
                .value(if *size > 0 { value.max(1) } else { 0 })
                .label(Line::from(label.clone()))
                .style(Style::default().fg(bar_color(i + color_offset)))
        })
        .collect();

    f.render_widget(
        BarChart::default()
            .block(panel_block(title))
            .data(BarGroup::default().bars(&bars))
            .bar_width(3)
            .bar_gap(1),
        area,
    );
}

fn draw_sparkline(f: &mut ratatui::Frame, area: Rect, history: &[f64], current: f64) {
    let title = format!("Usage trend — {:.1}%", current * 100.0);
    let block = panel_block(&title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if history.len() < 2 {
        f.render_widget(
            Paragraph::new("Trend builds as volume refreshes (v or --interval)").style(muted_style()),
            inner,
        );
        return;
    }
    f.render_widget(Sparkline::new(history).color(usage_color(current)), inner);
}

fn draw_footer(
    f: &mut ratatui::Frame,
    area: Rect,
    volume_interval: Duration,
    since_deep_scan: Duration,
    snap: &WatchSnapshot,
    status: &str,
    scanning: bool,
    cleaning: bool,
) {
    let vol_hint = if volume_interval > Duration::ZERO {
        format!("vol every {}", format_duration(volume_interval))
    } else {
        "vol manual (v)".into()
    };
    let deep = if snap.deep_scan_done {
        format!("deep scan {} ago", format_duration(since_deep_scan))
    } else {
        "no deep scan yet".into()
    };
    let keys = if cleaning {
        "cleaning…"
    } else if scanning {
        "q quit (cancels scan)"
    } else {
        "Space toggle · * all · n none · c clean · r scan · a analyze · ? help · q quit"
    };
    let text = format!("{status} · {deep} · {vol_hint} · {keys}");
    f.render_widget(
        Paragraph::new(text).block(footer_block("")).style(selected_style()),
        area,
    );
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
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
