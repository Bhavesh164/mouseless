use crate::core::OverlayState;
use crate::free_mode::FreeModeController;
use crate::settings::Settings;
use crate::ControlMsg;
use std::collections::HashMap;
use std::fs;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static GRAB_FDS: OnceLock<Mutex<Vec<RawFd>>> = OnceLock::new();
static GRAB_ACTIVE: OnceLock<Mutex<bool>> = OnceLock::new();
const EVIOCGRAB: libc::c_ulong = 0x40044590;

pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTMETA: u16 = 125;
pub const KEY_RIGHTMETA: u16 = 126;

#[derive(Clone, Copy, Default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// Maps a Linux evdev key code to the logical label used by the core logic
/// (positional, hardware-code based — same approach as the macOS/Windows
/// ports).
pub fn label_for_code(code: u16) -> Option<&'static str> {
    Some(match code {
        1 => "Escape",
        2 => "1",
        3 => "2",
        4 => "3",
        5 => "4",
        6 => "5",
        7 => "6",
        8 => "7",
        9 => "8",
        10 => "9",
        11 => "0",
        12 => "-",
        13 => "=",
        14 => "Backspace",
        15 => "Tab",
        16 => "Q",
        17 => "W",
        18 => "E",
        19 => "R",
        20 => "T",
        21 => "Y",
        22 => "U",
        23 => "I",
        24 => "O",
        25 => "P",
        26 => "[",
        27 => "]",
        28 => "Enter",
        29 => "Control",
        30 => "A",
        31 => "S",
        32 => "D",
        33 => "F",
        34 => "G",
        35 => "H",
        36 => "J",
        37 => "K",
        38 => "L",
        39 => ";",
        40 => "'",
        41 => "`",
        42 => "Shift",
        43 => "\\",
        44 => "Z",
        45 => "X",
        46 => "C",
        47 => "V",
        48 => "B",
        49 => "N",
        50 => "M",
        51 => ",",
        52 => ".",
        53 => "/",
        54 => "Shift",
        56 => "Alt",
        57 => "Space",
        58 => "CapsLock",
        59 => "F1",
        60 => "F2",
        61 => "F3",
        62 => "F4",
        63 => "F5",
        64 => "F6",
        65 => "F7",
        66 => "F8",
        67 => "F9",
        68 => "F10",
        71 => "7",
        72 => "8",
        73 => "9",
        75 => "4",
        76 => "5",
        77 => "6",
        79 => "1",
        80 => "2",
        81 => "3",
        82 => "0",
        83 => ".",
        87 => "F11",
        88 => "F12",
        96 => "Enter",
        100 => "Alt",
        102 => "Home",
        103 => "ArrowUp",
        104 => "PageUp",
        105 => "ArrowLeft",
        106 => "ArrowRight",
        107 => "End",
        108 => "ArrowDown",
        109 => "PageDown",
        110 => "Insert",
        111 => "Delete",
        125 => "LWin",
        126 => "RWin",
        127 => "Compose",
        _ => return None,
    })
}

fn update_modifiers(mods: &mut Modifiers, code: u16, down: bool) -> bool {
    match code {
        KEY_LEFTCTRL | KEY_RIGHTCTRL => mods.control = down,
        KEY_LEFTALT | KEY_RIGHTALT => mods.alt = down,
        KEY_LEFTSHIFT | KEY_RIGHTSHIFT => mods.shift = down,
        KEY_LEFTMETA | KEY_RIGHTMETA => mods.win = down,
        _ => return false,
    }
    true
}

fn open_keyboards() -> Vec<evdev::Device> {
    let mut devices = Vec::new();
    let Ok(entries) = fs::read_dir("/dev/input") else {
        return devices;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("event") {
            continue;
        }
        let Ok(dev) = evdev::Device::open(entry.path()) else {
            continue;
        };
        let is_keyboard = dev
            .supported_keys()
            .map(|keys| {
                keys.contains(evdev::Key::KEY_A)
                    && keys.contains(evdev::Key::KEY_SPACE)
                    && keys.contains(evdev::Key::KEY_ENTER)
            })
            .unwrap_or(false);
        if is_keyboard {
            devices.push(dev);
        }
    }
    devices
}

