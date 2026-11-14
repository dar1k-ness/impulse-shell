//! Port of Qt Quick Layouts (QQuickLinearLayout over QGridLayoutEngine, snapToPixelGrid=true).
#![allow(clippy::needless_range_loop)]
//! Sources: qtbase/src/gui/util/qgridlayoutengine.cpp, qtdeclarative/src/quicklayouts/*.

pub const FLT_MAX: f64 = f32::MAX as f64;
const LAYOUTITEMSIZE_MAX: f64 = (1 << 24) as f64;

bits::bit_flags! {
pub const GROW: u8 = 1;
pub const EXPAND: u8 = 2;
pub const SHRINK: u8 = 4;
pub const IGNORE: u8 = 8;
}
pub const FIXED: u8 = 0;
pub const PREFERRED: u8 = GROW | SHRINK;

bits::bit_flags! {
pub const ALIGN_RIGHT: u32 = 0x2;
pub const ALIGN_HCENTER: u32 = 0x4;
pub const ALIGN_BOTTOM: u32 = 0x40;
pub const ALIGN_VCENTER: u32 = 0x80;
}
const H_MASK: u32 = 0x1f;
const V_MASK: u32 = 0x1e0;

const MIN: usize = 0;
const PREF: usize = 1;
const MAX: usize = 2;

