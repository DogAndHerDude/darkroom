//! Darkroom's edit model, independent of the UI and the GPU.
//!
//! An [`Edit`] is everything Darkroom knows about one photo beyond its pixels: the adjustments,
//! the crop, the rating and the flag. It lives in a [`sidecar`] file next to the photo; the
//! original is never written.

pub mod curve;
pub mod edit;
pub mod exif;
pub mod geometry;
pub mod library;
pub mod raf;
pub mod sidecar;

pub use curve::Curve;
pub use edit::{Channel, Color, Curves, Detail, Edit, Flag, Geometry, Light, Picker};
