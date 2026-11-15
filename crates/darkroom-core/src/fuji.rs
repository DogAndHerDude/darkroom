//! The look a Fujifilm camera was set to when it took a photo, read from its MakerNote.
//!
//! The MakerNote is Fujifilm's own block inside the EXIF: the text `FUJIFILM`, the offset of a
//! tag table, and the table itself, always little-endian, with offsets counted from the start of
//! the block. The tag numbers and their values follow ExifTool's FujiFilm table
//! (https://exiftool.org/TagNames/FujiFilm.html). Only the look is read. A RAF's starting look is
//! fitted from its embedded JPEG (`camera_look`); from here it takes only whether to drop colour.

/// The camera's look settings as the info panel shows them. Each is present only if the file
/// has it and its value is one known here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    /// "Classic Chrome", "Acros + R".
    pub simulation: Option<String>,
    /// "Off", "Weak, small".
    pub grain: Option<String>,
    /// "Off", "Weak", "Strong".
    pub color_chrome: Option<String>,
    /// "DR200".
    pub dynamic_range: Option<String>,
    pub white_balance: Option<String>,
}

const WHITE_BALANCE: u16 = 0x1002;
/// Colour saturation on older cameras; on the X series it names the black-and-white modes.
const SATURATION: u16 = 0x1003;
const GRAIN_ROUGHNESS: u16 = 0x1047;
const COLOR_CHROME: u16 = 0x1048;
const GRAIN_SIZE: u16 = 0x104c;
const FILM_MODE: u16 = 0x1401;
/// 0 when the camera chose the dynamic range itself.
const DYNAMIC_RANGE_SETTING: u16 = 0x1402;
/// The range in percent when it was set by hand.
const DEVELOPMENT_DYNAMIC_RANGE: u16 = 0x1403;
/// The range in percent the camera chose.
const AUTO_DYNAMIC_RANGE: u16 = 0x140b;

impl Look {
    /// The look in a MakerNote's bytes, or None when they aren't Fujifilm's or hold none of it.
    pub fn read(note: &[u8]) -> Option<Look> {
        if !note.starts_with(b"FUJIFILM") {
            return None;
        }
        let value = |tag| value(note, tag);
        // A black-and-white mode is written as a saturation, and the film mode is left out.
        let simulation = value(SATURATION).and_then(monochrome).or_else(|| value(FILM_MODE).and_then(film_mode));
        let grain = value(GRAIN_ROUGHNESS).and_then(|rough| grain(rough, value(GRAIN_SIZE).unwrap_or(0)));
        let range = if value(DYNAMIC_RANGE_SETTING) == Some(0) { value(AUTO_DYNAMIC_RANGE) } else { value(DEVELOPMENT_DYNAMIC_RANGE) };
        let look = Look {
            simulation: simulation.map(Into::into),
            grain,
            color_chrome: value(COLOR_CHROME).and_then(strength).map(Into::into),
            dynamic_range: range.filter(|&r| r > 0).map(|r| format!("DR{r}")),
            white_balance: value(WHITE_BALANCE).and_then(white_balance),
        };
        (look != Look::default()).then_some(look)
    }

    /// Whether the simulation renders in black and white. Sepia is toned, so it isn't.
    pub fn is_black_and_white(&self) -> bool {
        self.simulation.as_deref().is_some_and(|s| s.starts_with("Monochrome") || s.starts_with("Acros"))
    }

    /// Label and value for each setting present, in the info panel's order.
    pub fn rows(&self) -> Vec<(&'static str, &str)> {
        [("Simulation", &self.simulation), ("Grain effect", &self.grain), ("Color chrome", &self.color_chrome), ("Dynamic range", &self.dynamic_range), ("White balance", &self.white_balance)]
            .into_iter()
            .filter_map(|(label, value)| Some((label, value.as_deref()?)))
            .collect()
    }
}

/// The single number stored under `tag`, or None when the tag is missing, holds something else,
/// or the note is cut short.
fn value(note: &[u8], tag: u16) -> Option<i64> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(note.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(note.get(at..at + 4)?.try_into().ok()?));
    let table = u32_at(8)? as usize;
    for i in 0..u16_at(table)? as usize {
        // An entry is 12 bytes: tag, type, count, then the value itself when it fits in four.
        let entry = table + 2 + 12 * i;
        if u16_at(entry)? != tag {
            continue;
        }
        return match (u16_at(entry + 2)?, u32_at(entry + 4)?) {
            (3, 1) => u16_at(entry + 8).map(i64::from),
            (4, 1) => u32_at(entry + 8).map(i64::from),
            (9, 1) => u32_at(entry + 8).map(|v| i64::from(v as i32)),
            _ => None,
        };
    }
    None
}

/// Names as the X series' menus have them. The portrait modes of older cameras aren't listed.
fn film_mode(v: i64) -> Option<&'static str> {
    Some(match v {
        0x000 => "Provia",
        0x120 => "Astia",
        0x200 | 0x400 => "Velvia",
        0x500 => "Pro Neg. Std",
        0x501 => "Pro Neg. Hi",
        0x600 => "Classic Chrome",
        0x700 => "Eterna",
        0x800 => "Classic Neg.",
        0x900 => "Eterna Bleach Bypass",
        0xa00 => "Nostalgic Neg.",
        0xb00 => "Reala Ace",
        _ => return None,
    })
}

