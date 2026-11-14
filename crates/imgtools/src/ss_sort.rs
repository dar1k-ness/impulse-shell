//! libstdc++ `std::sort` (introsort) and glibc `rand()`, reproduced so that tie orders and
//! random ranks match the OpenCV build shipped in the Python wheels (GCC/glibc).

/// `std::sort(v.begin(), v.end(), less)` as implemented by libstdc++ (`bits/stl_algo.h`).
pub fn std_sort<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], less: &F) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let depth = 2 * (usize::BITS - 1 - n.leading_zeros()) as usize;
    introsort_loop(v, 0, n, depth, less);
    final_insertion_sort(v, 0, n, less);
}

const THRESHOLD: usize = 16;

fn introsort_loop<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth: usize,
    less: &F,
) {
    while last - first > THRESHOLD {
        if depth == 0 {
            partial_sort(v, first, last, last, less);
            return;
        }
        depth -= 1;
        let cut = unguarded_partition_pivot(v, first, last, less);
        introsort_loop(v, cut, last, depth, less);
        last = cut;
    }
}

fn move_median_to_first<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    result: usize,
    a: usize,
    b: usize,
    c: usize,
    less: &F,
) {
    if less(&v[a], &v[b]) {
        if less(&v[b], &v[c]) {
            v.swap(result, b);
        } else if less(&v[a], &v[c]) {
            v.swap(result, c);
        } else {
            v.swap(result, a);
        }
    } else if less(&v[a], &v[c]) {
        v.swap(result, a);
    } else if less(&v[b], &v[c]) {
        v.swap(result, c);
    } else {
        v.swap(result, b);
    }
}

fn unguarded_partition_pivot<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &F,
) -> usize {
    let mid = first + (last - first) / 2;
    move_median_to_first(v, first, first + 1, mid, last - 1, less);
    let mut lo = first + 1;
    let mut hi = last;
    loop {
        while less(&v[lo], &v[first]) {
            lo += 1;
        }
        hi -= 1;
        while less(&v[first], &v[hi]) {
            hi -= 1;
        }
        if lo >= hi {
            return lo;
        }
        v.swap(lo, hi);
        lo += 1;
    }
}

fn partial_sort<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    less: &F,
) {
    heap_select(v, first, middle, last, less);
    sort_heap(v, first, middle, less);
}

fn heap_select<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    less: &F,
) {
    make_heap(v, first, middle, less);
    for i in middle..last {
        if less(&v[i], &v[first]) {
            pop_heap(v, first, middle, i, less);
        }
    }
}

fn make_heap<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], first: usize, last: usize, less: &F) {
    let len = last - first;
    if len < 2 {
        return;
    }
    let mut parent = (len - 2) / 2;
    loop {
        let value = v[first + parent];
        adjust_heap(v, first, parent, len, value, less);
        if parent == 0 {
            return;
        }
        parent -= 1;
    }
}

fn sort_heap<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], first: usize, mut last: usize, less: &F) {
    while last - first > 1 {
        last -= 1;
        pop_heap(v, first, last, last, less);
    }
}

fn pop_heap<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    result: usize,
    less: &F,
) {
    let value = v[result];
    v[result] = v[first];
    adjust_heap(v, first, 0, last - first, value, less);
}

fn adjust_heap<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut hole: usize,
    len: usize,
    value: T,
    less: &F,
) {
    let top = hole;
    let mut child = hole;
    while child < (len - 1) / 2 {
        child = 2 * (child + 1);
        if less(&v[first + child], &v[first + child - 1]) {
            child -= 1;
        }
        v[first + hole] = v[first + child];
        hole = child;
    }
    if len & 1 == 0 && child == (len - 2) / 2 {
        child = 2 * (child + 1);
        v[first + hole] = v[first + child - 1];
        hole = child - 1;
    }
    let mut parent = hole.wrapping_sub(1) / 2;
    while hole > top && less(&v[first + parent], &value) {
        v[first + hole] = v[first + parent];
        hole = parent;
        parent = hole.wrapping_sub(1) / 2;
    }
    v[first + hole] = value;
}

fn insertion_sort<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &F,
) {
    if first == last {
        return;
    }
    for i in first + 1..last {
        let val = v[i];
        if less(&val, &v[first]) {
            v.copy_within(first..i, first + 1);
            v[first] = val;
        } else {
            unguarded_linear_insert(v, i, less);
        }
    }
}

fn unguarded_linear_insert<T: Copy, F: Fn(&T, &T) -> bool>(v: &mut [T], mut last: usize, less: &F) {
    let val = v[last];
    let mut next = last - 1;
    while less(&val, &v[next]) {
        v[last] = v[next];
        last = next;
        next -= 1;
    }
    v[last] = val;
}

fn final_insertion_sort<T: Copy, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &F,
) {
    if last - first > THRESHOLD {
        insertion_sort(v, first, first + THRESHOLD, less);
        for i in first + THRESHOLD..last {
            unguarded_linear_insert(v, i, less);
        }
    } else {
        insertion_sort(v, first, last, less);
    }
}

/// glibc `rand()` (TYPE_3 additive feedback generator) in a process that never called `srand`.
pub struct GlibcRand {
    r: [i32; 34],
    pos: usize,
}

pub const RAND_MAX: i32 = i32::MAX;

impl Default for GlibcRand {
    fn default() -> Self {
        Self::new(1)
    }
}

impl GlibcRand {
    pub fn new(seed: u32) -> Self {
        let mut r = [0i32; 34];
        r[0] = if seed == 0 { 1 } else { seed as i32 };
        for i in 1..31 {
            let hi = r[i - 1] / 127_773;
            let lo = r[i - 1] % 127_773;
            let mut word = 16_807 * lo - 2_836 * hi;
            if word < 0 {
                word += i32::MAX;
            }
            r[i] = word;
        }
        for i in 31..34 {
            r[i] = r[i - 31];
        }
        let mut g = GlibcRand { r, pos: 0 };
        for _ in 34..344 {
            g.step();
        }
        g
    }

    fn step(&mut self) -> u32 {
        let v = (self.r[(self.pos + 34 - 31) % 34] as u32)
            .wrapping_add(self.r[(self.pos + 34 - 3) % 34] as u32);
        self.r[self.pos] = v as i32;
        self.pos = (self.pos + 1) % 34;
        v
    }

    pub fn rand(&mut self) -> i32 {
        (self.step() >> 1) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glibc_rand_first_values() {
        let mut g = GlibcRand::default();
        let v: Vec<i32> = (0..3).map(|_| g.rand()).collect();
        assert_eq!(v, [1_804_289_383, 846_930_886, 1_681_692_777]);
    }

    #[test]
    fn sort_orders() {
        let mut g = GlibcRand::default();
        for n in [0usize, 1, 5, 16, 17, 100, 1000] {
            let mut v: Vec<(i32, usize)> = (0..n).map(|i| (g.rand() % 7, i)).collect();
            std_sort(&mut v, &|a: &(i32, usize), b: &(i32, usize)| a.0 < b.0);
            assert!(v.windows(2).all(|w| w[0].0 <= w[1].0));
        }
    }
}
