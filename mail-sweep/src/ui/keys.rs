use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::theme::{footer_block, key_style, label_style, MUTED};
use super::Tab;

/// Key hint rows inside the tab panel (tab-specific actions).
pub fn panel_key_rows(tab: Tab) -> Vec<Line<'static>> {
    match tab {
        Tab::Triage => vec![
            Line::from(vec![
                Span::styled("j/k", key_style()),
                Span::styled(" select  ", label_style()),
                Span::styled("Enter", key_style()),
                Span::styled(" read  ", label_style()),
                Span::styled("m", key_style()),
                Span::styled(" mark read  ", label_style()),
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" teach  ", label_style()),
                Span::styled("Z/G/I/O", key_style()),
                Span::styled(" sender", label_style()),
            ]),
            Line::from(vec![
                Span::styled("/", key_style()),
                Span::styled(" pattern  ", label_style()),
                Span::styled("p", key_style()),
                Span::styled(" AI suggest  ", label_style()),
                Span::styled("x", key_style()),
                Span::styled(" classify  ", label_style()),
                Span::styled("a", key_style()),
                Span::styled(" apply  ", label_style()),
                Span::styled("s", key_style()),
                Span::styled(" sync  ", label_style()),
                Span::styled(".", key_style()),
                Span::styled(" chart", label_style()),
            ]),
        ],
        Tab::Review => vec![
            Line::from(vec![
                Span::styled("j/k", key_style()),
                Span::styled(" select  ", label_style()),
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" correct  ", label_style()),
                Span::styled("Z/G/I/O", key_style()),
                Span::styled(" sender  ", label_style()),
                Span::styled("r", key_style()),
                Span::styled(" reject → Triage", label_style()),
            ]),
            Line::from(vec![
                Span::styled("a", key_style()),
                Span::styled(" apply full plan  ", label_style()),
                Span::styled("s", key_style()),
                Span::styled(" sync", label_style()),
            ]),
        ],
        Tab::Rules => vec![
            Line::from(vec![
                Span::styled("j/k", key_style()),
                Span::styled(" select  ", label_style()),
                Span::styled("d", key_style()),
                Span::styled(" delete rule  ", label_style()),
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" filter  ", label_style()),
                Span::styled("0", key_style()),
                Span::styled(" all", label_style()),
            ]),
            Line::from(vec![
                Span::styled("c", key_style()),
                Span::styled(" category  ", label_style()),
                Span::styled("e", key_style()),
                Span::styled(" edit  ", label_style()),
                Span::styled("x", key_style()),
                Span::styled(" covered  ", label_style()),
                Span::styled("X", key_style()),
                Span::styled(" AI audit  ", label_style()),
                Span::styled("t", key_style()),
                Span::styled(" test  ", label_style()),
                Span::styled("n", key_style()),
                Span::styled(" preset", label_style()),
            ]),
        ],
        Tab::Setup => vec![Line::from(vec![
            Span::styled("A", key_style()),
            Span::styled(" toggle AUTO  ", label_style()),
            Span::styled("s", key_style()),
            Span::styled(" sync now", label_style()),
        ])],
    }
}

pub fn panel_keys_height(tab: Tab) -> u16 {
    let rows = panel_key_rows(tab).len() as u16;
    if rows == 0 {
        0
    } else {
        // footer_block uses Borders::TOP — reserve one extra row so all key lines fit.
        rows + 1
    }
}

pub fn render_panel_keys(area: ratatui::layout::Rect, f: &mut ratatui::Frame, tab: Tab) {
    let lines = panel_key_rows(tab);
    if lines.is_empty() {
        return;
    }
    let block = footer_block();
    let inner = block.inner(area);
    f.render_widget(block, area);
    for (i, line) in lines.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        f.render_widget(
            ratatui::widgets::Paragraph::new(line.clone()),
            ratatui::layout::Rect {
                x: inner.x,
                y: inner.y + i as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
}

/// Footer rows below the status line (tab keys + global keys).
pub fn footer_key_row_count(tab: Tab) -> usize {
    match tab {
        Tab::Triage | Tab::Rules | Tab::Review => 2,
        Tab::Setup => 1,
    }
}

pub fn footer_height(tab: Tab) -> u16 {
    1 + footer_key_row_count(tab) as u16
}

pub fn keys_for_footer(tab: Tab, row: u8) -> Line<'static> {
    let global = Line::from(vec![
        Span::styled("A", key_style()),
        Span::styled(" auto  ", label_style()),
        Span::styled("?", key_style()),
        Span::styled(" help  ", label_style()),
        Span::styled("Tab", key_style()),
        Span::styled(" / ", label_style()),
        Span::styled("1-4", key_style()),
        Span::styled(" switch tab  ", label_style()),
        Span::styled("q", key_style()),
        Span::styled(" quit", label_style()),
    ]);

    match tab {
        Tab::Triage => match row {
            0 => Line::from(vec![
                Span::styled("Enter", key_style()),
                Span::styled(" read  ", label_style()),
                Span::styled("m", key_style()),
                Span::styled(" mark read  ", label_style()),
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" teach  ", label_style()),
                Span::styled("Z/G/I/O", key_style()),
                Span::styled(" sender", label_style()),
            ]),
            _ => global,
        },
        Tab::Review => match row {
            0 => Line::from(vec![
                Span::styled("Review", Style::default().fg(MUTED)),
                Span::styled(" — ", label_style()),
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" correct (saves rule) · ", label_style()),
                Span::styled("r", key_style()),
                Span::styled(" reject · ", label_style()),
                Span::styled("a", key_style()),
                Span::styled(" apply plan", label_style()),
            ]),
            _ => global,
        },
        Tab::Rules => match row {
            0 => Line::from(vec![
                Span::styled("Rules", Style::default().fg(MUTED)),
                Span::styled(" — ", label_style()),
                Span::styled("d", key_style()),
                Span::styled(" delete · ", label_style()),
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" filter · ", label_style()),
                Span::styled("0", key_style()),
                Span::styled(" all · ", label_style()),
                Span::styled("c", key_style()),
                Span::styled(" category · ", label_style()),
                Span::styled("x", key_style()),
                Span::styled(" covered · ", label_style()),
                Span::styled("X", key_style()),
                Span::styled(" AI audit", label_style()),
            ]),
            _ => global,
        },
        Tab::Setup => global,
    }
}
