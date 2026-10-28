//! Page layout from vector primitives: staves, systems, barlines, and the
//! assignment of glyphs, lines and fills to them.

use scorelift_pdf::{Glyph, PdfPage, Point, Shape};

use crate::smufl::{self, Sym};

/// A music-font glyph.
#[derive(Debug, Clone)]
pub struct MusicGlyph {
    pub sym: Sym,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    /// Font size relative to the staff's normal size (1.0 = 4 staff spaces).
    pub rel_size: f32,
}

impl MusicGlyph {
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
}

/// A run of text glyphs on one baseline with no large gaps or spaces.
#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub font: String,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub size: f32,
}

impl Word {
    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    pub fn italic(&self) -> bool {
        let f = self.font.to_ascii_lowercase();
        f.contains("italic") || f.contains("oblique")
    }

    pub fn bold(&self) -> bool {
        self.font.to_ascii_lowercase().contains("bold")
    }
}

/// A straight stroked segment.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
    pub width: f32,
}

impl Segment {
    pub fn is_horizontal(&self) -> bool {
        (self.a.y - self.b.y).abs() < 0.05 && (self.a.x - self.b.x).abs() > 0.05
    }

    pub fn is_vertical(&self) -> bool {
        (self.a.x - self.b.x).abs() < 0.05 && (self.a.y - self.b.y).abs() > 0.05
    }

    pub fn x0(&self) -> f32 {
        self.a.x.min(self.b.x)
    }

    pub fn x1(&self) -> f32 {
        self.a.x.max(self.b.x)
    }

    pub fn y0(&self) -> f32 {
        self.a.y.min(self.b.y)
    }

    pub fn y1(&self) -> f32 {
        self.a.y.max(self.b.y)
    }

    pub fn len(&self) -> f32 {
        ((self.a.x - self.b.x).powi(2) + (self.a.y - self.b.y).powi(2)).sqrt()
    }
}

/// A filled area: beams (straight polygons), ties and slurs (curved).
#[derive(Debug, Clone)]
pub struct Fill {
    pub points: Vec<Point>,
    pub curved: bool,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

impl Fill {
    /// Vertical extent of the fill at `x` (for slanted beams).
    pub fn y_range_at(&self, x: f32) -> Option<(f32, f32)> {
        let mut ys = Vec::new();
        let n = self.points.len();
        for i in 0..n {
            let (p, q) = (self.points[i], self.points[(i + 1) % n]);
            let (lo, hi) = (p.x.min(q.x), p.x.max(q.x));
            if x < lo || x > hi || (hi - lo).abs() < 1e-4 {
                continue;
            }
            let t = (x - p.x) / (q.x - p.x);
            ys.push(p.y + t * (q.y - p.y));
        }
        let lo = ys.iter().copied().fold(f32::MAX, f32::min);
        let hi = ys.iter().copied().fold(f32::MIN, f32::max);
        (lo <= hi).then_some((lo, hi))
    }
}

#[derive(Debug, Clone)]
pub struct Staff {
    /// Line y positions, top to bottom.
    pub lines: [f32; 5],
    /// Staff space.
    pub sp: f32,
    pub x0: f32,
    pub x1: f32,
    // Everything that belongs to this staff, by vertical zone.
    pub glyphs: Vec<MusicGlyph>,
    pub words: Vec<Word>,
    pub segments: Vec<Segment>,
    pub fills: Vec<Fill>,
}

impl Staff {
    pub fn top(&self) -> f32 {
        self.lines[0]
    }

    pub fn bottom(&self) -> f32 {
        self.lines[4]
    }

    pub fn middle(&self) -> f32 {
        self.lines[2]
    }

