//! The palette of an item, a popup and a window (QQuickPalette): an object with a colour for
//! each role in three groups. A colour nothing set is the one set in what the object is in
//! (QQuickPaletteProviderPrivateBase::inheritPalette), else the application's, or the Basic
//! style's for a type of Qt Quick Controls (QQuickTheme::palette): `qt_palette.tsv`, written
//! from what `tests/unset_defaults/palette.qml` gives in Qt 6.12. The Basic style has a second
//! set of colours for a dark system (QQuickBasicTheme: QPlatformTheme::colorScheme is Dark).
//!
//! A file of the Basic style binds some colours of its type to the palette (TextField.qml:
//! `color: control.palette.text`): `qt_style_palette.tsv` is those lines of Qt 6.12's
//! QtQuick/Controls/Basic/*.qml, and such a property nothing set is the colour of its role.
//!
//! A colour that is set is a property of the owner: `palette.button` in every group,
//! `palette.disabled.button` in one, which goes first.

use crate::color::Color;
use crate::font_inherit::{POPUP, STYLED, WINDOW};
use crate::runtime::{Prop, Sym, World};
use crate::value::Value;
use std::sync::OnceLock;

const GROUPS: [&str; 3] = ["active", "inactive", "disabled"];
const DISABLED: usize = 2;

/// A role and its colours nothing set: the application's by group, then the style's, then
/// the style's in a dark system (none for a role the style leaves to the application).
type Line = (&'static str, [Color; 6], [Option<Color>; 3]);

fn table() -> &'static [Line] {
    static TABLE: OnceLock<Vec<Line>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let colour = |s: &str| {
            let hex = s.strip_prefix('#').expect(s);
            let v = u32::from_str_radix(hex, 16).expect(s);
            Color::from_argb32(if hex.len() == 6 { 0xff00_0000 | v } else { v })
        };
        (include_str!("qt_palette.tsv").lines())
            .map(|l| {
                let mut cells = l.split('\t');
                let role = cells.next().expect(l);
                let light = [(); 6].map(|()| colour(cells.next().expect(l)));
                let dark = [(); 3].map(|()| Some(cells.next().expect(l)));
                let dark = dark.map(|c| c.filter(|c| *c != "-").map(colour));
                (role, light, dark)
            })
            .collect()
    })
}

/// The properties a Basic style file binds to a palette role, as (type, property, the
/// `palette.<role>` it reads).
fn style_bindings() -> &'static [(Sym, &'static str, String)] {
    static TABLE: OnceLock<Vec<(Sym, &'static str, String)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (include_str!("qt_style_palette.tsv").lines())
            .map(|l| {
                let (name, role) = l.split_once('\t').expect(l);
                let (ty, prop) = name.split_once('.').expect(l);
                (Sym::new(ty), prop, format!("palette.{role}"))
            })
            .collect()
    })
}

fn role_at(role: &str) -> Option<usize> {
    table().iter().position(|l| l.0 == role)
}

/// The group and role `name` is the colour of: `palette.button` (every group),
/// `palette.disabled.button`.
fn colour_of(name: &str) -> Option<(Option<usize>, usize)> {
    let rest = name.strip_prefix("palette.")?;
    match rest.split_once('.') {
        Some((group, role)) => Some((
            Some(GROUPS.iter().position(|g| *g == group)?),
            role_at(role)?,
        )),
        None => Some((None, role_at(rest)?)),
    }
}

fn key(group: Option<usize>, role: usize) -> String {
    let role = table()[role].0;
    match group {
        Some(g) => format!("palette.{}.{role}", GROUPS[g]),
        None => format!("palette.{role}"),
    }
}

/// The colours of every group of one role.
type Shown = [Color; 3];

/// An item below one whose colours changed: the (group, role) it may take, and whether it
/// is in a popup.
type Below = (usize, Vec<(usize, usize)>, bool);

impl World {
    fn has_palette(&self, item: usize) -> bool {
        (self.items.get(item))
            .and_then(|i| i.prop_info("palette"))
            .is_some_and(|p| p.ty == "QQuickPalette*")
    }

