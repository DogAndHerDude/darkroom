//! The camera's own rendering of a raw file, fitted from the JPEG it embedded.
//!
//! Every RAF carries the camera's JPEG of the same exposure, made with whatever film simulation,
//! dynamic range, highlight and shadow tone and colour setting it was set to. Comparing the
//! developed sensor data with that JPEG gives the camera's rendering of this photo without
//! knowing what any of those settings do. Two curves are fitted:
//!
//! - tone: output luminance for each input luminance (in log-log space), applied as a ratio to
//!   all three channels so hue holds;
//! - chroma: output Oklab chroma for each input chroma, at constant lightness and hue. A curve,
//!   not one scale: cameras lift muted colours (foliage) far more than strong ones.
//!
//! The two images can't be compared pixel by pixel: the camera corrects the lens's distortion
//! and vignetting in its JPEG, so the same position is a different point of the scene. They are
//! compared by distribution instead. Both curves only ever give more for more, so the raw's n-th
//! percentile becomes the JPEG's n-th percentile.
//!
//! Hue shifts a simulation makes (Classic Chrome's greens) aren't fitted. Where the JPEG is
//! clipped there is nothing to fit, so the tone curve rolls off towards white past its last
//! point and the raw's highlights stay recoverable.

/// Luminance bins across the input range, a third of a stop each.
const BINS: usize = 39;
/// The input range the tone curve is fitted over, in stops below sensor white.
const LOWEST_STOP: f32 = -13.0;
/// Percentiles of luminance compared, evenly spaced; a bin needs `MIN_PER_BIN` of them to count.
const QUANTILES: usize = 2000;
const MIN_PER_BIN: usize = 3;
/// A fit needs this many bins, spread over at least `MIN_STOPS`, or it isn't trusted.
const MIN_BINS: usize = 3;
const MIN_STOPS: f32 = 0.6;
/// JPEG luminance at or above this (linear) is taken as clipped: near white, a channel has
/// likely hit 255.
const JPEG_CLIPPED: f32 = 0.85;
/// Raw luminance at or above this is at the sensor's white.
const RAW_CLIPPED: f32 = 0.99;
/// Percentiles of chroma compared, the points of the chroma curve.
const CHROMA_QUANTILES: usize = 40;
/// Chroma is compared over midtones only: in shadows it is mostly noise, and near white the
/// JPEG has clipped it.
const MIDTONES: std::ops::Range<f32> = 0.03..0.6;
/// A camera JPEG counts as black and white when no more than one pixel in a thousand has Oklab
/// chroma over this. JPEG keeps a grey pixel's colour at exactly neutral, so a monochrome
/// picture style leaves nothing near it; a toned one (sepia) has far more and counts as colour.
const MONOCHROME_CHROMA: f32 = 0.01;

#[derive(Clone, Debug, PartialEq)]
pub struct CameraLook {
    /// (log2 input luminance, log2 output luminance), input increasing, output non-decreasing.
    tone: Vec<[f32; 2]>,
    /// (input chroma, output chroma) from (0, 0), both increasing. Past the last point the
    /// ratio there holds.
    chroma: Vec<[f32; 2]>,
}

fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The `q` quantile (0..1) of sorted values.
fn quantile(sorted: &[f32], q: f32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * q) as usize]
}

impl CameraLook {
    /// Fits the look from `raw` (developed sensor data) and `jpeg` (the camera's JPEG) of the
    /// same scene, both linear light in sRGB primaries. They needn't line up or be the same
    /// size. `black_and_white` says the JPEG is in one of the camera's monochrome simulations:
    /// it has no colour to compare, so colour is left as the raw has it, for the Black & White
    /// mixer to work on. None when too little can be compared to trust a curve.
    pub fn fit(raw: &[[f32; 3]], jpeg: &[[f32; 3]], black_and_white: bool) -> Option<CameraLook> {
        let sorted = |pixels: &[[f32; 3]]| {
            let mut y: Vec<f32> = pixels.iter().map(|&p| luminance(p)).collect();
            y.sort_by(f32::total_cmp);
            y
        };
        let (yr, yj) = (sorted(raw), sorted(jpeg));
        if yr.len() < QUANTILES || yj.len() < QUANTILES {
            return None;
        }
        let mut bins: Vec<Vec<f32>> = vec![Vec::new(); BINS];
        for i in 0..QUANTILES {
            let q = (i as f32 + 0.5) / QUANTILES as f32;
            let (r, j) = (quantile(&yr, q), quantile(&yj, q));
            if r <= 0.0 || j <= 0.0 || r >= RAW_CLIPPED || j >= JPEG_CLIPPED {
                continue;
            }
            if let Some(bin) = bin_of(r.log2()) {
                bins[bin].push(j.log2());
            }
        }
        let mut points: Vec<([f32; 2], f32)> = Vec::new();
        for (i, values) in bins.iter_mut().enumerate() {
            if values.len() < MIN_PER_BIN {
                continue;
            }
            values.sort_by(f32::total_cmp);
            points.push(([centre_of(i), values[values.len() / 2]], values.len() as f32));
        }
        if points.len() < MIN_BINS || points[points.len() - 1].0[0] - points[0].0[0] < MIN_STOPS {
            return None;
        }
        let mut look = CameraLook { tone: monotone(points), chroma: vec![[0.0, 0.0], [1.0, 1.0]] };
        look.chroma = if black_and_white { vec![[0.0, 0.0], [1.0, 1.0]] } else { look.fit_chroma(raw, jpeg) };
        Some(look)
    }

