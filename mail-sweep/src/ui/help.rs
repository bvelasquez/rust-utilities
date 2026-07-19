use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use super::theme::{modal_block, ACCENT, MUTED, OK};
use super::Tab;

pub fn help_line_count() -> usize {
    help_lines().len()
}

pub fn render_help(f: &mut Frame, area: Rect, scroll: usize) {
    f.render_widget(Clear, area);
    let block = modal_block(" mail-sweep — noise filter ", ACCENT);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = help_lines();
    let max_scroll = lines.len().saturating_sub(inner.height as usize);
    let scroll = scroll.min(max_scroll);
    let visible: Vec<Line> = lines.into_iter().skip(scroll).collect();

    f.render_widget(
        Paragraph::new(visible)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(Color::Rgb(28, 28, 38))),
        inner,
    );

    if max_scroll > 0 {
        let mut state = ScrollbarState::new(max_scroll + inner.height as usize).position(scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).thumb_symbol("█"),
            Rect {
                x: area.right().saturating_sub(1),
                y: inner.y,
                width: 1,
                height: inner.height,
            },
            &mut state,
        );
    }
}

fn help_lines() -> Vec<Line<'static>> {
    vec![
        heading("What this is"),
        plain("Not a Gmail reader — a noise filter. You teach rules + AI patterns,"),
        plain("then AUTO handles new mail so your real inbox stays clean."),
        blank(),
        heading("Workflow"),
        plain("  1. s — sync new/unread mail from Gmail"),
        plain("  2. Triage tab — teach rules per sender (z/g/i/o) or / for custom patterns"),
        plain("  3. x — AI proposes patterns for remaining unclassified senders"),
        plain("  4. a — apply plans (archive, delete, etc.) to Gmail"),
        plain("  5. A — AUTO: repeat sync → AI → safe apply on interval"),
        blank(),
        heading("Triage tab (unclassified senders only)"),
        plain("Teaching saves a rule + plans matching mail. Sender leaves Triage when all"),
        plain("its messages are planned. Subject/pattern rules may not cover every message."),
        key("j/k", "Select sender group"),
        key("z / Z", "Junk — subject / whole sender (delete)"),
        key("g / G", "Archive subject / sender"),
        key("i / I", "Important subject / sender"),
        key("o / O", "Keep subject / sender"),
        key("/", "Custom pattern editor (regex OK in subject:)"),
        key("p", "AI suggest richer patterns for sender"),
        key("x", "AI classify next batch of senders"),
        key("a", "Apply latest plan"),
        key("s", "Sync now"),
        blank(),
        heading("Review tab"),
        plain("Risky/uncertain plans land here. Taught/high-confidence actions may leave"),
        plain("this list empty while a pending plan still exists — press a to apply all."),
        plain("  • All planned deletes"),
        plain("  • Archive/flag/keep below confidence threshold"),
        key("j/k", "Inspect queued messages"),
        key("a", "Apply full pending plan to Gmail"),
        key("s", "Sync mail now"),
        blank(),
        heading("Global keys (every tab)"),
        key("A", "Toggle AUTO — sync, AI classify, safe apply on interval"),
        key("?", "This help overlay"),
        key("Tab / 1-4", "Switch tabs"),
        key("q / Esc", "Quit"),
        blank(),
        heading("Rules tab"),
        plain("Rules are grouped by category. Editing a rule also re-plans matching mail."),
        key("c", "Change category for selected rule"),
        key("e", "Edit selected rule pattern (regex supported)"),
        key("t", "Test rule against cached mail"),
        key("x", "AI audit — merge similar rules, suggest generalizations"),
        key("n", "Add newsletter preset"),
        key("d", "Delete selected rule"),
        blank(),
        heading("Pattern grammar"),
        plain("  subject:…  from:…  domain:…  body:…  header:Name"),
        plain("  has:list-unsubscribe  all:domain:x.com+subject:y"),
        blank(),
        heading("Setup tab"),
        plain("AUTO applies archive/mark-read automatically. Deletes always need Review."),
        key("A", "Toggle AUTO"),
        key("s", "Sync mail now"),
        blank(),
        Line::from(Span::styled("j/k scroll · ? Esc close", Style::default().fg(MUTED))),
    ]
}

pub fn tab_hint(tab: Tab) -> Line<'static> {
    let text = match tab {
        Tab::Triage => "Unclassified senders — teach, p for AI pattern ideas, x to classify rest",
        Tab::Review => "Deletes + low-confidence plans — j/k inspect, a applies full plan",
        Tab::Rules => "Rules grouped by category — c to recategorize · x for AI audit",
        Tab::Setup => "Enable AUTO to stop babysitting new mail",
    };
    Line::from(vec![
        Span::styled(" ⓘ ", Style::default().fg(ACCENT)),
        Span::styled(text, Style::default().fg(MUTED)),
    ])
}

fn heading(s: &'static str) -> Line<'static> {
    Line::from(Span::styled(s, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
}

fn plain(s: &'static str) -> Line<'static> {
    Line::from(s)
}

fn key(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:<14}"), Style::default().fg(OK).add_modifier(Modifier::BOLD)),
        Span::raw(desc),
    ])
}

fn blank() -> Line<'static> {
    Line::from("")
}
