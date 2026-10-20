use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::catalog::Catalog;
use crate::organize::{self, ApplyReport, MovePlan, default_rules_path};

use super::progress::centered_rect;
use super::theme::{
    draw_modal_panel, fill_rects, footer_block, highlight_style, modal_surface_style, panel_block,
};

enum Screen {
    Review,
    Confirm,
}

pub fn run_organize_tui<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let rules_path = default_rules_path();
    let loaded = organize::load_rules(&rules_path);
    let rules = match loaded {
        Ok(rules) => rules,
        Err(err) => {
            message_loop(
                terminal,
                &format!("Cannot load {}:\n{err}", rules_path.display()),
            )?;
            return Ok(());
        }
    };
    let catalog = match Catalog::open_default() {
        Ok(catalog) => catalog,
        Err(err) => {
            message_loop(terminal, &format!("Cannot open the organize catalog:\n{err}"))?;
            return Ok(());
        }
    };
    let mut indexed = catalog
        .index_slice(&rules, Duration::from_millis(1))?
        .done;
    let mut plan = if indexed {
        organize::plan_with_catalog(&rules, &catalog)?
    } else {
        organize::plan_sources(&rules)?
    };
    let mut state = ListState::default();
    state.select(Some(0));
    let mut screen = Screen::Review;
    let mut confirm = String::new();
    let mut auto = false;
    let mut last_sweep = Instant::now();
    let mut status = if indexed {
        status_line(&rules_path, &plan)
    } else {
        "Indexing the library. Files stay put until that finishes.".to_string()
    };

    loop {
        if !indexed {
            let progress = catalog.index_slice(&rules, Duration::from_millis(40))?;
            if progress.done {
                indexed = true;
                plan = organize::plan_with_catalog(&rules, &catalog)?;
                state.select(Some(0));
                status = status_line(&rules_path, &plan);
            } else {
                status = format!(
                    "Indexing {} — checked {}, hashed {}. q back",
                    progress.dest, progress.checked, progress.hashed
                );
            }
        } else if auto && last_sweep.elapsed() >= Duration::from_secs(1) {
            match organize::organize_pending(&rules) {
                Ok(report) => {
                    if report.moved > 0 || report.skipped > 0 {
                        status = format!("auto: {}", summarize(&report));
                    }
                    if let Ok(fresh) = organize::plan_with_catalog(&rules, &catalog) {
                        plan = fresh;
                    }
                }
                Err(err) => status = format!("auto: {err}"),
            }
            last_sweep = Instant::now();
        }
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(4)])
                .split(area);
            let items: Vec<ListItem> = plan
                .moves
                .iter()
                .map(|item| {
                    let name = item
                        .source
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| item.source.display().to_string());
                    ListItem::new(format!(
                        "{}  {}  ->  {}",
                        item.rule,
                        name,
                        item.dest.display()
                    ))
                })
                .collect();
            let title = if !indexed {
                format!(
                    "Organize — indexing library ({} in drop)",
                    plan.moves.len()
                )
            } else if auto {
                format!("Organize — auto ({} waiting)", plan.moves.len())
            } else if plan.moves.is_empty() {
                "Organize — drop folder is empty".to_string()
            } else {
                format!("Organize — {} files ready to move", plan.moves.len())
            };
            frame.render_stateful_widget(
                List::new(items)
                    .block(panel_block(&title))
                    .highlight_style(highlight_style())
                    .highlight_symbol("› "),
                chunks[0],
                &mut state,
            );
            let keys = if !indexed {
                "Indexing the library so duplicate files are recognized. q back"
            } else if auto {
                "Auto is on. New files in drop are moved as soon as they finish copying. A stops auto · q back"
            } else {
                "j/k move · r refresh · a move these now · A auto · w review file · q back"
            };
            frame.render_widget(
                Paragraph::new(format!("{status}\n{keys}"))
                .block(footer_block(""))
                .wrap(Wrap { trim: true }),
                chunks[1],
            );
            if matches!(screen, Screen::Confirm) {
                let popup = centered_rect(72, 40, area);
                let inner = draw_modal_panel(frame, popup, "Organize these files", Color::Yellow);
                fill_rects(frame, &[inner], modal_surface_style());
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::from(format!(
                            "Move {} files using {}",
                            plan.moves.len(),
                            rules_path.display()
                        )),
                        Line::from("This follows the rules now. A review file is not required."),
                        Line::from("Type apply and press Enter."),
                        Line::from(Span::raw(format!("> {confirm}"))),
                    ])
                    .style(modal_surface_style())
                    .wrap(Wrap { trim: true }),
                    inner,
                );
            }
        })?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match screen {
            Screen::Review => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => {
                    move_list(&mut state, plan.moves.len(), 1);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    move_list(&mut state, plan.moves.len(), -1);
                }
                KeyCode::Char('r') => {
                    if !indexed {
                        status = "Still indexing the library.".into();
                    } else {
                        plan = organize::plan_with_catalog(&rules, &catalog)?;
                        state.select(Some(0));
                        status = status_line(&rules_path, &plan);
                    }
                }
                KeyCode::Char('A') => {
                    auto = !auto;
                    if auto {
                        last_sweep = Instant::now() - Duration::from_secs(2);
                        status = "Auto on. Files in the drop folder move as soon as they settle.".into();
                    } else {
                        status = "Auto off. Press A to watch the drop folder again.".into();
                    }
                }
                KeyCode::Char('w') => {
                    let plan_path = std::env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .join("disk-sweep-organize-plan.yaml");
                    organize::write_plan(&plan_path, &plan)?;
                    status = format!(
                        "Wrote {} moves to {}. Nothing was moved.",
                        plan.moves.len(),
                        plan_path.display()
                    );
                }
                KeyCode::Char('a') => {
                    if !indexed {
                        status = "Still indexing the library. Moves start after that.".into();
                    } else if plan.moves.is_empty() {
                        status = "Nothing in the drop folder matches the rules.".into();
                    } else {
                        confirm.clear();
                        screen = Screen::Confirm;
                    }
                }
                _ => {}
            },
            Screen::Confirm => match key.code {
                KeyCode::Esc => screen = Screen::Review,
                KeyCode::Backspace => {
                    confirm.pop();
                }
                KeyCode::Char(ch) => confirm.push(ch),
                KeyCode::Enter => {
                    if confirm.trim() != "apply" {
                        status = "Confirmation must be the word apply".into();
                        screen = Screen::Review;
                        continue;
                    }
                    let report = organize::apply_recorded(&plan, &catalog)?;
                    plan = organize::plan_with_catalog(&rules, &catalog)?;
                    state.select(Some(0));
                    status = format!(
                        "{}. {} still waiting.",
                        summarize(&report),
                        plan.moves.len()
                    );
                    screen = Screen::Review;
                }
                _ => {}
            },
        }
    }
}

fn status_line(rules_path: &std::path::Path, plan: &MovePlan) -> String {
    format!(
        "{} files from the rules in {}. a moves them now.",
        plan.moves.len(),
        rules_path.display()
    )
}

fn summarize(report: &ApplyReport) -> String {
    format!("Moved {} files, skipped {}.", report.moved, report.skipped)
}

fn move_list(state: &mut ListState, len: usize, delta: i32) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0) as i32;
    state.select(Some((current + delta).clamp(0, len as i32 - 1) as usize));
}

fn message_loop<B: Backend>(terminal: &mut Terminal<B>, message: &str) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nPress q to go back"))
                    .block(super::theme::panel_block("Organize"))
                    .wrap(Wrap { trim: true }),
                area,
            );
        })?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press
                    && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter)
                {
                    return Ok(());
                }
            }
        }
    }
}
