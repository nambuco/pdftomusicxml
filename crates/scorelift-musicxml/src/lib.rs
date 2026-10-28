//! Writes a [`Score`] as MusicXML 4.0 (`score-partwise`), either as plain
//! XML (`.musicxml`) or compressed (`.mxl`).
//!
//! Element order follows the MusicXML 4.0 XSD, which is strict about
//! sequence; `scripts/validate-xsd.sh` checks the output against it.

mod xml;

use std::io::{Cursor, Write as _};
use std::path::Path;

use scorelift_model::*;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use xml::XmlWriter;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the score has no parts")]
    NoParts,
    #[error("part {0} has no measures")]
    NoMeasures(String),
    #[error("part {0}: these durations need too many divisions per quarter note")]
    Divisions(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
}

/// The largest `<divisions>` value we emit. Many readers store durations in
/// 32-bit integers.
const MAX_DIVISIONS: u64 = 1 << 20;

const MXL_SCORE_NAME: &str = "score.musicxml";

/// Serializes `score` as an uncompressed MusicXML document.
pub fn to_musicxml_string(score: &Score) -> Result<String, Error> {
    if score.parts.is_empty() {
        return Err(Error::NoParts);
    }
    let mut w = XmlWriter::new();
    w.raw_line(r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>"#);
    w.raw_line(
        r#"<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">"#,
    );
    w.open("score-partwise", &[("version", "4.0")]);
    write_header(&mut w, &score.metadata);
    write_part_list(&mut w, &score.parts);
    for part in &score.parts {
        write_part(&mut w, part)?;
    }
    w.close();
    Ok(w.finish())
}

/// Serializes `score` as a compressed MusicXML (`.mxl`) archive.
pub fn to_mxl_bytes(score: &Score) -> Result<Vec<u8>, Error> {
    let xml = to_musicxml_string(score)?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // The mimetype entry must come first and be stored uncompressed.
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )?;
    zip.write_all(b"application/vnd.recordare.musicxml")?;
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(
        format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<container>\n",
                "  <rootfiles>\n",
                "    <rootfile full-path=\"{}\" media-type=\"application/vnd.recordare.musicxml+xml\"/>\n",
                "  </rootfiles>\n",
                "</container>\n"
            ),
            MXL_SCORE_NAME
        )
        .as_bytes(),
    )?;
    zip.start_file(MXL_SCORE_NAME, deflated)?;
    zip.write_all(xml.as_bytes())?;
    Ok(zip.finish()?.into_inner())
}

/// Writes `score` to `path`: compressed if the extension is `.mxl`,
/// plain XML otherwise.
pub fn write_file(score: &Score, path: &Path) -> Result<(), Error> {
    let compressed = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mxl"));
    if compressed {
        std::fs::write(path, to_mxl_bytes(score)?)?;
    } else {
        std::fs::write(path, to_musicxml_string(score)?)?;
    }
    Ok(())
}

fn write_header(w: &mut XmlWriter, meta: &Metadata) {
    // No <credit> elements: without page coordinates, editors pile them up
    // in one corner. Given the titles and creators, MuseScore lays out the
    // title and composer itself; it prefers movement-title, so write both.
    // The subtitle has no un-positioned home and goes to <miscellaneous>.
    if let Some(title) = &meta.title {
        w.open("work", &[]);
        w.text("work-title", &[], title);
        w.close();
        w.text("movement-title", &[], title);
    }

    w.open("identification", &[]);
    for (kind, value) in [
        ("composer", &meta.composer),
        ("lyricist", &meta.lyricist),
        ("arranger", &meta.arranger),
    ] {
        if let Some(v) = value {
            w.text("creator", &[("type", kind)], v);
        }
    }
    if let Some(rights) = &meta.rights {
        w.text("rights", &[], rights);
    }
    w.open("encoding", &[]);
    w.text(
        "software",
        &[],
        concat!("scorelift ", env!("CARGO_PKG_VERSION")),
    );
    w.close();
    let subtitle = meta.subtitle.iter().map(|s| ("subtitle", s.as_str()));
    let misc: Vec<(&str, &str)> = subtitle
        .chain(meta.misc.iter().map(|(n, v)| (n.as_str(), v.as_str())))
        .collect();
    if !misc.is_empty() {
        w.open("miscellaneous", &[]);
        for (name, value) in misc {
            w.text("miscellaneous-field", &[("name", name)], value);
        }
        w.close();
    }
    w.close();
}

