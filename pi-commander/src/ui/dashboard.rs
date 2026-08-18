//! ratatui dashboard — the "one pane all day" view. Polls the daemon, shows
//! per-project agent status, activity inbox for completions, and drives commands
//! from a single input line with mouse + keyboard navigation.
//!
//! Digits: 1 projects · 2 agents · 3 log · 4 input. Tab cycles focus.
//! Activity: n/N jump. Hotkeys: f follow-up · s steer · a abort · / cmds.

use std::collections::{HashMap, VecDeque};
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use serde_json::Value;

use crate::commands::{ApiClient, AppContext};
use crate::dispatch_cmds::{self, ParsedCmd};

const POLL: Duration = Duration::from_millis(350);
const ACTIVITY_CAP: usize = 20;
const HISTORY_CAP: usize = 50;
const SHORTCUTS: &str =
    "1-4 panes  Tab  j/k  n/N activity  f/s/a hotkeys  / cmds  z dense  b/! filter  Ctrl+R reload  q quit";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Projects,
    Agents,
    Log,
    Input,
    Activity,
}

impl Focus {
    fn next(self) -> Self {
        match self {
            Focus::Projects => Focus::Agents,
            Focus::Agents => Focus::Log,
            Focus::Log => Focus::Input,
            Focus::Input => Focus::Activity,
            Focus::Activity => Focus::Projects,
        }
    }
    fn prev(self) -> Self {
        match self {
            Focus::Projects => Focus::Activity,
            Focus::Agents => Focus::Projects,
            Focus::Log => Focus::Agents,
            Focus::Input => Focus::Log,
            Focus::Activity => Focus::Input,
        }
    }
    fn from_digit(ch: char) -> Option<Self> {
        match ch {
            '1' => Some(Focus::Projects),
            '2' => Some(Focus::Agents),
            '3' => Some(Focus::Log),
            '4' => Some(Focus::Input),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Focus::Projects => "projects",
            Focus::Agents => "agents",
            Focus::Log => "log",
            Focus::Input => "input",
            Focus::Activity => "activity",
        }
    }
}

enum Mode {
    Browse,
    Input,
    Help,
    ConfirmQuit,
    ConfirmAbort,
    ConfirmStop,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhaseFilter {
    All,
    Busy,
    Error,
}

#[derive(Clone)]
struct ActivityEvent {
    worker: String,
    phase: String,
    summary: String,
    at_secs: u64,
}

struct LayoutHit {
    header: Rect,
    activity: Rect,
    projects: Rect,
    agents: Rect,
    log: Rect,
    input: Rect,
}

struct UiState {
    agents: Vec<Value>,
    projects: Vec<Value>,
    selected_project: usize, // 0 = "all projects"
    selected_agent_id: Option<String>,
    log_lines: Vec<String>,
    log_raw: bool,
    log_scroll: usize,
    log_follow: bool,
    dense_agents: bool,
    phase_filter: PhaseFilter,
    input: String,
    input_cursor: usize,
    history: Vec<String>,
    history_idx: Option<usize>,
    slash_highlight: usize,
    mode: Mode,
    focus: Focus,
    status: String,
    broker_ok: bool,
    daemon_uptime: u64,
    running_count: usize,
    prev_phases: HashMap<String, String>,
    activity: VecDeque<ActivityEvent>,
    activity_cursor: usize,
    activity_unread: usize,
    hit: LayoutHit,
}

impl UiState {
    fn known_projects(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter_map(|p| p.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
            .collect()
    }

    fn project_filter_id(&self) -> Option<&str> {
        if self.selected_project == 0 {
            None
        } else {
            self.projects
                .get(self.selected_project - 1)
                .and_then(|p| p.get("id").and_then(|i| i.as_str()))
        }
    }

    /// Agent rows for current project + phase filter, busy-first.
    fn filtered(&self) -> Vec<Value> {
        let mut out: Vec<Value> = self
            .agents
            .iter()
            .filter(|a| {
                if let Some(pid) = self.project_filter_id() {
                    if a.get("project_id").and_then(|x| x.as_str()) != Some(pid) {
                        return false;
                    }
                }
                let phase = a.get("phase").and_then(|p| p.as_str()).unwrap_or("");
                match self.phase_filter {
                    PhaseFilter::All => true,
                    PhaseFilter::Busy => {
                        matches!(phase, "busy" | "retrying" | "spawning")
                    }
                    PhaseFilter::Error => matches!(phase, "error" | "stopped"),
                }
            })
            .cloned()
            .collect();
        out.sort_by_key(|a| {
            let phase = a.get("phase").and_then(|p| p.as_str()).unwrap_or("");
            let rank = match phase {
                "error" => 0u8,
                "busy" | "retrying" => 1,
                "spawning" => 2,
                "idle" => 3,
                "stopped" => 4,
                _ => 5,
            };
            (rank, a.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string())
        });
        out
    }

    fn selected_agent_id(&self) -> Option<String> {
        self.selected_agent_id.clone()
    }

    fn selected_agent_index(&self) -> Option<usize> {
        let id = self.selected_agent_id.as_deref()?;
        self.filtered()
            .iter()
            .position(|a| a.get("id").and_then(|i| i.as_str()) == Some(id))
    }

    fn select_agent_by_id(&mut self, id: Option<String>) {
        self.selected_agent_id = id;
        self.log_follow = true;
        self.log_scroll = 0;
        if let Some(ref wid) = self.selected_agent_id {
            let phase = self
                .agents
                .iter()
                .find(|a| a.get("id").and_then(|i| i.as_str()) == Some(wid.as_str()))
                .and_then(|a| a.get("phase").and_then(|p| p.as_str()))
                .unwrap_or("?");
            self.status = format!("selected {wid}, {phase}");
        }
    }

    fn select_agent_at(&mut self, idx: usize) {
        let filtered = self.filtered();
        if filtered.is_empty() {
            self.selected_agent_id = None;
            return;
        }
        let idx = idx.min(filtered.len() - 1);
        let id = filtered[idx]
            .get("id")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string());
        self.select_agent_by_id(id);
    }

