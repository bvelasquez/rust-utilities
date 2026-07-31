mod actions;
mod analytics;
mod footer;
mod help;
mod jobs;
mod keys;
mod message_view;
mod pattern_prompt;
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

use crate::agent::rules_audit::apply_audit_suggestions;
use crate::agent::schema::RuleAuditPlan;
use crate::commands::CommandContext;
use crate::config::{save_config_file, RuleConfig};
use crate::process::{self, TeachReport};
use crate::rules::{find_subsumed_rules, remove_rules_at};
use crate::rules::patterns::{subject_pattern_from, validate_pattern};
use crate::store::{
    AnalyticsPeriod, AppliedAnalytics, CachedMessage, PendingSenderGroup, Store,
};
use footer::{Activity, render_footer};
use jobs::{JobEvent, JobResult, JobRunner};
use pattern_prompt::PatternEditFocus;
use rule_overlays::{accepted_suggestions, SuggestItem};
use rules_view::{
    category_options, render_category_picker, resolve_rule_index, selected_category_index,
    visible_rule_count, visual_index_for_rule, ActionFilter,
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

/// Which Triage list is active when both pending senders and unread leftovers exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TriageFocus {
    Senders,
    Unread,
}

impl TriageFocus {
    fn resolve(self, groups: &[PendingSenderGroup], leftovers: &[CachedMessage]) -> Self {
        match self {
            TriageFocus::Senders if !groups.is_empty() => TriageFocus::Senders,
            TriageFocus::Senders if !leftovers.is_empty() => TriageFocus::Unread,
            TriageFocus::Unread if !leftovers.is_empty() => TriageFocus::Unread,
            TriageFocus::Unread if !groups.is_empty() => TriageFocus::Senders,
            other => other,
        }
    }

    fn showing_unread(self, groups: &[PendingSenderGroup], leftovers: &[CachedMessage]) -> bool {
        matches!(self.resolve(groups, leftovers), TriageFocus::Unread)
    }
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
        desc: String,
        focus: PatternEditFocus,
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
    /// Deterministic: rules covered by a broader keeper (same action).
    SubsumeAudit {
        keeper_index: usize,
        keeper_pattern: String,
        candidates: Vec<crate::rules::SubsumedRule>,
        selected: usize,
        accepted: Vec<usize>,
    },
    /// Read a cached message body (Triage leftovers or Review queue).
    MessageRead {
        message: CachedMessage,
        scroll: usize,
    },
}

struct UiSnapshot<'a> {
    tab: Tab,
    sender_groups: &'a [PendingSenderGroup],
    leftovers: &'a [CachedMessage],
    triage_focus: TriageFocus,
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
    rules_filter: ActionFilter,
    analytics: &'a AppliedAnalytics,
}

const SENDER_GROUP_LIMIT: usize = 500;
const UNREAD_KEPT_LIMIT: usize = 200;

struct LoopState {
    tab: Tab,
    selected: usize,
    rules_selected: usize,
    activity: Activity,
    auto_on: bool,
    show_help: bool,
    help_scroll: usize,
    overlay: OverlayMode,
    rules_filter: ActionFilter,
    analytics_period: AnalyticsPeriod,
    /// Remembers senders vs unread when both lists are non-empty.
    triage_focus: TriageFocus,
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
    let leftovers = store.unread_kept_messages(UNREAD_KEPT_LIMIT)?;
    let queue = store.review_queue(ctx.app.config.safety.review_threshold())?;
    let pending = store.pending_count(None)?;
    let queued = queue.len() as i64;
    let plan_total = store.pending_plan_message_count()?;
    let cached_total = store.total_count(None)?;
    let rules = ctx.app.config.rules.clone();
    let analytics = store
        .applied_analytics(state.analytics_period)
        .unwrap_or_else(|_| AppliedAnalytics {
            period: state.analytics_period,
            buckets: vec![],
            totals: Default::default(),
        });

