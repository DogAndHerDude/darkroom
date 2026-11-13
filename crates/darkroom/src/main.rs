//! Darkroom: a RUNE window around one photo, developed on RUNE's own GPU device, with the v1
//! Light, Color and vignette adjustments. Edits load from and save to the photo's edit file.
//!
//! `cargo run -p darkroom -- path/to/photo.jpg` (no path: a generated test chart, not saved).

mod curves;
mod photo;
mod theme;

use std::path::PathBuf;
use std::sync::Arc;

use darkroom_core::{Channel, Curve, Edit, sidecar};
use darkroom_render::{Develop, Histogram, Placement, Results};
use rune::compose::DeviceKey;
use rune::{AppSpec, Compositor, Cursor, Cx, Drag, El, Frame, Phase, ScrollSpec, Tx, Waker, col, effect, el, row, spacer, text};

use crate::curves::Picker;

use crate::photo::Photo;
use crate::theme as t;

/// The effect slot the photo is drawn into.
const PHOTO_SLOT: u32 = 1;
/// Slider track width: the 300 pt panel less its 12 pt padding on each side.
const TRACK: f32 = 276.0;

struct State {
    photo: Arc<Photo>,
    /// Where the photo came from; None for the test chart, which is never saved.
    path: Option<PathBuf>,
    edit: Edit,
    show_original: bool,
    /// The last save's outcome, shown under the file name.
    status: String,
    /// The curve the Curves section shows.
    curve_channel: Channel,
    /// The selected point on that curve.
    curve_point: Option<usize>,
    /// The selected point is being dragged off the graph and goes on release.
    curve_removing: bool,
    /// The active curve point picker: the next click on the photo sets points from that pixel.
    picker: Option<Picker>,
    /// A pixel the compositor should probe: (request id, photo pixel).
    probe_request: Option<(u64, [u32; 2])>,
    /// The picker the outstanding probe is for, by request id.
    probe_for: Option<(u64, Picker)>,
    next_probe: u64,
    /// The edited photo's histogram, live from the GPU (the original's until the first arrives).
    histogram: Histogram,
    /// Where the GPU leaves results for `poll` to pick up.
    results: Arc<Results>,
    /// Wakes the window when results arrive; set in `setup`.
    waker: Option<Waker>,
}

impl State {
    /// The edit the preview shows.
    fn shown(&self) -> Edit {
        if self.show_original { self.edit.without_adjustments() } else { self.edit.clone() }
    }

    /// Saves the edit beside the photo.
    fn save(&mut self) {
        let Some(path) = &self.path else { return };
        self.status = match sidecar::save(path, &self.edit) {
            Ok(()) if self.edit.has_adjustments() => "Edited · saved".into(),
            Ok(()) => "Not edited".into(),
            Err(e) => format!("Couldn't save: {e}"),
        };
    }
}

fn init() -> State {
    let arg = rune::tooling::extra_args().first().cloned();
    let (photo, path) = match &arg {
        Some(p) => match Photo::open(p) {
            Ok(photo) => (photo, Some(PathBuf::from(p))),
            Err(e) => {
                eprintln!("couldn't open {p}: {e}; showing the test chart");
                (Photo::test_chart(), None)
            }
        },
        None => (Photo::test_chart(), None),
    };
    let (edit, status) = match &path {
        Some(p) => match sidecar::load(p) {
            Ok(e) => {
                let status = if e.has_adjustments() { "Edited" } else { "Not edited" };
                (e, status.to_owned())
            }
            // A broken edit file is left alone; editing would overwrite it, so say so.
            Err(e) => (Edit::default(), format!("Edit file unreadable, starting fresh: {e}")),
        },
        None => (Edit::default(), "Test chart · edits aren't saved".to_owned()),
    };
    let histogram = photo.histogram;
    State {
        photo: Arc::new(photo),
        path,
        edit,
        show_original: false,
        status,
        curve_channel: Channel::Rgb,
        curve_point: None,
        curve_removing: false,
        picker: None,
        probe_request: None,
        probe_for: None,
        next_probe: 1,
        histogram,
        results: Arc::new(Results::default()),
        waker: None,
    }
}

