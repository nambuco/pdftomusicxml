//! Note-level comparison of two scores, for accuracy measurement.
//!
//! Each note and rest becomes a key (part, measure position, staff, onset,
//! pitch, duration); keys are matched as multisets, so voice numbering and
//! event order do not matter.

use std::collections::HashMap;
use std::fmt;

use crate::{EventContent, Fraction, Pitch, Score};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    part: usize,
    measure: usize,
    staff: u8,
    onset: Fraction,
    /// `None` for rests.
    pitch: Option<Pitch>,
    duration: Fraction,
    grace: bool,
}

impl Key {
    fn what(&self) -> String {
        match self.pitch {
            Some(p) => format!(
                "{:?}{}{}",
                p.step,
                ["bb", "b", "", "#", "x"][(p.alter + 2).clamp(0, 4) as usize],
                p.octave
            ),
            None => "rest".into(),
        }
    }

    /// `onset pitch duration`, without the part and measure.
    fn short(&self) -> String {
        format!(
            "{} {} {}{}",
            self.onset,
            self.what(),
            self.duration,
            if self.grace { " grace" } else { "" }
        )
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = self.what();
        write!(
            f,
            "part {} measure {} staff {} at {}: {} for {}{}",
            self.part + 1,
            self.measure + 1,
            self.staff,
            self.onset,
            what,
            self.duration,
            if self.grace { " (grace)" } else { "" }
        )
    }
}

