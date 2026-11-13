//! The Develop module: the photo, the adjustments panel, and the filmstrip of the current
//! folder.

use darkroom_core::Edit;
use rune::{Cursor, Drag, El, Phase, ScrollSpec, SurfaceFit, Tx, col, effect, el, row, spacer, text};

use crate::theme as t;
use crate::ui::{button, rule, segmented, stars, top_bar};
use crate::{PHOTO_SLOT, State, crop, curves};

/// Slider track width: the 300 pt panel less its 12 pt padding on each side.
const TRACK: f32 = 276.0;
/// Filmstrip cell size, in points.
const STRIP_CELL: f32 = 112.0;

#[derive(Clone, Copy)]
pub enum Readout {
    /// +0.35
    Stops,
    /// −18, +12
    Whole,
    /// −1.8°
    Degrees,
}

/// One slider: its range, how its value reads, and where it lives in the edit.
#[derive(Clone, Copy)]
pub struct Adjustment {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub readout: Readout,
    pub get: fn(&Edit) -> f32,
    pub set: fn(&mut Edit, f32),
}

pub const fn adj(key: &'static str, label: &'static str, get: fn(&Edit) -> f32, set: fn(&mut Edit, f32)) -> Adjustment {
    Adjustment { key, label, min: -100.0, max: 100.0, readout: Readout::Whole, get, set }
}

const LIGHT: [Adjustment; 7] = [
    Adjustment { min: -3.0, max: 3.0, readout: Readout::Stops, ..adj("exposure", "Exposure", |e| e.light.exposure, |e, v| e.light.exposure = v) },
    adj("brilliance", "Brilliance", |e| e.light.brilliance, |e, v| e.light.brilliance = v),
    adj("highlights", "Highlights", |e| e.light.highlights, |e, v| e.light.highlights = v),
    adj("shadows", "Shadows", |e| e.light.shadows, |e, v| e.light.shadows = v),
    adj("brightness", "Brightness", |e| e.light.brightness, |e, v| e.light.brightness = v),
    adj("contrast", "Contrast", |e| e.light.contrast, |e, v| e.light.contrast = v),
    adj("black-point", "Black point", |e| e.light.black_point, |e, v| e.light.black_point = v),
];

const COLOR: [Adjustment; 4] = [
    adj("saturation", "Saturation", |e| e.color.saturation, |e, v| e.color.saturation = v),
    adj("vibrance", "Vibrance", |e| e.color.vibrance, |e, v| e.color.vibrance = v),
    adj("temperature", "Temperature", |e| e.color.temperature, |e, v| e.color.temperature = v),
    adj("tint", "Tint", |e| e.color.tint, |e, v| e.color.tint = v),
];

const DETAIL: [Adjustment; 4] = [
    adj("definition", "Definition", |e| e.detail.definition, |e, v| e.detail.definition = v),
    Adjustment { min: 0.0, ..adj("sharpening", "Sharpening", |e| e.detail.sharpening, |e, v| e.detail.sharpening = v) },
    Adjustment { min: 0.0, ..adj("noise-reduction", "Noise reduction", |e| e.detail.noise_reduction, |e, v| e.detail.noise_reduction = v) },
    Adjustment { min: 0.0, ..adj("vignette", "Vignette", |e| e.detail.vignette, |e, v| e.detail.vignette = v) },
];

