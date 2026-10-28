//! Writes every built-in fixture as `<name>.musicxml` into the given directory.
//! Used by `scripts/validate-xsd.sh`.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: write_fixtures <dir>")?,
    );
    std::fs::create_dir_all(&dir)?;
    for name in scorelift_model::fixtures::NAMES {
        let score = scorelift_model::fixtures::by_name(name).expect("listed fixture exists");
        let path = dir.join(format!("{name}.musicxml"));
        scorelift_musicxml::write_file(&score, &path)?;
        println!("{}", path.display());
    }
    Ok(())
}