pub fn set_keyboard_grab(grab: bool) {
    // Grab/ungrab all keyboard devices so that while the overlay (or free
    // mode) is visible, the underlying Wayland client (e.g. a YouTube video)
    // does not also receive Space/J/K and pause/seek.
    //
    // EVIOCGRAB is per-client (per-open file description). The compositor's
    // libinput holds its own fd for each keyboard; grabbing on *any* of our
    // clients blocks the compositor. Crucially, the *grabbing client* still
    // receives events, while all other clients (including the compositor and
    // other apps' libinput clients) are blocked.
    //
    // The previous implementation opened a fresh fd, called grab(), then
    // immediately closed the fd – which released the grab instantly (the
    // kernel releases EVIOCGRAB on close). It also opened a *different*
    // client than the one our reader threads use, so when it *did* hold the
    // grab it starved our own readers.
    //
    // Correct fix: grab the *same* clients that the reader threads are
    // blocking on (their fds), and keep the grab held by not closing those
    // fds. We store the reader fds at startup and ioctl them directly from
    // any thread – any fd number refers to the same per-process file
    // description, so an ioctl from the main/IPC thread on a reader's fd
    // grabs that reader's client and keeps it privileged.
    let active_lock = GRAB_ACTIVE.get_or_init(|| Mutex::new(false));
    let mut active = active_lock.lock().unwrap();
    if *active == grab {
        return;
    }
    let Some(fds_lock) = GRAB_FDS.get() else {
        // Input thread hasn't populated GRAB_FDS yet (early IPC). Just track
        // desired state; the input thread will apply it once initialized.
        eprintln!(
            "mouseless: grab requested before input devices initialized (grab={})",
            grab
        );
        *active = grab;
        return;
    };
    let fds = fds_lock.lock().unwrap().clone();
    if fds.is_empty() {
        eprintln!("mouseless: no keyboard fds to grab (grab={})", grab);
        *active = grab;
        return;
    }
    let mut ok = 0usize;
    let mut failed = 0usize;
    for &fd in &fds {
        let ret = unsafe { libc::ioctl(fd, EVIOCGRAB as _, if grab { 1 } else { 0 }) };
        if ret == 0 {
            ok += 1;
        } else {
            let err = std::io::Error::last_os_error();
            // Already grabbed (EBUSY) when grabbing, or not grabbed when
            // ungrabbing, is not a fatal error – treat as success for
            // idempotency.
            if err.raw_os_error() == Some(libc::EBUSY) && grab {
                ok += 1;
            } else {
                failed += 1;
                eprintln!(
                    "mouseless: EVIOCGRAB fd {} grab={} failed: {} (ret {})",
                    fd, grab, err, ret
                );
            }
        }
    }
    if ok > 0 {
        *active = grab;
        eprintln!(
            "mouseless: keyboard {}",
            if grab { "grabbed (exclusive)" } else { "released" }
        );
    } else if failed > 0 {
        eprintln!("mouseless: keyboard grab failed");
    }
}

pub fn update_grab(ctx: &InputContext) {
    let should_grab = ctx.state.lock().unwrap().is_visible()
        || ctx.free_mode.lock().unwrap().is_active();
    set_keyboard_grab(should_grab);
}

pub fn update_grab_from_state(state: &Arc<Mutex<OverlayState>>, free_mode: &Arc<Mutex<FreeModeController>>) {
    let should_grab = state.lock().unwrap().is_visible() || free_mode.lock().unwrap().is_active();
    set_keyboard_grab(should_grab);
}

#[derive(Clone, Copy)]
struct RepeatEntry {
    label: &'static str,
    pressed_at: Instant,
    last_fired: Instant,
}

const REPEAT_DELAY: Duration = Duration::from_millis(450);
const REPEAT_INTERVAL: Duration = Duration::from_millis(55);

