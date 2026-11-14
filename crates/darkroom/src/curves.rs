//! The Curves section's body: a channel switch, the curve graph and the selected point's values.
//!
//! On the graph: press on a point to drag it, press anywhere else to add a point there and drag
//! it, drag a point off the graph (or double-click it) to remove it. The end points stay.

use darkroom_core::{Channel, Curve, Edit};
use darkroom_render::Histogram;
use rune::{Color, Drag, El, LineCap, Phase, Shape, Tx, canvas, col, el, row, spacer, text};

pub use darkroom_core::Picker;

use crate::State;
use crate::develop::Pick;
use crate::theme as t;
use crate::ui::eyedropper_icon;

/// What a picker makes the clicked pixel, for the hint over the photo.
pub fn target(p: Picker) -> &'static str {
    match p {
        Picker::Black => "black",
        Picker::Grey => "neutral grey",
        Picker::White => "white",
    }
}

/// Sets curve points from a probed pixel (see `Curves::set_from_pixel`).
pub fn apply_picker(edit: &mut Edit, picker: Picker, rgb: [f32; 3]) {
    edit.curves.set_from_pixel(picker, rgb);
}

/// The graph's drawing area, in points, and its inset inside the graph box.
const GRAPH: f32 = 260.0;
const INSET: f32 = 8.0;
const BOX: f32 = GRAPH + 2.0 * INSET;
/// How close a press must be to a point to pick it up.
const REACH: f32 = 10.0;
/// How far past the graph's edge a dragged point has to go to be removed.
const REMOVE_BEYOND: f32 = 24.0;

pub fn body(s: &State) -> El {
    let channel = s.curve_channel;
    let curve = s.edit.curves.get(channel);
    let selected = s.curve_point.filter(|&i| i < curve.points().len());

    let readout = match selected {
        Some(i) if !s.curve_removing => {
            let [x, y] = curve.points()[i];
            format!("Input {}  ·  Output {}", (x * 255.0).round(), (y * 255.0).round())
        }
        Some(_) => "Release to remove the point".to_owned(),
        None => "Click the curve to add a point".to_owned(),
    };
    let footer = row()
        .w_full()
        .center_y()
        .child(text(readout).font(t::VALUE_SM).color(t::INK_MUTED))
        .child(spacer())
        .child(
            el().key("curve-reset")
                .role("button")
                .label("Reset curve")
                .h(24.0)
                .px(8.0)
                .center()
                .radius(7.0)
                .hover(t::BG_HOVER)
                .on_press(move |s: &mut State| {
                    *s.edit.curves.get_mut(s.curve_channel) = Curve::identity();
                    s.curve_point = None;
                    s.save();
                })
                .child(text("Reset curve").font(t::BODY).color(t::INK_MUTED)),
        );

    let pickers = row()
        .w_full()
        .center_y()
        .child(text("Set points from the photo").font(t::LABEL).color(t::INK_MUTED))
        .child(spacer())
        .child(pickers(s.picker, s.probe_for.is_some()));

    col().w_full().gap(12.0).child(channels(channel)).child(graph(s, channel, selected)).child(footer).child(pickers)
}

/// Black, grey and white point pickers: the design system's toolbar of small icon buttons, each
/// an eyedropper with a swatch of what it sets. The active one is accent-tinted.
fn pickers(active: Option<Pick>, waiting: bool) -> El {
    let mut tray = row().padding(2.0).gap(2.0).radius(9.0).fill(t::BG_RAISED);
    for (p, swatch) in [(Picker::Black, t::BG_CANVAS), (Picker::Grey, t::LINE_STRONG), (Picker::White, t::INK)] {
        let on = active == Some(Pick::Point(p));
        let ink = if on { t::ACCENT_INK } else if waiting { t::INK_FAINT } else { t::INK_MUTED };
        tray = tray.child(
            el().key(format!("picker-{}", target(p)))
                .role("button")
                .label(format!("Pick {}", p.name()))
                .checked(on)
                .size(24.0, 24.0)
                .radius(7.0)
                .fill(if on { t::ACCENT_SOFT } else { Color::TRANSPARENT })
                .hover(if on { t::ACCENT_SOFT } else { t::BG_HOVER })
                .on_press(move |s: &mut State| s.toggle_picker(Pick::Point(p)))
                .child(canvas(eyedropper_icon(ink)).absolute(4.0, 4.0).size(16.0, 16.0))
                .child(el().absolute(4.0, 15.0).size(6.0, 6.0).radius(3.0).fill(swatch).border(1.0, t::LINE_STRONG)),
        );
    }
    tray
}

