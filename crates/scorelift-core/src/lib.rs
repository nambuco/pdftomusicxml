//! The conversion pipeline: PDF or image in, [`Score`] out.
//!
//! Each stage lives in its own crate; this crate wires them together.

use std::path::Path;

pub use scorelift_model::Score;

/// The pipeline steps, in order. Modelled on Audiveris' named steps so a
/// caller can stop after any step and inspect its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Load,
    Binary,
    Scale,
    Grid,
    Symbols,
    Interpret,
    Rhythms,
    Page,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not implemented yet")]
    NotImplemented(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Options for [`convert`].
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Stop after this step. `None` runs the whole pipeline.
    pub until: Option<Step>,
}

/// Converts a PDF or image file into a [`Score`].
pub fn convert(input: &Path, _options: &ConvertOptions) -> Result<Score, Error> {
    if !input.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} does not exist", input.display()),
        )));
    }
    Err(Error::NotImplemented("PDF recognition"))
}
