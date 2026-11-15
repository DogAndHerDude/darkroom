//! Presets: named looks, one TOML file each in a folder of their own
//! (`Warm matte.toml`), in the edit file's format.
//!
//! A preset holds what a paste takes: Light, Color, Curves and Detail. Applying one is a paste,
//! so the photo keeps its own crop, rating and flag. The file's name is the preset's name, so a
//! name that couldn't be a file name is refused.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Edit;

pub const EXTENSION: &str = "toml";

#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    /// Only Light, Color, Curves and Detail are set; the rest is neutral.
    pub look: Edit,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The file exists but isn't a valid preset; it is left untouched.
    Parse(toml::de::Error),
    Write(toml::ser::Error),
    /// The name can't be used: empty, not a file name, or taken.
    Name(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Parse(e) => write!(f, "the preset file isn't valid: {e}"),
            Error::Write(e) => write!(f, "couldn't write the preset: {e}"),
            Error::Name(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

fn path_for(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{EXTENSION}"))
}

/// The presets in `dir`, sorted by name, and a message for each file that couldn't be read. A
/// missing folder is no presets.
pub fn load_all(dir: &Path) -> (Vec<Preset>, Vec<String>) {
    let (mut presets, mut failed) = (Vec::new(), Vec::new());
    let Ok(files) = fs::read_dir(dir) else { return (presets, failed) };
    for path in files.flatten().map(|f| f.path()) {
        if path.extension().is_none_or(|e| e != EXTENSION) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|n| n.to_str()).filter(|n| !n.starts_with('.')) else { continue };
        match fs::read_to_string(&path).map_err(Error::Io).and_then(|text| toml::from_str::<Edit>(&text).map_err(Error::Parse)) {
            Ok(edit) => presets.push(Preset { name: name.to_owned(), look: edit.look() }),
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    presets.sort_by_key(|p| p.name.to_lowercase());
    (presets, failed)
}

/// `typed` as a preset name: trimmed, a valid file name, and not already one of `existing`'s.
/// Names differing only in case count as taken, as they do on the Mac's disks.
pub fn check_name(typed: &str, existing: &[Preset]) -> Result<String, Error> {
    let name = typed.trim();
    if name.is_empty() {
        return Err(Error::Name("Give the preset a name".into()));
    }
    if name.contains(['/', ':']) || name.starts_with('.') {
        return Err(Error::Name("A preset name can't contain / or : or start with a dot".into()));
    }
    if let Some(taken) = existing.iter().find(|p| p.name.to_lowercase() == name.to_lowercase()) {
        return Err(Error::Name(format!("There is already a preset called “{}”", taken.name)));
    }
    Ok(name.to_owned())
}

/// Saves `edit`'s look in `dir` as a new preset called `typed`. Never replaces a file: a name
/// whose file exists is refused even if it wasn't in `existing`.
pub fn save(dir: &Path, typed: &str, edit: &Edit, existing: &[Preset]) -> Result<Preset, Error> {
    let name = check_name(typed, existing)?;
    let path = path_for(dir, &name);
    if path.exists() {
        return Err(Error::Name(format!("There is already a preset file called “{name}”")));
    }
    let preset = Preset { name, look: edit.look() };
    let text = toml::to_string(&preset.look).map_err(Error::Write)?;
    fs::create_dir_all(dir).map_err(Error::Io)?;
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, text).map_err(Error::Io)?;
    fs::rename(&tmp, &path).map_err(Error::Io)?;
    Ok(preset)
}

/// Removes the preset called `name` from `dir`.
pub fn delete(dir: &Path, name: &str) -> Result<(), Error> {
    fs::remove_file(path_for(dir, name)).map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Flag;

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("darkroom-preset-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn warm() -> Edit {
        let mut e = Edit { rating: 4, flag: Flag::Pick, ..Edit::default() };
        (e.light.exposure, e.color.temperature, e.detail.vignette) = (0.3, 25.0, 40.0);
        e.curves.blue.insert([0.5, 0.45]);
        e.geometry.crop = Some([0.1, 0.1, 0.8, 0.8]);
        e
    }

    #[test]
    fn a_preset_keeps_the_look_and_nothing_of_the_photo() {
        let dir = scratch("look");
        let saved = save(&dir, "  Warm  ", &warm(), &[]).unwrap();
        assert_eq!(saved.name, "Warm");
        let (presets, failed) = load_all(&dir);
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(presets, vec![saved]);
        let look = &presets[0].look;
        assert_eq!((&look.light, &look.color, &look.curves, &look.detail), (&warm().light, &warm().color, &warm().curves, &warm().detail));
        assert_eq!((look.rating, look.flag, &look.geometry), (0, Flag::None, &Default::default()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn presets_never_overwrite() {
        let dir = scratch("overwrite");
        let first = save(&dir, "Warm", &warm(), &[]).unwrap();
        assert!(matches!(save(&dir, "warm", &Edit::default(), std::slice::from_ref(&first)), Err(Error::Name(_))));
        // Taken on disk though not in the list given.
        assert!(matches!(save(&dir, "Warm", &Edit::default(), &[]), Err(Error::Name(_))));
        assert_eq!(load_all(&dir).0, vec![first]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_that_are_not_file_names_are_refused() {
        for name in ["", "   ", "a/b", "a:b", ".hidden"] {
            assert!(matches!(check_name(name, &[]), Err(Error::Name(_))), "{name:?}");
        }
        assert_eq!(check_name("Fade · 2", &[]).unwrap(), "Fade · 2");
    }

    #[test]
    fn presets_list_by_name_and_a_broken_file_is_reported_not_lost() {
        let dir = scratch("list");
        for name in ["matte", "Bright", "cool"] {
            save(&dir, name, &warm(), &[]).unwrap();
        }
        fs::write(dir.join("Broken.toml"), "[light\nexposure = ").unwrap();
        fs::write(dir.join("notes.txt"), "not a preset").unwrap();
        let (presets, failed) = load_all(&dir);
        assert_eq!(presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Bright", "cool", "matte"]);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].starts_with("Broken:"), "{failed:?}");
        assert!(dir.join("Broken.toml").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deleting_removes_only_that_preset() {
        let dir = scratch("delete");
        save(&dir, "Warm", &warm(), &[]).unwrap();
        let cool = save(&dir, "Cool", &warm(), &[]).unwrap();
        delete(&dir, "Warm").unwrap();
        assert_eq!(load_all(&dir).0, vec![cool]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_folder_is_no_presets() {
        let (presets, failed) = load_all(&scratch("missing"));
        assert!(presets.is_empty() && failed.is_empty());
    }
}
