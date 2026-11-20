//! Darkroom's edit model, independent of the UI and the GPU.
//!
//! An [`Edit`] is everything Darkroom knows about one photo beyond its pixels: the adjustments,
//! the crop, the rating and the flag. It lives in a [`sidecar`] file next to the photo; the
//! original is never written.

pub mod bands;
pub mod camera_look;
pub mod curve;
pub mod edit;
pub mod exif;
pub mod fuji;
pub mod geometry;
pub mod history;
pub mod library;
pub mod preset;
pub mod raf;
pub mod sidecar;
pub mod white;

pub use curve::Curve;
pub use history::History;
pub use edit::{Band, BlackAndWhite, Channel, Color, Curves, Detail, Edit, Flag, Geometry, Hsl, Light, Picker};
