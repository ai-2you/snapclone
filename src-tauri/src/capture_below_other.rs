//! Non-macOS placeholder for `capture_below`, selected via `#[path]` in `lib.rs`. Phase 2 fills
//! in a real Windows implementation behind `#[cfg(windows)]` without changing this signature.

use xcap::image::RgbaImage;

use crate::geometry::Rect;

pub fn capture(_rect: Rect, _below: u32) -> Result<RgbaImage, String> {
    Err("scroll capture is only available on macOS".into())
}
