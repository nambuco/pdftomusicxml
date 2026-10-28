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

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.pitch {
            Some(p) => format!(
                "{:?}{}{}",
                p.step,
                ["bb", "b", "", "#", "x"][(p.alter + 2).clamp(0, 4) as usize],
                p.octave
            ),
            None => "rest".into(),
        };
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
}