    /// Staff position of `y` in half spaces: 0 = bottom line, 8 = top line.
    pub fn step(&self, y: f32) -> i32 {
        ((self.bottom() - y) / (self.sp / 2.0)).round() as i32
    }
}

/// A group of barline strokes at one place: single, double, final, repeat.
#[derive(Debug, Clone)]
pub struct BarGroup {
    pub x0: f32,
    pub x1: f32,
    /// Stroke widths from left to right.
    pub widths: Vec<f32>,
    pub repeat_left: bool,
    pub repeat_right: bool,
}

#[derive(Debug, Clone)]
pub struct System {
    pub page: u32,
    pub staves: Vec<Staff>,
    pub bars: Vec<BarGroup>,
    /// Groups of staff indices joined by a brace (piano-style parts).
    pub braces: Vec<Vec<usize>>,
    /// Words to the left of the staves (part names).
    pub labels: Vec<Word>,
    pub x0: f32,
    pub x1: f32,
}

/// Page content that is not inside any system: titles, credits, footers.
#[derive(Debug, Clone, Default)]
pub struct PageText {
    pub page: u32,
    pub width: f32,
    pub height: f32,
    pub words: Vec<Word>,
}

pub fn has_music_font(page: &PdfPage) -> bool {
    page.glyphs
        .iter()
        .filter(|g| g.text.chars().next().is_some_and(smufl::is_music))
        .count()
        >= 3
}

fn segments(shapes: &[Shape]) -> (Vec<Segment>, Vec<Fill>) {
    let mut segs = Vec::new();
    let mut fills = Vec::new();
    for s in shapes {
        if s.filled {
            for sp in &s.subpaths {
                let mut pts = sp.clone();
                if pts.len() > 1 && pts.first() == pts.last() {
                    pts.pop();
                }
                if pts.len() < 3 {
                    continue;
                }
                let (x0, x1) = pts
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
                let (y0, y1) = pts
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
                fills.push(Fill {
                    points: pts,
                    curved: s.curved,
                    x0,
                    x1,
                    y0,
                    y1,
                });
            }
        } else if let Some(width) = s.stroke {
            for sp in &s.subpaths {
                for w in sp.windows(2) {
                    let seg = Segment {
                        a: w[0],
                        b: w[1],
                        width,
                    };
                    if seg.len() > 0.01 {
                        segs.push(seg);
                    }
                }
            }
        }
    }
    (segs, fills)
}

/// Joins collinear horizontal segments of equal width that touch.
fn merge_horizontal(mut lines: Vec<Segment>) -> Vec<Segment> {
    lines.sort_by(|a, b| a.a.y.total_cmp(&b.a.y).then(a.x0().total_cmp(&b.x0())));
    let mut out: Vec<Segment> = Vec::new();
    for l in lines {
        let l = Segment {
            a: Point {
                x: l.x0(),
                y: l.a.y,
            },
            b: Point {
                x: l.x1(),
                y: l.a.y,
            },
            width: l.width,
        };
        if let Some(last) = out.last_mut()
            && (last.a.y - l.a.y).abs() < 0.05
            && (last.width - l.width).abs() < 0.1 * last.width.max(0.01)
            && l.x0() <= last.x1() + 1.0
        {
            last.b.x = last.b.x.max(l.x1());
            continue;
        }
        out.push(l);
    }
    out
}

/// Joins collinear vertical segments of equal width that touch: some
/// programs draw one system barline as a segment per gap between staves.
fn merge_vertical(mut lines: Vec<Segment>) -> Vec<Segment> {
    lines.sort_by(|a, b| a.a.x.total_cmp(&b.a.x).then(a.y0().total_cmp(&b.y0())));
    let mut out: Vec<Segment> = Vec::new();
    for l in lines {
        let l = Segment {
            a: Point {
                x: l.a.x,
                y: l.y0(),
            },
            b: Point {
                x: l.a.x,
                y: l.y1(),
            },
            width: l.width,
        };
        if let Some(last) = out.last_mut()
            && (last.a.x - l.a.x).abs() < 0.05
            && (last.width - l.width).abs() < 0.1 * last.width.max(0.01)
            && l.y0() <= last.y1() + 0.5
        {
            last.b.y = last.b.y.max(l.y1());
            continue;
        }
        out.push(l);
    }
    out
}

fn find_staves(horizontal: &[Segment]) -> Vec<Staff> {
    let mut used = vec![false; horizontal.len()];
    let mut staves = Vec::new();
    for i in 0..horizontal.len() {
        if used[i] || horizontal[i].len() < 20.0 {
            continue;
        }
        let first = horizontal[i];
        let overlaps = |s: &Segment| {
            let ov = s.x1().min(first.x1()) - s.x0().max(first.x0());
            ov >= 0.9 * first.len().min(s.len())
                && (s.len() - first.len()).abs() < 0.1 * first.len()
        };
        // Candidates below with the same extent and width.
        let below: Vec<usize> = (i + 1..horizontal.len())
            .filter(|&j| {
                !used[j]
                    && overlaps(&horizontal[j])
                    && (horizontal[j].width - first.width).abs() < 0.2 * first.width.max(0.01)
                    && horizontal[j].a.y - first.a.y < 40.0
            })
            .collect();
        let Some(&second) = below.first() else {
            continue;
        };
        let sp = horizontal[second].a.y - first.a.y;
        if !(1.5..=15.0).contains(&sp) {
            continue;
        }
        let mut chain = vec![i, second];
        for &j in &below[1..] {
            let expected = first.a.y + sp * chain.len() as f32;
            if (horizontal[j].a.y - expected).abs() < 0.05 * sp {
                chain.push(j);
                if chain.len() == 5 {
                    break;
                }
            }
        }
        if chain.len() < 5 {
            continue;
        }
        for &j in &chain {
            used[j] = true;
        }
        let lines = [0, 1, 2, 3, 4].map(|k| horizontal[chain[k]].a.y);
        let x0 = chain
            .iter()
            .map(|&k| horizontal[k].x0())
            .fold(f32::MAX, f32::min);
        let x1 = chain
            .iter()
            .map(|&k| horizontal[k].x1())
            .fold(f32::MIN, f32::max);
        staves.push(Staff {
            lines,
            sp: (lines[4] - lines[0]) / 4.0,
            x0,
            x1,
            glyphs: Vec::new(),
            words: Vec::new(),
            segments: Vec::new(),
            fills: Vec::new(),
        });
    }
    staves.sort_by(|a, b| a.top().total_cmp(&b.top()));
    staves
}

pub fn words(glyphs: &[&Glyph]) -> Vec<Word> {
    let mut sorted: Vec<&&Glyph> = glyphs.iter().collect();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    let mut out: Vec<Word> = Vec::new();
    for g in sorted {
        let ch = g.text.chars().next().unwrap_or(' ');
        let is_space = g.text.is_empty() || ch.is_whitespace() || ch.is_control();
        if let Some(w) = out.last_mut()
            && w.font == g.font
            && (w.y - g.y).abs() < 0.1
            && (w.size - g.size).abs() < 0.05 * w.size
            && g.x - w.x1 < 0.15 * g.size
            && g.x >= w.x0
            && !w.text.ends_with(' ')
        {
            if is_space {
                w.text.push(' ');
            } else {
                w.text.push_str(&g.text);
                w.x1 = g.x + g.width;
            }
            continue;
        }
        if is_space {
            continue;
        }
        out.push(Word {
            text: g.text.clone(),
            font: g.font.clone(),
            x0: g.x,
            x1: g.x + g.width,
            y: g.y,
            size: g.size,
        });
    }
    for w in &mut out {
        w.text = w.text.trim_end().to_string();
    }
    out
}

/// Splits the page into systems, and everything else into `PageText`.
pub fn layout(page: &PdfPage) -> (Vec<System>, PageText) {
    let (segs, fills) = segments(&page.shapes);
    let horizontal = merge_horizontal(
        segs.iter()
            .copied()
            .filter(Segment::is_horizontal)
            .collect(),
    );
    let staves = find_staves(&horizontal);
    let mut page_text = PageText {
        page: page.number,
        width: page.width,
        height: page.height,
        words: Vec::new(),
    };
    let text_glyphs: Vec<&Glyph> = page
        .glyphs
        .iter()
        .filter(|g| !g.text.chars().next().is_some_and(smufl::is_music))
        .collect();
    let all_words = words(&text_glyphs);
    if staves.is_empty() {
        page_text.words = all_words;
        return (Vec::new(), page_text);
    }

    let vertical = merge_vertical(segs.iter().copied().filter(Segment::is_vertical).collect());
    let music: Vec<(MusicGlyph, f32)> = page
        .glyphs
        .iter()
        .filter_map(|g| {
            let sym = smufl::classify(g.text.chars().next()?)?;
            Some((
                MusicGlyph {
                    sym,
                    x: g.x,
                    y: g.y,
                    w: g.width,
                    rel_size: 0.0,
                },
                g.size,
            ))
        })
        .collect();

    // Union staves into systems: barlines or braces spanning several staves.
    let n = staves.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let spans = |y0: f32, y1: f32| -> Vec<usize> {
        staves
            .iter()
            .enumerate()
            .filter(|(_, s)| y0 <= s.top() + 0.5 * s.sp && y1 >= s.bottom() - 0.5 * s.sp)
            .map(|(i, _)| i)
            .collect()
    };
    let mut joins: Vec<Vec<usize>> = Vec::new();
    for v in &vertical {
        let covered: Vec<usize> = spans(v.y0(), v.y1())
            .into_iter()
            .filter(|&i| {
                v.a.x >= staves[i].x0 - staves[i].sp && v.a.x <= staves[i].x1 + staves[i].sp
            })
            .collect();
        if covered.len() > 1 {
            joins.push(covered);
        }
    }
    let mut braces: Vec<Vec<usize>> = Vec::new();
    for (g, size) in &music {
        if matches!(g.sym, Sym::Brace | Sym::Bracket) {
            // The glyph's origin is at its bottom; its height scales with the size.
            let covered: Vec<usize> = staves
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    let c = (s.top() + s.bottom()) / 2.0;
                    c <= g.y && c >= g.y - size && (g.x - s.x0).abs() < 4.0 * s.sp
                })
                .map(|(i, _)| i)
                .collect();
            if covered.len() > 1 {
                if g.sym == Sym::Brace {
                    braces.push(covered.clone());
                }
                joins.push(covered);
            }
        }
    }
    for j in &joins {
        for w in j.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[a] = b;
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        match groups.iter_mut().find(|g| find(&mut parent, g[0]) == root) {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    groups.sort_by(|a, b| staves[a[0]].top().total_cmp(&staves[b[0]].top()));

    // Vertical zones: each staff owns the space halfway to its neighbours.
    let order: Vec<usize> = (0..n).collect();
    let zone = |i: usize| -> (f32, f32) {
        let s = &staves[order[i]];
        // Above the first staff on the page, only the nearest text belongs
        // to it; further up are titles and credits.
        let lo = if i == 0 {
            s.top() - 5.5 * s.sp
        } else {
            (staves[i - 1].bottom() + s.top()) / 2.0
        };
        let hi = if i + 1 == n {
            s.bottom() + 12.0 * s.sp
        } else {
            (s.bottom() + staves[i + 1].top()) / 2.0
        };
        (lo, hi)
    };
    let zones: Vec<(f32, f32)> = (0..n).map(zone).collect();
    let mut staves = staves;
    let staff_of = |x: f32, y: f32, staves: &[Staff]| -> Option<usize> {
        (0..n).find(|&i| {
            let s = &staves[i];
            y >= zones[i].0 && y < zones[i].1 && x >= s.x0 - 6.0 * s.sp && x <= s.x1 + 2.0 * s.sp
        })
    };

    for (g, size) in music {
        if let Some(i) = staff_of(g.x, g.y, &staves) {
            let sp = staves[i].sp;
            staves[i].glyphs.push(MusicGlyph {
                rel_size: size / (4.0 * sp),
                ..g
            });
        }
    }
    let mut leftover = Vec::new();
    for w in all_words {
        match staff_of(w.x0, w.y, &staves) {
            Some(i) if w.x0 >= staves[i].x0 - 0.5 * staves[i].sp => staves[i].words.push(w),
            _ => leftover.push(w),
        }
    }
    for s in segs {
        let (x, y) = ((s.a.x + s.b.x) / 2.0, (s.a.y + s.b.y) / 2.0);
        // Staff lines themselves are not needed any more.
        if s.is_horizontal()
            && staves.iter().any(|st| {
                st.lines.iter().any(|&l| (l - s.a.y).abs() < 0.05)
                    && s.len() > 4.0 * st.sp
                    && s.width < 0.15 * st.sp
            })
        {
            continue;
        }
        if let Some(i) = staff_of(x, y, &staves) {
            staves[i].segments.push(s);
        }
    }
    for f in fills {
        if let Some(i) = staff_of((f.x0 + f.x1) / 2.0, (f.y0 + f.y1) / 2.0, &staves) {
            staves[i].fills.push(f);
        }
    }

    // Dynamics between the staves of one instrument (a piano) belong to the
    // upper staff, even when they sit slightly closer to the lower one.
    for b in &braces {
        for w in b.windows(2) {
            let (upper, lower) = (w[0], w[1]);
            let top = staves[lower].top();
            let moved: Vec<MusicGlyph> = staves[lower]
                .glyphs
                .iter()
                .filter(|g| matches!(g.sym, Sym::Dynamic(_)) && g.y < top)
                .cloned()
                .collect();
            staves[lower]
                .glyphs
                .retain(|g| !(matches!(g.sym, Sym::Dynamic(_)) && g.y < top));
            staves[upper].glyphs.extend(moved);
        }
    }

    let mut systems = Vec::new();
    for g in &groups {
        let members: Vec<Staff> = g.iter().map(|&i| staves[i].clone()).collect();
        let x0 = members.iter().map(|s| s.x0).fold(f32::MAX, f32::min);
        let x1 = members.iter().map(|s| s.x1).fold(f32::MIN, f32::max);
        let local = |i: usize| g.iter().position(|&k| k == i);
        let sys_braces: Vec<Vec<usize>> = braces
            .iter()
            .filter_map(|b| b.iter().map(|&i| local(i)).collect::<Option<Vec<_>>>())
            .collect();
        let top = members.first().unwrap().top();
        let bottom = members.last().unwrap().bottom();
        let sp = members[0].sp;
        let labels: Vec<Word> = leftover
            .iter()
            .filter(|w| w.x1 < x0 && w.y > top - 2.0 * sp && w.y < bottom + 2.0 * sp)
            .cloned()
            .collect();
        let bars = bar_groups(&members, &vertical);
        systems.push(System {
            page: page.number,
            staves: members,
            bars,
            braces: sys_braces,
            labels,
            x0,
            x1,
        });
    }
    let in_label = |w: &Word| {
        systems
            .iter()
            .any(|s| s.labels.iter().any(|l| l.x0 == w.x0 && l.y == w.y))
    };
    page_text.words = leftover.into_iter().filter(|w| !in_label(w)).collect();
    (systems, page_text)
}

/// Vertical strokes that span a whole staff and have no notehead at their
/// ends, grouped when they are closer than a staff space.
fn bar_groups(staves: &[Staff], vertical: &[Segment]) -> Vec<BarGroup> {
    let mut strokes: Vec<(f32, f32)> = Vec::new();
    for s in staves {
        let sp = s.sp;
        for v in vertical {
            let x = v.a.x;
            if x < s.x0 - 0.5 * sp || x > s.x1 + 0.5 * sp {
                continue;
            }
            if v.y0() > s.top() + 0.2 * sp || v.y1() < s.bottom() - 0.2 * sp {
                continue;
            }
            // Ends must be at this staff's outer lines or at another staff of the system.
            let at_edge = |y: f32| {
                staves
                    .iter()
                    .any(|o| (y - o.top()).abs() < 0.5 * sp || (y - o.bottom()).abs() < 0.5 * sp)
            };
            if !at_edge(v.y0()) || !at_edge(v.y1()) {
                continue;
            }
            let head_at_end = s.glyphs.iter().any(|g| {
                matches!(g.sym, Sym::Head(_))
                    && x >= g.x - 0.3 * sp
                    && x <= g.x + g.w + 0.3 * sp
                    && ((g.y - v.y0()).abs() < 0.7 * sp || (g.y - v.y1()).abs() < 0.7 * sp)
            });
            if head_at_end {
                continue;
            }
            if !strokes.iter().any(|&(sx, _)| (sx - x).abs() < 0.05) {
                strokes.push((x, v.width));
            }
        }
    }
    strokes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let sp = staves[0].sp;
    let mut groups: Vec<BarGroup> = Vec::new();
    for (x, w) in strokes {
        match groups.last_mut() {
            Some(g) if x - w / 2.0 - g.x1 < 1.0 * sp => {
                g.x1 = x + w / 2.0;
                g.widths.push(w);
            }
            _ => groups.push(BarGroup {
                x0: x - w / 2.0,
                x1: x + w / 2.0,
                widths: vec![w],
                repeat_left: false,
                repeat_right: false,
            }),
        }
    }
    for g in &mut groups {
        let dots: Vec<&MusicGlyph> = staves
            .iter()
            .flat_map(|s| s.glyphs.iter())
            .filter(|d| d.sym == Sym::RepeatDot)
            .collect();
        g.repeat_left = dots
            .iter()
            .any(|d| d.x + d.w <= g.x0 + 0.1 && d.x + d.w > g.x0 - 1.2 * sp);
        g.repeat_right = dots
            .iter()
            .any(|d| d.x >= g.x1 - 0.1 && d.x < g.x1 + 1.2 * sp);
    }
    groups
}
