//! A raw photo's white balance as a colour temperature and tint.
//!
//! A white is a point in CIE 1960 uv: its colour temperature is the nearest point on the
//! Planckian locus (the colours of a glowing black body), and `duv` how far it sits off it,
//! positive towards green. A RAF's as-shot white comes from the camera's own multipliers; the
//! Temperature and Tint sliders move away from it in mired (a million over kelvin, so equal steps
//! look equally large) and in duv.

/// The temperatures the locus formula below covers.
pub const KELVIN_RANGE: (f32, f32) = (1667.0, 25000.0);
/// Mired per unit of the Temperature slider: ±100 moves a 5200 K white to about 10 900 K or
/// 3400 K. A guess, set to span what daylight and tungsten need.
const MIRED_PER_UNIT: f32 = 1.0;
/// Duv per unit of the Tint slider. A guess.
const DUV_PER_UNIT: f32 = 0.0002;

/// Linear Rec.2020 from CIE XYZ (D65), the working space's primaries.
const XYZ_TO_REC2020: [[f32; 3]; 3] = [[1.716_651, -0.355_671, -0.253_366], [-0.666_684, 1.616_481, 0.015_769], [0.017_640, -0.042_771, 0.942_103]];
/// Rec.2020 luminance weights, for keeping white's brightness when the balance changes.
const LUMA_2020: [f32; 3] = [0.2627, 0.6780, 0.0593];

/// The white a camera balanced a photo for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AsShot {
    pub kelvin: f32,
    pub duv: f32,
}

impl AsShot {
    /// The white at CIE xy, read as a temperature and tint.
    pub fn from_xy([x, y]: [f32; 2]) -> AsShot {
        let (kelvin, duv) = cct_duv(xy_to_uv([x, y]));
        AsShot { kelvin, duv }
    }

    /// The white the sliders ask for. Both describe the light, as a camera's setting does: a
    /// positive Temperature takes it as bluer (a higher kelvin), so the photo comes out warmer; a
    /// positive Tint takes it as greener, so the photo comes out more magenta.
    pub fn target(self, temperature: f32, tint: f32) -> AsShot {
        let (lo, hi) = KELVIN_RANGE;
        let mired = 1.0e6 / self.kelvin - temperature * MIRED_PER_UNIT;
        let kelvin = (1.0e6 / mired.max(1.0e6 / hi)).clamp(lo, hi);
        AsShot { kelvin, duv: self.duv + tint * DUV_PER_UNIT }
    }

    /// The Temperature that asks for `kelvin`: the inverse of [`Self::target`]'s.
    pub fn temperature_for(self, kelvin: f32) -> f32 {
        (1.0e6 / self.kelvin - 1.0e6 / kelvin.clamp(KELVIN_RANGE.0, KELVIN_RANGE.1)) / MIRED_PER_UNIT
    }

    /// The per-channel multipliers, in linear Rec.2020, that take a photo balanced for this white
    /// to one balanced for `target`, with white's luminance kept.
    pub fn multipliers(self, target: AsShot) -> [f32; 3] {
        let (from, to) = (white_rgb(self), white_rgb(target));
        let m = [from[0] / to[0], from[1] / to[1], from[2] / to[2]];
        let luma = m[0] * LUMA_2020[0] + m[1] * LUMA_2020[1] + m[2] * LUMA_2020[2];
        m.map(|c| c / luma)
    }

    /// The slider values whose white makes a pixel neutral: `rgb` is linear Rec.2020 as the
    /// photo is now. None when no setting in range comes close.
    pub fn neutralize(self, rgb: [f32; 3]) -> Option<[f32; 2]> {
        let off = |[t, n]: [f32; 2]| {
            let m = self.multipliers(self.target(t, n));
            let c = [rgb[0] * m[0], rgb[1] * m[1], rgb[2] * m[2]];
            let mean = (c[0] + c[1] + c[2]) / 3.0;
            if mean <= 0.0 { f32::MAX } else { c.iter().map(|v| (v / mean - 1.0).powi(2)).sum::<f32>() }
        };
        // A coarse grid, then finer steps about the best point.
        let mut best = [0.0f32, 0.0];
        let mut step = 10.0f32;
        for _ in 0..5 {
            let centre = best;
            for i in -10..=10 {
                for j in -10..=10 {
                    let p = [(centre[0] + i as f32 * step).clamp(-100.0, 100.0), (centre[1] + j as f32 * step).clamp(-100.0, 100.0)];
                    if off(p) < off(best) {
                        best = p;
                    }
                }
            }
            step /= 5.0;
        }
        (off(best) < 1e-3).then_some(best)
    }
}

/// A white of a temperature and tint, as linear Rec.2020 with luminance 1.
fn white_rgb(w: AsShot) -> [f32; 3] {
    let [x, y] = uv_to_xy(white_uv(w.kelvin, w.duv));
    let xyz = [x / y, 1.0, (1.0 - x - y) / y];
    XYZ_TO_REC2020.map(|r| r[0] * xyz[0] + r[1] * xyz[1] + r[2] * xyz[2])
}

/// The Planckian locus in CIE xy (Kim et al. 2002), 1667 K to 25 000 K.
fn planck_xy(kelvin: f32) -> [f32; 2] {
    let t = kelvin.clamp(KELVIN_RANGE.0, KELVIN_RANGE.1) as f64;
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / t.powi(3) - 0.234_358_9e6 / t.powi(2) + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / t.powi(3) + 2.107_037_9e6 / t.powi(2) + 0.222_634_7e3 / t + 0.240_390
    };
    let y = if t <= 2222.0 {
        -1.106_381_4 * x.powi(3) - 1.348_110_20 * x.powi(2) + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x.powi(3) - 1.374_185_93 * x.powi(2) + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x.powi(3) - 5.873_386_70 * x.powi(2) + 3.751_129_97 * x - 0.370_014_83
    };
    [x as f32, y as f32]
}

