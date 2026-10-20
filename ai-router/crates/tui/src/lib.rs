//! Dashboard for a running airouter process. It talks to the loopback admin API.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use airouter_core::{DashboardSnapshot, RequestRecord};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{BarChart, Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::{Frame, Terminal};

const TABS: [&str; 6] = [
    "Dashboard",
    "Live log",
    "Providers",
    "Models",
    "Usage",
    "Config",
];

struct KeyPrompt {
    provider: String,
    secret: String,
}

struct ModelPicker {
    provider: String,
    models: Vec<String>,
    selected: usize,
    filter: String,
    filtering: bool,
}

impl ModelPicker {
    fn visible(&self) -> Vec<String> {
        let query = self.filter.to_ascii_lowercase();
        self.models
            .iter()
            .filter(|id| query.is_empty() || id.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect()
    }
}

struct Model {
    tab: usize,
    selected: usize,
    paused: bool,
    filter: String,
    filtering: bool,
    status: String,
    frozen: Vec<RequestRecord>,
    key_prompt: Option<KeyPrompt>,
    model_picker: Option<ModelPicker>,
}

pub fn run(admin_base: &str) -> Result<(), String> {
    let base = admin_base.trim_end_matches('/').to_string();
    let snapshot: Arc<Mutex<Option<DashboardSnapshot>>> = Arc::new(Mutex::new(None));
    let live: Arc<Mutex<VecDeque<RequestRecord>>> = Arc::new(Mutex::new(VecDeque::new()));
    let connected = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    spawn_pollers(
        base.clone(),
        snapshot.clone(),
        live.clone(),
        connected.clone(),
        paused.clone(),
    );

    let mut stdout = std::io::stdout();
    enable_raw_mode().map_err(|e| e.to_string())?;
    stdout
        .execute(EnterAlternateScreen)
        .map_err(|e| e.to_string())?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    let mut ui = Model {
        tab: 0,
        selected: 0,
        paused: false,
        filter: String::new(),
        filtering: false,
        status: "connecting".into(),
        frozen: Vec::new(),
        key_prompt: None,
        model_picker: None,
    };
    let result = loop {
        let snap = snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if snap.is_some() && ui.status == "connecting" {
            ui.status = "ready".into();
        }
        let logs = merged_logs(&live, snap.as_ref(), &ui);
        if let Err(err) = terminal.draw(|frame| {
            draw(
                frame,
                &ui,
                snap.as_ref(),
                &logs,
                connected.load(Ordering::Relaxed),
                &base,
            )
        }) {
            break Err(err.to_string());
        }
        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            let ev = event::read().map_err(|e| e.to_string())?;
            if ui.key_prompt.is_some() {
                match ev {
                    Event::Paste(text) => {
                        if let Some(prompt) = ui.key_prompt.as_mut() {
                            prompt.secret.push_str(&text);
                        }
                    }
                    Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Esc => {
                            ui.key_prompt = None;
                            ui.status = "key entry cancelled".into();
                        }
                        KeyCode::Enter => {
                            let prompt = ui.key_prompt.take().expect("prompt");
                            ui.status = save_provider_key(&base, &prompt.provider, &prompt.secret);
                        }
                        KeyCode::Backspace => {
                            if let Some(prompt) = ui.key_prompt.as_mut() {
                                prompt.secret.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(prompt) = ui.key_prompt.as_mut() {
                                prompt.secret.push(c);
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                continue;
            }
            if ui.model_picker.is_some() {
                if let Event::Key(key) = ev {
                    if key.kind == KeyEventKind::Press {
                        handle_model_picker(&mut ui, &base, key.code);
                    }
                }
                continue;
            }
            if let Event::Key(key) = ev {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if ui.filtering {
                    match key.code {
                        KeyCode::Esc => {
                            ui.filtering = false;
                            ui.filter.clear();
                        }
                        KeyCode::Enter => ui.filtering = false,
                        KeyCode::Backspace => {
                            ui.filter.pop();
                        }
                        KeyCode::Char(c) => ui.filter.push(c),
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break Ok(())
                    }
                    KeyCode::Tab => ui.tab = (ui.tab + 1) % TABS.len(),
                    KeyCode::BackTab => ui.tab = (ui.tab + TABS.len() - 1) % TABS.len(),
                    KeyCode::Char(c @ '1'..='6') => ui.tab = (c as usize) - ('1' as usize),
                    KeyCode::Char('j') | KeyCode::Down => {
                        ui.selected = ui.selected.saturating_add(1)
                    }
                    KeyCode::Char('k') | KeyCode::Up => ui.selected = ui.selected.saturating_sub(1),
                    KeyCode::Char('/') => ui.filtering = true,
                    KeyCode::Char('p') if ui.tab == 1 => {
                        ui.paused = !ui.paused;
                        paused.store(ui.paused, Ordering::Relaxed);
                        if ui.paused {
                            ui.frozen = logs.clone();
                            ui.status = "log paused".into();
                        } else {
                            ui.status = "log resumed".into();
                        }
                    }
                    KeyCode::Char('r') => {
                        ui.status = post_simple(&base, "/reload", &serde_json::json!({}))
                    }
                    KeyCode::Char('a')
                        if ui.tab == 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        if let Some(name) = provider_at(snap.as_ref(), ui.selected) {
                            ui.key_prompt = Some(KeyPrompt {
                                provider: name.clone(),
                                secret: String::new(),
                            });
                            ui.status = format!("enter API key for {name}");
                        } else {
                            ui.status = "no provider selected".into();
                        }
                    }
                    KeyCode::Char('l') if ui.tab == 2 => {
                        if let Some(name) = provider_at(snap.as_ref(), ui.selected) {
                            ui.status = format!("loading models from {name}");
                            let _ = terminal.draw(|frame| {
                                draw(
                                    frame,
                                    &ui,
                                    snap.as_ref(),
                                    &logs,
                                    connected.load(Ordering::Relaxed),
                                    &base,
                                )
                            });
                            match post_json(
                                &base,
                                "/providers/models",
                                &serde_json::json!({"name": name}),
                            ) {
                                Ok(body) => {
                                    let models = body
                                        .get("models")
                                        .and_then(|v| v.as_array())
                                        .map(|rows| {
                                            rows.iter()
                                                .filter_map(|row| row.as_str().map(str::to_string))
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default();
                                    if models.is_empty() {
                                        ui.status = format!("{name} returned no models");
                                    } else {
                                        let count = models.len();
                                        ui.model_picker = Some(ModelPicker {
                                            provider: name,
                                            models,
                                            selected: 0,
                                            filter: String::new(),
                                            filtering: false,
                                        });
                                        ui.status =
                                            format!("{count} models, enter adds, esc closes");
                                    }
                                }
                                Err(err) => ui.status = err,
                            }
                        } else {
                            ui.status = "no provider selected".into();
                        }
                    }
                    KeyCode::Char(' ') if ui.tab == 2 => {
                        if let Some(name) = snap
                            .as_ref()
                            .and_then(|s| s.providers.get(ui.selected).map(|p| p.name.clone()))
                        {
                            ui.status = post_simple(
                                &base,
                                "/providers/toggle",
                                &serde_json::json!({"name": name}),
                            );
                        }
                    }
                    KeyCode::Char(' ') if ui.tab == 3 => {
                        if let Some(id) = snap
                            .as_ref()
                            .and_then(|s| s.models.get(ui.selected).map(|m| m.id.clone()))
                        {
                            ui.status = post_simple(
                                &base,
                                "/models/toggle",
                                &serde_json::json!({"id": id}),
                            );
                        }
                    }
                    KeyCode::Char('e') if ui.tab == 5 => {
                        if let Some(path) = snap.as_ref().map(|s| s.config_path.clone()) {
                            suspend_and_edit(&mut terminal, &path)?;
                            ui.status = post_simple(&base, "/reload", &serde_json::json!({}));
                        }
                    }
                    _ => {}
                }
            }
        }
    };
    disable_raw_mode().ok();
    let _ = std::io::stdout().execute(LeaveAlternateScreen);
    result
}

fn spawn_pollers(
    base: String,
    snapshot: Arc<Mutex<Option<DashboardSnapshot>>>,
    live: Arc<Mutex<VecDeque<RequestRecord>>>,
    connected: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .expect("http client");
    let snap_client = client.clone();
    let snap_base = base.clone();
    let snap_flag = connected.clone();
    thread::spawn(move || loop {
        match snap_client.get(format!("{snap_base}/snapshot")).send() {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.json::<DashboardSnapshot>() {
                    *snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(body);
                    snap_flag.store(true, Ordering::Relaxed);
                }
            }
            _ => snap_flag.store(false, Ordering::Relaxed),
        }
        thread::sleep(Duration::from_millis(800));
    });
    thread::spawn(move || loop {
        if read_events(&client, &base, &live, &paused).is_err() {
            connected.store(false, Ordering::Relaxed);
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn read_events(
    client: &reqwest::blocking::Client,
    base: &str,
    live: &Mutex<VecDeque<RequestRecord>>,
    paused: &AtomicBool,
) -> Result<(), ()> {
    let resp = client
        .get(format!("{base}/events"))
        .send()
        .map_err(|_| ())?;
    if !resp.status().is_success() {
        return Err(());
    }
    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        if paused.load(Ordering::Relaxed) {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<RequestRecord>(data.trim()) {
            let mut logs = live.lock().unwrap_or_else(|e| e.into_inner());
            logs.retain(|existing| existing.id != rec.id);
            logs.push_front(rec);
            while logs.len() > 500 {
                logs.pop_back();
            }
        }
    }
}

fn merged_logs(
    live: &Mutex<VecDeque<RequestRecord>>,
    snap: Option<&DashboardSnapshot>,
    ui: &Model,
) -> Vec<RequestRecord> {
    if ui.paused {
        return filter_logs(ui.frozen.clone(), &ui.filter);
    }
    let mut out: Vec<RequestRecord> = live
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    if let Some(snap) = snap {
        for rec in &snap.recent {
            if !out.iter().any(|existing| existing.id == rec.id) {
                out.push(rec.clone());
            }
        }
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts).then(b.id.cmp(&a.id)));
    filter_logs(out, &ui.filter)
}

fn filter_logs(logs: Vec<RequestRecord>, filter: &str) -> Vec<RequestRecord> {
    if filter.is_empty() {
        return logs;
    }
    let q = filter.to_ascii_lowercase();
    logs.into_iter()
        .filter(|r| {
            r.requested_model.to_ascii_lowercase().contains(&q)
                || r.provider.to_ascii_lowercase().contains(&q)
                || r.client_label.to_ascii_lowercase().contains(&q)
                || r.status.to_string().contains(&q)
                || r.error
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains(&q)
        })
        .collect()
}

fn handle_model_picker(ui: &mut Model, base: &str, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            let filtering = ui
                .model_picker
                .as_ref()
                .is_some_and(|picker| picker.filtering);
            if filtering {
                if let Some(picker) = ui.model_picker.as_mut() {
                    picker.filtering = false;
                    picker.filter.clear();
                    picker.selected = 0;
                }
            } else {
                ui.model_picker = None;
                ui.status = "ready".into();
            }
        }
        KeyCode::Enter => {
            let choice = ui.model_picker.as_ref().and_then(|picker| {
                picker
                    .visible()
                    .get(picker.selected)
                    .cloned()
                    .map(|upstream| (picker.provider.clone(), upstream))
            });
            ui.status = match choice {
                Some((provider, upstream)) => add_upstream_model(base, &provider, &upstream),
                None => "no models match".into(),
            };
        }
        KeyCode::Char('/') => {
            if let Some(picker) = ui.model_picker.as_mut() {
                picker.filtering = true;
            }
        }
        KeyCode::Char(c) => {
            let filtering = ui
                .model_picker
                .as_ref()
                .is_some_and(|picker| picker.filtering);
            if filtering {
                if let Some(picker) = ui.model_picker.as_mut() {
                    picker.filter.push(c);
                    picker.selected = 0;
                }
            } else if c == 'j' {
                move_picker(ui, 1);
            } else if c == 'k' {
                move_picker(ui, -1);
            }
        }
        KeyCode::Backspace => {
            if let Some(picker) = ui.model_picker.as_mut() {
                if picker.filtering {
                    picker.filter.pop();
                    picker.selected = 0;
                }
            }
        }
        KeyCode::Down => move_picker(ui, 1),
        KeyCode::Up => move_picker(ui, -1),
        _ => {}
    }
}

fn move_picker(ui: &mut Model, delta: i32) {
    let Some(picker) = ui.model_picker.as_mut() else {
        return;
    };
    let len = picker.visible().len();
    if len == 0 {
        picker.selected = 0;
        return;
    }
    let next = picker.selected as i32 + delta;
    picker.selected = next.clamp(0, len as i32 - 1) as usize;
}

fn add_upstream_model(base: &str, provider: &str, upstream: &str) -> String {
    match post_json(
        base,
        "/models/add",
        &serde_json::json!({"provider": provider, "upstream": upstream}),
    ) {
        Ok(body) => body
            .get("id")
            .and_then(|id| id.as_str())
            .map(|id| format!("added {id}"))
            .unwrap_or_else(|| format!("added {provider}/{upstream}")),
        Err(err) => err,
    }
}

fn post_json(
    base: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{base}{path}"))
        .json(body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{path} -> {status} {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("{path} returned {e}"))
}

fn provider_at(snap: Option<&DashboardSnapshot>, selected: usize) -> Option<String> {
    let providers = snap?.providers.as_slice();
    if providers.is_empty() {
        return None;
    }
    let idx = selected.min(providers.len() - 1);
    Some(providers[idx].name.clone())
}

fn save_provider_key(base: &str, provider: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return "empty key ignored".into();
    }
    let status = post_simple(
        base,
        "/providers/key",
        &serde_json::json!({"name": provider, "key": secret}),
    );
    if status.ends_with(" ok") {
        format!("saved API key for {provider}")
    } else {
        status.replace(secret, "••••")
    }
}

fn post_simple(base: &str, path: &str, body: &serde_json::Value) -> String {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build();
    let Ok(client) = client else {
        return "http client failed".into();
    };
    match client.post(format!("{base}{path}")).json(body).send() {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            if status.is_success() {
                format!("{path} ok")
            } else {
                format!("{path} -> {status} {text}")
            }
        }
        Err(err) => format!("{path} failed: {err}"),
    }
}

fn suspend_and_edit<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    path: &str,
) -> Result<(), String> {
    disable_raw_mode().ok();
    let _ = std::io::stdout().execute(LeaveAlternateScreen);
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".into());
    let status = std::process::Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    std::io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| e.to_string())?;
    terminal.clear().ok();
    if status.success() {
        Ok(())
    } else {
        Err(format!("{editor} exited with {status}"))
    }
}

fn draw(
    frame: &mut Frame,
    ui: &Model,
    snap: Option<&DashboardSnapshot>,
    logs: &[RequestRecord],
    connected: bool,
    base: &str,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let titles: Vec<Line> = TABS.iter().map(|t| Line::from(*t)).collect();
    let tabs = Tabs::new(titles)
        .select(ui.tab)
        .block(Block::default().borders(Borders::ALL).title(" airouter "))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[0]);
    match ui.tab {
        0 => draw_dashboard(frame, chunks[1], snap, connected, base),
        1 => draw_logs(frame, chunks[1], logs, ui),
        2 => {
            if let Some(picker) = ui.model_picker.as_ref() {
                draw_model_picker(frame, chunks[1], picker);
            } else {
                draw_providers(frame, chunks[1], snap, ui.selected, ui.key_prompt.as_ref());
            }
        }
        3 => draw_models(frame, chunks[1], snap, ui.selected),
        4 => draw_usage(frame, chunks[1], snap),
        _ => draw_config(frame, chunks[1], snap),
    }
    let filter = if ui.filtering || !ui.filter.is_empty() {
        format!(" filter: {}_", ui.filter)
    } else {
        String::new()
    };
    let paused = if ui.paused { " paused" } else { "" };
    let listen = snap.map(|s| s.listen.as_str()).unwrap_or("starting");
    let line = format!(
        "listen {listen}  {}{paused}{filter}  |  tab 1-6  j/k  space  a key  l models  p pause  r reload  e edit  / filter  q quit",
        ui.status
    );
    frame.render_widget(Paragraph::new(line), chunks[2]);
}

fn draw_dashboard(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    snap: Option<&DashboardSnapshot>,
    connected: bool,
    base: &str,
) {
    let Some(snap) = snap else {
        let msg = if connected {
            "waiting for snapshot"
        } else {
            "admin API unreachable"
        };
        frame.render_widget(
            Paragraph::new(format!("{msg}\n{base}"))
                .block(Block::default().borders(Borders::ALL).title("Dashboard")),
            area,
        );
        return;
    };
    let rows = vec![
        Row::new(vec!["public listen".to_string(), snap.listen.clone()]),
        Row::new(vec!["admin (local)".to_string(), snap.admin_listen.clone()]),
        Row::new(vec!["uptime".to_string(), format!("{}s", snap.uptime_secs)]),
        Row::new(vec![
            "requests".to_string(),
            snap.requests_total.to_string(),
        ]),
        Row::new(vec![
            "last minute".to_string(),
            snap.requests_last_minute.to_string(),
        ]),
        Row::new(vec![
            "error rate (1h)".to_string(),
            format!("{:.1}%", snap.error_rate * 100.0),
        ]),
        Row::new(vec![
            "spend today".to_string(),
            format!("${:.4}", snap.spend_today_usd),
        ]),
        Row::new(vec![
            "spend month".to_string(),
            format!("${:.4}", snap.spend_month_usd),
        ]),
    ];
    let table = Table::new(rows, [Constraint::Length(18), Constraint::Min(20)])
        .block(Block::default().borders(Borders::ALL).title("Dashboard"));
    frame.render_widget(table, area);
}

fn draw_logs(frame: &mut Frame, area: ratatui::layout::Rect, logs: &[RequestRecord], ui: &Model) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);
    let selected = if logs.is_empty() {
        0
    } else {
        ui.selected.min(logs.len() - 1)
    };
    let rows: Vec<Row> = logs
        .iter()
        .enumerate()
        .map(|(i, rec)| {
            let style = if i == selected {
                Style::default().bg(Color::DarkGray)
            } else if rec.status >= 400 {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(rec.ts.to_string()),
                Cell::from(rec.status.to_string()),
                Cell::from(rec.provider.clone()),
                Cell::from(rec.requested_model.clone()),
                Cell::from(format_tokens(rec)),
                Cell::from(format!("${:.4}", rec.cost_usd)),
                Cell::from(format!("{}ms", rec.latency_ms)),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Min(16),
            Constraint::Length(16),
            Constraint::Length(10),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec![
            "ts", "status", "provider", "model", "tokens", "cost", "latency",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title("Live log"));
    frame.render_widget(table, chunks[0]);
    let detail = logs
        .get(selected)
        .map(|rec| serde_json::to_string_pretty(rec).unwrap_or_default())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Detail")),
        chunks[1],
    );
}

fn format_tokens(rec: &RequestRecord) -> String {
    match (rec.input_tokens, rec.output_tokens) {
        (Some(i), Some(o)) => format!("{i}/{o}"),
        _ => "-".into(),
    }
}

fn draw_providers(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    snap: Option<&DashboardSnapshot>,
    selected: usize,
    prompt: Option<&KeyPrompt>,
) {
    let table_area = if prompt.is_some() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);
        if let Some(prompt) = prompt {
            let masked = "•".repeat(prompt.secret.chars().count());
            frame.render_widget(
                Paragraph::new(format!("{masked}_")).block(
                    Block::default().borders(Borders::ALL).title(format!(
                        " API key for {}  (enter saves, esc cancels) ",
                        prompt.provider
                    )),
                ),
                chunks[1],
            );
        }
        chunks[0]
    } else {
        area
    };
    let providers = snap.map(|s| s.providers.as_slice()).unwrap_or(&[]);
    let rows: Vec<Row> = providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let style = if i == selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                p.name.clone(),
                p.provider_type.clone(),
                if p.enabled { "yes" } else { "no" }.to_string(),
                if p.key_present { "present" } else { "missing" }.to_string(),
                p.last_status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".into()),
                p.base_url.clone(),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec!["name", "type", "enabled", "key", "last", "base url"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Providers  (a sets the API key, l lists models)"),
    );
    frame.render_widget(table, table_area);
}