pub fn view(s: &State) -> El {
    let (shown, _) = s.view_geometry();
    let mut photo = effect(PHOTO_SLOT).key("photo").aspect_fit(shown[0] / shown[1]);
    if s.cropping {
        photo = crop::overlay(s, photo);
    } else if s.picker.is_some() {
        photo = photo.cursor(Cursor::Crosshair).on_drag(|s: &mut State, g: &mut Drag, _tx: &mut Tx| {
            if g.phase == Phase::Down {
                pick(s, g);
            }
        });
    }
    let hint = s.picker.map(|p| {
        text(format!("Click something that should be {}. Click the picker again to cancel.", curves::target(p)))
            .font(t::LABEL)
            .color(t::INK_MUTED)
    });
    let opening = s.opening.as_ref().map(|_| text("Opening…").font(t::LABEL).color(t::INK_MUTED));
    let stage = col().w_full().grow(1.0).min_h(0.0).padding(24.0).gap(8.0).items_center().children(hint).children(opening).child(photo);
    let left = col().grow(1.0).shrink().min_w(0.0).h_full().child(stage).children(filmstrip(s));

    let e = &s.edit;
    let sections = col().key("sections").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false });
    let sections = if s.cropping {
        sections.child(crop::panel(s))
    } else {
        sections
            .child(section("light", "Light", e.light.enabled, |e| e.light.enabled = !e.light.enabled, sliders(&LIGHT, e)))
            .child(section("color", "Color", e.color.enabled, |e| e.color.enabled = !e.color.enabled, sliders(&COLOR, e)))
            .child(section("curves", "Curves", e.curves.enabled, |e| e.curves.enabled = !e.curves.enabled, curves::body(s)))
            .child(section("detail", "Detail", e.detail.enabled, |e| e.detail.enabled = !e.detail.enabled, sliders(&DETAIL, e)))
    };

    let tools = segmented("tool", &[(false, "Adjust"), (true, "Crop")], s.cropping, |s, on| s.set_cropping(on));
    let header = row()
        .w_full()
        .padding(12.0)
        .gap(8.0)
        .center_y()
        .child(tools)
        .child(col().gap(2.0).shrink().child(text(s.status.clone()).font(t::CAPTION).color(t::INK_MUTED).truncate_fade()))
        .child(spacer())
        .child(stars(e.rating, 12.0));

    let footer = row()
        .w_full()
        .padding(12.0)
        .gap(8.0)
        .child(button("original", if s.show_original { "Show edit" } else { "Show original" }, |s| s.show_original = !s.show_original))
        .child(spacer())
        .child(button("reset", "Reset all", |s| {
            s.edit = s.edit.without_adjustments();
            s.save();
        }));

    let histogram = col().w_full().px(12.0).pb(12.0).child(curves::histogram_panel(&s.histogram));
    let panel = col()
        .key("panel")
        .w(300.0)
        .h_full()
        .min_h(0.0)
        .fill(t::BG_PANEL)
        .child(header)
        .child(histogram)
        .child(rule())
        .child(sections)
        .child(rule())
        .child(footer);

    let body = row().w_full().grow(1.0).min_h(0.0).child(left).child(panel);
    col().w_full().h_full().fill(t::BG_CANVAS).child(top_bar(s, s.photo.name.clone(), spacer())).child(rule()).child(body)
}

/// The current folder's photos along the bottom, the open one framed. Click one to open it.
fn filmstrip(s: &State) -> Option<El> {
    let visible = s.visible();
    if visible.is_empty() {
        return None;
    }
    let mut strip = row().gap(8.0).px(12.0).py(8.0).center_y();
    for i in visible {
        let entry = &s.entries[i];
        let current = s.path.as_ref() == Some(&entry.path);
        let path = entry.path.clone();
        let mut image = el().w(STRIP_CELL - 8.0).h(68.0).radius(4.0).fill(t::BG_PANEL);
        if let Some(img) = s.thumbs.get(&entry.path) {
            image = image.pixels_fit(img.pixels(), SurfaceFit::Fit);
        }
        let mut cell = col()
            .key(format!("strip-{}", entry.name))
            .role("button")
            .label(entry.name.clone())
            .w(STRIP_CELL)
            .padding(4.0)
            .gap(4.0)
            .radius(6.0)
            .hover(t::BG_PANEL)
            .on_press(move |s: &mut State| s.open(path.clone()))
            .child(image)
            .child(row().w_full().child(text(entry.name.clone()).font(t::CAPTION).color(t::INK_MUTED).truncate_fade().shrink()).child(spacer()).child(stars(entry.rating, 8.0)));
        if current {
            cell = cell.border(2.0, t::ACCENT);
        }
        strip = strip.child(cell);
    }
    Some(col().w_full().h(112.0).child(rule()).child(el().w_full().grow(1.0).scroll_x().child(strip)))
}

/// A click on the photo while a picker is active: probe the pixel under it. The slot is the
/// output exactly, so the click maps through the geometry to the source.
fn pick(s: &mut State, g: &Drag) {
    let Some(picker) = s.picker else { return };
    let size = s.photo.size;
    let (_, map) = s.view_geometry();
    let [u, v] = darkroom_core::geometry::apply(map, [g.x / g.w.max(1.0), g.y / g.h.max(1.0)]);
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return;
    }
    let pixel = [(u * size[0] as f32) as u32, (v * size[1] as f32) as u32];
    let id = s.next_probe;
    s.next_probe += 1;
    s.probe_request = Some((id, pixel));
    s.probe_for = Some((id, picker));
    s.picker = None;
}

