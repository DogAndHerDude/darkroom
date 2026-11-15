//! Darkroom: a photo editor in a RUNE window. The Library browses a folder of photos; Develop
//! edits one, on RUNE's own GPU device. Edits load from and save to each photo's edit file.
//!
//! `cargo run -p darkroom` opens the folder chosen last time (or asks for one).
//! `cargo run -p darkroom -- path/to/folder` browses that folder; `-- path/to/photo.jpg` opens a
//! photo in Develop. `DARKROOM_NO_PREFS=1` ignores and keeps no saved settings (for tests).

mod crop;
mod curves;
mod develop;
mod export;
mod jobs;
mod library;
mod photo;
mod raw;
mod theme;
mod ui;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use darkroom_core::library::{Entry, Filter, Folder, Selection, Sort};
use darkroom_core::geometry::{self, Affine};
use darkroom_core::{Channel, Curve, Edit, Flag, History, sidecar};
use darkroom_render::{Develop, Histogram, Placement, Results};
use rune::compose::DeviceKey;
use rune::prefs::Prefs;
use rune::{App, AppSpec, Chrome, Compositor, Cx, El, Frame, Image, ImageCache, Key, KeyEvent, TextEdit, Tx, Waker};

use crate::curves::Picker;
use crate::develop::{Adjustment, Pick, Zoom};
use crate::jobs::{Full, Jobs, Msg, Prepared, ThumbStore};
use crate::photo::Photo;

/// The effect slot the photo is drawn into.
const PHOTO_SLOT: u32 = 1;
const SIDEBAR_WIDTH: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Module {
    Library,
    Develop,
}

struct State {
    module: Module,
    prefs: Prefs,
    jobs: Jobs,
    /// Thumbnails become images here; made in `setup`, which has the waker.
    images: Option<ImageCache>,

    // Library.
    /// The chosen folder and the folders under it that hold photos.
    root: Option<PathBuf>,
    tree: Option<Folder>,
    /// The folder the grid and filmstrip show.
    folder: Option<PathBuf>,
    folder_generation: u64,
    loading_folder: bool,
    entries: Vec<Entry>,
    thumbs: HashMap<PathBuf, Image>,
    /// Edited photos decoded for their thumbnails, waiting for the compositor to develop them.
    thumb_gpu: Vec<(Box<Prepared>, Option<ThumbStore>)>,
    /// A photo whose edit was saved since its thumbnail was made.
    thumb_stale: Option<PathBuf>,
    filter: Filter,
    sort: Sort,
    thumb_size: f32,
    selected: Selection,

    // Develop.
    photo: Arc<Photo>,
    /// Where the open photo came from; None for the test chart, which is never saved.
    path: Option<PathBuf>,
    /// A photo being opened in the background.
    opening: Option<PathBuf>,
    edit: Edit,
    /// The open photo's undo steps.
    history: History,
    /// The undo steps of photos opened earlier, kept until the app quits.
    histories: HashMap<PathBuf, History>,
    /// The preview shows the photo without its edits: the top-bar toggle, or `\` held down.
    show_original: bool,
    /// The edit taken by "Copy edits", for Paste. Kept until the app quits.
    copied: Option<Edit>,
    /// The last save's or open's outcome, shown in the panel.
    status: String,
    /// The curve the Curves section shows.
    curve_channel: Channel,
    /// The selected point on that curve.
    curve_point: Option<usize>,
    /// The selected point is being dragged off the graph and goes on release.
    curve_removing: bool,
    /// The crop tool is open.
    cropping: bool,
    /// The crop tool's aspect: width over height, `crop::ORIGINAL`, or None for free.
    crop_aspect: Option<f32>,
    /// The crop when the current drag on its frame began.
    crop_start: Option<[f32; 4]>,
    /// The active picker: the next click on the photo sets something from that pixel.
    picker: Option<Pick>,
    /// A pixel the compositor should probe: (request id, photo pixel).
    probe_request: Option<(u64, [u32; 2])>,
    /// The curve picker the outstanding probe is for, by request id.
    probe_for: Option<(u64, Picker)>,
    next_probe: u64,
    /// Where the pointer is over the photo while a picker is active, in the slot's points.
    hover: Option<[f32; 2]>,
    /// The outstanding probe of the pixel under the pointer: (request id, photo pixel).
    hover_probe: Option<(u64, [u32; 2])>,
    /// The pixel under the pointer as the curves receive it, for the readout beside it.
    readout: Option<[f32; 3]>,
    zoom: Zoom,
    /// The middle of the 100% view, in fractions of the output.
    centre: [f32; 2],
    /// The centre when the current pan began.
    pan_start: [f32; 2],
    /// The photo slot's size in points (from layout) and in screen pixels (from the last draw).
    slot_pts: [f32; 2],
    slot_px: [f32; 2],
    /// The open photo at full size, for 100%, and the id of a photo being decoded for it.
    full: Option<Full>,
    full_loading: Option<u64>,
    /// The slider whose value is being typed, and the field it is typed in.
    entry: Option<Adjustment>,
    entry_text: TextEdit,
    /// The clipping warnings painted over the photo, switched by the histogram's two squares.
    clip_shadows: bool,
    clip_highlights: bool,
    /// The edited photo's histogram, live from the GPU (the original's until the first arrives).
    histogram: Histogram,
    /// Where the GPU leaves results for `poll` to pick up.
    results: Arc<Results>,
    /// Wakes the window when results arrive; set in `setup`.
    waker: Option<Waker>,