    terminal.draw(|f| {
        draw_ui(
            f,
            ctx,
            &UiSnapshot {
                tab: state.tab,
                sender_groups: &sender_groups,
                leftovers: &leftovers,
                triage_focus: state.triage_focus,
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
                rules_filter: state.rules_filter,
                analytics: &analytics,
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
        activity: if ctx.app.config.sync.auto_process {
            Activity::AutoIdle
        } else {
            Activity::Ready
        },
        auto_on: ctx.app.config.sync.auto_process,
        show_help: false,
        help_scroll: 0,
        overlay: OverlayMode::None,
        rules_filter: ActionFilter::All,
        analytics_period: AnalyticsPeriod::Day,
        triage_focus: TriageFocus::Senders,
    };
    let mut scroll = ScrollStates {
        triage_table: TableState::default(),
        review_table: TableState::default(),
        rules_list: ListState::default(),
        audit_list: ListState::default(),
        suggest_list: ListState::default(),
        category_list: ListState::default(),
    };
    let mut last_auto = if state.auto_on {
        // Run first AUTO cycle soon after startup when preference is on.
        Instant::now() - Duration::from_secs(1)
    } else {
        Instant::now()
    };
    let poll = auto_interval(ctx);
    let poll_label = poll_label(&poll);
    let mut jobs = JobRunner::new();

    if let Ok(store) = Store::open(&ctx.app.db_path()) {
        store.reset_remaining_planned_if_no_pending_plan().ok();
    }

    loop {
        while let Some(ev) = jobs.try_recv() {
            apply_job_event(ctx, &mut state, &mut scroll, &mut jobs, &mut last_auto, ev);
        }

        let store = Store::open(&ctx.app.db_path())?;
        let sender_groups = store.pending_sender_groups(SENDER_GROUP_LIMIT)?;
        let leftovers = store.unread_kept_messages(UNREAD_KEPT_LIMIT)?;
        let queue = store.review_queue(ctx.app.config.safety.review_threshold())?;
        let pending = store.pending_count(None)?;
        let rules = ctx.app.config.rules.clone();
        let triage_focus = state
            .triage_focus
            .resolve(&sender_groups, &leftovers);
        state.triage_focus = triage_focus;
        let showing_unread = triage_focus.showing_unread(&sender_groups, &leftovers);

        if !jobs.is_busy()
            && state.tab == Tab::Triage
            && pending > 0
            && sender_groups.is_empty()
            && matches!(
                state.activity,
                Activity::Ready | Activity::AutoIdle | Activity::Success(_)
            )
        {
            state.activity =
                Activity::Success(format!("{pending} msgs pending — press x for AI classify"));
        }

        clamp_selection(
            state.tab,
            showing_unread,
            &sender_groups,
            &leftovers,
            &queue,
            &rules,
            state.rules_filter,
            &mut state.selected,
            &mut state.rules_selected,
        );

        redraw(&mut terminal, ctx, &state, &mut scroll, &poll_label)?;

        // Short poll while a job runs so keys stay responsive and we notice completion.
        let timeout = if jobs.is_busy() {
            Duration::from_millis(50)
        } else if state.auto_on {
            poll.saturating_sub(last_auto.elapsed())
                .min(Duration::from_millis(250))
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
                if matches!(state.overlay, OverlayMode::MessageRead { .. }) {
                    handle_message_read_key(
                        &mut terminal,
                        ctx,
                        &mut state,
                        &mut scroll,
                        &poll_label,
                        &mut jobs,
                        key.code,
                    );
                    continue;
                }
                if matches!(state.overlay, OverlayMode::PatternEdit { .. })
                    && is_ai_pattern_generate_key(key.code, key.modifiers)
                {
                    start_pattern_from_desc(ctx, &mut state, &mut jobs);
                    continue;
                }
                if !matches!(state.overlay, OverlayMode::None)
                    && handle_overlay_key(
                        &mut state.overlay,
                        key.code,
                        key.modifiers,
                        &store,
                        &sender_groups,
                        state.selected,
                        &mut state.rules_selected,
                        state.rules_filter,
                        ctx,
                        &mut state.activity,
                    )
                {
                    continue;
                }

                let sample_msg = if showing_unread {
                    None
                } else {
                    sample_message(&store, &sender_groups, state.selected)
                };
                let leftover_msg = if showing_unread {
                    leftovers.get(state.selected).cloned()
                } else {
                    None
                };
                let review_msg = queue.get(state.selected).cloned();
                let teach_msg = match state.tab {
                    Tab::Review => review_msg.as_ref(),
                    Tab::Triage if showing_unread => leftover_msg.as_ref(),
                    Tab::Triage => sample_msg.as_ref(),
                    _ => None,
                };

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
                        Tab::Triage if showing_unread && !leftovers.is_empty() => {
                            state.selected = (state.selected + 1).min(leftovers.len() - 1);
                        }
                        Tab::Triage if !sender_groups.is_empty() => {
                            state.selected = (state.selected + 1).min(sender_groups.len() - 1);
                        }
                        Tab::Review if !queue.is_empty() => {
                            state.selected = (state.selected + 1).min(queue.len() - 1);
                        }
                        Tab::Rules if !rules.is_empty() => {
                            let n = visible_rule_count(&rules, state.rules_filter);
                            if n > 0 {
                                state.rules_selected =
                                    (state.rules_selected + 1).min(n - 1);
                            }
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
                    KeyCode::Char('u')
                        if state.tab == Tab::Triage
                            && !sender_groups.is_empty()
                            && !leftovers.is_empty() =>
                    {
                        state.triage_focus = match state.triage_focus {
                            TriageFocus::Senders => TriageFocus::Unread,
                            TriageFocus::Unread => TriageFocus::Senders,
                        };
                        state.selected = 0;
                        state.activity = Activity::Success(match state.triage_focus {
                            TriageFocus::Unread => format!(
                                "Unread leftovers ({}) — m mark · M mark all · u back to senders",
                                leftovers.len()
                            ),
                            TriageFocus::Senders => format!(
                                "Pending senders ({}) — u for {} unread leftover{}",
                                sender_groups.len(),
                                leftovers.len(),
                                if leftovers.len() == 1 { "" } else { "s" }
                            ),
                        });
                    }
                    KeyCode::Enter if state.tab == Tab::Triage => {
                        // Unread leftovers or a sample from the selected sender group.
                        let message = leftover_msg.or_else(|| sample_msg.clone());
                        if let Some(message) = message {
                            state.overlay = OverlayMode::MessageRead {
                                message,
                                scroll: 0,
                            };
                        }
                    }
                    KeyCode::Enter if state.tab == Tab::Review && review_msg.is_some() => {
                        if let Some(message) = review_msg {
                            state.overlay = OverlayMode::MessageRead {
                                message,
                                scroll: 0,
                            };
                        }
                    }
                    KeyCode::Char('m') if state.tab == Tab::Triage => {
                        if let Some(msg) = leftover_msg.as_ref() {
                            state.overlay = OverlayMode::None;
                            match jobs::spawn_mark_read(
                                &mut jobs,
                                ctx,
                                msg.account_id.clone(),
                                msg.uid,
                            ) {
                                Ok(()) => {
                                    state.activity = Activity::Busy(format!(
                                        "Marking read · {} uid {}",
                                        msg.account_id, msg.uid
                                    ));
                                }
                                Err(msg) => state.activity = Activity::Success(msg),
                            }
                        } else if !leftovers.is_empty() {
                            state.activity = Activity::Success(format!(
                                "Press u for unread leftovers ({}), then m — or M to mark all",
                                leftovers.len()
                            ));
                        } else {
                            state.activity =
                                Activity::Success("No unread keep/flag mail to mark".into());
                        }
                    }
                    KeyCode::Char('M') if state.tab == Tab::Triage => {
                        if leftovers.is_empty() {
                            state.activity =
                                Activity::Success("No unread keep/flag mail to mark".into());
                        } else {
                            state.overlay = OverlayMode::None;
                            match jobs::spawn_mark_all_read(&mut jobs, ctx) {
                                Ok(()) => {
                                    state.activity = Activity::Busy(
                                        "Marking all unread keep/flag mail read".into(),
                                    );
                                }
                                Err(msg) => state.activity = Activity::Success(msg),
                            }
                        }
                    }
                    KeyCode::Char('.') if state.tab == Tab::Triage => {
                        state.analytics_period = state.analytics_period.next();
                        state.activity = Activity::Success(format!(
                            "Analytics: {}",
                            state.analytics_period.title()
                        ));
                    }
                    KeyCode::Char('A') => {
                        state.auto_on = !state.auto_on;
                        ctx.app.config.sync.auto_process = state.auto_on;
                        if let Err(e) = ctx.app.save_config() {
                            state.activity =
                                Activity::Error(format!("Could not save AUTO preference: {e}"));
                        } else if state.auto_on {
                            state.activity = Activity::AutoIdle;
                            last_auto = Instant::now() - poll;
                        } else {
                            state.activity = Activity::Ready;
                        }
                    }
                    KeyCode::Char('s') => {
                        state.overlay = OverlayMode::None;
                        match jobs::spawn_sync(&mut jobs, ctx) {
                            Ok(()) => state.activity = Activity::Busy("Syncing mail".into()),
                            Err(msg) => state.activity = Activity::Success(msg),
                        }
                    }
                    KeyCode::Char('x') if state.tab == Tab::Triage => {
                        state.overlay = OverlayMode::None;
                        match jobs::spawn_classify(&mut jobs, ctx) {
                            Ok(()) => state.activity = Activity::Busy("AI classifying".into()),
                            Err(msg) => state.activity = Activity::Success(msg),
                        }
                    }
                    KeyCode::Char('x') if state.tab == Tab::Rules => {
                        open_subsume_for_selected(&store, &rules, &mut state);
                    }
                    KeyCode::Char('X') if state.tab == Tab::Rules => {
                        match jobs::spawn_rules_audit(&mut jobs, ctx) {
                            Ok(()) => state.activity = Activity::Busy("Auditing rules".into()),
                            Err(msg) => state.activity = Activity::Success(msg),
                        }
                    }
                    KeyCode::Char('a')
                        if matches!(state.tab, Tab::Triage | Tab::Review) =>
                    {
                        match jobs::spawn_apply(&mut jobs, ctx) {
                            Ok(()) => state.activity = Activity::Applying,
                            Err(msg) => state.activity = Activity::Success(msg),
                        }
                    }
                    KeyCode::Char('r') if state.tab == Tab::Review => {
                        if let Some(msg) = teach_msg {
                            match store.reject_from_plan(&msg.account_id, msg.uid) {
                                Ok(_) => {
                                    state.activity = Activity::Success(format!(
                                        "Rejected — {} back to Triage (not applied)",
                                        short_addr(&msg.from_address)
                                    ));
                                }
                                Err(e) => {
                                    state.activity =
                                        Activity::Error(format!("Reject failed: {e}"));
                                }
                            }
                        }
                    }
                    KeyCode::Char('p') if state.tab == Tab::Triage && !sender_groups.is_empty() => {
                        if let Some(group) = sender_groups.get(state.selected) {
                            match jobs::spawn_pattern_suggest(
                                &mut jobs,
                                ctx,
                                group.from_address.clone(),
                                group.account_id.clone(),
                            ) {
                                Ok(()) => {
                                    state.activity = Activity::Busy(format!(
                                        "Suggesting patterns · {}",
                                        group.from_address
                                    ));
                                }
                                Err(msg) => state.activity = Activity::Success(msg),
                            }
                        }
                    }
                    KeyCode::Char('z') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Junk subject rule",
                                |c| process::teach_junk_message(c, msg, false),
                            );
                        }
                    }
                    KeyCode::Char('Z') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Junk sender rule",
                                |c| process::teach_junk_message(c, msg, true),
                            );
                        }
                    }
                    KeyCode::Char('/') if state.tab == Tab::Triage => {
                        let pattern_src = sample_msg.as_ref().or(leftover_msg.as_ref());
                        if let Some(msg) = pattern_src {
                            state.overlay = OverlayMode::PatternEdit {
                                buffer: subject_pattern_from(&msg.subject),
                                desc: String::new(),
                                focus: PatternEditFocus::Pattern,
                                context: PatternEditContext::TriageTeach,
                            };
                        }
                    }
                    KeyCode::Char('g') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Archive subject",
                                |c| process::teach_message_subject(
                                    c,
                                    msg,
                                    "archive",
                                    Some("newsletter"),
                                    2,
                                ),
                            );
                        }
                    }
                    KeyCode::Char('G') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Archive sender",
                                |c| process::teach_message_sender(
                                    c,
                                    msg,
                                    "archive",
                                    Some("newsletter"),
                                    2,
                                ),
                            );
                        }
                    }
                    KeyCode::Char('i') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Important subject",
                                |c| process::teach_message_subject(
                                    c,
                                    msg,
                                    "flag",
                                    Some("priority"),
                                    5,
                                ),
                            );
                        }
                    }
                    KeyCode::Char('I') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Important sender",
                                |c| process::teach_message_sender(
                                    c,
                                    msg,
                                    "flag",
                                    Some("priority"),
                                    5,
                                ),
                            );
                        }
                    }
                    KeyCode::Char('o') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Keep subject",
                                |c| process::teach_message_subject(
                                    c,
                                    msg,
                                    "keep",
                                    Some("personal"),
                                    4,
                                ),
                            );
                        }
                    }
                    KeyCode::Char('O') if matches!(state.tab, Tab::Triage | Tab::Review) => {
                        if let Some(msg) = teach_msg {
                            run_teach_with_busy(
                                &mut terminal,
                                ctx,
                                &mut state,
                                &mut scroll,
                                &poll_label,
                                "Keep sender",
                                |c| process::teach_message_sender(
                                    c,
                                    msg,
                                    "keep",
                                    Some("personal"),
                                    4,
                                ),
                            );
                        }
                    }
                    KeyCode::Char('c') if state.tab == Tab::Rules && !rules.is_empty() => {
                        let rule_index =
                            resolve_rule_index(&rules, state.rules_selected, state.rules_filter);
                        let selected = selected_category_index(&rules[rule_index]);
                        scroll.category_list = ListState::default();
                        state.overlay = OverlayMode::CategoryPick {
                            rule_index,
                            selected,
                        };
                    }
                    KeyCode::Char('e') if state.tab == Tab::Rules && !rules.is_empty() => {
                        let rule_index =
                            resolve_rule_index(&rules, state.rules_selected, state.rules_filter);
                        let rule = &rules[rule_index];
                        state.overlay = OverlayMode::PatternEdit {
                            buffer: rule.r#match.clone(),
                            desc: String::new(),
                            focus: PatternEditFocus::Pattern,
                            context: PatternEditContext::RulesEdit { index: rule_index },
                        };
                    }
                    KeyCode::Char('t') if state.tab == Tab::Rules && !rules.is_empty() => {
                        let rule_index =
                            resolve_rule_index(&rules, state.rules_selected, state.rules_filter);
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
                    KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace
                        if state.tab == Tab::Rules
                            && visible_rule_count(&rules, state.rules_filter) > 0 =>
                    {
                        let rule_index =
                            resolve_rule_index(&rules, state.rules_selected, state.rules_filter);
                        let pattern = rules
                            .get(rule_index)
                            .map(|r| r.r#match.clone())
                            .unwrap_or_default();
                        match remove_rule(ctx, rule_index) {
                            Ok(()) => {
                                let n = visible_rule_count(
                                    &ctx.app.config.rules,
                                    state.rules_filter,
                                );
                                if n == 0 {
                                    state.rules_selected = 0;
                                } else {
                                    state.rules_selected =
                                        state.rules_selected.min(n - 1);
                                }
                                state.activity = Activity::Success(format!(
                                    "Deleted rule: {pattern}"
                                ));
                            }
                            Err(e) => {
                                state.activity = Activity::Error(format!("Rule error: {e}"))
                            }
                        }
                    }
                    KeyCode::Char(c @ ('z' | 'g' | 'i' | 'o' | '0' | '*' | '.'))
                        if state.tab == Tab::Rules =>
                    {
                        if let Some(filter) = ActionFilter::from_key(c) {
                            state.rules_filter = filter;
                            state.rules_selected = 0;
                            state.activity = Activity::Success(format!(
                                "Rules filter: {}",
                                filter.label()
                            ));
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        } else if state.auto_on && !jobs.is_busy() && last_auto.elapsed() >= poll {
            match jobs::spawn_auto(&mut jobs, ctx) {
                Ok(()) => {
                    state.overlay = OverlayMode::None;
                    state.activity = Activity::Busy("AUTO sync".into());
                }
                Err(msg) => state.activity = Activity::Success(msg),
            }
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
                Tab::Triage if snap.pending > 0 && !snap.leftovers.is_empty() => {
                    format!(" ({}/{}✉)", snap.sender_groups.len(), snap.leftovers.len())
                }
                Tab::Triage if snap.pending > 0 => format!(" ({})", snap.sender_groups.len()),
                Tab::Triage if !snap.leftovers.is_empty() => {
                    format!(" ({}✉)", snap.leftovers.len())
                }
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
            snap.leftovers,
            snap.triage_focus.showing_unread(snap.sender_groups, snap.leftovers),
            snap.pending,
            snap.selected,
            &mut scroll.triage_table,
            snap.analytics,
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
            snap.rules_filter,
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
            OverlayMode::PatternEdit {
                buffer,
                desc,
                focus,
                context,
            } => {
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
                    centered_rect(74, 62, f.area()),
                    buffer,
                    desc,
                    *focus,
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
            OverlayMode::SubsumeAudit {
                keeper_pattern,
                candidates,
                selected,
                accepted,
                ..
            } => rule_overlays::render_subsume_audit(
                f,
                centered_rect(72, 75, f.area()),
                keeper_pattern,
                candidates,
                *selected,
                accepted,
                &mut scroll.audit_list,
            ),
            OverlayMode::MessageRead { message, scroll } => {
                message_view::render_message_read(
                    f,
                    centered_rect(86, 82, f.area()),
                    message,
                    *scroll,
                );
            }
            OverlayMode::None => {}
        }
    }
}

fn is_ai_pattern_generate_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    // F5 is reliable in every terminal (Ctrl combos are often eaten or remapped).
    if matches!(code, KeyCode::F(5)) {
        return true;
    }
    if !modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }
    matches!(
        code,
        KeyCode::Char('g')
            | KeyCode::Char('G')
            | KeyCode::Enter
            | KeyCode::Char('\n')
            | KeyCode::Char('j')
            | KeyCode::Char('J')
    )
}

