# Mouseless

Mouseless is a macOS-only keyboard mouse controller. It gives you a global hotkey, a full-screen coordinate grid, keyboard-driven click/drag actions, and a free cursor mode.

This is a local build for personal use. It does not include licensing, network calls, or copied vendor code.

## Run From Source

```sh
make run
```

The app will appear in the macOS status bar as `Mouseless`.

## Build The App Binary

```sh
make app
open .build/release/Mouseless.app
```

The packaged app bundle is created at:

```text
.build/release/Mouseless.app
```

## Build The DMG Installer

```sh
make dmg
```

The compressed DMG is created at:

```text
.build/release/Mouseless.dmg
```

## First Run

macOS must allow Accessibility/Input Monitoring style access before global keyboard and mouse control can work.

1. Launch `Mouseless.app`.
2. Open System Settings > Privacy & Security > Accessibility.
3. Enable Mouseless.
4. Restart the app.

## Keyboard Shortcuts

Configurable in Preferences:

- `Option+U`: show/hide the overlay. Configurable as the overlay shortcut.
- `Q`: hide the overlay grid. Configurable as the quit grid key.

Overlay grid controls:

- `A S D F G H J K L M W E R T Y U I O P Z X C V B N`: narrow the target cell.
- `Enter`: left-click the nearest clickable control; otherwise hide the grid.
- Hold `Space` + `J`: scroll down while keeping the grid open.
- Hold `Space` + `K`: scroll up while keeping the grid open.
- `1`: force left click at the target.
- `2`: double click.
- `3`: right click.
- `4`: start/drop a drag.
- `Backspace`: undo one grid step.
- `Tab`: toggle persistent overlay mode.
- `ArrowLeft` / `ArrowRight`: switch monitors before precision mode.
- Arrow keys: nudge the target in precision mode.
- `-` / `=`: decrease/increase overlay opacity.
- `` ` ``: repeat the last action.
- `Escape`: hide overlay fallback.

Free mode controls:

Toggle it by pressing the free-mode shortcut, **Option+T** by default, or from the
status-bar menu. Toggle off the same way, or press `q`. While free mode is active
an orange **FREE MODE** badge stays on screen and the status-bar menu shows a
checkmark next to `Free Mode`.

Free mode reads letters in their **lowercase** form only. Shifted presses and
Caps Lock do nothing, so always press the plain lowercase key.

Movement is **WASD**, the same as in a game.

| Key | Action |
|---|---|
| `w` / `↑` | move up |
| `a` / `←` | move left |
| `s` / `↓` | move down |
| `d` / `→` | move right |
| `Space` | left click |
| `r` | right click |
| `,` | scroll up |
| `.` | scroll down |
| `/` | scroll left |
| `m` | scroll right |
| `q` / `Esc` | exit free mode |

Both shortcuts are reconfigurable in Preferences. The cursor is kept inside the
display bounds, so it cannot be driven off-screen.

## Preferences

Use the status bar `Mouseless` menu to open Preferences. Only these shortcuts are reconfigurable in the app:

- Overlay shortcut, default `Option+U`
- Quit grid key, default `Q`

Settings are saved to:

```text
~/Library/Application Support/Mouseless/config.json
```

## Testing Precision Mode

1. Run `make app && open .build/release/Mouseless.app`.
2. Grant Accessibility permission if macOS asks, then restart the app.
3. Press the overlay shortcut, default `Option+U`.
4. Press grid letters three times, for example `A`, then `S`, then `D`.
5. When the cells get tiny, Mouseless shows a larger precision grid so the letters stay readable.
6. Use the arrow keys to nudge the pink target marker by a small adaptive step.
7. Press `Enter` to left-click a clickable target, hold `Space` + `J`/`K` to scroll, press `1` to force-click, or press `Q`/`Escape` to close the grid.
