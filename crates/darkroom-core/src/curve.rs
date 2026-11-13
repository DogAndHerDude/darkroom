//! Tone curves: control points joined by a monotone cubic (Fritsch–Carlson), so a curve never
//! overshoots between its points and a rising set of points always gives a rising curve.

use serde::{Deserialize, Serialize};

/// A curve on 0..1 → 0..1, as control points `[input, output]`. Always at least two points,
/// sorted by input, with distinct inputs (see [`Curve::new`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<[f32; 2]>", into = "Vec<[f32; 2]>")]
pub struct Curve {
    points: Vec<[f32; 2]>,
}

/// Inputs closer than this are one point.
const MIN_GAP: f32 = 1.0 / 256.0;

impl Default for Curve {
    fn default() -> Self {
        Curve::identity()
    }
}

impl From<Vec<[f32; 2]>> for Curve {
    fn from(points: Vec<[f32; 2]>) -> Self {
        Curve::new(points)
    }
}

impl From<Curve> for Vec<[f32; 2]> {
    fn from(c: Curve) -> Self {
        c.points
    }
}

impl Curve {
    pub fn identity() -> Curve {
        Curve { points: vec![[0.0, 0.0], [1.0, 1.0]] }
    }

    /// A curve through `points`, clamped to 0..1, sorted, with near-duplicate inputs merged.
    /// Fewer than two points give the identity.
    pub fn new(points: Vec<[f32; 2]>) -> Curve {
        let mut pts: Vec<[f32; 2]> =
            points.into_iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| p.map(|v| v.clamp(0.0, 1.0))).collect();
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        pts.dedup_by(|b, a| b[0] - a[0] < MIN_GAP);
        if pts.len() < 2 {
            return Curve::identity();
        }
        Curve { points: pts }
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.points
    }

    pub fn is_identity(&self) -> bool {
        self.points.iter().all(|p| (p[0] - p[1]).abs() < 1e-6) && self.points[0][0] == 0.0 && self.points[self.points.len() - 1][0] == 1.0
    }

    /// Adds a point, or moves the existing point at (nearly) the same input to it. Returns the
    /// point's index.
    pub fn insert(&mut self, p: [f32; 2]) -> usize {
        let p = p.map(|v| v.clamp(0.0, 1.0));
        if let Some(i) = self.points.iter().position(|q| (q[0] - p[0]).abs() < MIN_GAP) {
            self.points[i][1] = p[1];
            return i;
        }
        let i = self.points.partition_point(|q| q[0] < p[0]);
        self.points.insert(i, p);
        i
    }

    /// Moves point `i` to `p`, kept between its neighbours so the points stay in order (a point
    /// can't be dragged past another). Out-of-range `i` does nothing.
    pub fn move_point(&mut self, i: usize, p: [f32; 2]) {
        let n = self.points.len();
        if i >= n {
            return;
        }
        let lo = if i == 0 { 0.0 } else { self.points[i - 1][0] + MIN_GAP };
        let hi = if i == n - 1 { 1.0 } else { self.points[i + 1][0] - MIN_GAP };
        self.points[i] = [p[0].clamp(lo, hi.max(lo)), p[1].clamp(0.0, 1.0)];
    }

    /// Removes point `i`. The two end points stay, so a curve always spans its range; returns
    /// whether a point was removed.
    pub fn remove(&mut self, i: usize) -> bool {
        if i == 0 || i + 1 >= self.points.len() {
            return false;
        }
        self.points.remove(i);
        true
    }

    /// The point closest to `p` within `radius` (in curve units), if any.
    pub fn nearest(&self, p: [f32; 2], radius: f32) -> Option<usize> {
        let d = |q: &[f32; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
        self.points.iter().enumerate().filter(|(_, q)| d(q) <= radius).min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map(|(i, _)| i)
    }

    /// Tangents at each point (Fritsch–Carlson).
    fn tangents(&self) -> Vec<f32> {
        let p = &self.points;
        let n = p.len();
        let d: Vec<f32> = p.windows(2).map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0])).collect();
        let mut m = vec![0.0; n];
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for i in 1..n - 1 {
            m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
        }
        for i in 0..n - 1 {
            if d[i] == 0.0 {
                m[i] = 0.0;
                m[i + 1] = 0.0;
                continue;
            }
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
        m
    }

    /// The curve at `x` (clamped to 0..1). Flat before the first point and after the last.
    pub fn eval(&self, x: f32) -> f32 {
        self.sample(&self.tangents(), x)
    }

    fn sample(&self, m: &[f32], x: f32) -> f32 {
        let p = &self.points;
        let x = x.clamp(0.0, 1.0);
        if x <= p[0][0] {
            return p[0][1];
        }
        if x >= p[p.len() - 1][0] {
            return p[p.len() - 1][1];
        }
        let i = p.windows(2).position(|w| x < w[1][0]).unwrap_or(p.len() - 2);
        let (x0, y0, x1, y1) = (p[i][0], p[i][1], p[i + 1][0], p[i + 1][1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        (h00 * y0 + h10 * h * m[i] + h01 * y1 + h11 * h * m[i + 1]).clamp(0.0, 1.0)
    }

    /// `n` evenly spaced samples from 0 to 1 inclusive, for a GPU lookup table.
    pub fn lut(&self, n: usize) -> Vec<f32> {
        let m = self.tangents();
        (0..n).map(|i| self.sample(&m, i as f32 / (n - 1) as f32)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_the_diagonal() {
        let c = Curve::identity();
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            assert!((c.eval(x) - x).abs() < 1e-6);
        }
        assert!(c.is_identity());
    }

    #[test]
    fn the_curve_passes_through_its_points() {
        let c = Curve::new(vec![[0.0, 0.04], [0.25, 0.21], [0.5, 0.5], [0.75, 0.79], [1.0, 0.97]]);
        for p in c.points() {
            assert!((c.eval(p[0]) - p[1]).abs() < 1e-5, "{p:?}");
        }
        assert!(!c.is_identity());
    }

    #[test]
    fn rising_points_never_overshoot_or_dip() {
        // A steep step: a natural cubic spline would ring here.
        let c = Curve::new(vec![[0.0, 0.0], [0.45, 0.1], [0.55, 0.9], [1.0, 1.0]]);
        let lut = c.lut(1024);
        assert!(lut.windows(2).all(|w| w[1] >= w[0] - 1e-6), "monotone");
        assert!(lut.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn points_are_sorted_clamped_and_deduplicated() {
        let c = Curve::new(vec![[1.2, 1.0], [0.5, 0.6], [0.0, -0.1], [0.5001, 0.7]]);
        assert_eq!(c.points(), &[[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]]);
    }

    #[test]
    fn too_few_points_fall_back_to_identity() {
        assert_eq!(Curve::new(vec![[0.3, 0.3]]), Curve::identity());
    }

    #[test]
    fn inserting_keeps_points_in_order_and_returns_the_index() {
        let mut c = Curve::identity();
        assert_eq!(c.insert([0.5, 0.6]), 1);
        assert_eq!(c.insert([0.25, 0.2]), 1);
        assert_eq!(c.points(), &[[0.0, 0.0], [0.25, 0.2], [0.5, 0.6], [1.0, 1.0]]);
    }

    #[test]
    fn inserting_on_an_existing_input_moves_that_point() {
        let mut c = Curve::new(vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]);
        assert_eq!(c.insert([0.5005, 0.7]), 1);
        assert_eq!(c.points().len(), 3);
        assert_eq!(c.points()[1], [0.5, 0.7]);
    }

    #[test]
    fn a_dragged_point_cant_pass_its_neighbours() {
        let mut c = Curve::new(vec![[0.0, 0.0], [0.3, 0.3], [0.6, 0.6], [1.0, 1.0]]);
        c.move_point(1, [0.9, 1.4]);
        let p = c.points()[1];
        assert!(p[0] < 0.6 && p[1] == 1.0, "{p:?}");
        c.move_point(0, [0.2, 0.1]);
        assert_eq!(c.points()[0], [0.2, 0.1], "an end point can move inwards");
    }

    #[test]
    fn end_points_cant_be_removed() {
        let mut c = Curve::new(vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]]);
        assert!(!c.remove(0));
        assert!(!c.remove(2));
        assert!(c.remove(1));
        assert_eq!(c, Curve::identity());
    }

    #[test]
    fn nearest_finds_the_closest_point_within_reach() {
        let c = Curve::new(vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]]);
        assert_eq!(c.nearest([0.52, 0.58], 0.05), Some(1));
        assert_eq!(c.nearest([0.3, 0.9], 0.05), None);
    }

    #[test]
    fn a_raised_black_point_holds_flat_before_the_first_point() {
        let c = Curve::new(vec![[0.1, 0.05], [1.0, 1.0]]);
        assert!((c.eval(0.0) - 0.05).abs() < 1e-6);
    }
}
