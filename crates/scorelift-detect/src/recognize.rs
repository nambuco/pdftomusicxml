//! From a binarized sheet image and its staff layout to `System`s of
//! symbols, stems, beams and curves.

use image::GrayImage;
use scorelift_model::BarStyle;
use scorelift_pdf::Point;
use scorelift_semantic::layout::{BarGroup, Fill, MusicGlyph, Segment, Staff, System};
use scorelift_semantic::smufl::{Head, Sym};
use scorelift_vision::{BinaryImage, Layout, binarize};

use crate::templates::{self, Template};

/// Size of grace-note glyphs relative to normal ones.
const GRACE_SIZE: f32 = 0.65;

/// Score added to a notehead with a stem at its edge.
const STEM_BONUS: f32 = 0.06;

/// Final notehead thresholds. Hollow heads are thin rings and score lower
/// when slightly off; heads in chords score lower because their neighbours
/// touch the ring, which the stem bonus makes up for.
fn head_threshold(sym: Sym) -> f32 {
    match sym {
        Sym::Head(Head::Black) => 0.85,
        Sym::Head(Head::Whole) => 0.75,
        _ => 0.7,
    }
}

/// Ink counts with O(1) rectangle sums.
struct Integral {
    w: usize,
    sums: Vec<u32>,
}

