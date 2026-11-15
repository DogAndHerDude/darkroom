//! The adjustments on one photo, in the units the UI shows them in.
//!
//! Every field has a neutral default, and every struct reads with `#[serde(default)]`, so an edit
//! file only needs the values someone changed, and a file from an older version still opens.

use serde::{Deserialize, Serialize};

use crate::Curve;

/// The edit file format's version, written into every file.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Edit {
    pub version: u32,
    /// 0 to 5 stars.
    pub rating: u8,
    pub flag: Flag,
    pub light: Light,
    pub color: Color,
    pub curves: Curves,
    pub detail: Detail,
    pub geometry: Geometry,
}

impl Default for Edit {
    fn default() -> Self {
        Edit {
            version: VERSION,
            rating: 0,
            flag: Flag::None,
            light: Light::default(),
            color: Color::default(),
            curves: Curves::default(),
            detail: Detail::default(),
            geometry: Geometry::default(),
        }
    }
}

impl Edit {
    /// Whether anything changes the photo's pixels (rating and flag don't).
    pub fn has_adjustments(&self) -> bool {
        let neutral = Edit::default();
        self.light != neutral.light || self.color != neutral.color || self.curves != neutral.curves || self.detail != neutral.detail || self.geometry != neutral.geometry
    }

    /// The same edit with every adjustment reset, keeping rating and flag.
    pub fn without_adjustments(&self) -> Edit {
        Edit { rating: self.rating, flag: self.flag, ..Edit::default() }
    }

    /// Takes `from`'s Light, Color, Curves and Detail, for pasting one photo's look onto
    /// another. Crop, rating and flag belong to the photo and stay as they are.
    pub fn paste_look(&mut self, from: &Edit) {
        (self.light, self.color, self.curves, self.detail) = (from.light.clone(), from.color.clone(), from.curves.clone(), from.detail.clone());
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    #[default]
    None,
    Pick,
    Reject,
}

/// Light. Exposure is in stops (−3..3); everything else is −100..100.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    /// Off hides this section's edits without losing them (the eye icon).
    pub enabled: bool,
    pub exposure: f32,
    pub brilliance: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub black_point: f32,
}

impl Default for Light {
    fn default() -> Self {
        Light { enabled: true, exposure: 0.0, brilliance: 0.0, highlights: 0.0, shadows: 0.0, brightness: 0.0, contrast: 0.0, black_point: 0.0 }
    }
}

/// Color, all −100..100. Temperature and Tint are relative to the photo's own white balance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Color {
    pub enabled: bool,
    pub saturation: f32,
    pub vibrance: f32,
    pub temperature: f32,
    pub tint: f32,
}

impl Default for Color {
    fn default() -> Self {
        Color { enabled: true, saturation: 0.0, vibrance: 0.0, temperature: 0.0, tint: 0.0 }
    }
}

/// Curves on display-encoded values: a composite RGB curve, then one per channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curves {
    pub enabled: bool,
    pub rgb: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

impl Default for Curves {
    fn default() -> Self {
        Curves { enabled: true, rgb: Curve::identity(), red: Curve::identity(), green: Curve::identity(), blue: Curve::identity() }
    }
}

/// Which curve: the composite or one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channel {
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Channel::Rgb, Channel::Red, Channel::Green, Channel::Blue];

    pub fn name(self) -> &'static str {
        match self {
            Channel::Rgb => "RGB",
            Channel::Red => "Red",
            Channel::Green => "Green",
            Channel::Blue => "Blue",
        }
    }
}

/// The curve point pickers: what the picked pixel should become.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Black,
    Grey,
    White,
}

impl Picker {
    pub fn name(self) -> &'static str {
        match self {
            Picker::Black => "black point",
            Picker::Grey => "grey point",
            Picker::White => "white point",
        }
    }
}

impl Curves {
    /// Sets points on the red, green and blue curves so a pixel whose values reach the curves
    /// as `rgb` (0..1, the curves' encoding) comes out black, neutral grey or white. Grey keeps
    /// the pixel's own average brightness and only removes its cast.
    pub fn set_from_pixel(&mut self, picker: Picker, rgb: [f32; 3]) {
        let target = match picker {
            Picker::Black => 0.0,
            Picker::White => 1.0,
            Picker::Grey => (rgb[0] + rgb[1] + rgb[2]) / 3.0,
        };
        for (channel, v) in [Channel::Red, Channel::Green, Channel::Blue].into_iter().zip(rgb) {
            self.get_mut(channel).insert([v, target]);
        }
    }

