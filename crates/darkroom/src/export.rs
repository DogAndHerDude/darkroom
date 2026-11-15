//! Export: settings, the dialog, and writing a rendered photo to a file.
//!
//! The GPU renders each photo at full resolution (or the chosen size) through its whole edit,
//! geometry included (`Develop::export`). This module turns that into a file: the sRGB transfer
//! curve, 8-bit JPEG or PNG or 16-bit TIFF, the colour profile (sRGB or Display P3, from macOS),
//! and, if asked, the camera's shooting details. Location is never written.

use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use darkroom_core::raf;
use darkroom_render::Exported;
use image::{ExtendedColorType, ImageEncoder};
use rune::{El, FileDialogOptions, Tx, col, el, row, spacer, text};
use serde::{Deserialize, Serialize};

use crate::State;
use crate::theme as t;
use crate::ui::{button, primary, segmented};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Jpeg,
    Png,
    /// 16 bits per channel.
    Tiff,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Tiff => "tif",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub format: Format,
    /// JPEG quality, 1–100.
    pub quality: u8,
    /// Longest output edge in pixels; None keeps full size.
    pub long_edge: Option<u32>,
    pub display_p3: bool,
    /// Camera, lens, exposure and capture time. Location is never included.
    pub camera_info: bool,
    /// None: `~/Pictures/Darkroom/<folder name>`.
    pub destination: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { format: Format::Jpeg, quality: 90, long_edge: None, display_p3: false, camera_info: true, destination: None }
    }
}

impl Settings {
    /// Where files go: the chosen folder, or one named after the photos' folder.
    pub fn destination_for(&self, folder: Option<&Path>) -> PathBuf {
        self.destination.clone().unwrap_or_else(|| {
            let pictures = std::env::var_os("HOME").map_or_else(std::env::temp_dir, |h| PathBuf::from(h).join("Pictures"));
            let name = folder.and_then(|f| f.file_name()).map_or_else(|| "Export".into(), |n| n.to_string_lossy().into_owned());
            pictures.join("Darkroom").join(name)
        })
    }
}

/// `path` for showing: the home folder as `~`.
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.and_then(|h| path.strip_prefix(h).ok().map(|rest| Path::new("~").join(rest))) {
        Some(short) => short.display().to_string(),
        None => path.display().to_string(),
    }
}

/// The size presets the dialog offers, as long edges.
const SIZES: [(Option<u32>, &str); 4] = [(None, "Full"), (Some(4000), "4000"), (Some(3000), "3000"), (Some(2048), "2048")];

/// Which photos the dialog will export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The open photo (Develop) or the selected one (Library).
    One,
    /// Every photo the grid shows, after filters.
    Shown,
}

/// An export in progress.
pub struct Run {
    pub id: u64,
    pub settings: Settings,
    pub destination: PathBuf,
    pub paths: Vec<PathBuf>,
    /// The next photo to start.
    pub next: usize,
    pub written: Vec<PathBuf>,
    pub failed: Vec<String>,
}

impl Run {
    pub fn finished(&self) -> bool {
        self.written.len() + self.failed.len() == self.paths.len()
    }
}

/// A path in `dir` for `stem` that doesn't exist yet: `stem.ext`, else `stem (2).ext`, …
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).expect("an unused name")
}

/// Half-float bits → sRGB-encoded 8- and 16-bit values, for every possible half.
pub fn tables() -> &'static (Vec<u8>, Vec<u16>) {
    static TABLES: OnceLock<(Vec<u8>, Vec<u16>)> = OnceLock::new();
    TABLES.get_or_init(|| {
        let encode = |bits: u16| {
            let v = half::f16::from_bits(bits).to_f32();
            let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
            if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
        };
        let eight = (0..=u16::MAX).map(|b| (encode(b) * 255.0 + 0.5) as u8).collect();
        let sixteen = (0..=u16::MAX).map(|b| (encode(b) * 65535.0 + 0.5) as u16).collect();
        (eight, sixteen)
    })
}

