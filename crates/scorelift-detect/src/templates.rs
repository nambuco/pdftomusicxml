//! Symbol templates rendered from Bravura (SIL Open Font License 1.1) by
//! `examples/make_templates.rs`, scaled at run time to the image's staff space.

use image::{GrayImage, Luma, imageops};
use scorelift_semantic::smufl::{self, Sym};

/// The glyphs the recogniser looks for.
pub const TEMPLATE_GLYPHS: &[u32] = &[
    0xE0A4, 0xE0A3, 0xE0A2, // noteheads: black, half, whole
    0xE050, 0xE062, 0xE05C, // clefs: G, F, C
    0xE4E3, 0xE4E4, 0xE4E5, 0xE4E6, 0xE4E7, 0xE4E8, // rests: whole … 32nd
    0xE262, 0xE260, 0xE261, // sharp, flat, natural
    0xE240, 0xE241, 0xE242, 0xE243, 0xE244, 0xE245, // flags: 8th, 16th, 32nd (up, down)
    0xE1E7, // augmentation dot
    0xE080, 0xE081, 0xE082, 0xE083, 0xE084, 0xE085, 0xE086, 0xE087, 0xE088,
    0xE089, // time digits
    0xE08A, 0xE08B, // common, cut time
    0xE520, 0xE522, 0xE52B, 0xE52C, 0xE52D, 0xE52F, // dynamics: p f pp mp mf ff
];

const DATA: &[u8] = include_bytes!("../assets/templates.bin");

/// A binary template at the target scale.
#[derive(Debug, Clone)]
pub struct Template {
    pub sym: Sym,
    pub width: u32,
    pub height: u32,
    /// Glyph origin (SMuFL reference point) from the top-left corner.
    pub origin_x: f32,
    pub origin_y: f32,
    pub mask: Vec<bool>,
    /// Number of ink pixels in `mask`.
    pub ink: u32,
    /// Width of the ring around the template where ink counts against a match.
    pub margin: i64,
    /// `mask` grown by one pixel: image ink here is consistent with the shape.
    pub zone: Vec<bool>,
    /// Size relative to normal glyphs (below 1 for grace and cue notes).
    pub rel: f32,
}

impl Template {
    pub fn get(&self, x: u32, y: u32) -> bool {
        self.mask[(y * self.width + x) as usize]
    }

    pub fn in_zone(&self, x: u32, y: u32) -> bool {
        self.zone[(y * self.width + x) as usize]
    }
}

/// Grows a mask by one pixel in the four directions.
fn dilate(mask: &[bool], w: u32, h: u32) -> Vec<bool> {
    let at = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < i64::from(w)
            && y < i64::from(h)
            && mask[(y as u32 * w + x as u32) as usize]
    };
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (i64::from(x), i64::from(y))))
        .map(|(x, y)| at(x, y) || at(x - 1, y) || at(x + 1, y) || at(x, y - 1) || at(x, y + 1))
        .collect()
}

fn u16_at(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([d[i], d[i + 1]])
}

fn u32_at(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]])
}

/// All templates scaled to staff space `sp` (pixels).
pub fn load(sp: f32) -> Vec<Template> {
    load_sized(sp, 1.0)
}

/// Templates for small (grace or cue) glyphs, `rel` times the normal size.
pub fn load_small(sp: f32, rel: f32) -> Vec<Template> {
    load_sized(sp, rel)
}

fn load_sized(sp: f32, rel: f32) -> Vec<Template> {
    assert_eq!(&DATA[..4], b"SLT1", "corrupt templates.bin");
    let sp_ref = u32_at(DATA, 4) as f32 / 100.0;
    let count = u32_at(DATA, 8) as usize;
    let factor = sp * rel / sp_ref;
    let mut out = Vec::with_capacity(count);
    let mut i = 12;
    for _ in 0..count {
        let cp = u32_at(DATA, i);
        let (w, h) = (
            u32::from(u16_at(DATA, i + 4)),
            u32::from(u16_at(DATA, i + 6)),
        );
        let ox = i16::from_le_bytes([DATA[i + 8], DATA[i + 9]]);
        let oy = i16::from_le_bytes([DATA[i + 10], DATA[i + 11]]);
        let pixels = &DATA[i + 12..i + 12 + (w * h) as usize];
        i += 12 + (w * h) as usize;
        let Some(sym) = char::from_u32(cp).and_then(smufl::classify) else {
            continue;
        };
        if w == 0 || h == 0 {
            continue;
        }
        let src = GrayImage::from_fn(w, h, |x, y| Luma([pixels[(y * w + x) as usize]]));
        let (nw, nh) = (
            ((w as f32 * factor).round() as u32).max(1),
            ((h as f32 * factor).round() as u32).max(1),
        );
        let scaled = imageops::resize(&src, nw, nh, imageops::FilterType::Triangle);
        let mask: Vec<bool> = scaled.pixels().map(|p| p.0[0] >= 110).collect();
        let ink = mask.iter().filter(|&&b| b).count() as u32;
        let zone = dilate(&mask, nw, nh);
        out.push(Template {
            sym,
            width: nw,
            height: nh,
            origin_x: f32::from(ox) * factor,
            origin_y: f32::from(oy) * factor,
            mask,
            ink,
            margin: (0.2 * sp).round().max(2.0) as i64,
            zone,
            rel,
        });
    }
    out
}
