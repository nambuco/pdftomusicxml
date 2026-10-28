//! Reads the musical content of a MusicXML file (`score-partwise`, plain or
//! `.mxl`): parts, measures, notes, rests, chords, grace notes, tuplets, ties,
//! keys, time signatures and clefs. Layout, directions and lyrics are skipped.
//!
//! It exists to load hand-made transcriptions as ground truth for accuracy
//! measurement, so it accepts what notation programs export rather than
//! validating it.

use std::io::Read as _;
use std::path::Path;

use scorelift_model::*;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("not MusicXML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("only score-partwise MusicXML is supported")]
    NotPartwise,
    #[error("measure {measure}: {message}")]
    Content { measure: String, message: String },
    #[error("the .mxl archive has no score")]
    NoRootfile,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
}

/// Reads a `.musicxml`/`.xml` file, or a compressed `.mxl` archive.
pub fn read_file(path: &Path) -> Result<Score, ReadError> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(b"PK") {
        return read_str(&mxl_score(bytes)?);
    }
    read_str(&String::from_utf8_lossy(&bytes))
}

fn mxl_score(bytes: Vec<u8>) -> Result<String, ReadError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut container = String::new();
    zip.by_name("META-INF/container.xml")?
        .read_to_string(&mut container)?;
    let doc = roxmltree::Document::parse(&container)?;
    let path = doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .ok_or(ReadError::NoRootfile)?
        .to_string();
    let mut xml = String::new();
    zip.by_name(&path)?.read_to_string(&mut xml)?;
    Ok(xml)
}

/// Parses an uncompressed MusicXML document.
pub fn read_str(xml: &str) -> Result<Score, ReadError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(xml, options)?;
    let root = doc.root_element();
    if !root.has_tag_name("score-partwise") {
        return Err(ReadError::NotPartwise);
    }
    let mut score = Score::default();
    score.metadata.title = child_text(root, "movement-title")
        .or_else(|| {
            root.children()
                .find(|n| n.has_tag_name("work"))
                .and_then(|w| child_text(w, "work-title"))
        })
        .map(str::to_string);
    let names: Vec<(String, String)> = root
        .descendants()
        .filter(|n| n.has_tag_name("score-part"))
        .map(|n| {
            let id = n.attribute("id").unwrap_or_default().to_string();
            let name = child_text(n, "part-name").unwrap_or_default().to_string();
            (id, name)
        })
        .collect();
    for part in root.children().filter(|n| n.has_tag_name("part")) {
        let id = part.attribute("id").unwrap_or_default();
        let name = names
            .iter()
            .find(|(i, _)| i == id)
            .map_or("", |(_, n)| n.as_str());
        score.parts.push(read_part(part, id, name)?);
    }
    Ok(score)
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(tag))
}

fn child_text<'a>(n: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
    child(n, tag).and_then(|c| c.text()).map(str::trim)
}

fn number<T: std::str::FromStr>(n: roxmltree::Node, tag: &str) -> Option<T> {
    child_text(n, tag).and_then(|t| t.parse().ok())
}

/// Events collected for one voice while walking a measure.
struct VoiceBuilder {
    number: u8,
    staff: u8,
    end: Fraction,
    events: Vec<Event>,
}

