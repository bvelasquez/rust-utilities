//! ratatui deploy dashboard — projects, targets, deployments, and live output.
//! Mouse + keyboard navigation modeled on pi-commander watch.
//!
//! Digits: 1 deployments · 2 projects · 3 targets · 4 log. Tab cycles focus.
//! Click a pane or row to select; wheel scrolls log or moves list selection.

use std::io;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::Terminal;
use uuid::Uuid;

use crate::api::ApiHandle;
use crate::commands::AppContext;
use crate::config::find_project;
use crate::jobs::{JobPhase, JobRecord};

const SHORTCUTS: &str =
    "1-4 panes  Tab  j/k · wheel  Enter run  [/] scroll  g/G top/end  r reload  x reset  q quit";

enum Mode {
    Browse,
    ConfirmRun,
    ConfirmQuit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Deployments,
    Projects,
    Targets,
    Log,
}

impl Focus {
    fn next(self) -> Self {
        match self {
            Focus::Deployments => Focus::Projects,
            Focus::Projects => Focus::Targets,
            Focus::Targets => Focus::Log,
            Focus::Log => Focus::Deployments,
        }
    }

    fn prev(self) -> Self {
        match self {
            Focus::Deployments => Focus::Log,
            Focus::Projects => Focus::Deployments,
            Focus::Targets => Focus::Projects,
            Focus::Log => Focus::Targets,
        }
    }

    fn from_digit(ch: char) -> Option<Self> {
        match ch {
            '1' => Some(Focus::Deployments),
            '2' => Some(Focus::Projects),
            '3' => Some(Focus::Targets),
            '4' => Some(Focus::Log),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Focus::Deployments => "deployments",
            Focus::Projects => "projects",
            Focus::Targets => "targets",
            Focus::Log => "log",
        }
    }
}

struct LayoutHit {
    header: Rect,
    deployments: Rect,
    projects: Rect,
    targets: Rect,
    log: Rect,
    footer: Rect,
}

/// Restores the terminal if the TUI exits via error, Ctrl+C, or panic.
struct TuiGuard;

impl TuiGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = stdout.execute(Show);
}

