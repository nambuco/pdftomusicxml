//! Small hand-built scores for tests and demos. All music here is original.

use crate::*;

fn p(name: &str) -> Pitch {
    Pitch::parse(name).unwrap_or_else(|| panic!("bad pitch {name}"))
}

fn ps(names: &[&str]) -> Vec<Pitch> {
    names.iter().map(|n| p(n)).collect()
}

/// Names of all fixtures, for the CLI `demo` command and the XSD check.
pub const NAMES: &[&str] = &["c-major-scale", "feature-tour", "piano-duet"];

pub fn by_name(name: &str) -> Option<Score> {
    match name {
        "c-major-scale" => Some(c_major_scale()),
        "feature-tour" => Some(feature_tour()),
        "piano-duet" => Some(piano_duet()),
        _ => None,
    }
}

/// Two measures of quarter notes. The smallest useful score.
pub fn c_major_scale() -> Score {
    let q = |n: &str| Event::note(&[p(n)], NoteType::Quarter, 0);
    let mut m1 = Measure::new(1);
    m1.attributes = Attributes {
        key: Some(Key {
            fifths: 0,
            mode: Some(Mode::Major),
        }),
        time: Some(TimeSignature::new(4, 4)),
        clefs: vec![StaffClef {
            staff: 1,
            clef: Clef::TREBLE,
        }],
        multiple_rest: None,
    };
    m1.voices = vec![Voice::new(1, 1, ["C4", "D4", "E4", "F4"].map(q).to_vec())];
    let mut m2 = Measure::new(2);
    m2.voices = vec![Voice::new(1, 1, ["G4", "A4", "B4", "C5"].map(q).to_vec())];
    m2.right_barline = Some(Barline::final_bar());

    let mut part = Part::new("P1", "Piano", 1);
    part.measures = vec![m1, m2];
    Score {
        metadata: Metadata {
            title: Some("C Major Scale".into()),
            ..Default::default()
        },
        parts: vec![part],
    }
}

