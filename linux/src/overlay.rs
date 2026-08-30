use crate::common::Rect;
use crate::core::Snapshot;
use crate::render;
use crate::virtual_pointer::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_compositor::WlCompositor,
    wl_output::{Event as OutputEvent, WlOutput},
    wl_registry::{self, WlRegistry},
    wl_region::WlRegion,
    wl_seat::{Event as SeatEvent, WlSeat},
    wl_shm::{Format, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

pub struct Globals {
    pub compositor: Option<WlCompositor>,
    pub shm: Option<WlShm>,
    pub seat: Option<WlSeat>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub vp_manager: Option<ZwlrVirtualPointerManagerV1>,
    pub vp_manager_version: u32,
    pub region: Option<WlRegion>,
}

impl Default for Globals {
    fn default() -> Self {
        Globals {
            compositor: None,
            shm: None,
            seat: None,
            layer_shell: None,
            vp_manager: None,
            vp_manager_version: 0,
            region: None,
        }
    }
}

pub struct OutputData {
    pub output: WlOutput,
    pub name: String,
    pub scale: i32,
    pub width: i32,
    pub height: i32,
}

struct BufferInfo {
    wl_buffer: WlBuffer,
    _file: File,
    /// mmap of `_file`: the compositor reads the pool from this mapping, so
    /// all rendering must go through `data()` and never into a private copy.
    map: *mut libc::c_void,
    len: usize,
    width: i32,
    height: i32,
}

impl BufferInfo {
    fn data(&mut self) -> &mut [u32] {
        debug_assert!(!self.map.is_null());
        unsafe { std::slice::from_raw_parts_mut(self.map.cast::<u32>(), self.len / 4) }
    }
}

impl Drop for BufferInfo {
    fn drop(&mut self) {
        if !self.map.is_null() {
            unsafe { libc::munmap(self.map, self.len) };
            self.map = std::ptr::null_mut();
        }
    }
}

pub struct OverlayWindow {
    pub rect: Rect,
    pub scale: i32,
    surface: WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    configured: bool,
    mapped: bool,
    want_visible: bool,
    buffers: [Option<BufferInfo>; 2],
    current_buffer: usize,
}

struct FreeSurface {
    surface: WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    /// Mirrors OverlayWindow::want_visible: without this guard a stray
    /// configure from the compositor would attach a transparent buffer and
    /// map an invisible surface that swallows the keyboard exclusively.
    want_mapped: bool,
    /// The compositor drops a layer surface's configured state on unmap, so
    /// a buffer may only be attached after a configure acked this session.
    configured: bool,
    mapped: bool,
    buffer: Option<BufferInfo>,
}

pub struct Srv {
    pub globals: Globals,
    pub outputs: Vec<OutputData>,
    pub overlays: Vec<OverlayWindow>,
    free: Option<FreeSurface>,
    snapshot: Option<Snapshot>,
}

impl Srv {
    pub fn new() -> Self {
        Srv {
            globals: Globals::default(),
            outputs: Vec::new(),
            overlays: Vec::new(),
            free: None,
            snapshot: None,
        }
    }

    /// Creates overlay surfaces (one per output) plus the invisible
    /// keyboard-stealing surface used by free mode.
    pub fn setup_surfaces(&mut self, rects: Vec<(String, Rect, i32)>, qh: &QueueHandle<Self>) {
        let Some(compositor) = self.globals.compositor.clone() else {
            return;
        };
        let Some(shell) = self.globals.layer_shell.clone() else {
            return;
        };
        if self.globals.region.is_none() {
            let region = compositor.create_region(qh, ());
            region.add(0, 0, 0, 0);
            self.globals.region = Some(region);
        }

        self.overlays = Vec::new();
        for (name, rect, scale) in rects {
            let Some(output) = self
                .outputs
                .iter()
                .find(|o| o.name == name)
                .map(|o| o.output.clone())
            else {
                continue;
            };
            let surface = compositor.create_surface(qh, ());
            let layer_surface = shell.get_layer_surface(
                &surface,
                Some(&output),
                Layer::Overlay,
                "mouseless".to_string(),
                qh,
                (),
            );
            // Sizing 0x0 with 4-way anchors tells compositor to span the full output.
            layer_surface.set_size(0, 0);
            layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
            layer_surface.set_exclusive_zone(-1);
            layer_surface.set_margin(0, 0, 0, 0);
            layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            if let Some(region) = &self.globals.region {
                surface.set_input_region(Some(region));
            }
            self.overlays.push(OverlayWindow {
                rect,
                scale,
                surface,
                layer_surface,
                configured: false,
                mapped: false,
                want_visible: false,
                buffers: [None, None],
                current_buffer: 0,
            });
        }

        // Invisible surface that consumes keyboard input in free mode.
        if let Some(seat) = self.globals.seat.clone() {
            let _ = seat; // keyboard focus is stolen via the layer surface
        }
        let surface = compositor.create_surface(qh, ());
        let layer_surface = shell.get_layer_surface(
            &surface,
            None,
            Layer::Overlay,
            "mouseless-free".to_string(),
            qh,
            (),
        );
        layer_surface.set_size(0, 0);
        layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_margin(0, 0, 0, 0);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Some(region) = &self.globals.region {
            surface.set_input_region(Some(region));
        }
        self.free = Some(FreeSurface {
            surface,
            layer_surface,
            want_mapped: false,
            configured: false,
            mapped: false,
            buffer: None,
        });
    }

    pub fn show_overlays(&mut self, snapshot: Snapshot) {
        self.snapshot = Some(snapshot);
        for w in &mut self.overlays {
            w.want_visible = true;
            // Keep Wayland keyboard interactivity at None – the grid
            // receives all keys via raw evdev (EVIOCGRAB) while visible,
            // so it does not need Wayland focus.  Using OnDemand or
            // Exclusive here makes Hyprland put the overlay in
            // m_exclusiveLSes / m_lastFocusOnLS, which forces
            // onMouseWheel to send axis to the overlay (which has an
            // empty input region and ignores it) instead of the window
            // under the cursor – scroll appears broken.  None keeps
            // pointer focus on the underlying window so wheel events
            // reach it, while EVIOCGRAB still blocks Space/J/K from
            // reaching YouTube/rofi.  Tested: None + EVIOCGRAB fixes
            // both YouTube leak and rofi/scroll.
            w.layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            w.configured = false;
            w.surface.commit();
        }
    }

    pub fn hide_overlays(&mut self) {
        for w in &mut self.overlays {
            w.want_visible = false;
            w.layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            if w.mapped {
                w.surface.attach(None, 0, 0);
                w.surface.commit();
            }
            w.mapped = false;
            w.configured = false;
        }
        self.snapshot = None;
    }

    pub fn render(&mut self, snapshot: &Snapshot) {
        self.snapshot = Some(snapshot.clone());
        for w in &mut self.overlays {
            if w.want_visible && w.configured && w.mapped {
                Self::render_window(w, snapshot);
            }
        }
    }

    pub fn map_free_surface(&mut self) {
        if let Some(free) = &mut self.free {
            free.want_mapped = true;
            free.layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            if free.configured && free.buffer.is_some() {
                let buffer = free.buffer.as_mut().unwrap();
                buffer.data().fill(0);
                free.surface.attach(Some(&buffer.wl_buffer), 0, 0);
                free.surface.damage_buffer(0, 0, buffer.width, buffer.height);
                free.surface.damage(0, 0, i32::MAX, i32::MAX);
                free.surface.commit();
                free.mapped = true;
            } else {
                free.surface.commit();
            }
        }
    }

    pub fn unmap_free_surface(&mut self) {
        if let Some(free) = &mut self.free {
            free.want_mapped = false;
            free.layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            if free.mapped {
                free.surface.attach(None, 0, 0);
                free.surface.commit();
            }
            free.mapped = false;
            free.configured = false;
        }
    }

fn ensure_buffer(
    shm: &WlShm,
    qh: &QueueHandle<Srv>,
    w: i32,
    h: i32,
) -> Option<BufferInfo> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let len = (w as usize) * (h as usize) * 4;
    let memfd = unsafe { libc::memfd_create(c"mouseless".as_ptr(), 0) };
    if memfd < 0 {
        return None;
    }
    let file = unsafe { File::from_raw_fd(memfd) };
    file.set_len(len as u64).ok()?;
    let pool: WlShmPool = shm.create_pool(file.as_fd(), len as i32, qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, Format::Argb8888, qh, ());
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if map == libc::MAP_FAILED {
        return None;
    }
    Some(BufferInfo {
        wl_buffer: buffer,
        _file: file,
        map,
        len,
        width: w,
        height: h,
    })
}

fn render_window(w: &mut OverlayWindow, snapshot: &Snapshot) {
    let next_buf_idx = (w.current_buffer + 1) % 2;
    let buf_idx = if w.buffers[next_buf_idx].is_some() {
        w.current_buffer = next_buf_idx;
        next_buf_idx
    } else {
        0
    };
    let Some(buffer) = w.buffers[buf_idx].as_mut() else {
        return;
    };
    let scale = w.scale.max(1);
    let buf_w = buffer.width;
    let buf_h = buffer.height;
    render::render_snapshot(
        buffer.data(),
        buf_w,
        buf_h,
        scale,
        (w.rect.left, w.rect.top),
        snapshot,
    );
    w.surface.set_buffer_scale(scale);
    w.surface.attach(Some(&buffer.wl_buffer), 0, 0);
    w.surface.damage_buffer(0, 0, buf_w, buf_h);
    w.surface.damage(0, 0, i32::MAX, i32::MAX);
    w.surface.commit();
}
}

