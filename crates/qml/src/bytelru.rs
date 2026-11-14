//! Least-recently-used cache bounded by the total weight (bytes) of its entries and, optionally,
//! by their number. Entries a `pin` predicate holds (shared with a frame still drawing them)
//! count towards the weight but are never evicted: dropping them would free nothing and only
//! make the next lookup decode a second copy.

use std::borrow::Borrow;
use std::hash::Hash;

pub struct ByteLru<K: Hash + Eq, V> {
    map: lru::LruCache<K, (V, usize)>,
    bytes: usize,
    budget: usize,
    max_len: usize,
    pin: fn(&V) -> bool,
    removed: fn(&K),
}

impl<K: Hash + Eq + Clone, V> ByteLru<K, V> {
    /// A cache keeping at most `budget` bytes of unpinned entries.
    pub fn new(budget: usize) -> Self {
        ByteLru {
            map: lru::LruCache::unbounded(),
            bytes: 0,
            budget,
            max_len: usize::MAX,
            pin: |_| false,
            removed: |_| {},
        }
    }

    /// Also keep at most `n` entries.
    pub fn max_len(mut self, n: usize) -> Self {
        self.max_len = n;
        self
    }

    /// Never evict entries `pin` holds.
    pub fn pinned(mut self, pin: fn(&V) -> bool) -> Self {
        self.pin = pin;
        self
    }

    /// Call `f` with each key that leaves the cache (removed, retained out, evicted, cleared);
    /// replacing an entry's value keeps its key.
    pub fn on_removed(mut self, f: fn(&K)) -> Self {
        self.removed = f;
        self
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Total weight of the cached entries.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// The entry, marked most recently used.
    pub fn get<Q>(&mut self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get(k).map(|e| &e.0)
    }

    /// The entry, leaving the recency order alone.
    pub fn peek<Q>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.peek(k).map(|e| &e.0)
    }

