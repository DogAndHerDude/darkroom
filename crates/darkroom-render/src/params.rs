//! An [`Edit`] in the shape the develop shader reads: UI units turned into shader units, and
//! hidden sections turned into no-ops.

use darkroom_core::{Edit, bands};
use darkroom_core::white::AsShot;

/// Rec.2020 luminance weights: the working space's.
pub const LUMA_2020: [f32; 3] = [0.2627, 0.6780, 0.0593];

/// Stops of red/blue shift at Temperature ±100.
const TEMPERATURE_STOPS: f32 = 0.3;
/// Stops of green shift at Tint ±100.
const TINT_STOPS: f32 = 0.25;
/// Selective Color's Luminance ±100 multiplies Oklab lightness by 2^±this: about ±1.5 stops of
/// linear light. A guess, not tuned on photos.
const BAND_LIGHTNESS: f32 = 0.5;
/// The black-and-white mixer's ±100, the same way: about ±2.25 stops, enough for a red filter's
/// dark sky. A guess.
const MIX_LIGHTNESS: f32 = 0.75;
/// How much of the way to a neighbour's centre Hue ±100 turns a band. Between two centres the
/// turn changes along a raised cosine, steepest at π/2 times the change over the gap; past a
/// slope of 1 a later hue would land before an earlier one, a fold that shows as a hard edge.
/// 0.6 keeps it under 0.95.
const HUE_REACH: f32 = 0.6;

/// The develop shader's uniform. Mirrors `Params` in `develop.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Params {
    /// White balance multipliers in the working space (r, g, b), normalized to keep white's
    /// luminance.
    pub white_balance: [f32; 4],
    /// Exposure multiplier, black point, brilliance, highlights (the last three −1..1).
    pub light: [f32; 4],
    /// Shadows, brightness, contrast (−1..1).
    pub tone: [f32; 4],
    /// Saturation, vibrance (−1..1), Selective Color (1 on), black and white (1 on).
    pub color: [f32; 4],
    /// Vignette (0..1), curves (0 off, 1 on, 2 on with the composite curve on luminance), and
    /// the pixel the probe reads (x, y).
    pub effects: [f32; 4],
    /// Definition (−1..1), sharpening and noise reduction (0..1), and the edge-aware base's
    /// window radius in quarter-size pixels (set per photo by `Develop`).
    pub detail: [f32; 4],
    /// x: 1 for Display P3 output, 0 for sRGB (the preview is always sRGB). y: the detail scale,
    /// pixels of this render per preview pixel. Both set by `Develop`.
    pub output: [f32; 4],
    /// Selective Color per band, in band order: hue turn (radians), chroma scale less 1, and
    /// lightness exponent (base 2).
    pub band_hue: [[f32; 4]; 2],
    pub band_saturation: [[f32; 4]; 2],
    pub band_luminance: [[f32; 4]; 2],
    /// The black-and-white mixer per band: lightness exponent (base 2).
    pub band_mix: [[f32; 4]; 2],
    /// Each band's centre, Oklab hue in radians (`bands::CENTRES`).
    pub band_centres: [[f32; 4]; 2],
}

/// Each band's hue turn in radians, for Hue −1..1 per band: towards the neighbour turned to, by
/// [`HUE_REACH`] of the way. Two neighbours turning towards each other share that much of the
/// gap between them, so the colours between can't fold over.
fn hue_turns(hue: [f32; 8]) -> [f32; 8] {
    let mut turns: [f32; 8] = std::array::from_fn(|i| hue[i] * HUE_REACH * bands::gap(i, hue[i] > 0.0).to_radians());
    // Shrinking one pair can tighten its other neighbour's; eight rounds settle all eight pairs.
    for _ in 0..bands::COUNT {
        for i in 0..bands::COUNT {
            let j = (i + 1) % bands::COUNT;
            let (closing, room) = (turns[i] - turns[j], HUE_REACH * bands::gap(i, true).to_radians());
            if closing > room {
                let k = room / closing;
                (turns[i], turns[j]) = (turns[i] * k, turns[j] * k);
            }
        }
    }
    turns
}

/// Eight per-band values as the shader's two vec4s.
fn per_band(v: [f32; 8]) -> [[f32; 4]; 2] {
    [[v[0], v[1], v[2], v[3]], [v[4], v[5], v[6], v[7]]]
}