fn layer_configure(
    srv: &mut Srv,
    qh: &QueueHandle<Srv>,
    this: &ZwlrLayerSurfaceV1,
    serial: u32,
    width: u32,
    height: u32,
) {
    // Overlay surfaces.
    let Some(shm) = srv.globals.shm.clone() else {
        return;
    };
    for w in srv.overlays.iter_mut() {
        if w.layer_surface.id() == this.id() {
            w.layer_surface.ack_configure(serial);
            let lw = if width > 0 { width as i32 } else { w.rect.width() as i32 };
            let lh = if height > 0 { height as i32 } else { w.rect.height() as i32 };
            let scale = w.scale.max(1);
            let need_realloc = match &w.buffers[0] {
                Some(b) => b.width != lw * scale || b.height != lh * scale,
                None => true,
            };
            if need_realloc {
                w.buffers[0] = Srv::ensure_buffer(&shm, qh, lw * scale, lh * scale);
                w.buffers[1] = Srv::ensure_buffer(&shm, qh, lw * scale, lh * scale);
                w.current_buffer = 0;
            }
            w.configured = true;
            // A stray configure while hidden must not attach a buffer:
            // attaching maps the surface and it holds an exclusive keyboard grab.
            if w.want_visible {
                w.mapped = true;
                if let Some(snapshot) = srv.snapshot.clone() {
                    Srv::render_window(w, &snapshot);
                }
            }
            return;
        }
    }

    // Free surface.
    if let Some(free) = &mut srv.free {
        if free.layer_surface.id() == this.id() {
            free.layer_surface.ack_configure(serial);
            let lw = if width > 0 { width as i32 } else { 1 };
            let lh = if height > 0 { height as i32 } else { 1 };
            let need_realloc = match &free.buffer {
                Some(b) => b.width != lw || b.height != lh,
                None => true,
            };
            if need_realloc {
                free.buffer = Srv::ensure_buffer(&shm, qh, lw, lh);
            }
            free.configured = true;
            // Only attach when free mode actually wants the surface: attaching
            // maps it, and this surface grabs the keyboard exclusively.
            if free.want_mapped {
                if let Some(buffer) = &mut free.buffer {
                    buffer.data().fill(0);
                    free.surface.attach(Some(&buffer.wl_buffer), 0, 0);
                    free.surface.damage_buffer(0, 0, buffer.width, buffer.height);
                    free.surface.commit();
                    free.mapped = true;
                }
            }
            return;
        }
    }
}

