mod actions;
mod footer;
mod help;
mod keys;
mod pattern_prompt;
mod progress;
mod queue;
mod rule_overlays;
mod rules_view;
mod setup;
mod theme;
mod triage;

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
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListState, Paragraph, TableState, Tabs};
use ratatui::Frame;
use ratatui::Terminal;

use crate::agent::rules_audit::{apply_audit_suggestions, audit_rules};
use crate::agent::pattern_suggest::{sender_detail_input, suggest_patterns};
use crate::agent::schema::RuleAuditPlan;
use crate::commands::CommandContext;
use crate::config::{save_config_file, RuleConfig};
use crate::process::{self, TeachReport};
use crate::rules::patterns::{subject_pattern_from, validate_pattern};
use crate::store::{CachedMessage, PendingSenderGroup, Store};
use footer::{Activity, render_footer};
use rule_overlays::{accepted_suggestions, SuggestItem};
use rules_view::{
    category_options, render_category_picker, resolve_rule_index, selected_category_index,
    visual_index_for_rule,
};

#[derive(Clone, Copy, Debug)]
enum PatternEditContext {
    TriageTeach,
    RulesEdit { index: usize },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Triage = 0,
    Review = 1,
    Rules = 2,
    Setup = 3,
}

impl Tab {
    fn all() -> [Tab; 4] {
        [Tab::Triage, Tab::Review, Tab::Rules, Tab::Setup]
    }

    fn title(self) -> &'static str {
        match self {
            Tab::Triage => "Triage",
            Tab::Review => "Review",
            Tab::Rules => "Rules",
            Tab::Setup => "Setup",
        }
    }

    fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Triage,
            1 => Tab::Review,
            2 => Tab::Rules,
            _ => Tab::Setup,
        }
    }

    fn next(self) -> Self {
        Tab::from_index((self as usize + 1) % 4)
    }
}

#[derive(Clone, Debug)]
enum OverlayMode {
    None,
    PatternEdit {
        buffer: String,
        context: PatternEditContext,
    },
    PatternAction {
        pattern: String,
        context: PatternEditContext,
    },
    RuleTest {
        pattern: String,
        match_count: usize,
        samples: Vec<(String, String)>,
    },
    RuleAudit {
        plan: RuleAuditPlan,
        selected: usize,
        accepted: Vec<usize>,
    },
    PatternSuggest {
        items: Vec<SuggestItem>,
        selected: usize,
    },
    CategoryPick {
        rule_index: usize,
        selected: usize,
    },
}

struct UiSnapshot<'a> {
    tab: Tab,
    sender_groups: &'a [PendingSenderGroup],
    queue: &'a [CachedMessage],
    rules: &'a [RuleConfig],
    selected: usize,
    rules_selected: usize,
    pending: i64,
    queued: i64,
    plan_total: usize,
    cached_total: i64,
    activity: &'a Activity,
    auto_on: bool,
    poll_label: &'a str,
    show_help: bool,
    help_scroll: usize,
    overlay: OverlayMode,
}

const SENDER_GROUP_LIMIT: usize = 500;

struct LoopState {
    tab: Tab,
    selected: usize,
    rules_selected: usize,
    activity: Activity,
    auto_on: bool,
    show_help: bool,
    help_scroll: usize,
    overlay: OverlayMode,
}

struct ScrollStates {
    triage_table: TableState,
    review_table: TableState,
    rules_list: ListState,
    audit_list: ListState,
    suggest_list: ListState,
    category_list: ListState,
}

fn redraw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &CommandContext,
    state: &LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
) -> Result<()> {
    let store = Store::open(&ctx.app.db_path())?;
    let sender_groups = store.pending_sender_groups(SENDER_GROUP_LIMIT)?;
    let queue = store.review_queue(ctx.app.config.safety.require_review_above)?;
    let pending = store.pending_count(None)?;
    let queued = queue.len() as i64;
    let plan_total = store.pending_plan_message_count()?;
    let cached_total = store.total_count(None)?;
    let rules = ctx.app.config.rules.clone();

    terminal.draw(|f| {
        draw_ui(
            f,
            ctx,
            &UiSnapshot {
                tab: state.tab,
                sender_groups: &sender_groups,
                queue: &queue,
                rules: &rules,
                selected: state.selected,
                rules_selected: state.rules_selected,
                pending,
                queued,
                plan_total,
                cached_total,
                activity: &state.activity,
                auto_on: state.auto_on,
                poll_label,
                show_help: state.show_help,
                help_scroll: state.help_scroll,
                overlay: state.overlay.clone(),
            },
            scroll,
        );
    })?;
    Ok(())
}

