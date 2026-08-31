# Mouseless for Linux (Wayland)

Keyboard-driven mouse controller — the Linux/Wayland port of the macOS
[Mouseless](../README.md) app. Press a global hotkey (default `Alt+U`) to open a
recursive keyboard grid over the screen, refine it with letter keys, and click
without touching the mouse.

## Prerequisites & System Requirements

### 1. Wayland Compositor
A Wayland compositor supporting:
- `zwlr_layer_shell_v1` (full-screen transparent grid overlay)
- `zwlr_virtual_pointer_v1` (mouse cursor motion and click/scroll synthesis)
Supported compositors: **Hyprland**, **Sway**, **River**, and other wlroots-based compositors.

### 2. User Input Permissions
Mouseless reads raw keyboard events via `/dev/input/event*` for global hotkey and overlay interception. Your user must belong to the `input` group:

```sh
sudo usermod -aG input $USER
```
*(Note: Log out and log back in for group membership to take effect).*

### 3. Build Dependencies & Packages

#### Arch Linux / Omarchy Linux / Manjaro:
```sh
sudo pacman -S base-devel rust wayland wayland-protocols ttf-jetbrains-mono-nerd
```

#### Ubuntu / Debian:
```sh
sudo apt update
sudo apt install build-essential cargo rustc libwayland-dev wayland-protocols fonts-dejavu-core
```

#### Fedora:
```sh
sudo dnf install gcc cargo rust wayland-devel wayland-protocols-devel dejavu-sans-fonts
```

#### Optional Tray AppIndicator (Python 3):
If you wish to run `mouseless_tray.py`:
- **Arch**: `sudo pacman -S python-gobject libayatana-appindicator`
- **Ubuntu/Debian**: `sudo apt install python3-gi gir1.2-ayatanaappindicator3-0.1`
- **Fedora**: `sudo dnf install python3-gobject libayatana-appindicator-gtk3`

---

## How to Build the Binary

Navigate into the `linux` directory and build with Cargo:

```sh
cd linux
cargo build --release
```

The optimized compiled binary will be located at:
```
linux/target/release/mouseless
```

### Optional: Install System-wide or User-local

To make `mouseless` available in your `$PATH`:

```sh
# Install to ~/.local/bin (recommended)
cp target/release/mouseless ~/.local/bin/

# Or install system-wide
sudo cp target/release/mouseless /usr/local/bin/
```

### Autostart with Hyprland (runs in background, no terminal needed)

`exec-once` already runs the daemon detached — you do **not** need `&`, `nohup`, or a terminal.

1. Install the binary so Hyprland can find it (you already use `/usr/local/bin/mouseless`):
   ```sh
   cd linux
   cargo build --release
   sudo cp target/release/mouseless /usr/local/bin/mouseless
   # verify
   /usr/local/bin/mouseless --help
   ```

2. Add to `~/.config/hypr/hyprland.conf` (or `~/.config/hypr/hyprland.conf` via Omarchy):
   ```ini
   exec-once = /usr/local/bin/mouseless
   # or if /usr/local/bin is in $PATH:
   exec-once = mouseless
   ```

   Reload Hyprland (`Super+Shift+R` or `hyprctl reload`) or log out/in. The daemon starts automatically on login and stays in background.

3. Manual background start (without Hyprland, or for testing):
   ```sh
   # preferred — uses the IPC socket, no terminal spam
   nohup /usr/local/bin/mouseless > /tmp/mouseless.log 2>&1 &
   disown
   # check
   mouseless status
   cat /tmp/mouseless.log
   ```

### Managing the Daemon — Stop, Restart, Logs

All of these work whether installed to `~/.local/bin` or `/usr/local/bin`:

```sh
# status
mouseless status
pgrep -a mouseless
ls -l /run/user/$UID/mouseless-*.sock

# stop (graceful — preferred)
mouseless quit
# or
pkill mouseless
# force
killall mouseless

# if you see "another mouseless daemon is already running"
mouseless quit; sleep 0.5; pkill mouseless; sleep 0.5
rm -f /run/user/$UID/mouseless-*.sock
nohup /usr/local/bin/mouseless > /tmp/mouseless.log 2>&1 &

# restart after updating the binary
sudo cp linux/target/release/mouseless /usr/local/bin/mouseless
mouseless quit; sleep 0.5
nohup /usr/local/bin/mouseless > /tmp/mouseless.log 2>&1 &

# logs (Hyprland exec-once logs go to Hyprland log; manual logs to /tmp)
cat /tmp/mouseless.log
hyprctl rollinglog | grep -i mouseless
```

Tip: `exec-once = mouseless` in Hyprland needs no `&`. If you run manually in a terminal, **always** redirect (`> /tmp/mouseless.log 2>&1 &`) otherwise `keyboard grabbed/released` messages will spam the terminal — that's what you saw when running `./target/release/mouseless &` without redirection.

## Keys

While the grid overlay is open:

- Letter labels (`A S D F ...`) refine the grid; `Backspace` undoes a step
- `Enter` guarded click: scans the accessibility tree (AT-SPI) for the nearest
  clickable control around the target and clicks it; without a match it clicks
  the raw target point. `1` does the same
- `2` double click, `3` right click, `4` start/end drag
- Hold `Space`, then `J`/`K` (or `↓`/`↑`) to scroll down/up (grid stays open)
- `↑`/`↓` also scroll directly; `←`/`→` switch monitors
- In precision mode (small cells) the arrows nudge the target instead
- `Tab` toggles persistent mode (grid stays open between actions)
- `-` / `=` adjust overlay opacity
- Backtick repeats the last click/drag
- `Q` (configurable) or `Escape` closes the grid

Free mode (`mouseless free`): `H J K L` or arrows move, `Space` left click,
`R` right click, `U`/`D` scroll up/down, `Y`/`O` scroll left/right, `Escape`
exits.

## Configuration

`~/.config/Mouseless/config.json` (respects `XDG_CONFIG_HOME`). A default file
is written on first run; `mouseless reload` picks up edits without restarting.

```json
{
  "gridRows": 5,
  "gridColumns": 5,
  "overlayOpacity": 0.72,
  "continuousMode": false,
  "freeModeStep": 26.0,
  "scrollStep": 18,
  "overlayHotkey": { "key": "U", "control": false, "alt": true, "shift": false, "win": false },
  "quitGridKey": "Q"
}
```

`gridRows`/`gridColumns` accept 3–5, `overlayOpacity` 0.25–0.95.

## Architecture

- `main.rs` — daemon, IPC socket, Wayland connection, output discovery
- `core.rs` — platform-independent grid state machine (portable core)
- `overlay.rs` — layer-shell surfaces, one per output, free-mode surface
- `render.rs` — CPU rasterization of grid, labels, precision view (fontdue)
- `input.rs` — evdev keyboard monitor, hotkey + grid key handling
- `mouse.rs` — virtual pointer motion/click/scroll, position tracking
- `hypr.rs` — Hyprland IPC for cursor position and monitor layout
- `accessibility.rs` — AT-SPI hit test for the guarded `Enter` click
- `free_mode.rs` — free-cursor mode state machine
- `settings.rs` — config load/save, hotkey normalization
