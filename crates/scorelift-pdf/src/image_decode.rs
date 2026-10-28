//! Decodes PDF image XObjects to 8-bit grayscale.

use image::{GrayImage, Luma};
use lopdf::{Document, Object, Stream};

use crate::{MAX_STREAM_BYTES, deref, number};

enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
    /// Tint 0 means no ink.
    Separation,
    /// Palette of luma values, one per index.
    Indexed(Vec<u8>),
}

impl ColorSpace {
    fn components(&self) -> usize {
        match self {
            ColorSpace::Gray | ColorSpace::Separation | ColorSpace::Indexed(_) => 1,
            ColorSpace::Rgb => 3,
            ColorSpace::Cmyk => 4,
        }
    }
}

fn luma(r: u32, g: u32, b: u32) -> u8 {
    ((299 * r + 587 * g + 114 * b) / 1000) as u8
}

fn color_space(doc: &Document, obj: &Object) -> Result<ColorSpace, String> {
    match deref(doc, obj) {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" | b"G" => Ok(ColorSpace::Gray),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Ok(ColorSpace::Rgb),
            b"DeviceCMYK" | b"CMYK" => Ok(ColorSpace::Cmyk),
            other => Err(format!(
                "unsupported color space {}",
                String::from_utf8_lossy(other)
            )),
        },
        Object::Array(a) => {
            let family = match a.first().map(|o| deref(doc, o)) {
                Some(Object::Name(n)) => n.as_slice(),
                _ => return Err("malformed color space".into()),
            };
            match family {
                b"ICCBased" => {
                    let Some(Object::Stream(s)) = a.get(1).map(|o| deref(doc, o)) else {
                        return Err("malformed ICCBased color space".into());
                    };
                    match s.dict.get(b"N").ok().and_then(number).map(|n| n as i64) {
                        Some(1) => Ok(ColorSpace::Gray),
                        Some(3) => Ok(ColorSpace::Rgb),
                        Some(4) => Ok(ColorSpace::Cmyk),
                        _ => Err("unsupported ICC component count".into()),
                    }
                }
                b"CalGray" => Ok(ColorSpace::Gray),
                b"CalRGB" | b"Lab" => Ok(ColorSpace::Rgb),
                b"Separation" => Ok(ColorSpace::Separation),
                b"Indexed" | b"I" => indexed(doc, a),
                other => Err(format!(
                    "unsupported color space {}",
                    String::from_utf8_lossy(other)
                )),
            }
        }
        _ => Err("missing color space".into()),
    }
}

fn indexed(doc: &Document, a: &[Object]) -> Result<ColorSpace, String> {
    let base = color_space(doc, a.get(1).ok_or("malformed Indexed color space")?)?;
    let lookup = match a.get(3).map(|o| deref(doc, o)) {
        Some(Object::String(bytes, _)) => bytes.clone(),
        Some(Object::Stream(s)) => s
            .decompressed_content_with_limit(MAX_STREAM_BYTES)
            .map_err(|e| e.to_string())?,
        _ => return Err("malformed Indexed lookup table".into()),
    };
    let n = base.components();
    let palette = lookup
        .chunks_exact(n)
        .map(|c| match &base {
            ColorSpace::Rgb => luma(c[0].into(), c[1].into(), c[2].into()),
            ColorSpace::Cmyk => cmyk_luma(c[0], c[1], c[2], c[3]),
            ColorSpace::Separation => 255 - c[0],
            _ => c[0],
        })
        .collect();
    Ok(ColorSpace::Indexed(palette))
}

fn cmyk_luma(c: u8, m: u8, y: u8, k: u8) -> u8 {
    let inv = |v: u8| 255u32.saturating_sub(u32::from(v) + u32::from(k));
    luma(inv(c), inv(m), inv(y))
}

fn int(doc: &Document, dict: &lopdf::Dictionary, key: &[u8]) -> Option<u32> {
    let v = number(deref(doc, dict.get(key).ok()?))?;
    (v >= 0.0 && v <= f64::from(u32::MAX)).then_some(v as u32)
}