    /// The objects whose set colours `item` has, nearest first, `item` itself the first:
    /// the items it is in; for a popup its window; a window has its own only. What is in a
    /// popup has the colours set in the popup and in its ApplicationWindow.
    fn palette_givers(&self, item: usize) -> Vec<usize> {
        let mut givers = vec![item];
        let mut at = item;
        loop {
            let kind = self.control_kind(at);
            if kind & WINDOW != 0 {
                break;
            }
            let popup = kind & POPUP != 0;
            let mut p = self.items[at].parent;
            while let Some(i) = p
                && if popup {
                    self.control_kind(i) & WINDOW == 0
                } else {
                    !self.has_palette(i)
                }
            {
                p = self.items[i].parent;
            }
            match p {
                Some(i) if !popup || at == item || self.control_kind(i) & STYLED != 0 => {
                    givers.push(i);
                    at = i;
                }
                _ => break,
            }
        }
        givers
    }

    /// The colour of `role` in `group` that is set in `item` itself.
    fn palette_own(&mut self, item: usize, group: usize, role: usize) -> Option<Color> {
        for k in [key(Some(group), role), key(None, role)] {
            if !self.items[item].props.contains_key(&k) {
                continue;
            }
            // What the property holds, not what a script reads there: while its binding has
            // given no colour a script reads the colour this looks for.
            let script = std::mem::take(&mut self.script_read);
            let held = self.read(item, &k);
            self.script_read = script;
            match held {
                None | Some(Value::Undefined) => {}
                Some(v) => return Some(crate::builtins::to_color(&v)),
            }
        }
        None
    }

    /// The colour of `role` in `group` that is set in `item` or in what it is in.
    fn palette_set(&mut self, item: usize, group: usize, role: usize) -> Option<Color> {
        for g in self.palette_givers(item) {
            if g != item
                && let Some(taken) = self.palette_taken.get(&item)
            {
                let took = taken.iter().find(|t| (t.0, t.1) == (group, role));
                return took.map(|t| t.2);
            }
            if let Some(c) = self.palette_own(g, group, role) {
                return Some(c);
            }
        }
        None
    }

    /// The colours of `window` are about to change: a popup made in it keeps the ones it
    /// has from it. As measured in Qt 6.12, a popup takes the colours set in its window
    /// when it is made and when its own are written, not when the window's change; the
    /// popups of an ApplicationWindow follow it.
    fn palette_keep(&mut self, window: usize) {
        let kind = self.control_kind(window);
        if kind & WINDOW == 0 || kind & STYLED != 0 {
            return;
        }
        let reading = self.reading.take();
        let mut set = None;
        // A popup of the component being created is not made yet.
        let made = match self.behaviors_live {
            true => self.items.len(),
            false => self.items.len().min(self.creating_from),
        };
        for popup in 0..made {
            if self.dead.contains(&popup)
                || self.palette_taken.contains_key(&popup)
                || self.control_kind(popup) & POPUP == 0
                || self.palette_givers(popup).get(1) != Some(&window)
            {
                continue;
            }
            if set.is_none() {
                let mut colours = Vec::new();
                for r in 0..table().len() {
                    for g in 0..3 {
                        colours.extend(self.palette_own(window, g, r).map(|c| (g, r, c)));
                    }
                }
                set = Some(colours);
            }
            self.palette_taken
                .insert(popup, set.clone().unwrap_or_default());
        }
        self.reading = reading;
    }

    fn palette_colour(&mut self, item: usize, group: usize, role: usize) -> Color {
        if let Some(c) = self.palette_set(item, group, role) {
            return c;
        }
        let styled = self.control_kind(item) & STYLED != 0;
        // QPalette::Link of the application is the platform theme's.
        if !styled && table()[role].0 == "link" {
            let [r, g, b, a] = crate::colorscheme::link_color();
            return Color::from_argb32(u32::from_be_bytes([a, r, g, b]));
        }
        let line = &table()[role];
        match line.2[group] {
            Some(dark) if styled && crate::colorscheme::system_dark() => dark,
            _ => line.1[usize::from(styled) * 3 + group],
        }
    }

    /// The colour of property `name` of `item` while nothing sets it, for a property the
    /// style file of the item's type binds to a role of its palette (`selectionColor` of a
    /// TextField is `palette.highlight`); `None` for any other property or type.
    pub fn style_colour(&mut self, item: usize, name: &str) -> Option<Value> {
        let types = &self.items.get(item)?.types;
        let role = (style_bindings().iter())
            .find(|(ty, prop, _)| *prop == name && types.contains(ty))
            .map(|b| b.2.as_str())?;
        self.palette_read(item, role)
    }

