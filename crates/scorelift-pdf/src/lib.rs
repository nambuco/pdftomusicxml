//! PDF ingest: finds the raster images drawn on each page, decodes them to
//! grayscale at their native resolution, and records where they sit on the
//! page. Also extracts the page's text layer (titles, credits).
//!
//! Pure Rust (via `lopdf`), so there is no native PDF library to ship.
//! Rendering vector pages is not needed here: vector scores take the
//! glyph-reading fast path (milestone M2) instead.

mod image_decode;

use std::path::Path;

use image::GrayImage;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use serde::Serialize;

/// Upper bound for any single decompressed stream, against decompression bombs.
const MAX_STREAM_BYTES: usize = 512 * 1024 * 1024;
/// Form XObjects can nest; stop at this depth to avoid cycles.
const MAX_FORM_DEPTH: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read PDF: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct PdfDocument {
    pub pages: Vec<PdfPage>,
}

#[derive(Debug)]
pub struct PdfPage {
    /// 1-based page number.
    pub number: u32,
    /// Page size in points (1/72 inch).
    pub width: f32,
    pub height: f32,
    /// Raster images in drawing order.
    pub images: Vec<PageImage>,
    pub text: String,
    /// Things that could not be read, e.g. unsupported image encodings.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct PageImage {
    /// The XObject resource name, e.g. `X7`.
    pub name: String,
    /// Where the image is drawn, in points from the page's top-left corner.
    pub bbox: Rect,
    /// Decoded pixels, already oriented as they appear on the page.
    pub image: GrayImage,
}