    fn clamp(&mut self) {
        let pn = self.projects.len() + 1;
        if self.selected_project >= pn {
            self.selected_project = pn.saturating_sub(1);
        }
        let filtered = self.filtered();
        if let Some(ref id) = self.selected_agent_id {
            if !filtered
                .iter()
                .any(|a| a.get("id").and_then(|i| i.as_str()) == Some(id.as_str()))
            {
                // Selection not in filter — pick first or clear.
                self.selected_agent_id = filtered
                    .first()
                    .and_then(|a| a.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()));
            }
        } else if let Some(a) = filtered.first() {
            self.selected_agent_id = a
                .get("id")
                .and_then(|i| i.as_str())
                .map(|s| s.to_string());
        }
        if !self.activity.is_empty() {
            self.activity_cursor = self.activity_cursor.min(self.activity.len() - 1);
        } else {
            self.activity_cursor = 0;
        }
    }

    fn enter_input(&mut self, prefill: &str) {
        self.input = prefill.to_string();
        self.input_cursor = self.input.len();
        self.history_idx = None;
        self.slash_highlight = 0;
        self.mode = Mode::Input;
        self.focus = Focus::Input;
    }

    fn leave_input(&mut self) {
        self.mode = Mode::Browse;
        self.focus = Focus::Agents;
        self.slash_highlight = 0;
    }

    fn push_history(&mut self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }
        if self.history.last().map(|s| s.as_str()) != Some(cmd) {
            self.history.push(cmd.to_string());
            if self.history.len() > HISTORY_CAP {
                self.history.remove(0);
            }
        }
    }

    fn ingest_phases(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for a in &self.agents {
            let id = match a.get("id").and_then(|i| i.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let phase = a
                .get("phase")
                .and_then(|p| p.as_str())
                .unwrap_or("?")
                .to_string();
            let summary = a
                .get("last_text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .chars()
                .take(48)
                .collect::<String>();
            let prev = self.prev_phases.get(&id).cloned();
            let interesting = match (prev.as_deref(), phase.as_str()) {
                (Some(p), "idle") if p != "idle" && p != "stopped" => true,
                (Some(p), "error") if p != "error" => true,
                (None, "idle" | "error") => false, // first sight — skip flood
                _ => false,
            };
            if interesting {
                self.activity.push_front(ActivityEvent {
                    worker: id.clone(),
                    phase: phase.clone(),
                    summary,
                    at_secs: now,
                });
                while self.activity.len() > ACTIVITY_CAP {
                    self.activity.pop_back();
                }
                self.activity_unread = self.activity_unread.saturating_add(1);
                let glyph = if phase == "idle" { "✓" } else { "✕" };
                self.status = format!("{glyph} {phase} {id}");
            }
            self.prev_phases.insert(id, phase);
        }
    }

    fn jump_activity(&mut self, forward: bool) {
        if self.activity.is_empty() {
            self.status = "no activity yet".into();
            return;
        }
        if forward {
            self.activity_cursor = (self.activity_cursor + 1) % self.activity.len();
        } else {
            self.activity_cursor = if self.activity_cursor == 0 {
                self.activity.len() - 1
            } else {
                self.activity_cursor - 1
            };
        }
        if let Some(ev) = self.activity.get(self.activity_cursor) {
            let worker = ev.worker.clone();
            let phase = ev.phase.clone();
            self.select_agent_by_id(Some(worker.clone()));
            // Prefer project's filter matching this agent.
            if let Some(pid) = worker.split('#').next() {
                if let Some(idx) = self
                    .projects
                    .iter()
                    .position(|p| p.get("id").and_then(|i| i.as_str()) == Some(pid))
                {
                    self.selected_project = idx + 1;
                }
            }
            self.activity_unread = 0;
            self.focus = Focus::Agents;
            self.status = format!("activity → {worker} ({phase})");
        }
    }

    fn log_visible_rows(&self, height: u16) -> usize {
        height.saturating_sub(2) as usize
    }

    fn pin_log_to_end(&mut self, height: u16) {
        let vis = self.log_visible_rows(height);
        if self.log_lines.len() > vis {
            self.log_scroll = self.log_lines.len() - vis;
        } else {
            self.log_scroll = 0;
        }
        self.log_follow = true;
    }
}

fn phase_style(phase: &str) -> Style {
    let color = match phase {
        "idle" => Color::Green,
        "busy" => Color::Yellow,
        "retrying" => Color::Magenta,
        "error" => Color::Red,
        "spawning" => Color::Cyan,
        "stopped" => Color::DarkGray,
        _ => Color::White,
    };
    Style::default().fg(color)
}

fn phase_char(phase: &str) -> &'static str {
    match phase {
        "idle" => "●",
        "busy" => "▶",
        "retrying" => "↻",
        "error" => "✕",
        "spawning" => "…",
        "stopped" => "■",
        _ => "?",
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ago(at: u64) -> String {
    let n = now_secs().saturating_sub(at);
    if n < 60 {
        format!("{n}s")
    } else if n < 3600 {
        format!("{}m", n / 60)
    } else {
        format!("{}h", n / 3600)
    }
}

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
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

pub async fn run_watch(ctx: AppContext) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        anyhow::bail!("watch needs a TTY — use `pi-commander status` or `pi-commander send '...'`");
    }

    let client = ApiClient::new(&ctx.api_base);
    if !client.health().await {
        println!("pi-commander daemon not running — starting it… ");
        let _child = crate::commands::start_daemon_detached(Some(&ctx.config_path))?;
        let deadline = Instant::now() + Duration::from_secs(12);
        while !client.health().await {
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "daemon did not come up on {} — check `pi-commander config validate` and {}",
                    ctx.api_base,
                    crate::config::config_dir()?.join("daemon.log").display()
                );
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        println!("daemon up on {}", ctx.api_base);
    }

    let _guard = TuiGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let empty = Rect::new(0, 0, 0, 0);
    let mut st = UiState {
        agents: vec![],
        projects: vec![],
        selected_project: 0,
        selected_agent_id: None,
        log_lines: vec![],
        log_raw: false,
        log_scroll: 0,
        log_follow: true,
        dense_agents: true,
        phase_filter: PhaseFilter::All,
        input: String::new(),
        input_cursor: 0,
        history: vec![],
        history_idx: None,
        slash_highlight: 0,
        mode: Mode::Browse,
        focus: Focus::Agents,
        status: "ready".into(),
        broker_ok: false,
        daemon_uptime: 0,
        running_count: 0,
        prev_phases: HashMap::new(),
        activity: VecDeque::new(),
        activity_cursor: 0,
        activity_unread: 0,
        hit: LayoutHit {
            header: empty,
            activity: empty,
            projects: empty,
            agents: empty,
            log: empty,
            input: empty,
        },
    };

    let mut last_poll = Instant::now() - POLL;

    loop {
        if last_poll.elapsed() >= POLL {
            if let Ok(h) = client.get("/health").await {
                st.broker_ok = h
                    .pointer("/data/broker")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                st.daemon_uptime = h
                    .pointer("/data/uptime_secs")
                    .and_then(|u| u.as_u64())
                    .unwrap_or(0);
            }
            if let Ok(v) = client.get("/state").await {
                if let Some(data) = v.get("data") {
                    st.agents = data
                        .get("agents")
                        .and_then(|a| a.as_array())
                        .cloned()
                        .unwrap_or_default();
                    st.projects = data
                        .get("projects")
                        .and_then(|p| p.as_array())
                        .cloned()
                        .unwrap_or_default();
                    st.running_count = st
                        .agents
                        .iter()
                        .filter(|a| {
                            a.get("phase")
                                .and_then(|p| p.as_str())
                                .map(|p| matches!(p, "busy" | "retrying" | "spawning"))
                                .unwrap_or(false)
                        })
                        .count();
                    st.ingest_phases();
                    st.clamp();
                }
            }
            let detail = if let Some(id) = st.selected_agent_id() {
                client.get(&format!("/agents/{id}")).await.ok()
            } else {
                None
            };
            if let Some(d) = detail {
                if let Some(data) = d.get("data") {
                    let key = if st.log_raw {
                        "log_tail"
                    } else {
                        "transcript_tail"
                    };
                    st.log_lines = data
                        .get(key)
                        .and_then(|l| l.as_array())
                        .map(|l| {
                            l.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if st.log_follow {
                        st.pin_log_to_end(st.hit.log.height.max(4));
                    } else if st.log_scroll > st.log_lines.len().saturating_sub(1) {
                        st.log_scroll = st.log_lines.len().saturating_sub(1);
                    }
                }
            }
            last_poll = Instant::now();
        }

        terminal.draw(|f| {
            render(f.area(), &mut st, f);
        })?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match handle_key(&mut st, &client, key).await {
                        Ok(()) => {}
                        Err(e) => {
                            let is_quit = e
                                .downcast_ref::<std::io::Error>()
                                .map(|io| io.kind() == std::io::ErrorKind::Interrupted)
                                .unwrap_or(false);
                            if is_quit {
                                break;
                            }
                            return Err(e);
                        }
                    }
                }
                Event::Mouse(m) => {
                    handle_mouse(&mut st, &client, m).await;
                }
                Event::Resize(_, _) => {
                    // next draw recomputes hit rects
                }
                _ => {}
            }
        }
    }

    terminal.clear()?;
    Ok(())
}

