//! Reads one staff of one system: measures, chords, rests, voices, pitch
//! and rhythm, then the things that attach across measures (ties, slurs,
//! dynamics, hairpins, text, lyrics, endings).

use std::collections::HashMap;

use scorelift_model::*;

use crate::layout::{BarGroup, Fill, MusicGlyph, Segment, Staff, System, Word};
use crate::smufl::{ClefGlyph, Head, Sym};

/// What carries over from one measure (and system) to the next on a staff.
#[derive(Debug, Clone)]
pub struct StaffState {
    pub clef: Clef,
    pub key: i8,
    pub time: Option<TimeSignature>,
    /// Alteration of notes whose tie continues into the next measure.
    pub tied_alters: HashMap<i32, i8>,
    /// Set once the first measure has been read.
    pub started: bool,
}

impl Default for StaffState {
    fn default() -> Self {
        StaffState {
            clef: Clef::TREBLE,
            key: 0,
            time: None,
            tied_alters: HashMap::new(),
            started: false,
        }
    }
}

/// One staff's content in one measure.
#[derive(Debug, Clone, Default)]
pub struct StaffMeasure {
    pub x0: f32,
    pub x1: f32,
    pub clef: Option<Clef>,
    pub key: Option<i8>,
    pub time: Option<TimeSignature>,
    pub multirest: Option<u16>,
    /// Voices in order (local index 0 = upper voice).
    pub voices: Vec<Vec<Event>>,
    pub directions: Vec<Direction>,
    /// Onset and x position of each event in the first voice, for placing directions.
    pub anchors: Vec<(f32, Fraction)>,
    pub measure_len: Fraction,
}

/// A notehead and where its note ended up, for ties and lyrics.
#[derive(Debug, Clone, Copy)]
struct HeadRef {
    x: f32,
    y: f32,
    w: f32,
    diatonic: i32,
    measure: usize,
    voice: usize,
    event: usize,
    note: usize,
}

#[derive(Debug, Clone)]
struct Chord {
    heads: Vec<MusicGlyph>,
    stem: Option<Segment>,
    up: Option<bool>,
    small: bool,
}

impl Chord {
    fn head_x0(&self) -> f32 {
        self.heads.iter().map(|h| h.x).fold(f32::MAX, f32::min)
    }
    fn head_x1(&self) -> f32 {
        self.heads
            .iter()
            .map(|h| h.x + h.w)
            .fold(f32::MIN, f32::max)
    }
}

/// Lowest staff position in diatonic steps (C0 = 0) for the clef.
fn bottom_line_diatonic(clef: Clef) -> i32 {
    let line = i32::from(clef.line);
    let base = match clef.sign {
        ClefSign::G => 4 * 7 + 4 - 2 * (line - 1),
        ClefSign::F => 3 * 7 + 3 - 2 * (line - 1),
        ClefSign::C => 4 * 7 - 2 * (line - 1),
        ClefSign::Percussion | ClefSign::Tab => 4 * 7 + 2,
    };
    base + 7 * i32::from(clef.octave_change)
}

fn clef_from(g: ClefGlyph, line: u8) -> Clef {
    let (sign, octave_change) = match g {
        ClefGlyph::G => (ClefSign::G, 0),
        ClefGlyph::G8vb => (ClefSign::G, -1),
        ClefGlyph::G8va => (ClefSign::G, 1),
        ClefGlyph::F => (ClefSign::F, 0),
        ClefGlyph::F8vb => (ClefSign::F, -1),
        ClefGlyph::C => (ClefSign::C, 0),
        ClefGlyph::Percussion => (ClefSign::Percussion, 0),
        ClefGlyph::Tab => (ClefSign::Tab, 0),
    };
    Clef {
        sign,
        line,
        octave_change,
    }
}

const STEPS: [Step; 7] = [
    Step::C,
    Step::D,
    Step::E,
    Step::F,
    Step::G,
    Step::A,
    Step::B,
];

fn key_alter(fifths: i8, step: usize) -> i8 {
    // Sharps are added in the order F C G D A E B, flats in reverse.
    const SHARP_ORDER: [usize; 7] = [3, 0, 4, 1, 5, 2, 6];
    let n = fifths.unsigned_abs() as usize;
    if fifths > 0 && SHARP_ORDER[..n.min(7)].contains(&step) {
        1
    } else if fifths < 0 && SHARP_ORDER[7 - n.min(7)..].contains(&step) {
        -1
    } else {
        0
    }
}

fn accidental_alter(a: Accidental) -> i8 {
    match a {
        Accidental::Sharp => 1,
        Accidental::Flat => -1,
        Accidental::Natural => 0,
        Accidental::DoubleSharp => 2,
        Accidental::DoubleFlat => -2,
    }
}

fn halve(t: NoteType, times: u8) -> NoteType {
    use NoteType::*;
    const ORDER: [NoteType; 9] = [
        Breve,
        Whole,
        Half,
        Quarter,
        Eighth,
        Sixteenth,
        ThirtySecond,
        SixtyFourth,
        OneHundredTwentyEighth,
    ];
    let i = ORDER.iter().position(|&o| o == t).unwrap_or(3);
    ORDER[(i + times as usize).min(ORDER.len() - 1)]
}

/// Largest power of two below `n`: the "normal" count of an n-tuplet.
fn tuplet_normal(n: u8) -> u8 {
    let mut p = 1;
    while p * 2 < n {
        p *= 2;
    }
    p.max(1)
}

