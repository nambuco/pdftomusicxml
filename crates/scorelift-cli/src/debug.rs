//! `scorelift debug`: runs the pipeline up to GRID and shows what it found.

use std::path::Path;

use serde_json::json;

pub fn run(input: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let analysis = scorelift_core::analyze(input)?;
    std::fs::create_dir_all(out_dir)?;

    println!(
        "{:>4}  {:<6} {:>10}  {:>9} {:>6} {:>5}  {:>6} {:>8} {:>8}",
        "page", "image", "pixels", "interline", "line", "stem", "staves", "barlines", "measures"
    );
    let mut sheets_json = Vec::new();
    let (mut staves, mut measures) = (0, 0);
    for (i, sheet) in analysis.sheets.iter().enumerate() {
        let size = format!("{}x{}", sheet.image.width(), sheet.image.height());
        let file = format!("p{:02}-{:02}-{}.png", sheet.page, i + 1, sheet.name);
        match &sheet.layout {
            Some(layout) => {
                let s = &layout.scale;
                let barlines: usize = layout.staves.iter().map(|s| s.barlines.len()).sum();
                println!(
                    "{:>4}  {:<6} {:>10}  {:>9.2} {:>6.2} {:>5}  {:>6} {:>8} {:>8}",
                    sheet.page,
                    sheet.name,
                    size,
                    s.interline,
                    s.line_thickness,
                    s.stem_thickness.map_or("-".into(), |t| format!("{t:.1}")),
                    layout.staves.len(),
                    barlines,
                    layout.measure_count()
                );
                staves += layout.staves.len();
                measures += layout.measure_count();
                scorelift_vision::draw_overlay(&sheet.image, layout).save(out_dir.join(&file))?;
            }
            None => {
                println!(
                    "{:>4}  {:<6} {:>10}  no staff found",
                    sheet.page, sheet.name, size
                );
                // Save the plain image so it can be inspected.
                sheet.image.save(out_dir.join(&file))?;
            }
        }
        sheets_json.push(json!({
            "page": sheet.page,
            "name": sheet.name,
            "bbox": sheet.bbox,
            "width": sheet.image.width(),
            "height": sheet.image.height(),
            "layout": sheet.layout,
        }));
    }
    println!(
        "total: {} images, {staves} staves, {measures} measures",
        analysis.sheets.len()
    );
    for w in &analysis.warnings {
        eprintln!("warning: {w}");
    }

    let doc = json!({
        "input": input.display().to_string(),
        "sheets": sheets_json,
        "page_text": analysis.page_text,
        "warnings": analysis.warnings,
    });
    std::fs::write(
        out_dir.join("layout.json"),
        serde_json::to_string_pretty(&doc)?,
    )?;
    println!("wrote overlays and layout.json to {}", out_dir.display());
    Ok(())
}