impl Params {
    pub fn from_edit(e: &Edit) -> Params {
        let n = |v: f32| (v / 100.0).clamp(-1.0, 1.0);
        let (l, c, d) = (&e.light, &e.color, &e.detail);
        let light = if l.enabled {
            [l.exposure.clamp(-5.0, 5.0).exp2(), n(l.black_point), n(l.brilliance), n(l.highlights)]
        } else {
            [1.0, 0.0, 0.0, 0.0]
        };
        let tone = if l.enabled { [n(l.shadows), n(l.brightness), n(l.contrast), 0.0] } else { [0.0; 4] };
        let (white_balance, color) = if c.enabled {
            (white_balance(n(c.temperature), n(c.tint)), [n(c.saturation), n(c.vibrance), 0.0, 0.0])
        } else {
            ([1.0, 1.0, 1.0, 0.0], [0.0; 4])
        };
        let unit = |v: f32| (v / 100.0).clamp(0.0, 1.0);
        let (vignette, detail) = if d.enabled {
            (unit(d.vignette), [n(d.definition), unit(d.sharpening), unit(d.noise_reduction), 0.0])
        } else {
            (0.0, [0.0; 4])
        };
        let curves = match (e.curves.enabled && !e.curves.is_identity(), e.curves.luminance) {
            (false, _) => 0.0,
            (true, false) => 1.0,
            (true, true) => 2.0,
        };
        let mut p = Params {
            white_balance,
            light,
            tone,
            color,
            effects: [vignette, curves, 0.0, 0.0],
            detail,
            output: [0.0; 4],
            band_hue: [[0.0; 4]; 2],
            band_saturation: [[0.0; 4]; 2],
            band_luminance: [[0.0; 4]; 2],
            band_mix: [[0.0; 4]; 2],
            band_centres: per_band(bands::CENTRES.map(f32::to_radians)),
        };
        p.set_bands(e, false);
        p
    }

    /// Selective Color and black and white. `camera_black_and_white` is whether the photo was
    /// shot in black and white, which is what it shows until the switch is set. Black and white
    /// replaces Selective Color: its Luminance would do the mixer's job twice, and its Hue and
    /// Saturation can't be seen.
    fn set_bands(&mut self, e: &Edit, camera_black_and_white: bool) {
        let n = |v: f32| (v / 100.0).clamp(-1.0, 1.0);
        let mono = e.black_and_white.is_on(camera_black_and_white);
        self.color[3] = if mono { 1.0 } else { 0.0 };
        self.band_mix = per_band(e.black_and_white.mix().map(|v| n(v) * MIX_LIGHTNESS));
        let hsl = &e.hsl;
        let active = hsl.enabled && !mono && !hsl.is_neutral();
        self.color[2] = if active { 1.0 } else { 0.0 };
        let b = if active { hsl.bands() } else { [darkroom_core::Band::default(); 8] };
        self.band_hue = per_band(hue_turns(b.map(|b| n(b.hue))));
        self.band_saturation = per_band(b.map(|b| n(b.saturation)));
        self.band_luminance = per_band(b.map(|b| n(b.luminance) * BAND_LIGHTNESS));
    }

    /// [`Self::from_edit`] for a photo as the camera shot it: a raw file's Temperature and Tint
    /// move its white away from the one it was shot at, in kelvin and off the locus, and a raw
    /// shot in a monochrome simulation starts in black and white.
    pub fn from_edit_shot(e: &Edit, as_shot: Option<AsShot>, camera_black_and_white: bool) -> Params {
        let mut p = Params::from_edit(e);
        p.set_bands(e, camera_black_and_white);
        let c = &e.color;
        if let Some(shot) = as_shot
            && c.enabled
            && (c.temperature != 0.0 || c.tint != 0.0)
        {
            let [r, g, b] = shot.multipliers(shot.target(c.temperature.clamp(-100.0, 100.0), c.tint.clamp(-100.0, 100.0)));
            p.white_balance = [r, g, b, 0.0];
        }
        p
    }
}

/// Channel multipliers for a relative temperature and tint (−1..1): warmer raises red and lowers
/// blue, positive tint lowers green (towards magenta). Scaled so white keeps its luminance.
pub fn white_balance(temperature: f32, tint: f32) -> [f32; 4] {
    // Exactly neutral, not neutral to within the luminance weights' f32 rounding.
    if temperature == 0.0 && tint == 0.0 {
        return [1.0, 1.0, 1.0, 0.0];
    }
    let r = (temperature * TEMPERATURE_STOPS).exp2();
    let b = (-temperature * TEMPERATURE_STOPS).exp2();
    let g = (-tint * TINT_STOPS).exp2();
    let y = LUMA_2020[0] * r + LUMA_2020[1] * g + LUMA_2020[2] * b;
    [r / y, g / y, b / y, 0.0]
}