fn quit_err() -> anyhow::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "quit").into()
}

async fn handle_key(
    st: &mut UiState,
    client: &ApiClient,
    key: crossterm::event::KeyEvent,
) -> Result<()> {
    match st.mode {
        Mode::Browse => match key.code {
            KeyCode::Char('q') => {
                if st.running_count > 0 {
                    st.mode = Mode::ConfirmQuit;
                } else {
                    return Err(quit_err());
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Err(quit_err());
            }
            KeyCode::Char('/') => {
                if let Some(id) = st.selected_agent_id() {
                    st.status = format!("target {id} — / for commands");
                } else {
                    st.status = "/ for commands — free text dispatches".into();
                }
                st.enter_input("/");
            }
            KeyCode::Enter => {
                if st.focus == Focus::Activity {
                    st.jump_activity(true);
                    // re-select current activity item
                    if let Some(ev) = st.activity.get(st.activity_cursor).cloned() {
                        st.select_agent_by_id(Some(ev.worker));
                        st.activity_unread = 0;
                    }
                } else if let Some(id) = st.selected_agent_id() {
                    st.status = format!("target {id} — type a dispatch…");
                    st.enter_input("");
                } else {
                    st.enter_input("");
                }
            }
            KeyCode::Tab => {
                st.focus = st.focus.next();
                if st.focus == Focus::Input {
                    st.enter_input("");
                    if let Some(id) = st.selected_agent_id() {
                        st.status = format!("target {id} — type a dispatch…");
                    }
                } else {
                    st.status = format!("focus: {}", st.focus.label());
                }
            }
            KeyCode::BackTab => {
                st.focus = st.focus.prev();
                if st.focus == Focus::Input {
                    st.enter_input("");
                } else {
                    st.status = format!("focus: {}", st.focus.label());
                }
            }
            KeyCode::Char('R') | KeyCode::Char('r')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                st.status = reload_daemon(client).await;
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                if let Some(focus) = Focus::from_digit(ch) {
                    st.focus = focus;
                    if focus == Focus::Input {
                        st.enter_input("");
                        if let Some(id) = st.selected_agent_id() {
                            st.status = format!("target {id} — type a dispatch…");
                        }
                    } else {
                        st.status = format!("focus: {}", focus.label());
                    }
                }
            }
            KeyCode::Char('f') => {
                if st.selected_agent_id().is_some() {
                    st.enter_input("/fup ");
                    st.status = "follow-up — type message, Enter".into();
                } else {
                    st.status = "select an agent first".into();
                }
            }
            KeyCode::Char('s') => {
                if st.selected_agent_id().is_some() {
                    st.enter_input("/steer ");
                    st.status = "steer — type message, Enter".into();
                } else {
                    st.status = "select an agent first".into();
                }
            }
            KeyCode::Char('a') => {
                if st.selected_agent_id().is_some() {
                    st.mode = Mode::ConfirmAbort;
                } else {
                    st.status = "select an agent first".into();
                }
            }
            KeyCode::Char('x') => {
                if st.selected_agent_id().is_some() {
                    st.mode = Mode::ConfirmStop;
                } else {
                    st.status = "select an agent first".into();
                }
            }
            KeyCode::Char('n') => st.jump_activity(true),
            KeyCode::Char('N') => st.jump_activity(false),
            KeyCode::Char('z') => {
                st.dense_agents = !st.dense_agents;
                st.status = format!(
                    "agents: {}",
                    if st.dense_agents {
                        "dense"
                    } else {
                        "preview"
                    }
                );
            }
            KeyCode::Char('b') => {
                st.phase_filter = if st.phase_filter == PhaseFilter::Busy {
                    PhaseFilter::All
                } else {
                    PhaseFilter::Busy
                };
                st.clamp();
                st.status = format!(
                    "filter: {}",
                    match st.phase_filter {
                        PhaseFilter::All => "all",
                        PhaseFilter::Busy => "busy",
                        PhaseFilter::Error => "error",
                    }
                );
            }
            KeyCode::Char('!') => {
                st.phase_filter = if st.phase_filter == PhaseFilter::Error {
                    PhaseFilter::All
                } else {
                    PhaseFilter::Error
                };
                st.clamp();
                st.status = format!(
                    "filter: {}",
                    match st.phase_filter {
                        PhaseFilter::All => "all",
                        PhaseFilter::Busy => "busy",
                        PhaseFilter::Error => "error",
                    }
                );
            }
            KeyCode::Char('r') => {
                st.log_raw = !st.log_raw;
                st.log_follow = true;
                st.status = format!("log: {}", if st.log_raw { "raw JSON" } else { "friendly" });
            }
            KeyCode::Char('g') | KeyCode::Home => match st.focus {
                Focus::Agents => st.select_agent_at(0),
                Focus::Projects => {
                    st.selected_project = 0;
                    st.clamp();
                }
                Focus::Log => {
                    st.log_scroll = 0;
                    st.log_follow = false;
                }
                Focus::Activity => {
                    st.activity_cursor = 0;
                }
                _ => {}
            },
            KeyCode::Char('G') | KeyCode::End => match st.focus {
                Focus::Agents => {
                    let n = st.filtered().len();
                    if n > 0 {
                        st.select_agent_at(n - 1);
                    }
                }
                Focus::Projects => {
                    let n = st.projects.len() + 1;
                    if n > 0 {
                        st.selected_project = n - 1;
                        st.clamp();
                    }
                }
                Focus::Log => {
                    st.pin_log_to_end(st.hit.log.height.max(4));
                }
                Focus::Activity => {
                    if !st.activity.is_empty() {
                        st.activity_cursor = st.activity.len() - 1;
                    }
                }
                _ => {}
            },
            KeyCode::PageDown => {
                if st.focus == Focus::Log {
                    let step = st.log_visible_rows(st.hit.log.height.max(4)).max(1);
                    st.log_scroll = (st.log_scroll + step)
                        .min(st.log_lines.len().saturating_sub(1));
                    st.log_follow = false;
                }
            }
            KeyCode::PageUp => {
                if st.focus == Focus::Log {
                    let step = st.log_visible_rows(st.hit.log.height.max(4)).max(1);
                    st.log_scroll = st.log_scroll.saturating_sub(step);
                    st.log_follow = false;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => move_selection(st, 1),
            KeyCode::Char('k') | KeyCode::Up => move_selection(st, -1),
            _ => {}
        },
        Mode::Input => match key.code {
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                st.status = reload_daemon(client).await;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                st.input.clear();
                st.input_cursor = 0;
                st.slash_highlight = 0;
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_word_before_cursor(st);
            }
            KeyCode::Esc => {
                st.input.clear();
                st.input_cursor = 0;
                st.leave_input();
                st.status = "cancelled".into();
            }
            KeyCode::Enter => {
                // Incomplete slash prefix (e.g. `/ste`) → complete highlighted cmd instead of submit.
                if st.input.starts_with('/') && !st.input.contains(' ') {
                    let prefix = dispatch_cmds::strip_slash(st.input.trim());
                    let matches = dispatch_cmds::filter_prefix(&st.input);
                    let exact = matches.iter().any(|c| {
                        c.name == prefix || c.aliases.iter().any(|a| *a == prefix)
                    });
                    if !exact {
                        if let Some(cmd) = matches.get(st.slash_highlight) {
                            if cmd.name != "help" {
                                let insert = dispatch_cmds::completion_insert(cmd);
                                st.input = insert;
                                st.input_cursor = st.input.len();
                                st.slash_highlight = 0;
                                return Ok(());
                            }
                        }
                    }
                }
                let cmd = st.input.trim().to_string();
                st.push_history(&cmd);
                st.input.clear();
                st.input_cursor = 0;
                st.leave_input();
                if cmd.is_empty() || cmd == "/" {
                    st.mode = Mode::Help;
                    st.status = "dispatch commands (/help)".into();
                } else if dispatch_cmds::is_help_command(&cmd) {
                    st.mode = Mode::Help;
                    st.status = "dispatch commands".into();
                } else {
                    st.status = format!("sending: {cmd}");
                    let worker = st.selected_agent_id();
                    let known = st.known_projects();
                    match execute_ui_command(client, &cmd, worker.as_deref(), &known).await {
                        Ok(msg) => st.status = msg,
                        Err(e) => st.status = format!("✗ {e}"),
                    }
                }
            }
            KeyCode::Tab => {
                accept_slash_completion(st);
            }
            KeyCode::Up => {
                if st.input.starts_with('/') && !st.input[1..].contains(' ') {
                    let n = dispatch_cmds::filter_prefix(&st.input).len();
                    if n > 0 {
                        st.slash_highlight = if st.slash_highlight == 0 {
                            n - 1
                        } else {
                            st.slash_highlight - 1
                        };
                    }
                } else {
                    history_prev(st);
                }
            }
            KeyCode::Down => {
                if st.input.starts_with('/') && !st.input[1..].contains(' ') {
                    let n = dispatch_cmds::filter_prefix(&st.input).len();
                    if n > 0 {
                        st.slash_highlight = (st.slash_highlight + 1) % n;
                    }
                } else {
                    history_next(st);
                }
            }
            KeyCode::Left => {
                if st.input_cursor > 0 {
                    st.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if st.input_cursor < st.input.len() {
                    st.input_cursor += 1;
                }
            }
            KeyCode::Home => st.input_cursor = 0,
            KeyCode::End => st.input_cursor = st.input.len(),
            KeyCode::Backspace => {
                if st.input_cursor > 0 {
                    let before = st.input_cursor - 1;
                    st.input.remove(before);
                    st.input_cursor = before;
                    st.slash_highlight = 0;
                }
            }
            KeyCode::Delete => {
                if st.input_cursor < st.input.len() {
                    st.input.remove(st.input_cursor);
                    st.slash_highlight = 0;
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                st.input.insert(st.input_cursor, ch);
                st.input_cursor += 1;
                st.slash_highlight = 0;
            }
            _ => {}
        },
        Mode::Help => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                st.mode = Mode::Browse;
                st.status = "ready".into();
            }
            _ => {}
        },
        Mode::ConfirmQuit => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => return Err(quit_err()),
            KeyCode::Char('n') | KeyCode::Esc => st.mode = Mode::Browse,
            _ => {}
        },
        Mode::ConfirmAbort => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                st.mode = Mode::Browse;
                if let Some(w) = st.selected_agent_id() {
                    match execute_ui_command(client, &format!("abort {w}"), Some(&w), &st.known_projects())
                        .await
                    {
                        Ok(msg) => st.status = msg,
                        Err(e) => st.status = format!("✗ {e}"),
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                st.mode = Mode::Browse;
                st.status = "abort cancelled".into();
            }
            _ => {}
        },
        Mode::ConfirmStop => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                st.mode = Mode::Browse;
                if let Some(w) = st.selected_agent_id() {
                    match execute_ui_command(client, &format!("stop {w}"), Some(&w), &st.known_projects())
                        .await
                    {
                        Ok(msg) => st.status = msg,
                        Err(e) => st.status = format!("✗ {e}"),
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                st.mode = Mode::Browse;
                st.status = "stop cancelled".into();
            }
            _ => {}
        },
    }
    Ok(())
}

