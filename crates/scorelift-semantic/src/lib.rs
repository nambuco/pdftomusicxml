//! From symbols to music: turns what was found on the page into a [`Score`].
//!
//! Milestone M2 implements the vector fast path: PDFs exported from
//! notation software, where the music font exposes SMuFL code points and
//! staff lines, stems and beams are plain vector paths. No recognition is
//! needed, only interpretation.

mod interpret;
pub mod layout;
pub mod smufl;

use scorelift_model::*;
use scorelift_pdf::PdfDocument;

use interpret::{StaffMeasure, StaffState, bar_style, endings, measure_ranges, read_staff};
use layout::{PageText, System, Word};

pub use layout::has_music_font;

#[derive(Debug)]
pub struct VectorReading {
    pub score: Score,
    pub warnings: Vec<String>,
}

/// Reads a PDF exported from notation software. Returns `None` if no page
/// uses a SMuFL music font (scans and image-only PDFs).
pub fn read_vector(doc: &PdfDocument) -> Option<VectorReading> {
    let mut systems: Vec<System> = Vec::new();
    let mut texts: Vec<PageText> = Vec::new();
    for page in doc.pages.iter().filter(|p| has_music_font(p)) {
        let (s, t) = layout::layout(page);
        systems.extend(s);
        texts.push(t);
    }
    if systems.is_empty() {
        return None;
    }
    Some(read_systems(systems, texts))
}