/// Picks up what the GPU sent back since the last frame. True when the view must rebuild.
fn poll(s: &mut State, _t: f64, _tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(h) = s.results.take_histogram() {
        s.histogram = h;
        changed = true;
    }
    if let Some(probed) = s.results.take_probe()
        && let Some((id, picker)) = s.probe_for
        && id == probed.id
    {
        curves::apply_picker(&mut s.edit, picker, probed.rgb);
        s.probe_for = None;
        s.curve_point = None;
        s.save();
        changed = true;
    }
    changed
}

/// A click on the photo while a picker is active: probe the pixel under it.
fn pick(s: &mut State, g: &Drag) {
    let Some(picker) = s.picker else { return };
    let size = s.photo.size;
    let [fx, fy, fw, fh] = darkroom_render::fit([0.0, 0.0, g.w, g.h], size);
    let (u, v) = ((g.x - fx) / fw, (g.y - fy) / fh);
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

// ---- adjustments ------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Readout {
    /// +0.35
    Stops,
    /// −18, +12
    Whole,
}

/// One slider: its range, how its value reads, and where it lives in the edit.
#[derive(Clone, Copy)]
struct Adjustment {
    key: &'static str,
    label: &'static str,
    min: f32,
    max: f32,
    readout: Readout,
    get: fn(&Edit) -> f32,
    set: fn(&mut Edit, f32),
}

const fn adj(key: &'static str, label: &'static str, get: fn(&Edit) -> f32, set: fn(&mut Edit, f32)) -> Adjustment {
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

const DETAIL: [Adjustment; 1] = [Adjustment { min: 0.0, ..adj("vignette", "Vignette", |e| e.detail.vignette, |e, v| e.detail.vignette = v) }];

// ---- view ---------------------------------------------------------------------------------------

fn view(s: &State, _: &mut Cx<State>) -> El {
    let mut photo = effect(PHOTO_SLOT).key("photo").w_full().grow(1.0);
    if s.picker.is_some() {
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
    let stage = col().grow(1.0).shrink().min_w(0.0).h_full().padding(24.0).gap(8.0).items_center().children(hint).child(photo);

    let e = &s.edit;
    let sections = col()
        .key("sections")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(section("light", "Light", e.light.enabled, |e| e.light.enabled = !e.light.enabled, sliders(&LIGHT, e)))
        .child(section("color", "Color", e.color.enabled, |e| e.color.enabled = !e.color.enabled, sliders(&COLOR, e)))
        .child(section("curves", "Curves", e.curves.enabled, |e| e.curves.enabled = !e.curves.enabled, curves::body(s)))
        .child(section("detail", "Detail", e.detail.enabled, |e| e.detail.enabled = !e.detail.enabled, sliders(&DETAIL, e)));

    let header = col()
        .w_full()
        .padding(12.0)
        .gap(2.0)
        .child(text(s.photo.name.clone()).font(t::TITLE).color(t::INK))
        .child(text(s.status.clone()).font(t::CAPTION).color(t::INK_MUTED));

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

    row().w_full().h_full().fill(t::BG_CANVAS).child(stage).child(panel)
}

fn rule() -> El {
    el().w_full().h(1.0).fill(t::LINE)
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
    };
    // A real minus sign, as the design system asks.
    s.replace('-', "−")
}

/// The design system's AdjustSlider: label and value over a track whose accent fill runs from
/// zero (centre for bipolar sliders, left for unipolar) to the thumb. Drag or click to set,
/// double-click to reset; the edit saves when the pointer lifts.
fn slider(a: Adjustment, value: f32) -> El {
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

fn button(key: &'static str, label: &'static str, press: fn(&mut State)) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .h(28.0)
        .px(12.0)
        .center()
        .radius(7.0)
        .fill(t::BG_RAISED)
        .hover(t::BG_HOVER)
        .on_press(press)
        .child(text(label).font(t::BODY).color(t::INK))
}

// ---- drawing ----------------------------------------------------------------------------------

/// Draws RUNE's frame, running the develop pass first when the edit changed and painting the
/// developed photo at the photo slot. Everything stays on RUNE's device.
#[derive(Default)]
struct Preview {
    gpu: Option<(DeviceKey, Develop)>,
}

impl Compositor<State> for Preview {
    fn render(&mut self, f: &mut Frame<'_, State>) {
        let key = f.device_key();
        if self.gpu.as_ref().map(|(k, _)| *k) != Some(key) {
            let s = &f.rt.state;
            let wake = s.waker.clone().unwrap_or_else(|| Arc::new(|| {}));
            self.gpu = Some((key, Develop::new(f.device, s.results.clone(), wake)));
        }
        let develop = &mut self.gpu.as_mut().expect("made above").1;

        let s = &mut f.rt.state;
        if develop.photo_id() != Some(s.photo.id) {
            develop.set_photo(f.device, f.queue, s.photo.id, s.photo.size, &s.photo.rgba);
        }
        if let Some((id, pixel)) = s.probe_request.take() {
            develop.probe(id, pixel);
        }
        develop.update(f.device, f.queue, &s.shown());

        let (device, queue, clear) = (f.device, f.queue, f.rt.clear_color());
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.mark != PHOTO_SLOT {
                return;
            }
            develop.prepare_format(device, site.format);
            let [x, y, w, h] = site.instance.rect;
            let rect = [x - site.origin[0], y - site.origin[1], w, h];
            develop.draw(queue, pass, Placement { rect, target: site.size, format: site.format, encode_srgb: site.srgb_blend });
        });
    }
}

