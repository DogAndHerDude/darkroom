//! Developing a camera raw file (Fujifilm RAF) into linear pixels the pipeline can take.
//!
//! `rawler` reads the sensor data and does the camera-specific part: black and white levels,
//! demosaicing (Markesteijn's method for X-Trans sensors), the camera's white balance and its
//! colour matrix. What comes out is
//! linear sRGB-primary light at full precision, where a JPEG gives 8 encoded bits; the edit
//! then runs on it like on any photo.

use std::path::Path;

use image::{DynamicImage, Rgb32FImage, metadata::Orientation};
use rawler::decoders::RawDecodeParams;
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
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
const BASE_GAIN: f32 = 1.8;
/// Luminance above this rolls off towards white instead of clipping under the gain.
const SHOULDER: f32 = 0.7;

/// The starting look for raw files: the base gain with a soft highlight shoulder, applied to
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
    for px in data.as_chunks_mut::<3>().0 {
        *px = base_tone(*px);
    }
    let buffer = Rgb32FImage::from_raw(w, h, data).ok_or("the developed image has the wrong size")?;
    let mut img = DynamicImage::ImageRgb32F(buffer);
    if let Some(o) = u8::try_from(raw.orientation.to_u16()).ok().and_then(Orientation::from_exif) {
        img.apply_orientation(o);
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

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