/// A panel section: a heading with an eye toggle that hides its edits, then its controls.
fn section(key: &'static str, title: &'static str, enabled: bool, toggle: fn(&mut Edit), content: El) -> El {
    let head = row()
        .w_full()
        .h(36.0)
        .px(12.0)
        .center_y()
        .child(text(title).font(t::HEADING).color(t::INK))
        .child(spacer())
        .child(
            el().key(format!("{key}-toggle"))
                .role("button")
                .label(if enabled { format!("Hide {title} edits") } else { format!("Show {title} edits") })
                .h(24.0)
                .px(8.0)
                .center()
                .radius(7.0)
                .hover(t::BG_HOVER)
                .on_press(move |s: &mut State| {
                    toggle(&mut s.edit);
                    s.save();
                })
                .child(text(if enabled { "Hide" } else { "Show" }).font(t::CAPTION).color(t::INK_MUTED)),
        );
    let mut body = col().w_full().px(12.0).pb(16.0).child(content);
    if !enabled {
        body = body.dim(0.45);
    }
    col().key(key).w_full().child(head).child(body).child(rule())
}

fn sliders(adjustments: &'static [Adjustment], e: &Edit) -> El {
    adjustments.iter().fold(col().w_full().gap(12.0), |c, a| c.child(slider(*a, (a.get)(e))))
}

fn readout(r: Readout, v: f32) -> String {
    let s = match r {
        Readout::Stops if v.abs() < 0.005 => "0.00".to_owned(),
        Readout::Stops => format!("{v:+.2}"),
        Readout::Whole if v.round() == 0.0 => "0".to_owned(),
        Readout::Whole => format!("{:+.0}", v.round()),
        Readout::Degrees if v.abs() < 0.05 => "0°".to_owned(),
        Readout::Degrees => format!("{v:+.1}°"),
    };
    // A real minus sign, as the design system asks.
    s.replace('-', "−")
}

/// The design system's AdjustSlider: label and value over a track whose accent fill runs from
/// zero (centre for bipolar sliders, left for unipolar) to the thumb. Drag or click to set,
/// double-click to reset; the edit saves when the pointer lifts.
pub fn slider(a: Adjustment, value: f32) -> El {
    let span = a.max - a.min;
    let frac = ((value - a.min) / span).clamp(0.0, 1.0);
    let zero = ((0.0 - a.min) / span).clamp(0.0, 1.0);
    let (lo, hi) = (frac.min(zero), frac.max(zero));
    let changed = value.abs() > 1e-4;
    let header = row()
        .w_full()
        .child(text(a.label).font(t::LABEL).color(if changed { t::INK } else { t::INK_MUTED }))
        .child(spacer())
        .child(text(readout(a.readout, value)).font(t::VALUE).color(t::INK));
    let mut track = el()
        .key(a.key)
        .role("slider")
        .label(a.label)
        .w(TRACK)
        .h(16.0)
        .child(el().absolute(0.0, 6.0).w(TRACK).h(4.0).radius(2.0).fill(t::BG_ACTIVE))
        .child(el().absolute(lo * TRACK, 6.0).w((hi - lo) * TRACK).h(4.0).radius(2.0).fill(t::ACCENT));
    if zero > 0.0 {
        track = track.child(el().absolute(zero * TRACK - 0.5, 3.0).w(1.0).h(10.0).fill(t::LINE_STRONG));
    }
    let (min, max, set) = (a.min, a.max, a.set);
    let track = track
        .child(el().absolute(frac * TRACK - 6.0, 2.0).size(12.0, 12.0).radius(6.0).fill(t::INK))
        .on_drag(move |s: &mut State, g: &mut Drag, tx: &mut Tx| {
            tx.instant();
            match g.phase {
                Phase::Down => {
                    let v = min + (g.x / g.w.max(1.0)).clamp(0.0, 1.0) * span;
                    g.data = v;
                    set(&mut s.edit, v);
                }
                Phase::Move => set(&mut s.edit, (g.data + g.dx / g.w.max(1.0) * span).clamp(min, max)),
                Phase::Up => s.save(),
            }
        })
        .on_double_press(move |s: &mut State| {
            set(&mut s.edit, 0.0);
            s.save();
        });
    col().w_full().gap(6.0).child(header).child(track)
}
