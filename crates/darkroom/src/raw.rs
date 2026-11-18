//! Developing a camera raw file (Fujifilm RAF) into linear pixels the pipeline can take.
//!
//! `rawler` reads the sensor data and does the camera-specific part: black and white levels,
//! demosaicing (Markesteijn's method for X-Trans sensors), the camera's white balance and its
//! colour matrix. What comes out is
//! linear sRGB-primary light at full precision, where a JPEG gives 8 encoded bits; the edit
//! then runs on it like on any photo.
//!
//! The starting look is the camera's: a tone curve and chroma fitted against the JPEG the camera
//! embedded (`darkroom_core::camera_look`), so an unedited RAF looks like its JPEG, film
//! simulation and all, and its edit file stays empty. A file whose JPEG can't be used starts
//! from a fixed base tone instead.

use std::path::Path;

use darkroom_core::camera_look::CameraLook;
use darkroom_core::exif::Exif;
use darkroom_core::raf;
use darkroom_core::white::AsShot;
use image::{DynamicImage, Rgb32FImage, metadata::Orientation};
use rawler::decoders::RawDecodeParams;
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use rawler::imgop::xyz::Illuminant;
use rawler::imgop::sensor::{Demosaic, SensorType};
use rawler::imgop::sensor::xtrans::markesteijn::XTransMarkesteijnDemosaic;
use rawler::pixarray::PixF32;
use rawler::rawimage::{RawImage, RawImageData, RawPhotometricInterpretation};
use rawler::rawsource::RawSource;

/// Everything `rawler` does by default except the sRGB transfer curve: the result stays linear.
const STEPS: [ProcessingStep; 6] =
    [ProcessingStep::Rescale, ProcessingStep::Demosaic, ProcessingStep::CropActiveArea, ProcessingStep::WhiteBalance, ProcessingStep::Calibrate, ProcessingStep::CropDefault];

/// Sensor data is exposed to protect highlights, so straight off the sensor a photo is about a
/// stop darker than the camera's own JPEG. The gain brings middle grey to where the JPEG has it.
/// Fitted to one X-T4 sample; only used when the camera's look can't be fitted.
const BASE_GAIN: f32 = 1.8;
/// Luminance above this rolls off towards white instead of clipping under the gain.
const SHOULDER: f32 = 0.7;
/// The long edge both images are shrunk to before the camera's look is fitted: plenty of pixels
/// for every brightness, quick to sort.
const FIT_EDGE: usize = 512;
/// The middle part of each image, across and down, that the fit compares. The camera's JPEG is
/// corrected for the lens and the raw isn't; the edges, where distortion and vignetting are
/// strongest, would skew the comparison.
const FIT_CENTRE: f32 = 0.6;

/// The fallback starting look: the base gain with a soft highlight shoulder, applied to
/// luminance so colours keep their hue. Everything stays recoverable by the edit.
fn base_tone([r, g, b]: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = [r.max(0.0), g.max(0.0), b.max(0.0)];
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if y <= 0.0 {
        return [0.0; 3];
    }
    let lit = y * BASE_GAIN;
    let toned = if lit <= SHOULDER { lit } else { SHOULDER + (1.0 - SHOULDER) * (1.0 - (-(lit - SHOULDER) / (1.0 - SHOULDER)).exp()) };
    let k = toned / y;
    [r * k, g * k, b * k]
}

/// Demosaics an X-Trans image in place with Markesteijn's method, which keeps fine detail that
/// the bilinear method `rawler` develops with smears. False for any other sensor.
fn demosaic_xtrans(raw: &mut RawImage) -> Result<bool, String> {
    let RawPhotometricInterpretation::Cfa(config) = raw.photometric.clone() else { return Ok(false) };
    if raw.cpp != 1 || config.sensor != SensorType::Xtrans || !config.cfa.is_rgb() {
        return Ok(false);
    }
    raw.apply_scaling().map_err(|e| e.to_string())?;
    let pixels = PixF32::new_with(raw.data.as_f32().into_owned(), raw.width, raw.height);
    let area = raw.active_area.unwrap_or(pixels.rect());
    let rgb = XTransMarkesteijnDemosaic::new_pass_1().demosaic(&pixels, &config.cfa, &config.colors, area);
    (raw.width, raw.height, raw.cpp) = (rgb.dim().w, rgb.dim().h, 3);
    raw.data = RawImageData::Float(rgb.flatten());
    Ok(true)
}

