//! QV4's `sortHelper` (qv4arraydata_p.h, Qt 6.12), the quicksort behind `Array.prototype.sort`.
//! It is not stable: ties end up where V4 puts them, not in input order.

pub fn sort<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    helper(v, &mut less);
}

fn helper<T, F: FnMut(&T, &T) -> bool>(mut v: &mut [T], less: &mut F) {
    loop {
        let span = v.len();
        if span < 2 {
            return;
        }
        let end = span - 1;
        let pivot = span / 2;
        if less(&v[end], &v[0]) {
            v.swap(end, 0);
        }
        if span == 2 {
            return;
        }
        if less(&v[pivot], &v[0]) {
            v.swap(pivot, 0);
        }
        if less(&v[end], &v[pivot]) {
            v.swap(end, pivot);
        }
        if span == 3 {
            return;
        }
        v.swap(pivot, end);
        let (mut low, mut high) = (0, end - 1);
        while low < high {
            while low < high && less(&v[low], &v[end]) {
                low += 1;
            }
            while high > low && less(&v[end], &v[high]) {
                high -= 1;
            }
            if low < high {
                v.swap(low, high);
                low += 1;
                high -= 1;
            } else {
                break;
            }
        }
        if less(&v[low], &v[end]) {
            low += 1;
        }
        v.swap(end, low);
        let (left, right) = std::mem::take(&mut v).split_at_mut(low);
        helper(left, less);
        v = &mut right[1..];
    }
}

#[cfg(test)]
mod tests {
    /// Orders printed by Qt 6.12 `qml` for `[...].sort((a, b) => a.k - b.k)`.
    #[test]
    fn ties_follow_v4() {
        let run = |keys: &[(char, i32)]| {
            let mut v = keys.to_vec();
            super::sort(&mut v, |a, b| a.1 - b.1 < 0);
            v.iter().map(|x| x.0).collect::<String>()
        };
        assert_eq!(run(&[('C', 1), ('N', 1), ('H', 0)]), "HNC");
        assert_eq!(run(&[('A', 1), ('B', 1), ('C', 1), ('D', 0)]), "DACB");
        assert_eq!(run(&[('H', 0), ('C', 1), ('N', 1)]), "HCN");
    }
}
