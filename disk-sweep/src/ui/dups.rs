use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::dups::{self, DeleteAction, DupGroup, DupOptions, DupProgress, DupReport, KeepPolicy};
use crate::scan::format_bytes;

use super::progress::centered_rect;
use super::theme::{
    checked_style, draw_modal_panel, fill_rects, footer_block, highlight_style, modal_surface_style,
    muted_style, panel_block,
};

enum Screen {
    Browse,
    Scanning,
    Results,
    Confirm,
}

struct GroupView {
    group: DupGroup,
    keep: usize,
    delete: Vec<bool>,
}

pub fn run_dups_standalone() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_dups_tui(&mut terminal);
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

pub fn run_dups_tui<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let mut cwd = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut show_hidden = false;
    let mut recursive = true;
    let mut sha256 = false;
    let mut excluded: HashSet<PathBuf> = HashSet::new();
    let mut entries = list_dirs(&cwd, show_hidden);
    let mut state = ListState::default();
    state.select(Some(0));
    let mut screen = Screen::Browse;
    let mut status = "Choose a folder, then press s to scan. The scan is read-only.".to_string();
    let mut scan: Option<ActiveDup> = None;
    let mut report: Option<DupReport> = None;
    let mut views: Vec<GroupView> = Vec::new();
    let mut group_state = ListState::default();
    let mut path_state = ListState::default();
    let mut focus_paths = false;
    let mut policy = KeepPolicy::Shortest;
    let mut confirm = String::new();
    let mut progress = String::new();

    loop {
        if let Some(active) = scan.as_mut() {
            if let Some(done) = active.poll(&mut progress) {
                scan = None;
                match done {
                    Ok(found) => {
                        views = views_from(&found, policy);
                        group_state.select(Some(0));
                        path_state.select(Some(0));
                        status = format!(
                            "{} groups, {} extra copies, {} reclaimable",
                            found.groups.len(),
                            found.extra_files(),
                            format_bytes(found.reclaimable_bytes())
                        );
                        report = Some(found);
                        screen = Screen::Results;
                    }
                    Err(err) => {
                        status = err.to_string();
                        screen = Screen::Browse;
                    }
                }
            }
        }

        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(4)])
                .split(area);
            match screen {
                Screen::Browse => {
                    let items: Vec<ListItem> = entries
                        .iter()
                        .map(|entry| {
                            let name = entry_label(entry);
                            if entry.as_os_str() != ".." && excluded.contains(entry) {
                                ListItem::new(Line::from(vec![
                                    Span::styled("[skip] ", checked_style()),
                                    Span::raw(name),
                                ]))
                            } else {
                                ListItem::new(name)
                            }
                        })
                        .collect();
                    let title = format!(
                        "Duplicates — {}  recursive:{}  sha256:{}  skip:{}",
                        short_path(&cwd),
                        if recursive { "on" } else { "off" },
                        if sha256 { "on" } else { "off" },
                        excluded.len()
                    );
                    let list = List::new(items)
                        .block(panel_block(&title))
                        .highlight_style(highlight_style())
                        .highlight_symbol("› ");
                    frame.render_stateful_widget(list, chunks[0], &mut state);
                }
                Screen::Scanning => {
                    frame.render_widget(
                        Paragraph::new(progress.clone())
                            .block(panel_block("Scanning duplicates"))
                            .wrap(Wrap { trim: true }),
                        chunks[0],
                    );
                }
                Screen::Results | Screen::Confirm => {
                    let cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                        .split(chunks[0]);
                    let groups: Vec<ListItem> = views
                        .iter()
                        .map(|view| {
                            ListItem::new(format!(
                                "{} × {}  reclaim {}",
                                format_bytes(view.group.size_bytes),
                                view.group.files.len(),
                                format_bytes(
                                    view.group.size_bytes
                                        * view.group.files.len().saturating_sub(1) as u64
                                )
                            ))
                        })
                        .collect();
                    frame.render_stateful_widget(
                        List::new(groups)
                            .block(panel_block("Duplicate groups"))
                            .highlight_style(if focus_paths {
                                muted_style()
                            } else {
                                highlight_style()
                            })
                            .highlight_symbol("› "),
                        cols[0],
                        &mut group_state,
                    );
                    let paths = views
                        .get(group_state.selected().unwrap_or(0))
                        .map(|view| {
                            view.group
                                .files
                                .iter()
                                .enumerate()
                                .map(|(idx, file)| {
                                    let mark = if idx == view.keep {
                                        "KEEP"
                                    } else if view.delete.get(idx).copied().unwrap_or(false) {
                                        "del "
                                    } else {
                                        "keep"
                                    };
                                    ListItem::new(format!("{mark}  {}", file.path.display()))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    frame.render_stateful_widget(
                        List::new(paths)
                            .block(panel_block("Copies"))
                            .highlight_style(if focus_paths {
                                highlight_style()
                            } else {
                                muted_style()
                            })
                            .highlight_symbol("› "),
                        cols[1],
                        &mut path_state,
                    );
                }
            }
            let keys = match screen {
                Screen::Browse => "j/k move · Space skip folder · * skip all here · n clear skips · Enter open · s scan · q back",
                Screen::Scanning => "q cancel",
                Screen::Results => "Tab paths · Space toggle · K keep this copy · 1/2/3 shortest/newest/oldest · x delete extras · q back",
                Screen::Confirm => "type delete then Enter · Esc cancel",
            };
            frame.render_widget(
                Paragraph::new(format!("{status}\n{keys}"))
                .block(footer_block(""))
                .wrap(Wrap { trim: true }),
                chunks[1],
            );
            if matches!(screen, Screen::Confirm) {
                let popup = centered_rect(70, 40, area);
                let inner = draw_modal_panel(frame, popup, "Delete duplicate copies", Color::Red);
                fill_rects(frame, &[inner], modal_surface_style());
                let marked = marked_actions(report.as_ref(), &views);
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::from(format!(
                            "Permanently delete {} files. One copy of each group is kept.",
                            marked.len()
                        )),
                        Line::from("Each file is hashed again before it is removed."),
                        Line::from(""),
                        Line::from("Type delete and press Enter."),
                        Line::from(Span::styled(
                            format!("> {confirm}"),
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                    ])
                    .style(modal_surface_style())
                    .wrap(Wrap { trim: true }),
                    inner,
                );
            }
        })?;

        if !event::poll(Duration::from_millis(80))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if let Some(active) = scan.take() {
                active.cancel();
            }
            return Ok(());
        }

        match screen {
            Screen::Browse => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => move_state(&mut state, entries.len(), 1),
                KeyCode::Char('k') | KeyCode::Up => move_state(&mut state, entries.len(), -1),
                KeyCode::Char('h') => {
                    show_hidden = !show_hidden;
                    entries = list_dirs(&cwd, show_hidden);
                    state.select(Some(0));
                }
                KeyCode::Char('r') => recursive = !recursive,
                KeyCode::Char('S') => sha256 = !sha256,
                KeyCode::Char(' ') => {
                    if let Some(entry) = entries.get(state.selected().unwrap_or(0)) {
                        if entry.as_os_str() != ".." {
                            if !excluded.remove(entry) {
                                excluded.insert(entry.clone());
                            }
                            status = format!("{} folders will not be scanned", excluded.len());
                        }
                    }
                }
                KeyCode::Char('*') | KeyCode::Char('A') => {
                    for entry in &entries {
                        if entry.as_os_str() != ".." {
                            excluded.insert(entry.clone());
                        }
                    }
                    status = format!("skipping all {} folders here", entries.len().saturating_sub(1));
                }
                KeyCode::Char('n') => {
                    excluded.clear();
                    status = "no folders skipped".into();
                }
                KeyCode::Backspace => {
                    if let Some(parent) = cwd.parent() {
                        cwd = parent.to_path_buf();
                        entries = list_dirs(&cwd, show_hidden);
                        state.select(Some(0));
                    }
                }
                KeyCode::Enter => {
                    if let Some(entry) = entries.get(state.selected().unwrap_or(0)) {
                        if entry.as_os_str() == ".." {
                            if let Some(parent) = cwd.parent() {
                                cwd = parent.to_path_buf();
                            }
                        } else if entry.is_dir() {
                            cwd = entry.clone();
                        }
                        entries = list_dirs(&cwd, show_hidden);
                        state.select(Some(0));
                    }
                }
                KeyCode::Char('s') => {
                    scan = Some(ActiveDup::start(DupOptions {
                        root: cwd.clone(),
                        recursive,
                        min_bytes: 1,
                        confirm_sha256: sha256,
                        cross_devices: false,
                        exclude: excluded.iter().cloned().collect(),
                    }));
                    progress = "Starting read-only duplicate scan…".into();
                    screen = Screen::Scanning;
                }
                _ => {}
            },
            Screen::Scanning => {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    if let Some(active) = scan.as_ref() {
                        active.cancel_flag.store(true, Ordering::Relaxed);
                        status = "Cancelling scan…".into();
                    }
                }
            }
            Screen::Results => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('b') => screen = Screen::Browse,
                KeyCode::Tab => focus_paths = !focus_paths,
                KeyCode::Char('j') | KeyCode::Down => {
                    if focus_paths {
                        let len = views
                            .get(group_state.selected().unwrap_or(0))
                            .map(|v| v.group.files.len())
                            .unwrap_or(0);
                        move_state(&mut path_state, len, 1);
                    } else {
                        move_state(&mut group_state, views.len(), 1);
                        path_state.select(Some(0));
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if focus_paths {
                        let len = views
                            .get(group_state.selected().unwrap_or(0))
                            .map(|v| v.group.files.len())
                            .unwrap_or(0);
                        move_state(&mut path_state, len, -1);
                    } else {
                        move_state(&mut group_state, views.len(), -1);
                        path_state.select(Some(0));
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(view) = views.get_mut(group_state.selected().unwrap_or(0)) {
                        let idx = path_state.selected().unwrap_or(0);
                        if idx != view.keep {
                            if let Some(flag) = view.delete.get_mut(idx) {
                                *flag = !*flag;
                            }
                        }
                    }
                }
                KeyCode::Char('K') => {
                    if let Some(view) = views.get_mut(group_state.selected().unwrap_or(0)) {
                        let idx = path_state.selected().unwrap_or(0);
                        if view.group.files.get(idx).is_some() {
                            view.keep = idx;
                            for (i, flag) in view.delete.iter_mut().enumerate() {
                                *flag = i != idx;
                            }
                        }
                    }
                }
                KeyCode::Char('1') => {
                    policy = KeepPolicy::Shortest;
                    if let Some(found) = &report {
                        views = views_from(found, policy);
                    }
                }
                KeyCode::Char('2') => {
                    policy = KeepPolicy::Newest;
                    if let Some(found) = &report {
                        views = views_from(found, policy);
                    }
                }
                KeyCode::Char('3') => {
                    policy = KeepPolicy::Oldest;
                    if let Some(found) = &report {
                        views = views_from(found, policy);
                    }
                }
                KeyCode::Char('x') => {
                    if marked_actions(report.as_ref(), &views).is_empty() {
                        status = "Nothing marked for deletion".into();
                    } else {
                        confirm.clear();
                        screen = Screen::Confirm;
                    }
                }
                _ => {}
            },
            Screen::Confirm => match key.code {
                KeyCode::Esc => screen = Screen::Results,
                KeyCode::Backspace => {
                    confirm.pop();
                }
                KeyCode::Char(ch) => confirm.push(ch),
                KeyCode::Enter => {
                    if confirm.trim() != "delete" {
                        status = "Confirmation must be the word delete".into();
                        screen = Screen::Results;
                        continue;
                    }
                    let Some(found) = &report else {
                        screen = Screen::Results;
                        continue;
                    };
                    let actions = marked_actions(Some(found), &views);
                    match dups::apply_deletes(&found.root, &actions, true, None) {
                        Ok(prune) => {
                            status = format!(
                                "Deleted {} files. Log: {}",
                                prune.deleted,
                                prune.log_path.unwrap_or_else(|| "none".into())
                            );
                            screen = Screen::Browse;
                            report = None;
                            views.clear();
                        }
                        Err(err) => {
                            status = err.to_string();
                            screen = Screen::Results;
                        }
                    }
                }
                _ => {}
            },
        }
    }
}