/// One treble staff exercising most of what the IR can express: pickup,
/// beams, chords, ties, slurs, triplets, grace notes, articulations, lyrics,
/// dynamics, hairpins, tempo, repeats with endings, a multi-measure rest.
pub fn feature_tour() -> Score {
    use BeamValue::*;
    use StartStop::*;
    let whole = TimeSignature::new(4, 4).measure_duration();
    let mut measures = Vec::new();

    // Pickup.
    let mut m = Measure::new(0);
    m.implicit = true;
    m.attributes = Attributes {
        key: Some(Key {
            fifths: 1,
            mode: Some(Mode::Major),
        }),
        time: Some(TimeSignature::new(4, 4)),
        clefs: vec![StaffClef {
            staff: 1,
            clef: Clef::TREBLE,
        }],
        multiple_rest: None,
    };
    m.directions = vec![
        Direction::new(Fraction::ZERO, DirectionKind::Rehearsal("Intro".into())).above(),
        Direction::new(
            Fraction::ZERO,
            DirectionKind::Tempo {
                beat_unit: NoteType::Quarter,
                dots: 0,
                per_minute: 98,
                text: None,
            },
        )
        .above(),
        Direction::new(Fraction::ZERO, DirectionKind::Dynamic(Dynamic::Mf)).below(),
    ];
    m.voices = vec![Voice::new(
        1,
        1,
        vec![Event::note(&[p("D4")], NoteType::Quarter, 0).lyric(Syllabic::Single, "do")],
    )];
    measures.push(m);

    // Beamed, slurred eighths and a tied chord.
    let mut m = Measure::new(1);
    let e = |n: &str| Event::note(&[p(n)], NoteType::Eighth, 0);
    m.voices = vec![Voice::new(
        1,
        1,
        vec![
            e("G4")
                .beam(1, Begin)
                .slur(1, Start)
                .lyric(Syllabic::Begin, "so"),
            e("A4").beam(1, Continue).lyric(Syllabic::End, "la"),
            e("B4").beam(1, Continue).lyric(Syllabic::Single, "ti"),
            e("G4")
                .beam(1, End)
                .slur(1, Stop)
                .lyric(Syllabic::Single, "so"),
            Event::note(&ps(&["D4", "G4", "B4"]), NoteType::Half, 0)
                .articulation(Articulation::Accent)
                .tie_start()
                .lyric(Syllabic::Single, "mi"),
        ],
    )];
    measures.push(m);

    // Tie end, triplet, dotted rhythm, courtesy natural.
    let mut m = Measure::new(2);
    let t = |n: &str| e(n).time_mod(3, 2);
    m.voices = vec![Voice::new(
        1,
        1,
        vec![
            Event::note(&ps(&["D4", "G4", "B4"]), NoteType::Quarter, 0).tie_stop(),
            t("C5").beam(1, Begin).tuplet(Start),
            t("B4").beam(1, Continue),
            t("A4").beam(1, End).tuplet(Stop),
            Event::note(&[p("G4")], NoteType::Quarter, 1).stem(Stem::Up),
            e("F4").accidental(Accidental::Natural),
        ],
    )];
    measures.push(m);

    // Grace note, staccato, sixteenths, hairpin.
    let mut m = Measure::new(3);
    let s = |n: &str, v: BeamValue| {
        Event::note(&[p(n)], NoteType::Sixteenth, 0)
            .beam(1, v)
            .beam(2, v)
    };
    m.voices = vec![Voice::new(
        1,
        1,
        vec![
            Event::grace(p("A4"), NoteType::Eighth, true),
            Event::note(&[p("G4")], NoteType::Quarter, 0).articulation(Articulation::Staccato),
            s("G4", Begin),
            s("A4", Continue),
            s("B4", Continue),
            s("C5", End),
            Event::rest(NoteType::Half, 0),
        ],
    )];
    m.directions = vec![
        Direction::new(
            Fraction::new(1, 4),
            DirectionKind::Wedge(WedgeKind::Crescendo),
        )
        .below(),
        Direction::new(Fraction::new(1, 2), DirectionKind::Wedge(WedgeKind::Stop)).below(),
    ];
    measures.push(m);

    // Repeated section with first and second endings.
    let mut m = Measure::new(4);
    m.new_system = true;
    m.left_barline = Some(Barline {
        style: BarStyle::HeavyLight,
        repeat: Some(RepeatDirection::Forward),
        ending: None,
    });
    m.directions = vec![
        Direction::new(Fraction::ZERO, DirectionKind::Dynamic(Dynamic::F)).below(),
        Direction::new(Fraction::ZERO, DirectionKind::Words("let ring".into())).below(),
    ];
    m.voices = vec![Voice::new(
        1,
        1,
        vec![
            Event::note(&[p("D5")], NoteType::Whole, 0)
                .fermata()
                .let_ring(),
        ],
    )];
    measures.push(m);

    let mut m = Measure::new(5);
    m.left_barline = Some(Barline {
        style: BarStyle::Regular,
        repeat: None,
        ending: Some(Ending {
            number: "1".into(),
            kind: EndingKind::Start,
        }),
    });
    m.right_barline = Some(Barline {
        style: BarStyle::LightHeavy,
        repeat: Some(RepeatDirection::Backward),
        ending: Some(Ending {
            number: "1".into(),
            kind: EndingKind::Stop,
        }),
    });
    m.voices = vec![Voice::new(1, 1, vec![Event::measure_rest(whole)])];
    measures.push(m);

    let mut m = Measure::new(6);
    m.left_barline = Some(Barline {
        style: BarStyle::Regular,
        repeat: None,
        ending: Some(Ending {
            number: "2".into(),
            kind: EndingKind::Start,
        }),
    });
    m.right_barline = Some(Barline {
        style: BarStyle::Regular,
        repeat: None,
        ending: Some(Ending {
            number: "2".into(),
            kind: EndingKind::Discontinue,
        }),
    });
    m.voices = vec![Voice::new(
        1,
        1,
        vec![
            Event::note(&[p("G4")], NoteType::Half, 0),
            Event::rest(NoteType::Half, 0),
        ],
    )];
    measures.push(m);

    // Three-measure multi-rest, then a final note.
    for (i, number) in (7..=9).enumerate() {
        let mut m = Measure::new(number);
        if i == 0 {
            m.new_system = true;
            m.attributes.multiple_rest = Some(3);
        }
        m.voices = vec![Voice::new(1, 1, vec![Event::measure_rest(whole)])];
        measures.push(m);
    }
    let mut m = Measure::new(10);
    m.right_barline = Some(Barline::final_bar());
    m.voices = vec![Voice::new(
        1,
        1,
        vec![Event::note(&ps(&["G3", "D4", "G4"]), NoteType::Whole, 0)],
    )];
    measures.push(m);

    let mut part = Part::new("P1", "Organ", 1);
    part.abbreviation = Some("Org.".into());
    part.measures = measures;
    Score {
        metadata: Metadata {
            title: Some("Feature Tour".into()),
            subtitle: Some("scorelift test fixture".into()),
            composer: Some("The scorelift contributors".into()),
            rights: Some("CC0 1.0".into()),
            misc: vec![("tuning".into(), "E A D G B E".into())],
            ..Default::default()
        },
        parts: vec![part],
    }
}

