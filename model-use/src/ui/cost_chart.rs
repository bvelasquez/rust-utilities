use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Widget};
use ratatui::Frame;

use crate::aggregate::{Period, SummaryData, TimeSeriesPoint};
use crate::budget::prorated_budget;
use crate::providers::types::Provider;

use super::theme;

const BAR_SYMBOL: &str = "█";

fn period_label(point: &TimeSeriesPoint, period: Period) -> String {
    match period {
        Period::Day => point.start.format("%m/%d").to_string(),
        Period::Week => point.start.format("%m/%d").to_string(),
        Period::Month => point.start.format("%b").to_string(),
    }
}

fn short_cost(usd: f64) -> String {
    if usd >= 100.0 {
        format!("${:.0}", usd)
    } else if usd >= 10.0 {
        format!("${:.0}", usd)
    } else {
        format!("${:.1}", usd)
    }
}

fn bar_width_for_count(count: usize, area_width: u16) -> u16 {
    if count == 0 {
        return 1;
    }
    let inner = area_width.saturating_sub(2);
    let n = count as u16;
    let slot = inner / n;
    slot.saturating_sub(1).clamp(1, 4)
}

fn provider_budget_lines(
    summary: &SummaryData,
    providers: &[Provider],
    period: Period,
) -> Vec<(Provider, f64)> {
    providers
        .iter()
        .filter_map(|provider| {
            summary
                .budgets
                .iter()
                .find(|b| b.label == provider.to_string())
                .and_then(|b| b.budget_usd)
                .filter(|monthly| *monthly > 0.0)
                .map(|monthly| (*provider, prorated_budget(monthly, period)))
        })
        .collect()
}

fn usd_to_row(usd: f64, scale_usd: f64, bars_bottom: u16, bars_height: u16) -> Option<u16> {
    if usd <= 0.0 || scale_usd <= 0.0 || bars_height == 0 {
        return None;
    }
    let rows = ((usd / scale_usd) * f64::from(bars_height))
        .round()
        .clamp(0.0, f64::from(bars_height)) as u16;
    Some(bars_bottom.saturating_sub(rows))
}

fn draw_budget_line(buf: &mut Buffer, x: u16, width: u16, y: u16, color: ratatui::style::Color) {
    if width == 0 || y < buf.area.top() || y >= buf.area.bottom() {
        return;
    }
    for dx in 0..width {
        let col = x + dx;
        if col >= buf.area.right() {
            break;
        }
        buf[(col, y)].set_symbol("─").set_fg(color);
    }
}

/// Providers sorted by total spend (for consistent stacking order and legend).
fn provider_order(summary: &SummaryData) -> Vec<Provider> {
    if !summary.by_provider.is_empty() {
        return summary.by_provider.iter().map(|(p, _)| *p).collect();
    }
    Provider::all().to_vec()
}

fn ordered_segments<'a>(
    point: &'a TimeSeriesPoint,
    order: &[Provider],
) -> Vec<(Provider, f64)> {
    order
        .iter()
        .filter_map(|provider| {
            point
                .by_provider
                .iter()
                .find(|(p, _)| p == provider)
                .map(|(_, cost)| (*provider, *cost))
        })
        .filter(|(_, cost)| *cost > 0.0)
        .collect()
}