fn monochrome(v: i64) -> Option<&'static str> {
    Some(match v {
        0x300 => "Monochrome",
        0x301 => "Monochrome + R",
        0x302 => "Monochrome + Ye",
        0x303 => "Monochrome + G",
        0x310 => "Sepia",
        0x500 => "Acros",
        0x501 => "Acros + R",
        0x502 => "Acros + Ye",
        0x503 => "Acros + G",
        _ => return None,
    })
}

fn strength(v: i64) -> Option<&'static str> {
    Some(match v {
        0 => "Off",
        32 => "Weak",
        64 => "Strong",
        _ => return None,
    })
}

/// "Weak, small" from the grain's roughness and size. Cameras without a size setting give the
/// roughness alone; the X half writes both into the roughness (16 and 48 are the small grains).
fn grain(roughness: i64, size: i64) -> Option<String> {
    let (strength, small) = match roughness {
        0 => return Some("Off".into()),
        16 => ("Weak", true),
        32 => ("Weak", false),
        48 => ("Strong", true),
        64 => ("Strong", false),
        _ => return None,
    };
    Some(match (small, size) {
        (true, _) | (_, 16) => format!("{strength}, small"),
        (_, 32) => format!("{strength}, large"),
        _ => strength.into(),
    })
}

fn white_balance(v: i64) -> Option<String> {
    Some(match v {
        0x0 => "Auto".into(),
        0x1 => "Auto, white priority".into(),
        0x2 => "Auto, ambience priority".into(),
        0x100 => "Daylight".into(),
        0x200 => "Shade".into(),
        0x300..=0x302 => format!("Fluorescent {}", v - 0x2ff),
        0x400 => "Incandescent".into(),
        0x500 => "Flash".into(),
        0x600 => "Underwater".into(),
        0xf00..=0xf04 => format!("Custom {}", v - 0xeff),
        0xff0 => "Kelvin".into(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A MakerNote holding `tags`, each (tag, type, value): type 3 is a u16, 9 an i32.
    fn note(tags: &[(u16, u16, i32)]) -> Vec<u8> {
        let mut note = b"FUJIFILM".to_vec();
        note.extend(12u32.to_le_bytes());
        note.extend((tags.len() as u16).to_le_bytes());
        for &(tag, kind, value) in tags {
            note.extend(tag.to_le_bytes());
            note.extend(kind.to_le_bytes());
            note.extend(1u32.to_le_bytes());
            note.extend(value.to_le_bytes());
        }
        note
    }

    #[test]
    fn a_look_reads_as_the_info_panel_writes_it() {
        let bytes = note(&[(WHITE_BALANCE, 3, 0), (SATURATION, 3, 0), (GRAIN_ROUGHNESS, 9, 32), (COLOR_CHROME, 9, 64), (GRAIN_SIZE, 3, 16), (FILM_MODE, 3, 0x600), (DYNAMIC_RANGE_SETTING, 3, 0), (DEVELOPMENT_DYNAMIC_RANGE, 3, 100), (AUTO_DYNAMIC_RANGE, 3, 200)]);
        let look = Look::read(&bytes).unwrap();
        assert_eq!(look.rows(), [("Simulation", "Classic Chrome"), ("Grain effect", "Weak, small"), ("Color chrome", "Strong"), ("Dynamic range", "DR200"), ("White balance", "Auto")]);
    }

    #[test]
    fn a_black_and_white_mode_is_the_simulation() {
        let look = Look::read(&note(&[(SATURATION, 3, 0x501), (DYNAMIC_RANGE_SETTING, 3, 1), (DEVELOPMENT_DYNAMIC_RANGE, 3, 400), (AUTO_DYNAMIC_RANGE, 3, 100)])).unwrap();
        assert_eq!(look.rows(), [("Simulation", "Acros + R"), ("Dynamic range", "DR400")]);
        assert!(look.is_black_and_white());
        assert!(Look::read(&note(&[(SATURATION, 3, 0x302)])).unwrap().is_black_and_white());
        assert!(!Look::read(&note(&[(SATURATION, 3, 0x310)])).unwrap().is_black_and_white(), "sepia is toned");
        assert!(!Look::read(&note(&[(FILM_MODE, 3, 0x600)])).unwrap().is_black_and_white());
    }

    #[test]
    fn grain_is_named_with_or_without_a_size() {
        assert_eq!(grain(0, 0).as_deref(), Some("Off"));
        assert_eq!(grain(64, 32).as_deref(), Some("Strong, large"));
        assert_eq!(grain(32, 0).as_deref(), Some("Weak"));
        assert_eq!(grain(48, 0).as_deref(), Some("Strong, small"));
        assert_eq!(grain(7, 16), None);
    }

    #[test]
    fn unknown_values_and_other_makers_show_nothing() {
        assert_eq!(Look::read(&note(&[(FILM_MODE, 3, 0x7777), (WHITE_BALANCE, 3, 0x999)])), None);
        assert_eq!(Look::read(b"Nikon\0\x02\x10\0\0MM\0\x2a\0\0\0\x08"), None);
        assert_eq!(Look::read(b""), None);
    }

    #[test]
    fn a_note_cut_short_reads_what_is_whole() {
        let whole = note(&[(FILM_MODE, 3, 0x800), (WHITE_BALANCE, 3, 0x100)]);
        // The table says two entries; the second is missing its last bytes.
        let look = Look::read(&whole[..whole.len() - 3]).unwrap();
        assert_eq!(look.rows(), [("Simulation", "Classic Neg.")]);
        assert_eq!(Look::read(&whole[..10]), None);
    }
}