fn marked_actions(report: Option<&DupReport>, views: &[GroupView]) -> Vec<DeleteAction> {
    let _ = report;
    let mut actions = Vec::new();
    for view in views {
        let Some(keep) = view.group.files.get(view.keep) else {
            continue;
        };
        for (idx, file) in view.group.files.iter().enumerate() {
            if idx == view.keep || !view.delete.get(idx).copied().unwrap_or(false) {
                continue;
            }
            actions.push(DeleteAction {
                remove: file.path.clone(),
                keep: keep.path.clone(),
                size_bytes: view.group.size_bytes,
                xxh64: view.group.xxh64.clone(),
            });
        }
    }
    actions
}

fn views_from(report: &DupReport, policy: KeepPolicy) -> Vec<GroupView> {
    report
        .groups
        .iter()
        .map(|group| {
            let keep = dups::keep_index(&group.files, policy);
            let delete = (0..group.files.len()).map(|idx| idx != keep).collect();
            GroupView {
                group: group.clone(),
                keep,
                delete,
            }
        })
        .collect()
}

fn entry_label(entry: &Path) -> String {
    if entry.as_os_str() == ".." {
        return "..".to_string();
    }
    entry
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.display().to_string())
}

fn list_dirs(cwd: &Path, show_hidden: bool) -> Vec<PathBuf> {
    let mut entries = vec![PathBuf::from("..")];
    let Ok(read) = fs::read_dir(cwd) else {
        return entries;
    };
    let mut dirs: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| {
            show_hidden
                || path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| !n.starts_with('.'))
                    .unwrap_or(true)
        })
        .collect();
    dirs.sort();
    entries.extend(dirs);
    entries
}

fn move_state(state: &mut ListState, len: usize, delta: i32) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0) as i32;
    let next = (current + delta).clamp(0, len as i32 - 1) as usize;
    state.select(Some(next));
}

fn short_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}

struct ActiveDup {
    rx: Receiver<DupProgress>,
    handle: Option<JoinHandle<Result<DupReport>>>,
    cancel_flag: Arc<AtomicBool>,
}

impl ActiveDup {
    fn start(opts: DupOptions) -> Self {
        let (tx, rx) = mpsc::sync_channel(32);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&cancel_flag);
        let handle = thread::spawn(move || dups::find_duplicates(&opts, &cancel, Some(&tx)));
        Self {
            rx,
            handle: Some(handle),
            cancel_flag,
        }
    }

    fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    fn poll(&mut self, progress: &mut String) -> Option<Result<DupReport>> {
        while let Ok(event) = self.rx.try_recv() {
            *progress = format!(
                "{} {}/{} {}",
                event.phase, event.current, event.total, event.detail
            );
        }
        let handle = self.handle.as_ref()?;
        if !handle.is_finished() {
            return None;
        }
        let handle = self.handle.take()?;
        Some(
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("duplicate scan panicked"))
                .and_then(|r| r),
        )
    }
}