fn xy_to_uv([x, y]: [f32; 2]) -> [f32; 2] {
    let d = -2.0 * x + 12.0 * y + 3.0;
    [4.0 * x / d, 6.0 * y / d]
}

fn uv_to_xy([u, v]: [f32; 2]) -> [f32; 2] {
    let d = 2.0 * u - 8.0 * v + 4.0;
    [3.0 * u / d, 2.0 * v / d]
}

/// The unit normal to the locus at `kelvin`, in uv, pointing to the green side.
fn locus_normal(kelvin: f32) -> [f32; 2] {
    let (a, b) = (xy_to_uv(planck_xy(kelvin - 10.0)), xy_to_uv(planck_xy(kelvin + 10.0)));
    let (du, dv) = (b[0] - a[0], b[1] - a[1]);
    let len = (du * du + dv * dv).sqrt();
    let n = [-dv / len, du / len];
    if n[1] >= 0.0 { n } else { [-n[0], -n[1]] }
}

fn white_uv(kelvin: f32, duv: f32) -> [f32; 2] {
    let p = xy_to_uv(planck_xy(kelvin));
    let n = locus_normal(kelvin);
    [p[0] + duv * n[0], p[1] + duv * n[1]]
}

/// The nearest point on the locus to `uv`, as its temperature and the signed distance to it.
fn cct_duv(uv: [f32; 2]) -> (f32, f32) {
    let dist = |mired: f32| {
        let p = xy_to_uv(planck_xy(1.0e6 / mired));
        (p[0] - uv[0]).powi(2) + (p[1] - uv[1]).powi(2)
    };
    // The distance has one minimum along the locus: a ternary search in mired finds it.
    let (mut lo, mut hi) = (1.0e6 / KELVIN_RANGE.1, 1.0e6 / KELVIN_RANGE.0);
    for _ in 0..80 {
        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if dist(a) < dist(b) { hi = b } else { lo = a }
    }
    let kelvin = 1.0e6 / ((lo + hi) / 2.0);
    let p = xy_to_uv(planck_xy(kelvin));
    let n = locus_normal(kelvin);
    (kelvin, (uv[0] - p[0]) * n[0] + (uv[1] - p[1]) * n[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daylight_reads_as_6500_kelvin_a_little_green() {
        let d65 = AsShot::from_xy([0.3127, 0.3290]);
        assert!((d65.kelvin - 6504.0).abs() < 30.0, "{d65:?}");
        assert!((d65.duv - 0.0032).abs() < 0.0005, "{d65:?}");
    }

    #[test]
    fn tungsten_reads_as_2856_kelvin_on_the_locus() {
        let a = AsShot::from_xy([0.4476, 0.4074]);
        assert!((a.kelvin - 2856.0).abs() < 20.0, "{a:?}");
        assert!(a.duv.abs() < 0.0005, "{a:?}");
    }

    #[test]
    fn a_white_reads_back_as_itself() {
        let w = AsShot { kelvin: 4300.0, duv: -0.004 };
        let back = AsShot::from_xy(uv_to_xy(white_uv(w.kelvin, w.duv)));
        assert!((back.kelvin - w.kelvin).abs() < 5.0 && (back.duv - w.duv).abs() < 1e-4, "{back:?}");
    }

    #[test]
    fn as_shot_changes_nothing() {
        let w = AsShot { kelvin: 5200.0, duv: 0.002 };
        let m = w.multipliers(w.target(0.0, 0.0));
        assert!(m.iter().all(|c| (c - 1.0).abs() < 1e-4), "{m:?}");
    }

    #[test]
    fn warmer_raises_red_and_lowers_blue_and_keeps_white_as_bright() {
        let w = AsShot { kelvin: 5200.0, duv: 0.0 };
        let target = w.target(40.0, 0.0);
        assert!(target.kelvin > w.kelvin);
        let m = w.multipliers(target);
        assert!(m[0] > 1.0 && m[2] < 1.0, "{m:?}");
        let luma: f32 = m.iter().zip(LUMA_2020).map(|(a, b)| a * b).sum();
        assert!((luma - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_typed_kelvin_reads_back() {
        let w = AsShot { kelvin: 5200.0, duv: 0.0 };
        assert!((w.target(w.temperature_for(7000.0), 0.0).kelvin - 7000.0).abs() < 1.0);
    }

    #[test]
    fn magenta_tint_lowers_green() {
        let w = AsShot { kelvin: 5200.0, duv: 0.0 };
        let m = w.multipliers(w.target(0.0, 50.0));
        assert!(m[1] < m[0] && m[1] < m[2], "{m:?}");
    }

    #[test]
    fn a_picked_grey_comes_out_neutral() {
        let w = AsShot { kelvin: 5200.0, duv: 0.0 };
        // A grey card lit warmer than the camera assumed, as the photo shows it.
        let m = w.multipliers(w.target(-30.0, 10.0));
        let seen = [0.4 / m[0], 0.4 / m[1], 0.4 / m[2]];
        let [t, n] = w.neutralize(seen).expect("in range");
        assert!((t + 30.0).abs() < 0.5 && (n - 10.0).abs() < 0.5, "{t} {n}");
    }
}