/// The histogram at the top of the panel: red, green and blue over luminance, on the canvas
/// ground, as the design system's Histogram, with the photo's exposure line (`exif`, empty
/// when the file has none) under it.
pub fn histogram_panel(h: &Histogram, exif: &str) -> El {
    const W: f32 = 260.0;
    const H: f32 = 80.0;
    let mut shape = Shape::new().fill(area(h, Channel::Rgb, W, H, 0.92), t::CHAN_LUMA.alpha(0.35));
    for c in [Channel::Blue, Channel::Green, Channel::Red] {
        shape = shape.fill(area(h, c, W, H, 0.92), tint(c).alpha(0.45));
    }
    let line = (!exif.is_empty()).then(|| text(exif).key("exif").font(t::VALUE_SM).color(t::INK_MUTED));
    col().key("histogram")
        .role("image")
        .label("Histogram")
        .w_full()
        .padding(8.0)
        .gap(6.0)
        .items_center()
        .radius(7.0)
        .fill(t::BG_CANVAS)
        .child(canvas(shape.view_box(W, H)).size(W, H))
        .children(line)
}

/// RGB, Red, Green, Blue: the design system's SegmentedControl.
fn channels(current: Channel) -> El {
    let mut seg = row().self_start().padding(2.0).gap(2.0).radius(9.0).fill(t::BG_RAISED);
    for c in Channel::ALL {
        let on = c == current;
        seg = seg.child(
            el().key(format!("curve-{}", c.name()))
                .role("radio")
                .label(c.name())
                .checked(on)
                .h(24.0)
                .px(12.0)
                .center()
                .radius(7.0)
                .fill(if on { t::BG_ACTIVE } else { Color::TRANSPARENT })
                .on_press(move |s: &mut State| {
                    s.curve_channel = c;
                    s.curve_point = None;
                })
                .child(text(c.name()).font(if on { t::BODY_STRONG } else { t::BODY }).color(if on { t::INK } else { t::INK_MUTED })),
        );
    }
    seg
}

fn tint(c: Channel) -> Color {
    match c {
        Channel::Rgb => t::CHAN_LUMA,
        Channel::Red => t::CHAN_RED,
        Channel::Green => t::CHAN_GREEN,
        Channel::Blue => t::CHAN_BLUE,
    }
}

/// A curve point in graph coordinates (y down).
fn to_graph(p: [f32; 2]) -> [f32; 2] {
    [p[0] * GRAPH, (1.0 - p[1]) * GRAPH]
}

/// The curve sampled along its whole width.
fn polyline(c: &Curve) -> Vec<[f32; 2]> {
    let lut = c.lut(129);
    lut.iter().enumerate().map(|(i, &y)| to_graph([i as f32 / 128.0, y])).collect()
}