fn move_selection(st: &mut UiState, delta: i32) {
    match st.focus {
        Focus::Agents => {
            let n = st.filtered().len() as i32;
            if n == 0 {
                return;
            }
            let cur = st.selected_agent_index().unwrap_or(0) as i32;
            let next = (cur + delta).rem_euclid(n) as usize;
            st.select_agent_at(next);
        }
        Focus::Projects => {
            let n = (st.projects.len() + 1) as i32;
            let next = (st.selected_project as i32 + delta).rem_euclid(n) as usize;
            st.selected_project = next;
            // Reset agent selection to first of new filter.
            st.selected_agent_id = None;
            st.clamp();
            st.status = format!(
                "project filter: {}",
                st.project_filter_id().unwrap_or("all")
            );
        }
        Focus::Log => {
            if delta > 0 {
                if st.log_scroll < st.log_lines.len().saturating_sub(1) {
                    st.log_scroll += 1;
                    st.log_follow = false;
                }
            } else if st.log_scroll > 0 {
                st.log_scroll -= 1;
                st.log_follow = false;
            }
        }
        Focus::Activity => {
            if delta > 0 {
                st.jump_activity(true);
            } else {
                st.jump_activity(false);
            }
        }
        Focus::Input => {
            st.enter_input("");
        }
    }
}

