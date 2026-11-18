//! The Develop module: the photo, the adjustments panel, and the filmstrip of the current
//! folder.

use std::path::{Path, PathBuf};

use darkroom_core::white::AsShot;
use darkroom_core::{Edit, History, geometry, sidecar};
use darkroom_render::{fit_aspect, neutral_point, srgb_to_rec2020};
use rune::{Cursor, Drag, El, Key, KeyEvent, Phase, PointerEvent, ScrollSpec, SurfaceFit, TextEdit, Tx, WheelEvent, canvas, col, effect, el, row, spacer, text, text_input};

use crate::curves::Picker;
use crate::theme as t;
use crate::ui::{alert_icon, before_after_icon, button, divider, eye_icon, eyedropper_icon, icon_button, icon_toggle, redo_icon, rule, segmented, stars, top_bar, undo_icon, warning_icon};
use crate::library::{Note, first_line};
use crate::{Module, PHOTO_SLOT, State, crop, curves, presets};

/// Slider track width: the 300 pt panel less its 12 pt padding on each side.
const TRACK: f32 = 276.0;
/// Filmstrip cell size, in points.
const STRIP_CELL: f32 = 112.0;

/// How the photo fills its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    /// The whole photo.
    Fit,
    /// One pixel of the photo per pixel of the screen.
    Full,
}

/// What the next click on the photo sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// Curve points, from the clicked pixel.
    Point(Picker),
    /// The white balance that makes the clicked pixel neutral.
    Neutral,
}

