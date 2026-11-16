//! Changes to the layout. Every change is an [`Edit`] applied to a copy, checked with
//! [`Layout::validate`], and only then made current, so a rejected edit leaves nothing
//! half done. Each applied batch is one undo step; monitors attached for newly connected
//! outputs are part of every step, so undo never takes a monitor away.

use super::monitors::{MonitorId, MonitorMatch};
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// `index`: position among the widgets of the same zone (panel) or of the container,
    /// `None` to append.
    AddWidget {
        monitor: usize,
        container: String,
        widget: Widget,
        index: Option<usize>,
    },
    RemoveWidget {
        monitor: usize,
        id: String,
    },
    /// Moves a widget to `container` (the same one or another) with a new place: a drag, a
    /// resize, a zone change. `index` as for [`Edit::AddWidget`]; `None` keeps the widget's
    /// position when it stays in its container.
    MoveWidget {
        monitor: usize,
        id: String,
        container: String,
        place: Place,
        index: Option<usize>,
    },
    SetSettings {
        monitor: usize,
        id: String,
        settings: toml::Table,
    },
    /// `index` into the monitor's containers, `None` to append.
    AddContainer {
        monitor: usize,
        container: Container,
        index: Option<usize>,
    },
    /// Removes the container with its widgets.
    RemoveContainer {
        monitor: usize,
        id: String,
    },
    /// New properties for a container of the same kind (a panel's edge, thickness, ...).
    SetContainerKind {
        monitor: usize,
        id: String,
        kind: ContainerKind,
    },
    SetPrimary {
        monitor: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    NoMonitor(usize),
    NoContainer(String),
    NoWidget(String),
    /// [`Edit::SetContainerKind`] to another kind: its widgets' places would not fit.
    KindChange {
        from: &'static str,
        to: &'static str,
    },
    Invalid(SchemaError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EditError::NoMonitor(i) => write!(f, "no monitor {i}"),
            EditError::NoContainer(id) => write!(f, "no container \"{id}\""),
            EditError::NoWidget(id) => write!(f, "no widget \"{id}\""),
            EditError::KindChange { from, to } => {
                write!(f, "a {from} cannot become a {to}")
            }
            EditError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EditError {}

/// The current layout with its undo and redo history.
pub struct Editor {
    layout: Layout,
    undo: VecDeque<Layout>,
    redo: Vec<Layout>,
    limit: usize,
    revision: u64,
}

impl Editor {
    /// Undo steps kept by [`Editor::new`].
    pub const DEFAULT_LIMIT: usize = 100;

    pub fn new(layout: Layout) -> Editor {
        Editor::with_limit(layout, Editor::DEFAULT_LIMIT)
    }

    pub fn with_limit(layout: Layout, limit: usize) -> Editor {
        Editor {
            layout,
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit,
            revision: 0,
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Bumped by every change of [`Editor::layout`] (edits, undo, redo, attached monitors):
    /// what was saved or built at another revision is stale.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn apply(&mut self, edit: Edit) -> Result<bool, EditError> {
        self.apply_all(std::slice::from_ref(&edit))
    }

    /// Applies `edits` as one undo step, all or none. `Ok(false)`: nothing changed, no step.
    pub fn apply_all(&mut self, edits: &[Edit]) -> Result<bool, EditError> {
        let mut next = self.layout.clone();
        for e in edits {
            apply(&mut next, e.clone())?;
        }
        next.validate().map_err(EditError::Invalid)?;
        if next == self.layout {
            return Ok(false);
        }
        self.undo
            .push_back(std::mem::replace(&mut self.layout, next));
        if self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.revision += 1;
        Ok(true)
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop_back() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.layout, prev));
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push_back(std::mem::replace(&mut self.layout, next));
        self.revision += 1;
        true
    }

    /// Changes the layout and every step of its history the same way, as the attached monitors
    /// do: not an undo step, since going back would undo it. Returns whether the layout shown
    /// changed.
    pub fn amend(&mut self, f: impl Fn(&mut Layout) -> bool) -> bool {
        for l in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            f(l);
        }
        let changed = f(&mut self.layout);
        if changed {
            self.revision += 1;
        }
        changed
    }

    /// The monitor index for each connected output: stored layouts are matched
    /// ([`Layout::resolve`]), a monitor found by EDID on another port takes that port's
    /// name, and never-seen monitors are added with a copy of the primary layout. None of
    /// this is an undo step: the whole history gets the same monitors.
    pub fn attach(&mut self, outputs: &[MonitorId]) -> Vec<usize> {
        let matches = self.layout.resolve(outputs);
        let before = self.layout.clone();
        let mut out = Vec::with_capacity(outputs.len());
        for (m, o) in matches.into_iter().zip(outputs) {
            let i = match m {
                MonitorMatch::Edid(i) => {
                    if self.layout.monitors[i].connector != o.connector {
                        let c = o.connector.clone();
                        self.each_snapshot(|l| l.monitors[i].connector = c.clone());
                    }
                    i
                }
                MonitorMatch::Connector(i) => i,
                MonitorMatch::New(layout) => {
                    let i = self.layout.monitors.len();
                    self.each_snapshot(|l| l.monitors.push(layout.clone()));
                    i
                }
            };
            out.push(i);
        }
        if self.layout != before {
            self.revision += 1;
        }
        out
    }

    fn each_snapshot(&mut self, mut f: impl FnMut(&mut Layout)) {
        f(&mut self.layout);
        self.undo.iter_mut().chain(self.redo.iter_mut()).for_each(f);
    }
}