/// The shooting details to carry over, as an EXIF block (TIFF structure): camera, lens,
/// exposure, focal length and capture time. No location, and no orientation (the pixels are
/// already upright).
fn camera_exif(source: &Path) -> Option<Vec<u8>> {
    use exif::{In, Tag};
    // A RAF keeps its EXIF in the JPEG the camera embedded.
    let data = if raf::is_raf(source) {
        exif::Reader::new().read_from_container(&mut Cursor::new(raf::embedded_jpeg(source)?)).ok()?
    } else {
        exif::Reader::new().read_from_container(&mut BufReader::new(File::open(source).ok()?)).ok()?
    };
    const KEEP: [Tag; 11] = [
        Tag::Make,
        Tag::Model,
        Tag::LensMake,
        Tag::LensModel,
        Tag::DateTimeOriginal,
        Tag::OffsetTimeOriginal,
        Tag::FNumber,
        Tag::ExposureTime,
        Tag::PhotographicSensitivity,
        Tag::FocalLength,
        Tag::ExposureBiasValue,
    ];
    let fields: Vec<&exif::Field> = KEEP.iter().filter_map(|&tag| data.get_field(tag, In::PRIMARY)).collect();
    if fields.is_empty() {
        return None;
    }
    let mut writer = exif::experimental::Writer::new();
    for f in &fields {
        writer.push_field(f);
    }
    let mut out = Cursor::new(Vec::new());
    writer.write(&mut out, false).ok()?;
    Some(out.into_inner())
}

/// The macOS colour profile for the output space, if the system has it.
fn icc_profile(display_p3: bool) -> Option<Vec<u8>> {
    let name = if display_p3 { "Display P3.icc" } else { "sRGB Profile.icc" };
    std::fs::read(Path::new("/System/Library/ColorSync/Profiles").join(name)).ok()
}

/// Writes `image` (rendered from `source`) into `dir` as `settings` say. Returns the new file.
pub fn write(image: &Exported, source: &Path, dir: &Path, settings: &Settings) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    let stem = source.file_stem().map_or_else(|| "photo".into(), |s| s.to_string_lossy().into_owned());
    let path = unique_path(dir, &stem, settings.format.extension());
    // Written beside the target, then renamed, so a failure never leaves half a file.
    let partial = path.with_extension(format!("{}.partial", settings.format.extension()));
    let [w, h] = image.size;
    let (eight, sixteen) = tables();
    let rgb = |c: usize| c % 4 != 3;
    let result = (|| -> image::ImageResult<()> {
        let file = BufWriter::new(File::create(&partial)?);
        let icc = icc_profile(settings.display_p3);
        let exif = if settings.camera_info { camera_exif(source) } else { None };
        match settings.format {
            Format::Jpeg | Format::Png => {
                let pixels: Vec<u8> = image.rgba16f.iter().enumerate().filter(|(i, _)| rgb(*i)).map(|(_, &b)| eight[b as usize]).collect();
                if settings.format == Format::Jpeg {
                    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(file, settings.quality.clamp(1, 100));
                    if let Some(icc) = icc {
                        let _ = enc.set_icc_profile(icc);
                    }
                    if let Some(exif) = exif {
                        let _ = enc.set_exif_metadata(exif);
                    }
                    enc.write_image(&pixels, w, h, ExtendedColorType::Rgb8)
                } else {
                    let mut enc = image::codecs::png::PngEncoder::new(file);
                    if let Some(icc) = icc {
                        let _ = enc.set_icc_profile(icc);
                    }
                    if let Some(exif) = exif {
                        let _ = enc.set_exif_metadata(exif);
                    }
                    enc.write_image(&pixels, w, h, ExtendedColorType::Rgb8)
                }
            }
            Format::Tiff => {
                let pixels: Vec<u8> = image.rgba16f.iter().enumerate().filter(|(i, _)| rgb(*i)).flat_map(|(_, &b)| sixteen[b as usize].to_ne_bytes()).collect();
                let mut enc = image::codecs::tiff::TiffEncoder::new(file);
                if let Some(icc) = icc {
                    let _ = enc.set_icc_profile(icc);
                }
                enc.write_image(&pixels, w, h, ExtendedColorType::Rgb16)
            }
        }
    })();
    match result {
        Ok(()) => std::fs::rename(&partial, &path).map(|()| path).map_err(|e| e.to_string()),
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e.to_string())
        }
    }
}

