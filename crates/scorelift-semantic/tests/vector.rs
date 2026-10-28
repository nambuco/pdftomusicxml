//! Round trip: built-in fixture → MusicXML → engraved PDF (committed in
//! tests/fixtures/<program>) → vector reader → compared with the fixture.
//! MuseScore PDFs are regenerated with `scripts/make-vector-fixtures.sh`;
//! Dorico has no command line, so its PDFs were exported by hand.

use std::path::PathBuf;

use scorelift_model::compare::compare;
use scorelift_model::*;

fn read(engraver: &str, name: &str) -> Score {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(engraver)
        .join(format!("{name}.pdf"));
    let doc = scorelift_pdf::load(&path).expect("fixture PDF loads");
    let reading = scorelift_semantic::read_vector(&doc).expect("music font found");
    assert!(reading.warnings.is_empty(), "{:?}", reading.warnings);
    reading.score
}

fn assert_notes_match(engraver: &str, name: &str) -> Score {
    let expected = fixtures::by_name(name).unwrap();
    let found = read(engraver, name);
    let c = compare(&expected, &found);
    assert_eq!(c.f1(), 1.0, "{name}:\n{c}");
    assert_eq!(c.expected_measures, c.found_measures, "{name}:\n{c}");
    assert!(
        validate::validate(&found).is_empty(),
        "{:?}",
        validate::validate(&found)
    );
    scorelift_musicxml::to_musicxml_string(&found).expect("writable");
    found
}

#[test]
fn c_major_scale() {
    let s = assert_notes_match("musescore", "c-major-scale");
    assert_eq!(s.metadata.title.as_deref(), Some("C Major Scale"));
    let m1 = &s.parts[0].measures[0];
    assert_eq!(m1.attributes.key.map(|k| k.fifths), Some(0));
    assert_eq!(m1.attributes.clefs[0].clef, Clef::TREBLE);
    assert_eq!(
        s.parts[0].measures[1]
            .right_barline
            .as_ref()
            .map(|b| b.style),
        Some(BarStyle::LightHeavy)
    );
}

#[test]
fn piano_duet() {
    let s = assert_notes_match("musescore", "piano-duet");
    assert_eq!(s.parts.len(), 2);
    assert_eq!(s.parts[0].name, "Voice");
    assert_eq!(s.parts[1].name, "Piano");
    assert_eq!(s.parts[1].staves, 2);
    let m1 = &s.parts[1].measures[0];
    let clefs: Vec<(u8, Clef)> = m1
        .attributes
        .clefs
        .iter()
        .map(|c| (c.staff, c.clef))
        .collect();
    assert_eq!(clefs, [(1, Clef::TREBLE), (2, Clef::BASS)]);
    // Two voices on the upper staff, one on the lower.
    let voices: Vec<(u8, u8)> = m1.voices.iter().map(|v| (v.number, v.staff)).collect();
    assert_eq!(voices, [(1, 1), (2, 1), (5, 2)]);
    let dynamic = &m1.directions[0];
    assert_eq!(
        (dynamic.staff, dynamic.placement),
        (1, Some(Placement::Below))
    );
}

