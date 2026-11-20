//! Darkroom design tokens for the spike, converted from the design system's `tokens.json`
//! (sRGB hex) to RUNE's linear `Color`. To be replaced by a module generated from `tokens.json`
//! at build time, as the design system's README asks.
#![allow(dead_code)]

use rune::Color;
use rune::tok::{Family, TextStyle};

/// `bg-canvas` #121110
pub const BG_CANVAS: Color = Color { r: 0.006049, g: 0.005605, b: 0.005182, a: 1.0 };
/// `bg-panel` #1a1917
pub const BG_PANEL: Color = Color { r: 0.010330, g: 0.009721, b: 0.008568, a: 1.0 };
/// `bg-raised` #242220
pub const BG_RAISED: Color = Color { r: 0.017642, g: 0.015996, b: 0.014444, a: 1.0 };
/// `bg-hover` #2c2a27
pub const BG_HOVER: Color = Color { r: 0.025187, g: 0.023153, b: 0.020289, a: 1.0 };
/// `bg-active` #35322e
pub const BG_ACTIVE: Color = Color { r: 0.035601, g: 0.031896, b: 0.027321, a: 1.0 };
/// `line` #2f2c29
pub const LINE: Color = Color { r: 0.028426, g: 0.025187, b: 0.022174, a: 1.0 };
/// `line-strong` #7a746b
pub const LINE_STRONG: Color = Color { r: 0.194618, g: 0.174647, b: 0.147027, a: 1.0 };
/// `ink` #ede7de
pub const INK: Color = Color { r: 0.846873, g: 0.799103, b: 0.730461, a: 1.0 };
/// `ink-muted` #aaa297
pub const INK_MUTED: Color = Color { r: 0.401978, g: 0.361307, b: 0.309469, a: 1.0 };
/// `ink-faint` #736c63
pub const INK_FAINT: Color = Color { r: 0.171441, g: 0.149960, b: 0.124772, a: 1.0 };
/// `accent` #e2a27a
pub const ACCENT: Color = Color { r: 0.760525, g: 0.361307, b: 0.194618, a: 1.0 };

// Type styles. Sizes and weights from the design system; the spike draws them in RUNE's built-in
// fonts until Rubik and DM Mono are bundled.
pub const DISPLAY: TextStyle = TextStyle::new(22.0, 500.0);
pub const TITLE: TextStyle = TextStyle::new(15.0, 500.0);
pub const HEADING: TextStyle = TextStyle::new(13.0, 500.0);
pub const BODY: TextStyle = TextStyle::new(13.0, 400.0);
pub const BODY_STRONG: TextStyle = TextStyle::new(13.0, 500.0);
pub const LABEL: TextStyle = TextStyle::new(12.0, 400.0);
pub const CAPTION: TextStyle = TextStyle::new(11.0, 400.0);
pub const VALUE: TextStyle = TextStyle { family: Family::Mono, ..TextStyle::new(12.0, 400.0) };
pub const VALUE_SM: TextStyle = TextStyle { family: Family::Mono, ..TextStyle::new(11.0, 400.0) };

// Photo-only colours: histogram and curves. Never in chrome.
/// `chan-red` #e0675e
pub const CHAN_RED: Color = Color { r: 0.745404, g: 0.135633, b: 0.111932, a: 1.0 };
/// `chan-green` #6fbf73
pub const CHAN_GREEN: Color = Color { r: 0.158961, g: 0.520996, b: 0.171441, a: 1.0 };
/// `chan-blue` #5f8fe0
pub const CHAN_BLUE: Color = Color { r: 0.114435, g: 0.274677, b: 0.745404, a: 1.0 };
/// `chan-luma` #cfc8bd
pub const CHAN_LUMA: Color = Color { r: 0.623960, g: 0.577580, b: 0.508881, a: 1.0 };

/// `clip-highlight` #ff3b4e
pub const CLIP_HIGHLIGHT: Color = Color { r: 1.000000, g: 0.043735, b: 0.076185, a: 1.0 };
/// `clip-shadow` #3d7bff
pub const CLIP_SHADOW: Color = Color { r: 0.046665, g: 0.198069, b: 1.000000, a: 1.0 };

/// `accent-soft` #3c2b20
pub const ACCENT_SOFT: Color = Color { r: 0.045186, g: 0.024158, b: 0.014444, a: 1.0 };
/// `accent-ink` #ecb38f
pub const ACCENT_INK: Color = Color { r: 0.838799, g: 0.450786, b: 0.274677, a: 1.0 };
/// `accent-hover` #ebb490
pub const ACCENT_HOVER: Color = Color { r: 0.830770, g: 0.456411, b: 0.278894, a: 1.0 };
/// `on-accent` #24170e
pub const ON_ACCENT: Color = Color { r: 0.017642, g: 0.008568, b: 0.004391, a: 1.0 };
/// `danger-ink` #ff8a80
pub const DANGER_INK: Color = Color { r: 1.000000, g: 0.254152, b: 0.215861, a: 1.0 };
/// `danger-soft` #3f1e1b
pub const DANGER_SOFT: Color = Color { r: 0.049707, g: 0.012983, b: 0.010960, a: 1.0 };
/// `warning` #f0c64e
pub const WARNING: Color = Color { r: 0.871367, g: 0.564712, b: 0.076185, a: 1.0 };
/// `warning-soft` #3a3017
pub const WARNING_SOFT: Color = Color { r: 0.042311, g: 0.029557, b: 0.008568, a: 1.0 };

// Selective Color's band swatches: each band's centre hue at Oklab lightness 0.72, as much chroma
// as sRGB holds up to 0.15. Not in the design system yet.
/// Red band #f47c6c
pub const BAND_RED: Color = Color { r: 0.904077, g: 0.201884, b: 0.151046, a: 1.0 };
/// Orange band #e78b30
pub const BAND_ORANGE: Color = Color { r: 0.802432, g: 0.257581, b: 0.030033, a: 1.0 };
/// Yellow band #b3a90f
pub const BAND_YELLOW: Color = Color { r: 0.451920, g: 0.395190, b: 0.004851, a: 1.0 };
/// Green band #69bc61
pub const BAND_GREEN: Color = Color { r: 0.141275, g: 0.500766, b: 0.118886, a: 1.0 };
/// Aqua band #16bbbc
pub const BAND_AQUA: Color = Color { r: 0.008132, g: 0.499824, b: 0.501391, a: 1.0 };
/// Blue band #69a5fe
pub const BAND_BLUE: Color = Color { r: 0.140518, g: 0.376530, b: 0.991900, a: 1.0 };
/// Purple band #ab90f9
pub const BAND_PURPLE: Color = Color { r: 0.404853, g: 0.278754, b: 0.943358, a: 1.0 };
/// Magenta band #d680d4
pub const BAND_MAGENTA: Color = Color { r: 0.673623, g: 0.216192, b: 0.656475, a: 1.0 };
/// The swatches in band order.
pub const BANDS: [Color; 8] = [BAND_RED, BAND_ORANGE, BAND_YELLOW, BAND_GREEN, BAND_AQUA, BAND_BLUE, BAND_PURPLE, BAND_MAGENTA];