    pub fn contains<Q>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.contains(k)
    }

    /// Insert (or replace) `k` as the most recently used entry, then evict least recently used
    /// unpinned entries beyond the budget. The new entry itself is kept even when it alone
    /// exceeds the budget, so a lookup right after the insert always hits.
    pub fn insert(&mut self, k: K, v: V, weight: usize) {
        if let Some((_, (_, w))) = self.map.push(k.clone(), (v, weight)) {
            self.bytes -= w;
        }
        self.bytes += weight;
        self.evict(Some(&k));
    }

    pub fn remove<Q>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (k, (v, w)) = self.map.pop_entry(k)?;
        self.bytes -= w;
        (self.removed)(&k);
        Some(v)
    }

    /// Change values in place, keeping their recency: `f` returns the new weight of a value it
    /// changed. Only `evict` enforces the budget again.
    pub fn update(&mut self, mut f: impl FnMut(&K, &mut V) -> Option<usize>) {
        for (k, (v, w)) in self.map.iter_mut() {
            if let Some(n) = f(k, v) {
                self.bytes = self.bytes - *w + n;
                *w = n;
            }
        }
    }

    /// The entries, most recently used first, leaving the recency order alone.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map.iter().map(|(k, (v, _))| (k, v))
    }

    /// Keep only the entries `f` accepts.
    pub fn retain(&mut self, mut f: impl FnMut(&K, &V) -> bool) {
        let drop: Vec<K> = self
            .map
            .iter()
            .filter(|(k, (v, _))| !f(k, v))
            .map(|(k, _)| k.clone())
            .collect();
        for k in drop {
            self.remove(&k);
        }
    }

    pub fn clear(&mut self) {
        for (k, _) in std::mem::replace(&mut self.map, lru::LruCache::unbounded()) {
            (self.removed)(&k);
        }
        self.bytes = 0;
    }

    /// Evict least recently used unpinned entries (sparing `keep`) until the unpinned weight
    /// fits the budget and the entry count fits `max_len`.
    pub fn evict(&mut self, keep: Option<&K>) {
        if self.bytes <= self.budget && self.map.len() <= self.max_len {
            return;
        }
        let pinned: usize = self
            .map
            .iter()
            .filter(|(_, (v, _))| (self.pin)(v))
            .map(|(_, (_, w))| *w)
            .sum();
        let mut loose = self.bytes - pinned;
        let mut len = self.map.len();
        let mut drop = Vec::new();
        for (k, (v, w)) in self.map.iter().rev() {
            if loose <= self.budget && len <= self.max_len {
                break;
            }
            if keep == Some(k) || (self.pin)(v) {
                continue;
            }
            loose -= w;
            len -= 1;
            drop.push(k.clone());
        }
        for k in drop {
            self.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ByteLru;
    use std::rc::Rc;

    #[test]
    fn evicts_least_recently_used_beyond_budget() {
        let mut c = ByteLru::new(30);
        c.insert(1, "a", 10);
        c.insert(2, "b", 10);
        c.insert(3, "c", 10);
        assert_eq!(c.get(&1), Some(&"a"));
        c.insert(4, "d", 10);
        assert!(!c.contains(&2), "2 was the least recently used");
        assert!(c.contains(&1) && c.contains(&3) && c.contains(&4));
        assert_eq!(c.bytes(), 30);
    }

    #[test]
    fn replacing_an_entry_updates_its_weight() {
        let mut c = ByteLru::new(100);
        c.insert("k", 1, 60);
        c.insert("k", 2, 20);
        assert_eq!(c.bytes(), 20);
        assert_eq!(c.len(), 1);
        assert_eq!(c.remove("k"), Some(2));
        assert_eq!(c.bytes(), 0);
    }

    #[test]
    fn oversized_newest_entry_is_kept_alone() {
        let mut c = ByteLru::new(10);
        c.insert(1, (), 5);
        c.insert(2, (), 50);
        assert!(!c.contains(&1));
        assert!(c.contains(&2));
        c.insert(3, (), 1);
        assert!(!c.contains(&2) && c.contains(&3));
        assert_eq!(c.bytes(), 1);
    }

    #[test]
    fn entry_count_limit() {
        let mut c = ByteLru::new(usize::MAX).max_len(2);
        for i in 0..5 {
            c.insert(i, (), 0);
        }
        assert_eq!(c.len(), 2);
        assert!(c.contains(&3) && c.contains(&4));
    }

    #[test]
    fn pinned_entries_are_never_evicted_and_freed_once_released() {
        let mut c = ByteLru::new(10).pinned(|v: &Rc<u8>| Rc::strong_count(v) > 1);
        let held = Rc::new(0u8);
        c.insert(0, held.clone(), 100);
        for i in 1..5 {
            c.insert(i, Rc::new(i as u8), 10);
        }
        assert!(c.contains(&0), "a held entry stays cached");
        assert!(c.contains(&4));
        assert_eq!(c.len(), 2);
        drop(held);
        c.evict(None);
        assert!(
            !c.contains(&0),
            "released, it is evicted as least recently used"
        );
        assert_eq!(c.bytes(), 10);
    }

    #[test]
    fn retain_and_clear_keep_the_weight_in_step() {
        let mut c = ByteLru::new(usize::MAX);
        for i in 0..10 {
            c.insert(i, i, i);
        }
        c.retain(|k, _| k % 2 == 0);
        assert_eq!(c.len(), 5);
        assert_eq!(c.bytes(), 2 + 4 + 6 + 8);
        c.clear();
        assert_eq!((c.len(), c.bytes()), (0, 0));
    }

    #[test]
    fn update_reweighs_in_place_and_keeps_recency() {
        let mut c = ByteLru::new(10);
        c.insert(1, 1, 4);
        c.insert(2, 2, 4);
        c.update(|k, v| {
            (*k == 1).then(|| {
                *v = 10;
                1
            })
        });
        assert_eq!(c.bytes(), 5);
        assert_eq!(c.peek(&1), Some(&10));
        c.insert(3, 3, 6);
        assert!(
            !c.contains(&1) && c.contains(&2) && c.contains(&3),
            "1 stayed least recent"
        );
    }

    #[test]
    fn removal_hook_sees_every_key_that_leaves() {
        thread_local!(static GONE: std::cell::RefCell<Vec<u8>> = Default::default());
        let mut c = ByteLru::new(20).on_removed(|k| GONE.with_borrow_mut(|g| g.push(*k)));
        for k in 0..4 {
            c.insert(k, (), 10);
        }
        c.insert(3, (), 5);
        c.remove(&3);
        c.retain(|_, _| false);
        c.insert(5, (), 1);
        c.clear();
        assert_eq!(GONE.with_borrow(|g| g.clone()), [0, 1, 3, 2, 5]);
    }
}
