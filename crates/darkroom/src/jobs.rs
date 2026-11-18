//! Work that mustn't block a frame: scanning folders, reading a folder's photos (EXIF and edit
//! files), making thumbnails, and opening a photo for Develop. Each runs on a background thread
//! and leaves a [`Msg`] in the inbox, then wakes the window; `poll` takes the messages.
//!
//! Thumbnails are cached on disk (`~/Library/Caches/darkroom/thumbs`), keyed by the photo's
//! path, size and modification time, so a folder opens instantly the second time.
//!
//! An edited photo's thumbnail shows its edit, which only the GPU can develop. Its worker
//! decodes the photo at thumbnail size and posts [`Msg::ThumbRender`]; the compositor develops
//! it, and the readback thread ends in [`thumb_rendered`]. That thumbnail has one cache file
//! per photo, beside a note of the edit it shows, so a changed edit replaces it.
//!
//! The library root is watched (FSEvents, through `notify`). Changes arrive in bursts, a card
//! import or a Finder copy, so they are gathered until things go quiet and posted as one
//! [`Msg::Changed`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use darkroom_core::exif::Exif;
use darkroom_core::library::{self, Entry, Folder};
use darkroom_core::white::AsShot;
use darkroom_core::{Edit, Flag, raf, sidecar};
use darkroom_render::Exported;
use image::DynamicImage;
use rune::Waker;

use crate::export;
use crate::photo::{self, Photo, Pixels};

/// The long edge of a thumbnail, in pixels: sharp at the largest grid size on a Retina screen.
const THUMB_EDGE: u32 = 448;
/// A burst of changes on disk ends when nothing has changed for this long...
const QUIET: Duration = Duration::from_millis(400);
/// ...or after this long, so a long copy shows its photos as it goes.
const LONGEST: Duration = Duration::from_secs(2);

pub enum Msg {
    Tree(Folder),
    /// A folder's photos; `found` is false when the folder isn't there (moved, deleted, or on a drive that isn't connected).
    Folder { generation: u64, folder: PathBuf, found: bool, entries: Vec<Entry> },
    Thumb { path: PathBuf, width: u32, height: u32, rgba: Vec<u8> },
    /// An edited photo decoded at thumbnail size, for the GPU to develop into its thumbnail.
    ThumbRender { prepared: Box<Prepared>, store: Option<ThumbStore> },
    Opened { path: PathBuf, result: Result<Box<(Photo, Edit)>, String> },
    /// The open photo decoded at full size, for 100% zoom.
    Full { photo: u64, result: Result<Box<Full>, String> },
    /// A photo decoded at export size, ready for the GPU.
    ExportPrepared { run: u64, index: usize, result: Result<Box<Prepared>, String> },
    /// A photo written (or not) by an export.
    Exported { run: u64, index: usize, result: Result<PathBuf, String> },
    /// Photos or folders under the root changed on disk, as paths under the root as it was given.
    Changed(Vec<PathBuf>),
    /// The root went away (a drive ejected) or came back.
    RootThere(bool),
}

/// A photo ready to render for export: its pixels at export size (sRGB RGBA8) and its edit.
pub struct Prepared {
    pub path: PathBuf,
    pub size: [u32; 2],
    pub pixels: Pixels,
    pub edit: Edit,
    /// Pixels here per pixel of the preview this photo is edited on, for the detail tools.
    pub detail_scale: f32,
    /// A raw file's white as shot, which its Temperature and Tint move from.
    pub as_shot: Option<AsShot>,
}

/// Where an edited photo's thumbnail is cached, and the edit it will show.
pub struct ThumbStore {
    image: PathBuf,
    /// Holds `key`, written after the image so a half-written pair reads as out of date.
    note: PathBuf,
    key: String,
}

/// A photo at its own size, for 100% zoom.
pub struct Full {
    /// The id of the preview (`Photo::id`) this is the full size of.
    pub photo: u64,
    pub size: [u32; 2],
    /// Taken by the compositor when it uploads them, so they are held only once.
    pub pixels: Option<Pixels>,
    pub detail_scale: f32,
}

