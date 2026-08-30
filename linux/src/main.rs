mod accessibility;
mod common;
mod core;
mod free_mode;
mod hypr;
mod input;
mod mouse;
mod overlay;
mod render;
mod settings;

mod virtual_pointer {
    #![allow(dead_code, non_camel_case_types, unused_variables, non_upper_case_globals, non_snake_case, unused_imports)]

    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/virtual-pointer.xml");
    }
    use self::__interfaces::*;

    pub mod client {
        use super::*;
        wayland_scanner::generate_client_code!("protocols/virtual-pointer.xml");
    }
}

use crate::common::Rect;
use crate::core::{MouseBackend, OverlayState};
use crate::free_mode::FreeModeController;
use crate::hypr::Hyprland;
use crate::input::InputContext;
use crate::mouse::{SystemEnvironment, VirtualPointerMouse};
use crate::overlay::Srv;
use crate::settings::{Settings, SettingsStore};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

#[derive(Debug)]
pub enum ControlMsg {
    ToggleOverlay,
    RenderOverlay,
    HideOverlay,
    FreeOn,
    FreeOff,
    Scroll { vertical: i32, horizontal: i32 },
    ScrollAt { x: f64, y: f64, vertical: i32, horizontal: i32 },
    Exit,
}

const HELP: &str = "\
mouseless - keyboard-driven mouse controller (Linux/Wayland)

Usage:
  mouseless                 start the daemon
  mouseless toggle          toggle the grid overlay
  mouseless show | hide     show or hide the grid overlay
  mouseless free            toggle free mode (HJKL move, Space/R click, U/D/Y/O scroll)
  mouseless status          print daemon status
  mouseless reload          reload config from ~/.config/Mouseless/config.json
  mouseless quit            stop the daemon
  mouseless warp <x> <y>    diagnostic: move cursor, report position
  mouseless scrolltest      diagnostic: verify scroll synthesis works

Hotkey: Alt+U by default (configurable via overlayHotkey in config.json).
";

fn socket_path() -> PathBuf {
    // Fallback must not include the pid: the daemon and the CLI subcommands
    // are different processes and would compute different socket paths.
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let uid = unsafe { libc::getuid() };
    PathBuf::from(runtime).join(format!("mouseless-{}.sock", uid))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", HELP);
        return;
    }
    let result = match args.first().map(|s| s.as_str()) {
        None => daemon(),
        Some("warp") => {
            let Some(x) = args.get(1).and_then(|v| v.parse::<f64>().ok()) else {
                eprintln!("usage: mouseless warp <x> <y>");
                return;
            };
            let Some(y) = args.get(2).and_then(|v| v.parse::<f64>().ok()) else {
                eprintln!("usage: mouseless warp <x> <y>");
                return;
            };
            warp(x, y)
        }
        Some("scrolltest") => scrolltest(),
        Some("key") => {
            let label = args.get(1).cloned().unwrap_or_default();
            if label.is_empty() {
                eprintln!("usage: mouseless key <label>");
                return;
            }
            send_command(&format!("key {}", label))
        }
        Some("keyup") => {
            let label = args.get(1).cloned().unwrap_or_default();
            if label.is_empty() {
                eprintln!("usage: mouseless keyup <label>");
                return;
            }
            send_command(&format!("keyup {}", label))
        }
        Some("scroll_at") => {
            let x = args.get(1).cloned().unwrap_or_else(|| "683".to_string());
            let y = args.get(2).cloned().unwrap_or_else(|| "384".to_string());
            let v = args.get(3).cloned().unwrap_or_else(|| "54".to_string());
            let h = args.get(4).cloned().unwrap_or_else(|| "0".to_string());
            send_command(&format!("scroll_at {} {} {} {}", x, y, v, h))
        }
        Some("scroll") => {
            let v = args.get(1).cloned().unwrap_or_else(|| "54".to_string());
            let h = args.get(2).cloned().unwrap_or_else(|| "0".to_string());
            send_command(&format!("scroll {} {}", v, h))
        }
        Some(cmd) => send_command(cmd),
    };
    if let Err(e) = result {
        eprintln!("mouseless: {}", e);
        std::process::exit(1);
    }
}