    // Export.
    export_settings: export::Settings,
    /// The export dialog is open, for these photos.
    export_scope: Option<export::Scope>,
    export_run: Option<export::Run>,
    next_export_run: u64,
    /// A photo decoded for export, waiting for the compositor (which has the GPU) to render it.
    export_gpu: Option<(u64, usize, Box<Prepared>)>,
}

impl State {
    /// The edit the preview shows.
    fn shown(&self) -> Edit {
        if self.show_original { self.edit.without_adjustments() } else { self.edit.clone() }
    }

    /// Saves the open photo's edit beside it. Every finished change ends here, so this is
    /// also where it becomes an undo step.
    fn save(&mut self) {
        self.history.record(&self.edit);
        let Some(path) = self.path.clone() else { return };
        self.status = match sidecar::save(&path, &self.edit) {
            Ok(()) if self.edit.has_adjustments() => "Edited · saved".into(),
            Ok(()) => "Not edited".into(),
            Err(e) => format!("Couldn't save: {e}"),
        };
        self.thumb_stale = Some(path.clone());
        let (rating, flag, edited) = (self.edit.rating, self.edit.flag, self.edit.has_adjustments());
        if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
            (e.rating, e.flag, e.edited) = (rating, flag, edited);
        }
    }

    /// What the photo slot shows: its size in pixels and the map from it to the photo. The
    /// cropped output in preview pixels; while cropping the whole straightened canvas; at 100%
    /// the part of the output that fits the slot, in screen pixels.
    fn view_geometry(&self) -> ([f32; 2], Affine) {
        self.view_geometry_in(self.slot_px)
    }

    /// [`Self::view_geometry`] for a slot of `view` screen pixels, which only 100% depends on.
    fn view_geometry_in(&self, view: [f32; 2]) -> ([f32; 2], Affine) {
        let g = &self.shown().geometry;
        let source = self.photo.size;
        if self.cropping {
            (g.oriented_size(source), g.rect_to_source(source, [0.0, 0.0, 1.0, 1.0]))
        } else if self.zoom == Zoom::Full {
            let out = g.output_size(self.photo.full_size);
            let window = geometry::window(out, view, self.centre);
            ([window[2] * out[0] as f32, window[3] * out[1] as f32], g.window_to_source(source, window))
        } else {
            (g.output_size(source).map(|v| v as f32), g.output_to_source(source))
        }
    }

    fn set_cropping(&mut self, on: bool) {
        self.cropping = on;
        self.picker = None;
        if on {
            self.zoom = Zoom::Fit;
        }
        if !on {
            self.crop_start = None;
        }
    }

    /// The photos an export of `scope` would write.
    fn export_paths(&self, scope: export::Scope) -> Vec<PathBuf> {
        match scope {
            export::Scope::Chosen => self.targets(),
            export::Scope::Shown => self.visible().into_iter().map(|i| self.entries[i].path.clone()).collect(),
        }
    }

    fn start_export(&mut self, scope: export::Scope) {
        let paths = self.export_paths(scope);
        if paths.is_empty() {
            self.export_scope = None;
            return;
        }
        let _ = self.prefs.set("export", &self.export_settings);
        self.next_export_run += 1;
        let run = export::Run {
            id: self.next_export_run,
            destination: self.export_settings.destination_for(self.folder.as_deref()),
            settings: self.export_settings.clone(),
            paths,
            next: 0,
            written: Vec::new(),
            failed: Vec::new(),
        };
        self.export_run = Some(run);
        self.export_next();
    }

    /// Starts the run's next photo; one at a time keeps memory to one full-size photo.
    fn export_next(&mut self) {
        let Some(run) = &mut self.export_run else { return };
        let Some(path) = run.paths.get(run.next).cloned() else { return };
        let index = run.next;
        run.next += 1;
        self.jobs.prepare_export(run.id, index, path, run.settings.long_edge);
    }

    fn export_failed(&mut self, index: usize, error: String) {
        if let Some(run) = &mut self.export_run {
            let name = run.paths.get(index).and_then(|p| p.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            run.failed.push(format!("Couldn't export {name}: {error}"));
        }
        self.export_next();
    }

    /// Indices into `entries` the grid and filmstrip show, in order.
    fn visible(&self) -> Vec<usize> {
        darkroom_core::library::visible(&self.entries, self.filter, self.sort)
    }

    fn entry(&self, path: &Path) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.primary.as_deref().and_then(|p| self.entry(p))
    }

    /// The selected photos the grid shows, in its order. One that a filter hides is left out,
    /// so nothing is done to a photo that can't be seen.
    fn selection(&self) -> Vec<PathBuf> {
        self.visible().into_iter().map(|i| &self.entries[i].path).filter(|p| self.selected.contains(p)).cloned().collect()
    }

    /// Makes the last edited photo's thumbnail again. Done on leaving the photo, not on every
    /// save, as each one decodes the file.
    fn refresh_thumb(&mut self) {
        if let Some(path) = self.thumb_stale.take() {
            self.jobs.rethumbnail(path);
        }
    }

    fn switch_to(&mut self, module: Module) {
        self.refresh_thumb();
        if module == Module::Develop
            && self.path.is_none()
            && let Some(p) = self.selected.primary.clone()
        {
            self.open(p);
        }
        self.module = module;
    }

    /// Uses `root` as the library, scanning its folders. `remember` keeps it for next time.
    fn set_root(&mut self, root: PathBuf, remember: bool) {
        if remember {
            let _ = self.prefs.set("root", &root);
        }
        self.jobs.scan(root.clone());
        let last = self.prefs.get::<PathBuf>("folder").filter(|f| f.starts_with(&root) && f.is_dir());
        self.root = Some(root.clone());
        self.open_folder(last.unwrap_or(root));
    }

    /// Shows `folder`'s photos, loading them in the background.
    fn open_folder(&mut self, folder: PathBuf) {
        if self.root.is_some() {
            let _ = self.prefs.set("folder", &folder);
        }
        self.folder_generation = self.jobs.load_folder(folder.clone());
        self.folder = Some(folder);
        self.entries.clear();
        self.loading_folder = true;
        self.selected = Selection::default();
    }

    fn undo(&mut self) {
        let step = self.history.undo(&self.edit);
        self.restore(step);
    }

    fn redo(&mut self) {
        let step = self.history.redo(&self.edit);
        self.restore(step);
    }

    /// Puts an undo or redo step in place and saves it.
    fn restore(&mut self, step: Option<Edit>) {
        let Some(edit) = step else { return };
        self.edit = edit;
        // A selected curve point or a crop drag may no longer exist in the restored edit.
        (self.curve_point, self.curve_removing, self.crop_start) = (None, false, None);
        self.save();
    }

    /// Opens a photo in Develop (in the background; the current one stays until it's ready).
    fn open(&mut self, path: PathBuf) {
        self.selected = Selection::only(path.clone());
        if self.path.as_ref() == Some(&path) || self.opening.as_ref() == Some(&path) {
            return;
        }
        self.opening = Some(path.clone());
        self.jobs.open(path);
    }

    /// Sets a photo's rating (0–5), in its edit file and the library.
    fn set_rating(&mut self, path: &Path, rating: u8) {
        self.change(path, |e| e.rating = rating.min(5));
    }

    fn set_flag(&mut self, path: &Path, flag: Flag) {
        self.change(path, |e| e.flag = flag);
    }

    /// Changes a photo's edit: the open one's directly, any other through its edit file.
    fn change(&mut self, path: &Path, f: impl FnOnce(&mut Edit)) {
        if self.path.as_deref() == Some(path) {
            f(&mut self.edit);
            self.save();
            return;
        }
        match sidecar::load(path) {
            Ok(mut edit) => {
                f(&mut edit);
                if let Err(e) = sidecar::save(path, &edit) {
                    self.status = format!("Couldn't save: {e}");
                }
                if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
                    (e.rating, e.flag) = (edit.rating, edit.flag);
                }
            }
            Err(e) => self.status = format!("Couldn't read the edit file: {e}"),
        }
    }

    /// The photo the arrow keys move from: the open one in Develop, the selection's primary in
    /// the Library.
    fn target(&self) -> Option<PathBuf> {
        match self.module {
            Module::Develop => self.path.clone(),
            Module::Library => self.selected.primary.clone(),
        }
    }

    /// The photos a rating, a flag, a paste or an export acts on: the open one in Develop, the
    /// whole selection in the Library. A primary that the filter hides is still shown in the
    /// info panel, so when nothing else is selected it is acted on alone.
    fn targets(&self) -> Vec<PathBuf> {
        let shown = if self.module == Module::Library { self.selection() } else { Vec::new() };
        if shown.is_empty() { self.target().into_iter().collect() } else { shown }
    }

    /// Moves to the next (`+1`) or previous (`-1`) photo in the grid's order.
    fn step(&mut self, by: isize) {
        let visible = self.visible();
        if visible.is_empty() {
            return;
        }
        let at = self.target().and_then(|p| visible.iter().position(|&i| self.entries[i].path == p));
        let next = at.map_or(0, |i| (i as isize + by).clamp(0, visible.len() as isize - 1) as usize);
        let path = self.entries[visible[next]].path.clone();
        match self.module {
            Module::Library => self.selected = Selection::only(path),
            Module::Develop => self.open(path),
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Tree(tree) => {
                // A root with no photos of its own opens its first folder that has some.
                if self.folder.as_ref() == Some(&tree.path)
                    && tree.photos == 0
                    && let Some((_, first)) = tree.flatten().into_iter().find(|(_, f)| f.photos > 0)
                {
                    let first = first.path.clone();
                    self.open_folder(first);
                }
                self.tree = Some(tree);
            }
            Msg::Folder { generation, folder, entries } => {
                if generation != self.folder_generation || self.folder.as_ref() != Some(&folder) {
                    return;
                }
                self.entries = entries;
                self.loading_folder = false;
                let visible = self.visible();
                if self.selected.is_empty() {
                    self.selected = visible.first().map(|&i| Selection::only(self.entries[i].path.clone())).unwrap_or_default();
                }
                let wanted: Vec<PathBuf> = visible.iter().map(|&i| self.entries[i].path.clone()).filter(|p| !self.thumbs.contains_key(p)).collect();
                self.jobs.thumbnails(wanted);
            }
            Msg::ThumbRender { prepared, store } => self.thumb_gpu.push((prepared, store)),
            Msg::Thumb { path, width, height, rgba } => {
                if let Some(images) = &self.images {
                    self.thumbs.insert(path, images.rgba(width, height, rgba));
                }
            }
            Msg::Full { photo, result } => {
                if photo != self.photo.id {
                    return;
                }
                self.full_loading = None;
                match result {
                    Ok(full) => self.full = Some(*full),
                    Err(e) => {
                        self.status = format!("Couldn't load the full size: {e}");
                        self.zoom = Zoom::Fit;
                    }
                }
            }
            Msg::ExportPrepared { run, index, result } => {
                if self.export_run.as_ref().map(|r| r.id) != Some(run) {
                    return;
                }
                match result {
                    Ok(prepared) => self.export_gpu = Some((run, index, prepared)),
                    Err(e) => self.export_failed(index, e),
                }
            }
            Msg::Exported { run, index, result } => {
                if self.export_run.as_ref().map(|r| r.id) != Some(run) {
                    return;
                }
                match result {
                    Ok(path) => {
                        if let Some(r) = &mut self.export_run {
                            r.written.push(path);
                        }
                        self.export_next();
                    }
                    Err(e) => self.export_failed(index, e),
                }
            }
            Msg::Opened { path, result } => {
                if self.opening.as_ref() != Some(&path) {
                    return;
                }
                self.opening = None;
                match result {
                    Ok(opened) => {
                        let (photo, edit) = *opened;
                        self.refresh_thumb();
                        self.histogram = photo.histogram;
                        self.photo = Arc::new(photo);
                        self.status = if edit.has_adjustments() { "Edited".into() } else { "Not edited".into() };
                        // Pick up this photo's earlier history, unless its edit file has changed since.
                        let history = self.histories.remove(&path).filter(|h| h.is_at(&edit)).unwrap_or_else(|| History::new(&edit));
                        let left = std::mem::replace(&mut self.history, history);
                        if let Some(p) = self.path.take() {
                            self.histories.insert(p, left);
                        }
                        self.edit = edit;
                        self.path = Some(path);
                        self.show_original = false;
                        (self.curve_point, self.curve_removing, self.picker, self.probe_for) = (None, false, None, None);
                        (self.hover, self.hover_probe, self.readout, self.entry) = (None, None, None, None);
                        (self.cropping, self.crop_start) = (false, None);
                        // 100% stays on from photo to photo, each starting at its middle.
                        (self.full, self.full_loading, self.centre) = (None, None, [0.5, 0.5]);
                        self.ensure_full();
                    }
                    Err(e) => {
                        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                        self.status = format!("Couldn't open {name}: {e}");
                    }
                }
            }
        }
    }
}

