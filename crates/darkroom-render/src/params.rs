//! An [`Edit`] in the shape the develop shader reads: UI units turned into shader units, and
//! hidden sections turned into no-ops.

use darkroom_core::Edit;

/// Rec.2020 luminance weights: the working space's.
pub const LUMA_2020: [f32; 3] = [0.2627, 0.6780, 0.0593];

/// Stops of red/blue shift at Temperature ±100.
const TEMPERATURE_STOPS: f32 = 0.3;
/// Stops of green shift at Tint ±100.
const TINT_STOPS: f32 = 0.25;

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
    /// Saturation, vibrance (−1..1).
    pub color: [f32; 4],
    /// Vignette (0..1), curves on (0 or 1), and the pixel the probe reads (x, y).
    pub effects: [f32; 4],
    /// Definition (−1..1), sharpening and noise reduction (0..1), and the definition blur's σ in
    /// quarter-size pixels (set per photo by `Develop`).
    pub detail: [f32; 4],
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
        let curves = if e.curves.enabled && !e.curves.is_identity() { 1.0 } else { 0.0 };
        Params { white_balance, light, tone, color, effects: [vignette, curves, 0.0, 0.0], detail }
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
