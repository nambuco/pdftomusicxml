//! SMuFL code points used by the vector reader. Every SMuFL font (Bravura,
//! Leland, Petaluma, Finale Maestro, …) uses the same code points, and
//! notation programs map them in the PDF's ToUnicode table.

use scorelift_model::{Accidental, Articulation, Dynamic, NoteType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    DoubleWhole,
    Whole,
    Half,
    Black,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClefGlyph {
    G,
    G8vb,
    G8va,
    F,
    F8vb,
    C,
    Percussion,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sym {
    Head(Head),
    Clef(ClefGlyph),
    /// A time signature digit 0–9.
    TimeDigit(u8),
    TimeCommon,
    TimeCut,
    Accidental(Accidental),
    /// Number of flags (1 = eighth) and whether the stem points up.
    Flag(u8, bool),
    Rest(NoteType),
    /// Augmentation dot.
    Dot,
    RepeatDot,
    Articulation(Articulation),
    Fermata,
    Dynamic(Dynamic),
    TupletDigit(u8),
    /// Metronome-mark note value (for tempo marks).
    MetNote(NoteType),
    MetDot,
    Brace,
    Bracket,
}

pub fn classify(c: char) -> Option<Sym> {
    use NoteType::*;
    let cp = c as u32;
    Some(match cp {
        0xE000 => Sym::Brace,
        0xE002 => Sym::Bracket,
        0xE043 | 0xE044 => Sym::RepeatDot,
        0xE050 => Sym::Clef(ClefGlyph::G),
        0xE052 => Sym::Clef(ClefGlyph::G8vb),
        0xE053 => Sym::Clef(ClefGlyph::G8va),
        0xE05C => Sym::Clef(ClefGlyph::C),
        0xE062 => Sym::Clef(ClefGlyph::F),
        0xE064 => Sym::Clef(ClefGlyph::F8vb),
        0xE069 | 0xE06A => Sym::Clef(ClefGlyph::Percussion),
        0xE06D | 0xE06E => Sym::Clef(ClefGlyph::Tab),
        0xE080..=0xE089 => Sym::TimeDigit((cp - 0xE080) as u8),
        0xE08A => Sym::TimeCommon,
        0xE08B => Sym::TimeCut,
        0xE0A0 => Sym::Head(Head::DoubleWhole),
        0xE0A2 => Sym::Head(Head::Whole),
        0xE0A3 => Sym::Head(Head::Half),
        0xE0A4 => Sym::Head(Head::Black),
        0xE1D0 | 0xE1D1 => Sym::MetNote(Breve),
        0xE1D2 => Sym::MetNote(Whole),
        0xE1D3 | 0xE1D4 => Sym::MetNote(Half),
        0xE1D5 | 0xE1D6 => Sym::MetNote(Quarter),
        0xE1D7 | 0xE1D8 => Sym::MetNote(Eighth),
        0xE1D9 | 0xE1DA => Sym::MetNote(Sixteenth),
        0xE1E7 => Sym::Dot,
        // Metronome marks (the newer range used by text fonts such as Leland Text).
        0xECA0 | 0xECA1 => Sym::MetNote(Breve),
        0xECA2 => Sym::MetNote(Whole),
        0xECA3 | 0xECA4 => Sym::MetNote(Half),
        0xECA5 | 0xECA6 => Sym::MetNote(Quarter),
        0xECA7 | 0xECA8 => Sym::MetNote(Eighth),
        0xECA9 | 0xECAA => Sym::MetNote(Sixteenth),
        0xECB7 => Sym::MetDot,
        0xE240..=0xE249 => {
            let k = cp - 0xE240;
            Sym::Flag((k / 2 + 1) as u8, k % 2 == 0)
        }
        0xE260 => Sym::Accidental(Accidental::Flat),
        0xE261 => Sym::Accidental(Accidental::Natural),
        0xE262 => Sym::Accidental(Accidental::Sharp),
        0xE263 => Sym::Accidental(Accidental::DoubleSharp),
        0xE264 => Sym::Accidental(Accidental::DoubleFlat),
        0xE4A0 | 0xE4A1 => Sym::Articulation(Articulation::Accent),
        0xE4A2 | 0xE4A3 => Sym::Articulation(Articulation::Staccato),
        0xE4A4 | 0xE4A5 => Sym::Articulation(Articulation::Tenuto),
        0xE4A6..=0xE4A9 => Sym::Articulation(Articulation::Staccatissimo),
        0xE4AC | 0xE4AD => Sym::Articulation(Articulation::StrongAccent),
        0xE4C0 | 0xE4C1 => Sym::Fermata,
        0xE4E2 => Sym::Rest(Breve),
        0xE4E3 => Sym::Rest(Whole),
        0xE4E4 => Sym::Rest(Half),
        0xE4E5 => Sym::Rest(Quarter),
        0xE4E6 => Sym::Rest(Eighth),
        0xE4E7 => Sym::Rest(Sixteenth),
        0xE4E8 => Sym::Rest(ThirtySecond),
        0xE4E9 => Sym::Rest(SixtyFourth),
        0xE4EA => Sym::Rest(OneHundredTwentyEighth),
        0xE520 => Sym::Dynamic(Dynamic::P),
        0xE522 => Sym::Dynamic(Dynamic::F),
        0xE52A => Sym::Dynamic(Dynamic::Ppp),
        0xE52B => Sym::Dynamic(Dynamic::Pp),
        0xE52C => Sym::Dynamic(Dynamic::Mp),
        0xE52D => Sym::Dynamic(Dynamic::Mf),
        0xE52F => Sym::Dynamic(Dynamic::Ff),
        0xE530 => Sym::Dynamic(Dynamic::Fff),
        0xE534 => Sym::Dynamic(Dynamic::Fp),
        0xE536 => Sym::Dynamic(Dynamic::Sf),
        0xE539 => Sym::Dynamic(Dynamic::Sfz),
        0xE880..=0xE889 => Sym::TupletDigit((cp - 0xE880) as u8),
        _ => return None,
    })
}

/// True for code points in the Unicode Private Use Area, where SMuFL lives.
pub fn is_music(c: char) -> bool {
    (0xE000..=0xF8FF).contains(&(c as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_glyphs() {
        assert_eq!(classify('\u{E0A4}'), Some(Sym::Head(Head::Black)));
        assert_eq!(classify('\u{E050}'), Some(Sym::Clef(ClefGlyph::G)));
        assert_eq!(classify('\u{E084}'), Some(Sym::TimeDigit(4)));
        assert_eq!(classify('\u{E242}'), Some(Sym::Flag(2, true)));
        assert_eq!(classify('\u{E241}'), Some(Sym::Flag(1, false)));
        assert_eq!(classify('\u{E52D}'), Some(Sym::Dynamic(Dynamic::Mf)));
        assert_eq!(classify('\u{ECA5}'), Some(Sym::MetNote(NoteType::Quarter)));
        assert_eq!(classify('A'), None);
    }
}