fn read_part(part: roxmltree::Node, id: &str, name: &str) -> Result<Part, ReadError> {
    let mut out = Part::new(id, name, 1);
    let mut divisions: u64 = 1;
    let mut measure_length = Fraction::new(1, 1);
    for m in part.children().filter(|n| n.has_tag_name("measure")) {
        let label = m.attribute("number").unwrap_or("?").to_string();
        let err = |message: &str| ReadError::Content {
            measure: label.clone(),
            message: message.into(),
        };
        let mut measure = Measure::new(label.parse().unwrap_or(out.measures.len() as u32 + 1));
        measure.implicit = m.attribute("implicit") == Some("yes");
        let mut voices: Vec<VoiceBuilder> = Vec::new();
        let mut t = Fraction::ZERO;
        // Onset of the last non-chord note, for `<chord/>` members.
        let mut last_onset = Fraction::ZERO;
        let to_fraction = |d: u64, div: u64| Fraction::new(d, 4 * div);
        for el in m.children().filter(|n| n.is_element()) {
            match el.tag_name().name() {
                "attributes" => {
                    if let Some(d) = number::<u64>(el, "divisions").filter(|&d| d > 0) {
                        divisions = d;
                    }
                    if let Some(k) = child(el, "key")
                        && let Some(fifths) = number::<i8>(k, "fifths")
                    {
                        measure.attributes.key = Some(Key { fifths, mode: None });
                    }
                    if let Some(ts) = child(el, "time")
                        && let (Some(b), Some(bt)) =
                            (number::<u8>(ts, "beats"), number::<u8>(ts, "beat-type"))
                    {
                        let time = TimeSignature::new(b, bt);
                        measure_length = time.measure_duration();
                        measure.attributes.time = Some(time);
                    }
                    if let Some(s) = number::<u8>(el, "staves") {
                        out.staves = s.max(1);
                    }
                    for c in el.children().filter(|n| n.has_tag_name("clef")) {
                        if let Some(clef) = read_clef(c) {
                            let staff = c
                                .attribute("number")
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(1);
                            measure.attributes.clefs.push(StaffClef { staff, clef });
                        }
                    }
                }
                "backup" => {
                    let d = to_fraction(number(el, "duration").unwrap_or(0), divisions);
                    t = if d > t { Fraction::ZERO } else { t - d };
                }
                "forward" => {
                    t += to_fraction(number(el, "duration").unwrap_or(0), divisions);
                }
                "note" => {
                    let grace = child(el, "grace").map(|g| Grace {
                        slash: g.attribute("slash") == Some("yes"),
                    });
                    let in_chord = child(el, "chord").is_some();
                    let duration = if grace.is_some() {
                        Fraction::ZERO
                    } else {
                        to_fraction(
                            number(el, "duration").ok_or_else(|| err("note without duration"))?,
                            divisions,
                        )
                    };
                    let staff: u8 = number(el, "staff").unwrap_or(1);
                    let voice: u8 = number(el, "voice").unwrap_or(1);
                    let onset = if in_chord { last_onset } else { t };
                    let vb = match voices
                        .iter()
                        .position(|v| v.number == voice && v.staff == staff)
                    {
                        Some(i) => &mut voices[i],
                        None => {
                            voices.push(VoiceBuilder {
                                number: voice,
                                staff,
                                end: Fraction::ZERO,
                                events: Vec::new(),
                            });
                            voices.last_mut().unwrap()
                        }
                    };
                    if in_chord {
                        let note = read_pitch(el).ok_or_else(|| err("chord note without pitch"))?;
                        match vb.events.last_mut().map(|e| &mut e.content) {
                            Some(EventContent::Chord(notes)) => notes.push(note),
                            _ => return Err(err("<chord/> without a preceding note")),
                        }
                        continue;
                    }
                    if onset > vb.end {
                        // Hidden time in this voice: count it as silence.
                        vb.events.push(gap_rest(onset - vb.end));
                    }
                    let mut event = if let Some(r) = child(el, "rest") {
                        if r.attribute("measure") == Some("yes") || child(el, "type").is_none() {
                            Event::measure_rest(duration)
                        } else {
                            let mut e = Event::rest(
                                note_type(el).ok_or_else(|| err("bad note type"))?,
                                dots(el),
                            );
                            e.duration = duration;
                            e
                        }
                    } else {
                        let note = read_pitch(el).ok_or_else(|| err("note without pitch"))?;
                        let nt = note_type(el).unwrap_or(NoteType::Quarter);
                        let mut e = Event::note(&[note.pitch], nt, dots(el));
                        if let EventContent::Chord(n) = &mut e.content {
                            n[0] = note;
                        }
                        e.duration = duration;
                        e
                    };
                    event.grace = grace;
                    if let Some(tm) = child(el, "time-modification")
                        && let (Some(actual), Some(normal)) = (
                            number::<u8>(tm, "actual-notes"),
                            number::<u8>(tm, "normal-notes"),
                        )
                    {
                        event.time_modification = Some(TimeModification { actual, normal });
                    }
                    vb.events.push(event);
                    vb.end = onset + duration;
                    last_onset = onset;
                    t = onset + duration;
                }
                _ => {}
            }
        }
        if voices.is_empty() {
            // An empty measure (Dorico writes these as a bare `<forward>`).
            voices.push(VoiceBuilder {
                number: 1,
                staff: 1,
                end: measure_length,
                events: vec![Event::measure_rest(measure_length)],
            });
        }
        voices.sort_by_key(|v| (v.staff, v.number));
        measure.voices = voices
            .into_iter()
            .map(|v| Voice::new(v.number, v.staff, v.events))
            .collect();
        out.measures.push(measure);
    }
    Ok(out)
}

