# Free Mode — Fix List

Status: **open**. Nothing here has been implemented yet.

Findings below come from reading `windows/src/` against the macOS reference
implementation (`Sources/Mouseless/main.swift`). macOS-side empirical results are in
`../Sources/Mouseless/main.swift` context and were verified with live `CGEvent`
probes on a single 1920x1080 display.

---

## P0 — Actions fire on key-up as well as key-down

**Every free-mode action runs twice per physical keypress.**

`input.rs:51-65` invokes the callback for both `WM_KEYDOWN` and `WM_KEYUP`,
passing `is_down` correctly:

```rust
let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
if let Some(cb) = CALLBACK.get() {
    let consume = cb(vk, is_down);   // called for key-up too
```

`main.rs:95` `dispatch_key(vk, is_down)` receives that flag and uses it in
**one place only** — the overlay-hotkey block at `main.rs:107`:

```rust
if is_down {
    let s = shared.settings.lock().unwrap();
    if s.overlay_hotkey.matches(&label, ctrl, alt, shift, win) { ... }
}
```

The free-mode block immediately below is **not gated on `is_down`**
(`main.rs:119-126`):

```rust
{
    let mut fm = shared.free_mode.lock().unwrap();
    if fm.is_active() {                       // <- no is_down check
        let s = shared.settings.lock().unwrap();
        let consumed = fm.handle(&label, &s);  // runs on key-up as well
        return if consumed { 1 } else { 0 };
    }
}
```

Observed effect with `free_mode_step = 26.0`:

| Key | Expected | Actual | Consequence |
|---|---|---|---|
| `H` / `L` / `J` / `K` | 26 px | 52 px | Cursor doubles your intended distance |
| `Space` | one click | two clicks | Accidental double-click on buttons/links |
| `R` | one context menu | two | Second menu opens on top of the first |
| `U` / `D` / `Y` / `O` | one notch | two | Double scroll speed |

macOS does not have this bug: `main.swift:1588-1590` returns early on `keyUp`
before reaching free mode.

### Fix

Gate the free-mode block on `is_down`, mirroring the hotkey block:

```rust
{
    let mut fm = shared.free_mode.lock().unwrap();
    if fm.is_active() {
        let consumed = if is_down {
            let s = shared.settings.lock().unwrap();
            fm.handle(&label, &s)
        } else {
            true // consume the key-up, but do not act on it
        };
        return if consumed { 1 } else { 0 };
    }
}
```

Consuming the key-up matters: leaving it unconsumed would let the key-up leak
into the foreground application.

### Related minor leak

`Escape` exits free mode on key-down (`free_mode.rs:62`). Because the key-up is
then processed with `is_active() == false`, the block is skipped and
`dispatch_key` falls through to the overlay check. With no overlay visible it
returns `0`, so the `Escape` **key-up is delivered to the focused app**. The fix
above closes this too. A holding application may react to it.

---

## P0 — No screen-edge clamping

`free_mode.rs:63-78` applies raw deltas with no bounds check:

```rust
"H" | "ArrowLeft" => {
    x -= step;
    self.mouse.move_to(x, y);   // no clamp
}
```

On macOS this was reproduced empirically: the cursor was driven to `x = -26` on a
1920px-wide display and **stayed off-screen** after a full second of idle — the
OS did not pull it back. The user gets an invisible cursor and a `Space` click
that goes nowhere.

### Fix

Clamp against the union of all monitor bounds before calling `move_to`. The
`Environment` trait already exposes monitor geometry (used at `main.rs:88`
via `shared.env.monitors()`), so no new plumbing is needed:

```rust
let bounds = self.env.monitors()
    .iter()
    .fold(None::<MonitorBounds>, |acc, m| Some(match acc {
        Some(b) => b.union(m),
        None => MonitorBounds::from(m),
    }));
// then clamp x/y into bounds before move_to
```

Guard the edge case where `monitors()` returns empty — fall back to no clamping
rather than collapsing the cursor to a point.

Also clamp `click_async`/`scroll` targets to the same bounds so a stranded cursor
cannot click off-screen.

---

## P1 — Free mode has no global keyboard shortcut

`tray.rs:103-104` registers a tray menu item `Toggle Free Mode` and nothing else.
Entering free mode therefore requires the mouse — in an app whose purpose is
keyboard-driven mouse control. This is the single biggest usability blocker and
the reason the mode is hard to discover.

macOS has the identical gap (`main.swift:1443-1447`, and `README.md:76` states it
explicitly). The upstream Mouseless app solves it with `ControlLeft tap`.

### Fix

Add a configurable `free_mode_hotkey` to `Settings` (default: tap `ControlLeft`,
matching upstream) and handle it alongside the overlay hotkey in
`dispatch_key`. Register the hook so a *tap* (down-then-up within a short window)
is distinguished from a hold, since `ControlLeft` is otherwise consumed as a
modifier.