/// Measure x-ranges of a system, from its barline groups.
pub fn measure_ranges(sys: &System) -> Vec<(f32, f32, Option<usize>, Option<usize>)> {
    let mut out = Vec::new();
    let mut start = sys.x0;
    let mut left_bar = None;
    for (i, b) in sys.bars.iter().enumerate() {
        if b.x0 - start > 0.5 * sys.staves[0].sp {
            out.push((start, b.x0, left_bar, Some(i)));
        }
        start = b.x1;
        left_bar = Some(i);
    }
    if sys.x1 - start > 1.0 * sys.staves[0].sp {
        out.push((start, sys.x1, left_bar, None));
    }
    out
}

pub struct StaffReading {
    pub measures: Vec<StaffMeasure>,
}

/// Reads one staff across all measure ranges of its system.
pub fn read_staff(staff: &Staff, ranges: &[(f32, f32)], state: &mut StaffState) -> StaffReading {
    let sp = staff.sp;
    let mut measures = Vec::new();
    let mut heads_index: Vec<HeadRef> = Vec::new();

    for (mi, &(mx0, mx1)) in ranges.iter().enumerate() {
        let inside = |x: f32| x >= mx0 && x < mx1;
        let glyphs: Vec<&MusicGlyph> = staff.glyphs.iter().filter(|g| inside(g.cx())).collect();
        let mut m = StaffMeasure {
            x0: mx0,
            x1: mx1,
            ..Default::default()
        };

        // Clefs (position gives the line).
        let mut clef_changes: Vec<(f32, Clef)> = Vec::new();
        for g in &glyphs {
            if let Sym::Clef(c) = g.sym {
                let line = (((staff.bottom() - g.y) / sp).round() as i32 + 1).clamp(1, 5) as u8;
                clef_changes.push((g.x, clef_from(c, line)));
            }
        }
        clef_changes.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Time signature digits inside the staff; multi-rest numbers above it.
        let mut num = String::new();
        let mut den = String::new();
        let mut multi = String::new();
        let mut digits: Vec<&&MusicGlyph> = glyphs
            .iter()
            .filter(|g| matches!(g.sym, Sym::TimeDigit(_)))
            .collect();
        digits.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut time_x: Option<f32> = None;
        for g in digits {
            let Sym::TimeDigit(d) = g.sym else { continue };
            let ch = char::from(b'0' + d);
            if g.y < staff.top() - 0.25 * sp {
                multi.push(ch);
            } else if g.y < staff.middle() {
                num.push(ch);
                time_x.get_or_insert(g.x);
            } else {
                den.push(ch);
                time_x.get_or_insert(g.x);
            }
        }
        let mut time = match (num.parse::<u8>(), den.parse::<u8>()) {
            (Ok(n), Ok(d)) => Some(TimeSignature::new(n, d)),
            _ => None,
        };
        for g in &glyphs {
            match g.sym {
                Sym::TimeCommon => {
                    time = Some(TimeSignature {
                        beats: 4,
                        beat_type: 4,
                        symbol: Some(TimeSymbol::Common),
                    })
                }
                Sym::TimeCut => {
                    time = Some(TimeSignature {
                        beats: 2,
                        beat_type: 2,
                        symbol: Some(TimeSymbol::Cut),
                    })
                }
                _ => {}
            }
        }

        // Noteheads and stems.
        let heads: Vec<MusicGlyph> = glyphs
            .iter()
            .filter(|g| matches!(g.sym, Sym::Head(_)))
            .map(|g| (*g).clone())
            .collect();
        let stems: Vec<Segment> = staff
            .segments
            .iter()
            .filter(|s| {
                s.is_vertical() && inside(s.a.x) && s.width < 0.3 * sp && s.len() > 1.0 * sp
            })
            .copied()
            .collect();
        let chords = build_chords(&heads, &stems, sp);

        // Accidentals next to heads; the rest before the first note form the key signature.
        let first_event_x = chords
            .iter()
            .map(|c| c.head_x0())
            .chain(
                glyphs
                    .iter()
                    .filter(|g| matches!(g.sym, Sym::Rest(_)))
                    .map(|g| g.x),
            )
            .fold(f32::MAX, f32::min);
        let mut note_accidentals: Vec<(f32, f32, Accidental)> = Vec::new();
        let mut key_accidentals: Vec<Accidental> = Vec::new();
        for g in &glyphs {
            let Sym::Accidental(a) = g.sym else { continue };
            let by_head = heads.iter().any(|h| {
                (h.y - g.y).abs() < 0.3 * sp
                    && h.x >= g.x + g.w - 0.2 * sp
                    && h.x - (g.x + g.w) < 2.5 * sp
            });
            if by_head && g.rel_size > 0.0 {
                note_accidentals.push((g.x + g.w, g.y, a));
            } else if g.x < first_event_x {
                key_accidentals.push(a);
            }
        }
        let key = if key_accidentals.is_empty() {
            None
        } else {
            let sharps = key_accidentals
                .iter()
                .filter(|&&a| a == Accidental::Sharp)
                .count() as i8;
            let flats = key_accidentals
                .iter()
                .filter(|&&a| a == Accidental::Flat)
                .count() as i8;
            Some(if sharps > 0 { sharps } else { -flats })
        };

        // Header-only segment (clef/key/time before a repeat bar): nothing else here.
        let rests: Vec<&MusicGlyph> = glyphs
            .iter()
            .filter(|g| matches!(g.sym, Sym::Rest(_)))
            .copied()
            .collect();

        // Clef, key and time are reprinted at every system start; only the
        // first measure and real changes are written out.
        let first = !state.started;
        state.started = true;
        if let Some(&(x, c)) = clef_changes.first()
            && x <= first_event_x
        {
            if first || c != state.clef {
                m.clef = Some(c);
            }
            state.clef = c;
        }
        let key = key.or(first.then_some(0));
        if let Some(k) = key
            && (first || k != state.key)
        {
            m.key = Some(k);
            state.key = k;
        }
        if let Some(t) = time {
            if first || state.time != Some(t) {
                m.time = Some(t);
            }
            state.time = Some(t);
        }
        let measure_len = state
            .time
            .map_or(Fraction::new(1, 1), |t| t.measure_duration());
        m.measure_len = measure_len;

        if chords.is_empty() && rests.is_empty() {
            if let Ok(n) = multi.parse::<u16>() {
                m.multirest = Some(n);
                m.voices = vec![vec![Event::measure_rest(measure_len)]];
                m.anchors = vec![(mx0, Fraction::ZERO)];
            }
            measures.push(m);
            continue;
        }

        // Pitch of each chord head.
        let clef_at = |x: f32| -> Clef {
            clef_changes
                .iter()
                .rev()
                .find(|(cx, _)| *cx < x)
                .map_or(state.clef, |(_, c)| *c)
        };
        let mut alters: HashMap<i32, i8> = HashMap::new();
        let mut items: Vec<Item> = Vec::new();
        let mut order: Vec<&Chord> = chords.iter().collect();
        order.sort_by(|a, b| a.head_x0().total_cmp(&b.head_x0()));
        for c in order {
            let clef = clef_at(c.head_x0());
            let mut notes = Vec::new();
            let mut sorted_heads = c.heads.clone();
            sorted_heads.sort_by(|a, b| b.y.total_cmp(&a.y));
            for h in &sorted_heads {
                let step = staff.step(h.y);
                let d = bottom_line_diatonic(clef) + step;
                let shown = note_accidentals
                    .iter()
                    .filter(|(ax, ay, _)| {
                        (ay - h.y).abs() < 0.3 * sp && *ax <= h.x + 0.2 * sp && h.x - ax < 2.5 * sp
                    })
                    .min_by(|a, b| (h.x - a.0).total_cmp(&(h.x - b.0)))
                    .map(|a| a.2);
                let step_idx = d.rem_euclid(7) as usize;
                let alter = match shown {
                    Some(a) => {
                        let v = accidental_alter(a);
                        alters.insert(d, v);
                        v
                    }
                    None => alters
                        .get(&d)
                        .copied()
                        .or_else(|| state.tied_alters.get(&d).copied())
                        .unwrap_or_else(|| key_alter(state.key, step_idx)),
                };
                let mut note =
                    ChordNote::new(Pitch::new(STEPS[step_idx], alter, d.div_euclid(7) as i8));
                note.accidental = shown;
                notes.push((note, h.clone(), step, d));
            }
            items.push(Item::Chord(c.clone(), notes));
        }
        for r in &rests {
            items.push(Item::Rest((*r).clone()));
        }
        items.sort_by(|a, b| a.x().total_cmp(&b.x()));
        state.tied_alters.clear();

        // Durations.
        let beams: Vec<&Fill> = staff
            .fills
            .iter()
            .filter(|f| !f.curved && inside((f.x0 + f.x1) / 2.0))
            .collect();
        let dots: Vec<&MusicGlyph> = glyphs
            .iter()
            .filter(|g| g.sym == Sym::Dot)
            .copied()
            .collect();
        let mut built: Vec<Built> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            // Dots stop at the next event to the right; events of another
            // voice at the same position do not count.
            let right = item.x1();
            let next_x = items[i + 1..]
                .iter()
                .map(Item::x)
                .find(|&x| x > right + 0.1 * sp)
                .unwrap_or(mx1);
            built.push(build_event(
                item,
                &glyphs,
                &beams,
                &dots,
                &staff.segments,
                next_x,
                sp,
            ));
        }

        // Voices.
        let multi_voice = is_multi_voice(&built, sp);
        let mut voices: Vec<Vec<usize>> = vec![Vec::new(), Vec::new()];
        for (i, b) in built.iter().enumerate() {
            let v = if !multi_voice {
                0
            } else if b.event.is_grace() {
                usize::from(b.up == Some(false))
            } else {
                match b.up {
                    Some(true) => 0,
                    Some(false) => 1,
                    None => usize::from(b.y > staff.middle()),
                }
            };
            voices[v].push(i);
        }
        voices.retain(|v| !v.is_empty());

        // Whole rest alone in a voice means a full-measure rest.
        for v in &voices {
            if v.len() == 1 {
                let b = &mut built[v[0]];
                if matches!(b.event.content, EventContent::Rest { .. })
                    && b.event.note_type == Some(NoteType::Whole)
                {
                    b.event = Event::measure_rest(measure_len);
                }
            }
        }

        apply_tuplets(&mut built, &voices, staff, mx0, mx1);

        m.voices = voices
            .iter()
            .map(|v| v.iter().map(|&i| built[i].event.clone()).collect())
            .collect();
        let mut t = Fraction::ZERO;
        if let Some(v0) = voices.first() {
            for &i in v0 {
                m.anchors.push((built[i].x, t));
                t += built[i].event.duration;
            }
        }
        for (vi, v) in voices.iter().enumerate() {
            for (ei, &i) in v.iter().enumerate() {
                for (ni, (x, y, w, _, d)) in built[i].heads.iter().enumerate() {
                    heads_index.push(HeadRef {
                        x: *x,
                        y: *y,
                        w: *w,
                        diatonic: *d,
                        measure: mi,
                        voice: vi,
                        event: ei,
                        note: ni,
                    });
                }
            }
        }
        measures.push(m);
    }

    attach_curves(staff, &mut measures, &heads_index, state);
    attach_directions(staff, &mut measures);
    attach_lyrics(staff, &mut measures, &heads_index);
    StaffReading { measures }
}