/// Turns systems (from vector pages, or recognised from images) into a score.
/// `texts` holds the page text outside the systems, for titles and credits.
pub fn read_systems(systems: Vec<System>, texts: Vec<PageText>) -> VectorReading {
    let mut warnings = Vec::new();

    // Parts come from the first system: braced staves form one part.
    let first = &systems[0];
    let mut parts_staves: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < first.staves.len() {
        match first.braces.iter().find(|b| b.first() == Some(&i)) {
            Some(b) => {
                parts_staves.push(b.clone());
                i = b.last().unwrap() + 1;
            }
            None => {
                parts_staves.push(vec![i]);
                i += 1;
            }
        }
    }
    let staff_count = first.staves.len();
    let mut parts: Vec<Part> = parts_staves
        .iter()
        .enumerate()
        .map(|(pi, staves)| {
            let mut p = Part::new(
                format!("P{}", pi + 1),
                format!("Part {}", pi + 1),
                staves.len() as u8,
            );
            if let Some(name) = label_for(first, staves) {
                p.name = name;
            }
            if let Some(abbr) = systems.get(1).and_then(|s| label_for(s, staves)) {
                p.abbreviation = Some(abbr);
            }
            p
        })
        .collect();

    let mut states: Vec<StaffState> = vec![StaffState::default(); staff_count];
    let mut last_page = systems[0].page;
    for (si, sys) in systems.iter().enumerate() {
        if sys.staves.len() != staff_count {
            warnings.push(format!(
                "system {} has {} staves, expected {staff_count}; skipped",
                si + 1,
                sys.staves.len()
            ));
            continue;
        }
        let ranges = measure_ranges(sys);
        let xs: Vec<(f32, f32)> = ranges.iter().map(|r| (r.0, r.1)).collect();
        let readings: Vec<Vec<StaffMeasure>> = sys
            .staves
            .iter()
            .zip(states.iter_mut())
            .map(|(staff, state)| read_staff(staff, &xs, state).measures)
            .collect();

        // Ranges holding only a clef, key or time signature (before a
        // start-repeat bar) fold into the following measure.
        let header_only: Vec<bool> = (0..ranges.len())
            .map(|j| readings.iter().all(|r| r[j].voices.is_empty()))
            .collect();

        let sp = sys.staves[0].sp;
        let top_endings = endings(&sys.staves[0]);
        let new_page = sys.page != last_page;
        last_page = sys.page;
        let mut first_in_system = true;
        let mut carried: Vec<StaffMeasure> = Vec::new();

        for (j, &(x0, x1, left_bar, right_bar)) in ranges.iter().enumerate() {
            if header_only[j] {
                carried = readings.iter().map(|r| r[j].clone()).collect();
                continue;
            }
            for (pi, staves) in parts_staves.iter().enumerate() {
                let mut m = Measure::new(0);
                m.new_system = first_in_system && si > 0;
                m.new_page = first_in_system && new_page;
                for (k, &s) in staves.iter().enumerate() {
                    let mut sm = readings[s][j].clone();
                    if let Some(c) = carried.get(s) {
                        sm.clef = sm.clef.or(c.clef);
                        sm.key = sm.key.or(c.key);
                        sm.time = sm.time.or(c.time);
                    }
                    let staff_no = k as u8 + 1;
                    if k == 0 {
                        m.attributes.key = sm.key.map(|f| Key {
                            fifths: f,
                            mode: None,
                        });
                        m.attributes.time = sm.time;
                        m.attributes.multiple_rest = sm.multirest;
                    }
                    if let Some(c) = sm.clef {
                        m.attributes.clefs.push(StaffClef {
                            staff: staff_no,
                            clef: c,
                        });
                    }
                    for (vi, events) in sm.voices.into_iter().enumerate() {
                        m.voices
                            .push(Voice::new(k as u8 * 4 + vi as u8 + 1, staff_no, events));
                    }
                    for d in sm.directions {
                        m.directions.push(d.on_staff(staff_no));
                    }
                }

                if let Some(b) = right_bar.map(|i| &sys.bars[i]) {
                    let style = bar_style(b, sp);
                    if b.repeat_left {
                        m.right_barline = Some(Barline {
                            style,
                            repeat: Some(RepeatDirection::Backward),
                            ending: None,
                        });
                    } else if style != BarStyle::Regular && !b.repeat_right {
                        m.right_barline = Some(Barline::new(style));
                    }
                }
                if let Some(b) = left_bar.map(|i| &sys.bars[i])
                    && b.repeat_right
                {
                    m.left_barline = Some(Barline {
                        style: BarStyle::HeavyLight,
                        repeat: Some(RepeatDirection::Forward),
                        ending: None,
                    });
                }
                for (ex0, ex1, label, closed) in &top_endings {
                    let starts = *ex0 >= x0 - 0.5 * sp && *ex0 < x1;
                    let ends = *ex1 > x0 && *ex1 <= x1 + 0.5 * sp;
                    if starts {
                        m.left_barline
                            .get_or_insert(Barline::new(BarStyle::Regular))
                            .ending = Some(Ending {
                            number: label.clone(),
                            kind: EndingKind::Start,
                        });
                    }
                    if ends {
                        let kind = if *closed {
                            EndingKind::Stop
                        } else {
                            EndingKind::Discontinue
                        };
                        m.right_barline
                            .get_or_insert(Barline::new(BarStyle::Regular))
                            .ending = Some(Ending {
                            number: label.clone(),
                            kind,
                        });
                    }
                }
                parts[pi].measures.push(m);
            }
            carried.clear();
            first_in_system = false;
        }
    }

    expand_multirests(&mut parts);
    number_measures(&mut parts);
    let metadata = metadata(&texts, &systems[0]);
    VectorReading {
        score: Score { metadata, parts },
        warnings,
    }
}

/// A multi-measure rest is written once on the page but must appear as
/// that many measures in MusicXML.
fn expand_multirests(parts: &mut [Part]) {
    for part in parts {
        let mut out = Vec::new();
        let mut len = Fraction::new(1, 1);
        for m in part.measures.drain(..) {
            if let Some(t) = m.attributes.time {
                len = t.measure_duration();
            }
            let n = m.attributes.multiple_rest.unwrap_or(1);
            let staves: Vec<u8> = m.voices.iter().map(|v| v.staff).collect();
            out.push(m);
            for _ in 1..n {
                let mut extra = Measure::new(0);
                extra.voices = staves
                    .iter()
                    .map(|&s| Voice::new((s - 1) * 4 + 1, s, vec![Event::measure_rest(len)]))
                    .collect();
                out.push(extra);
            }
        }
        part.measures = out;
    }
}