/// Diagnostic: warps the cursor to (x, y) via a virtual pointer and reports
/// the resulting position. Verifies pointer synthesis end-to-end.
fn warp(x: f64, y: f64) -> Result<(), String> {
    let conn = wayland_client::Connection::connect_to_env()
        .map_err(|e| format!("cannot connect to Wayland display: {e}"))?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());
    let mut srv = Srv::new();
    event_queue
        .roundtrip(&mut srv)
        .map_err(|e| format!("wayland setup failed: {e}"))?;
    let mgr = srv
        .globals
        .vp_manager
        .clone()
        .ok_or("zwlr_virtual_pointer_v1 is not available")?;
    let seat = srv.globals.seat.clone().ok_or("no wl_seat on the display")?;
    let vp = mgr.create_virtual_pointer(Some(&seat), &qh, ());
    let hypr = Hyprland::detect();
    let before = hypr
        .as_ref()
        .and_then(|h| h.cursor_pos())
        .ok_or("cannot query the cursor position (hyprctl missing?)")?;
    let mouse = VirtualPointerMouse::new(conn.clone(), Some(vp), None, before);
    mouse.move_to(x, y);
    let _ = conn.flush();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let after = hypr
        .as_ref()
        .and_then(|h| h.cursor_pos())
        .unwrap_or((f64::NAN, f64::NAN));
    println!(
        "cursor {:.0},{:.0} -> requested {:.0},{:.0} -> got {:.0},{:.0}",
        before.0, before.1, x, y, after.0, after.1
    );
    Ok(())
}

/// Diagnostic: sends a small scroll through a virtual pointer and checks the
/// Wayland connection survives. Exercises the exact requests used by the grid
/// scroll keys, including the version-gated `axis_discrete`.
fn scrolltest() -> Result<(), String> {
    let conn = wayland_client::Connection::connect_to_env()
        .map_err(|e| format!("cannot connect to Wayland display: {e}"))?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());
    let mut srv = Srv::new();
    event_queue
        .roundtrip(&mut srv)
        .map_err(|e| format!("wayland setup failed: {e}"))?;
    let mgr = srv
        .globals
        .vp_manager
        .clone()
        .ok_or("zwlr_virtual_pointer_v1 is not available")?;
    let seat = srv.globals.seat.clone().ok_or("no wl_seat on the display")?;
    let vp = mgr.create_virtual_pointer(Some(&seat), &qh, ());
    let version = srv.globals.vp_manager_version;
    let mouse = VirtualPointerMouse::new(conn.clone(), Some(vp), None, (0.0, 0.0));
    mouse.scroll(12, 0);
    let _ = conn.flush();
    // If the compositor rejected any request above, the roundtrip fails.
    event_queue
        .roundtrip(&mut srv)
        .map_err(|e| format!("scroll was rejected by the compositor: {e}"))?;
    println!(
        "scroll ok: virtual-pointer v{} (axis_discrete {})",
        version,
        if version >= 2 { "sent" } else { "skipped (compositor binds v1)" }
    );
    Ok(())
}

fn send_command(cmd: &str) -> Result<(), String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|_| "daemon is not running (start it with `mouseless`)".to_string())?;
    stream
        .write_all(cmd.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("failed to send command: {e}"))?;
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    if !reply.trim().is_empty() {
        println!("{}", reply.trim());
    }
    Ok(())
}

/// Removes a stale socket file if no live daemon is listening.
fn prepare_socket(path: &PathBuf) -> Result<UnixListener, String> {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err("another mouseless daemon is already running".to_string());
        }
        let _ = std::fs::remove_file(path);
    }
    UnixListener::bind(path).map_err(|e| format!("cannot bind IPC socket {:?}: {e}", path))
}