fn start_pattern_from_desc(ctx: &CommandContext, state: &mut LoopState, jobs: &mut JobRunner) {
    let OverlayMode::PatternEdit { buffer, desc, focus, .. } = &state.overlay else {
        return;
    };
    if desc.trim().is_empty() {
        state.activity = Activity::Error(
            "Tab to description, type what to match, then press F5 to generate".into(),
        );
        return;
    }
    let description = desc.clone();
    let current = buffer.clone();
    let _ = focus;

    match jobs::spawn_pattern_from_desc(jobs, ctx, description, current) {
        Ok(()) => state.activity = Activity::Busy("Generating pattern".into()),
        Err(msg) => state.activity = Activity::Success(msg),
    }
}

fn open_subsume_for_selected(
    store: &Store,
    rules: &[RuleConfig],
    state: &mut LoopState,
) {
    if rules.is_empty() {
        state.activity = Activity::Error("No rules to audit".into());
        return;
    }
    let keeper_index = resolve_rule_index(rules, state.rules_selected, state.rules_filter);
    let covered = find_subsumed_rules(rules, keeper_index, store);
    if covered.is_empty() {
        state.activity = Activity::Success(format!(
            "No covered duplicates for {}",
            rules[keeper_index].r#match
        ));
        return;
    }
    let n = covered.len();
    state.overlay = OverlayMode::SubsumeAudit {
        keeper_pattern: rules[keeper_index].r#match.clone(),
        keeper_index,
        candidates: covered,
        selected: 0,
        accepted: (0..n).collect(),
    };
    state.activity = Activity::Success(format!(
        "Found {n} rule(s) covered by broader pattern — review to remove"
    ));
}

