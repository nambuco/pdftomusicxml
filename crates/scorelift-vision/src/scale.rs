//! SCALE step: the image's musical proportions, from run-length histograms
//! (the classic Fujinaga method, also used by Audiveris).
//!
//! In every column, a staff line is a short ink run followed by a white run
//! up to the next line, so the most common (ink + white) pair is the
//! interline and the most common ink run is the line thickness.

use serde::Serialize;

use crate::BinaryImage;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Scale {
    /// Staff line thickness in pixels.
    pub line_thickness: f32,
    /// Distance between staff line centers in pixels.
    pub interline: f32,
    /// Interline of small (cue) staves, if there are any.
    pub small_interline: Option<f32>,
    /// Typical beam thickness, if beams were seen.
    pub beam_thickness: Option<f32>,
    /// Typical stem thickness, if stems were seen.
    pub stem_thickness: Option<f32>,
}

/// Index of the highest bin in `lo..=hi`, refined to sub-pixel precision by
/// a weighted mean over its neighbours. `None` if the range is empty.
fn peak(hist: &[u64], lo: usize, hi: usize) -> Option<(f32, u64)> {
    let hi = hi.min(hist.len().saturating_sub(1));
    if lo > hi {
        return None;
    }
    let (i, &count) = hist[lo..=hi]
        .iter()
        .enumerate()
        .max_by_key(|&(i, &c)| (c, std::cmp::Reverse(i)))?;
    if count == 0 {
        return None;
    }
    let i = i + lo;
    let (mut sum, mut weight) = (0.0, 0.0);
    let from = i.saturating_sub(1);
    for (j, &c) in hist.iter().enumerate().skip(from).take(i + 2 - from) {
        sum += j as f64 * c as f64;
        weight += c as f64;
    }
    Some(((sum / weight) as f32, count))
}

pub fn estimate_scale(img: &BinaryImage) -> Option<Scale> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut black = vec![0u64; h + 1];
    let mut combo = vec![0u64; h + 1];

    for x in 0..w as i64 {
        // Runs in this column, as (is_ink, length); runs touching the border are dropped.
        let mut runs: Vec<(bool, usize)> = Vec::new();
        let mut y = 0usize;
        while y < h {
            let ink = img.get(x, y as i64);
            let start = y;
            while y < h && img.get(x, y as i64) == ink {
                y += 1;
            }
            if start > 0 && y < h {
                runs.push((ink, y - start));
            }
        }
        for (i, &(ink, len)) in runs.iter().enumerate() {
            if !ink {
                continue;
            }
            black[len] += 1;
            if let Some(&(false, white)) = runs.get(i + 1) {
                combo[len + white] += 1;
            }
        }
    }

    let (line, _) = peak(&black, 1, h / 8)?;
    let min_interline = (2.0 * line + 2.0) as usize;
    let (interline, main_count) = peak(&combo, min_interline, h / 2)?;

    let small_interline = peak(
        &combo,
        (0.5 * interline) as usize,
        (0.85 * interline) as usize,
    )
    .filter(|&(_, c)| c * 5 >= main_count)
    .map(|(v, _)| v);

    let beam_lo = ((0.3 * interline) as usize).max((2.0 * line).ceil() as usize + 1);
    let beam_thickness = peak(&black, beam_lo, (0.8 * interline) as usize)
        .filter(|&(_, c)| c >= 3 * w as u64 / 100)
        .map(|(v, _)| v);

    // Horizontal ink runs: stems give many identical short runs.
    let mut horizontal = vec![0u64; w + 1];
    for y in 0..h as u32 {
        let row = img.row(y);
        let mut x = 0usize;
        while x < w {
            if row[x] {
                let start = x;
                while x < w && row[x] {
                    x += 1;
                }
                horizontal[x - start] += 1;
            } else {
                x += 1;
            }
        }
    }
    let stem_thickness = peak(&horizontal, 1, (0.4 * interline) as usize).map(|(v, _)| v);

    Some(Scale {
        line_thickness: line,
        interline,
        small_interline,
        beam_thickness,
        stem_thickness,
    })
}
