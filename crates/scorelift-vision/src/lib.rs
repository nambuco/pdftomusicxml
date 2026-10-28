//! Image analysis for one sheet image: binarization (BINARY), musical
//! scale (SCALE), and staves, barlines and measures (GRID).

mod binary;
mod grid;
mod overlay;
mod scale;

use image::GrayImage;
use serde::Serialize;

pub use binary::{BinaryImage, binarize, otsu_threshold};
pub use grid::{Barline, LineBand, MeasureSpan, Staff, find_staves, line_bands};
pub use overlay::draw_overlay;
pub use scale::{Scale, estimate_scale};

/// Everything the vision steps found in one image.
#[derive(Debug, Clone, Serialize)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// Gray level at or below which a pixel counts as ink.
    pub threshold: u8,
    pub scale: Scale,
    pub staves: Vec<Staff>,
}

impl Layout {
    pub fn measure_count(&self) -> usize {
        self.staves.iter().map(|s| s.measures.len()).sum()
    }
}

/// Runs BINARY, SCALE and GRID. Returns `None` when the image has no
/// recognisable staff (a title block, a picture, an empty area).
pub fn analyze(gray: &GrayImage) -> Option<Layout> {
    let threshold = otsu_threshold(gray);
    let bin = binarize(gray, threshold);
    let scale = estimate_scale(&bin)?;
    let staves = find_staves(gray, &bin, &scale);
    if staves.is_empty() {
        return None;
    }
    Some(Layout {
        width: gray.width(),
        height: gray.height(),
        threshold,
        scale,
        staves,
    })
}