impl Pick {
    /// What the clicked pixel should be, for the hint over the photo.
    fn target(self) -> &'static str {
        match self {
            Pick::Point(p) => curves::target(p),
            Pick::Neutral => "neutral grey",
        }
    }

    /// The heading of the readout beside the pointer.
    fn title(self) -> &'static str {
        match self {
            Pick::Point(Picker::Black) => "Black point",
            Pick::Point(Picker::Grey) => "Grey point",
            Pick::Point(Picker::White) => "White point",
            Pick::Neutral => "Neutral point",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Readout {
    /// +0.35
    Stops,
    /// −18, +12
    Whole,
    /// −1.8°
    Degrees,
    /// 5600 K: a raw file's Temperature, from the kelvin it was shot at.
    Kelvin(f32),
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

impl State {
    /// Whether the photo shows at 100%. The crop tool always shows the whole photo.
    pub fn zoomed(&self) -> bool {
        self.zoom == Zoom::Full && !self.cropping
    }

    /// The split view shows; the crop tool, which shows the whole photo, hides it.
    pub fn splitting(&self) -> bool {
        self.split && !self.cropping
    }

    pub fn set_zoom(&mut self, zoom: Zoom) {
        self.zoom = zoom;
        if zoom == Zoom::Full {
            self.set_cropping(false);
            self.ensure_full();
        }
    }

    /// Starts decoding the open photo at full size, if 100% needs it and it isn't here or on its
    /// way. A photo no larger than its preview is at full size already.
    pub fn ensure_full(&mut self) {
        let id = self.photo.id;
        let have = self.full.as_ref().is_some_and(|f| f.photo == id) || self.full_loading == Some(id);
        if self.zoom != Zoom::Full || have || self.photo.full_size == self.photo.size {
            return;
        }
        let Some(path) = self.path.clone() else { return };
        self.full_loading = Some(id);
        self.jobs.full(id, path);
    }

    /// The output's size at the photo's own resolution, which 100% shows pixel for pixel.
    pub fn full_output(&self) -> [u32; 2] {
        self.shown().geometry.output_size(self.photo.full_size)
    }

    /// Moves the 100% view by `by` points of content from where its centre was at `from`.
    fn pan(&mut self, from: [f32; 2], by: [f32; 2]) {
        let out = self.full_output();
        // Screen pixels per point, from the slot's two sizes.
        let k = if self.slot_pts[0] > 0.0 && self.slot_px[0] > 0.0 { self.slot_px[0] / self.slot_pts[0] } else { 1.0 };
        self.centre = [from[0] - by[0] * k / out[0] as f32, from[1] - by[1] * k / out[1] as f32];
        // Keep the centre where the view can be, so a drag back from an edge moves at once.
        let [x, y, w, h] = geometry::window(out, self.slot_px, self.centre);
        self.centre = [x + w / 2.0, y + h / 2.0];
    }

    /// Where in a slot of `view` screen pixels the photo is drawn, `shown` being its size from
    /// `view_geometry`: fitted, or at 100% pixel for pixel, and centred either way.
    pub fn placed(&self, view: [f32; 2], shown: [f32; 2]) -> [f32; 4] {
        if !self.zoomed() {
            return fit_aspect([0.0, 0.0, view[0], view[1]], shown);
        }
        let [w, h] = shown.map(f32::round);
        [((view[0] - w) / 2.0).round(), ((view[1] - h) / 2.0).round(), w, h]
    }

    /// The photo pixel under a point of the photo slot (points from its top-left), if the
    /// photo is there.
    fn pixel_at(&self, p: [f32; 2]) -> Option<[u32; 2]> {
        let (pts, px) = (self.slot_pts, self.slot_px);
        if pts[0].min(pts[1]).min(px[0]).min(px[1]) <= 0.0 {
            return None;
        }
        let (shown, map) = self.view_geometry();
        let [x, y, w, h] = self.placed(px, shown);
        let inside = |v: f32| (0.0..1.0).contains(&v);
        let (u, v) = ((p[0] / pts[0] * px[0] - x) / w.max(1.0), (p[1] / pts[1] * px[1] - y) / h.max(1.0));
        let [x, y] = geometry::apply(map, [u, v]);
        if !(inside(u) && inside(v) && inside(x) && inside(y)) {
            return None;
        }
        let size = self.photo.size;
        Some([(x * size[0] as f32) as u32, (y * size[1] as f32) as u32])
    }

    /// Turns a picker on, or off if it is the active one.
    pub fn toggle_picker(&mut self, pick: Pick) {
        self.picker = if self.picker == Some(pick) { None } else { Some(pick) };
        (self.hover, self.hover_probe, self.readout) = (None, None, None);
    }

    /// Asks for the pixel under the pointer, for the readout beside it.
    pub fn probe_hover(&mut self) {
        let Some(pixel) = self.hover.and_then(|p| self.pixel_at(p)) else {
            self.readout = None;
            return;
        };
        let id = self.next_probe;
        self.next_probe += 1;
        self.probe_request = Some((id, pixel));
        self.hover_probe = Some((id, pixel));
    }

    /// A hover probe's answer. The pointer may have moved on while it was read; then the pixel
    /// it is over now is asked for.
    pub fn hover_probed(&mut self, pixel: [u32; 2], rgb: [f32; 3]) {
        self.hover_probe = None;
        if self.picker.is_none() {
            return;
        }
        self.readout = Some(rgb);
        if self.hover.and_then(|p| self.pixel_at(p)) != Some(pixel) {
            self.probe_hover();
        }
    }

    /// Takes a photo's edit for a later paste: the open photo's in Develop, the selection's
    /// primary's in the Library.
    pub fn copy_edits(&mut self) {
        let from = match (self.module, &self.selected.primary) {
            (Module::Library, None) => return,
            (Module::Library, Some(p)) if self.path.as_ref() != Some(p) => sidecar::load(p),
            _ => Ok(self.edit.clone()),
        };
        let name = |p: &PathBuf| p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let source = if self.module == Module::Library { self.selected.primary.as_ref() } else { self.path.as_ref() };
        let source = source.map(name).unwrap_or_default();
        let note = match from {
            Ok(edit) => {
                (self.copied, self.status) = (Some(edit), "Edits copied".into());
                Note { text: format!("Copied {source}'s edits"), failed: false, hint: Some("⇧⌘V pastes them onto the selection") }
            }
            Err(e) => {
                self.status = format!("Couldn't read the edit file: {}", first_line(&e.to_string()));
                Note { text: format!("Nothing copied from {source}: {}", first_line(&e.to_string())), failed: true, hint: None }
            }
        };
        if self.module == Module::Library {
            self.library_note = Some(note);
        }
    }

    /// Puts the copied look on the open photo, as one undo step.
    pub fn paste_edits(&mut self) {
        let Some(from) = self.copied.clone() else { return };
        self.put_look(&from);
    }

    /// Puts `from`'s look (a copied edit or a preset) on the open photo, as one undo step.
    pub fn put_look(&mut self, from: &Edit) {
        self.edit.paste_look(from);
        // A selected curve point may not exist on the pasted curves.
        (self.curve_point, self.curve_removing) = (None, false);
        self.save();
    }

    /// Puts the copied look on every selected photo, from the Library. Each photo's own undo
    /// history takes the step, so ⌘Z takes the paste back once that photo is open in Develop.
    pub fn paste_onto_selected(&mut self) {
        let Some(from) = self.copied.clone() else { return };
        let targets = self.targets();
        let mut failed = Vec::new();
        for path in targets.clone() {
            if self.path.as_ref() == Some(&path) {
                self.paste_edits();
                self.refresh_thumb();
                continue;
            }
            let mut edit = match sidecar::load(&path) {
                Ok(edit) => edit,
                Err(e) => {
                    failed.push((path, first_line(&e.to_string()).to_owned()));
                    continue;
                }
            };
            let mut history = self.histories.remove(&path).filter(|h| h.is_at(&edit)).unwrap_or_else(|| History::new(&edit));
            edit.paste_look(&from);
            if let Err(e) = sidecar::save(&path, &edit) {
                failed.push((path, first_line(&e.to_string()).to_owned()));
                continue;
            }
            history.record(&edit);
            self.histories.insert(path.clone(), history);
            if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
                e.edited = edit.has_adjustments();
            }
            self.jobs.rethumbnail(path);
        }
        self.report(&targets, &failed, |what| format!("Pasted edits onto {what}"), Some("⌘Z in Develop takes it back, photo by photo"));
    }

    /// Starts typing a slider's value.
    fn start_entry(&mut self, a: Adjustment) {
        let shown = readout(a.readout, (a.get)(&self.edit));
        let mut field = TextEdit::new(shown.trim_end_matches(['°', 'K']).trim_end());
        // Selected, so typing replaces the old value.
        field.set_selection(0, field.text.len());
        field.focused = true;
        self.entry = Some(a);
        self.entry_text = field;
    }

    /// Ends typing: the slider takes the typed value if it is a number, kept to its range.
    pub fn commit_entry(&mut self) {
        let Some(a) = self.entry.take() else { return };
        if let Some(typed) = typed_value(&self.entry_text.text) {
            // A kelvin is typed as itself and kept as the Temperature that reaches it.
            let v = match a.readout {
                Readout::Kelvin(shot) => AsShot { kelvin: shot, duv: 0.0 }.temperature_for(typed),
                _ => typed,
            };
            (a.set)(&mut self.edit, v.clamp(a.min, a.max));
            self.save();
        }
    }
}

/// A typed slider value: "12", "+0.35", "−18", "1,5" or "5600 K", with or without a degree sign.
fn typed_value(typed: &str) -> Option<f32> {
    let plain = typed.trim().trim_end_matches(['°', 'K', 'k']).trim_end().trim_start_matches('+').replace('−', "-").replace(',', ".");
    plain.parse::<f32>().ok().filter(|v| v.is_finite())
}

pub fn view(s: &State, inset: f32) -> El {
    if let Some((path, error)) = &s.open_failed {
        return failed_view(s, inset, path, error);
    }
    let (shown, _) = s.view_geometry();
    let zoomed = s.zoomed();
    let mut photo = effect(PHOTO_SLOT).key("photo").on_layout(|s: &mut State, [_, _, w, h]: [f32; 4]| s.slot_pts = [w, h]);
    // Fitted, the slot is the photo. At 100% it is the whole stage, showing part of the photo.
    photo = if zoomed { photo.w_full().grow(1.0).min_h(0.0) } else { photo.aspect_fit(shown[0] / shown[1]) };
    if s.cropping {
        photo = crop::overlay(s, photo);
    } else if let Some(p) = s.picker {
        photo = photo
            .cursor(Cursor::Crosshair)
            .on_drag(|s: &mut State, g: &mut Drag, _tx: &mut Tx| {
                if g.phase == Phase::Down {
                    pick(s, [g.x, g.y]);
                }
            })
            .on_pointer_move_local(|s: &mut State, p: PointerEvent, _tx: &mut Tx| {
                s.hover = Some([p.x, p.y]);
                if s.hover_probe.is_none() {
                    s.probe_hover();
                }
            })
            .on_hover_change(|s: &mut State, inside: bool, _tx: &mut Tx| {
                if !inside {
                    (s.hover, s.readout) = (None, None);
                }
            })
            .children(pixel_readout(s, p));
    } else if zoomed {
        photo = photo.cursor(Cursor::Grab).on_drag(|s: &mut State, g: &mut Drag, tx: &mut Tx| {
            tx.instant();
            match g.phase {
                Phase::Down => {
                    s.pan(s.centre, [0.0, 0.0]);
                    s.pan_start = s.centre;
                }
                Phase::Move => s.pan(s.pan_start, [g.dx, g.dy]),
                Phase::Up => {}
            }
        });
    }
    if s.splitting() {
        photo = photo.children(split_line(s));
    }
    if zoomed {
        photo = photo.on_wheel(|s: &mut State, e: WheelEvent, tx: &mut Tx| {
            tx.instant();
            s.pan(s.centre, [e.dx, e.dy]);
            true
        });
    }
    let hint = s.picker.map(|p| {
        row()
            .h(32.0)
            .gap(8.0)
            .center_y()
            .child(canvas(eyedropper_icon(t::INK_MUTED)).size(16.0, 16.0))
            .child(text(format!("Click something that should be {}. Press Esc to cancel.", p.target())).font(t::LABEL).color(t::INK_MUTED))
    });
    let opening = s.opening.as_ref().map(|_| text("Opening…").font(t::LABEL).color(t::INK_MUTED));
    let loading = (zoomed && s.full_loading == Some(s.photo.id)).then(|| text("Loading the full size…").font(t::LABEL).color(t::INK_MUTED));
    let stage = col().w_full().grow(1.0).min_h(0.0).padding(24.0).gap(8.0).items_center().children(hint).children(opening).children(loading).child(photo);
    let left = col().grow(1.0).shrink().min_w(0.0).h_full().child(stage).children(filmstrip(s));

    let e = &s.edit;
    let sections = col().key("sections").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false });
    let sections = if s.cropping {
        sections.child(crop::panel(s))
    } else {
        sections
            .child(section("light", "Light", e.light.enabled, |e| e.light.enabled = !e.light.enabled, None, sliders(&LIGHT, s)))
            .child(section("color", "Color", e.color.enabled, |e| e.color.enabled = !e.color.enabled, Some(neutral_picker(s)), color_sliders(s)))
            .child(section("curves", "Curves", e.curves.enabled, |e| e.curves.enabled = !e.curves.enabled, None, curves::body(s)))
            .child(section("detail", "Detail", e.detail.enabled, |e| e.detail.enabled = !e.detail.enabled, None, sliders(&DETAIL, s)))
            .child(presets::section(s))
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

    let paste = button("paste", "Paste", State::paste_edits);
    let footer = row()
        .w_full()
        .padding(12.0)
        .gap(8.0)
        .child(button("copy", "Copy edits", State::copy_edits))
        .child(if s.copied.is_some() { paste } else { paste.disabled(true).dim(0.45) })
        .child(spacer())
        .child(button("reset", "Reset all", |s| {
            s.edit = s.edit.without_adjustments();
            s.save();
        }));

    let histogram = col().w_full().px(12.0).pb(12.0).child(curves::histogram_panel(s));
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

    let zoom = segmented("zoom", &[(Zoom::Fit, "Fit"), (Zoom::Full, "100%")], s.zoom, |s, z| s.set_zoom(z)).self_center();
    let history = row()
        .gap(8.0)
        .center_y()
        .child(icon_button("undo", "Undo", undo_icon, s.history.can_undo(), State::undo))
        .child(icon_button("redo", "Redo", redo_icon, s.history.can_redo(), State::redo))
        .child(divider())
        .child(icon_toggle("before-after", "Before and after", before_after_icon, s.split, |s| s.split = !s.split).help("Before and after side by side (Y). Hold \\ to see the whole original"))
        .child(zoom)
        .child(divider());

    let body = row().w_full().grow(1.0).min_h(0.0).child(left).child(panel);
    col().w_full().h_full().fill(t::BG_CANVAS).child(top_bar(s, inset, s.photo.name.clone(), subtitle(s), history)).child(rule()).child(body)
}

