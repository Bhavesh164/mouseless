use crate::common::rgb;
use crate::common::Rect;
use crate::core::Snapshot;
use std::ptr::null_mut;
use std::sync::OnceLock;
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HBITMAP,
    HBRUSH, HDC, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HCURSOR, HICON, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOW, ULW_ALPHA, WNDCLASSW, WNDCLASS_STYLES, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

fn class_name() -> windows::core::PCWSTR {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    let v = NAME.get_or_init(|| {
        "MouselessOverlay"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    });
    windows::core::PCWSTR::from_raw(v.as_ptr())
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct PerMonitorWindow {
    hwnd: HWND,
    monitor: Rect,
    width: i32,
    height: i32,
    hdc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
}

impl Drop for PerMonitorWindow {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_invalid() {
                let _ = DestroyWindow(self.hwnd);
            }
            if !self.bitmap.is_invalid() {
                let _ = DeleteObject(self.bitmap);
            }
            if !self.hdc.is_invalid() {
                let _ = DeleteDC(self.hdc);
            }
        }
    }
}

unsafe impl Send for PerMonitorWindow {}
unsafe impl Sync for PerMonitorWindow {}

unsafe impl Send for OverlayWindows {}
unsafe impl Sync for OverlayWindows {}

pub struct OverlayWindows {
    windows: Vec<PerMonitorWindow>,
    class_registered: bool,
    instance: HINSTANCE,
}

extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl OverlayWindows {
    pub fn new() -> Self {
        let instance = HINSTANCE(
            unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
                .unwrap_or_default()
                .0,
        );
        OverlayWindows {
            windows: Vec::new(),
            class_registered: false,
            instance,
        }
    }

    fn register_class(&mut self) {
        if self.class_registered {
            return;
        }
        unsafe {
            let wc = WNDCLASSW {
                style: WNDCLASS_STYLES(0),
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: self.instance,
                hIcon: HICON::default(),
                hCursor: HCURSOR::default(),
                hbrBackground: HBRUSH::default(),
                lpszMenuName: windows::core::PCWSTR::null(),
                lpszClassName: class_name(),
            };
            let _ = RegisterClassW(&wc);
        }
        self.class_registered = true;
    }

    fn ensure_windows(&mut self, monitors: &[Rect]) {
        if self.windows.len() == monitors.len()
            && self
                .windows
                .iter()
                .zip(monitors)
                .all(|(w, m)| w.monitor == *m)
        {
            return;
        }
        self.windows.clear();
        self.register_class();
        for m in monitors {
            if let Some(win) = self.create_window(*m) {
                self.windows.push(win);
            }
        }
    }

    fn create_window(&self, monitor: Rect) -> Option<PerMonitorWindow> {
        let width = (monitor.width()).round() as i32;
        let height = (monitor.height()).round() as i32;
        let width = width.max(1);
        let height = height.max(1);
        unsafe {
            let hwnd = match CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                class_name(),
                windows::core::PCWSTR::null(),
                WS_POPUP,
                monitor.left as i32,
                monitor.top as i32,
                width,
                height,
                None,
                None,
                self.instance,
                None,
            ) {
                Ok(h) => h,
                Err(_) => return None,
            };
            if hwnd.is_invalid() {
                return None;
            }
            let screen_dc = GetDC(HWND::default());
            let hdc = CreateCompatibleDC(screen_dc);
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(HWND::default(), screen_dc);
            if hdc.is_invalid() {
                let _ = DestroyWindow(hwnd);
                return None;
            }
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = null_mut();
            let bitmap =
                match CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, HANDLE(null_mut()), 0)
                {
                    Ok(b) => b,
                    Err(_) => {
                        let _ = DeleteDC(hdc);
                        let _ = DestroyWindow(hwnd);
                        return None;
                    }
                };
            let _ = SelectObject(hdc, bitmap);
            Some(PerMonitorWindow {
                hwnd,
                monitor,
                width,
                height,
                hdc,
                bitmap,
                bits: bits as *mut u32,
            })
        }
    }

    pub fn show(&mut self, monitors: &[Rect]) {
        self.ensure_windows(monitors);
        for w in &self.windows {
            unsafe {
                // Taskbar windows can be topmost too. Reassert the z-order
                // when every overlay is shown so the full grid covers the
                // taskbar instead of appearing behind it.
                let _ = SetWindowPos(
                    w.hwnd,
                    HWND_TOPMOST,
                    w.monitor.left as i32,
                    w.monitor.top as i32,
                    w.width,
                    w.height,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(w.hwnd, SW_SHOW);
            }
        }
    }

    pub fn hide(&mut self) {
        for w in &self.windows {
            unsafe {
                ShowWindow(w.hwnd, SW_HIDE);
            }
        }
    }

    pub fn render(&self, snapshot: &Snapshot, monitors: &[Rect]) {
        for w in &self.windows {
            if w.monitor.intersects(&snapshot.active_region)
                || w.monitor.contains(snapshot.cursor.0, snapshot.cursor.1)
                || !snapshot.status.is_empty()
            {
                self.render_window(w, snapshot);
            }
        }
        let _ = monitors;
    }

    fn render_window(&self, w: &PerMonitorWindow, snapshot: &Snapshot) {
        let buf = w.bits;
        let width = w.width as usize;
        let height = w.height as usize;
        if buf.is_null() {
            return;
        }
        let dim_alpha = if snapshot.scroll_mode {
            0
        } else {
            (snapshot.opacity * 0.48 * 255.0) as u8
        };
        let dim_pixel = (dim_alpha as u32) << 24;
        unsafe {
            let slice = std::slice::from_raw_parts_mut(buf, width * height);
            for p in slice.iter_mut() {
                *p = dim_pixel;
            }
        }

        let local_region = local_rect(snapshot.active_region, w.monitor);
        if snapshot.scroll_mode {
            if snapshot.active_region.intersects(&w.monitor) {
                let r = local_region;
                draw_rect_outline(
                    buf, w.width, w.height, r.left, r.top, r.right, r.bottom, 0, 178, 178, 240,
                );
                if r.width() > 2 && r.height() > 2 {
                    draw_rect_outline(
                        buf,
                        w.width,
                        w.height,
                        r.left + 1,
                        r.top + 1,
                        r.right - 1,
                        r.bottom - 1,
                        0,
                        178,
                        178,
                        240,
                    );
                }
            }
        } else if snapshot.active_region.intersects(&w.monitor) {
            let region_alpha = (snapshot.opacity * 0.16 * 255.0) as u8;
            let r = local_region;
            fill_rect(
                buf,
                w.width,
                w.height,
                r.left,
                r.top,
                r.right,
                r.bottom,
                0,
                0,
                0,
                region_alpha,
            );

            if snapshot.precision_mode {
                self.draw_precision(w, snapshot, local_region);
            } else {
                self.draw_grid(w, snapshot, local_region);
            }
        }

        let (cx, cy) = snapshot.cursor;
        if w.monitor.contains(cx, cy) {
            let (lx, ly) = (
                (cx - w.monitor.left).round() as i32,
                (cy - w.monitor.top).round() as i32,
            );
            let radius = if snapshot.dragging { 9 } else { 7 };
            let (cr, cg, cb) = if snapshot.dragging {
                (255, 165, 0)
            } else {
                (0, 200, 80)
            };
            draw_circle(buf, w.width, w.height, lx, ly, radius, cr, cg, cb, 255);
            draw_ring(
                buf,
                w.width,
                w.height,
                lx,
                ly,
                radius + 4,
                255,
                255,
                255,
                230,
            );
        }

        self.draw_status(w, snapshot);

        unsafe {
            let slice = std::slice::from_raw_parts_mut(buf, width * height);
            for p in slice.iter_mut() {
                let a = (*p >> 24) & 0xff;
                let rgb = *p & 0x00ff_ffff;
                if a == 0 && rgb != 0 {
                    *p = rgb | (255u32 << 24);
                }
            }
        }

        unsafe {
            let point_dst = POINT {
                x: w.monitor.left as i32,
                y: w.monitor.top as i32,
            };
            let size = SIZE {
                cx: w.width,
                cy: w.height,
            };
            let pt_src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                w.hwnd,
                HDC::default(),
                Some(&point_dst),
                Some(&size),
                w.hdc,
                Some(&pt_src),
                rgb(0, 0, 0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    fn draw_grid(&self, w: &PerMonitorWindow, snapshot: &Snapshot, region: RectI) {
        let cols = snapshot.columns as usize;
        let rows = snapshot.rows as usize;
        let cw = region.width() as f64 / cols as f64;
        let ch = region.height() as f64 / rows as f64;
        let buf = w.bits;
        let width = w.width;
        let height = w.height;

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
                fill_rect(
                    buf,
                    width,
                    height,
                    cell.left,
                    cell.top,
                    cell.right,
                    cell.bottom,
                    0,
                    178,
                    178,
                    60,
                );
                draw_rect_outline(
                    buf,
                    width,
                    height,
                    cell.left,
                    cell.top,
                    cell.right,
                    cell.bottom,
                    0,
                    178,
                    178,
                    210,
                );
                self.draw_label(w, &snapshot.labels[index], &cell);
            }
        }
    }

    fn draw_precision(&self, w: &PerMonitorWindow, snapshot: &Snapshot, region: RectI) {
        let buf = w.bits;
        let width = w.width;
        let height = w.height;

        let box_w = ((width as f64 - 48.0)
            .min((snapshot.columns as f64) * 20.0)
            .max(100.0)) as i32;
        let box_h = ((height as f64 - 96.0)
            .min((snapshot.rows as f64) * 15.0)
            .max(75.0)) as i32;
        let mut bx = region.left + region.width() / 2 - box_w / 2;
        let mut by = region.top + region.height() / 2 - box_h / 2;
        bx = bx.clamp(24, width - box_w - 24);
        by = by.clamp(24, height - box_h - 58);

        draw_rect_outline(
            buf,
            width,
            height,
            region.left,
            region.top,
            region.right,
            region.bottom,
            255,
            235,
            0,
            230,
        );
        fill_rect(
            buf,
            width,
            height,
            bx - 8,
            by - 8,
            bx + box_w + 8,
            by + box_h + 8,
            0,
            0,
            0,
            102,
        );
        draw_line(
            buf,
            width,
            height,
            region.left + region.width() / 2,
            region.top + region.height() / 2,
            bx + box_w / 2,
            by + box_h / 2,
            255,
            235,
            0,
            150,
        );

        let mag = RectI {
            left: bx,
            top: by,
            right: bx + box_w,
            bottom: by + box_h,
        };
        self.draw_grid(w, snapshot, mag);
    }

    fn draw_label(&self, w: &PerMonitorWindow, label: &str, cell: &RectI) {
        let cell_w = cell.width() as f64;
        let cell_h = cell.height() as f64;
        let shortest = cell_w.min(cell_h);
        if shortest < 9.0 {
            return;
        }
        let font_size = (shortest * 0.48).clamp(7.0, 42.0) as i32;
        let mut wide = to_wide(label);
        unsafe {
            let hfont = CreateFontW(
                -font_size,
                0,
                0,
                0,
                700,
                0,
                0,
                0,
                1,
                0,
                0,
                0,
                0,
                windows::core::PCWSTR::null(),
            );
            let old = SelectObject(w.hdc, hfont);
            SetBkMode(w.hdc, TRANSPARENT);
            SetTextColor(w.hdc, rgb(255, 255, 255));
            let mut rect = RECT {
                left: cell.left,
                top: cell.top,
                right: cell.right,
                bottom: cell.bottom,
            };
            let _ = DrawTextW(
                w.hdc,
                &mut wide,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            let _ = SelectObject(w.hdc, old);
            let _ = DeleteObject(hfont);
        }
    }

    fn draw_status(&self, w: &PerMonitorWindow, snapshot: &Snapshot) {
        let mode = if snapshot.continuous_mode {
            "Persist"
        } else {
            "Once"
        };
        let drag = if snapshot.dragging { "  Dragging" } else { "" };
        let text = format!("{}  {}  {}  {}", "Mouseless", mode, drag, snapshot.status);
        let mut wide = to_wide(&text);
        unsafe {
            let hfont = CreateFontW(
                -14,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                1,
                0,
                0,
                0,
                0,
                windows::core::PCWSTR::null(),
            );
            let old = SelectObject(w.hdc, hfont);
            SetBkMode(w.hdc, TRANSPARENT);
            let bx = 14i32;
            let by = w.height - 36;
            let bw = (text.chars().count() as i32 * 8).clamp(120, 900);
            fill_rect(
                w.bits,
                w.width,
                w.height,
                bx,
                by,
                bx + bw,
                by + 24,
                0,
                0,
                0,
                135,
            );
            SetTextColor(w.hdc, rgb(255, 255, 255));
            let mut rect = RECT {
                left: bx + 4,
                top: by,
                right: bx + bw,
                bottom: by + 24,
            };
            let _ = DrawTextW(
                w.hdc,
                &mut wide,
                &mut rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            let _ = SelectObject(w.hdc, old);
            let _ = DeleteObject(hfont);
        }
    }
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

fn local_rect(virtual_rect: Rect, monitor: Rect) -> RectI {
    RectI {
        left: (virtual_rect.left - monitor.left).round() as i32,
        top: (virtual_rect.top - monitor.top).round() as i32,
        right: (virtual_rect.right - monitor.left).round() as i32,
        bottom: (virtual_rect.bottom - monitor.top).round() as i32,
    }
}

#[allow(clippy::too_many_arguments)]
fn set_pixel(buf: *mut u32, width: i32, height: i32, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8) {
    if x < 0 || y < 0 || x >= width || y >= height {
        return;
    }
    unsafe {
        let idx = (y as usize) * (width as usize) + x as usize;
        let bg = *buf.add(idx);
        let ba = ((bg >> 24) & 0xff) as i32;
        let na = a as i32 + ba * (255 - a as i32) / 255;
        if na <= 0 {
            *buf.add(idx) = 0;
            return;
        }
        let br = ((bg >> 16) & 0xff) as i32;
        let bgc = ((bg >> 8) & 0xff) as i32;
        let bb = (bg & 0xff) as i32;
        let inv = 255 - a as i32;
        let nr = (r as i32 * a as i32 + br * inv) / 255;
        let ng = (g as i32 * a as i32 + bgc * inv) / 255;
        let nb = (b as i32 * a as i32 + bb * inv) / 255;
        let pixel = ((na as u32 & 0xff) << 24)
            | (((nr as u32) & 0xff) << 16)
            | (((ng as u32) & 0xff) << 8)
            | ((nb as u32) & 0xff);
        *buf.add(idx) = pixel;
    }
}

fn fill_rect(
    buf: *mut u32,
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

fn draw_rect_outline(
    buf: *mut u32,
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
    buf: *mut u32,
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
    buf: *mut u32,
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
    buf: *mut u32,
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
