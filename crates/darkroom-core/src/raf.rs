//! Fujifilm RAF, as far as the library needs it: every RAF carries a full-size JPEG the camera
//! made, with the EXIF. Thumbnails and shooting details come from that; developing the sensor
//! data itself is the app's job.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MAGIC: &[u8; 16] = b"FUJIFILMCCD-RAW ";
/// Where the header keeps the embedded JPEG's offset and length (big-endian u32 each).
const JPEG_OFFSET: u64 = 84;

pub fn is_raf(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("raf"))
}

/// The JPEG the camera embedded in the RAF, or None if the file isn't one or is cut short.
pub fn embedded_jpeg(path: &Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let mut magic = [0u8; 16];
    file.read_exact(&mut magic).ok()?;
    if &magic != MAGIC {
        return None;
    }
    file.seek(SeekFrom::Start(JPEG_OFFSET)).ok()?;
    let mut at = [0u8; 8];
    file.read_exact(&mut at).ok()?;
    let offset = u32::from_be_bytes([at[0], at[1], at[2], at[3]]) as u64;
    let length = u32::from_be_bytes([at[4], at[5], at[6], at[7]]) as u64;
    if offset == 0 || length == 0 || offset + length > file.metadata().ok()?.len() {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut jpeg = vec![0u8; length as usize];
    file.read_exact(&mut jpeg).ok()?;
    jpeg.starts_with(&[0xFF, 0xD8]).then_some(jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("darkroom-raf-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn finds_the_embedded_jpeg() {
        let mut raf = MAGIC.to_vec();
        raf.resize(148, 0);
        raf[84..88].copy_from_slice(&148u32.to_be_bytes());
        raf[88..92].copy_from_slice(&4u32.to_be_bytes());
        raf.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
        let path = file("ok.RAF", &raf);
        assert_eq!(embedded_jpeg(&path), Some(vec![0xFF, 0xD8, 0xFF, 0xD9]));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_other_files_and_bad_offsets() {
        let jpeg = file("not.RAF", &[0xFF, 0xD8, 0xFF, 0xD9]);
        assert_eq!(embedded_jpeg(&jpeg), None);
        let mut raf = MAGIC.to_vec();
        raf.resize(148, 0);
        raf[84..88].copy_from_slice(&148u32.to_be_bytes());
        raf[88..92].copy_from_slice(&4000u32.to_be_bytes());
        let cut = file("cut.RAF", &raf);
        assert_eq!(embedded_jpeg(&cut), None);
        std::fs::remove_file(jpeg).unwrap();
        std::fs::remove_file(cut).unwrap();
    }

    #[test]
    fn knows_the_extension() {
        assert!(is_raf(Path::new("DSCF0267.RAF")));
        assert!(is_raf(Path::new("a/b.raf")));
        assert!(!is_raf(Path::new("DSCF0267.JPG")));
    }
}
