//! The crop tool: the whole straightened photo with the crop frame over it, and the panel with
//! aspect presets, Straighten, rotate and flip.
//!
//! Drag a corner or an edge to resize the frame, inside it to move it. The frame never leaves
//! the straightened image, and keeps its proportions when an aspect is chosen.

use darkroom_core::geometry::centred;
use darkroom_core::Geometry;
use rune::{Drag, El, LineCap, Phase, Shape, Tx, canvas, col, row, spacer, text};

use crate::State;
use crate::develop::{Adjustment, Readout, adj, slider};
use crate::theme as t;
use crate::ui::{button, segmented};

/// Aspect presets: width over height, or None for free. `ORIGINAL` means the photo's own.
pub const ORIGINAL: f32 = -1.0;
const ASPECTS: [(Option<f32>, &str); 5] = [(None, "Free"), (Some(ORIGINAL), "Original"), (Some(1.0), "1:1"), (Some(0.8), "4:5"), (Some(16.0 / 9.0), "16:9")];

/// How close a press must be to a corner or edge to grab it, in points.
const GRAB: f32 = 16.0;
/// The smallest crop, as a fraction of a side.
const MIN_SIDE: f32 = 0.05;

const STRAIGHTEN: Adjustment = Adjustment { min: -45.0, max: 45.0, readout: Readout::Degrees, ..adj("straighten", "Straighten", |e| e.geometry.straighten, |e, v| e.geometry.straighten = v) };

/// What a press on the frame grabbed: which edges move.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grab {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
}

impl Grab {
    const MOVE: Grab = Grab { left: true, top: true, right: true, bottom: true };

    fn encode(self) -> f32 {
        (self.left as u8 | (self.top as u8) << 1 | (self.right as u8) << 2 | (self.bottom as u8) << 3) as f32
    }

    fn decode(v: f32) -> Grab {
        let b = v as u8;
        Grab { left: b & 1 != 0, top: b & 2 != 0, right: b & 4 != 0, bottom: b & 8 != 0 }
    }
}

/// The crop as it would be after dragging `grab` by (`dx`, `dy`) (canvas fractions) from
/// `start`, kept to a minimum size and, when `aspect` is set (width / height in pixels on a canvas
/// of `canvas` pixels), to that aspect.
fn dragged(start: [f32; 4], grab: Grab, dx: f32, dy: f32, aspect: Option<f32>, canvas: [f32; 2]) -> [f32; 4] {
    let [x, y, w, h] = start;
    if grab == Grab::MOVE {
        return [(x + dx).clamp(0.0, 1.0 - w), (y + dy).clamp(0.0, 1.0 - h), w, h];
    }
    let (mut l, mut t, mut r, mut b) = (x, y, x + w, y + h);
    if grab.left {
        l = (l + dx).min(r - MIN_SIDE);
    }
    if grab.right {
        r = (r + dx).max(l + MIN_SIDE);
    }
    if grab.top {
        t = (t + dy).min(b - MIN_SIDE);
    }
    if grab.bottom {
        b = (b + dy).max(t + MIN_SIDE);
    }
    let (l, t, r, b) = (l.max(0.0), t.max(0.0), r.min(1.0), b.min(1.0));
    let (mut w, mut h) = (r - l, b - t);
    if let Some(a) = aspect {
        // In fractions, the aspect is a / (canvas aspect). Side edges set the width; top and
        // bottom edges set the height; corners keep whichever the drag grew less.
        let k = a / (canvas[0] / canvas[1]);
        let by_width = w / k;
        let by_height = h * k;
        let sides = grab.left || grab.right;
        let ends = grab.top || grab.bottom;
        if sides && (!ends || by_width <= h) { h = by_width } else { w = by_height }
    }
    // Anchor the edge opposite the one dragged.
    let nx = if grab.left && !grab.right { r - w } else { l };
    let ny = if grab.top && !grab.bottom { b - h } else { t };
    [nx, ny, w, h]
}

/// Which part of the frame a press at (`px`, `py`) points on a `w` × `h` point slot grabbed.
fn grabbed(crop: [f32; 4], px: f32, py: f32, w: f32, h: f32) -> Option<Grab> {
    let [x, y, cw, ch] = crop;
    let (l, t, r, b) = (x * w, y * h, (x + cw) * w, (y + ch) * h);
    let near = |a: f32, b: f32| (a - b).abs() <= GRAB;
    let within = |v: f32, lo: f32, hi: f32| v >= lo - GRAB && v <= hi + GRAB;
    let g = Grab {
        left: near(px, l) && within(py, t, b),
        right: near(px, r) && within(py, t, b),
        top: near(py, t) && within(px, l, r),
        bottom: near(py, b) && within(px, l, r),
    };
    if g.left || g.right || g.top || g.bottom {
        Some(g)
    } else if px > l && px < r && py > t && py < b {
        Some(Grab::MOVE)
    } else {
        None
    }
}

