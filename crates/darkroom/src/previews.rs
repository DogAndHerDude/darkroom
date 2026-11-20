//! Each RAF's developed preview, kept on disk (`~/Library/Caches/darkroom/previews`), so a RAF
//! opened before opens in a moment instead of being developed again for several seconds.
//!
//! A preview is stored as the half floats Develop works on, not re-encoded, so a cached one is
//! bit for bit what developing gives. That makes each about 26 MB for a 26 MP photo, so the
//! folder is kept under [`LIMIT`], the least recently opened going first.
//!
//! A preview is keyed by the photo's path, size and modification time, as a thumbnail is, and
//! by the app's version and [`FORMAT`]. A change to how a RAF is developed must bump `FORMAT`,
//! or previews cached before it go on being shown until the next release.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAGIC: &[u8; 4] = b"DRPV";
/// Bump when a RAF develops differently, so previews cached before aren't used.
const FORMAT: u32 = 2;
/// Magic, format, full size and size.
const HEADER: usize = 24;
/// The most the previews may take on disk: about 80 RAFs from a 26 MP camera. A guess.
const LIMIT: u64 = 2 << 30;

/// A developed preview: linear half floats, RGBA.
pub struct Preview {
    pub full_size: [u32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u16>,
}

fn dir() -> PathBuf {
    crate::jobs::caches().join("previews")
}

/// Where the photo's preview is cached. None when the photo can't be read.
pub fn path(photo: &Path) -> Option<PathBuf> {
    let mut hash = crate::jobs::file_hash(photo)?;
    hash.update(env!("CARGO_PKG_VERSION").as_bytes());
    hash.update(&FORMAT.to_le_bytes());
    Some(dir().join(format!("{}.preview", &hash.finalize().to_hex()[..32])))
}

/// The preview cached at `file`, if there is one and it is whole.
pub fn read(file: &Path) -> Option<Preview> {
    let bytes = fs::read(file).ok()?;
    let (header, body) = bytes.split_at_checked(HEADER)?;
    let word = |i: usize| header[i..i + 4].try_into().map_or(0, u32::from_le_bytes);
    if &header[..4] != MAGIC || word(4) != FORMAT {
        return None;
    }
    let (full_size, size) = ([word(8), word(12)], [word(16), word(20)]);
    if body.len() as u64 != size[0] as u64 * size[1] as u64 * 6 {
        return None;
    }
    let one = half::f16::ONE.to_bits();
    let mut rgba = Vec::with_capacity(body.len() / 6 * 4);
    for p in body.as_chunks::<6>().0 {
        rgba.extend_from_slice(&[u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]), u16::from_le_bytes([p[4], p[5]]), one]);
    }
    // Opening counts as use, for the order previews are let go in.
    let _ = File::options().write(true).open(file).and_then(|f| f.set_modified(SystemTime::now()));
    Some(Preview { full_size, size, rgba })
}

/// Caches a preview at `file`, then makes room under [`LIMIT`]. Failing to is only slower.
pub fn write(file: &Path, preview: &Preview) {
    let mut bytes = Vec::with_capacity(HEADER + preview.rgba.len() / 4 * 6);
    bytes.extend_from_slice(MAGIC);
    for word in [FORMAT, preview.full_size[0], preview.full_size[1], preview.size[0], preview.size[1]] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for p in preview.rgba.as_chunks::<4>().0 {
        for c in &p[..3] {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    let Some(folder) = file.parent() else { return };
    let partial = file.with_extension("partial");
    let written = fs::create_dir_all(folder).and_then(|()| fs::write(&partial, &bytes)).and_then(|()| fs::rename(&partial, file));
    if written.is_err() {
        let _ = fs::remove_file(&partial);
        return;
    }
    prune(folder, LIMIT);
}

/// Removes the least recently used previews in `folder` until they take at most `limit` bytes.
fn prune(folder: &Path, limit: u64) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    let mut previews: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "preview"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = previews.iter().map(|p| p.1).sum();
    previews.sort();
    for (_, len, path) in previews {
        if total <= limit {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("darkroom-previews-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_cached_preview_reads_back_bit_for_bit() {
        let dir = scratch("roundtrip");
        let one = half::f16::ONE.to_bits();
        let rgba: Vec<u16> = (0..3 * 2).flat_map(|i| [i * 7, 0x7bff - i, half::f16::from_f32(1.5).to_bits(), one]).collect();
        let preview = Preview { full_size: [6240, 4160], size: [3, 2], rgba };
        let file = dir.join("a.preview");
        write(&file, &preview);
        let back = read(&file).expect("the preview reads back");
        assert_eq!((back.full_size, back.size), (preview.full_size, preview.size));
        assert_eq!(back.rgba, preview.rgba);
        assert!(!dir.join("a.partial").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cut_short_or_foreign_file_is_not_a_preview() {
        let dir = scratch("broken");
        let file = dir.join("a.preview");
        write(&file, &Preview { full_size: [4, 4], size: [2, 2], rgba: vec![0; 16] });
        let bytes = fs::read(&file).unwrap();
        fs::write(&file, &bytes[..bytes.len() - 1]).unwrap();
        assert!(read(&file).is_none(), "cut short");
        let mut other = bytes.clone();
        other[4] = FORMAT as u8 + 1;
        fs::write(&file, &other).unwrap();
        assert!(read(&file).is_none(), "another format");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_least_recently_used_previews_go_first() {
        let dir = scratch("prune");
        let now = SystemTime::now();
        for (name, age) in [("old", 30), ("middle", 20), ("new", 10)] {
            let file = dir.join(format!("{name}.preview"));
            fs::write(&file, [0u8; 100]).unwrap();
            File::options().write(true).open(&file).unwrap().set_modified(now - Duration::from_secs(age)).unwrap();
        }
        fs::write(dir.join("other.jpg"), [0u8; 1000]).unwrap();
        prune(&dir, 200);
        assert!(!dir.join("old.preview").exists());
        assert!(dir.join("middle.preview").exists() && dir.join("new.preview").exists());
        assert!(dir.join("other.jpg").exists(), "only previews count");
        let _ = fs::remove_dir_all(&dir);
    }
}
