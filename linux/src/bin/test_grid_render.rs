#[path = "../common.rs"]
mod common;
#[path = "../core.rs"]
mod core;
#[path = "../settings.rs"]
mod settings;
#[path = "../render.rs"]
mod render;

use common::{MouseButton, Rect};
use core::{ClickDetector, Environment, MouseBackend, OverlayState};
use settings::Settings;
use std::sync::{Arc, Mutex};

struct MockMouse;
impl MouseBackend for MockMouse {
    fn move_to(&self, _: f64, _: f64) {}
    fn click(&self, _: f64, _: f64, _: MouseButton, _: u32) {}
    fn toggle_drag(&self, _: f64, _: f64) {}
    fn move_drag(&self, _: f64, _: f64) {}
    fn scroll(&self, _: i32, _: i32) {}
    fn scroll_at(&self, _: f64, _: f64, _: i32, _: i32) {}
    fn is_dragging(&self) -> bool { false }
}
struct MockDetector;
impl ClickDetector for MockDetector {
    fn snap_to_clickable(&self, _: f64, _: f64) -> Option<(f64,f64)> { None }
}
struct MockEnv;
impl Environment for MockEnv {
    fn monitors(&self) -> Vec<Rect> { vec![Rect::new(0.0,0.0,1366.0,768.0)] }
    fn cursor_pos(&self) -> (f64,f64) { (683.0,384.0) }
}

fn save_ppm(path: &str, data: &[u32], w: i32, h: i32) {
    use std::io::Write;
    let mut f = std::fs::File::create(path).unwrap();
    writeln!(f, "P6\n{} {}\n255", w, h).unwrap();
    for &pixel in data {
        let r = ((pixel>>16)&0xff) as u8;
        let g = ((pixel>>8)&0xff) as u8;
        let b = (pixel&0xff) as u8;
        f.write_all(&[r,g,b]).unwrap();
    }
    println!("saved {}", path);
}

fn main() {
    let settings = Arc::new(Mutex::new(Settings::default()));
    let mouse = Arc::new(MockMouse);
    let detector = Arc::new(MockDetector);
    let env = Arc::new(MockEnv);
    let mut state = OverlayState::new(settings.clone(), mouse, detector, env);
    state.show();
    println!("Initial: region {:?} precision {}", state.snapshot().active_region, state.snapshot().precision_mode);
    render_and_save(&state, "/tmp/grid0.ppm");
    // Level 1: A
    state.handle_key("A", true);
    println!("After A: region {:?} precision {} cursor {:?} status {}", state.snapshot().active_region, state.snapshot().precision_mode, state.snapshot().cursor, state.snapshot().status);
    render_and_save(&state, "/tmp/grid1.ppm");
    // Level 2: S
    state.handle_key("S", true);
    println!("After S: region {:?} precision {} cursor {:?} status {}", state.snapshot().active_region, state.snapshot().precision_mode, state.snapshot().cursor, state.snapshot().status);
    render_and_save(&state, "/tmp/grid2.ppm");
    // Level 3: D (should be precision)
    state.handle_key("D", true);
    println!("After D (third): region {:?} precision {} cursor {:?} status {}", state.snapshot().active_region, state.snapshot().precision_mode, state.snapshot().cursor, state.snapshot().status);
    render_and_save(&state, "/tmp/grid3.ppm");
    // Level 4: F
    state.handle_key("F", true);
    println!("After F (fourth): region {:?} precision {} cursor {:?} status {}", state.snapshot().active_region, state.snapshot().precision_mode, state.snapshot().cursor, state.snapshot().status);
    render_and_save(&state, "/tmp/grid4.ppm");

    // Also test nudge
    state.handle_key("ArrowRight", true);
    println!("After nudge right: cursor {:?} status {}", state.snapshot().cursor, state.snapshot().status);
    render_and_save(&state, "/tmp/grid_nudge.ppm");
}

fn render_and_save(state: &OverlayState, path: &str) {
    let snap = state.snapshot();
    let w = 1366;
    let h = 768;
    let scale = 1;
    let mut buf = vec![0u32; (w*h) as usize];
    render::render_snapshot(&mut buf, w, h, scale, (0.0,0.0), &snap);
    save_ppm(path, &buf, w, h);
}
