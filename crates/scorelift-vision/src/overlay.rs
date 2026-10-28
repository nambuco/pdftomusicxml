//! Debug overlays: draws what the vision steps found on top of the image.

use image::{GrayImage, Rgb, RgbImage};

use crate::Layout;

const STAFF_LINE: Rgb<u8> = Rgb([230, 30, 30]);
const STAFF_BOX: Rgb<u8> = Rgb([20, 160, 60]);
const BARLINE: Rgb<u8> = Rgb([30, 80, 230]);
const REPEAT: Rgb<u8> = Rgb([200, 40, 200]);
const MEASURE_TINTS: [Rgb<u8>; 2] = [Rgb([255, 225, 120]), Rgb([140, 220, 255])];

fn blend(img: &mut RgbImage, x: u32, y: u32, c: Rgb<u8>, alpha: f32) {
    if x >= img.width() || y >= img.height() {
        return;
    }
    let p = img.get_pixel_mut(x, y);
    for i in 0..3 {
        p.0[i] = (f32::from(p.0[i]) * (1.0 - alpha) + f32::from(c.0[i]) * alpha) as u8;
    }
}

fn fill(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>, alpha: f32) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            blend(img, x, y, c, alpha);
        }
    }
}

fn rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
    fill(img, x0, y0, x1, y0, c, 1.0);
    fill(img, x0, y1, x1, y1, c, 1.0);
    fill(img, x0, y0, x0, y1, c, 1.0);
    fill(img, x1, y0, x1, y1, c, 1.0);
}

/// The image in a lightened gray with staves, barlines and measures on top.
pub fn draw_overlay(gray: &GrayImage, layout: &Layout) -> RgbImage {
    let mut img = RgbImage::from_fn(gray.width(), gray.height(), |x, y| {
        let v = gray.get_pixel(x, y).0[0];
        let v = 255 - (255 - v) / 2;
        Rgb([v, v, v])
    });
    for staff in &layout.staves {
        let il = staff.interline();
        let top = (staff.top() - il).max(0.0) as u32;
        let bottom = ((staff.bottom() + il) as u32).min(img.height() - 1);
        for (i, m) in staff.measures.iter().enumerate() {
            let tint = MEASURE_TINTS[i % 2];
            fill(&mut img, m.x0, top, m.x1, bottom, tint, 0.35);
        }
        for &y in &staff.lines {
            fill(
                &mut img,
                staff.left,
                y.round() as u32,
                staff.right,
                y.round() as u32,
                STAFF_LINE,
                1.0,
            );
        }
        rect(&mut img, staff.left, top, staff.right, bottom, STAFF_BOX);
        for b in &staff.barlines {
            fill(&mut img, b.x0, top, b.x1, bottom, BARLINE, 0.8);
            let dot = (il / 2.0) as u32;
            let mid = ((staff.lines[1] + staff.lines[3]) / 2.0) as u32;
            if b.repeat_backward {
                fill(
                    &mut img,
                    b.x0.saturating_sub(dot * 2),
                    mid - dot,
                    b.x0,
                    mid + dot,
                    REPEAT,
                    0.6,
                );
            }
            if b.repeat_forward {
                fill(
                    &mut img,
                    b.x1,
                    mid - dot,
                    b.x1 + dot * 2,
                    mid + dot,
                    REPEAT,
                    0.6,
                );
            }
        }
    }
    img
}
