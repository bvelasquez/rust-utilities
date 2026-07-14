use std::io;
use std::time::{Duration, Instant};

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
use ratatui::widgets::{Block, Paragraph, Tabs, Wrap};
use ratatui::Terminal;

use crate::aggregate::{build_summary, Period, SummaryData};
use crate::commands::fetch::{self, FetchMode};
use crate::commands::AppContext;
use crate::store::Store;

use super::cost_chart::render_cost_chart;
use super::gauges::budget_gauge_over;
use super::theme::{self, footer_block, key_style};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview = 0,
    Providers = 1,
    Models = 2,
    Budgets = 3,
}

impl Tab {
    fn all() -> [Tab; 4] {
        [Tab::Overview, Tab::Providers, Tab::Models, Tab::Budgets]
    }

    fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Providers => "By Provider",
            Tab::Models => "By Model",
            Tab::Budgets => "Budgets",
        }
    }

    fn next(self) -> Self {
        match self {
            Tab::Overview => Tab::Providers,
            Tab::Providers => Tab::Models,
            Tab::Models => Tab::Budgets,
            Tab::Budgets => Tab::Overview,
        }
    }
}

pub async fn run(ctx: &mut AppContext, mut period: Period) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut tab = Tab::Overview;
    let mut summary = load_summary(ctx, period)?;
    let mut last_refresh = Instant::now();
    let mut status_msg = "loaded from cache".to_string();

    loop {
        terminal.draw(|f| {
            draw_ui(f, f.area(), &summary, tab, period, &status_msg);
        })?;

        let _ = ctx.reload_config();
        let refresh_interval = ctx.config.tui.refresh_interval();
        let timeout = refresh_interval
            .map(|interval| interval.saturating_sub(last_refresh.elapsed()))
            .unwrap_or(Duration::MAX);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => tab = tab.next(),
                        KeyCode::Char('1') => tab = Tab::Overview,
                        KeyCode::Char('2') => tab = Tab::Providers,
                        KeyCode::Char('3') => tab = Tab::Models,
                        KeyCode::Char('4') => tab = Tab::Budgets,
                        KeyCode::Char('d') => {
                            period = Period::Day;
                            summary = load_summary(ctx, period)?;
                        }
                        KeyCode::Char('w') => {
                            period = Period::Week;
                            summary = load_summary(ctx, period)?;
                        }
                        KeyCode::Char('m') => {
                            period = Period::Month;
                            summary = load_summary(ctx, period)?;
                        }
                        KeyCode::Char('r') => {
                            status_msg = "fetching...".into();
                            terminal.draw(|f| {
                                draw_ui(f, f.area(), &summary, tab, period, &status_msg);
                            })?;
                            let _ = ctx.reload_config();
                            match fetch::run(ctx, 90, FetchMode::Quiet).await {
                                Ok(outcome) => {
                                    summary = load_summary(ctx, period)?;
                                    status_msg = outcome.tui_status();
                                    last_refresh = Instant::now();
                                }
                                Err(e) => status_msg = format!("fetch failed: {e:#}"),
                            }
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                        _ => {}
                    }
                }
            }
        } else if refresh_interval.is_some_and(|interval| last_refresh.elapsed() >= interval) {
            status_msg = "auto-refreshing...".into();
            terminal.draw(|f| {
                draw_ui(f, f.area(), &summary, tab, period, &status_msg);
            })?;
            match fetch::run(ctx, 90, FetchMode::Quiet).await {
                Ok(outcome) => {
                    summary = load_summary(ctx, period)?;
                    status_msg = format!("auto · {}", outcome.tui_status());
                    last_refresh = Instant::now();
                }
                Err(e) => status_msg = format!("auto-refresh failed: {e:#}"),
            }
        }
    }

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn load_summary(ctx: &AppContext, period: Period) -> Result<SummaryData> {
    let store = Store::open(&ctx.cache_path)?;
    let rows = store.daily_rows()?;
    Ok(build_summary(&rows, &ctx.config, period))
}

