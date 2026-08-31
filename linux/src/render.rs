use crate::core::Snapshot;
use fontdue::{Font, FontSettings};
use std::sync::OnceLock;

pub struct Fonts {
    pub bold: Option<Font>,
    pub regular: Option<Font>,
}

static FONTS: OnceLock<Fonts> = OnceLock::new();

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Bold.ttf",
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/TTF/LiberationSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

fn load_font(paths: &[&str]) -> Option<Font> {
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return Some(font);
            }
        }
    }
    None
}

pub fn fonts() -> &'static Fonts {
    FONTS.get_or_init(|| Fonts {
        bold: load_font(&[
            FONT_CANDIDATES[0],
            FONT_CANDIDATES[2],
            FONT_CANDIDATES[4],
            FONT_CANDIDATES[5],
            FONT_CANDIDATES[6],
            FONT_CANDIDATES[7],
            FONT_CANDIDATES[8],
        ]),
        regular: load_font(&[
            FONT_CANDIDATES[1],
            FONT_CANDIDATES[3],
            FONT_CANDIDATES[5],
            FONT_CANDIDATES[7],
            FONT_CANDIDATES[8],
        ]),
    })
}

#[derive(Clone, Copy)]
struct RectI {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl RectI {
    fn width(&self) -> i32 {
        self.right - self.left
    }

    fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// Sets a pixel with Porter-Duff Over alpha blending into premultiplied ARGB8888 buffer.
fn set_pixel(
    buf: &mut [u32],
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    if x < 0 || y < 0 || x >= width || y >= height || a == 0 {
        return;
    }
    let idx = (y as usize) * (width as usize) + x as usize;
    if idx >= buf.len() {
        return;
    }
    let sa = a as u32;
    let sr = (r as u32 * sa + 127) / 255;
    let sg = (g as u32 * sa + 127) / 255;
    let sb = (b as u32 * sa + 127) / 255;

    if a == 255 {
        buf[idx] = (255 << 24) | (sr << 16) | (sg << 8) | sb;
        return;
    }

    let bg = buf[idx];
    let da = (bg >> 24) & 0xff;
    let dr = (bg >> 16) & 0xff;
    let dg = (bg >> 8) & 0xff;
    let db = bg & 0xff;

    let inv = 255 - sa;
    let out_a = sa + (da * inv + 127) / 255;
    let out_r = sr + (dr * inv + 127) / 255;
    let out_g = sg + (dg * inv + 127) / 255;
    let out_b = sb + (db * inv + 127) / 255;

    buf[idx] = (out_a << 24) | (out_r.min(255) << 16) | (out_g.min(255) << 8) | out_b.min(255);
}

fn fill_rect(
    buf: &mut [u32],
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    for y in top.max(0)..bottom.min(height) {
        for x in left.max(0)..right.min(width) {
            set_pixel(buf, width, height, x, y, r, g, b, a);
        }
    }
}

fn fill_rounded_rect(
    buf: &mut [u32],
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    radius: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    let rad = radius.max(0).min((right - left) / 2).min((bottom - top) / 2);
    for y in top.max(0)..bottom.min(height) {
        for x in left.max(0)..right.min(width) {
            let in_corner = if x < left + rad && y < top + rad {
                let dx = x - (left + rad);
                let dy = y - (top + rad);
                dx * dx + dy * dy > rad * rad
            } else if x >= right - rad && y < top + rad {
                let dx = x - (right - rad - 1);
                let dy = y - (top + rad);
                dx * dx + dy * dy > rad * rad
            } else if x < left + rad && y >= bottom - rad {
                let dx = x - (left + rad);
                let dy = y - (bottom - rad - 1);
                dx * dx + dy * dy > rad * rad
            } else if x >= right - rad && y >= bottom - rad {
                let dx = x - (right - rad - 1);
                let dy = y - (bottom - rad - 1);
                dx * dx + dy * dy > rad * rad
            } else {
                false
            };
            if !in_corner {
                set_pixel(buf, width, height, x, y, r, g, b, a);
            }
        }
    }
}

fn draw_rect_outline(
    buf: &mut [u32],
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    for x in left.max(0)..right.min(width) {
        set_pixel(buf, width, height, x, top, r, g, b, a);
        set_pixel(buf, width, height, x, bottom - 1, r, g, b, a);
    }
    for y in top.max(0)..bottom.min(height) {
        set_pixel(buf, width, height, left, y, r, g, b, a);
        set_pixel(buf, width, height, right - 1, y, r, g, b, a);
    }
}

fn draw_line(
    buf: &mut [u32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        set_pixel(buf, width, height, x, y, r, g, b, a);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn draw_circle(
    buf: &mut [u32],
    width: i32,
    height: i32,
    cx: i32,
    cy: i32,
    radius: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= radius * radius {
                set_pixel(buf, width, height, cx + x, cy + y, r, g, b, a);
            }
        }
    }
}

fn draw_ring(
    buf: &mut [u32],
    width: i32,
    height: i32,
    cx: i32,
    cy: i32,
    radius: i32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    for y in -radius..=radius {
        for x in -radius..=radius {
            let d = x * x + y * y;
            if d <= radius * radius && d >= (radius - 2) * (radius - 2) {
                set_pixel(buf, width, height, cx + x, cy + y, r, g, b, a);
            }
        }
    }
}

fn draw_text_with(
    font: &Font,
    buf: &mut [u32],
    width: i32,
    height: i32,
    text: &str,
    x: i32,
    baseline_y: i32,
    px: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    let mut pen_x = x as f32;
    for ch in text.chars() {
        let (m, cov) = font.rasterize(ch, px);
        if m.height > 0 && m.width > 0 {
            let glyph_left = pen_x as i32 + m.xmin;
            let glyph_top = baseline_y - (m.ymin + m.height as i32);
            for row in 0..m.height {
                for col in 0..m.width {
                    let coverage = cov[row * m.width + col];
                    if coverage == 0 {
                        continue;
                    }
                    let alpha = ((coverage as u32) * (a as u32) / 255) as u8;
                    set_pixel(
                        buf,
                        width,
                        height,
                        glyph_left + col as i32,
                        glyph_top + row as i32,
                        r,
                        g,
                        b,
                        alpha,
                    );
                }
            }
        }
        pen_x += m.advance_width;
    }
}

fn text_width(font: &Font, text: &str, px: f32) -> f32 {
    let mut w = 0.0;
    for ch in text.chars() {
        let (m, _) = font.rasterize(ch, px);
        w += m.advance_width;
    }
    w
}

fn baseline_for_center(font: &Font, center_y: i32, px: f32) -> i32 {
    match font.horizontal_line_metrics(px) {
        Some(lm) => center_y + ((lm.ascent + lm.descent) / 2.0) as i32,
        None => center_y,
    }
}

/// Renders one overlay buffer (whole monitor, buffer-pixel coordinates are
/// logical coordinates scaled by `scale`).
pub fn render_snapshot(
    data: &mut [u32],
    buf_w: i32,
    buf_h: i32,
    scale: i32,
    monitor_origin: (f64, f64),
    snapshot: &Snapshot,
) {
    let s = scale as f64;
    let mx = monitor_origin.0;
    let my = monitor_origin.1;

    let dim_alpha = if snapshot.scroll_mode {
        0
    } else {
        (snapshot.opacity * 0.48 * 255.0) as u8
    };
    let dim_pixel = (dim_alpha as u32) << 24;
    for p in data.iter_mut() {
        *p = dim_pixel;
    }

    let local = |r: &crate::common::Rect| RectI {
        left: ((r.left - mx) * s).round() as i32,
        top: ((r.top - my) * s).round() as i32,
        right: ((r.right - mx) * s).round() as i32,
        bottom: ((r.bottom - my) * s).round() as i32,
    };

    let region = local(&snapshot.active_region);
    if snapshot.scroll_mode {
        if snapshot.active_region.intersects(&active_monitor_rect(snapshot, mx, my, buf_w, buf_h, s))
        {
            draw_rect_outline(data, buf_w, buf_h, region.left, region.top, region.right, region.bottom, 0, 178, 178, 240);
            if region.width() > 2 && region.height() > 2 {
                draw_rect_outline(data, buf_w, buf_h, region.left + 1, region.top + 1, region.right - 1, region.bottom - 1, 0, 178, 178, 240);
            }
        }
    } else if snapshot.active_region.intersects(&active_monitor_rect(snapshot, mx, my, buf_w, buf_h, s)) {
        let region_alpha = (snapshot.opacity * 0.16 * 255.0) as u8;
        fill_rect(data, buf_w, buf_h, region.left, region.top, region.right, region.bottom, 0, 0, 0, region_alpha);

        if snapshot.precision_mode {
            draw_precision(data, buf_w, buf_h, snapshot, region, scale);
        } else {
            draw_grid(data, buf_w, buf_h, snapshot, region, s);
        }
    }

    // Virtual cursor indicator (only in non-precision mode; precision mode has its own crosshair in the magnified box)
    if !snapshot.precision_mode {
        let (cx, cy) = snapshot.cursor;
        if cx >= mx && cx <= mx + buf_w as f64 / s && cy >= my && cy <= my + buf_h as f64 / s {
            let lx = ((cx - mx) * s).round() as i32;
            let ly = ((cy - my) * s).round() as i32;
            let radius = if snapshot.dragging { 9 * scale } else { 7 * scale };
            let (cr, cg, cb) = if snapshot.dragging { (255, 165, 0) } else { (0, 200, 80) };
            draw_circle(data, buf_w, buf_h, lx, ly, radius, cr, cg, cb, 255);
            draw_ring(data, buf_w, buf_h, lx, ly, radius + 4 * scale, 255, 255, 255, 230);
        }
    }

    draw_status(data, buf_w, buf_h, snapshot, scale);
}

/// Reconstructs this monitor's rect from the snapshot-independent info we
/// pass along (origin + logical size) — used for intersects() tests.
fn active_monitor_rect(
    _snapshot: &Snapshot,
    mx: f64,
    my: f64,
    buf_w: i32,
    buf_h: i32,
    s: f64,
) -> crate::common::Rect {
    crate::common::Rect::new(mx, my, mx + buf_w as f64 / s, my + buf_h as f64 / s)
}

fn draw_grid(data: &mut [u32], width: i32, height: i32, snapshot: &Snapshot, region: RectI, _s: f64) {
    let cols = snapshot.columns as usize;
    let rows = snapshot.rows as usize;
    let cw = region.width() as f64 / cols as f64;
    let ch = region.height() as f64 / rows as f64;

    for row in 0..rows {
        for col in 0..cols {
            let index = row * cols + col;
            if index >= snapshot.labels.len() {
                continue;
            }
            let cell = RectI {
                left: (region.left as f64 + col as f64 * cw) as i32,
                top: (region.top as f64 + row as f64 * ch) as i32,
                right: (region.left as f64 + (col + 1) as f64 * cw) as i32,
                bottom: (region.top as f64 + (row + 1) as f64 * ch) as i32,
            };
            fill_rect(data, width, height, cell.left, cell.top, cell.right, cell.bottom, 0, 178, 178, 60);
            draw_rect_outline(data, width, height, cell.left, cell.top, cell.right, cell.bottom, 0, 178, 178, 210);
            draw_label(data, width, height, &snapshot.labels[index], &cell);
        }
    }
}

fn draw_label(data: &mut [u32], width: i32, height: i32, label: &str, cell: &RectI) {
    let cell_w = cell.width() as f64;
    let cell_h = cell.height() as f64;
    let shortest = cell_w.min(cell_h);
    if shortest < 9.0 {
        return;
    }
    let font_px = (shortest * 0.48).clamp(7.0, 42.0);
    let Some(font) = fonts().bold.as_ref().or(fonts().regular.as_ref()) else {
        return;
    };
    let center_x = cell.left + cell.width() / 2;
    let center_y = cell.top + cell.height() / 2;
    let tw = text_width(font, label, font_px as f32) as i32;
    let baseline = baseline_for_center(font, center_y, font_px as f32);

    let pad_x = (font_px * 0.28).max(2.0) as i32;
    let pad_y = (font_px * 0.14).max(1.0) as i32;
    let bg_left = center_x - tw / 2 - pad_x;
    let bg_right = center_x + tw / 2 + pad_x;
    let bg_top = center_y - (font_px * 0.5) as i32 - pad_y;
    let bg_bottom = center_y + (font_px * 0.5) as i32 + pad_y;
    fill_rounded_rect(
        data,
        width,
        height,
        bg_left,
        bg_top,
        bg_right,
        bg_bottom,
        pad_x.min(4),
        0,
        0,
        0,
        77, // 0.30 opacity
    );

    draw_text_with(
        font,
        data,
        width,
        height,
        label,
        center_x - tw / 2,
        baseline,
        font_px as f32,
        255,
        255,
        255,
        255,
    );
}

fn draw_precision(
    data: &mut [u32],
    width: i32,
    height: i32,
    snapshot: &Snapshot,
    region: RectI,
    scale: i32,
) {
    // Slightly larger magnified grid for third-level readability — inspired by
    // mouseless.click "Zoom on subgrid" (#53). Keeps all labels (characters)
    // but makes each magnified cell ~24×18px (vs 20×15) so the third-grid
    // targets like Chromium's back button are easier to distinguish.
    // Still clamped to screen size, with the same positioning logic as macOS.
    let box_w = ((width as f64 - 48.0 * scale as f64)
        .min((snapshot.columns as f64) * 24.0 * scale as f64)
        .max(120.0 * scale as f64)) as i32;
    let box_h = ((height as f64 - 96.0 * scale as f64)
        .min((snapshot.rows as f64) * 18.0 * scale as f64)
        .max(90.0 * scale as f64)) as i32;
    let mut bx = region.left + region.width() / 2 - box_w / 2;
    let mut by = region.top + region.height() / 2 - box_h / 2;
    bx = bx.clamp(24 * scale, width - box_w - 24 * scale);
    by = by.clamp(24 * scale, height - box_h - 58 * scale);

    // Yellow outline around active target region on screen
    draw_rect_outline(data, width, height, region.left, region.top, region.right, region.bottom, 255, 215, 0, 230);

    // Dark rounded background for magnified precision grid
    fill_rounded_rect(data, width, height, bx - 8 * scale, by - 8 * scale, bx + box_w + 8 * scale, by + box_h + 8 * scale, 8 * scale, 0, 0, 0, 150);

    // Connector line from target region to magnified grid
    draw_line(
        data,
        width,
        height,
        region.left + region.width() / 2,
        region.top + region.height() / 2,
        bx + box_w / 2,
        by + box_h / 2,
        255,
        215,
        0,
        153,
    );

    let mag = RectI {
        left: bx,
        top: by,
        right: bx + box_w,
        bottom: by + box_h,
    };
    draw_grid(data, width, height, snapshot, mag, scale as f64);
    draw_precision_cursor(data, width, height, snapshot, mag);
}

fn draw_precision_cursor(
    data: &mut [u32],
    width: i32,
    height: i32,
    snapshot: &Snapshot,
    mag: RectI,
) {
    if snapshot.active_region.width() <= 0.0 || snapshot.active_region.height() <= 0.0 {
        return;
    }
    let norm_x = ((snapshot.cursor.0 - snapshot.active_region.left) / snapshot.active_region.width()).clamp(0.0, 1.0);
    let norm_y = ((snapshot.cursor.1 - snapshot.active_region.top) / snapshot.active_region.height()).clamp(0.0, 1.0);
    let px = mag.left + (norm_x * mag.width() as f64).round() as i32;
    let py = mag.top + (norm_y * mag.height() as f64).round() as i32;

    // Pink center dot
    draw_circle(data, width, height, px, py, 4, 255, 105, 180, 255);

    // White crosshair
    draw_line(data, width, height, px - 10, py, px + 10, py, 255, 255, 255, 242);
    draw_line(data, width, height, px, py - 10, px, py + 10, 255, 255, 255, 242);
}

fn draw_status(data: &mut [u32], width: i32, height: i32, snapshot: &Snapshot, scale: i32) {
    let mode = if snapshot.continuous_mode { "Persist" } else { "Once" };
    let drag = if snapshot.dragging { "  Dragging" } else { "" };
    let text = format!("Mouseless  {}  {}  {}", mode, drag, snapshot.status);
    let px = 14.0 * scale as f32;
    let Some(font) = fonts().regular.as_ref().or(fonts().bold.as_ref()) else {
        return;
    };
    let bx = 14 * scale;
    let by = height - 36 * scale;
    let tw = text_width(font, &text, px) as i32;
    let bw = tw.clamp(120 * scale, 900 * scale);
    fill_rounded_rect(data, width, height, bx, by, bx + bw + 8 * scale, by + 24 * scale, 4 * scale, 0, 0, 0, 135);
    let baseline = baseline_for_center(font, by + 12 * scale, px);
    draw_text_with(font, data, width, height, &text, bx + 4 * scale, baseline, px, 255, 255, 255, 255);
}