/// The raw file as an upright, linear, sRGB-primary float image at full size.
/// The white the camera balanced a RAF for, from its multipliers and its colour matrix for
/// daylight. Reads the file's header only. None when the file doesn't give both.
pub fn as_shot(path: &Path) -> Option<AsShot> {
    let read = || {
        let source = RawSource::new(path).ok()?;
        let decoder = rawler::get_decoder(&source).ok()?;
        let raw = decoder.raw_image(&source, &RawDecodeParams::default(), true).ok()?;
        let wb = raw.wb_coeffs;
        if wb[..3].iter().any(|c| !c.is_finite() || *c <= 0.0) {
            return None;
        }
        let (_, m) = raw.color_matrix_find_first([Illuminant::D65]).filter(|(_, m)| m.len() >= 9)?;
        let xyz_to_cam = [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]];
        // What the camera saw as neutral, in its own channels, back to XYZ.
        let neutral = [1.0 / wb[0], 1.0 / wb[1], 1.0 / wb[2]];
        let xyz = solve(xyz_to_cam, neutral)?;
        let sum = xyz[0] + xyz[1] + xyz[2];
        let shot = (sum > 0.0).then(|| AsShot::from_xy([xyz[0] / sum, xyz[1] / sum]));
        if std::env::var_os("DARKROOM_DEBUG").is_some() {
            eprintln!("darkroom: {} as shot: multipliers {wb:?}, {shot:?}", path.display());
        }
        shot
    };
    // As for developing: a malformed file fails this photo, not the job thread.
    std::panic::catch_unwind(read).ok().flatten()
}

