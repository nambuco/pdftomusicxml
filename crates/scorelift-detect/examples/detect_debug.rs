//! Draws what the recogniser found on a PNG and lists the symbols:
//! `cargo run -p scorelift-detect --example detect_debug -- in.png out.png`
//!
//! Colours: noteheads red, rests blue, clefs/time green, accidentals
//! orange, flags/dots magenta, dynamics cyan, stems dark red, beams
//! yellow, curves purple.

use image::{Rgb, RgbImage};
use scorelift_semantic::smufl::Sym;

fn rect(img: &mut RgbImage, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb<u8>) {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (x0, y0, x1, y1) = (x0 as i64, y0 as i64, x1 as i64, y1 as i64);
    for x in x0..=x1 {
        for y in [y0, y1] {
            if x >= 0 && y >= 0 && x < w && y < h {
                img.put_pixel(x as u32, y as u32, c);
            }
        }
    }
    for y in y0..=y1 {
        for x in [x0, x1] {
            if x >= 0 && y >= 0 && x < w && y < h {
                img.put_pixel(x as u32, y as u32, c);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: detect_debug <in.png> <out.png>")?;
    let output = args
        .next()
        .ok_or("usage: detect_debug <in.png> <out.png>")?;
    let rgba = image::open(&input)?.to_rgba8();
    let gray = image::GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let l = (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000;
        image::Luma([(255 - (255 - l) * u32::from(a) / 255) as u8])
    });
    let layout = scorelift_vision::analyze(&gray).ok_or("no staff")?;
    let systems = scorelift_detect::recognize(&gray, &layout, 1);
    for (i, sys) in systems.iter().enumerate() {
        let tops: Vec<f32> = sys.staves.iter().map(|s| s.top().round()).collect();
        let bars: Vec<f32> = sys.bars.iter().map(|b| b.x0.round()).collect();
        eprintln!(
            "system {i}: staves at {tops:?}, x {:.0}..{:.0}, bars {bars:?}, braces {:?}",
            sys.x0, sys.x1, sys.braces
        );
    }
    let mut img = RgbImage::from_fn(gray.width(), gray.height(), |x, y| {
        let v = 255 - (255 - gray.get_pixel(x, y).0[0]) / 2;
        Rgb([v, v, v])
    });
    for sys in &systems {
        for st in &sys.staves {
            for g in &st.glyphs {
                let c = match g.sym {
                    Sym::Head(_) => Rgb([230, 0, 0]),
                    Sym::Rest(_) => Rgb([0, 60, 230]),
                    Sym::Clef(_) | Sym::TimeDigit(_) | Sym::TimeCommon | Sym::TimeCut => {
                        Rgb([0, 160, 0])
                    }
                    Sym::Accidental(_) => Rgb([255, 140, 0]),
                    Sym::Dynamic(_) => Rgb([0, 190, 190]),
                    _ => Rgb([210, 0, 210]),
                };
                rect(&mut img, g.x, g.y - 3.0, g.x + g.w, g.y + 3.0, c);
                println!(
                    "{:?} at ({:.0}, {:.0}) step {}",
                    g.sym,
                    g.x,
                    g.y,
                    st.step(g.y)
                );
            }
            for s in &st.segments {
                rect(
                    &mut img,
                    s.a.x - 1.0,
                    s.a.y,
                    s.a.x + 1.0,
                    s.b.y,
                    Rgb([140, 0, 0]),
                );
            }
            for f in &st.fills {
                let c = if f.curved {
                    Rgb([140, 0, 200])
                } else {
                    Rgb([220, 200, 0])
                };
                rect(&mut img, f.x0, f.y0, f.x1, f.y1, c);
            }
        }
    }
    img.save(&output)?;
    Ok(())
}