/// Shared per-event state across all keyboard device threads.
#[derive(Default)]
struct InputShared {
    modifiers: Modifiers,
    repeats: HashMap<u16, RepeatEntry>,
    /// Many keyboards expose two evdev nodes that both report letter keys
    /// (e.g. "X" and "X Keyboard"); the same physical press then arrives once
    /// per node, microseconds apart. Dedupe identical events in this window.
    last_events: HashMap<(u16, i32), Instant>,
    /// Last time the overlay was toggled via hotkey, for debouncing.
    last_toggle: Option<Instant>,
}

/// Window for collapsing twin-device duplicate events. Duplicates arrive
/// within microseconds to a few milliseconds; humans cannot re-press a key this fast.
const DEDUPE_WINDOW: Duration = Duration::from_millis(50);
const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(250);

/// Returns whether this is a second report of the same physical event from a
/// keyboard's companion evdev node. Some keyboards expose, for example, a
/// regular keyboard node and a consumer-control node which both report the
/// letter keys. Without this guard, one physical press can toggle the overlay
/// twice or select a grid cell immediately after opening it.
fn is_duplicate_event(
    shared: &mut InputShared,
    code: u16,
    value: i32,
    now: Instant,
) -> bool {
    let key = (code, value);
    let duplicate = shared
        .last_events
        .get(&key)
        .is_some_and(|previous| now.duration_since(*previous) < DEDUPE_WINDOW);
    if !duplicate {
        shared.last_events.insert(key, now);
    }
    // Keep this bounded during long-running sessions. Events older than the
    // dedupe window cannot influence a future key press.
    shared
        .last_events
        .retain(|_, previous| now.duration_since(*previous) < DEDUPE_WINDOW);
    duplicate
}

#[derive(Clone)]
pub struct InputContext {
    pub settings: Arc<Mutex<Settings>>,
    pub state: Arc<Mutex<OverlayState>>,
    pub free_mode: Arc<Mutex<FreeModeController>>,
    pub tx: Sender<ControlMsg>,
}

pub fn run_input(ctx: InputContext) -> Result<(), String> {
    let devices = open_keyboards();
    if devices.is_empty() {
        return Err(
            "No keyboard devices found in /dev/input. Add yourself to the `input` \
             group and log out and back in: `sudo usermod -aG input $USER`"
                .to_string(),
        );
    }

    // Store the reader fds for EVIOCGRAB. set_keyboard_grab() will ioctl these
    // exact fds so the grabbing client is the one the readers block on (the
    // privileged client that still receives events while all other clients –
    // including the compositor's libinput – are blocked).
    {
        let grab_fds = GRAB_FDS.get_or_init(|| Mutex::new(Vec::new()));
        let mut fds = grab_fds.lock().unwrap();
        fds.clear();
        for dev in &devices {
            fds.push(dev.as_raw_fd());
        }
        let active = GRAB_ACTIVE.get_or_init(|| Mutex::new(false));
        *active.lock().unwrap() = false;
        eprintln!("mouseless: monitoring keyboard, fds {:?}", *fds);
    }

    let shared = Arc::new(Mutex::new(InputShared::default()));

    // Auto-repeat ticker: the compositor does not generate repeats while our
    // layer surface holds exclusive keyboard focus, so the input side does it.
    {
        let ctx = ctx.clone();
        let shared = shared.clone();
        std::thread::Builder::new()
            .name("repeat".to_string())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(10));
                let now = Instant::now();
                let due: Vec<(u16, &'static str)> = {
                    let mut guard = shared.lock().unwrap();
                    let mut due = Vec::new();
                    guard.repeats.retain(|code, entry| {
                        // Keep the entry so the key keeps repeating while it is
                        // held; the key-up event removes it.
                        if now.duration_since(entry.pressed_at) >= REPEAT_DELAY
                            && now.duration_since(entry.last_fired) >= REPEAT_INTERVAL
                        {
                            entry.last_fired = now;
                            due.push((*code, entry.label));
                        }
                        true
                    });
                    due
                };
                for (code, label) in due {
                    if !route_to_state_machines(&ctx, label, true) {
                        shared.lock().unwrap().repeats.remove(&code);
                    }
                }
            })
            .ok();
    }

    // One blocking reader thread per keyboard device (evdev has no
    // non-blocking mode), all feeding the same handler.
    let mut handles = Vec::new();
    for mut dev in devices {
        let ctx = ctx.clone();
        let shared = shared.clone();
        let name = dev.name().unwrap_or_default();
        let handle = std::thread::Builder::new()
            .name(format!("kbd:{}", name))
            .spawn(move || loop {
                match dev.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() == evdev::EventType::KEY {
                                handle_event(&ctx, &shared, ev.code(), ev.value());
                            }
                        }
                    }
                    Err(_) => return, // device vanished
                }
            });
        if let Ok(handle) = handle {
            handles.push(handle);
        }
    }
    for handle in handles {
        let _ = handle.join();
    }
    Ok(())
}