#[test]
fn feature_tour() {
    let s = assert_notes_match("musescore", "feature-tour");
    let meta = &s.metadata;
    assert_eq!(meta.title.as_deref(), Some("Feature Tour"));
    assert_eq!(meta.composer.as_deref(), Some("The scorelift contributors"));
    assert_eq!(meta.rights.as_deref(), Some("CC0 1.0"));

    let part = &s.parts[0];
    assert_eq!(
        (part.name.as_str(), part.abbreviation.as_deref()),
        ("Organ", Some("Org."))
    );
    let m = &part.measures;
    assert!(m[0].implicit);
    assert_eq!(m[0].attributes.key.map(|k| k.fifths), Some(1));

    let kinds: Vec<&DirectionKind> = m[0].directions.iter().map(|d| &d.kind).collect();
    assert!(kinds.contains(&&DirectionKind::Rehearsal("Intro".into())));
    assert!(kinds.contains(&&DirectionKind::Dynamic(Dynamic::Mf)));
    assert!(kinds.iter().any(|k| matches!(
        k,
        DirectionKind::Tempo {
            beat_unit: NoteType::Quarter,
            per_minute: 98,
            ..
        }
    )));

    // Lyrics with a drawn hyphen: "so-la".
    let lyrics: Vec<(String, Syllabic)> = m[1].voices[0]
        .events
        .iter()
        .flat_map(|e| e.lyrics.iter().map(|l| (l.text.clone(), l.syllabic)))
        .collect();
    assert_eq!(lyrics[0], ("so".into(), Syllabic::Begin));
    assert_eq!(lyrics[1], ("la".into(), Syllabic::End));

    // Tie across the barline, slur, triplet, grace note, hairpin.
    let tied = |e: &Event, start: bool| match &e.content {
        EventContent::Chord(n) => n
            .iter()
            .all(|n| if start { n.tie_start } else { n.tie_stop }),
        _ => false,
    };
    assert!(tied(m[1].voices[0].events.last().unwrap(), true));
    assert!(tied(&m[2].voices[0].events[0], false));
    assert_eq!(m[1].voices[0].events[0].slurs[0].kind, StartStop::Start);
    assert_eq!(
        m[2].voices[0].events[1]
            .time_modification
            .map(|t| (t.actual, t.normal)),
        Some((3, 2))
    );
    assert!(m[3].voices[0].events[0].is_grace());
    assert!(
        m[3].directions
            .iter()
            .any(|d| d.kind == DirectionKind::Wedge(WedgeKind::Crescendo))
    );

    // Repeat with first and second endings.
    assert_eq!(
        m[4].left_barline.as_ref().and_then(|b| b.repeat),
        Some(RepeatDirection::Forward)
    );
    let end1 = m[5].right_barline.as_ref().unwrap();
    assert_eq!(end1.repeat, Some(RepeatDirection::Backward));
    assert_eq!(
        end1.ending.as_ref().map(|e| (e.number.as_str(), e.kind)),
        Some(("1", EndingKind::Stop))
    );
    let end2 = m[6]
        .left_barline
        .as_ref()
        .and_then(|b| b.ending.as_ref())
        .unwrap();
    assert_eq!((end2.number.as_str(), end2.kind), ("2", EndingKind::Start));

    // The 3-bar multi-measure rest expands to three measures.
    assert_eq!(m[7].attributes.multiple_rest, Some(3));
    assert!(m[7].new_system);
}

// Dorico draws noteheads with Bravura's optional "oversized" glyphs, typed
// lyric hyphens, filled hairpins, and lays out its own rehearsal marks.

#[test]
fn dorico_c_major_scale() {
    assert_notes_match("dorico", "c-major-scale");
}

#[test]
fn dorico_piano_duet() {
    let s = assert_notes_match("dorico", "piano-duet");
    assert_eq!(s.parts[1].staves, 2);
    let lyrics: Vec<Syllabic> = s.parts[0].measures[0].voices[0]
        .events
        .iter()
        .flat_map(|e| e.lyrics.iter().map(|l| l.syllabic))
        .collect();
    assert_eq!(lyrics, [Syllabic::Single, Syllabic::Begin, Syllabic::End]);
}

#[test]
fn dorico_feature_tour() {
    let s = assert_notes_match("dorico", "feature-tour");
    let m = &s.parts[0].measures;
    // Dorico prints its own rehearsal letter, and the dynamic sits below
    // the lyrics, closer to the midpoint between systems.
    let kinds: Vec<&DirectionKind> = m[0].directions.iter().map(|d| &d.kind).collect();
    assert!(kinds.contains(&&DirectionKind::Rehearsal("A".into())));
    assert!(kinds.contains(&&DirectionKind::Dynamic(Dynamic::Mf)));
    assert!(
        m[3].directions
            .iter()
            .any(|d| d.kind == DirectionKind::Wedge(WedgeKind::Crescendo))
    );
    let ending = |b: &Option<Barline>| {
        b.as_ref()
            .and_then(|b| b.ending.clone())
            .map(|e| (e.number, e.kind))
    };
    assert_eq!(
        ending(&m[5].left_barline),
        Some(("1".into(), EndingKind::Start))
    );
    assert_eq!(
        ending(&m[6].right_barline),
        Some(("2".into(), EndingKind::Discontinue))
    );
}
