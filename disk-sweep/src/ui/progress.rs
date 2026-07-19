use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, Paragraph, Wrap};

use crate::watch_data::ScanUpdate;

use super::theme::{
    draw_modal_panel, fill_rects, modal_surface_style, muted_style, MODAL_SURFACE,
};

pub struct CleanProgressView {
    pub current: usize,
    pub total: usize,
    pub path: String,
    pub log: Vec<String>,
}

pub struct ProgressView {
    pub phase: String,
    pub detail: String,
    pub current: usize,
    pub total: usize,
    pub log: Vec<String>,
}

impl ProgressView {
    pub fn new(detail: &str, total: usize) -> Self {
        Self {
            phase: "Starting".into(),
            detail: detail.into(),
            current: 0,
            total,
            log: vec![],
        }
    }

    pub fn apply(&mut self, update: &ScanUpdate) {
        match update {
            ScanUpdate::Phase {
                phase,
                detail,
                current,
                total,
            } => {
                self.phase = (*phase).to_string();
                self.detail = detail.clone();
                self.current = *current;
                self.total = *total;
            }
            ScanUpdate::Log(line) => {
                self.log.push(line.clone());
                if self.log.len() > 8 {
                    self.log.remove(0);
                }
            }
            ScanUpdate::Failed(msg) => {
                self.phase = "Error".into();
                self.detail = msg.clone();
            }
            ScanUpdate::Snapshot(_) | ScanUpdate::Cancelled | ScanUpdate::AnalyzeComplete(_) => {}
        }
    }

    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.current as f64 / self.total as f64).clamp(0.0, 1.0)
    }
}

pub fn draw_progress_overlay(
    f: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    progress: &ProgressView,
    footer: &str,
) {
    let popup = centered_rect(72, 44, area);
    let inner_area = draw_modal_panel(f, popup, title, Color::Cyan);
    let text_style = modal_surface_style();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner_area);
    fill_rects(f, &chunks, modal_surface_style());

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Phase: ", muted_style()),
            Span::styled(&progress.phase, Style::default().add_modifier(Modifier::BOLD)),
        ]))
        .style(text_style),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(progress.detail.as_str())
            .style(text_style)
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new(format!("Step {} of {}", progress.current, progress.total)).style(text_style),
        chunks[2],
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .ratio(progress.ratio())
            .label(format!("{:.0}%", progress.ratio() * 100.0)),
        chunks[3],
    );

    let log_lines: Vec<Line> = progress
        .log
        .iter()
        .map(|l| Line::from(Span::styled(l.as_str(), muted_style())))
        .collect();
    f.render_widget(
        List::new(log_lines).block(
            Block::default()
                .title("Completed")
                .borders(ratatui::widgets::Borders::ALL)
                .style(modal_surface_style()),
        ),
        chunks[4],
    );
    f.render_widget(
        Paragraph::new(footer).style(muted_style().bg(MODAL_SURFACE)),
        chunks[5],
    );
}

pub fn draw_clean_overlay(f: &mut ratatui::Frame, area: Rect, progress: &CleanProgressView) {
    let popup = centered_rect(70, 40, area);
    let inner_area = draw_modal_panel(f, popup, "Cleaning", Color::Yellow);
    let text_style = modal_surface_style();

    let ratio = if progress.total > 0 {
        progress.current as f64 / progress.total as f64
    } else {
        0.0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .split(inner_area);
    fill_rects(f, &chunks, modal_surface_style());

    f.render_widget(
        Paragraph::new(format!(
            "Deleting item {} of {}",
            progress.current, progress.total
        ))
        .style(text_style),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(progress.path.as_str())
            .style(text_style)
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(format!("{:.0}%", ratio * 100.0)),
        chunks[2],
    );

    let log_lines: Vec<Line> = progress
        .log
        .iter()
        .map(|l| Line::from(Span::styled(l.as_str(), muted_style())))
        .collect();
    f.render_widget(
        List::new(log_lines).block(
            Block::default()
                .title("Log")
                .borders(ratatui::widgets::Borders::ALL)
                .style(modal_surface_style()),
        ),
        chunks[3],
    );
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
