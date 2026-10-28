//! Reads a vector PDF and compares it with a built-in fixture:
//! `cargo run -p scorelift-semantic --example vector_compare -- file.pdf feature-tour`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let pdf = args
        .next()
        .ok_or("usage: vector_compare <file.pdf> <fixture>")?;
    let name = args
        .next()
        .ok_or("usage: vector_compare <file.pdf> <fixture>")?;
    let expected = scorelift_model::fixtures::by_name(&name).ok_or("unknown fixture")?;
    let doc = scorelift_pdf::load(pdf.as_ref())?;
    let reading = scorelift_semantic::read_vector(&doc).ok_or("no music font found")?;
    for w in &reading.warnings {
        println!("warning: {w}");
    }
    print!(
        "{}",
        scorelift_model::compare::compare(&expected, &reading.score)
    );
    for issue in scorelift_model::validate::validate(&reading.score) {
        println!("  issue: {issue}");
    }
    if let Some(out) = args.next() {
        scorelift_musicxml::write_file(&reading.score, out.as_ref())?;
    }
    Ok(())
}
