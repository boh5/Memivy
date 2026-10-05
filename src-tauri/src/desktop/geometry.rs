//! Panel placement uses AppKit's global logical coordinates throughout.
use objc2_foundation::{NSPoint, NSRect, NSSize};

pub(super) const LEAF_SIZE: NSSize = NSSize::new(72.0, 76.0);

#[derive(Clone, Copy)]
pub(super) enum Placement {
    KeepAnchor,
    KeepTop,
    Cursor,
    SavedLeaf,
}

fn fit_size(size: NSSize, area: NSRect) -> NSSize {
    NSSize::new(
        size.width.min((area.size.width - 16.0).max(1.0)),
        size.height.min((area.size.height - 16.0).max(1.0)),
    )
}

pub(super) fn fit_and_clamp(mut frame: NSRect, area: NSRect) -> NSRect {
    let size = fit_size(frame.size, area);
    // Keep the title's top edge fixed while fitting a shorter destination.
    frame.origin.y += frame.size.height - size.height;
    frame.size = size;
    let left = area.origin.x + 8.0;
    let bottom = area.origin.y + 8.0;
    frame.origin.x = frame.origin.x.clamp(
        left,
        (area.origin.x + area.size.width - frame.size.width - 8.0).max(left),
    );
    frame.origin.y = frame.origin.y.clamp(
        bottom,
        (area.origin.y + area.size.height - frame.size.height - 8.0).max(bottom),
    );
    frame
}

pub(super) fn panel_frame(
    requested: NSSize,
    placement: Placement,
    current: NSRect,
    saved: Option<NSPoint>,
    area: NSRect,
) -> NSRect {
    let size = fit_size(requested, area);
    let origin = match placement {
        Placement::KeepTop => NSPoint::new(
            current.origin.x,
            current.origin.y + current.size.height - size.height,
        ),
        Placement::KeepAnchor => NSPoint::new(
            current.origin.x + current.size.width - size.width,
            current.origin.y,
        ),
        Placement::Cursor => NSPoint::new(
            area.origin.x + (area.size.width - size.width) / 2.0,
            area.origin.y + (area.size.height - size.height) * 2.0 / 3.0,
        ),
        Placement::SavedLeaf => saved
            .map(|p| NSPoint::new(p.x - (size.width - LEAF_SIZE.width), p.y))
            .unwrap_or(NSPoint::new(
                area.origin.x + area.size.width - size.width - 28.0,
                area.origin.y + 72.0,
            )),
    };
    fit_and_clamp(NSRect::new(origin, size), area)
}

/// Stored coordinates retain their existing top-left physical representation.
/// Recording its scale makes the representation independent of the next screen.
pub(super) fn saved_leaf(position: (i32, i32), scale: f64, primary_top: f64) -> NSPoint {
    NSPoint::new(
        position.0 as f64 / scale,
        primary_top - position.1 as f64 / scale - LEAF_SIZE.height,
    )
}

pub(super) fn save_leaf(frame: NSRect, scale: f64, primary_top: f64) -> (i32, i32) {
    (
        ((frame.origin.x + frame.size.width - LEAF_SIZE.width) * scale).round() as i32,
        ((primary_top - frame.origin.y - LEAF_SIZE.height) * scale).round() as i32,
    )
}

pub(super) fn center(frame: NSRect) -> NSPoint {
    NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    )
}

