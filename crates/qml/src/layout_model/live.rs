//! The layout the running shell renders: `layout.toml` opened through [`Store`], edited
//! through [`Editor`], and read again when the file changes on disk (an inotify watch, see
//! `fswatch`). Container types (`Impulse/PanelContainer`, `DesktopContainer`,
//! `SidebarContainer`) find their container here by monitor connector and kind.

use super::{
    Container, Edit, EditError, Editor, Layout, LoadError, LoadSource, MonitorId, MonitorLayout,
    Place, Store, store::Polled,
};
use std::path::{Path, PathBuf};

pub struct Live {
    store: Store,
    editor: Editor,
    /// Why the last load did not use the main file.
    problems: Vec<LoadError>,
    source: LoadSource,
    /// Bumped by every load and every applied edit, so dependents know to look again.
    generation: u64,
    /// The connected outputs, as the compositor last reported them ([`Live::set_outputs`]).
    outputs: Vec<MonitorId>,
    /// The stored monitors connected at some point since the shell started: the ones whose
    /// containers move to a remaining output while they are unplugged.
    seen: Vec<MonitorId>,
}

/// What a layout-backed view's outputs were last computed from: its own inputs and the part
/// of the layout it shows, with the [`Live::generation`] that was checked at. A moved
/// generation only makes the view compare its part again: an edit elsewhere in the layout
/// writes none of its outputs, so its delegates stay.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown<I, C> {
    pub generation: Option<u64>,
    pub inputs: I,
    pub content: C,
}

impl<I: PartialEq, C: PartialEq> Shown<I, C> {
    /// Whether `prev` still holds at `generation` for `inputs`, without looking at the layout.
    pub fn fresh(prev: Option<&Self>, generation: Option<u64>, inputs: &I) -> bool {
        prev.is_some_and(|p| p.generation == generation && p.inputs == *inputs)
    }

    /// Whether `prev` was computed from `inputs` and `content`: the layout changed, but not
    /// what the view shows.
    pub fn same(prev: Option<&Self>, inputs: &I, content: &C) -> bool {
        prev.is_some_and(|p| p.inputs == *inputs && p.content == *content)
    }
}

impl Live {
    pub fn open(path: impl Into<PathBuf>) -> Live {
        Live::open_with(path, super::migrate::Inputs::ri())
    }

