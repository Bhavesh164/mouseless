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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let val_str = args.get(1).map(|s| s.as_str()).unwrap_or("150.0");
    let dy: f64 = val_str.parse().unwrap();

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

    println!("Testing wlrctl-style finger scroll with dy={}", dy);

    for _ in 0..5 {
        let time = get_timestamp();
        vp.axis_source(AxisSource::Finger);
        vp.axis(time, Axis::VerticalScroll, dy);
        vp.frame();

        let time2 = get_timestamp();
        vp.axis_source(AxisSource::Finger);
        vp.axis_stop(time2, Axis::VerticalScroll);
        vp.frame();

        conn.flush().unwrap();
        sleep(Duration::from_millis(50));
    }

    event_queue.roundtrip(&mut state).unwrap();
}
