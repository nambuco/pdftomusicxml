//! Prints per-row ink statistics of an image, to debug staff line detection:
//! `cargo run -p scorelift-vision --example rows -- image.png`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: rows <image.png>")?;
    let gray = image::open(path)?.to_luma8();
    let threshold = scorelift_vision::otsu_threshold(&gray);
    let bin = scorelift_vision::binarize(&gray, threshold);
    println!("otsu threshold {threshold}");
    let scale = scorelift_vision::estimate_scale(&bin);
    println!("scale {scale:?}");
    if let Some(scale) = scale {
        for b in scorelift_vision::line_bands(&gray, &scale) {
            println!(
                "band y={:.1} thickness={} darkness={:.0}",
                b.center, b.thickness, b.darkness
            );
        }
    }
    println!("{:>5} {:>6} {:>9} {:>6}", "row", "ink", "darkness", "min");
    for y in 0..gray.height() {
        let ink = bin.row(y).iter().filter(|&&b| b).count();
        let (mut dark, mut min) = (0u64, 255u8);
        for x in 0..gray.width() {
            let v = gray.get_pixel(x, y).0[0];
            dark += u64::from(255 - v);
            min = min.min(v);
        }
        let dark = dark / 255;
        if dark as f64 > 0.2 * f64::from(gray.width()) {
            println!("{y:>5} {ink:>6} {dark:>9} {min:>6}");
        }
    }
    Ok(())
}
