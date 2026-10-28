use std::io::Read;

use roxmltree::{Document, Node};
use scorelift_model::fixtures;
use scorelift_musicxml::{to_musicxml_string, to_mxl_bytes};

fn children<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Vec<Node<'a, 'i>> {
    node.children().filter(|n| n.has_tag_name(name)).collect()
}

fn child_text<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children().find(|n| n.has_tag_name(name))?.text()
}

fn parse(xml: &str) -> Document<'_> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    Document::parse_with_options(xml, options).expect("well-formed XML")
}

#[test]
fn every_fixture_is_well_formed() {
    for name in fixtures::NAMES {
        let xml = to_musicxml_string(&fixtures::by_name(name).unwrap()).unwrap();
        let doc = parse(&xml);
        let root = doc.root_element();
        assert_eq!(root.tag_name().name(), "score-partwise");
        assert_eq!(root.attribute("version"), Some("4.0"));
    }
}

#[test]
fn scale_has_eight_quarter_notes() {
    let xml = to_musicxml_string(&fixtures::c_major_scale()).unwrap();
    let doc = parse(&xml);
    let notes: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("note"))
        .collect();
    assert_eq!(notes.len(), 8);
    for n in &notes {
        assert_eq!(child_text(*n, "duration"), Some("1"));
        assert_eq!(child_text(*n, "type"), Some("quarter"));
    }
    let divisions = doc
        .descendants()
        .find(|n| n.has_tag_name("divisions"))
        .unwrap();
    assert_eq!(divisions.text(), Some("1"));
}

#[test]
fn triplets_and_sixteenths_set_divisions() {
    let xml = to_musicxml_string(&fixtures::feature_tour()).unwrap();
    let doc = parse(&xml);
    // 16ths need 4 per quarter, eighth triplets need 3: lcm = 12.
    let divisions = doc
        .descendants()
        .find(|n| n.has_tag_name("divisions"))
        .unwrap();
    assert_eq!(divisions.text(), Some("12"));

    let measures: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("measure"))
        .collect();
    assert_eq!(measures[0].attribute("implicit"), Some("yes"));
    for m in &measures[1..] {
        // Every full 4/4 measure must add up to 4 quarters = 48 divisions.
        let total: u32 = children(*m, "note")
            .into_iter()
            .filter(|n| {
                !n.children()
                    .any(|c| c.has_tag_name("chord") || c.has_tag_name("grace"))
            })
            .map(|n| child_text(n, "duration").unwrap().parse::<u32>().unwrap())
            .sum();
        assert_eq!(total, 48, "measure {:?}", m.attribute("number"));
    }
}

#[test]
fn chord_notes_after_the_first_are_marked() {
    let xml = to_musicxml_string(&fixtures::feature_tour()).unwrap();
    let doc = parse(&xml);
    let m1 = doc
        .descendants()
        .find(|n| n.has_tag_name("measure") && n.attribute("number") == Some("1"))
        .unwrap();
    let notes = children(m1, "note");
    let chord: Vec<_> = notes[4..].iter().collect();
    assert_eq!(chord.len(), 3);
    assert!(!chord[0].children().any(|c| c.has_tag_name("chord")));
    assert!(chord[1].children().any(|c| c.has_tag_name("chord")));
    // Lyrics only on the first note of the chord; ties on every note.
    assert_eq!(children(*chord[0], "lyric").len(), 1);
    assert_eq!(children(*chord[1], "lyric").len(), 0);
    for n in chord {
        assert_eq!(children(*n, "tie").len(), 1);
    }
}

#[test]
fn piano_voices_are_separated_by_backups() {
    let xml = to_musicxml_string(&fixtures::piano_duet()).unwrap();
    let doc = parse(&xml);
    let piano = doc
        .descendants()
        .find(|n| n.has_tag_name("part") && n.attribute("id") == Some("P2"))
        .unwrap();
    let m1 = children(piano, "measure")[0];
    let backups: Vec<_> = children(m1, "backup")
        .into_iter()
        .map(|b| child_text(b, "duration").unwrap())
        .collect();
    // Divisions is 1 (only quarter-based values), a dotted half is 3.
    assert_eq!(backups, ["3", "3"]);
    let staves = m1.descendants().find(|n| n.has_tag_name("staves")).unwrap();
    assert_eq!(staves.text(), Some("2"));
    let staffs: Vec<_> = children(m1, "note")
        .into_iter()
        .map(|n| child_text(n, "staff").unwrap())
        .collect();
    assert_eq!(staffs, ["1", "1", "1", "1", "1", "2", "2", "2"]);
}

#[test]
fn output_is_deterministic() {
    let a = to_musicxml_string(&fixtures::feature_tour()).unwrap();
    let b = to_musicxml_string(&fixtures::feature_tour()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn mxl_archive_layout() {
    let bytes = to_mxl_bytes(&fixtures::c_major_scale()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(zip.by_index(0).unwrap().name(), "mimetype");

    let mut container = String::new();
    zip.by_name("META-INF/container.xml")
        .unwrap()
        .read_to_string(&mut container)
        .unwrap();
    let doc = parse(&container);
    let rootfile = doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .unwrap();
    let path = rootfile.attribute("full-path").unwrap();

    let mut score = String::new();
    zip.by_name(path)
        .unwrap()
        .read_to_string(&mut score)
        .unwrap();
    assert!(score.contains("<score-partwise version=\"4.0\">"));
}

#[test]
fn empty_score_is_rejected() {
    let score = scorelift_model::Score::default();
    assert!(to_musicxml_string(&score).is_err());
}
