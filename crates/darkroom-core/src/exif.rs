//! The EXIF fields the library shows, read from a photo's header.

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;

use exif::{In, Tag, Value};

use crate::fuji::Look;

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
    /// The film simulation and its settings, from a Fujifilm camera.
    pub look: Option<Look>,
}

/// Shooting details as numbers, for a source other than an EXIF block (a raw file's metadata,
/// read by the app's decoder). [`Exif::from_parts`] formats them as [`Exif::read`] would.
#[derive(Clone, Debug, Default)]
pub struct Parts {
    pub taken: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub f_number: Option<f64>,
    /// Seconds.
    pub exposure_time: Option<f64>,
    pub iso: Option<u32>,
    /// Millimetres.
    pub focal_length: Option<f64>,
}

impl Exif {
    pub fn from_parts(p: Parts) -> Exif {
        let text = |s: Option<String>| s.map(|s| s.trim_end_matches('\0').trim().to_owned()).filter(|s| !s.is_empty());
        let positive = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0);
        Exif {
            taken: text(p.taken),
            camera: camera_name(text(p.make), text(p.model)),
            lens: text(p.lens),
            aperture: positive(p.f_number).map(|f| format!("ƒ/{}", trim(f, 1))),
            shutter: positive(p.exposure_time).map(shutter),
            iso: p.iso.filter(|&n| n > 0).map(|n| format!("ISO {n}")),
            focal_length: positive(p.focal_length).map(|f| format!("{} mm", trim(f, 0))),
            look: None,
        }
    }

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
        let camera = camera_name(text(Tag::Make), text(Tag::Model));
        Exif {
            taken: text(Tag::DateTimeOriginal).or_else(|| text(Tag::DateTime)),
            camera,
            lens: text(Tag::LensModel),
            aperture: field(Tag::FNumber).and_then(rational).map(|f| format!("ƒ/{}", trim(f, 1))),
            shutter: field(Tag::ExposureTime).and_then(rational).map(shutter),
            iso: field(Tag::PhotographicSensitivity).and_then(|v| v.get_uint(0)).map(|n| format!("ISO {n}")),
            focal_length: field(Tag::FocalLength).and_then(rational).map(|f| format!("{} mm", trim(f, 0))),
            look: field(Tag::MakerNote).and_then(|v| if let Value::Undefined(note, _) = v { Look::read(note) } else { None }),
        }
    }

    /// "ƒ/2.8 · 1/250 s · ISO 400 · 23 mm", with whatever is present.
    pub fn exposure_line(&self) -> String {
        [&self.aperture, &self.shutter, &self.iso, &self.focal_length].iter().filter_map(|v| v.as_deref()).collect::<Vec<_>>().join(" · ")
    }
}

/// "FUJIFILM" and "X-T5" read as "Fujifilm X-T5"; the make is left out when the model starts
/// with it ("Canon" and "Canon EOS R5").
fn camera_name(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) if !model.to_lowercase().starts_with(&make.to_lowercase()) => Some(format!("{} {model}", title_case(&make))),
        (_, Some(model)) => Some(model),
        (make, None) => make,
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
    fn parts_read_as_an_exif_block_would() {
        let p = Parts {
            make: Some("SONY".into()),
            model: Some("ILCE-7RM5".into()),
            f_number: Some(2.8),
            exposure_time: Some(1.0 / 250.0),
            iso: Some(400),
            focal_length: Some(35.0),
            ..Parts::default()
        };
        let e = Exif::from_parts(p);
        assert_eq!(e.camera.as_deref(), Some("Sony ILCE-7RM5"));
        assert_eq!(e.exposure_line(), "ƒ/2.8 · 1/250 s · ISO 400 · 35 mm");
        let canon = Exif::from_parts(Parts { make: Some("Canon".into()), model: Some("Canon EOS R5".into()), f_number: Some(0.0), ..Parts::default() });
        assert_eq!((canon.camera.as_deref(), canon.aperture), (Some("Canon EOS R5"), None), "no make twice; an unknown aperture is left out");
    }

    #[test]
    fn a_file_without_exif_reads_as_empty() {
        assert_eq!(Exif::read(Path::new("/nonexistent.jpg")), Exif::default());
    }
}