fn handle_event(ctx: &InputContext, shared: &Mutex<InputShared>, code: u16, value: i32) {
    // evdev already sends key-repeat events (value 2). They previously went
    // through as ordinary grid selections, so holding Alt+U long enough to
    // open the overlay also selected U about half a second later, producing a
    // second, smaller grid. Repeating is handled below in a controlled way
    // for navigation/scrolling keys only.
    if value == 2 {
        return;
    }

    let is_down = value != 0;
    let fresh_press = value == 1;

    let Some(label) = label_for_code(code) else {
        return;
    };

    let mut guard = shared.lock().unwrap();
    let now = Instant::now();
    if is_duplicate_event(&mut guard, code, value, now) {
        return;
    }
    if update_modifiers(&mut guard.modifiers, code, is_down) {
        return;
    }

    if fresh_press {
        let s = ctx.settings.lock().unwrap();
        let (ctrl, alt, shift, win) = {
            let m = &guard.modifiers;
            (m.control, m.alt, m.shift, m.win)
        };
        let is_hotkey = s.overlay_hotkey.matches(label, ctrl, alt, shift, win);
        drop(s);

        if is_hotkey {
            let can_toggle = guard
                .last_toggle
                .map(|t| now.duration_since(t) >= TOGGLE_DEBOUNCE)
                .unwrap_or(true);
            if can_toggle {
                guard.last_toggle = Some(now);
                let mut state = ctx.state.lock().unwrap();
                if state.is_visible() {
                    state.hide();
                    drop(state);
                    let _ = ctx.tx.send(ControlMsg::HideOverlay);
                } else {
                    state.show();
                    drop(state);
                    let _ = ctx.tx.send(ControlMsg::ToggleOverlay);
                }
                drop(guard);
                update_grab(ctx);
                return;
            }
            guard.repeats.remove(&code);
            return;
        }
    }

    {
        let fm_active = ctx.free_mode.lock().unwrap().is_active();
        if fm_active {
            if is_down {
                drop(guard);
                let s = ctx.settings.lock().unwrap();
                let mut fm = ctx.free_mode.lock().unwrap();
                let was_active_before = fm.is_active();
                let _ = fm.handle(label, &s);
                let pending = fm.take_pending_scroll();
                let should_repeat = fm.is_repeatable_key(label);
                let now_active = fm.is_active();
                drop(fm);
                drop(s);
                if let Some((v, h)) = pending {
                    let _ = ctx.tx.send(ControlMsg::Scroll { vertical: v, horizontal: h });
                }
                // Free mode exit (Escape) or enter changes grab state.
                if was_active_before != now_active {
                    update_grab(ctx);
                }
                let mut guard = shared.lock().unwrap();
                if fresh_press && should_repeat {
                    guard.repeats.insert(
                        code,
                        RepeatEntry {
                            label,
                            pressed_at: Instant::now(),
                            last_fired: Instant::now(),
                        },
                    );
                } else if fresh_press {
                    guard.repeats.remove(&code);
                }
            } else {
                guard.repeats.remove(&code);
            }
            return;
        }
    }

    {
        let shift_held = guard.modifiers.shift;
        let mut state = ctx.state.lock().unwrap();
        if state.is_visible() {
            let was_visible = state.is_visible();
            let hide = state.handle_key_with_modifiers(label, is_down, shift_held);
            let pending = state.take_pending_scroll();
            let should_repeat = is_down && state.is_repeatable_key(label);
            let now_visible = state.is_visible();
            drop(state);
            if let Some((x, y, v, h)) = pending {
                let _ = ctx.tx.send(ControlMsg::ScrollAt { x, y, vertical: v, horizontal: h });
            }
            if hide {
                guard.last_toggle = Some(now);
                let _ = ctx.tx.send(ControlMsg::HideOverlay);
            } else if is_down || label == "Space" {
                // Key-down or Space release (exiting scroll mode) modifies overlay visuals.
                let _ = ctx.tx.send(ControlMsg::RenderOverlay);
            }
            if was_visible != now_visible {
                update_grab(ctx);
            }
            if fresh_press && should_repeat {
                guard.repeats.insert(
                    code,
                    RepeatEntry {
                        label,
                        pressed_at: Instant::now(),
                        last_fired: Instant::now(),
                    },
                );
            } else if fresh_press {
                guard.repeats.remove(&code);
            } else if !is_down {
                guard.repeats.remove(&code);
            }
            return;
        }
    }

    // Not handled by us: the compositor delivers the key normally.
    guard.repeats.remove(&code);
}