pub(crate) fn decode(doc: &Document, stream: &Stream) -> Result<GrayImage, String> {
    let dict = &stream.dict;
    let width = int(doc, dict, b"Width").ok_or("missing Width")?;
    let height = int(doc, dict, b"Height").ok_or("missing Height")?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 400_000_000 {
        return Err(format!("unreasonable image size {width}x{height}"));
    }

    let filters = stream.filters().unwrap_or_default();
    if let Some(f) = filters
        .iter()
        .find(|f| matches!(**f, b"JPXDecode" | b"CCITTFaxDecode" | b"JBIG2Decode"))
    {
        return Err(format!(
            "{} images are not supported yet",
            String::from_utf8_lossy(f)
        ));
    }

    let mut img = if filters.last() == Some(&&b"DCTDecode"[..]) {
        if filters.len() > 1 {
            return Err("DCTDecode combined with other filters is not supported".into());
        }
        image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?
            .to_luma8()
    } else {
        let data = stream
            .decompressed_content_with_limit(MAX_STREAM_BYTES)
            .map_err(|e| e.to_string())?;
        decode_samples(doc, dict, &data, width, height)?
    };

    if let Ok(smask) = dict.get(b"SMask")
        && let Object::Stream(s) = deref(doc, smask)
        && let Ok(alpha) = decode(doc, s)
        && alpha.dimensions() == img.dimensions()
    {
        // Composite onto a white page.
        for (p, a) in img.pixels_mut().zip(alpha.pixels()) {
            let ink = u32::from(255 - p.0[0]) * u32::from(a.0[0]) / 255;
            p.0[0] = 255 - ink as u8;
        }
    }
    Ok(img)
}

fn decode_samples(
    doc: &Document,
    dict: &lopdf::Dictionary,
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<GrayImage, String> {
    let is_mask = matches!(dict.get(b"ImageMask"), Ok(Object::Boolean(true)));
    let bpc = if is_mask {
        1
    } else {
        int(doc, dict, b"BitsPerComponent").unwrap_or(8)
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Err(format!("unsupported BitsPerComponent {bpc}"));
    }
    let cs = if is_mask {
        ColorSpace::Gray
    } else {
        color_space(doc, dict.get(b"ColorSpace").unwrap_or(&Object::Null))?
    };
    let n = cs.components();
    // /Decode [1 0] inverts single-component images. A stencil mask paints
    // where the sample is 0, which the plain gray mapping already shows as black.
    let invert = match dict.get(b"Decode").map(|d| deref(doc, d)) {
        Ok(Object::Array(a)) if n == 1 => {
            a.first().and_then(number).unwrap_or(0.0) > a.get(1).and_then(number).unwrap_or(1.0)
        }
        _ => false,
    };

    let row_bytes = (width as usize * n * bpc as usize).div_ceil(8);
    if data.len() < row_bytes * height as usize {
        return Err("image data is shorter than its dimensions".into());
    }
    let max = (1u32 << bpc.min(8)) - 1;
    let sample = |row: &[u8], i: usize| -> u32 {
        match bpc {
            8 => u32::from(row[i]),
            16 => u32::from(row[2 * i]),
            _ => {
                let bit = i * bpc as usize;
                let byte = row[bit / 8];
                let shift = 8 - bpc as usize - bit % 8;
                (u32::from(byte) >> shift) & max
            }
        }
    };
    // Scale a sample to 0..=255.
    let scale = |v: u32| {
        if bpc == 8 || bpc == 16 {
            v
        } else {
            v * 255 / max
        }
    };

    let mut img = GrayImage::new(width, height);
    for y in 0..height as usize {
        let row = &data[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..width as usize {
            let i = x * n;
            let v = match &cs {
                ColorSpace::Gray => scale(sample(row, i)) as u8,
                ColorSpace::Separation => 255 - scale(sample(row, i)) as u8,
                ColorSpace::Rgb => luma(
                    scale(sample(row, i)),
                    scale(sample(row, i + 1)),
                    scale(sample(row, i + 2)),
                ),
                ColorSpace::Cmyk => cmyk_luma(
                    scale(sample(row, i)) as u8,
                    scale(sample(row, i + 1)) as u8,
                    scale(sample(row, i + 2)) as u8,
                    scale(sample(row, i + 3)) as u8,
                ),
                ColorSpace::Indexed(palette) => {
                    palette.get(sample(row, i) as usize).copied().unwrap_or(255)
                }
            };
            let v = if invert { 255 - v } else { v };
            img.put_pixel(x as u32, y as u32, Luma([v]));
        }
    }
    Ok(img)
}