    /// The group whose colours the palette of `item` itself has: `disabled` in what is not
    /// enabled.
    fn palette_group(&mut self, item: usize) -> usize {
        match self.read(item, "enabled") {
            Some(Value::Bool(false)) => DISABLED,
            _ => 0,
        }
    }

    /// What a script reads for `name` (`palette`, `palette.disabled`, `palette.button`,
    /// `palette.disabled.button`) of `item`.
    pub(crate) fn palette_read(&mut self, item: usize, name: &str) -> Option<Value> {
        if !self.has_palette(item) {
            return None;
        }
        let rest = name.strip_prefix("palette")?;
        if rest.is_empty() || (rest.strip_prefix('.')).is_some_and(|g| GROUPS.contains(&g)) {
            self.palette_made.insert(item);
            return Some(Value::Item(self.group(item, name)));
        }
        let (group, role) = colour_of(name)?;
        if let Some(r) = &mut self.reading {
            r.push((item, Sym::new(name)));
        }
        let group = match group {
            Some(g) => g,
            None => self.palette_group(item),
        };
        Some(Value::Color(self.palette_colour(item, group, role)))
    }

    fn palette_shown(&mut self, item: usize, roles: &[usize]) -> Vec<Shown> {
        let reading = self.reading.take();
        let shown = (roles.iter())
            .map(|&r| [0, 1, 2].map(|g| self.palette_colour(item, g, r)))
            .collect();
        self.reading = reading;
        shown
    }

    /// Tells that the colours of `roles` of `item` that are not what `was` has changed, the
    /// palette with them, and the palettes of what takes them from `item`.
    fn palette_changed(&mut self, item: usize, roles: &[usize], was: &[Shown]) -> bool {
        let now = self.palette_shown(item, roles);
        let mut changed: Vec<(usize, usize)> = Vec::new();
        for (&role, (a, b)) in roles.iter().zip(was.iter().zip(&now)) {
            changed.extend((0..3).filter(|&g| a[g] != b[g]).map(|g| (g, role)));
        }
        if changed.is_empty() {
            return false;
        }
        self.palette_notify(item, &changed);
        // QQuickPaletteProviderPrivateBase::updateChildrenPalettes.
        let window = self.control_kind(item) & WINDOW != 0;
        let styled = self.control_kind(item) & STYLED != 0;
        let mut stack: Vec<Below> = (self.items[item].children.iter())
            .map(|&c| (c, changed.clone(), false))
            .collect();
        while let Some((d, mut follows, in_popup)) = stack.pop() {
            if self.dead.contains(&d) {
                continue;
            }
            let kind = self.control_kind(d);
            // A window has its own colours; a popup those of its window, and what is in the
            // popup those of an ApplicationWindow.
            if kind & WINDOW != 0
                || (kind & POPUP != 0 && (!window || self.palette_taken.contains_key(&d)))
                || (in_popup && !styled)
            {
                continue;
            }
            if self.has_palette(d) {
                let props = &self.items[d].props;
                follows.retain(|&(g, r)| {
                    !props.contains_key(&key(Some(g), r)) && !props.contains_key(&key(None, r))
                });
                if follows.is_empty() {
                    continue;
                }
                self.palette_notify(d, &follows);
            }
            let in_popup = in_popup || kind & POPUP != 0;
            let children = self.items[d].children.iter();
            stack.extend(children.map(|&c| (c, follows.clone(), in_popup)));
        }
        true
    }

    fn palette_notify(&mut self, item: usize, changed: &[(usize, usize)]) {
        let mut last = None;
        for &(g, r) in changed {
            self.notify(item, &key(Some(g), r));
            if last != Some(r) {
                self.notify(item, &key(None, r));
                self.style_notify(item, table()[r].0);
                last = Some(r);
            }
        }
        self.notify(item, "palette");
    }

    /// The properties of `item` that nothing set and its style file binds to `role` changed.
    fn style_notify(&mut self, item: usize, role: &str) {
        for (ty, prop, read) in style_bindings() {
            let inst = &self.items[item];
            if read.strip_prefix("palette.") == Some(role)
                && inst.types.contains(ty)
                && !inst.props.contains_key(prop)
            {
                self.notify(item, prop);
                // What is drawn in that colour is drawn again, as after a write of it.
                self.touched.push(item);
            }
        }
    }

