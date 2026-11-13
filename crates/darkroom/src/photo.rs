//! Loading a photo into the preview buffer the GPU develops.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};

/// The longest edge of the preview the develop pass runs on. Full resolution is for 100% zoom
/// and export, later.
const PREVIEW_EDGE: u32 = 2560;

pub struct Photo {
    /// Unique per load, so the GPU side knows when to upload again.
    pub id: u64,
    pub name: String,
    pub size: [u32; 2],
    /// sRGB RGBA8, tightly packed.
    pub rgba: Vec<u8>,
    /// Counts of each encoded value, 0..=255: luminance, red, green, blue. Of the unedited
    /// photo for now; the live histogram of the edit needs a GPU readback.
    pub histogram: [[u32; 256]; 4],
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
    /// Decodes a JPEG or PNG, applies its EXIF orientation and scales it to preview size.
    pub fn open(path: &str) -> Result<Photo, image::ImageError> {
        let mut decoder = ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut img = DynamicImage::from_decoder(decoder)?;
        img.apply_orientation(orientation);
        if img.width().max(img.height()) > PREVIEW_EDGE {
            img = img.resize(PREVIEW_EDGE, PREVIEW_EDGE, FilterType::Triangle);
        }
        let rgba = img.into_rgba8();
        let name = Path::new(path).file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        let size = [rgba.width(), rgba.height()];
        let rgba = rgba.into_raw();
        Ok(Photo { id: next_id(), name, size, histogram: histogram(&rgba), rgba })
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
        Photo { id: next_id(), name: "Test chart".into(), size: [w, h], histogram: histogram(&rgba), rgba }
    }
}

/// A fully saturated hue, `h` in 0..1.
fn hue(h: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        1.0 - (k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [f(5.0), f(3.0), f(1.0)]
}