pub async fn run_dashboard(mut ctx: AppContext, api: Option<ApiHandle>) -> Result<()> {
    let api_label = api
        .as_ref()
        .map(|h| format!("api http://{}", h.bind))
        .unwrap_or_else(|| "api off".into());

    let _tui_guard = TuiGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let empty = Rect::new(0, 0, 0, 0);
    let mut project_state = ListState::default();
    project_state.select(Some(0));
    let mut target_state = ListState::default();
    target_state.select(Some(0));
    let mut deployment_state = ListState::default();
    deployment_state.select(Some(0));
    let mut focus = Focus::Deployments;
    let mut mode = Mode::Browse;
    let mut status_line = format!("ready — {api_label} — click a pane or press 1–4 / Tab");
    let mut pending_target: Option<(String, String)> = None;
    let mut broker_ok = ctx.jobs.broker().health().await.unwrap_or(false);
    let mut log_scroll: usize = 0;
    let mut log_follow_tail = true;
    let mut selected_job_id: Option<Uuid> = None;
    let mut hit = LayoutHit {
        header: empty,
        deployments: empty,
        projects: empty,
        targets: empty,
        log: empty,
        footer: empty,
    };

    loop {
        let jobs = ctx.jobs.list_jobs().await;
        let running_count = ctx.jobs.running_count().await;
        clamp_deployment_selection(&jobs, &mut deployment_state, &mut selected_job_id);

        let tick = Duration::from_millis(200);
        if event::poll(tick)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match mode {
                        Mode::Browse => match key.code {
                            KeyCode::Char('q') => {
                                let running = ctx.jobs.running_count().await;
                                if running > 0 {
                                    mode = Mode::ConfirmQuit;
                                } else {
                                    break;
                                }
                            }
                            KeyCode::Char('r') => {
                                ctx.reload_config()?;
                                status_line = "reloaded config".into();
                            }
                            KeyCode::Char('x') => {
                                let removed = ctx.jobs.clear_history().await?;
                                selected_job_id = None;
                                deployment_state.select(None);
                                log_scroll = 0;
                                log_follow_tail = true;
                                status_line = if removed == 0 {
                                    "nothing to clear (status reset)".into()
                                } else {
                                    format!("cleared {removed} finished deployment(s)")
                                };
                            }
                            KeyCode::Tab => {
                                focus = focus.next();
                                status_line = format!("focus: {}", focus.label());
                            }
                            KeyCode::BackTab => {
                                focus = focus.prev();
                                status_line = format!("focus: {}", focus.label());
                            }
                            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                                if let Some(f) = Focus::from_digit(ch) {
                                    focus = f;
                                    status_line = format!("focus: {}", f.label());
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                move_selection(
                                    &ctx,
                                    &jobs,
                                    focus,
                                    -1,
                                    &mut project_state,
                                    &mut target_state,
                                    &mut deployment_state,
                                    &mut selected_job_id,
                                    &mut log_scroll,
                                    &mut log_follow_tail,
                                );
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                move_selection(
                                    &ctx,
                                    &jobs,
                                    focus,
                                    1,
                                    &mut project_state,
                                    &mut target_state,
                                    &mut deployment_state,
                                    &mut selected_job_id,
                                    &mut log_scroll,
                                    &mut log_follow_tail,
                                );
                            }
                            KeyCode::PageUp => {
                                if focus == Focus::Log {
                                    log_follow_tail = false;
                                    let step = log_visible_rows(hit.log).max(1);
                                    log_scroll = log_scroll.saturating_sub(step);
                                }
                            }
                            KeyCode::PageDown => {
                                if focus == Focus::Log {
                                    log_follow_tail = false;
                                    let step = log_visible_rows(hit.log).max(1);
                                    log_scroll = log_scroll.saturating_add(step);
                                }
                            }
                            KeyCode::Char('[') => {
                                if focus == Focus::Log {
                                    log_follow_tail = false;
                                    log_scroll = log_scroll.saturating_sub(1);
                                }
                            }
                            KeyCode::Char(']') => {
                                if focus == Focus::Log {
                                    log_follow_tail = false;
                                    log_scroll = log_scroll.saturating_add(1);
                                }
                            }
                            KeyCode::Char('g') | KeyCode::Home => {
                                jump_edge(
                                    &ctx,
                                    &jobs,
                                    focus,
                                    true,
                                    &mut project_state,
                                    &mut target_state,
                                    &mut deployment_state,
                                    &mut selected_job_id,
                                    &mut log_scroll,
                                    &mut log_follow_tail,
                                );
                            }
                            KeyCode::Char('G') | KeyCode::End => {
                                jump_edge(
                                    &ctx,
                                    &jobs,
                                    focus,
                                    false,
                                    &mut project_state,
                                    &mut target_state,
                                    &mut deployment_state,
                                    &mut selected_job_id,
                                    &mut log_scroll,
                                    &mut log_follow_tail,
                                );
                            }
                            KeyCode::Enter => {
                                if focus == Focus::Deployments {
                                    focus = Focus::Log;
                                    log_follow_tail = true;
                                    status_line = "focus: log".into();
                                } else if let Some((pid, tid)) =
                                    selected_target(&ctx, &project_state, &target_state)
                                {
                                    if ctx.config.defaults.confirm_deploys {
                                        pending_target = Some((pid, tid));
                                        mode = Mode::ConfirmRun;
                                    } else {
                                        if let Err(e) =
                                            start_deploy(&ctx, &pid, &tid, &mut status_line).await
                                        {
                                            status_line = format!("deploy failed: {e:#}");
                                        }
                                        log_follow_tail = true;
                                    }
                                }
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                break;
                            }
                            _ => {}
                        },
                        Mode::ConfirmRun => match key.code {
                            KeyCode::Char('y') | KeyCode::Enter => {
                                if let Some((pid, tid)) = pending_target.take() {
                                    if let Err(e) =
                                        start_deploy(&ctx, &pid, &tid, &mut status_line).await
                                    {
                                        status_line = format!("deploy failed: {e:#}");
                                    }
                                    log_follow_tail = true;
                                }
                                mode = Mode::Browse;
                            }
                            KeyCode::Char('n') | KeyCode::Esc => {
                                pending_target = None;
                                mode = Mode::Browse;
                            }
                            _ => {}
                        },
                        Mode::ConfirmQuit => match key.code {
                            KeyCode::Char('y') => break,
                            KeyCode::Char('n') | KeyCode::Esc => mode = Mode::Browse,
                            _ => {}
                        },
                    }
                }
                Event::Mouse(m) => {
                    if matches!(mode, Mode::Browse) {
                        handle_mouse(
                            m,
                            &ctx,
                            &jobs,
                            &hit,
                            &mut focus,
                            &mut project_state,
                            &mut target_state,
                            &mut deployment_state,
                            &mut selected_job_id,
                            &mut log_scroll,
                            &mut log_follow_tail,
                            &mut status_line,
                        );
                    }
                }
                Event::Resize(_, _) => {
                    // next draw recomputes hit rects
                }
                _ => {}
            }
        }

        let active_job = selected_job(&jobs, &selected_job_id);

        terminal.draw(|f| {
            let area = f.area();
            let deploy_h = deployments_panel_height(&jobs, area.height);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(deploy_h),
                    Constraint::Min(4),
                    Constraint::Length(1),
                    Constraint::Length(3),
                ])
                .split(area);

            hit.header = chunks[0];
            hit.deployments = chunks[1];
            hit.footer = chunks[4];

            let broker_span = if broker_ok {
                Span::styled("broker ●", Style::default().fg(Color::Green))
            } else {
                Span::styled("broker ○", Style::default().fg(Color::Yellow))
            };
            let running_span = if running_count > 0 {
                Span::styled(
                    format!("  {running_count} active"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  idle")
            };
            let header = Paragraph::new(Line::from(vec![
                Span::styled(
                    "soki-ci",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                broker_span,
                running_span,
                Span::raw(format!("  focus[{}]", focus.label())),
                Span::styled(
                    format!("  {api_label}"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(format!("  {}", ctx.config_path.display())),
            ]))
            .block(Block::default().borders(Borders::ALL).title("deploy"));
            f.render_widget(header, chunks[0]);

            render_deployments(
                f,
                chunks[1],
                &jobs,
                &mut deployment_state,
                focus == Focus::Deployments,
            );

            let mid = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(22),
                    Constraint::Percentage(22),
                    Constraint::Percentage(56),
                ])
                .split(chunks[2]);

            hit.projects = mid[0];
            hit.targets = mid[1];
            hit.log = mid[2];

            render_projects(
                f,
                mid[0],
                &ctx,
                &jobs,
                &mut project_state,
                focus == Focus::Projects,
            );
            render_targets(
                f,
                mid[1],
                &ctx,
                &jobs,
                &project_state,
                &mut target_state,
                focus == Focus::Targets,
            );
            log_scroll = render_log(
                f,
                mid[2],
                active_job,
                log_scroll,
                log_follow_tail,
                focus == Focus::Log,
            );

            f.render_widget(
                Paragraph::new(Span::styled(
                    SHORTCUTS,
                    Style::default().fg(Color::DarkGray),
                )),
                chunks[3],
            );

            let footer = match mode {
                Mode::Browse => status_line.clone(),
                Mode::ConfirmRun => "Run this deploy? [y] yes  [n] cancel".to_string(),
                Mode::ConfirmQuit => format!(
                    "{running_count} job(s) still running — quit anyway? Deploys keep running. [y] quit  [n] stay"
                ),
            };
            let foot = Paragraph::new(footer).block(Block::default().borders(Borders::ALL));
            f.render_widget(foot, chunks[4]);

            if matches!(mode, Mode::ConfirmRun | Mode::ConfirmQuit) {
                draw_modal(f, f.area(), &mode);
            }
        })?;

        broker_ok = ctx.jobs.broker().health().await.unwrap_or(false);
    }

    if let Some(handle) = api {
        handle.shutdown().await;
    }

    Ok(())
}