    pub fn is_identity(&self) -> bool {
        Channel::ALL.iter().all(|&c| self.get(c).is_identity())
    }

    pub fn get(&self, channel: Channel) -> &Curve {
        match channel {
            Channel::Rgb => &self.rgb,
            Channel::Red => &self.red,
            Channel::Green => &self.green,
            Channel::Blue => &self.blue,
        }
    }

    pub fn get_mut(&mut self, channel: Channel) -> &mut Curve {
        match channel {
            Channel::Rgb => &mut self.rgb,
            Channel::Red => &mut self.red,
            Channel::Green => &mut self.green,
            Channel::Blue => &mut self.blue,
        }
    }
}

/// Detail. Definition is −100..100; the rest 0..100.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Detail {
    pub enabled: bool,
    pub definition: f32,
    pub sharpening: f32,
    pub noise_reduction: f32,
    pub vignette: f32,
}

impl Default for Detail {
    fn default() -> Self {
        Detail { enabled: true, definition: 0.0, sharpening: 0.0, noise_reduction: 0.0, vignette: 0.0 }
    }
}

/// Crop, straighten, rotate and flip, applied when displaying and exporting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Geometry {
    /// The kept area as fractions of the (rotated, flipped) image: x, y, width, height. None keeps
    /// everything.
    pub crop: Option<[f32; 4]>,
    /// Degrees, −45..45.
    pub straighten: f32,
    /// Quarter turns clockwise, 0..3.
    pub quarter_turns: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_edit_changes_nothing() {
        assert!(!Edit::default().has_adjustments());
    }

    #[test]
    fn rating_and_flag_are_not_adjustments() {
        let e = Edit { rating: 4, flag: Flag::Pick, ..Edit::default() };
        assert!(!e.has_adjustments());
    }

    #[test]
    fn hiding_a_section_counts_as_a_change() {
        let mut e = Edit::default();
        e.light.enabled = false;
        assert!(e.has_adjustments());
    }

    #[test]
    fn the_grey_picker_neutralizes_the_picked_pixel() {
        let mut c = Curves::default();
        let warm = [0.6, 0.5, 0.4];
        c.set_from_pixel(Picker::Grey, warm);
        let out = [c.red.eval(warm[0]), c.green.eval(warm[1]), c.blue.eval(warm[2])];
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-5), "{out:?}");
        assert!(c.rgb.is_identity(), "the composite curve is left alone");
    }

    #[test]
    fn the_black_and_white_pickers_map_the_pixel_to_the_ends() {
        let mut c = Curves::default();
        c.set_from_pixel(Picker::Black, [0.1, 0.08, 0.12]);
        c.set_from_pixel(Picker::White, [0.9, 0.95, 0.85]);
        assert!(c.red.eval(0.1) < 1e-5 && c.blue.eval(0.12) < 1e-5);
        assert!((c.green.eval(0.95) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pasting_a_look_keeps_crop_rating_and_flag() {
        let mut from = Edit { rating: 5, flag: Flag::Pick, ..Edit::default() };
        (from.light.exposure, from.color.saturation, from.detail.sharpening, from.curves.enabled) = (0.5, 20.0, 40.0, false);
        from.curves.red.insert([0.5, 0.6]);
        from.geometry.straighten = 3.0;
        let mut to = Edit { rating: 2, flag: Flag::Reject, ..Edit::default() };
        to.light.contrast = 30.0;
        to.geometry.crop = Some([0.1, 0.1, 0.5, 0.5]);
        let before = to.clone();
        to.paste_look(&from);
        assert_eq!((&to.light, &to.color, &to.curves, &to.detail), (&from.light, &from.color, &from.curves, &from.detail));
        assert_eq!((to.rating, to.flag, &to.geometry), (2, Flag::Reject, &before.geometry));
    }

    #[test]
    fn resetting_keeps_rating_and_flag() {
        let mut e = Edit { rating: 3, flag: Flag::Reject, ..Edit::default() };
        e.light.exposure = 1.0;
        let reset = e.without_adjustments();
        assert_eq!((reset.rating, reset.flag), (3, Flag::Reject));
        assert!(!reset.has_adjustments());
    }
}