fn qround(f: f64) -> f64 {
    (f + 0.5).floor()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GBox {
    pub s: [f64; 3],
}

impl Default for GBox {
    fn default() -> Self {
        GBox {
            s: [0.0, 0.0, FLT_MAX],
        }
    }
}

impl GBox {
    fn add(&mut self, o: &GBox, stretch: i32, spacing: f64) {
        self.s[MIN] += o.s[MIN] + spacing;
        self.s[PREF] += o.s[PREF] + spacing;
        self.s[MAX] += (if stretch == 0 { o.s[PREF] } else { o.s[MAX] }) + spacing;
    }

    fn combine(&mut self, o: &GBox) {
        self.s[MIN] = self.s[MIN].max(o.s[MIN]);
        let max_max = if self.s[MAX] == FLT_MAX && o.s[MAX] != FLT_MAX {
            o.s[MAX]
        } else if o.s[MAX] == FLT_MAX && self.s[MAX] != FLT_MAX {
            self.s[MAX]
        } else {
            self.s[MAX].max(o.s[MAX])
        };
        self.s[MAX] = self.s[MIN].max(max_max);
        self.s[PREF] = self.s[PREF]
            .max(o.s[PREF])
            .max(self.s[MIN])
            .min(self.s[MAX]);
    }
}

/// One axis of the grid (QGridLayoutRowData).
#[derive(Default, Debug)]
pub struct RowData {
    pub ignore: Vec<bool>,
    pub boxes: Vec<GBox>,
    pub stretches: Vec<i32>,
    pub spacings: Vec<f64>,
    pub has_ignore_flag: bool,
}

fn growth_below_pref(desired: f64, avail: f64, sum_desired: f64) -> f64 {
    desired * (avail / sum_desired).powf(desired / sum_desired)
}

impl RowData {
    pub fn new(n: usize) -> Self {
        RowData {
            ignore: vec![false; n],
            boxes: vec![GBox::default(); n],
            stretches: vec![0; n],
            spacings: vec![0.0; n],
            has_ignore_flag: false,
        }
    }

    pub fn total_box(&self, start: usize, end: usize) -> GBox {
        let mut r = GBox::default();
        if start < end {
            r.s[MAX] = 0.0;
            let mut next = 0.0;
            for i in start..end {
                if self.ignore[i] {
                    continue;
                }
                r.add(&self.boxes[i], self.stretches[i], next);
                next = self.spacings[i];
            }
        }
        r
    }

    fn steal_box(
        &self,
        start: usize,
        end: usize,
        which: usize,
        pos: &mut [f64],
        sizes: &mut [f64],
    ) {
        let mut offset = 0.0;
        let mut next = 0.0;
        for (k, i) in (start..end).enumerate() {
            let mut avail = 0.0;
            if !self.ignore[i] {
                avail = self.boxes[i].s[which];
                offset += next;
                next = self.spacings[i];
            }
            pos[k] = offset;
            sizes[k] = avail;
            offset += avail;
        }
    }

    /// `info_boxes`: per-row user hints (QGridLayoutRowInfo::boxes); empty for Row/ColumnLayout.
    #[allow(clippy::too_many_arguments)]
    pub fn calculate_geometries(
        &self,
        start: usize,
        end: usize,
        target: f64,
        pos: &mut [f64],
        sizes: &mut [f64],
        total: &GBox,
        info_boxes: &[GBox],
        snap: bool,
    ) {
        let n = end - start;
        let target = target.max(total.s[MIN]);
        let mut new_sizes = vec![0.0; n];
        let mut factors = vec![0.0; n];
        let mut sum_factors = 0.0;
        let sum_stretches: i32 = (0..n)
            .map(|i| self.stretches[start + i])
            .filter(|&s| s > 0)
            .sum();
        let sum_available;
        let info = |i: usize| info_boxes.get(i).copied().unwrap_or_default();

        if target < total.s[PREF] {
            self.steal_box(start, end, MIN, pos, sizes);
            sum_available = target - total.s[MIN];
            if sum_available > 0.0 {
                let total_pref = total.s[PREF].min(LAYOUTITEMSIZE_MAX);
                let sum_desired = total_pref - total.s[MIN];
                for i in 0..n {
                    if self.ignore[start + i] {
                        factors[i] = 0.0;
                        continue;
                    }
                    let b = &self.boxes[start + i];
                    let desired = b.s[PREF].min(LAYOUTITEMSIZE_MAX) - b.s[MIN];
                    factors[i] = growth_below_pref(desired, sum_available, sum_desired);
                    sum_factors += factors[i];
                }
                for i in 0..n {
                    new_sizes[i] = sizes[i] + sum_available * factors[i] / sum_factors;
                }
            }
        } else {
            let larger = target > total.s[MAX];
            if larger {
                self.steal_box(start, end, MAX, pos, sizes);
                sum_available = target - total.s[MAX];
            } else {
                self.steal_box(start, end, PREF, pos, sizes);
                sum_available = target - total.s[PREF];
            }
            if sum_available > 0.0 {
                let mut cur = sum_available;
                let mut something_max = false;
                let sum_sizes: f64 = sizes[..n].iter().sum();
                for i in 0..n {
                    if self.ignore[start + i] {
                        new_sizes[i] = 0.0;
                        factors[i] = 0.0;
                        continue;
                    }
                    let b = &self.boxes[start + i];
                    let (box_size, desired) = if larger {
                        (b.s[MAX], info(start + i).s[MAX] - b.s[MAX])
                    } else {
                        (b.s[PREF], b.s[MAX] - b.s[PREF])
                    };
                    if desired == 0.0 {
                        new_sizes[i] = sizes[i];
                        factors[i] = 0.0;
                        continue;
                    }
                    let stretch = self.stretches[start + i];
                    factors[i] = if sum_stretches == 0 {
                        if self.has_ignore_flag || sizes[i] == 0.0 {
                            if stretch < 0 { 1.0 } else { 0.0 }
                        } else if stretch < 0 {
                            sizes[i]
                        } else {
                            0.0
                        }
                    } else if stretch == sum_stretches {
                        1.0
                    } else if stretch <= 0 {
                        0.0
                    } else {
                        let (st, ss) = (stretch as f64, sum_stretches as f64);
                        let x = (st * sum_sizes - ss * box_size) / (ss - st);
                        let (mut us, mut uss) = if x >= 0.0 {
                            (box_size + x, sum_sizes + x)
                        } else {
                            (box_size, ss * box_size / st)
                        };
                        us = us * 3.0 / 2.0;
                        uss = uss * 3.0 / 2.0;
                        let beta = uss - sum_sizes;
                        if beta.abs() <= 1e-12 {
                            1.0
                        } else {
                            let alpha = cur.min(beta);
                            let ultimate = st * uss / ss - box_size;
                            let transitional = cur * (us - box_size) / beta;
                            (alpha * ultimate + (beta - alpha) * transitional) / beta
                        }
                    };
                    sum_factors += factors[i];
                    if desired < cur {
                        something_max = true;
                    }
                    new_sizes[i] = -1.0;
                }
                let mut keep = something_max;
                while keep {
                    keep = false;
                    for i in 0..n {
                        if new_sizes[i] >= 0.0 {
                            continue;
                        }
                        let b = if larger {
                            info(start + i)
                        } else {
                            self.boxes[start + i]
                        };
                        let mut max_box = b.s[MAX];
                        if snap {
                            max_box = b.s[MIN].max(max_box.floor());
                        }
                        let avail = cur * factors[i] / sum_factors;
                        if sizes[i] + avail >= max_box {
                            new_sizes[i] = max_box;
                            cur -= max_box - sizes[i];
                            sum_factors -= factors[i];
                            keep = cur > 0.0;
                            if !keep {
                                break;
                            }
                        }
                    }
                }
                for i in 0..n {
                    if new_sizes[i] < 0.0 {
                        let d = if sum_factors == 0.0 {
                            0.0
                        } else {
                            cur * factors[i] / sum_factors
                        };
                        new_sizes[i] = sizes[i] + d;
                    }
                }
            }
        }
        if sum_available > 0.0 {
            let mut offset = 0.0;
            for i in 0..n {
                let d = new_sizes[i] - sizes[i];
                pos[i] += offset;
                sizes[i] += d;
                offset += d;
            }
        }
        if snap {
            for i in 0..n {
                let old = pos[i];
                pos[i] = qround(old);
                let d = pos[i] - old;
                sizes[i] -= d;
                if i > 0 {
                    sizes[i - 1] += d;
                }
            }
            sizes[n - 1] = target - pos[n - 1];
            for i in 0..n {
                sizes[i] = self.boxes[start + i].s[MIN].max(qround(sizes[i]));
            }
        }
    }
}

/// QQuickLayoutAttached-derived inputs for one child item.
#[derive(Clone, Debug)]
pub struct Child {
    /// Effective [min, pref, max] hints, `[w, h]` each (output of `effective_size_hints`).
    pub hints: [[f64; 2]; 3],
    pub policy: [u8; 2],
    /// Layout.horizontal/verticalStretchFactor, -1 if unset.
    pub stretch: [i32; 2],
    pub align: u32,
    /// Layout margins [l, t, r, b].
    pub margins: [f64; 4],
}

impl Child {
    fn stretch_factor(&self, o: usize) -> i32 {
        if self.stretch[o] >= 0 {
            return self.stretch[o];
        }
        let p = self.policy[o];
        if p & EXPAND != 0 {
            1
        } else if p & GROW != 0 {
            -1
        } else {
            0
        }
    }

    fn gbox(&self, o: usize) -> GBox {
        let p = self.policy[o];
        let pref = self.hints[PREF][o];
        let mut min = if p & SHRINK != 0 {
            self.hints[MIN][o]
        } else {
            pref
        };
        min = min.ceil();
        let max = if p & (GROW | EXPAND) != 0 {
            self.hints[MAX][o]
        } else {
            pref
        };
        GBox {
            s: [min, if p & IGNORE != 0 { min } else { pref }, max],
        }
    }

    fn effective_max(&self) -> [f64; 2] {
        let mut s = [-1.0, -1.0];
        for o in 0..2 {
            if self.policy[o] & GROW == 0 {
                s[o] = self.hints[PREF][o];
            }
        }
        for o in 0..2 {
            if s[o] == -1.0 {
                s[o] = self.hints[MAX][o];
            }
        }
        s
    }

    /// QGridLayoutItem::geometryWithin (no baseline / dynamic constraints).
    fn geometry_within(&self, x: f64, y: f64, cw: f64, ch: f64, align: u32) -> [f64; 4] {
        let m = self.effective_max();
        let (w, h) = (m[0].min(cw), m[1].min(ch));
        let mut x = x;
        let mut y = y;
        match align & H_MASK {
            ALIGN_HCENTER => x += (cw - w) / 2.0,
            ALIGN_RIGHT => x += cw - w,
            _ => {}
        }
        match align & V_MASK {
            ALIGN_VCENTER => y += (ch - h) / 2.0,
            ALIGN_BOTTOM => y += ch - h,
            _ => {}
        }
        [x, y, w, h]
    }
}

/// Explicitly set Layout.* values (None = not set) plus item geometry, for `effective_size_hints`.
#[derive(Clone, Debug, Default)]
pub struct Attached {
    pub min: [Option<f64>; 2],
    pub pref: [Option<f64>; 2],
    pub max: [Option<f64>; 2],
    /// Implicit min/max (set on nested layouts from their own size hints).
    pub implicit_min: [f64; 2],
    pub implicit_max: [f64; 2],
    pub margins: [f64; 4],
}

impl Attached {
    pub fn new() -> Self {
        Attached {
            implicit_max: [f64::INFINITY; 2],
            ..Default::default()
        }
    }
}

fn normalize_hints(min: &mut f64, pref: &mut f64, max: &mut f64) {
    if *min >= 0.0 && *max >= 0.0 && *min > *max {
        *min = *max;
    }
    if *pref >= 0.0 {
        if *min >= 0.0 && *pref < *min {
            *pref = *min;
        } else if *max >= 0.0 && *pref > *max {
            *pref = *max;
        }
    }
}

/// QQuickLayout::effectiveSizeHints_helper. `fallback` = item width/height captured the first time.
pub fn effective_size_hints(a: &Attached, implicit: [f64; 2], fallback: [f64; 2]) -> [[f64; 2]; 3] {
    let mut h = [[-1.0f64; 2]; 3];
    for o in 0..2 {
        if let Some(v) = a.min[o] {
            h[MIN][o] = v;
        }
        if let Some(v) = a.pref[o] {
            h[PREF][o] = v;
        }
        if let Some(v) = a.max[o] {
            h[MAX][o] = v;
        }
        let [mn, pf, mx] = &mut h;
        normalize_hints(&mut mn[o], &mut pf[o], &mut mx[o]);
    }
    for o in 0..2 {
        if a.max[o].is_none() && h[MAX][o] < 0.0 {
            h[MAX][o] = a.implicit_max[o];
        }
        if h[MAX][o] < 0.0 {
            h[MAX][o] = f64::INFINITY;
        }
        for k in [PREF, MIN] {
            if h[k][o] >= 0.0 && h[k][o] > h[MAX][o] {
                h[MAX][o] = h[k][o];
            }
        }
        if a.min[o].is_none() && h[MIN][o] < 0.0 {
            h[MIN][o] = a.implicit_min[o];
        }
        if h[MIN][o] < 0.0 {
            h[MIN][o] = 0.0;
        }
        for k in [PREF, MAX] {
            if h[k][o] >= 0.0 && h[k][o] < h[MIN][o] {
                h[MIN][o] = h[k][o];
            }
        }
        if h[PREF][o] < 0.0 && implicit[o] > 0.0 {
            h[PREF][o] = implicit[o].ceil();
        }
    }
    if h[PREF][0] < 0.0 || h[PREF][1] < 0.0 {
        for o in 0..2 {
            if h[PREF][o] < 0.0 {
                h[PREF][o] = fallback[o];
            }
        }
    }
    for o in 0..2 {
        if h[MIN][o] >= 0.0 && h[MIN][o] > h[PREF][o] {
            h[PREF][o] = h[MIN][o];
        }
        if h[MAX][o] >= 0.0 && h[MAX][o] < h[PREF][o] {
            h[PREF][o] = h[MAX][o];
        }
        let extra = if o == 0 {
            a.margins[0] + a.margins[2]
        } else {
            a.margins[1] + a.margins[3]
        };
        for k in [MIN, PREF, MAX] {
            h[k][o] += extra;
        }
    }
    h
}

/// Row/ColumnLayout along orientation `o` (0 = horizontal): a grid of one row or one column.
pub struct Linear<'a> {
    pub children: &'a [Child],
    pub o: usize,
    pub spacing: f64,
    /// `uniformCellSizes`.
    pub uniform: bool,
}

