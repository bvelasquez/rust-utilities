use crate::config::StoreshotsConfig;

#[derive(Debug, Clone, Copy)]
pub struct PrintLayout {
    pub dpi: u32,
    pub export_scale: u32,
}

impl PrintLayout {
    pub fn from_config(cfg: &StoreshotsConfig) -> Self {
        Self {
            dpi: cfg.print.dpi.max(72),
            export_scale: cfg.print.export_scale.max(1),
        }
    }

    pub fn px(&self, inches: f64) -> u32 {
        (inches * self.dpi as f64).round() as u32
    }

    pub fn landscape_spread(&self) -> (u32, u32) {
        (self.px(11.0), self.px(8.5))
    }

    pub fn portrait_sheet(&self) -> (u32, u32) {
        (self.px(8.5), self.px(11.0))
    }

    pub fn business_card_bleed(&self, w_in: f64, h_in: f64) -> (u32, u32) {
        (self.px(w_in), self.px(h_in))
    }
}