fn log_visible_rows(area: Rect) -> usize {
    area.height.saturating_sub(2) as usize
}

fn point_in(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x
        && col < r.x.saturating_add(r.width)
        && row >= r.y
        && row < r.y.saturating_add(r.height)
}

fn list_row_index(area: Rect, row: u16) -> usize {
    row.saturating_sub(area.y.saturating_add(1)) as usize
}

fn move_selection(
    ctx: &AppContext,
    jobs: &[JobRecord],
    focus: Focus,
    delta: i32,
    project_state: &mut ListState,
    target_state: &mut ListState,
    deployment_state: &mut ListState,
    selected_job_id: &mut Option<Uuid>,
    log_scroll: &mut usize,
    log_follow_tail: &mut bool,
) {
    match focus {
        Focus::Projects => {
            let n = ctx.config.projects.len() as i32;
            if n == 0 {
                return;
            }
            let cur = project_state.selected().unwrap_or(0) as i32;
            let next = (cur + delta).rem_euclid(n) as usize;
            project_state.select(Some(next));
            clamp_target_selection(ctx, project_state, target_state);
        }
        Focus::Targets => {
            let n = target_count_for_project(ctx, project_state) as i32;
            if n == 0 {
                return;
            }
            let cur = target_state.selected().unwrap_or(0) as i32;
            let next = (cur + delta).rem_euclid(n) as usize;
            target_state.select(Some(next));
        }
        Focus::Deployments => {
            if jobs.is_empty() {
                return;
            }
            let n = jobs.len() as i32;
            let cur = deployment_state.selected().unwrap_or(0) as i32;
            let next = (cur + delta).rem_euclid(n) as usize;
            deployment_state.select(Some(next));
            sync_selected_job(jobs, deployment_state, selected_job_id);
            *log_follow_tail = true;
        }
        Focus::Log => {
            *log_follow_tail = false;
            if delta > 0 {
                *log_scroll = log_scroll.saturating_add(1);
            } else {
                *log_scroll = log_scroll.saturating_sub(1);
            }
        }
    }
}