/// Use the nearest display when a stored position's display was disconnected.
pub(super) fn screen_for_point(screens: &[NSRect], point: NSPoint) -> usize {
    screens
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            distance_to_rect(**a, point).total_cmp(&distance_to_rect(**b, point))
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn distance_to_rect(frame: NSRect, point: NSPoint) -> f64 {
    let dx = point.x
        - point
            .x
            .clamp(frame.origin.x, frame.origin.x + frame.size.width);
    let dy = point.y
        - point
            .y
            .clamp(frame.origin.y, frame.origin.y + frame.size.height);
    dx * dx + dy * dy
}

#[derive(Default)]
pub(super) struct Drag {
    latest: u64,
    active: Option<(u64, NSRect)>,
}

impl Drag {
    pub(super) fn start(&mut self, gesture: u64, frame: NSRect) -> bool {
        if gesture <= self.latest {
            return false;
        }
        self.latest = gesture;
        self.active = Some((gesture, frame));
        true
    }

    pub(super) fn frame(&self, gesture: u64, x: f64, y: f64) -> Option<NSRect> {
        self.active
            .filter(|(id, _)| *id == gesture)
            .map(|(_, frame)| {
                NSRect::new(
                    NSPoint::new(frame.origin.x + x, frame.origin.y - y),
                    frame.size,
                )
            })
    }

    pub(super) fn finish(&mut self, gesture: u64) -> bool {
        if self.active.is_some_and(|(id, _)| id == gesture) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub(super) fn relayout(&mut self, previous: NSRect, next: NSRect) {
        if let Some((_, frame)) = &mut self.active {
            frame.origin.x += next.origin.x - previous.origin.x;
            frame.origin.y += next.origin.y - previous.origin.y;
            frame.size = next.size;
        }
    }

    pub(super) fn cancel(&mut self) {
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
    }

    #[test]
    fn content_resize_preserves_top_and_open_preserves_leaf_anchor() {
        let area = rect(-1920.0, 30.0, 1920.0, 1050.0);
        let current = rect(-1500.0, 200.0, 500.0, 300.0);
        let resized = panel_frame(
            NSSize::new(500.0, 337.0),
            Placement::KeepTop,
            current,
            None,
            area,
        );
        assert_eq!(resized, rect(-1500.0, 163.0, 500.0, 337.0));
        let restored = panel_frame(current.size, Placement::KeepTop, resized, None, area);
        assert_eq!(restored, current);
        let leaf = rect(-800.0, 200.0, 72.0, 76.0);
        let open = panel_frame(
            NSSize::new(500.0, 620.0),
            Placement::KeepAnchor,
            leaf,
            None,
            area,
        );
        assert_eq!(open, rect(-1228.0, 200.0, 500.0, 620.0));
        let collapsed = panel_frame(LEAF_SIZE, Placement::KeepAnchor, open, None, area);
        assert_eq!(collapsed, leaf);
    }

    #[test]
    fn panel_stays_inside_negative_display_work_area() {
        let area = rect(-1920.0, 30.0, 1920.0, 1050.0);
        assert_eq!(
            fit_and_clamp(rect(-5000.0, -200.0, 500.0, 620.0), area).origin,
            NSPoint::new(-1912.0, 38.0)
        );
        assert_eq!(
            fit_and_clamp(rect(4000.0, 2000.0, 500.0, 620.0), area).origin,
            NSPoint::new(-508.0, 452.0)
        );
        let fit = panel_frame(
            NSSize::new(4000.0, 3000.0),
            Placement::KeepTop,
            area,
            None,
            area,
        );
        assert_eq!(fit, rect(-1912.0, 38.0, 1904.0, 1034.0));
    }

    #[test]
    fn a_dragged_panel_fits_the_destination_and_keeps_its_title_visible() {
        let destination = rect(-800.0, 20.0, 800.0, 540.0);
        let final_frame = fit_and_clamp(rect(-700.0, -80.0, 500.0, 620.0), destination);
        assert_eq!(final_frame, rect(-700.0, 28.0, 500.0, 524.0));
        assert!(final_frame.origin.y + final_frame.size.height <= 552.0);
        let narrow = fit_and_clamp(
            rect(-700.0, -80.0, 500.0, 620.0),
            rect(-400.0, 20.0, 400.0, 540.0),
        );
        assert_eq!(narrow, rect(-392.0, 28.0, 384.0, 524.0));
    }

    #[test]
    fn content_resize_keeps_an_active_drag_continuous_and_finishable() {
        let mut drag = Drag::default();
        let original = rect(200.0, 300.0, 500.0, 260.0);
        assert!(drag.start(1, original));
        let moved = drag.frame(1, 20.0, 30.0).unwrap();
        let resized = panel_frame(
            NSSize::new(500.0, 400.0),
            Placement::KeepTop,
            moved,
            None,
            rect(0.0, 0.0, 1440.0, 900.0),
        );
        drag.relayout(moved, resized);
        assert_eq!(drag.frame(1, 20.0, 30.0), Some(resized));
        assert_eq!(
            drag.frame(1, 25.0, 40.0),
            Some(rect(225.0, 120.0, 500.0, 400.0))
        );
        assert!(drag.finish(1));
    }

    #[test]
    fn receipt_expiry_preserves_drag_offset_and_the_final_leaf_anchor() {
        let mut drag = Drag::default();
        let receipt = rect(100.0, 300.0, 230.0, 76.0);
        assert!(drag.start(1, receipt));
        let moved = drag.frame(1, 30.0, 10.0).unwrap();
        let leaf = panel_frame(
            LEAF_SIZE,
            Placement::KeepAnchor,
            moved,
            None,
            rect(0.0, 0.0, 1440.0, 900.0),
        );
        drag.relayout(moved, leaf);
        assert_eq!(drag.frame(1, 30.0, 10.0), Some(leaf));
        let released = drag.frame(1, 40.0, 20.0).unwrap();
        assert_eq!(released, rect(298.0, 280.0, 72.0, 76.0));
        assert!(drag.finish(1));
        let saved = save_leaf(released, 2.0, 900.0);
        assert_eq!(saved_leaf(saved, 2.0, 900.0), released.origin);
    }

    #[test]
    fn saved_anchor_restores_on_a_different_scale_without_changing_logical_position() {
        let primary_top = 900.0;
        let open = rect(-1300.0, 100.0, 500.0, 620.0);
        for saved_scale in [1.0, 2.0] {
            let position = save_leaf(open, saved_scale, primary_top);
            let anchor = saved_leaf(position, saved_scale, primary_top);
            assert_eq!(anchor, NSPoint::new(-872.0, 100.0));
            let area = rect(-1920.0, 30.0, 1920.0, 1050.0);
            let leaf = panel_frame(LEAF_SIZE, Placement::SavedLeaf, open, Some(anchor), area);
            assert_eq!(leaf, rect(-872.0, 100.0, 72.0, 76.0));
            let receipt = panel_frame(
                NSSize::new(230.0, 76.0),
                Placement::SavedLeaf,
                open,
                Some(anchor),
                area,
            );
            assert_eq!(receipt.origin, NSPoint::new(-1030.0, 100.0));
        }
    }

    #[test]
    fn logical_display_selection_handles_mixed_scales_and_disconnected_displays() {
        // These are AppKit points; neither a 2x primary nor a 1x secondary
        // changes a screen's global coordinates or the movement delta.
        let screens = [
            rect(0.0, 0.0, 1440.0, 900.0),
            rect(-1920.0, 0.0, 1920.0, 1080.0),
        ];
        assert_eq!(screen_for_point(&screens, NSPoint::new(-1700.0, 800.0)), 1);
        assert_eq!(screen_for_point(&screens, NSPoint::new(1400.0, 800.0)), 0);
        assert_eq!(
            screen_for_point(&screens[..1], NSPoint::new(-1700.0, 800.0)),
            0
        );
    }

    #[test]
    fn final_pointer_offset_wins_and_stale_gestures_cannot_move_or_end_the_next_drag() {
        let mut drag = Drag::default();
        let origin = rect(200.0, 300.0, 72.0, 76.0);
        assert!(drag.start(1, origin));
        assert_eq!(
            drag.frame(1, 20.0, 30.0),
            Some(rect(220.0, 270.0, 72.0, 76.0))
        );
        let release = drag.frame(1, 24.0, 36.0).unwrap();
        assert_eq!(release, rect(224.0, 264.0, 72.0, 76.0));
        assert!(drag.start(2, release));
        assert!(!drag.finish(1));
        assert!(!drag.start(1, origin));
        assert_eq!(drag.frame(1, 100.0, 100.0), None);
        assert_eq!(
            drag.frame(2, -12.0, 8.0),
            Some(rect(212.0, 256.0, 72.0, 76.0))
        );
        assert!(drag.finish(2));
        assert_eq!(drag.frame(2, 900.0, 900.0), None);
        assert!(!drag.start(2, origin));
    }
}
