//! PDF ingest. For each page this records:
//!
//! - the raster images drawn on it, decoded to grayscale at their native
//!   resolution, with their placement (scanned or image-only scores);
//! - every text glyph with its font, Unicode text, position and size, and
//!   every stroked or filled path (vector scores: music fonts expose SMuFL
//!   code points, staff lines and stems are plain strokes);
//! - the plain text layer.
//!
//! All coordinates are in points from the page's top-left corner, y down.
//! Pure Rust (via `lopdf`), so there is no native PDF library to ship.

mod fonts;
mod image_decode;

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use image::GrayImage;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use serde::Serialize;

use fonts::FontInfo;

/// Upper bound for any single decompressed stream, against decompression bombs.
const MAX_STREAM_BYTES: usize = 512 * 1024 * 1024;
/// Form XObjects can nest; stop at this depth to avoid cycles.
const MAX_FORM_DEPTH: usize = 12;
/// Line segments used to approximate one Bézier curve.
const CURVE_STEPS: usize = 8;

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
    /// Text glyphs in drawing order.
    pub glyphs: Vec<Glyph>,
    /// Painted paths in drawing order (white fills are left out).
    pub shapes: Vec<Shape>,
    pub text: String,
    /// Things that could not be read, e.g. unsupported image encodings.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct PageImage {
    /// The XObject resource name, e.g. `X7`.
    pub name: String,
    /// Where the image is drawn.
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// One shown character.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Glyph {
    /// Font name without subset prefix, e.g. `Leland` or `Bravura`.
    pub font: String,
    /// Unicode text of the glyph (SMuFL code points for music fonts).
    /// Empty if the font has no Unicode mapping.
    pub text: String,
    /// The raw character code.
    pub code: u32,
    /// Glyph origin on the baseline.
    pub x: f32,
    pub y: f32,
    /// Font size on the page, in points.
    pub size: f32,
    /// Advance width on the page, in points.
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shape {
    /// Line width if the path was stroked.
    pub stroke: Option<f32>,
    pub filled: bool,
    /// Curves are flattened into line segments.
    pub subpaths: Vec<Vec<Point>>,
    /// The path contained Bézier curves (ties, slurs, glyph outlines).
    pub curved: bool,
}

impl Shape {
    pub fn bbox(&self) -> Rect {
        let pts = self.subpaths.iter().flatten();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }
}

pub fn load(path: &Path) -> Result<PdfDocument, Error> {
    load_mem(&std::fs::read(path)?)
}

pub fn load_mem(bytes: &[u8]) -> Result<PdfDocument, Error> {
    let doc = Document::load_mem(bytes)?;
    let mut fonts = HashMap::new();
    let pages = doc
        .get_pages()
        .into_iter()
        .map(|(number, id)| read_page(&doc, number, id, &mut fonts))
        .collect();
    Ok(PdfDocument { pages })
}

