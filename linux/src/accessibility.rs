use crate::core::ClickDetector;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::component::ComponentProxy;
use zbus::Connection;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const DEST: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const MAX_NODES: usize = 2500;
const MAX_DEPTH: u32 = 12;
const BUDGET: Duration = Duration::from_millis(150);
const NEAR_PAD: f64 = 10.0;
const CLOSE_RADIUS: f64 = 16.0;

const CLICKABLE_ROLES: &[&str] = &[
    "push button",
    "toggle button",
    "check box",
    "radio button",
    "combo box",
    "menu item",
    "check menu item",
    "radio menu item",
    "link",
    "entry",
    "document web",
    "document text",
    "document email",
    "text",
    "slider",
    "spin button",
    "list item",
    "tree item",
    "table cell",
    "page tab",
];

struct Candidate {
    cx: f64,
    cy: f64,
    area: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    is_compact_close: bool,
}

impl Candidate {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    fn padded_contains(&self, x: f64, y: f64) -> bool {
        x >= self.x - NEAR_PAD
            && x <= self.x + self.w + NEAR_PAD
            && y >= self.y - NEAR_PAD
            && y <= self.y + self.h + NEAR_PAD
    }
}

/// Best-effort clickable-target snapping via the AT-SPI accessibility tree
/// (the Linux analogue of UI Automation on Windows / the AX tree on macOS).
/// Returns `None` whenever nothing confident is found, in which case the
/// caller performs a raw click at the requested point.
pub struct AtspiDetector;

impl AtspiDetector {
    pub fn new() -> Self {
        AtspiDetector
    }
}

struct Walker<'a> {
    accessible: HashMap<String, AccessibleProxy<'a>>,
    components: HashMap<String, ComponentProxy<'a>>,
    visited: HashSet<String>,
    candidates: Vec<Candidate>,
    deadline: Instant,
    count: usize,
}

impl<'a> Walker<'a> {
    async fn accessible_at(&mut self, conn: &Connection, path: &str) -> Option<AccessibleProxy<'a>> {
        if let Some(p) = self.accessible.get(path) {
            return Some(p.clone());
        }
        let proxy = AccessibleProxy::builder(conn)
            .destination(DEST)
            .ok()?
            .path(path.to_owned())
            .ok()?
            .build()
            .await
            .ok()?;
        self.accessible.insert(path.to_string(), proxy.clone());
        Some(proxy)
    }

    async fn extents_at(
        &mut self,
        conn: &Connection,
        path: &str,
    ) -> Option<(f64, f64, f64, f64)> {
        if !self.components.contains_key(path) {
            let proxy = ComponentProxy::builder(conn)
                .destination(DEST)
                .ok()?
                .path(path.to_owned())
                .ok()?
                .build()
                .await
                .ok()?;
            self.components.insert(path.to_string(), proxy);
        }
        let proxy = self.components.get(path)?;
        let extents = proxy.get_extents(atspi::proxy::common::CoordType::Screen).await.ok()?;
        if extents.2 <= 0 || extents.3 <= 0 {
            return None;
        }
        Some((extents.0 as f64, extents.1 as f64, extents.2 as f64, extents.3 as f64))
    }

    async fn walk(&mut self, conn: &Connection, path: &str, depth: u32) {
        if self.count >= MAX_NODES || Instant::now() > self.deadline || depth > MAX_DEPTH {
            return;
        }
        if !self.visited.insert(path.to_string()) {
            return;
        }
        self.count += 1;

        let Some(node) = self.accessible_at(conn, path).await else {
            return;
        };

        if let Some((x, y, w, h)) = self.extents_at(conn, path).await {
            if let Ok(role) = node.get_role_name().await {
                if CLICKABLE_ROLES.iter().any(|r| role.eq_ignore_ascii_case(r)) {
                    let name = node.name().await.unwrap_or_default().to_ascii_lowercase();
                    let is_compact_close = (name.contains("close") || name.contains("dismiss"))
                        && w <= 48.0
                        && h <= 48.0;
                    self.candidates.push(Candidate {
                        cx: x + w / 2.0,
                        cy: y + h / 2.0,
                        area: (w * h).max(1.0),
                        x,
                        y,
                        w,
                        h,
                        is_compact_close,
                    });
                }
            }
        }

        let Ok(count) = node.child_count().await else {
            return;
        };
        for index in 0..count {
            if self.count >= MAX_NODES || Instant::now() > self.deadline {
                return;
            }
            let Ok(child) = node.get_child_at_index(index).await else {
                continue;
            };
            let child_path = child.path.as_str().to_string();
            if child_path.is_empty() || child_path == path {
                continue;
            }
            Box::pin(self.walk(conn, &child_path, depth + 1)).await;
        }
    }
}

fn pick(candidates: &[Candidate], x: f64, y: f64) -> Option<(f64, f64)> {
    if candidates.is_empty() {
        return None;
    }

    // A nearby explicit compact Close control wins (e.g. browser tab close).
    if let Some(close) = candidates
        .iter()
        .filter(|c| c.is_compact_close)
        .min_by(|a, b| {
            (a.cx - x)
                .hypot(a.cy - y)
                .partial_cmp(&(b.cx - x).hypot(b.cy - y))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        if (close.cx - x).hypot(close.cy - y) <= CLOSE_RADIUS {
            return Some((close.cx, close.cy));
        }
    }

    // Point already inside a clickable control: keep the user's point.
    if candidates.iter().any(|c| c.contains(x, y)) {
        return Some((x, y));
    }

    // Otherwise snap to the centre of the smallest control near the point.
    if let Some(best) = candidates
        .iter()
        .filter(|c| c.padded_contains(x, y))
        .min_by(|a, b| a.area.partial_cmp(&b.area).unwrap_or(std::cmp::Ordering::Equal))
    {
        return Some((best.cx, best.cy));
    }

    None
}

impl ClickDetector for AtspiDetector {
    fn snap_to_clickable(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        async_std::task::block_on(async {
            let Ok(conn) = atspi::connection::AccessibilityConnection::new().await else {
                return None;
            };
            let bus = conn.connection();
            let mut walker = Walker {
                accessible: HashMap::new(),
                components: HashMap::new(),
                visited: HashSet::new(),
                candidates: Vec::new(),
                deadline: Instant::now() + BUDGET,
                count: 0,
            };
            walker.walk(bus, ROOT_PATH, 0).await;
            pick(&walker.candidates, x, y)
        })
    }
}