/// Split `total_rows` across segments proportionally (largest-remainder method).
fn allocate_rows(total_rows: u16, segments: &[(Provider, f64)]) -> Vec<(Provider, u16)> {
    if total_rows == 0 || segments.is_empty() {
        return vec![];
    }
    let total_cost: f64 = segments.iter().map(|(_, c)| c).sum();
    if total_cost <= 0.0 {
        return vec![];
    }

    let mut alloc: Vec<(Provider, u16, f64, usize)> = segments
        .iter()
        .enumerate()
        .map(|(idx, (provider, cost))| {
            let exact = f64::from(total_rows) * (cost / total_cost);
            let floor = exact.floor() as u16;
            (*provider, floor, exact - f64::from(floor), idx)
        })
        .collect();

    let assigned: u16 = alloc.iter().map(|(_, rows, _, _)| *rows).sum();
    let mut remainder = total_rows.saturating_sub(assigned);
    let mut by_remainder: Vec<usize> = (0..alloc.len()).collect();
    by_remainder.sort_by(|&a, &b| {
        alloc[b]
            .2
            .partial_cmp(&alloc[a].2)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for &idx in &by_remainder {
        if remainder == 0 {
            break;
        }
        alloc[idx].1 += 1;
        remainder -= 1;
    }

    alloc.sort_by_key(|(_, _, _, idx)| *idx);
    alloc
        .into_iter()
        .filter(|(_, rows, _, _)| *rows > 0)
        .map(|(provider, rows, _, _)| (provider, rows))
        .collect()
}

fn draw_stacked_bar(
    buf: &mut Buffer,
    x: u16,
    width: u16,
    bottom_y: u16,
    total_rows: u16,
    segments: &[(Provider, f64)],
) {
    if total_rows == 0 {
        return;
    }

    let stacks = if segments.is_empty() {
        vec![(Provider::Openrouter, total_rows)]
    } else {
        allocate_rows(total_rows, segments)
    };

    let mut y = bottom_y;
    for (provider, rows) in stacks {
        let color = theme::provider_color(provider);
        for _ in 0..rows {
            if y < buf.area.top() {
                break;
            }
            for dx in 0..width {
                let cell = &mut buf[(x + dx, y)];
                cell.set_symbol(BAR_SYMBOL);
                cell.set_fg(color);
            }
            y = y.saturating_sub(1);
        }
    }
}

fn draw_centered_text(buf: &mut Buffer, x: u16, width: u16, y: u16, text: &str, style: Style) {
    if width == 0 || y >= buf.area.bottom() {
        return;
    }
    let text_width = text.chars().count() as u16;
    let start = if text_width >= width {
        x
    } else {
        x + (width - text_width) / 2
    };
    for (i, ch) in text.chars().enumerate() {
        let col = start + i as u16;
        if col >= x + width {
            break;
        }
        let cell = &mut buf[(col, y)];
        cell.set_symbol(ch.to_string().as_str());
        cell.set_style(style);
    }
}

fn render_legend(buf: &mut Buffer, area: Rect, providers: &[Provider]) {
    if providers.is_empty() || area.width < 4 {
        return;
    }

    let mut x = area.x;
    for (i, provider) in providers.iter().enumerate() {
        if i > 0 {
            if x + 1 >= area.right() {
                break;
            }
            buf[(x, area.y)].set_symbol("·").set_style(theme::label_style());
            x += 2;
        }

        let label = theme::provider_short_name(*provider);
        let color = theme::provider_color(*provider);
        let needed = 1 + label.len() as u16;
        if x + needed > area.right() {
            break;
        }

        buf[(x, area.y)]
            .set_symbol(BAR_SYMBOL)
            .set_fg(color);
        x += 1;

        for (j, ch) in label.chars().enumerate() {
            buf[(x + j as u16, area.y)]
                .set_symbol(ch.to_string().as_str())
                .set_style(theme::label_style());
        }
        x += label.len() as u16;
    }
}

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

    let budget_usd = summary
        .budgets
        .iter()
        .find(|b| b.label == "global")
        .and_then(|b| b.budget_usd)
        .map(|monthly| prorated_budget(monthly, period));

    let max_cost = summary
        .series
        .iter()
        .map(|p| p.cost_usd)
        .fold(0.0f64, f64::max);

    let providers = provider_order(summary);
    let provider_budgets = provider_budget_lines(summary, &providers, period);
    let max_provider_budget = provider_budgets
        .iter()
        .map(|(_, b)| *b)
        .fold(0.0f64, f64::max);

    let scale_usd = budget_usd
        .filter(|b| *b > 0.0)
        .unwrap_or(0.0)
        .max(max_cost)
        .max(max_provider_budget)
        .max(0.01);

    let title = format!(
        " Cost · {} · total ${:.2} ",
        period.label(),
        summary.total_usd
    );
    let budget_label = budget_usd
        .filter(|b| *b > 0.0)
        .map(|b| format!(" budget ${b:.2}/{} ", period.label()))
        .unwrap_or_default();

    let block = Block::default()
        .title(title)
        .title_bottom(budget_label)
        .title_style(theme::label_style().add_modifier(Modifier::ITALIC))
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(ratatui::style::Color::Rgb(60, 66, 82)));

    let inner = block.inner(area);
    block.render(area, f.buffer_mut());

    if inner.height < 4 || inner.width < 4 {
        return;
    }

    let has_legend = providers.len() > 1;
    let legend_rows = u16::from(has_legend);
    let label_row = inner.bottom() - 1;
    let value_row = inner.bottom().saturating_sub(2);
    let bars_bottom = value_row.saturating_sub(1);
    let bars_top = inner.y + legend_rows;
    let bars_height = bars_bottom.saturating_sub(bars_top) + 1;

    if has_legend {
        let legend_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        render_legend(f.buffer_mut(), legend_area, &providers);
    }

    for (provider, budget) in &provider_budgets {
        if let Some(y) = usd_to_row(*budget, scale_usd, bars_bottom, bars_height) {
            draw_budget_line(
                f.buffer_mut(),
                inner.x,
                inner.width,
                y,
                theme::provider_color(*provider),
            );
        }
    }

    let bar_width = bar_width_for_count(summary.series.len(), inner.width);
    let bar_gap = 1u16;
    let label_style = theme::label_style();
    let value_style = theme::label_style();

    for (i, point) in summary.series.iter().enumerate() {
        let x = inner.x + (i as u16) * (bar_width + bar_gap);
        if x + bar_width > inner.right() {
            break;
        }

        let bar_rows = if point.cost_usd <= 0.0 {
            0
        } else {
            ((point.cost_usd / scale_usd) * f64::from(bars_height))
                .round()
                .clamp(1.0, f64::from(bars_height)) as u16
        };

        let segments = ordered_segments(point, &providers);
        draw_stacked_bar(
            f.buffer_mut(),
            x,
            bar_width,
            bars_bottom,
            bar_rows,
            &segments,
        );

        draw_centered_text(
            f.buffer_mut(),
            x,
            bar_width,
            label_row,
            &period_label(point, period),
            label_style,
        );

        if point.cost_usd > 0.0 {
            draw_centered_text(
                f.buffer_mut(),
                x,
                bar_width,
                value_row,
                &short_cost(point.cost_usd),
                value_style,
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn period_labels_vary_by_view() {
        let day = Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap();
        let point = TimeSeriesPoint {
            start: day,
            end: day,
            cost_usd: 1.0,
            by_provider: vec![],
        };
        assert_eq!(period_label(&point, Period::Month), "Mar");
        assert_eq!(period_label(&point, Period::Day), "03/15");
    }

    #[test]
    fn allocate_rows_splits_proportionally() {
        let segments = [
            (Provider::Openrouter, 60.0),
            (Provider::Anthropic, 40.0),
        ];
        let rows = allocate_rows(10, &segments);
        let or_rows = rows
            .iter()
            .find(|(p, _)| *p == Provider::Openrouter)
            .map(|(_, r)| *r)
            .unwrap_or(0);
        let an_rows = rows
            .iter()
            .find(|(p, _)| *p == Provider::Anthropic)
            .map(|(_, r)| *r)
            .unwrap_or(0);
        assert_eq!(or_rows, 6);
        assert_eq!(an_rows, 4);
    }

    #[test]
    fn allocate_rows_preserves_total() {
        let segments = [
            (Provider::Openrouter, 33.0),
            (Provider::Anthropic, 33.0),
            (Provider::Openai, 34.0),
        ];
        let rows = allocate_rows(7, &segments);
        let total: u16 = rows.iter().map(|(_, r)| *r).sum();
        assert_eq!(total, 7);
    }

    #[test]
    fn ordered_segments_follow_provider_order() {
        let point = TimeSeriesPoint {
            start: Utc::now(),
            end: Utc::now(),
            cost_usd: 10.0,
            by_provider: vec![
                (Provider::Openai, 3.0),
                (Provider::Anthropic, 7.0),
            ],
        };
        let order = vec![Provider::Anthropic, Provider::Openai];
        let segments = ordered_segments(&point, &order);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].0, Provider::Anthropic);
        assert_eq!(segments[1].0, Provider::Openai);
    }

    #[test]
    fn usd_to_row_maps_budget_to_chart_position() {
        assert_eq!(usd_to_row(50.0, 100.0, 10, 8), Some(6));
        assert_eq!(usd_to_row(100.0, 100.0, 10, 8), Some(2));
        assert_eq!(usd_to_row(0.0, 100.0, 10, 8), None);
    }
}
