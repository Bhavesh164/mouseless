# Free Mode — Linux Porting Spec

> **There is no Linux port in this repository yet.** This directory does not
> currently exist elsewhere in the tree — `ls` at the repo root shows only
> `Sources/` (macOS, Swift) and `windows/` (Rust). Linux appears solely as a
> forward-looking item in `PLAN.md:67`:
>
> > "Linux likely needs separate paths for X11 and Wayland. X11 can use global
> > grabs and XTest-style mouse events; Wayland support depends heavily on
> > compositor protocols and permissions."
>
> This file is a **specification to build against**, not a description of
> existing code. Line references point at the macOS reference implementation and
> the Windows Rust port, which are the only two implementations that exist.
> Create the `linux/` crate before treating any of this as applied.

Findings carry over from `../windows/free-mode-fix.md`. The point of recording
them here is to **not reintroduce them** in the new port.

---

## Platform split is not optional

`PLAN.md:67` is the real constraint. Design the crate around an adapter trait
from day one rather than `cfg`-gating a single implementation:

- **X11** — global grabs and XTest-style synthetic input are broadly available.
  Global hotkeys work by grabbing the root window with key grabs.
- **Wayland** — no global keyboard hook exists in the base protocol. Options:
  - **XWayland** — only works for X11 clients; a native Wayland app is invisible
    to it. Acceptable as a first target if the user runs X11 apps.
  - **libinput / evdev** — requires `CAP_SYS_ADMIN` or root, and the session
    must grant the grab.
  - **wlr virtual keyboard / portal** — compositor-specific, no portable path.

Recommendation: **ship X11 first, detect Wayland at startup, and fail loudly with
an actionable message.** Per `PLAN.md:68`, isolate the grid/refinement/action
logic from platform adapters before writing either.

`PLAN.md:69` — treat accessibility permission and secure-input failures as
first-class UI states. On Linux that means detecting an active secure-input
client (X: `XFixes`/`XInput` grabs) and saying so in the UI, not failing silently.

---

## Must not reintroduce these bugs

All three are present or were present in the existing ports. Treat each as a
test case for the Linux implementation.

### 1. Actions must fire on key-down only

`../windows/src/main.rs:119-126` routes free-mode input without checking the
down/up flag that the hook already provides, so every action fires twice per
press: 52 px of movement instead of 26, and a double-click on the click key.
The macOS port gets this right by returning early on key-up
(`../Sources/Mouseless/main.swift:1588-1590`).

Gate the free-mode handler on key-down, and still **consume** the key-up so it
does not leak into the focused application.

### 2. Clamp the cursor to the monitor bounds

`../Sources/Mouseless/main.swift:1098-1134` applies raw deltas with no bounds
check. Verified empirically: the cursor was driven to `x = -26` on a 1920px-wide
display and stayed off-screen indefinitely.

On Linux this is more dangerous than on macOS — a multi-head setup with
non-zero origins is the norm rather than the exception, so naive arithmetic goes
wrong immediately. Clamp against the union of monitor geometry before every
`move`, and clamp click targets too.

### 3. Decide one precedence order and match the other ports

macOS checks overlay visibility before free mode
(`main.swift:1580` before `main.swift:1592`). Windows checks free mode first
(`../windows/src/main.rs:119` before `:128`). They already disagree — resolve
that first, then apply the winning order here.

---

## Coordinate spaces — the macOS multi-monitor bug

`../Sources/Mouseless/main.swift:341-343`:

```swift
static func appKitToQuartz(_ point: CGPoint) -> CGPoint {
    return CGPoint(x: point.x, y: primaryScreenHeight - point.y)
}
```

This ignores every screen's **origin** and assumes the primary display sits at
`(0, 0)` with the y-flip measured against its height. It is correct only for a
single display at the origin — which is exactly the configuration it was tested
on, so the bug is latent, not absent.

On Linux with Xinerama/RandR outputs at non-zero origins, this same formula
breaks immediately. Do not port it. Build the transform from actual display
geometry (`XRRGetScreenResourcesCurrent` / `XineramaQueryScreens` on X11,
`wl_output` geometry on Wayland) and round-trip it in a unit test:

