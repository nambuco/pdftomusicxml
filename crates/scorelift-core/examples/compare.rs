//! Converts any input (vector PDF, image PDF, PNG) and compares the result
//! with a built-in fixture:
//! `cargo run -p scorelift-core --example compare -- input.png feature-tour [out.musicxml]`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: compare <input> <fixture> [out.musicxml]";
    let input = args.next().ok_or(usage)?;
    let name = args.next().ok_or(usage)?;
    let expected = scorelift_model::fixtures::by_name(&name).ok_or("unknown fixture")?;
    let conversion = scorelift_core::convert(input.as_ref(), &Default::default())?;
    for w in &conversion.warnings {
        println!("warning: {w}");
    }
    print!(
        "{}",
        scorelift_model::compare::compare(&expected, &conversion.score)
    );
    for issue in scorelift_model::validate::validate(&conversion.score) {
        println!("  issue: {issue}");
    }
    if let Some(out) = args.next() {
        scorelift_musicxml::write_file(&conversion.score, out.as_ref())?;
    }
    Ok(())
}
