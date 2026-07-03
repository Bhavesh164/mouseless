# Mouseless Feature And Porting Plan

This document describes the current macOS app features and the platform seams that matter if the project is ported to another OS.

## Current Product Features

- Global overlay shortcut, default `Option+U`, configurable in Preferences.
- Status bar app with menu actions for showing the overlay, toggling free mode, opening Preferences, and quitting.
- Full-screen transparent overlay windows across all connected displays.
- Recursive keyboard grid targeting with these labels: `A S D F G H J K L Q W E R T Y U I O P Z X C V B N`.
- Configurable grid size from 3x3 through 5x5.
- Target refinement by pressing grid labels; `Backspace` undoes one refinement step.
- `Enter` guarded click: clicks only if OS accessibility hit-testing reports an interactable target; if not, the grid closes.
- `Space` or `1` force left-clicks at the current target.
- `2` double-clicks, `3` right-clicks, and `4` starts or drops a drag.
- `Tab` toggles persistent overlay mode.
- `ArrowLeft` and `ArrowRight` switch the active monitor.
- `-` and `=` adjust overlay opacity.
- Backtick repeats the last click/drag action.
- Quit grid key defaults to `Q` and is configurable in Preferences; `Escape` remains a fallback.
- Free mode from the status bar menu for cursor movement, click, right-click, and scrolling.
- Preferences saved as JSON under `~/Library/Application Support/Mouseless/config.json`.
- App icon generation from `Resources/AppIcon.png`.
- Build outputs for `.app` and `.dmg`.

## Current macOS Implementation

- Language/UI: Swift with AppKit.
- Global keyboard capture: `CGEvent.tapCreate`.
- Mouse movement and clicks: CoreGraphics `CGEvent`, `CGWarpMouseCursorPosition`.
- Clickability detection: Accessibility APIs, especially `AXUIElementCopyElementAtPosition`, action names, roles, and parent traversal.
- Multi-monitor overlay: one borderless `NSPanel` per `NSScreen`.
- Status bar and preferences: `NSStatusBar`, `NSMenu`, `NSWindow`, `NSStackView`.
- Packaging: SwiftPM release binary copied into a macOS `.app`, ad-hoc signed with `codesign`, then wrapped into a `.dmg` with `hdiutil`.

## Portable Core Logic

These parts should remain mostly OS-independent in a future port:

- Settings schema: grid rows/columns, opacity, persistent mode, free-mode step, overlay hotkey, quit grid key.
- Hotkey normalization and matching.
- Grid label ordering.
- Active-region refinement math.
- Undo stack for grid refinement.
- Action state machine: guarded click, force click, double click, right click, drag/drop, repeat last action, persistent mode.
- User-facing shortcut reference and configuration model.

## OS-Specific Replacement Points

Each port needs native replacements for these adapters:

- Global keyboard listener.
- Full-screen always-on-top transparent overlay window.
- Multi-monitor bounds and coordinate conversion.
- Synthetic mouse movement, click, drag, and scroll events.
- Accessibility hit-testing to decide whether `Enter` should click or close the grid.
- System tray/status menu.
- Preferences storage location.
- Installer/package generation.

## Porting Notes

- Windows likely needs low-level keyboard hooks, overlay windows, UI Automation hit-testing, and SendInput for mouse events.
- Linux likely needs separate paths for X11 and Wayland. X11 can use global grabs and XTest-style mouse events; Wayland support depends heavily on compositor protocols and permissions.
- Keep the core grid/refinement/action logic isolated before porting. The current single-file Swift implementation works for the MVP, but a port should split the app into core logic plus platform adapters first.
- Treat accessibility permission and secure-input failures as first-class UI states on every OS.
- Preserve the guarded `Enter` behavior: if the platform cannot confirm a clickable target, close the grid rather than clicking blindly.