fn init() -> State {
    let prefs = if std::env::var_os("DARKROOM_NO_PREFS").is_some() { Prefs::memory() } else { Prefs::open("app.darkroom") };
    let photo = Photo::test_chart();
    let mut s = State {
        module: Module::Library,
        prefs,
        jobs: Jobs::new(),
        images: None,
        root: None,
        tree: None,
        folder: None,
        folder_generation: 0,
        loading_folder: false,
        entries: Vec::new(),
        thumbs: HashMap::new(),
        thumb_gpu: Vec::new(),
        thumb_stale: None,
        filter: Filter::default(),
        sort: Sort::default(),
        thumb_size: 168.0,
        selected: Selection::default(),
        histogram: photo.histogram,
        photo: Arc::new(photo),
        path: None,
        opening: None,
        edit: Edit::default(),
        history: History::new(&Edit::default()),
        histories: HashMap::new(),
        show_original: false,
        copied: None,
        status: String::new(),
        curve_channel: Channel::Rgb,
        curve_point: None,
        curve_removing: false,
        cropping: false,
        crop_aspect: None,
        crop_start: None,
        picker: None,
        probe_request: None,
        probe_for: None,
        next_probe: 1,
        hover: None,
        hover_probe: None,
        readout: None,
        zoom: Zoom::Fit,
        centre: [0.5, 0.5],
        pan_start: [0.5, 0.5],
        slot_pts: [0.0; 2],
        slot_px: [0.0; 2],
        full: None,
        full_loading: None,
        entry: None,
        entry_text: TextEdit::new(""),
        clip_shadows: false,
        clip_highlights: false,
        results: Arc::new(Results::default()),
        waker: None,
        export_settings: export::Settings::default(),
        export_scope: None,
        export_run: None,
        next_export_run: 0,
        export_gpu: None,
    };
    s.export_settings = s.prefs.get("export").unwrap_or_default();
    s.thumb_size = s.prefs.get("thumb_size").unwrap_or(s.thumb_size).clamp(library::THUMB_MIN, library::THUMB_MAX);

    match rune::tooling::extra_args().first().map(PathBuf::from) {
        Some(dir) if dir.is_dir() => s.set_root(dir, false),
        Some(file) => {
            // A photo: open it at once (headless snapshots need it on the first frame) and
            // browse its folder.
            if let Some(folder) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
                s.root = Some(folder.to_owned());
                s.jobs.scan(folder.to_owned());
                s.open_folder(folder.to_owned());
            }
            // As `Jobs::open` does: a photo whose edit file can't be read isn't opened, or the
            // next change would save over that file.
            let opened = sidecar::load(&file).map_err(|e| format!("its edit file is unreadable: {e}")).and_then(|edit| Ok((Photo::open(&file.to_string_lossy())?, edit)));
            match opened {
                Ok((photo, edit)) => {
                    s.histogram = photo.histogram;
                    s.photo = Arc::new(photo);
                    s.edit = edit;
                    s.history = History::new(&s.edit);
                    s.status = if s.edit.has_adjustments() { "Edited".into() } else { "Not edited".into() };
                    s.selected = Selection::only(file.clone());
                    s.path = Some(file);
                    s.module = Module::Develop;
                }
                Err(e) => {
                    s.status = format!("Couldn't open {}: {e}", file.display());
                    // Still the selected photo, so rating keys don't land on the folder's first.
                    s.selected = Selection::only(file);
                }
            }
        }
        None => {
            if let Some(root) = s.prefs.get::<PathBuf>("root").filter(|r| r.is_dir()) {
                s.set_root(root, false);
            }
        }
    }
    s
}