impl Integral {
    fn new(img: &BinaryImage) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let mut sums = vec![0u32; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = 0u32;
            for x in 0..w {
                row += u32::from(img.get(x as i64, y as i64));
                sums[(y + 1) * (w + 1) + x + 1] = sums[y * (w + 1) + x + 1] + row;
            }
        }
        Integral { w, sums }
    }

    /// Ink in `[x0, x1) × [y0, y1)`; the rectangle must lie inside the image.
    fn count(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> u32 {
        let s = |x: usize, y: usize| self.sums[y * (self.w + 1) + x];
        s(x1, y1) + s(x0, y0) - s(x0, y1) - s(x1, y0)
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    sym: Sym,
    score: f32,
    x0: i64,
    y0: i64,
    w: i64,
    h: i64,
    origin_y: f32,
    /// Size relative to normal glyphs (grace notes are smaller).
    rel: f32,
}

impl Candidate {
    fn overlap(&self, o: &Candidate) -> f32 {
        let ix = (self.x0 + self.w).min(o.x0 + o.w) - self.x0.max(o.x0);
        let iy = (self.y0 + self.h).min(o.y0 + o.h) - self.y0.max(o.y0);
        if ix <= 0 || iy <= 0 {
            return 0.0;
        }
        let smaller = (self.w * self.h).min(o.w * o.h).max(1);
        (ix * iy) as f32 / smaller as f32
    }
}

/// Dice similarity of template `t` placed with its top-left at (x0, y0).
fn score(img: &BinaryImage, integral: &Integral, t: &Template, x0: i64, y0: i64) -> Option<f32> {
    let (w, h) = (i64::from(t.width), i64::from(t.height));
    if x0 < 0 || y0 < 0 || x0 + w > i64::from(img.width()) || y0 + h > i64::from(img.height()) {
        return None;
    }
    let region = integral.count(
        x0 as usize,
        y0 as usize,
        (x0 + w) as usize,
        (y0 + h) as usize,
    );
    // Cheap rejection: far too little or too much ink in the window. Some
    // fonts draw quarter and shorter rests with thinner strokes than Bravura.
    let thin = matches!(t.sym, Sym::Rest(n) if n >= scorelift_model::NoteType::Quarter);
    let least = if thin { 0.45 } else { 0.6 };
    if (region as f32) < least * t.ink as f32 || (region as f32) > 1.7 * t.ink as f32 {
        return None;
    }
    // Ink in a thin ring around the window counts against the match: a real
    // symbol stands apart, a part of a larger shape does not.
    let m = t.margin;
    let (ex0, ey0) = ((x0 - m).max(0) as usize, (y0 - m).max(0) as usize);
    let ex1 = ((x0 + w + m) as usize).min(img.width() as usize);
    let ey1 = ((y0 + h + m) as usize).min(img.height() as usize);
    let ring = integral.count(ex0, ey0, ex1, ey1) - region;
    // Both directions allow one pixel of slack, which matters for thin
    // shapes such as hollow noteheads: recall is the share of template
    // pixels with ink nearby, precision the share of ink (in the window and
    // its ring) that lies on or next to the template shape.
    let (mut hit, mut consistent) = (0u32, 0u32);
    for ty in 0..t.height {
        for tx in 0..t.width {
            let (x, y) = (x0 + i64::from(tx), y0 + i64::from(ty));
            if t.get(tx, ty)
                && (img.get(x, y)
                    || img.get(x - 1, y)
                    || img.get(x + 1, y)
                    || img.get(x, y - 1)
                    || img.get(x, y + 1))
            {
                hit += 1;
            }
            if t.in_zone(tx, ty) && img.get(x, y) {
                consistent += 1;
            }
        }
    }
    let recall = hit as f32 / t.ink.max(1) as f32;
    let precision = consistent as f32 / (region + ring).max(1) as f32;
    if recall + precision == 0.0 {
        return Some(0.0);
    }
    Some(2.0 * recall * precision / (recall + precision))
}

/// Slides `t` along a row (origin at height `oy`) and keeps local maxima above `min`.
#[allow(clippy::too_many_arguments)]
fn scan_row(
    img: &BinaryImage,
    integral: &Integral,
    t: &Template,
    oy: f32,
    x_from: i64,
    x_to: i64,
    min: f32,
    out: &mut Vec<Candidate>,
) {
    let y0 = (oy - t.origin_y).round() as i64;
    let mut best: Option<Candidate> = None;
    for x0 in x_from..x_to {
        let Some(s) = score(img, integral, t, x0, y0) else {
            continue;
        };
        if s < min {
            continue;
        }
        let c = Candidate {
            sym: t.sym,
            score: s,
            x0,
            y0,
            w: i64::from(t.width),
            h: i64::from(t.height),
            origin_y: oy,
            rel: t.rel,
        };
        match &best {
            // Keep the best within one template width.
            Some(b) if x0 - b.x0 < i64::from(t.width) / 2 => {
                if s > b.score {
                    best = Some(c);
                }
            }
            _ => {
                if let Some(b) = best.take() {
                    out.push(b);
                }
                best = Some(c);
            }
        }
    }
    if let Some(b) = best {
        out.push(b);
    }
}

/// Erases thin horizontal lines through `rows`: in each column, an ink run
/// no taller than `max_run` that touches the line rows is line, not symbol.
fn erase_line(
    img: &mut BinaryImage,
    center: f32,
    thickness: f32,
    x0: u32,
    x1: u32,
    keep: impl Fn(u32) -> bool,
) {
    let max_run = (thickness.ceil() as i64) + 2;
    let (r0, r1) = (
        (center - thickness / 2.0 - 1.0).floor() as i64,
        (center + thickness / 2.0 + 1.0).ceil() as i64,
    );
    for x in x0..=x1.min(img.width() - 1) {
        if keep(x) {
            continue;
        }
        let xi = i64::from(x);
        let Some(start) = (r0..=r1).find(|&y| img.get(xi, y)) else {
            continue;
        };
        let mut top = start;
        while img.get(xi, top - 1) {
            top -= 1;
        }
        let mut bottom = start;
        while img.get(xi, bottom + 1) {
            bottom += 1;
        }
        // Only the line's own rows go: a thin symbol stroke touching the line
        // (the rim of a hollow notehead) keeps its pixels outside them.
        if bottom - top < max_run {
            for y in top.max(r0)..=bottom.min(r1) {
                if y >= 0 && y < i64::from(img.height()) {
                    img.set(x, y as u32, false);
                }
            }
        }
    }
}

fn horizontal_run(img: &BinaryImage, x: i64, y: i64) -> i64 {
    if !img.get(x, y) {
        return 0;
    }
    let (mut a, mut b) = (x, x);
    while img.get(a - 1, y) {
        a -= 1;
    }
    while img.get(b + 1, y) {
        b += 1;
    }
    b - a + 1
}

/// Vertical strokes at least `min_len` tall and at most `max_w` wide.
fn find_stems(
    img: &BinaryImage,
    x0: i64,
    x1: i64,
    y0: i64,
    y1: i64,
    min_len: i64,
    max_w: i64,
) -> Vec<(i64, i64, i64, i64)> {
    let mut runs: Vec<(i64, i64, i64)> = Vec::new();
    for x in x0.max(0)..x1.min(i64::from(img.width())) {
        let mut y = y0.max(0);
        let end = y1.min(i64::from(img.height()));
        while y < end {
            if img.get(x, y) {
                let s = y;
                while y < end && img.get(x, y) {
                    y += 1;
                }
                if y - s >= min_len {
                    runs.push((x, s, y - 1));
                }
            } else {
                y += 1;
            }
        }
    }
    // Group neighbouring columns with overlapping runs into strokes.
    let mut strokes: Vec<(i64, i64, i64, i64)> = Vec::new();
    for (x, a, b) in runs {
        match strokes
            .iter_mut()
            .rev()
            .take(8)
            .find(|s| x - s.1 <= 1 && a <= s.3 && b >= s.2)
        {
            Some(s) => {
                s.1 = x;
                s.2 = s.2.min(a);
                s.3 = s.3.max(b);
            }
            None => strokes.push((x, x, a, b)),
        }
    }
    strokes.retain(|s| s.1 - s.0 < max_w);
    strokes
}

/// Connected components (8-neighbour) as pixel lists.
fn components(img: &BinaryImage, x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Vec<(i64, i64)>> {
    let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    for sy in 0..h {
        for sx in 0..w {
            let (gx, gy) = (x0 + sx as i64, y0 + sy as i64);
            if seen[sy * w + sx] || !img.get(gx, gy) {
                continue;
            }
            let mut stack = vec![(sx, sy)];
            seen[sy * w + sx] = true;
            let mut px = Vec::new();
            while let Some((cx, cy)) = stack.pop() {
                px.push((x0 + cx as i64, y0 + cy as i64));
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                            continue;
                        }
                        let i = ny as usize * w + nx as usize;
                        if !seen[i] && img.get(x0 + nx, y0 + ny) {
                            seen[i] = true;
                            stack.push((nx as usize, ny as usize));
                        }
                    }
                }
            }
            out.push(px);
        }
    }
    out
}