/// Posts messages from any thread, waking the window.
#[derive(Clone)]
pub struct Deliver {
    inbox: Arc<Mutex<Vec<Msg>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Deliver {
    pub fn send(&self, msg: Msg) {
        deliver(&self.inbox, &self.waker, msg);
    }
}

struct ThumbJob {
    generation: u64,
    path: PathBuf,
}

pub struct Jobs {
    inbox: Arc<Mutex<Vec<Msg>>>,
    waker: Arc<Mutex<Option<Waker>>>,
    thumbs: Sender<ThumbJob>,
    /// Thumbnail requests from an older folder are skipped.
    thumb_generation: Arc<AtomicU64>,
    /// Bumped per folder load, so a slow load for a folder you've left is ignored.
    folder_generation: u64,
    /// Watches the root; dropping it ends the watch and its thread.
    watcher: Option<notify::RecommendedWatcher>,
    /// Held while the root is checked for; the check stops once this goes.
    root_check: Option<Arc<()>>,
}

impl Jobs {
    pub fn new() -> Jobs {
        let inbox = Arc::new(Mutex::new(Vec::new()));
        let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let thumb_generation = Arc::new(AtomicU64::new(0));
        let (thumbs, receive) = channel::<ThumbJob>();
        let receive = Arc::new(Mutex::new(receive));
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(2).clamp(1, 6));
        for _ in 0..workers {
            let (receive, inbox, waker, current) = (receive.clone(), inbox.clone(), waker.clone(), thumb_generation.clone());
            std::thread::Builder::new()
                .name("darkroom-thumbs".into())
                .spawn(move || thumb_worker(&receive, &inbox, &waker, &current))
                .expect("thumbnail worker");
        }
        Jobs { inbox, waker, thumbs, thumb_generation, folder_generation: 0, watcher: None, root_check: None }
    }

