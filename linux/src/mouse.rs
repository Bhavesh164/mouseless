use crate::core::{Environment, MouseBackend};
use crate::hypr::Hyprland;
use crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;
use crate::common::{MouseButton, Rect};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_pointer::{Axis, AxisSource, ButtonState};
use wayland_client::{Connection, Proxy};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

pub struct VirtualPointerMouse {
    conn: Connection,
    vp: Mutex<Option<ZwlrVirtualPointerV1>>,
    hypr: Option<Hyprland>,
    last_pos: Arc<Mutex<(f64, f64)>>,
    dragging: AtomicBool,
    epoch: Instant,
}

impl VirtualPointerMouse {
    pub fn new(
        conn: Connection,
        vp: Option<ZwlrVirtualPointerV1>,
        hypr: Option<Hyprland>,
        initial_pos: (f64, f64),
    ) -> Self {
        let last_pos = Arc::new(Mutex::new(initial_pos));
        VirtualPointerMouse {
            conn,
            vp: Mutex::new(vp),
            hypr,
            last_pos,
            dragging: AtomicBool::new(false),
            epoch: Instant::now(),
        }
    }

    pub fn position_tracker(&self) -> Arc<Mutex<(f64, f64)>> {
        self.last_pos.clone()
    }

    fn now_ms(&self) -> u32 {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        unsafe {
            libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        }
        ((ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000)) as u32
    }

    fn with_vp<R>(&self, f: impl FnOnce(&ZwlrVirtualPointerV1) -> R) -> Option<R> {
        let guard = self.vp.lock().unwrap();
        let vp = guard.as_ref()?;
        let result = f(vp);
        let _ = self.conn.flush();
        Some(result)
    }

    fn set_button(&self, button: u32, state: ButtonState) {
        self.with_vp(|vp| {
            vp.button(self.now_ms(), button, state);
            vp.frame();
        });
    }

    fn wheel(&self, vertical: i32, horizontal: i32) {
        if vertical == 0 && horizontal == 0 {
            return;
        }
        // Use a fresh Wayland connection for scroll, like hypruse and
        // test_scroll_brute do. The daemon's main connection is busy with
        // layer-shell and its event queue; reusing it for virtual-pointer
        // scroll can race with layer-shell commits and cause the compositor
        // to drop the axis. A short-lived connection avoids that and
        // matches the proven hypruse wire protocol. This path *does*
        // deliver scroll even when the grid has OnDemand + EVIOCGRAB –
        // tested: scroll_at with Wheel discrete 15 produces
        // SCROLL_RECEIVED in gtk_test even while grabbed, whereas
        // reusing the main vp (same client as the exclusive layer) is
        // ignored by Hyprland's m_exclusiveLSes logic.
        let conn = match Connection::connect_to_env() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut event_queue = conn.new_event_queue();
        let qh = event_queue.handle();
        conn.display().get_registry(&qh, ());

        struct ScrollState {
            vp_manager: Option<crate::virtual_pointer::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
            seat: Option<wayland_client::protocol::wl_seat::WlSeat>,
        }
        impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()> for ScrollState {
            fn event(
                state: &mut Self,
                registry: &wayland_client::protocol::wl_registry::WlRegistry,
                event: <wayland_client::protocol::wl_registry::WlRegistry as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                qh: &wayland_client::QueueHandle<Self>,
            ) {
                if let wayland_client::protocol::wl_registry::Event::Global { name, interface, version } = event {
                    if interface == "zwlr_virtual_pointer_manager_v1" {
                        state.vp_manager = Some(registry.bind(name, version.min(2), qh, ()));
                    } else if interface == "wl_seat" {
                        state.seat = Some(registry.bind(name, 1, qh, ()));
                    }
                }
            }
        }
        impl wayland_client::Dispatch<crate::virtual_pointer::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, ()> for ScrollState {
            fn event(_: &mut Self, _: &crate::virtual_pointer::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _: <crate::virtual_pointer::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &wayland_client::QueueHandle<Self>) {}
        }
        impl wayland_client::Dispatch<wayland_client::protocol::wl_seat::WlSeat, ()> for ScrollState {
            fn event(_: &mut Self, _: &wayland_client::protocol::wl_seat::WlSeat, _: <wayland_client::protocol::wl_seat::WlSeat as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &wayland_client::QueueHandle<Self>) {}
        }
        impl wayland_client::Dispatch<crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()> for ScrollState {
            fn event(_: &mut Self, _: &crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, _: <crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &wayland_client::QueueHandle<Self>) {}
        }

        let mut state = ScrollState { vp_manager: None, seat: None };
        let _ = event_queue.roundtrip(&mut state);
        let Some(mgr) = state.vp_manager.clone() else { return; };
        let vp = mgr.create_virtual_pointer(state.seat.as_ref(), &qh, ());
        let _ = event_queue.roundtrip(&mut state);

        let time = self.now_ms();
        vp.axis_source(AxisSource::Wheel);
        const UNITS_PER_NOTCH: f64 = 15.0;
        let mut sent = false;
        for (axis, val) in [
            (Axis::HorizontalScroll, horizontal),
            (Axis::VerticalScroll, vertical),
        ] {
            if val == 0 {
                continue;
            }
            let v = val as f64;
            let notches = (v / UNITS_PER_NOTCH).round() as i32;
            if notches != 0 {
                let discrete_v = notches as f64 * UNITS_PER_NOTCH;
                vp.axis_discrete(time, axis, discrete_v, notches);
            } else {
                vp.axis(time, axis, v);
            }
            sent = true;
        }
        if sent {
            vp.frame();
            let _ = conn.flush();
            let _ = event_queue.roundtrip(&mut state);
        }
    }
}

