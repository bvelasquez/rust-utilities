use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Gauge;

use crate::budget::BudgetStatus;

use super::theme;

pub fn budget_gauge_over(status: &BudgetStatus) -> Gauge<'static> {
    let ratio = status.ratio.unwrap_or(0.0);
    let display = if ratio > 1.0 { 1.0 } else { ratio.clamp(0.0, 1.0) };
    let budget = status
        .budget_usd
        .map(|b| format!("${b:.0}"))
        .unwrap_or_else(|| "—".into());
    let over = if status.over_budget { " OVER" } else { "" };
    Gauge::default()
        .gauge_style(
            Style::default()
                .fg(theme::budget_gauge_fill_color(&status.label, ratio))
                .bg(Color::DarkGray)
                .add_modifier(if status.over_budget {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )
        .ratio(display)
        .label(format!(
            "{} ${:.2}/{}{}",
            status.label, status.spent_usd, budget, over
        ))
}