    /// Watches `root` and everything under it, in place of any earlier root.
    pub fn watch(&mut self, root: &Path) -> Result<(), String> {
        use notify::Watcher;
        self.watcher = None;
        self.check_root(root);
        if !root.is_dir() {
            // Nothing to watch until it comes back; the check says when.
            return Ok(());
        }
        let (send, receive) = channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            for path in event.map(|e| e.paths).unwrap_or_default() {
                let _ = send.send(path);
            }
        })
        .map_err(|e| e.to_string())?;
        watcher.watch(root, notify::RecursiveMode::Recursive).map_err(|e| e.to_string())?;
        // FSEvents names files by their real path (/private/tmp for /tmp); the app knows them
        // under the root as it was given.
        let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
        let (root, inbox, waker) = (root.to_owned(), self.inbox.clone(), self.waker.clone());
        std::thread::Builder::new()
            .name("darkroom-watch".into())
            .spawn(move || {
                while let Some(paths) = settle(&receive) {
                    let mut paths: Vec<PathBuf> = paths.into_iter().map(|p| p.strip_prefix(&real).map_or(p.clone(), |rest| root.join(rest))).filter(|p| library::is_library_change(p)).collect();
                    paths.sort();
                    paths.dedup();
                    if !paths.is_empty() {
                        deliver(&inbox, &waker, Msg::Changed(paths));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        self.watcher = Some(watcher);
        Ok(())
    }

    /// Looks every two seconds for the root, and says when it goes or comes back. FSEvents says
    /// nothing when the drive a watched folder is on is ejected.
    fn check_root(&mut self, root: &Path) {
        if self.root_check.is_some() {
            return;
        }
        let token = Arc::new(());
        let held = Arc::downgrade(&token);
        self.root_check = Some(token);
        let (root, inbox, waker) = (root.to_owned(), self.inbox.clone(), self.waker.clone());
        let mut there = root.is_dir();
        std::thread::Builder::new()
            .name("darkroom-root-check".into())
            .spawn(move || {
                while held.upgrade().is_some() {
                    std::thread::sleep(Duration::from_secs(2));
                    let now = root.is_dir();
                    if now != there {
                        there = now;
                        deliver(&inbox, &waker, Msg::RootThere(now));
                    }
                }
            })
            .expect("root check");
    }

    /// Stops watching and checking for the root, before another is chosen.
    pub fn unwatch(&mut self) {
        (self.watcher, self.root_check) = (None, None);
    }

    /// Called from `setup`, once the window can be woken. Wakes it at once for anything that
    /// arrived before.
    pub fn set_waker(&self, waker: Waker) {
        if let Ok(mut w) = self.waker.lock() {
            *w = Some(waker.clone());
        }
        waker();
    }

    /// Everything that arrived since the last call.
    pub fn take(&self) -> Vec<Msg> {
        self.inbox.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }

    fn spawn(&self, name: &str, work: impl FnOnce() -> Msg + Send + 'static) {
        let (inbox, waker) = (self.inbox.clone(), self.waker.clone());
        std::thread::Builder::new()
            .name(format!("darkroom-{name}"))
            .spawn(move || deliver(&inbox, &waker, work()))
            .expect("background job");
    }

    /// The folder tree under `root`.
    pub fn scan(&self, root: PathBuf) {
        self.spawn("scan", move || Msg::Tree(library::scan(&root)));
    }

    /// The photos in `folder` with their EXIF, rating and flag. Returns the load's generation.
    pub fn load_folder(&mut self, folder: PathBuf) -> u64 {
        self.folder_generation += 1;
        let generation = self.folder_generation;
        self.spawn("folder", move || {
            let entries = library::pair_raws(library::photos_in(&folder)).into_iter().map(|(path, pair)| Entry { pair, ..entry(&path) }).collect();
            Msg::Folder { generation, found: folder.is_dir(), folder, entries }
        });
        generation
    }

    /// Thumbnails for `paths`, in this order. Drops any still queued for an earlier call.
    pub fn thumbnails(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let generation = self.thumb_generation.fetch_add(1, Ordering::AcqRel) + 1;
        for path in paths {
            let _ = self.thumbs.send(ThumbJob { generation, path });
        }
    }

    /// One photo's thumbnail again, after its edit changed. Leaves the queue as it is.
    pub fn rethumbnail(&self, path: PathBuf) {
        let _ = self.thumbs.send(ThumbJob { generation: self.thumb_generation.load(Ordering::Acquire), path });
    }

    pub fn deliverer(&self) -> Deliver {
        Deliver { inbox: self.inbox.clone(), waker: self.waker.clone() }
    }

    /// Decodes a photo at full resolution for export, scaled down first so its output's long
    /// edge is at most `long_edge`.
    pub fn prepare_export(&self, run: u64, index: usize, path: PathBuf, long_edge: Option<u32>) {
        self.spawn("export", move || {
            let result = prepare(&path, long_edge).map(Box::new);
            Msg::ExportPrepared { run, index, result }
        });
    }

    /// Decodes the open photo (the preview with id `photo`) at full size.
    pub fn full(&self, photo: u64, path: PathBuf) {
        self.spawn("full", move || {
            let result = photo::decode(&path).map(|img| {
                let (size, pixels) = Pixels::from_image(img);
                Box::new(Full { photo, size, pixels: Some(pixels), detail_scale: detail_scale(size, size) })
            });
            Msg::Full { photo, result }
        });
    }

    /// Opens a photo for Develop: decodes its preview and reads its edit.
    pub fn open(&self, path: PathBuf) {
        self.spawn("open", move || {
            let result = Photo::open(&path.to_string_lossy()).and_then(|photo| {
                let edit = sidecar::load(&path).map_err(|e| format!("its edit file is unreadable: {e}"))?;
                Ok(Box::new((photo, edit)))
            });
            Msg::Opened { path, result }
        });
    }
}

fn prepare(path: &Path, long_edge: Option<u32>) -> Result<Prepared, String> {
    let edit = sidecar::load(path).map_err(|e| format!("its edit file is unreadable: {e}"))?;
    let mut img = photo::decode(path)?;
    let full = [img.width(), img.height()];
    if let Some(limit) = long_edge {
        // Scale so the output (after crop) has the long edge asked for; never enlarge.
        let out = edit.geometry.output_size(full);
        let k = limit as f32 / out[0].max(out[1]) as f32;
        if k < 1.0 {
            let (w, h) = (((full[0] as f32 * k).round() as u32).max(1), ((full[1] as f32 * k).round() as u32).max(1));
            img = img.resize_exact(w, h, image::imageops::FilterType::CatmullRom);
        }
    }
    let (size, pixels) = Pixels::from_image(img);
    let detail_scale = detail_scale(full, size);
    let as_shot = if raf::is_raf(path) { crate::raw::as_shot(path) } else { None };
    Ok(Prepared { path: path.to_owned(), size, pixels, edit, detail_scale, as_shot })
}

/// How many pixels of a render at `size` cover one pixel of the preview of a photo `full` in
/// size: the preview is the photo itself up to `PREVIEW_EDGE` on its long edge.
fn detail_scale(full: [u32; 2], size: [u32; 2]) -> f32 {
    let long = |[w, h]: [u32; 2]| w.max(h).max(1) as f32;
    long(size) / long(full).min(photo::PREVIEW_EDGE as f32)
}

/// The JPEG inside a RAF, decoded upright.
fn embedded(path: &Path) -> Option<DynamicImage> {
    use image::ImageDecoder;
    let jpeg = raf::embedded_jpeg(path)?;
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(jpeg)).with_guessed_format().ok()?.into_decoder().ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    img.apply_orientation(orientation);
    Some(img)
}

/// The paths of the next burst of changes: from the first until none has come for [`QUIET`],
/// or for at most [`LONGEST`]. None once the watch has ended.
fn settle(receive: &Receiver<PathBuf>) -> Option<Vec<PathBuf>> {
    let mut paths = vec![receive.recv().ok()?];
    let start = Instant::now();
    while start.elapsed() < LONGEST {
        match receive.recv_timeout(QUIET) {
            Ok(path) => paths.push(path),
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
    Some(paths)
}

fn deliver(inbox: &Mutex<Vec<Msg>>, waker: &Mutex<Option<Waker>>, msg: Msg) {
    if let Ok(mut v) = inbox.lock() {
        v.push(msg);
    }
    if let Some(w) = waker.lock().ok().and_then(|w| w.clone()) {
        w();
    }
}

/// What the library needs to know about one photo.
pub fn entry(path: &Path) -> Entry {
    let edit = sidecar::load(path).unwrap_or_default();
    Entry {
        path: path.to_owned(),
        name: path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        modified: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
        exif: Exif::read(path),
        rating: edit.rating,
        flag: edit.flag,
        edited: edit.has_adjustments(),
        pair: None,
    }
}

fn thumb_worker(receive: &Mutex<Receiver<ThumbJob>>, inbox: &Mutex<Vec<Msg>>, waker: &Mutex<Option<Waker>>, current: &AtomicU64) {
    loop {
        let Ok(job) = receive.lock().map_err(|_| ()).and_then(|r| r.recv().map_err(|_| ())) else { return };
        if job.generation != current.load(Ordering::Acquire) {
            continue;
        }
        match thumbnail(&job.path) {
            Some(Thumb::Ready(width, height, rgba)) => deliver(inbox, waker, Msg::Thumb { path: job.path, width, height, rgba }),
            Some(Thumb::Render(prepared, store)) => deliver(inbox, waker, Msg::ThumbRender { prepared, store }),
            None => {}
        }
    }
}

fn cache_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(std::env::temp_dir, |h| PathBuf::from(h).join("Library/Caches")).join("darkroom/thumbs")
}

/// The cached thumbnail's file: changes when the photo's path, size or modification time do.
fn cache_path(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_nanos();
    let mut hash = blake3::Hasher::new();
    hash.update(path.as_os_str().as_encoded_bytes());
    hash.update(&meta.len().to_le_bytes());
    hash.update(&modified.to_le_bytes());
    Some(cache_dir().join(format!("{}.jpg", &hash.finalize().to_hex()[..32])))
}

/// What a thumbnail job ends in.
enum Thumb {
    /// The thumbnail as straight sRGB RGBA: width, height, pixels.
    Ready(u32, u32, Vec<u8>),
    /// An edited photo the GPU has to develop first.
    Render(Box<Prepared>, Option<ThumbStore>),
}

impl Thumb {
    fn ready(img: DynamicImage) -> Thumb {
        let rgba = img.into_rgba8();
        Thumb::Ready(rgba.width(), rgba.height(), rgba.into_raw())
    }
}

/// Names the look an edited thumbnail shows. Rating and flag don't change the picture.
fn edit_key(edit: &Edit) -> String {
    let look = Edit { rating: 0, flag: Flag::None, ..edit.clone() };
    blake3::hash(sidecar::to_string(&look).unwrap_or_default().as_bytes()).to_hex().to_string()
}

/// An edited photo's thumbnail: from the disk cache when it shows this edit, or else the
/// photo decoded at thumbnail size for the GPU. None when the photo can't be decoded.
fn edited_thumbnail(path: &Path, edit: &Edit) -> Option<Thumb> {
    let key = edit_key(edit);
    let store = cache_path(path).map(|c| ThumbStore { image: c.with_extension("edit.jpg"), note: c.with_extension("edit.key"), key });
    if let Some(s) = &store
        && std::fs::read_to_string(&s.note).is_ok_and(|shown| shown == s.key)
        && let Ok(img) = image::open(&s.image)
    {
        return Some(Thumb::ready(img));
    }
    let prepared = prepare(path, Some(THUMB_EDGE)).ok()?;
    Some(Thumb::Render(Box::new(prepared), store))
}

/// Ends an edited thumbnail: the GPU's render becomes sRGB bytes, is cached, and goes to the
/// library. Runs on the readback's thread.
pub fn thumb_rendered(path: PathBuf, image: &Exported, store: Option<ThumbStore>) -> Msg {
    let eight = &export::tables().0;
    let [width, height] = image.size;
    let rgba: Vec<u8> = image.rgba16f.iter().enumerate().map(|(i, &b)| if i % 4 == 3 { 255 } else { eight[b as usize] }).collect();
    if let Some(s) = store
        && let Some(small) = image::RgbaImage::from_raw(width, height, rgba.clone())
    {
        let _ = std::fs::create_dir_all(cache_dir());
        if DynamicImage::ImageRgba8(small).into_rgb8().save_with_format(&s.image, image::ImageFormat::Jpeg).is_ok() {
            let _ = std::fs::write(&s.note, &s.key);
        }
    }
    Msg::Thumb { path, width, height, rgba }
}

/// The photo's thumbnail: from the disk cache, or made from the photo (EXIF orientation
/// applied) and cached. An edited photo's goes through [`edited_thumbnail`]; if that can't
/// decode it, the original's stands in.
fn thumbnail(path: &Path) -> Option<Thumb> {
    let edit = sidecar::load(path).unwrap_or_default();
    if edit.has_adjustments()
        && let Some(thumb) = edited_thumbnail(path, &edit)
    {
        return Some(thumb);
    }
    let cached = cache_path(path);
    if let Some(img) = cached.as_ref().and_then(|c| image::open(c).ok()) {
        return Some(Thumb::ready(img));
    }
    // A RAF's thumbnail is the JPEG the camera embedded: far quicker than developing it.
    let img = if raf::is_raf(path) { embedded(path)? } else { photo::decode(path).ok()? };
    let small = img.thumbnail(THUMB_EDGE, THUMB_EDGE).into_rgb8();
    if let Some(c) = cached {
        let _ = std::fs::create_dir_all(cache_dir());
        let _ = small.save_with_format(&c, image::ImageFormat::Jpeg);
    }
    Some(Thumb::ready(DynamicImage::ImageRgb8(small)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edited_thumbnail_is_keyed_by_the_look_alone() {
        let mut edit = Edit::default();
        edit.light.exposure = 0.5;
        let rated = Edit { rating: 4, flag: Flag::Pick, ..edit.clone() };
        assert_eq!(edit_key(&edit), edit_key(&rated));
        let mut cropped = edit.clone();
        cropped.geometry.straighten = 2.0;
        assert_ne!(edit_key(&edit), edit_key(&cropped));
    }

    #[test]
    fn detail_scale_is_measured_against_the_preview() {
        assert_eq!(detail_scale([1800, 1200], [1800, 1200]), 1.0, "a small photo is its own preview");
        assert_eq!(detail_scale([6240, 4160], [6240, 4160]), 6240.0 / 2560.0, "full size");
        assert_eq!(detail_scale([4160, 6240], [1365, 2048]), 0.8, "portrait, exported smaller than the preview");
    }
}
