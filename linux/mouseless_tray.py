#!/usr/bin/env python3
"""Small Linux StatusNotifier/AppIndicator companion for Mouseless."""

import os
import signal
import subprocess
import sys
from pathlib import Path

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("AyatanaAppIndicator3", "0.1")
from gi.repository import AyatanaAppIndicator3, GLib, Gtk


SCRIPT_DIR = Path(__file__).resolve().parent
# An installed copy can be selected explicitly, while a checkout uses the
# Cargo release build next to this script. Do not fall back to an arbitrary
# `mouseless` found on PATH: an older Go implementation may have that name.
DAEMON = Path(os.environ.get("MOUSELESS_BIN", SCRIPT_DIR / "target/release/mouseless"))
ICON = SCRIPT_DIR.parent / "Resources/TrayIcon.png"


def running():
    return subprocess.run(
        [DAEMON, "status"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode == 0


def start_daemon():
    if not DAEMON.is_file() or not os.access(DAEMON, os.X_OK):
        raise FileNotFoundError(
            f"Mouseless binary not found at {DAEMON}. Build it with cargo build --release."
        )
    # The daemon must inherit the active user's Wayland and Hyprland sockets.
    # Running it via pkexec starts it as root, outside that session.
    return subprocess.Popen([DAEMON], start_new_session=True)


def daemon_command(command):
    return subprocess.run(
        [DAEMON, command],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode == 0


class MouselessTray:
    def __init__(self):
        self.indicator = AyatanaAppIndicator3.Indicator.new(
            "mouseless", str(ICON), AyatanaAppIndicator3.IndicatorCategory.APPLICATION_STATUS
        )
        self.indicator.set_status(AyatanaAppIndicator3.IndicatorStatus.ACTIVE)
        self.indicator.set_title("Mouseless")
        self.build_menu()

    def build_menu(self):
        menu = Gtk.Menu()

        title = Gtk.MenuItem(label="Mouseless")
        title.set_sensitive(False)
        menu.append(title)

        self.toggle_item = Gtk.MenuItem()
        self.toggle_item.connect("activate", self.toggle)
        menu.append(self.toggle_item)

        stop = Gtk.MenuItem(label="Stop Mouseless")
        stop.connect("activate", self.stop)
        menu.append(stop)

        menu.append(Gtk.SeparatorMenuItem())
        quit_item = Gtk.MenuItem(label="Quit tray")
        quit_item.connect("activate", self.quit)
        menu.append(quit_item)

        menu.connect("show", lambda *_: self.refresh())
        menu.show_all()
        self.menu = menu
        self.indicator.set_menu(menu)
        self.refresh()

    def refresh(self):
        self.toggle_item.set_label("Toggle Grid" if running() else "Start Mouseless")

    def toggle(self, *_):
        try:
            if running():
                daemon_command("toggle")
            else:
                start_daemon()
        except OSError as error:
            print(f"mouseless tray: {error}", file=sys.stderr)
        GLib.timeout_add(500, self.refresh)

    def stop(self, *_):
        if running():
            daemon_command("quit")
        self.refresh()

    def quit(self, *_):
        Gtk.main_quit()


if __name__ == "__main__":
    if not Gtk.init_check()[0]:
        print("mouseless tray: no graphical display is available", file=sys.stderr)
        sys.exit(1)
    signal.signal(signal.SIGTERM, lambda *_: Gtk.main_quit())
    MouselessTray()
    Gtk.main()