/// The Temperature and Tint (−100..100) that make `rgb` neutral: the inverse of
/// [`white_balance`], kept to the sliders' range. `rgb` is a pixel of the photo as decoded, in
/// linear Rec.2020. None when a channel is empty, which no white balance can make grey.
pub fn neutral_point(rgb: [f32; 3]) -> Option<[f32; 2]> {
    let [r, g, b] = rgb;
    if r.min(g).min(b) <= 0.0 {
        return None;
    }
    let temperature = (b / r).log2() / (2.0 * TEMPERATURE_STOPS);
    let tint = (g / (r * b).sqrt()).log2() / TINT_STOPS;
    Some([temperature, tint].map(|v| (v * 100.0).clamp(-100.0, 100.0)))
}

/// Linear sRGB-primary values in the working space (`srgb_to_2020` in `common.wgsl`).
pub fn srgb_to_rec2020([r, g, b]: [f32; 3]) -> [f32; 3] {
    [
        0.627404 * r + 0.329283 * g + 0.043313 * b,
        0.069097 * r + 0.919540 * g + 0.011362 * b,
        0.016391 * r + 0.088013 * g + 0.895595 * b,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_edit_is_the_identity() {
        let p = Params::from_edit(&Edit::default());
        assert_eq!(p.white_balance, [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(p.light, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(p.tone, [0.0; 4]);
        assert_eq!(p.color, [0.0; 4]);
        assert_eq!(p.effects, [0.0; 4], "identity curves skip the curves step");
        assert_eq!(p.detail, [0.0; 4], "no detail passes");
        assert_eq!([p.band_hue, p.band_saturation, p.band_luminance, p.band_mix], [[[0.0; 4]; 2]; 4]);
    }

    #[test]
    fn selective_color_reaches_the_shader_only_when_a_band_is_moved_and_shown() {
        let mut e = Edit::default();
        assert_eq!(Params::from_edit(&e).color[2], 0.0);
        e.hsl.blue.saturation = 40.0;
        e.hsl.red.hue = 100.0;
        let p = Params::from_edit(&e);
        assert_eq!(p.color[2], 1.0);
        assert_eq!(p.band_saturation[1][1], 0.4, "Blue is band 5");
        assert!((p.band_hue[0][0] - 0.6 * 31f32.to_radians()).abs() < 1e-6, "Red +100 turns 60% of the way to Orange's centre");
        e.hsl.enabled = false;
        assert_eq!(Params::from_edit(&e).color[2], 0.0);
        assert_eq!(Params::from_edit(&e).band_saturation, [[0.0; 4]; 2]);
    }

    #[test]
    fn black_and_white_follows_the_camera_and_replaces_selective_color() {
        let mut e = Edit::default();
        e.hsl.green.luminance = 30.0;
        assert_eq!(Params::from_edit_shot(&e, None, false).color[3], 0.0);
        let shot_mono = Params::from_edit_shot(&e, None, true);
        assert_eq!((shot_mono.color[2], shot_mono.color[3]), (0.0, 1.0));
        assert_eq!(shot_mono.band_luminance, [[0.0; 4]; 2]);
        e.black_and_white.on = Some(false);
        assert_eq!(Params::from_edit_shot(&e, None, true).color[3], 0.0, "turned off by hand");
        e.black_and_white.on = Some(true);
        assert_eq!(Params::from_edit(&e).color[3], 1.0);
    }

    #[test]
    fn hue_never_folds_over_whatever_the_bands_say() {
        // The turned hue must keep rising with the hue it came from, all the way round.
        let cases = [[1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0], [0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0], [-1.0; 8], [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0]];
        for hue in cases {
            let turns = hue_turns(hue);
            let turned = |deg: f32| {
                let (i, j, t) = bands::pair(deg);
                deg.to_radians() + turns[i] * (1.0 - t) + turns[j] * t
            };
            for step in 0..3600 {
                let (a, b) = (step as f32 / 10.0, (step + 1) as f32 / 10.0);
                assert!(turned(b) > turned(a), "{hue:?} folds at {a}°");
            }
        }
    }

    #[test]
    fn the_shader_fades_bands_out_at_the_same_chroma_as_the_core() {
        let source = include_str!("develop.wgsl");
        assert!(source.contains(&format!("const GREY_CHROMA: f32 = {:?};", bands::GREY_CHROMA)), "develop.wgsl's GREY_CHROMA differs from bands::GREY_CHROMA");
    }

    #[test]
    fn the_luminance_mode_reaches_the_shader_only_with_a_curve_to_apply() {
        let mut e = Edit::default();
        e.curves.luminance = true;
        assert_eq!(Params::from_edit(&e).effects[1], 0.0, "identity curves still skip the step");
        e.curves.rgb = darkroom_core::Curve::new(vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]]);
        assert_eq!(Params::from_edit(&e).effects[1], 2.0);
        e.curves.luminance = false;
        assert_eq!(Params::from_edit(&e).effects[1], 1.0);
    }

    #[test]
    fn a_raw_white_balance_is_exactly_neutral_until_it_moves() {
        let shot = Some(AsShot { kelvin: 5200.0, duv: 0.003 });
        let mut e = Edit::default();
        assert_eq!(Params::from_edit_shot(&e, shot, false).white_balance, [1.0, 1.0, 1.0, 0.0]);
        e.color.temperature = 30.0;
        let raw = Params::from_edit_shot(&e, shot, false).white_balance;
        assert!(raw[0] > 1.0 && raw[2] < 1.0, "{raw:?}");
        assert_ne!(raw, Params::from_edit(&e).white_balance, "a raw file's Temperature is in kelvin, not stops");
        e.color.enabled = false;
        assert_eq!(Params::from_edit_shot(&e, shot, false).white_balance, [1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn detail_reaches_the_shader_in_shader_units_and_hides_with_its_section() {
        let mut e = Edit::default();
        e.detail.definition = -40.0;
        e.detail.sharpening = 50.0;
        e.detail.noise_reduction = 120.0;
        let p = Params::from_edit(&e);
        assert_eq!(&p.detail[..3], &[-0.4, 0.5, 1.0], "noise reduction clamps at 100");
        e.detail.enabled = false;
        assert_eq!(Params::from_edit(&e).detail, [0.0; 4]);
    }

    #[test]
    fn white_balance_keeps_whites_luminance() {
        for (t, g) in [(1.0, 0.0), (-1.0, 0.5), (0.3, -1.0)] {
            let m = white_balance(t, g);
            let y = LUMA_2020[0] * m[0] + LUMA_2020[1] * m[1] + LUMA_2020[2] * m[2];
            assert!((y - 1.0).abs() < 1e-5, "{t} {g}: {y}");
        }
        let warm = white_balance(1.0, 0.0);
        assert!(warm[0] > warm[2], "warmer means more red than blue");
    }

    #[test]
    fn the_neutral_point_of_a_cast_removes_it() {
        // Casts the sliders can reach: Temperature moves red against blue by 0.6 stops at most.
        for rgb in [[0.30, 0.25, 0.22], [0.20, 0.22, 0.27], [0.4, 0.4, 0.4]] {
            let [t, g] = neutral_point(rgb).expect("no empty channel");
            let m = white_balance(t / 100.0, g / 100.0);
            let out = [rgb[0] * m[0], rgb[1] * m[1], rgb[2] * m[2]];
            assert!((out[0] - out[1]).abs() < 1e-5 && (out[1] - out[2]).abs() < 1e-5, "{rgb:?} became {out:?}");
        }
        assert_eq!(neutral_point([0.4, 0.4, 0.4]), Some([0.0, 0.0]), "grey is already neutral");
    }

    #[test]
    fn the_neutral_point_stays_on_the_sliders() {
        assert_eq!(neutral_point([1.0, 0.5, 0.001]), Some([-100.0, 100.0]));
        assert_eq!(neutral_point([0.2, 0.0, 0.2]), None);
    }

    #[test]
    fn a_hidden_section_contributes_nothing() {
        let mut e = Edit::default();
        e.light.exposure = 1.0;
        e.light.contrast = 50.0;
        e.light.enabled = false;
        e.color.saturation = 40.0;
        e.color.enabled = false;
        let p = Params::from_edit(&e);
        assert_eq!(p.light, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(p.tone, [0.0; 4]);
        assert_eq!(p.color, [0.0; 4]);
    }

    #[test]
    fn ui_units_become_shader_units() {
        let mut e = Edit::default();
        e.light.exposure = 1.0;
        e.light.highlights = -42.0;
        e.detail.vignette = 40.0;
        let p = Params::from_edit(&e);
        assert_eq!(p.light[0], 2.0, "+1 stop doubles");
        assert!((p.light[3] + 0.42).abs() < 1e-6);
        assert!((p.effects[0] - 0.4).abs() < 1e-6);
    }
}