impl MouseBackend for VirtualPointerMouse {
    fn move_to(&self, x: f64, y: f64) {
        // Absolute position via relative motion: query the real cursor and
        // send a single relative delta. Avoids compositor-specific absolute
        // mapping semantics (e.g. movecursor is broken on Hyprland 0.56).
        let cur = self
            .hypr
            .as_ref()
            .and_then(|h| h.cursor_pos())
            .unwrap_or_else(|| *self.last_pos.lock().unwrap());
        let dx = x - cur.0;
        let dy = y - cur.1;
        self.with_vp(|vp| {
            vp.motion(self.now_ms(), dx, dy);
            vp.frame();
        });
        *self.last_pos.lock().unwrap() = (x, y);
    }

    fn click(&self, x: f64, y: f64, button: MouseButton, count: u32) {
        self.move_to(x, y);
        // Allow web UI to process hover state (e.g. YouTube controls).
        sleep(Duration::from_millis(30));
        let code = match button {
            MouseButton::Left => BTN_LEFT,
            MouseButton::Right => BTN_RIGHT,
        };
        for _ in 0..count.max(1) {
            // The 50 ms mouse-down/up gap ensures JS registers a valid click.
            self.set_button(code, ButtonState::Pressed);
            sleep(Duration::from_millis(50));
            self.set_button(code, ButtonState::Released);
            sleep(Duration::from_millis(10));
        }
    }

    fn toggle_drag(&self, x: f64, y: f64) {
        if self.dragging.load(Ordering::SeqCst) {
            self.move_drag(x, y);
            self.set_button(BTN_LEFT, ButtonState::Released);
            self.dragging.store(false, Ordering::SeqCst);
        } else {
            self.move_to(x, y);
            sleep(Duration::from_millis(30));
            self.set_button(BTN_LEFT, ButtonState::Pressed);
            self.dragging.store(true, Ordering::SeqCst);
        }
    }

    fn move_drag(&self, x: f64, y: f64) {
        if self.dragging.load(Ordering::SeqCst) {
            self.move_to(x, y);
        }
    }

    fn scroll(&self, vertical: i32, horizontal: i32) {
        self.wheel(vertical, horizontal);
    }

    fn scroll_at(&self, x: f64, y: f64, vertical: i32, horizontal: i32) {
        // Use the main connection's move_to (relative motion via hyprctl) to
        // warp the global cursor, then use a fresh connection's wheel (like
        // hypruse) to scroll at that global position. This matches the
        // proven test_scroll_brute pattern: move via one virtual pointer
        // (parking + move) correctly updates the global cursor, and a
        // subsequent wheel on a *separate* device still scrolls the window
        // under that global cursor (Hyprland's onMouseWheel uses the global
        // cursor, not the device's cursor).
        self.move_to(x, y);
        sleep(Duration::from_millis(30));
        self.wheel(vertical, horizontal);
    }

    fn is_dragging(&self) -> bool {
        self.dragging.load(Ordering::SeqCst)
    }
}

/// Environment implementation: Hyprland IPC when available, with tracked
/// state as fallback.
pub struct SystemEnvironment {
    hypr: Option<Hyprland>,
    fallback_monitors: Vec<Rect>,
    last_pos: Arc<Mutex<(f64, f64)>>,
}

impl SystemEnvironment {
    pub fn new(
        hypr: Option<Hyprland>,
        fallback_monitors: Vec<Rect>,
        last_pos: Arc<Mutex<(f64, f64)>>,
    ) -> Self {
        SystemEnvironment {
            hypr,
            fallback_monitors,
            last_pos,
        }
    }
}

impl Environment for SystemEnvironment {
    fn monitors(&self) -> Vec<Rect> {
        if let Some(hypr) = &self.hypr {
            if let Some(monitors) = hypr.monitors() {
                return monitors;
            }
        }
        self.fallback_monitors.clone()
    }

    fn cursor_pos(&self) -> (f64, f64) {
        if let Some(hypr) = &self.hypr {
            if let Some(pos) = hypr.cursor_pos() {
                *self.last_pos.lock().unwrap() = pos;
                return pos;
            }
        }
        *self.last_pos.lock().unwrap()
    }
}