fn read_page(
    doc: &Document,
    number: u32,
    page_id: ObjectId,
    fonts: &mut HashMap<ObjectId, Rc<FontInfo>>,
) -> PdfPage {
    let mut warnings = Vec::new();
    let media_box = page_media_box(doc, page_id).unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let mut walker = Walker {
        doc,
        page_origin: (media_box[0], media_box[3]),
        images: Vec::new(),
        glyphs: Vec::new(),
        shapes: Vec::new(),
        fonts,
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
        Ok(content) => walker.walk(&content, &resources, GState::new(IDENTITY), 0),
        Err(e) => walker.warnings.push(format!("page content: {e}")),
    }
    let Walker {
        images,
        glyphs,
        shapes,
        ..
    } = walker;

    let text = doc.extract_text(&[number]).unwrap_or_default();
    PdfPage {
        number,
        width: (media_box[2] - media_box[0]) as f32,
        height: (media_box[3] - media_box[1]) as f32,
        images,
        glyphs,
        shapes,
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

fn translate(tx: f64, ty: f64) -> Matrix {
    [1.0, 0.0, 0.0, 1.0, tx, ty]
}

/// How much `m` scales lengths, on average.
fn scale_of(m: Matrix) -> f64 {
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
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

fn numbers(operands: &[Object]) -> Vec<f64> {
    operands.iter().filter_map(number).collect()
}

/// Lightness (0 black, 1 white) of a color given as 1 (gray), 3 (RGB) or 4 (CMYK) components.
fn lightness(c: &[f64]) -> f64 {
    match c {
        [g] => *g,
        [r, g, b] => 0.299 * r + 0.587 * g + 0.114 * b,
        [c, m, y, k] => {
            let inv = |v: f64| 1.0 - (v + k).min(1.0);
            0.299 * inv(*c) + 0.587 * inv(*m) + 0.114 * inv(*y)
        }
        _ => 0.0,
    }
}

#[derive(Debug, Clone)]
struct TextState {
    font: Option<Rc<FontInfo>>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
}

#[derive(Debug, Clone)]
struct GState {
    ctm: Matrix,
    line_width: f64,
    fill_lightness: f64,
    stroke_lightness: f64,
    text: TextState,
}

impl GState {
    fn new(ctm: Matrix) -> Self {
        GState {
            ctm,
            line_width: 1.0,
            fill_lightness: 0.0,
            stroke_lightness: 0.0,
            text: TextState {
                font: None,
                size: 0.0,
                char_spacing: 0.0,
                word_spacing: 0.0,
                h_scale: 1.0,
                leading: 0.0,
                rise: 0.0,
            },
        }
    }
}

struct Walker<'a, 'f> {
    doc: &'a Document,
    /// Top-left corner of the page in PDF user space.
    page_origin: (f64, f64),
    images: Vec<PageImage>,
    glyphs: Vec<Glyph>,
    shapes: Vec<Shape>,
    fonts: &'f mut HashMap<ObjectId, Rc<FontInfo>>,
    warnings: &'a mut Vec<String>,
}

impl<'a> Walker<'a, '_> {
    fn to_page(&self, m: Matrix, x: f64, y: f64) -> Point {
        let (px, py) = apply(m, x, y);
        let (ox, oy) = self.page_origin;
        Point {
            x: (px - ox) as f32,
            y: (oy - py) as f32,
        }
    }

    fn resource(
        &self,
        resources: &[&'a Dictionary],
        category: &[u8],
        name: &[u8],
    ) -> Option<(Option<ObjectId>, &'a Object)> {
        for res in resources {
            let Ok(entries) = res.get(category) else {
                continue;
            };
            let Object::Dictionary(entries) = deref(self.doc, entries) else {
                continue;
            };
            if let Ok(obj) = entries.get(name) {
                let id = match obj {
                    Object::Reference(id) => Some(*id),
                    _ => None,
                };
                return Some((id, deref(self.doc, obj)));
            }
        }
        None
    }

    fn font(&mut self, resources: &[&'a Dictionary], name: &[u8]) -> Option<Rc<FontInfo>> {
        let (id, obj) = self.resource(resources, b"Font", name)?;
        let Object::Dictionary(dict) = obj else {
            return None;
        };
        if let Some(id) = id
            && let Some(f) = self.fonts.get(&id)
        {
            return Some(f.clone());
        }
        let info = Rc::new(fonts::load_font(self.doc, dict));
        if let Some(id) = id {
            self.fonts.insert(id, info.clone());
        }
        Some(info)
    }

    fn walk(&mut self, content: &[u8], resources: &[&'a Dictionary], gs: GState, depth: usize) {
        let content = match Content::decode(content) {
            Ok(c) => c,
            Err(e) => {
                self.warnings.push(format!("content stream: {e}"));
                return;
            }
        };
        let mut stack: Vec<GState> = Vec::new();
        let mut gs = gs;
        // Text matrix and text line matrix (only meaningful inside BT … ET).
        let (mut tm, mut tlm) = (IDENTITY, IDENTITY);
        // Path under construction, in page coordinates.
        let mut subpaths: Vec<Vec<Point>> = Vec::new();
        let mut curved = false;
        let mut current = (0.0f64, 0.0f64);

        for op in &content.operations {
            let args = numbers(&op.operands);
            match op.operator.as_str() {
                "q" => stack.push(gs.clone()),
                "Q" => gs = stack.pop().unwrap_or(gs),
                "cm" => {
                    if let Ok(m) = <[f64; 6]>::try_from(args) {
                        gs.ctm = multiply(m, gs.ctm);
                    }
                }
                "w" => gs.line_width = args.first().copied().unwrap_or(1.0),
                "g" | "rg" | "k" | "sc" | "scn" => {
                    if !args.is_empty() {
                        gs.fill_lightness = lightness(&args);
                    }
                }
                "G" | "RG" | "K" | "SC" | "SCN" => {
                    if !args.is_empty() {
                        gs.stroke_lightness = lightness(&args);
                    }
                }

                // Paths.
                "m" if args.len() == 2 => {
                    current = (args[0], args[1]);
                    subpaths.push(vec![self.to_page(gs.ctm, args[0], args[1])]);
                }
                "l" if args.len() == 2 => {
                    current = (args[0], args[1]);
                    let p = self.to_page(gs.ctm, args[0], args[1]);
                    match subpaths.last_mut() {
                        Some(sp) => sp.push(p),
                        None => subpaths.push(vec![p]),
                    }
                }
                "c" | "v" | "y" => {
                    let (c1, c2, end) = match (op.operator.as_str(), args.as_slice()) {
                        ("c", [a, b, c, d, e, f]) => ((*a, *b), (*c, *d), (*e, *f)),
                        ("v", [c, d, e, f]) => (current, (*c, *d), (*e, *f)),
                        ("y", [a, b, e, f]) => ((*a, *b), (*e, *f), (*e, *f)),
                        _ => continue,
                    };
                    curved = true;
                    let start = current;
                    let pts: Vec<Point> = (1..=CURVE_STEPS)
                        .map(|i| {
                            let t = i as f64 / CURVE_STEPS as f64;
                            let u = 1.0 - t;
                            let x = u * u * u * start.0
                                + 3.0 * u * u * t * c1.0
                                + 3.0 * u * t * t * c2.0
                                + t * t * t * end.0;
                            let y = u * u * u * start.1
                                + 3.0 * u * u * t * c1.1
                                + 3.0 * u * t * t * c2.1
                                + t * t * t * end.1;
                            self.to_page(gs.ctm, x, y)
                        })
                        .collect();
                    current = end;
                    match subpaths.last_mut() {
                        Some(sp) => sp.extend(pts),
                        None => subpaths.push(pts),
                    }
                }
                "re" if args.len() == 4 => {
                    let (x, y, w, h) = (args[0], args[1], args[2], args[3]);
                    subpaths.push(vec![
                        self.to_page(gs.ctm, x, y),
                        self.to_page(gs.ctm, x + w, y),
                        self.to_page(gs.ctm, x + w, y + h),
                        self.to_page(gs.ctm, x, y + h),
                    ]);
                    current = (x, y);
                }
                "h" => {
                    if let Some(sp) = subpaths.last_mut()
                        && let Some(&first) = sp.first()
                    {
                        sp.push(first);
                    }
                }
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    let o = op.operator.as_str();
                    if matches!(o, "s" | "b" | "b*") {
                        for sp in &mut subpaths {
                            if let Some(&first) = sp.first() {
                                sp.push(first);
                            }
                        }
                    }
                    let stroked = matches!(o, "S" | "s" | "B" | "B*" | "b" | "b*")
                        && gs.stroke_lightness < 0.9;
                    let filled = matches!(o, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*")
                        && gs.fill_lightness < 0.9;
                    let paths = std::mem::take(&mut subpaths);
                    if (stroked || filled) && !paths.is_empty() {
                        self.shapes.push(Shape {
                            stroke: stroked.then(|| (gs.line_width * scale_of(gs.ctm)) as f32),
                            filled,
                            subpaths: paths,
                            curved,
                        });
                    }
                    curved = false;
                }

                // Text.
                "BT" => {
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                "Tf" => {
                    if let (Some(Object::Name(name)), Some(size)) =
                        (op.operands.first(), op.operands.get(1).and_then(number))
                    {
                        gs.text.font = self.font(resources, name);
                        gs.text.size = size;
                    }
                }
                "Tc" => gs.text.char_spacing = args.first().copied().unwrap_or(0.0),
                "Tw" => gs.text.word_spacing = args.first().copied().unwrap_or(0.0),
                "Tz" => gs.text.h_scale = args.first().copied().unwrap_or(100.0) / 100.0,
                "TL" => gs.text.leading = args.first().copied().unwrap_or(0.0),
                "Ts" => gs.text.rise = args.first().copied().unwrap_or(0.0),
                "Td" | "TD" if args.len() == 2 => {
                    if op.operator == "TD" {
                        gs.text.leading = -args[1];
                    }
                    tlm = multiply(translate(args[0], args[1]), tlm);
                    tm = tlm;
                }
                "Tm" => {
                    if let Ok(m) = <[f64; 6]>::try_from(args) {
                        tm = m;
                        tlm = m;
                    }
                }
                "T*" => {
                    tlm = multiply(translate(0.0, -gs.text.leading), tlm);
                    tm = tlm;
                }
                "Tj" | "'" | "\"" => {
                    if op.operator != "Tj" {
                        if op.operator == "\"" && args.len() >= 2 {
                            gs.text.word_spacing = args[0];
                            gs.text.char_spacing = args[1];
                        }
                        tlm = multiply(translate(0.0, -gs.text.leading), tlm);
                        tm = tlm;
                    }
                    if let Some(Object::String(bytes, _)) = op.operands.last() {
                        self.show(bytes, &gs, &mut tm);
                    }
                }
                "TJ" => {
                    if let Some(Object::Array(items)) = op.operands.first() {
                        for item in items {
                            match item {
                                Object::String(bytes, _) => self.show(bytes, &gs, &mut tm),
                                other => {
                                    if let Some(n) = number(other) {
                                        let tx = -n / 1000.0 * gs.text.size * gs.text.h_scale;
                                        tm = multiply(translate(tx, 0.0), tm);
                                    }
                                }
                            }
                        }
                    }
                }

                "Do" => {
                    let Some(Object::Name(name)) = op.operands.first() else {
                        continue;
                    };
                    let Some((_, Object::Stream(stream))) =
                        self.resource(resources, b"XObject", name)
                    else {
                        continue;
                    };
                    let name = String::from_utf8_lossy(name).into_owned();
                    match stream.dict.get(b"Subtype") {
                        Ok(Object::Name(s)) if s == b"Image" => {
                            self.place_image(&name, stream, gs.ctm)
                        }
                        Ok(Object::Name(s)) if s == b"Form" => {
                            self.walk_form(stream, resources, &gs, depth)
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

    /// Places each character of a shown string and advances the text matrix.
    fn show(&mut self, bytes: &[u8], gs: &GState, tm: &mut Matrix) {
        let Some(font) = gs.text.font.clone() else {
            return;
        };
        let t = &gs.text;
        for code in font.codes(bytes) {
            let trm = multiply(
                multiply([t.size * t.h_scale, 0.0, 0.0, t.size, 0.0, t.rise], *tm),
                gs.ctm,
            );
            let w0 = f64::from(font.width(code) * font.scale);
            let origin = self.to_page(trm, 0.0, 0.0);
            let end = self.to_page(trm, w0, 0.0);
            let size = (trm[2] * trm[2] + trm[3] * trm[3]).sqrt();
            self.glyphs.push(Glyph {
                font: font.name.clone(),
                text: font.text(code),
                code,
                x: origin.x,
                y: origin.y,
                size: size as f32,
                width: ((end.x - origin.x).powi(2) + (end.y - origin.y).powi(2)).sqrt(),
            });
            let word = if !font.two_byte && code == 32 {
                t.word_spacing
            } else {
                0.0
            };
            let tx = (w0 * t.size + t.char_spacing + word) * t.h_scale;
            *tm = multiply(translate(tx, 0.0), *tm);
        }
    }

    fn walk_form(
        &mut self,
        form: &'a Stream,
        parent: &[&'a Dictionary],
        gs: &GState,
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
        let mut inner = gs.clone();
        inner.ctm = multiply(matrix, gs.ctm);
        match form.decompressed_content_with_limit(MAX_STREAM_BYTES) {
            Ok(content) => self.walk(&content, &resources, inner, depth + 1),
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