impl Linear<'_> {
    fn cells(&self) -> Vec<Cell> {
        (0..self.children.len())
            .map(|i| {
                let mut pos = [0; 2];
                pos[self.o] = i;
                Cell { pos, span: [1, 1] }
            })
            .collect()
    }

    /// QQuickGridLayoutBase::sizeHint → [min, pref, max] as `[w, h]`.
    pub fn size_hints(&self) -> [[f64; 2]; 3] {
        Grid {
            children: self.children,
            cells: &self.cells(),
            spacing: [self.spacing; 2],
            uniform: [self.uniform; 2],
        }
        .size_hints()
    }

    /// QGridLayoutEngine::setGeometries for a layout of `size`; returns child rects (x, y, w, h).
    pub fn arrange(&self, size: [f64; 2]) -> Vec<[f64; 4]> {
        Grid {
            children: self.children,
            cells: &self.cells(),
            spacing: [self.spacing; 2],
            uniform: [self.uniform; 2],
        }
        .arrange(size)
    }
}

/// Where a GridLayout item sits: first `[column, row]` and `[columnSpan, rowSpan]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub pos: [usize; 2],
    pub span: [usize; 2],
}

/// QQuickGridLayout::insertLayoutItems: the cell of each item, in child order. `set` is the
/// item's `Layout.column`/`Layout.row` (when either is set, the other reads 0) and `span` its
/// `[columnSpan, rowSpan]`; `bound` is `[columns, rows]` (None when not set) and `flow` 0 for
/// LeftToRight, 1 for TopToBottom. Items are placed until one cannot be (span below 1 or wider
/// than the flow bound): the rest stay out of the layout.
pub fn grid_cells(
    items: &[(Option<[i32; 2]>, [i32; 2])],
    bound: [Option<usize>; 2],
    flow: usize,
) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::new();
    let taken = |out: &[Cell], col: usize, row: usize| {
        out.iter().any(|c| {
            (c.pos[0]..c.pos[0] + c.span[0]).contains(&col)
                && (c.pos[1]..c.pos[1] + c.span[1]).contains(&row)
        })
    };
    let flow_bound = bound[flow].unwrap_or(usize::MAX);
    // [column, row] of the next cell to try.
    let mut next = [0usize; 2];
    for (set, span) in items {
        let (mut row, mut column) = (None, None);
        if let Some([c, r]) = set {
            // A negative row or column reads 0; a column past `columns` is as if not set.
            row = Some((*r).max(0) as usize);
            let c = (*c).max(0) as usize;
            if bound[0].is_none_or(|n| c < n) {
                column = Some(c);
            }
        }
        if span[0] < 1 || span[1] < 1 {
            break;
        }
        let span = [span[0] as usize, span[1] as usize];
        let sp = span[flow];
        if sp > flow_bound {
            break;
        }
        if let Some(r) = row {
            next[1] = r;
        }
        if let Some(c) = column {
            next[0] = c;
        }
        if row.is_none() || column.is_none() {
            // Advance along the flow to the first place where the item fits inside the bound
            // and none of its cells is taken.
            loop {
                let fits = next[flow] + sp <= flow_bound
                    && !(0..span[1])
                        .any(|rs| (0..span[0]).any(|cs| taken(&out, next[0] + cs, next[1] + rs)));
                if fits {
                    break;
                }
                next[flow] += 1;
                if next[flow] == flow_bound {
                    next[flow] = 0;
                    next[1 - flow] += 1;
                }
            }
        }
        out.push(Cell { pos: next, span });
    }
    out
}

