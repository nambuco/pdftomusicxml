//! The score intermediate representation (IR).
//!
//! It mirrors the parts of MusicXML that OMR can produce, but is organised
//! around voices rather than MusicXML's flat note/backup stream: each voice
//! is a list of consecutive events, and the writer works out the backups.

use serde::{Deserialize, Serialize};

use crate::Fraction;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub metadata: Metadata,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub arranger: Option<String>,
    pub rights: Option<String>,
    /// Free-form name/value pairs, e.g. `("tuning", "E A D G B E")`.
    pub misc: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// Unique id within the score, e.g. `"P1"`.
    pub id: String,
    pub name: String,
    pub abbreviation: Option<String>,
    /// Number of staves (1 for most instruments, 2 for piano).
    pub staves: u8,
    pub measures: Vec<Measure>,
}

impl Part {
    pub fn new(id: impl Into<String>, name: impl Into<String>, staves: u8) -> Self {
        Part {
            id: id.into(),
            name: name.into(),
            abbreviation: None,
            staves,
            measures: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    pub number: u32,
    /// A pickup or other measure that does not have to fill its time signature.
    pub implicit: bool,
    /// Changes that take effect at the start of this measure.
    pub attributes: Attributes,
    pub voices: Vec<Voice>,
    pub directions: Vec<Direction>,
    pub left_barline: Option<Barline>,
    pub right_barline: Option<Barline>,
    pub new_system: bool,
    pub new_page: bool,
}

impl Measure {
    pub fn new(number: u32) -> Self {
        Measure {
            number,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Attributes {
    pub key: Option<Key>,
    pub time: Option<TimeSignature>,
    pub clefs: Vec<StaffClef>,
    /// This measure starts a multi-measure rest spanning this many measures.
    /// The following measures must still be present, each with a measure rest.
    pub multiple_rest: Option<u16>,
}

impl Attributes {
    pub fn is_empty(&self) -> bool {
        self.key.is_none()
            && self.time.is_none()
            && self.clefs.is_empty()
            && self.multiple_rest.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// Sharps (positive) or flats (negative) in the key signature.
    pub fifths: i8,
    pub mode: Option<Mode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub beats: u8,
    pub beat_type: u8,
    pub symbol: Option<TimeSymbol>,
}

impl TimeSignature {
    pub fn new(beats: u8, beat_type: u8) -> Self {
        TimeSignature {
            beats,
            beat_type,
            symbol: None,
        }
    }

    /// The length of a full measure, in whole notes.
    pub fn measure_duration(&self) -> Fraction {
        Fraction::new(self.beats.into(), self.beat_type.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeSymbol {
    Common,
    Cut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaffClef {
    /// 1-based staff number within the part.
    pub staff: u8,
    pub clef: Clef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clef {
    pub sign: ClefSign,
    /// Staff line the clef sits on, counted from the bottom (1-5).
    pub line: u8,
    /// Octave transposition, e.g. -1 for a guitar "treble 8vb" clef.
    pub octave_change: i8,
}

impl Clef {
    pub const TREBLE: Clef = Clef {
        sign: ClefSign::G,
        line: 2,
        octave_change: 0,
    };
    pub const TREBLE_8VB: Clef = Clef {
        sign: ClefSign::G,
        line: 2,
        octave_change: -1,
    };
    pub const BASS: Clef = Clef {
        sign: ClefSign::F,
        line: 4,
        octave_change: 0,
    };
    pub const ALTO: Clef = Clef {
        sign: ClefSign::C,
        line: 3,
        octave_change: 0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClefSign {
    G,
    F,
    C,
    Percussion,
    Tab,
}

/// A sequence of consecutive events in one voice, starting at the beginning
/// of the measure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    /// MusicXML voice number. By convention 1-4 for staff 1, 5-8 for staff 2.
    pub number: u8,
    /// 1-based staff number within the part.
    pub staff: u8,
    pub events: Vec<Event>,
}

impl Voice {
    pub fn new(number: u8, staff: u8, events: Vec<Event>) -> Self {
        Voice {
            number,
            staff,
            events,
        }
    }

    /// Sum of the durations of all events (grace notes count as zero).
    pub fn duration(&self) -> Fraction {
        self.events
            .iter()
            .fold(Fraction::ZERO, |acc, e| acc + e.duration)
    }
}

/// A note, chord or rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub content: EventContent,
    /// Sounding duration in whole notes. Zero for grace notes.
    pub duration: Fraction,
    /// Written note type. `None` only for whole-measure rests.
    pub note_type: Option<NoteType>,
    pub dots: u8,
    pub time_modification: Option<TimeModification>,
    pub grace: Option<Grace>,
    pub stem: Option<Stem>,
    pub beams: Vec<Beam>,
    pub tuplet: Option<StartStop>,
    pub slurs: Vec<Slur>,
    pub articulations: Vec<Articulation>,
    pub fermata: bool,
    pub lyrics: Vec<Lyric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventContent {
    Rest {
        whole_measure: bool,
    },
    /// One or more simultaneous notes sharing a stem. Never empty.
    Chord(Vec<ChordNote>),
}

impl Event {
    fn base(content: EventContent, note_type: Option<NoteType>, dots: u8) -> Self {
        let duration = note_type.map_or(Fraction::ZERO, |t| t.dotted_duration(dots));
        Event {
            content,
            duration,
            note_type,
            dots,
            time_modification: None,
            grace: None,
            stem: None,
            beams: Vec::new(),
            tuplet: None,
            slurs: Vec::new(),
            articulations: Vec::new(),
            fermata: false,
            lyrics: Vec::new(),
        }
    }

    /// A single note or a chord.
    pub fn note(pitches: &[Pitch], note_type: NoteType, dots: u8) -> Self {
        let notes = pitches.iter().map(|&p| ChordNote::new(p)).collect();
        Event::base(EventContent::Chord(notes), Some(note_type), dots)
    }

    pub fn rest(note_type: NoteType, dots: u8) -> Self {
        Event::base(
            EventContent::Rest {
                whole_measure: false,
            },
            Some(note_type),
            dots,
        )
    }

    /// A rest filling the whole measure, whatever its length.
    pub fn measure_rest(measure_duration: Fraction) -> Self {
        let mut e = Event::base(
            EventContent::Rest {
                whole_measure: true,
            },
            None,
            0,
        );
        e.duration = measure_duration;
        e
    }

    /// A grace note (takes no time).
    pub fn grace(pitch: Pitch, note_type: NoteType, slash: bool) -> Self {
        let mut e = Event::note(&[pitch], note_type, 0);
        e.duration = Fraction::ZERO;
        e.grace = Some(Grace { slash });
        e
    }

    pub fn is_grace(&self) -> bool {
        self.grace.is_some()
    }

    /// Places the event in a tuplet, e.g. `(3, 2)` for a triplet.
    pub fn time_mod(mut self, actual: u8, normal: u8) -> Self {
        self.time_modification = Some(TimeModification { actual, normal });
        if !self.is_grace() {
            self.duration = self.duration * Fraction::new(normal.into(), actual.into());
        }
        self
    }

    pub fn tuplet(mut self, mark: StartStop) -> Self {
        self.tuplet = Some(mark);
        self
    }

    pub fn stem(mut self, stem: Stem) -> Self {
        self.stem = Some(stem);
        self
    }

    pub fn beam(mut self, number: u8, value: BeamValue) -> Self {
        self.beams.push(Beam { number, value });
        self
    }

    pub fn slur(mut self, number: u8, kind: StartStop) -> Self {
        self.slurs.push(Slur { number, kind });
        self
    }

    pub fn articulation(mut self, a: Articulation) -> Self {
        self.articulations.push(a);
        self
    }

    pub fn fermata(mut self) -> Self {
        self.fermata = true;
        self
    }

    pub fn lyric(mut self, syllabic: Syllabic, text: impl Into<String>) -> Self {
        self.lyrics.push(Lyric {
            number: 1,
            syllabic,
            text: text.into(),
            extend: false,
        });
        self
    }

    fn for_each_note(mut self, f: impl Fn(&mut ChordNote)) -> Self {
        if let EventContent::Chord(notes) = &mut self.content {
            notes.iter_mut().for_each(f);
        }
        self
    }

    /// Starts a tie on every note of the chord.
    pub fn tie_start(self) -> Self {
        self.for_each_note(|n| n.tie_start = true)
    }

    /// Ends a tie on every note of the chord.
    pub fn tie_stop(self) -> Self {
        self.for_each_note(|n| n.tie_stop = true)
    }

    /// Marks every note of the chord as "let ring".
    pub fn let_ring(self) -> Self {
        self.for_each_note(|n| n.let_ring = true)
    }

    /// Sets the displayed accidental on every note of the chord.
    pub fn accidental(self, a: Accidental) -> Self {
        self.for_each_note(|n| n.accidental = Some(a))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChordNote {
    pub pitch: Pitch,
    /// The accidental printed in front of the note, if any.
    pub accidental: Option<Accidental>,
    pub tie_start: bool,
    pub tie_stop: bool,
    pub let_ring: bool,
}

impl ChordNote {
    pub fn new(pitch: Pitch) -> Self {
        ChordNote {
            pitch,
            accidental: None,
            tie_start: false,
            tie_stop: false,
            let_ring: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pitch {
    pub step: Step,
    /// Chromatic alteration in semitones (-1 flat, +1 sharp).
    pub alter: i8,
    /// Scientific octave: middle C is C4.
    pub octave: i8,
}

impl Pitch {
    pub fn new(step: Step, alter: i8, octave: i8) -> Self {
        Pitch {
            step,
            alter,
            octave,
        }
    }

    /// Parses names like `C4`, `F#5`, `Bb3`, `Ebb2`, `Cx4`.
    pub fn parse(s: &str) -> Option<Pitch> {
        let mut chars = s.chars();
        let step = match chars.next()?.to_ascii_uppercase() {
            'C' => Step::C,
            'D' => Step::D,
            'E' => Step::E,
            'F' => Step::F,
            'G' => Step::G,
            'A' => Step::A,
            'B' => Step::B,
            _ => return None,
        };
        let rest = chars.as_str();
        let split = rest.find(|c: char| c == '-' || c.is_ascii_digit())?;
        let (accidentals, octave) = rest.split_at(split);
        let mut alter = 0i8;
        for c in accidentals.chars() {
            alter += match c {
                '#' => 1,
                'x' => 2,
                'b' => -1,
                _ => return None,
            };
        }
        Some(Pitch::new(step, alter, octave.parse().ok()?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Step {
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Accidental {
    Sharp,
    Flat,
    Natural,
    DoubleSharp,
    DoubleFlat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NoteType {
    Breve,
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
    OneHundredTwentyEighth,
}

impl NoteType {
    /// Undotted duration in whole notes.
    pub fn duration(self) -> Fraction {
        match self {
            NoteType::Breve => Fraction::new(2, 1),
            NoteType::Whole => Fraction::new(1, 1),
            NoteType::Half => Fraction::new(1, 2),
            NoteType::Quarter => Fraction::new(1, 4),
            NoteType::Eighth => Fraction::new(1, 8),
            NoteType::Sixteenth => Fraction::new(1, 16),
            NoteType::ThirtySecond => Fraction::new(1, 32),
            NoteType::SixtyFourth => Fraction::new(1, 64),
            NoteType::OneHundredTwentyEighth => Fraction::new(1, 128),
        }
    }

    /// Duration with `dots` augmentation dots: each dot adds half of the previous value.
    pub fn dotted_duration(self, dots: u8) -> Fraction {
        let base = self.duration();
        // base * (2 - 1/2^dots)
        let p = 1u64 << dots;
        base * Fraction::new(2 * p - 1, p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeModification {
    /// Notes actually played (3 in a triplet).
    pub actual: u8,
    /// Notes normally played in the same time (2 in a triplet).
    pub normal: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grace {
    pub slash: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stem {
    Up,
    Down,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beam {
    /// Beam level: 1 for eighths, 2 for sixteenths, …
    pub number: u8,
    pub value: BeamValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeamValue {
    Begin,
    Continue,
    End,
    ForwardHook,
    BackwardHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartStop {
    Start,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slur {
    pub number: u8,
    pub kind: StartStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Articulation {
    Accent,
    StrongAccent,
    Staccato,
    Staccatissimo,
    Tenuto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lyric {
    /// Verse number, starting at 1.
    pub number: u8,
    pub syllabic: Syllabic,
    pub text: String,
    /// A melisma line follows this syllable.
    pub extend: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Syllabic {
    Single,
    Begin,
    Middle,
    End,
}

/// A marking that is not attached to a note: dynamics, text, tempo, hairpins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Direction {
    /// Position from the start of the measure, in whole notes.
    pub offset: Fraction,
    pub staff: u8,
    pub placement: Option<Placement>,
    pub kind: DirectionKind,
}

impl Direction {
    pub fn new(offset: Fraction, kind: DirectionKind) -> Self {
        Direction {
            offset,
            staff: 1,
            placement: None,
            kind,
        }
    }

    pub fn above(mut self) -> Self {
        self.placement = Some(Placement::Above);
        self
    }

    pub fn below(mut self) -> Self {
        self.placement = Some(Placement::Below);
        self
    }

    pub fn on_staff(mut self, staff: u8) -> Self {
        self.staff = staff;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    Above,
    Below,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DirectionKind {
    Dynamic(Dynamic),
    /// Free text such as "let ring" or "Verse 1".
    Words(String),
    /// A boxed rehearsal mark.
    Rehearsal(String),
    Tempo {
        beat_unit: NoteType,
        dots: u8,
        per_minute: u16,
        text: Option<String>,
    },
    Wedge(WedgeKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dynamic {
    Ppp,
    Pp,
    P,
    Mp,
    Mf,
    F,
    Ff,
    Fff,
    Fp,
    Sf,
    Sfz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WedgeKind {
    Crescendo,
    Diminuendo,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Barline {
    pub style: BarStyle,
    pub repeat: Option<RepeatDirection>,
    pub ending: Option<Ending>,
}

impl Barline {
    pub fn new(style: BarStyle) -> Self {
        Barline {
            style,
            repeat: None,
            ending: None,
        }
    }

    pub fn final_bar() -> Self {
        Barline::new(BarStyle::LightHeavy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarStyle {
    Regular,
    Dotted,
    Dashed,
    Heavy,
    LightLight,
    LightHeavy,
    HeavyLight,
    HeavyHeavy,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ending {
    /// Ending numbers, e.g. "1" or "1, 2".
    pub number: String,
    pub kind: EndingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndingKind {
    Start,
    Stop,
    Discontinue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pitches() {
        assert_eq!(Pitch::parse("C4"), Some(Pitch::new(Step::C, 0, 4)));
        assert_eq!(Pitch::parse("F#5"), Some(Pitch::new(Step::F, 1, 5)));
        assert_eq!(Pitch::parse("Bb3"), Some(Pitch::new(Step::B, -1, 3)));
        assert_eq!(Pitch::parse("Ebb2"), Some(Pitch::new(Step::E, -2, 2)));
        assert_eq!(Pitch::parse("A-1"), Some(Pitch::new(Step::A, 0, -1)));
        assert_eq!(Pitch::parse("H4"), None);
        assert_eq!(Pitch::parse("C"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(NoteType::Quarter.dotted_duration(1), Fraction::new(3, 8));
        assert_eq!(NoteType::Half.dotted_duration(2), Fraction::new(7, 8));
        let triplet =
            Event::note(&[Pitch::parse("C4").unwrap()], NoteType::Eighth, 0).time_mod(3, 2);
        assert_eq!(triplet.duration, Fraction::new(1, 12));
        let grace = Event::grace(Pitch::parse("C4").unwrap(), NoteType::Eighth, true);
        assert_eq!(grace.duration, Fraction::ZERO);
    }
}