fn write_part_list(w: &mut XmlWriter, parts: &[Part]) {
    w.open("part-list", &[]);
    for part in parts {
        w.open("score-part", &[("id", &part.id)]);
        w.text("part-name", &[], &part.name);
        if let Some(abbr) = &part.abbreviation {
            w.text("part-abbreviation", &[], abbr);
        }
        w.close();
    }
    w.close();
}

/// Divisions per quarter note: the smallest value that makes every duration
/// and direction offset in the part a whole number.
fn divisions_for(part: &Part) -> Result<u64, Error> {
    // f is in whole notes; in quarters it is 4f.
    let include = |divisions: u64, f: Fraction| lcm(divisions, (f * 4).den());
    let mut divisions = 1u64;
    for m in &part.measures {
        for v in &m.voices {
            for e in &v.events {
                divisions = include(divisions, e.duration);
            }
        }
        for d in &m.directions {
            divisions = include(divisions, d.offset);
        }
        if divisions > MAX_DIVISIONS {
            return Err(Error::Divisions(part.id.clone()));
        }
    }
    Ok(divisions)
}

/// Converts a duration in whole notes to divisions.
fn to_div(f: Fraction, divisions: u64) -> u64 {
    let d = f * (4 * divisions);
    debug_assert_eq!(d.den(), 1, "{f} is not a multiple of 1/{divisions} quarter");
    d.num()
}

fn write_part(w: &mut XmlWriter, part: &Part) -> Result<(), Error> {
    if part.measures.is_empty() {
        return Err(Error::NoMeasures(part.id.clone()));
    }
    let divisions = divisions_for(part)?;
    w.open("part", &[("id", &part.id)]);
    for (i, m) in part.measures.iter().enumerate() {
        let number = m.number.to_string();
        let mut attrs = vec![("number", number.as_str())];
        if m.implicit {
            attrs.push(("implicit", "yes"));
        }
        w.open("measure", &attrs);
        let first = i == 0;

        if (m.new_system || m.new_page) && !first {
            let mut attrs = Vec::new();
            if m.new_system {
                attrs.push(("new-system", "yes"));
            }
            if m.new_page {
                attrs.push(("new-page", "yes"));
            }
            w.empty("print", &attrs);
        }
        if let Some(b) = &m.left_barline {
            write_barline(w, b, "left");
        }
        if first || !m.attributes.is_empty() {
            write_attributes(w, &m.attributes, first.then_some((divisions, part.staves)));
        }
        write_measure_content(w, m, part.staves, divisions);
        if let Some(b) = &m.right_barline {
            write_barline(w, b, "right");
        }
        w.close();
    }
    w.close();
    Ok(())
}

fn write_attributes(w: &mut XmlWriter, a: &Attributes, first: Option<(u64, u8)>) {
    w.open("attributes", &[]);
    if let Some((divisions, _)) = first {
        w.text("divisions", &[], divisions);
    }
    if let Some(key) = &a.key {
        w.open("key", &[]);
        w.text("fifths", &[], key.fifths);
        if let Some(mode) = key.mode {
            w.text(
                "mode",
                &[],
                match mode {
                    Mode::Major => "major",
                    Mode::Minor => "minor",
                },
            );
        }
        w.close();
    }
    if let Some(time) = &a.time {
        let symbol = time.symbol.map(|s| match s {
            TimeSymbol::Common => "common",
            TimeSymbol::Cut => "cut",
        });
        let attrs: Vec<_> = symbol.map(|s| ("symbol", s)).into_iter().collect();
        w.open("time", &attrs);
        w.text("beats", &[], time.beats);
        w.text("beat-type", &[], time.beat_type);
        w.close();
    }
    if let Some((_, staves)) = first
        && staves > 1
    {
        w.text("staves", &[], staves);
    }
    for c in &a.clefs {
        let number = c.staff.to_string();
        w.open("clef", &[("number", &number)]);
        w.text(
            "sign",
            &[],
            match c.clef.sign {
                ClefSign::G => "G",
                ClefSign::F => "F",
                ClefSign::C => "C",
                ClefSign::Percussion => "percussion",
                ClefSign::Tab => "TAB",
            },
        );
        w.text("line", &[], c.clef.line);
        if c.clef.octave_change != 0 {
            w.text("clef-octave-change", &[], c.clef.octave_change);
        }
        w.close();
    }
    if let Some(n) = a.multiple_rest {
        w.open("measure-style", &[]);
        w.text("multiple-rest", &[], n);
        w.close();
    }
    w.close();
}

