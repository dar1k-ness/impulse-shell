//! opencv_contrib ximgproc `GraphSegmentationImpl` (Felzenszwalb), as used by selective search.

use super::ss_cv::gaussian_blur_f32;
use super::ss_sort::std_sort;

#[derive(Clone, Copy)]
pub struct Edge {
    from: u32,
    to: u32,
    weight: f32,
}

struct PointSet {
    p: Vec<u32>,
    size: Vec<i32>,
}

impl PointSet {
    fn base(&mut self, p: usize) -> usize {
        let mut b = p;
        while b != self.p[b] as usize {
            b = self.p[b] as usize;
        }
        self.p[p] = b as u32;
        b
    }
    fn join(&mut self, mut a: usize, mut b: usize) {
        if self.size[a] < self.size[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.p[b] = a as u32;
        self.size[a] += self.size[b];
    }
}

/// The k-independent half of `processImage`: blur, 4-neighbour edges, `std::sort` by weight.
pub fn sorted_edges(img: &[u8], w: usize, h: usize, fma: bool) -> Vec<Edge> {
    let cn = 3;
    let f: Vec<f32> = img.iter().map(|&v| v as f32).collect();
    let f = gaussian_blur_f32(&f, w, h, cn, fma);
    let mut edges = Vec::with_capacity(w * h * 4);
    for i in 0..h as isize {
        for j in 0..w as isize {
            for (i2, j2) in [(i - 1, j), (i, j - 1), (i + 1, j), (i, j + 1)] {
                if i2 >= 0 && i2 < h as isize && j2 >= 0 && j2 < w as isize {
                    let a = (i as usize * w + j as usize) * cn;
                    let b = (i2 as usize * w + j2 as usize) * cn;
                    let mut t = 0f32;
                    for c in 0..cn {
                        let d = f[a + c] - f[b + c];
                        t += d * d;
                    }
                    edges.push(Edge {
                        from: (i as usize * w + j as usize) as u32,
                        to: (i2 as usize * w + j2 as usize) as u32,
                        weight: t.sqrt(),
                    });
                }
            }
        }
    }
    std_sort(&mut edges, &|a: &Edge, b: &Edge| a.weight < b.weight);
    edges
}

/// The rest of `processImage(img8u3, out)` for one k on `n` pixels: per-pixel segment ids
/// (scan-order numbering) and the count.
pub fn segment(sorted: &[Edge], n: usize, k: f32, min_size: i32) -> (Vec<i32>, usize) {
    let mut edges = sorted.to_vec();
    let mut es = PointSet {
        p: (0..n as u32).collect(),
        size: vec![1; n],
    };
    let mut th = vec![k; n];
    for e in edges.iter_mut() {
        let pa = es.base(e.from as usize);
        let pb = es.base(e.to as usize);
        if pa != pb && e.weight <= th[pa] && e.weight <= th[pb] {
            es.join(pa, pb);
            let pa = es.base(pa);
            th[pa] = e.weight + k / es.size[pa] as f32;
            e.weight = 0.0;
        }
    }
    for e in &edges {
        if e.weight > 0.0 {
            let pa = es.base(e.from as usize);
            let pb = es.base(e.to as usize);
            if pa != pb && (es.size[pa] < min_size || es.size[pb] < min_size) {
                es.join(pa, pb);
            }
        }
    }
    let mut mapped = vec![-1i32; n];
    let mut last = 0;
    let mut out = vec![0i32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let p = es.base(i);
        if mapped[p] == -1 {
            mapped[p] = last;
            last += 1;
        }
        *o = mapped[p];
    }
    (out, last as usize)
}
