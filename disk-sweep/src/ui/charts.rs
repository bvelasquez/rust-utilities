use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

/// Minimal sparkline for usage ratio history (0.0–1.0).
pub struct Sparkline<'a> {
    values: &'a [f64],
    color: Color,
}

impl<'a> Sparkline<'a> {
    pub fn new(values: &'a [f64]) -> Self {
        Self {
            values,
            color: Color::Cyan,
        }
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

const BARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

impl Widget for Sparkline<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let n = self.values.len();
        if n == 0 {
            return;
        }

        let width = area.width as usize;
        let step = (n as f64 / width as f64).max(1.0);

        for x in 0..width {
            let idx = ((x as f64) * step) as usize;
            let idx = idx.min(n - 1);
            let v = self.values[idx].clamp(0.0, 1.0);
            let bar_idx = ((v * (BARS.len() - 1) as f64).round() as usize).min(BARS.len() - 1);
            let ch = BARS[bar_idx];
            buf[(area.x + x as u16, area.y)].set_char(ch).set_style(
                Style::default().fg(self.color),
            );
        }
    }
}

pub fn usage_color(ratio: f64) -> Color {
    if ratio >= 0.9 {
        Color::Red
    } else if ratio >= 0.75 {
        Color::Yellow
    } else {
        Color::Green
    }
}

pub fn bar_color(idx: usize) -> Color {
    const PALETTE: [Color; 6] = [
        Color::Cyan,
        Color::Magenta,
        Color::Blue,
        Color::Yellow,
        Color::Green,
        Color::LightRed,
    ];
    PALETTE[idx % PALETTE.len()]
}
