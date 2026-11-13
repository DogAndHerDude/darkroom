//! Work that mustn't block a frame: scanning folders, reading a folder's photos (EXIF and edit
//! files), making thumbnails, and opening a photo for Develop. Each runs on a background thread
//! and leaves a [`Msg`] in the inbox, then wakes the window; `poll` takes the messages.
//!
//! Thumbnails are cached on disk (`~/Library/Caches/darkroom/thumbs`), keyed by the photo's
//! path, size and modification time, so a folder opens instantly the second time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use darkroom_core::exif::Exif;
use darkroom_core::library::{self, Entry, Folder};
use darkroom_core::{Edit, sidecar};
use image::{DynamicImage, ImageDecoder, ImageReader};
use rune::Waker;

use crate::photo::Photo;

/// The long edge of a thumbnail, in pixels: sharp at the largest grid size on a Retina screen.
const THUMB_EDGE: u32 = 448;

pub enum Msg {
    Tree(Folder),
    Folder { generation: u64, folder: PathBuf, entries: Vec<Entry> },
    Thumb { path: PathBuf, width: u32, height: u32, rgba: Vec<u8> },
    Opened { path: PathBuf, result: Result<Box<(Photo, Edit)>, String> },
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
        Jobs { inbox, waker, thumbs, thumb_generation, folder_generation: 0 }
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
            let entries = library::photos_in(&folder).into_iter().map(|path| entry(&path)).collect();
            Msg::Folder { generation, folder, entries }
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

    /// Opens a photo for Develop: decodes its preview and reads its edit.
    pub fn open(&self, path: PathBuf) {
        self.spawn("open", move || {
            let result = Photo::open(&path.to_string_lossy()).map_err(|e| e.to_string()).and_then(|photo| {
                let edit = sidecar::load(&path).map_err(|e| format!("its edit file is unreadable: {e}"))?;
                Ok(Box::new((photo, edit)))
            });
            Msg::Opened { path, result }
        });
    }
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
    }
}

fn thumb_worker(receive: &Mutex<Receiver<ThumbJob>>, inbox: &Mutex<Vec<Msg>>, waker: &Mutex<Option<Waker>>, current: &AtomicU64) {
    loop {
        let Ok(job) = receive.lock().map_err(|_| ()).and_then(|r| r.recv().map_err(|_| ())) else { return };
        if job.generation != current.load(Ordering::Acquire) {
            continue;
        }
        if let Some((width, height, rgba)) = thumbnail(&job.path) {
            deliver(inbox, waker, Msg::Thumb { path: job.path, width, height, rgba });
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

/// The photo's thumbnail as straight sRGB RGBA: from the disk cache, or made from the photo
/// (EXIF orientation applied) and cached.
fn thumbnail(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let cached = cache_path(path);
    if let Some(img) = cached.as_ref().and_then(|c| image::open(c).ok()) {
        let rgba = img.into_rgba8();
        return Some((rgba.width(), rgba.height(), rgba.into_raw()));
    }
    let mut decoder = ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    img.apply_orientation(orientation);
    let small = img.thumbnail(THUMB_EDGE, THUMB_EDGE).into_rgb8();
    if let Some(c) = cached {
        let _ = std::fs::create_dir_all(cache_dir());
        let _ = small.save_with_format(&c, image::ImageFormat::Jpeg);
    }
    let rgba = DynamicImage::ImageRgb8(small).into_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}
