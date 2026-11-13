//! Darkroom: a photo editor in a RUNE window. The Library browses a folder of photos; Develop
//! edits one, on RUNE's own GPU device. Edits load from and save to each photo's edit file.
//!
//! `cargo run -p darkroom` opens the folder chosen last time (or asks for one).
//! `cargo run -p darkroom -- path/to/folder` browses that folder; `-- path/to/photo.jpg` opens a
//! photo in Develop. `DARKROOM_NO_PREFS=1` ignores and keeps no saved settings (for tests).

mod curves;
mod develop;
mod jobs;
mod library;
mod photo;
mod theme;
mod ui;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use darkroom_core::library::{Entry, Filter, Folder, Sort};
use darkroom_core::{Channel, Curve, Edit, Flag, sidecar};
use darkroom_render::{Develop, Histogram, Placement, Results};
use rune::compose::DeviceKey;
use rune::prefs::Prefs;
use rune::{AppSpec, Compositor, Cx, El, Frame, Image, ImageCache, Key, KeyEvent, Tx, Waker};

use crate::curves::Picker;
use crate::jobs::{Jobs, Msg};
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
    filter: Filter,
    sort: Sort,
    thumb_size: f32,
    selected: Option<PathBuf>,

    // Develop.
    photo: Arc<Photo>,
    /// Where the open photo came from; None for the test chart, which is never saved.
    path: Option<PathBuf>,
    /// A photo being opened in the background.
    opening: Option<PathBuf>,
    edit: Edit,
    show_original: bool,
    /// The last save's or open's outcome, shown in the panel.
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

    /// Saves the open photo's edit beside it.
    fn save(&mut self) {
        let Some(path) = self.path.clone() else { return };
        self.status = match sidecar::save(&path, &self.edit) {
            Ok(()) if self.edit.has_adjustments() => "Edited · saved".into(),
            Ok(()) => "Not edited".into(),
            Err(e) => format!("Couldn't save: {e}"),
        };
        let (rating, flag, edited) = (self.edit.rating, self.edit.flag, self.edit.has_adjustments());
        if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
            (e.rating, e.flag, e.edited) = (rating, flag, edited);
        }
    }

    /// Indices into `entries` the grid and filmstrip show, in order.
    fn visible(&self) -> Vec<usize> {
        darkroom_core::library::visible(&self.entries, self.filter, self.sort)
    }

    fn entry(&self, path: &Path) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_deref().and_then(|p| self.entry(p))
    }

    fn switch_to(&mut self, module: Module) {
        if module == Module::Develop
            && self.path.is_none()
            && let Some(p) = self.selected.clone()
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
        self.selected = None;
    }

    /// Opens a photo in Develop (in the background; the current one stays until it's ready).
    fn open(&mut self, path: PathBuf) {
        self.selected = Some(path.clone());
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

    /// The photo keyboard shortcuts act on: the open one in Develop, the selected one in the
    /// Library.
    fn target(&self) -> Option<PathBuf> {
        match self.module {
            Module::Develop => self.path.clone(),
            Module::Library => self.selected.clone(),
        }
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
            Module::Library => self.selected = Some(path),
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
                if self.selected.is_none() {
                    self.selected = visible.first().map(|&i| self.entries[i].path.clone());
                }
                let wanted: Vec<PathBuf> = visible.iter().map(|&i| self.entries[i].path.clone()).filter(|p| !self.thumbs.contains_key(p)).collect();
                self.jobs.thumbnails(wanted);
            }
            Msg::Thumb { path, width, height, rgba } => {
                if let Some(images) = &self.images {
                    self.thumbs.insert(path, images.rgba(width, height, rgba));
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
                        self.histogram = photo.histogram;
                        self.photo = Arc::new(photo);
                        self.status = if edit.has_adjustments() { "Edited".into() } else { "Not edited".into() };
                        self.edit = edit;
                        self.path = Some(path);
                        self.show_original = false;
                        (self.curve_point, self.curve_removing, self.picker, self.probe_for) = (None, false, None, None);
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
        filter: Filter::default(),
        sort: Sort::default(),
        thumb_size: 168.0,
        selected: None,
        histogram: photo.histogram,
        photo: Arc::new(photo),
        path: None,
        opening: None,
        edit: Edit::default(),
        show_original: false,
        status: String::new(),
        curve_channel: Channel::Rgb,
        curve_point: None,
        curve_removing: false,
        picker: None,
        probe_request: None,
        probe_for: None,
        next_probe: 1,
        results: Arc::new(Results::default()),
        waker: None,
    };
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
            match Photo::open(&file.to_string_lossy()) {
                Ok(photo) => {
                    s.histogram = photo.histogram;
                    s.photo = Arc::new(photo);
                    s.edit = sidecar::load(&file).unwrap_or_default();
                    s.status = if s.edit.has_adjustments() { "Edited".into() } else { "Not edited".into() };
                    s.selected = Some(file.clone());
                    s.path = Some(file);
                    s.module = Module::Develop;
                }
                Err(e) => s.status = format!("Couldn't open {}: {e}", file.display()),
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

/// Keyboard shortcuts, as in Lightroom: 0–5 rate, P pick, X reject, U unflag, ← → move between
/// photos, Return opens the selected photo in Develop, G or Esc goes back to the Library.
fn on_key(s: &mut State, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return false;
    }
    match &ev.key {
        Key::Left => s.step(-1),
        Key::Right => s.step(1),
        Key::Enter if s.module == Module::Library => s.switch_to(Module::Develop),
        Key::Escape if s.module == Module::Develop => s.module = Module::Library,
        Key::Text(t) => {
            let target = s.target();
            match (t.to_lowercase().as_str(), target) {
                (d @ ("0" | "1" | "2" | "3" | "4" | "5"), Some(p)) => s.set_rating(&p, d.parse().unwrap_or(0)),
                ("p", Some(p)) => s.set_flag(&p, Flag::Pick),
                ("x", Some(p)) => s.set_flag(&p, Flag::Reject),
                ("u", Some(p)) => s.set_flag(&p, Flag::None),
                ("g", _) => s.module = Module::Library,
                ("d", _) => s.switch_to(Module::Develop),
                _ => return false,
            }
        }
        _ => return false,
    }
    true
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    match s.module {
        Module::Library => library::view(s),
        Module::Develop => develop::view(s),
    }
}

// ---- drawing ----------------------------------------------------------------------------------

/// Draws RUNE's frame, running the develop passes first when the edit changed and painting the
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
        if s.module == Module::Develop {
            if develop.photo_id() != Some(s.photo.id) {
                develop.set_photo(f.device, f.queue, s.photo.id, s.photo.size, &s.photo.rgba);
            }
            if let Some((id, pixel)) = s.probe_request.take() {
                develop.probe(id, pixel);
            }
            develop.update(f.device, f.queue, &s.shown());
        }

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
            ("hidden", |s: &mut State| {
                develop_scene(s);
                s.edit.light.exposure = 1.0;
                s.edit.light.enabled = false;
            }),
        ],
        ..AppSpec::new("Darkroom", (1440.0, 900.0), init, view)
    };
    rune::tooling::run_with(spec, Preview::default());
}