fn write_barline(w: &mut XmlWriter, b: &Barline, location: &str) {
    w.open("barline", &[("location", location)]);
    w.text(
        "bar-style",
        &[],
        match b.style {
            BarStyle::Regular => "regular",
            BarStyle::Dotted => "dotted",
            BarStyle::Dashed => "dashed",
            BarStyle::Heavy => "heavy",
            BarStyle::LightLight => "light-light",
            BarStyle::LightHeavy => "light-heavy",
            BarStyle::HeavyLight => "heavy-light",
            BarStyle::HeavyHeavy => "heavy-heavy",
            BarStyle::None => "none",
        },
    );
    if let Some(e) = &b.ending {
        let kind = match e.kind {
            EndingKind::Start => "start",
            EndingKind::Stop => "stop",
            EndingKind::Discontinue => "discontinue",
        };
        w.empty("ending", &[("number", &e.number), ("type", kind)]);
    }
    if let Some(r) = b.repeat {
        let direction = match r {
            RepeatDirection::Forward => "forward",
            RepeatDirection::Backward => "backward",
        };
        w.empty("repeat", &[("direction", direction)]);
    }
    w.close();
}

/// Writes the voices one after another with `<backup>` in between.
/// Directions are anchored in the first voice at the right time position.
fn write_measure_content(w: &mut XmlWriter, m: &Measure, staves: u8, divisions: u64) {
    let mut directions: Vec<&Direction> = m.directions.iter().collect();
    directions.sort_by_key(|d| d.offset);
    let mut pending = directions.into_iter().peekable();

    for (i, voice) in m.voices.iter().enumerate() {
        let mut t = Fraction::ZERO;
        for event in &voice.events {
            if i == 0 {
                while let Some(d) = pending.next_if(|d| d.offset < t + event.duration) {
                    write_direction(w, d, d.offset - t, staves, divisions);
                }
            }
            write_event(w, event, voice, staves, divisions);
            t += event.duration;
        }
        if i == 0 {
            for d in pending.by_ref() {
                write_direction(w, d, d.offset - t, staves, divisions);
            }
        }
        if i + 1 < m.voices.len() && !t.is_zero() {
            w.open("backup", &[]);
            w.text("duration", &[], to_div(t, divisions));
            w.close();
        }
    }
    // A measure without voices still keeps its directions.
    for d in pending {
        write_direction(w, d, d.offset, staves, divisions);
    }
}

fn note_type_name(t: NoteType) -> &'static str {
    match t {
        NoteType::Breve => "breve",
        NoteType::Whole => "whole",
        NoteType::Half => "half",
        NoteType::Quarter => "quarter",
        NoteType::Eighth => "eighth",
        NoteType::Sixteenth => "16th",
        NoteType::ThirtySecond => "32nd",
        NoteType::SixtyFourth => "64th",
        NoteType::OneHundredTwentyEighth => "128th",
    }
}

fn start_stop(s: StartStop) -> &'static str {
    match s {
        StartStop::Start => "start",
        StartStop::Stop => "stop",
    }
}