fn draw_model_picker(frame: &mut Frame, area: ratatui::layout::Rect, picker: &ModelPicker) {
    let visible = picker.visible();
    let selected = if visible.is_empty() {
        0
    } else {
        picker.selected.min(visible.len() - 1)
    };
    let height = area.height.saturating_sub(3) as usize;
    let start = if height == 0 || visible.len() <= height {
        0
    } else {
        selected
            .saturating_sub(height / 2)
            .min(visible.len() - height)
    };
    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .skip(start)
        .take(height.max(1))
        .map(|(i, id)| {
            let style = if i == selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![format!("{}/{}", picker.provider, id)]).style(style)
        })
        .collect();
    let filter = if picker.filter.is_empty() {
        String::new()
    } else {
        format!("  filter: {}", picker.filter)
    };
    let table = Table::new(rows, [Constraint::Min(20)]).block(
        Block::default().borders(Borders::ALL).title(format!(
            " {} models  (enter adds, / filters, esc closes){filter} ",
            picker.provider
        )),
    );
    frame.render_widget(table, area);
}

fn draw_models(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    snap: Option<&DashboardSnapshot>,
    selected: usize,
) {
    let models = snap.map(|s| s.models.as_slice()).unwrap_or(&[]);
    let rows: Vec<Row> = models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let style = if i == selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let price = match (
                m.input_per_mtok,
                m.output_per_mtok,
                m.per_image,
                m.per_minute,
                m.per_1k_chars,
            ) {
                (Some(i), Some(o), _, _, _) => format!("{i}/{o}"),
                (_, _, Some(img), _, _) => format!("${img}/img"),
                (_, _, _, Some(min), _) => format!("${min}/min"),
                (_, _, _, _, Some(chars)) => format!("${chars}/1k"),
                _ => "-".into(),
            };
            Row::new(vec![
                m.id.clone(),
                m.modality.clone(),
                if m.enabled { "yes" } else { "no" }.to_string(),
                price,
                m.capabilities.join(","),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(24),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(14),
            Constraint::Min(16),
        ],
    )
    .header(
        Row::new(vec!["id", "modality", "enabled", "price", "capabilities"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Models  (space toggles)"),
    );
    frame.render_widget(table, area);
}

fn draw_usage(frame: &mut Frame, area: ratatui::layout::Rect, snap: Option<&DashboardSnapshot>) {
    let Some(snap) = snap else {
        frame.render_widget(
            Paragraph::new("no data").block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let data: Vec<(String, u64)> = snap
        .usage_by_model
        .iter()
        .take(8)
        .map(|row| {
            (
                row.key.clone(),
                (row.cost_usd * 10000.0).round().max(0.0) as u64,
            )
        })
        .collect();
    let borrowed: Vec<(&str, u64)> = data.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Spend by model, last 30d (units of $0.0001)"),
        )
        .data(&borrowed)
        .bar_width(8)
        .bar_gap(1);
    frame.render_widget(chart, chunks[0]);
    let rows: Vec<Row> = snap
        .usage_by_day
        .iter()
        .map(|row| {
            Row::new(vec![
                row.key.clone(),
                row.requests.to_string(),
                format!("${:.4}", row.cost_usd),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new(vec!["day", "requests", "cost"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title("By day"));
    frame.render_widget(table, chunks[1]);
}

fn draw_config(frame: &mut Frame, area: ratatui::layout::Rect, snap: Option<&DashboardSnapshot>) {
    let Some(snap) = snap else {
        frame.render_widget(Paragraph::new("no data"), area);
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::raw("path: "),
            Span::raw(snap.config_path.clone()),
        ]),
        Line::from(format!("loaded at: {}", snap.config_loaded_at)),
        Line::from("press e to edit in $EDITOR, then the server reloads"),
        Line::from("press r to reload now"),
        Line::from(""),
    ];
    if let Some(err) = &snap.config_error {
        lines.push(Line::from(Span::styled(
            format!("error: {err}"),
            Style::default().fg(Color::Red),
        )));
    }
    if snap.config_warnings.is_empty() {
        lines.push(Line::from("no warnings"));
    } else {
        lines.push(Line::from("warnings:"));
        for warning in &snap.config_warnings {
            lines.push(Line::from(format!("- {warning}")));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Config")),
        area,
    );
}
