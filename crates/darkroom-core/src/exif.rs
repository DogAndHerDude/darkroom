//! The EXIF fields the library shows, read from a photo's header.

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;

use exif::{In, Tag, Value};

/// Shooting details, each present only if the file has it. Display strings follow the design
/// system's number style: ƒ/2.8, 1/250 s, ISO 400, 23 mm.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exif {
    /// "YYYY:MM:DD HH:MM:SS", as stored; sorts as text.
    pub taken: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub aperture: Option<String>,
    pub shutter: Option<String>,
    pub iso: Option<String>,
    pub focal_length: Option<String>,
}

impl Exif {
    /// The photo's EXIF, or an empty one when it has none or can't be read.
    pub fn read(path: &Path) -> Exif {
        // A RAF's EXIF is in the JPEG the camera embedded.
        let read = if crate::raf::is_raf(path) {
            crate::raf::embedded_jpeg(path).and_then(|jpeg| exif::Reader::new().read_from_container(&mut Cursor::new(jpeg)).ok())
        } else {
            File::open(path).ok().and_then(|file| exif::Reader::new().read_from_container(&mut BufReader::new(file)).ok())
        };
        let Some(data) = read else { return Exif::default() };
        let field = |tag| data.get_field(tag, In::PRIMARY).map(|f| &f.value);
        let text = |tag| field(tag).and_then(ascii);
        let make = text(Tag::Make);
        let model = text(Tag::Model);
        let camera = match (make, model) {
            // "FUJIFILM" + "X-T5" reads as "Fujifilm X-T5"; skip the make when the model has it.
            (Some(make), Some(model)) if !model.to_lowercase().starts_with(&make.to_lowercase()) => Some(format!("{} {model}", title_case(&make))),
            (_, Some(model)) => Some(model),
            (make, None) => make,
        };
        Exif {
            taken: text(Tag::DateTimeOriginal).or_else(|| text(Tag::DateTime)),
            camera,
            lens: text(Tag::LensModel),
            aperture: field(Tag::FNumber).and_then(rational).map(|f| format!("ƒ/{}", trim(f, 1))),
            shutter: field(Tag::ExposureTime).and_then(rational).map(shutter),
            iso: field(Tag::PhotographicSensitivity).and_then(|v| v.get_uint(0)).map(|n| format!("ISO {n}")),
            focal_length: field(Tag::FocalLength).and_then(rational).map(|f| format!("{} mm", trim(f, 0))),
        }
    }

    /// "ƒ/2.8 · 1/250 s · ISO 400 · 23 mm", with whatever is present.
    pub fn exposure_line(&self) -> String {
        [&self.aperture, &self.shutter, &self.iso, &self.focal_length].iter().filter_map(|v| v.as_deref()).collect::<Vec<_>>().join(" · ")
    }
}

fn ascii(v: &Value) -> Option<String> {
    match v {
        Value::Ascii(parts) => {
            let s = parts.iter().map(|p| String::from_utf8_lossy(p).trim_end_matches('\0').trim().to_owned()).collect::<Vec<_>>().join(" ");
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

fn rational(v: &Value) -> Option<f64> {
    match v {
        Value::Rational(r) if !r.is_empty() && r[0].denom != 0 => Some(r[0].to_f64()),
        _ => None,
    }
}

/// `v` with at most `places` decimals and no trailing zeros: 2.8, 23.
fn trim(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

fn shutter(seconds: f64) -> String {
    if seconds >= 0.3 { format!("{} s", trim(seconds, 1)) } else { format!("1/{} s", (1.0 / seconds).round()) }
}

fn title_case(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut chars = lower.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_the_way_the_design_system_writes_them() {
        assert_eq!(shutter(1.0 / 250.0), "1/250 s");
        assert_eq!(shutter(2.0), "2 s");
        assert_eq!(trim(2.8, 1), "2.8");
        assert_eq!(trim(23.0, 0), "23");
        assert_eq!(title_case("FUJIFILM"), "Fujifilm");
    }

    #[test]
    fn the_exposure_line_joins_what_is_present() {
        let e = Exif { aperture: Some("ƒ/2.8".into()), iso: Some("ISO 400".into()), ..Exif::default() };
        assert_eq!(e.exposure_line(), "ƒ/2.8 · ISO 400");
    }

    #[test]
    fn a_file_without_exif_reads_as_empty() {
        assert_eq!(Exif::read(Path::new("/nonexistent.jpg")), Exif::default());
    }
}
