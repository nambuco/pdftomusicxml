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
        for w in &page.warnings {
            println!("  warning: {w}");
        }
    }
    Ok(())
}