fn accept_slash_completion(st: &mut UiState) {
    if !st.input.starts_with('/') {
        st.input.insert(st.input_cursor, ' ');
        st.input_cursor += 1;
        return;
    }
    let matches = dispatch_cmds::filter_prefix(&st.input);
    if let Some(cmd) = matches.get(st.slash_highlight) {
        let insert = dispatch_cmds::completion_insert(cmd);
        st.input = insert;
        st.input_cursor = st.input.len();
        st.slash_highlight = 0;
    }
}

fn delete_word_before_cursor(st: &mut UiState) {
    if st.input_cursor == 0 {
        return;
    }
    let bytes = st.input.as_bytes();
    let mut i = st.input_cursor;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    while i > 0 && !bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    st.input.replace_range(i..st.input_cursor, "");
    st.input_cursor = i;
}

fn history_prev(st: &mut UiState) {
    if st.history.is_empty() {
        return;
    }
    let idx = match st.history_idx {
        None => st.history.len() - 1,
        Some(0) => 0,
        Some(i) => i - 1,
    };
    st.history_idx = Some(idx);
    st.input = st.history[idx].clone();
    st.input_cursor = st.input.len();
}

fn history_next(st: &mut UiState) {
    let Some(idx) = st.history_idx else {
        return;
    };
    if idx + 1 >= st.history.len() {
        st.history_idx = None;
        st.input.clear();
        st.input_cursor = 0;
    } else {
        st.history_idx = Some(idx + 1);
        st.input = st.history[idx + 1].clone();
        st.input_cursor = st.input.len();
    }
}

