//! The conversion pipeline: PDF or image in, [`Score`] out.
//!
//! Each stage lives in its own crate; this crate wires them together.

use std::path::Path;

use image::GrayImage;
pub use scorelift_model::Score;
pub use scorelift_pdf::Rect;
pub use scorelift_vision::Layout;

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
    #[error("unsupported input file type: {0}")]
    UnsupportedInput(String),
    #[error(transparent)]
    Pdf(#[from] scorelift_pdf::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Options for [`convert`].
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Stop after this step. `None` runs the whole pipeline.
    pub until: Option<Step>,
}

/// One raster image of music: a whole scan, or one image embedded in a PDF page.
#[derive(Debug)]
pub struct Sheet {
    /// 1-based page number (1 for image files).
    pub page: u32,
    /// The image's name in the PDF, or the file name.
    pub name: String,
    /// Placement on the PDF page, in points from the top-left corner.
    pub bbox: Option<Rect>,
    pub image: GrayImage,
    /// `None` if no staff was found in the image.
    pub layout: Option<Layout>,
}

#[derive(Debug)]
pub struct Analysis {
    pub sheets: Vec<Sheet>,
    /// Text layer of each PDF page, as (page number, text).
    pub page_text: Vec<(u32, String)>,
    pub warnings: Vec<String>,
}

/// Runs LOAD through GRID: finds every image of music in `input` and its
/// staves, barlines and measures.
pub fn analyze(input: &Path) -> Result<Analysis, Error> {
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut analysis = match ext.as_str() {
        "pdf" => load_pdf(input)?,
        "png" | "jpg" | "jpeg" => Analysis {
            sheets: vec![Sheet {
                page: 1,
                name: input
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                bbox: None,
                image: image::open(input)?.to_luma8(),
                layout: None,
            }],
            page_text: Vec::new(),
            warnings: Vec::new(),
        },
        _ => return Err(Error::UnsupportedInput(input.display().to_string())),
    };
    for sheet in &mut analysis.sheets {
        sheet.layout = scorelift_vision::analyze(&sheet.image);
    }
    Ok(analysis)
}

fn load_pdf(input: &Path) -> Result<Analysis, Error> {
    let doc = scorelift_pdf::load(input)?;
    let mut analysis = Analysis {
        sheets: Vec::new(),
        page_text: Vec::new(),
        warnings: Vec::new(),
    };
    for page in doc.pages {
        if page.images.is_empty() {
            analysis.warnings.push(format!(
                "page {}: no raster images (vector pages are read in milestone M2)",
                page.number
            ));
        }
        analysis.warnings.extend(
            page.warnings
                .into_iter()
                .map(|w| format!("page {}: {w}", page.number)),
        );
        analysis.page_text.push((page.number, page.text));
        for img in page.images {
            analysis.sheets.push(Sheet {
                page: page.number,
                name: img.name,
                bbox: Some(img.bbox),
                image: img.image,
                layout: None,
            });
        }
    }
    Ok(analysis)
}

/// Converts a PDF or image file into a [`Score`].
pub fn convert(input: &Path, _options: &ConvertOptions) -> Result<Conversion, Error> {
    if !input.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} does not exist", input.display()),
        )));
    }
    let is_pdf = input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        // Fast path: PDFs exported from notation software carry the music
        // as SMuFL glyphs and vector lines, so nothing needs recognising.
        let doc = scorelift_pdf::load(input)?;
        if let Some(reading) = scorelift_semantic::read_vector(&doc) {
            return Ok(Conversion {
                score: reading.score,
                warnings: reading.warnings,
            });
        }
    }
    Err(Error::NotImplemented(
        "recognition of scanned or image-only scores (this file has no music font)",
    ))
}

/// The result of [`convert`].
#[derive(Debug)]
pub struct Conversion {
    pub score: Score,
    /// Parts of the input that could not be read.
    pub warnings: Vec<String>,
}