/// The line under the photo's name: its film simulation and where it is among the photos
/// shown, "Classic Chrome · 4 of 248", each part only when known.
fn subtitle(s: &State) -> Option<String> {
    let simulation = s.photo.exif.look.as_ref().and_then(|l| l.simulation.clone());
    let visible = s.visible();
    let place = s.target().and_then(|p| visible.iter().position(|&i| s.entries[i].path == p)).map(|i| format!("{} of {}", i + 1, visible.len()));
    let line: Vec<String> = simulation.into_iter().chain(place).collect();
    (!line.is_empty()).then(|| line.join(" · "))
}

/// Develop when the photo it was asked for couldn't be opened: why, in place of the photo, and
/// the filmstrip to go on from. Nothing is open, so there is nothing to edit.
fn failed_view(s: &State, inset: f32, path: &Path, error: &str) -> El {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let (retry, reveal) = (path.to_path_buf(), path.to_path_buf());
    let actions = row()
        .gap(8.0)
        .child(button("open-again", "Try again", move |s| s.open(retry.clone())))
        .child(
            el().key("show-in-finder")
                .role("button")
                .label("Show in Finder")
                .h(28.0)
                .px(12.0)
                .center()
                .radius(7.0)
                .hover(t::BG_HOVER)
                .on_press(move |_: &mut State| {
                    let _ = std::process::Command::new("open").arg("-R").arg(&reveal).spawn();
                })
                .child(text("Show in Finder").font(t::BODY).color(t::INK_MUTED)),
        );
    let body = col()
        .gap(4.0)
        .shrink()
        .child(text(format!("{name} can't be opened")).font(t::BODY_STRONG).color(t::DANGER_INK))
        .child(text(format!("{error}. The original is untouched.")).font(t::BODY).color(t::INK).wrap())
        .child(el().h(4.0))
        .child(actions);
    let alert = row().key("open-failed").w(420.0).padding(12.0).gap(8.0).radius(12.0).fill(t::DANGER_SOFT).child(canvas(alert_icon(t::DANGER_INK)).size(20.0, 20.0)).child(body);
    let stage = col().w_full().grow(1.0).min_h(0.0).padding(24.0).center().child(alert);
    let left = col().grow(1.0).shrink().min_w(0.0).h_full().child(stage).children(filmstrip(s));
    let panel = col()
        .key("panel")
        .w(300.0)
        .h_full()
        .padding(12.0)
        .fill(t::BG_PANEL)
        .child(text("Nothing to edit until the photo opens. ← → go to the next one.").font(t::CAPTION).color(t::INK_MUTED).w_full().wrap());
    let body = row().w_full().grow(1.0).min_h(0.0).child(left).child(panel);
    col().w_full().h_full().fill(t::BG_CANVAS).child(top_bar(s, inset, name, subtitle(s), row())).child(rule()).child(body)
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
        let current = s.target().as_ref() == Some(&entry.path);
        let failed = s.open_failed.as_ref().is_some_and(|(p, _)| *p == entry.path);
        let path = entry.path.clone();
        let mut image = el().w(STRIP_CELL - 8.0).h(68.0).radius(4.0).fill(t::BG_PANEL);
        if let Some(img) = s.thumbs.get(&entry.path) {
            image = image.pixels_fit(img.pixels(), SurfaceFit::Fit);
        }
        if failed {
            let badge = el().absolute(STRIP_CELL - 30.0, 4.0).size(18.0, 18.0).radius(4.0).center().fill(t::BG_CANVAS.alpha(0.7)).child(canvas(warning_icon(t::WARNING)).size(12.0, 12.0));
            image = image.child(badge);
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

/// A click on the photo (at `p`, slot points) while a picker is active. A curve picker probes
/// the developed pixel on the GPU; the neutral picker reads the photo as decoded, since white
/// balance is the first thing done to it.
fn pick(s: &mut State, p: [f32; 2]) {
    let Some(picker) = s.picker else { return };
    let Some(pixel) = s.pixel_at(p) else { return };
    s.toggle_picker(picker);
    match picker {
        Pick::Point(point) => {
            let id = s.next_probe;
            s.next_probe += 1;
            s.probe_request = Some((id, pixel));
            s.probe_for = Some((id, point));
        }
        // A raw file's white is found in kelvin and tint, the way its sliders read.
        Pick::Neutral => match s.photo.as_shot.map_or_else(|| neutral_point(srgb_to_rec2020(s.photo.linear_at(pixel))), |shot| shot.neutralize(srgb_to_rec2020(s.photo.linear_at(pixel)))) {
            Some([temperature, tint]) => {
                (s.edit.color.temperature, s.edit.color.tint) = (temperature.round(), tint.round());
                s.save();
            }
            None => s.status = "That spot can't be made neutral".into(),
        },
    }
}

/// The design's picking marks: a ring round the pointer and, once the pixel under it has been
/// read, what it is now: red, green and blue as the curves receive them.
fn pixel_readout(s: &State, pick: Pick) -> Vec<El> {
    let Some([x, y]) = s.hover else { return Vec::new() };
    let ring = el().absolute(x - 11.0, y - 11.0).size(22.0, 22.0).radius(11.0).border(1.5, t::INK).outline(1.5, 0.0, t::BG_CANVAS.alpha(0.7));
    let Some(rgb) = s.readout else { return vec![ring] };
    let [r, g, b] = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round());
    let tip = col()
        .key("pixel-readout")
        .absolute(x + 18.0, y + 14.0)
        .px(8.0)
        .py(6.0)
        .gap(2.0)
        .radius(7.0)
        .fill(t::BG_RAISED)
        .shadow(4.0, 12.0, t::BG_CANVAS.alpha(0.6))
        .child(text(pick.title()).font(t::BODY).color(t::INK))
        .child(text(format!("R {r}  G {g}  B {b}")).font(t::VALUE_SM).color(t::INK_MUTED));
    vec![ring, tip]
}

/// The split view's line, dragged sideways to move it, and the two sides' names in the top
/// corners of the slot.
fn split_line(s: &State) -> Vec<El> {
    let tag = |label: &'static str| el().px(8.0).py(4.0).radius(7.0).fill(t::BG_CANVAS.alpha(0.7)).child(text(label).font(t::CAPTION).color(t::INK));
    let names = row().absolute(0.0, 0.0).w_full().padding(8.0).child(tag("Before")).child(spacer()).child(tag("After"));
    let line = el()
        .key("split-line")
        .absolute(s.split_at * s.slot_pts[0] - 8.0, 0.0)
        .w(16.0)
        .h_full()
        .center()
        .cursor(Cursor::ResizeColumn)
        .on_drag(|s: &mut State, d: &mut Drag, tx: &mut Tx| {
            tx.instant();
            match d.phase {
                Phase::Down => s.split_start = s.split_at,
                Phase::Move => s.split_at = (s.split_start + d.dx / s.slot_pts[0].max(1.0)).clamp(0.0, 1.0),
                Phase::Up => {}
            }
        })
        .child(el().w(2.0).h_full().fill(t::INK).outline(1.0, 0.0, t::BG_CANVAS.alpha(0.5)));
    vec![names, line]
}

/// The Color section's sliders. A raw file's Temperature reads in kelvin, with what it was
/// shot at under the sliders and a way back to it.
fn color_sliders(s: &State) -> El {
    let Some(shot) = s.photo.as_shot else { return sliders(&COLOR, s) };
    let mut raw = COLOR;
    raw[2].readout = Readout::Kelvin(shot.kelvin);
    let moved = s.edit.color.temperature != 0.0 || s.edit.color.tint != 0.0;
    let back = el()
        .key("as-shot")
        .role("button")
        .label("Back to the white balance as shot")
        .h(24.0)
        .px(8.0)
        .center()
        .radius(7.0)
        .disabled(!moved)
        .hover(t::BG_HOVER)
        .on_press(|s: &mut State| {
            (s.edit.color.temperature, s.edit.color.tint) = (0.0, 0.0);
            s.save();
        })
        .child(text("As shot").font(t::BODY).color(if moved { t::INK_MUTED } else { t::INK_FAINT }));
    let line = row().w_full().center_y().child(text(format!("Shot at {:.0} K", (shot.kelvin / 50.0).round() * 50.0)).font(t::CAPTION).color(t::INK_MUTED)).child(spacer()).child(back);
    sliders(&raw, s).child(line)
}

/// The Color section's eyedropper: the next click on the photo sets Temperature and Tint.
fn neutral_picker(s: &State) -> El {
    icon_toggle("neutral-picker", "Pick a neutral point", eyedropper_icon, s.picker == Some(Pick::Neutral), |s| s.toggle_picker(Pick::Neutral))
}

/// A panel section: a heading with the section's own tools and an eye that hides its edits
/// (lit while they are hidden), then its controls.
fn section(key: &'static str, title: &'static str, enabled: bool, toggle: fn(&mut Edit), tools: Option<El>, content: El) -> El {
    let label = if enabled { format!("Hide {title} edits") } else { format!("Show {title} edits") };
    let head = row()
        .w_full()
        .h(36.0)
        .px(12.0)
        .gap(4.0)
        .center_y()
        .child(text(title).font(t::HEADING).color(t::INK))
        .child(spacer())
        .children(tools)
        .child(icon_toggle(format!("{key}-toggle"), label, eye_icon, !enabled, move |s| {
            toggle(&mut s.edit);
            s.save();
        }));
    let mut body = col().w_full().px(12.0).pb(16.0).child(content);
    if !enabled {
        body = body.dim(0.45);
    }
    col().key(key).w_full().child(head).child(body).child(rule())
}

fn sliders(adjustments: &[Adjustment], s: &State) -> El {
    adjustments.iter().fold(col().w_full().gap(12.0), |c, a| c.child(slider(*a, s)))
}

fn readout(r: Readout, v: f32) -> String {
    let s = match r {
        Readout::Stops if v.abs() < 0.005 => "0.00".to_owned(),
        Readout::Stops => format!("{v:+.2}"),
        Readout::Whole if v.round() == 0.0 => "0".to_owned(),
        Readout::Whole => format!("{:+.0}", v.round()),
        Readout::Degrees if v.abs() < 0.05 => "0°".to_owned(),
        Readout::Degrees => format!("{v:+.1}°"),
        Readout::Kelvin(shot) => format!("{:.0} K", (AsShot { kelvin: shot, duv: 0.0 }.target(v, 0.0).kelvin / 50.0).round() * 50.0),
    };
    // A real minus sign, as the design system asks.
    s.replace('-', "−")
}

/// A slider's value as the design system's AdjustSlider shows it: a click on it turns it into a
/// field to type in. Return or a click elsewhere sets the typed value; Esc leaves it as it was.
fn value(a: Adjustment, value: f32, typing: Option<&TextEdit>) -> El {
    let slot = row().key(format!("{}-value", a.key)).min_w(44.0).h(18.0).px(4.0).radius(4.0).center_y();
    let Some(field) = typing else {
        return slot
            .role("button")
            .label(format!("Type {}", a.label))
            .hover(t::BG_RAISED)
            .on_press(move |s: &mut State| s.start_entry(a))
            .child(spacer())
            .child(text(readout(a.readout, value)).font(t::VALUE).color(t::INK));
    };
    let input = text_input(field, t::VALUE)
        .single_line()
        .key(format!("{}-entry", a.key))
        .label(a.label)
        .grow(1.0)
        .color(t::INK)
        .selection_color(t::ACCENT_SOFT)
        .on_edit(|s: &mut State| &mut s.entry_text)
        .on_submit(|s: &mut State, tx: &mut Tx| {
            s.commit_entry();
            tx.blur();
        })
        .on_input_key(|s: &mut State, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key != Key::Escape {
                return false;
            }
            s.entry = None;
            tx.blur();
            true
        });
    slot.w(64.0).fill(t::BG_RAISED).border(1.0, t::ACCENT).outline(3.0, 0.0, t::ACCENT_SOFT).child(input)
}

