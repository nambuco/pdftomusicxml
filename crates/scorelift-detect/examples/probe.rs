//! Prints template scores at a position: `probe in.png x y`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let rgba = image::open(&a[0])?.to_rgba8();
    let gray = image::GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, al] = rgba.get_pixel(x, y).0;
        let l = (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000;
        image::Luma([(255 - (255 - l) * u32::from(al) / 255) as u8])
    });
    let layout = scorelift_vision::analyze(&gray).ok_or("no staff")?;
    let s = &layout.staves[0];
    println!("staff lines {:?} interline {:.2}", s.lines, s.interline());
    for (sym, score, x) in scorelift_detect::probe(&gray, &layout, a[1].parse()?, a[2].parse()?)
        .iter()
        .take(6)
    {
        println!("{sym:?} {score:.3} at x {x}");
    }
    Ok(())
}