// ---- dialog -------------------------------------------------------------------------------------

/// The export dialog over the app, or its progress once running.
pub fn dialog(s: &State) -> Option<El> {
    let content = if let Some(run) = &s.export_run { progress(run) } else { settings_view(s, s.export_scope?) };
    Some(
        col()
            .key("export-overlay")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .fill(t::BG_CANVAS.alpha(0.7))
            .child(col().key("export-dialog").w(520.0).padding(16.0).gap(20.0).radius(12.0).fill(t::BG_PANEL).child(content)),
    )
}

fn field(label: &'static str, control: El) -> El {
    row().w_full().gap(16.0).center_y().child(text(label).font(t::LABEL).color(t::INK_MUTED).w(96.0)).child(control)
}

fn toggle(key: &'static str, label: &'static str, on: bool, flip: fn(&mut Settings)) -> El {
    row()
        .key(key)
        .role("switch")
        .label(label)
        .checked(on)
        .gap(8.0)
        .center_y()
        .on_press(move |s: &mut State| flip(&mut s.export_settings))
        .child(
            el().w(30.0)
                .h(18.0)
                .radius(9.0)
                .fill(if on { t::ACCENT } else { t::BG_RAISED })
                .border(if on { 0.0 } else { 1.5 }, t::LINE_STRONG)
                .child(el().absolute(if on { 15.0 } else { 3.0 }, 3.0).size(12.0, 12.0).radius(6.0).fill(if on { t::ON_ACCENT } else { t::INK_MUTED })),
        )
        .child(text(label).font(t::BODY).color(t::INK))
}

fn settings_view(s: &State, scope: Scope) -> El {
    let x = &s.export_settings;
    let count = s.export_paths(scope).len();
    let title = if count == 1 { "Export 1 photo".to_owned() } else { format!("Export {count} photos") };
    let destination = x.destination_for(s.folder.as_deref());
    let mut body = col().w_full().gap(14.0);
    // Offered when there is both an open or selected photo and more than it in the grid.
    let (one, shown) = (s.export_paths(Scope::One).len(), s.export_paths(Scope::Shown).len());
    if one == 1 && shown > 1 {
        let all = if s.filter == Default::default() { "All in folder" } else { "All shown" };
        body = body.child(field("Photos", segmented("scope", &[(Scope::One, "This photo"), (Scope::Shown, all)], scope, |s, v| s.export_scope = Some(v))));
    }
    body = body
        .child(field("Format", segmented("format", &[(Format::Jpeg, "JPEG"), (Format::Png, "PNG"), (Format::Tiff, "TIFF")], x.format, |s, f| s.export_settings.format = f)));
    if x.format == Format::Jpeg {
        body = body.child(field(
            "Quality",
            segmented("quality", &[(70, "70"), (80, "80"), (90, "90"), (100, "100")], x.quality, |s, q| s.export_settings.quality = q),
        ));
    }
    body = body
        .child(field("Long edge", segmented("size", &SIZES, x.long_edge, |s, v| s.export_settings.long_edge = v)))
        .child(field("Colour space", segmented("space", &[(false, "sRGB"), (true, "Display P3")], x.display_p3, |s, v| s.export_settings.display_p3 = v)))
        .child(field("Metadata", toggle("camera-info", "Include camera and lens info", x.camera_info, |x| x.camera_info = !x.camera_info)))
        .child(field(
            "Save to",
            row()
                .gap(8.0)
                .shrink()
                .center_y()
                .child(text(tilde(&destination)).font(t::VALUE_SM).color(t::INK).truncate_fade().shrink())
                .child(
                    el().key("choose-destination")
                        .role("button")
                        .label("Choose a folder to export to")
                        .h(28.0)
                        .px(12.0)
                        .center()
                        .radius(7.0)
                        .fill(t::BG_RAISED)
                        .hover(t::BG_HOVER)
                        .on_press_tx(|_: &mut State, tx: &mut Tx| {
                            let options = FileDialogOptions { title: "Export to".into(), ..Default::default() };
                            tx.open_folder(options, |s: &mut State, result, _tx| {
                                if let Ok(Some(dir)) = result {
                                    s.export_settings.destination = Some(dir);
                                }
                            });
                        })
                        .child(text("Choose…").font(t::BODY).color(t::INK)),
                ),
        ));
    let note = if x.format == Format::Tiff { "16 bits per channel, without camera info." } else { "Location is never included." };
    let footer = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text(note).font(t::CAPTION).color(t::INK_MUTED).shrink())
        .child(spacer())
        .child(button("export-cancel", "Cancel", |s| s.export_scope = None))
        .child(primary("export-go", "Export").on_press(move |s: &mut State| s.start_export(scope)));
    col().w_full().gap(20.0).child(text(title).font(t::DISPLAY).color(t::INK)).child(body).child(footer)
}

