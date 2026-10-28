//! Just enough font handling to place glyphs: code lengths, advance widths
//! and the ToUnicode map (which is how music fonts reveal SMuFL code points).

use std::collections::HashMap;

use lopdf::{Dictionary, Document, Object};

use crate::{MAX_STREAM_BYTES, deref, number};

#[derive(Debug)]
pub(crate) struct FontInfo {
    /// BaseFont without a subset prefix such as `ABCDEF+`.
    pub name: String,
    /// Type0 fonts with Identity encodings use two-byte codes.
    pub two_byte: bool,
    /// Advance widths in glyph space (1/1000 em), by character code.
    widths: HashMap<u32, f32>,
    default_width: f32,
    to_unicode: HashMap<u32, String>,
    /// Glyph space to text space scale (0.001 except for Type3 fonts).
    pub scale: f32,
}

impl FontInfo {
    pub fn width(&self, code: u32) -> f32 {
        self.widths
            .get(&code)
            .copied()
            .unwrap_or(self.default_width)
    }

    pub fn text(&self, code: u32) -> String {
        if let Some(s) = self.to_unicode.get(&code) {
            return s.clone();
        }
        if self.two_byte {
            return String::new();
        }
        // Simple fonts without a map: assume a Latin-1 compatible encoding.
        char::from_u32(code).map(String::from).unwrap_or_default()
    }

    /// Splits a shown string into character codes.
    pub fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        if self.two_byte {
            bytes
                .chunks(2)
                .map(|c| (u32::from(c[0]) << 8) | u32::from(*c.get(1).unwrap_or(&0)))
                .collect()
        } else {
            bytes.iter().map(|&b| u32::from(b)).collect()
        }
    }
}

