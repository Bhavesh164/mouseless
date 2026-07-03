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

- `A S D F G H J K L Q W E R T Y U I O P Z X C V B N`: narrow the target cell.
- `Enter`: click only when Accessibility sees a clickable control there; otherwise hide the grid.
- `Space` or `1`: force left click at the target.
- `2`: double click.
- `3`: right click.
- `4`: start/drop a drag.
- `Backspace`: undo one grid step.
- `Tab`: toggle persistent overlay mode.
- `ArrowLeft` / `ArrowRight`: switch monitors.
- `-` / `=`: decrease/increase overlay opacity.
- `` ` ``: repeat the last action.
- `Escape`: hide overlay fallback.

Free mode controls:

Free mode is started from the status-bar menu and does not have a global keyboard shortcut.

- `H/J/K/L` or arrow keys: move the system cursor.
- `Space`: left click.
- `R`: right click.
- `U/D`: scroll up/down.
- `Y/O`: scroll left/right.
- `Escape`: exit free mode.

## Preferences

Use the status bar `Mouseless` menu to open Preferences. Only these shortcuts are reconfigurable in the app:

- Overlay shortcut, default `Option+U`
- Quit grid key, default `Q`

Settings are saved to:

```text
~/Library/Application Support/Mouseless/config.json
```