/// `x` with `m · x = b`, by Cramer's rule; None when `m` is singular.
fn solve(m: [[f32; 3]; 3], b: [f32; 3]) -> Option<[f32; 3]> {
    let det = |m: [[f32; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(m);
    if d.abs() < 1e-9 {
        return None;
    }
    let with = |col: usize| {
        let mut c = m;
        for (row, v) in c.iter_mut().zip(b) {
            row[col] = v;
        }
        det(c) / d
    };
    Some([with(0), with(1), with(2)])
}

pub fn develop(path: &Path) -> Result<DynamicImage, String> {
    // The decoder panics on some malformed files; that must fail this photo, not the job thread.
    std::panic::catch_unwind(|| develop_inner(path)).unwrap_or_else(|_| Err("the raw decoder gave up on this file".into()))
}

fn develop_inner(path: &Path) -> Result<DynamicImage, String> {
    let source = RawSource::new(path).map_err(|e| e.to_string())?;
    let decoder = rawler::get_decoder(&source).map_err(|e| e.to_string())?;
    let mut raw = decoder.raw_image(&source, &RawDecodeParams::default(), false).map_err(|e| e.to_string())?;
    // With the image already in three colours, the remaining steps pass it through demosaicing.
    let steps = if demosaic_xtrans(&mut raw)? { &STEPS[1..] } else { &STEPS[..] };
    let developed = RawDevelop::new_with(steps).develop_intermediate(&raw).map_err(|e| e.to_string())?;
    let Intermediate::ThreeColor(rgb) = developed else { return Err("not a colour raw file".into()) };
    let (w, h) = (rgb.dim().w as u32, rgb.dim().h as u32);
    let mut data = rgb.flatten();
    let look = camera_look(path, &data, w as usize, h as usize);
    // Every pixel goes through Oklab and back: split across the cores.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = (data.len() / 3).div_ceil(threads).max(1) * 3;
    std::thread::scope(|s| {
        for part in data.chunks_mut(chunk) {
            let look = &look;
            s.spawn(move || {
                for px in part.as_chunks_mut::<3>().0 {
                    *px = match look {
                        Some(look) => look.apply(*px),
                        None => base_tone(*px),
                    };
                }
            });
        }
    });
    let buffer = Rgb32FImage::from_raw(w, h, data).ok_or("the developed image has the wrong size")?;
    let mut img = DynamicImage::ImageRgb32F(buffer);
    if let Some(o) = u8::try_from(raw.orientation.to_u16()).ok().and_then(Orientation::from_exif) {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// The camera's look for this photo, fitted against the JPEG it embedded. `rgb` is the developed
/// image, `w` × `h`, not yet turned upright, as the JPEG's pixels are stored. None when there is
/// no JPEG, it frames the photo differently, or the fit isn't trusted.
fn camera_look(path: &Path, rgb: &[f32], w: usize, h: usize) -> Option<CameraLook> {
    let bytes = raf::embedded_jpeg(path)?;
    let jpeg = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).ok()?.into_rgb8();
    let (jw, jh) = (jpeg.width() as usize, jpeg.height() as usize);
    if (jw as f32 / jh as f32 - w as f32 / h as f32).abs() > 0.01 {
        return None;
    }
    let (fw, fh) = if w >= h { (FIT_EDGE, FIT_EDGE * h / w) } else { (FIT_EDGE * w / h, FIT_EDGE) };
    let raw = centre(&shrink(|i| [rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2]], w, h, fw, fh), fw, fh);
    let decode: Vec<f32> = (0..=255u8).map(srgb_decode).collect();
    let pixels = jpeg.as_raw();
    let camera = centre(&shrink(|i| [pixels[3 * i], pixels[3 * i + 1], pixels[3 * i + 2]].map(|b| decode[b as usize]), jw, jh, fw, fh), fw, fh);
    let black_and_white = Exif::read(path).look.is_some_and(|l| l.is_black_and_white());
    let look = CameraLook::fit(&raw, &camera, black_and_white);
    if std::env::var_os("DARKROOM_DEBUG").is_some() {
        eprintln!("camera look: raw {w}×{h}, jpeg {jw}×{jh}, black and white {black_and_white}: {look:?}");
    }
    look
}

/// `w` × `h` pixels shrunk to `fw` × `fh`, each output pixel the mean of its box of input pixels.
fn shrink(pixel: impl Fn(usize) -> [f32; 3], w: usize, h: usize, fw: usize, fh: usize) -> Vec<[f32; 3]> {
    let mut sums = vec![[0.0f32; 4]; fw * fh];
    for y in 0..h {
        let row = y * fh / h * fw;
        for x in 0..w {
            let [r, g, b] = pixel(y * w + x);
            let s = &mut sums[row + x * fw / w];
            *s = [s[0] + r, s[1] + g, s[2] + b, s[3] + 1.0];
        }
    }
    sums.into_iter().map(|[r, g, b, n]| [r / n, g / n, b / n]).collect()
}

/// The middle `FIT_CENTRE` of a `w` × `h` image, across and down.
fn centre(pixels: &[[f32; 3]], w: usize, h: usize) -> Vec<[f32; 3]> {
    let (cw, ch) = ((w as f32 * FIT_CENTRE) as usize, (h as f32 * FIT_CENTRE) as usize);
    let (x0, y0) = ((w - cw) / 2, (h - ch) / 2);
    (y0..y0 + ch).flat_map(|y| pixels[y * w + x0..y * w + x0 + cw].iter().copied()).collect()
}

/// An sRGB-encoded byte as linear light.
fn srgb_decode(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_averages_each_box() {
        // 4 × 2 to 2 × 1: each output pixel is the mean of a 2 × 2 box.
        let px = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.2, 0.4, 0.6], [0.2, 0.4, 0.6], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [0.2, 0.4, 0.6], [0.2, 0.4, 0.6]];
        let out = shrink(|i| px[i], 4, 2, 2, 1);
        assert_eq!(out, vec![[0.5; 3], [0.2, 0.4, 0.6]]);
    }

    #[test]
    fn the_base_tone_lifts_midtones_and_keeps_highlights_under_white() {
        let grey = base_tone([0.2; 3]);
        assert!((grey[0] - 0.36).abs() < 1e-4, "below the shoulder it is the plain gain");
        let white = base_tone([1.0; 3]);
        assert!(white[0] > 0.98 && white[0] < 1.0);
        assert!(base_tone([0.9; 3])[0] < white[0], "still increasing at the top");
        assert_eq!(base_tone([0.0; 3]), [0.0; 3]);
    }

    #[test]
    fn the_base_tone_keeps_channel_ratios() {
        let [r, g, b] = base_tone([0.4, 0.2, 0.1]);
        assert!((r / g - 2.0).abs() < 1e-4 && (g / b - 2.0).abs() < 1e-4);
    }
}
