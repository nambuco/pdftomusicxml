//! `scorelift eval`: converts a PDF and compares it, bar by bar, with a
//! correct transcription of the same music. Both are compared as they
//! sound (see [`sounding`]), so that re-notated rhythms, such as two tied
//! quarters for a half note, are not counted as errors.

use std::path::Path;

use scorelift_model::Score;
use scorelift_model::compare::{compare, measure_diffs, sounding};

/// Keeps part `part` (0-based) and the measures in `bars` (0-based, end
/// exclusive) only.
fn excerpt(score: &Score, part: usize, bars: (usize, usize)) -> Score {
    let mut out = score.clone();
    out.parts = score.parts.get(part).cloned().into_iter().collect();
    for p in &mut out.parts {
        let end = bars.1.min(p.measures.len());
        p.measures = p
            .measures
            .get(bars.0.min(end)..end)
            .unwrap_or_default()
            .to_vec();
    }
    out
}

/// Parses `12`, `1-40` or `20-` (1-based, inclusive) into a 0-based range.
fn parse_bars(s: &str) -> Result<(usize, usize), String> {
    let bad = || format!("bad --bars {s:?}; use e.g. 1-40");
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let first: usize = a.trim().parse().map_err(|_| bad())?;
    let last: usize = if b.trim().is_empty() {
        usize::MAX
    } else {
        b.trim().parse().map_err(|_| bad())?
    };
    if first == 0 || last < first {
        return Err(bad());
    }
    Ok((first - 1, last))
}

pub fn run(
    input: &Path,
    truth: &Path,
    bars: Option<&str>,
    part: usize,
    all: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let range = bars.map(parse_bars).transpose()?.unwrap_or((0, usize::MAX));
    let part = part.checked_sub(1).ok_or("--part counts from 1")?;
    let expected = sounding(&excerpt(
        &scorelift_musicxml::read_file(truth)?,
        part,
        range,
    ));
    let conversion = scorelift_core::convert(input, &Default::default())?;
    for w in &conversion.warnings {
        println!("warning: {w}");
    }
    let found = sounding(&excerpt(&conversion.score, part, range));
    let first_bar = range.0 + 1;
    let diffs = measure_diffs(&expected, &found, 0);
    let mut exact = 0;
    for d in &diffs {
        if d.is_exact() {
            exact += 1;
            if !all {
                continue;
            }
        }
        let bar = first_bar + d.index;
        println!("bar {bar}{}", if d.is_exact() { ": exact" } else { "" });
        if !d.is_exact() {
            println!("  expected: {}", d.expected.join(" | "));
            println!("  found:    {}", d.found.join(" | "));
            for m in &d.missing {
                println!("  - missing {m}");
            }
            for e in &d.extra {
                println!("  + extra   {e}");
            }
        }
    }
    let c = compare(&expected, &found);
    println!();
    println!(
        "{exact} of {} bars exact; {} of {} notes and rests found, {} wrong or extra (precision {:.1}%, recall {:.1}%, F1 {:.1}%)",
        diffs.len(),
        c.matched,
        c.expected,
        c.found - c.matched,
        100.0 * c.precision(),
        100.0 * c.recall(),
        100.0 * c.f1()
    );
    if c.found_measures != c.expected_measures {
        println!(
            "measure count differs in this range: {} expected, {} found",
            c.expected_measures, c.found_measures
        );
    }
    Ok(())
}