async fn handle_mouse(st: &mut UiState, client: &ApiClient, m: crossterm::event::MouseEvent) {
    let _ = client;
    let col = m.column;
    let row = m.row;

    match m.kind {
        MouseEventKind::ScrollDown => {
            if point_in(st.hit.log, col, row) || st.focus == Focus::Log {
                st.focus = Focus::Log;
                if st.log_scroll < st.log_lines.len().saturating_sub(1) {
                    st.log_scroll += 1;
                    st.log_follow = false;
                }
            } else if point_in(st.hit.agents, col, row) {
                move_selection(st, 1);
            } else if point_in(st.hit.projects, col, row) {
                st.focus = Focus::Projects;
                move_selection(st, 1);
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in(st.hit.log, col, row) || st.focus == Focus::Log {
                st.focus = Focus::Log;
                if st.log_scroll > 0 {
                    st.log_scroll -= 1;
                    st.log_follow = false;
                }
            } else if point_in(st.hit.agents, col, row) {
                move_selection(st, -1);
            } else if point_in(st.hit.projects, col, row) {
                st.focus = Focus::Projects;
                move_selection(st, -1);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in(st.hit.input, col, row) {
                st.enter_input(&st.input.clone());
                return;
            }
            if point_in(st.hit.activity, col, row) {
                st.focus = Focus::Activity;
                // Approximate which event from x position — pick cursor item / first.
                if !st.activity.is_empty() {
                    let ev = st
                        .activity
                        .get(st.activity_cursor)
                        .cloned()
                        .or_else(|| st.activity.front().cloned());
                    if let Some(ev) = ev {
                        st.select_agent_by_id(Some(ev.worker));
                        st.activity_unread = 0;
                    }
                }
                return;
            }
            if point_in(st.hit.projects, col, row) {
                st.focus = Focus::Projects;
                st.mode = Mode::Browse;
                let inner_y = row.saturating_sub(st.hit.projects.y.saturating_add(1));
                let idx = inner_y as usize;
                let max = st.projects.len() + 1;
                if idx < max {
                    st.selected_project = idx;
                    st.selected_agent_id = None;
                    st.clamp();
                    st.status = format!(
                        "project filter: {}",
                        st.project_filter_id().unwrap_or("all")
                    );
                }
                return;
            }
            if point_in(st.hit.agents, col, row) {
                st.focus = Focus::Agents;
                st.mode = Mode::Browse;
                let inner_y = row.saturating_sub(st.hit.agents.y.saturating_add(1));
                let row_h: u16 = if st.dense_agents { 1 } else { 2 };
                let idx = (inner_y / row_h) as usize;
                let filtered = st.filtered();
                if idx < filtered.len() {
                    let id = filtered[idx]
                        .get("id")
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string());
                    st.select_agent_by_id(id);
                }
                return;
            }
            if point_in(st.hit.log, col, row) {
                st.focus = Focus::Log;
                st.mode = Mode::Browse;
            }
        }
        _ => {}
    }
}

fn point_in(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x.saturating_add(r.width) && row >= r.y && row < r.y.saturating_add(r.height)
}

fn render(area: Rect, st: &mut UiState, f: &mut ratatui::Frame) {
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(2), // activity
            Constraint::Min(6),    // mid panes
            Constraint::Length(3), // input
            Constraint::Length(1), // shortcuts
            Constraint::Length(1), // status
        ])
        .split(area);

    st.hit.header = chunks[0];
    st.hit.activity = chunks[1];
    st.hit.input = chunks[3];

    // header
    let unread = if st.activity_unread > 0 {
        format!("  ✉{}", st.activity_unread)
    } else {
        String::new()
    };
    let header = Line::from(vec![
        Span::styled(
            "pi-commander",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(format!("daemon {}s", st.daemon_uptime)),
        Span::raw("  broker "),
        Span::styled(
            if st.broker_ok { "●" } else { "○" },
            Style::default().fg(if st.broker_ok {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw(format!(
            "  agents {} ({} running)",
            st.agents.len(),
            st.running_count
        )),
        Span::styled(unread, Style::default().fg(Color::Yellow)),
        Span::raw(format!("  focus[{}]", st.focus.label())),
    ]);
    f.render_widget(
        Paragraph::new(header).block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );

    render_activity(chunks[1], st, f);

    let mid = Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            Constraint::Percentage(18),
            Constraint::Percentage(34),
            Constraint::Percentage(48),
        ])
        .split(chunks[2]);

    st.hit.projects = mid[0];
    st.hit.agents = mid[1];
    st.hit.log = mid[2];

    render_projects(mid[0], st, f);
    render_agents(mid[1], st, f);
    render_log(mid[2], st, f);

    // input with cursor
    let input_focused = matches!(st.focus, Focus::Input) || matches!(st.mode, Mode::Input);
    let input_style = if input_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };
    let target = st.selected_agent_id().unwrap_or_else(|| "—".into());
    let cursor = if input_focused { "▌" } else { "" };
    let (before, after) = if st.input_cursor <= st.input.len() {
        (
            &st.input[..st.input_cursor],
            &st.input[st.input_cursor..],
        )
    } else {
        (st.input.as_str(), "")
    };
    let prompt = if st.input.is_empty() && !input_focused {
        format!("[{target}] > ")
    } else {
        format!("[{target}] {before}{cursor}{after}")
    };
    f.render_widget(
        Paragraph::new(Span::styled(prompt, input_style))
            .block(pane_block("input", Focus::Input, st.focus)),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Span::styled(
            SHORTCUTS,
            Style::default().fg(Color::DarkGray),
        )),
        chunks[4],
    );

    let status_style = Style::default().fg(if st.status.starts_with('✗') {
        Color::Red
    } else if st.status.starts_with('✓') || st.status.starts_with('→') {
        Color::Green
    } else {
        Color::DarkGray
    });
    f.render_widget(
        Paragraph::new(Span::styled(st.status.clone(), status_style)),
        chunks[5],
    );

    if matches!(st.mode, Mode::ConfirmQuit) {
        render_confirm(area, f, &format!("{} agents running — really quit?\n(y/n)", st.running_count));
    }
    if matches!(st.mode, Mode::ConfirmAbort) {
        let w = st.selected_agent_id().unwrap_or_default();
        render_confirm(area, f, &format!("abort {w}?\n(y/n)"));
    }
    if matches!(st.mode, Mode::ConfirmStop) {
        let w = st.selected_agent_id().unwrap_or_default();
        render_confirm(area, f, &format!("stop {w}?\n(y/n)"));
    }
    if matches!(st.mode, Mode::Help) {
        render_help_overlay(area, st, f);
    } else if matches!(st.mode, Mode::Input) && st.input.starts_with('/') {
        render_slash_menu(area, st, f);
    }
}

fn render_confirm(area: Rect, f: &mut ratatui::Frame, text: &str) {
    let box_w = 48u16;
    let box_h = 4u16;
    let box_x = area.x + area.width.saturating_sub(box_w) / 2;
    let box_y = area.y + area.height.saturating_sub(box_h) / 2;
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL)),
        Rect::new(box_x, box_y, box_w, box_h),
    );
}

