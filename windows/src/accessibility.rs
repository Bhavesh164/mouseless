use crate::core::ClickDetector;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker, CUIAutomation,
    UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_HyperlinkControlTypeId,
    UIA_ListItemControlTypeId, UIA_MenuItemControlTypeId, UIA_RadioButtonControlTypeId,
    UIA_SliderControlTypeId, UIA_SpinnerControlTypeId, UIA_SplitButtonControlTypeId,
    UIA_TabItemControlTypeId, UIA_TextControlTypeId, UIA_ThumbControlTypeId,
    UIA_TreeItemControlTypeId, UIA_CONTROLTYPE_ID,
};

pub struct UiAutomationDetector {
    automation: Option<IUIAutomation>,
}

impl UiAutomationDetector {
    pub fn new() -> Self {
        let automation = unsafe {
            CoCreateInstance::<_, IUIAutomation>(
                &CUIAutomation,
                None,
                CLSCTX_ALL,
            )
            .ok()
        };
        UiAutomationDetector { automation }
    }

    fn is_clickable(control_type: UIA_CONTROLTYPE_ID) -> bool {
        matches!(
            control_type,
            UIA_ButtonControlTypeId
                | UIA_CheckBoxControlTypeId
                | UIA_RadioButtonControlTypeId
                | UIA_ComboBoxControlTypeId
                | UIA_SplitButtonControlTypeId
                | UIA_MenuItemControlTypeId
                | UIA_HyperlinkControlTypeId
                | UIA_EditControlTypeId
                | UIA_DocumentControlTypeId
                | UIA_TextControlTypeId
                | UIA_SliderControlTypeId
                | UIA_SpinnerControlTypeId
                | UIA_ListItemControlTypeId
                | UIA_TreeItemControlTypeId
                | UIA_TabItemControlTypeId
                | UIA_ThumbControlTypeId
        )
    }

    fn clickable_bounds(
        &self,
        walker: &IUIAutomationTreeWalker,
        element: &IUIAutomationElement,
    ) -> Option<(f64, f64, f64, RECT)> {
        let mut current: Option<IUIAutomationElement> = Some(element.clone());
        for _ in 0..8 {
            let el = current?;
            if let Ok(control_type) = unsafe { el.CurrentControlType() } {
                if Self::is_clickable(control_type) {
                    if let Ok(rect) = unsafe { el.CurrentBoundingRectangle() } {
                        let cx = (rect.left + rect.right) as f64 / 2.0;
                        let cy = (rect.top + rect.bottom) as f64 / 2.0;
                        let area = ((rect.right - rect.left) * (rect.bottom - rect.top)) as f64;
                        return Some((cx, cy, area.max(1.0), rect));
                    }
                }
            }
            current = unsafe { walker.GetParentElement(Some(&el)) }.ok();
        }
        None
    }
}

fn scan_offsets() -> [(f64, f64); 25] {
    let mut points = [(0.0f64, 0.0f64); 25];
    points[0] = (0.0, 0.0);
    let mut idx = 1;
    for &offset in &[3.0f64, 6.0, 10.0] {
        let deltas = [
            (-offset, 0.0),
            (offset, 0.0),
            (0.0, -offset),
            (0.0, offset),
            (-offset, -offset),
            (offset, -offset),
            (-offset, offset),
            (offset, offset),
        ];
        for d in deltas {
            if idx < points.len() {
                points[idx] = d;
                idx += 1;
            }
        }
    }
    points
}

impl ClickDetector for UiAutomationDetector {
    fn snap_to_clickable(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        // This runs on a short-lived background thread; make sure it has joined
        // the process multithreaded apartment before touching the COM object.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let automation = self.automation.as_ref()?;
        let walker = unsafe { automation.ControlViewWalker() }.ok()?;

        let offsets = scan_offsets();
        // A browser often exposes a whole video card as one accessible
        // control.  Clicking its centre can land on the title even when the
        // user selected the thumbnail.  Keep the user's point when it is
        // already inside a clickable control; only use a centre point to snap
        // to a nearby control.
        let mut candidates: Vec<(f64, f64, f64, i32, bool)> = Vec::new();
        for (order, (ox, oy)) in offsets.iter().enumerate() {
            let px = (x + *ox) as i32;
            let py = (y + *oy) as i32;
            let element = match unsafe { automation.ElementFromPoint(POINT { x: px, y: py }) } {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some((cx, cy, area, rect)) = self.clickable_bounds(&walker, &element) {
                let contains_requested_point = x >= rect.left as f64
                    && x <= rect.right as f64
                    && y >= rect.top as f64
                    && y <= rect.bottom as f64;
                candidates.push((cx, cy, area, order as i32, contains_requested_point));
            }
        }

        if candidates.is_empty() {
            return None;
        }
        candidates.sort_by(|a, b| {
            b.4.cmp(&a.4).then_with(|| {
                a.2.partial_cmp(&b.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.3.cmp(&b.3))
            })
        });
        let candidate = candidates[0];
        if candidate.4 {
            Some((x, y))
        } else {
            Some((candidate.0, candidate.1))
        }
    }
}

// IUIAutomation wraps a raw COM pointer and is !Send/!Sync in windows-rs, but we
// only ever touch it from the single UI thread, so this is safe here.
unsafe impl Send for UiAutomationDetector {}
unsafe impl Sync for UiAutomationDetector {}

#[allow(dead_code)]
fn _unused(_r: RECT) {}
