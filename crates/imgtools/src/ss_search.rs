//! opencv_contrib ximgproc `SelectiveSearchSegmentationImpl` in fast mode
//! (`switchToSelectiveSearchFast` + `process`).

use super::ss_cv::{
    bgr2hsv, bgr2lab, convert_scale_u8, rotated_rect_bbox, rotation_matrix, scharr, warp_affine,
};
use super::ss_graph::{segment, sorted_edges};
use super::ss_sort::{GlibcRand, RAND_MAX, std_sort};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn union(self, b: Rect) -> Rect {
        let x = self.x.min(b.x);
        let y = self.y.min(b.y);
        Rect {
            x,
            y,
            w: (self.x + self.w).max(b.x + b.w) - x,
            h: (self.y + self.h).max(b.y + b.h) - y,
        }
    }
}

#[derive(Clone, Copy)]
struct Region {
    id: usize,
    level: i32,
    rank: f64,
    bbox: Rect,
}

#[derive(Clone, Copy)]
struct Neighbour {
    from: usize,
    to: usize,
    sim: f32,
}

/// Per-segment histograms of one strategy instance (rows of `bins` floats).
#[derive(Clone)]
struct Hist {
    bins: usize,
    h: Vec<f32>,
}

impl Hist {
    fn get(&self, a: usize, b: usize) -> f32 {
        let (h1, h2) = (
            &self.h[a * self.bins..(a + 1) * self.bins],
            &self.h[b * self.bins..(b + 1) * self.bins],
        );
        let mut r = 0f32;
        for i in 0..self.bins {
            r += h1[i].min(h2[i]);
        }
        r
    }
    fn merge(&mut self, a: usize, b: usize, sa: i32, sb: i32) {
        let n = self.bins;
        for i in 0..n {
            let v =
                (self.h[a * n + i] * sa as f32 + self.h[b * n + i] * sb as f32) / (sa + sb) as f32;
            self.h[a * n + i] = v;
            self.h[b * n + i] = v;
        }
    }
}

fn color_hist(img: &[u8], seg: &[i32], nb: usize) -> Hist {
    let bins = 75;
    let mut cnt = vec![0i32; nb * bins];
    for (p, &r) in img.as_chunks::<3>().0.iter().zip(seg) {
        for c in 0..3 {
            cnt[r as usize * bins + c * 25 + ((p[c] as usize * 25) >> 8)] += 1;
        }
    }
    let mut h = vec![0f32; nb * bins];
    for r in 0..nb {
        let row = &cnt[r * bins..(r + 1) * bins];
        let mut tt = 0f32;
        for &v in row {
            tt += v as f32;
        }
        for i in 0..bins {
            h[r * bins + i] = row[i] as f32 / tt;
        }
    }
    Hist { bins, h }
}

fn min_max(v: &[f32]) -> (f64, f64) {
    let mut lo = v[0];
    let mut hi = v[0];
    for &x in v {
        if x < lo {
            lo = x;
        }
        if x > hi {
            hi = x;
        }
    }
    (lo as f64, hi as f64)
}

/// The region-independent half of the texture strategy's `setImage`: the 24 quantized
/// gradient images (3 channels x 4 directions x pos/neg).
fn texture_quant(img: &[u8], w: usize, h: usize) -> Vec<Vec<u8>> {
    let n = w * h;
    let pos_neg = |g: &[f32], out: &mut Vec<Vec<f32>>| {
        out.push(g.iter().map(|&v| if v > 0.0 { v } else { 0.0 }).collect());
        out.push(g.iter().map(|&v| if v > 0.0 { 0.0 } else { v }).collect());
    };
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut rot = rotation_matrix(cx as f64, cy as f64, 45.0);
    let bb = rotated_rect_bbox(cx, cy, w as f32, h as f32, 45.0);
    rot[2] += bb.2 as f64 / 2.0 - cx as f64;
    rot[5] += bb.3 as f64 / 2.0 - cy as f64;
    let (rw, rh) = (bb.2 as usize, bb.3 as usize);
    let (cx2, cy2) = (
        (rw as f64 / 2.0) as i32 as f32,
        (rh as f64 / 2.0) as i32 as f32,
    );
    let rot2 = rotation_matrix(cx2 as f64, cy2 as f64, -45.0);
    let bb2 = rotated_rect_bbox(cx2, cy2, rw as f32, rh as f32, -45.0);
    let (bw2, bh2) = (bb2.2 as usize, bb2.3 as usize);
    let sx = 0.max((bb.2 - w as i32) / 2) as usize;
    let sy = 0.max((bb.3 - h as i32) / 2) as usize;
    let per_channel: Vec<Vec<Vec<f32>>> = (0..3)
        .into_par_iter()
        .map(|c| {
            let mut grads = Vec::with_capacity(8);
            let plane: Vec<u8> = img.iter().skip(c).step_by(3).copied().collect();
            pos_neg(&scharr(&plane, w, h, true), &mut grads);
            pos_neg(&scharr(&plane, w, h, false), &mut grads);
            let rp = warp_affine(&plane, w, h, &rot, rw, rh);
            for dx in [true, false] {
                let g = scharr(&rp, rw, rh, dx);
                let back = warp_affine(&g, rw, rh, &rot2, bw2, bh2);
                let mut crop = Vec::with_capacity(n);
                for y in 0..h {
                    crop.extend_from_slice(&back[(sy + y) * bw2 + sx..(sy + y) * bw2 + sx + w]);
                }
                pos_neg(&crop, &mut grads);
            }
            grads
        })
        .collect();
    let grads: Vec<Vec<f32>> = per_channel.into_iter().flatten().collect();
    let quant: Vec<Vec<u8>> = grads
        .par_iter()
        .map(|g| {
            let (lo, hi) = min_max(g);
            convert_scale_u8(g, 255.0 / (hi - lo), -255.0 * lo / (hi - lo))
        })
        .collect();
    quant
}