/// `l` with `edits` made, as [`Editor::apply_all`] would leave it: what a gesture under way
/// would give, to show before it is dropped.
pub fn applied(l: &Layout, edits: &[Edit]) -> Result<Layout, EditError> {
    let mut next = l.clone();
    for e in edits {
        apply(&mut next, e.clone())?;
    }
    next.validate().map_err(EditError::Invalid)?;
    Ok(next)
}

fn monitor(l: &mut Layout, i: usize) -> Result<&mut MonitorLayout, EditError> {
    l.monitors.get_mut(i).ok_or(EditError::NoMonitor(i))
}

fn container_index(m: &MonitorLayout, id: &str) -> Result<usize, EditError> {
    m.containers
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| EditError::NoContainer(id.to_string()))
}

fn widget_index(m: &MonitorLayout, id: &str) -> Result<(usize, usize), EditError> {
    m.containers
        .iter()
        .enumerate()
        .find_map(|(ci, c)| c.widgets.iter().position(|w| w.id == id).map(|wi| (ci, wi)))
        .ok_or_else(|| EditError::NoWidget(id.to_string()))
}

/// A gap: the room edit mode leaves beside a panel widget pulled in by an end, so the widget's
/// other end and the widgets around it stay where they were. In the file it is a spacer of a
/// set length whose id begins with `gap`, which every release reads; a panel shows it as room
/// beside its neighbour, not as a widget.
pub fn is_gap(w: &Widget) -> bool {
    w.r#type == "spacer"
        && w.id.starts_with("gap")
        && matches!(
            w.place,
            Place::Panel {
                width: Width::Px(_),
                ..
            }
        )
}

/// A gap that is no room beside a widget but how far a zone that stands in the middle of its
/// panel stands off it (`gap-shift…`): first or last in the zone, it moves the zone's widgets
/// half its length away from that end, as a spacer there does in every release. Edit mode
/// leaves one where a widget of the zone was pulled out by an end, so that only that end went.
pub fn shifts(w: &Widget) -> bool {
    is_gap(w) && w.id.starts_with("gap-shift")
}

fn zone_of(w: &Widget) -> Option<Zone> {
    match w.place {
        Place::Panel { zone, .. } => Some(zone),
        _ => None,
    }
}

/// The widgets of `zone` in `c`, in layout order: their positions among all of its widgets.
fn zone_peers(c: &Container, zone: Zone) -> Vec<usize> {
    (0..c.widgets.len())
        .filter(|&i| zone_of(&c.widgets[i]) == Some(zone))
        .collect()
}