/// QQuickGridLayout with its items at `cells` (QGridLayoutEngine).
pub struct Grid<'a> {
    pub children: &'a [Child],
    pub cells: &'a [Cell],
    /// `[columnSpacing, rowSpacing]`.
    pub spacing: [f64; 2],
    /// `[uniformCellWidths, uniformCellHeights]`.
    pub uniform: [bool; 2],
}

impl Grid<'_> {
    /// Columns (axis 0) or rows (axis 1) the items reach.
    fn count(&self, axis: usize) -> usize {
        self.cells
            .iter()
            .map(|c| c.pos[axis] + c.span[axis])
            .max()
            .unwrap_or(0)
    }

    /// QGridLayoutEngine::fillRowData and distributeMultiCells for the columns (axis 0) or
    /// rows (axis 1). Below, "row" is a line along `axis` and "column" one across it.
    fn row_data(&self, axis: usize) -> RowData {
        let (n, across) = (self.count(axis), self.count(1 - axis));
        let mut rd = RowData::new(n);
        rd.stretches.fill(-1);
        rd.spacings.fill(self.spacing[axis]);
        // Item at each (row, column).
        let mut at = vec![None; n * across];
        for (i, c) in self.cells.iter().enumerate().take(self.children.len()) {
            for r in c.pos[axis]..c.pos[axis] + c.span[axis] {
                for k in c.pos[1 - axis]..c.pos[1 - axis] + c.span[1 - axis] {
                    at[r * across + k] = Some(i);
                }
            }
        }
        // A row without items, or with the same items as the row before, takes no space.
        for r in 0..n {
            let row = &at[r * across..(r + 1) * across];
            let empty = row.iter().all(|i| i.is_none());
            let same = r > 0 && row == &at[(r - 1) * across..r * across];
            rd.ignore[r] = empty || same;
        }
        // Items spanning several rows that take space, keyed by (first row, span).
        let mut multi: std::collections::BTreeMap<(usize, usize), (GBox, i32)> =
            std::collections::BTreeMap::new();
        for r in 0..n {
            if rd.ignore[r] {
                continue;
            }
            let mut all_ignore = true;
            for k in 0..across {
                let Some(i) = at[r * across + k] else {
                    continue;
                };
                let (c, cell) = (&self.children[i], &self.cells[i]);
                if cell.pos[axis] != r || cell.pos[1 - axis] != k {
                    continue;
                }
                let st = c.stretch_factor(axis);
                if c.policy[axis] & IGNORE == 0 {
                    all_ignore = false;
                }
                let span = cell.span[axis];
                let effective = 1 + (1..span).filter(|d| !rd.ignore[r + d]).count();
                if effective == 1 {
                    if st != 0 {
                        rd.stretches[r] = rd.stretches[r].max(st);
                    }
                    rd.boxes[r].combine(&c.gbox(axis));
                } else {
                    let m = multi.entry((r, span)).or_insert((GBox::default(), 0));
                    m.0.combine(&c.gbox(axis));
                    m.1 = st;
                }
            }
            if all_ignore {
                rd.has_ignore_flag = true;
            }
        }
        if n > 1 && self.uniform[axis] {
            // Every cell gets the average preferred size, kept between the largest minimum and
            // the smallest maximum.
            let mut pref = 0.0;
            let mut max = f64::MAX;
            let mut min = 0.0f64;
            for b in &rd.boxes {
                pref += b.s[PREF];
                max = max.min(b.s[MAX]);
                min = min.max(b.s[MIN]);
            }
            let max = max.max(min);
            let pref = (pref / n as f64).max(min).min(max);
            rd.boxes.fill(GBox {
                s: [min, pref, max],
            });
        }
        // QGridLayoutRowData::distributeMultiCells: what a spanning item needs beyond its rows'
        // sizes is shared out between those rows.
        for ((start, span), (b, stretch)) in multi {
            let end = start + span;
            let total = rd.total_box(start, end);
            let mut extras = vec![GBox::default(); span];
            let mut dummy = vec![0.0; span];
            let mut sizes = vec![0.0; span];
            for j in 0..3 {
                let extra = if j == MAX {
                    total.s[j] - b.s[j]
                } else {
                    b.s[j] - total.s[j]
                };
                if extra > 0.0 {
                    rd.calculate_geometries(
                        start,
                        end,
                        b.s[j],
                        &mut dummy,
                        &mut sizes,
                        &total,
                        &[],
                        true,
                    );
                    for k in 0..span {
                        extras[k].s[j] = sizes[k];
                    }
                }
            }
            for k in 0..span {
                rd.boxes[start + k].combine(&extras[k]);
                if stretch != 0 {
                    rd.stretches[start + k] = rd.stretches[start + k].max(stretch);
                }
            }
        }
        rd
    }

    /// QQuickGridLayoutBase::sizeHint → [min, pref, max] as `[w, h]`.
    pub fn size_hints(&self) -> [[f64; 2]; 3] {
        if self.children.is_empty() || self.cells.is_empty() {
            return [[0.0; 2], [0.0; 2], [FLT_MAX; 2]];
        }
        let mut out = [[0.0; 2]; 3];
        for axis in 0..2 {
            let rd = self.row_data(axis);
            let t = rd.total_box(0, rd.boxes.len());
            for k in 0..3 {
                out[k][axis] = t.s[k];
            }
        }
        out
    }

    /// QGridLayoutEngine::setGeometries for a layout of `size`; returns child rects (x, y, w, h).
    /// A child without a cell keeps a zero rect.
    pub fn arrange(&self, size: [f64; 2]) -> Vec<[f64; 4]> {
        let n = self.children.len();
        if n == 0 || self.cells.is_empty() {
            return vec![[0.0; 4]; n];
        }
        let mut pos = [Vec::new(), Vec::new()];
        let mut len = [Vec::new(), Vec::new()];
        for axis in 0..2 {
            let rd = self.row_data(axis);
            let m = rd.boxes.len();
            let total = rd.total_box(0, m);
            pos[axis] = vec![0.0; m];
            len[axis] = vec![0.0; m];
            rd.calculate_geometries(
                0,
                m,
                size[axis],
                &mut pos[axis],
                &mut len[axis],
                &total,
                &[],
                true,
            );
        }
        let mut out = vec![[0.0; 4]; n];
        for (i, (c, cell)) in self.children.iter().zip(self.cells).enumerate().rev() {
            // The rect from the first cell to the last one the item spans.
            let mut r = [0.0; 4];
            for axis in 0..2 {
                let (first, last) = (cell.pos[axis], cell.pos[axis] + cell.span[axis] - 1);
                r[axis] = pos[axis][first];
                r[axis + 2] = len[axis][last] + pos[axis][last] - pos[axis][first];
            }
            let mut align = c.align;
            if align & V_MASK == 0 {
                align |= ALIGN_VCENTER;
            }
            let mut g = c.geometry_within(r[0], r[1], r[2], r[3], align);
            g[0] = qround(g[0]);
            g[1] = qround(g[1]);
            let m = c.margins;
            out[i] = [
                g[0] + m[0],
                g[1] + m[1],
                g[2] - m[0] - m[2],
                g[3] - m[1] - m[3],
            ];
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(w: f64, h: f64) -> Child {
        let hints = effective_size_hints(&Attached::new(), [w, h], [0.0, 0.0]);
        Child {
            hints,
            policy: [FIXED; 2],
            stretch: [-1; 2],
            align: 0,
            margins: [0.0; 4],
        }
    }

    #[test]
    fn grid_four_columns() {
        let kids = vec![fixed(10.0, 10.0); 6];
        let cells = grid_cells(&[(None, [1, 1]); 6], [Some(4), None], 0);
        let g = Grid {
            children: &kids,
            cells: &cells,
            spacing: [15.0, 15.0],
            uniform: [false; 2],
        };
        assert_eq!(g.size_hints()[PREF], [85.0, 35.0]);
        let r = g.arrange([85.0, 35.0]);
        assert_eq!(r[3], [75.0, 0.0, 10.0, 10.0]);
        assert_eq!(r[5], [25.0, 25.0, 10.0, 10.0]);
    }

    #[test]
    fn row_fixed_and_fill() {
        let mut fill = fixed(10.0, 10.0);
        fill.policy[0] = PREFERRED;
        let kids = [fixed(20.0, 10.0), fill, fixed(30.0, 20.0)];
        let l = Linear {
            children: &kids,
            o: 0,
            spacing: 5.0,
            uniform: false,
        };
        assert_eq!(l.size_hints()[PREF], [70.0, 20.0]);
        let r = l.arrange([200.0, 40.0]);
        assert_eq!(r[0], [0.0, 15.0, 20.0, 10.0]);
        assert_eq!(r[1], [25.0, 15.0, 140.0, 10.0]);
        assert_eq!(r[2], [170.0, 10.0, 30.0, 20.0]);
    }

    #[test]
    fn row_two_fills_grow_equally() {
        let mut a = fixed(153.0, 34.0);
        a.policy[0] = PREFERRED;
        let kids = [a.clone(), a];
        let l = Linear {
            children: &kids,
            o: 0,
            spacing: 5.0,
            uniform: false,
        };
        let r = l.arrange([322.0, 34.0]);
        // QGridLayoutEngine snaps to whole pixels.
        assert_eq!((r[0][2], r[1][0], r[1][2]), (159.0, 164.0, 158.0));
    }
}