    /// Takes the set colour `k` of `item` away. The binding that is being reset stays, with
    /// no colour.
    fn palette_drop(&mut self, item: usize, k: &str, resets: &str) {
        match self.items[item].props.get_mut(k) {
            Some(p) if k == resets && p.binding.is_some() => p.value = Value::Undefined,
            Some(_) => {
                self.clear_binding(item, k);
                self.items[item].props.remove(k);
            }
            None => {}
        }
    }

    fn palette_put(&mut self, item: usize, k: &str, c: Color) {
        let p = (self.items[item].props)
            .entry_sym(Sym::new(k))
            .or_insert_with(|| Prop {
                ty: Sym::new("color"),
                ..Default::default()
            });
        p.value = Value::Color(c);
    }

    /// A write of `v` to `name` of `item`, when that is a colour of its palette or the
    /// palette: whether anything changed. QQuickColorGroup::setColor: a colour written to
    /// the palette is the colour of every group.
    pub(crate) fn palette_write(&mut self, item: usize, name: &str, v: &Value) -> Option<bool> {
        if !self.has_palette(item) {
            return None;
        }
        if name == "palette" {
            return self.palette_take(item, v);
        }
        // A group is the object it is, whatever is written to it.
        if (name.strip_prefix("palette.")).is_some_and(|g| GROUPS.contains(&g)) {
            tracing::warn!(target: "ri::binding", "Unable to assign to QQuickColorGroup*");
            return Some(false);
        }
        let (group, role) = colour_of(name)?;
        let c = crate::builtins::to_color(v);
        self.palette_keep(item);
        // A popup whose colour is written takes the colours of its window again.
        let roles: Vec<usize> = match self.palette_taken.contains_key(&item) {
            true => (0..table().len()).collect(),
            false => vec![role],
        };
        let was = self.palette_shown(item, &roles);
        self.palette_taken.remove(&item);
        if group.is_none() {
            for g in 0..3 {
                self.palette_drop(item, &key(Some(g), role), name);
            }
        }
        self.palette_put(item, name, c);
        Some(self.palette_changed(item, &roles, &was))
    }

    /// QQuickPaletteProviderPrivateBase::setPalette: the colours set in the palette `v`, or
    /// in what its owner is in, are the ones set in `item`. What is no palette leaves the
    /// one there is (Qt refuses it: "Cannot assign int to QQuickPalette*").
    fn palette_take(&mut self, item: usize, v: &Value) -> Option<bool> {
        let from = match v {
            Value::Item(g) => match self.groups.get(g) {
                Some((o, name)) if name == "palette" && self.has_palette(*o) => *o,
                _ => return Some(false),
            },
            Value::Null => {
                tracing::warn!(target: "ri::binding", "Palette cannot be null.");
                return Some(false);
            }
            _ => return Some(false),
        };
        self.palette_keep(item);
        let roles: Vec<usize> = (0..table().len()).collect();
        let was = self.palette_shown(item, &roles);
        self.palette_taken.remove(&item);
        let reading = self.reading.take();
        let mut set = Vec::new();
        for &r in &roles {
            for g in 0..3 {
                if let Some(c) = self.palette_set(from, g, r) {
                    set.push((g, r, c));
                }
            }
        }
        self.reading = reading;
        for &r in &roles {
            self.palette_drop(item, &key(None, r), "");
            for g in 0..3 {
                self.palette_drop(item, &key(Some(g), r), "");
            }
        }
        for (g, r, c) in set {
            self.palette_put(item, &key(Some(g), r), c);
        }
        Some(self.palette_changed(item, &roles, &was))
    }

