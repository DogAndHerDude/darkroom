//! The library: a root folder on disk, the folders under it that hold photos, and the photos in
//! one folder, filtered and sorted. There's no catalog; the folders are the library.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Flag;
use crate::exif::Exif;

/// File types the library lists (lowercase).
pub const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "raf"];

pub fn is_photo(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| PHOTO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn hidden(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'))
}

/// A folder in the tree: its photos (directly in it) and the folders under it that hold photos.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub path: PathBuf,
    pub name: String,
    pub photos: usize,
    pub children: Vec<Folder>,
}

impl Folder {
    /// This folder and every folder under it, depth first, with their depth (this one is 0).
    pub fn flatten(&self) -> Vec<(usize, &Folder)> {
        let mut out = Vec::new();
        fn walk<'a>(f: &'a Folder, depth: usize, out: &mut Vec<(usize, &'a Folder)>) {
            out.push((depth, f));
            for c in &f.children {
                walk(c, depth + 1, out);
            }
        }
        walk(self, 0, &mut out);
        out
    }
}

/// The folder tree under `root`, keeping only folders with photos in them or below them. Hidden
/// folders are skipped. Folders are sorted by name.
pub fn scan(root: &Path) -> Folder {
    let mut photos = 0;
    let mut dirs = Vec::new();
    if let Ok(read) = fs::read_dir(root) {
        for entry in read.flatten() {
            let path = entry.path();
            if hidden(&path) {
                continue;
            }
            match entry.file_type() {
                Ok(t) if t.is_dir() => dirs.push(path),
                Ok(_) if is_photo(&path) => photos += 1,
                _ => {}
            }
        }
    }
    dirs.sort();
    let children = dirs.iter().map(|d| scan(d)).filter(|f| f.photos > 0 || !f.children.is_empty()).collect();
    let name = root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
    Folder { path: root.to_owned(), name, photos, children }
}

/// The photos directly in `folder`, sorted by file name.
pub fn photos_in(folder: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(folder)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_file() && is_photo(p) && !hidden(p)).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// One photo as the library shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub modified: Option<SystemTime>,
    pub exif: Exif,
    pub rating: u8,
    pub flag: Flag,
    /// It has adjustments in its edit file.
    pub edited: bool,
}

/// Which photos to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Show {
    #[default]
    All,
    Picks,
    Rejects,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// Capture time from EXIF, falling back to the file's modification time, then the name.
    #[default]
    CaptureTime,
    Name,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub show: Show,
    /// Only photos rated at least this (0 shows all).
    pub min_rating: u8,
}

impl Filter {
    pub fn keeps(&self, e: &Entry) -> bool {
        let shown = match self.show {
            Show::All => true,
            Show::Picks => e.flag == Flag::Pick,
            Show::Rejects => e.flag == Flag::Reject,
        };
        shown && e.rating >= self.min_rating
    }
}

/// Indices into `entries` of the photos `filter` keeps, in `sort` order.
pub fn visible(entries: &[Entry], filter: Filter, sort: Sort) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len()).filter(|&i| filter.keeps(&entries[i])).collect();
    match sort {
        Sort::Name => idx.sort_by(|&a, &b| entries[a].name.cmp(&entries[b].name)),
        Sort::CaptureTime => idx.sort_by(|&a, &b| {
            let (ea, eb) = (&entries[a], &entries[b]);
            // EXIF times are "YYYY:MM:DD HH:MM:SS", so they sort as text.
            match (&ea.exif.taken, &eb.exif.taken) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => ea.modified.cmp(&eb.modified),
            }
            .then_with(|| ea.name.cmp(&eb.name))
        }),
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, taken: Option<&str>, rating: u8, flag: Flag) -> Entry {
        Entry {
            path: PathBuf::from(name),
            name: name.into(),
            modified: None,
            exif: Exif { taken: taken.map(Into::into), ..Exif::default() },
            rating,
            flag,
            edited: false,
        }
    }

    #[test]
    fn photos_are_recognised_by_extension_in_any_case() {
        assert!(is_photo(Path::new("DSCF4127.JPG")) && is_photo(Path::new("a.jpeg")) && is_photo(Path::new("b.png")));
        assert!(!is_photo(Path::new("DSCF4127.JPG.darkroom")) && is_photo(Path::new("DSCF4127.RAF")));
    }

    #[test]
    fn capture_time_sorts_before_undated_photos_then_by_name() {
        let e = [entry("c", None, 0, Flag::None), entry("b", Some("2026:11:09 17:42:00"), 0, Flag::None), entry("a", Some("2026:11:09 09:00:00"), 0, Flag::None)];
        assert_eq!(visible(&e, Filter::default(), Sort::CaptureTime), [2, 1, 0]);
        assert_eq!(visible(&e, Filter::default(), Sort::Name), [2, 1, 0]);
    }

    #[test]
    fn filters_combine_flag_and_minimum_rating() {
        let e = [entry("a", None, 4, Flag::Pick), entry("b", None, 2, Flag::Pick), entry("c", None, 5, Flag::Reject), entry("d", None, 0, Flag::None)];
        let picks = Filter { show: Show::Picks, min_rating: 0 };
        assert_eq!(visible(&e, picks, Sort::Name), [0, 1]);
        assert_eq!(visible(&e, Filter { min_rating: 3, ..picks }, Sort::Name), [0]);
        assert_eq!(visible(&e, Filter { show: Show::Rejects, min_rating: 0 }, Sort::Name), [2]);
        assert_eq!(visible(&e, Filter { show: Show::All, min_rating: 4 }, Sort::Name), [0, 2]);
    }

    #[test]
    fn scanning_keeps_folders_with_photos_and_skips_hidden_ones() {
        let root = std::env::temp_dir().join(format!("darkroom-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for d in ["2026-11 Lisbon", "2026-10 Alps/day 2", "empty", ".hidden"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["2026-11 Lisbon/DSCF0001.JPG", "2026-11 Lisbon/DSCF0001.JPG.darkroom", "2026-10 Alps/day 2/x.jpg", ".hidden/y.jpg"] {
            fs::write(root.join(f), b"").unwrap();
        }
        let tree = scan(&root);
        let names: Vec<(usize, String, usize)> = tree.flatten().iter().map(|(d, f)| (*d, f.name.clone(), f.photos)).collect();
        let root_name = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(names, [(0, root_name, 0), (1, "2026-10 Alps".into(), 0), (2, "day 2".into(), 1), (1, "2026-11 Lisbon".into(), 1)]);
        assert_eq!(photos_in(&root.join("2026-11 Lisbon")).len(), 1, "edit files aren't photos");
        fs::remove_dir_all(&root).unwrap();
    }
}