#[derive(Debug, Clone)]
enum Item {
    Chord(Chord, Vec<(ChordNote, MusicGlyph, i32, i32)>),
    Rest(MusicGlyph),
}

impl Item {
    fn x(&self) -> f32 {
        match self {
            Item::Chord(c, _) => c.head_x0(),
            Item::Rest(g) => g.x,
        }
    }

    fn x1(&self) -> f32 {
        match self {
            Item::Chord(c, _) => c.head_x1(),
            Item::Rest(g) => g.x + g.w,
        }
    }
}

#[derive(Debug, Clone)]
struct Built {
    event: Event,
    x: f32,
    x1: f32,
    y: f32,
    up: Option<bool>,
    /// (x, y, width, staff step, diatonic) per chord note, in note order.
    heads: Vec<(f32, f32, f32, i32, i32)>,
}

fn build_chords(heads: &[MusicGlyph], stems: &[Segment], sp: f32) -> Vec<Chord> {
    let mut chords: Vec<Chord> = stems
        .iter()
        .map(|s| Chord {
            heads: Vec::new(),
            stem: Some(*s),
            up: None,
            small: false,
        })
        .collect();
    let mut loose: Vec<MusicGlyph> = Vec::new();
    for h in heads {
        // The stem touches the head's left or right edge and reaches its centre.
        let best = chords
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let s = c.stem.unwrap();
                let dx = (s.a.x - (h.x + h.w)).abs().min((s.a.x - h.x).abs());
                let tol = 0.25 * sp * h.rel_size.max(0.5);
                let covers = h.y >= s.y0() - 0.6 * sp && h.y <= s.y1() + 0.6 * sp;
                (dx < tol && covers).then_some((i, dx))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) => chords[i].heads.push(h.clone()),
            None => loose.push(h.clone()),
        }
    }
    chords.retain(|c| !c.heads.is_empty());
    for c in &mut chords {
        let s = c.stem.unwrap();
        let mean_y = c.heads.iter().map(|h| h.y).sum::<f32>() / c.heads.len() as f32;
        // The stem extends away from the heads.
        c.up = Some(s.y0() < mean_y - 0.5 * sp);
        c.small = c.heads.iter().all(|h| h.rel_size < 0.85);
    }
    // Stemless heads (whole notes) at the same x form chords.
    loose.sort_by(|a, b| a.x.total_cmp(&b.x));
    for h in loose {
        match chords
            .iter_mut()
            .find(|c| c.stem.is_none() && (c.heads[0].x - h.x).abs() < 1.2 * h.w)
        {
            Some(c) => c.heads.push(h),
            None => chords.push(Chord {
                small: h.rel_size < 0.85,
                heads: vec![h],
                stem: None,
                up: None,
            }),
        }
    }
    chords
}

