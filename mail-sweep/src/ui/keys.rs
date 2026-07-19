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
                Span::styled("z/g/i/o", key_style()),
                Span::styled(" teach  ", label_style()),
                Span::styled("Z/G/I/O", key_style()),
                Span::styled(" whole sender  ", label_style()),
                Span::styled("/", key_style()),
                Span::styled(" pattern  ", label_style()),
                Span::styled("p", key_style()),
                Span::styled(" AI suggest  ", label_style()),
                Span::styled("x", key_style()),
                Span::styled(" classify", label_style()),
            ]),
            Line::from(vec![
                Span::styled("a", key_style()),
                Span::styled(" apply plan  ", label_style()),
                Span::styled("s", key_style()),
                Span::styled(" sync", label_style()),
            ]),
        ],
        Tab::Review => vec![Line::from(vec![
            Span::styled("j/k", key_style()),
            Span::styled(" inspect  ", label_style()),
            Span::styled("a", key_style()),
            Span::styled(" apply full plan  ", label_style()),
            Span::styled("s", key_style()),
            Span::styled(" sync", label_style()),
        ])],
        Tab::Rules => vec![Line::from(vec![
            Span::styled("j/k", key_style()),
            Span::styled(" select  ", label_style()),
            Span::styled("c", key_style()),
            Span::styled(" category  ", label_style()),
            Span::styled("e", key_style()),
            Span::styled(" edit  ", label_style()),
            Span::styled("t", key_style()),
            Span::styled(" test  ", label_style()),
            Span::styled("x", key_style()),
            Span::styled(" audit  ", label_style()),
            Span::styled("n", key_style()),
            Span::styled(" preset  ", label_style()),
            Span::styled("d", key_style()),
            Span::styled(" delete", label_style()),
        ])],
        Tab::Setup => vec![Line::from(vec![
            Span::styled("A", key_style()),
            Span::styled(" toggle AUTO  ", label_style()),
            Span::styled("s", key_style()),
            Span::styled(" sync now", label_style()),
        ])],
    }
}

pub fn panel_keys_height(tab: Tab) -> u16 {
    panel_key_rows(tab).len() as u16
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
        Tab::Triage | Tab::Rules => 2,
        Tab::Review | Tab::Setup => 1,
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
                Span::styled("z", key_style()),
                Span::styled(" junk subject  ", label_style()),
                Span::styled("Z", key_style()),
                Span::styled(" junk sender  ", label_style()),
                Span::styled("g", key_style()),
                Span::styled(" archive  ", label_style()),
                Span::styled("G", key_style()),
                Span::styled(" archive sender  ", label_style()),
                Span::styled("i", key_style()),
                Span::styled(" important  ", label_style()),
                Span::styled("o", key_style()),
                Span::styled(" keep", label_style()),
            ]),
            _ => global,
        },
        Tab::Review => match row {
            0 => Line::from(vec![
                Span::styled("Review", Style::default().fg(MUTED)),
                Span::styled(
                    " — deletes + low-confidence only · empty can still mean plan ready · ",
                    label_style(),
                ),
                Span::styled("a", key_style()),
                Span::styled(" applies entire pending plan", label_style()),
            ]),
            _ => global,
        },
        Tab::Rules => match row {
            0 => Line::from(vec![
                Span::styled("Rules", Style::default().fg(MUTED)),
                Span::styled(" — grouped by category · ", label_style()),
                Span::styled("c", key_style()),
                Span::styled(" recategorize · ", label_style()),
                Span::styled("x", key_style()),
                Span::styled(" AI audit", label_style()),
            ]),
            _ => global,
        },
        Tab::Setup => global,
    }
}
