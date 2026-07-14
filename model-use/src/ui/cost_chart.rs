use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line};
use ratatui::widgets::Block;
use ratatui::Frame;

use crate::aggregate::{Period, SummaryData, TimeSeriesPoint};
use crate::budget::prorated_budget;

use super::theme;

pub fn render_cost_chart(f: &mut Frame, area: Rect, summary: &SummaryData, period: Period) {
    if summary.series.is_empty() {
        f.render_widget(
            ratatui::widgets::Paragraph::new("No usage data — run `model-use fetch`")
                .style(theme::label_style())
                .block(theme::panel_block("Cost")),
            area,
        );
        return;
    }

    let points: Vec<(f64, f64)> = summary
        .series
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f64, p.cost_usd))
        .collect();

    let x_max = (points.len().saturating_sub(1)) as f64;
    let y_max = points
        .iter()
        .map(|(_, y)| *y)
        .fold(0.0f64, f64::max);
    let budget_line = summary
        .budgets
        .iter()
        .find(|b| b.label == "global")
        .and_then(|b| b.budget_usd)
        .map(|monthly| prorated_budget(monthly, period))
        .unwrap_or(0.0);
    let y_max = y_max.max(budget_line) * 1.1 + 0.01;

    let title = format!(
        " Cost · {} · total ${:.2} ",
        period.label(),
        summary.total_usd
    );
    let budget_label = if budget_line > 0.0 {
        format!(" budget ${budget_line:.2} ")
    } else {
        String::new()
    };

    let canvas = Canvas::default()
        .block(
            Block::default()
                .title(title)
                .title_bottom(budget_label)
                .title_style(theme::label_style().add_modifier(Modifier::ITALIC)),
        )
        .marker(Marker::Braille)
        .x_bounds([0.0, x_max.max(1.0)])
        .y_bounds([0.0, y_max])
        .paint(move |ctx| {
            if budget_line > 0.0 {
                ctx.draw(&Line::new(
                    0.0,
                    budget_line,
                    x_max.max(1.0),
                    budget_line,
                    theme::LOSS,
                ));
            }

            for window in points.windows(2) {
                let (x1, y1) = window[0];
                let (x2, y2) = window[1];
                let color = if budget_line > 0.0 && y2 > budget_line {
                    theme::LOSS
                } else if budget_line > 0.0 && y1 > budget_line {
                    theme::LOSS
                } else {
                    theme::ACCENT
                };
                ctx.draw(&Line::new(x1, y1, x2, y2, color));
            }

            for (x, y) in &points {
                if budget_line > 0.0 && *y > budget_line {
                    ctx.print(*x, *y, "●");
                }
            }
        });

    f.render_widget(canvas, area);
}

pub fn series_table_lines(summary: &SummaryData) -> Vec<String> {
    summary
        .series
        .iter()
        .rev()
        .take(8)
        .map(format_point)
        .collect()
}

fn format_point(p: &TimeSeriesPoint) -> String {
    format!(
        "{}  ${:.2}",
        p.start.format("%Y-%m-%d"),
        p.cost_usd
    )
}
