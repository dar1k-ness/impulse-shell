//! Rectangle allocator for texture atlases, with deallocation: a guillotine tree as in Qt's
//! `QSGAreaAllocator` (qtdeclarative src/quick/scenegraph/util/qsgareaallocator.cpp). A free
//! leaf is cut in two to fit a request; freeing an area merges a node whose children are both
//! free back into one free leaf, so released space is reused at any size.

#[derive(Clone, Copy)]
struct Node {
    /// x, y, w, h.
    rect: [i32; 4],
    parent: Option<usize>,
    kids: Option<(usize, usize)>,
    used: bool,
    /// Free pixels in the subtree, to skip subtrees that cannot hold a request.
    free: i64,
}

pub struct AreaAllocator {
    nodes: Vec<Node>,
    /// Indices of merged-away nodes, reused by later cuts.
    spare: Vec<usize>,
}

impl AreaAllocator {
    pub fn new(w: i32, h: i32) -> Self {
        AreaAllocator {
            nodes: vec![Node {
                rect: [0, 0, w, h],
                parent: None,
                kids: None,
                used: false,
                free: w as i64 * h as i64,
            }],
            spare: Vec::new(),
        }
    }

    /// Top-left of a free `w` x `h` area, now taken; None when no free area is that large.
    pub fn allocate(&mut self, w: i32, h: i32) -> Option<(i32, i32)> {
        if w <= 0 || h <= 0 {
            return None;
        }
        let n = self.find(0, w, h)?;
        let n = self.cut(n, w, h);
        self.nodes[n].used = true;
        self.add_free(n, -(w as i64 * h as i64));
        let r = self.nodes[n].rect;
        Some((r[0], r[1]))
    }

    /// Releases the area `allocate` returned at (`x`, `y`); false if none starts there.
    pub fn deallocate(&mut self, x: i32, y: i32) -> bool {
        let mut n = 0;
        while let Some((a, b)) = self.nodes[n].kids {
            let r = self.nodes[b].rect;
            n = if x >= r[0] && y >= r[1] { b } else { a };
        }
        let r = self.nodes[n].rect;
        if !self.nodes[n].used || (r[0], r[1]) != (x, y) {
            return false;
        }
        self.nodes[n].used = false;
        self.add_free(n, r[2] as i64 * r[3] as i64);
        while let Some(p) = self.nodes[n].parent {
            let (a, b) = self.nodes[p].kids.expect("parent has kids");
            if !self.free_leaf(a) || !self.free_leaf(b) {
                break;
            }
            self.nodes[p].kids = None;
            self.spare.extend([a, b]);
            n = p;
        }
        true
    }

    /// Free pixels left.
    pub fn free_area(&self) -> i64 {
        self.nodes[0].free
    }

    fn free_leaf(&self, n: usize) -> bool {
        self.nodes[n].kids.is_none() && !self.nodes[n].used
    }

    fn add_free(&mut self, mut n: usize, d: i64) {
        loop {
            self.nodes[n].free += d;
            match self.nodes[n].parent {
                Some(p) => n = p,
                None => break,
            }
        }
    }

    /// First free leaf (in tree order) that holds `w` x `h`.
    fn find(&self, n: usize, w: i32, h: i32) -> Option<usize> {
        let node = &self.nodes[n];
        if node.rect[2] < w || node.rect[3] < h || node.free < w as i64 * h as i64 {
            return None;
        }
        match node.kids {
            None if node.used => None,
            None => Some(n),
            Some((a, b)) => self.find(a, w, h).or_else(|| self.find(b, w, h)),
        }
    }

    /// Cuts the free leaf `n` down to a `w` x `h` leaf at its top-left and returns it. The
    /// first cut goes across the larger leftover, keeping the remainder in one piece.
    fn cut(&mut self, mut n: usize, w: i32, h: i32) -> usize {
        loop {
            let [x, y, rw, rh] = self.nodes[n].rect;
            let (a, b) = if rw == w && rh == h {
                return n;
            } else if rh == h || (rw != w && rw - w >= rh - h) {
                ([x, y, w, rh], [x + w, y, rw - w, rh])
            } else {
                ([x, y, rw, h], [x, y + h, rw, rh - h])
            };
            let a = self.node(a, n);
            let b = self.node(b, n);
            self.nodes[n].kids = Some((a, b));
            n = a;
        }
    }

    fn node(&mut self, rect: [i32; 4], parent: usize) -> usize {
        let node = Node {
            rect,
            parent: Some(parent),
            kids: None,
            used: false,
            free: rect[2] as i64 * rect[3] as i64,
        };
        match self.spare.pop() {
            Some(i) => {
                self.nodes[i] = node;
                i
            }
            None => {
                self.nodes.push(node);
                self.nodes.len() - 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: [i32; 4], b: [i32; 4]) -> bool {
        a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    }

    #[test]
    fn areas_are_disjoint_and_inside() {
        let mut al = AreaAllocator::new(64, 64);
        let mut got: Vec<[i32; 4]> = Vec::new();
        for k in 0..200 {
            let (w, h) = (3 + k % 7, 4 + k % 5);
            let Some((x, y)) = al.allocate(w, h) else {
                continue;
            };
            let r = [x, y, w, h];
            assert!(
                x >= 0 && y >= 0 && x + w <= 64 && y + h <= 64,
                "{r:?} outside"
            );
            assert!(got.iter().all(|g| !overlaps(*g, r)), "{r:?} overlaps");
            got.push(r);
        }
        let used: i64 = got.iter().map(|r| r[2] as i64 * r[3] as i64).sum();
        assert_eq!(al.free_area(), 64 * 64 - used);
        assert!(used > 64 * 64 / 2, "packed only {used} px");
    }

    /// Freed areas are reused, and freeing everything merges back to one free leaf.
    #[test]
    fn freed_areas_merge_and_are_reused() {
        let mut al = AreaAllocator::new(32, 32);
        let mut got = Vec::new();
        while let Some(p) = al.allocate(8, 8) {
            got.push(p);
        }
        assert_eq!(got.len(), 16);
        assert_eq!(al.allocate(1, 1), None, "full");
        assert!(al.deallocate(got[5].0, got[5].1));
        assert!(!al.deallocate(got[5].0, got[5].1), "double free");
        assert!(!al.deallocate(got[6].0 + 1, got[6].1), "not an area start");
        assert_eq!(al.allocate(8, 8), Some(got[5]));
        for &(x, y) in &got {
            assert!(al.deallocate(x, y));
        }
        assert_eq!(al.free_area(), 32 * 32);
        assert_eq!(al.allocate(32, 32), Some((0, 0)), "not merged back");
        assert_eq!(
            al.nodes.len() - al.spare.len(),
            1 + 2 * (al.nodes.len() - al.spare.len() - 1) / 2
        );
    }

    #[test]
    fn rejects_what_cannot_fit() {
        let mut al = AreaAllocator::new(16, 8);
        assert_eq!(al.allocate(17, 1), None);
        assert_eq!(al.allocate(1, 9), None);
        assert_eq!(al.allocate(0, 4), None);
        assert_eq!(al.allocate(16, 8), Some((0, 0)));
        assert_eq!(al.allocate(1, 1), None);
    }
}
