/// iPhone photoreal mockup measurements (from app-store-screenshots skill).
pub const MK_W: f32 = 1022.0;
pub const MK_H: f32 = 2082.0;
pub const MK_RATIO: f32 = MK_W / MK_H;

/// Screen inset within mockup (fractions of mockup width/height).
pub const SC_LEFT: f32 = 52.0 / MK_W;
pub const SC_TOP: f32 = 46.0 / MK_H;
pub const SC_WIDTH: f32 = 918.0 / MK_W;
pub const SC_HEIGHT: f32 = 1990.0 / MK_H;

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

/// Hero slide: centered phone, bottom-anchored with slight downward offset.
pub fn hero_phone_placement(canvas_w: u32, canvas_h: u32) -> PhonePlacement {
    let frac = phone_width_fraction(canvas_w, canvas_h, 0.84);
    let mock_w = (canvas_w as f32 * frac).round() as u32;
    let mock_h = (mock_w as f32 * (MK_H / MK_W)).round() as u32;

    let x = (canvas_w - mock_w) / 2;
    let y_offset = (canvas_h as f32 * 0.13).round() as u32;
    let y = canvas_h.saturating_sub(mock_h).saturating_add(y_offset);

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
