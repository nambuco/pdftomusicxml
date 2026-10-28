//! Image recognition on the built-in fixtures, engraved by MuseScore 4 as
//! 280 dpi PNGs in Bravura and in Leland (see scripts/make-vector-fixtures.sh).
//!
//! The floors are the accuracy reached so far, so tuning
//! cannot silently make things worse. Raise them as recognition improves.

use std::path::PathBuf;

use scorelift_model::compare::compare;
use scorelift_model::fixtures;

fn f1(font: &str, name: &str) -> f64 {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../scorelift-detect/tests/fixtures")
        .join(font)
        .join(format!("{name}.png"));
    let conversion = scorelift_core::convert(&path, &Default::default()).expect("converts");
    let c = compare(&fixtures::by_name(name).unwrap(), &conversion.score);
    println!("{font} {name}: {c}");
    c.f1()
}

#[test]
fn c_major_scale_is_read_exactly() {
    assert_eq!(f1("musescore-bravura", "c-major-scale"), 1.0);
    assert_eq!(f1("musescore", "c-major-scale"), 1.0);
}

#[test]
fn piano_duet() {
    assert!(f1("musescore-bravura", "piano-duet") >= 0.9);
    assert!(f1("musescore", "piano-duet") >= 0.9);
}

#[test]
fn feature_tour() {
    // Chords, ties, a triplet, a grace note and a multi-measure rest.
    assert_eq!(f1("musescore-bravura", "feature-tour"), 1.0);
    assert!(f1("musescore", "feature-tour") >= 0.95);
}
