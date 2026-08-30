use std::thread::sleep;
use std::time::Duration;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{
    protocol::{
        wl_pointer::{Axis, AxisSource},
        wl_seat::WlSeat,
    },
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct State {
    vp_manager: Option<ZwlrVirtualPointerManagerV1>,
    seat: Option<WlSeat>,
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "zwlr_virtual_pointer_manager_v1" {
                state.vp_manager = Some(registry.bind(name, version.min(2), qh, ()));
            } else if interface == "wl_seat" {
                state.seat = Some(registry.bind(name, 1, qh, ()));
            }
        }
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn get_timestamp() -> u32 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ((ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000)) as u32
}

fn parse_source(s: &str) -> AxisSource {
    match s {
        "wheel" => AxisSource::Wheel,
        "finger" => AxisSource::Finger,
        "continuous" => AxisSource::Continuous,
        "tilt" => AxisSource::WheelTilt,
        _ => AxisSource::Wheel,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Usage: test_scroll_brute <source> <mode> <value> <discrete> [order] [with_stop]
    // source: wheel/finger/continuous
    // mode: axis, discrete, both, both_rev, hypruse, move
    // value: fixed delta like 15.0
    // discrete: int steps like 1
    // order: before/after (axis_source before or after axis)
    // with_stop: 0/1 whether to send axis_stop after

    // Special move mode: test_scroll_brute move <x> <y>
    if args.get(1).map(|s| s.as_str()) == Some("move") {
        let x: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(400.0);
        let y: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(300.0);
        let conn = Connection::connect_to_env().unwrap();
        let mut event_queue = conn.new_event_queue();
        let qh = event_queue.handle();
        let display = conn.display();
        display.get_registry(&qh, ());
        let mut state = State { vp_manager: None, seat: None };
        event_queue.roundtrip(&mut state).unwrap();
        let mgr = state.vp_manager.as_ref().expect("No manager");
        let seat = state.seat.as_ref();
        let vp = mgr.create_virtual_pointer(seat, &qh, ());
        event_queue.roundtrip(&mut state).unwrap();
        // Park at 0,0
        for _ in 0..10 {
            vp.motion(get_timestamp(), -4000.0, -4000.0);
            vp.frame();
            conn.flush().unwrap();
            sleep(Duration::from_millis(10));
        }
        // Move to target
        vp.motion(get_timestamp(), x, y);
        vp.frame();
        conn.flush().unwrap();
        event_queue.roundtrip(&mut state).unwrap();
        println!("moved to {},{}", x, y);
        return;
    }

    let source_str = args.get(1).map(|s| s.as_str()).unwrap_or("wheel");
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("hypruse");
    let val: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15.0);
    let discrete: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    let order = args.get(5).map(|s| s.as_str()).unwrap_or("before");
    let with_stop: bool = args.get(6).map(|s| s.as_str()=="1").unwrap_or(false);

    let source = parse_source(source_str);

    let conn = Connection::connect_to_env().unwrap();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let display = conn.display();
    display.get_registry(&qh, ());

    let mut state = State {
        vp_manager: None,
        seat: None,
    };

    event_queue.roundtrip(&mut state).unwrap();

    let mgr = state.vp_manager.as_ref().expect("No zwlr_virtual_pointer_manager_v1");
    let seat = state.seat.as_ref();

    let vp = mgr.create_virtual_pointer(seat, &qh, ());
    event_queue.roundtrip(&mut state).unwrap();

    println!("TEST source={:?} mode={} val={} discrete={} order={} stop={}", source, mode, val, discrete, order, with_stop);

    let send = |vp: &ZwlrVirtualPointerV1, source: AxisSource, mode: &str, val: f64, discrete: i32, order: &str, with_stop: bool| {
        let time = get_timestamp();
        match mode {
            "axis" => {
                if order == "before" {
                    vp.axis_source(source);
                    vp.axis(time, Axis::VerticalScroll, val);
                } else {
                    vp.axis(time, Axis::VerticalScroll, val);
                    vp.axis_source(source);
                }
                vp.frame();
                conn.flush().unwrap();
                sleep(Duration::from_millis(50));
                if with_stop {
                    let time2 = get_timestamp();
                    // Hyprland expects axis_stop to be paired with source? Let's keep source before stop as well
                    vp.axis_source(source);
                    vp.axis_stop(time2, Axis::VerticalScroll);
                    vp.frame();
                    conn.flush().unwrap();
                }
            },
            "discrete" => {
                if order == "before" {
                    vp.axis_source(source);
                    vp.axis_discrete(time, Axis::VerticalScroll, val, discrete);
                } else {
                    vp.axis_discrete(time, Axis::VerticalScroll, val, discrete);
                    vp.axis_source(source);
                }
                vp.frame();
                conn.flush().unwrap();
                sleep(Duration::from_millis(50));
                if with_stop {
                    let time2 = get_timestamp();
                    vp.axis_source(source);
                    vp.axis_stop(time2, Axis::VerticalScroll);
                    vp.frame();
                    conn.flush().unwrap();
                }
            },
            "both" => {
                // axis + discrete in same frame
                // Order: source, axis, discrete
                if order == "before" {
                    vp.axis_source(source);
                    vp.axis(time, Axis::VerticalScroll, val);
                    vp.axis_discrete(time, Axis::VerticalScroll, val, discrete);
                } else {
                    vp.axis(time, Axis::VerticalScroll, val);
                    vp.axis_discrete(time, Axis::VerticalScroll, val, discrete);
                    vp.axis_source(source);
                }
                vp.frame();
                conn.flush().unwrap();
                sleep(Duration::from_millis(50));
                if with_stop {
                    let time2 = get_timestamp();
                    vp.axis_source(source);
                    vp.axis_stop(time2, Axis::VerticalScroll);
                    vp.frame();
                    conn.flush().unwrap();
                }
            },
            "both_rev" => {
                if order == "before" {
                    vp.axis_source(source);
                    vp.axis_discrete(time, Axis::VerticalScroll, val, discrete);
                    vp.axis(time, Axis::VerticalScroll, val);
                } else {
                    vp.axis_discrete(time, Axis::VerticalScroll, val, discrete);
                    vp.axis(time, Axis::VerticalScroll, val);
                    vp.axis_source(source);
                }
                vp.frame();
                conn.flush().unwrap();
                sleep(Duration::from_millis(50));
                if with_stop {
                    let time2 = get_timestamp();
                    vp.axis_source(source);
                    vp.axis_stop(time2, Axis::VerticalScroll);
                    vp.frame();
                    conn.flush().unwrap();
                }
            },
            "hypruse" => {
                // Exactly hypruse's scroll_messages logic for dy notches
                // It sends axis_source Wheel, then axis_discrete if integer notches, else axis, then frame
                // We'll replicate: source Wheel, then discrete with value v*15
                // Here val is notches? We'll treat val as notches
                // But our val param is already delta? For hypruse, dy=1 -> value=15
                // So if caller passes val=15, that's notches? confusing. We'll treat val as delta and compute notches
                // For hypruse mode, we interpret val as notches count (like 1, 2, 3)
                // and discrete = round(val)
                let notches = discrete; // use discrete param as notches
                let delta = val; // val is already delta? hypruse uses val=notches*15
                // Actually hypruse does value = v*15, notches = round(v)
                // So for notches=1, value=15
                // For notches=3, value=45
                // We'll follow that: value = notches*15
                let fixed_val = (notches as f64)*15.0;
                vp.axis_source(source);
                vp.axis_discrete(time, Axis::VerticalScroll, fixed_val, notches);
                vp.frame();
                conn.flush().unwrap();
            },
            "mouseless_current" => {
                // Replicate current mouseless wheel: Finger, axis 135ish, axis_stop, 2 frames
                let dx = 0.0;
                let dy = val; // should be 135
                let time = get_timestamp();
                vp.axis_source(AxisSource::Finger);
                vp.axis(time, Axis::VerticalScroll, dy);
                vp.frame();
                let time2 = get_timestamp();
                vp.axis_source(AxisSource::Finger);
                vp.axis_stop(time2, Axis::VerticalScroll);
                vp.frame();
                conn.flush().unwrap();
                // Also test horizontal if needed? No
            },
            _ => {
                println!("unknown mode {}", mode);
            }
        }
    };

    // Send 5 times with interval
    for i in 0..3 {
        println!("  iteration {}", i+1);
        send(&vp, source, mode, val, discrete, order, with_stop);
        sleep(Duration::from_millis(200));
    }

    event_queue.roundtrip(&mut state).unwrap();
    println!("done");
}