    /// The chroma curve: the toned raw's midtone chroma, percentile by percentile, against the
    /// JPEG's. Unchanged chroma when either has too few midtones or too little colour to tell.
    fn fit_chroma(&self, raw: &[[f32; 3]], jpeg: &[[f32; 3]]) -> Vec<[f32; 2]> {
        let sorted = |pixels: &mut dyn Iterator<Item = [f32; 3]>| {
            let mut c: Vec<f32> = pixels.filter(|&p| MIDTONES.contains(&luminance(p))).map(|p| chroma(oklab(p))).collect();
            c.sort_by(f32::total_cmp);
            (c.len() >= 20 * CHROMA_QUANTILES).then_some(c)
        };
        let (Some(toned), Some(camera)) = (sorted(&mut raw.iter().map(|&p| self.tone(p))), sorted(&mut jpeg.iter().copied())) else {
            return vec![[0.0, 0.0], [1.0, 1.0]];
        };
        let mut points = vec![([0.0, 0.0], 1.0)];
        for i in 0..CHROMA_QUANTILES {
            let q = (i as f32 + 0.5) / CHROMA_QUANTILES as f32;
            let (c_in, c_out) = (quantile(&toned, q), quantile(&camera, q));
            // A point no further along than the last says nothing new (grey pixels share 0).
            if c_in > points[points.len() - 1].0[0] + 1e-4 {
                points.push(([c_in, c_out], 1.0));
            }
        }
        if points.len() < 4 {
            return vec![[0.0, 0.0], [1.0, 1.0]];
        }
        monotone(points)
    }

    /// The output luminance for input luminance `y`. Below the first point the ratio there
    /// holds; past the last it rolls off towards 1, continuing the curve's slope, so nothing
    /// clips and the brightest raw values stay just under white.
    pub fn curve(&self, y: f32) -> f32 {
        if y <= 0.0 {
            return 0.0;
        }
        let x = y.log2();
        let (first, last) = (self.tone[0], self.tone[self.tone.len() - 1]);
        if x <= first[0] {
            return y * (first[1] - first[0]).exp2();
        }
        if x >= last[0] {
            let (x0, a) = (last[0].exp2(), last[1].exp2().min(0.999));
            // Slope in linear terms at the last point, from the last segment's log-log slope.
            let before = self.tone[self.tone.len().saturating_sub(2)];
            let s = if last[0] > before[0] { ((last[1] - before[1]) / (last[0] - before[0])).max(0.0) } else { 1.0 };
            let slope = s * a / x0;
            return a + (1.0 - a) * (1.0 - (-slope * (y - x0) / (1.0 - a)).exp());
        }
        let ([x0, y0], [x1, y1]) = segment(&self.tone, x);
        (y0 + (y1 - y0) * (x - x0) / (x1 - x0)).exp2()
    }

    /// The output chroma for input chroma `c`.
    pub fn chroma_curve(&self, c: f32) -> f32 {
        let last = self.chroma[self.chroma.len() - 1];
        if c >= last[0] {
            return if last[0] > 0.0 { c * last[1] / last[0] } else { c };
        }
        let ([x0, y0], [x1, y1]) = segment(&self.chroma, c);
        y0 + (y1 - y0) * (c - x0) / (x1 - x0)
    }

