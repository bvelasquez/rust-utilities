/// iPhone photoreal mockup measurements (from app-store-screenshots skill).
pub const MK_W: f32 = 1022.0;
pub const MK_H: f32 = 2082.0;
pub const MK_RATIO: f32 = MK_W / MK_H;

/// Screen inset within mockup (fractions of mockup width/height).
/// Measured from opaque screen-fill pixels in `assets/mockup.png` (not the
/// smaller visible window from the Next.js skill — that leaves gaps when the
/// screenshot is drawn beneath the frame and screen-fill pixels are skipped).
pub const SC_LEFT: f32 = 20.0 / MK_W;
pub const SC_TOP: f32 = 14.0 / MK_H;
pub const SC_WIDTH: f32 = 982.0 / MK_W;
pub const SC_HEIGHT: f32 = 2054.0 / MK_H;
pub const SC_RX: f32 = 126.0 / 982.0;
pub const SC_RY: f32 = 126.0 / 2054.0;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct PhonePlacement {
    pub mockup: Rect,
    pub screen: Rect,
}

/// Width of phone mockup as fraction of canvas width (hero layout).
pub fn phone_width_fraction(canvas_w: u32, canvas_h: u32, clamp: f32) -> f32 {
    let cw = canvas_w as f32;
    let ch = canvas_h as f32;
    (0.72 * (ch / cw) * MK_RATIO).min(clamp)
}

/// Hero slide: phone sits below the caption block with a modest gap.
pub fn hero_phone_placement(canvas_w: u32, canvas_h: u32, caption_bottom: f32) -> PhonePlacement {
    let frac = phone_width_fraction(canvas_w, canvas_h, 0.84);
    let mock_w = (canvas_w as f32 * frac).round() as u32;
    let mock_h = (mock_w as f32 * (MK_H / MK_W)).round() as u32;

    let x = (canvas_w - mock_w) / 2;
    let gap = canvas_w as f32 * 0.015;
    let mut y = (caption_bottom + gap).round() as i32;

    // Slight bottom bleed (skill uses ~6% vs 13%) so the device feels grounded.
    let max_y = canvas_h as i32 - mock_h as i32 + (canvas_h as f32 * 0.06).round() as i32;
    if y > max_y {
        y = max_y;
    }
    let y = y.max(0) as u32;

    let screen = screen_rect_in_mockup(x, y, mock_w, mock_h);
    PhonePlacement {
        mockup: Rect {
            x,
            y,
            w: mock_w,
            h: mock_h,
        },
        screen,
    }
}

pub fn screen_rect_in_mockup(mx: u32, my: u32, mw: u32, mh: u32) -> Rect {
    Rect {
        x: mx + (mw as f32 * SC_LEFT).round() as u32,
        y: my + (mh as f32 * SC_TOP).round() as u32,
        w: (mw as f32 * SC_WIDTH).round() as u32,
        h: (mh as f32 * SC_HEIGHT).round() as u32,
    }
}