    /// `undefined` in `name` of `item`, when that is its palette, a group or a colour of it:
    /// the colours it names are no longer set ones (QQuickItem::resetPalette,
    /// QQuickColorGroup::resetColor). A popup keeps them.
    pub(crate) fn palette_reset(&mut self, item: usize, name: &str) -> bool {
        if !self.has_palette(item) {
            return false;
        }
        let rest = name.strip_prefix("palette").unwrap_or("x");
        let (group, roles): (Option<usize>, Vec<usize>) = match rest.strip_prefix('.') {
            None if rest.is_empty() => (None, (0..table().len()).collect()),
            Some(g) if GROUPS.contains(&g) => (
                GROUPS.iter().position(|x| *x == g),
                (0..table().len()).collect(),
            ),
            _ => match colour_of(name) {
                Some((group, role)) => (group, vec![role]),
                None => return false,
            },
        };
        // What the document left in the property itself (`palette: undefined`).
        if rest.is_empty() && self.items[item].props.contains_key("palette") {
            let g = self.group(item, "palette");
            match self.items[item].props.get_mut("palette") {
                Some(p) if p.binding.is_some() => p.value = Value::Item(g),
                _ => {
                    self.items[item].props.remove("palette");
                }
            }
        }
        if self.control_kind(item) & POPUP != 0 {
            return true;
        }
        self.palette_keep(item);
        let was = self.palette_shown(item, &roles);
        for &r in &roles {
            let every = key(None, r);
            match group {
                None => {
                    for g in 0..3 {
                        self.palette_drop(item, &key(Some(g), r), name);
                    }
                }
                // The colour of every group stays the colour of the other two.
                Some(g) => {
                    for h in (0..3).filter(|h| *h != g) {
                        if let Some(c) = self.palette_own(item, h, r) {
                            self.palette_put(item, &key(Some(h), r), c);
                        }
                    }
                    self.palette_drop(item, &key(Some(g), r), name);
                }
            }
            self.palette_drop(item, &every, name);
        }
        self.palette_changed(item, &roles, &was);
        true
    }

    /// `item` is enabled or no longer is: its palette has the colours of another group
    /// (QQuickItemPrivate::setEffectiveEnableRecur), when it has a palette object.
    pub(crate) fn palette_enabled(&mut self, item: usize) {
        if self.palette_made.contains(&item)
            || (self.control_kind(item) & STYLED != 0 && self.has_palette(item))
        {
            for (r, (role, ..)) in table().iter().enumerate() {
                self.notify(item, &format!("palette.{role}"));
                // A binding to the role gives a signal when its colour is another one.
                if self.palette_colour(item, 0, r) != self.palette_colour(item, DISABLED, r) {
                    self.style_notify(item, role);
                }
            }
            self.notify(item, "palette");
        }
    }

    /// The items in `c` (and `c`), each with its colours, when `c` moves from under `from`
    /// to under `to` and a colour is set around either.
    pub(crate) fn palette_takers(
        &mut self,
        c: usize,
        from: Option<usize>,
        to: Option<usize>,
    ) -> Vec<(usize, Vec<Shown>)> {
        if self.palette_made.is_empty() || self.control_kind(c) & (WINDOW | POPUP) != 0 {
            return Vec::new();
        }
        let set_above = |w: &Self, p: Option<usize>| {
            p.is_some_and(|p| {
                (w.palette_givers(p).into_iter())
                    .any(|g| w.items[g].props.keys().any(|k| k.starts_with("palette.")))
            })
        };
        if !(set_above(self, from) || set_above(self, to)) {
            return Vec::new();
        }
        let roles: Vec<usize> = (0..table().len()).collect();
        let mut takers = Vec::new();
        let mut stack = vec![c];
        while let Some(d) = stack.pop() {
            if self.dead.contains(&d) || (d != c && self.control_kind(d) & (WINDOW | POPUP) != 0) {
                continue;
            }
            if self.palette_made.contains(&d) {
                takers.push((d, self.palette_shown(d, &roles)));
            }
            stack.extend(self.items[d].children.iter().copied());
        }
        takers
    }

    /// The palettes [`World::palette_takers`] gave, after the move.
    pub(crate) fn palette_moved(&mut self, takers: Vec<(usize, Vec<Shown>)>) {
        let roles: Vec<usize> = (0..table().len()).collect();
        for (d, was) in takers {
            let now = self.palette_shown(d, &roles);
            let mut changed: Vec<(usize, usize)> = Vec::new();
            for (&role, (a, b)) in roles.iter().zip(was.iter().zip(&now)) {
                changed.extend((0..3).filter(|&g| a[g] != b[g]).map(|g| (g, role)));
            }
            if !changed.is_empty() {
                self.palette_notify(d, &changed);
            }
        }
    }
}
