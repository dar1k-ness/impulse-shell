//! Monitor identities from `zwlr_output_management_unstable_v1`: each head's connector name
//! with the make, model and serial number the compositor read from the monitor's EDID
//! (`wl_output` carries no serial). Head events are only applied at the manager's `done`,
//! as the protocol asks clients to treat them atomically.
use std::collections::HashMap;
use std::hash::Hash;

use qml::layout_model::Edid;

/// One property of a head, as its events report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadEvent {
    Name(String),
    Make(String),
    Model(String),
    Serial(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Head {
    name: String,
    edid: Edid,
}

/// The heads (keyed by their protocol object) as the compositor described them, and the
/// identities applied at the last `done`.
#[derive(Debug)]
pub struct Heads<K> {
    pending: HashMap<K, Head>,
    applied: HashMap<String, Edid>,
}

impl<K> Default for Heads<K> {
    fn default() -> Self {
        Heads {
            pending: HashMap::new(),
            applied: HashMap::new(),
        }
    }
}

impl<K: Hash + Eq> Heads<K> {
    pub fn event(&mut self, head: K, e: HeadEvent) {
        let h = self.pending.entry(head).or_default();
        match e {
            HeadEvent::Name(n) => h.name = n,
            HeadEvent::Make(s) => h.edid.make = s,
            HeadEvent::Model(s) => h.edid.model = s,
            HeadEvent::Serial(s) => h.edid.serial = s,
        }
    }

    /// The head is gone (`finished`).
    pub fn remove(&mut self, head: &K) {
        self.pending.remove(head);
    }

    /// The manager's `done`: the described state becomes current. Returns whether any
    /// connector's identity changed.
    pub fn done(&mut self) -> bool {
        let next: HashMap<String, Edid> = self
            .pending
            .values()
            .filter(|h| !h.name.is_empty())
            .map(|h| (h.name.clone(), h.edid.clone()))
            .collect();
        let changed = next != self.applied;
        self.applied = next;
        changed
    }

    /// The EDID identity of the monitor on connector `name`, as of the last `done`.
    pub fn edid(&self, name: &str) -> Option<&Edid> {
        self.applied.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_apply_at_done_only() {
        let h = 1u32;
        let mut heads = Heads::default();
        heads.event(h, HeadEvent::Name("DP-1".into()));
        heads.event(h, HeadEvent::Make("Dell".into()));
        heads.event(h, HeadEvent::Model("U2720Q".into()));
        heads.event(h, HeadEvent::Serial("AAA".into()));
        heads.event(2, HeadEvent::Name("HDMI-A-1".into()));
        assert_eq!(heads.edid("DP-1"), None);
        assert!(heads.done());
        assert_eq!(heads.edid("HDMI-A-1"), Some(&Edid::default()));
        let e = heads.edid("DP-1").unwrap();
        assert_eq!(
            (e.make.as_str(), e.model.as_str(), e.serial.as_str()),
            ("Dell", "U2720Q", "AAA")
        );
        assert!(!heads.done());
        heads.remove(&h);
        assert_eq!(heads.edid("DP-1").map(|e| e.serial.as_str()), Some("AAA"));
        assert!(heads.done());
        assert_eq!(heads.edid("DP-1"), None);
    }
}
