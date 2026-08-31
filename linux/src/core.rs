use crate::common::{MouseButton, Rect};
use crate::settings::Settings;
use std::sync::{Arc, Mutex};

pub trait MouseBackend: Send + Sync {
    fn move_to(&self, x: f64, y: f64);
    fn click(&self, x: f64, y: f64, button: MouseButton, count: u32);
    fn toggle_drag(&self, x: f64, y: f64);
    fn move_drag(&self, x: f64, y: f64);
    fn scroll(&self, vertical: i32, horizontal: i32);
    fn scroll_at(&self, x: f64, y: f64, vertical: i32, horizontal: i32);
    fn is_dragging(&self) -> bool;
}

pub trait ClickDetector: Send + Sync {
    /// Returns a clickable point near (x, y), or `None` if nothing clickable is found.
    fn snap_to_clickable(&self, x: f64, y: f64) -> Option<(f64, f64)>;
}

pub trait Environment: Send + Sync {
    fn monitors(&self) -> Vec<Rect>;
    fn cursor_pos(&self) -> (f64, f64);
}

const GRID_SEQUENCE: &[&str] = &[
    "A", "S", "D", "F", "G", "H", "J", "K", "L", "M", "W", "E", "R", "T", "Y", "U", "I", "O", "P",
    "Z", "X", "C", "V", "B", "N",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastAction {
    None,
    LeftClick,
    DoubleClick,
    RightClick,
    Drag,
}

pub struct OverlayState {
    settings: Arc<Mutex<Settings>>,
    mouse: Arc<dyn MouseBackend>,
    detector: Arc<dyn ClickDetector>,
    env: Arc<dyn Environment>,
    history: Vec<Rect>,
    active_region: Rect,
    target_offset: (f64, f64),
    scroll_modifier_active: bool,
    current_screen_index: usize,
    last_action: LastAction,
    action_status: String,
    visible: bool,
    request_hide: bool,
    pending_scroll: Option<(f64, f64, i32, i32)>,
}

#[derive(Clone)]
pub struct Snapshot {
    pub active_region: Rect,
    pub rows: i32,
    pub columns: i32,
    pub labels: Vec<String>,
    pub opacity: f64,
    pub cursor: (f64, f64),
    pub status: String,
    pub continuous_mode: bool,
    pub dragging: bool,
    pub precision_mode: bool,
    pub scroll_mode: bool,
}

impl OverlayState {
    pub fn new(
        settings: Arc<Mutex<Settings>>,
        mouse: Arc<dyn MouseBackend>,
        detector: Arc<dyn ClickDetector>,
        env: Arc<dyn Environment>,
    ) -> Self {
        OverlayState {
            settings,
            mouse,
            detector,
            env,
            history: Vec::new(),
            active_region: Rect::new(0.0, 0.0, 1.0, 1.0),
            target_offset: (0.0, 0.0),
            scroll_modifier_active: false,
            current_screen_index: 0,
            last_action: LastAction::None,
            action_status: String::new(),
            visible: false,
            request_hide: false,
            pending_scroll: None,
        }
    }

    fn s(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Keys that may safely be repeated while held. Grid labels and action
    /// keys are intentionally excluded: a repeated label refines the newly
    /// selected cell a second time, which looks like a second grid appearing.
    pub fn is_repeatable_key(&self, label: &str) -> bool {
        match label {
            "Backspace" | "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => true,
            "J" | "K" => self.scroll_modifier_active,
            _ => false,
        }
    }

    pub fn show(&mut self) {
        let monitors = self.env.monitors();
        if monitors.is_empty() {
            return;
        }
        let (cx, cy) = self.env.cursor_pos();
        self.current_screen_index = monitors
            .iter()
            .position(|m| m.contains(cx, cy))
            .unwrap_or(0);
        self.active_region = monitors[self.current_screen_index];
        self.target_offset = (0.0, 0.0);
        self.scroll_modifier_active = false;
        self.history.clear();
        self.request_hide = false;
        self.pending_scroll = None;
        self.visible = true;
        let quit = self.s().quit_grid_key.clone();
        self.action_status = format!(
            "Enter left-click  Hold Space+J/K scroll  1 force-click  {} quit",
            quit
        );
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.history.clear();
        self.scroll_modifier_active = false;
        self.request_hide = false;
        self.pending_scroll = None;
    }

    pub fn take_pending_scroll(&mut self) -> Option<(f64, f64, i32, i32)> {
        self.pending_scroll.take()
    }

    fn grid_keys(&self) -> Vec<String> {
        let s = self.s();
        GRID_SEQUENCE
            .iter()
            .take((s.grid_rows * s.grid_columns) as usize)
            .map(|s| s.to_string())
            .collect()
    }

    fn virtual_cursor(&self) -> (f64, f64) {
        let r = self.active_region;
        let x = (r.mid_x() + self.target_offset.0).clamp(r.left, r.right);
        let y = (r.mid_y() + self.target_offset.1).clamp(r.top, r.bottom);
        (x, y)
    }

    fn precision_mode(&self) -> bool {
        let s = self.s();
        let cell_w = self.active_region.width() / s.grid_columns as f64;
        let cell_h = self.active_region.height() / s.grid_rows as f64;
        cell_w.min(cell_h) < 24.0
    }

    fn precision_nudge_step(&self) -> f64 {
        let w = self.active_region.width();
        let h = self.active_region.height();
        (1.0f64).max((w.min(h) / 32.0).min(4.0))
    }

    /// Handle a keyboard event while the overlay is visible.
    /// Returns `true` when the overlay should be hidden afterwards.
    pub fn handle_key(&mut self, label: &str, is_key_down: bool) -> bool {
        self.handle_key_with_modifiers(label, is_key_down, false)
    }

    pub fn handle_key_with_modifiers(&mut self, label: &str, is_key_down: bool, shift_held: bool) -> bool {
        self.request_hide = false;

        if !is_key_down {
            if label == "Space" {
                self.scroll_modifier_active = false;
                self.action_status = "Scroll off".to_string();
            }
            return self.request_hide;
        }

        let quit = self.s().quit_grid_key.clone();
        if label == quit || label == "Escape" {
            self.hide();
            return true;
        }

        if self.scroll_modifier_active {
            match label {
                "J" | "ArrowDown" => {
                    self.scroll_overlay(ScrollDirection::Down);
                    return self.request_hide;
                }
                "K" | "ArrowUp" => {
                    self.scroll_overlay(ScrollDirection::Up);
                    return self.request_hide;
                }
                _ => {}
            }
        }

        match label {
            "Backspace" => self.undo(),
            "Space" => {
                self.scroll_modifier_active = true;
                self.action_status = "Scroll mode: J down  K up".to_string();
            }
            "1" => self.perform("Left click", LastAction::LeftClick),
            "2" => self.perform("Double click", LastAction::DoubleClick),
            "3" => self.perform("Right click", LastAction::RightClick),
            "4" => self.perform_toggle_drag(),
            "Enter" => self.perform("Left click", LastAction::LeftClick),
            "Tab" => {
                let mut s = self.settings.lock().unwrap();
                s.continuous_mode = !s.continuous_mode;
                self.action_status = if s.continuous_mode {
                    "Persistent overlay on".to_string()
                } else {
                    "Persistent overlay off".to_string()
                };
            }
            "ArrowRight" => {
                if self.precision_mode() {
                    if shift_held {
                        self.move_screen(1);
                    } else {
                        let (cx, _) = self.virtual_cursor();
                        if (cx - self.active_region.right).abs() < 0.5 {
                            self.move_screen(1);
                        } else {
                            self.nudge(self.precision_nudge_step(), 0.0);
                        }
                    }
                } else {
                    self.move_screen(1);
                }
            }
            "ArrowLeft" => {
                if self.precision_mode() {
                    if shift_held {
                        self.move_screen(-1);
                    } else {
                        let (cx, _) = self.virtual_cursor();
                        if (cx - self.active_region.left).abs() < 0.5 {
                            self.move_screen(-1);
                        } else {
                            self.nudge(-self.precision_nudge_step(), 0.0);
                        }
                    }
                } else {
                    self.move_screen(-1);
                }
            }
            "ArrowUp" => {
                if self.precision_mode() {
                    // Top-left origin: up is negative y.
                    self.nudge(0.0, -self.precision_nudge_step());
                } else {
                    self.scroll_overlay(ScrollDirection::Up);
                }
            }
            "ArrowDown" => {
                if self.precision_mode() {
                    self.nudge(0.0, self.precision_nudge_step());
                } else {
                    self.scroll_overlay(ScrollDirection::Down);
                }
            }
            "=" => {
                let mut s = self.settings.lock().unwrap();
                s.overlay_opacity = (s.overlay_opacity + 0.06).min(0.95);
            }
            "-" => {
                let mut s = self.settings.lock().unwrap();
                s.overlay_opacity = (s.overlay_opacity - 0.06).max(0.25);
            }
            "`" => self.repeat_last(),
            _ => {
                if let Some(index) = self.grid_keys().iter().position(|l| l == label) {
                    self.select_cell(index);
                }
            }
        }

        if self.request_hide {
            self.hide();
            return true;
        }
        false
    }

    fn run_action(&self, action: LastAction) {
        if action == LastAction::None {
            return;
        }
        // The clickable-target scan and the click itself (with its inter-event
        // sleeps) are too slow to run inline on the input thread, which must
        // stay responsive to keystrokes. Run them on a background thread.
        let (x, y) = self.virtual_cursor();
        let detector = self.detector.clone();
        let mouse = self.mouse.clone();
        std::thread::spawn(move || {
            let (tx, ty) = detector.snap_to_clickable(x, y).unwrap_or((x, y));
            match action {
                LastAction::None => {}
                LastAction::LeftClick => mouse.click(tx, ty, MouseButton::Left, 1),
                LastAction::DoubleClick => mouse.click(tx, ty, MouseButton::Left, 2),
                LastAction::RightClick => mouse.click(tx, ty, MouseButton::Right, 1),
                LastAction::Drag => mouse.toggle_drag(tx, ty),
            }
        });
    }

    fn perform(&mut self, status: &str, action: LastAction) {
        self.run_action(action);
        self.last_action = action;
        self.action_status = status.to_string();
        if !self.s().continuous_mode && !self.mouse.is_dragging() {
            self.request_hide = true;
        }
    }

    fn perform_toggle_drag(&mut self) {
        // The drag toggle runs asynchronously, so predict the resulting state.
        let was_dragging = self.mouse.is_dragging();
        let status = if was_dragging { "Drop" } else { "Drag" };
        self.run_action(LastAction::Drag);
        self.last_action = LastAction::Drag;
        self.action_status = status.to_string();
        let now_dragging = !was_dragging;
        if !self.s().continuous_mode && !now_dragging {
            self.request_hide = true;
        }
    }

    fn repeat_last(&mut self) {
        if self.last_action != LastAction::None {
            self.run_action(self.last_action);
        }
        if !self.s().continuous_mode {
            self.request_hide = true;
        }
    }

    fn scroll_overlay(&mut self, direction: ScrollDirection) {
        let s = self.s();
        let amount = s.scroll_step * 3;
        let (x, y) = self.virtual_cursor();
        let vertical = match direction {
            ScrollDirection::Up => -amount,
            ScrollDirection::Down => amount,
        };
        self.pending_scroll = Some((x, y, vertical, 0));
        self.action_status = match direction {
            ScrollDirection::Up => "Scrolled up".to_string(),
            ScrollDirection::Down => "Scrolled down".to_string(),
        };
    }

    fn select_cell(&mut self, index: usize) {
        let s = self.s();
        let row = index / s.grid_columns as usize;
        let column = index % s.grid_columns as usize;
        if row >= s.grid_rows as usize {
            return;
        }

        if self.precision_mode() {
            let sub_w = self.active_region.width() / s.grid_columns as f64;
            let sub_h = self.active_region.height() / s.grid_rows as f64;
            let target_x = self.active_region.left + (column as f64 + 0.5) * sub_w;
            let target_y = self.active_region.top + (row as f64 + 0.5) * sub_h;
            self.target_offset = (
                target_x - self.active_region.mid_x(),
                target_y - self.active_region.mid_y(),
            );
            let (vx, vy) = self.virtual_cursor();
            self.mouse.move_drag(vx, vy);
            self.action_status = format!(
                "Precision {}  arrows nudge {}px",
                self.label_for_current_path(),
                self.precision_nudge_step() as i32
            );
            return;
        }

        self.history.push(self.active_region);
        let width = self.active_region.width() / s.grid_columns as f64;
        let height = self.active_region.height() / s.grid_rows as f64;
        self.active_region = Rect::new(
            self.active_region.left + column as f64 * width,
            self.active_region.top + row as f64 * height,
            self.active_region.left + (column as f64 + 1.0) * width,
            self.active_region.top + (row as f64 + 1.0) * height,
        );
        self.target_offset = (0.0, 0.0);
        let (vx, vy) = self.virtual_cursor();
        self.mouse.move_drag(vx, vy);
        self.action_status = if self.precision_mode() {
            format!(
                "Precision {}  arrows nudge {}px",
                self.label_for_current_path(),
                self.precision_nudge_step() as i32
            )
        } else {
            format!("Target {}", self.label_for_current_path())
        };
    }

    fn undo(&mut self) {
        if let Some(previous) = self.history.pop() {
            self.active_region = previous;
            self.target_offset = (0.0, 0.0);
            self.action_status = "Undo".to_string();
        }
    }

    fn nudge(&mut self, dx: f64, dy: f64) {
        let (cx, cy) = self.virtual_cursor();
        let next_x = (cx + dx).clamp(self.active_region.left, self.active_region.right);
        let next_y = (cy + dy).clamp(self.active_region.top, self.active_region.bottom);
        self.target_offset = (
            next_x - self.active_region.mid_x(),
            next_y - self.active_region.mid_y(),
        );
        if self.mouse.is_dragging() {
            self.mouse.move_drag(next_x, next_y);
        }
        self.action_status = format!(
            "Nudged {}px  Enter click  Space+J/K scroll",
            self.precision_nudge_step() as i32
        );
    }

    fn move_screen(&mut self, delta: i32) {
        let monitors = self.env.monitors();
        if monitors.is_empty() {
            return;
        }
        let n = monitors.len() as i32;
        self.current_screen_index =
            (((self.current_screen_index as i32 + delta) % n) + n) as usize % monitors.len();
        self.active_region = monitors[self.current_screen_index];
        self.target_offset = (0.0, 0.0);
        self.history.clear();
        self.action_status = format!("Monitor {}", self.current_screen_index + 1);
    }

    fn label_for_current_path(&self) -> String {
        let size = self
            .active_region
            .width()
            .min(self.active_region.height())
            .max(1.0);
        format!("{}px cell", size.round() as i32)
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.s();
        Snapshot {
            active_region: self.active_region,
            rows: s.grid_rows,
            columns: s.grid_columns,
            labels: self.grid_keys(),
            opacity: s.overlay_opacity,
            cursor: self.virtual_cursor(),
            status: self.action_status.clone(),
            continuous_mode: s.continuous_mode,
            dragging: self.mouse.is_dragging(),
            precision_mode: self.precision_mode(),
            scroll_mode: self.scroll_modifier_active,
        }
    }
}

enum ScrollDirection {
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::MouseButton;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockMouse {
        clicks: Arc<AtomicU32>,
    }

    impl MouseBackend for MockMouse {
        fn move_to(&self, _: f64, _: f64) {}
        fn click(&self, _: f64, _: f64, _: MouseButton, count: u32) {
            self.clicks.fetch_add(count, Ordering::SeqCst);
        }
        fn toggle_drag(&self, _: f64, _: f64) {}
        fn move_drag(&self, _: f64, _: f64) {}
        fn scroll(&self, _: i32, _: i32) {}
        fn scroll_at(&self, _: f64, _: f64, _: i32, _: i32) {}
        fn is_dragging(&self) -> bool {
            false
        }
    }

    struct MockDetector;

    impl ClickDetector for MockDetector {
        fn snap_to_clickable(&self, _: f64, _: f64) -> Option<(f64, f64)> {
            None
        }
    }

    struct MockEnv;

    impl Environment for MockEnv {
        fn monitors(&self) -> Vec<Rect> {
            vec![Rect::new(0.0, 0.0, 1920.0, 1080.0)]
        }
        fn cursor_pos(&self) -> (f64, f64) {
            (960.0, 540.0)
        }
    }

    #[test]
    fn grid_refinement_and_undo() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let clicks = Arc::new(AtomicU32::new(0));
        let mouse = Arc::new(MockMouse { clicks });
        let detector = Arc::new(MockDetector);
        let env = Arc::new(MockEnv);

        let mut state = OverlayState::new(settings, mouse, detector, env);
        state.show();
        assert!(state.is_visible());
        assert_eq!(state.snapshot().active_region, Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert!(!state.snapshot().precision_mode);

        // Step 1: Select "A" (row 0, col 0 in 5x5)
        let hide = state.handle_key("A", true);
        assert!(!hide);
        assert!(state.is_visible());
        let reg1 = state.snapshot().active_region;
        assert_eq!(reg1, Rect::new(0.0, 0.0, 384.0, 216.0));
        assert!(!state.snapshot().precision_mode);

        // Step 2: Select "S" (row 0, col 1 in 5x5: x from 384/5=76.8 to 153.6)
        let hide = state.handle_key("S", true);
        assert!(!hide);
        assert!(state.is_visible());
        let reg2 = state.snapshot().active_region;
        assert_eq!(reg2, Rect::new(76.8, 0.0, 153.6, 43.2));
        assert!(state.snapshot().precision_mode);

        // Step 3: Select "A" -> cell size 76.8/5 = 15.36 < 24.0 -> enters precision mode
        let hide = state.handle_key("A", true);
        assert!(!hide);
        assert!(state.is_visible());
        assert!(state.snapshot().precision_mode);

        // Step 4: Undo steps back
        state.handle_key("Backspace", true);
        assert_eq!(state.snapshot().active_region, reg1);
        assert!(!state.snapshot().precision_mode);

        state.handle_key("Backspace", true);
        assert_eq!(state.snapshot().active_region, Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert!(!state.snapshot().precision_mode);
    }

    #[test]
    fn quit_keys_hide_overlay() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let clicks = Arc::new(AtomicU32::new(0));
        let mouse = Arc::new(MockMouse { clicks });
        let detector = Arc::new(MockDetector);
        let env = Arc::new(MockEnv);

        let mut state = OverlayState::new(settings, mouse, detector, env);
        state.show();
        assert!(state.is_visible());

        assert!(state.handle_key("Escape", true));
        assert!(!state.is_visible());

        state.show();
        assert!(state.is_visible());
        assert!(state.handle_key("Q", true));
        assert!(!state.is_visible());
    }
}
