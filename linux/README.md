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

### 4. Bluetooth / Hotplug Keyboards

`run_input()` (`linux/src/input.rs:389`) calls `open_keyboards()` (`linux/src/input.rs:148`)
exactly **once**, at startup. It does `fs::read_dir("/dev/input")`, keeps every node that
exposes `KEY_A` + `KEY_SPACE` + `KEY_ENTER`, and never rescans. There is no udev/hotplug
monitor.

A Bluetooth keyboard usually enumerates **after** the compositor and after your autostart
runs, so at startup `/dev/input` does not yet contain it and mouseless locks on to whatever
else is plugged in — often an unused USB board. The symptom is that the hotkey never fires
on the keyboard you actually type on, while the daemon looks perfectly healthy.

Check which devices it really grabbed:

```sh
ls -l /proc/$(pgrep -x mouseless)/fd | grep input
for e in /dev/input/event*; do
  echo "$(basename $e) $(cat /sys/class/input/$(basename $e)/device/name)"
done
```

Compare the two lists. If the fds list a device you do not type on, that is the bug. Restart
the daemon once the keyboard has paired:

```sh
systemctl --user restart mouseless.service   # systemd
mouseless quit; mouseless &                  # manual
```

> Because the hotkey is read from raw evdev rather than through the compositor, Hyprland
> keybindings cannot see or intercept it, and adding `bind = ALT, U, ...` to
> `~/.config/hypr/bindings.lua` will not help. Configure it in
> `~/.config/Mouseless/config.json` instead. For the same reason, while the grid is open
> mouseless holds an exclusive `EVIOCGRAB` on those keyboards and Hyprland receives nothing
> from them (`linux/src/input.rs:224`) — the grab is released on `Q`/`Escape`.

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

### Autostart as a systemd user service (recommended)

This is the most reliable way to run the daemon: it survives terminal closes, starts at
login, logs to the journal, and restarts on crash. It also keeps the daemon out of the
compositor's config entirely — no Hyprland/Omarchy edits required.

Create `~/.config/systemd/user/mouseless.service`:

```ini
[Unit]
Description=Keyboard-driven mouse controller
After=graphical-session.target bluetooth.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/local/bin/mouseless
Restart=always
RestartSec=3
Environment=WAYLAND_DISPLAY=wayland-1

[Install]
WantedBy=graphical-session.target
```

Then enable and start it:

```sh
systemctl --user daemon-reload
systemctl --user enable --now mouseless.service

# verify
systemctl --user status mouseless.service
journalctl --user -u mouseless.service -f
mouseless status      # -> overlay=hidden free-mode=inactive hotkey=Alt+U
```

On the first successful start you should see in the journal:

```
mouseless: ready  hotkey=Alt+U  layer-shell ok  virtual-pointer ok  Hyprland IPC ok
mouseless: monitoring keyboard, fds [7, 8]
```

