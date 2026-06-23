#[derive(Debug, Clone, Copy)]
pub struct ExportSize {
    pub label: &'static str,
    pub w: u32,
    pub h: u32,
}

/// Design canvas — largest required iPhone size (6.9").
pub const IPHONE_DESIGN_W: u32 = 1320;
pub const IPHONE_DESIGN_H: u32 = 2868;

pub const IPHONE_SIZES: &[ExportSize] = &[
    ExportSize {
        label: "6.9\"",
        w: 1320,
        h: 2868,
    },
    ExportSize {
        label: "6.5\"",
        w: 1284,
        h: 2778,
    },
    ExportSize {
        label: "6.3\"",
        w: 1206,
        h: 2622,
    },
    ExportSize {
        label: "6.1\"",
        w: 1125,
        h: 2436,
    },
];