fn jump_edge(
    ctx: &AppContext,
    jobs: &[JobRecord],
    focus: Focus,
    to_start: bool,
    project_state: &mut ListState,
    target_state: &mut ListState,
    deployment_state: &mut ListState,
    selected_job_id: &mut Option<Uuid>,
    log_scroll: &mut usize,
    log_follow_tail: &mut bool,
) {
    match focus {
        Focus::Projects => {
            let n = ctx.config.projects.len();
            if n == 0 {
                return;
            }
            project_state.select(Some(if to_start { 0 } else { n - 1 }));
            clamp_target_selection(ctx, project_state, target_state);
        }
        Focus::Targets => {
            let n = target_count_for_project(ctx, project_state);
            if n == 0 {
                return;
            }
            target_state.select(Some(if to_start { 0 } else { n - 1 }));
        }
        Focus::Deployments => {
            if jobs.is_empty() {
                return;
            }
            deployment_state.select(Some(if to_start { 0 } else { jobs.len() - 1 }));
            sync_selected_job(jobs, deployment_state, selected_job_id);
            *log_follow_tail = true;
        }
        Focus::Log => {
            if to_start {
                *log_follow_tail = false;
                *log_scroll = 0;
            } else {
                *log_follow_tail = true;
            }
        }
    }
}