fn open_subsume_after_edit(
    overlay: &mut OverlayMode,
    activity: &mut Activity,
    ctx: &CommandContext,
    store: &Store,
    keeper_index: usize,
    label: &str,
) {
    let rules = &ctx.app.config.rules;
    if keeper_index >= rules.len() {
        *activity = Activity::Success(label.into());
        *overlay = OverlayMode::None;
        return;
    }
    let covered = find_subsumed_rules(rules, keeper_index, store);
    if covered.is_empty() {
        *activity = Activity::Success(format!(
            "{label}: {} — no covered duplicates",
            rules[keeper_index].r#match
        ));
        *overlay = OverlayMode::None;
        return;
    }
    let n = covered.len();
    *overlay = OverlayMode::SubsumeAudit {
        keeper_pattern: rules[keeper_index].r#match.clone(),
        keeper_index,
        candidates: covered,
        selected: 0,
        accepted: (0..n).collect(),
    };
    *activity = Activity::Success(format!(
        "{label} — {n} covered rule(s) to review (Space/a)"
    ));
}

fn sample_message(
    store: &Store,
    groups: &[PendingSenderGroup],
    selected: usize,
) -> Option<CachedMessage> {
    let group = groups.get(selected)?;
    store.get_message(group.sample_message_id).ok().flatten()
}