impl State {
    /// The chosen aspect, as width over height in output pixels.
    fn crop_ratio(&self) -> Option<f32> {
        let canvas = self.edit.geometry.oriented_size(self.photo.size);
        self.crop_aspect.map(|a| if a == ORIGINAL { canvas[0] / canvas[1] } else { a })
    }
}

/// The crop frame over the photo slot: shades what's cropped away, draws the frame with rule-of-
/// thirds lines and corner handles, and takes the drags.
pub fn overlay(s: &State, slot: El) -> El {
    let g = &s.edit.geometry;
    let [cw, ch] = g.oriented_size(s.photo.size);
    let [x, y, w, h] = g.effective_crop(s.photo.size);
    // Drawn in canvas pixels, stretched over the slot (which has the canvas's proportions).
    let px = |fx: f32, fy: f32| [fx * cw, fy * ch];
    let (l, tp, r, b) = (x * cw, y * ch, (x + w) * cw, (y + h) * ch);
    let line = cw / 700.0;
    let shade = t::BG_CANVAS.alpha(0.62);
    let ink = t::INK;
    let mut shape = Shape::new()
        .fill(vec![px(0.0, 0.0), px(1.0, 0.0), [cw, tp], [0.0, tp]], shade)
        .fill(vec![[0.0, b], [cw, b], px(1.0, 1.0), px(0.0, 1.0)], shade)
        .fill(vec![[0.0, tp], [l, tp], [l, b], [0.0, b]], shade)
        .fill(vec![[r, tp], [cw, tp], [cw, b], [r, b]], shade)
        .stroke(vec![[l, tp], [r, tp], [r, b], [l, b], [l, tp]], line, ink);
    for k in [1.0 / 3.0, 2.0 / 3.0] {
        let (vx, hy) = (l + (r - l) * k, tp + (b - tp) * k);
        shape = shape.line([vx, tp], [vx, b], line * 0.7, LineCap::Butt, ink.alpha(0.45)).line([l, hy], [r, hy], line * 0.7, LineCap::Butt, ink.alpha(0.45));
    }
    let arm = cw.min(ch) * 0.035;
    let thick = line * 3.0;
    for (cx, cy, sx, sy) in [(l, tp, 1.0, 1.0), (r, tp, -1.0, 1.0), (l, b, 1.0, -1.0), (r, b, -1.0, -1.0)] {
        shape = shape.stroke(vec![[cx + sx * arm, cy], [cx, cy], [cx, cy + sy * arm]], thick, ink);
    }

    slot.child(canvas(shape.view_box(cw, ch)).absolute(0.0, 0.0).w_full().h_full()).on_drag(|s: &mut State, d: &mut Drag, tx: &mut Tx| {
        tx.instant();
        let source = s.photo.size;
        match d.phase {
            Phase::Down => {
                let crop = s.edit.geometry.effective_crop(source);
                match grabbed(crop, d.x, d.y, d.w, d.h) {
                    Some(grab) => {
                        d.data = grab.encode();
                        s.crop_start = Some(crop);
                    }
                    None => s.crop_start = None,
                }
            }
            Phase::Move => {
                let Some(start) = s.crop_start else { return };
                let (geometry, ratio) = (s.edit.geometry.clone(), s.crop_ratio());
                let canvas = geometry.oriented_size(source);
                let grab = Grab::decode(d.data);
                let (dx, dy) = (d.dx / d.w.max(1.0), d.dy / d.h.max(1.0));
                let fits = |c: [f32; 4]| geometry.is_inside(source, c);
                // As far along the drag as the frame stays on the straightened image. A moving
                // frame tries each direction alone too, so it slides along an edge it meets.
                let along = |dx: f32, dy: f32| {
                    let (mut lo, mut hi) = (0.0f32, 1.0f32);
                    if fits(dragged(start, grab, dx, dy, ratio, canvas)) {
                        return Some(dragged(start, grab, dx, dy, ratio, canvas));
                    }
                    for _ in 0..16 {
                        let k = (lo + hi) / 2.0;
                        if fits(dragged(start, grab, dx * k, dy * k, ratio, canvas)) { lo = k } else { hi = k }
                    }
                    (lo > 0.0).then(|| dragged(start, grab, dx * lo, dy * lo, ratio, canvas))
                };
                let next = if grab == Grab::MOVE {
                    along(dx, 0.0).map(|c| [c[0], start[1], c[2], c[3]]).and_then(|c| {
                        let moved_x = c[0] - start[0];
                        along(moved_x, dy).filter(|both| (both[0] - c[0]).abs() < 1e-5).or(Some(c))
                    })
                } else {
                    along(dx, dy)
                };
                if let Some(next) = next {
                    s.edit.geometry.crop = Some(next);
                }
            }
            Phase::Up => {
                s.crop_start = None;
                s.save();
            }
        }
    })
}