/// Numbers measures from 1, or from 0 when the first one is a pickup.
fn number_measures(parts: &mut [Part]) {
    let pickup = parts
        .first()
        .and_then(|p| p.measures.first())
        .is_some_and(|m| {
            let Some(t) = m.attributes.time else {
                return false;
            };
            let full = t.measure_duration();
            let longest = m
                .voices
                .iter()
                .map(|v| v.duration())
                .max()
                .unwrap_or(Fraction::ZERO);
            !longest.is_zero() && longest < full
        });
    for part in parts {
        for (i, m) in part.measures.iter_mut().enumerate() {
            m.number = if pickup { i as u32 } else { i as u32 + 1 };
            m.implicit = pickup && i == 0;
        }
    }
}

/// The text left of a part's staves in a system: its name or abbreviation.
fn label_for(sys: &System, staves: &[usize]) -> Option<String> {
    let top = sys.staves[*staves.first()?].top();
    let bottom = sys.staves[*staves.last()?].bottom();
    let sp = sys.staves[0].sp;
    let mut words: Vec<&Word> = sys
        .labels
        .iter()
        .filter(|w| w.y > top - sp && w.y < bottom + 2.0 * sp)
        .collect();
    words.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

/// Words on the same baseline joined into lines: (text, x0, x1, y, size).
fn lines(words: &[Word]) -> Vec<(String, f32, f32, f32, f32)> {
    let mut sorted: Vec<&Word> = words.iter().collect();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<(String, f32, f32, f32, f32)> = Vec::new();
    for w in sorted {
        if let Some(l) = out.last_mut()
            && (l.3 - w.y).abs() < 0.2 * w.size
            && (l.4 - w.size).abs() < 0.1 * w.size
            && w.x0 - l.2 < 2.0 * w.size
        {
            l.0.push(' ');
            l.0.push_str(&w.text);
            l.2 = w.x1;
            continue;
        }
        out.push((w.text.clone(), w.x0, w.x1, w.y, w.size));
    }
    out
}

fn metadata(texts: &[PageText], first_system: &System) -> Metadata {
    let mut meta = Metadata::default();
    let Some(page) = texts.iter().find(|t| t.page == first_system.page) else {
        return meta;
    };
    let top = first_system.staves[0].top() - 3.0 * first_system.staves[0].sp;
    let all = lines(&page.words);
    let header: Vec<&(String, f32, f32, f32, f32)> = all.iter().filter(|l| l.3 < top).collect();
    if let Some(title) = header.iter().max_by(|a, b| a.4.total_cmp(&b.4)) {
        meta.title = Some(title.0.clone());
        let (tsize, ty) = (title.4, title.3);
        for l in &header {
            if l.0 == title.0 && l.3 == ty {
                continue;
            }
            let cx = (l.1 + l.2) / 2.0;
            if l.2 > 0.7 * page.width && l.1 > 0.5 * page.width {
                meta.composer.get_or_insert(l.0.clone());
            } else if l.2 < 0.45 * page.width {
                meta.lyricist.get_or_insert(l.0.clone());
            } else if (cx - page.width / 2.0).abs() < 0.1 * page.width && l.4 < tsize && l.3 > ty {
                meta.subtitle.get_or_insert(l.0.clone());
            }
        }
    }
    // A centred line at the very bottom of the first page is usually the copyright.
    if let Some(footer) = all
        .iter()
        .filter(|l| {
            l.3 > 0.9 * page.height
                && ((l.1 + l.2) / 2.0 - page.width / 2.0).abs() < 0.15 * page.width
        })
        .max_by(|a, b| a.3.total_cmp(&b.3))
    {
        meta.rights = Some(footer.0.clone());
    }
    meta
}
