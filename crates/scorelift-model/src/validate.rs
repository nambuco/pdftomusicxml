//! Structural and rhythmic consistency checks.
//!
//! The recogniser uses these to find measures it probably misread; the
//! writer does not require a score to pass them.

use std::fmt;

use crate::{EventContent, Fraction, Score};

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub part: String,
    pub measure: u32,
    pub kind: IssueKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueKind {
    /// No time signature has been set by this measure.
    NoTimeSignature,
    /// A voice is longer than the measure, or the first voice on a staff is shorter.
    VoiceDuration {
        voice: u8,
        expected: Fraction,
        actual: Fraction,
    },
    /// A voice, direction or clef refers to a staff the part does not have.
    StaffOutOfRange { staff: u8 },
    /// A chord event with no notes.
    EmptyChord { voice: u8 },
    /// This part has a different number of measures from the first part.
    MeasureCount { expected: usize, actual: usize },
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "part {} measure {}: ", self.part, self.measure)?;
        match &self.kind {
            IssueKind::NoTimeSignature => write!(f, "no time signature"),
            IssueKind::VoiceDuration {
                voice,
                expected,
                actual,
            } => write!(
                f,
                "voice {voice} lasts {actual} whole notes, expected {expected}"
            ),
            IssueKind::StaffOutOfRange { staff } => write!(f, "staff {staff} out of range"),
            IssueKind::EmptyChord { voice } => write!(f, "empty chord in voice {voice}"),
            IssueKind::MeasureCount { expected, actual } => {
                write!(f, "{actual} measures, expected {expected}")
            }
        }
    }
}

/// Returns every issue found; an empty list means the score is consistent.
pub fn validate(score: &Score) -> Vec<Issue> {
    let mut issues = Vec::new();
    let first_count = score.parts.first().map(|p| p.measures.len());

    for part in &score.parts {
        let mut issue = |measure: u32, kind: IssueKind| {
            issues.push(Issue {
                part: part.id.clone(),
                measure,
                kind,
            })
        };
        if let Some(expected) = first_count
            && part.measures.len() != expected
        {
            issue(
                part.measures.last().map_or(0, |m| m.number),
                IssueKind::MeasureCount {
                    expected,
                    actual: part.measures.len(),
                },
            );
        }

        let staff_ok = |staff: u8| (1..=part.staves).contains(&staff);
        let mut time = None;
        for m in &part.measures {
            if let Some(t) = m.attributes.time {
                time = Some(t);
            }
            for c in &m.attributes.clefs {
                if !staff_ok(c.staff) {
                    issue(m.number, IssueKind::StaffOutOfRange { staff: c.staff });
                }
            }
            for d in &m.directions {
                if !staff_ok(d.staff) {
                    issue(m.number, IssueKind::StaffOutOfRange { staff: d.staff });
                }
            }

            let Some(time) = time else {
                issue(m.number, IssueKind::NoTimeSignature);
                continue;
            };
            let expected = time.measure_duration();
            let mut staves_seen = Vec::new();
            for v in &m.voices {
                if !staff_ok(v.staff) {
                    issue(m.number, IssueKind::StaffOutOfRange { staff: v.staff });
                }
                if v.events
                    .iter()
                    .any(|e| matches!(&e.content, EventContent::Chord(n) if n.is_empty()))
                {
                    issue(m.number, IssueKind::EmptyChord { voice: v.number });
                }
                // Secondary voices may legitimately cover only part of a measure.
                let first_on_staff = !staves_seen.contains(&v.staff);
                staves_seen.push(v.staff);
                let actual = v.duration();
                let too_long = actual > expected;
                let too_short = actual < expected && first_on_staff && !m.implicit;
                if too_long || too_short {
                    issue(
                        m.number,
                        IssueKind::VoiceDuration {
                            voice: v.number,
                            expected,
                            actual,
                        },
                    );
                }
            }
        }
    }
    issues
}
