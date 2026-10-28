//! Prints the images found on each page of a PDF: `cargo run -p scorelift-pdf --example dump -- file.pdf`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: dump <file.pdf>")?;
    let doc = scorelift_pdf::load(path.as_ref())?;
    for page in &doc.pages {
        println!(
            "page {} ({:.0}x{:.0} pt): {} images, {} chars of text",
            page.number,
            page.width,
            page.height,
            page.images.len(),
            page.text.chars().count()
        );
        for img in &page.images {
            let r = img.bbox;
            println!(
                "  {:>4} {}x{} px at ({:.1}, {:.1}) {:.1}x{:.1} pt, {:.0} dpi",
                img.name,
                img.image.width(),
                img.image.height(),
                r.x,
                r.y,
                r.w,
                r.h,
                img.dpi()
            );
        }
        println!(
            "  {} glyphs, {} shapes",
            page.glyphs.len(),
            page.shapes.len()
        );
        if std::env::var_os("DUMP_VECTOR").is_some() {
            for g in &page.glyphs {
                let cps: Vec<String> = g
                    .text
                    .chars()
                    .map(|c| format!("U+{:04X}", c as u32))
                    .collect();
                println!(
                    "  glyph {:<12} {:<14} at ({:7.2}, {:7.2}) size {:5.2} width {:5.2}",
                    g.font,
                    cps.join(" "),
                    g.x,
                    g.y,
                    g.size,
                    g.width
                );
            }
            for s in &page.shapes {
                let b = s.bbox();
                println!(
                    "  shape stroke={:<6} filled={:<5} curved={:<5} paths={} bbox ({:.2}, {:.2}) {:.2}x{:.2}",
                    s.stroke.map_or("-".into(), |w| format!("{w:.2}")),
                    s.filled,
                    s.curved,
                    s.subpaths.len(),
                    b.x,
                    b.y,
                    b.w,
                    b.h
                );
            }
        }
        for w in &page.warnings {
            println!("  warning: {w}");
        }
    }
    Ok(())
}