/// Recognises every staff of one sheet image.
pub fn recognize(gray: &GrayImage, layout: &Layout, page: u32) -> Vec<System> {
    let bin = binarize(gray, layout.threshold);
    let mut clean = bin.clone();
    let il = layout.scale.interline;
    let thickness = layout.scale.line_thickness;

    // Erase staff lines and ledger lines (short lines above and below).
    for s in &layout.staves {
        let barline = |x: u32| s.barlines.iter().any(|b| x + 1 >= b.x0 && x <= b.x1 + 1);
        for &y in &s.lines {
            erase_line(
                &mut clean,
                y,
                s.line_thickness.max(thickness),
                s.left,
                s.right,
                barline,
            );
        }
        for k in 1..=6 {
            for y in [s.top() - k as f32 * il, s.bottom() + k as f32 * il] {
                let row = y.round() as i64;
                let ledger = |x: u32| {
                    let r = (row - 1..=row + 1)
                        .map(|r| horizontal_run(&bin, i64::from(x), r))
                        .max()
                        .unwrap_or(0);
                    !(r as f32 >= 0.9 * il && r as f32 <= 4.5 * il)
                };
                erase_line(&mut clean, y, thickness, s.left, s.right, ledger);
            }
        }
    }
    // Barlines are known from the staff analysis; left in, their strokes
    // look like the stems of flats or the edges of digits and rests.
    for s in &layout.staves {
        for b in &s.barlines {
            for x in b.x0.saturating_sub(1)..=(b.x1 + 1).min(clean.width() - 1) {
                for y in (s.top() - 2.0).max(0.0) as u32
                    ..=((s.bottom() + 2.0) as u32).min(clean.height() - 1)
                {
                    clean.set(x, y, false);
                }
            }
        }
    }
    let integral = Integral::new(&clean);
    let tpl = templates::load(il);
    // Grace notes: small noteheads and flags.
    let small: Vec<Template> = templates::load_small(il, GRACE_SIZE)
        .into_iter()
        .filter(|t| matches!(t.sym, Sym::Head(Head::Black) | Sym::Flag(..)))
        .collect();
    let by = |pred: &dyn Fn(Sym) -> bool| -> Vec<&Template> {
        tpl.iter().filter(|t| pred(t.sym)).collect()
    };

    let mut staves_out = Vec::new();
    for s in &layout.staves {
        let sp = s.interline();
        let bottom = s.bottom();
        let step_y = |step: i32| bottom - step as f32 * sp / 2.0;
        let (xl, xr) = (i64::from(s.left), i64::from(s.right));
        let mut cands: Vec<Candidate> = Vec::new();

        for t in by(&|s| matches!(s, Sym::Head(_))) {
            // Hollow heads are thin rings and score lower when slightly off.
            // Scanned below the final threshold: a stem at the edge can still
            // lift a head over it (see `head_threshold`).
            let min = head_threshold(t.sym) - STEM_BONUS;
            for step in -8..=16 {
                scan_row(&clean, &integral, t, step_y(step), xl, xr, min, &mut cands);
            }
        }
        for t in small.iter().filter(|t| matches!(t.sym, Sym::Head(_))) {
            for step in -6..=14 {
                scan_row(
                    &clean,
                    &integral,
                    t,
                    step_y(step),
                    xl,
                    xr,
                    head_threshold(t.sym) - STEM_BONUS,
                    &mut cands,
                );
            }
        }
        for t in by(&|s| matches!(s, Sym::Accidental(_))) {
            for step in -6..=14 {
                scan_row(&clean, &integral, t, step_y(step), xl, xr, 0.85, &mut cands);
            }
        }
        for t in by(&|s| matches!(s, Sym::Clef(_))) {
            let step = match t.sym {
                Sym::Clef(scorelift_semantic::smufl::ClefGlyph::F) => 6,
                Sym::Clef(scorelift_semantic::smufl::ClefGlyph::C) => 4,
                _ => 2,
            };
            scan_row(&clean, &integral, t, step_y(step), xl, xr, 0.6, &mut cands);
        }
        for t in by(&|s| matches!(s, Sym::TimeDigit(_))) {
            for step in [6, 2] {
                scan_row(&clean, &integral, t, step_y(step), xl, xr, 0.85, &mut cands);
            }
            // Multi-measure rest counts sit above the staff.
            for step in 10..=13 {
                scan_row(&clean, &integral, t, step_y(step), xl, xr, 0.85, &mut cands);
            }
        }
        for t in by(&|s| matches!(s, Sym::TimeCommon | Sym::TimeCut)) {
            scan_row(&clean, &integral, t, step_y(4), xl, xr, 0.7, &mut cands);
        }
        for t in by(&|s| matches!(s, Sym::Rest(_))) {
            // Whole and half rests hang from or sit on a line.
            let steps: Vec<i32> = match t.sym {
                Sym::Rest(scorelift_model::NoteType::Whole | scorelift_model::NoteType::Half) => {
                    (2..=8).step_by(2).collect()
                }
                _ => (0..=8).collect(),
            };
            // A pixel or two off the line position matters to thin rest shapes.
            for step in steps {
                for dy in [-2.0, -1.0, 0.0, 1.0, 2.0] {
                    let y = step_y(step).round() + dy;
                    scan_row(&clean, &integral, t, y, xl, xr, 0.68, &mut cands);
                }
            }
        }
        for t in by(&|s| matches!(s, Sym::Dynamic(_))) {
            let mut y = s.bottom() + 1.5 * sp;
            while y < s.bottom() + 6.0 * sp {
                scan_row(&clean, &integral, t, y, xl, xr, 0.72, &mut cands);
                y += 1.0;
            }
        }

        // Stems: tall thin strokes, not barlines, attached to a head.
        let barline_at = |x: i64| {
            s.barlines
                .iter()
                .any(|b| x + 2 >= i64::from(b.x0) && x <= i64::from(b.x1) + 2)
        };
        let stems: Vec<(i64, i64, i64, i64)> = find_stems(
            &clean,
            xl,
            xr,
            (s.top() - 6.0 * sp) as i64,
            (s.bottom() + 6.0 * sp) as i64,
            (1.6 * sp) as i64,
            (0.3 * sp).max(3.0) as i64,
        )
        .into_iter()
        .filter(|st| !barline_at(st.0) && !barline_at(st.1))
        .collect();

        // Engraving rules that templates alone cannot see:
        // - a notehead two or more steps outside the staff sits on or next
        //   to ledger lines, one per line position between it and the staff
        //   (text and tempo marks above or below the staff have none);
        // - a whole or half rest stands alone: no ink continues sideways out
        //   of it, as it would from a beam.
        let ledger_at = |step: i32, x0: i64, w: i64| {
            let y = step_y(step).round() as i64;
            let mid = x0 + w / 2;
            (y - 2..=y + 2).any(|r| {
                let run = horizontal_run(&bin, mid, r);
                run as f32 >= 0.9 * w as f32 && run as f32 <= 3.5 * sp
            })
        };
        // Fonts differ in rest width, so the band starts a little away from
        // the template's edges.
        let side_ink = |c: &Candidate| {
            let (gap, pad) = ((0.2 * sp) as i64, (0.5 * sp) as i64);
            let rows = c.y0..c.y0 + c.h;
            let left = rows
                .clone()
                .map(|y| {
                    (c.x0 - gap - pad..c.x0 - gap)
                        .filter(|&x| clean.get(x, y))
                        .count()
                })
                .sum::<usize>();
            let right = rows
                .map(|y| {
                    (c.x0 + c.w + gap..c.x0 + c.w + gap + pad)
                        .filter(|&x| clean.get(x, y))
                        .count()
                })
                .sum::<usize>();
            left + right
        };
        cands.retain(|c| match c.sym {
            Sym::Head(_) => {
                let step = ((bottom - c.origin_y) / (sp / 2.0)).round() as i32;
                let needed: Vec<i32> = if step <= -2 {
                    (step.max(-12)..=-2).filter(|k| k % 2 == 0).collect()
                } else if step >= 10 {
                    (10..=step.min(20)).filter(|k| k % 2 == 0).collect()
                } else {
                    Vec::new()
                };
                needed.iter().all(|&k| ledger_at(k, c.x0, c.w))
            }
            Sym::Rest(scorelift_model::NoteType::Whole | scorelift_model::NoteType::Half) => {
                (side_ink(c) as f32) < 0.1 * (c.w * c.h) as f32
            }
            _ => true,
        });

        // A notehead with a stem at its edge is supported by it; a rest or
        // other shape in the same place is not.
        for c in &mut cands {
            if !matches!(c.sym, Sym::Head(_)) {
                continue;
            }
            let attached = stems.iter().any(|st| {
                let at_edge = (st.0 - c.x0).abs() <= 3 || (st.1 - (c.x0 + c.w - 1)).abs() <= 3;
                let reaches =
                    st.2 as f32 <= c.origin_y + 0.6 * sp && st.3 as f32 >= c.origin_y - 0.6 * sp;
                at_edge && reaches
            });
            if attached {
                c.score += STEM_BONUS;
            }
        }
        cands.retain(|c| !matches!(c.sym, Sym::Head(_)) || c.score >= head_threshold(c.sym));

        // Beams first, from the image with only the stems removed: a false
        // notehead on a beam must not cut the beam short. Candidates lying
        // mostly on beam pixels are not notes, rests, accidentals or dots.
        let mut stem_free = clean.clone();
        erase_stems(&mut stem_free, &stems);
        let mut beam_mask = BinaryImage::new(clean.width(), clean.height());
        let mut beams: Vec<Fill> = Vec::new();
        let region = (
            xl.max(0),
            ((s.top() - 6.0 * sp) as i64).max(0),
            xr,
            (s.bottom() + 6.0 * sp) as i64,
        );
        for comp in components(&stem_free, region.0, region.1, region.2, region.3) {
            if let Some(f) = beam_from(&comp, sp) {
                for &(x, y) in &comp {
                    beam_mask.set(x as u32, y as u32, true);
                }
                beams.push(f);
            }
        }
        cands.retain(|c| {
            if !matches!(
                c.sym,
                Sym::Head(_) | Sym::Rest(_) | Sym::Accidental(_) | Sym::Dot
            ) {
                return true;
            }
            let on_beam = beam_mask.count(c.x0, c.y0, c.x0 + c.w, c.y0 + c.h) as f32;
            on_beam < 0.25 * (c.w * c.h) as f32
        });

        // Multi-measure rests: a thick bar on the middle line with a tall
        // serif at each end. Nothing inside it is a note or a rest.
        let bars = hbars(&clean, s.lines[2], sp, xl, xr);
        cands.retain(|c| {
            !(matches!(
                c.sym,
                Sym::Head(_) | Sym::Rest(_) | Sym::Accidental(_) | Sym::Dot
            ) && bars.iter().any(|&(b0, b1)| {
                c.x0 + c.w > b0 && c.x0 < b1 && (c.origin_y - s.lines[2]).abs() < 2.5 * sp
            }))
        });

        // Keep the best candidate where several overlap.
        cands.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Candidate> = Vec::new();
        for c in cands {
            if kept.iter().all(|k| c.overlap(k) < 0.3) {
                kept.push(c);
            }
        }
        if std::env::var_os("SCORELIFT_TRACE").is_some() {
            for c in &kept {
                eprintln!(
                    "{:?} {:.3} at ({}, {:.0})",
                    c.sym, c.score, c.x0, c.origin_y
                );
            }
        }

        // Vertical strokes inside clefs, time signatures and accidentals are
        // not stems.
        let stems: Vec<(i64, i64, i64, i64)> = stems
            .into_iter()
            .filter(|st| {
                !kept.iter().any(|c| {
                    matches!(
                        c.sym,
                        Sym::Clef(_)
                            | Sym::TimeDigit(_)
                            | Sym::TimeCommon
                            | Sym::TimeCut
                            | Sym::Accidental(_)
                    ) && st.0 >= c.x0 - 1
                        && st.1 <= c.x0 + c.w + 1
                })
            })
            .collect();

        // Flags at stem ends, dots right of heads and rests.
        let mut flag_tpl = by(&|s| matches!(s, Sym::Flag(..)));
        flag_tpl.extend(small.iter().filter(|t| matches!(t.sym, Sym::Flag(..))));
        let dot_tpl = by(&|s| s == Sym::Dot);
        let mut extras: Vec<Candidate> = Vec::new();
        for st in &stems {
            for t in &flag_tpl {
                let up = matches!(t.sym, Sym::Flag(_, true));
                let end = if up { st.2 } else { st.3 } as f32;
                let mut y = end - 0.6 * sp;
                while y <= end + 0.6 * sp {
                    scan_row(
                        &clean,
                        &integral,
                        t,
                        y,
                        st.0 - 2 - t.origin_x as i64,
                        st.1 + 2,
                        0.62,
                        &mut extras,
                    );
                    y += 1.0;
                }
            }
        }
        let anchors: Vec<(i64, f32)> = kept
            .iter()
            .filter(|c| matches!(c.sym, Sym::Head(_) | Sym::Rest(_)))
            .map(|c| (c.x0 + c.w, c.origin_y))
            .collect();
        for (x, y) in anchors {
            // Augmentation dots always sit in a space, never on a line.
            let step = ((bottom - y) / (sp / 2.0)).round() as i32;
            let space = if step.rem_euclid(2) == 1 {
                step
            } else {
                step + 1
            };
            for t in &dot_tpl {
                let cy = step_y(space);
                for dy in -1..=1 {
                    scan_row(
                        &clean,
                        &integral,
                        t,
                        cy + dy as f32,
                        x,
                        x + (2.2 * sp) as i64,
                        0.7,
                        &mut extras,
                    );
                }
            }
        }
        extras.sort_by(|a, b| b.score.total_cmp(&a.score));
        for c in extras {
            if kept.iter().all(|k| c.overlap(k) < 0.3) {
                kept.push(c);
            }
        }

        // Curves and tuplet marks: what is left after removing stems and symbols.
        let mut rest_img = clean.clone();
        erase_stems(&mut rest_img, &stems);
        for c in &kept {
            for y in c.y0..c.y0 + c.h {
                for x in c.x0..c.x0 + c.w {
                    if x >= 0
                        && y >= 0
                        && x < i64::from(rest_img.width())
                        && y < i64::from(rest_img.height())
                    {
                        rest_img.set(x as u32, y as u32, false);
                    }
                }
            }
        }
        let mut fills = beams;
        let mut marks: Vec<(i64, i64, i64, i64)> = Vec::new();
        for comp in components(&rest_img, region.0, region.1, region.2, region.3) {
            let (cx0, cx1) = comp
                .iter()
                .fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
            let (cy0, cy1) = comp
                .iter()
                .fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            let width = cx1 - cx0 + 1;
            let height = cy1 - cy0 + 1;
            // Small compact marks may be tuplet numbers; checked against
            // beam groups below.
            if (width as f32) <= 1.2 * sp
                && (height as f32) >= 0.6 * sp
                && (height as f32) <= 1.6 * sp
            {
                marks.push((cx0, cy0, cx1, cy1));
            }
            if (width as f32) < 1.0 * sp || (cy1 - cy0) as f32 > 3.0 * sp {
                continue;
            }
            let mean_thick = comp.len() as f32 / width as f32;
            let column = |x: i64| -> Option<(f32, f32)> {
                let ys: Vec<i64> = comp.iter().filter(|p| p.0 == x).map(|p| p.1).collect();
                let lo = *ys.iter().min()?;
                let hi = *ys.iter().max()?;
                Some((lo as f32, hi as f32 + 1.0))
            };
            if mean_thick < 0.28 * sp && (cy1 - cy0) as f32 <= 1.5 * sp {
                // A tie or slur: a thin, shallow curve. Its outline is
                // approximated by the centre line of each column.
                let mut points = Vec::new();
                let n = 8;
                for k in 0..=n {
                    let x = cx0 + (width - 1) * k / n;
                    if let Some((lo, hi)) = column(x) {
                        points.push(Point {
                            x: x as f32,
                            y: (lo + hi) / 2.0,
                        });
                    }
                }
                if points.len() >= 3 {
                    fills.push(Fill {
                        points,
                        curved: true,
                        x0: cx0 as f32,
                        x1: (cx1 + 1) as f32,
                        y0: cy0 as f32,
                        y1: (cy1 + 1) as f32,
                    });
                }
            }
        }

        let fills = join_beams(fills, sp);

        // Triplets: a beam over exactly three stems with a small mark
        // centred just beyond it. The digit itself is italic text in most
        // engravings (no SMuFL glyph), so its position identifies it.
        for f in fills.iter().filter(|f| !f.curved) {
            let covered = stems
                .iter()
                .filter(|st| {
                    let x = (st.0 + st.1) as f32 / 2.0;
                    x >= f.x0 - 0.3 * sp
                        && x <= f.x1 + 0.3 * sp
                        && st.2 as f32 <= f.y1 + 0.3 * sp
                        && st.3 as f32 >= f.y0 - 0.3 * sp
                })
                .count();
            if covered != 3 {
                continue;
            }
            let cx = (f.x0 + f.x1) / 2.0;
            // A letter of a lyric under the beam has other letters beside it;
            // a tuplet number stands alone.
            let alone = |m: &(i64, i64, i64, i64)| {
                !marks.iter().any(|o| {
                    o != m
                        && o.1 <= m.3
                        && o.3 >= m.1
                        && ((o.0 - m.2) as f32).max((m.0 - o.2) as f32) < 0.6 * sp
                })
            };
            let mark = marks.iter().find(|m| {
                let mx = (m.0 + m.2) as f32 / 2.0;
                let (my0, my1) = (m.1 as f32, m.3 as f32);
                let above = my1 <= f.y0 && f.y0 - my1 <= 2.5 * sp;
                let below = my0 >= f.y1 && my0 - f.y1 <= 2.5 * sp;
                (mx - cx).abs() <= 0.8 * sp && (above || below) && alone(m)
            });
            if let Some(m) = mark {
                kept.push(Candidate {
                    sym: Sym::TupletDigit(3),
                    score: 1.0,
                    x0: m.0,
                    y0: m.1,
                    w: m.2 - m.0 + 1,
                    h: m.3 - m.1 + 1,
                    origin_y: m.3 as f32,
                    rel: 1.0,
                });
            }
        }

        // A rest never has a stem through it (an eighth rest's own stroke is
        // slanted, so it is not found as a stem): the flag or slash of a grace
        // note is not a rest.
        // A stroke that stays within the rest is the rest's own (some fonts
        // draw the quarter rest with a near-vertical stroke); it is not a
        // stem, and a "notehead" on the rest's curl is not a note.
        let reach = (0.5 * sp) as i64;
        let inside = |st: &(i64, i64, i64, i64), c: &Candidate| {
            st.1 >= c.x0 && st.0 < c.x0 + c.w && st.2 >= c.y0 - reach && st.3 < c.y0 + c.h + reach
        };
        kept.retain(|c| {
            !matches!(c.sym, Sym::Rest(_))
                || !stems.iter().any(|st| {
                    st.1 >= c.x0
                        && st.0 < c.x0 + c.w
                        && st.3 >= c.y0
                        && st.2 < c.y0 + c.h
                        && !inside(st, c)
                })
        });
        let rests: Vec<Candidate> = kept
            .iter()
            .filter(|c| matches!(c.sym, Sym::Rest(_)))
            .cloned()
            .collect();
        let stems: Vec<(i64, i64, i64, i64)> = stems
            .into_iter()
            .filter(|st| !rests.iter().any(|r| inside(st, r)))
            .collect();
        kept.retain(|c| {
            if !matches!(c.sym, Sym::Head(_)) {
                return true;
            }
            let (cx, cy) = (c.x0 + c.w / 2, c.y0 + c.h / 2);
            !rests
                .iter()
                .any(|r| (r.x0..r.x0 + r.w).contains(&cx) && (r.y0..r.y0 + r.h).contains(&cy))
        });

        // Grace notes: a full-size template also fits a smaller notehead, so
        // measure the head's ink across its middle row instead.
        let normal_width = tpl
            .iter()
            .find(|t| t.sym == Sym::Head(Head::Black))
            .map_or(1.2 * sp, |t| t.width as f32);
        for c in kept.iter_mut().filter(|c| c.sym == Sym::Head(Head::Black)) {
            let row = c.origin_y.round() as i64;
            let width = (c.x0 - 2..c.x0 + c.w + 2)
                .filter(|&x| clean.get(x, row))
                .count() as f32;
            if width < 0.82 * normal_width {
                c.rel = GRACE_SIZE;
            }
        }

        let glyphs: Vec<MusicGlyph> = kept
            .iter()
            .map(|c| MusicGlyph {
                sym: c.sym,
                x: c.x0 as f32,
                y: c.origin_y,
                w: c.w as f32,
                rel_size: c.rel,
            })
            .collect();
        let segments: Vec<Segment> = stems
            .iter()
            .map(|st| {
                let x = (st.0 + st.1) as f32 / 2.0 + 0.5;
                Segment {
                    a: Point { x, y: st.2 as f32 },
                    b: Point {
                        x,
                        y: st.3 as f32 + 1.0,
                    },
                    width: (st.1 - st.0 + 1) as f32,
                }
            })
            .collect();
        let bars = s
            .barlines
            .iter()
            .map(|b| {
                let (thin, thick) = (0.12 * sp, 0.5 * sp);
                let widths = match b.style {
                    BarStyle::LightLight => vec![thin, thin],
                    BarStyle::LightHeavy => vec![thin, thick],
                    BarStyle::HeavyLight => vec![thick, thin],
                    BarStyle::Heavy => vec![thick],
                    BarStyle::HeavyHeavy => vec![thick, thick],
                    _ => vec![thin],
                };
                BarGroup {
                    x0: b.x0 as f32,
                    x1: b.x1 as f32 + 1.0,
                    widths,
                    repeat_left: b.repeat_backward,
                    repeat_right: b.repeat_forward,
                }
            })
            .collect::<Vec<_>>();
        staves_out.push((
            Staff {
                lines: s.lines,
                sp,
                x0: s.left as f32,
                x1: s.right as f32,
                glyphs,
                words: Vec::new(),
                segments,
                fills,
            },
            bars,
        ));
    }

    group_systems(&bin, staves_out, page)
}

