use std::path::{Path, PathBuf};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use crate::analyze::project_root_candidates;

use super::progress::centered_rect;
use super::theme::{
    draw_modal_panel, fill_rects, highlight_style, modal_surface_style, muted_style,
};

pub struct ProjectRootPicker {
    pub roots: Vec<PathBuf>,
    pub state: ListState,
}

impl ProjectRootPicker {
    pub fn new(preferred: Option<&Path>) -> Self {
        let roots = project_root_candidates();
        let mut state = ListState::default();
        let select = preferred
            .and_then(|p| roots.iter().position(|r| r == p))
            .unwrap_or(0);
        state.select(Some(select.min(roots.len().saturating_sub(1))));
        Self { roots, state }
    }

    pub fn selected(&self) -> Option<&PathBuf> {
        let idx = self.state.selected()?;
        self.roots.get(idx)
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.roots.is_empty() {
            return;
        }
        let current = self.state.selected().unwrap_or(0);
        let max = self.roots.len().saturating_sub(1);
        let next = ((current as i32 + delta).clamp(0, max as i32)) as usize;
        self.state.select(Some(next));
    }
}

pub fn draw_project_picker(
    f: &mut ratatui::Frame,
    area: Rect,
    picker: &mut ProjectRootPicker,
) {
    let popup = centered_rect(68, 50, area);
    let inner = draw_modal_panel(f, popup, "Select projects root", Color::Cyan);
    fill_rects(f, &[inner], modal_surface_style());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(
            "Choose where stale projects and Rust build artifacts are scanned.\n\
             Enter to start analyze · Esc to cancel",
        )
        .style(modal_surface_style())
        .wrap(Wrap { trim: true }),
        chunks[0],
    );

    if picker.roots.is_empty() {
        f.render_widget(
            Paragraph::new("No project roots found.").style(modal_surface_style()),
            chunks[1],
        );
        return;
    }

    let items: Vec<ListItem> = picker
        .roots
        .iter()
        .map(|p| {
            let label = short_path(p);
            let exists = if p.is_dir() { "" } else { " (missing)" };
            ListItem::new(format!("{label}{exists}"))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(highlight_style())
        .highlight_symbol("› ")
        .style(modal_surface_style());
    f.render_stateful_widget(list, chunks[1], &mut picker.state);

    if let Some(sel) = picker.selected() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Selected: ", muted_style()),
                Span::styled(
                    short_path(sel),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]))
            .style(modal_surface_style()),
            chunks[2],
        );
    }
}

fn short_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}