fn progress(run: &Run) -> El {
    let total = run.paths.len();
    let done = run.written.len() + run.failed.len();
    let title = if run.finished() {
        if run.failed.is_empty() { format!("Exported {} photo{}", total, if total == 1 { "" } else { "s" }) } else { format!("Exported {} of {total}", run.written.len()) }
    } else {
        format!("Exporting {} of {total}…", (done + 1).min(total))
    };
    let frac = done as f32 / total.max(1) as f32;
    let bar = el().w_full().h(4.0).radius(2.0).fill(t::BG_ACTIVE).child(el().absolute(0.0, 0.0).h(4.0).radius(2.0).w_frac(frac).fill(t::ACCENT));
    let mut c = col().w_full().gap(16.0).child(text(title).font(t::DISPLAY).color(t::INK)).child(bar).child(row().w_full().child(text(tilde(&run.destination)).font(t::VALUE_SM).color(t::INK_MUTED).truncate_fade().shrink()));
    for f in &run.failed {
        c = c.child(text(f.clone()).font(t::CAPTION).color(t::DANGER_INK));
    }
    if run.finished() {
        let dir = run.destination.clone();
        c = c.child(
            row()
                .w_full()
                .gap(8.0)
                .child(spacer())
                .child(button("show-in-finder", "Show in Finder", move |_| {
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                }))
                .child(primary("export-done", "Done").on_press(|s: &mut State| {
                    s.export_run = None;
                    s.export_scope = None;
                })),
        );
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("darkroom-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "DSCF4127", "jpg"), dir.join("DSCF4127.jpg"));
        std::fs::write(dir.join("DSCF4127.jpg"), b"").unwrap();
        std::fs::write(dir.join("DSCF4127 (2).jpg"), b"").unwrap();
        assert_eq!(unique_path(&dir, "DSCF4127", "jpg"), dir.join("DSCF4127 (3).jpg"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_tables_apply_the_srgb_curve() {
        let (eight, sixteen) = tables();
        let at = |v: f32| half::f16::from_f32(v).to_bits() as usize;
        assert_eq!(eight[at(0.0)], 0);
        assert_eq!(eight[at(1.0)], 255);
        assert_eq!(eight[at(0.2159)], 128, "linear 0.216 is middle grey encoded");
        assert_eq!(sixteen[at(1.0)], 65535);
        assert_eq!(eight[at(-0.5)], 0, "negatives clamp");
    }

    #[test]
    fn a_small_render_writes_every_format() {
        let dir = std::env::temp_dir().join(format!("darkroom-write-{}", std::process::id()));
        let image = Exported { size: [4, 2], rgba16f: vec![half::f16::from_f32(0.5).to_bits(); 4 * 2 * 4] };
        for format in [Format::Jpeg, Format::Png, Format::Tiff] {
            let settings = Settings { format, ..Settings::default() };
            let path = write(&image, Path::new("/nonexistent/DSCF0001.JPG"), &dir, &settings).unwrap();
            assert_eq!(path.extension().unwrap(), format.extension());
            let back = image::open(&path).unwrap();
            assert_eq!((back.width(), back.height()), (4, 2));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