fn setup(s: &mut State, waker: Waker) {
    s.images = Some(ImageCache::new(waker.clone()));
    s.jobs.set_waker(waker.clone());
    s.waker = Some(waker);
}

/// Picks up what background jobs and the GPU sent back since the last frame. True when the
/// view must rebuild.
fn poll(s: &mut State, _t: f64, _tx: &mut Tx) -> bool {
    let mut changed = false;
    for msg in s.jobs.take() {
        s.handle(msg);
        changed = true;
    }
    if let Some(h) = s.results.take_histogram() {
        s.histogram = h;
        changed = true;
    }
    if let Some(probed) = s.results.take_probe() {
        if let Some((id, picker)) = s.probe_for
            && id == probed.id
        {
            curves::apply_picker(&mut s.edit, picker, probed.rgb);
            s.probe_for = None;
            s.curve_point = None;
            s.save();
            changed = true;
        } else if let Some((id, pixel)) = s.hover_probe
            && id == probed.id
        {
            s.hover_probed(pixel, probed.rgb);
            changed = true;
        }
    }
    // A click away from a slider's value field takes its focus, which ends the typing.
    if s.entry.is_some() && !s.entry_text.focused {
        s.commit_entry();
        changed = true;
    }
    changed
}

/// Keyboard shortcuts, as in Lightroom: 0–5 rate, P pick, X reject, U unflag, ← → move between
/// photos, Return opens the selected photo in Develop, G or Esc goes back to the Library. ⌘Z
/// undoes and ⇧⌘Z redoes in Develop, where Z switches between the whole photo and 100%, and Esc
/// first puts down a picker. ⇧⌘C copies the photo's edits and ⇧⌘V pastes them, in the Library
/// onto every selected photo, which ratings and flags go to as well; ⌘A there selects all the
/// grid shows. In Develop `\` shows the original while it is held (`key_up` ends it).
fn on_key(s: &mut State, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    // Typing a slider's value: every key is the field's.
    if s.entry.is_some() {
        return false;
    }
    if ev.is_command('z') && s.module == Module::Develop && s.export_scope.is_none() && s.export_run.is_none() {
        if ev.mods.shift { s.redo() } else { s.undo() }
        return true;
    }
    // As in Photos and Lightroom; plain ⌘C and ⌘V stay with text.
    if ev.mods.shift && (ev.is_command('c') || ev.is_command('v')) && s.export_scope.is_none() && s.export_run.is_none() {
        match (ev.is_command('c'), s.module) {
            (true, _) => s.copy_edits(),
            (false, Module::Develop) => s.paste_edits(),
            (false, Module::Library) => s.paste_onto_selected(),
        }
        return true;
    }
    if ev.is_command('a') && s.module == Module::Library && s.export_scope.is_none() && s.export_run.is_none() {
        let shown: Vec<PathBuf> = s.visible().into_iter().map(|i| s.entries[i].path.clone()).collect();
        s.selected.all(&shown);
        return true;
    }
    if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return false;
    }
    match &ev.key {
        Key::Left => s.step(-1),
        Key::Right => s.step(1),
        Key::Escape if s.export_scope.is_some() && s.export_run.is_none() => s.export_scope = None,
        _ if s.export_scope.is_some() || s.export_run.is_some() => return false,
        Key::Enter | Key::Escape if s.cropping => s.set_cropping(false),
        Key::Escape if s.picker.is_some() && s.module == Module::Develop => s.picker = None,
        Key::Enter if s.module == Module::Library => s.switch_to(Module::Develop),
        Key::Escape if s.module == Module::Develop => s.switch_to(Module::Library),
        Key::Text(t) => {
            let targets = s.targets();
            match (t.to_lowercase().as_str(), !targets.is_empty()) {
                (d @ ("0" | "1" | "2" | "3" | "4" | "5"), true) => targets.iter().for_each(|p| s.set_rating(p, d.parse().unwrap_or(0))),
                ("p", true) => targets.iter().for_each(|p| s.set_flag(p, Flag::Pick)),
                ("x", true) => targets.iter().for_each(|p| s.set_flag(p, Flag::Reject)),
                ("u", true) => targets.iter().for_each(|p| s.set_flag(p, Flag::None)),
                ("g", _) => s.switch_to(Module::Library),
                ("d", _) => s.switch_to(Module::Develop),
                ("c", _) if s.module == Module::Develop => s.set_cropping(!s.cropping),
                ("z", _) if s.module == Module::Develop => s.set_zoom(if s.zoom == Zoom::Fit { Zoom::Full } else { Zoom::Fit }),
                ("\\", _) if s.module == Module::Develop => s.show_original = true,
                _ => return false,
            }
        }
        _ => return false,
    }
    true
}