/// Two parts: a voice and a two-staff piano with two voices on the upper
/// staff and one on the lower.
pub fn piano_duet() -> Score {
    let three_four = TimeSignature::new(3, 4);
    let key = Key {
        fifths: -1,
        mode: Some(Mode::Major),
    };

    let mut voice = Part::new("P1", "Voice", 1);
    let mut m1 = Measure::new(1);
    m1.attributes = Attributes {
        key: Some(key),
        time: Some(three_four),
        clefs: vec![StaffClef {
            staff: 1,
            clef: Clef::TREBLE,
        }],
        multiple_rest: None,
    };
    m1.voices = vec![Voice::new(
        1,
        1,
        vec![
            Event::note(&[p("A4")], NoteType::Quarter, 0).lyric(Syllabic::Single, "la"),
            Event::note(&[p("Bb4")], NoteType::Quarter, 0).lyric(Syllabic::Begin, "la"),
            Event::note(&[p("C5")], NoteType::Quarter, 0).lyric(Syllabic::End, "la"),
        ],
    )];
    let mut m2 = Measure::new(2);
    m2.right_barline = Some(Barline::final_bar());
    m2.voices = vec![Voice::new(
        1,
        1,
        vec![Event::note(&[p("F4")], NoteType::Half, 1).lyric(Syllabic::Single, "la")],
    )];
    voice.measures = vec![m1, m2];

    let mut piano = Part::new("P2", "Piano", 2);
    piano.abbreviation = Some("Pno.".into());
    let mut m1 = Measure::new(1);
    m1.attributes = Attributes {
        key: Some(key),
        time: Some(three_four),
        clefs: vec![
            StaffClef {
                staff: 1,
                clef: Clef::TREBLE,
            },
            StaffClef {
                staff: 2,
                clef: Clef::BASS,
            },
        ],
        multiple_rest: None,
    };
    m1.voices = vec![
        Voice::new(
            1,
            1,
            vec![Event::note(&ps(&["F4", "A4", "C5"]), NoteType::Half, 1).stem(Stem::Up)],
        ),
        Voice::new(
            2,
            1,
            vec![
                Event::rest(NoteType::Quarter, 0),
                Event::note(&[p("C4")], NoteType::Half, 0).stem(Stem::Down),
            ],
        ),
        Voice::new(
            5,
            2,
            vec![
                Event::note(&[p("F2")], NoteType::Quarter, 0),
                Event::note(&[p("C3")], NoteType::Quarter, 0),
                Event::note(&[p("A3")], NoteType::Quarter, 0),
            ],
        ),
    ];
    m1.directions = vec![
        Direction::new(Fraction::ZERO, DirectionKind::Dynamic(Dynamic::P))
            .below()
            .on_staff(1),
    ];
    let mut m2 = Measure::new(2);
    m2.right_barline = Some(Barline::final_bar());
    m2.voices = vec![
        Voice::new(
            1,
            1,
            vec![Event::note(&ps(&["E4", "G4", "C5"]), NoteType::Half, 1)],
        ),
        Voice::new(5, 2, vec![Event::note(&[p("C3")], NoteType::Half, 1)]),
    ];
    piano.measures = vec![m1, m2];

    Score {
        metadata: Metadata {
            title: Some("Piano Duet".into()),
            composer: Some("The scorelift contributors".into()),
            ..Default::default()
        },
        parts: vec![voice, piano],
    }
}