impl PageImage {
    /// Effective resolution of the image on the page, in dots per inch.
    pub fn dpi(&self) -> f32 {
        if self.bbox.w <= 0.0 {
            return 0.0;
        }
        self.image.width() as f32 / (self.bbox.w / 72.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn load(path: &Path) -> Result<PdfDocument, Error> {
    load_mem(&std::fs::read(path)?)
}

pub fn load_mem(bytes: &[u8]) -> Result<PdfDocument, Error> {
    let doc = Document::load_mem(bytes)?;
    let pages = doc
        .get_pages()
        .into_iter()
        .map(|(number, id)| read_page(&doc, number, id))
        .collect();
    Ok(PdfDocument { pages })
}

fn read_page(doc: &Document, number: u32, page_id: ObjectId) -> PdfPage {
    let mut warnings = Vec::new();
    let media_box = page_media_box(doc, page_id).unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let mut walker = Walker {
        doc,
        page_origin: (media_box[0], media_box[3]),
        images: Vec::new(),
        warnings: &mut warnings,
    };

    let resources = match doc.get_page_resources(page_id) {
        Ok((own, inherited)) => {
            let mut chain: Vec<&Dictionary> = own.into_iter().collect();
            chain.extend(
                inherited
                    .into_iter()
                    .filter_map(|id| doc.get_dictionary(id).ok()),
            );
            chain
        }
        Err(e) => {
            walker.warnings.push(format!("page resources: {e}"));
            Vec::new()
        }
    };
    match doc.get_page_content_with_limit(page_id, MAX_STREAM_BYTES) {
        Ok(content) => walker.walk(&content, &resources, IDENTITY, 0),
        Err(e) => walker.warnings.push(format!("page content: {e}")),
    }
    let images = walker.images;

    let text = doc.extract_text(&[number]).unwrap_or_default();
    PdfPage {
        number,
        width: (media_box[2] - media_box[0]) as f32,
        height: (media_box[3] - media_box[1]) as f32,
        images,
        text,
        warnings,
    }
}

fn page_media_box(doc: &Document, page_id: ObjectId) -> Option<[f64; 4]> {
    // MediaBox is inheritable, so walk up the page tree.
    let mut dict = doc.get_dictionary(page_id).ok()?;
    for _ in 0..32 {
        if let Ok(obj) = dict.get(b"MediaBox") {
            let arr = match deref(doc, obj) {
                Object::Array(a) if a.len() == 4 => a,
                _ => return None,
            };
            let mut out = [0.0; 4];
            for (o, v) in out.iter_mut().zip(arr) {
                *o = number(deref(doc, v))?;
            }
            return Some(out);
        }
        let parent = dict.get(b"Parent").ok()?;
        dict = match deref(doc, parent) {
            Object::Dictionary(d) => d,
            _ => return None,
        };
    }
    None
}

/// A PDF transformation matrix `[a b c d e f]`.
type Matrix = [f64; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m` then `n`, i.e. the PDF product `m × n`.
fn multiply(m: Matrix, n: Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn apply(m: Matrix, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

pub(crate) fn deref<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    doc.dereference(obj)
        .map(|(_, o)| o)
        .unwrap_or(&Object::Null)
}

pub(crate) fn number(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

struct Walker<'a> {
    doc: &'a Document,
    /// Top-left corner of the page in PDF user space.
    page_origin: (f64, f64),
    images: Vec<PageImage>,
    warnings: &'a mut Vec<String>,
}

impl<'a> Walker<'a> {
    fn find_xobject(&self, resources: &[&'a Dictionary], name: &[u8]) -> Option<&'a Stream> {
        for res in resources {
            let Ok(xobjects) = res.get(b"XObject") else {
                continue;
            };
            let Object::Dictionary(xobjects) = deref(self.doc, xobjects) else {
                continue;
            };
            if let Ok(obj) = xobjects.get(name)
                && let Object::Stream(s) = deref(self.doc, obj)
            {
                return Some(s);
            }
        }
        None
    }

    fn walk(&mut self, content: &[u8], resources: &[&'a Dictionary], ctm: Matrix, depth: usize) {
        let content = match Content::decode(content) {
            Ok(c) => c,
            Err(e) => {
                self.warnings.push(format!("content stream: {e}"));
                return;
            }
        };
        let mut stack = Vec::new();
        let mut ctm = ctm;
        for op in &content.operations {
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(ctm),
                "cm" => {
                    let m: Vec<f64> = op.operands.iter().filter_map(number).collect();
                    if let Ok(m) = <[f64; 6]>::try_from(m) {
                        ctm = multiply(m, ctm);
                    }
                }
                "Do" => {
                    let Some(Object::Name(name)) = op.operands.first() else {
                        continue;
                    };
                    let Some(stream) = self.find_xobject(resources, name) else {
                        continue;
                    };
                    let name = String::from_utf8_lossy(name).into_owned();
                    match stream.dict.get(b"Subtype") {
                        Ok(Object::Name(s)) if s == b"Image" => {
                            self.place_image(&name, stream, ctm)
                        }
                        Ok(Object::Name(s)) if s == b"Form" => {
                            self.walk_form(stream, resources, ctm, depth)
                        }
                        _ => {}
                    }
                }
                "BI" => self
                    .warnings
                    .push("inline images are not supported yet".into()),
                _ => {}
            }
        }
    }

    fn walk_form(
        &mut self,
        form: &'a Stream,
        parent: &[&'a Dictionary],
        ctm: Matrix,
        depth: usize,
    ) {
        if depth >= MAX_FORM_DEPTH {
            self.warnings.push("form XObjects nested too deeply".into());
            return;
        }
        let matrix = match form.dict.get(b"Matrix").map(|m| deref(self.doc, m)) {
            Ok(Object::Array(a)) => {
                let m: Vec<f64> = a.iter().filter_map(number).collect();
                <[f64; 6]>::try_from(m).unwrap_or(IDENTITY)
            }
            _ => IDENTITY,
        };
        let own = match form.dict.get(b"Resources").map(|r| deref(self.doc, r)) {
            Ok(Object::Dictionary(d)) => Some(d),
            _ => None,
        };
        let resources: Vec<&Dictionary> = own.into_iter().chain(parent.iter().copied()).collect();
        match form.decompressed_content_with_limit(MAX_STREAM_BYTES) {
            Ok(content) => self.walk(&content, &resources, multiply(matrix, ctm), depth + 1),
            Err(e) => self.warnings.push(format!("form content: {e}")),
        }
    }

    fn place_image(&mut self, name: &str, stream: &Stream, ctm: Matrix) {
        let mut image = match image_decode::decode(self.doc, stream) {
            Ok(img) => img,
            Err(e) => {
                self.warnings.push(format!("image {name}: {e}"));
                return;
            }
        };
        // The image fills the unit square; row 0 is at y = 1.
        let corners =
            [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(x, y)| apply(ctm, x, y));
        let (min_x, max_x, min_y, max_y) = corners.iter().fold(
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
            |(a, b, c, d), &(x, y)| (a.min(x), b.max(x), c.min(y), d.max(y)),
        );
        if ctm[1].abs() > 1e-6 || ctm[2].abs() > 1e-6 {
            self.warnings.push(format!(
                "image {name} is rotated; orientation not corrected"
            ));
        } else {
            if ctm[0] < 0.0 {
                image::imageops::flip_horizontal_in_place(&mut image);
            }
            if ctm[3] < 0.0 {
                image::imageops::flip_vertical_in_place(&mut image);
            }
        }
        let (ox, oy) = self.page_origin;
        self.images.push(PageImage {
            name: name.to_string(),
            bbox: Rect {
                x: (min_x - ox) as f32,
                y: (oy - max_y) as f32,
                w: (max_x - min_x) as f32,
                h: (max_y - min_y) as f32,
            },
            image,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_product_matches_pdf_order() {
        // Scale by 2, then translate by (10, 20).
        let m = multiply(
            [2.0, 0.0, 0.0, 2.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0, 10.0, 20.0],
        );
        assert_eq!(apply(m, 1.0, 1.0), (12.0, 22.0));
    }
}
