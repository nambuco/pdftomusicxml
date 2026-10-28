//! The conversion pipeline: PDF or image in, [`Score`] out.
//!
//! Each stage lives in its own crate; this crate wires them together.

use std::path::Path;

use image::GrayImage;
use scorelift_model::Metadata;
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
                image: gray_on_white(&image::open(input)?),
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

/// Grayscale with any transparency composited onto a white page (notation
/// programs export PNGs with a transparent background).
fn gray_on_white(img: &image::DynamicImage) -> GrayImage {
    let rgba = img.to_rgba8();
    GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let luma = (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000;
        let ink = (255 - luma) * u32::from(a) / 255;
        image::Luma([(255 - ink) as u8])
    })
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
    let mut metadata = Metadata::default();
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
        metadata = text_layer_metadata(&doc);
    }

    // Image path: find staves, recognise symbols, then interpret them with
    // the same code as the vector path.
    let analysis = analyze(input)?;
    let mut systems = Vec::new();
    for sheet in &analysis.sheets {
        if let Some(layout) = &sheet.layout {
            systems.extend(scorelift_detect::recognize(
                &sheet.image,
                layout,
                sheet.page,
            ));
        }
    }
    if systems.is_empty() {
        return Err(Error::NotImplemented("reading files without any staff"));
    }
    let mut reading = scorelift_semantic::read_systems(systems, Vec::new());
    if reading.score.metadata.title.is_none() {
        reading.score.metadata = metadata;
    }
    let mut warnings = analysis.warnings;
    warnings.extend(reading.warnings);
    Ok(Conversion {
        score: reading.score,
        warnings,
    })
}

/// Title, composer and other credits from the real text of an image-only
/// PDF (web exports keep the title as text and draw only the music).
fn text_layer_metadata(doc: &scorelift_pdf::PdfDocument) -> Metadata {
    let mut meta = Metadata::default();
    let Some(page) = doc.pages.first() else {
        return meta;
    };
    let glyphs: Vec<&scorelift_pdf::Glyph> = page.glyphs.iter().collect();
    let words = scorelift_semantic::layout::words(&glyphs);
    // Join words on one baseline into lines: (text, size, y).
    let mut lines: Vec<(String, f32, f32)> = Vec::new();
    for w in words {
        match lines
            .iter_mut()
            .find(|l| (l.2 - w.y).abs() < 0.2 * w.size && (l.1 - w.size).abs() < 0.1 * w.size)
        {
            Some(l) => {
                l.0.push(' ');
                l.0.push_str(&w.text);
            }
            None => lines.push((w.text.clone(), w.size, w.y)),
        }
    }
    for (text, ..) in &lines {
        if let Some((name, value)) = text.split_once(':')
            && name.trim().eq_ignore_ascii_case("tuning")
        {
            meta.misc.push(("tuning".into(), value.trim().to_string()));
        }
    }
    lines.retain(|l| !l.0.contains(':'));
    lines.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.total_cmp(&b.2)));
    let body = lines.iter().map(|l| l.1).fold(f32::MAX, f32::min);
    let mut big = lines
        .iter()
        .filter(|l| l.1 > body * 1.2 || lines.len() <= 2);
    meta.title = big.next().map(|l| l.0.clone());
    meta.composer = big.next().map(|l| l.0.clone());
    meta
}

/// The result of [`convert`].
#[derive(Debug)]
pub struct Conversion {
    pub score: Score,
    /// Parts of the input that could not be read.
    pub warnings: Vec<String>,
}