fn graph(s: &State, channel: Channel, selected: Option<usize>) -> El {
    let curve = s.edit.curves.get(channel);
    let mut shape = Shape::new();

    for k in 1..4 {
        let v = k as f32 * GRAPH / 4.0;
        shape = shape.line([v, 0.0], [v, GRAPH], 1.0, LineCap::Butt, t::LINE).line([0.0, v], [GRAPH, v], 1.0, LineCap::Butt, t::LINE);
    }
    shape = shape.fill(area(&s.histogram, channel, GRAPH, GRAPH, 0.42), tint(channel).alpha(0.18));
    shape = shape.line([0.0, GRAPH], [GRAPH, 0.0], 1.0, LineCap::Butt, t::LINE_STRONG.alpha(0.5));
    // On the composite, the edited channel curves show faintly behind it.
    if channel == Channel::Rgb {
        for c in [Channel::Red, Channel::Green, Channel::Blue] {
            let other = s.edit.curves.get(c);
            if !other.is_identity() {
                shape = shape.stroke(polyline(other), 1.25, tint(c).alpha(0.55));
            }
        }
    }
    shape = shape.stroke(polyline(curve), 1.5, tint(channel));

    let mut graph = el()
        .key("curve-graph")
        .role("group")
        .label(format!("{} curve", channel.name()))
        .size(BOX, BOX)
        .radius(7.0)
        .fill(t::BG_CANVAS)
        .child(canvas(shape.view_box(GRAPH, GRAPH)).absolute(INSET, INSET).size(GRAPH, GRAPH));

    for (i, p) in curve.points().iter().enumerate() {
        let [x, y] = to_graph(*p);
        let picked = selected == Some(i);
        if picked && s.curve_removing {
            continue;
        }
        let r = if picked { 5.0 } else { 4.0 };
        let dot = el().absolute(INSET + x - r, INSET + y - r).size(2.0 * r, 2.0 * r).radius(r);
        graph = graph.child(if picked { dot.fill(t::ACCENT) } else { dot.fill(t::BG_CANVAS).border(1.5, t::INK) });
    }

    graph
        .on_drag(|s: &mut State, g: &mut Drag, tx: &mut Tx| {
            tx.instant();
            let ch = s.curve_channel;
            match g.phase {
                Phase::Down => {
                    let p = to_curve(g.x, g.y);
                    let curve = s.edit.curves.get_mut(ch);
                    let i = curve.nearest(p, REACH / GRAPH).unwrap_or_else(|| curve.insert(p));
                    s.curve_point = Some(i);
                    s.curve_removing = false;
                }
                Phase::Move => {
                    let Some(i) = s.curve_point else { return };
                    let (x, y) = (g.start_x + g.dx, g.start_y + g.dy);
                    let curve = s.edit.curves.get_mut(ch);
                    let inner = i > 0 && i + 1 < curve.points().len();
                    let outside = |v: f32| !(-REMOVE_BEYOND..=BOX + REMOVE_BEYOND).contains(&v);
                    s.curve_removing = inner && (outside(x) || outside(y));
                    if !s.curve_removing {
                        curve.move_point(i, to_curve(x, y));
                    }
                }
                Phase::Up => {
                    if s.curve_removing
                        && let Some(i) = s.curve_point
                    {
                        s.edit.curves.get_mut(ch).remove(i);
                        s.curve_point = None;
                    }
                    s.curve_removing = false;
                    s.save();
                }
            }
        })
        .on_double_press(|s: &mut State| {
            let ch = s.curve_channel;
            if let Some(i) = s.curve_point
                && s.edit.curves.get_mut(ch).remove(i)
            {
                s.curve_point = None;
                s.save();
            }
        })
}

/// A position in the graph box (points) to curve coordinates, clamped to 0..1.
fn to_curve(x: f32, y: f32) -> [f32; 2] {
    [((x - INSET) / GRAPH).clamp(0.0, 1.0), (1.0 - (y - INSET) / GRAPH).clamp(0.0, 1.0)]
}

/// A channel's histogram as a closed area along the bottom of a `w` × `h` box, at most `reach`
/// of its height. Scaled to the tallest bin that isn't a clipped end (pure black or white would
/// flatten everything else). `Channel::Rgb` is luminance.
fn area(hist: &Histogram, channel: Channel, w: f32, h: f32, reach: f32) -> Vec<[f32; 2]> {
    let bins = &hist[Channel::ALL.iter().position(|&c| c == channel).unwrap_or(0)];
    // Pairs of bins, so the outline stays smooth at this size.
    let pairs: Vec<f32> = bins.chunks(2).map(|b| (b[0] + b[1]) as f32).collect();
    let peak = pairs[1..pairs.len() - 1].iter().copied().fold(1.0, f32::max);
    let mut pts = vec![[0.0, h]];
    for (i, &v) in pairs.iter().enumerate() {
        let x = (i as f32 + 0.5) / pairs.len() as f32 * w;
        pts.push([x, h - (v / peak).min(1.0) * h * reach]);
    }
    pts.push([w, h]);
    pts
}
