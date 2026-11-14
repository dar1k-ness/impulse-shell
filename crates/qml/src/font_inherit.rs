//! The font of a Qt Quick Controls type: the fields nothing set in it are those of the
//! control, popup or window it is in (QQuickControlPrivate::parentFont, QFont::resolve), and
//! `font = undefined` leaves none of them set (QQuickControl::resetFont).

use crate::runtime::{Sym, World};
use crate::value::Value;
use crate::valuetypes::FONT_FIELDS;
use std::cell::RefCell;
use std::collections::HashMap;

/// The fields QFont::resolve takes together: a size is one in points or in pixels, a weight
/// is what `bold` reads.
fn unit(field: &str) -> &'static [&'static str] {
    match field {
        "pointSize" | "pixelSize" => &["font.pointSize", "font.pixelSize"],
        "bold" | "weight" => &["font.bold", "font.weight"],
        _ => &[],
    }
}

/// The fields that are one value each (`features` and `variableAxes` are maps).
fn scalars() -> impl Iterator<Item = &'static str> {
    (FONT_FIELDS.iter().copied()).filter(|f| !matches!(*f, "features" | "variableAxes"))
}

bits::bit_flags! {
const TAKES: u8 = 1;
pub(crate) const WINDOW: u8 = 2;
pub(crate) const POPUP: u8 = 4;
/// A type of Qt Quick Controls: what nothing set in it is the style's.
pub(crate) const STYLED: u8 = 8;
}

thread_local!(static KINDS: RefCell<HashMap<Vec<Sym>, u8>> = RefCell::new(HashMap::new()));

impl World {
    /// What `item` is to a font and a palette: a type of Qt Quick Controls (`STYLED`), one
    /// with a font (`TAKES`), a window, a popup.
    pub(crate) fn control_kind(&self, item: usize) -> u8 {
        let Some(inst) = self.items.get(item) else {
            return 0;
        };
        if let Some(k) = KINDS.with_borrow(|m| m.get(&inst.types[..]).copied()) {
            return k;
        }
        let r = crate::types::registry();
        let controls = inst.types.iter().any(|t| {
            r.resolve(t).is_some_and(|(_, ti)| {
                (r.chain(&ti.cpp).iter())
                    .any(|c| c.exports.iter().any(|e| e.0 == "QtQuick.Templates"))
            })
        });
        let mut k = if controls { STYLED } else { 0 };
        if controls && inst.prop_info("font").is_some_and(|p| p.ty == "QFont") {
            k |= TAKES;
        }
        if crate::types::inherits(&inst.types, "QQuickWindow") {
            k |= WINDOW;
        }
        if crate::types::is_popup(&inst.types) {
            k |= POPUP;
        }
        KINDS.with_borrow_mut(|m| m.insert(inst.types.clone(), k));
        k
    }

    /// The object `item` takes the fields of its font from: the nearest control, label, text
    /// field or popup it is in, else its ApplicationWindow; a popup takes them from its
    /// window only.
    fn font_giver(&self, item: usize) -> Option<usize> {
        let popup = self.control_kind(item) & POPUP != 0;
        let mut p = self.items.get(item)?.parent;
        while let Some(i) = p {
            let k = self.control_kind(i);
            if k & WINDOW != 0 {
                return (k & TAKES != 0).then_some(i);
            }
            if k & TAKES != 0 && !popup {
                return Some(i);
            }
            p = self.items[i].parent;
        }
        None
    }

    /// Whether the font of `item` is one value (`font: Qt.font(...)`, `font: other.font`),
    /// not fields.
    fn font_whole(&self, item: usize) -> bool {
        match self.items[item].props.get("font").map(|p| &p.value) {
            Some(Value::Obj(_)) => true,
            Some(Value::Item(g)) => self.groups.get(g).is_none_or(|(o, _)| *o != item),
            _ => false,
        }
    }

    fn font_set(&self, item: usize, key: &str) -> bool {
        self.items[item].props.contains_key(key) || self.is_assigned(item, key)
    }

    /// The object whose font gives `item` field `name` (`font.pixelSize`), when nothing set
    /// it in `item`.
    pub(crate) fn font_from(&self, item: usize, name: &str) -> Option<usize> {
        if self.control_kind(item) & TAKES == 0 {
            return None;
        }
        let keys = unit(&name["font.".len()..]);
        let set = |i: usize| {
            self.font_whole(i) || self.font_set(i, name) || keys.iter().any(|k| self.font_set(i, k))
        };
        if set(item) {
            return None;
        }
        let mut g = self.font_giver(item)?;
        while !set(g) {
            g = self.font_giver(g)?;
        }
        Some(g)
    }

    /// Field `name` of the font of `item`, which holds it.
    pub(crate) fn font_held(&mut self, item: usize, name: &str) -> Option<Value> {
        match self.items[item].props.get("font").map(|p| p.value.clone()) {
            Some(Value::Obj(o)) => o.borrow().props.get(&name["font.".len()..]).cloned(),
            Some(Value::Item(g)) => match self.groups.get(&g) {
                Some(&(o, _)) if o != item => self.read(o, name),
                _ => self.read(item, name),
            },
            _ => self.read(item, name),
        }
    }

    /// The fields of the font of `item` as a script reads them.
    fn font_shown(&mut self, item: usize) -> Vec<Value> {
        let script = std::mem::replace(&mut self.script_read, true);
        let reading = self.reading.take();
        let shown = scalars()
            .map(|f| (self.font_held(item, &format!("font.{f}"))).unwrap_or(Value::Undefined))
            .collect();
        self.reading = reading;
        self.script_read = script;
        shown
    }