    /// The tone curve as a ratio on all three channels.
    fn tone(&self, rgb: [f32; 3]) -> [f32; 3] {
        let rgb = rgb.map(|v| v.max(0.0));
        let y = luminance(rgb);
        if y <= 0.0 {
            return [0.0; 3];
        }
        let k = self.curve(y) / y;
        rgb.map(|v| v * k)
    }

    /// One pixel of the raw (linear, sRGB primaries) as the camera rendered it.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let [l, a, b] = oklab(self.tone(rgb));
        let c = a.hypot(b);
        if c <= 0.0 {
            return from_oklab([l, 0.0, 0.0]).map(|v| v.max(0.0));
        }
        let k = self.chroma_curve(c) / c;
        from_oklab([l, a * k, b * k]).map(|v| v.max(0.0))
    }
}

/// The two points either side of `x` in `knots` (x increasing), `x` strictly inside them.
fn segment(knots: &[[f32; 2]], x: f32) -> ([f32; 2], [f32; 2]) {
    let i = knots.partition_point(|k| k[0] <= x).clamp(1, knots.len() - 1);
    (knots[i - 1], knots[i])
}

fn bin_of(stops: f32) -> Option<usize> {
    let i = ((stops - LOWEST_STOP) / (-LOWEST_STOP) * BINS as f32).floor();
    (0.0..BINS as f32).contains(&i).then_some(i as usize)
}

fn centre_of(bin: usize) -> f32 {
    LOWEST_STOP + (bin as f32 + 0.5) * (-LOWEST_STOP) / BINS as f32
}

/// Makes the outputs non-decreasing (pool adjacent violators, weighted), so a noisy point
/// can't make more input come out as less.
fn monotone(points: Vec<([f32; 2], f32)>) -> Vec<[f32; 2]> {
    // Each block: (summed weight × value, summed weight, how many points).
    let mut blocks: Vec<(f32, f32, usize)> = Vec::new();
    for &([_, y], w) in &points {
        blocks.push((y * w, w, 1));
        while blocks.len() > 1 {
            let (b, a) = (blocks[blocks.len() - 1], blocks[blocks.len() - 2]);
            if a.0 / a.1 <= b.0 / b.1 {
                break;
            }
            blocks.pop();
            *blocks.last_mut().unwrap() = (a.0 + b.0, a.1 + b.1, a.2 + b.2);
        }
    }
    let ys = blocks.iter().flat_map(|&(sum, w, n)| std::iter::repeat_n(sum / w, n));
    points.iter().zip(ys).map(|(&([x, _], _), y)| [x, y]).collect()
}

/// Whether a camera's JPEG (linear sRGB pixels) is black and white: how a monochrome picture
/// style is told apart on cameras whose maker notes Darkroom doesn't read.
pub fn is_monochrome(jpeg: &[[f32; 3]]) -> bool {
    !jpeg.is_empty() && jpeg.iter().filter(|&&p| chroma(oklab(p)) > MONOCHROME_CHROMA).count() * 1000 <= jpeg.len()
}

fn chroma([_, a, b]: [f32; 3]) -> f32 {
    a.hypot(b)
}

