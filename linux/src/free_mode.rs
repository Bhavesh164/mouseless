use crate::common::MouseButton;
use crate::core::{Environment, MouseBackend};
use crate::settings::Settings;
use std::sync::Arc;

pub struct FreeModeController {
    active: bool,
    mouse: Arc<dyn MouseBackend>,
    env: Arc<dyn Environment>,
    notify: Arc<dyn Fn(&str) + Send + Sync>,
    pending_scroll: Option<(i32, i32)>,
}

impl FreeModeController {
    pub fn new(
        mouse: Arc<dyn MouseBackend>,
        env: Arc<dyn Environment>,
        notify: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        FreeModeController {
            active: false,
            mouse,
            env,
            notify,
            pending_scroll: None,
        }
    }

    pub fn toggle(&mut self) {
        if self.active {
            self.exit();
        } else {
            self.enter();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Movement and scrolling are safe to repeat; mouse buttons are not.
    pub fn is_repeatable_key(&self, label: &str) -> bool {
        matches!(
            label,
            "H" | "J" | "K" | "L"
                | "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "U"
                | "D"
                | "Y"
                | "O"
        )
    }

    pub fn enter(&mut self) {
        self.active = true;
        (self.notify)("Free mode");
    }

    pub fn exit(&mut self) {
        self.active = false;
        (self.notify)("Free mode off");
    }

    pub fn take_pending_scroll(&mut self) -> Option<(i32, i32)> {
        self.pending_scroll.take()
    }

    // The click sleeps must not block the input thread.
    fn click_async(&self, x: f64, y: f64, button: MouseButton) {
        let mouse = self.mouse.clone();
        std::thread::spawn(move || mouse.click(x, y, button, 1));
    }

    pub fn handle(&mut self, label: &str, settings: &Settings) -> bool {
        if !self.active {
            return false;
        }
        let step = settings.free_mode_step;
        let (mut x, mut y) = self.env.cursor_pos();
        match label {
            "Escape" => self.exit(),
            "H" | "ArrowLeft" => {
                x -= step;
                self.mouse.move_to(x, y);
            }
            "L" | "ArrowRight" => {
                x += step;
                self.mouse.move_to(x, y);
            }
            "K" | "ArrowUp" => {
                y -= step;
                self.mouse.move_to(x, y);
            }
            "J" | "ArrowDown" => {
                y += step;
                self.mouse.move_to(x, y);
            }
            "Space" => self.click_async(x, y, MouseButton::Left),
            "R" => self.click_async(x, y, MouseButton::Right),
            "U" => self.pending_scroll = Some((-(settings.scroll_step * 3), 0)),
            "D" => self.pending_scroll = Some((settings.scroll_step * 3, 0)),
            "Y" => self.pending_scroll = Some((0, -(settings.scroll_step * 3))),
            "O" => self.pending_scroll = Some((0, settings.scroll_step * 3)),
            _ => {}
        }
        true
    }
}
