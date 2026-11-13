//! The edit file next to each photo: `DSCF4127.JPG` → `DSCF4127.JPG.darkroom`, TOML inside.
//!
//! Keeping the photo's full name (extension included) means a JPEG and RAF of the same shot keep
//! separate edits. Writes go to a temporary file first and are renamed into place, so a crash
//! mid-save never leaves a half-written edit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Edit;

pub const SUFFIX: &str = ".darkroom";

/// Where `photo`'s edits live.
pub fn path_for(photo: &Path) -> PathBuf {
    let mut name = photo.as_os_str().to_owned();
    name.push(SUFFIX);
    PathBuf::from(name)
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The file exists but isn't a valid edit; it is left untouched.
    Parse(toml::de::Error),
    Write(toml::ser::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Parse(e) => write!(f, "the edit file isn't valid: {e}"),
            Error::Write(e) => write!(f, "couldn't write the edit: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// `photo`'s edit, or a fresh one when it has no edit file yet.
pub fn load(photo: &Path) -> Result<Edit, Error> {
    match fs::read_to_string(path_for(photo)) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Edit::default()),
        Err(e) => Err(Error::Io(e)),
    }
}

pub fn parse(text: &str) -> Result<Edit, Error> {
    toml::from_str(text).map_err(Error::Parse)
}

pub fn to_string(edit: &Edit) -> Result<String, Error> {
    let edit = Edit { version: crate::edit::VERSION, ..edit.clone() };
    toml::to_string(&edit).map_err(Error::Write)
}

/// Saves `edit` beside `photo`. An edit with no adjustments, rating or flag removes the file
/// instead, so untouched photos leave nothing behind.
pub fn save(photo: &Path, edit: &Edit) -> Result<(), Error> {
    let path = path_for(photo);
    if (Edit { version: crate::edit::VERSION, ..edit.clone() }) == Edit::default() {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(Error::Io(e)),
            _ => Ok(()),
        };
    }
    let text = to_string(edit)?;
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, text).map_err(Error::Io)?;
    fs::rename(&tmp, &path).map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Curve, Flag};

    #[test]
    fn the_edit_file_sits_beside_the_photo_with_its_full_name() {
        assert_eq!(path_for(Path::new("/p/DSCF4127.JPG")), PathBuf::from("/p/DSCF4127.JPG.darkroom"));
    }

    #[test]
    fn an_edit_round_trips_through_toml() {
        let mut e = Edit { rating: 4, flag: Flag::Pick, ..Edit::default() };
        e.light.exposure = 0.35;
        e.light.black_point = 8.0;
        e.color.vibrance = 24.0;
        e.curves.rgb = Curve::new(vec![[0.0, 0.04], [0.25, 0.21], [0.75, 0.79], [1.0, 0.97]]);
        e.geometry.crop = Some([0.08, 0.08, 0.84, 0.84]);
        let text = to_string(&e).unwrap();
        assert_eq!(parse(&text).unwrap(), e, "{text}");
    }

    #[test]
    fn a_sparse_file_fills_in_the_rest() {
        let e = parse("rating = 2\n[light]\nexposure = 0.5\n").unwrap();
        assert_eq!(e.rating, 2);
        assert_eq!(e.light.exposure, 0.5);
        assert!(e.light.enabled && e.color.enabled, "sections default to on");
        assert!(e.curves.is_identity());
    }

    #[test]
    fn a_broken_file_is_an_error_not_a_reset() {
        assert!(matches!(parse("[light\nexposure = "), Err(Error::Parse(_))));
    }

    #[test]
    fn saving_writes_and_loading_reads_back_and_a_neutral_edit_removes_the_file() {
        let dir = std::env::temp_dir().join(format!("darkroom-sidecar-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("DSCF0001.JPG");
        let mut e = Edit::default();
        e.color.saturation = -20.0;
        save(&photo, &e).unwrap();
        assert_eq!(load(&photo).unwrap(), e);

        save(&photo, &Edit::default()).unwrap();
        assert!(!path_for(&photo).exists());
        assert_eq!(load(&photo).unwrap(), Edit::default(), "no file reads as a fresh edit");
        fs::remove_dir_all(&dir).unwrap();
    }
}