/// A key coming up: letting go of `\` brings the edit back.
fn key_up(s: &mut State, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if !matches!(&ev.key, Key::Text(t) if t == "\\") || !s.show_original {
        return false;
    }
    s.show_original = false;
    true
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    // Where the traffic lights end; 0 in full screen, where there are none.
    let inset = cx.titlebar_inset();
    let module = match s.module {
        Module::Library => library::view(s, inset),
        Module::Develop => develop::view(s, inset),
    };
    rune::el().w_full().h_full().on_key_up(key_up).child(module).children(export::dialog(s))
}

// ---- drawing ----------------------------------------------------------------------------------

/// Draws RUNE's frame, running the develop passes first when the edit changed and painting the
/// developed photo at the photo slot. Everything stays on RUNE's device.
#[derive(Default)]
struct Preview {
    gpu: Option<(DeviceKey, Develop)>,
    /// The open photo developed at full size, for 100%: made when its pixels arrive and kept
    /// while that photo stays open.
    full: Option<Develop>,
    /// Develops edited photos' thumbnails, one after another.
    thumbs: Option<Develop>,
}

/// Edited thumbnails developed in one frame; the rest wait for the next.
const THUMBS_PER_FRAME: usize = 4;

impl Compositor<State> for Preview {
    fn render(&mut self, f: &mut Frame<'_, State>) {
        let key = f.device_key();
        if self.gpu.as_ref().map(|(k, _)| *k) != Some(key) {
            let s = &f.rt.state;
            let wake = s.waker.clone().unwrap_or_else(|| Arc::new(|| {}));
            self.gpu = Some((key, Develop::new(f.device, s.results.clone(), wake)));
            // The full-size copy lived on the old device; its pixels are decoded again.
            (self.full, self.thumbs) = (None, None);
            f.rt.state.full = None;
            f.rt.state.ensure_full();
        }
        let develop = &mut self.gpu.as_mut().expect("made above").1;
        let full = &mut self.full;

        // An export's photo: develop it at export size on a Develop of its own, render it through
        // its geometry, and write the file from the readback thread.
        if let Some((run, index, prepared)) = f.rt.state.export_gpu.take() {
            let s = &f.rt.state;
            let settings = s.export_run.as_ref().map(|r| (r.settings.clone(), r.destination.clone()));
            if let Some((settings, destination)) = settings {
                let deliver = s.jobs.deliverer();
                let mut gpu = Develop::new(f.device, Arc::new(Results::default()), Arc::new(|| {}));
                gpu.histogram_enabled = false;
                gpu.display_p3 = settings.display_p3;
                gpu.detail_scale = prepared.detail_scale;
                gpu.set_photo(f.device, f.queue, u64::MAX - index as u64, prepared.size, prepared.pixels.source());
                gpu.update(f.device, f.queue, &prepared.edit);
                let g = &prepared.edit.geometry;
                let (map, out) = (g.output_to_source(prepared.size), g.output_size(prepared.size));
                let source = prepared.path.clone();
                gpu.export(f.device, f.queue, map, out, move |rendered| {
                    let result = rendered.and_then(|image| export::write(&image, &source, &destination, &settings));
                    deliver.send(Msg::Exported { run, index, result });
                });
            }
        }

        // Edited thumbnails go the same way as an export, at thumbnail size.
        let s = &mut f.rt.state;
        let take = s.thumb_gpu.len().min(THUMBS_PER_FRAME);
        for (prepared, store) in s.thumb_gpu.drain(..take) {
            let gpu = self.thumbs.get_or_insert_with(|| {
                let mut gpu = Develop::new(f.device, Arc::new(Results::default()), Arc::new(|| {}));
                gpu.histogram_enabled = false;
                gpu
            });
            gpu.detail_scale = prepared.detail_scale;
            gpu.set_photo(f.device, f.queue, 0, prepared.size, prepared.pixels.source());
            gpu.update(f.device, f.queue, &prepared.edit);
            let g = &prepared.edit.geometry;
            let (deliver, path) = (s.jobs.deliverer(), prepared.path.clone());
            gpu.export(f.device, f.queue, g.output_to_source(prepared.size), g.output_size(prepared.size), move |rendered| {
                if let Ok(image) = rendered {
                    deliver.send(jobs::thumb_rendered(path, &image, store));
                }
            });
        }
        if !s.thumb_gpu.is_empty()
            && let Some(wake) = &s.waker
        {
            wake();
        }

        let s = &mut f.rt.state;
        if s.module == Module::Develop {
            if develop.photo_id() != Some(s.photo.id) {
                develop.set_photo(f.device, f.queue, s.photo.id, s.photo.size, s.photo.pixels.source());
            }
            if let Some((id, pixel)) = s.probe_request.take() {
                develop.probe(id, pixel);
            }
            develop.update(f.device, f.queue, &s.shown());

            if full.as_ref().is_some_and(|d| d.photo_id() != Some(s.photo.id)) {
                *full = None;
            }
            if let Some(decoded) = s.full.as_mut().filter(|d| d.photo == s.photo.id)
                && let Some(pixels) = decoded.pixels.take()
            {
                let mut gpu = Develop::new(f.device, Arc::new(Results::default()), Arc::new(|| {}));
                gpu.histogram_enabled = false;
                gpu.detail_scale = decoded.detail_scale;
                gpu.set_photo(f.device, f.queue, s.photo.id, decoded.size, pixels.source());
                *full = Some(gpu);
            }
            // Developed only while it shows; the preview above keeps the histogram and probes.
            if s.zoomed()
                && let Some(d) = full.as_mut()
            {
                d.update(f.device, f.queue, &s.shown());
            }
        }

        // At 100% the full-size photo, once it is here; until then the preview, enlarged.
        let shown = if f.rt.state.zoomed() { full.as_mut().unwrap_or(develop) } else { develop };
        let state = &f.rt.state;
        let mut slot_px = None;
        let (device, queue, clear) = (f.device, f.queue, f.rt.clear_color());
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.mark != PHOTO_SLOT {
                return;
            }
            shown.prepare_format(device, site.format);
            let [x, y, w, h] = site.instance.rect;
            let (output, map) = state.view_geometry_in([w, h]);
            let [px, py, pw, ph] = state.placed([w, h], output);
            let rect = [x - site.origin[0] + px, y - site.origin[1] + py, pw, ph];
            slot_px = Some([w, h]);
            let c = theme::BG_CANVAS;
            shown.draw(
                queue,
                pass,
                Placement {
                    rect,
                    target: site.size,
                    format: site.format,
                    encode_srgb: site.srgb_blend,
                    output,
                    map,
                    background: [c.r, c.g, c.b],
                    clip_shadow: state.clip_shadows.then_some(theme::CLIP_SHADOW).map(|c| [c.r, c.g, c.b]),
                    clip_highlight: state.clip_highlights.then_some(theme::CLIP_HIGHLIGHT).map(|c| [c.r, c.g, c.b]),
                },
            );
        });
        // For the handlers that turn a point on the slot into a place on the photo.
        if let Some(px) = slot_px {
            f.rt.state.slot_px = px;
        }
    }
}