pub fn panel(s: &State) -> El {
    let g = &s.edit.geometry;
    let action = |key: &'static str, label: &'static str, f: fn(&mut Geometry)| {
        button(key, label, move |s| {
            f(&mut s.edit.geometry);
            s.save();
        })
    };
    let aspects = segmented("aspect", &ASPECTS, s.crop_aspect, |s, a| {
        s.crop_aspect = a;
        if a.is_some() {
            // Snap the frame to the aspect, as large as fits on the straightened image.
            let canvas = s.edit.geometry.oriented_size(s.photo.size);
            let ratio = s.crop_ratio().unwrap_or(canvas[0] / canvas[1]);
            let frame = s.edit.geometry.fit_crop(s.photo.size, centred(ratio, canvas));
            s.edit.geometry.crop = Some(frame);
            s.save();
        }
    });
    let size = g.output_size(s.photo.size);
    col()
        .w_full()
        .px(12.0)
        .py(12.0)
        .gap(16.0)
        .child(col().w_full().gap(8.0).child(text("Aspect").font(t::LABEL).color(t::INK_MUTED)).child(aspects))
        .child(slider(STRAIGHTEN, g.straighten))
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(text("Rotate and flip").font(t::LABEL).color(t::INK_MUTED))
                .child(
                    row()
                        .gap(8.0)
                        .child(action("rotate-left", "Rotate left", |g| g.turn(-1)))
                        .child(action("rotate-right", "Rotate right", |g| g.turn(1))),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(action("flip-h", "Flip horizontal", |g| g.flip_horizontal = !g.flip_horizontal))
                        .child(action("flip-v", "Flip vertical", |g| g.flip_vertical = !g.flip_vertical)),
                ),
        )
        .child(text(format!("Preview {} × {} px", size[0], size[1])).font(t::VALUE_SM).color(t::INK_MUTED))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(button("reset-crop", "Reset", |s| {
                    s.edit.geometry = Geometry::default();
                    s.crop_aspect = None;
                    s.save();
                }))
                .child(spacer())
                .child(button("crop-done", "Done", |s| s.set_cropping(false))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: [f32; 2] = [3000.0, 2000.0];

    #[test]
    fn moving_keeps_the_size_and_stays_on_the_canvas() {
        let c = dragged([0.1, 0.1, 0.5, 0.5], Grab::MOVE, 0.9, -0.5, None, CANVAS);
        assert_eq!(c, [0.5, 0.0, 0.5, 0.5]);
    }

    #[test]
    fn a_corner_resizes_from_the_opposite_corner() {
        let br = Grab { left: false, top: false, right: true, bottom: true };
        assert_eq!(dragged([0.1, 0.1, 0.5, 0.5], br, 0.1, 0.2, None, CANVAS), [0.1, 0.1, 0.6, 0.7]);
        let tl = Grab { left: true, top: true, right: false, bottom: false };
        let c = dragged([0.1, 0.1, 0.5, 0.5], tl, 0.05, 0.05, None, CANVAS);
        assert!((c[0] - 0.15).abs() < 1e-6 && (c[0] + c[2] - 0.6).abs() < 1e-6, "{c:?}");
    }

    #[test]
    fn a_crop_never_shrinks_below_the_minimum() {
        let r = Grab { left: false, top: false, right: true, bottom: false };
        let c = dragged([0.1, 0.1, 0.5, 0.5], r, -0.9, 0.0, None, CANVAS);
        assert!((c[2] - MIN_SIDE).abs() < 1e-6);
    }

    #[test]
    fn a_square_aspect_holds_while_dragging_a_side() {
        let r = Grab { left: false, top: false, right: true, bottom: false };
        let [_, _, w, h] = dragged([0.1, 0.1, 0.4, 0.6], r, 0.1, 0.0, Some(1.0), CANVAS);
        assert!((w * CANVAS[0] - h * CANVAS[1]).abs() < 0.5, "{w} {h}");
    }

    #[test]
    fn presses_find_corners_edges_and_the_middle() {
        let crop = [0.25, 0.25, 0.5, 0.5];
        let corner = grabbed(crop, 100.0, 100.0, 400.0, 400.0).unwrap();
        assert!(corner.left && corner.top && !corner.right);
        let edge = grabbed(crop, 300.0, 200.0, 400.0, 400.0).unwrap();
        assert!(edge.right && !edge.top && !edge.bottom);
        assert_eq!(grabbed(crop, 200.0, 200.0, 400.0, 400.0), Some(Grab::MOVE));
        assert_eq!(grabbed(crop, 10.0, 10.0, 400.0, 400.0), None);
    }
}