    /// [`Live::open`] migrating an old file with `inputs` ([`Store::open_with`]).
    pub fn open_with(path: impl Into<PathBuf>, inputs: super::migrate::Inputs) -> Live {
        let (store, loaded) = Store::open_with(path, inputs);
        Live {
            store,
            editor: Editor::new(loaded.layout),
            problems: loaded.problems,
            source: loaded.source,
            generation: 0,
            outputs: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// The connected outputs changed (hotplug, or an EDID arrived). Returns whether they did;
    /// the generation then moves on so every container looks up its monitor again.
    pub fn set_outputs(&mut self, outputs: &[MonitorId]) -> bool {
        if self.outputs == outputs {
            return false;
        }
        self.outputs = outputs.to_vec();
        self.note_seen();
        self.generation += 1;
        true
    }

    fn note_seen(&mut self) {
        for id in self.editor.layout().present(&self.outputs) {
            if !self.seen.contains(&id) {
                self.seen.push(id);
            }
        }
    }

    /// Reads the file again after it changed on disk ([`Store::poll`]). A different valid
    /// layout replaces the editor and its undo history: the steps it held were relative to
    /// the layout that was replaced. The shell's own save, or a file holding exactly the
    /// current layout, keeps the editor as it is. A file that is unusable now (half-written,
    /// a typo) keeps the layout shown, and a removed file gets it written back, so neither
    /// blanks the screen; what happened is returned for the log.
    pub fn reload(&mut self) -> Option<String> {
        let at = self.store.path().display().to_string();
        match self.store.poll() {
            Polled::Unchanged => None,
            Polled::Changed(layout) => {
                self.problems.clear();
                self.source = LoadSource::Main;
                if layout != *self.editor.layout() {
                    self.editor = Editor::new(layout);
                    self.generation += 1;
                    self.note_seen();
                }
                None
            }
            Polled::Invalid(e) => {
                let msg = format!("{at}: {e}; keeping the layout shown");
                self.problems = vec![e];
                Some(msg)
            }
            Polled::Missing if self.editor.layout().monitors.is_empty() => None,
            Polled::Missing => match self.store.save(self.editor.layout()) {
                Ok(_) => {
                    self.problems.clear();
                    self.source = LoadSource::Main;
                    Some(format!("{at}: removed; wrote the layout shown back"))
                }
                Err(e) => Some(format!("{at}: removed, and not written back: {e}")),
            },
        }
    }

    /// Brings each desktop widget's size down to the largest its type allows (`largest`, by
    /// type name; `None` for a type without a limit): a layout written when the limit was
    /// higher, or by hand, shows the widget no larger than the edit mode would let it grow.
    /// The file keeps what it holds until the next edit writes the layout. Returns whether a
    /// size changed; the generation then moves on.
    pub fn cap_desktop_sizes(&mut self, largest: impl Fn(&str) -> Option<(u32, u32)>) -> bool {
        let changed = self.editor.amend(|l| {
            let mut changed = false;
            for wd in l
                .monitors
                .iter_mut()
                .flat_map(|m| m.containers.iter_mut())
                .flat_map(|c| c.widgets.iter_mut())
            {
                let Place::Desktop {
                    size: Some((w, h)), ..
                } = &mut wd.place
                else {
                    continue;
                };
                let Some((mw, mh)) = largest(&wd.r#type) else {
                    continue;
                };
                if *w > mw || *h > mh {
                    (*w, *h) = ((*w).min(mw), (*h).min(mh));
                    changed = true;
                }
            }
            changed
        });
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Applies `edits` as one undo step and writes the file. A failed write leaves the edit
    /// applied (the shell keeps showing it) and is returned as the message for the log.
    pub fn apply(&mut self, edits: &[Edit]) -> Result<Result<bool, String>, EditError> {
        let changed = self.editor.apply_all(edits)?;
        if !changed {
            return Ok(Ok(false));
        }
        self.generation += 1;
        Ok(match self.store.save(self.editor.layout()) {
            Ok(_) => Ok(true),
            Err(e) => Err(format!("{}: {e}", self.store.path().display())),
        })
    }

    /// Steps back one undo step and writes the file; `Ok(false)` when there is none. As for
    /// [`Live::apply`], a failed write keeps the step taken and is returned for the log.
    pub fn undo(&mut self) -> Result<bool, String> {
        let stepped = self.editor.undo();
        self.saved_step(stepped)
    }

    /// Redoes one undone step and writes the file; `Ok(false)` when there is none.
    pub fn redo(&mut self) -> Result<bool, String> {
        let stepped = self.editor.redo();
        self.saved_step(stepped)
    }

    fn saved_step(&mut self, stepped: bool) -> Result<bool, String> {
        if !stepped {
            return Ok(false);
        }
        self.generation += 1;
        match self.store.save(self.editor.layout()) {
            Ok(_) => Ok(true),
            Err(e) => Err(format!("{}: {e}", self.store.path().display())),
        }
    }

    /// The monitor index edits for an output go to: a monitor without its own layout gets a
    /// copy of the primary one first ([`Editor::attach`]), so editing it leaves the others be.
    /// Attaching a monitor that already has its layout changes nothing, so the generation
    /// stays: the edit controller attaches on every pointer motion of a drag.
    pub fn attach(&mut self, connector: &str) -> usize {
        let revision = self.editor.revision();
        let id = self.output(connector);
        let i = self.editor.attach(&[id])[0];
        if self.editor.revision() != revision {
            self.note_seen();
            self.generation += 1;
        }
        i
    }

    /// Whether there is no layout yet: no file, no previous one, nothing unreadable in the way.
    /// The shell then makes the first one ([`Live::seed`]).
    pub fn first_run(&self) -> bool {
        self.source == LoadSource::Empty
            && self.problems.is_empty()
            && !self.store.path().exists()
            && !self.store.previous_path().exists()
    }

    /// Makes `layout` the first layout and writes it, when there is none yet
    /// ([`Live::first_run`]); `Ok(false)` leaves an existing layout alone.
    pub fn seed(&mut self, layout: Layout) -> std::io::Result<bool> {
        if !self.first_run() {
            return Ok(false);
        }
        self.store.save(&layout)?;
        self.editor = Editor::new(layout);
        self.source = LoadSource::Main;
        self.generation += 1;
        Ok(true)
    }

    pub fn path(&self) -> &Path {
        self.store.path()
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn problems(&self) -> &[LoadError] {
        &self.problems
    }

    pub fn source(&self) -> LoadSource {
        self.source
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The connected output named `connector`, with its EDID when the compositor gave one.
    fn output(&self, connector: &str) -> MonitorId {
        self.outputs
            .iter()
            .find(|o| o.connector == connector)
            .cloned()
            .unwrap_or_else(|| MonitorId {
                connector: connector.to_string(),
                edid: None,
            })
    }

    /// The monitor layout an output shows: its own, matched by EDID and then connector
    /// ([`super::Layout::resolve`]), otherwise the primary one, which is what a never-seen
    /// monitor starts from. Before any outputs are known, by connector alone.
    pub fn monitor(&self, connector: &str) -> Option<&MonitorLayout> {
        let l = self.editor.layout();
        let i = match self.outputs.iter().position(|o| o.connector == connector) {
            Some(oi) => l.shown(&self.outputs)[oi],
            None => l
                .monitors
                .iter()
                .position(|m| m.connector == connector)
                .or_else(|| l.primary()),
        }?;
        l.monitors.get(i)
    }

    /// The containers of `kind` of unplugged monitors that output `connector` shows until they
    /// come back, each with the monitor it belongs to ([`super::Layout::guests`]).
    pub fn guests(&self, connector: &str, kind: &str) -> Vec<(&MonitorLayout, &Container)> {
        let l = self.editor.layout();
        let Some((host, gone)) = l.guests(&self.outputs, &self.seen) else {
            return Vec::new();
        };
        if self.outputs[host].connector != connector {
            return Vec::new();
        }
        gone.into_iter()
            .map(|i| &l.monitors[i])
            .filter_map(|m| {
                let c = m.containers.iter().find(|c| c.kind.name() == kind)?;
                Some((m, c))
            })
            .collect()
    }

    /// The first container of `kind` ("panel", "desktop", "sidebar") on the monitor.
    pub fn container(&self, connector: &str, kind: &str) -> Option<&Container> {
        self.monitor(connector)?
            .containers
            .iter()
            .find(|c| c.kind.name() == kind)
    }

    /// The container `id` on the monitor, when it is of `kind`.
    pub fn container_by_id(&self, connector: &str, kind: &str, id: &str) -> Option<&Container> {
        self.monitor(connector)?
            .containers
            .iter()
            .find(|c| c.id == id && c.kind.name() == kind)
    }

    /// Whether any monitor has a container of `kind`.
    pub fn has(&self, kind: &str) -> bool {
        self.editor
            .layout()
            .monitors
            .iter()
            .any(|m| m.containers.iter().any(|c| c.kind.name() == kind))
    }
}