fn draw_ui(
    f: &mut ratatui::Frame,
    area: Rect,
    summary: &SummaryData,
    tab: Tab,
    period: Period,
    status_msg: &str,
) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let header = header_line(summary, period);
    f.render_widget(
        Paragraph::new(header).wrap(Wrap { trim: true }).block(theme::chrome_block("Model Use")),
        outer[0],
    );

    let titles: Vec<Line> = Tab::all()
        .iter()
        .enumerate()
        .map(|(i, t)| {
            Line::from(vec![
                Span::styled(format!(" {} ", i + 1), Style::default().fg(theme::MUTED)),
                Span::raw(t.title()),
            ])
        })
        .collect();
    let tabs = Tabs::new(titles)
        .style(Style::default().fg(theme::MUTED))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│")
        .select(tab as usize)
        .padding(" ", " ");
    f.render_widget(tabs.block(footer_block()), outer[1]);

    match tab {
        Tab::Overview => render_overview(f, outer[2], summary, period),
        Tab::Providers => render_providers(f, outer[2], summary),
        Tab::Models => render_models(f, outer[2], summary),
        Tab::Budgets => render_budgets(f, outer[2], summary),
    }

    let footer = Line::from(vec![
        Span::styled(" Tab ", key_style()),
        Span::styled("/1-4  ", theme::label_style()),
        Span::styled("d/w/m ", key_style()),
        Span::styled("period  ", theme::label_style()),
        Span::styled("r ", key_style()),
        Span::styled("fetch  ", theme::label_style()),
        Span::styled("q ", key_style()),
        Span::styled("quit  ", theme::label_style()),
        Span::styled("│ ", theme::label_style()),
        Span::styled(status_msg, theme::label_style()),
    ]);
    f.render_widget(
        Paragraph::new(footer).block(footer_block()),
        outer[3],
    );
}

fn header_line(summary: &SummaryData, period: Period) -> Line<'static> {
    let global = summary
        .budgets
        .iter()
        .find(|b| b.label == "global");
    let budget_str = global
        .and_then(|b| b.budget_usd)
        .map(|v| format!(" / ${v:.0} budget"))
        .unwrap_or_default();
    let over = global
        .map(|b| b.over_budget)
        .unwrap_or(false);
    let mtd_style = if over {
        Style::default().fg(theme::LOSS).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::PROFIT)
    };

    Line::from(vec![
        Span::styled(
            format!("MTD ${:.2}", summary.mtd_usd),
            mtd_style,
        ),
        Span::raw(budget_str),
        Span::raw(format!(" · {} view ${:.2}", period.label(), summary.total_usd)),
    ])
}

fn render_overview(f: &mut ratatui::Frame, area: Rect, summary: &SummaryData, period: Period) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(5)])
        .split(area);

    render_cost_chart(f, chunks[0], summary, period);

    if let Some(global) = summary.budgets.iter().find(|b| b.label == "global") {
        f.render_widget(
            budget_gauge_over(global).block(theme::panel_block("Global budget")),
            chunks[1],
        );
    }
}

fn render_providers(f: &mut ratatui::Frame, area: Rect, summary: &SummaryData) {
    let lines: Vec<Line> = summary
        .by_provider
        .iter()
        .map(|(p, cost)| {
            let budget = summary
                .budgets
                .iter()
                .find(|b| b.label == p.to_string());
            let style = budget
                .map(|b| if b.over_budget {
                    Style::default().fg(theme::LOSS)
                } else {
                    Style::default().fg(Color::White)
                })
                .unwrap_or(Style::default());
            Line::from(Span::styled(format!("  {p}: ${cost:.2}"), style))
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(theme::panel_block("Spend by provider")),
        area,
    );
}

fn render_models(f: &mut ratatui::Frame, area: Rect, summary: &SummaryData) {
    let lines: Vec<Line> = summary
        .top_models
        .iter()
        .map(|m| {
            Line::from(format!(
                "  {} / {}  ${:.2}",
                m.provider, m.model, m.cost_usd
            ))
        })
        .collect();
    f.render_widget(
        Paragraph::new(if lines.is_empty() {
            vec![Line::from("  (no model breakdown)")]
        } else {
            lines
        })
        .block(theme::panel_block("Top models")),
        area,
    );
}

fn render_budgets(f: &mut ratatui::Frame, area: Rect, summary: &SummaryData) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(3); summary.budgets.len().max(1)])
        .split(area);

    for (i, b) in summary.budgets.iter().enumerate() {
        if i < chunks.len() {
            f.render_widget(
                budget_gauge_over(b).block(Block::default().borders(ratatui::widgets::Borders::NONE)),
                chunks[i],
            );
        }
    }
}
