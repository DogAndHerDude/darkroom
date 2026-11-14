//! Loading a photo into the preview buffer the GPU develops.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use darkroom_core::exif::Exif;
use darkroom_core::raf;
use darkroom_render::Source;
use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};

/// The longest edge of the preview the develop pass runs on. Full resolution is for 100% zoom
/// and export.
pub const PREVIEW_EDGE: u32 = 2560;

pub struct Photo {
    /// Unique per load, so the GPU side knows when to upload again.
    pub id: u64,
    pub name: String,
    pub size: [u32; 2],
    /// The photo's own size; larger than `size` when the preview was scaled down.
    pub full_size: [u32; 2],
    pub pixels: Pixels,
    pub exif: Exif,
    /// Counts of each encoded value, 0..=255: luminance, red, green, blue. Of the unedited
    /// photo for now; the live histogram of the edit needs a GPU readback.
    pub histogram: [[u32; 256]; 4],
}

/// A photo's pixels, RGBA and tightly packed.
pub enum Pixels {
    /// sRGB-encoded, 8 bits per channel: a JPEG or PNG.
    Srgb8(Vec<u8>),
    /// Linear sRGB-primary light as half floats: a developed raw file.
    LinearHalf(Vec<u16>),
}

impl Pixels {
    pub fn source(&self) -> Source<'_> {
        match self {
            Pixels::Srgb8(v) => Source::Srgb8(v),
            Pixels::LinearHalf(v) => Source::LinearHalf(v),
        }
    }

    /// From a decoded image: float images (raw files) stay linear and keep their precision.
    pub fn from_image(img: DynamicImage) -> ([u32; 2], Pixels) {
        let size = [img.width(), img.height()];
        let pixels = match img {
            DynamicImage::ImageRgb32F(f) => {
                let half = |v: f32| half::f16::from_f32(v.max(0.0)).to_bits();
                Pixels::LinearHalf(f.pixels().flat_map(|p| [half(p[0]), half(p[1]), half(p[2]), half(1.0)]).collect())
            }
            other => Pixels::Srgb8(other.into_rgba8().into_raw()),
        };
        (size, pixels)
    }

    fn histogram(&self) -> [[u32; 256]; 4] {
        match self {
            Pixels::Srgb8(rgba) => histogram(rgba),
            Pixels::LinearHalf(rgba) => {
                let encode = |b: u16| {
                    let v = half::f16::from_bits(b).to_f32().clamp(0.0, 1.0);
                    ((if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8
                };
                histogram(&rgba.iter().map(|&b| encode(b)).collect::<Vec<u8>>())
            }
        }
    }
}

/// Decodes a photo upright at full size: a JPEG or PNG as it is, a RAF developed to a linear
/// float image.
pub fn decode(path: &Path) -> Result<DynamicImage, String> {
    if raf::is_raf(path) {
        return crate::raw::develop(path);
    }
    let mut decoder = ImageReader::open(path).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string())?.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    Ok(img)
}

fn histogram(rgba: &[u8]) -> [[u32; 256]; 4] {
    let mut h = [[0u32; 256]; 4];
    for &[r, g, b, _] in rgba.as_chunks::<4>().0 {
        let (r, g, b) = (r as usize, g as usize, b as usize);
        let y = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as usize;
        h[0][y.min(255)] += 1;
        h[1][r] += 1;
        h[2][g] += 1;
        h[3][b] += 1;
    }
    h
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Photo {
    /// Decodes a photo and scales it to preview size.
    pub fn open(path: &str) -> Result<Photo, String> {
        let mut img = decode(Path::new(path))?;
        let full_size = [img.width(), img.height()];
        if img.width().max(img.height()) > PREVIEW_EDGE {
            img = img.resize(PREVIEW_EDGE, PREVIEW_EDGE, FilterType::Triangle);
        }
        let name = Path::new(path).file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        let (size, pixels) = Pixels::from_image(img);
        Ok(Photo { id: next_id(), name, size, full_size, exif: Exif::read(Path::new(path)), histogram: pixels.histogram(), pixels })
    }

    /// The photo as decoded around `pixel`, in linear light (sRGB primaries): the mean of the
    /// 5 × 5 pixels there, so one noisy pixel doesn't decide a white balance.
    pub fn linear_at(&self, pixel: [u32; 2]) -> [f32; 3] {
        let [w, h] = self.size;
        let mut sum = [0.0f32; 3];
        let mut count = 0.0;
        for y in pixel[1].saturating_sub(2)..(pixel[1] + 3).min(h) {
            for x in pixel[0].saturating_sub(2)..(pixel[0] + 3).min(w) {
                let i = ((y * w + x) * 4) as usize;
                let rgb = match &self.pixels {
                    Pixels::Srgb8(v) => [v[i], v[i + 1], v[i + 2]].map(srgb_decode),
                    Pixels::LinearHalf(v) => [v[i], v[i + 1], v[i + 2]].map(|b| half::f16::from_bits(b).to_f32()),
                };
                sum = [sum[0] + rgb[0], sum[1] + rgb[1], sum[2] + rgb[2]];
                count += 1.0;
            }
        }
        sum.map(|v| v / f32::max(count, 1.0))
    }

    /// A 3:2 chart to judge the adjustments by without a photo: a grey ramp across the top half,
    /// saturated hue bars below, and a dark-to-light ramp of each primary at the bottom.
    pub fn test_chart() -> Photo {
        let (w, h) = (1800u32, 1200u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let u = x as f32 / (w - 1) as f32;
                let v = y as f32 / h as f32;
                let [r, g, b] = if v < 0.5 {
                    [u, u, u]
                } else if v < 0.8 {
                    hue(u)
                } else {
                    let band = ((v - 0.8) / 0.2 * 3.0) as usize;
                    let mut c = [0.0; 3];
                    c[band.min(2)] = u;
                    c
                };
                rgba.extend_from_slice(&[(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]);
            }
        }
        Photo { id: next_id(), name: "Test chart".into(), size: [w, h], full_size: [w, h], exif: Exif::default(), histogram: histogram(&rgba), pixels: Pixels::Srgb8(rgba) }
    }
}

/// An sRGB-encoded byte as linear light, as the GPU decodes an `*Srgb` texture.
fn srgb_decode(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// A fully saturated hue, `h` in 0..1.
fn hue(h: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        1.0 - (k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [f(5.0), f(3.0), f(1.0)]
}