/// Staves joined by a barline drawn through the gap between them belong to
/// one system; joined at an inner barline (not just the opening line), they
/// belong to one instrument (a braced piano).
fn group_systems(bin: &BinaryImage, staves: Vec<(Staff, Vec<BarGroup>)>, page: u32) -> Vec<System> {
    let joined_at = |a: &Staff, b: &Staff, x: f32| {
        let (y0, y1) = (a.bottom().ceil() as i64 + 1, b.top().floor() as i64 - 1);
        let span = (y1 - y0).max(1);
        (-1..=1).any(|dx| {
            let xi = x.round() as i64 + dx;
            (y0..y1).filter(|&y| bin.get(xi, y)).count() as i64 >= span * 9 / 10
        })
    };
    let mut systems: Vec<System> = Vec::new();
    let mut prev: Option<(Staff, Vec<BarGroup>)> = None;
    for (staff, bars) in staves {
        let link = prev.as_ref().map(|(p, pbars)| {
            let opening = joined_at(p, &staff, p.x0 + 1.0) || joined_at(p, &staff, p.x0);
            let inner = pbars
                .iter()
                .any(|b| b.x0 > p.x0 + 2.0 * p.sp && joined_at(p, &staff, (b.x0 + b.x1) / 2.0));
            (opening || inner, inner)
        });
        match (link, systems.last_mut()) {
            (Some((true, braced)), Some(sys)) => {
                let i = sys.staves.len();
                if braced {
                    match sys.braces.iter_mut().find(|g| g.last() == Some(&(i - 1))) {
                        Some(g) => g.push(i),
                        None => sys.braces.push(vec![i - 1, i]),
                    }
                }
                sys.x0 = sys.x0.min(staff.x0);
                sys.x1 = sys.x1.max(staff.x1);
                for b in &bars {
                    if !sys.bars.iter().any(|o| (o.x0 - b.x0).abs() < 3.0) {
                        sys.bars.push(b.clone());
                    }
                }
                sys.bars.sort_by(|a, b| a.x0.total_cmp(&b.x0));
                sys.staves.push(staff.clone());
            }
            _ => systems.push(System {
                page,
                staves: vec![staff.clone()],
                bars: bars.clone(),
                braces: Vec::new(),
                labels: Vec::new(),
                x0: staff.x0,
                x1: staff.x1,
            }),
        }
        prev = Some((staff, bars));
    }
    systems
}