```
display_to_root(x, y) -> root_to_display(x, y) == identity
```

Note also that `quartzToAppKit` is written as the identical formula
(`main.swift:345-347`), so round-tripping the app's own two functions always
passes and proves nothing. Compare against real display bounds instead — that is
what caught the problem on macOS.

---

## UX requirements (currently unmet on both existing ports)

Carry these as requirements rather than copying the current behaviour.

- **Global toggle hotkey.** Both existing ports are menu-only, so entering free
  mode needs the mouse — a catch-22 in a keyboard-driven mouse tool. Upstream
  Mouseless binds `OptionLeft tap` on macOS and `ControlLeft tap` on
  Windows/Linux. Use a tap (down-then-up within a window) so the modifier key
  stays usable otherwise.
- **Persistent active indicator.** Both ports signal state with a ~1.2 s toast
  that then disappears, while simultaneously swallowing every keystroke. If a
  user cannot tell they are in free mode, the mode is undiscoverable and
  unrecoverable. Reflect state in the tray icon and menu, and consider an
  auto-off timeout after inactivity.
- **Escape hatch.** Ensure a documented key always exits. `Escape` works today,
  but it is the *only* key guaranteed to reach the handler while active.

---

## Settings parity

Match the macOS `Settings` shape so configs stay conceptually portable
(`../Sources/Mouseless/main.swift:9-32`):

| Field | Default | Clamp |
|---|---|---|
| `gridRows` | 5 | 3–5 |
| `gridColumns` | 5 | 3–5 |
| `overlayOpacity` | 0.72 | 0.25–0.95 |
| `freeModeStep` | 26.0 | 6.0–90.0 |
| `scrollStep` | 18 | — |
| `overlayHotkey` | `Option+U` / `ControlLeft` per platform | — |
| `quitGridKey` | `"Q"` | — |

Two parity gaps to settle deliberately rather than by accident:

1. `scrollStep` has **no UI control** in the macOS Preferences window — only the
   Windows port exposes it. Pick one.
2. Windows scrolls at `scroll_step * 6` (`../windows/src/free_mode.rs:81-84`)
   while macOS uses the raw value. Unify the constant.

If you add a `free_mode_hotkey` field, fill defaults for older config files the
way `../windows/src/settings.rs:136-149` does, or existing users' settings will
fail to deserialize.

---

## Key reference

Free mode uses **WASD**, the same layout as a game, and letters are matched in
**lowercase** only — with Shift or Caps Lock active the key does nothing. Arrows
and Space are unaffected, since they have no case.

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

Note that scroll lives on punctuation and `m` rather than letters, because `d`
would otherwise collide with move-right. Your keymap layer needs entries for
`,` `.` `/` or those four bindings will never fire.

**Y-axis caveat:** Windows is y-down and macOS AppKit is y-up, so the "move up"
key produces opposite sign changes on each (`../windows/src/free_mode.rs:71-74` vs
`../Sources/Mouseless/main.swift`). Both are correct for their platform. On X11
the cursor position is **y-down** (root coordinates), so follow the Windows
convention — this is a genuine trap when porting from the macOS source.

**Indicator:** macOS shows a persistent on-screen `FREE MODE` badge listing the
controls, because free mode swallows every key and the toast alone fades. Build
the equivalent into whatever status/notification surface the port has.

Not supported in free mode on any existing platform: drag, middle-click,
modifier-click. Consider adding these in the Linux port, since `MouseController`
already implements drag on macOS (`main.swift:259-276`) and only `handle()` fails
to wire it up.

---

## Acceptance checklist for the port

- [ ] Key-down-only handling; key-up consumed but not acted on
- [ ] Cursor clamped to monitor bounds on all four movement keys and on clicks
- [ ] Monitor origin/geometry transform, round-trip tested against real output bounds
- [ ] Precedence order matches the decision made for macOS and Windows
- [ ] Global hotkey toggles free mode without the mouse
- [ ] Persistent visual indicator while active
- [ ] `Escape` exits and does not leak through
- [ ] X11 path works; Wayland path either works or fails with an actionable message
- [ ] Secure-input / grab detection surfaced as a first-class UI state