    /// Tells that the fields of the font of `item` that are not what `was` has changed, and
    /// the font with them.
    fn font_changed(&mut self, item: usize, was: &[Value]) -> bool {
        let now = self.font_shown(item);
        let mut any = false;
        for (f, (a, b)) in scalars().zip(was.iter().zip(&now)) {
            if !a.strict_eq(b) {
                self.notify(item, &format!("font.{f}"));
                any = true;
            }
        }
        if any {
            self.font_notify(item);
        }
        any
    }

    fn font_notify(&mut self, item: usize) {
        self.notify(item, "font");
        if self.text_measure.is_some()
            && (self.items[item].types.iter()).any(|t| crate::runtime::measured_text(t))
        {
            self.text_dirty.insert(item);
        }
    }

    /// QQuickControl::resetFont: no field of the font of `item` is a set one, and no binding
    /// sets one.
    pub(crate) fn font_reset(&mut self, item: usize) {
        let was = self.font_shown(item);
        for f in FONT_FIELDS {
            let k = format!("font.{f}");
            self.clear_binding(item, &k);
            self.items[item].props.remove(&k);
            self.assigned.remove(&(item, Sym::new(&k)));
        }
        // What the property holds is its fields again, whatever a binding or the document
        // left in it (a whole font, `undefined`).
        if self.items[item].props.contains_key("font") {
            let g = self.group(item, "font");
            match self.items[item].props.get_mut("font") {
                Some(p) if p.binding.is_some() => p.value = Value::Item(g),
                _ => {
                    self.items[item].props.remove("font");
                }
            }
        }
        if self.font_changed(item, &was) {
            self.font_spread_all(item);
        }
    }

    /// The font of `item` changed in `fields`: the fonts of the controls in it that take
    /// them from it changed too (QQuickControlPrivate::updateFontRecur).
    pub(crate) fn font_spread(&mut self, item: usize, fields: &[&str]) {
        let kind = self.control_kind(item);
        if kind & (TAKES | POPUP) == 0 || self.items[item].children.is_empty() {
            return;
        }
        let window = kind & WINDOW != 0;
        let mut told: Vec<usize> = Vec::new();
        for f in fields {
            let name = format!("font.{f}");
            let keys = unit(f);
            let mut stack: Vec<(usize, bool)> = (self.items[item].children.iter())
                .map(|&c| (c, true))
                .collect();
            while let Some((d, mut follows)) = stack.pop() {
                if self.dead.contains(&d) {
                    continue;
                }
                let k = self.control_kind(d);
                // A popup takes its font from the window, whatever control it is in.
                if k & WINDOW != 0 || (k & POPUP != 0 && !window) {
                    continue;
                }
                if k & POPUP != 0 {
                    follows = true;
                }
                if k & TAKES != 0 && follows {
                    if self.font_whole(d)
                        || self.font_set(d, &name)
                        || keys.iter().any(|k| self.font_set(d, k))
                    {
                        follows = false;
                    } else {
                        self.notify(d, &name);
                        for k in keys.iter().filter(|k| **k != name) {
                            self.notify(d, k);
                        }
                        if !told.contains(&d) {
                            told.push(d);
                        }
                    }
                }
                if follows || window {
                    stack.extend(self.items[d].children.iter().map(|&c| (c, follows)));
                }
            }
        }
        for d in told {
            self.font_notify(d);
        }
    }

    /// [`World::font_spread`] of every field.
    pub(crate) fn font_spread_all(&mut self, item: usize) {
        let fields: Vec<&str> = scalars().collect();
        self.font_spread(item, &fields);
    }

    /// Whether a font above `p` (and `p` itself) has a field that is set.
    fn font_set_above(&self, mut p: Option<usize>) -> bool {
        while let Some(i) = p {
            if self.control_kind(i) & TAKES != 0
                && (self.font_whole(i)
                    || (FONT_FIELDS.iter()).any(|f| self.font_set(i, &format!("font.{f}"))))
            {
                return true;
            }
            p = self.items[i].parent;
        }
        false
    }

    /// The controls in `c` (and `c`) whose font takes fields from around `c`, each with its
    /// font, when `c` moves from under `from` to under `to`: `None` when no font around is set.
    pub(crate) fn font_takers(
        &mut self,
        c: usize,
        from: Option<usize>,
        to: Option<usize>,
    ) -> Vec<(usize, Vec<Value>)> {
        if self.control_kind(c) & (WINDOW | POPUP) != 0
            || !(self.font_set_above(from) || self.font_set_above(to))
        {
            return Vec::new();
        }
        let mut takers = Vec::new();
        let mut stack = vec![c];
        while let Some(d) = stack.pop() {
            let k = self.control_kind(d);
            if self.dead.contains(&d) || (d != c && k & (WINDOW | POPUP) != 0) {
                continue;
            }
            if k & TAKES != 0 {
                takers.push((d, self.font_shown(d)));
            }
            stack.extend(self.items[d].children.iter().copied());
        }
        takers
    }

    /// The fonts [`World::font_takers`] gave, after the move.
    pub(crate) fn font_moved(&mut self, takers: Vec<(usize, Vec<Value>)>) {
        for (d, was) in takers {
            self.font_changed(d, &was);
        }
    }
}
