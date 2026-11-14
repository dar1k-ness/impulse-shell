//! Matching connected outputs to stored monitor layouts. The EDID identifies the monitor
//! itself, so its layout follows it to another port; the connector name is the fallback
//! for monitors without one. A monitor seen for the first time starts from a copy of the
//! primary layout instead of an empty screen. A monitor unplugged while the shell runs has
//! its containers shown on a remaining output until it comes back ([`Layout::guests`]); its
//! stored layout is never touched for that.

use super::{Edid, Layout, MonitorLayout};

/// A connected output, as the compositor reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorId {
    pub connector: String,
    pub edid: Option<Edid>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MonitorMatch {
    /// The stored layout of this very monitor (index into [`Layout::monitors`]).
    Edid(usize),
    /// The stored layout for this connector, from a monitor without a known EDID.
    Connector(usize),
    /// A monitor never seen: a copy of the primary layout (no containers when there is no
    /// layout at all), not yet part of the layout.
    New(MonitorLayout),
}

impl MonitorId {
    /// The identity a stored monitor layout was saved under.
    pub fn of(m: &MonitorLayout) -> MonitorId {
        MonitorId {
            connector: m.connector.clone(),
            edid: known(&m.edid).cloned(),
        }
    }
}

/// An EDID with nothing in it identifies nothing.
pub(super) fn known(e: &Option<Edid>) -> Option<&Edid> {
    e.as_ref()
        .filter(|e| !(e.make.is_empty() && e.model.is_empty() && e.serial.is_empty()))
}

/// One matching rule; `None` when it needs an EDID that one side lacks.
type Rule = dyn Fn(&MonitorId, &MonitorLayout) -> Option<bool>;

impl Layout {
    /// The layout for each connected output, each stored layout used at most once (two
    /// identical monitors without serials are told apart by their connectors).
    pub fn resolve(&self, outputs: &[MonitorId]) -> Vec<MonitorMatch> {
        self.matched(outputs)
            .into_iter()
            .zip(outputs)
            .map(|(m, o)| match m {
                Some((mi, true)) => MonitorMatch::Edid(mi),
                Some((mi, false)) => MonitorMatch::Connector(mi),
                None => MonitorMatch::New(self.template(o)),
            })
            .collect()
    }

    /// The stored monitor matched to each output, and whether by EDID.
    fn matched(&self, outputs: &[MonitorId]) -> Vec<Option<(usize, bool)>> {
        let mut taken = vec![false; self.monitors.len()];
        let mut out: Vec<Option<(usize, bool)>> = vec![None; outputs.len()];
        let passes: [&Rule; 3] = [
            &|o, m| Some(known(&o.edid)? == known(&m.edid)? && o.connector == m.connector),
            &|o, m| Some(known(&o.edid)? == known(&m.edid)?),
            &|o, m| {
                let either_unknown = known(&o.edid).is_none() || known(&m.edid).is_none();
                Some(either_unknown && o.connector == m.connector)
            },
        ];
        for (pass, matches) in passes.iter().enumerate() {
            for (oi, o) in outputs.iter().enumerate() {
                if out[oi].is_some() {
                    continue;
                }
                let found = self
                    .monitors
                    .iter()
                    .enumerate()
                    .position(|(mi, m)| !taken[mi] && matches(o, m) == Some(true));
                if let Some(mi) = found {
                    taken[mi] = true;
                    out[oi] = Some((mi, pass < 2));
                }
            }
        }
        out
    }

    /// The stored monitor each output shows: its own, or the primary one for a monitor never
    /// seen (which starts from a copy of it); `None` when there is no layout at all.
    pub fn shown(&self, outputs: &[MonitorId]) -> Vec<Option<usize>> {
        let primary = self.primary();
        self.matched(outputs)
            .into_iter()
            .map(|m| m.map(|(i, _)| i).or(primary))
            .collect()
    }

    /// The identities of the stored monitors that `outputs` are (not the primary layout a
    /// never-seen monitor borrows).
    pub fn present(&self, outputs: &[MonitorId]) -> Vec<MonitorId> {
        self.matched(outputs)
            .into_iter()
            .flatten()
            .map(|(i, _)| MonitorId::of(&self.monitors[i]))
            .collect()
    }

    /// The monitors that were connected (`seen`) and are gone now, while they hold containers,
    /// and the output that shows them meanwhile: the one showing the primary layout, else the
    /// first. Their layouts stay as stored; when the monitor comes back it shows them again.
    pub fn guests(&self, outputs: &[MonitorId], seen: &[MonitorId]) -> Option<(usize, Vec<usize>)> {
        if outputs.is_empty() {
            return None;
        }
        let shown = self.shown(outputs);
        let primary = self.primary();
        let host = shown
            .iter()
            .position(|&s| s.is_some() && s == primary)
            .unwrap_or(0);
        let gone: Vec<usize> = (0..self.monitors.len())
            .filter(|&i| {
                let m = &self.monitors[i];
                !shown.contains(&Some(i))
                    && !m.containers.is_empty()
                    && seen.contains(&MonitorId::of(m))
            })
            .collect();
        (!gone.is_empty()).then_some((host, gone))
    }

    pub fn monitor_for(&self, output: &MonitorId) -> MonitorMatch {
        self.resolve(std::slice::from_ref(output)).remove(0)
    }

    fn template(&self, o: &MonitorId) -> MonitorLayout {
        let containers = self
            .primary()
            .map(|i| self.monitors[i].containers.clone())
            .unwrap_or_default();
        MonitorLayout {
            connector: o.connector.clone(),
            edid: known(&o.edid).cloned(),
            primary: false,
            containers,
        }
    }
}