/// Paint busy status in the footer and keep the main UI visible (no full-screen modal).
fn show_busy(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
    label: &str,
) {
    state.activity = Activity::Busy(label.into());
    let _ = redraw(terminal, ctx, state, scroll, poll_label);
}

/// Show footer busy status, then run a sync teach without covering the UI.
fn run_teach_with_busy<F>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &mut CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
    label: &str,
    op: F,
) where
    F: FnOnce(&mut CommandContext) -> Result<TeachReport>,
{
    show_busy(
        terminal,
        ctx,
        state,
        scroll,
        poll_label,
        &format!("Teaching · {label}"),
    );
    apply_teach_activity(&mut state.activity, op(ctx), label);
}

fn handle_message_read_key(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ctx: &mut CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    poll_label: &str,
    jobs: &mut JobRunner,
    code: KeyCode,
) {
    let OverlayMode::MessageRead { message, scroll: body_scroll } = &state.overlay else {
        return;
    };
    let account_id = message.account_id.clone();
    let uid = message.uid;
    let msg = message.clone();
    let max_scroll = message_view::body_line_count(&msg).saturating_sub(1);

    match code {
        KeyCode::Esc => {
            state.overlay = OverlayMode::None;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let OverlayMode::MessageRead { scroll, .. } = &mut state.overlay {
                *scroll = (*scroll + 1).min(max_scroll);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let OverlayMode::MessageRead { scroll, .. } = &mut state.overlay {
                *scroll = scroll.saturating_sub(1);
            }
        }
        KeyCode::Char('m') => {
            let _ = body_scroll;
            state.overlay = OverlayMode::None;
            match jobs::spawn_mark_read(jobs, ctx, account_id.clone(), uid) {
                Ok(()) => {
                    state.activity =
                        Activity::Busy(format!("Marking read · {account_id} uid {uid}"));
                }
                Err(msg) => state.activity = Activity::Success(msg),
            }
        }
        KeyCode::Char('M') => {
            state.overlay = OverlayMode::None;
            match jobs::spawn_mark_all_read(jobs, ctx) {
                Ok(()) => {
                    state.activity =
                        Activity::Busy("Marking all unread keep/flag mail read".into());
                }
                Err(msg) => state.activity = Activity::Success(msg),
            }
        }
        KeyCode::Char('z') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Junk subject rule",
                |c| process::teach_junk_message(c, &msg, false),
            );
        }
        KeyCode::Char('Z') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Junk sender rule",
                |c| process::teach_junk_message(c, &msg, true),
            );
        }
        KeyCode::Char('g') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Archive subject",
                |c| process::teach_message_subject(c, &msg, "archive", Some("newsletter"), 2),
            );
        }
        KeyCode::Char('G') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Archive sender",
                |c| process::teach_message_sender(c, &msg, "archive", Some("newsletter"), 2),
            );
        }
        KeyCode::Char('i') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Flag subject",
                |c| process::teach_message_subject(c, &msg, "flag", Some("priority"), 5),
            );
        }
        KeyCode::Char('I') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Flag sender",
                |c| process::teach_message_sender(c, &msg, "flag", Some("priority"), 5),
            );
        }
        KeyCode::Char('o') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Keep subject",
                |c| process::teach_message_subject(c, &msg, "keep", Some("personal"), 4),
            );
        }
        KeyCode::Char('O') => {
            state.overlay = OverlayMode::None;
            run_teach_with_busy(
                terminal,
                ctx,
                state,
                scroll,
                poll_label,
                "Keep sender",
                |c| process::teach_message_sender(c, &msg, "keep", Some("personal"), 4),
            );
        }
        _ => {}
    }
}