fn build_event(
    item: &Item,
    glyphs: &[&MusicGlyph],
    beams: &[&Fill],
    dots: &[&MusicGlyph],
    segments: &[Segment],
    next_x: f32,
    sp: f32,
) -> Built {
    match item {
        Item::Rest(g) => {
            let Sym::Rest(t) = g.sym else { unreachable!() };
            let n = count_dots(g.x + g.w, &[g.y, g.y - 0.5 * sp], dots, next_x, sp);
            let event = Event::rest(t, n);
            Built {
                event,
                x: g.x,
                x1: g.x + g.w,
                y: g.y,
                up: None,
                heads: Vec::new(),
            }
        }
        Item::Chord(c, notes) => {
            let head_kind = match c.heads[0].sym {
                Sym::Head(h) => h,
                _ => Head::Black,
            };
            let mut note_type = match head_kind {
                Head::DoubleWhole => NoteType::Breve,
                Head::Whole => NoteType::Whole,
                Head::Half => NoteType::Half,
                Head::Black => NoteType::Quarter,
            };
            let mut beam_marks = Vec::new();
            if let (Some(s), Head::Black) = (c.stem, head_kind) {
                let far = if c.up == Some(true) { s.y0() } else { s.y1() };
                let flags = glyphs
                    .iter()
                    .filter_map(|g| match g.sym {
                        Sym::Flag(n, _)
                            if (g.x - s.a.x).abs() < 0.4 * sp && (g.y - far).abs() < 1.5 * sp =>
                        {
                            Some(n)
                        }
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0);
                // Beams crossing the stem, nearest the far end first.
                let mut crossing: Vec<(f32, &Fill)> = beams
                    .iter()
                    .filter_map(|f| {
                        let x = s.a.x.clamp(f.x0 + 0.01, f.x1 - 0.01);
                        if s.a.x < f.x0 - 0.3 * sp || s.a.x > f.x1 + 0.3 * sp {
                            return None;
                        }
                        let (y0, y1) = f.y_range_at(x)?;
                        let mid = (y0 + y1) / 2.0;
                        (mid >= s.y0() - 0.4 * sp && mid <= s.y1() + 0.4 * sp)
                            .then_some(((mid - far).abs(), *f))
                    })
                    .collect();
                crossing.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (level, (_, f)) in crossing.iter().enumerate() {
                    let at_start = (s.a.x - f.x0).abs() < 0.4 * sp;
                    let at_end = (s.a.x - f.x1).abs() < 0.4 * sp;
                    let short = f.x1 - f.x0 < 1.6 * sp;
                    let value = if short && at_start {
                        BeamValue::ForwardHook
                    } else if short && at_end {
                        BeamValue::BackwardHook
                    } else if at_start {
                        BeamValue::Begin
                    } else if at_end {
                        BeamValue::End
                    } else {
                        BeamValue::Continue
                    };
                    beam_marks.push(Beam {
                        number: level as u8 + 1,
                        value,
                    });
                }
                note_type = halve(note_type, flags.max(crossing.len() as u8));
            }
            let pitches: Vec<Pitch> = notes.iter().map(|(n, ..)| n.pitch).collect();
            let head_ys: Vec<f32> = notes.iter().map(|(_, h, ..)| h.y).collect();
            let ys: Vec<f32> = head_ys.iter().flat_map(|&y| [y, y - 0.5 * sp]).collect();
            let n_dots = count_dots(c.head_x1(), &ys, dots, next_x, sp);
            let mut event = if c.small {
                let slash = c.stem.is_some_and(|s| {
                    segments.iter().any(|g| {
                        !g.is_vertical()
                            && !g.is_horizontal()
                            && g.x0() < s.a.x
                            && g.x1() > s.a.x
                            && g.y0() < s.y1()
                            && g.y1() > s.y0()
                    })
                });
                let mut e = Event::grace(pitches[0], note_type, slash);
                if let EventContent::Chord(ns) = &mut e.content {
                    ns.clear();
                    ns.extend(pitches.iter().map(|&p| ChordNote::new(p)));
                }
                e
            } else {
                Event::note(&pitches, note_type, n_dots)
            };
            if let EventContent::Chord(ns) = &mut event.content {
                for (n, (src, ..)) in ns.iter_mut().zip(notes) {
                    n.accidental = src.accidental;
                }
            }
            event.beams = beam_marks;
            if let Some(up) = c.up {
                event.stem = Some(if up { Stem::Up } else { Stem::Down });
            }
            // Articulations and fermatas just above or below the chord.
            let (cx0, cx1) = (c.head_x0() - 0.3 * sp, c.head_x1() + 0.3 * sp);
            for g in glyphs {
                let cx = g.cx();
                if cx < cx0 || cx > cx1 {
                    continue;
                }
                match g.sym {
                    Sym::Articulation(a) => event.articulations.push(a),
                    Sym::Fermata => event.fermata = true,
                    _ => {}
                }
            }
            Built {
                event,
                x: c.head_x0(),
                x1: c.head_x1(),
                y: head_ys.iter().sum::<f32>() / head_ys.len() as f32,
                up: c.up,
                heads: notes
                    .iter()
                    .map(|(_, h, step, d)| (h.x, h.y, h.w, *step, *d))
                    .collect(),
            }
        }
    }
}

/// Dots in distinct columns right of `x`, level with one of `ys`.
fn count_dots(x: f32, ys: &[f32], dots: &[&MusicGlyph], next_x: f32, sp: f32) -> u8 {
    let mut cols: Vec<f32> = Vec::new();
    for d in dots {
        if d.x < x || d.x > (x + 2.5 * sp).min(next_x) {
            continue;
        }
        if !ys.iter().any(|&y| (d.y - y).abs() < 0.4 * sp) {
            continue;
        }
        if !cols.iter().any(|&c| (c - d.x).abs() < 0.3 * sp) {
            cols.push(d.x);
        }
    }
    cols.len() as u8
}

fn is_multi_voice(built: &[Built], sp: f32) -> bool {
    let chords: Vec<&Built> = built
        .iter()
        .filter(|b| !b.heads.is_empty() && !b.event.is_grace())
        .collect();
    let rests: Vec<&Built> = built.iter().filter(|b| b.heads.is_empty()).collect();
    let up_down = chords.iter().any(|a| {
        a.up == Some(true)
            && chords
                .iter()
                .any(|b| b.up == Some(false) && (a.x - b.x).abs() < 1.0 * sp)
    });
    let rest_under_chord = rests.iter().any(|r| {
        chords
            .iter()
            .any(|c| r.x < c.x1 + 0.2 * sp && r.x1 > c.x - 0.2 * sp)
    });
    up_down || rest_under_chord
}

/// Tuplet numbers are italic digits (or SMuFL tuplet digits) above or below
/// a group of notes; the nearest run of that many events gets the ratio.
fn apply_tuplets(built: &mut [Built], voices: &[Vec<usize>], staff: &Staff, mx0: f32, mx1: f32) {
    let sp = staff.sp;
    let mut numbers: Vec<(f32, u8)> = staff
        .glyphs
        .iter()
        .filter_map(|g| match g.sym {
            Sym::TupletDigit(d) if g.cx() >= mx0 && g.cx() < mx1 && d >= 2 => Some((g.cx(), d)),
            _ => None,
        })
        .collect();
    for w in &staff.words {
        if w.cx() < mx0 || w.cx() >= mx1 || !w.italic() || w.x0 < mx0 + 1.5 * sp {
            continue;
        }
        if let Ok(n) = w.text.parse::<u8>()
            && (2..=16).contains(&n)
            && w.y > staff.top() - 5.0 * sp
            && w.y < staff.bottom() + 5.0 * sp
        {
            numbers.push((w.cx(), n));
        }
    }
    for (x, n) in numbers {
        let normal = tuplet_normal(n);
        let mut best: Option<(f32, usize, usize)> = None;
        for (vi, v) in voices.iter().enumerate() {
            let evs: Vec<usize> = v
                .iter()
                .copied()
                .filter(|&i| !built[i].event.is_grace())
                .collect();
            if evs.len() < n as usize {
                continue;
            }
            for start in 0..=evs.len() - n as usize {
                let group = &evs[start..start + n as usize];
                let gx0 = built[group[0]].x;
                let gx1 = built[*group.last().unwrap()].x1;
                if x < gx0 - sp || x > gx1 + sp {
                    continue;
                }
                let dist = ((gx0 + gx1) / 2.0 - x).abs();
                if best.is_none_or(|b| dist < b.0) {
                    best = Some((dist, vi, start));
                }
            }
        }
        if let Some((_, vi, start)) = best {
            let evs: Vec<usize> = voices[vi]
                .iter()
                .copied()
                .filter(|&i| !built[i].event.is_grace())
                .collect();
            let group = &evs[start..start + n as usize];
            for (k, &i) in group.iter().enumerate() {
                let e = built[i].event.clone().time_mod(n, normal);
                let e = if k == 0 {
                    e.tuplet(StartStop::Start)
                } else if k + 1 == group.len() {
                    e.tuplet(StartStop::Stop)
                } else {
                    e
                };
                built[i].event = e;
            }
        }
    }
}

/// Ties and slurs: curved fills whose ends sit next to noteheads.
fn attach_curves(
    staff: &Staff,
    measures: &mut [StaffMeasure],
    heads: &[HeadRef],
    state: &mut StaffState,
) {
    let sp = staff.sp;
    for f in staff
        .fills
        .iter()
        .filter(|f| f.curved && f.x1 - f.x0 > 0.8 * sp && f.y1 - f.y0 < 3.0 * sp)
    {
        let left = f
            .points
            .iter()
            .copied()
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        let right = f
            .points
            .iter()
            .copied()
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        let near = |p: scorelift_pdf::Point, from_left: bool, reach: f32| {
            heads
                .iter()
                .filter(|h| {
                    let edge = if from_left { h.x + h.w } else { h.x };
                    let dx = if from_left { p.x - edge } else { edge - p.x };
                    dx > -0.8 * sp && dx < reach * sp && (p.y - h.y).abs() < reach * sp
                })
                .min_by(|a, b| {
                    let da = (a.x - p.x).abs() + (a.y - p.y).abs();
                    let db = (b.x - p.x).abs() + (b.y - p.y).abs();
                    da.total_cmp(&db)
                })
                .copied()
        };
        let flat = (left.y - right.y).abs() < 0.3 * sp;
        let a = near(left, true, 1.2);
        let b = near(right, false, 1.2);
        match (a, b) {
            (Some(a), Some(b)) if flat && a.diatonic == b.diatonic => {
                set_tie(measures, &a, true);
                set_tie(measures, &b, false);
            }
            (Some(a), None) if flat && right.x > staff.x1 - 2.0 * sp => {
                // Tie into the next system: the continued note keeps its alteration.
                set_tie(measures, &a, true);
                if let Some(alter) = note_alter(measures, &a) {
                    state.tied_alters.insert(a.diatonic, alter);
                }
            }
            (None, Some(b)) if flat && left.x < staff.x0 + 8.0 * sp => set_tie(measures, &b, false),
            _ => {
                // A slur: ends are further from the heads.
                let (Some(a), Some(b)) = (near(left, true, 2.5), near(right, false, 2.5)) else {
                    continue;
                };
                if let Some(e) = measures
                    .get_mut(a.measure)
                    .and_then(|m| m.voices.get_mut(a.voice))
                    .and_then(|v| v.get_mut(a.event))
                {
                    e.slurs.push(Slur {
                        number: 1,
                        kind: StartStop::Start,
                    });
                }
                if let Some(e) = measures
                    .get_mut(b.measure)
                    .and_then(|m| m.voices.get_mut(b.voice))
                    .and_then(|v| v.get_mut(b.event))
                {
                    e.slurs.push(Slur {
                        number: 1,
                        kind: StartStop::Stop,
                    });
                }
            }
        }
    }
}

fn note_alter(measures: &[StaffMeasure], h: &HeadRef) -> Option<i8> {
    match &measures
        .get(h.measure)?
        .voices
        .get(h.voice)?
        .get(h.event)?
        .content
    {
        EventContent::Chord(ns) => ns.get(h.note).map(|n| n.pitch.alter),
        _ => None,
    }
}

fn set_tie(measures: &mut [StaffMeasure], h: &HeadRef, start: bool) {
    if let Some(EventContent::Chord(ns)) = measures
        .get_mut(h.measure)
        .and_then(|m| m.voices.get_mut(h.voice))
        .and_then(|v| v.get_mut(h.event))
        .map(|e| &mut e.content)
        && let Some(n) = ns.get_mut(h.note)
    {
        if start {
            n.tie_start = true;
        } else {
            n.tie_stop = true;
        }
    }
}

/// Measure index and offset for an x position on this staff.
fn place(measures: &[StaffMeasure], x: f32) -> Option<(usize, Fraction)> {
    let mi = measures
        .iter()
        .position(|m| x >= m.x0 - 0.01 && x < m.x1)
        .or_else(|| {
            if x < measures.first()?.x0 {
                Some(0)
            } else {
                None
            }
        })?;
    let m = &measures[mi];
    let offset = m
        .anchors
        .iter()
        .filter(|(ax, _)| *ax <= x + 0.5)
        .map(|(_, t)| *t)
        .next_back()
        .unwrap_or(Fraction::ZERO);
    Some((mi, offset))
}

fn attach_directions(staff: &Staff, measures: &mut [StaffMeasure]) {
    let sp = staff.sp;
    let placement = |y: f32| {
        if y > staff.middle() {
            Placement::Below
        } else {
            Placement::Above
        }
    };
    let push = |measures: &mut [StaffMeasure], x: f32, y: f32, kind: DirectionKind| {
        if let Some((mi, offset)) = place(measures, x) {
            let mut d = Direction::new(offset, kind);
            d.placement = Some(placement(y));
            measures[mi].directions.push(d);
        }
    };

    for g in &staff.glyphs {
        if let Sym::Dynamic(d) = g.sym {
            push(measures, g.x, g.y, DirectionKind::Dynamic(d));
        }
    }

    // Hairpins: two slanted strokes meeting at the tip.
    let slanted: Vec<&Segment> = staff
        .segments
        .iter()
        .filter(|s| {
            !s.is_horizontal()
                && !s.is_vertical()
                && (s.a.y - s.b.y).abs() < 0.3 * (s.a.x - s.b.x).abs()
        })
        .collect();
    let mut used = vec![false; slanted.len()];
    for i in 0..slanted.len() {
        for j in i + 1..slanted.len() {
            if used[i] || used[j] {
                continue;
            }
            let (a, b) = (slanted[i], slanted[j]);
            let ends = [(a.a, b.a), (a.a, b.b), (a.b, b.a), (a.b, b.b)];
            let Some(&(tip, _)) = ends
                .iter()
                .find(|(p, q)| (p.x - q.x).abs() < 0.2 && (p.y - q.y).abs() < 0.2)
            else {
                continue;
            };
            used[i] = true;
            used[j] = true;
            let x0 = a.x0().min(b.x0());
            let x1 = a.x1().max(b.x1());
            let kind = if (tip.x - x0).abs() < (tip.x - x1).abs() {
                WedgeKind::Crescendo
            } else {
                WedgeKind::Diminuendo
            };
            push(measures, x0, tip.y, DirectionKind::Wedge(kind));
            push(measures, x1, tip.y, DirectionKind::Wedge(WedgeKind::Stop));
        }
    }

    // Hairpins drawn as one filled outline: thin at the tip, open at the
    // other end (a beam keeps the same thickness all along).
    for f in &staff.fills {
        let (w, h) = (f.x1 - f.x0, f.y1 - f.y0);
        let outside = f.y0 > staff.bottom() + 0.3 * sp || f.y1 < staff.top() - 0.3 * sp;
        if f.curved || !outside || w < 2.0 * sp || h > 2.5 * sp {
            continue;
        }
        let thickness = |x: f32| f.y_range_at(x).map_or(0.0, |(a, b)| b - a);
        let (left, right) = (thickness(f.x0 + 0.05 * w), thickness(f.x1 - 0.05 * w));
        let kind = if left < 0.4 * right {
            WedgeKind::Crescendo
        } else if right < 0.4 * left {
            WedgeKind::Diminuendo
        } else {
            continue;
        };
        let y = (f.y0 + f.y1) / 2.0;
        push(measures, f.x0, y, DirectionKind::Wedge(kind));
        push(measures, f.x1, y, DirectionKind::Wedge(WedgeKind::Stop));
    }

    // Tempo: a metronome note glyph followed by "= N".
    let mut tempo_words: Vec<(f32, f32)> = Vec::new();
    for g in &staff.glyphs {
        let Sym::MetNote(unit) = g.sym else { continue };
        let after: Vec<&Word> = {
            let mut v: Vec<&Word> = staff
                .words
                .iter()
                .filter(|w| (w.y - g.y).abs() < 0.5 * sp && w.x0 > g.x && w.x0 < g.x + 8.0 * sp)
                .collect();
            v.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            v
        };
        let text: String = after
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        let digits: String = text
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !text.contains('=') {
            continue;
        }
        if let Ok(bpm) = digits.parse::<u16>() {
            let dots = staff
                .glyphs
                .iter()
                .filter(|d| {
                    matches!(d.sym, Sym::MetDot | Sym::Dot)
                        && (d.y - g.y).abs() < 0.5 * sp
                        && d.x > g.x
                        && d.x < g.x + 2.0 * g.w.max(sp)
                })
                .count() as u8;
            push(
                measures,
                g.x,
                g.y,
                DirectionKind::Tempo {
                    beat_unit: unit,
                    dots,
                    per_minute: bpm,
                    text: None,
                },
            );
            for w in after {
                tempo_words.push((w.x0, w.y));
            }
        }
    }

    // Remaining words: rehearsal marks (boxed), measure numbers, ending labels, plain text.
    let boxed = |w: &Word| {
        let has = |left: bool| {
            staff.segments.iter().any(|s| {
                s.is_vertical()
                    && s.y0() < w.y - 0.3 * w.size
                    && s.y1() > w.y
                    && if left {
                        s.a.x < w.x0 && s.a.x > w.x0 - 0.8 * w.size
                    } else {
                        s.a.x > w.x1 && s.a.x < w.x1 + 0.8 * w.size
                    }
            })
        };
        has(true) && has(false)
    };
    for p in phrases(&staff.words) {
        if tempo_words
            .iter()
            .any(|&(x, y)| (x - p.x0).abs() < 0.01 && (y - p.y).abs() < 0.01)
        {
            continue;
        }
        let is_number = p
            .text
            .trim_end_matches('.')
            .chars()
            .all(|c| c.is_ascii_digit());
        if is_number {
            // Measure numbers, tuplet numbers and ending labels.
            continue;
        }
        if p.text == "-" || p.text.chars().all(|c| c == '_') {
            continue;
        }
        let below = p.y > staff.bottom();
        let single = !p.text.contains(' ');
        if below && single && !p.italic() && !p.bold() {
            // Handled as a lyric.
            continue;
        }
        if p.y > staff.top() - 0.5 * sp && p.y < staff.bottom() + 0.5 * sp {
            continue;
        }
        let kind = if boxed(&p) {
            DirectionKind::Rehearsal(p.text.clone())
        } else {
            DirectionKind::Words(p.text.clone())
        };
        push(measures, p.x0, p.y, kind);
    }
}

/// Words on one baseline, in one font, separated by no more than a space,
/// joined into phrases ("let ring" is one phrase, lyric syllables are not).
fn phrases(words: &[Word]) -> Vec<Word> {
    let mut sorted: Vec<&Word> = words.iter().collect();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<Word> = Vec::new();
    for w in sorted {
        // A hyphen between words is a lyric hyphen, never part of a phrase.
        if let Some(p) = out.last_mut()
            && (p.y - w.y).abs() < 0.1
            && p.font == w.font
            && w.x0 - p.x1 < 0.6 * w.size
            && p.text != "-"
            && w.text != "-"
        {
            p.text.push(' ');
            p.text.push_str(&w.text);
            p.x1 = w.x1;
            continue;
        }
        out.push(w.clone());
    }
    out
}

/// Single words below the staff, centred under a note, become lyrics.
/// Hyphens between syllables are either "-" text or short drawn lines;
/// a longer line after a syllable is a melisma extender.
fn attach_lyrics(staff: &Staff, measures: &mut [StaffMeasure], heads: &[HeadRef]) {
    let sp = staff.sp;
    let mut lines: Vec<f32> = Vec::new();
    let all = phrases(&staff.words);
    let candidates: Vec<&Word> = all
        .iter()
        .filter(|w| {
            w.y > staff.bottom() + 0.5 * sp
                && !w.italic()
                && !w.bold()
                && !w.text.contains(' ')
                && !w.text.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    for w in &candidates {
        if w.text != "-" && !lines.iter().any(|&y| (y - w.y).abs() < 0.5 * sp) {
            lines.push(w.y);
        }
    }
    lines.sort_by(|a, b| a.total_cmp(b));
    let drawn_line = |line_y: f32, size: f32, x0: f32, x1: f32, min_len: f32, max_len: f32| {
        staff.segments.iter().any(|s| {
            s.is_horizontal()
                && s.a.y <= line_y + 0.1 * size
                && s.a.y >= line_y - 0.8 * size
                && s.x0() >= x0 - 0.1
                && s.x1() <= x1 + 0.1
                && s.len() >= min_len
                && s.len() <= max_len
        })
    };
    for (verse, &line_y) in lines.iter().enumerate() {
        let mut row: Vec<&Word> = candidates
            .iter()
            .copied()
            .filter(|w| (w.y - line_y).abs() < 0.5 * sp)
            .collect();
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let syllables: Vec<&Word> = row.iter().copied().filter(|w| w.text != "-").collect();
        let joined = |a: &Word, b: &Word| {
            row.iter()
                .any(|w| w.text == "-" && w.x0 >= a.x1 - 0.1 && w.x1 <= b.x0 + 0.1)
                || drawn_line(line_y, a.size, a.x1, b.x0, 0.1, 1.5 * sp)
        };
        let mut placed: Vec<(usize, usize, usize, String, bool, bool, bool)> = Vec::new();
        for (i, w) in syllables.iter().enumerate() {
            let Some(h) = heads
                .iter()
                .filter(|h| h.voice == 0 && ((h.x + h.w / 2.0) - w.cx()).abs() < 1.5 * sp)
                .min_by(|a, b| {
                    ((a.x + a.w / 2.0) - w.cx())
                        .abs()
                        .total_cmp(&((b.x + b.w / 2.0) - w.cx()).abs())
                })
            else {
                continue;
            };
            let before = i > 0 && joined(syllables[i - 1], w);
            let next_x = syllables.get(i + 1).map_or(staff.x1, |n| n.x0);
            let after = syllables.get(i + 1).is_some_and(|n| joined(w, n));
            let extend = !after && drawn_line(line_y, w.size, w.x1, next_x, 1.5 * sp, f32::MAX);
            placed.push((
                h.measure,
                h.voice,
                h.event,
                w.text.clone(),
                before,
                after,
                extend,
            ));
        }
        placed.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        for (mi, vi, ei, text, before, after, extend) in placed {
            let syllabic = match (before, after) {
                (false, false) => Syllabic::Single,
                (false, true) => Syllabic::Begin,
                (true, true) => Syllabic::Middle,
                (true, false) => Syllabic::End,
            };
            if let Some(e) = measures
                .get_mut(mi)
                .and_then(|m| m.voices.get_mut(vi))
                .and_then(|v| v.get_mut(ei))
            {
                e.lyrics.push(Lyric {
                    number: verse as u8 + 1,
                    syllabic,
                    text,
                    extend,
                });
            }
        }
    }
}

/// Barline style of a group, from its stroke widths.
pub fn bar_style(g: &BarGroup, sp: f32) -> BarStyle {
    let thin = g.widths.iter().copied().fold(f32::MAX, f32::min);
    let thick: Vec<bool> = g
        .widths
        .iter()
        .map(|&w| w > 1.8 * thin && w > 0.25 * sp)
        .collect();
    match thick.as_slice() {
        [false] => BarStyle::Regular,
        [true] => BarStyle::Heavy,
        [false, false] => BarStyle::LightLight,
        [false, true] => BarStyle::LightHeavy,
        [true, false] => BarStyle::HeavyLight,
        [true, true] => BarStyle::HeavyHeavy,
        _ => BarStyle::Regular,
    }
}

/// Volta brackets above the top staff: (first x, last x, label, closed at the end).
pub fn endings(staff: &Staff) -> Vec<(f32, f32, String, bool)> {
    let sp = staff.sp;
    let mut out = Vec::new();
    for s in &staff.segments {
        if !s.is_horizontal() || s.a.y > staff.top() - 1.0 * sp || s.len() < 2.0 * sp {
            continue;
        }
        // Line caps can put the hook half a line width away from the end.
        let hook = |x: f32| {
            staff.segments.iter().any(|v| {
                v.is_vertical()
                    && (v.a.x - x).abs() < 0.3 * sp
                    && (v.y0() - s.a.y).abs() < 0.3 * sp
                    && v.len() > 0.8 * sp
            })
        };
        if !hook(s.x0()) {
            continue;
        }
        let label = staff
            .words
            .iter()
            .filter(|w| {
                w.x0 >= s.x0() - 0.2
                    && w.x0 < s.x0() + 2.0 * sp
                    && w.y > s.a.y
                    && w.y < s.a.y + 3.0 * sp
            })
            .map(|w| w.text.trim_end_matches('.').to_string())
            .find(|t| {
                !t.is_empty()
                    && t.split([',', ' '])
                        .all(|p| p.trim().parse::<u8>().is_ok() || p.trim().is_empty())
            });
        if let Some(label) = label {
            out.push((s.x0(), s.x1(), label, hook(s.x1())));
        }
    }
    out
}
