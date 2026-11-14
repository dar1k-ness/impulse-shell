use serde::{Deserialize, Serialize};

/// A logistic model on CLIP embeddings learnt from likes and dislikes. Everything ever shown is a
/// soft neutral anchor (target 0.5), so a handful of likes alone already points it somewhere.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Taste {
    pub pos: Vec<Vec<f32>>,
    pub neg: Vec<Vec<f32>>,
    pub seen: Vec<Vec<f32>>,
    pub w: Vec<f32>,
    pub b: f32,
}

const SCALE: f32 = 10.0;
const SEEN_CAP: usize = 400;
const STEPS: usize = 300;

impl Taste {
    pub(crate) fn samples(&self) -> usize {
        self.pos.len() + self.neg.len()
    }

    /// A candidate that was scored; one that is scored again counts once.
    pub(crate) fn add_seen(&mut self, e: &[f32]) {
        if self.seen.iter().any(|s| s == e) {
            return;
        }
        self.seen.push(e.to_vec());
        if self.seen.len() > SEEN_CAP {
            let extra = self.seen.len() - SEEN_CAP;
            self.seen.drain(..extra);
        }
    }

    /// Records a vote for `e`, replacing an earlier vote on the same embedding, and retrains.
    pub(crate) fn vote(&mut self, e: &[f32], like: bool) {
        self.pos.retain(|v| v != e);
        self.neg.retain(|v| v != e);
        if like {
            self.pos.push(e.to_vec());
        } else {
            self.neg.push(e.to_vec());
        }
        self.train();
    }

    /// Full-batch gradient descent with L2 on weighted logistic loss; deterministic.
    pub(crate) fn train(&mut self) {
        let dim = self.pos.first().or(self.neg.first()).map_or(0, Vec::len);
        if dim == 0 {
            self.w.clear();
            self.b = 0.0;
            return;
        }
        let data: Vec<(&[f32], f32, f32)> = self
            .pos
            .iter()
            .map(|e| (e.as_slice(), 1.0, 1.0))
            .chain(self.neg.iter().map(|e| (e.as_slice(), 0.0, 1.0)))
            .chain(self.seen.iter().map(|e| (e.as_slice(), 0.5, 0.25)))
            .filter(|d| d.0.len() == dim)
            .collect();
        let mut w = vec![0f32; dim];
        let mut b = 0f32;
        let wsum: f32 = data.iter().map(|d| d.2).sum();
        let (lr, l2) = (0.5f32, 0.01f32);
        for _ in 0..STEPS {
            let mut gw = vec![0f32; dim];
            let mut gb = 0f32;
            for (x, y, wt) in &data {
                let z = b + SCALE * crate::clip::dot(x, &w);
                let p = 1.0 / (1.0 + (-z).exp());
                let g = (p - y) * wt;
                gb += g;
                for (gi, xi) in gw.iter_mut().zip(x.iter()) {
                    *gi += g * SCALE * xi;
                }
            }
            for (wi, gi) in w.iter_mut().zip(&gw) {
                *wi -= lr * (gi / wsum + l2 * *wi);
            }
            b -= lr * gb / wsum;
        }
        self.w = w;
        self.b = b;
    }

    /// Log-odds of a like, clamped to ±4 and damped while there are fewer than 6 votes.
    pub(crate) fn score(&self, e: &[f32]) -> f32 {
        if self.w.len() != e.len() {
            return 0.0;
        }
        let z = self.b + SCALE * crate::clip::dot(e, &self.w);
        let conf = (self.samples() as f32 / 6.0).min(1.0);
        z.clamp(-4.0, 4.0) * conf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::normalize;
    use rand::{Rng, SeedableRng, rngs::StdRng};

    fn around(rng: &mut StdRng, centre: &[f32]) -> Vec<f32> {
        normalize(
            centre
                .iter()
                .map(|c| c + rng.random_range(-0.3..0.3))
                .collect(),
        )
    }

    #[test]
    fn learns_likes_from_dislikes() {
        let mut rng = StdRng::seed_from_u64(1);
        let dim = 32;
        let mut a = vec![0f32; dim];
        let mut b = vec![0f32; dim];
        a[0] = 1.0;
        b[1] = 1.0;
        let mut t = Taste::default();
        assert_eq!(t.score(&a), 0.0);
        for _ in 0..20 {
            let e = around(&mut rng, &a);
            t.add_seen(&e);
            let e = around(&mut rng, &b);
            t.add_seen(&e);
        }
        for _ in 0..4 {
            let e = around(&mut rng, &a);
            t.vote(&e, true);
            let e = around(&mut rng, &b);
            t.vote(&e, false);
        }
        let (la, lb) = (around(&mut rng, &a), around(&mut rng, &b));
        assert!(t.score(&la) > 1.0, "{}", t.score(&la));
        assert!(t.score(&lb) < -1.0, "{}", t.score(&lb));
        assert!(
            t.score(&[0.0; 3]) == 0.0,
            "dimension mismatch scores neutral"
        );
    }

    #[test]
    fn revote_replaces_and_confidence_ramps() {
        let mut e = vec![0f32; 8];
        e[2] = 1.0;
        let mut t = Taste::default();
        t.vote(&e, true);
        let one = t.score(&e);
        assert!(one > 0.0 && one <= 4.0 / 6.0 + 1e-6, "{one}");
        t.vote(&e, false);
        assert_eq!((t.pos.len(), t.neg.len()), (0, 1));
        assert!(t.score(&e) < 0.0);
        let before = t.clone();
        t.train();
        assert_eq!(t, before, "training is deterministic");
    }

    #[test]
    fn seen_is_capped() {
        let mut t = Taste::default();
        for i in 0..SEEN_CAP + 10 {
            t.add_seen(&[i as f32]);
        }
        assert_eq!(t.seen.len(), SEEN_CAP);
        assert_eq!(t.seen[0], [10.0]);
    }
}
