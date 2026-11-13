//! Crop, straighten, rotate and flip as one affine mapping.
//!
//! The steps, in order: the source photo is turned in quarter turns, then flipped, giving the
//! *oriented* image. That is straightened (turned by a small angle about its centre) on a canvas
//! of the same size, the *canvas*. The crop is a rectangle on the canvas, in fractions of it.
//! Every step is affine, so the whole chain from an output position to a source position is one
//! affine map, which is what the GPU samples with.
//!
//! Coordinates are y-down. A positive straighten angle turns the image clockwise on screen.

use crate::Geometry;

/// `[a, b, c, d, e, f]`: `x' = a·x + b·y + e`, `y' = c·x + d·y + f`.
pub type Affine = [f32; 6];

/// Inputs to the crop search's bisection; finer than a pixel at any size.
const STEPS: usize = 24;

impl Geometry {
    /// The oriented image's size: the source turned by its quarter turns.
    pub fn oriented_size(&self, source: [u32; 2]) -> [f32; 2] {
        let [w, h] = source.map(|v| v as f32);
        if self.quarter_turns % 2 == 1 { [h, w] } else { [w, h] }
    }

    /// A canvas position (fractions of the canvas) → source position (fractions of the source).
    pub fn canvas_to_source(&self, source: [u32; 2], p: [f32; 2]) -> [f32; 2] {
        let [w, h] = self.oriented_size(source);
        // Undo the straighten: the image was turned by +θ about the centre to make the canvas.
        let (sin, cos) = (-self.straighten.to_radians()).sin_cos();
        let (x, y) = (p[0] * w - w / 2.0, p[1] * h - h / 2.0);
        let mut o = [(x * cos - y * sin + w / 2.0) / w, (x * sin + y * cos + h / 2.0) / h];
        // Undo the flips, then the quarter turns (each turn is clockwise).
        if self.flip_horizontal {
            o[0] = 1.0 - o[0];
        }
        if self.flip_vertical {
            o[1] = 1.0 - o[1];
        }
        for _ in 0..self.quarter_turns % 4 {
            // A clockwise turn sends source (u, v) to (1 − v, u); undo it.
            o = [o[1], 1.0 - o[0]];
        }
        o
    }

    /// Whether a canvas rectangle (fractions) lies wholly on the straightened image.
    pub fn is_inside(&self, source: [u32; 2], crop: [f32; 4]) -> bool {
        let [x, y, w, h] = crop;
        let eps = 1e-4;
        [[x, y], [x + w, y], [x, y + h], [x + w, y + h]].iter().all(|&c| {
            let s = self.canvas_to_source(source, c);
            (-eps..=1.0 + eps).contains(&s[0]) && (-eps..=1.0 + eps).contains(&s[1])
        })
    }

    /// `crop` scaled about its centre, as little as needed, to lie on the straightened image.
    pub fn fit_crop(&self, source: [u32; 2], crop: [f32; 4]) -> [f32; 4] {
        let [x, y, w, h] = crop;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let scaled = |k: f32| [cx - w * k / 2.0, cy - h * k / 2.0, w * k, h * k];
        if self.is_inside(source, crop) {
            return crop;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..STEPS {
            let k = (lo + hi) / 2.0;
            if self.is_inside(source, scaled(k)) { lo = k } else { hi = k }
        }
        scaled(lo)
    }

    /// The crop in use: the chosen one, or the whole canvas, either fitted to the straightened
    /// image (a straightened photo with no crop loses just enough to hide the turned corners).
    pub fn effective_crop(&self, source: [u32; 2]) -> [f32; 4] {
        self.fit_crop(source, self.crop.unwrap_or([0.0, 0.0, 1.0, 1.0]))
    }

    /// The output's size in source pixels (before any export resize).
    pub fn output_size(&self, source: [u32; 2]) -> [u32; 2] {
        let [w, h] = self.oriented_size(source);
        let [_, _, cw, ch] = self.effective_crop(source);
        [(w * cw).round().max(1.0) as u32, (h * ch).round().max(1.0) as u32]
    }

    /// The map from a position in `rect` (fractions of the canvas; the output when it's the
    /// crop) to a source position, both as fractions.
    pub fn rect_to_source(&self, source: [u32; 2], rect: [f32; 4]) -> Affine {
        let [x, y, w, h] = rect;
        let at = |u: f32, v: f32| self.canvas_to_source(source, [x + u * w, y + v * h]);
        // Affine, so three points pin it down.
        let (o, ux, vy) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
        [ux[0] - o[0], vy[0] - o[0], ux[1] - o[1], vy[1] - o[1], o[0], o[1]]
    }

    /// The map from an output position to a source position.
    pub fn output_to_source(&self, source: [u32; 2]) -> Affine {
        self.rect_to_source(source, self.effective_crop(source))
    }

    /// Whether this geometry changes anything.
    pub fn is_identity(&self) -> bool {
        *self == Geometry::default()
    }

    /// One more quarter turn, clockwise (`by` = 1) or anticlockwise (`by` = −1). The crop turns
    /// with the image.
    pub fn turn(&mut self, by: i32) {
        self.quarter_turns = (self.quarter_turns as i32 + by).rem_euclid(4) as u8;
        if let Some([x, y, w, h]) = self.crop {
            self.crop = Some(if by > 0 { [1.0 - y - h, x, h, w] } else { [y, 1.0 - x - w, h, w] });
        }
    }
}

/// Applies an affine map to a point.
pub fn apply(m: Affine, p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[1] * p[1] + m[4], m[2] * p[0] + m[3] * p[1] + m[5]]
}

