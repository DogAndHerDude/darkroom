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
    pub hsl: Hsl,
    pub black_and_white: BlackAndWhite,
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
            hsl: Hsl::default(),
            black_and_white: BlackAndWhite::default(),
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
        self.light != neutral.light || self.color != neutral.color || self.hsl != neutral.hsl || self.black_and_white != neutral.black_and_white || self.curves != neutral.curves || self.detail != neutral.detail || self.geometry != neutral.geometry
    }

    /// The same edit with every adjustment reset, keeping rating and flag.
    pub fn without_adjustments(&self) -> Edit {
        Edit { rating: self.rating, flag: self.flag, ..Edit::default() }
    }

    /// Takes `from`'s Light, Color, Selective Color, Black & White, Curves and Detail, for
    /// pasting one photo's look onto another. Crop, rating and flag belong to the photo and stay
    /// as they are.
    pub fn paste_look(&mut self, from: &Edit) {
        (self.light, self.color, self.curves, self.detail) = (from.light.clone(), from.color.clone(), from.curves.clone(), from.detail.clone());
        (self.hsl, self.black_and_white) = (from.hsl.clone(), from.black_and_white.clone());
    }

    /// Only what a paste or a preset carries; crop, rating and flag neutral.
    pub fn look(&self) -> Edit {
        let mut look = Edit::default();
        look.paste_look(self);
        look
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
    /// The composite curve moves luminance only, keeping each pixel's hue and saturation,
    /// instead of acting on red, green and blue alike. The channel curves are unaffected.
    pub luminance: bool,
    pub rgb: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

impl Default for Curves {
    fn default() -> Self {
        Curves { enabled: true, luminance: false, rgb: Curve::identity(), red: Curve::identity(), green: Curve::identity(), blue: Curve::identity() }
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

/// One colour band's Selective Color sliders, all −100..100.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Band {
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
}

/// Selective Color: hue, saturation and luminance per colour band (`bands.rs`), in band order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hsl {
    pub enabled: bool,
    pub red: Band,
    pub orange: Band,
    pub yellow: Band,
    pub green: Band,
    pub aqua: Band,
    pub blue: Band,
    pub purple: Band,
    pub magenta: Band,
}

impl Default for Hsl {
    fn default() -> Self {
        let b = Band::default();
        Hsl { enabled: true, red: b, orange: b, yellow: b, green: b, aqua: b, blue: b, purple: b, magenta: b }
    }
}

impl Hsl {
    pub fn bands(&self) -> [Band; 8] {
        [self.red, self.orange, self.yellow, self.green, self.aqua, self.blue, self.purple, self.magenta]
    }

    pub fn band_mut(&mut self, i: usize) -> &mut Band {
        [&mut self.red, &mut self.orange, &mut self.yellow, &mut self.green, &mut self.aqua, &mut self.blue, &mut self.purple, &mut self.magenta]
            .into_iter()
            .nth(i)
            .expect("one of the eight bands")
    }

    /// Whether no band is moved.
    pub fn is_neutral(&self) -> bool {
        self.bands().iter().all(|b| *b == Band::default())
    }
}

/// Black and white, with a mixer: how bright each colour band comes out in grey, −100..100.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlackAndWhite {
    /// None is as the camera shot it: on for a RAF shot in a monochrome simulation, off for
    /// everything else. So an unedited RAF in Acros opens in black and white with no edit file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    pub red: f32,
    pub orange: f32,
    pub yellow: f32,
    pub green: f32,
    pub aqua: f32,
    pub blue: f32,
    pub purple: f32,
    pub magenta: f32,
}

impl BlackAndWhite {
    /// Whether the photo shows in black and white; `camera` is what the camera shot.
    pub fn is_on(&self, camera: bool) -> bool {
        self.on.unwrap_or(camera)
    }

    pub fn mix(&self) -> [f32; 8] {
        [self.red, self.orange, self.yellow, self.green, self.aqua, self.blue, self.purple, self.magenta]
    }

    pub fn mix_mut(&mut self, i: usize) -> &mut f32 {
        [&mut self.red, &mut self.orange, &mut self.yellow, &mut self.green, &mut self.aqua, &mut self.blue, &mut self.purple, &mut self.magenta]
            .into_iter()
            .nth(i)
            .expect("one of the eight bands")
    }

    pub fn set_mix(&mut self, mix: [f32; 8]) {
        for (i, v) in mix.into_iter().enumerate() {
            *self.mix_mut(i) = v;
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
    fn curves_saved_before_the_luminance_mode_act_on_the_channels() {
        let c: Curves = toml::from_str("enabled = true").expect("parses");
        assert!(!c.luminance);
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
        (from.hsl.blue.saturation, from.black_and_white.on, from.black_and_white.red) = (30.0, Some(true), 20.0);
        from.curves.red.insert([0.5, 0.6]);
        from.geometry.straighten = 3.0;
        let mut to = Edit { rating: 2, flag: Flag::Reject, ..Edit::default() };
        to.light.contrast = 30.0;
        to.geometry.crop = Some([0.1, 0.1, 0.5, 0.5]);
        let before = to.clone();
        to.paste_look(&from);
        assert_eq!((&to.light, &to.color, &to.curves, &to.detail), (&from.light, &from.color, &from.curves, &from.detail));
        assert_eq!((&to.hsl, &to.black_and_white), (&from.hsl, &from.black_and_white));
        assert_eq!((to.rating, to.flag, &to.geometry), (2, Flag::Reject, &before.geometry));
    }

    #[test]
    fn selective_color_and_black_and_white_count_as_changes() {
        let mut e = Edit::default();
        e.hsl.band_mut(5).saturation = 10.0;
        assert!(e.has_adjustments());
        assert_eq!(e.hsl.blue.saturation, 10.0, "band 5 is Blue");
        let mut e = Edit::default();
        e.black_and_white.on = Some(false);
        assert!(e.has_adjustments(), "turning a camera's black and white off is an edit");
    }

    #[test]
    fn black_and_white_follows_the_camera_until_it_is_set() {
        let mut bw = BlackAndWhite::default();
        assert!(bw.is_on(true) && !bw.is_on(false));
        bw.on = Some(false);
        assert!(!bw.is_on(true));
    }

    #[test]
    fn the_new_sections_read_from_their_own_tables_and_leave_the_switch_out_unset() {
        let e: Edit = toml::from_str("[hsl]\norange = { saturation = -15, luminance = 10 }\n\n[black_and_white]\non = true\nblue = -60\n").expect("parses");
        assert_eq!((e.hsl.orange.saturation, e.hsl.orange.luminance, e.hsl.orange.hue), (-15.0, 10.0, 0.0));
        assert_eq!((e.black_and_white.on, e.black_and_white.blue), (Some(true), -60.0));
        let written = toml::to_string(&Edit::default()).expect("writes");
        assert!(!written.lines().any(|l| l.starts_with("on ")), "{written}");
        assert_eq!(toml::from_str::<Edit>(&written).expect("reads back"), Edit::default());
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