/// Best score of each template near (x, y), for tuning: (symbol, score, x).
#[doc(hidden)]
pub fn probe(gray: &GrayImage, layout: &Layout, x: i64, y: f32) -> Vec<(Sym, f32, i64)> {
    let bin = binarize(gray, layout.threshold);
    let mut clean = bin.clone();
    for s in &layout.staves {
        for &ly in &s.lines {
            erase_line(
                &mut clean,
                ly,
                s.line_thickness.max(layout.scale.line_thickness),
                s.left,
                s.right,
                |_| false,
            );
        }
    }
    let integral = Integral::new(&clean);
    let mut out = Vec::new();
    for t in templates::load(layout.scale.interline) {
        let mut best = (0.0f32, 0);
        let y0 = (y - t.origin_y).round() as i64;
        for x0 in x - 15..x + 15 {
            if let Some(s) = score(&clean, &integral, &t, x0, y0)
                && s > best.0
            {
                best = (s, x0);
            }
        }
        out.push((t.sym, best.0, best.1));
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

/// Erasing stems cuts each beam into pieces between them. Pieces that
/// continue each other (small gap, same height at the joint) become one
/// beam again, and every beam is extended over the stem columns erased at
/// its ends so that the outer stems still touch it.
fn join_beams(fills: Vec<Fill>, sp: f32) -> Vec<Fill> {
    let (mut beams, curves): (Vec<Fill>, Vec<Fill>) = fills.into_iter().partition(|f| !f.curved);
    beams.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let edge = |f: &Fill, right: bool| -> (f32, f32) {
        // Points are [left top, right top, right bottom, left bottom].
        if right {
            (f.points[1].y, f.points[2].y)
        } else {
            (f.points[0].y, f.points[3].y)
        }
    };
    let mut out: Vec<Fill> = Vec::new();
    for b in beams {
        let joined = out.iter_mut().rev().find(|a| {
            let (at, ab) = edge(a, true);
            let (bt, bb) = edge(&b, false);
            b.x0 >= a.x1 - 1.0
                && b.x0 - a.x1 <= 0.45 * sp
                && (at - bt).abs() <= 0.3 * sp
                && (ab - bb).abs() <= 0.3 * sp
        });
        match joined {
            Some(a) => {
                a.points[1] = b.points[1];
                a.points[2] = b.points[2];
                a.x1 = b.x1;
                a.y0 = a.y0.min(b.y0);
                a.y1 = a.y1.max(b.y1);
            }
            None => out.push(b),
        }
    }
    let pad = 0.2 * sp;
    for f in &mut out {
        f.x0 -= pad;
        f.x1 += pad;
        f.points[0].x -= pad;
        f.points[3].x -= pad;
        f.points[1].x += pad;
        f.points[2].x += pad;
    }
    out.extend(curves);
    out
}

/// Multi-measure rest bars on the middle line: column runs at least 3 staff
/// spaces long that are thick around the line, ending in tall serifs.
fn hbars(img: &BinaryImage, mid: f32, sp: f32, x0: i64, x1: i64) -> Vec<(i64, i64)> {
    let ink_in = |x: i64, ya: f32, yb: f32| {
        (ya.round() as i64..=yb.round() as i64)
            .filter(|&y| img.get(x, y))
            .count() as f32
    };
    let thick = |x: i64| ink_in(x, mid - 0.4 * sp, mid + 0.4 * sp) >= 0.35 * sp;
    // A serif crosses the bar, reaching both above and below it; the stems
    // at the ends of a beam on the middle line go one way only.
    let tall = |x: i64| {
        ink_in(x, mid - 1.2 * sp, mid - 0.5 * sp) >= 0.5 * sp
            && ink_in(x, mid + 0.5 * sp, mid + 1.2 * sp) >= 0.5 * sp
    };
    let mut out = Vec::new();
    let mut x = x0;
    while x < x1 {
        if !thick(x) {
            x += 1;
            continue;
        }
        let start = x;
        while x < x1 && thick(x) {
            x += 1;
        }
        let end = x - 1;
        let serif = |from: i64, to: i64| (from..=to).any(tall);
        let reach = (0.4 * sp) as i64;
        if (end - start) as f32 >= 3.0 * sp
            && serif(start - reach, start + reach)
            && serif(end - reach, end + reach)
        {
            out.push((start - reach, end + reach));
        }
    }
    out
}

/// Erases stem columns (with one pixel on each side).
fn erase_stems(img: &mut BinaryImage, stems: &[(i64, i64, i64, i64)]) {
    for st in stems {
        for x in st.0 - 1..=st.1 + 1 {
            for y in st.2..=st.3 {
                if x >= 0 && y >= 0 && x < i64::from(img.width()) && y < i64::from(img.height()) {
                    img.set(x as u32, y as u32, false);
                }
            }
        }
    }
}

/// A beam: a band at least 1.5 staff spaces long and of beam thickness,
/// returned as a four-point polygon.
fn beam_from(comp: &[(i64, i64)], sp: f32) -> Option<Fill> {
    let (cx0, cx1) = comp
        .iter()
        .fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (cy0, cy1) = comp
        .iter()
        .fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let width = cx1 - cx0 + 1;
    if (width as f32) < 1.5 * sp || (cy1 - cy0) as f32 > 3.0 * sp {
        return None;
    }
    let mean_thick = comp.len() as f32 / width as f32;
    if !(0.28 * sp..=0.9 * sp).contains(&mean_thick) {
        return None;
    }
    // A beam is a straight band: every column one run of about the same
    // thickness, centres on a line. A hollow notehead has two runs per
    // column, a fermata or slur bends, a dynamic letter varies in thickness.
    let mut cols: Vec<Vec<i64>> = vec![Vec::new(); width as usize];
    for &(x, y) in comp {
        cols[(x - cx0) as usize].push(y);
    }
    let mut runs = Vec::with_capacity(cols.len());
    for ys in &mut cols {
        ys.sort_unstable();
        // One run, allowing a one-pixel notch where a staff line was erased
        // along the beam's edge.
        let single = ys.windows(2).all(|w| w[1] - w[0] <= 2);
        runs.push((
            single,
            ys.len() as f32,
            (ys[0] + ys[ys.len() - 1]) as f32 / 2.0,
        ));
    }
    let single = runs.iter().filter(|r| r.0).count() as f32 / runs.len() as f32;
    let mut thick: Vec<f32> = runs.iter().map(|r| r.1).collect();
    thick.sort_by(f32::total_cmp);
    let median = thick[thick.len() / 2];
    let even = runs
        .iter()
        .filter(|r| (r.1 - median).abs() <= 0.3 * median + 1.0)
        .count() as f32
        / runs.len() as f32;
    let (first, last) = (runs[0].2, runs[runs.len() - 1].2);
    let straight = runs.iter().enumerate().all(|(i, r)| {
        let t = i as f32 / (runs.len() - 1).max(1) as f32;
        (r.2 - (first + t * (last - first))).abs() <= 0.2 * sp + 1.0
    });
    if single < 0.9 || even < 0.8 || !straight {
        return None;
    }
    let column = |x: i64| -> Option<(f32, f32)> {
        let ys = comp.iter().filter(|p| p.0 == x).map(|p| p.1);
        let (lo, hi) = ys.fold((i64::MAX, i64::MIN), |(a, b), y| (a.min(y), b.max(y)));
        (lo <= hi).then_some((lo as f32, hi as f32 + 1.0))
    };
    let (l, r) = (column(cx0 + 1)?, column(cx1 - 1)?);
    let (fx0, fx1) = (cx0 as f32, (cx1 + 1) as f32);
    Some(Fill {
        points: vec![
            Point { x: fx0, y: l.0 },
            Point { x: fx1, y: r.0 },
            Point { x: fx1, y: r.1 },
            Point { x: fx0, y: l.1 },
        ],
        curved: false,
        x0: fx0,
        x1: fx1,
        y0: l.0.min(r.0),
        y1: l.1.max(r.1),
    })
}
