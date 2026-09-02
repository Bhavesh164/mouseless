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
Mouseless reads raw keyboard events via `/dev/input/event*` for global hotkey and overlay interception (`linux/src/input.rs:148` `open_keyboards()`). Your user must belong to the `input` group:

```sh
sudo usermod -aG input $USER
```
*(Note: Log out and log back in for group membership to take effect — `groups` must show `input` after relogin).*

#### Why does my keyboard work but mouseless says "No keyboard devices found"?

Your keyboard works in Hyprland because the compositor (`Hyprland`/`wlroots` + `libinput`) opens `/dev/input/event*` as `root` via `systemd-logind` on `seat0` (`loginctl show-session $(loginctl | grep $USER | awk '{print $1}')`). Mouseless cannot use that path — it opens the same nodes directly via `evdev` (`evdev::Device::open`) to `EVIOCGRAB` the keyboard while the grid is visible (`linux/src/input.rs:177` `set_keyboard_grab`). Those nodes are `crw-rw---- root input` (`0660`):

```sh
ls -l /dev/input/event*   # crw-rw---- 1 root input 13, 64 ...
getfacl /dev/input/event6 # no user:bv:rw- unless uaccess, only group::rw-
groups                    # must contain input; fresh Arch/Omarchy creates group input:GID 992 empty
id; getent group input    # verify
```

Only `root` or members of `input` can `open()` them. Wayland's `uaccess` tag (`70-uaccess.rules`) is **not** applied to keyboards (only `ID_INPUT_JOYSTICK` etc.), so `logind` does not add a per-user ACL for them — `udevadm info /dev/input/event6 | grep TAGS` shows only `power-switch`, not `uaccess`. That's why `hyprctl devices` lists keyboards but `mouseless` (`linux/src/input.rs:390`) returns `No keyboard devices found`.

Quick check without relogin:

```sh
# fails as non-input user:
python3 -c "import os; os.open('/dev/input/event6', os.O_RDONLY)"
# Permission denied -> you need input group

# after sudo usermod -aG input $USER, verify effective group:
groups                 # after logout/login should include input
# or temporary test without logout:
newgrp input
mouseless status
```

#### It worked before — why does it need `usermod` now?

* Fresh Arch/Omarchy install (`/var/log/pacman.log` `Creating group 'input' with GID 992` on 2026-08-29) creates an empty `input` group — previous membership is lost.
* Running `sudo mouseless`/`sudo ./mouseless` works as `root` and hides the permission error (check `history | grep mouseless`).
* The Hyprland autostart (`~/.config/hypr/autostart.lua` `o.launch_on_start("mouseless")` or `exec-once = mouseless`) now runs unprivileged via `systemd-run --user`, so it needs the group.

> Security: `input` grants raw keylogging capability — intended for this use. Alternative is a `udev` rule with `TAG+="uaccess"` for `ID_INPUT_KEYBOARD`, but `input` group is the upstream-recommended way.

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

`exec-once` / `o.launch_on_start` already runs the daemon detached — you do **not** need `&`, `nohup`, or a terminal.

1. Install the binary so Hyprland can find it (you already use `/usr/local/bin/mouseless`):
   ```sh
   cd linux
   cargo build --release
   sudo cp target/release/mouseless /usr/local/bin/mouseless
   # verify
   /usr/local/bin/mouseless --help
   ```

2. Autostart — pick your setup:

   **Standard Hyprland** (`~/.config/hypr/hyprland.conf` is your main config):
   ```ini
   exec-once = /usr/local/bin/mouseless
   # or if /usr/local/bin is in $PATH:
   exec-once = mouseless
   ```
   Then reload (`Super+Shift+R` or `hyprctl reload`) or log out/in. The daemon starts automatically on login and stays in background.

   **Omarchy** (uses `~/.config/hypr/hyprland.lua:1` + `~/.config/hypr/autostart.lua:1` — `~/.config/hypr/hyprland.conf:1` is **ignored**):
   ```lua
   -- edit ~/.config/hypr/autostart.lua
   o.launch_on_start("mouseless")
   -- or with absolute path:
   -- o.launch_on_start("/usr/local/bin/mouseless")
   ```
   This is the Omarchy wrapper for `hl.on("hyprland.start", ...)` + `uwsm-app --` (see `~/.config/hypr/hyprland.lua:18` → `require("hypr.autostart")` and `/usr/share/omarchy/default/hypr/helpers.lua:118`). You **must log out/in** — `o.launch_on_start` only runs on `hyprland.start`, `hyprctl reload` is not enough. For an immediate test without relogin: `uwsm-app -- mouseless &` then `mouseless status` should show `overlay=hidden ...` instead of `daemon is not running`.

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