/// Linear sRGB to Oklab (Björn Ottosson's matrices).
pub(crate) fn oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_, -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_, -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pseudo-random pixels across ten stops and every hue, the same each run.
    fn pixels(n: usize) -> Vec<[f32; 3]> {
        let mut seed = 12345u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        (0..n)
            .map(|_| {
                let level = (-10.0 * next()).exp2();
                let tint = [next(), next(), next()].map(|t| 0.6 + 0.8 * t);
                tint.map(|t| (level * t).min(0.98))
            })
            .collect()
    }

    /// A camera that lifts by a stop with a gentle power curve, and lifts muted colour by half
    /// and strong colour by a tenth.
    fn camera() -> CameraLook {
        let tone = (0..=13).map(|i| {
            let x = -13.0 + i as f32;
            [x, 0.9 * x + 1.0]
        });
        CameraLook { tone: tone.collect(), chroma: vec![[0.0, 0.0], [0.04, 0.06], [0.1, 0.12], [0.3, 0.33]] }
    }

    fn render(look: &CameraLook, raw: &[[f32; 3]]) -> Vec<[f32; 3]> {
        raw.iter().map(|&p| look.apply(p).map(|v| v.clamp(0.0, 1.0))).collect()
    }

    #[test]
    fn the_fit_finds_the_cameras_tone_and_chroma_curves() {
        let raw = pixels(60_000);
        let truth = camera();
        // The camera's JPEG doesn't line up with the raw, and covers a little less of the scene.
        let mut jpeg = render(&truth, &raw);
        jpeg.reverse();
        jpeg.truncate(55_000);
        let fitted = CameraLook::fit(&raw, &jpeg, false).unwrap();
        for y in [0.002, 0.02, 0.1, 0.3] {
            let (want, got) = (truth.curve(y), fitted.curve(y));
            assert!((got / want - 1.0).abs() < 0.04, "tone at {y}: {got} for {want}");
        }
        for c in [0.03, 0.06, 0.09] {
            let (want, got) = (truth.chroma_curve(c), fitted.chroma_curve(c));
            assert!((got / want - 1.0).abs() < 0.08, "chroma at {c}: {got} for {want}");
        }
    }

    #[test]
    fn past_the_jpegs_white_the_curve_rolls_off_under_one() {
        let raw = pixels(60_000);
        let fitted = CameraLook::fit(&raw, &render(&camera(), &raw), false).unwrap();
        let mut last = 0.0;
        for i in 1..=100 {
            let v = fitted.curve(i as f32 / 100.0);
            assert!(v >= last, "never darker for brighter input");
            assert!(v < 1.0, "never past white");
            last = v;
        }
        assert!(fitted.curve(1.0) > 0.95, "sensor white comes out close to white");
    }

    #[test]
    fn a_black_and_white_jpeg_leaves_the_colour_as_the_raw_has_it() {
        let raw = pixels(60_000);
        let grey: Vec<[f32; 3]> = render(&camera(), &raw).iter().map(|&p| [luminance(p); 3]).collect();
        let look = CameraLook::fit(&raw, &grey, true).unwrap();
        let red = [0.3, 0.1, 0.05];
        let out = look.apply(red);
        let ratio = |c: [f32; 3]| chroma(oklab(c)) / oklab(c)[0];
        assert!((ratio(out) - ratio(red)).abs() < 0.02, "{out:?}");
        let grey = camera().apply([0.2; 3]);
        assert!((grey[0] - grey[1]).abs() < 1e-5 && (grey[1] - grey[2]).abs() < 1e-5, "{grey:?}");
    }

    #[test]
    fn a_grey_jpeg_is_monochrome_and_a_toned_or_coloured_one_is_not() {
        let raw = pixels(20_000);
        let grey: Vec<[f32; 3]> = raw.iter().map(|&p| [luminance(p); 3]).collect();
        assert!(is_monochrome(&grey));
        let sepia: Vec<[f32; 3]> = grey.iter().map(|&[y, _, _]| [y * 1.1, y, y * 0.8]).collect();
        assert!(!is_monochrome(&sepia));
        assert!(!is_monochrome(&raw));
        assert!(!is_monochrome(&[]));
    }

    #[test]
    fn a_noisy_point_cant_make_a_curve_go_down() {
        let points = vec![([-6.0, -5.0], 10.0), ([-5.0, -3.0], 10.0), ([-4.0, -3.5], 30.0), ([-3.0, -2.0], 10.0)];
        let knots = monotone(points);
        assert!(knots.windows(2).all(|w| w[0][1] <= w[1][1]), "{knots:?}");
        assert_eq!(knots.len(), 4);
    }

    #[test]
    fn too_little_to_compare_is_no_fit() {
        let raw = pixels(50);
        assert_eq!(CameraLook::fit(&raw, &raw, false), None);
        // Everything clipped in the JPEG.
        let raw = pixels(20_000);
        assert_eq!(CameraLook::fit(&raw, &vec![[1.0; 3]; raw.len()], false), None);
    }

    #[test]
    fn a_grey_photo_keeps_its_chroma_unchanged() {
        let raw: Vec<[f32; 3]> = pixels(20_000).into_iter().map(|p| [luminance(p); 3]).collect();
        let look = CameraLook::fit(&raw, &raw, false).unwrap();
        assert_eq!(look.chroma, vec![[0.0, 0.0], [1.0, 1.0]]);
    }

    #[test]
    fn oklab_round_trips() {
        for rgb in [[0.2, 0.5, 0.8], [0.9, 0.1, 0.05], [0.18; 3]] {
            let back = from_oklab(oklab(rgb));
            assert!(rgb.iter().zip(back).all(|(a, b)| (a - b).abs() < 1e-4), "{rgb:?} {back:?}");
        }
    }
}