/// Puts a scene's edit on the test chart in Develop.
fn develop_scene(s: &mut State) {
    s.module = Module::Develop;
}

fn main() {
    let spec = AppSpec {
        min_size: Some((1000.0, 640.0)),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        // For headless snapshots: `-- --snap out.png --scene <name>`.
        scenes: &[
            ("develop", develop_scene),
            ("edited", |s: &mut State| {
                develop_scene(s);
                let l = &mut s.edit.light;
                (l.exposure, l.brilliance, l.highlights, l.shadows, l.contrast, l.black_point) = (0.35, 22.0, -42.0, 18.0, 12.0, 8.0);
                (s.edit.color.saturation, s.edit.color.vibrance, s.edit.color.temperature) = (10.0, 24.0, 6.0);
            }),
            ("curves", |s: &mut State| {
                develop_scene(s);
                s.edit.curves.rgb = Curve::new(vec![[0.0, 0.04], [0.25, 0.21], [0.5, 0.5], [0.75, 0.79], [1.0, 0.97]]);
                s.edit.curves.red = Curve::new(vec![[0.0, 0.0], [0.5, 0.54], [1.0, 1.0]]);
                s.curve_point = Some(1);
            }),
            ("curve-red", |s: &mut State| {
                develop_scene(s);
                s.edit.curves.red = Curve::new(vec![[0.0, 0.0], [0.5, 0.54], [1.0, 1.0]]);
                s.curve_channel = Channel::Red;
            }),
            ("extreme", |s: &mut State| {
                develop_scene(s);
                (s.edit.light.exposure, s.edit.color.saturation, s.edit.detail.vignette) = (1.5, 100.0, 60.0);
            }),
            ("definition", |s: &mut State| {
                develop_scene(s);
                s.edit.detail.definition = 70.0;
            }),
            ("sharpen", |s: &mut State| {
                develop_scene(s);
                s.edit.detail.sharpening = 80.0;
            }),
            ("denoise", |s: &mut State| {
                develop_scene(s);
                s.edit.detail.noise_reduction = 70.0;
            }),
            // A strong warm cast, for checking the grey point picker removes it.
            ("warm", |s: &mut State| {
                develop_scene(s);
                s.edit.color.temperature = 70.0;
            }),
            ("crop", |s: &mut State| {
                develop_scene(s);
                s.edit.geometry.straighten = 8.0;
                s.cropping = true;
            }),
            ("cropped", |s: &mut State| {
                develop_scene(s);
                s.edit.geometry = darkroom_core::Geometry { straighten: 8.0, crop: Some([0.15, 0.1, 0.5, 0.7]), quarter_turns: 1, ..Default::default() };
            }),
            // 100% on the chart, which is its own full size.
            ("zoom", |s: &mut State| {
                develop_scene(s);
                s.set_zoom(Zoom::Full);
            }),
            // The neutral picker waiting for a click.
            ("picking", |s: &mut State| {
                develop_scene(s);
                s.edit.color.temperature = 70.0;
                s.toggle_picker(Pick::Neutral);
            }),
            // Both clipping warnings on a photo pushed past both ends.
            ("clipping", |s: &mut State| {
                develop_scene(s);
                (s.edit.light.exposure, s.edit.light.contrast, s.edit.light.black_point) = (1.0, 60.0, 40.0);
                (s.clip_shadows, s.clip_highlights) = (true, true);
            }),
            // An edited photo showing its original: the stage must match `develop`.
            ("original", |s: &mut State| {
                develop_scene(s);
                (s.edit.light.exposure, s.edit.color.saturation) = (1.0, 60.0);
                s.show_original = true;
            }),
            ("hidden", |s: &mut State| {
                develop_scene(s);
                s.edit.light.exposure = 1.0;
                s.edit.light.enabled = false;
            }),
        ],
        ..AppSpec::new("Darkroom", (1440.0, 900.0), init, view)
    };
    // The traffic lights sit in the top bar, centred in its height and as far from the left.
    let lights = (ui::TOP_BAR - Chrome::TRAFFIC_LIGHT) / 2.0;
    App::new(spec)
        .compositor(Preview::default())
        .chrome(Chrome { traffic_lights: Some((lights, lights)), ..Chrome::DEFAULT })
        .run();
}
