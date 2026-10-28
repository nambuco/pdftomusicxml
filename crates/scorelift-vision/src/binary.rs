//! BINARY step: grayscale to ink/background.

use image::GrayImage;

/// A black-and-white image; `true` means ink.
#[derive(Debug, Clone)]
pub struct BinaryImage {
    width: u32,
    height: u32,
    ink: Vec<bool>,
}

impl BinaryImage {
    pub fn new(width: u32, height: u32) -> Self {
        BinaryImage {
            width,
            height,
            ink: vec![false; width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Out-of-bounds pixels count as background.
    pub fn get(&self, x: i64, y: i64) -> bool {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return false;
        }
        self.ink[y as usize * self.width as usize + x as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, ink: bool) {
        let w = self.width as usize;
        self.ink[y as usize * w + x as usize] = ink;
    }

    pub fn row(&self, y: u32) -> &[bool] {
        let w = self.width as usize;
        &self.ink[y as usize * w..(y as usize + 1) * w]
    }

    /// Number of ink pixels in `[x0, x1) × [y0, y1)`, clipped to the image.
    pub fn count(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> u32 {
        let x0 = x0.clamp(0, i64::from(self.width)) as usize;
        let x1 = x1.clamp(0, i64::from(self.width)) as usize;
        let y0 = y0.clamp(0, i64::from(self.height)) as u32;
        let y1 = y1.clamp(0, i64::from(self.height)) as u32;
        if x0 >= x1 {
            return 0;
        }
        (y0..y1)
            .map(|y| self.row(y)[x0..x1].iter().filter(|&&b| b).count() as u32)
            .sum()
    }
}

/// Global Otsu threshold. Clean computer renders need nothing smarter;
/// scans will get an adaptive method later.
pub fn otsu_threshold(gray: &GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for p in gray.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total: u64 = hist.iter().sum();
    let sum_all: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum();
    let (mut best, mut best_var) = (127u8, -1.0f64);
    let (mut w0, mut sum0) = (0u64, 0.0f64);
    for (t, &count) in hist.iter().enumerate() {
        w0 += count;
        if w0 == 0 {
            continue;
        }
        let w1 = total - w0;
        if w1 == 0 {
            break;
        }
        sum0 += t as f64 * count as f64;
        let m0 = sum0 / w0 as f64;
        let m1 = (sum_all - sum0) / w1 as f64;
        let var = w0 as f64 * w1 as f64 * (m0 - m1).powi(2);
        if var > best_var {
            best_var = var;
            best = t as u8;
        }
    }
    best
}

/// Pixels at or below `threshold` become ink.
pub fn binarize(gray: &GrayImage, threshold: u8) -> BinaryImage {
    let mut out = BinaryImage::new(gray.width(), gray.height());
    for (x, y, p) in gray.enumerate_pixels() {
        if p.0[0] <= threshold {
            out.set(x, y, true);
        }
    }
    out
}
