//! The score intermediate representation shared by every scorelift stage.
//!
//! Durations and offsets are exact [`Fraction`]s of a whole note, so rhythm
//! checks never suffer from floating-point rounding.

pub mod compare;
pub mod fixtures;
mod fraction;
mod score;
pub mod validate;

pub use fraction::{Fraction, gcd, lcm};
pub use score::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_consistent() {
        for name in fixtures::NAMES {
            let score = fixtures::by_name(name).unwrap();
            let issues = validate::validate(&score);
            assert!(issues.is_empty(), "{name}: {issues:#?}");
        }
    }

    #[test]
    fn short_first_voice_is_reported() {
        let mut score = fixtures::c_major_scale();
        score.parts[0].measures[1].voices[0].events.pop();
        let issues = validate::validate(&score);
        assert_eq!(issues.len(), 1);
        assert!(matches!(
            issues[0].kind,
            validate::IssueKind::VoiceDuration { voice: 1, .. }
        ));
    }

    #[test]
    fn triplet_duration_is_exact() {
        let score = fixtures::feature_tour();
        let triplet = &score.parts[0].measures[2].voices[0].events[1];
        assert_eq!(triplet.duration, Fraction::new(1, 12));
    }
}