Requires a migration path: `Settings::clamped()` (`settings.rs:136-149`) should
fill the new field when loading an older `config.json` that lacks it, otherwise
`serde` will fail to deserialize existing user settings. Check
`settings.rs` for the existing default-filling pattern and mirror it.

---

## P1 — Routing precedence differs from macOS

The two ports disagree on which mode wins when free mode is active *and* the
overlay is visible.

**Windows** (`main.rs`): hotkey -> **free mode** -> overlay visibility.
`main.rs:119` (free mode) runs before `main.rs:128` (overlay), so free mode
takes precedence.

**macOS** (`main.swift:1575-1594`): hotkey -> **overlay visibility** -> free mode.
`main.swift:1580` runs before `main.swift:1592`, so the overlay takes precedence.

The two apps therefore behave differently for identical input. Pick one order and
apply it to both. Recommend overlay-first, matching macOS, since the overlay is
the more explicit mode of the two.

---

## P2 — Scroll speed does not match macOS

`free_mode.rs:81-84` scales by `scroll_step * 6` (= 108 at default `scroll_step =
18`). macOS uses the raw `scroll_step` (18) — `main.swift:1123-1129`.

The same physical key therefore scrolls roughly 6x further on Windows. Pick one
convention and apply it to both; the shared constant is the right home for it.

---

## P2 — No persistent mode indicator

`free_mode.rs:41,46` notifies via the `notify` callback, which surfaces the same
transient toast used elsewhere in the app (`main.rs:329` shows
`"Mouseless ready"`). Once the toast fades there is no persistent signal that
free mode is active.

Because `handle()` returns `true` unconditionally (`free_mode.rs:87`), *all*
keyboard input is swallowed while active — the only reliable user-visible tell is
"my typing went nowhere". That is a bad failure mode for a mode that is easy to
enter accidentally.

### Fix

Reflect state in the tray menu (checkmark or a distinct label) and/or swap the
tray icon while free mode is active. macOS needs the same treatment.

---

## Already correct — do not regress

The Windows port already does two things better than the macOS one. Preserve
them if this logic is ever ported back:

1. **Clicks are off the hook thread.** `free_mode.rs:49-53` spawns a thread for
   `click_async`, with the comment explaining that click sleeps must not block
   the low-level keyboard hook. macOS does the opposite — `main.swift:251,255`
   calls `usleep(30000)` and `usleep(50000)` (80 ms total) *synchronously inside
   the CGEventTap callback*. That risks `tapDisabledByTimeout`; macOS recovers
   from that condition at `main.swift:200-205` but only after the freeze.

2. **Expensive work is deferred off the hook.** `main.rs:112-114` and
   `main.rs:139-141` post overlay work to the message loop via
   `post_main(...)` rather than rendering inside the hook callback.

## Y-axis note (not a bug)

`free_mode.rs:71-78` maps `K` to `y -= step` and `J` to `y += step`, because Windows screen
coordinates are y-down; macOS AppKit coordinates are y-up, so its "up" key adds to
`y` instead. Both are correct. Do not "fix" this to
match macOS — just keep the inversion in mind when porting.

## Key reference

macOS free mode now uses **WASD**, the same layout as a game. This file predates
that change, so the Rust implementation below still needs to be moved onto the new
mapping — see "Adopt the WASD layout" below.

| Key | Action |
|---|---|
| `w` / `ArrowUp` | move up |
| `a` / `ArrowLeft` | move left |
| `s` / `ArrowDown` | move down |
| `d` / `ArrowRight` | move right |
| `Space` | left click |
| `r` | right click |
| `,` | scroll up |
| `.` | scroll down |
| `/` | scroll left |
| `m` | scroll right |
| `q` / `Escape` | exit free mode |

Letters are matched in **lowercase** only: if Shift or Caps Lock is active the key
does nothing. Arrows and Space are unaffected, since they have no case.

## Adopt the WASD layout

`free_mode.rs:61-86` still maps `H`/`J`/`K`/`L` for movement and `U`/`D`/`Y`/`O`
for scrolling. Two things to settle while porting:

1. **`d` collides.** On macOS `d` was scroll-down and became move-right. Here
   `free_mode.rs:74` maps `"J"` to `y += step` (screen coords are y-down, so
   `J` moving down is correct on Windows). Moving to WASD means `J`/`K` disappear
   and `W`/`S` take over. Do **not** flip the y sign while doing this — see the
   y-axis note below.
2. **Scroll keys moved to `,` `.` `/` `m`** because `d` was taken by movement.
   These are punctuation and `m` rather than letters, so `label_for_vk` in
   `input.rs` needs to cover them — check what it currently maps before assuming
   the default arm will fire.

macOS reference: `FreeModeController.handle` in `Sources/Mouseless/main.swift`.
Its lowercase guard is worth copying too — it drops the whole letter switch behind
`guard isLowercase` so a shifted press cannot move the cursor.

Not supported in free mode on any platform: drag, middle-click, modifier-click.
`MouseController` on macOS has `toggleDrag`/`moveDrag` (`main.swift:259-276`) but
`handle()` never wires them up.