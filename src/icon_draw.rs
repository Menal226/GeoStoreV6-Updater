// Shared by build.rs (via include!) to produce the .ico and by the app for the window icon.

pub fn render_icon(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let s = size as f32;
    let fill = [0x1D as f32, 0x5F as f32, 0xD8 as f32];
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut bg = 0u32;
            let mut fg = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / s;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / s;
                    if in_rounded_square(fx, fy, 0.22) {
                        bg += 1;
                        if in_glyph(fx, fy) {
                            fg += 1;
                        }
                    }
                }
            }
            let n = (SS * SS) as f32;
            let f = if bg > 0 { fg as f32 / bg as f32 } else { 0.0 };
            let i = ((y * size + x) * 4) as usize;
            for c in 0..3 {
                px[i + c] = (fill[c] * (1.0 - f) + 255.0 * f).round() as u8;
            }
            px[i + 3] = ((bg as f32 / n) * 255.0).round() as u8;
        }
    }
    px
}

fn in_rounded_square(x: f32, y: f32, r: f32) -> bool {
    let cx = x.clamp(r, 1.0 - r);
    let cy = y.clamp(r, 1.0 - r);
    let dx = x - cx;
    let dy = y - cy;
    (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) && dx * dx + dy * dy <= r * r
}

fn in_glyph(x: f32, y: f32) -> bool {
    let d = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
    let ring = (0.205..=0.285).contains(&d);
    let dot = d <= 0.085;
    let tick = |a: f32, b: f32| (a - 0.5).abs() <= 0.03 && (0.10..=0.20).contains(&b) || (a - 0.5).abs() <= 0.03 && (0.80..=0.90).contains(&b);
    ring || dot || tick(x, y) || tick(y, x)
}