/// Re-inject a repeat only while that action is still safe to repeat. This
/// deliberately excludes grid labels and click keys: repeated labels would
/// keep refining the target without a new physical press.
fn route_to_state_machines(ctx: &InputContext, label: &'static str, is_down: bool) -> bool {
    let fm_active = ctx.free_mode.lock().unwrap().is_active();
    if fm_active {
        // Keep the same settings -> free-mode lock order as the physical-key
        // path above, otherwise a repeat tick could deadlock with a key event.
        let s = ctx.settings.lock().unwrap();
        let mut fm = ctx.free_mode.lock().unwrap();
        if !fm.is_active() || !fm.is_repeatable_key(label) {
            return false;
        }
        let was_active = fm.is_active();
        let _ = fm.handle(label, &s);
        let pending = fm.take_pending_scroll();
        let now_active = fm.is_active();
        drop(fm);
        drop(s);
        if let Some((v, h)) = pending {
            let _ = ctx.tx.send(ControlMsg::Scroll { vertical: v, horizontal: h });
        }
        if was_active != now_active {
            update_grab(ctx);
        }
        return now_active && ctx.free_mode.lock().unwrap().is_repeatable_key(label);
    }

    let mut state = ctx.state.lock().unwrap();
    if !state.is_visible() || !state.is_repeatable_key(label) {
        return false;
    }
    let was_visible = state.is_visible();
    let hide = state.handle_key(label, is_down);
    let pending = state.take_pending_scroll();
    let now_visible = state.is_visible();
    let keep_repeating = !hide && now_visible && state.is_repeatable_key(label);
    drop(state);
    if let Some((x, y, v, h)) = pending {
        let _ = ctx.tx.send(ControlMsg::ScrollAt { x, y, vertical: v, horizontal: h });
    }
    if was_visible != now_visible {
        update_grab(ctx);
    }
    let _ = ctx.tx.send(if hide {
        ControlMsg::HideOverlay
    } else {
        ControlMsg::RenderOverlay
    });
    keep_repeating
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{MouseButton, Rect};
    use crate::core::{ClickDetector, Environment, MouseBackend};
    use crate::settings::Settings;
    use std::sync::mpsc::channel;

    struct TestMouse;

    impl MouseBackend for TestMouse {
        fn move_to(&self, _: f64, _: f64) {}
        fn click(&self, _: f64, _: f64, _: MouseButton, _: u32) {}
        fn toggle_drag(&self, _: f64, _: f64) {}
        fn move_drag(&self, _: f64, _: f64) {}
        fn scroll(&self, _: i32, _: i32) {}
        fn scroll_at(&self, _: f64, _: f64, _: i32, _: i32) {}
        fn is_dragging(&self) -> bool {
            false
        }
    }

    struct TestDetector;

    impl ClickDetector for TestDetector {
        fn snap_to_clickable(&self, _: f64, _: f64) -> Option<(f64, f64)> {
            None
        }
    }

    struct TestEnvironment;

    impl Environment for TestEnvironment {
        fn monitors(&self) -> Vec<Rect> {
            vec![Rect::new(0.0, 0.0, 1920.0, 1080.0)]
        }

        fn cursor_pos(&self) -> (f64, f64) {
            (960.0, 540.0)
        }
    }

    #[test]
    fn collapses_twin_keyboard_events_but_not_a_later_press() {
        let start = Instant::now();
        let mut shared = InputShared::default();

        assert!(!is_duplicate_event(&mut shared, 22, 1, start));
        assert!(is_duplicate_event(
            &mut shared,
            22,
            1,
            start + Duration::from_millis(5)
        ));
        assert!(is_duplicate_event(
            &mut shared,
            22,
            1,
            start + Duration::from_millis(35)
        ));
        assert!(!is_duplicate_event(
            &mut shared,
            22,
            1,
            start + Duration::from_millis(60)
        ));
    }

    #[test]
    fn holding_the_hotkey_does_not_refine_the_new_grid() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let state = Arc::new(Mutex::new(OverlayState::new(
            settings.clone(),
            Arc::new(TestMouse),
            Arc::new(TestDetector),
            Arc::new(TestEnvironment),
        )));
        let (tx, rx) = channel();
        let ctx = InputContext {
            settings,
            state: state.clone(),
            free_mode: Arc::new(Mutex::new(FreeModeController::new(
                Arc::new(TestMouse),
                Arc::new(TestEnvironment),
                Arc::new(|_| {}),
            ))),
            tx,
        };
        let shared = Mutex::new(InputShared::default());

        handle_event(&ctx, &shared, KEY_LEFTALT, 1);
        handle_event(&ctx, &shared, 22, 1); // U: opens Alt+U overlay
        assert!(matches!(rx.try_recv(), Ok(ControlMsg::ToggleOverlay)));
        let original_region = state.lock().unwrap().snapshot().active_region;

        handle_event(&ctx, &shared, 22, 2); // evdev autorepeat for held U

        assert_eq!(state.lock().unwrap().snapshot().active_region, original_region);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn pressing_u_without_alt_refines_grid_when_visible() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let state = Arc::new(Mutex::new(OverlayState::new(
            settings.clone(),
            Arc::new(TestMouse),
            Arc::new(TestDetector),
            Arc::new(TestEnvironment),
        )));
        let (tx, rx) = channel();
        let ctx = InputContext {
            settings,
            state: state.clone(),
            free_mode: Arc::new(Mutex::new(FreeModeController::new(
                Arc::new(TestMouse),
                Arc::new(TestEnvironment),
                Arc::new(|_| {}),
            ))),
            tx,
        };
        let shared = Mutex::new(InputShared::default());

        // Open overlay with Alt+U
        handle_event(&ctx, &shared, KEY_LEFTALT, 1);
        handle_event(&ctx, &shared, 22, 1); // U down
        assert!(matches!(rx.try_recv(), Ok(ControlMsg::ToggleOverlay)));

        // Release U and Alt
        handle_event(&ctx, &shared, 22, 0); // U up
        handle_event(&ctx, &shared, KEY_LEFTALT, 0); // Alt up

        let initial_region = state.lock().unwrap().snapshot().active_region;

        // Press 'A' to refine into cell 'A'
        handle_event(&ctx, &shared, 30, 1); // A down
        assert!(matches!(rx.try_recv(), Ok(ControlMsg::RenderOverlay)));
        let cell_a_region = state.lock().unwrap().snapshot().active_region;
        assert_ne!(initial_region, cell_a_region);
        assert!(state.lock().unwrap().is_visible());

        // Press 'S' to refine further into cell 'S'
        handle_event(&ctx, &shared, 31, 1); // S down
        assert!(matches!(rx.try_recv(), Ok(ControlMsg::RenderOverlay)));
        let cell_s_region = state.lock().unwrap().snapshot().active_region;
        assert_ne!(cell_a_region, cell_s_region);
        assert!(state.lock().unwrap().is_visible());
    }
}