fn texture_hist(quant: &[Vec<u8>], seg: &[i32], nb: usize) -> Hist {
    let n = seg.len();
    let bins = 240;
    let mut cnt = vec![0i32; nb * bins];
    let mut totals = vec![0i32; nb];
    let bw = 256.0f32 / 10.0;
    for x in 0..n {
        let r = seg[x] as usize;
        for (i, q) in quant.iter().enumerate() {
            let bin = (q[x] as f32 / bw) as usize;
            cnt[r * bins + i * 10 + bin] += 1;
            totals[r] += 1;
        }
    }
    let h = (0..nb * bins)
        .map(|i| cnt[i] as f32 / totals[i / bins] as f32)
        .collect();
    Hist { bins, h }
}

/// One `SelectiveSearchSegmentationStrategyMultiple`: [color,] fill, texture, size.
struct Strategy {
    color: Option<Hist>,
    texture: Hist,
    fill: Vec<Rect>,
    size_image: i32,
    weights: &'static [f32],
}

impl Strategy {
    fn get(&self, sizes: &[i32], a: usize, b: usize) -> f32 {
        let si = self.size_image as f32;
        let u = self.fill[a].union(self.fill[b]);
        let fill = (1.0f32 - (u.w * u.h - sizes[a] - sizes[b]) as f32 / si).clamp(0.0, 1.0);
        let size = (1.0f32 - (sizes[a] + sizes[b]) as f32 / si).clamp(0.0, 1.0);
        let tex = self.texture.get(a, b);
        let mut parts = [0f32; 4];
        let n = match &self.color {
            Some(c) => {
                parts = [c.get(a, b), fill, tex, size];
                4
            }
            None => {
                parts[..3].copy_from_slice(&[fill, tex, size]);
                3
            }
        };
        let mut tt = 0f32;
        let mut wt = 0f32;
        for (p, &w) in parts[..n].iter().zip(self.weights) {
            tt += w * p;
            wt += w;
        }
        tt / wt
    }
    fn merge(&mut self, sizes: &[i32], a: usize, b: usize) {
        if let Some(c) = &mut self.color {
            c.merge(a, b, sizes[a], sizes[b]);
        }
        let u = self.fill[a].union(self.fill[b]);
        self.fill[a] = u;
        self.fill[b] = u;
        self.texture.merge(a, b, sizes[a], sizes[b]);
    }
}

fn hierarchical_grouping(
    s: &mut Strategy,
    nbr: &[bool],
    sizes0: &[i32],
    rects: &[Rect],
    rng: &mut GlibcRand,
    out: &mut Vec<Region>,
) {
    let nb = sizes0.len();
    let mut sizes = sizes0.to_vec();
    let mut regions: Vec<Region> = Vec::with_capacity(nb * 2);
    let mut sims: Vec<Neighbour> = Vec::new();
    for i in 0..nb {
        regions.push(Region {
            id: i,
            level: 1,
            rank: 0.0,
            bbox: rects[i],
        });
        for j in i + 1..nb {
            if nbr[i * nb + j] {
                sims.push(Neighbour {
                    from: i,
                    to: j,
                    sim: s.get(&sizes, i, j),
                });
            }
        }
    }
    let less = |a: &Neighbour, b: &Neighbour| a.sim < b.sim;
    let mut local: Vec<usize> = Vec::new();
    while !sims.is_empty() {
        std_sort(&mut sims, &less);
        let p = sims.pop().unwrap();
        let (rf, rt) = (regions[p.from], regions[p.to]);
        regions.push(Region {
            id: rf.id.min(rt.id),
            level: rf.level.max(rt.level) + 1,
            rank: 0.0,
            bbox: rf.bbox.union(rt.bbox),
        });
        s.merge(&sizes, rf.id, rt.id);
        sizes[rf.id] += sizes[rt.id];
        sizes[rt.id] = sizes[rf.id];
        local.clear();
        sims.retain(|n| {
            let hit = n.from == p.from || n.to == p.from || n.from == p.to || n.to == p.to;
            if hit {
                let from = if n.from == p.from || n.from == p.to {
                    n.to
                } else {
                    n.from
                };
                if !local.contains(&from) {
                    local.push(from);
                }
            }
            !hit
        });
        let new = regions.len() - 1;
        for &t in &local {
            sims.push(Neighbour {
                from: new,
                to: t,
                sim: s.get(&sizes, regions[new].id, regions[t].id),
            });
        }
    }
    for r in regions.iter_mut() {
        r.rank = rng.rand() as f64 / RAND_MAX as f64 * r.level as f64;
    }
    out.extend_from_slice(&regions);
}