fn main() {
    let spec = AppSpec {
        min_size: Some((900.0, 600.0)),
        setup: Some(|s: &mut State, waker: Waker| s.waker = Some(waker)),
        poll: Some(poll),
        // For headless snapshots: `-- --snap out.png --scene <name>`.
        scenes: &[
            ("edited", |s: &mut State| {
                let l = &mut s.edit.light;
                (l.exposure, l.brilliance, l.highlights, l.shadows, l.contrast, l.black_point) = (0.35, 22.0, -42.0, 18.0, 12.0, 8.0);
                (s.edit.color.saturation, s.edit.color.vibrance, s.edit.color.temperature) = (10.0, 24.0, 6.0);
            }),
            ("curves", |s: &mut State| {
                s.edit.curves.rgb = Curve::new(vec![[0.0, 0.04], [0.25, 0.21], [0.5, 0.5], [0.75, 0.79], [1.0, 0.97]]);
                s.edit.curves.red = Curve::new(vec![[0.0, 0.0], [0.5, 0.54], [1.0, 1.0]]);
                s.curve_point = Some(1);
            }),
            ("curve-red", |s: &mut State| {
                s.edit.curves.red = Curve::new(vec![[0.0, 0.0], [0.5, 0.54], [1.0, 1.0]]);
                s.curve_channel = Channel::Red;
            }),
            ("extreme", |s: &mut State| {
                (s.edit.light.exposure, s.edit.color.saturation, s.edit.detail.vignette) = (1.5, 100.0, 60.0);
            }),
            // A strong warm cast, for checking the grey point picker removes it.
            ("warm", |s: &mut State| {
                s.edit.color.temperature = 70.0;
            }),
            ("hidden", |s: &mut State| {
                s.edit.light.exposure = 1.0;
                s.edit.light.enabled = false;
            }),
        ],
        ..AppSpec::new("Darkroom", (1280.0, 800.0), init, view)
    };
    rune::tooling::run_with(spec, Preview::default());
}