/// The gaps beside widget `id` in its zone, on either side.
pub fn gaps_beside<'a>(c: &'a Container, id: &str) -> (Option<&'a Widget>, Option<&'a Widget>) {
    let Some(zone) = c.widgets.iter().find(|w| w.id == id).and_then(zone_of) else {
        return (None, None);
    };
    let peers = zone_peers(c, zone);
    let Some(at) = peers.iter().position(|&i| c.widgets[i].id == id) else {
        return (None, None);
    };
    let gap = |k: Option<usize>| {
        k.and_then(|k| peers.get(k))
            .map(|&i| &c.widgets[i])
            .filter(|w| is_gap(w) && !shifts(w))
    };
    (gap(at.checked_sub(1)), gap(Some(at + 1)))
}

fn beside_ids(c: &Container, id: &str) -> Vec<String> {
    let (a, b) = gaps_beside(c, id);
    a.into_iter().chain(b).map(|w| w.id.clone()).collect()
}

/// Gaps never pile up: two that stand side by side in a zone become one of both lengths. How
/// far the zone stands off the middle ([`shifts`]) is no room beside a widget and joins none.
fn merge_gaps(c: &mut Container) {
    for zone in [Zone::Start, Zone::Center, Zone::End] {
        let peers = zone_peers(c, zone);
        let mut gone = vec![];
        let mut into: Option<usize> = None;
        for &i in &peers {
            if !is_gap(&c.widgets[i]) || shifts(&c.widgets[i]) {
                into = None;
                continue;
            }
            let Some(first) = into else {
                into = Some(i);
                continue;
            };
            let len = |w: &Widget| match w.place {
                Place::Panel {
                    width: Width::Px(n),
                    ..
                } => n,
                _ => 0,
            };
            let sum = len(&c.widgets[first]).saturating_add(len(&c.widgets[i]));
            c.widgets[first].place = Place::Panel {
                zone,
                width: Width::Px(sum.min(super::MAX_PX)),
            };
            gone.push(i);
        }
        // A zone with no widget left stands nowhere.
        if peers.iter().all(|&i| is_gap(&c.widgets[i])) {
            gone.extend(peers.iter().filter(|&&i| shifts(&c.widgets[i])));
        }
        gone.sort_unstable();
        gone.dedup();
        for i in gone.into_iter().rev() {
            c.widgets.remove(i);
        }
    }
}

/// The ids of `c`'s widgets that are no gaps, zone by zone in layout order.
fn standing(c: &Container) -> Vec<(Option<Zone>, &str)> {
    let mut v: Vec<_> = c
        .widgets
        .iter()
        .filter(|w| !is_gap(w))
        .map(|w| (zone_of(w), w.id.as_str()))
        .collect();
    v.sort_by_key(|(z, _)| z.map(|z| z as u8));
    v
}

/// The zone ordering is per zone in a panel: `index` counts only the widgets of `w`'s zone,
/// and of those only the ones a panel shows as widgets, not the gaps ([`is_gap`]). A gap
/// stays with the widget it is the room of: what is put before a widget goes before the gap
/// before it too.
fn insert(c: &mut Container, w: Widget, index: Option<usize>) {
    let z = zone_of(&w);
    let all: Vec<usize> = (0..c.widgets.len())
        .filter(|&i| zone_of(&c.widgets[i]) == z)
        .collect();
    let peers: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| !is_gap(&c.widgets[i]))
        .collect();
    let at = match index {
        Some(i) if i < peers.len() => {
            let mut k = all.iter().position(|&p| p == peers[i]).unwrap_or(0);
            while k > 0 && leads(&c.widgets[all[k - 1]]) {
                k -= 1;
            }
            all[k]
        }
        // The last of its zone, before what holds the zone off the middle from that end.
        _ => match all.last() {
            Some(&p) if shifts(&c.widgets[p]) && !shifts(&w) && !peers.is_empty() => p,
            Some(&p) => p + 1,
            None => c.widgets.len(),
        },
    };
    c.widgets.insert(at, w);
}

