//! Staff, barline and measure detection on synthetic images.

use image::{GrayImage, Luma};
use scorelift_model::BarStyle;
use scorelift_vision::analyze;

const IL: u32 = 20;

fn blank(w: u32, h: u32) -> GrayImage {
    GrayImage::from_pixel(w, h, Luma([255]))
}

fn hline(img: &mut GrayImage, y: u32, x0: u32, x1: u32, v: u8) {
    for x in x0..=x1 {
        img.put_pixel(x, y, Luma([v]));
    }
}

fn vline(img: &mut GrayImage, x0: u32, x1: u32, y0: u32, y1: u32) {
    for x in x0..=x1 {
        for y in y0..=y1 {
            img.put_pixel(x, y, Luma([0]));
        }
    }
}

/// A filled notehead centred at (cx, cy).
fn head(img: &mut GrayImage, cx: f32, cy: f32) {
    let (rx, ry) = (0.65 * IL as f32, 0.5 * IL as f32);
    for y in (cy - ry) as u32..=(cy + ry) as u32 {
        for x in (cx - rx) as u32..=(cx + rx) as u32 {
            let (dx, dy) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
            if dx * dx + dy * dy <= 1.0 {
                img.put_pixel(x, y, Luma([0]));
            }
        }
    }
}

/// Five lines from `top`, spanning x0..=x1. The fourth line straddles two
/// rows at half intensity, like an anti-aliased render.
fn staff(img: &mut GrayImage, top: u32, x0: u32, x1: u32) -> [u32; 5] {
    let ys = [0, 1, 2, 3, 4].map(|i| top + i * IL);
    for (i, &y) in ys.iter().enumerate() {
        if i == 3 {
            hline(img, y, x0, x1, 128);
            hline(img, y + 1, x0, x1, 128);
        } else {
            hline(img, y, x0, x1, 0);
        }
    }
    ys
}

#[test]
fn single_staff_with_stems_and_final_bar() {
    let mut img = blank(1000, 200);
    let ys = staff(&mut img, 60, 10, 990);
    let (top, bottom) = (ys[0], ys[4]);
    // Opening system line, two barlines, final thin + thick bar.
    vline(&mut img, 10, 11, top, bottom);
    vline(&mut img, 330, 331, top, bottom);
    vline(&mut img, 660, 661, top, bottom);
    vline(&mut img, 975, 976, top, bottom);
    vline(&mut img, 982, 990, top, bottom);
    // A chord stem crossing the whole staff with heads at both ends.
    vline(&mut img, 200, 201, top, bottom);
    head(&mut img, 200.0 - 0.6 * IL as f32, bottom as f32);
    head(&mut img, 200.0 - 0.6 * IL as f32, top as f32);
    // A stem from a low note running up past the top line.
    vline(&mut img, 500, 501, top - 30, bottom + 10);
    head(&mut img, 500.0 - 0.6 * IL as f32, (bottom + 10) as f32);

    let layout = analyze(&img).expect("staff found");
    assert!(
        (layout.scale.interline - IL as f32).abs() < 1.0,
        "{:?}",
        layout.scale
    );
    assert_eq!(layout.staves.len(), 1);
    let s = &layout.staves[0];
    for (found, &expected) in s.lines.iter().zip(&ys) {
        let expected = if expected == ys[3] {
            expected as f32 + 0.5
        } else {
            expected as f32
        };
        assert!(
            (found - expected).abs() <= 0.6,
            "line {found} vs {expected}"
        );
    }
    let styles: Vec<_> = s.barlines.iter().map(|b| (b.x0, b.style)).collect();
    assert_eq!(
        styles,
        [
            (10, BarStyle::Regular),
            (330, BarStyle::Regular),
            (660, BarStyle::Regular),
            (975, BarStyle::LightHeavy),
        ]
    );
    assert_eq!(s.measures.len(), 3);
}

#[test]
fn repeat_dots_are_detected() {
    let mut img = blank(800, 200);
    let ys = staff(&mut img, 60, 10, 790);
    let (top, bottom) = (ys[0], ys[4]);
    vline(&mut img, 10, 11, top, bottom);
    // Forward repeat: thick, thin, then dots in spaces 2 and 3.
    vline(&mut img, 300, 307, top, bottom);
    vline(&mut img, 312, 313, top, bottom);
    for cy in [(ys[1] + ys[2]) / 2, (ys[2] + ys[3]) / 2] {
        head(&mut img, 322.0, cy as f32);
    }
    vline(&mut img, 789, 790, top, bottom);

    let layout = analyze(&img).expect("staff found");
    let bars = &layout.staves[0].barlines;
    let repeat = bars.iter().find(|b| b.x0 == 300).expect("repeat barline");
    assert_eq!(repeat.style, BarStyle::HeavyLight);
    assert!(repeat.repeat_forward);
    assert!(!repeat.repeat_backward);
}

#[test]
fn barlines_joining_two_staves_count_for_both() {
    let mut img = blank(900, 340);
    let upper = staff(&mut img, 60, 10, 890);
    let lower = staff(&mut img, 220, 10, 890);
    for x in [10, 450, 889] {
        vline(&mut img, x, x + 1, upper[0], lower[4]);
    }
    let layout = analyze(&img).expect("staves found");
    assert_eq!(layout.staves.len(), 2);
    for s in &layout.staves {
        assert_eq!(s.barlines.len(), 3, "{:?}", s.barlines);
        assert_eq!(s.measures.len(), 2);
    }
}

#[test]
fn blank_image_has_no_staff() {
    assert!(analyze(&blank(400, 100)).is_none());
}