pub async fn run(ctx: &mut CommandContext) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut state = LoopState {
        tab: Tab::Triage,
        selected: 0,
        rules_selected: 0,
        activity: Activity::Ready,
        auto_on: false,
        show_help: false,
        help_scroll: 0,
        overlay: OverlayMode::None,
    };
    let mut scroll = ScrollStates {
        triage_table: TableState::default(),
        review_table: TableState::default(),
        rules_list: ListState::default(),
        audit_list: ListState::default(),
        suggest_list: ListState::default(),
        category_list: ListState::default(),
    };
    let mut last_auto = Instant::now();
    let poll = auto_interval(ctx);
    let poll_label = poll_label(&poll);

    if let Ok(store) = Store::open(&ctx.app.db_path()) {
        store.reset_remaining_planned_if_no_pending_plan().ok();
    }

    loop {
        let store = Store::open(&ctx.app.db_path())?;
        let sender_groups = store.pending_sender_groups(SENDER_GROUP_LIMIT)?;
        let queue = store.review_queue(ctx.app.config.safety.require_review_above)?;
        let pending = store.pending_count(None)?;
        let rules = ctx.app.config.rules.clone();

        if state.tab == Tab::Triage && pending > 0 && sender_groups.is_empty() {
            state.activity =
                Activity::Success(format!("{pending} msgs pending — press x for AI classify"));
        }

        clamp_selection(
            state.tab,
            &sender_groups,
            &queue,
            &rules,
            &mut state.selected,
            &mut state.rules_selected,
        );

        redraw(&mut terminal, ctx, &state, &mut scroll, &poll_label)?;

        let timeout = if state.auto_on {
            poll.saturating_sub(last_auto.elapsed())
        } else {
            Duration::from_millis(200)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if state.show_help {
                    match key.code {
                        KeyCode::Char('?') | KeyCode::Esc => state.show_help = false,
                        KeyCode::Char('j') | KeyCode::Down => {
                            state.help_scroll = (state.help_scroll + 1)
                                .min(help::help_line_count().saturating_sub(1));
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            state.help_scroll = state.help_scroll.saturating_sub(1);
                        }
                        _ => {}
                    }
                    continue;
                }
                if !matches!(state.overlay, OverlayMode::None)
                    && handle_overlay_key(
                        &mut state.overlay,
                        key.code,
                        &store,
                        &sender_groups,
                        state.selected,
                        &mut state.rules_selected,
                        ctx,
                        &mut state.activity,
                    )
                {
                    continue;
                }

                let sample_msg = sample_message(&store, &sender_groups, state.selected);

                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => break,
                    KeyCode::Char('?') => {
                        state.show_help = true;
                        state.help_scroll = 0;
                    }
                    KeyCode::Tab => state.tab = state.tab.next(),
                    KeyCode::Char(c @ '1'..='4') => {
                        state.tab = Tab::from_index((c as usize) - ('1' as usize));
                    }
                    KeyCode::Char('j') | KeyCode::Down => match state.tab {
                        Tab::Triage if !sender_groups.is_empty() => {
                            state.selected = (state.selected + 1).min(sender_groups.len() - 1);
                        }
                        Tab::Review if !queue.is_empty() => {
                            state.selected = (state.selected + 1).min(queue.len() - 1);
                        }
                        Tab::Rules if !rules.is_empty() => {
                            state.rules_selected =
                                (state.rules_selected + 1).min(rules.len() - 1);
                        }
                        _ => {}
                    },
                    KeyCode::Char('k') | KeyCode::Up => match state.tab {
                        Tab::Triage | Tab::Review => {
                            state.selected = state.selected.saturating_sub(1);
                        }
                        Tab::Rules => {
                            state.rules_selected = state.rules_selected.saturating_sub(1);
                        }
                        _ => {}
                    },
                    KeyCode::Char('A') => {
                        state.auto_on = !state.auto_on;
                        if state.auto_on {
                            state.activity = Activity::AutoIdle;
                            last_auto = Instant::now() - poll;
                        } else {
                            state.activity = Activity::Ready;
                        }
                    }
                    KeyCode::Char('s') => {
                        run_sync(&mut terminal, ctx, &mut state, &mut scroll, &poll_label).await;
                    }
                    KeyCode::Char('x') if state.tab == Tab::Triage => {
                        run_classify(&mut terminal, ctx, &mut state, &mut scroll, &poll_label).await;
                    }
                    KeyCode::Char('x') if state.tab == Tab::Rules => {
                        run_rules_audit(&mut terminal, ctx, &mut state, &mut scroll, &poll_label).await;
                    }
                    KeyCode::Char('a')
                        if matches!(state.tab, Tab::Triage | Tab::Review) =>
                    {
                        run_apply(&mut terminal, ctx, &mut state.activity).await;
                    }
                    KeyCode::Char('p') if state.tab == Tab::Triage => {
                        run_pattern_suggest(
                            &mut terminal,
                            ctx,
                            &mut state,
                            &mut scroll,
                            &poll_label,
                            &store,
                            &sender_groups,
                        )
                        .await;
                    }
                    KeyCode::Char('z') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_junk_message(ctx, msg, false),
                                "Junk subject rule",
                            );
                        }
                    }
                    KeyCode::Char('Z') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_junk_message(ctx, msg, true),
                                "Junk sender rule",
                            );
                        }
                    }
                    KeyCode::Char('/') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            state.overlay = OverlayMode::PatternEdit {
                                buffer: subject_pattern_from(&msg.subject),
                                context: PatternEditContext::TriageTeach,
                            };
                        }
                    }
                    KeyCode::Char('g') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_message_subject(ctx, msg, "archive", Some("newsletter"), 2),
                                "Archive subject",
                            );
                        }
                    }
                    KeyCode::Char('G') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_message_sender(ctx, msg, "archive", Some("newsletter"), 2),
                                "Archive sender",
                            );
                        }
                    }
                    KeyCode::Char('i') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_message_subject(ctx, msg, "flag", Some("priority"), 5),
                                "Important subject",
                            );
                        }
                    }
                    KeyCode::Char('I') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_message_sender(ctx, msg, "flag", Some("priority"), 5),
                                "Important sender",
                            );
                        }
                    }
                    KeyCode::Char('o') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_message_subject(ctx, msg, "keep", Some("personal"), 4),
                                "Keep subject",
                            );
                        }
                    }
                    KeyCode::Char('O') if state.tab == Tab::Triage => {
                        if let Some(msg) = &sample_msg {
                            apply_teach_activity(
                                &mut state.activity,
                                process::teach_message_sender(ctx, msg, "keep", Some("personal"), 4),
                                "Keep sender",
                            );
                        }
                    }
                    KeyCode::Char('c') if state.tab == Tab::Rules && !rules.is_empty() => {
                        let rule_index = resolve_rule_index(&rules, state.rules_selected);
                        let selected = selected_category_index(&rules[rule_index]);
                        scroll.category_list = ListState::default();
                        state.overlay = OverlayMode::CategoryPick {
                            rule_index,
                            selected,
                        };
                    }
                    KeyCode::Char('e') if state.tab == Tab::Rules && !rules.is_empty() => {
                        let rule_index = resolve_rule_index(&rules, state.rules_selected);
                        let rule = &rules[rule_index];
                        state.overlay = OverlayMode::PatternEdit {
                            buffer: rule.r#match.clone(),
                            context: PatternEditContext::RulesEdit { index: rule_index },
                        };
                    }
                    KeyCode::Char('t') if state.tab == Tab::Rules && !rules.is_empty() => {
                        let rule_index = resolve_rule_index(&rules, state.rules_selected);
                        let pattern = rules[rule_index].r#match.clone();
                        let matches = store
                            .messages_matching_pattern(&pattern, 5000, 5)
                            .unwrap_or_default();
                        let match_count = store
                            .messages_matching_pattern(&pattern, 5000, usize::MAX)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        let samples = matches
                            .iter()
                            .map(|m| (m.from_address.clone(), m.subject.clone()))
                            .collect();
                        state.overlay = OverlayMode::RuleTest {
                            pattern,
                            match_count,
                            samples,
                        };
                    }
                    KeyCode::Char('n') if state.tab == Tab::Rules => {
                        match add_newsletter_preset(ctx) {
                            Ok(()) => state.activity = Activity::Success("Newsletter preset added".into()),
                            Err(e) => state.activity = Activity::Error(format!("Rule error: {e}")),
                        }
                    }
                    KeyCode::Char('d') if state.tab == Tab::Rules => {
                        let rule_index = resolve_rule_index(&rules, state.rules_selected);
                        match remove_rule(ctx, rule_index) {
                            Ok(()) => state.activity = Activity::Success("Rule removed".into()),
                            Err(e) => state.activity = Activity::Error(format!("Rule error: {e}")),
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        } else if state.auto_on {
            run_auto_cycle(
                &mut terminal,
                ctx,
                &mut state,
                &mut scroll,
                &poll_label,
                &mut last_auto,
            )
            .await;
        }
    }

    disable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(LeaveAlternateScreen)?;
    Ok(())
}