fn dict<'a>(doc: &'a Document, d: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
    match deref(doc, d.get(key).ok()?) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn name(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<String> {
    match deref(doc, d.get(key).ok()?) {
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

pub(crate) fn load_font(doc: &Document, font: &Dictionary) -> FontInfo {
    let subtype = name(doc, font, b"Subtype").unwrap_or_default();
    let base = name(doc, font, b"BaseFont").unwrap_or_default();
    let name = match base.split_once('+') {
        Some((prefix, rest)) if prefix.len() == 6 => rest.to_string(),
        _ => base,
    };
    let mut info = FontInfo {
        name,
        two_byte: false,
        widths: HashMap::new(),
        default_width: 500.0,
        to_unicode: HashMap::new(),
        scale: 0.001,
    };

    if subtype == "Type0" {
        // Identity-H/V are by far the most common; other CMaps are also two-byte in practice.
        info.two_byte = true;
        info.default_width = 1000.0;
        let descendant = match font.get(b"DescendantFonts").map(|o| deref(doc, o)) {
            Ok(Object::Array(a)) => a.first().and_then(|o| match deref(doc, o) {
                Object::Dictionary(d) => Some(d),
                _ => None,
            }),
            _ => None,
        };
        if let Some(cid) = descendant {
            if let Some(dw) = cid.get(b"DW").ok().and_then(|o| number(deref(doc, o))) {
                info.default_width = dw as f32;
            }
            if let Ok(Object::Array(w)) = cid.get(b"W").map(|o| deref(doc, o)) {
                parse_cid_widths(doc, w, &mut info.widths);
            }
        }
    } else {
        let first = font
            .get(b"FirstChar")
            .ok()
            .and_then(|o| number(deref(doc, o)))
            .unwrap_or(0.0) as u32;
        if let Ok(Object::Array(w)) = font.get(b"Widths").map(|o| deref(doc, o)) {
            for (i, v) in w.iter().enumerate() {
                if let Some(v) = number(deref(doc, v)) {
                    info.widths.insert(first + i as u32, v as f32);
                }
            }
        }
        if let Some(desc) = dict(doc, font, b"FontDescriptor")
            && let Some(mw) = desc
                .get(b"MissingWidth")
                .ok()
                .and_then(|o| number(deref(doc, o)))
        {
            info.default_width = mw as f32;
        }
        if subtype == "Type3"
            && let Ok(Object::Array(m)) = font.get(b"FontMatrix").map(|o| deref(doc, o))
            && let Some(a) = m.first().and_then(|o| number(deref(doc, o)))
        {
            info.scale = a as f32;
        }
    }

    if let Ok(Object::Stream(s)) = font.get(b"ToUnicode").map(|o| deref(doc, o))
        && let Ok(data) = s.decompressed_content_with_limit(MAX_STREAM_BYTES)
    {
        info.to_unicode = parse_to_unicode(&data);
    }
    info
}

/// CID width arrays mix `c [w1 w2 …]` and `c_first c_last w` entries.
fn parse_cid_widths(doc: &Document, w: &[Object], out: &mut HashMap<u32, f32>) {
    let mut i = 0;
    while i < w.len() {
        let Some(first) = number(deref(doc, &w[i])) else {
            break;
        };
        match w.get(i + 1).map(|o| deref(doc, o)) {
            Some(Object::Array(list)) => {
                for (k, v) in list.iter().enumerate() {
                    if let Some(v) = number(deref(doc, v)) {
                        out.insert(first as u32 + k as u32, v as f32);
                    }
                }
                i += 2;
            }
            Some(last) => {
                let (Some(last), Some(v)) = (
                    number(last),
                    w.get(i + 2).and_then(|o| number(deref(doc, o))),
                ) else {
                    break;
                };
                for c in first as u32..=last as u32 {
                    out.insert(c, v as f32);
                }
                i += 3;
            }
            None => break,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Hex(Vec<u8>),
    Word(String),
    ArrayStart,
    ArrayEnd,
}

fn tokenize(data: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'<' if data.get(i + 1) != Some(&b'<') => {
                let end = data[i..]
                    .iter()
                    .position(|&b| b == b'>')
                    .map_or(data.len(), |p| i + p);
                let hex: Vec<u8> = data[i + 1..end]
                    .iter()
                    .copied()
                    .filter(u8::is_ascii_hexdigit)
                    .collect();
                let bytes = hex
                    .chunks(2)
                    .map(|c| {
                        let s = std::str::from_utf8(c).unwrap_or("0");
                        u8::from_str_radix(&format!("{s:0<2}"), 16).unwrap_or(0)
                    })
                    .collect();
                out.push(Token::Hex(bytes));
                i = end + 1;
            }
            b'[' => {
                out.push(Token::ArrayStart);
                i += 1;
            }
            b']' => {
                out.push(Token::ArrayEnd);
                i += 1;
            }
            b'%' => {
                while i < data.len() && data[i] != b'\n' {
                    i += 1;
                }
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < data.len()
                    && !data[i].is_ascii_whitespace()
                    && !matches!(data[i], b'<' | b'[' | b']' | b'%')
                {
                    i += 1;
                }
                if i == start {
                    i += 1;
                    continue;
                }
                out.push(Token::Word(
                    String::from_utf8_lossy(&data[start..i]).into_owned(),
                ));
            }
        }
    }
    out
}

fn code(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |acc, &b| (acc << 8) | u32::from(b))
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| (u16::from(c[0]) << 8) | u16::from(*c.get(1).unwrap_or(&0)))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Adds `n` to the last UTF-16 unit of `dst` (bfrange semantics).
fn offset(dst: &[u8], n: u32) -> String {
    let mut units: Vec<u16> = dst
        .chunks(2)
        .map(|c| (u16::from(c[0]) << 8) | u16::from(*c.get(1).unwrap_or(&0)))
        .collect();
    if let Some(last) = units.last_mut() {
        *last = last.wrapping_add(n as u16);
    }
    String::from_utf16_lossy(&units)
}

pub(crate) fn parse_to_unicode(data: &[u8]) -> HashMap<u32, String> {
    let tokens = tokenize(data);
    let mut map = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Word(w) if w == "beginbfchar" => {
                i += 1;
                while let (Some(Token::Hex(src)), Some(Token::Hex(dst))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    map.insert(code(src), utf16(dst));
                    i += 2;
                }
            }
            Token::Word(w) if w == "beginbfrange" => {
                i += 1;
                while let (Some(Token::Hex(lo)), Some(Token::Hex(hi))) =
                    (tokens.get(i), tokens.get(i + 1))
                {
                    let (lo, hi) = (code(lo), code(hi));
                    match tokens.get(i + 2) {
                        Some(Token::Hex(dst)) => {
                            for (k, c) in (lo..=hi.min(lo + 0xFFFF)).enumerate() {
                                map.insert(c, offset(dst, k as u32));
                            }
                            i += 3;
                        }
                        Some(Token::ArrayStart) => {
                            i += 3;
                            let mut c = lo;
                            while let Some(Token::Hex(dst)) = tokens.get(i) {
                                map.insert(c, utf16(dst));
                                c += 1;
                                i += 1;
                            }
                            i += 1; // ArrayEnd
                        }
                        _ => break,
                    }
                }
            }
            _ => i += 1,
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bfchar_and_both_bfrange_forms() {
        let cmap = b"2 beginbfchar\n<0003> <0041>\n<0004> <E0A4>\nendbfchar\n\
            2 beginbfrange\n<0010> <0012> <0061>\n<0020> <0021> [<E050> <E062>]\nendbfrange";
        let map = parse_to_unicode(cmap);
        assert_eq!(map[&3], "A");
        assert_eq!(map[&4], "\u{E0A4}");
        assert_eq!(map[&0x12], "c");
        assert_eq!(map[&0x20], "\u{E050}");
        assert_eq!(map[&0x21], "\u{E062}");
    }
}
