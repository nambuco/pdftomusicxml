//! Symbol recognition for raster scores (milestone M3): classical methods,
//! no machine learning. Staff lines are erased, stems and beams found by
//! their shape, and fixed-shape symbols matched against Bravura templates.
//! The result is the same `System` structure the vector reader builds, so
//! pitch, rhythm and voices are worked out by the same interpreter.

mod recognize;
mod templates;

pub use recognize::{probe, recognize};
pub use templates::{TEMPLATE_GLYPHS, Template, load as load_templates};