fn draw_ui(
    f: &mut Frame,
    ctx: &CommandContext,
    snap: &UiSnapshot<'_>,
    scroll: &mut ScrollStates,
) {
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(theme::BG)),
        f.area(),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(footer::footer_height(snap.tab)),
        ])
        .split(f.area());

    let header_title = if snap.plan_total > 0 && snap.queued == 0 {
        format!(
            "mail-sweep · {} unclassified · {} ready to apply · {} cached",
            snap.pending, snap.plan_total, snap.cached_total
        )
    } else {
        format!(
            "mail-sweep · {} unclassified · {} in review · {} cached",
            snap.pending, snap.queued, snap.cached_total
        )
    };

    let tab_titles: Vec<Line> = Tab::all()
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let badge = match t {
                Tab::Triage if snap.pending > 0 => format!(" ({})", snap.sender_groups.len()),
                Tab::Review if snap.plan_total > 0 => {
                    if snap.queue.is_empty() {
                        format!(" ({}✓)", snap.plan_total)
                    } else {
                        format!(" ({}/{})", snap.queue.len(), snap.plan_total)
                    }
                }
                Tab::Rules if !snap.rules.is_empty() => format!(" ({})", snap.rules.len()),
                _ => String::new(),
            };
            let style = if snap.tab as usize == i {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::MUTED)
            };
            Line::from(Span::styled(format!("{}{}", t.title(), badge), style))
        })
        .collect();

    let tabs = Tabs::new(tab_titles)
        .block(theme::chrome_block(&header_title))
        .select(snap.tab as usize)
        .highlight_style(
            Style::default()
                .fg(theme::ACCENT2)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(Span::styled(" │ ", Style::default().fg(theme::MUTED)));
    f.render_widget(tabs, chunks[0]);
    f.render_widget(Paragraph::new(help::tab_hint(snap.tab)), chunks[1]);

    match snap.tab {
        Tab::Triage => triage::render_triage(
            f,
            chunks[2],
            snap.sender_groups,
            snap.pending,
            snap.selected,
            &mut scroll.triage_table,
        ),
        Tab::Review => queue::render_queue(
            f,
            chunks[2],
            snap.queue,
            snap.selected,
            &mut scroll.review_table,
            snap.plan_total,
        ),
        Tab::Rules => rules_view::render_rules(
            f,
            chunks[2],
            snap.rules,
            snap.rules_selected,
            &mut scroll.rules_list,
        ),
        Tab::Setup => {
            let account_lines: Vec<Line> = if ctx.app.config.accounts.is_empty() {
                vec![Line::from("  (none — run: mail-sweep accounts add)")]
            } else {
                ctx.app
                    .config
                    .accounts
                    .iter()
                    .map(|a| Line::from(format!("  {} — {}", a.id, a.email)))
                    .collect()
            };
            setup::render_setup(f, chunks[2], snap.auto_on, snap.poll_label, account_lines);
        }
    }

    render_footer(f, chunks[3], snap.tab, snap.activity, snap.auto_on);

    if snap.show_help {
        help::render_help(f, centered_rect(72, 85, f.area()), snap.help_scroll);
    } else {
        match &snap.overlay {
            OverlayMode::PatternEdit { buffer, context } => {
                let title = match context {
                    PatternEditContext::TriageTeach => " custom rule pattern ",
                    PatternEditContext::RulesEdit { .. } => " edit rule pattern ",
                };
                let match_count = if validate_pattern(buffer).error.is_none() {
                    Store::open(&ctx.app.db_path())
                        .ok()
                        .and_then(|s| {
                            s.messages_matching_pattern(buffer, 5000, usize::MAX)
                                .ok()
                                .map(|m| m.len())
                        })
                } else {
                    None
                };
                pattern_prompt::render_pattern_editor(
                    f,
                    centered_rect(70, 45, f.area()),
                    buffer,
                    title,
                    match_count,
                );
            }
            OverlayMode::PatternAction { pattern, context } => {
                let editing = matches!(context, PatternEditContext::RulesEdit { .. });
                pattern_prompt::render_pattern_action_picker(
                    f,
                    centered_rect(60, 38, f.area()),
                    pattern,
                    editing,
                );
            }
            OverlayMode::RuleTest {
                pattern,
                match_count,
                samples,
            } => rule_overlays::render_rule_test(
                f,
                centered_rect(70, 45, f.area()),
                pattern,
                *match_count,
                samples,
            ),
            OverlayMode::RuleAudit {
                plan,
                selected,
                accepted,
            } => rule_overlays::render_rule_audit(
                f,
                centered_rect(80, 80, f.area()),
                plan,
                *selected,
                accepted,
                &mut scroll.audit_list,
            ),
            OverlayMode::PatternSuggest { items, selected } => {
                rule_overlays::render_pattern_suggest(
                    f,
                    centered_rect(75, 70, f.area()),
                    items,
                    *selected,
                    &mut scroll.suggest_list,
                );
            }
            OverlayMode::CategoryPick {
                rule_index,
                selected,
            } => {
                if let Some(rule) = snap.rules.get(*rule_index) {
                    render_category_picker(
                        f,
                        centered_rect(50, 70, f.area()),
                        rule,
                        *selected,
                        &mut scroll.category_list,
                    );
                }
            }
            OverlayMode::None => {}
        }
    }
}