fn keys(score: &Score) -> HashMap<Key, usize> {
    let mut out = HashMap::new();
    for (pi, part) in score.parts.iter().enumerate() {
        for (mi, m) in part.measures.iter().enumerate() {
            for v in &m.voices {
                let mut t = Fraction::ZERO;
                for e in &v.events {
                    let pitches: Vec<Option<Pitch>> = match &e.content {
                        EventContent::Rest { .. } => vec![None],
                        EventContent::Chord(notes) => notes.iter().map(|n| Some(n.pitch)).collect(),
                    };
                    for pitch in pitches {
                        let key = Key {
                            part: pi,
                            measure: mi,
                            staff: v.staff,
                            onset: t,
                            pitch,
                            duration: e.duration,
                            grace: e.is_grace(),
                        };
                        *out.entry(key).or_insert(0) += 1;
                    }
                    t += e.duration;
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct Comparison {
    pub expected: usize,
    pub found: usize,
    pub matched: usize,
    pub expected_measures: usize,
    pub found_measures: usize,
    /// A few unmatched items from each side, for debugging.
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

impl Comparison {
    pub fn precision(&self) -> f64 {
        if self.found == 0 {
            0.0
        } else {
            self.matched as f64 / self.found as f64
        }
    }

    pub fn recall(&self) -> f64 {
        if self.expected == 0 {
            0.0
        } else {
            self.matched as f64 / self.expected as f64
        }
    }

    pub fn f1(&self) -> f64 {
        let (p, r) = (self.precision(), self.recall());
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "notes+rests: {} matched of {} expected, {} found (precision {:.1}%, recall {:.1}%, F1 {:.1}%)",
            self.matched,
            self.expected,
            self.found,
            100.0 * self.precision(),
            100.0 * self.recall(),
            100.0 * self.f1()
        )?;
        writeln!(
            f,
            "measures: {} expected, {} found",
            self.expected_measures, self.found_measures
        )?;
        for m in &self.missing {
            writeln!(f, "  missing {m}")?;
        }
        for e in &self.extra {
            writeln!(f, "  extra   {e}")?;
        }
        Ok(())
    }
}

pub fn compare(expected: &Score, found: &Score) -> Comparison {
    let a = keys(expected);
    let b = keys(found);
    let mut c = Comparison {
        expected: a.values().sum(),
        found: b.values().sum(),
        expected_measures: expected.parts.first().map_or(0, |p| p.measures.len()),
        found_measures: found.parts.first().map_or(0, |p| p.measures.len()),
        ..Default::default()
    };
    let mut missing: Vec<String> = Vec::new();
    for (k, &n) in &a {
        let m = b.get(k).copied().unwrap_or(0);
        c.matched += n.min(m);
        if n > m {
            missing.push(k.to_string());
        }
    }
    let mut extra: Vec<String> = b
        .iter()
        .filter(|(k, n)| a.get(k).copied().unwrap_or(0) < **n)
        .map(|(k, _)| k.to_string())
        .collect();
    missing.sort();
    extra.sort();
    c.missing = missing.into_iter().take(12).collect();
    c.extra = extra.into_iter().take(12).collect();
    c
}

/// Rewrites every voice as it sounds, so that notations of the same music
/// compare equal: notes tied within a measure become one longer note, and
/// consecutive rests become one rest. A half note and two tied quarters, or a
/// half rest and two quarter rests, then give the same key.
pub fn sounding(score: &Score) -> Score {
    let mut out = score.clone();
    for m in out.parts.iter_mut().flat_map(|p| p.measures.iter_mut()) {
        for v in &mut m.voices {
            let mut merged: Vec<crate::Event> = Vec::with_capacity(v.events.len());
            for e in v.events.drain(..) {
                if let Some(prev) = merged.last_mut()
                    && !prev.is_grace()
                    && !e.is_grace()
                    && continues(prev, &e)
                {
                    prev.duration += e.duration;
                    if let (EventContent::Chord(a), EventContent::Chord(b)) =
                        (&mut prev.content, &e.content)
                    {
                        for (x, y) in a.iter_mut().zip(b) {
                            x.tie_start = y.tie_start;
                        }
                    }
                    continue;
                }
                merged.push(e);
            }
            v.events = merged;
        }
    }
    out
}

/// Whether `next` only prolongs `prev`: a rest after a rest, or the same
/// pitches with every note tied over.
fn continues(prev: &crate::Event, next: &crate::Event) -> bool {
    match (&prev.content, &next.content) {
        (EventContent::Rest { .. }, EventContent::Rest { .. }) => true,
        (EventContent::Chord(a), EventContent::Chord(b)) => {
            a.len() == b.len()
                && a.iter().all(|n| n.tie_start)
                && a.iter().zip(b).all(|(x, y)| x.pitch == y.pitch)
        }
        _ => false,
    }
}

/// One measure of a part, compared event by event.
#[derive(Debug, Clone, Default)]
pub struct MeasureDiff {
    /// 0-based measure index.
    pub index: usize,
    /// `onset pitch duration` of every note and rest, in time order.
    pub expected: Vec<String>,
    pub found: Vec<String>,
    /// Expected events that were not found, and found events not expected.
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

impl MeasureDiff {
    pub fn is_exact(&self) -> bool {
        self.missing.is_empty() && self.extra.is_empty()
    }
}

/// Compares part `part` of both scores measure by measure, over the measures
/// of `expected`.
pub fn measure_diffs(expected: &Score, found: &Score, part: usize) -> Vec<MeasureDiff> {
    let (a, b) = (keys(expected), keys(found));
    let in_measure = |map: &HashMap<Key, usize>, m: usize| {
        let mut v: Vec<(Key, usize)> = map
            .iter()
            .filter(|(k, _)| k.part == part && k.measure == m)
            .map(|(k, &n)| (k.clone(), n))
            .collect();
        v.sort_by_key(|x| (x.0.onset, x.0.short()));
        v
    };
    let count = expected.parts.get(part).map_or(0, |p| p.measures.len());
    (0..count)
        .map(|m| {
            let (ea, fb) = (in_measure(&a, m), in_measure(&b, m));
            let list = |v: &[(Key, usize)]| -> Vec<String> {
                v.iter()
                    .flat_map(|(k, n)| std::iter::repeat_n(k.short(), *n))
                    .collect()
            };
            let surplus = |x: &[(Key, usize)], y: &[(Key, usize)]| -> Vec<String> {
                x.iter()
                    .flat_map(|(k, n)| {
                        let other = y.iter().find(|(j, _)| j == k).map_or(0, |(_, m)| *m);
                        std::iter::repeat_n(k.short(), n.saturating_sub(other))
                    })
                    .collect()
            };
            MeasureDiff {
                index: m,
                expected: list(&ea),
                found: list(&fb),
                missing: surplus(&ea, &fb),
                extra: surplus(&fb, &ea),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn identical_scores_match_fully() {
        let s = fixtures::feature_tour();
        let c = compare(&s, &s);
        assert_eq!(c.matched, c.expected);
        assert_eq!(c.f1(), 1.0);
    }

    #[test]
    fn a_wrong_pitch_is_one_miss_and_one_extra() {
        let a = fixtures::c_major_scale();
        let mut b = a.clone();
        if let EventContent::Chord(n) = &mut b.parts[0].measures[0].voices[0].events[0].content {
            n[0].pitch.octave += 1;
        }
        let c = compare(&a, &b);
        assert_eq!(c.expected - c.matched, 1);
        assert_eq!(c.missing.len(), 1);
        assert_eq!(c.extra.len(), 1);
    }

    #[test]
    fn tied_notes_and_split_rests_sound_like_one() {
        use crate::{Event, NoteType, Pitch};
        let a4 = Pitch::parse("A4").unwrap();
        let mut a = fixtures::c_major_scale();
        let mut b = a.clone();
        a.parts[0].measures[0].voices[0].events = vec![
            Event::note(&[a4], NoteType::Half, 0),
            Event::rest(NoteType::Half, 0),
        ];
        b.parts[0].measures[0].voices[0].events = vec![
            Event::note(&[a4], NoteType::Quarter, 0).tie_start(),
            Event::note(&[a4], NoteType::Quarter, 0).tie_stop(),
            Event::rest(NoteType::Quarter, 0),
            Event::rest(NoteType::Quarter, 0),
        ];
        assert!(compare(&a, &b).f1() < 1.0);
        assert_eq!(compare(&sounding(&a), &sounding(&b)).f1(), 1.0);
    }
}
