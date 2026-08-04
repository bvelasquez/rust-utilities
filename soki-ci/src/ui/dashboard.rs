use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::commands::AppContext;
use crate::config::find_project;
use crate::jobs::{JobPhase, JobRecord};

enum Mode {
    Browse,
    ConfirmRun,
    ConfirmQuit,
}

pub async fn run_dashboard(mut ctx: AppContext) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut project_state = ListState::default();
    project_state.select(Some(0));
    let mut target_state = ListState::default();
    target_state.select(Some(0));
    let mut mode = Mode::Browse;
    let mut status_line = String::from("↑↓ projects · Tab targets · Enter run · r reload · q quit");
    let mut pending_target: Option<(String, String)> = None;
    let mut broker_ok = ctx.jobs.broker().health().await.unwrap_or(false);

    loop {
        let jobs = ctx.jobs.list_jobs().await;
        let running_count = ctx.jobs.running_count().await;
        let tick = Duration::from_millis(200);
        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
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
                        KeyCode::Tab => {
                            // focus stays visual; j/k on both lists via panel — simplified: one list focus toggle
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            project_state.select_previous();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            project_state.select_next();
                        }
                        KeyCode::Char('J') => {
                            target_state.select_next();
                        }
                        KeyCode::Char('K') => {
                            target_state.select_previous();
                        }
                        KeyCode::Enter => {
                            if let Some((pid, tid)) = selected_target(&ctx, &project_state, &target_state)
                            {
                                if ctx.config.defaults.confirm_deploys {
                                    pending_target = Some((pid, tid));
                                    mode = Mode::ConfirmRun;
                                } else {
                                    start_deploy(&ctx, &pid, &tid, &mut status_line).await?;
                                }
                            }
                        }
                        _ => {}
                    },
                    Mode::ConfirmRun => match key.code {
                        KeyCode::Char('y') | KeyCode::Enter => {
                            if let Some((pid, tid)) = pending_target.take() {
                                start_deploy(&ctx, &pid, &tid, &mut status_line).await?;
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
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(3),
                ])
                .split(f.area());

            let broker_span = if broker_ok {
                Span::styled("broker ●", Style::default().fg(Color::Green))
            } else {
                Span::styled("broker ○", Style::default().fg(Color::Yellow))
            };
            let header = Paragraph::new(Line::from(vec![
                Span::raw("soki-ci  "),
                broker_span,
                Span::raw(format!("  {}", ctx.config_path.display())),
            ]))
            .block(Block::default().borders(Borders::ALL).title("deploy"));
            f.render_widget(header, chunks[0]);

            let mid = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(50),
                ])
                .split(chunks[1]);

            render_projects(f, mid[0], &ctx, &jobs, &mut project_state);
            render_targets(f, mid[1], &ctx, &jobs, &project_state, &mut target_state);
            render_log(f, mid[2], &jobs, &project_state, &ctx);

            let footer = match mode {
                Mode::Browse => status_line.clone(),
                Mode::ConfirmRun => "Run this deploy? [y] yes  [n] cancel".to_string(),
                Mode::ConfirmQuit => format!(
                    "{running_count} job(s) still running — quit anyway? Deploys keep running. [y] quit  [n] stay"
                ),
            };
            let foot = Paragraph::new(footer).block(Block::default().borders(Borders::ALL));
            f.render_widget(foot, chunks[2]);

            if matches!(mode, Mode::ConfirmRun | Mode::ConfirmQuit) {
                draw_modal(f, f.area(), &mode);
            }
        })?;

        broker_ok = ctx.jobs.broker().health().await.unwrap_or(false);
    }

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    Ok(())
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

fn render_projects(
    f: &mut ratatui::Frame,
    area: Rect,
    ctx: &AppContext,
    jobs: &[JobRecord],
    state: &mut ListState,
) {
    let items: Vec<ListItem> = ctx
        .config
        .projects
        .iter()
        .map(|p| {
            let badge = project_badge(&p.id, jobs);
            ListItem::new(format!("{badge} {}", p.name))
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("projects"))
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
) {
    let mut items = Vec::new();
    if let Some(i) = project_state.selected() {
        if let Some(p) = ctx.config.projects.get(i) {
            for (tid, t) in &p.targets {
                let badge = target_badge(&p.id, tid, jobs);
                items.push(ListItem::new(format!("{badge} {} ({tid})", t.label)));
            }
        }
    }
    if items.is_empty() {
        items.push(ListItem::new("(no targets)"));
    }
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("targets"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, area, state);
}

fn render_log(
    f: &mut ratatui::Frame,
    area: Rect,
    jobs: &[JobRecord],
    project_state: &ListState,
    ctx: &AppContext,
) {
    let text = if let Some(job) = latest_job_for_project(jobs, project_state, ctx) {
        job.log_tail.iter().cloned().collect::<Vec<_>>().join("\n")
    } else {
        String::from("(no jobs yet)")
    };
    let p = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("log"));
    f.render_widget(p, area);
}

fn latest_job_for_project<'a>(
    jobs: &'a [JobRecord],
    project_state: &ListState,
    ctx: &AppContext,
) -> Option<&'a JobRecord> {
    let pid = project_state.selected().and_then(|i| ctx.config.projects.get(i))?;
    jobs.iter().find(|j| j.project_id == pid.id)
}

fn project_badge(project_id: &str, jobs: &[JobRecord]) -> &'static str {
    let relevant: Vec<_> = jobs.iter().filter(|j| j.project_id == project_id).collect();
    if relevant.is_empty() {
        return "○";
    }
    let mut worst = relevant[0].phase;
    for j in relevant.iter().skip(1) {
        worst = worst_priority(worst, j.phase);
    }
    phase_glyph(worst)
}

fn target_badge(project_id: &str, target_id: &str, jobs: &[JobRecord]) -> &'static str {
    for j in jobs.iter().filter(|j| j.project_id == project_id && j.target_id == target_id) {
        return phase_glyph(j.phase);
    }
    "○"
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

fn phase_glyph(p: JobPhase) -> &'static str {
    match p {
        JobPhase::Queued => "◔",
        JobPhase::Running => "●",
        JobPhase::Success => "✓",
        JobPhase::Error => "✗",
        JobPhase::Cancelled => "⊘",
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