fn sample_message(
    store: &Store,
    groups: &[PendingSenderGroup],
    selected: usize,
) -> Option<CachedMessage> {
    let group = groups.get(selected)?;
    store.get_message(group.sample_message_id).ok().flatten()
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

async fn run_sync(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
) {
    state.activity = Activity::Syncing;
    redraw(terminal, ctx, state, scroll, poll_label).ok();
    match actions::do_sync(ctx).await {
        Ok(msg) => {
            let hint = if ctx.app.config.rules.is_empty() {
                String::new()
            } else {
                " · press x to classify new mail against rules".into()
            };
            state.activity = Activity::Success(format!("{msg}{hint}"));
        }
        Err(e) => state.activity = Activity::Error(format!("Sync failed: {e}")),
    }
}

async fn run_classify(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &mut CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
) {
    state.activity = Activity::Classifying;
    redraw(terminal, ctx, state, scroll, poll_label).ok();
    match actions::do_classify(ctx).await {
        Ok(msg) => state.activity = Activity::Success(msg),
        Err(e) => state.activity = Activity::Error(format!("Classify failed: {e}")),
    }
}

async fn run_apply(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &CommandContext,
    activity: &mut Activity,
) {
    *activity = Activity::Applying;
    let mut snap = crate::apply_progress::ApplySnapshot::default();
    let draw = |t: &mut Terminal<CrosstermBackend<io::Stdout>>,
                progress: &crate::apply_progress::ApplySnapshot| {
        t.draw(|f| progress::render_apply_progress(f, f.area(), progress)).ok();
    };
    draw(terminal, &snap);

    let result = actions::do_apply(ctx, Some(&mut |progress| {
        snap = progress.clone();
        draw(terminal, &snap);
    }))
    .await;

    match result {
        Ok(msg) => *activity = Activity::Success(msg),
        Err(e) => *activity = Activity::Error(format!("Apply failed: {e}")),
    }
}

async fn run_auto_cycle(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &mut CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
    last_auto: &mut Instant,
) {
    use crate::agent::schema::ClassificationPlan;

    state.activity = Activity::Syncing;
    redraw(terminal, ctx, state, scroll, poll_label).ok();

    let sync_result = actions::do_sync(ctx).await;

    state.activity = Activity::Classifying;
    redraw(terminal, ctx, state, scroll, poll_label).ok();

    let classify_result = actions::do_classify(ctx).await;

    let apply_result: Result<Option<String>, anyhow::Error> = async {
        let store = Store::open(&ctx.app.db_path())?;
        let Some(stored) = store.latest_pending_plan()? else {
            return Ok(None);
        };
        let plan: ClassificationPlan =
            serde_json::from_str(&stored.json_plan).map_err(|e| anyhow::anyhow!("{e}"))?;
        let deletes = plan
            .messages
            .iter()
            .filter(|m| m.action.is_destructive())
            .count();
        if deletes > 0 {
            return Ok(Some(format!("{deletes} deletes need Review before apply")));
        }
        state.activity = Activity::Applying;
        redraw(terminal, ctx, state, scroll, poll_label)?;
        let summary = actions::do_apply(ctx, None).await?;
        Ok(Some(summary))
    }
    .await;

    state.activity = match (sync_result, classify_result, apply_result) {
        (Err(e), _, _) => Activity::Error(format!("Auto sync failed: {e}")),
        (_, Err(e), _) => Activity::Error(format!("Auto classify failed: {e}")),
        (_, _, Err(e)) => Activity::Error(format!("Auto apply failed: {e}")),
        (Ok(sync), Ok(classify), Ok(apply)) => {
            let apply_msg = apply.unwrap_or_else(|| "no plan to apply".into());
            Activity::Success(format!("AUTO: {sync} · {classify} · {apply_msg}"))
        }
    };

    if state.auto_on {
        state.activity = Activity::AutoIdle;
    }

    *last_auto = Instant::now();
}

fn clamp_selection(
    tab: Tab,
    groups: &[PendingSenderGroup],
    queue: &[CachedMessage],
    rules: &[RuleConfig],
    selected: &mut usize,
    rules_selected: &mut usize,
) {
    match tab {
        Tab::Triage if !groups.is_empty() => {
            *selected = (*selected).min(groups.len() - 1);
        }
        Tab::Review if !queue.is_empty() => {
            *selected = (*selected).min(queue.len() - 1);
        }
        Tab::Rules if !rules.is_empty() => {
            *rules_selected = (*rules_selected).min(rules.len() - 1);
        }
        _ => {}
    }
}

fn apply_teach_activity(activity: &mut Activity, result: Result<TeachReport>, label: &str) {
    match result {
        Ok(r) => {
            let mut msg = format!(
                "{label}: {} → {} msgs planned — press a to apply to Gmail",
                r.pattern, r.messages_affected
            );
            if let Some(remaining) = r.sender_pending_remaining {
                msg.push_str(&format!(
                    " · {remaining} still unclassified from this sender (I=whole sender, /=pattern)"
                ));
            }
            *activity = Activity::Success(msg);
        }
        Err(e) => *activity = Activity::Error(format!("{label} error: {e}")),
    }
}

async fn run_rules_audit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
) {
    state.activity = Activity::Classifying;
    redraw(terminal, ctx, state, scroll, poll_label).ok();

    let result = async {
        let store = Store::open(&ctx.app.db_path())?;
        audit_rules(&ctx.app, &ctx.app.config.rules, &store).await
    }
    .await;

    match result {
        Ok(plan) => {
            if plan.suggestions.is_empty() {
                state.activity = Activity::Success(plan.summary);
            } else {
                scroll.audit_list = ListState::default();
                state.overlay = OverlayMode::RuleAudit {
                    selected: 0,
                    accepted: (0..plan.suggestions.len()).collect(),
                    plan,
                };
                state.activity = Activity::Success("Review audit suggestions".into());
            }
        }
        Err(e) => state.activity = Activity::Error(format!("Audit failed: {e}")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_pattern_suggest(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
    store: &Store,
    groups: &[PendingSenderGroup],
) {
    let Some(group) = groups.get(state.selected) else {
        return;
    };
    state.activity = Activity::Classifying;
    redraw(terminal, ctx, state, scroll, poll_label).ok();

    let result = async {
        let messages = store.messages_for_sender(
            &group.from_address,
            Some(&group.account_id),
            50,
        )?;
        let detail = sender_detail_input(&group.from_address, &messages);
        let plan = suggest_patterns(&ctx.app, &detail).await?;
        let items: Vec<SuggestItem> = plan
            .patterns
            .into_iter()
            .map(|p| {
                let match_count = store
                    .messages_matching_pattern(&p.match_pattern, 5000, usize::MAX)
                    .map(|m| m.len())
                    .unwrap_or(0);
                SuggestItem {
                    pattern: p,
                    match_count,
                }
            })
            .collect();
        Ok::<_, anyhow::Error>((items, plan.summary))
    }
    .await;

    match result {
        Ok((items, summary)) => {
            if items.is_empty() {
                state.activity = Activity::Success(summary);
            } else {
                scroll.suggest_list = ListState::default();
                state.overlay = OverlayMode::PatternSuggest {
                    items,
                    selected: 0,
                };
                state.activity = Activity::Success("Pick a suggested pattern".into());
            }
        }
        Err(e) => state.activity = Activity::Error(format!("Suggest failed: {e}")),
    }
}

fn apply_rule_action(
    ctx: &mut CommandContext,
    context: &PatternEditContext,
    pattern: &str,
    action: &str,
    category: Option<&str>,
    priority: u8,
    msg: Option<&CachedMessage>,
) -> Result<()> {
    match context {
        PatternEditContext::TriageTeach => {
            if let Some(m) = msg {
                process::teach_pattern(ctx, pattern, action, category, priority, Some(m))?;
            }
        }
        PatternEditContext::RulesEdit { index } => {
            update_rule_at(ctx, *index, pattern, action, category, Some(priority))?;
            // Re-plan matching teachable mail so the pending plan tracks the edit.
            process::teach_pattern(ctx, pattern, action, category, priority, msg)?;
        }
    }
    Ok(())
}

fn update_rule_at(
    ctx: &mut CommandContext,
    index: usize,
    pattern: &str,
    action: &str,
    category: Option<&str>,
    priority: Option<u8>,
) -> Result<()> {
    let mut config = ctx.app.config.clone();
    if index >= config.rules.len() {
        anyhow::bail!("no rule at index {index}");
    }
    let rule = &mut config.rules[index];
    rule.r#match = pattern.into();
    rule.action = action.into();
    rule.category = category.map(|s| s.into());
    if let Some(p) = priority {
        rule.priority = Some(p);
    }
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;
    Ok(())
}

fn update_rule_category(
    ctx: &mut CommandContext,
    index: usize,
    category: Option<&str>,
) -> Result<()> {
    let mut config = ctx.app.config.clone();
    if index >= config.rules.len() {
        anyhow::bail!("no rule at index {index}");
    }
    config.rules[index].category = category.map(|s| s.into());
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_overlay_key(
    overlay: &mut OverlayMode,
    code: KeyCode,
    store: &Store,
    groups: &[PendingSenderGroup],
    selected: usize,
    rules_selected: &mut usize,
    ctx: &mut CommandContext,
    activity: &mut Activity,
) -> bool {
    let msg = sample_message(store, groups, selected);
    match overlay {
        OverlayMode::None => return false,
        OverlayMode::PatternEdit { buffer, context } => match code {
            KeyCode::Esc => *overlay = OverlayMode::None,
            KeyCode::Enter => {
                let pattern = buffer.trim().to_string();
                if pattern.is_empty() || validate_pattern(&pattern).error.is_some() {
                    return true;
                }
                let ctx_copy = *context;
                *overlay = OverlayMode::PatternAction {
                    pattern,
                    context: ctx_copy,
                };
            }
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(c) if !c.is_control() && buffer.len() < 200 => buffer.push(c),
            _ => {}
        },
        OverlayMode::PatternAction { pattern, context } => {
            let pat = pattern.clone();
            let ctx_mode = *context;
            let finish = |overlay: &mut OverlayMode,
                          activity: &mut Activity,
                          label: &str,
                          result: Result<()>| {
                match result {
                    Ok(()) => *activity = Activity::Success(format!("{label}: {pat}")),
                    Err(e) => *activity = Activity::Error(format!("{label} error: {e}")),
                }
                *overlay = OverlayMode::None;
            };
            match code {
                KeyCode::Esc => *overlay = OverlayMode::None,
                KeyCode::Char('z') => finish(
                    overlay,
                    activity,
                    "Rule saved",
                    apply_rule_action(ctx, &ctx_mode, &pat, "delete", Some("spam"), 1, msg.as_ref()),
                ),
                KeyCode::Char('g') => finish(
                    overlay,
                    activity,
                    "Rule saved",
                    apply_rule_action(
                        ctx,
                        &ctx_mode,
                        &pat,
                        "archive",
                        Some("newsletter"),
                        2,
                        msg.as_ref(),
                    ),
                ),
                KeyCode::Char('i') => finish(
                    overlay,
                    activity,
                    "Rule saved",
                    apply_rule_action(
                        ctx,
                        &ctx_mode,
                        &pat,
                        "flag",
                        Some("priority"),
                        5,
                        msg.as_ref(),
                    ),
                ),
                KeyCode::Char('o') => finish(
                    overlay,
                    activity,
                    "Rule saved",
                    apply_rule_action(
                        ctx,
                        &ctx_mode,
                        &pat,
                        "keep",
                        Some("personal"),
                        4,
                        msg.as_ref(),
                    ),
                ),
                _ => {}
            }
        }
        OverlayMode::RuleTest { .. } => {
            if code == KeyCode::Esc {
                *overlay = OverlayMode::None;
            }
        }
        OverlayMode::RuleAudit {
            plan,
            selected,
            accepted,
        } => match code {
            KeyCode::Esc => *overlay = OverlayMode::None,
            KeyCode::Char('j') | KeyCode::Down => {
                *selected = (*selected + 1).min(plan.suggestions.len().saturating_sub(1));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *selected = selected.saturating_sub(1);
            }
            KeyCode::Char(' ') => {
                if accepted.contains(selected) {
                    accepted.retain(|&i| i != *selected);
                } else {
                    accepted.push(*selected);
                }
            }
            KeyCode::Char('a') => {
                let to_apply: Vec<_> = accepted_suggestions(plan, accepted)
                    .into_iter()
                    .cloned()
                    .collect();
                if to_apply.is_empty() {
                    *activity = Activity::Error("No suggestions selected".into());
                } else {
                    let new_rules =
                        apply_audit_suggestions(&ctx.app.config.rules, &to_apply);
                    let mut config = ctx.app.config.clone();
                    config.rules = new_rules;
                    match save_config_file(&ctx.app.config_path, &config) {
                        Ok(()) => {
                            ctx.app.config = config;
                            *activity = Activity::Success(format!(
                                "Applied {} audit suggestions",
                                to_apply.len()
                            ));
                        }
                        Err(e) => *activity = Activity::Error(format!("Save failed: {e}")),
                    }
                }
                *overlay = OverlayMode::None;
            }
            _ => {}
        },
        OverlayMode::PatternSuggest { items, selected } => {
            let pick = match code {
                KeyCode::Char('1') => Some(0),
                KeyCode::Char('2') => Some(1),
                KeyCode::Char('3') => Some(2),
                KeyCode::Char('4') => Some(3),
                KeyCode::Enter => Some(*selected),
                KeyCode::Esc => {
                    *overlay = OverlayMode::None;
                    return true;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    *selected = (*selected + 1).min(items.len().saturating_sub(1));
                    return true;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                    return true;
                }
                _ => None,
            };
            if let Some(idx) = pick {
                if let Some(item) = items.get(idx) {
                    let pat = item.pattern.match_pattern.clone();
                    let action = item.pattern.action.clone();
                    let category = Some(item.pattern.category.as_str());
                    let priority = item.pattern.priority;
                    if let Some(m) = &msg {
                        apply_teach_activity(
                            activity,
                            process::teach_pattern(
                                ctx,
                                &pat,
                                &action,
                                category,
                                priority,
                                Some(m),
                            ),
                            "Suggested pattern",
                        );
                    }
                }
                *overlay = OverlayMode::None;
            }
        }
        OverlayMode::CategoryPick {
            rule_index,
            selected,
        } => {
            let options = category_options();
            match code {
                KeyCode::Esc => *overlay = OverlayMode::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    *selected = (*selected + 1).min(options.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Enter => {
                    let picked = options.get(*selected).copied();
                    let category = picked.filter(|c| *c != "uncategorized");
                    let label = category.unwrap_or("uncategorized");
                    match update_rule_category(ctx, *rule_index, category) {
                        Ok(()) => {
                            *rules_selected =
                                visual_index_for_rule(&ctx.app.config.rules, *rule_index);
                            *activity = Activity::Success(format!("Category → {label}"));
                        }
                        Err(e) => *activity = Activity::Error(format!("Category error: {e}")),
                    }
                    *overlay = OverlayMode::None;
                }
                _ => {}
            }
        }
    }
    true
}

fn add_newsletter_preset(ctx: &mut CommandContext) -> Result<()> {
    push_rule(ctx, "subject:unsubscribe", "archive", Some("newsletter"), Some(2), None)?;
    Ok(())
}

fn push_rule(
    ctx: &mut CommandContext,
    pattern: &str,
    action: &str,
    category: Option<&str>,
    priority: Option<u8>,
    target_folder: Option<&str>,
) -> Result<()> {
    let mut config = ctx.app.config.clone();
    config.rules.push(RuleConfig {
        id: None,
        r#match: pattern.into(),
        category: category.map(|s| s.into()),
        action: action.into(),
        priority,
        target_folder: target_folder.map(|s| s.into()),
    });
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;
    Ok(())
}

fn remove_rule(ctx: &mut CommandContext, index: usize) -> Result<()> {
    let mut config = ctx.app.config.clone();
    if index >= config.rules.len() {
        anyhow::bail!("no rule at index {index}");
    }
    config.rules.remove(index);
    save_config_file(&ctx.app.config_path, &config)?;
    ctx.app.config = config;
    Ok(())
}

fn auto_interval(ctx: &CommandContext) -> Duration {
    parse_poll_interval(&ctx.app.config.sync.poll_interval).unwrap_or(Duration::from_secs(300))
}

fn poll_label(poll: &Duration) -> String {
    if poll.as_secs() >= 3600 {
        format!("{}h", poll.as_secs() / 3600)
    } else if poll.as_secs() >= 60 {
        format!("{}m", poll.as_secs() / 60)
    } else {
        format!("{}s", poll.as_secs())
    }
}

fn parse_poll_interval(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() || s == "0" {
        return None;
    }
    if let Some(secs) = s.strip_suffix('s') {
        return secs.parse().ok().map(Duration::from_secs);
    }
    if let Some(mins) = s.strip_suffix('m') {
        return mins.parse().ok().map(|m: u64| Duration::from_secs(m * 60));
    }
    if let Some(hours) = s.strip_suffix('h') {
        return hours
            .parse()
            .ok()
            .map(|h: u64| Duration::from_secs(h * 3600));
    }
    None
}
