//! The eight colour bands Selective Color and the black-and-white mixer work on.
//!
//! A band is a range of Oklab hue, the hue a person sees. Each colour belongs to the two bands
//! either side of its hue, weighted by a raised cosine that is 1 at one band's centre and 0 at
//! the next, so the weights always sum to 1 and change smoothly: a hue ramp never shows a step.
//! Near grey, hue is mostly noise, so every band's effect fades out as chroma goes to zero.
//!
//! The develop shader does the same per pixel; it gets the centres from `Params` and keeps its
//! own copy of [`GREY_CHROMA`] (a test checks they agree).

use crate::camera_look::oklab;

pub const COUNT: usize = 8;

pub const NAMES: [&str; COUNT] = ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"];

/// Each band's centre, as Oklab hue in degrees. Red, Yellow, Green and Magenta are the hues of
/// sRGB's primaries and secondaries; Orange sits on skin (about 53°), Aqua on cyan, Blue between
/// a clear sky (254°) and sRGB blue (264°), Purple on violet. Measured on a handful of colours,
/// not tuned on photos.
pub const CENTRES: [f32; COUNT] = [29.0, 60.0, 105.0, 142.0, 195.0, 258.0, 294.0, 328.0];

/// Oklab chroma at which a colour feels the bands fully; below it their effect fades smoothly to
/// nothing at grey. A guess: skin is about 0.075, sRGB's primaries 0.26–0.32.
pub const GREY_CHROMA: f32 = 0.04;

/// The bands a hue (degrees) belongs to: the band below it, the one above, and the weight of the
/// one above (0..1).
pub fn pair(hue: f32) -> (usize, usize, f32) {
    let hue = hue.rem_euclid(360.0);
    for (i, &centre) in CENTRES.iter().enumerate() {
        let j = (i + 1) % COUNT;
        let gap = (CENTRES[j] - centre).rem_euclid(360.0);
        let along = (hue - centre).rem_euclid(360.0);
        if along < gap {
            let t = along / gap;
            return (i, j, 0.5 - 0.5 * (std::f32::consts::PI * t).cos());
        }
    }
    (0, 1, 0.0)
}

/// Degrees from band `i`'s centre to its neighbour's: the next band's when `towards_next`, else
/// the previous one's. How far Hue ±100 turns the band.
pub fn gap(i: usize, towards_next: bool) -> f32 {
    let (from, to) = if towards_next { (i, (i + 1) % COUNT) } else { ((i + COUNT - 1) % COUNT, i) };
    (CENTRES[to] - CENTRES[from]).rem_euclid(360.0)
}

/// How much a colour of Oklab chroma `c` feels the bands, 0 at grey to 1 at [`GREY_CHROMA`].
pub fn presence(c: f32) -> f32 {
    let t = (c / GREY_CHROMA).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The band a colour (linear sRGB) mostly belongs to, or None for a colour too near grey to
/// have one.
pub fn band_of(rgb: [f32; 3]) -> Option<usize> {
    let [_, a, b] = oklab(rgb);
    if a.hypot(b) < GREY_CHROMA / 2.0 {
        return None;
    }
    let (i, j, t) = pair(b.atan2(a).to_degrees());
    Some(if t < 0.5 { i } else { j })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_band_has_all_of_its_centre_and_none_of_its_neighbours() {
        for (i, &c) in CENTRES.iter().enumerate() {
            let (lo, hi, t) = pair(c);
            let weight_of_i = if lo == i { 1.0 - t } else if hi == i { t } else { 0.0 };
            assert!((weight_of_i - 1.0).abs() < 1e-5, "{}: {:?}", NAMES[i], (lo, hi, t));
        }
    }

    #[test]
    fn the_weights_change_smoothly_all_the_way_round() {
        // The weight of each band, every tenth of a degree: no jumps, including across 360°.
        let weight = |h: f32, band: usize| {
            let (i, j, t) = pair(h);
            if i == band { 1.0 - t } else if j == band { t } else { 0.0 }
        };
        for (band, name) in NAMES.iter().enumerate() {
            for step in 0..3600 {
                let (a, b) = (step as f32 / 10.0, (step + 1) as f32 / 10.0);
                assert!((weight(a, band) - weight(b, band)).abs() < 0.01, "{name} at {a}°");
            }
        }
    }

    #[test]
    fn colours_land_in_the_bands_they_are_named_for() {
        let named = [([1.0, 0.0, 0.0], "Red"), ([0.0, 1.0, 0.0], "Green"), ([0.0, 0.0, 1.0], "Blue"), ([1.0, 1.0, 0.0], "Yellow"), ([1.0, 0.0, 1.0], "Magenta"), ([0.0, 1.0, 1.0], "Aqua")];
        for (rgb, name) in named {
            assert_eq!(band_of(rgb).map(|i| NAMES[i]), Some(name), "{rgb:?}");
        }
        // Skin (#e0ac8c) and a clear sky (#6ea0dc), in linear light.
        assert_eq!(band_of([0.745, 0.413, 0.262]).map(|i| NAMES[i]), Some("Orange"));
        assert_eq!(band_of([0.155, 0.352, 0.716]).map(|i| NAMES[i]), Some("Blue"));
    }

    #[test]
    fn grey_has_no_band_and_feels_none() {
        assert_eq!(band_of([0.4, 0.4, 0.4]), None);
        assert_eq!(presence(0.0), 0.0);
        assert_eq!(presence(GREY_CHROMA), 1.0);
    }

    #[test]
    fn hue_reach_is_the_distance_to_the_neighbour() {
        assert_eq!(gap(0, true), 31.0, "red to orange");
        assert_eq!(gap(0, false), 61.0, "red back to magenta, across 360°");
    }
}