/// The largest rectangle of `aspect` (width / height, in pixels) centred on the canvas that fits
/// a canvas of `canvas` pixels, as fractions. For aspect-ratio crop presets.
pub fn centred(aspect: f32, canvas: [f32; 2]) -> [f32; 4] {
    let [cw, ch] = canvas;
    let (w, h) = if cw / ch > aspect { (ch * aspect, ch) } else { (cw, cw / aspect) };
    let (fw, fh) = (w / cw, h / ch);
    [(1.0 - fw) / 2.0, (1.0 - fh) / 2.0, fw, fh]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: [u32; 2] = [3000, 2000];

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn no_geometry_maps_output_straight_to_source() {
        let g = Geometry::default();
        assert_eq!(g.output_to_source(SRC), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        assert_eq!(g.output_size(SRC), SRC);
        assert!(g.is_identity());
    }

    #[test]
    fn a_clockwise_quarter_turn_puts_the_bottom_left_corner_top_left() {
        let g = Geometry { quarter_turns: 1, ..Geometry::default() };
        assert_eq!(g.output_size(SRC), [2000, 3000]);
        let m = g.output_to_source(SRC);
        assert!(near(apply(m, [0.0, 0.0]), [0.0, 1.0]), "output top-left shows source bottom-left");
        assert!(near(apply(m, [1.0, 0.0]), [0.0, 0.0]), "output top-right shows source top-left");
    }

    #[test]
    fn flipping_mirrors_the_output() {
        let g = Geometry { flip_horizontal: true, ..Geometry::default() };
        assert!(near(apply(g.output_to_source(SRC), [0.0, 0.5]), [1.0, 0.5]));
    }

    #[test]
    fn a_crop_selects_its_part_of_the_source() {
        let g = Geometry { crop: Some([0.25, 0.5, 0.5, 0.5]), ..Geometry::default() };
        assert_eq!(g.output_size(SRC), [1500, 1000]);
        let m = g.output_to_source(SRC);
        assert!(near(apply(m, [0.0, 0.0]), [0.25, 0.5]) && near(apply(m, [1.0, 1.0]), [0.75, 1.0]));
    }

    #[test]
    fn straightening_without_a_crop_trims_just_enough_to_hide_the_corners() {
        let g = Geometry { straighten: 5.0, ..Geometry::default() };
        let crop = g.effective_crop(SRC);
        assert!(g.is_inside(SRC, crop));
        assert!(crop[2] > 0.8 && crop[2] < 1.0, "trims a little: {crop:?}");
        assert!((crop[2] - crop[3]).abs() < 1e-4, "keeps the photo's proportions");
        let grown = [crop[0] - 0.005, crop[1] - 0.005, crop[2] + 0.01, crop[3] + 0.01];
        assert!(!g.is_inside(SRC, grown), "and no more than that");
    }

    #[test]
    fn the_straightened_output_centre_is_the_source_centre() {
        let g = Geometry { straighten: -12.0, ..Geometry::default() };
        assert!(near(apply(g.output_to_source(SRC), [0.5, 0.5]), [0.5, 0.5]));
    }

    #[test]
    fn turning_four_times_comes_back_with_the_crop_where_it_was() {
        let mut g = Geometry { crop: Some([0.1, 0.2, 0.3, 0.4]), ..Geometry::default() };
        for _ in 0..4 {
            g.turn(1);
        }
        let c = g.crop.unwrap();
        assert!(c.iter().zip([0.1, 0.2, 0.3, 0.4]).all(|(a, b)| (a - b).abs() < 1e-6), "{c:?}");
        g.turn(1);
        g.turn(-1);
        assert_eq!(g.quarter_turns, 0);
    }

    #[test]
    fn a_turned_crop_still_shows_the_same_part_of_the_photo() {
        let mut g = Geometry { crop: Some([0.0, 0.0, 0.5, 0.5]), ..Geometry::default() };
        let before = apply(g.output_to_source(SRC), [0.5, 0.5]);
        g.turn(1);
        let after = apply(g.output_to_source(SRC), [0.5, 0.5]);
        assert!(near(before, after), "{before:?} vs {after:?}");
    }

    #[test]
    fn centred_presets_fill_one_side() {
        let square = centred(1.0, [3000.0, 2000.0]);
        assert!(square.iter().zip([1.0 / 6.0, 0.0, 2.0 / 3.0, 1.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{square:?}");
        let r = centred(16.0 / 9.0, [3000.0, 2000.0]);
        assert!((r[2] - 1.0).abs() < 1e-6 && r[3] < 1.0);
    }
}
