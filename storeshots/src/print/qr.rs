use anyhow::Result;
use image::{Rgba, RgbaImage};
use qrcode::QrCode;

pub fn render_qr(url: &str, module_px: u32) -> Result<RgbaImage> {
    render_qr_quiet(url, module_px, 4)
}

pub fn render_qr_quiet(url: &str, module_px: u32, quiet: usize) -> Result<RgbaImage> {
    let code = QrCode::new(url.as_bytes())?;
    let modules = code.width();
    let dim = modules + quiet * 2;
    let size = dim as u32 * module_px;
    let mut img = RgbaImage::new(size, size);
    for px in img.pixels_mut() {
        *px = Rgba([255, 255, 255, 255]);
    }
    for my in 0..modules {
        for mx in 0..modules {
            if code[(mx, my)] == qrcode::Color::Dark {
                let x0 = ((mx + quiet) as u32) * module_px;
                let y0 = ((my + quiet) as u32) * module_px;
                for dy in 0..module_px {
                    for dx in 0..module_px {
                        img.put_pixel(x0 + dx, y0 + dy, Rgba([0, 0, 0, 255]));
                    }
                }
            }
        }
    }
    Ok(img)
}