fn daemon() -> Result<(), String> {
    let store = SettingsStore::new();
    let store = Arc::new(Mutex::new(store));
    let settings = Arc::new(Mutex::new(store.lock().unwrap().settings.clone()));

    let conn = wayland_client::Connection::connect_to_env()
        .map_err(|e| format!("cannot connect to Wayland display: {e}"))?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());
    let mut srv = Srv::new();
    event_queue
        .roundtrip(&mut srv)
        .map_err(|e| format!("wayland setup failed: {e}"))?;
    // Binds are issued while dispatching the first roundtrip's globals, so
    // their events (wl_output name/mode/scale) arrive after it completes.
    event_queue
        .roundtrip(&mut srv)
        .map_err(|e| format!("wayland setup failed: {e}"))?;

    let hypr = Hyprland::detect();
    let hypr_monitors = hypr.as_ref().and_then(|h| h.monitors_named());

    // Per-output rects (logical layout coordinates) + integer buffer scale.
    let mut rects: Vec<(String, Rect, i32)> = Vec::new();
    match &hypr_monitors {
        Some(list) => {
            for (name, rect) in list {
                if let Some(o) = srv.outputs.iter().find(|o| &o.name == name) {
                    rects.push((name.clone(), *rect, o.scale.max(1)));
                }
            }
        }
        None => {
            let mut cursor_x = 0.0;
            for o in &srv.outputs {
                let scale = o.scale.max(1) as f64;
                let w = (o.width as f64 / scale).max(1.0);
                let h = (o.height as f64 / scale).max(1.0);
                rects.push((
                    o.name.clone(),
                    Rect::new(cursor_x, 0.0, cursor_x + w, h),
                    o.scale.max(1),
                ));
                cursor_x += w;
            }
        }
    }
    if rects.is_empty() {
        return Err("no outputs found on the Wayland display".to_string());
    }

    srv.setup_surfaces(rects.clone(), &qh);
    event_queue
        .roundtrip(&mut srv)
        .map_err(|e| format!("wayland setup failed: {e}"))?;

    // Virtual pointer for mouse synthesis.
    let vp = match (&srv.globals.vp_manager, &srv.globals.seat) {
        (Some(mgr), Some(seat)) => Some(mgr.create_virtual_pointer(Some(&seat), &qh, ())),
        _ => None,
    };
    let Some(vp) = vp else {
        return Err(
            "zwlr_virtual_pointer_v1 is not supported by this compositor; \
             cannot synthesize mouse input"
                .to_string(),
        );
    };

    let first_rect = rects[0].1;
    let initial_pos = hypr
        .as_ref()
        .and_then(|h| h.cursor_pos())
        .unwrap_or((first_rect.mid_x(), first_rect.mid_y()));

    let mouse = Arc::new(VirtualPointerMouse::new(
        conn.clone(),
        Some(vp),
        hypr.clone(),
        initial_pos,
    ));
    let env = Arc::new(SystemEnvironment::new(
        hypr.clone(),
        rects.iter().map(|(_, r, _)| *r).collect(),
        mouse.position_tracker(),
    ));
    let detector = Arc::new(accessibility::AtspiDetector::new());
    let state = Arc::new(Mutex::new(OverlayState::new(
        settings.clone(),
        mouse.clone(),
        detector,
        env.clone(),
    )));

    let (tx, rx) = channel::<ControlMsg>();

    let notify_tx = tx.clone();
    let notify: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |text: &str| {
        if text.ends_with("off") {
            let _ = notify_tx.send(ControlMsg::FreeOff);
        } else {
            let _ = notify_tx.send(ControlMsg::FreeOn);
        }
        eprintln!("mouseless: {}", text);
    });
    let free_mode = Arc::new(Mutex::new(FreeModeController::new(
        mouse.clone(),
        env.clone(),
        notify,
    )));

    // Input thread: evdev keyboard monitoring + hotkey + state machine.
    {
        let ctx = InputContext {
            settings: settings.clone(),
            state: state.clone(),
            free_mode: free_mode.clone(),
            tx: tx.clone(),
        };
        std::thread::Builder::new()
            .name("input".to_string())
            .spawn(move || {
                if let Err(e) = input::run_input(ctx) {
                    eprintln!("mouseless: {}", e);
                    std::process::exit(1);
                }
            })
            .map_err(|e| format!("failed to spawn input thread: {e}"))?;
    }

    // IPC listener thread.
    {
        let path = socket_path();
        let listener = prepare_socket(&path)?;
        let tx = tx.clone();
        let state = state.clone();
        let settings = settings.clone();
        let store = store.clone();
        let free_mode = free_mode.clone();
        let mouse = mouse.clone();
        std::thread::Builder::new()
            .name("ipc".to_string())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    handle_ipc(
                        stream,
                        &tx,
                        &state,
                        &settings,
                        &store,
                        &free_mode,
                        &mouse,
                    );
                }
            })
            .ok();
    }

    let hotkey = settings.lock().unwrap().overlay_hotkey.display_name();
    let layer_shell = if srv.globals.layer_shell.is_some() {
        "layer-shell ok"
    } else {
        "layer-shell MISSING"
    };
    let pointer = if srv.globals.vp_manager.is_some() {
        "virtual-pointer ok"
    } else {
        "virtual-pointer MISSING"
    };
    let hypr_note = if hypr.is_some() {
        "Hyprland IPC ok"
    } else {
        "Hyprland IPC unavailable (fallback to Wayland layout)"
    };
    eprintln!(
        "mouseless: ready  hotkey={}  {}  {}  {}",
        hotkey, layer_shell, pointer, hypr_note
    );

    while let Ok(msg) = rx.recv() {
        match msg {
            ControlMsg::ToggleOverlay => {
                let snapshot = state.lock().unwrap().snapshot();
                srv.show_overlays(snapshot);
                event_queue
                    .roundtrip(&mut srv)
                    .map_err(|e| format!("wayland connection lost: {e}"))?;
                // The configure event is dispatched during the roundtrip and
                // render_window queues attach+commit while handling it; those
                // requests are only buffered, so flush them explicitly or the
                // first frame never reaches the compositor.
                let _ = conn.flush();
            }
            ControlMsg::RenderOverlay => {
                let snapshot = state.lock().unwrap().snapshot();
                srv.render(&snapshot);
                let _ = conn.flush();
                let _ = event_queue.dispatch_pending(&mut srv);
            }
            ControlMsg::HideOverlay => {
                srv.hide_overlays();
                event_queue
                    .roundtrip(&mut srv)
                    .map_err(|e| format!("wayland connection lost: {e}"))?;
            }
            ControlMsg::FreeOn => {
                {
                    let mut st = state.lock().unwrap();
                    if st.is_visible() {
                        st.hide();
                    }
                }
                srv.hide_overlays();
                srv.map_free_surface();
                event_queue
                    .roundtrip(&mut srv)
                    .map_err(|e| format!("wayland connection lost: {e}"))?;
                let _ = conn.flush();
            }
            ControlMsg::FreeOff => {
                srv.unmap_free_surface();
                event_queue
                    .roundtrip(&mut srv)
                    .map_err(|e| format!("wayland connection lost: {e}"))?;
                let _ = conn.flush();
            }
            ControlMsg::Scroll { vertical, horizontal } => {
                eprintln!("mouseless: scroll {} {}", vertical, horizontal);
                mouse.scroll(vertical, horizontal);
            }
            ControlMsg::ScrollAt { x, y, vertical, horizontal } => {
                eprintln!("mouseless: scroll_at {} {} {} {}", x, y, vertical, horizontal);
                mouse.scroll_at(x, y, vertical, horizontal);
            }
            ControlMsg::Exit => break,
        }
    }

    srv.hide_overlays();
    srv.unmap_free_surface();
    let _ = conn.flush();
    let _ = std::fs::remove_file(socket_path());
    eprintln!("mouseless: bye");
    Ok(())
}

