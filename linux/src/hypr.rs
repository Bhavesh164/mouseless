use crate::common::Rect;
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// Minimal Hyprland IPC client (the same requests `hyprctl` issues).
#[derive(Clone)]
pub struct Hyprland {
    socket: PathBuf,
}

impl Hyprland {
    pub fn detect() -> Option<Self> {
        let runtime = env::var("XDG_RUNTIME_DIR").ok()?;
        let sig = env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let socket = PathBuf::from(runtime)
            .join("hypr")
            .join(sig)
            .join(".socket.sock");
        UnixStream::connect(&socket).ok()?;
        Some(Hyprland { socket })
    }

    fn request(&self, cmd: &str) -> Option<String> {
        let mut stream = UnixStream::connect(&self.socket).ok()?;
        let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
        stream.write_all(cmd.as_bytes()).ok()?;
        stream.write_all(b"\n").ok()?;
        let mut buf = String::new();
        stream.read_to_string(&mut buf).ok()?;
        Some(buf)
    }

    /// Monitors with names, in global layout coordinates (logical pixels).
    pub fn monitors_named(&self) -> Option<Vec<(String, Rect)>> {
        let text = self.request("j/monitors")?;
        let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
        let arr = parsed.as_array()?;
        let mut out = Vec::new();
        for m in arr {
            if m.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let name = m.get("name")?.as_str()?.to_string();
            let x = m.get("x")?.as_f64()?;
            let y = m.get("y")?.as_f64()?;
            let width = m.get("width")?.as_f64()?;
            let height = m.get("height")?.as_f64()?;
            let scale = m.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let scale = if scale > 0.0 { scale } else { 1.0 };
            out.push((name, Rect::new(x, y, x + width / scale, y + height / scale)));
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// Monitors in global layout coordinates (logical pixels).
    pub fn monitors(&self) -> Option<Vec<Rect>> {
        self.monitors_named().map(|list| list.into_iter().map(|(_, r)| r).collect())
    }

    pub fn cursor_pos(&self) -> Option<(f64, f64)> {
        // `cursorpos` is not exposed over the raw IPC socket on all Hyprland
        // versions, so go through the hyprctl CLI ("x, y").
        let output = std::process::Command::new("hyprctl")
            .arg("cursorpos")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut parts = text.trim().split(',');
        let x = parts.next()?.trim().parse::<f64>().ok()?;
        let y = parts.next()?.trim().parse::<f64>().ok()?;
        Some((x, y))
    }
}