/// The design system's AdjustSlider: label and value over a track whose accent fill runs from
/// zero (centre for bipolar sliders, left for unipolar) to the thumb. Drag or click to set,
/// double-click the track or the label to reset; the edit saves when the pointer lifts.
pub fn slider(a: Adjustment, s: &State) -> El {
    let value_now = (a.get)(&s.edit);
    let typing = s.entry.filter(|e| e.key == a.key).map(|_| &s.entry_text);
    let span = a.max - a.min;
    let frac = ((value_now - a.min) / span).clamp(0.0, 1.0);
    let zero = ((0.0 - a.min) / span).clamp(0.0, 1.0);
    let (lo, hi) = (frac.min(zero), frac.max(zero));
    let changed = value_now.abs() > 1e-4;
    let (min, max, set) = (a.min, a.max, a.set);
    let reset = move |s: &mut State| {
        set(&mut s.edit, 0.0);
        s.save();
    };
    let header = row()
        .w_full()
        .center_y()
        .child(text(a.label).key(format!("{}-label", a.key)).font(t::LABEL).color(if changed { t::INK } else { t::INK_MUTED }).on_double_press(reset))
        .child(spacer())
        .child(value(a, value_now, typing));
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
        .on_double_press(reset);
    col().w_full().gap(6.0).child(header).child(track)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values_read_as_the_readout_writes_them() {
        assert_eq!(typed_value("12"), Some(12.0));
        assert_eq!(typed_value(" +0.35 "), Some(0.35));
        assert_eq!(typed_value("−18"), Some(-18.0));
        assert_eq!(typed_value("1,5"), Some(1.5));
        assert_eq!(typed_value("-2.5°"), Some(-2.5));
    }

    #[test]
    fn text_that_is_not_a_number_is_no_value() {
        assert_eq!(typed_value(""), None);
        assert_eq!(typed_value("abc"), None);
        assert_eq!(typed_value("inf"), None);
        assert_eq!(typed_value("NaN"), None);
    }
}
