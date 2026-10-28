//! GRID step: staves, barlines and the measures between them.
//!
//! Assumes horizontal staff lines, which holds for computer renders.
//! Skewed scans will need line tracking instead of row projections.

use image::GrayImage;
use scorelift_model::BarStyle;
use serde::Serialize;

use crate::{BinaryImage, Scale};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Staff {
    /// Line centers from top to bottom, in pixels.
    pub lines: [f32; 5],
    pub line_thickness: f32,
    /// Horizontal extent of the staff lines.
    pub left: u32,
    pub right: u32,
    pub barlines: Vec<Barline>,
    pub measures: Vec<MeasureSpan>,
}

impl Staff {
    pub fn top(&self) -> f32 {
        self.lines[0]
    }

    pub fn bottom(&self) -> f32 {
        self.lines[4]
    }

    pub fn interline(&self) -> f32 {
        (self.bottom() - self.top()) / 4.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Barline {
    /// Horizontal extent of the whole group (e.g. both strokes of a final bar).
    pub x0: u32,
    pub x1: u32,
    pub style: BarStyle,
    /// Repeat dots on the left (end of a repeated section).
    pub repeat_backward: bool,
    /// Repeat dots on the right (start of a repeated section).
    pub repeat_forward: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MeasureSpan {
    pub x0: u32,
    pub x1: u32,
}

/// A horizontal band of rows that could be one staff line.
#[derive(Debug, Clone, Copy)]
pub struct LineBand {
    pub center: f32,
    pub thickness: u32,
    /// Total darkness of the band, in fully-black pixel equivalents.
    pub darkness: f32,
}

/// Finds all five-line staves, then their barlines and measures.
pub fn find_staves(gray: &GrayImage, img: &BinaryImage, scale: &Scale) -> Vec<Staff> {
    let bands = line_bands(gray, scale);
    let mut staves = Vec::new();
    let il = scale.interline;
    let tolerance = 0.2 * il + 1.0;
    let mut i = 0;
    while i < bands.len() {
        // The longest run of evenly spaced bands starting here. One missing
        // line is filled in: a multi-measure rest bar lying on a line, or a
        // faint anti-aliased line, can hide it from `line_bands`.
        let mut chain = vec![bands[i]];
        let mut filled = false;
        for b in &bands[i + 1..] {
            let last = *chain.last().unwrap();
            let gap = b.center - last.center;
            if (gap - il).abs() <= tolerance {
                chain.push(*b);
            } else if !filled && (gap - 2.0 * il).abs() <= tolerance {
                filled = true;
                chain.push(LineBand {
                    center: (last.center + b.center) / 2.0,
                    thickness: last.thickness.min(b.thickness),
                    darkness: 0.0,
                });
                chain.push(*b);
            } else if gap > 2.0 * il + tolerance {
                break;
            }
        }
        if chain.len() < 5 {
            i += 1;
            continue;
        }
        // More than five (e.g. a full-width line just outside the staff):
        // keep the darkest five consecutive lines.
        let start = (0..=chain.len() - 5)
            .max_by(|&a, &b| {
                let sum = |s: usize| chain[s..s + 5].iter().map(|l| l.darkness).sum::<f32>();
                sum(a).total_cmp(&sum(b))
            })
            .unwrap_or(0);
        let chosen = &chain[start..start + 5];
        let lines = [0, 1, 2, 3, 4].map(|k| chosen[k].center);
        let thickness = chosen.iter().map(|b| b.thickness as f32).sum::<f32>() / 5.0;
        if let Some((left, right)) = staff_extent(gray, &lines, thickness, il) {
            staves.push(Staff {
                lines,
                line_thickness: thickness,
                left,
                right,
                barlines: Vec::new(),
                measures: Vec::new(),
            });
        }
        // Skip past this staff's lines.
        while i < bands.len() && bands[i].center <= lines[4] {
            i += 1;
        }
    }

    let tops_bottoms: Vec<(f32, f32)> = staves.iter().map(|s| (s.top(), s.bottom())).collect();
    for staff in &mut staves {
        staff.barlines = find_barlines(img, scale, staff, &tops_bottoms);
        staff.measures = measures(staff, il);
    }
    staves
}

/// Rows much darker than their surroundings, grouped into bands.
///
/// Works on the gray image because an anti-aliased one-pixel line that
/// straddles two rows is only half dark in each, and would vanish from the
/// binary image. Darkness is measured against the lightest row nearby, so
/// rows that are dark only because of dense notes do not count.
#[doc(hidden)]
pub fn line_bands(gray: &GrayImage, scale: &Scale) -> Vec<LineBand> {
    let raw: Vec<f32> = gray
        .rows()
        .map(|row| row.map(|p| f32::from(255 - p.0[0])).sum::<f32>() / 255.0)
        .collect();
    let reach = (0.4 * scale.interline).max(2.0) as usize;
    let darkness: Vec<f32> = (0..raw.len())
        .map(|y| {
            let lo = y.saturating_sub(reach);
            let hi = (y + reach).min(raw.len() - 1);
            let floor = raw[lo..=hi].iter().copied().fold(f32::MAX, f32::min);
            raw[y] - floor
        })
        .collect();
    let max = darkness.iter().copied().fold(0.0, f32::max);
    if max <= 0.0 {
        return Vec::new();
    }
    let max_thickness = (2.5 * scale.line_thickness).max(3.0) as usize + 1;

    // Grow a band around each local peak over rows at least half as dark.
    let mut bands = Vec::new();
    let mut next_free = 0usize;
    for y in 0..darkness.len() {
        let d = darkness[y];
        let left = if y > 0 { darkness[y - 1] } else { 0.0 };
        let right = darkness.get(y + 1).copied().unwrap_or(0.0);
        if y < next_free || d < 0.3 * max || d <= left || d < right {
            continue;
        }
        let (mut start, mut end) = (y, y);
        while start > next_free && y - start < max_thickness && darkness[start - 1] >= 0.5 * d {
            start -= 1;
        }
        while end + 1 < darkness.len() && end - y < max_thickness && darkness[end + 1] >= 0.5 * d {
            end += 1;
        }
        next_free = end + 1;
        // Too thick for a line: a beam or rest bar lying on it. Keep only the
        // peak row; the spacing and extent checks reject it if it is no line.
        if end - start + 1 > max_thickness {
            (start, end) = (y, y);
        }
        let thickness = (end - start + 1) as u32;
        let (mut sum, mut total) = (0.0f64, 0.0f32);
        for (row, &v) in darkness.iter().enumerate().take(end + 1).skip(start) {
            sum += row as f64 * f64::from(v);
            total += v;
        }
        bands.push(LineBand {
            center: (sum / f64::from(total)) as f32,
            thickness,
            darkness: total,
        });
    }
    bands
}

/// Rows covered by a staff line centred at `center`.
fn line_rows(center: f32, thickness: f32) -> (i64, i64) {
    let half = thickness / 2.0 + 1.0;
    (
        (center - half).floor() as i64,
        (center + half).ceil() as i64,
    )
}

/// The longest horizontal span where at least four of the five lines have ink.
fn staff_extent(gray: &GrayImage, lines: &[f32; 5], thickness: f32, il: f32) -> Option<(u32, u32)> {
    let height = i64::from(gray.height());
    let covered: Vec<bool> = (0..gray.width())
        .map(|x| {
            lines
                .iter()
                .filter(|&&c| {
                    let (y0, y1) = line_rows(c, thickness);
                    (y0.max(0)..=y1.min(height - 1)).any(|y| gray.get_pixel(x, y as u32).0[0] < 200)
                })
                .count()
                >= 4
        })
        .collect();
    let max_gap = (il / 4.0).max(2.0) as usize;
    let mut best: Option<(usize, usize)> = None;
    let mut x = 0;
    while x < covered.len() {
        if !covered[x] {
            x += 1;
            continue;
        }
        let start = x;
        let mut end = x;
        let mut gap = 0;
        while x < covered.len() && gap <= max_gap {
            if covered[x] {
                end = x;
                gap = 0;
            } else {
                gap += 1;
            }
            x += 1;
        }
        if best.is_none_or(|(s, e)| end - start > e - s) {
            best = Some((start, end));
        }
    }
    let (left, right) = best?;
    // A staff is much wider than tall; anything shorter is a false match.
    ((right - left) as f32 >= 8.0 * il).then_some((left as u32, right as u32))
}

/// A vertical ink stroke spanning the staff.
#[derive(Debug, Clone, Copy)]
struct Stroke {
    x0: u32,
    x1: u32,
}

impl Stroke {
    fn width(&self) -> u32 {
        self.x1 - self.x0 + 1
    }
}

fn find_barlines(
    img: &BinaryImage,
    scale: &Scale,
    staff: &Staff,
    all_staves: &[(f32, f32)],
) -> Vec<Barline> {
    let il = scale.interline;
    let (y0, _) = line_rows(staff.top(), staff.line_thickness);
    let (_, y1) = line_rows(staff.bottom(), staff.line_thickness);
    let span = (y1 - y0 + 1) as f32;

    // Columns where ink runs (almost) uninterrupted from the top to the bottom line.
    let x_lo = staff.left.saturating_sub((il / 2.0) as u32);
    let x_hi = (staff.right + (il / 2.0) as u32).min(img.width() - 1);
    let full: Vec<bool> = (x_lo..=x_hi)
        .map(|x| img.count(i64::from(x), y0, i64::from(x) + 1, y1 + 1) as f32 >= 0.92 * span)
        .collect();

    let mut strokes = Vec::new();
    let mut i = 0;
    while i < full.len() {
        if !full[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < full.len() && full[i] {
            i += 1;
        }
        let stroke = Stroke {
            x0: x_lo + start as u32,
            x1: x_lo + i as u32 - 1,
        };
        if (stroke.width() as f32) <= 0.8 * il && is_barline(img, stroke, staff, il, all_staves) {
            strokes.push(stroke);
        }
    }

    // Strokes close together form one barline (double, final, repeat bars).
    let thick_min = scale
        .stem_thickness
        .map_or(0.25 * il, |s| (2.0 * s).max(0.25 * il));
    let mut groups: Vec<Vec<Stroke>> = Vec::new();
    for s in strokes {
        match groups.last_mut() {
            Some(g) if (s.x0 - g.last().unwrap().x1) as f32 <= 1.0 * il => g.push(s),
            _ => groups.push(vec![s]),
        }
    }
    groups
        .into_iter()
        .map(|g| {
            let thick: Vec<bool> = g.iter().map(|s| s.width() as f32 >= thick_min).collect();
            let style = match thick.as_slice() {
                [false] => BarStyle::Regular,
                [true] => BarStyle::Heavy,
                [false, false] => BarStyle::LightLight,
                [false, true] => BarStyle::LightHeavy,
                [true, false] => BarStyle::HeavyLight,
                [true, true] => BarStyle::HeavyHeavy,
                _ => BarStyle::Regular,
            };
            let x0 = g.first().unwrap().x0;
            let x1 = g.last().unwrap().x1;
            Barline {
                x0,
                x1,
                style,
                repeat_backward: has_repeat_dots(img, staff, x0, -1, il),
                repeat_forward: has_repeat_dots(img, staff, x1, 1, il),
            }
        })
        .collect()
}

/// Ink run length leaving the staff from row `y` along column `x`, stepping by `dir`.
fn run_from(img: &BinaryImage, x: i64, y: i64, dir: i64, limit: i64) -> i64 {
    let mut n = 0;
    while n < limit && img.get(x, y + dir * n) {
        n += 1;
    }
    n
}

/// A stem can also cross the whole staff, but it keeps going towards its
/// notehead or beam, and has a notehead at one end. A barline stops exactly
/// at the outer lines (unless it continues to the next staff of the system)
/// and has nothing attached.
fn is_barline(img: &BinaryImage, s: Stroke, staff: &Staff, il: f32, all: &[(f32, f32)]) -> bool {
    let (top0, _) = line_rows(staff.top(), staff.line_thickness);
    let (_, bottom1) = line_rows(staff.bottom(), staff.line_thickness);
    let limit = (20.0 * il) as i64;
    let max_ext = (0.15 * il).max(staff.line_thickness + 2.0) as i64;
    let x = i64::from((s.x0 + s.x1) / 2);

    let up = run_from(img, x, top0, -1, limit);
    let down = run_from(img, x, bottom1, 1, limit);
    let reaches = |y: f32| {
        all.iter()
            .any(|&(t, b)| (y - t).abs() <= 0.5 * il || (y - b).abs() <= 0.5 * il)
    };
    let up_ok = up <= max_ext || reaches((top0 - up) as f32);
    let down_ok = down <= max_ext || reaches((bottom1 + down) as f32);
    up_ok && down_ok && !has_head_beside(img, s, staff, il)
}

/// True if a solid blob (a notehead) touches either end of the stroke.
fn has_head_beside(img: &BinaryImage, s: Stroke, staff: &Staff, il: f32) -> bool {
    let w = (0.6 * il).max(2.0) as i64;
    let half_h = (0.35 * il).max(1.0) as i64;
    let on_line = |y: i64| {
        staff.lines.iter().any(|&c| {
            let (y0, y1) = line_rows(c, staff.line_thickness);
            (y0..=y1).contains(&y)
        })
    };
    let boxes = [
        (i64::from(s.x0) - 1 - w, i64::from(s.x0) - 1),
        (i64::from(s.x1) + 2, i64::from(s.x1) + 2 + w),
    ];
    [staff.top(), staff.bottom()].iter().any(|&end| {
        let cy = end.round() as i64;
        boxes.iter().any(|&(x0, x1)| {
            let (mut ink, mut area) = (0, 0);
            for y in cy - half_h..=cy + half_h {
                if on_line(y) {
                    continue;
                }
                ink += img.count(x0, y, x1, y + 1);
                area += x1 - x0;
            }
            area > 0 && ink as f32 >= 0.5 * area as f32
        })
    })
}

/// Repeat dots sit in the second and third spaces, just beside the barline.
fn has_repeat_dots(img: &BinaryImage, staff: &Staff, edge: u32, dir: i64, il: f32) -> bool {
    let size = (0.3 * il).max(2.0) as i64;
    let space_centers = [
        (staff.lines[1] + staff.lines[2]) / 2.0,
        (staff.lines[2] + staff.lines[3]) / 2.0,
    ];
    let from = (0.2 * il) as i64;
    let to = (1.0 * il) as i64;
    (from..=to).any(|d| {
        let cx = i64::from(edge) + dir * d;
        space_centers.iter().all(|&cy| {
            let cy = cy.round() as i64;
            let n = img.count(
                cx - size / 2,
                cy - size / 2,
                cx - size / 2 + size,
                cy - size / 2 + size,
            );
            n as f32 >= 0.8 * (size * size) as f32
        })
    })
}

/// Spans between consecutive barlines. A barline at the very start of the
/// staff (the system's opening line) does not open an empty measure.
fn measures(staff: &Staff, il: f32) -> Vec<MeasureSpan> {
    let mut edges: Vec<(u32, u32)> = staff.barlines.iter().map(|b| (b.x0, b.x1)).collect();
    if edges
        .first()
        .is_none_or(|&(x0, _)| x0 as f32 > staff.left as f32 + il)
    {
        edges.insert(0, (staff.left, staff.left));
    }
    if edges
        .last()
        .is_none_or(|&(_, x1)| (x1 as f32) < staff.right as f32 - il)
    {
        edges.push((staff.right, staff.right));
    }
    edges
        .windows(2)
        .filter(|w| (w[1].0 as f32 - w[0].1 as f32) > il)
        .map(|w| MeasureSpan {
            x0: w[0].1,
            x1: w[1].0,
        })
        .collect()
}
