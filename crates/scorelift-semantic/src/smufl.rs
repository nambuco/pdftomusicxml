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

/// Maps a font-specific optional glyph to its standard SMuFL code point.
/// Optional glyphs (U+F400 and up) differ per font; for example Dorico draws
/// noteheads with Bravura's oversized variants. Returns the character to
/// classify and whether it is a small (cue-size) variant drawn at full size.
pub fn standard(font: &str, c: char) -> (char, bool) {
    let cp = c as u32;
    if cp >= 0xF400
        && font.starts_with("Bravura")
        && let Some((std, small)) = bravura_alternate(cp)
        && let Some(std) = char::from_u32(std)
    {
        return (std, small);
    }
    (c, false)
}

/// Bravura's optional glyphs that stand for a standard symbol, generated
/// from the font's SMuFL metadata (`glyphsWithAlternates`). Returns the
/// standard code point and whether the glyph is a small (cue) variant.
fn bravura_alternate(cp: u32) -> Option<(u32, bool)> {
    Some(match cp {
        0xF400 => (0xE000, true),  // braceSmall
        0xF401 => (0xE000, false), // braceLarge
        0xF402 => (0xE000, false), // braceLarger
        0xF403 => (0xE000, false), // braceFlat
        0xF406 => (0xE062, false), // fClefFrench
        0xF407 => (0xE062, false), // fClef19thCentury
        0xF408 => (0xE05C, false), // cClefFrench
        0xF409 => (0xE069, false), // unpitchedPercussionClef1Alt
        0xF40A => (0xE06D, false), // 6stringTabClefTall
        0xF40B => (0xE06D, false), // 6stringTabClefSerif
        0xF40F => (0xE240, false), // flag8thUpStraight
        0xF410 => (0xE240, false), // flag8thUpShort
        0xF411 => (0xE241, false), // flag8thDownStraight
        0xF412 => (0xE242, false), // flag16thUpStraight
        0xF413 => (0xE242, false), // flag16thUpShort
        0xF414 => (0xE243, false), // flag16thDownStraight
        0xF415 => (0xE244, false), // flag32ndUpStraight
        0xF416 => (0xE244, false), // flag32ndUpShort
        0xF417 => (0xE245, false), // flag32ndDownStraight
        0xF418 => (0xE246, false), // flag64thUpStraight
        0xF419 => (0xE246, false), // flag64thUpShort
        0xF41A => (0xE247, false), // flag64thDownStraight
        0xF41B => (0xE248, false), // flag128thUpStraight
        0xF41C => (0xE248, false), // flag128thUpShort
        0xF41D => (0xE249, false), // flag128thDownStraight
        0xF427 => (0xE260, true),  // accidentalFlatSmall
        0xF428 => (0xE261, true),  // accidentalNaturalSmall
        0xF429 => (0xE262, true),  // accidentalSharpSmall
        0xF42A => (0xE4A0, false), // articAccentAboveLarge
        0xF42B => (0xE4A1, false), // articAccentBelowLarge
        0xF43F => (0xE0A0, false), // noteheadDoubleWholeAlt
        0xF440 => (0xE080, false), // timeSig0Large
        0xF441 => (0xE081, false), // timeSig1Large
        0xF442 => (0xE082, false), // timeSig2Large
        0xF443 => (0xE083, false), // timeSig3Large
        0xF444 => (0xE084, false), // timeSig4Large
        0xF445 => (0xE085, false), // timeSig5Large
        0xF446 => (0xE086, false), // timeSig6Large
        0xF447 => (0xE087, false), // timeSig7Large
        0xF448 => (0xE088, false), // timeSig8Large
        0xF449 => (0xE089, false), // timeSig9Large
        0xF44A => (0xE08A, false), // timeSigCommonLarge
        0xF44B => (0xE08B, false), // timeSigCutCommonLarge
        0xF45D => (0xE080, true),  // timeSig0Small
        0xF45E => (0xE081, true),  // timeSig1Small
        0xF45F => (0xE082, true),  // timeSig2Small
        0xF460 => (0xE083, true),  // timeSig3Small
        0xF461 => (0xE084, true),  // timeSig4Small
        0xF462 => (0xE085, true),  // timeSig5Small
        0xF463 => (0xE086, true),  // timeSig6Small
        0xF464 => (0xE087, true),  // timeSig7Small
        0xF465 => (0xE088, true),  // timeSig8Small
        0xF466 => (0xE089, true),  // timeSig9Small
        0xF467 => (0xE0A0, true),  // noteheadDoubleWholeSmall
        0xF468 => (0xE0A2, true),  // noteheadWholeSmall
        0xF469 => (0xE0A3, true),  // noteheadHalfSmall
        0xF46A => (0xE0A4, true),  // noteheadBlackSmall
        0xF46B => (0xE520, true),  // dynamicPianoSmall
        0xF46D => (0xE522, true),  // dynamicForteSmall
        0xF472 => (0xE050, true),  // gClefSmall
        0xF473 => (0xE05C, true),  // cClefSmall
        0xF474 => (0xE062, true),  // fClefSmall
        0xF475 => (0xE4A0, true),  // articAccentAboveSmall
        0xF476 => (0xE4A1, true),  // articAccentBelowSmall
        0xF477 => (0xE4A2, true),  // articStaccatoAboveSmall
        0xF478 => (0xE4A3, true),  // articStaccatoBelowSmall
        0xF479 => (0xE4A4, true),  // articTenutoAboveSmall
        0xF47A => (0xE4A5, true),  // articTenutoBelowSmall
        0xF481 => (0xE4AC, true),  // articMarcatoAboveSmall
        0xF482 => (0xE4AD, true),  // articMarcatoBelowSmall
        0xF48B => (0xE240, true),  // flag8thUpSmall
        0xF48C => (0xE241, true),  // flag8thDownSmall
        0xF48D => (0xE242, true),  // flag16thUpSmall
        0xF48E => (0xE243, true),  // flag16thDownSmall
        0xF48F => (0xE244, true),  // flag32ndUpSmall
        0xF490 => (0xE245, true),  // flag32ndDownSmall
        0xF491 => (0xE246, true),  // flag64thUpSmall
        0xF492 => (0xE247, true),  // flag64thDownSmall
        0xF493 => (0xE248, true),  // flag128thUpSmall
        0xF494 => (0xE249, true),  // flag128thDownSmall
        0xF4A1 => (0xE264, false), // accidentalDoubleFlatJoinedStems
        0xF4BA => (0xE0A0, false), // noteheadDoubleWholeOversized
        0xF4BC => (0xE0A2, false), // noteheadWholeOversized
        0xF4BD => (0xE0A3, false), // noteheadHalfOversized
        0xF4BE => (0xE0A4, false), // noteheadBlackOversized
        0xF4BF => (0xE05C, false), // cClefFrench20C
        0xF4CC => (0xE880, false), // tuplet0Light
        0xF4CD => (0xE881, false), // tuplet1Light
        0xF4CE => (0xE882, false), // tuplet2Light
        0xF4CF => (0xE883, false), // tuplet3Light
        0xF4D0 => (0xE884, false), // tuplet4Light
        0xF4D1 => (0xE885, false), // tuplet5Light
        0xF4D2 => (0xE886, false), // tuplet6Light
        0xF4D3 => (0xE887, false), // tuplet7Light
        0xF4D4 => (0xE888, false), // tuplet8Light
        0xF4D5 => (0xE889, false), // tuplet9Light
        0xF506 => (0xE080, false), // timeSig0Narrow
        0xF507 => (0xE081, false), // timeSig1Narrow
        0xF508 => (0xE082, false), // timeSig2Narrow
        0xF509 => (0xE083, false), // timeSig3Narrow
        0xF50A => (0xE084, false), // timeSig4Narrow
        0xF50B => (0xE085, false), // timeSig5Narrow
        0xF50C => (0xE086, false), // timeSig6Narrow
        0xF50D => (0xE087, false), // timeSig7Narrow
        0xF50E => (0xE088, false), // timeSig8Narrow
        0xF50F => (0xE089, false), // timeSig9Narrow
        0xF510 => (0xE08A, false), // timeSigCommonNarrow
        0xF511 => (0xE08B, false), // timeSigCutCommonNarrow
        0xF532 => (0xE4A0, false), // articAccentAboveRossini
        0xF533 => (0xE4A1, false), // articAccentBelowRossini
        0xF53D => (0xE052, false), // gClef8vbFrench
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
        assert_eq!(standard("Bravura", '\u{F4BE}'), ('\u{E0A4}', false));
        assert_eq!(standard("Bravura", '\u{F46A}'), ('\u{E0A4}', true));
        assert_eq!(standard("Leland", '\u{F4BE}'), ('\u{F4BE}', false));
    }
}