fn layer_closed(srv: &mut Srv, this: &ZwlrLayerSurfaceV1) {
    for w in srv.overlays.iter_mut() {
        if w.layer_surface.id() == this.id() {
            w.want_visible = false;
            w.mapped = false;
            w.configured = false;
            return;
        }
    }
    if let Some(free) = &mut srv.free {
        if free.layer_surface.id() == this.id() {
            free.want_mapped = false;
            free.mapped = false;
            free.configured = false;
        }
    }
}

impl Dispatch<WlRegistry, ()> for Srv {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_compositor" => {
                    state.globals.compositor = Some(
                        registry.bind(name, version.min(5), qh, ()),
                    );
                }
                "wl_shm" => {
                    state.globals.shm = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "wl_seat" => {
                    state.globals.seat = Some(registry.bind(name, version.min(7), qh, ()));
                }
                "wl_output" => {
                    let output: WlOutput =
                        registry.bind(name, version.min(4), qh, ());
                    state.outputs.push(OutputData {
                        output,
                        name: String::new(),
                        scale: 1,
                        width: 0,
                        height: 0,
                    });
                }
                "zwlr_layer_shell_v1" => {
                    state.globals.layer_shell = Some(
                        registry.bind(name, version.min(4), qh, ()),
                    );
                }
                "zwlr_virtual_pointer_manager_v1" => {
                    // Bind the advertised version (capped at our XML's max).
                    let bound = version.min(2);
                    state.globals.vp_manager_version = bound;
                    state.globals.vp_manager =
                        Some(registry.bind(name, bound, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlCompositor, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: <WlCompositor as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, ()> for Srv {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: OutputEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Output events can be interleaved across monitors. Updating the most
        // recently announced output made two physical monitors inherit the
        // same geometry/name, which in turn created overlapping layer
        // surfaces (two grids on one monitor). Route each event by proxy.
        let Some(data) = state
            .outputs
            .iter_mut()
            .find(|data| data.output.id() == output.id())
        else {
            return;
        };
        match event {
            OutputEvent::Name { name } => data.name = name,
            OutputEvent::Scale { factor } => data.scale = factor,
            OutputEvent::Mode {
                flags: _,
                width,
                height,
                refresh: _,
            } => {
                data.width = width;
                data.height = height;
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        event: SeatEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let SeatEvent::Capabilities { .. } = event {
            // We never bind a wl_keyboard: keyboard input is consumed by the
            // exclusive-focus layer surfaces and read directly from evdev.
        }
    }
}

impl Dispatch<WlSurface, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: <WlSurface as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlBuffer, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: <WlBuffer as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlShmPool,
        _: <WlShmPool as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShm, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: <WlShm as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegion, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &WlRegion,
        _: <WlRegion as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for Srv {
    fn event(
        state: &mut Self,
        surface: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Event as LayerEvent;
        match event {
            LayerEvent::Configure {
                serial,
                width,
                height,
            } => layer_configure(state, qh, surface, serial, width, height),
            LayerEvent::Closed => layer_closed(state, surface),
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: <ZwlrLayerShellV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for Srv {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()>
    for Srv
{
    fn event(
        _: &mut Self,
        _: &crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
        _: <crate::virtual_pointer::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// Keep WEnum imported for event enum conversions in generated code paths.
#[allow(dead_code)]
fn _wenum_check(_v: wayland_client::WEnum<wayland_client::protocol::wl_seat::Capability>) {}
