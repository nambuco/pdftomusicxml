//! Renders the symbol templates from a SMuFL font into `assets/templates.bin`:
//! `cargo run -p scorelift-detect --example make_templates -- /Library/Fonts/Bravura.otf`
//!
//! Format (little endian): b"SLT1", staff space in px × 100 (u32), count
//! (u32), then per glyph: code point (u32), width (u16), height (u16),
//! origin x and y from the bitmap's top-left (i16, i16), width × height
//! coverage bytes.

use std::io::Write;

use scorelift_detect::TEMPLATE_GLYPHS;

/// Templates are rendered large and scaled down to each image's staff space.
const SP_REF: f32 = 24.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let font_path = args
        .next()
        .unwrap_or_else(|| "/Library/Fonts/Bravura.otf".into());
    let out = args
        .next()
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/assets/templates.bin").into());
    let font =
        fontdue::Font::from_bytes(std::fs::read(&font_path)?, fontdue::FontSettings::default())?;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"SLT1");
    buf.extend_from_slice(&((SP_REF * 100.0) as u32).to_le_bytes());
    buf.extend_from_slice(&(TEMPLATE_GLYPHS.len() as u32).to_le_bytes());
    // SMuFL fonts are designed with an em of four staff spaces.
    let px = 4.0 * SP_REF;
    for &cp in TEMPLATE_GLYPHS {
        let c = char::from_u32(cp).ok_or("bad code point")?;
        if font.lookup_glyph_index(c) == 0 {
            return Err(format!("{font_path} has no glyph U+{cp:04X}").into());
        }
        let (m, bitmap) = font.rasterize(c, px);
        // fontdue: xmin is the left offset from the origin, ymin the bottom
        // offset from the baseline (y up).
        let origin_x = -m.xmin;
        let origin_y = m.ymin + m.height as i32;
        buf.extend_from_slice(&cp.to_le_bytes());
        buf.extend_from_slice(&(m.width as u16).to_le_bytes());
        buf.extend_from_slice(&(m.height as u16).to_le_bytes());
        buf.extend_from_slice(&(origin_x as i16).to_le_bytes());
        buf.extend_from_slice(&(origin_y as i16).to_le_bytes());
        buf.extend_from_slice(&bitmap);
        println!("U+{cp:04X} {}x{}", m.width, m.height);
    }
    std::fs::File::create(&out)?.write_all(&buf)?;
    println!("wrote {out} ({} bytes)", buf.len());
    Ok(())
}