fn handle_ipc(
    stream: UnixStream,
    tx: &Sender<ControlMsg>,
    state: &Arc<Mutex<OverlayState>>,
    settings: &Arc<Mutex<Settings>>,
    store: &Arc<Mutex<SettingsStore>>,
    free_mode: &Arc<Mutex<FreeModeController>>,
    _mouse: &Arc<VirtualPointerMouse>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let cmd = line.trim();
    let reply = match cmd {
        "toggle" => {
            let visible = state.lock().unwrap().is_visible();
            if visible {
                state.lock().unwrap().hide();
                let _ = tx.send(ControlMsg::HideOverlay);
            } else {
                state.lock().unwrap().show();
                let _ = tx.send(ControlMsg::ToggleOverlay);
            }
            crate::input::update_grab_from_state(state, free_mode);
            if visible { "hidden".to_string() } else { "shown".to_string() }
        }
        "show" => {
            let mut st = state.lock().unwrap();
            if !st.is_visible() {
                st.show();
                let _ = tx.send(ControlMsg::ToggleOverlay);
            }
            drop(st);
            crate::input::update_grab_from_state(state, free_mode);
            "shown".to_string()
        }
        "hide" => {
            let mut st = state.lock().unwrap();
            st.hide();
            let _ = tx.send(ControlMsg::HideOverlay);
            drop(st);
            crate::input::update_grab_from_state(state, free_mode);
            "hidden".to_string()
        }
        "free" => {
            free_mode.lock().unwrap().toggle();
            crate::input::update_grab_from_state(state, free_mode);
            "ok".to_string()
        }
        "reload" => {
            let new_settings = {
                let mut st = store.lock().unwrap();
                st.reload();
                st.settings.clone()
            };
            *settings.lock().unwrap() = new_settings;
            "ok".to_string()
        }
        "status" => {
            let visible = state.lock().unwrap().is_visible();
            let free = free_mode.lock().unwrap().is_active();
            let hotkey = settings.lock().unwrap().overlay_hotkey.display_name();
            format!(
                "overlay={} free-mode={} hotkey={}",
                if visible { "visible" } else { "hidden" },
                if free { "active" } else { "inactive" },
                hotkey
            )
        }
        cmd if cmd.starts_with("scroll_at") => {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            let x: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(683.0);
            let y: f64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(384.0);
            let v: i32 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(54);
            let h: i32 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            let _ = tx.send(ControlMsg::ScrollAt { x, y, vertical: v, horizontal: h });
            format!("ok scroll_at {} {} {} {}", x, y, v, h)
        }
        cmd if cmd.starts_with("scroll") => {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            let v: i32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(54);
            let h: i32 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let _ = tx.send(ControlMsg::Scroll { vertical: v, horizontal: h });
            "ok".to_string()
        }
        cmd if cmd.starts_with("key ") => {
            let label = cmd[4..].trim();
            let mut st = state.lock().unwrap();
            if !st.is_visible() {
                "not visible".to_string()
            } else {
                let hide = st.handle_key(label, true);
                let pending = st.take_pending_scroll();
                let snap = st.snapshot();
                drop(st);
                if let Some((x, y, v, h)) = pending {
                    let _ = tx.send(ControlMsg::ScrollAt { x, y, vertical: v, horizontal: h });
                }
                if hide {
                    let _ = tx.send(ControlMsg::HideOverlay);
                } else {
                    let _ = tx.send(ControlMsg::RenderOverlay);
                }
                format!(
                    "ok {} precision={} region={:?} cursor={:?} status={}",
                    label, snap.precision_mode, snap.active_region, snap.cursor, snap.status
                )
            }
        }
        cmd if cmd.starts_with("keyup ") => {
            let label = cmd[6..].trim();
            let mut st = state.lock().unwrap();
            if !st.is_visible() {
                "not visible".to_string()
            } else {
                let hide = st.handle_key(label, false);
                let pending = st.take_pending_scroll();
                let snap = st.snapshot();
                drop(st);
                if let Some((x, y, v, h)) = pending {
                    let _ = tx.send(ControlMsg::ScrollAt { x, y, vertical: v, horizontal: h });
                }
                if hide {
                    let _ = tx.send(ControlMsg::HideOverlay);
                } else {
                    let _ = tx.send(ControlMsg::RenderOverlay);
                }
                format!(
                    "ok keyup {} precision={} status={}",
                    label, snap.precision_mode, snap.status
                )
            }
        }
        cmd if cmd == "snapshot" => {
            let snap = state.lock().unwrap().snapshot();
            format!(
                "visible={} precision={} region={:?} cursor={:?} status={}",
                state.lock().unwrap().is_visible(),
                snap.precision_mode,
                snap.active_region,
                snap.cursor,
                snap.status
            )
        }
        "quit" => {
            let _ = tx.send(ControlMsg::Exit);
            "ok".to_string()
        }
        other => format!("unknown command: {}", other),
    };
    let _ = reader.get_mut().write_all(reply.as_bytes());
}