fn gap_rest(duration: Fraction) -> Event {
    let mut e = Event::measure_rest(duration);
    e.content = EventContent::Rest {
        whole_measure: false,
    };
    e
}

fn read_pitch(note: roxmltree::Node) -> Option<ChordNote> {
    let p = child(note, "pitch")?;
    let step = match child_text(p, "step")? {
        "C" => Step::C,
        "D" => Step::D,
        "E" => Step::E,
        "F" => Step::F,
        "G" => Step::G,
        "A" => Step::A,
        "B" => Step::B,
        _ => return None,
    };
    // Microtonal alters are rounded to the nearest semitone.
    let alter = child_text(p, "alter")
        .and_then(|a| a.parse::<f32>().ok())
        .map_or(0, |a| a.round() as i8);
    let mut n = ChordNote::new(Pitch::new(step, alter, number(p, "octave")?));
    for tie in note.children().filter(|c| c.has_tag_name("tie")) {
        match tie.attribute("type") {
            Some("start") => n.tie_start = true,
            Some("stop") => n.tie_stop = true,
            _ => {}
        }
    }
    Some(n)
}

fn note_type(note: roxmltree::Node) -> Option<NoteType> {
    Some(match child_text(note, "type")? {
        "breve" => NoteType::Breve,
        "whole" => NoteType::Whole,
        "half" => NoteType::Half,
        "quarter" => NoteType::Quarter,
        "eighth" => NoteType::Eighth,
        "16th" => NoteType::Sixteenth,
        "32nd" => NoteType::ThirtySecond,
        "64th" => NoteType::SixtyFourth,
        "128th" => NoteType::OneHundredTwentyEighth,
        _ => return None,
    })
}

fn dots(note: roxmltree::Node) -> u8 {
    note.children().filter(|c| c.has_tag_name("dot")).count() as u8
}

fn read_clef(c: roxmltree::Node) -> Option<Clef> {
    let sign = match child_text(c, "sign")? {
        "G" => ClefSign::G,
        "F" => ClefSign::F,
        "C" => ClefSign::C,
        "percussion" => ClefSign::Percussion,
        "TAB" => ClefSign::Tab,
        _ => return None,
    };
    Some(Clef {
        sign,
        line: number(c, "line").unwrap_or(2),
        octave_change: number(c, "clef-octave-change").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::to_musicxml_string;
    use scorelift_model::{compare::compare, fixtures};

    #[test]
    fn fixtures_survive_a_round_trip() {
        for name in fixtures::NAMES {
            let score = fixtures::by_name(name).unwrap();
            let back = read_str(&to_musicxml_string(&score).unwrap()).unwrap();
            let c = compare(&score, &back);
            assert_eq!(c.f1(), 1.0, "{name}: {c}");
        }
    }
}