/// One graph segmentation with what both strategy combinations need from it.
struct Seg {
    nbr: Vec<bool>,
    sizes: Vec<i32>,
    rects: Vec<Rect>,
    color: Hist,
    texture: Hist,
}

impl Seg {
    fn new(
        img: &[u8],
        w: usize,
        h: usize,
        edges: &[super::ss_graph::Edge],
        quant: &[Vec<u8>],
        k: f32,
    ) -> Seg {
        let (seg, nb) = segment(edges, w * h, k, 100);
        let mut nbr = vec![false; nb * nb];
        let mut sizes = vec![0i32; nb];
        let mut rects = vec![
            Rect {
                x: i32::MAX,
                y: i32::MAX,
                w: i32::MIN,
                h: i32::MIN
            };
            nb
        ];
        for i in 0..h {
            for j in 0..w {
                let p = seg[i * w + j] as usize;
                sizes[p] += 1;
                let r = &mut rects[p];
                r.x = r.x.min(j as i32);
                r.y = r.y.min(i as i32);
                r.w = r.w.max(j as i32);
                r.h = r.h.max(i as i32);
                if i > 0 && j > 0 {
                    for q in [
                        seg[i * w + j - 1],
                        seg[(i - 1) * w + j],
                        seg[(i - 1) * w + j - 1],
                    ] {
                        let q = q as usize;
                        nbr[p * nb + q] = true;
                        nbr[q * nb + p] = true;
                    }
                }
            }
        }
        for r in rects.iter_mut() {
            r.w = r.w - r.x + 1;
            r.h = r.h - r.y + 1;
        }
        let color = color_hist(img, &seg, nb);
        let texture = texture_hist(quant, &seg, nb);
        Seg {
            nbr,
            sizes,
            rects,
            color,
            texture,
        }
    }
}

/// The script's `--k` and `--min-size` defaults, the only ones the shell runs it with. The
/// script hands them to `switchToSelectiveSearchFast` as `base_k` and `inc_k`.
const BASE_K: i32 = 3000;
const INC_K: i32 = 50;

/// `ss.setBaseImage(img); ss.switchToSelectiveSearchFast(BASE_K, INC_K, SIGMA); ss.process()`.
pub fn selective_search_fast(bgr: &[u8], w: usize, h: usize, fma: bool) -> Vec<Rect> {
    let images = [bgr2hsv(bgr), bgr2lab(bgr)];
    let ks = [BASE_K, BASE_K + INC_K, BASE_K + 2 * INC_K].map(|k| k as f32);
    let pre: Vec<(Vec<_>, Vec<Vec<u8>>)> = images
        .par_iter()
        .map(|img| (sorted_edges(img, w, h, fma), texture_quant(img, w, h)))
        .collect();
    let jobs: Vec<(usize, f32)> = (0..images.len())
        .flat_map(|i| ks.iter().map(move |&k| (i, k)))
        .collect();
    let segs: Vec<Seg> = jobs
        .par_iter()
        .map(|&(i, k)| Seg::new(&images[i], w, h, &pre[i].0, &pre[i].1, k))
        .collect();
    let mut rng = GlibcRand::default();
    let mut all: Vec<Region> = Vec::new();
    let size_image = (w * h) as i32;
    static W1: [f32; 4] = [0.25; 4];
    static W2: [f32; 3] = [0.3333; 3];
    for s in segs {
        let mut m1 = Strategy {
            color: Some(s.color),
            texture: s.texture.clone(),
            fill: s.rects.clone(),
            size_image,
            weights: &W1,
        };
        hierarchical_grouping(&mut m1, &s.nbr, &s.sizes, &s.rects, &mut rng, &mut all);
        let mut m2 = Strategy {
            color: None,
            texture: s.texture,
            fill: s.rects.clone(),
            size_image,
            weights: &W2,
        };
        hierarchical_grouping(&mut m2, &s.nbr, &s.sizes, &s.rects, &mut rng, &mut all);
    }
    std_sort(&mut all, &|a: &Region, b: &Region| a.rank < b.rank);
    let mut seen = std::collections::HashSet::new();
    all.iter()
        .filter(|r| seen.insert(r.bbox))
        .map(|r| r.bbox)
        .collect()
}
