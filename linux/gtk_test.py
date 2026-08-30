import gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, Gdk

def on_scroll(widget, event):
    print(f"SCROLL RECEIVED: direction={event.direction.value_nick}, delta_x={event.delta_x}, delta_y={event.delta_y}", flush=True)

def on_button(widget, event):
    print(f"BUTTON RECEIVED: button={event.button}", flush=True)
    return False

def on_focus(widget, event):
    print("FOCUS IN", flush=True)
    return False

def on_enter(widget, event):
    print("POINTER ENTER", flush=True)
    return False

win = Gtk.Window(title="Scroll Tester")
win.set_default_size(800, 600)
win.connect("destroy", Gtk.main_quit)

win.add_events(Gdk.EventMask.SCROLL_MASK | Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.ENTER_NOTIFY_MASK | Gdk.EventMask.FOCUS_CHANGE_MASK)
win.connect("scroll-event", on_scroll)
win.connect("button-press-event", on_button)
win.connect("focus-in-event", on_focus)
win.connect("enter-notify-event", on_enter)

win.fullscreen()
win.show_all()
Gtk.main()