fn short_addr(addr: &str) -> String {
    if addr.chars().count() <= 28 {
        addr.to_string()
    } else {
        format!("{}…", addr.chars().take(27).collect::<String>())
    }
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

fn apply_job_event(
    ctx: &mut CommandContext,
    state: &mut LoopState,
    scroll: &mut ScrollStates,
    jobs: &mut JobRunner,
    last_auto: &mut Instant,
    ev: JobEvent,
) {
    match ev {
        JobEvent::Progress(label) => {
            state.activity = Activity::Busy(label);
        }
        JobEvent::Done(result) => {
            jobs.clear_inflight();
            match result {
                JobResult::Sync { result, has_rules } => match result {
                    Ok(msg) => {
                        let hint = if has_rules {
                            " · press x to classify new mail against rules"
                        } else {
                            ""
                        };
                        state.activity = Activity::Success(format!("{msg}{hint}"));
                    }
                    Err(e) => state.activity = Activity::Error(format!("Sync failed: {e}")),
                },
                JobResult::Classify(result) => {
                    let _ = ctx.app.reload_config();
                    match result {
                        Ok(msg) => state.activity = Activity::Success(msg),
                        Err(e) => {
                            state.activity = Activity::Error(format!("Classify failed: {e}"))
                        }
                    }
                }
                JobResult::Apply(result) => match result {
                    Ok(msg) => state.activity = Activity::Success(msg),
                    Err(e) => state.activity = Activity::Error(format!("Apply failed: {e}")),
                },
                JobResult::Auto(result) => {
                    let _ = ctx.app.reload_config();
                    *last_auto = Instant::now();
                    match result {
                        Ok(msg) => {
                            if state.auto_on {
                                state.activity = Activity::AutoIdle;
                            } else {
                                state.activity = Activity::Success(msg);
                            }
                        }
                        Err(e) => state.activity = Activity::Error(e),
                    }
                }
                JobResult::MarkRead(result) => match result {
                    Ok(msg) => state.activity = Activity::Success(msg),
                    Err(e) => {
                        state.activity = Activity::Error(format!("Mark read failed: {e}"))
                    }
                },
                JobResult::MarkAllRead(result) => match result {
                    Ok(msg) => {
                        state.selected = 0;
                        state.activity = Activity::Success(msg);
                    }
                    Err(e) => {
                        state.activity = Activity::Error(format!("Mark all read failed: {e}"))
                    }
                },
                JobResult::RulesAudit(result) => match result {
                    Ok(plan) => {
                        if plan.suggestions.is_empty() {
                            state.activity = Activity::Success(plan.summary);
                        } else {
                            scroll.audit_list = ListState::default();
                            state.overlay = OverlayMode::RuleAudit {
                                selected: 0,
                                // Never pre-accept — mass-retire + Enter used to wipe rules.
                                accepted: Vec::new(),
                                plan,
                            };
                            state.activity =
                                Activity::Success("Review audit suggestions".into());
                        }
                    }
                    Err(e) => state.activity = Activity::Error(format!("Audit failed: {e}")),
                },
                JobResult::PatternSuggest(result) => match result {
                    Ok((items, summary)) => {
                        if items.is_empty() {
                            state.activity = Activity::Success(summary);
                        } else {
                            scroll.suggest_list = ListState::default();
                            state.overlay = OverlayMode::PatternSuggest {
                                items,
                                selected: 0,
                            };
                            state.activity =
                                Activity::Success("Pick a suggested pattern".into());
                        }
                    }
                    Err(e) => {
                        state.activity = Activity::Error(format!("Suggest failed: {e}"))
                    }
                },
                JobResult::PatternFromDesc(result) => match result {
                    Ok(pattern) => {
                        if let OverlayMode::PatternEdit {
                            buffer, focus, ..
                        } = &mut state.overlay
                        {
                            *buffer = pattern.clone();
                            *focus = PatternEditFocus::Pattern;
                        }
                        state.activity =
                            Activity::Success(format!("AI filled pattern: {pattern}"));
                    }
                    Err(e) => {
                        state.activity = Activity::Error(format!("Pattern AI failed: {e}"))
                    }
                },
            }
        }
    }
}

fn clamp_selection(
    tab: Tab,
    showing_unread: bool,
    groups: &[PendingSenderGroup],
    leftovers: &[CachedMessage],
    queue: &[CachedMessage],
    rules: &[RuleConfig],
    rules_filter: ActionFilter,
    selected: &mut usize,
    rules_selected: &mut usize,
) {
    match tab {
        Tab::Triage if showing_unread && !leftovers.is_empty() => {
            *selected = (*selected).min(leftovers.len() - 1);
        }
        Tab::Triage if !groups.is_empty() => {
            *selected = (*selected).min(groups.len() - 1);
        }
        Tab::Triage if !leftovers.is_empty() => {
            *selected = (*selected).min(leftovers.len() - 1);
        }
        Tab::Triage => {
            *selected = 0;
        }
        Tab::Review if !queue.is_empty() => {
            *selected = (*selected).min(queue.len() - 1);
        }
        Tab::Rules => {
            let n = visible_rule_count(rules, rules_filter);
            if n > 0 {
                *rules_selected = (*rules_selected).min(n - 1);
            } else {
                *rules_selected = 0;
            }
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
    modifiers: KeyModifiers,
    store: &Store,
    groups: &[PendingSenderGroup],
    selected: usize,
    rules_selected: &mut usize,
    rules_filter: ActionFilter,
    ctx: &mut CommandContext,
    activity: &mut Activity,
) -> bool {
    let msg = sample_message(store, groups, selected);
    match overlay {
        OverlayMode::None => return false,
        OverlayMode::PatternEdit {
            buffer,
            desc,
            focus,
            context,
        } => match code {
            KeyCode::Esc => *overlay = OverlayMode::None,
            KeyCode::Tab => {
                *focus = match *focus {
                    PatternEditFocus::Pattern => PatternEditFocus::Desc,
                    PatternEditFocus::Desc => PatternEditFocus::Pattern,
                };
            }
            KeyCode::Enter if !modifiers.contains(KeyModifiers::CONTROL) => match *focus {
                PatternEditFocus::Pattern => {
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
                PatternEditFocus::Desc if desc.len() < 800 => {
                    desc.push('\n');
                }
                PatternEditFocus::Desc => {}
            },
            // Swallow generate chords so they never type into the buffer.
            KeyCode::F(5) => {}
            KeyCode::Char(c)
                if modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(c, 'g' | 'G' | 'j' | 'J') => {}
            KeyCode::Backspace => match *focus {
                PatternEditFocus::Pattern => {
                    buffer.pop();
                }
                PatternEditFocus::Desc => {
                    desc.pop();
                }
            },
            KeyCode::Char(c) if !c.is_control() => match *focus {
                PatternEditFocus::Pattern if buffer.len() < 200 => buffer.push(c),
                PatternEditFocus::Desc if desc.len() < 800 => desc.push(c),
                _ => {}
            },
            _ => {}
        },
        OverlayMode::PatternAction { pattern, context } => {
            let pat = pattern.clone();
            let ctx_mode = *context;
            let edit_index = match ctx_mode {
                PatternEditContext::RulesEdit { index } => Some(index),
                PatternEditContext::TriageTeach => None,
            };
            let run = |overlay: &mut OverlayMode,
                       activity: &mut Activity,
                       ctx: &mut CommandContext,
                       store: &Store,
                       action: &str,
                       category: Option<&str>,
                       priority: u8| {
                let result = apply_rule_action(
                    ctx,
                    &ctx_mode,
                    &pat,
                    action,
                    category,
                    priority,
                    msg.as_ref(),
                );
                match result {
                    Ok(()) => {
                        if let Some(idx) = edit_index {
                            open_subsume_after_edit(overlay, activity, ctx, store, idx, "Rule saved");
                        } else {
                            *activity = Activity::Success(format!("Rule saved: {pat}"));
                            *overlay = OverlayMode::None;
                        }
                    }
                    Err(e) => {
                        *activity = Activity::Error(format!("Rule saved error: {e}"));
                        *overlay = OverlayMode::None;
                    }
                }
            };
            match code {
                KeyCode::Esc => *overlay = OverlayMode::None,
                KeyCode::Char('z') => run(
                    overlay,
                    activity,
                    ctx,
                    store,
                    "delete",
                    Some("spam"),
                    1,
                ),
                KeyCode::Char('g') => run(
                    overlay,
                    activity,
                    ctx,
                    store,
                    "archive",
                    Some("newsletter"),
                    2,
                ),
                KeyCode::Char('i') => {
                    run(overlay, activity, ctx, store, "flag", Some("priority"), 5)
                }
                KeyCode::Char('o') => {
                    run(overlay, activity, ctx, store, "keep", Some("personal"), 4)
                }
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
                    *activity = Activity::Error(
                        "No suggestions selected — Space to toggle, then a".into(),
                    );
                } else {
                    match apply_audit_suggestions(&ctx.app.config.rules, &to_apply) {
                        Ok(new_rules) => {
                            let before = ctx.app.config.rules.len();
                            let mut config = ctx.app.config.clone();
                            config.rules = new_rules;
                            match save_config_file(&ctx.app.config_path, &config) {
                                Ok(()) => {
                                    let after = config.rules.len();
                                    ctx.app.config = config;
                                    *activity = Activity::Success(format!(
                                        "Applied {} suggestions · rules {before} → {after}",
                                        to_apply.len()
                                    ));
                                }
                                Err(e) => {
                                    *activity = Activity::Error(format!("Save failed: {e}"))
                                }
                            }
                        }
                        Err(e) => {
                            *activity = Activity::Error(format!("Audit apply blocked: {e}"));
                        }
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
        OverlayMode::SubsumeAudit {
            keeper_index,
            keeper_pattern,
            candidates,
            selected,
            accepted,
        } => match code {
            KeyCode::Esc => {
                *activity = Activity::Success(format!(
                    "Kept all — broader rule saved: {keeper_pattern}"
                ));
                *overlay = OverlayMode::None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                *selected = (*selected + 1).min(candidates.len().saturating_sub(1));
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
                let to_remove: Vec<usize> = accepted
                    .iter()
                    .filter_map(|&i| candidates.get(i).map(|c| c.index))
                    .collect();
                if to_remove.is_empty() {
                    *activity = Activity::Error("No covered rules selected".into());
                    *overlay = OverlayMode::None;
                } else {
                    let keeper_pat = keeper_pattern.clone();
                    let mut config = ctx.app.config.clone();
                    remove_rules_at(&mut config.rules, &to_remove);
                    // Keeper index may have shifted after removals.
                    let new_keeper = config
                        .rules
                        .iter()
                        .position(|r| r.r#match == keeper_pat)
                        .unwrap_or(0);
                    match save_config_file(&ctx.app.config_path, &config) {
                        Ok(()) => {
                            ctx.app.config = config;
                            *rules_selected = visual_index_for_rule(
                                &ctx.app.config.rules,
                                new_keeper,
                                rules_filter,
                            );
                            *activity = Activity::Success(format!(
                                "Removed {} covered rule(s) under {keeper_pat}",
                                to_remove.len()
                            ));
                        }
                        Err(e) => *activity = Activity::Error(format!("Save failed: {e}")),
                    }
                    let _ = keeper_index;
                    *overlay = OverlayMode::None;
                }
            }
            _ => {}
        },
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
                            *rules_selected = visual_index_for_rule(
                                &ctx.app.config.rules,
                                *rule_index,
                                rules_filter,
                            );
                            *activity = Activity::Success(format!("Category → {label}"));
                        }
                        Err(e) => *activity = Activity::Error(format!("Category error: {e}")),
                    }
                    *overlay = OverlayMode::None;
                }
                _ => {}
            }
        }
        OverlayMode::MessageRead { .. } => {
            // Handled by handle_message_read_key (async mark-read / teach).
            return false;
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