`monitoring keyboard, fds [...]` lists the raw fds it opened. Confirm they are the
keyboards you actually type on (see [Bluetooth / Hotplug Keyboards](#4-bluetooth--hotplug-keyboards)).

Manage it with:

```sh
systemctl --user restart mouseless.service   # re-enumerate /dev/input
systemctl --user stop  mouseless.service
systemctl --user status mouseless.service
```

**Remove the compositor autostart.** Running both is a real hazard, so make sure only the
systemd service starts the daemon. If you previously used `exec-once` or
`o.launch_on_start("mouseless")`, comment it out — two daemons will fight over the IPC
socket and the loser exits with `another mouseless daemon is already running`, which
`Restart=always` then turns into an endless restart loop.

On Omarchy the daemon is launched from `~/.config/hypr/autostart.lua`. Comment out the
line (a `--` prefix is enough, you do not need to delete it):

```lua
-- ~/.config/hypr/autostart.lua
-- Extra autostart processes.
-- o.launch_on_start("my-service")
-- no longer needed, handled by the systemd user service
-- o.launch_on_start("mouseless")
```

Then reload:

```sh
hyprctl reload
```

Confirm only one daemon is left:

```sh
pgrep -ax mouseless            # -> exactly one PID
systemctl --user status mouseless.service   # -> active (running)
```

If you use standard Hyprland instead, remove the `exec-once = mouseless` line from
`~/.config/hypr/hyprland.conf`.

### Autostart with Hyprland (alternative)

`exec-once` / `o.launch_on_start` already runs the daemon detached — you do **not** need `&`, `nohup`, or a terminal. Prefer the systemd user service above.

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

   **Omarchy** (config lives in Lua; `~/.config/hypr/hyprland.conf` is **ignored**):
   ```lua
   -- edit ~/.config/hypr/autostart.lua
   o.launch_on_start("mouseless")
   -- or with absolute path:
   -- o.launch_on_start("/usr/local/bin/mouseless")
   ```
   This is the Omarchy wrapper for `hl.on("hyprland.start", ...)` + `uwsm-app --` (see `require("hypr.autostart")` in `~/.config/hypr/hyprland.lua` and `o.launch_on_start` in `/usr/share/omarchy/default/hypr/helpers.lua`). You **must log out/in** — `o.launch_on_start` only runs on `hyprland.start`, `hyprctl reload` is not enough. For an immediate test without relogin: `uwsm-app -- mouseless &` then `mouseless status` should show `overlay=hidden ...` instead of `daemon is not running`.

3. Manual background start (without Hyprland, or for testing):
   ```sh
   # preferred — uses the IPC socket, no terminal spam
   nohup /usr/local/bin/mouseless > /tmp/mouseless.log 2>&1 &
   disown
   # check
   mouseless status
   cat /tmp/mouseless.log
   ```

### Fallback: Hyprland binding that restarts the daemon

Useful if you run the systemd service but want a keyboard shortcut to bring it back
without opening a terminal — for example after the Bluetooth keyboard re-pairs and the
daemon is watching the wrong device (§4).

Pick a key that is **not** currently bound. Check first with:

```sh
hyprctl binds | grep -i "<key>"
```

Then add to `~/.config/hypr/bindings.lua` (Omarchy):

```lua
o.bind("SUPER + SHIFT + U", "Restart mouseless", "systemctl --user restart mouseless.service")
```

`SUPER + SHIFT + U` is unbound in stock Omarchy, so no `hl.unbind(...)` is needed. If you
pick a key that Omarchy already binds, unbind it first with `hl.unbind("SUPER + SHIFT + X")`
above the `o.bind(...)` (see `/usr/share/omarchy/config/hypr/bindings.lua`).

In a stock `hyprland.conf`:

```ini
bind = SUPER SHIFT, U, exec, systemctl --user restart mouseless.service
```

Apply with `hyprctl reload`, then confirm it worked:

```sh
journalctl --user -u mouseless.service -f
```

> Use a `SUPER`-based key, **not** `ALT`. Mouseless grabs `Alt+U` itself, and while the
> grid is open it holds an exclusive `EVIOCGRAB` on the keyboards, so Hyprland receives
> nothing from them — an `ALT`-based fallback binding would be dead exactly when you
> need it.

> On Omarchy, put this in `~/.config/hypr/bindings.lua`, **not**
> `~/.config/hypr/hyprland.conf` — the latter is ignored entirely.

### Managing the Daemon — Stop, Restart, Logs

All of these work whether installed to `~/.local/bin` or `/usr/local/bin`:

```sh
# status
mouseless status
pgrep -a mouseless
ls -l /run/user/$UID/mouseless-*.sock

# which keyboards is it actually reading?
ls -l /proc/$(pgrep -x mouseless)/fd | grep input

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

With the systemd user service, use `systemctl` instead — it restarts the daemon for you:

```sh
sudo cp linux/target/release/mouseless /usr/local/bin/mouseless
systemctl --user restart mouseless.service
journalctl --user -u mouseless.service -f
```

Tip: `exec-once = mouseless` in Hyprland needs no `&`. If you run manually in a terminal, **always** redirect (`> /tmp/mouseless.log 2>&1 &`) otherwise `keyboard grabbed/released` messages will spam the terminal — that's what you saw when running `./target/release/mouseless &` without redirection. A daemon started from a terminal also dies with that terminal; use the systemd unit to avoid this.

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