fn handle_mouse(
    m: crossterm::event::MouseEvent,
    ctx: &AppContext,
    jobs: &[JobRecord],
    hit: &LayoutHit,
    focus: &mut Focus,
    project_state: &mut ListState,
    target_state: &mut ListState,
    deployment_state: &mut ListState,
    selected_job_id: &mut Option<Uuid>,
    log_scroll: &mut usize,
    log_follow_tail: &mut bool,
    status_line: &mut String,
) {
    let col = m.column;
    let row = m.row;

    match m.kind {
        MouseEventKind::ScrollDown => {
            if point_in(hit.log, col, row) || *focus == Focus::Log {
                *focus = Focus::Log;
                *log_follow_tail = false;
                *log_scroll = log_scroll.saturating_add(1);
            } else if point_in(hit.deployments, col, row) {
                *focus = Focus::Deployments;
                move_selection(
                    ctx,
                    jobs,
                    Focus::Deployments,
                    1,
                    project_state,
                    target_state,
                    deployment_state,
                    selected_job_id,
                    log_scroll,
                    log_follow_tail,
                );
            } else if point_in(hit.projects, col, row) {
                *focus = Focus::Projects;
                move_selection(
                    ctx,
                    jobs,
                    Focus::Projects,
                    1,
                    project_state,
                    target_state,
                    deployment_state,
                    selected_job_id,
                    log_scroll,
                    log_follow_tail,
                );
            } else if point_in(hit.targets, col, row) {
                *focus = Focus::Targets;
                move_selection(
                    ctx,
                    jobs,
                    Focus::Targets,
                    1,
                    project_state,
                    target_state,
                    deployment_state,
                    selected_job_id,
                    log_scroll,
                    log_follow_tail,
                );
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in(hit.log, col, row) || *focus == Focus::Log {
                *focus = Focus::Log;
                *log_follow_tail = false;
                *log_scroll = log_scroll.saturating_sub(1);
            } else if point_in(hit.deployments, col, row) {
                *focus = Focus::Deployments;
                move_selection(
                    ctx,
                    jobs,
                    Focus::Deployments,
                    -1,
                    project_state,
                    target_state,
                    deployment_state,
                    selected_job_id,
                    log_scroll,
                    log_follow_tail,
                );
            } else if point_in(hit.projects, col, row) {
                *focus = Focus::Projects;
                move_selection(
                    ctx,
                    jobs,
                    Focus::Projects,
                    -1,
                    project_state,
                    target_state,
                    deployment_state,
                    selected_job_id,
                    log_scroll,
                    log_follow_tail,
                );
            } else if point_in(hit.targets, col, row) {
                *focus = Focus::Targets;
                move_selection(
                    ctx,
                    jobs,
                    Focus::Targets,
                    -1,
                    project_state,
                    target_state,
                    deployment_state,
                    selected_job_id,
                    log_scroll,
                    log_follow_tail,
                );
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in(hit.deployments, col, row) {
                *focus = Focus::Deployments;
                if !jobs.is_empty() {
                    let idx = list_row_index(hit.deployments, row);
                    if idx < jobs.len() {
                        deployment_state.select(Some(idx));
                        sync_selected_job(jobs, deployment_state, selected_job_id);
                        *log_follow_tail = true;
                        *status_line = format!(
                            "selected {} / {}",
                            jobs[idx].project_name, jobs[idx].target_label
                        );
                    }
                }
                return;
            }
            if point_in(hit.projects, col, row) {
                *focus = Focus::Projects;
                let idx = list_row_index(hit.projects, row);
                if idx < ctx.config.projects.len() {
                    project_state.select(Some(idx));
                    clamp_target_selection(ctx, project_state, target_state);
                    *status_line = format!("project: {}", ctx.config.projects[idx].name);
                }
                return;
            }
            if point_in(hit.targets, col, row) {
                *focus = Focus::Targets;
                let idx = list_row_index(hit.targets, row);
                let n = target_count_for_project(ctx, project_state);
                if idx < n {
                    target_state.select(Some(idx));
                    if let Some((pid, tid)) = selected_target(ctx, project_state, target_state) {
                        *status_line = format!("target: {pid}/{tid}");
                    }
                }
                return;
            }
            if point_in(hit.log, col, row) {
                *focus = Focus::Log;
                *status_line = "focus: log".into();
            }
        }
        _ => {}
    }
}

fn deployments_panel_height(jobs: &[JobRecord], total_height: u16) -> u16 {
    const HEADER: u16 = 3;
    const SHORTCUTS_H: u16 = 1;
    const FOOTER: u16 = 3;
    const MIN_MAIN: u16 = 4;
    let budget = total_height.saturating_sub(HEADER + SHORTCUTS_H + FOOTER + MIN_MAIN);
    if budget == 0 {
        return 1;
    }
    let rows = jobs.len().saturating_add(1).min(12) as u16;
    let want = if jobs.is_empty() {
        rows.max(2)
    } else {
        rows.max(3)
    };
    want.min(budget).max(1)
}

fn draw_modal(f: &mut ratatui::Frame, area: Rect, mode: &Mode) {
    let popup = centered_rect(60, 20, area);
    let text = match mode {
        Mode::ConfirmRun => "Confirm deploy",
        Mode::ConfirmQuit => "Quit with running jobs?",
        Mode::Browse => return,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(text)
        .style(Style::default().bg(Color::Black));
    f.render_widget(block, popup);
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

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let t = if focused {
        format!("▸ {title}")
    } else {
        format!("  {title}")
    };
    let mut b = Block::default().borders(Borders::ALL).title(t);
    if focused {
        b = b
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
    }
    b
}

fn target_count_for_project(ctx: &AppContext, project_state: &ListState) -> usize {
    project_state
        .selected()
        .and_then(|i| ctx.config.projects.get(i))
        .map(|p| p.targets.len())
        .unwrap_or(0)
}

fn clamp_target_selection(
    ctx: &AppContext,
    project_state: &ListState,
    target_state: &mut ListState,
) {
    let count = target_count_for_project(ctx, project_state);
    if count == 0 {
        target_state.select(None);
        return;
    }
    let idx = target_state.selected().unwrap_or(0);
    if idx >= count {
        target_state.select(Some(count - 1));
    } else if target_state.selected().is_none() {
        target_state.select(Some(0));
    }
}

fn clamp_deployment_selection(
    jobs: &[JobRecord],
    deployment_state: &mut ListState,
    selected_job_id: &mut Option<Uuid>,
) {
    if jobs.is_empty() {
        deployment_state.select(None);
        *selected_job_id = None;
        return;
    }
    let idx = deployment_state.selected().unwrap_or(0);
    if idx >= jobs.len() {
        deployment_state.select(Some(jobs.len() - 1));
    } else if deployment_state.selected().is_none() {
        deployment_state.select(Some(0));
    }
    sync_selected_job(jobs, deployment_state, selected_job_id);
}

fn sync_selected_job(
    jobs: &[JobRecord],
    deployment_state: &ListState,
    selected_job_id: &mut Option<Uuid>,
) {
    if let Some(i) = deployment_state.selected() {
        if let Some(job) = jobs.get(i) {
            *selected_job_id = Some(job.id);
        }
    }
}

fn selected_job<'a>(jobs: &'a [JobRecord], id: &Option<Uuid>) -> Option<&'a JobRecord> {
    if let Some(id) = id {
        if let Some(j) = jobs.iter().find(|j| j.id == *id) {
            return Some(j);
        }
    }
    jobs.first()
}

fn render_deployments(
    f: &mut ratatui::Frame,
    area: Rect,
    jobs: &[JobRecord],
    state: &mut ListState,
    focused: bool,
) {
    let items: Vec<ListItem> = if jobs.is_empty() {
        vec![ListItem::new(Span::styled(
            "(no deployments yet — pick a target and press Enter)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        jobs.iter()
            .map(|j| {
                let phase_style = phase_style(j.phase);
                let glyph = phase_glyph(j.phase);
                let when = job_time_label(j);
                let exit = j
                    .exit_code
                    .map(|c| format!(" exit {c}"))
                    .unwrap_or_default();
                Line::from(vec![
                    Span::styled(format!("{glyph} "), phase_style),
                    Span::styled(
                        format!("{} / {}", j.project_name, j.target_label),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(phase_label(j.phase), phase_style),
                    Span::styled(exit, phase_style),
                    Span::raw("  "),
                    Span::styled(when, Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("  {}", short_id(j.id)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            })
            .map(ListItem::new)
            .collect()
    };

    let list = List::new(items)
        .block(pane_block("deployments", focused))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, area, state);
}

fn render_projects(
    f: &mut ratatui::Frame,
    area: Rect,
    ctx: &AppContext,
    jobs: &[JobRecord],
    state: &mut ListState,
    focused: bool,
) {
    let items: Vec<ListItem> = ctx
        .config
        .projects
        .iter()
        .map(|p| {
            let (glyph, style) = project_badge_styled(&p.id, jobs);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{glyph} "), style),
                Span::raw(&p.name),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(pane_block("projects", focused))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, area, state);
}

fn render_targets(
    f: &mut ratatui::Frame,
    area: Rect,
    ctx: &AppContext,
    jobs: &[JobRecord],
    project_state: &ListState,
    state: &mut ListState,
    focused: bool,
) {
    let mut items = Vec::new();
    if let Some(i) = project_state.selected() {
        if let Some(p) = ctx.config.projects.get(i) {
            for (tid, t) in &p.targets {
                let (glyph, style) = target_badge_styled(&p.id, tid, jobs);
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(format!("{glyph} "), style),
                    Span::raw(format!("{} ({tid})", t.label)),
                ])));
            }
        }
    }
    if items.is_empty() {
        items.push(ListItem::new(Span::styled(
            "(no targets)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    let list = List::new(items)
        .block(pane_block("targets", focused))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, area, state);
}

/// Renders the log panel; returns the clamped scroll offset (and applies tail-follow).
fn render_log(
    f: &mut ratatui::Frame,
    area: Rect,
    job: Option<&JobRecord>,
    mut scroll: usize,
    follow_tail: bool,
    focused: bool,
) -> usize {
    let lines: Vec<Line> = if let Some(job) = job {
        job.log_tail.iter().map(|l| style_log_line(l)).collect()
    } else {
        vec![Line::from(Span::styled(
            "(select a deployment above to view output)",
            Style::default().fg(Color::DarkGray),
        ))]
    };

    let block_title = if focused {
        "output  [/] scroll · g top · G bottom"
    } else {
        "output"
    };
    let inner = Block::default()
        .borders(Borders::ALL)
        .title(block_title)
        .inner(area);
    let visible = inner.height.max(1) as usize;
    let total = lines.len();
    let max_scroll = total.saturating_sub(visible);

    if follow_tail {
        scroll = max_scroll;
    } else {
        scroll = scroll.min(max_scroll);
    }

    // ratatui Paragraph::scroll is (vertical, horizontal) — not (x, y).
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0))
        .block(pane_block(block_title, focused));
    f.render_widget(p, area);

    if total > visible {
        let mut sb = ScrollbarState::new(total).position(scroll);
        f.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .thumb_symbol("█")
                .track_symbol(Some("│")),
            area,
            &mut sb,
        );
    }

    scroll
}

fn style_log_line(line: &str) -> Line<'static> {
    let lower = line.to_lowercase();
    let style = if line.starts_with("[stderr]") {
        Style::default().fg(Color::Yellow)
    } else if lower.contains("orchestrator error")
        || lower.contains("error:")
        || lower.contains(" failed")
        || lower.contains("failure")
    {
        Style::default().fg(Color::Red)
    } else if line.starts_with("finished:") {
        if line.contains("exit 0") {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        }
    } else if line.starts_with("queued:") {
        Style::default().fg(Color::Yellow)
    } else if line.starts_with("running:") {
        Style::default().fg(Color::Cyan)
    } else if lower.contains("warning") {
        Style::default().fg(Color::Yellow)
    } else if lower.contains("success") {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Gray)
    };
    Line::from(Span::styled(line.to_string(), style))
}

fn phase_style(p: JobPhase) -> Style {
    match p {
        JobPhase::Queued => Style::default().fg(Color::Yellow),
        JobPhase::Running => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        JobPhase::Success => Style::default().fg(Color::Green),
        JobPhase::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        JobPhase::Cancelled => Style::default().fg(Color::DarkGray),
    }
}

fn phase_label(p: JobPhase) -> &'static str {
    match p {
        JobPhase::Queued => "queued",
        JobPhase::Running => "running",
        JobPhase::Success => "success",
        JobPhase::Error => "failed",
        JobPhase::Cancelled => "cancelled",
    }
}

fn phase_glyph(p: JobPhase) -> &'static str {
    match p {
        JobPhase::Queued => "◔",
        JobPhase::Running => "●",
        JobPhase::Success => "✓",
        JobPhase::Error => "✗",
        JobPhase::Cancelled => "⊘",
    }
}

fn short_id(id: Uuid) -> String {
    let s = id.to_string();
    s.chars().take(8).collect()
}

fn job_time_label(job: &JobRecord) -> String {
    let t: DateTime<Utc> = job
        .finished_at
        .or(job.started_at)
        .unwrap_or_else(Utc::now);
    t.format("%H:%M:%S").to_string()
}

fn project_badge_styled(project_id: &str, jobs: &[JobRecord]) -> (&'static str, Style) {
    let relevant: Vec<_> = jobs.iter().filter(|j| j.project_id == project_id).collect();
    if relevant.is_empty() {
        return ("○", Style::default().fg(Color::DarkGray));
    }
    let active: Vec<_> = relevant
        .iter()
        .filter(|j| j.phase == JobPhase::Running || j.phase == JobPhase::Queued)
        .collect();
    if !active.is_empty() {
        let mut phase = active[0].phase;
        for j in active.iter().skip(1) {
            phase = worst_priority(phase, j.phase);
        }
        return (phase_glyph(phase), phase_style(phase));
    }
    let latest = relevant[0];
    (phase_glyph(latest.phase), phase_style(latest.phase))
}

fn target_badge_styled(
    project_id: &str,
    target_id: &str,
    jobs: &[JobRecord],
) -> (&'static str, Style) {
    let relevant: Vec<_> = jobs
        .iter()
        .filter(|j| j.project_id == project_id && j.target_id == target_id)
        .collect();
    if relevant.is_empty() {
        return ("○", Style::default().fg(Color::DarkGray));
    }
    if let Some(active) = relevant.iter().find(|j| {
        j.phase == JobPhase::Running || j.phase == JobPhase::Queued
    }) {
        return (phase_glyph(active.phase), phase_style(active.phase));
    }
    let latest = relevant[0];
    (phase_glyph(latest.phase), phase_style(latest.phase))
}

fn worst_priority(a: JobPhase, b: JobPhase) -> JobPhase {
    let score = |p: JobPhase| match p {
        JobPhase::Error => 4,
        JobPhase::Running => 3,
        JobPhase::Queued => 2,
        JobPhase::Success => 1,
        JobPhase::Cancelled => 1,
    };
    if score(b) > score(a) {
        b
    } else {
        a
    }
}

fn selected_target(
    ctx: &AppContext,
    project_state: &ListState,
    target_state: &ListState,
) -> Option<(String, String)> {
    let pi = project_state.selected()?;
    let p = ctx.config.projects.get(pi)?;
    let ti = target_state.selected()?;
    let tid = p.targets.keys().nth(ti)?.clone();
    Some((p.id.clone(), tid))
}

async fn start_deploy(
    ctx: &AppContext,
    project_id: &str,
    target_id: &str,
    status: &mut String,
) -> Result<()> {
    let project = find_project(&ctx.config, project_id)
        .ok_or_else(|| anyhow::anyhow!("project missing"))?;
    let target = project
        .targets
        .get(target_id)
        .ok_or_else(|| anyhow::anyhow!("target missing"))?;
    let id = ctx.jobs.enqueue(project, target).await?;
    *status = format!("started {project_id}/{target_id} ({id})");
    Ok(())
}