fn render_activity(area: Rect, st: &UiState, f: &mut ratatui::Frame) {
    let focused = st.focus == Focus::Activity;
    let title = if st.activity_unread > 0 {
        format!("activity · {} new", st.activity_unread)
    } else {
        "activity".into()
    };
    let mut spans: Vec<Span> = Vec::new();
    if st.activity.is_empty() {
        spans.push(Span::styled(
            " (completions appear here — n/N to jump)",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        for (i, ev) in st.activity.iter().take(6).enumerate() {
            let sel = i == st.activity_cursor;
            let glyph = if ev.phase == "idle" { "✓" } else { "✕" };
            let style = if sel {
                Style::default()
                    .fg(if ev.phase == "idle" {
                        Color::Green
                    } else {
                        Color::Red
                    })
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                phase_style(&ev.phase)
            };
            if i > 0 {
                spans.push(Span::raw(" │ "));
            }
            spans.push(Span::styled(
                format!("{glyph} {} · {} · {}", ev.worker, ago(ev.at_secs), {
                    let s = ev.summary.trim();
                    if s.is_empty() {
                        ev.phase.clone()
                    } else {
                        s.chars().take(24).collect::<String>()
                    }
                }),
                style,
            ));
        }
    }
    let block = pane_block(&title, Focus::Activity, st.focus);
    let _ = focused;
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn pane_block(title: &str, focus: Focus, current: Focus) -> Block<'static> {
    let t = if focus == current {
        format!("▸ {title}")
    } else {
        format!("  {title}")
    };
    let mut b = Block::default().borders(Borders::ALL).title(t);
    if focus == current {
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

fn render_projects(area: Rect, st: &UiState, f: &mut ratatui::Frame) {
    let mut items = vec![ListItem::new(Line::from(vec![
        Span::raw("◧ "),
        Span::styled("all", Style::default().fg(Color::White)),
        Span::raw(format!(" ({})", st.agents.len())),
    ]))];
    for p in &st.projects {
        let id = p.get("id").and_then(|i| i.as_str()).unwrap_or("?");
        let running = p
            .get("running_agents")
            .and_then(|r| r.as_u64())
            .unwrap_or(0);
        let (busy, failing) = phase_totals(st, id);
        let dot = if failing > 0 {
            Span::styled("✕", Style::default().fg(Color::Red))
        } else if busy > 0 {
            Span::styled("▶", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("●", Style::default().fg(Color::DarkGray))
        };
        items.push(ListItem::new(Line::from(vec![
            Span::raw(" "),
            dot,
            Span::raw(format!(" {id} ({running})")),
        ])));
    }
    let mut state = ListState::default();
    state.select(Some(st.selected_project));
    let list = List::new(items)
        .block(pane_block("projects", Focus::Projects, st.focus))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, area, &mut state);
}

fn phase_totals(st: &UiState, pid: &str) -> (usize, usize) {
    let (mut busy, mut failing) = (0usize, 0usize);
    for a in &st.agents {
        if a.get("project_id").and_then(|x| x.as_str()) != Some(pid) {
            continue;
        }
        match a.get("phase").and_then(|p| p.as_str()) {
            Some("busy") | Some("retrying") | Some("spawning") => busy += 1,
            Some("error") | Some("stopped") => failing += 1,
            _ => {}
        }
    }
    (busy, failing)
}

fn render_agents(area: Rect, st: &UiState, f: &mut ratatui::Frame) {
    let filtered = st.filtered();
    let busy = filtered
        .iter()
        .filter(|a| {
            matches!(
                a.get("phase").and_then(|p| p.as_str()),
                Some("busy") | Some("retrying") | Some("spawning")
            )
        })
        .count();
    let filter_label = match st.phase_filter {
        PhaseFilter::All => "all",
        PhaseFilter::Busy => "busy",
        PhaseFilter::Error => "error",
    };
    let proj = st.project_filter_id().unwrap_or("all");
    let title = format!("agents · {busy} busy · {filter_label}:{proj}");

    let items: Vec<ListItem> = filtered
        .iter()
        .map(|a| {
            let id = a.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let phase = a.get("phase").and_then(|v| v.as_str()).unwrap_or("?");
            let model = a.get("model").and_then(|v| v.as_str()).unwrap_or("");
            let tool = a.get("current_tool").and_then(|v| v.as_str()).unwrap_or("");
            let text = a.get("last_text").and_then(|v| v.as_str()).unwrap_or("");
            let msgs = a.get("message_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let line = Line::from(vec![
                Span::styled(format!("{} ", phase_char(phase)), phase_style(phase)),
                Span::raw(format!("{id:<20}")),
                Span::styled(format!("{phase:<9}"), phase_style(phase)),
                Span::raw(format!("{msgs:<4}")),
                Span::styled(format!("{model} "), Style::default().fg(Color::DarkGray)),
                Span::styled(tool, Style::default().fg(Color::Blue)),
            ]);
            if st.dense_agents {
                ListItem::new(line)
            } else {
                let sub = Line::from(vec![
                    Span::raw("  "),
                    Span::raw(text.chars().take(48).collect::<String>()),
                ]);
                ListItem::new(vec![line, sub])
            }
        })
        .collect();

    let mut state = ListState::default();
    if let Some(idx) = st.selected_agent_index() {
        state.select(Some(idx));
    }
    let list = List::new(items)
        .block(pane_block(&title, Focus::Agents, st.focus))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_log(area: Rect, st: &UiState, f: &mut ratatui::Frame) {
    let follow = if st.log_follow { "follow" } else { "paused" };
    let title = format!(
        "log ({}) · {follow}",
        if st.log_raw { "raw" } else { "friendly" }
    );
    let mut lines: Vec<Line> = st
        .log_lines
        .iter()
        .skip(st.log_scroll)
        .take(area.height.saturating_sub(2) as usize)
        .map(|l| Line::raw(l.clone()))
        .collect();
    if lines.is_empty() {
        lines.push(Line::styled(
            "(no activity yet — select an agent, then send something)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    let log = Paragraph::new(lines)
        .block(pane_block(&title, Focus::Log, st.focus))
        .wrap(Wrap { trim: false });
    f.render_widget(log, area);
}

fn render_slash_menu(area: Rect, st: &UiState, f: &mut ratatui::Frame) {
    let lines: Vec<Line> = dispatch_cmds::format_menu_lines(&st.input, 8, st.slash_highlight)
        .into_iter()
        .map(Line::raw)
        .collect();
    let box_h = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let box_w = area.width.saturating_sub(4).min(72);
    let box_x = area.x + 2;
    let box_y = area.y + area.height.saturating_sub(box_h + 3);
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("commands (↑↓ Tab complete · Enter run)")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false }),
        Rect::new(box_x, box_y, box_w, box_h),
    );
}

fn render_help_overlay(area: Rect, st: &UiState, f: &mut ratatui::Frame) {
    let worker = st.selected_agent_id();
    let text = dispatch_cmds::format_help(worker.as_deref());
    let lines: Vec<Line> = text.lines().map(|l| Line::raw(l.to_string())).collect();
    let box_h = area.height.saturating_sub(4).min(24);
    let box_w = area.width.saturating_sub(4).min(82);
    let box_x = area.x + (area.width.saturating_sub(box_w)) / 2;
    let box_y = area.y + (area.height.saturating_sub(box_h)) / 2;
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("dispatch commands (Esc to close)")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false }),
        Rect::new(box_x, box_y, box_w, box_h),
    );
}

async fn reload_daemon(client: &ApiClient) -> String {
    match client.post("/reload", serde_json::json!({})).await {
        Ok(v) => {
            let count = v
                .pointer("/data/project_count")
                .and_then(|c| c.as_u64())
                .unwrap_or(0);
            let spawned = v
                .pointer("/data/spawned")
                .and_then(|s| s.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if spawned == 0 {
                format!("✓ reloaded — {count} project(s), all agents running")
            } else {
                format!("✓ reloaded — {count} project(s), spawned {spawned} agent(s)")
            }
        }
        Err(e) => format!("✗ reload failed: {e}"),
    }
}

/// Execute a command line typed in the TUI (through the daemon API).
async fn execute_ui_command(
    client: &ApiClient,
    cmd: &str,
    default_worker: Option<&str>,
    known_projects: &[String],
) -> Result<String, anyhow::Error> {
    let parsed = dispatch_cmds::parse_command(cmd, default_worker, known_projects);
    let out = match parsed {
        ParsedCmd::Help => {
            return Ok(dispatch_cmds::format_help(default_worker));
        }
        ParsedCmd::Dispatch { text } => {
            if text.is_empty() {
                anyhow::bail!("dispatch needs text — e.g. /dispatch fix bug on my-project");
            }
            client
                .post("/dispatch", serde_json::json!({ "text": text }))
                .await
        }
        ParsedCmd::Prompt { worker, message } => {
            let w = worker
                .or_else(|| default_worker.map(|s| s.to_string()))
                .ok_or_else(|| anyhow::anyhow!("no agent selected — pick one or use /dispatch"))?;
            client
                .post(
                    &format!("/agents/{w}/prompt"),
                    serde_json::json!({ "message": message }),
                )
                .await
        }
        ParsedCmd::Verb { name, worker, rest } => match name {
            "status" => client.get("/state").await,
            "new" => {
                let project = worker.ok_or_else(|| anyhow::anyhow!("usage: /new <project>"))?;
                client
                    .post(
                        &format!("/projects/{project}/spawn"),
                        serde_json::json!({ "count": 1 }),
                    )
                    .await
            }
            "steer" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("steer: select an agent or name one"))?;
                if rest.is_empty() {
                    anyhow::bail!("steer needs a message");
                }
                client
                    .post(
                        &format!("/agents/{w}/steer"),
                        serde_json::json!({ "message": rest }),
                    )
                    .await
            }
            "fup" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("fup: select an agent or name one"))?;
                if rest.is_empty() {
                    anyhow::bail!("fup needs a message");
                }
                client
                    .post(
                        &format!("/agents/{w}/follow_up"),
                        serde_json::json!({ "message": rest }),
                    )
                    .await
            }
            "abort" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("abort: select an agent"))?;
                client
                    .post(&format!("/agents/{w}/abort"), serde_json::json!({}))
                    .await
            }
            "model" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("model: select an agent"))?;
                let model = rest.split_whitespace().next().unwrap_or("");
                if model.is_empty() {
                    anyhow::bail!("usage: /model [worker] provider/id");
                }
                client
                    .post(
                        &format!("/agents/{w}/model"),
                        serde_json::json!({ "model": model }),
                    )
                    .await
            }
            "thinking" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("thinking: select an agent"))?;
                if rest.is_empty() {
                    anyhow::bail!("usage: /thinking [worker] <level>");
                }
                client
                    .post(
                        &format!("/agents/{w}/thinking"),
                        serde_json::json!({ "level": rest }),
                    )
                    .await
            }
            "compact" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("compact: select an agent"))?;
                client
                    .post(&format!("/agents/{w}/compact"), serde_json::json!({}))
                    .await
            }
            "bash" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("bash: select an agent"))?;
                if rest.is_empty() {
                    anyhow::bail!("bash needs a shell command");
                }
                client
                    .post(
                        &format!("/agents/{w}/bash"),
                        serde_json::json!({ "command": rest }),
                    )
                    .await
            }
            "stop" => {
                let w = worker.ok_or_else(|| anyhow::anyhow!("stop: select an agent"))?;
                client
                    .post(&format!("/agents/{w}/stop"), serde_json::json!({}))
                    .await
            }
            other => anyhow::bail!("unknown command: {other}"),
        },
    };
    match &out {
        Ok(v) => {
            let msg = v
                .pointer("/data/worker")
                .and_then(|w| w.as_str())
                .map(|w| format!("→ {w} queued"))
                .or_else(|| {
                    v.pointer("/data/routed_to")
                        .and_then(|w| w.as_str())
                        .map(|w| format!("→ {w} queued"))
                })
                .unwrap_or_else(|| "✓ ok".to_string());
            Ok(msg)
        }
        Err(e) => Ok(format!("✗ {e}")),
    }
}