/// Whether a gap is the room before the widget after it (`gap-before…`), not after the one
/// before it.
pub fn leads(w: &Widget) -> bool {
    is_gap(w) && w.id.starts_with("gap-before")
}

fn apply(l: &mut Layout, e: Edit) -> Result<(), EditError> {
    match e {
        Edit::AddWidget {
            monitor: mi,
            container,
            widget,
            index,
        } => {
            let m = monitor(l, mi)?;
            let ci = container_index(m, &container)?;
            insert(&mut m.containers[ci], widget, index);
            merge_gaps(&mut m.containers[ci]);
        }
        Edit::RemoveWidget { monitor: mi, id } => {
            let m = monitor(l, mi)?;
            let (ci, wi) = widget_index(m, &id)?;
            let c = &mut m.containers[ci];
            // The room it was pulled in from goes with it.
            let beside = match is_gap(&c.widgets[wi]) {
                true => vec![],
                false => beside_ids(c, &id),
            };
            c.widgets.retain(|w| w.id != id && !beside.contains(&w.id));
            merge_gaps(c);
        }
        Edit::MoveWidget {
            monitor: mi,
            id,
            container,
            place,
            index,
        } => {
            let m = monitor(l, mi)?;
            let to = container_index(m, &container)?;
            let (ci, wi) = widget_index(m, &id)?;
            let beside = match is_gap(&m.containers[ci].widgets[wi]) {
                true => vec![],
                false => beside_ids(&m.containers[ci], &id),
            };
            let stood: Vec<(Option<Zone>, String)> = standing(&m.containers[ci])
                .into_iter()
                .map(|(z, id)| (z, id.to_string()))
                .collect();
            let was = m.containers[ci].widgets.clone();
            let mut w = m.containers[ci].widgets.remove(wi);
            w.place = place;
            let placed = w.place.clone();
            if ci == to && index.is_none() {
                m.containers[ci].widgets.insert(wi, w);
            } else {
                insert(&mut m.containers[to], w, index);
            }
            // Carried to another place, it neither takes the room beside it along nor leaves
            // it behind; sized or dropped where it stood, the room stays.
            let stands = standing(&m.containers[ci]);
            let same = ci == to
                && stands.len() == stood.len()
                && stands
                    .iter()
                    .zip(&stood)
                    .all(|(a, b)| a.0 == b.0 && a.1 == b.1);
            if same {
                // Where it stood among the gaps too.
                m.containers[ci].widgets = was;
                m.containers[ci].widgets[wi].place = placed;
            } else {
                m.containers[ci].widgets.retain(|w| !beside.contains(&w.id));
            }
            merge_gaps(&mut m.containers[ci]);
            if ci != to {
                merge_gaps(&mut m.containers[to]);
            }
        }
        Edit::SetSettings {
            monitor: mi,
            id,
            settings,
        } => {
            let m = monitor(l, mi)?;
            let (ci, wi) = widget_index(m, &id)?;
            m.containers[ci].widgets[wi].settings = settings;
        }
        Edit::AddContainer {
            monitor: mi,
            container,
            index,
        } => {
            let m = monitor(l, mi)?;
            let at = index.map_or(m.containers.len(), |i| i.min(m.containers.len()));
            m.containers.insert(at, container);
        }
        Edit::RemoveContainer { monitor: mi, id } => {
            let m = monitor(l, mi)?;
            let ci = container_index(m, &id)?;
            m.containers.remove(ci);
        }
        Edit::SetContainerKind {
            monitor: mi,
            id,
            kind,
        } => {
            let m = monitor(l, mi)?;
            let ci = container_index(m, &id)?;
            let c = &mut m.containers[ci];
            if c.kind.name() != kind.name() {
                return Err(EditError::KindChange {
                    from: c.kind.name(),
                    to: kind.name(),
                });
            }
            c.kind = kind;
        }
        Edit::SetPrimary { monitor: mi } => {
            monitor(l, mi)?;
            for (i, m) in l.monitors.iter_mut().enumerate() {
                m.primary = i == mi;
            }
        }
    }
    Ok(())
}