fn write_event(w: &mut XmlWriter, e: &Event, voice: &Voice, staves: u8, divisions: u64) {
    match &e.content {
        EventContent::Rest { whole_measure } => {
            w.open("note", &[]);
            if *whole_measure {
                w.empty("rest", &[("measure", "yes")]);
            } else {
                w.empty("rest", &[]);
            }
            w.text("duration", &[], to_div(e.duration, divisions));
            write_note_tail(w, e, None, true, voice, staves);
            w.close();
        }
        EventContent::Chord(notes) => {
            for (i, n) in notes.iter().enumerate() {
                let first = i == 0;
                w.open("note", &[]);
                if e.is_grace() {
                    let slash = e.grace.is_some_and(|g| g.slash);
                    w.empty("grace", if slash { &[("slash", "yes")] } else { &[] });
                }
                if !first {
                    w.empty("chord", &[]);
                }
                write_pitch(w, n.pitch);
                if !e.is_grace() {
                    w.text("duration", &[], to_div(e.duration, divisions));
                }
                if n.tie_stop {
                    w.empty("tie", &[("type", "stop")]);
                }
                if n.tie_start {
                    w.empty("tie", &[("type", "start")]);
                }
                write_note_tail(w, e, Some(n), first, voice, staves);
                w.close();
            }
        }
    }
}

fn write_pitch(w: &mut XmlWriter, p: Pitch) {
    w.open("pitch", &[]);
    w.text(
        "step",
        &[],
        match p.step {
            Step::C => "C",
            Step::D => "D",
            Step::E => "E",
            Step::F => "F",
            Step::G => "G",
            Step::A => "A",
            Step::B => "B",
        },
    );
    if p.alter != 0 {
        w.text("alter", &[], p.alter);
    }
    w.text("octave", &[], p.octave);
    w.close();
}

/// Everything after `<tie>` in a `<note>`, in XSD order. Beams, slurs,
/// articulations and lyrics go on the first note of a chord only.
fn write_note_tail(
    w: &mut XmlWriter,
    e: &Event,
    note: Option<&ChordNote>,
    first: bool,
    voice: &Voice,
    staves: u8,
) {
    w.text("voice", &[], voice.number);
    if let Some(t) = e.note_type {
        w.text("type", &[], note_type_name(t));
    }
    for _ in 0..e.dots {
        w.empty("dot", &[]);
    }
    if let Some(a) = note.and_then(|n| n.accidental) {
        w.text(
            "accidental",
            &[],
            match a {
                Accidental::Sharp => "sharp",
                Accidental::Flat => "flat",
                Accidental::Natural => "natural",
                Accidental::DoubleSharp => "double-sharp",
                Accidental::DoubleFlat => "flat-flat",
            },
        );
    }
    if let Some(tm) = e.time_modification {
        w.open("time-modification", &[]);
        w.text("actual-notes", &[], tm.actual);
        w.text("normal-notes", &[], tm.normal);
        w.close();
    }
    if let Some(s) = e.stem {
        w.text(
            "stem",
            &[],
            match s {
                Stem::Up => "up",
                Stem::Down => "down",
                Stem::None => "none",
            },
        );
    }
    if staves > 1 {
        w.text("staff", &[], voice.staff);
    }
    if first {
        for b in &e.beams {
            let number = b.number.to_string();
            w.text(
                "beam",
                &[("number", &number)],
                match b.value {
                    BeamValue::Begin => "begin",
                    BeamValue::Continue => "continue",
                    BeamValue::End => "end",
                    BeamValue::ForwardHook => "forward hook",
                    BeamValue::BackwardHook => "backward hook",
                },
            );
        }
    }
    write_notations(w, e, note, first);
    if first {
        for l in &e.lyrics {
            let number = l.number.to_string();
            w.open("lyric", &[("number", &number)]);
            w.text(
                "syllabic",
                &[],
                match l.syllabic {
                    Syllabic::Single => "single",
                    Syllabic::Begin => "begin",
                    Syllabic::Middle => "middle",
                    Syllabic::End => "end",
                },
            );
            w.text("text", &[], &l.text);
            if l.extend {
                w.empty("extend", &[]);
            }
            w.close();
        }
    }
}

fn write_notations(w: &mut XmlWriter, e: &Event, note: Option<&ChordNote>, first: bool) {
    let tied = note.is_some_and(|n| n.tie_start || n.tie_stop || n.let_ring);
    let per_event = first
        && (!e.slurs.is_empty() || e.tuplet.is_some() || !e.articulations.is_empty() || e.fermata);
    if !tied && !per_event {
        return;
    }
    w.open("notations", &[]);
    if let Some(n) = note {
        if n.tie_stop {
            w.empty("tied", &[("type", "stop")]);
        }
        if n.tie_start {
            w.empty("tied", &[("type", "start")]);
        }
        if n.let_ring {
            w.empty("tied", &[("type", "let-ring")]);
        }
    }
    if first {
        for s in &e.slurs {
            let number = s.number.to_string();
            w.empty("slur", &[("type", start_stop(s.kind)), ("number", &number)]);
        }
        if let Some(t) = e.tuplet {
            w.empty("tuplet", &[("type", start_stop(t))]);
        }
        if !e.articulations.is_empty() {
            w.open("articulations", &[]);
            for a in &e.articulations {
                w.empty(
                    match a {
                        Articulation::Accent => "accent",
                        Articulation::StrongAccent => "strong-accent",
                        Articulation::Staccato => "staccato",
                        Articulation::Staccatissimo => "staccatissimo",
                        Articulation::Tenuto => "tenuto",
                    },
                    &[],
                );
            }
            w.close();
        }
        if e.fermata {
            w.empty("fermata", &[]);
        }
    }
    w.close();
}

fn write_direction(w: &mut XmlWriter, d: &Direction, offset: Fraction, staves: u8, divisions: u64) {
    let attrs: Vec<_> = d
        .placement
        .map(|p| {
            (
                "placement",
                match p {
                    Placement::Above => "above",
                    Placement::Below => "below",
                },
            )
        })
        .into_iter()
        .collect();
    w.open("direction", &attrs);
    let mut sound_tempo = None;
    match &d.kind {
        DirectionKind::Dynamic(dynamic) => {
            w.open("direction-type", &[]);
            w.open("dynamics", &[]);
            w.empty(
                match dynamic {
                    Dynamic::Ppp => "ppp",
                    Dynamic::Pp => "pp",
                    Dynamic::P => "p",
                    Dynamic::Mp => "mp",
                    Dynamic::Mf => "mf",
                    Dynamic::F => "f",
                    Dynamic::Ff => "ff",
                    Dynamic::Fff => "fff",
                    Dynamic::Fp => "fp",
                    Dynamic::Sf => "sf",
                    Dynamic::Sfz => "sfz",
                },
                &[],
            );
            w.close();
            w.close();
        }
        DirectionKind::Words(text) => {
            w.open("direction-type", &[]);
            w.text("words", &[], text);
            w.close();
        }
        DirectionKind::Rehearsal(text) => {
            w.open("direction-type", &[]);
            w.text("rehearsal", &[], text);
            w.close();
        }
        DirectionKind::Tempo {
            beat_unit,
            dots,
            per_minute,
            text,
        } => {
            if let Some(text) = text {
                w.open("direction-type", &[]);
                w.text("words", &[], text);
                w.close();
            }
            w.open("direction-type", &[]);
            w.open("metronome", &[]);
            w.text("beat-unit", &[], note_type_name(*beat_unit));
            for _ in 0..*dots {
                w.empty("beat-unit-dot", &[]);
            }
            w.text("per-minute", &[], per_minute);
            w.close();
            w.close();
            // <sound tempo> is always in quarter notes per minute.
            let quarters = beat_unit.dotted_duration(*dots) * 4;
            sound_tempo =
                Some(f64::from(*per_minute) * quarters.num() as f64 / quarters.den() as f64);
        }
        DirectionKind::Wedge(kind) => {
            w.open("direction-type", &[]);
            w.empty(
                "wedge",
                &[(
                    "type",
                    match kind {
                        WedgeKind::Crescendo => "crescendo",
                        WedgeKind::Diminuendo => "diminuendo",
                        WedgeKind::Stop => "stop",
                    },
                )],
            );
            w.close();
        }
    }
    if !offset.is_zero() {
        w.text("offset", &[], to_div(offset, divisions));
    }
    if staves > 1 {
        w.text("staff", &[], d.staff);
    }
    if let Some(tempo) = sound_tempo {
        let tempo = format_decimal(tempo);
        w.empty("sound", &[("tempo", &tempo)]);
    }
    w.close();
}

fn format_decimal(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
