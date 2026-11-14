//! Key delivery after QQuickDeliveryAgent: the window's active focus item gets `Keys.onPressed`
//! first, then its own editing (TextInput/TextEdit), then unaccepted events climb to parents.
use qml::runtime::{Runtime, TextOp, World};
use qml::textpos;
use qml::value::Value;
use xkbcommon::xkb;

use crate::scene::{explicit, is, is_text_edit, kids, num, truthy};
use crate::textmouse::{next_word, prev_word};

bits::bit_flags! {
const SHIFT: u32 = 0x0200_0000;
const CTRL: u32 = 0x0400_0000;
const ALT: u32 = 0x0800_0000;
const META: u32 = 0x1000_0000;
}

const KEY_A: u32 = 0x41;
const KEY_C: u32 = 0x43;
const KEY_X: u32 = 0x58;
const KEY_INSERT: u32 = 0x0100_0006;
const KEY_F16: u32 = 0x0100_003f;
const KEY_F20: u32 = 0x0100_0043;
const KEY_V: u32 = 0x56;
const KEY_Y: u32 = 0x59;
const KEY_Z: u32 = 0x5a;
const KEY_F18: u32 = 0x0100_0041;
const KEY_D: u32 = 0x44;
const KEY_E: u32 = 0x45;
const KEY_K: u32 = 0x4b;
const KEY_U: u32 = 0x55;
const KEY_BACKSPACE: u32 = 0x0100_0003;
const KEY_TAB: u32 = 0x0100_0001;
const KEY_BACKTAB: u32 = 0x0100_0002;
const KEY_RETURN: u32 = 0x0100_0004;
const KEY_ENTER: u32 = 0x0100_0005;
const KEY_DELETE: u32 = 0x0100_0007;
const KEY_HOME: u32 = 0x0100_0010;
const KEY_END: u32 = 0x0100_0011;
const KEY_LEFT: u32 = 0x0100_0012;
const KEY_UP: u32 = 0x0100_0013;
const KEY_RIGHT: u32 = 0x0100_0014;
const KEY_DOWN: u32 = 0x0100_0015;

/// What a cached `find_focus` result depends on besides the window (`Keyboard::found`).
type FocusKey = (u64, u64, usize);

#[derive(Default)]
pub struct Keyboard {
    /// Window (item) holding the wl_keyboard focus.
    pub win: Option<usize>,
    active: Option<usize>,
    pending: Option<usize>,
    pub mods: u32,
    /// Last `find_focus` result, valid while the window, focus/visible writes, the item tree
    /// and the set of destroyed items are unchanged.
    found: Option<(usize, FocusKey, Option<usize>)>,
    /// Window the focus chain was set in. QtWayland reports window (de)activation with
    /// Qt::OtherFocusReason, so a change of window always drops the selection.
    chain_win: Option<usize>,
    /// Open popups of the window, refreshed with the `found` key.
    popups: (Option<(usize, FocusKey)>, Vec<usize>),
    /// Font stacks for visual-line moves (the app's shared cache).
    pub fonts: std::rc::Rc<std::cell::RefCell<crate::scene::Fonts>>,
    /// QTextCursor's x kept across Up / Down: the editor, the caret the last vertical move left
    /// and the x it aimed at. Any other key in an editor drops it.
    line_x: Option<(usize, usize, f64)>,
}

pub fn qt_mods(shift: bool, ctrl: bool, alt: bool, logo: bool) -> u32 {
    (shift as u32 * SHIFT) | (ctrl as u32 * CTRL) | (alt as u32 * ALT) | (logo as u32 * META)
}

/// The seat keyboard's one xkb keymap/state and the locale's compose state, kept like
/// QWaylandInputDevice::Keyboard (mXkbKeymap/mXkbState) with QXkbCommon's lookups on top.
pub struct Xkb {
    ctx: xkb::Context,
    state: Option<xkb::State>,
    compose: Option<xkb::compose::State>,
}

impl Default for Xkb {
    fn default() -> Self {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        // The compose table of the C library's LC_CTYPE (libxkbcommon xkbcommon-compose.h:
        // the first non-empty of LC_ALL, LC_CTYPE, LANG, else "C").
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .filter_map(std::env::var_os)
            .find(|v| !v.is_empty())
            .unwrap_or_else(|| "C".into());
        let compose =
            xkb::compose::Table::new_from_locale(&ctx, &locale, xkb::compose::COMPILE_NO_FLAGS)
                .ok()
                .map(|t| xkb::compose::State::new(&t, xkb::compose::STATE_NO_FLAGS));
        Xkb {
            ctx,
            state: None,
            compose,
        }
    }
}

/// wl_keyboard key codes are evdev codes; xkb keycodes are offset by 8 (wayland.xml keymap_format).
fn keycode(key: u32) -> xkb::Keycode {
    xkb::Keycode::new(key + 8)
}

impl Xkb {
    /// wl_keyboard.keymap with format xkb_v1: compiles the shared fd (mapped MAP_PRIVATE, as
    /// wl_keyboard v7 requires) and starts a fresh state on it.
    pub fn keymap_fd(&mut self, fd: std::os::fd::OwnedFd, size: u32) -> bool {
        if size == 0 {
            return false;
        }
        // SAFETY: wayland-client hands over the fd of the keymap event and `size` is the
        // length the compositor sent with it (a NUL-terminated keymap string).
        let km = unsafe {
            xkb::Keymap::new_from_fd(
                &self.ctx,
                fd,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        match km {
            Ok(Some(km)) => {
                self.set_keymap(&km);
                true
            }
            _ => false,
        }
    }

    pub fn set_keymap(&mut self, km: &xkb::Keymap) {
        self.state = Some(xkb::State::new(km));
    }

    /// QWaylandInputDevice::Keyboard::createDefaultKeymap: a key before (or without) an xkb
    /// keymap uses the system default RMLVO keymap.
    fn state(&mut self) -> Option<&mut xkb::State> {
        if self.state.is_none() {
            let km = xkb::Keymap::new_from_names(
                &self.ctx,
                "",
                "",
                "",
                "",
                None,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )?;
            self.set_keymap(&km);
        }
        self.state.as_mut()
    }

    /// wl_keyboard.modifiers; returns the Qt modifiers now in effect.
    pub fn modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) -> u32 {
        let Some(st) = self.state.as_mut() else {
            return 0;
        };
        st.update_mask(depressed, latched, locked, 0, 0, group);
        let on = |m: &str| st.mod_name_is_active(m, xkb::STATE_MODS_EFFECTIVE);
        qt_mods(
            on(xkb::MOD_NAME_SHIFT),
            on(xkb::MOD_NAME_CTRL),
            on(xkb::MOD_NAME_ALT),
            on(xkb::MOD_NAME_LOGO),
        )
    }

    /// The keysym `key` delivers now, with Qt's Latin fallback under Ctrl (`shortcut_keysym`).
    pub fn sym(&mut self, key: u32) -> Option<u32> {
        let st = self.state()?;
        let code = keycode(key);
        let ctrl = st.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        Some(shortcut_keysym(
            st,
            code,
            st.key_get_one_sym(code).raw(),
            ctrl,
        ))
    }

    /// A key press: its keysym and text, the text going through the compose state like Qt's
    /// compose input context (a dead key or an unfinished sequence types nothing).
    pub fn press(&mut self, key: u32) -> Option<(u32, Option<String>)> {
        let sym = self.sym(key)?;
        let st = self.state.as_ref()?;
        let code = keycode(key);
        let utf8 = match self.compose.as_mut() {
            Some(c) => match c.feed(st.key_get_one_sym(code)) {
                xkb::FeedResult::Ignored => None,
                xkb::FeedResult::Accepted => match c.status() {
                    xkb::Status::Composed => c.utf8(),
                    xkb::Status::Nothing => Some(st.key_get_utf8(code)),
                    _ => None,
                },
            },
            None => Some(st.key_get_utf8(code)),
        };
        Some((sym, utf8))
    }

    /// A wl_keyboard v10 `repeated` key: the key's current keysym and text (QtWayland looks
    /// both up again for compositor repeats; compose sequences are not fed repeats).
    pub fn repeat(&mut self, key: u32) -> Option<(u32, Option<String>)> {
        let sym = self.sym(key)?;
        let utf8 = self.state.as_ref()?.key_get_utf8(keycode(key));
        Some((sym, Some(utf8)))
    }

    /// xkb_keymap_key_repeats: whether holding `key` auto-repeats (modifiers don't).
    pub fn repeats(&self, key: u32) -> bool {
        self.state
            .as_ref()
            .is_none_or(|st| st.get_keymap().key_repeats(keycode(key)))
    }
}

/// QXkbCommon::keysymToQtKey: with Ctrl held a non-Latin-1 keysym is replaced by the key's
/// Latin keysym from another layout, so `QKeySequence::Paste` etc. match under e.g. `ru`.
pub fn shortcut_keysym(state: &xkb::State, code: xkb::Keycode, sym: u32, ctrl: bool) -> u32 {
    if !ctrl || sym <= 0xff {
        return sym;
    }
    latin_keysym(state, code).unwrap_or(sym)
}

/// QXkbCommon::lookupLatinKeysym.
fn latin_keysym(state: &xkb::State, code: xkb::Keycode) -> Option<u32> {
    let km = state.get_keymap();
    let cur = state.key_get_layout(code);
    let (sym, layout) = (0..km.num_layouts_for_key(code))
        .filter(|&l| l != cur)
        .find_map(
            |l| match km.key_get_syms_by_level(code, l, state.key_get_level(code, l)) {
                [s] if s.raw() <= 0xff => Some((s.raw(), l)),
                _ => None,
            },
        )?;
    // Not unique when a layout listed earlier produces the same keysym on another key.
    let latched = state.serialize_mods(xkb::STATE_MODS_LATCHED);
    let locked = state.serialize_mods(xkb::STATE_MODS_LOCKED);
    let mut q = xkb::State::new(&km);
    for prev in 0..layout {
        q.update_mask(0, latched, locked, 0, 0, prev);
        if (km.min_keycode().raw()..km.max_keycode().raw())
            .any(|c| q.key_get_one_sym(c.into()).raw() == sym)
        {
            return None;
        }
    }
    Some(sym)
}

/// Qt::Key for an X keysym (QXkbCommon::keysymToQtKey, with Super and Hyper as Meta the way the
/// Wayland platform plugin asks for it).
fn qt_key(sym: u32, utf8: Option<&str>) -> u32 {
    const KEY_F1: u32 = 0x0100_0030;
    const KEY_META: u32 = 0x0100_0022;
    const KEY_UNKNOWN: u32 = 0x01ff_ffff;
    let direct = match sym {
        // F1..F35.
        0xffbe..=0xffe0 => Some(KEY_F1 + (sym - 0xffbe)),
        // KP_0..KP_9.
        0xffb0..=0xffb9 => Some(0x30 + (sym - 0xffb0)),
        // Latin-1: the upper-case letter, unless that leaves Latin-1 (Key_mu, Key_ydiaeresis).
        0x20..=0xff => char::from_u32(sym).map(|c| {
            let mut up = c.to_uppercase();
            match (up.next(), up.next()) {
                (Some(u), None) if (u as u32) <= 0xff => u as u32,
                _ => sym,
            }
        }),
        _ => crate::keysym::lookup(sym).map(|k| match k {
            // Key_Super_L/R, Key_Hyper_L/R.
            0x0100_0053 | 0x0100_0054 | 0x0100_0056 | 0x0100_0057 => KEY_META,
            k => k,
        }),
    };
    if let Some(k) = direct {
        return k;
    }
    utf8.and_then(|t| t.chars().next())
        .filter(|c| !c.is_control())
        .and_then(|c| c.to_uppercase().next())
        .map_or(KEY_UNKNOWN, |c| c as u32)
}

fn parent(w: &World, i: usize) -> Option<usize> {
    w.items.get(i).and_then(|it| it.parent)
}

fn shown(w: &mut World, i: usize, win: usize) -> bool {
    let mut c = Some(i);
    while let Some(x) = c {
        if w.dead.contains(&x) || (x != win && !truthy(w, x, "visible", true)) {
            return false;
        }
        if x == win {
            return true;
        }
        c = parent(w, x);
    }
    false
}

/// Item `i` is alive and in the tree of window `win`, shown or not, and no popup it is in has
/// closed: a hidden item keeps the keyboard, a closed popup gives it back
/// (QQuickPopupPrivate::finalizeExit).
fn in_window(w: &mut World, i: usize, win: usize) -> bool {
    let mut c = Some(i);
    while let Some(x) = c {
        if w.dead.contains(&x) {
            return false;
        }
        if x == win {
            return true;
        }
        if crate::scene::is_popup(w, x) && !truthy(w, x, "visible", false) {
            return false;
        }
        c = parent(w, x);
    }
    false
}

fn inside(w: &World, i: usize, anc: usize) -> bool {
    let mut c = parent(w, i);
    while let Some(x) = c {
        if x == anc {
            return true;
        }
        c = parent(w, x);
    }
    false
}

/// The focus item of a window: the first `focus: true` item, narrowed to its own focused
/// descendants (FocusScope semantics).
fn find_focus(w: &mut World, i: usize) -> Option<usize> {
    focus_in(w, i, None).flatten()
}

/// `find_focus` under `i`, whose focus scope is `scope` (the window when None): None when
/// nothing under it takes focus. A disabled item keeps its `focus` and has no active focus
/// (QQuickItemPrivate::setEffectiveEnableRecur: clearFocusInScope with
/// DontChangeFocusProperty), which stays with its scope until the item is enabled again.
fn focus_in(w: &mut World, i: usize, scope: Option<usize>) -> Option<Option<usize>> {
    for c in kids(w, i) {
        if w.dead.contains(&c) || !truthy(w, c, "visible", true) {
            continue;
        }
        let on = truthy(w, c, "enabled", true);
        if truthy(w, c, "focus", false) {
            if !on {
                return Some(scope);
            }
            let inner = if w.is_focus_scope(c) { Some(c) } else { scope };
            return Some(focus_in(w, c, inner).unwrap_or(Some(c)));
        }
        // An unfocused scope keeps its own focus item inactive.
        if w.is_focus_scope(c) {
            continue;
        }
        if let Some(d) = focus_in(w, c, scope) {
            return Some(if on { d } else { scope });
        }
    }
    None
}

/// QQuickItem::activeFocusOnTab; for Controls `focusPolicy & Qt.TabFocus` (StrongFocus by default
/// on buttons, text fields and other input controls). QQuickSwipeView's constructor sets
/// activeFocusOnTab.
fn tab_focusable(w: &mut World, i: usize) -> bool {
    if explicit(w, i, "focusPolicy") {
        return num(w, i, "focusPolicy") as i32 & 1 != 0;
    }
    if explicit(w, i, "activeFocusOnTab") {
        return truthy(w, i, "activeFocusOnTab", false);
    }
    strong_focus(w, i) || is(w, i, "SwipeView")
}

/// QQuickItemPrivate::setFocusIfNeeded on a press: `focusPolicy & Qt.ClickFocus`.
pub(crate) fn click_focusable(w: &mut World, i: usize) -> bool {
    if explicit(w, i, "focusPolicy") {
        return num(w, i, "focusPolicy") as i32 & 2 == 2;
    }
    strong_focus(w, i)
}

/// Controls whose default focusPolicy is Qt.StrongFocus.
fn strong_focus(w: &World, i: usize) -> bool {
    [
        "TextField",
        "TextArea",
        "Slider",
        "RangeSlider",
        "ComboBox",
        "SpinBox",
        "Dial",
        "Tumbler",
    ]
    .iter()
    .any(|t| is(w, i, t))
        || button_focus(w, i)
}

/// QQuickAbstractButton defaults to Qt.StrongFocus (TabButton, CheckBox, ToolButton... too);
/// QQuickItemDelegate and its subclasses reset it to Qt.NoFocus.
fn button_focus(w: &World, i: usize) -> bool {
    let t = &w.items[i].types;
    qml::types::inherits(t, "QQuickAbstractButton")
        && !qml::types::inherits(t, "QQuickItemDelegate")
}

fn enabled(w: &mut World, i: usize, win: usize) -> bool {
    let mut c = Some(i);
    while let Some(x) = c {
        if !truthy(w, x, "enabled", true) {
            return false;
        }
        if x == win {
            return true;
        }
        c = parent(w, x);
    }
    true
}

/// QQuickItemPrivate::focusNextPrev: the next (or previous) tab-focusable item in the window's
/// item tree order after `from`, wrapping around. Backwards, nextPrevItemInTabFocusChain skips
/// a focus scope that is a tab stop and has active focus: focusing it would give the focus
/// back to the item inside it the key came from.
fn focus_next(w: &mut World, win: usize, from: usize, forward: bool) -> Option<usize> {
    let mut around = vec![];
    let mut c = w.items[from].parent;
    while let Some(p) = c.filter(|_| !forward) {
        if w.is_focus_scope(p) {
            around.push(p);
        }
        c = w.items[p].parent;
    }
    let mut order = vec![];
    let mut stack = vec![win];
    while let Some(i) = stack.pop() {
        if w.dead.contains(&i) || (i != win && !truthy(w, i, "visible", true)) {
            continue;
        }
        order.push(i);
        stack.extend(kids(w, i).into_iter().rev());
    }
    if !forward {
        order.reverse();
    }
    let at = order.iter().position(|&i| i == from).unwrap_or(0);
    let n = order.len();
    (1..n)
        .map(|k| order[(at + k) % n])
        .find(|&i| tab_focusable(w, i) && enabled(w, i, win) && !around.contains(&i))
}

fn accepted(ev: &Value) -> bool {
    match ev {
        Value::Obj(o) => o.borrow().props.get("accepted").is_some_and(|v| v.truthy()),
        _ => false,
    }
}

impl Keyboard {
    pub fn sync(&mut self, rt: &mut Runtime) {
        rt.world.borrow_mut().kbd_win = self.win;
        if let Some(r) = rt.world.borrow_mut().focus_req.take() {
            self.pending = Some(r);
        }
        let next = match self.win {
            None => None,
            Some(win) => {
                let mut w = rt.world.borrow_mut();
                let req = self.pending.filter(|&r| shown(&mut w, r, win));
                if req.is_some() {
                    self.pending = None;
                }
                {
                    // An item (forceActiveFocus() target or not) loses active focus with its
                    // `focus`, e.g. to the next item of its scope that sets `focus: true`.
                    let keep = match req.filter(|&r| truthy(&mut w, r, "focus", false)) {
                        Some(r) => Some(r),
                        // An item that gets hidden keeps the keyboard focus and still gets
                        // key events (QQuickItem::visible): nothing takes active focus off it.
                        None => self.active.filter(|&a| {
                            in_window(&mut w, a, win)
                                && truthy(&mut w, a, "focus", false)
                                && enabled(&mut w, a, win)
                        }),
                    };
                    let key = (w.focus_gen, w.tree_gen, w.dead.len());
                    let found = match self.found {
                        Some((fw, k, r)) if fw == win && k == key => r,
                        _ => {
                            let r = find_focus(&mut w, win);
                            let key = (w.focus_gen, w.tree_gen, w.dead.len());
                            self.found = Some((win, key, r));
                            r
                        }
                    };
                    // A focused scope hands active focus on to the item inside it that takes
                    // `focus` (QQuickDeliveryAgentPrivate::setFocusInScope).
                    let next = match keep {
                        Some(a) if found.is_some_and(|f| f != a && inside(&w, f, a)) => found,
                        Some(a) => Some(a),
                        None => found,
                    };
                    next.and_then(|n| w.enabled_focus(n, win))
                }
            }
        };
        // QQuickItemPrivate::setEffectiveEnableRecur: an item disabled while it has active focus
        // gives it to its focus scope (clearFocusInScope, `focus` unchanged), so keys go to the
        // scope and its parents, not to the item; enabled again it takes active focus back.
        let next = next.map(|a| {
            let win = self.win.unwrap_or(a);
            let mut w = rt.world.borrow_mut();
            let mut a = a;
            while a != win && !enabled(&mut w, a, win) {
                let mut c = parent(&w, a);
                while let Some(x) = c.filter(|&x| x != win && !w.is_focus_scope(x)) {
                    c = parent(&w, x);
                }
                match c {
                    Some(x) => a = x,
                    None => break,
                }
            }
            a
        });
        let before = self.popups.1.clone();
        let now = self.open_popups(rt);
        if next != self.active {
            if qml::log_on!("ri::keys") {
                let ty = next.and_then(|a| rt.world.borrow().items[a].types.first().cloned());
                tracing::debug!(target: "ri::keys", "focus win={:?} {:?} -> {:?} {ty:?}", self.win, self.active, next);
            }
            let keep = self.chain_win == self.win && {
                let w = rt.world.borrow();
                let inner = |ps: &[usize], i: usize| {
                    ps.iter()
                        .rev()
                        .copied()
                        .find(|&p| p == i || crate::input::is_ancestor(&w, p, i))
                };
                next.and_then(|n| inner(&now, n))
                    .is_some_and(|p| !before.contains(&p))
                    || self
                        .active
                        .filter(|o| !w.dead.contains(o))
                        .and_then(|o| inner(&before, o))
                        .is_some_and(|p| !now.contains(&p))
            };
            self.set(rt, next, keep);
        }
    }

    /// The window's open popups, cached while focus/visible writes and the tree are unchanged.
    fn open_popups(&mut self, rt: &Runtime) -> Vec<usize> {
        let Some(win) = self.win else {
            self.popups = (None, Vec::new());
            return Vec::new();
        };
        let mut w = rt.world.borrow_mut();
        let key = (win, (w.focus_gen, w.tree_gen, w.dead.len()));
        if self.popups.0 != Some(key) {
            self.popups = (Some(key), crate::input::open_popups(&mut w, win));
        }
        self.popups.1.clone()
    }

    /// The focused TextInput/TextEdit, if any (it shows a blinking caret).
    pub fn active_editor(&self, rt: &Runtime) -> Option<usize> {
        let a = self.active?;
        let w = rt.world.borrow();
        (self.win.is_some() && !w.dead.contains(&a) && is_text_edit(&w, a)).then_some(a)
    }

    /// QQuickDeliveryAgentPrivate::setFocusInScope/clearFocusInScope. The World queues the
    /// old editor's FocusOut (see `World::set_active_focus`); `keep_selection` is
    /// Qt::ActiveWindowFocusReason / Qt::PopupFocusReason, which keep its selection.
    fn set(&mut self, rt: &mut Runtime, next: Option<usize>, keep_selection: bool) {
        let activated = self.chain_win != self.win;
        self.chain_win = self.win;
        self.active = next;
        {
            let mut w = rt.world.borrow_mut();
            w.kbd_win = self.win;
            // QQuickWindow::focusInEvent / focusOutEvent: Qt::ActiveWindowFocusReason.
            if activated {
                w.focus_reason = Some((None, 3));
            } else if keep_selection && w.focus_reason.is_none() {
                w.focus_reason = Some((None, 4));
            }
            w.focus_keep_selection = keep_selection;
            w.set_active_focus(next);
            w.focus_keep_selection = false;
        }
        rt.settle();
    }

    /// One key repeat as QWaylandInputDevice::Keyboard delivers it: an auto-repeat release
    /// followed by an auto-repeat press, both with the text of the original press.
    pub fn auto_repeat(&mut self, rt: &mut Runtime, sym: u32, utf8: Option<&str>) {
        self.key(rt, sym, utf8, false, true);
        self.key(rt, sym, utf8, true, true);
    }

    pub fn key(
        &mut self,
        rt: &mut Runtime,
        sym: u32,
        utf8: Option<&str>,
        press: bool,
        repeat: bool,
    ) {
        let Some(win) = self.win else { return };
        rt.settle();
        self.sync(rt);
        let mut target = self.active.unwrap_or(win);
        // A focused item view hands keys to its current delegate (the view is its focus scope).
        {
            let w = rt.world.borrow();
            if is_list(&w, target)
                && let Some(&Value::Item(c)) =
                    w.items[target].props.get("currentItem").map(|p| &p.value)
                && !w.dead.contains(&c)
            {
                target = c;
            }
        }
        let key = qt_key(sym, utf8);
        // QQuickPopup::keyPressEvent: Escape closes a CloseOnEscape popup only while it has active
        // focus (its own `focus`, or the focus item inside it); QQuickComboBox closes its popup.
        if press && key == 0x0100_0000 {
            let active = self.active;
            let top = {
                let mut w = rt.world.borrow_mut();
                crate::input::open_popups(&mut w, win)
                    .into_iter()
                    .rev()
                    .find(|&p| {
                        // QQuickMenu sets focus by default; other popups take it from `focus`.
                        let own = if crate::scene::explicit(&w, p, "focus") {
                            w.read(p, "focus").is_some_and(|v| v.truthy())
                        } else {
                            qml::types::inherits(&w.items[p].types, "QQuickMenu")
                        };
                        let focused = own
                            || active
                                .is_some_and(|a| a == p || crate::input::is_ancestor(&w, p, a))
                            || w.items[p]
                                .parent
                                .is_some_and(|c| w.items[c].types.iter().any(|t| t == "ComboBox"));
                        focused && crate::input::close_policy(&mut w, p) & 16 != 0
                    })
            };
            if let Some(p) = top {
                crate::input::close_popup(rt, p);
                return;
            }
        }
        let text: String = utf8.unwrap_or("").to_string();
        let ev = Runtime::js_object(&[
            ("key", Value::Num(key as f64)),
            ("text", Value::str(&text)),
            ("modifiers", Value::Num(self.mods as f64)),
            ("accepted", Value::Bool(false)),
            ("isAutoRepeat", Value::Bool(repeat)),
            ("count", Value::Num(1.0)),
            ("nativeScanCode", Value::Num(0.0)),
        ]);
        let handler = if press {
            "Keys.onPressed"
        } else {
            "Keys.onReleased"
        };
        if qml::log_on!("ri::keys") {
            let w = rt.world.borrow();
            tracing::debug!(
                target: "ri::keys",
                "key sym={sym:#x} qt={key:#x} text={text:?} mods={:#x} press={press} win={win} target={target} {:?}",
                self.mods,
                w.items[target].types.first()
            );
        }
        let tab = press && (key == KEY_TAB || key == KEY_BACKTAB) && self.mods & (CTRL | ALT) == 0;
        let forward = key == KEY_TAB && self.mods & SHIFT == 0;
        let mut c = Some(target);
        let trace = qml::log_on!("ri::keys");
        while let Some(i) = c {
            if trace {
                tracing::debug!(
                    target: "ri::keys",
                    "  chain {i} {:?} handler={}",
                    rt.world.borrow().items[i].types.first(),
                    rt.has_handler(i, handler)
                );
            }
            if rt.world.borrow().dead.contains(&i) {
                break;
            }
            if keys_attached(rt, i, key, press, &ev) {
                return;
            }
            if press && i == target && is_text_edit(&rt.world.borrow(), i) {
                // Not borrowed for the whole edit: what the edit writes is measured as it
                // settles (`World::text_measure`), with these fonts.
                let fonts = &*self.fonts.clone();
                let done = if key == KEY_UP || key == KEY_DOWN {
                    line_step(rt, fonts, i, key, self.mods, &mut self.line_x)
                } else {
                    self.line_x = None;
                    edit(rt, fonts, i, key, &text, self.mods)
                };
                if done {
                    return;
                }
            }
            if combo_key(rt, i, key, press, repeat) {
                return;
            }
            if press && navigate(rt, i, key, repeat) {
                self.sync(rt);
                return;
            }
            if button_key(rt, i, key, press) || range_key(rt, i, key, press) {
                return;
            }
            if press && menu_key(rt, i, key) {
                self.sync(rt);
                return;
            }
            // QQuickItemPrivate::deliverKeyEvent: a tab stop (or the window's content item) the
            // press reached unaccepted moves active focus along the focus chain, so the Keys
            // handlers of its parents never see that Tab.
            if tab
                && (i == win || tab_focusable(&mut rt.world.borrow_mut(), i))
                && self.focus_from(rt, win, i, forward)
            {
                return;
            }
            if i == win {
                return;
            }
            c = parent(&rt.world.borrow(), i);
        }
        // The chain ended outside the window's item tree (a popup's content).
        if tab {
            self.focus_from(rt, win, target, forward);
        }
    }

    /// QQuickItemPrivate::focusNextPrev: active focus to the tab stop after (or before) `from`.
    fn focus_from(&mut self, rt: &mut Runtime, win: usize, from: usize, forward: bool) -> bool {
        let next = focus_next(&mut rt.world.borrow_mut(), win, from, forward);
        let Some(n) = next.filter(|&n| n != from) else {
            return false;
        };
        rt.world
            .borrow_mut()
            .force_active_focus_for(n, if forward { 1 } else { 2 });
        self.sync(rt);
        true
    }
}

/// QQuickAbstractButton::keyPressEvent / keyReleaseEvent: Space (ButtonPressKeys) presses the
/// button; released while pressed it toggles a checkable button, emits `released` and triggers
/// it (`clicked`). Both events are accepted, so they do not climb to the parents.
fn button_key(rt: &mut Runtime, i: usize, key: u32, press: bool) -> bool {
    const SPACE: u32 = 0x20;
    let accepts = {
        let w = rt.world.borrow();
        let t = &w.items[i].types;
        // QQuickMenuItemPrivate::acceptKeyClick adds Return and Enter.
        qml::types::inherits(t, "QQuickAbstractButton")
            && (key == SPACE
                || (matches!(key, KEY_RETURN | KEY_ENTER)
                    && qml::types::inherits(t, "QQuickMenuItem")))
    };
    if !accepts {
        return false;
    }
    let put = |rt: &mut Runtime, name: &str, v: bool| {
        rt.world.borrow_mut().write(i, name, Value::Bool(v));
        rt.settle();
    };
    if press {
        put(rt, "pressed", true);
        put(rt, "down", true);
        rt.emit(i, "pressed", &[], Vec::new());
        rt.settle();
        return true;
    }
    if !truthy(&mut rt.world.borrow_mut(), i, "pressed", false) {
        return false;
    }
    put(rt, "pressed", false);
    put(rt, "down", false);
    // QQuickAbstractButton::nextCheckState: an auto-exclusive checked button stays checked.
    if crate::input::checkable(&mut rt.world.borrow_mut(), i) {
        let c = truthy(&mut rt.world.borrow_mut(), i, "checked", false);
        let radio = ["RadioButton", "TabButton"]
            .iter()
            .any(|t| is(&rt.world.borrow(), i, t));
        if !(radio && c) {
            put(rt, "checked", !c);
            rt.emit(i, "toggled", &[], Vec::new());
        }
    }
    rt.emit(i, "released", &[], Vec::new());
    crate::input::trigger_action(rt, i);
    rt.emit(i, "clicked", &[], Vec::new());
    crate::input::menu_item_triggered(rt, i);
    rt.settle();
    true
}

/// QQuickSlider::keyPressEvent and QQuickAbstractSpinBox::handleKeyPressEvent, with their
/// release halves. A horizontal Slider steps on Left / Right (swapped when mirrored), a vertical
/// one on Down / Up, by `stepSize` or 0.1, and emits `moved` when the value changed; the press
/// is accepted either way. A SpinBox steps on Down / Up while that side is enabled, wrapping
/// with `wrap`, and emits `valueModified` when the value changed.
fn range_key(rt: &mut Runtime, i: usize, key: u32, press: bool) -> bool {
    let (slider, spin) = {
        let w = rt.world.borrow();
        (is(&w, i, "Slider"), is(&w, i, "SpinBox"))
    };
    if !slider && !spin {
        return false;
    }
    let put = |rt: &mut Runtime, name: &str, on: bool| {
        if truthy(&mut rt.world.borrow_mut(), i, name, false) != on {
            rt.world.borrow_mut().write(i, name, Value::Bool(on));
            rt.settle();
        }
    };
    if !press {
        // keyReleaseEvent of either control drops its pressed state and leaves the event as is.
        for name in if slider {
            &["pressed"][..]
        } else {
            &["up.pressed", "down.pressed"][..]
        } {
            put(rt, name, false);
        }
        return false;
    }
    if slider {
        let up = {
            let mut w = rt.world.borrow_mut();
            // Qt.Vertical is 2.
            let vertical = num(&mut w, i, "orientation") == 2.0;
            let mirrored = truthy(&mut w, i, "mirrored", false);
            match (vertical, key) {
                (false, KEY_LEFT) => mirrored,
                (false, KEY_RIGHT) => !mirrored,
                (true, KEY_UP) => true,
                (true, KEY_DOWN) => false,
                _ => return false,
            }
        };
        put(rt, "pressed", true);
        let changed = {
            let mut w = rt.world.borrow_mut();
            let (f, t) = (num(&mut w, i, "from"), num(&mut w, i, "to"));
            let step = match num(&mut w, i, "stepSize") {
                s if fuzzy_null(s) => 0.1,
                s => s,
            };
            let v = num(&mut w, i, "value");
            let nv = (if up { v + step } else { v - step }).clamp(f.min(t), f.max(t));
            let nv = if fuzzy_null(nv) { 0.0 } else { nv };
            // QQuickSlider::setValue from increase() is a C++ write: the `value` binding stays.
            !fuzzy_eq(v, nv) && {
                w.set_internal(i, "value", Value::Num(nv));
                true
            }
        };
        if changed {
            rt.emit(i, "moved", &[], Vec::new());
        }
        rt.settle();
        return true;
    }
    let (side, sign) = match key {
        KEY_UP => ("up.pressed", 1.0),
        KEY_DOWN => ("down.pressed", -1.0),
        _ => return false,
    };
    let next = {
        let mut w = rt.world.borrow_mut();
        let (f, t) = (num(&mut w, i, "from"), num(&mut w, i, "to"));
        let (lo, hi) = (f.min(t), f.max(t));
        let wrap = truthy(&mut w, i, "wrap", false);
        let v = num(&mut w, i, "value");
        // QQuickSpinBoxPrivate::upEnabled / downEnabled: off at the bound the side steps to.
        let bound = if (sign > 0.0) == (f <= t) { t } else { f };
        if !wrap && v == bound {
            return false;
        }
        // effectiveStepSize: the step runs backwards on an inverted range.
        let step = num(&mut w, i, "stepSize") * if f > t { -1.0 } else { 1.0 };
        let nv = v + sign * step;
        match wrap {
            true if nv < lo => hi,
            true if nv > hi => lo,
            _ => nv.clamp(lo, hi),
        }
    };
    // Pressed before the step, as Qt does for code binding stepSize to up/down.pressed.
    put(rt, side, true);
    let changed = {
        let mut w = rt.world.borrow_mut();
        // QQuickSpinBox::increase/decrease are C++ writes: the `value` binding stays.
        num(&mut w, i, "value") != next && {
            w.set_internal(i, "value", Value::Num(next));
            true
        }
    };
    if changed {
        rt.emit(i, "valueModified", &[], Vec::new());
    }
    rt.settle();
    true
}

/// qFuzzyIsNull(double).
fn fuzzy_null(v: f64) -> bool {
    v.abs() <= 1e-12
}

/// qFuzzyCompare(double, double).
fn fuzzy_eq(a: f64, b: f64) -> bool {
    (a - b).abs() * 1e12 <= a.abs().min(b.abs())
}

/// QQuickMenu::keyPressEvent: Down makes the next enabled tab-stop item current, Up the previous
/// one (QQuickMenuPrivate::activateNextItem / activatePreviousItem, neither wraps). Down is
/// accepted with the popup's event; Up with no item above climbs on.
fn menu_key(rt: &mut Runtime, i: usize, key: u32) -> bool {
    if !matches!(key, KEY_UP | KEY_DOWN)
        || !qml::types::inherits(&rt.world.borrow().items[i].types, "QQuickMenu")
    {
        return false;
    }
    let mut w = rt.world.borrow_mut();
    let items = w.menu_items(i);
    let cur = num(&mut w, i, "currentIndex") as i64;
    let mut ok = |k: i64| {
        usize::try_from(k)
            .ok()
            .and_then(|k| items.get(k).copied())
            .is_some_and(|c| tab_focusable(&mut w, c) && truthy(&mut w, c, "enabled", true))
    };
    let n = items.len() as i64;
    let next = if key == KEY_DOWN {
        (cur + 1..n).find(|&k| ok(k))
    } else {
        (0..cur.min(n)).rev().find(|&k| ok(k))
    };
    if let Some(k) = next {
        w.menu_set_current(i, k);
    }
    drop(w);
    rt.settle();
    next.is_some() || key == KEY_DOWN
}

/// QQuickComboBox::keyPressEvent / keyReleaseEvent (non-editable): Space presses and on release
/// toggles the popup; Up/Down/Home/End move the highlighted item while the popup shows, else the
/// current item (emitting `activated`); Return/Enter accept the highlighted item.
fn combo_key(rt: &mut Runtime, i: usize, key: u32, press: bool, repeat: bool) -> bool {
    const SPACE: u32 = 0x20;
    const RETURN: u32 = 0x0100_0004;
    const ENTER: u32 = 0x0100_0005;
    const HOME: u32 = 0x0100_0010;
    const END: u32 = 0x0100_0011;
    const UP: u32 = 0x0100_0013;
    const DOWN: u32 = 0x0100_0015;
    let (popup, shown, cur, hi, count) = {
        let mut w = rt.world.borrow_mut();
        if !is(&w, i, "ComboBox") || truthy(&mut w, i, "editable", false) {
            return false;
        }
        let popup = match w.read(i, "popup") {
            Some(Value::Item(p)) if !w.dead.contains(&p) => Some(p),
            _ => None,
        };
        let shown = popup.is_some_and(|p| truthy(&mut w, p, "visible", false));
        (
            popup,
            shown,
            num(&mut w, i, "currentIndex"),
            num(&mut w, i, "highlightedIndex"),
            num(&mut w, i, "count"),
        )
    };
    let put = |rt: &mut Runtime, name: &str, v: f64| {
        rt.world.borrow_mut().write(i, name, Value::Num(v));
        rt.settle();
    };
    let pressed = |rt: &mut Runtime, on: bool| {
        rt.world.borrow_mut().write(i, "pressed", Value::Bool(on));
        rt.settle();
    };
    let activate = |rt: &mut Runtime, k: f64| {
        rt.emit(i, "activated", &["index"], vec![Value::Num(k)]);
        rt.settle();
    };
    // QQuickComboBoxPrivate::setCurrentItemAtIndex(index, Activate).
    let select = |rt: &mut Runtime, k: f64| {
        if k != cur {
            put(rt, "currentIndex", k);
            activate(rt, k);
        }
    };
    let hide = |rt: &mut Runtime, accept: bool| {
        if accept {
            if hi != cur {
                put(rt, "currentIndex", hi);
            }
            activate(rt, hi);
        }
        if let Some(p) = popup.filter(|_| shown) {
            crate::input::close_popup(rt, p);
        }
    };
    if !press {
        match key {
            SPACE if !repeat => {
                if truthy(&mut rt.world.borrow_mut(), i, "pressed", false) {
                    if shown {
                        hide(rt, true);
                    } else if let Some(p) = popup {
                        crate::input::open_popup(rt, p);
                    }
                }
                pressed(rt, false);
            }
            RETURN | ENTER if !repeat => {
                hide(rt, shown);
                pressed(rt, false);
            }
            _ => return false,
        }
        return true;
    }
    match key {
        SPACE => {
            if !repeat {
                pressed(rt, true);
            }
        }
        RETURN | ENTER => {
            if shown {
                pressed(rt, true);
            }
        }
        UP if shown => {
            if hi > 0.0 {
                put(rt, "highlightedIndex", hi - 1.0);
            }
        }
        UP => {
            if cur > 0.0 {
                select(rt, cur - 1.0);
            }
        }
        DOWN if shown => {
            if hi < count - 1.0 {
                put(rt, "highlightedIndex", hi + 1.0);
            }
        }
        DOWN => {
            if cur < count - 1.0 {
                select(rt, cur + 1.0);
            }
        }
        HOME | END => {
            let k = if key == HOME { 0.0 } else { count - 1.0 };
            if shown {
                if k != hi {
                    put(rt, "highlightedIndex", k);
                }
            } else {
                select(rt, k);
            }
        }
        _ => return false,
    }
    true
}

/// QQuickKeysAttached::keyToSignal: the per-key `Keys.on<Key>Pressed` handler name.
fn key_signal(key: u32) -> Option<String> {
    if (0x30..=0x39).contains(&key) {
        return Some(format!("Keys.onDigit{}Pressed", key - 0x30));
    }
    let name = match key {
        0x0100_0012 => "Left",
        0x0100_0014 => "Right",
        0x0100_0013 => "Up",
        0x0100_0015 => "Down",
        KEY_TAB => "Tab",
        KEY_BACKTAB => "Backtab",
        0x2a => "Asterisk",
        0x23 => "NumberSign",
        0x0100_0000 => "Escape",
        KEY_RETURN => "Return",
        KEY_ENTER => "Enter",
        KEY_DELETE => "Delete",
        0x20 => "Space",
        0x0100_0061 => "Back",
        0x0102_0001 => "Cancel",
        0x0101_0000 => "Select",
        0x0101_0001 => "Yes",
        0x0101_0002 => "No",
        0x0110_0000 => "Context1",
        0x0110_0001 => "Context2",
        0x0110_0002 => "Context3",
        0x0110_0003 => "Context4",
        0x0110_0020 => "Call",
        0x0110_0021 => "Hangup",
        0x0110_0022 => "Flip",
        0x0100_0055 => "Menu",
        0x0100_0072 => "VolumeUp",
        0x0100_0070 => "VolumeDown",
        _ => return None,
    };
    Some(format!("Keys.on{name}Pressed"))
}

/// QQuickKeysAttached::keyPressed/keyReleased: a connected per-key signal runs first with the
/// event accepted by default; `Keys.onPressed` only sees what it left unaccepted.
fn keys_attached(rt: &mut Runtime, i: usize, key: u32, press: bool, ev: &Value) -> bool {
    if !truthy(&mut rt.world.borrow_mut(), i, "Keys.enabled", true) {
        return false;
    }
    if press && let Some(sig) = key_signal(key).filter(|s| rt.has_handler(i, s)) {
        if let Value::Obj(o) = ev {
            qml::eval::set_own(o, "accepted", Value::Bool(true));
        }
        rt.fire(i, &sig, &["event"], vec![ev.clone()]);
        if accepted(ev) {
            return true;
        }
    }
    let handler = if press {
        "Keys.onPressed"
    } else {
        "Keys.onReleased"
    };
    if rt.has_handler(i, handler) {
        rt.fire(i, handler, &["event"], vec![ev.clone()]);
        return accepted(ev);
    }
    false
}

fn is_list(w: &World, i: usize) -> bool {
    w.items
        .get(i)
        .is_some_and(|it| it.types.iter().any(|t| t == "ListView"))
}

/// QQuickListView::keyPressEvent, then KeyNavigation. The view's arrows move currentIndex
/// while key navigation is on (`keyNavigationEnabled`, which follows `interactive` until it is
/// assigned); at an edge `keyNavigationWraps` wraps a first press and swallows an auto-repeat.
/// QQuickKeyNavigationAttached::setUp/setDown/... also set the reverse direction on the other
/// item unless that one was set explicitly.
fn navigate(rt: &mut Runtime, i: usize, key: u32, repeat: bool) -> bool {
    const LEFT: u32 = 0x0100_0012;
    const UP: u32 = 0x0100_0013;
    const RIGHT: u32 = 0x0100_0014;
    const DOWN: u32 = 0x0100_0015;
    let mut w = rt.world.borrow_mut();
    if is_list(&w, i) {
        let horiz = w
            .read(i, "orientation")
            .is_some_and(|v| v.to_number() == 1.0);
        let step = match key {
            DOWN if !horiz => 1.0,
            UP if !horiz => -1.0,
            RIGHT if horiz => 1.0,
            LEFT if horiz => -1.0,
            _ => 0.0,
        };
        let realized = w.delegated.get(&i).map_or(0, |d| d.1.len());
        let n = w
            .view_window
            .get(&i)
            .map_or(realized, |v| v.1.max(realized)) as f64;
        let on = if explicit(&w, i, "keyNavigationEnabled") {
            truthy(&mut w, i, "keyNavigationEnabled", true)
        } else {
            !explicit(&w, i, "interactive") || truthy(&mut w, i, "interactive", true)
        };
        if step != 0.0 && n > 0.0 && on {
            let wraps = truthy(&mut w, i, "keyNavigationWraps", false);
            let key = w.view_cur(i);
            let cur = w.read(i, key).map_or(-1.0, |v| v.to_number());
            let room = if step > 0.0 { cur < n - 1.0 } else { cur > 0.0 };
            if room || (wraps && !repeat) {
                // incrementCurrentIndex / decrementCurrentIndex.
                let next = match cur + step {
                    x if (0.0..n).contains(&x) => x,
                    _ if step > 0.0 => 0.0,
                    _ => n - 1.0,
                };
                let key = w.view_cur(i);
                drop(w);
                rt.set(i, key, Value::Num(next));
                rt.settle();
                return true;
            }
            if wraps {
                return true;
            }
        }
    }
    let (dir, rev) = match key {
        UP => ("up", "down"),
        DOWN => ("down", "up"),
        LEFT => ("left", "right"),
        RIGHT => ("right", "left"),
        0x0100_0001 => ("tab", "backtab"),
        0x0100_0002 => ("backtab", "tab"),
        _ => return false,
    };
    let t = match w.items[i]
        .props
        .get(&format!("KeyNavigation.{dir}"))
        .map(|p| p.value.clone())
    {
        Some(Value::Item(t)) => Some(t),
        Some(_) => None,
        None => {
            let key = format!("KeyNavigation.{rev}");
            w.items.ids().find(|&j| {
                !w.dead.contains(&j)
                    && matches!(w.items[j].props.get(&key).map(|p| &p.value), Some(&Value::Item(x)) if x == i)
            })
        }
    };
    let Some(t) = t.filter(|&t| !w.dead.contains(&t) && truthy(&mut w, t, "visible", true)) else {
        return false;
    };
    w.focus_req = Some(t);
    drop(w);
    rt.set(t, "focus", Value::Bool(true));
    rt.settle();
    true
}

thread_local! {
    /// Text the last left release over a text editor put into the primary selection
    /// (QClipboard::Selection); the Wayland side takes it with the release serial.
    static PRIMARY_OUT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

pub fn take_primary_out() -> Option<String> {
    PRIMARY_OUT.with(|p| p.borrow_mut().take())
}

/// Left release over a text editor (and a TextEdit keyboard selection):
/// QQuickTextInputPrivate::copy(QClipboard::Selection) (only with echoMode Normal) and
/// QQuickTextControlPrivate::setClipboardSelection.
pub fn export_primary(rt: &mut Runtime, i: usize) {
    let mut w = rt.world.borrow_mut();
    if w.items[i].prop_info("echoMode").is_some() && num(&mut w, i, "echoMode") != 0.0 {
        return;
    }
    let t = w
        .read(i, "selectedText")
        .as_ref()
        .and_then(Value::host_str)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    if !t.is_empty() {
        PRIMARY_OUT.with(|p| *p.borrow_mut() = Some(t));
    }
}

/// A TextInput/TextEdit's text (the display text with an inputMask), cursor and ordered
/// selection, as keyPressEvent sees them.
struct EditState {
    cs: Vec<char>,
    p: usize,
    s0: usize,
    s1: usize,
    single: bool,
    ro: bool,
    mask: Option<qml::mask::Mask>,
}

fn edit_state(rt: &Runtime, i: usize) -> EditState {
    let (s, pos, sel, single, ro, mask) = {
        let mut w = rt.world.borrow_mut();
        let mask = w.input_mask(i);
        // With an inputMask the cursor walks QQuickTextInputPrivate::m_text, the display text.
        let shown = if mask.is_some() {
            "displayText"
        } else {
            "text"
        };
        let s = w
            .read(i, shown)
            .as_ref()
            .and_then(Value::host_str)
            .map(|s| s.into_owned())
            .unwrap_or_default();
        let mut num = |n: &str| {
            w.read(i, n)
                .map(|v| v.to_number())
                .filter(|v| v.is_finite())
        };
        let pos = num("cursorPosition");
        let sel = (num("selectionStart"), num("selectionEnd"));
        let single = w.items[i].prop_info("acceptableInput").is_some();
        let ro = truthy(&mut w, i, "readOnly", false);
        (s, pos, sel, single, ro, mask)
    };
    let cs: Vec<char> = s.chars().collect();
    let clamp =
        |v: Option<f64>, d: usize| v.map_or(d, |v| textpos::to_char(&cs, v.max(0.0) as usize));
    let p = clamp(pos, cs.len());
    let (mut s0, mut s1) = (clamp(sel.0, p), clamp(sel.1, p));
    if s0 > s1 {
        std::mem::swap(&mut s0, &mut s1);
    }
    EditState {
        cs,
        p,
        s0,
        s1,
        single,
        ro,
        mask,
    }
}

/// removeSelectedText: a mask blanks the range instead of removing it; false without a
/// selection.
fn remove_selection(
    cs: &mut Vec<char>,
    p: &mut usize,
    (s0, s1): (usize, usize),
    mask: Option<&qml::mask::Mask>,
) -> bool {
    if s0 == s1 {
        return false;
    }
    match mask {
        Some(m) => {
            cs.splice(s0..s1, m.clear(s0, s1 - s0));
        }
        None => {
            cs.drain(s0..s1);
        }
    }
    *p = s0;
    true
}

/// QQuickTextInput/QQuickTextEdit keyPressEvent: insertion, deletion, selection and cursor movement.
fn edit(
    rt: &mut Runtime,
    fonts: &std::cell::RefCell<crate::scene::Fonts>,
    i: usize,
    key: u32,
    text: &str,
    mods: u32,
) -> bool {
    // QPlatformTheme's X11 key scheme (emacs): Ctrl+D is QKeySequence::Delete and Ctrl+E
    // MoveToEndOfLine for TextEdit and TextInput.
    if mods & (CTRL | SHIFT | ALT | META) == CTRL {
        match key {
            KEY_D => return edit(rt, fonts, i, KEY_DELETE, "", 0),
            KEY_E => return edit(rt, fonts, i, KEY_END, "", 0),
            _ => {}
        }
    }
    let EditState {
        mut cs,
        mut p,
        s0,
        s1,
        single,
        ro,
        mask,
    } = edit_state(rt, i);
    let anchor = if s0 == s1 || p == s0 { s1 } else { s0 };
    let ctrl = mods & CTRL != 0;
    let shift = mods & SHIFT != 0;
    let line_start = |cs: &[char], p: usize| {
        cs[..p]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |k| k + 1)
    };
    let line_end = |cs: &[char], p: usize| {
        cs[p..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(cs.len(), |k| p + k)
    };
    // Qt::LogicalMoveStyle: Left is towards the start of text set left to right and towards
    // the end of text set right to left: QQuickTextInputPrivate::processKeyEvent by
    // layoutDirection(), QTextCursor::Left by the direction of the document, whatever the
    // paragraph the caret is in.
    let back = (key == KEY_LEFT) != qsg::text::is_rtl(&cs.iter().collect::<String>());
    let n = cs.len();
    let visual = |fonts: &std::cell::RefCell<crate::scene::Fonts>, p: usize| {
        crate::scene::edit_visual_line(&mut rt.world.borrow_mut(), &mut fonts.borrow_mut(), i, p)
            .filter(|&(a, b)| a <= b && b <= n)
    };
    let mut changed = false;
    let del_sel = |cs: &mut Vec<char>, p: &mut usize, changed: &mut bool| {
        let removed = remove_selection(cs, p, (s0, s1), mask.as_ref());
        *changed |= removed;
        removed
    };
    let mut moved = false;
    let seq = mods & (SHIFT | CTRL | ALT | META);
    // QKeySequence::Copy (Ctrl+C, Ctrl+Insert, F16) / Cut (Ctrl+X, Shift+Delete, F20).
    let copy = (key == KEY_C && seq == CTRL)
        || (key == KEY_INSERT && seq == CTRL)
        || (key == KEY_F16 && seq == 0);
    let cut = (key == KEY_X && seq == CTRL)
        || (key == KEY_DELETE && seq == SHIFT)
        || (key == KEY_F20 && seq == 0);
    if copy || cut {
        // TextInput copies only with echoMode Normal; TextEdit has no echoMode.
        let normal = !single || num(&mut rt.world.borrow_mut(), i, "echoMode") == 0.0;
        let cuts = cut && !ro && normal && (single || s0 != s1);
        if (copy || cuts) && s0 != s1 && normal {
            clipboard_set(cs[s0..s1].iter().collect());
        }
        if !cuts {
            return true;
        }
        // QQuickTextInputPrivate::del(): without a selection cut deletes the next character.
        if !del_sel(&mut cs, &mut p, &mut changed) && p < cs.len() {
            cs.remove(p);
            changed = true;
        }
        return finish(rt, i, &cs, changed, p, (p, p));
    }
    // QKeySequence::Undo (Ctrl+Z) and Redo (Ctrl+Shift+Z, Ctrl+Y): accepted whether or not the
    // history has a step, ignored by a read-only editor.
    let undo = key == KEY_Z && seq == CTRL;
    let redo = (key == KEY_Z && seq == CTRL | SHIFT) || (key == KEY_Y && seq == CTRL);
    if undo || redo {
        if ro {
            return false;
        }
        rt.text_replay(i, undo);
        return true;
    }
    // QKeySequence::Paste (Ctrl+V, Shift+Insert, F18): QQuickTextInputPrivate::paste.
    if (key == KEY_V && seq == CTRL)
        || (key == KEY_INSERT && seq == SHIFT)
        || (key == KEY_F18 && seq == 0)
    {
        if !ro {
            request_paste(rt, i, false);
        }
        return true;
    }
    match key {
        KEY_A if ctrl && !shift => {
            let grew = (s0, s1) != (0, cs.len());
            finish_op(
                rt,
                i,
                &cs,
                false,
                cs.len(),
                (0, cs.len()),
                TextOp::SelectAll,
            );
            // QQuickTextControl::selectAll: selectionChanged(force when the selection grew)
            // -> setClipboardSelection.
            if !single && grew {
                export_primary(rt, i);
            }
            return true;
        }
        // QQuickTextInputPrivate::backspace/del with a mask: blank the slot, keep the length.
        KEY_BACKSPACE if !ro && mask.is_some() => {
            let m = mask.as_ref().unwrap();
            if !del_sel(&mut cs, &mut p, &mut changed) && p > 0 {
                p = m.prev_blank(p - 1);
                if p < cs.len() {
                    cs.splice(p..p + 1, m.clear(p, 1));
                }
                changed = true;
            }
        }
        KEY_DELETE if !ro && mask.is_some() => {
            let m = mask.as_ref().unwrap();
            if !del_sel(&mut cs, &mut p, &mut changed) && p < cs.len() {
                cs.splice(p..p + 1, m.clear(p, 1));
                changed = true;
            }
        }
        // QKeySequence::DeleteEndOfLine (Ctrl+K): up to the end of the line, not its newline.
        KEY_K if mods & (CTRL | SHIFT | ALT | META) == CTRL && !ro => {
            let to = line_end(&cs, p);
            cs.drain(p..to);
            changed = true;
        }
        // QKeySequence::DeleteCompleteLine (Ctrl+U): QQuickTextInput only. Qt runs
        // `selectAll(); copy(); del();`, and copy() takes text only with echoMode Normal.
        KEY_U if mods & (CTRL | SHIFT | ALT | META) == CTRL && single && !ro => {
            if !cs.is_empty() && num(&mut rt.world.borrow_mut(), i, "echoMode") == 0.0 {
                clipboard_set(cs.iter().collect());
            }
            cs.clear();
            p = 0;
            changed = true;
        }
        KEY_BACKSPACE if !ro => {
            if !del_sel(&mut cs, &mut p, &mut changed) && p > 0 {
                let from = if ctrl { prev_word(&cs, p) } else { p - 1 };
                cs.drain(from..p);
                p = from;
                changed = true;
            }
        }
        KEY_DELETE if !ro => {
            if !del_sel(&mut cs, &mut p, &mut changed) && p < cs.len() {
                let to = if ctrl { next_word(&cs, p) } else { p + 1 };
                cs.drain(p..to);
                changed = true;
            }
        }
        // QQuickTextInput::keyPressEvent ignores Left/Right that would move off the end without
        // a selection; Alt/Meta+Left/Right are no editing shortcut.
        KEY_LEFT | KEY_RIGHT
            if mods & (ALT | META) != 0
                || (single && s0 == s1 && p == if back { 0 } else { cs.len() }) =>
        {
            return false;
        }
        KEY_LEFT | KEY_RIGHT => {
            moved = true;
            p = if !shift && !ctrl && s0 != s1 {
                // A TextInput leaves the selection at its end on the key's side, a TextEdit at
                // the end the key moves towards.
                if if single { key == KEY_LEFT } else { back } {
                    s0
                } else {
                    s1
                }
            } else if back {
                if ctrl {
                    prev_word(&cs, p)
                } else {
                    p.saturating_sub(1)
                }
            } else if ctrl {
                next_word(&cs, p)
            } else {
                (p + 1).min(cs.len())
            }
        }
        // QTextCursor StartOfLine / EndOfLine: the visual (wrapped) line of a TextEdit.
        KEY_HOME => {
            moved = true;
            crate::scene::cursor_key(&mut rt.world.borrow_mut(), i);
            p = if ctrl || single {
                0
            } else {
                visual(fonts, p).map_or_else(|| line_start(&cs, p), |l| l.0)
            }
        }
        KEY_END => {
            moved = true;
            crate::scene::cursor_key(&mut rt.world.borrow_mut(), i);
            p = if ctrl || single {
                cs.len()
            } else {
                visual(fonts, p).map_or_else(|| line_end(&cs, p), |l| l.1)
            }
        }
        // QQuickTextEdit (tabChangesFocus false): Tab inserts a tab character.
        KEY_TAB if !single && !ro && mods & (CTRL | ALT | META) == 0 => {
            del_sel(&mut cs, &mut p, &mut changed);
            cs.insert(p, '\t');
            p += 1;
            changed = true;
        }
        KEY_RETURN | KEY_ENTER => {
            if single {
                // QQuickTextInputPrivate::processKeyEvent: only acceptable (or fixed up) input
                // finishes; editingFinished (while focused) comes before accepted.
                let ok = rt.world.borrow_mut().acceptable_or_fixup(i);
                rt.settle();
                if ok {
                    if truthy(&mut rt.world.borrow_mut(), i, "activeFocus", false) {
                        rt.emit(i, "editingFinished", &[], Vec::new());
                    }
                    rt.emit(i, "accepted", &[], Vec::new());
                }
                rt.settle();
                // ...then ignores the event, so it climbs on to the parents' Keys handlers.
                return false;
            }
            if ro {
                return false;
            }
            del_sel(&mut cs, &mut p, &mut changed);
            cs.insert(p, '\n');
            p += 1;
            changed = true;
        }
        _ => {
            // QInputControl::isAcceptableInput: only exactly Ctrl and Ctrl+Shift make a
            // printable character a shortcut; with Alt or Meta held it is still typed.
            if ro
                || seq == CTRL
                || seq == (CTRL | SHIFT)
                || text.is_empty()
                || text.chars().any(|c| c.is_control())
            {
                return false;
            }
            del_sel(&mut cs, &mut p, &mut changed);
            if let Some(m) = &mask {
                masked_insert(m, &mut cs, &mut p, text, &mut changed);
                return finish(rt, i, &cs, changed, p, (p, p));
            }
            // QQuickTextInputPrivate::internalInsert: `s.left(m_maxLength - m_text.size())`.
            let max = if single {
                rt.world.borrow_mut().max_length(i)
            } else {
                usize::MAX
            };
            let room = max.saturating_sub(textpos::to_utf16(&cs, cs.len()));
            for c in textpos::left(text, room).chars() {
                cs.insert(p, c);
                p += 1;
                changed = true;
            }
        }
    }
    let sel = if moved && shift {
        (anchor.min(p), anchor.max(p))
    } else {
        (p, p)
    };
    finish(rt, i, &cs, changed, p, sel);
    // QQuickTextControlPrivate::cursorMoveKeyEvent: a KeepAnchor move force-emits
    // selectionChanged, which sets the clipboard selection (QQuickTextInput does not).
    if !single && moved && shift {
        export_primary(rt, i);
    }
    true
}

/// QQuickTextControl's MoveToNextLine / MoveToPreviousLine and their Select variants (Up, Down,
/// with Shift): QTextCursor::Up / Down to the visual line above or below at the x kept in
/// `kept`. False, so the key climbs on, for a TextInput and when the caret has no such line.
fn line_step(
    rt: &mut Runtime,
    fonts: &std::cell::RefCell<crate::scene::Fonts>,
    i: usize,
    key: u32,
    mods: u32,
    kept: &mut Option<(usize, usize, f64)>,
) -> bool {
    let EditState {
        cs,
        p,
        s0,
        s1,
        single,
        ..
    } = edit_state(rt, i);
    let x = kept.take().filter(|k| (k.0, k.1) == (i, p)).map(|k| k.2);
    if single || mods & (CTRL | ALT | META) != 0 {
        return false;
    }
    let step = crate::scene::edit_line_step(
        &mut rt.world.borrow_mut(),
        &mut fonts.borrow_mut(),
        i,
        p,
        key == KEY_DOWN,
        x,
    );
    let Some((to, x)) = step else {
        return false;
    };
    *kept = Some((i, to, x));
    let shift = mods & SHIFT != 0;
    let anchor = if s0 == s1 || p == s0 { s1 } else { s0 };
    let sel = if shift {
        (anchor.min(to), anchor.max(to))
    } else {
        (to, to)
    };
    finish(rt, i, &cs, false, to, sel);
    // A KeepAnchor move sets the clipboard selection, as in `edit`.
    if shift {
        export_primary(rt, i);
    }
    true
}

/// QQuickTextInput/QQuickTextEdit mouseReleaseEvent with Qt::MiddleButton: `deselect()` then
/// `insert(clipboard()->text(QClipboard::Selection))` at the cursor, once the text arrived.
pub fn paste_selection(rt: &mut Runtime, i: usize) {
    if !truthy(&mut rt.world.borrow_mut(), i, "readOnly", false) {
        request_paste(rt, i, true);
    }
}

/// A paste into `item` waiting for the selection's text: QKeySequence::Paste and a script's
/// `paste()` read the clipboard, a middle click the primary selection. It still goes to `item`
/// only while the item lives and, asked for by a key or a click, the keyboard focus it was
/// asked under holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paste {
    pub item: usize,
    pub primary: bool,
    /// `None` for a script's `paste()`, which Qt runs whatever has the focus.
    focus: Option<(Option<usize>, Option<usize>)>,
}

impl Paste {
    /// Whether a script's `paste()` asked for it, not a key or a click.
    pub fn by_script(&self) -> bool {
        self.focus.is_none()
    }
}

thread_local! {
    static PASTE_REQ: std::cell::Cell<Option<(usize, bool)>> =
        const { std::cell::Cell::new(None) };
}

/// Qt reads `QClipboard::text` synchronously and waits for the owner; here the paste is
/// recorded for the shell, which reads the selection from its loop (`take_paste`) and inserts
/// the text once the owner closed the pipe (`apply_paste`).
fn request_paste(rt: &mut Runtime, i: usize, primary: bool) {
    #[cfg(test)]
    {
        let fake = if primary {
            FAKE_PRIMARY.with(|c| c.borrow().clone())
        } else {
            paste_tests::FAKE_CLIP.with(|c| c.borrow().clone())
        };
        if let Some(t) = fake {
            insert_paste(rt, i, primary, &t);
            return;
        }
    }
    let _ = rt;
    PASTE_REQ.set(Some((i, primary)));
}

/// The paste the last key or click asked for, stamped with the keyboard focus it was asked
/// under.
pub fn take_paste(rt: &Runtime) -> Option<Paste> {
    let (item, primary) = PASTE_REQ.take()?;
    let w = rt.world.borrow();
    Some(Paste {
        item,
        primary,
        focus: Some((w.kbd_win, w.focus_active)),
    })
}

/// `paste()` a script called on a TextInput or TextEdit (QQuickTextInput::paste,
/// QQuickTextEdit::paste), served inside the call as Qt serves it: `World::paste_now`. The
/// clipboard's text in the process is the text of the process's own selection
/// (`qml::ports::clip::own_text`), else `offer`, the text read from the data device's
/// selection offer. True if there was one and it went in; false leaves the call to
/// `take_script_paste`.
pub fn paste_in_call(rt: &mut Runtime, item: usize, offer: Option<String>) -> bool {
    let Some(text) = qml::ports::clip::own_text().or(offer) else {
        return false;
    };
    insert_paste(rt, item, false, &text);
    true
}

/// The oldest `paste()` a script called that `paste_in_call` had no text for: it reads the
/// clipboard like a paste key, without the key's tie to the keyboard focus.
pub fn take_script_paste(rt: &Runtime) -> Option<Paste> {
    let item = rt.world.borrow_mut().text_pastes.pop_front()?;
    Some(Paste {
        item,
        primary: false,
        focus: None,
    })
}

/// Whether the pending paste still goes to its editor: the item lives and the keyboard focus a
/// key or click asked under has not moved.
pub fn paste_wanted(rt: &Runtime, p: &Paste) -> bool {
    let w = rt.world.borrow();
    p.focus.is_none_or(|f| f == (w.kbd_win, w.focus_active)) && !w.dead.contains(&p.item)
}

/// Inserts the selection's `text` for `p` (QQuickTextInputPrivate::paste, or the middle-click
/// insert of the primary selection).
pub fn apply_paste(rt: &mut Runtime, p: &Paste, text: &str) {
    insert_paste(rt, p.item, p.primary, text);
}

fn insert_paste(rt: &mut Runtime, i: usize, primary: bool, clip: &str) {
    if primary {
        insert_primary(rt, i, clip);
        return;
    }
    let EditState {
        mut cs,
        mut p,
        s0,
        s1,
        single,
        ro,
        mask,
    } = edit_state(rt, i);
    // QQuickTextInputPrivate::paste inserts unless the clipboard is empty and nothing is
    // selected; QQuickTextControl::paste only with text on the clipboard, so an empty one
    // leaves a TextEdit's selection.
    if ro || clip.is_empty() && (s0 == s1 || !single) {
        return;
    }
    let mut changed = remove_selection(&mut cs, &mut p, (s0, s1), mask.as_ref());
    if let Some(m) = &mask {
        masked_insert(m, &mut cs, &mut p, clip, &mut changed);
        finish(rt, i, &cs, changed, p, (p, p));
        return;
    }
    // QQuickTextInputPrivate::internalInsert keeps at most m_maxLength chars; TextEdit has none.
    let max = if single {
        rt.world.borrow_mut().max_length(i)
    } else {
        usize::MAX
    };
    let room = max.saturating_sub(textpos::to_utf16(&cs, cs.len()));
    for c in textpos::left(clip, room).chars() {
        cs.insert(p, c);
        p += 1;
        changed = true;
    }
    if !single && s0 < s1 {
        // QTextDocument reports the cursor after the replaced selection as changed, also when
        // the pasted text puts it back where it was.
        let op = TextOp::Cut { at_cursor: true };
        finish_op(rt, i, &cs, true, p, (p, p), op);
        return;
    }
    finish(rt, i, &cs, changed, p, (p, p));
}

/// The middle-click insert of the primary selection's `clip` at the cursor.
fn insert_primary(rt: &mut Runtime, i: usize, clip: &str) {
    let (s, pos, ro) = {
        let mut w = rt.world.borrow_mut();
        let s = w
            .read(i, "text")
            .as_ref()
            .and_then(Value::host_str)
            .map(|s| s.into_owned())
            .unwrap_or_default();
        (
            s,
            num(&mut w, i, "cursorPosition"),
            truthy(&mut w, i, "readOnly", false),
        )
    };
    if ro {
        return;
    }
    let mut cs: Vec<char> = s.chars().collect();
    let mut p = textpos::to_char(&cs, pos.max(0.0) as usize);
    // TextInput.maximumLength defaults to 32767; TextEdit has none.
    let max = {
        let mut w = rt.world.borrow_mut();
        match w.items[i].prop_info("maximumLength") {
            None => usize::MAX,
            Some(_) if !explicit(&w, i, "maximumLength") => 32767,
            Some(_) => w.max_length(i),
        }
    };
    let mut changed = false;
    let room = max.saturating_sub(textpos::to_utf16(&cs, cs.len()));
    for c in textpos::left(clip, room).chars() {
        cs.insert(p, c);
        p += 1;
        changed = true;
    }
    finish(rt, i, &cs, changed, p, (p, p));
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAKE_PRIMARY: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn finish(
    rt: &mut Runtime,
    i: usize,
    cs: &[char],
    changed: bool,
    p: usize,
    sel: (usize, usize),
) -> bool {
    let op = if changed {
        TextOp::Edit
    } else {
        TextOp::Move {
            mark: sel.0 < sel.1,
        }
    };
    finish_op(rt, i, cs, changed, p, sel, op)
}

fn finish_op(
    rt: &mut Runtime,
    i: usize,
    cs: &[char],
    changed: bool,
    p: usize,
    sel: (usize, usize),
    op: TextOp,
) -> bool {
    let mask = rt.world.borrow_mut().input_mask(i);
    let t: String = match &mask {
        Some(m) => m.strip(cs),
        None => cs.iter().collect(),
    };
    // QQuickTextInputPrivate::finishChange: an edit that turns valid input Invalid is undone.
    if changed {
        use qml::validator::{INVALID, state};
        let mut w = rt.world.borrow_mut();
        if w.items[i].prop_info("acceptableInput").is_some() {
            let old = w
                .read(i, "text")
                .map(|v| v.to_js_string())
                .unwrap_or_default();
            if state(&mut w, i, &old) != Some(INVALID) && state(&mut w, i, &t) == Some(INVALID) {
                // The undo clears m_textDirty and leaves m_selDirty of a removed selection:
                // selectionChanged, with the selection back as it was.
                let selected = num(&mut w, i, "selectionStart") < num(&mut w, i, "selectionEnd");
                if selected {
                    w.notify(i, "selectedText");
                    drop(w);
                    rt.settle();
                }
                return true;
            }
        }
    }
    // The edit is QQuickTextInput's own, not a write to its properties: a binding on `text`
    // stays and takes the field's text again when what it reads changes.
    for k in [
        "selectionStart",
        "selectionEnd",
        "selectedText",
        "cursorPosition",
    ] {
        rt.world.borrow_mut().clear_binding(i, k);
    }
    let shown: String = cs.iter().collect();
    // World::text_steps counts QString's UTF-16 units; `cs` is chars.
    let u = |c: usize| textpos::to_utf16(cs, c);
    let same = rt
        .world
        .borrow_mut()
        .read(i, "text")
        .is_some_and(|v| v.to_js_string() == t);
    {
        let mut w = rt.world.borrow_mut();
        w.text_edit = Some((i, u(p), mask.as_ref().map(|_| shown.clone())));
        w.text_edit_dirty = changed.then_some(i);
    }
    let units: Vec<u16> = t.encode_utf16().collect();
    rt.text_change(i, &units, u(p), (u(sel.0), u(sel.1)), op, changed);
    // A masked insert marks m_textDirty even when maskString rejected every character:
    // finishChange emits textEdited and textChanged.
    if changed && same && mask.is_some() {
        rt.fire(i, "onTextEdited", &[], Vec::new());
        rt.world.borrow_mut().notify(i, "text");
        rt.settle();
    }
    {
        let mut w = rt.world.borrow_mut();
        w.text_edit = None;
        w.text_edit_dirty = None;
    }
    if mask.is_some() {
        rt.set(i, "displayText", Value::str(&shown));
    }
    rt.world.borrow_mut().caret_reset = Some(std::time::Instant::now());
    true
}

/// QQuickTextInputPrivate::internalInsert with a mask: maskString over the display text from
/// the cursor, then the cursor skips to the next blank.
fn masked_insert(m: &qml::mask::Mask, cs: &mut Vec<char>, p: &mut usize, s: &str, ch: &mut bool) {
    let s: Vec<char> = s.chars().collect();
    let ms = m.mask_string(*p, &s, false, cs);
    let end = (*p + ms.len()).min(cs.len());
    *ch = true;
    cs.splice(*p..end, ms.iter().copied());
    *p = m.next_blank((*p + ms.len()).min(cs.len()));
}

/// `QGuiApplication::clipboard()->setText(text)`.
fn clipboard_set(text: String) {
    #[cfg(test)]
    copy_tests::CLIP_SET.with(|c| *c.borrow_mut() = Some(text));
    #[cfg(not(test))]
    qml::ports::clip::set_text(text)
}

/// What the keyboard router knows about one shell window.
pub struct KbdWin {
    pub configured: bool,
    /// `WlrLayershell.keyboardFocus` (0 = None); 1 for lock surfaces.
    pub mode: u32,
    pub toplevel: bool,
    /// An xdg_popup in a live focus grab that also holds the focused surface. A popup has no
    /// keyboard interactivity of its own: hyprland_focus_grab_v1 enters some whitelisted surface
    /// (the compositor keeps one that already has focus), and the client hands the keys to the
    /// grab's newest popup, the menu opened last, like an xdg_popup grab would.
    pub grabbed: bool,
}

/// The window keys go to while the compositor focuses `focused`: itself, unless it is a layer
/// surface or popup that never takes keyboard focus. Then the newest configured popup of the
/// focus grab holding it, else the newest configured layer surface that takes keyboard focus.
/// An xdg_toplevel the compositor focused keeps the keys.
pub fn kbd_target(focused: Option<usize>, wins: &[KbdWin]) -> Option<usize> {
    let newest = |p: &dyn Fn(&KbdWin) -> bool| {
        (0..wins.len())
            .rev()
            .find(|&j| wins[j].configured && !wins[j].toplevel && p(&wins[j]))
    };
    match focused {
        Some(k) if wins[k].mode == 0 && !wins[k].toplevel => newest(&|w| w.grabbed)
            .or_else(|| newest(&|w| w.mode != 0))
            .or(Some(k)),
        r => r,
    }
}

#[cfg(test)]
mod kbd_target_tests {
    use super::{KbdWin, kbd_target};

    /// Keys follow the compositor's focus into an xdg_toplevel even while an OnDemand layer
    /// is open; a bar (keyboardFocus None) inside a focus grab still hands them to that layer.
    #[test]
    fn focused_toplevel_keeps_the_keys() {
        let win = |mode, toplevel| KbdWin {
            configured: true,
            mode,
            toplevel,
            grabbed: false,
        };
        let wins = [win(0, false), win(2, false), win(0, true)];
        assert_eq!(kbd_target(Some(2), &wins), Some(2));
        assert_eq!(kbd_target(Some(0), &wins), Some(1));
        assert_eq!(kbd_target(Some(1), &wins), Some(1));
        assert_eq!(kbd_target(None, &wins), None);
        // A toplevel is never where a bar's keys are redirected to.
        let wins = [win(2, false), win(0, false), win(0, true)];
        assert_eq!(kbd_target(Some(1), &wins), Some(0));
        let wins = [win(0, false), win(0, true)];
        assert_eq!(kbd_target(Some(0), &wins), Some(0));
    }

    /// A focus grab over a bar or an overflow popup (keyboardFocus None) and a menu opened from
    /// it: whichever of them the compositor enters, the keys go to the newest popup of the grab;
    /// an ungrabbed popup (a tooltip) never gets them.
    #[test]
    fn grabbed_popup_takes_the_grabs_keys() {
        let win = |mode, grabbed| KbdWin {
            configured: true,
            mode,
            toplevel: false,
            grabbed,
        };
        let wins = [win(0, false), win(0, true)];
        assert_eq!(kbd_target(Some(0), &wins), Some(1));
        assert_eq!(kbd_target(Some(1), &wins), Some(1));
        let wins = [win(0, false), win(0, true), win(0, true)];
        assert_eq!(kbd_target(Some(1), &wins), Some(2));
        let wins = [win(0, false), win(0, false)];
        assert_eq!(kbd_target(Some(0), &wins), Some(0));
        let wins = [win(0, false), win(2, false), win(0, true)];
        assert_eq!(kbd_target(Some(0), &wins), Some(2));
        let wins = [win(0, false), win(0, true), win(2, false)];
        assert_eq!(kbd_target(Some(0), &wins), Some(1));
        let mut menu = win(0, true);
        menu.configured = false;
        let wins = [win(0, false), menu];
        assert_eq!(kbd_target(Some(0), &wins), Some(0));
    }
}

#[cfg(test)]
mod paste_tests {
    #[cfg(feature = "interp")]
    use super::*;

    thread_local! {
        pub(super) static FAKE_CLIP: std::cell::RefCell<Option<String>> =
            const { std::cell::RefCell::new(None) };
    }

    #[cfg(feature = "interp")]
    pub(super) fn field(ty: &str, props: &str) -> (Runtime, Keyboard, usize, usize) {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nimport QtQuick.Controls\nItem {{ property int edits: 0\n \
                 {ty} {{ focus: true; onTextEdited: parent.edits++; {props} }} }}"
            ))
            .unwrap();
        rt.settle();
        let f = rt.world.borrow().items[root].children[0];
        let mut kb = Keyboard {
            win: Some(root),
            ..Default::default()
        };
        kb.sync(&mut rt);
        (rt, kb, root, f)
    }

    #[cfg(feature = "interp")]
    pub(super) fn get(rt: &Runtime, i: usize, n: &str) -> String {
        match rt.world.borrow_mut().read(i, n) {
            Some(Value::Str(s)) => s.to_string(),
            Some(v) => v.to_number().to_string(),
            None => String::new(),
        }
    }

    #[cfg(feature = "interp")]
    pub(super) fn press(rt: &mut Runtime, kb: &mut Keyboard, mods: u32, sym: u32) {
        kb.mods = mods;
        kb.key(rt, sym, None, true, false);
        kb.key(rt, sym, None, false, false);
    }

    /// A user edit leaves the QML binding on `text` in place (QQuickTextInput changes its text
    /// from C++, which removes no binding): the field shows what the bound value becomes later.
    #[cfg(feature = "interp")]
    #[test]
    fn typing_keeps_the_text_binding() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { id: r; property string v: \"ab\"\n \
                 TextInput { focus: true; text: r.v; onTextChanged: if (text !== r.v) r.v = text } }",
            )
            .unwrap();
        rt.settle();
        let f = rt.world.borrow().items[root].children[0];
        let mut kb = Keyboard {
            win: Some(root),
            ..Default::default()
        };
        kb.sync(&mut rt);
        kb.key(&mut rt, 0x7a, Some("z"), true, false);
        kb.key(&mut rt, 0x7a, Some("z"), false, false);
        assert_eq!(get(&rt, f, "text"), "abz");
        assert_eq!(get(&rt, root, "v"), "abz");
        assert_eq!(get(&rt, f, "cursorPosition"), "3");
        rt.set(root, "v", Value::str("new"));
        rt.settle();
        assert_eq!(get(&rt, f, "text"), "new");
        kb.key(&mut rt, 0x79, Some("y"), true, false);
        kb.key(&mut rt, 0x79, Some("y"), false, false);
        assert_eq!(get(&rt, root, "v"), "newy");
    }

    /// Ctrl+V into `f` with no fake clipboard: the paste waits for a selection owner that
    /// writes into the returned pipe, read from a loop like the shell's.
    #[cfg(feature = "interp")]
    fn ctrl_v_reading(
        rt: &mut Runtime,
        kb: &mut Keyboard,
    ) -> (
        calloop::EventLoop<'static, Runtime>,
        crate::selection::PasteRead<Runtime, Paste>,
        std::io::PipeWriter,
    ) {
        press(rt, kb, CTRL, 0x76);
        let p = take_paste(rt).expect("Ctrl+V asks for the clipboard");
        assert!(!p.primary);
        let ev = calloop::EventLoop::try_new().unwrap();
        let read = crate::selection::PasteRead::new(ev.handle());
        let (r, w) = std::io::pipe().unwrap();
        read.start(p, r.into(), |rt: &mut Runtime, p, got| {
            if paste_wanted(rt, &p) {
                apply_paste(rt, &p, &String::from_utf8_lossy(&got.unwrap()));
            }
        })
        .unwrap();
        (ev, read, w)
    }

    /// One loop turn that handles what is ready and does not wait.
    #[cfg(feature = "interp")]
    fn turn(ev: &mut calloop::EventLoop<'static, Runtime>, rt: &mut Runtime) {
        ev.dispatch(Some(std::time::Duration::ZERO), rt).unwrap();
    }

    /// The paste changes nothing while the owner holds its pipe open, then inserts the whole
    /// text once, like Qt's synchronous QClipboard::text, after EOF.
    #[cfg(feature = "interp")]
    #[test]
    fn paste_inserts_the_full_text_once_the_owner_closes() {
        use std::io::Write;
        // The cursor is written after creation: as literals beside `text` Qt 6.12 leaves it at
        // the end (TextField) or at 0 (TextArea), fixture `text_cursor_literals.qml`.
        let (mut rt, mut kb, root, f) = field("TextField", "text: \"<>\"");
        rt.set(f, "cursorPosition", Value::Num(1.0));
        let (mut ev, read, mut w) = ctrl_v_reading(&mut rt, &mut kb);
        w.write_all("ab\u{e9}".as_bytes()).unwrap();
        turn(&mut ev, &mut rt);
        assert!(read.pending().is_some());
        assert_eq!(get(&rt, f, "text"), "<>");
        assert_eq!(get(&rt, root, "edits"), "0");
        w.write_all("cd".as_bytes()).unwrap();
        drop(w);
        while read.pending().is_some() {
            ev.dispatch(None, &mut rt).unwrap();
        }
        assert_eq!(get(&rt, f, "text"), "<ab\u{e9}cd>");
        assert_eq!(get(&rt, f, "cursorPosition"), "6");
        assert_eq!(get(&rt, root, "edits"), "1");
    }

    /// Focus moving away while the owner writes drops the paste: the shell's loop cancels it
    /// (closing the pipe), and a read that ends anyway inserts nothing.
    #[cfg(feature = "interp")]
    #[test]
    fn paste_dropped_when_the_focus_moves() {
        use std::io::Write;
        let (mut rt, mut kb, root, f) = field("TextField", "");
        let (mut ev, read, mut w) = ctrl_v_reading(&mut rt, &mut kb);
        w.write_all(b"ab").unwrap();
        turn(&mut ev, &mut rt);
        rt.set(f, "focus", Value::Bool(false));
        rt.settle();
        kb.sync(&mut rt);
        let p = read.pending().unwrap();
        assert!(!paste_wanted(&rt, &p));
        drop(w);
        while read.pending().is_some() {
            ev.dispatch(None, &mut rt).unwrap();
        }
        assert_eq!(get(&rt, f, "text"), "");
        assert_eq!(get(&rt, root, "edits"), "0");

        let (mut rt, mut kb, root, f) = field("TextField", "");
        let (mut ev, read, mut w) = ctrl_v_reading(&mut rt, &mut kb);
        w.write_all(b"ab").unwrap();
        turn(&mut ev, &mut rt);
        kb.win = None;
        kb.sync(&mut rt);
        if read.pending().is_some_and(|p| !paste_wanted(&rt, &p)) {
            read.cancel();
        }
        assert!(read.pending().is_none());
        let e = w.write_all(b"cd").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
        turn(&mut ev, &mut rt);
        assert_eq!(get(&rt, f, "text"), "");
        assert_eq!(get(&rt, root, "edits"), "0");
    }

    /// A new selection cancels the read of the old one: its pipe closes, nothing is inserted.
    #[cfg(feature = "interp")]
    #[test]
    fn paste_cancelled_by_a_new_selection_inserts_nothing() {
        use std::io::Write;
        let (mut rt, mut kb, root, f) = field("TextInput", "text: \"x\"");
        let (mut ev, read, mut w) = ctrl_v_reading(&mut rt, &mut kb);
        w.write_all(b"ab").unwrap();
        turn(&mut ev, &mut rt);
        read.cancel();
        let e = w.write_all(b"cd").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
        turn(&mut ev, &mut rt);
        assert_eq!(get(&rt, f, "text"), "x");
        assert_eq!(get(&rt, root, "edits"), "0");
    }

    /// tests/text_paste/PasteSteps.qml against what Qt 6.12 printed for it (`paste.qt`):
    /// The fixture of `paste()` called from a script on TextEdit and TextInput (plain, with
    /// maximumLength, an input mask, a validator), for six clipboard texts and nine start
    /// states; `paste_now` is `World::paste_now`, with each step's text as the clipboard's.
    #[cfg(feature = "interp")]
    fn paste_steps(paste_now: Option<qml::runtime::PasteNow>) -> (Runtime, usize) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/text_paste");
        let mut rt = Runtime::new(Some(dir.clone()));
        let root = rt.load_file(&dir.join("PasteSteps.qml")).unwrap();
        rt.world.borrow_mut().paste_now = paste_now;
        rt.settle();
        (rt, root)
    }

    /// Fails unless the fixture's log is what Qt 6.12 printed for it (paste.qt).
    #[cfg(feature = "interp")]
    fn assert_paste_log_is_qts(rt: &mut Runtime, root: usize) {
        assert_eq!(rt.errors(), Vec::<String>::new());
        let ours = rt.get(root, "log").to_js_string();
        let qt = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/text_paste/paste.qt"),
        )
        .unwrap();
        let (ours, qt): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), qt.lines().collect());
        let diff: Vec<String> = (0..ours.len().max(qt.len()))
            .filter(|&k| ours.get(k) != qt.get(k))
            .map(|k| {
                let line = |v: &[&str]| v.get(k).map_or("<none>".to_string(), |l| format!("{l:?}"));
                format!(
                    "line {}:\n  qt   {}\n  ours {}",
                    k + 1,
                    line(&qt),
                    line(&ours)
                )
            })
            .collect();
        assert!(
            diff.is_empty(),
            "{} of {} lines differ from Qt\n{}",
            diff.len(),
            qt.len(),
            diff.join("\n")
        );
    }

    /// The clipboard's text is in the process: `paste()` inserts inside the call, as in Qt.
    /// Every step runs in one script, which reads the editor right after `paste()`.
    #[cfg(feature = "interp")]
    #[test]
    fn script_paste_matches_qt() {
        thread_local! {
            static ROOT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        let (mut rt, root) = paste_steps(Some(std::rc::Rc::new(|rt, item| {
            let clip = rt.get(ROOT.get(), "clip").to_js_string();
            insert_paste(rt, item, false, &clip);
            true
        })));
        ROOT.set(root);
        rt.call_method(root, "all", vec![Value::Null])
            .unwrap()
            .unwrap();
        assert!(
            take_script_paste(&rt).is_none(),
            "no paste was left for later"
        );
        assert_paste_log_is_qts(&mut rt, root);
    }

    /// The clipboard's text is not in the process: each call is served the way the shell's
    /// loop serves it, after the script, and leaves what Qt's call leaves.
    #[cfg(feature = "interp")]
    #[test]
    fn script_paste_after_the_script_matches_qt() {
        let (mut rt, root) = paste_steps(Some(std::rc::Rc::new(|_, _| false)));
        let steps = rt.get(root, "count").to_number() as usize;
        for k in 0..steps {
            let k = Value::Num(k as f64);
            rt.call_method(root, "before", vec![k.clone()])
                .unwrap()
                .unwrap();
            rt.call_method(root, "run", vec![k.clone()])
                .unwrap()
                .unwrap();
            let clip = rt.get(root, "clip").to_js_string();
            while let Some(p) = take_script_paste(&rt) {
                assert!(p.by_script() && !p.primary && paste_wanted(&rt, &p));
                apply_paste(&mut rt, &p, &clip);
            }
            rt.call_method(root, "after", vec![k]).unwrap().unwrap();
        }
        assert_paste_log_is_qts(&mut rt, root);
    }

    /// When `paste()` inserts inside the call and when it leaves the text for later
    /// (docs/qt-differences.md). `log` is what the script read right after each call.
    #[cfg(feature = "interp")]
    #[test]
    fn script_paste_inserts_in_the_call_only_with_the_text_in_the_process() {
        thread_local! {
            static CLIP: std::cell::RefCell<Option<&'static str>> =
                const { std::cell::RefCell::new(None) };
        }
        let load = || {
            let mut rt = Runtime::new(None);
            let root = rt
                .load_str(
                    r#"import QtQuick
Item {
    property string log: ""
    property bool bound: false
    property string viaBinding: bound ? pasted() : ""
    property alias pastedText: a.text
    function pasted() { a.paste(); return "called" }
    function one() { a.paste(); log += "<" + a.text + ">" }
    function two() { a.paste(); b.paste(); log += "<" + a.text + "|" + b.text + ">" }
    function nested() { c.paste(); log += "<" + c.text + "|" + a.text + ">" }
    function fixed() { r.paste(); log += "<" + r.text + ">" }
    TextInput { id: a }
    TextEdit { id: b }
    TextInput { id: c; onTextChanged: a.paste() }
    TextInput { id: r; readOnly: true }
}
"#,
                )
                .unwrap();
            rt.world.borrow_mut().paste_now = Some(std::rc::Rc::new(|rt, item| {
                let Some(clip) = CLIP.with(|c| *c.borrow()) else {
                    return false;
                };
                insert_paste(rt, item, false, clip);
                true
            }));
            rt.settle();
            (rt, root)
        };
        let call = |rt: &mut Runtime, root: usize, f: &str| {
            rt.call_method(root, f, vec![]).unwrap().unwrap();
            rt.get(root, "log").to_js_string()
        };
        let serve = |rt: &mut Runtime, clip: &str| {
            let mut n = 0;
            while let Some(p) = take_script_paste(rt) {
                apply_paste(rt, &p, clip);
                n += 1;
            }
            n
        };

        // The text is in the process: the script reads it back at once, and nothing is left
        // for the loop. So does the handler of a paste that pastes again.
        CLIP.with(|c| *c.borrow_mut() = Some("x"));
        let (mut rt, root) = load();
        assert_eq!(call(&mut rt, root, "one"), "<x>");
        assert_eq!(call(&mut rt, root, "two"), "<x><xx|x>");
        assert_eq!(call(&mut rt, root, "nested"), "<x><xx|x><x|xxx>");
        assert_eq!(serve(&mut rt, "late"), 0);
        // A read-only editor pastes nothing, in the call or after it.
        assert_eq!(call(&mut rt, root, "fixed"), "<x><xx|x><x|xxx><>");
        assert_eq!(serve(&mut rt, "late"), 0);

        // The text is not in the process: the script reads the old text, the loop inserts.
        CLIP.with(|c| *c.borrow_mut() = None);
        let (mut rt, root) = load();
        assert_eq!(call(&mut rt, root, "one"), "<>");
        // A paste that waits keeps the later ones behind it, although their text has come.
        CLIP.with(|c| *c.borrow_mut() = Some("x"));
        assert_eq!(call(&mut rt, root, "two"), "<><|>");
        assert_eq!(serve(&mut rt, "late"), 3);
        assert_eq!(call(&mut rt, root, "one"), "<><|><latelatex>");
        assert_eq!(serve(&mut rt, "late"), 0);

        // Called while a binding is evaluated: its change signals have to wait for the
        // binding, so the text does too.
        let (mut rt, root) = load();
        rt.set(root, "bound", Value::Bool(true));
        assert_eq!(rt.get(root, "viaBinding").to_js_string(), "called");
        assert_eq!(rt.get(root, "pastedText").to_js_string(), "");
        assert_eq!(serve(&mut rt, "late"), 1);
        assert_eq!(call(&mut rt, root, "one"), "<latex>");
        CLIP.with(|c| *c.borrow_mut() = None);
    }

    #[cfg(feature = "interp")]
    #[test]
    fn paste_inserts_clipboard_like_qt() {
        FAKE_CLIP.with(|c| *c.borrow_mut() = Some("молоко 2%".into()));
        // Ctrl+V into an empty TextField: text, cursor at the end, one textEdited.
        let (mut rt, mut kb, root, f) = field("TextField", "");
        press(&mut rt, &mut kb, CTRL, 0x76);
        assert_eq!(get(&rt, f, "text"), "молоко 2%");
        assert_eq!(get(&rt, f, "cursorPosition"), "9");
        assert_eq!(get(&rt, root, "edits"), "1");

        // Shift+Insert replaces the selection, maximumLength truncates the insert.
        let (mut rt, mut kb, _, f) = field("TextInput", "text: \"abXYcd\"; maximumLength: 8");
        rt.set(f, "cursorPosition", Value::Num(4.0));
        rt.set(f, "selectionStart", Value::Num(2.0));
        rt.set(f, "selectionEnd", Value::Num(4.0));
        press(&mut rt, &mut kb, SHIFT, 0xff63);
        assert_eq!(get(&rt, f, "text"), "abмолоcd");
        assert_eq!(get(&rt, f, "cursorPosition"), "6");

        // TextArea pastes too; readOnly ignores it; Ctrl+Shift+V is not QKeySequence::Paste.
        let (mut rt, mut kb, _, f) = field("TextArea", "text: \"x\"");
        rt.set(f, "cursorPosition", Value::Num(1.0));
        press(&mut rt, &mut kb, CTRL, 0x76);
        assert_eq!(get(&rt, f, "text"), "xмолоко 2%");
        let (mut rt, mut kb, root, f) = field("TextField", "readOnly: true");
        press(&mut rt, &mut kb, CTRL, 0x76);
        press(&mut rt, &mut kb, CTRL | SHIFT, 0x56);
        assert_eq!(get(&rt, f, "text"), "");
        assert_eq!(get(&rt, root, "edits"), "0");
        FAKE_CLIP.with(|c| *c.borrow_mut() = None);
    }

    #[cfg(feature = "interp")]
    #[test]
    fn maximum_length_cuts_typing_text_and_paste_like_qt() {
        // QQuickTextInput: maximumLength defaults to 32767.
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let (rt, _, _, f) = field("TextField", "");
        assert_eq!(get(&rt, f, "maximumLength"), "32767");

        // Typing past maximumLength is dropped; a selection is replaced first.
        let (mut rt, mut kb, root, f) = field("TextInput", "maximumLength: 3");
        for c in ['a', 'b', 'c', 'd'] {
            assert!(edit(&mut rt, &fonts, f, 0, &c.to_string(), 0));
        }
        assert_eq!(get(&rt, f, "text"), "abc");
        assert_eq!(get(&rt, f, "cursorPosition"), "3");
        assert_eq!(get(&rt, root, "edits"), "3");

        // Paste inserts only what fits.
        FAKE_CLIP.with(|c| *c.borrow_mut() = Some("12345".into()));
        rt.set(f, "cursorPosition", Value::Num(1.0));
        assert!(edit(&mut rt, &fonts, f, KEY_RIGHT, "", SHIFT));
        press(&mut rt, &mut kb, CTRL, 0x76);
        assert_eq!(get(&rt, f, "text"), "a1c");
        FAKE_CLIP.with(|c| *c.borrow_mut() = None);

        // A negative maximumLength limits nothing (QString::left of a negative count is the
        // whole string; Qt 6.12: `maximumLength: -1`, insert(5, " world") gives "hello world").
        {
            let (mut rt, _, _, n) = field("TextInput", "maximumLength: -1; text: \"hello\"");
            rt.set(n, "cursorPosition", Value::Num(5.0));
            assert!(edit(&mut rt, &fonts, n, 0, "!", 0));
            assert_eq!(get(&rt, n, "text"), "hello!");
        }

        // setText and a shrinking maximumLength cut the text (internalSetText: txt.left(max)).
        rt.set(f, "text", Value::str("wxyz"));
        assert_eq!(get(&rt, f, "text"), "wxy");
        rt.set(f, "maximumLength", Value::Num(2.0));
        assert_eq!(get(&rt, f, "text"), "wx");
        assert_eq!(get(&rt, f, "cursorPosition"), "2");

        // TextArea (TextEdit) has no maximumLength.
        let (mut rt, _, _, f) = field("TextArea", "text: \"abc\"");
        rt.set(f, "cursorPosition", Value::Num(3.0));
        assert!(edit(&mut rt, &fonts, f, 0, "d", 0));
        assert_eq!(get(&rt, f, "text"), "abcd");
    }

    #[cfg(feature = "interp")]
    #[test]
    fn typing_into_a_wrapping_text_area_grows_it_and_pushes_siblings_down() {
        // Settings AI system prompt: ContentPage > ColumnLayouts > MaterialTextArea, laid out
        // by the frame's local pass only (no full layout while typing).
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nimport QtQuick.Layouts\nimport Quickshell\n\
                 FloatingWindow { width: 800; height: 600\n property var ta: ta\n property var below: below\n\
                 Flickable { anchors.fill: parent; contentHeight: col.implicitHeight\n\
                 ColumnLayout { id: col; width: 600; spacing: 30\n\
                 ColumnLayout { Layout.fillWidth: true\n\
                 TextArea { id: ta; Layout.fillWidth: true; wrapMode: TextEdit.Wrap; focus: true\n\
                 background: Rectangle { implicitHeight: 56 }\n text: \"one\\ntwo\" } }\n\
                 ColumnLayout { id: below; Layout.fillWidth: true; Text { text: \"B\" } } } } }",
            )
            .unwrap();
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        rt.settle();
        crate::scene::layout_window(&mut rt, &mut fonts.borrow_mut(), win, [800.0, 600.0]);
        rt.settle();
        let (Value::Item(ta), Value::Item(below)) = (rt.get(win, "ta"), rt.get(win, "below"))
        else {
            panic!("no items")
        };
        rt.set(ta, "cursorPosition", Value::Num(7.0));
        let (h0, y0) = (
            rt.get(ta, "height").to_number(),
            rt.get(below, "y").to_number(),
        );
        rt.world.borrow_mut().resized.clear();
        for c in " the quick brown fox jumps over the lazy dog"
            .repeat(4)
            .chars()
        {
            edit(&mut rt, &fonts, ta, 0, &c.to_string(), 0);
            rt.settle();
            let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
            crate::scene::relayout_targets(
                &mut rt.world.borrow_mut(),
                &mut fonts.borrow_mut(),
                &rs,
                0,
            );
            rt.settle();
        }
        let (h1, y1) = (
            rt.get(ta, "height").to_number(),
            rt.get(below, "y").to_number(),
        );
        assert!(h1 > h0, "TextArea height {h0} -> {h1}");
        assert_eq!(y1 - y0, h1 - h0);
    }

    #[cfg(feature = "interp")]
    fn typed(rt: &mut Runtime, f: usize, text: &str) {
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        for c in text.chars() {
            edit(rt, &fonts, f, 0, &c.to_string(), 0);
        }
    }

    #[cfg(feature = "interp")]
    #[test]
    fn validators_reject_invalid_edits_and_gate_enter_like_qt() {
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        // IntValidator: Invalid keystrokes are dropped, Intermediate ones kept.
        let (mut rt, _, root, f) = field(
            "TextField",
            "property string log; validator: IntValidator { bottom: 0; top: 100 } \
             onEditingFinished: log += \"f\"; onAccepted: log += \"a\"",
        );
        assert_eq!(get(&rt, f, "acceptableInput"), "0");
        typed(&mut rt, f, "4a2");
        assert_eq!(get(&rt, f, "text"), "42");
        assert_eq!(get(&rt, f, "acceptableInput"), "1");
        assert_eq!(get(&rt, root, "edits"), "2");
        assert!(!edit(&mut rt, &fonts, f, KEY_RETURN, "", 0));
        assert_eq!(get(&rt, f, "log"), "fa");
        typed(&mut rt, f, "00");
        assert_eq!(get(&rt, f, "text"), "420");
        assert_eq!(get(&rt, f, "acceptableInput"), "0");
        edit(&mut rt, &fonts, f, KEY_RETURN, "", 0);
        assert_eq!(get(&rt, f, "log"), "fa");

        // Paste goes through the same check: "12a" is Invalid as a whole.
        let (mut rt, mut kb, _, f) = field(
            "TextInput",
            "validator: IntValidator { bottom: 0; top: 100 }",
        );
        FAKE_CLIP.with(|c| *c.borrow_mut() = Some("12a".into()));
        press(&mut rt, &mut kb, CTRL, 0x76);
        assert_eq!(get(&rt, f, "text"), "");
        FAKE_CLIP.with(|c| *c.borrow_mut() = Some("77".into()));
        press(&mut rt, &mut kb, CTRL, 0x76);
        assert_eq!(get(&rt, f, "text"), "77");
        FAKE_CLIP.with(|c| *c.borrow_mut() = None);

        // DoubleValidator decimals and RegularExpressionValidator prefixes.
        let (mut rt, _, _, f) = field(
            "TextInput",
            "validator: DoubleValidator { bottom: 0; top: 10; decimals: 2; \
             notation: DoubleValidator.StandardNotation }",
        );
        typed(&mut rt, f, "3.141");
        assert_eq!(get(&rt, f, "text"), "3.14");
        let (mut rt, _, _, f) = field(
            "TextInput",
            "validator: RegularExpressionValidator { regularExpression: /[a-z]{2}\\d{2}/ }",
        );
        typed(&mut rt, f, "a1b23");
        assert_eq!(get(&rt, f, "text"), "ab23");
        assert_eq!(get(&rt, f, "acceptableInput"), "1");
    }

    #[cfg(feature = "interp")]
    #[test]
    fn spinbox_validator_is_an_int_validator_over_from_to() {
        // Basic style SpinBox: `validator: IntValidator { bottom/top: from..to }` (StyledSpinBox).
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nItem {\n \
                 SpinBox { id: sb; to: 50; editable: true }\n \
                 TextInput { validator: sb.validator }\n}",
            )
            .unwrap();
        rt.settle();
        let f = rt.world.borrow().items[root].children[1];
        typed(&mut rt, f, "600");
        assert_eq!(get(&rt, f, "text"), "60");
        assert_eq!(get(&rt, f, "acceptableInput"), "0");
        rt.set(f, "text", Value::str("25"));
        assert_eq!(get(&rt, f, "acceptableInput"), "1");
    }

    #[cfg(feature = "interp")]
    #[test]
    fn retyping_the_selected_text_still_emits_text_changed() {
        // The original's launcher: Esc selects ";", reopening and typing ";" again must re-run
        // `onTextChanged: LauncherSearch.query = text` (QQuickTextInput m_textDirty).
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let (mut rt, _, root, f) = field(
            "TextField",
            "property int tc; property string seen; text: \";\"; \
             onTextChanged: { tc++; seen = text }",
        );
        assert!(edit(&mut rt, &fonts, f, KEY_A, "", CTRL));
        assert_eq!(get(&rt, f, "selectedText"), ";");
        typed(&mut rt, f, ";");
        assert_eq!(get(&rt, f, "text"), ";");
        assert_eq!(get(&rt, f, "tc"), "1");
        assert_eq!(get(&rt, f, "seen"), ";");
        assert_eq!(get(&rt, root, "edits"), "1");
        // Nothing replaced, nothing dirty: no signal.
        rt.set(f, "cursorPosition", Value::Num(0.0));
        assert!(edit(&mut rt, &fonts, f, KEY_BACKSPACE, "", 0));
        assert_eq!(get(&rt, f, "tc"), "1");
    }

    #[cfg(feature = "interp")]
    #[test]
    fn input_mask_formats_filters_and_pastes_like_qt() {
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let (mut rt, mut kb, _, f) = field(
            "TextField",
            "property string log; property string seen; property int tc; \
             inputMask: \"99-99;_\"; onAccepted: log += \"a\"; \
             onTextChanged: { tc++; seen = displayText + \"@\" + cursorPosition }",
        );
        assert_eq!(get(&rt, f, "displayText"), "__-__");
        assert_eq!(get(&rt, f, "text"), "-");
        assert_eq!(get(&rt, f, "maximumLength"), "5");
        assert_eq!(get(&rt, f, "acceptableInput"), "0");
        rt.set(f, "cursorPosition", Value::Num(0.0));
        typed(&mut rt, f, "1x23");
        assert_eq!(get(&rt, f, "displayText"), "12-3_");
        assert_eq!(get(&rt, f, "text"), "12-3");
        assert_eq!(get(&rt, f, "cursorPosition"), "4");
        // textChanged per key (the rejected "x" too) sees the new cursor and the old display.
        assert_eq!(get(&rt, f, "tc"), "4");
        assert_eq!(get(&rt, f, "seen"), "12-__@4");
        edit(&mut rt, &fonts, f, KEY_RETURN, "", 0);
        assert_eq!(get(&rt, f, "log"), "");
        assert!(edit(&mut rt, &fonts, f, KEY_BACKSPACE, "", 0));
        assert_eq!(get(&rt, f, "displayText"), "12-__");
        assert_eq!(get(&rt, f, "cursorPosition"), "3");
        // Paste runs through maskString too; the rest of the clipboard is dropped.
        FAKE_CLIP.with(|c| *c.borrow_mut() = Some("4a56789".into()));
        press(&mut rt, &mut kb, CTRL, 0x76);
        FAKE_CLIP.with(|c| *c.borrow_mut() = None);
        assert_eq!(get(&rt, f, "displayText"), "12-45");
        assert_eq!(get(&rt, f, "acceptableInput"), "1");
        edit(&mut rt, &fonts, f, KEY_RETURN, "", 0);
        assert_eq!(get(&rt, f, "log"), "a");
        rt.set(f, "text", Value::str("9876"));
        assert_eq!(get(&rt, f, "displayText"), "98-76");
        assert_eq!(get(&rt, f, "cursorPosition"), "5");
    }

    #[cfg(feature = "interp")]
    #[test]
    fn focus_out_finishes_editing_like_qt() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nItem { property string log: ''\n \
                 function to(n) { [a, b, c][n].forceActiveFocus() }\n \
                 TextField { id: a; focus: true; text: 'x'\n \
                  onEditingFinished: log += 'a.fin(' + activeFocus + ',' + b.activeFocus + ') '\n \
                  onActiveFocusChanged: log += 'a.af=' + activeFocus + ' '\n \
                  onAccepted: log += 'a.acc ' }\n \
                 TextArea { id: b; readOnly: true\n \
                  onEditingFinished: log += 'b.fin '\n \
                  onActiveFocusChanged: log += 'b.af=' + activeFocus + ' ' }\n \
                 TextInput { id: c; validator: IntValidator { bottom: 10; top: 99 }\n \
                  onEditingFinished: log += 'c.fin ' } }",
            )
            .unwrap();
        rt.settle();
        let c = rt.world.borrow().items[root].children[2];
        let focus = |rt: &mut Runtime, n: u8| {
            rt.call_method(root, "to", vec![Value::Num(n as f64)])
                .unwrap()
                .unwrap();
        };
        let mut kb = Keyboard {
            win: Some(root),
            ..Default::default()
        };
        kb.sync(&mut rt);
        let take = |rt: &mut Runtime| {
            let l = get(rt, root, "log");
            rt.set(root, "log", Value::str(""));
            l
        };
        assert_eq!(take(&mut rt), "a.af=true ");

        // Focus moves to another item: the old one finishes with activeFocus already false
        // (the new one already true), before any activeFocusChanged.
        focus(&mut rt, 1);
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "a.fin(false,true) a.af=false b.af=true ");

        // Window deactivation (wl_keyboard leave) is a focus out too; readOnly TextEdit finishes.
        kb.win = None;
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "b.fin b.af=false ");
        kb.win = Some(root);
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "b.af=true ");

        // TextInput with Intermediate input ("5" < bottom) does not finish; acceptable does.
        focus(&mut rt, 2);
        kb.sync(&mut rt);
        typed(&mut rt, c, "5");
        focus(&mut rt, 0);
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "b.fin b.af=false a.af=true ");
        focus(&mut rt, 2);
        kb.sync(&mut rt);
        typed(&mut rt, c, "5");
        focus(&mut rt, 0);
        kb.sync(&mut rt);
        assert_eq!(
            take(&mut rt),
            "a.fin(false,false) a.af=false c.fin a.af=true "
        );

        // Enter finishes (then accepts) while focused; the later focus out finishes again.
        press(&mut rt, &mut kb, 0, 0xff0d);
        assert_eq!(take(&mut rt), "a.fin(true,false) a.acc ");
        kb.win = None;
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "a.fin(false,false) a.af=false ");
    }

    #[cfg(feature = "interp")]
    #[test]
    fn focus_out_drops_selection_like_qt() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nItem { property string log: ''\n \
                 function to(n) { [a, b, c, d, e][n].forceActiveFocus() }\n \
                 function sel(n) { [a, b, c, d, e][n].select(1, 4) }\n \
                 TextField { id: a; focus: true; text: 'hello'\n \
                  onSelectedTextChanged: log += 'a.sel=' + selectedText + '(' + activeFocus + ') '\n \
                  onEditingFinished: log += 'a.fin '\n \
                  onActiveFocusChanged: log += 'a.af=' + activeFocus + ' ' }\n \
                 TextArea { id: b; text: 'hello'\n \
                  onSelectedTextChanged: log += 'b.sel=' + selectedText + ' ' }\n \
                 TextInput { id: c; text: 'hello'; persistentSelection: true\n \
                  onSelectedTextChanged: log += 'c.sel=' + selectedText + ' ' }\n \
                 TextEdit { id: d; text: 'hello'; readOnly: true\n \
                  onSelectedTextChanged: log += 'd.sel=' + selectedText + ' ' }\n \
                 Button { id: e } }",
            )
            .unwrap();
        rt.settle();
        let [a, b, c, d] = [0, 1, 2, 3].map(|k| rt.world.borrow().items[root].children[k]);
        let call = |rt: &mut Runtime, f: &str, n: u8| {
            rt.call_method(root, f, vec![Value::Num(n as f64)])
                .unwrap()
                .unwrap();
        };
        let mut kb = Keyboard {
            win: Some(root),
            ..Default::default()
        };
        kb.sync(&mut rt);
        let take = |rt: &mut Runtime| {
            let l = get(rt, root, "log");
            rt.set(root, "log", Value::str(""));
            l
        };
        call(&mut rt, "sel", 0);
        assert_eq!(take(&mut rt), "a.af=true a.sel=ell(true) ");

        // Window deactivation drops it too: QtWayland sends Qt::OtherFocusReason, not
        // Qt::ActiveWindowFocusReason.
        kb.win = None;
        kb.sync(&mut rt);
        kb.win = Some(root);
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "a.sel=(false) a.fin a.af=false a.af=true ");
        assert_eq!(get(&rt, a, "selectedText"), "");
        call(&mut rt, "sel", 0);
        take(&mut rt);

        // Focus moving inside the window drops it before editingFinished; the cursor stays.
        call(&mut rt, "to", 4);
        kb.sync(&mut rt);
        assert_eq!(take(&mut rt), "a.sel=(false) a.fin a.af=false ");
        for (n, v) in [
            ("selectionStart", "4"),
            ("selectionEnd", "4"),
            ("cursorPosition", "4"),
        ] {
            assert_eq!(get(&rt, a, n), v, "{n}");
        }

        // TextArea and a readOnly TextEdit drop it too, persistentSelection keeps it.
        for (k, it) in [(1, b), (2, c), (3, d)] {
            call(&mut rt, "to", k);
            call(&mut rt, "sel", k);
            kb.sync(&mut rt);
            call(&mut rt, "to", 4);
            kb.sync(&mut rt);
            let kept = if k == 2 { "ell" } else { "" };
            assert_eq!(get(&rt, it, "selectedText"), kept, "{k}");
            assert_eq!(get(&rt, it, "cursorPosition"), "4", "{k}");
        }
        assert_eq!(
            take(&mut rt),
            "b.sel=ell b.sel= c.sel=ell d.sel=ell d.sel= "
        );
    }

    #[cfg(feature = "interp")]
    #[test]
    fn popup_focus_keeps_selection_like_qt() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nItem { width: 400; height: 300\n \
                 TextField { id: a; focus: true; text: 'hello' }\n \
                 Popup { id: p; focus: true; TextField { id: f; focus: true; text: 'world' } }\n \
                 function sel(x) { x.select(1, 4) }\n \
                 function show(on) { if (on) p.open(); else p.close() } }",
            )
            .unwrap();
        rt.settle();
        let a = rt.world.borrow().items[root].children[0];
        let f = rt.id("f").unwrap();
        let mut kb = Keyboard {
            win: Some(root),
            ..Default::default()
        };
        kb.sync(&mut rt);
        rt.call_method(root, "sel", vec![Value::Item(a)])
            .unwrap()
            .unwrap();
        // Opening a focused popup moves focus with Qt::PopupFocusReason, and so does closing it.
        rt.call_method(root, "show", vec![Value::Bool(true)])
            .unwrap()
            .unwrap();
        kb.sync(&mut rt);
        assert!(rt.get(f, "activeFocus").truthy());
        assert_eq!(get(&rt, a, "selectedText"), "ell");
        rt.call_method(root, "sel", vec![Value::Item(f)])
            .unwrap()
            .unwrap();
        rt.call_method(root, "show", vec![Value::Bool(false)])
            .unwrap()
            .unwrap();
        kb.sync(&mut rt);
        assert!(!rt.get(f, "activeFocus").truthy());
        assert_eq!(get(&rt, f, "selectedText"), "orl");
        assert_eq!(get(&rt, a, "selectedText"), "ell");
    }
}

#[cfg(test)]
mod emacs_tests {
    use super::*;

    fn field(ty: &str, text: &str, pos: usize) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport QtQuick.Controls\n{ty} {{ text: \"{text}\" }}"
            ))
            .unwrap();
        rt.settle();
        // Not a `cursorPosition` in the document: Qt writes that one before `text`.
        rt.set(r, "cursorPosition", Value::Num(pos as f64));
        rt.settle();
        (rt, r)
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn x11_emacs_keys_edit_text() {
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let (mut rt, a) = field("TextArea", "ab\\ncd", 0);
        assert!(edit(&mut rt, &fonts, a, KEY_D, "\u{4}", CTRL));
        assert_eq!(rt.get(a, "text").to_js_string(), "b\ncd");
        assert!(edit(&mut rt, &fonts, a, KEY_K, "\u{b}", CTRL));
        assert_eq!(rt.get(a, "text").to_js_string(), "\ncd");
        assert!(edit(&mut rt, &fonts, a, KEY_E, "\u{5}", CTRL));
        assert_eq!(rt.get(a, "cursorPosition").to_number(), 0.0);
        // TextEdit has no DeleteCompleteLine: Ctrl+U goes on to the parents' Keys handlers.
        assert!(!edit(&mut rt, &fonts, a, KEY_U, "\u{15}", CTRL));

        let (mut rt, f) = field("TextField", "hello", 1);
        assert!(edit(&mut rt, &fonts, f, KEY_E, "\u{5}", CTRL));
        assert_eq!(rt.get(f, "cursorPosition").to_number(), 5.0);
        assert!(edit(&mut rt, &fonts, f, KEY_U, "\u{15}", CTRL));
        assert_eq!(rt.get(f, "text").to_js_string(), "");
    }

    /// Left and Right in text set right to left, the numbers Qt 6.12 gave (`keyClick` of
    /// QtTest): the cursor, then the selection, after the key from a cursor or a selection.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn arrows_follow_the_direction_of_the_text() {
        const DOC: &str = "שלום עולם\\nhello world\\nabc שלום";
        const RTL: &str = "שלום abc";
        // (type, text, cursor or selection before, key, modifiers, cursor and selection after)
        type Case = (
            &'static str,
            &'static str,
            (usize, usize),
            u32,
            u32,
            [f64; 3],
        );
        let cases: [Case; 18] = [
            // A TextEdit moves by the direction of the document in every paragraph.
            ("TextEdit", DOC, (3, 3), KEY_LEFT, 0, [4.0, 4.0, 4.0]),
            ("TextEdit", DOC, (3, 3), KEY_RIGHT, 0, [2.0, 2.0, 2.0]),
            ("TextEdit", DOC, (13, 13), KEY_LEFT, 0, [14.0, 14.0, 14.0]),
            ("TextEdit", DOC, (13, 13), KEY_RIGHT, 0, [12.0, 12.0, 12.0]),
            ("TextEdit", DOC, (28, 28), KEY_LEFT, 0, [29.0, 29.0, 29.0]),
            ("TextEdit", DOC, (3, 3), KEY_LEFT, CTRL, [5.0, 5.0, 5.0]),
            ("TextEdit", DOC, (3, 3), KEY_LEFT, SHIFT, [4.0, 3.0, 4.0]),
            ("TextEdit", DOC, (2, 5), KEY_LEFT, 0, [5.0, 5.0, 5.0]),
            ("TextEdit", DOC, (2, 5), KEY_RIGHT, 0, [2.0, 2.0, 2.0]),
            ("TextEdit", DOC, (0, 0), KEY_RIGHT, 0, [0.0, 0.0, 0.0]),
            ("TextEdit", DOC, (0, 0), KEY_LEFT, 0, [1.0, 1.0, 1.0]),
            // A TextInput by its first letter that has a direction.
            (
                "TextInput",
                "abc שלום עולם",
                (8, 8),
                KEY_LEFT,
                0,
                [7.0, 7.0, 7.0],
            ),
            ("TextInput", RTL, (6, 6), KEY_LEFT, 0, [7.0, 7.0, 7.0]),
            ("TextInput", RTL, (2, 2), KEY_LEFT, CTRL, [5.0, 5.0, 5.0]),
            ("TextInput", RTL, (2, 2), KEY_RIGHT, CTRL, [0.0, 0.0, 0.0]),
            ("TextInput", RTL, (2, 5), KEY_LEFT, 0, [2.0, 2.0, 2.0]),
            ("TextInput", RTL, (2, 5), KEY_RIGHT, 0, [5.0, 5.0, 5.0]),
            ("TextInput", RTL, (3, 3), KEY_LEFT, SHIFT, [4.0, 3.0, 4.0]),
        ];
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        for (n, (ty, text, (from, to), key, mods, want)) in cases.into_iter().enumerate() {
            let (mut rt, e) = field(ty, text, to);
            {
                let mut w = rt.world.borrow_mut();
                w.write(e, "selectionStart", Value::Num(from as f64));
                w.write(e, "selectionEnd", Value::Num(to as f64));
            }
            rt.settle();
            edit(&mut rt, &fonts, e, key, "", mods);
            let got = ["cursorPosition", "selectionStart", "selectionEnd"]
                .map(|k| rt.get(e, k).to_number());
            assert_eq!(
                got, want,
                "case {n}: {ty} {from}..{to}, key {key:#x}, mods {mods}"
            );
        }
    }
}

#[cfg(all(test, feature = "interp"))]
mod focus_tests {
    use super::*;

    #[test]
    fn forced_focus_yields_to_scope_sibling_taking_focus() {
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: win
    width: 400; height: 300
    property bool show: false
    property Item fab: null
    property Item input: null
    function focusFab() { fab.forceActiveFocus() }
    function open() { fab.forceActiveFocus(); win.show = true }
    FocusScope {
        anchors.fill: parent
        Button { id: fab; Component.onCompleted: win.fab = fab }
        TextField { id: input; visible: win.show; focus: win.show; Component.onCompleted: win.input = input }
    }
}"#,
            )
            .unwrap();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let item = |rt: &mut Runtime, n: &str| match rt.get(win, n) {
            Value::Item(i) => i,
            v => panic!("{n}: {v:?}"),
        };
        let (fab, input) = (item(&mut rt, "fab"), item(&mut rt, "input"));
        let mut kb = Keyboard {
            win: Some(win),
            ..Default::default()
        };
        rt.call_method(win, "focusFab", vec![]).unwrap().unwrap();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(fab));
        rt.call_method(win, "open", vec![]).unwrap().unwrap();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(input));
        assert!(rt.get(input, "activeFocus").truthy());
        assert!(!rt.get(fab, "activeFocus").truthy());
    }

    /// The original's pomodoro: Reset is disabled by its own click, and Space then starts the timer from
    /// the widget's Keys handler instead of pressing the disabled button.
    #[test]
    fn disabled_button_gives_active_focus_to_its_scope() {
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: win
    width: 400; height: 300
    property bool on: true
    property int keys: 0
    property int clicks: 0
    property Item reset: null
    property Item scope: null
    function focusReset() { reset.forceActiveFocus() }
    Item {
        anchors.fill: parent
        Keys.onPressed: event => { win.keys += 1; event.accepted = true }
        FocusScope {
            id: scope
            anchors.fill: parent
            Component.onCompleted: win.scope = scope
            Button { id: reset; enabled: win.on; onClicked: win.clicks += 1; Component.onCompleted: win.reset = reset }
        }
    }
}"#,
            )
            .unwrap();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let item = |rt: &mut Runtime, n: &str| match rt.get(win, n) {
            Value::Item(i) => i,
            v => panic!("{n}: {v:?}"),
        };
        let (reset, scope) = (item(&mut rt, "reset"), item(&mut rt, "scope"));
        let mut kb = Keyboard {
            win: Some(win),
            ..Default::default()
        };
        let counts = |rt: &mut Runtime| {
            let n = |rt: &mut Runtime, k: &str| rt.get(win, k).to_number();
            (n(rt, "clicks"), n(rt, "keys"))
        };
        let space = |rt: &mut Runtime, kb: &mut Keyboard| {
            kb.key(rt, 0x20, Some(" "), true, false);
            kb.key(rt, 0x20, Some(" "), false, false);
            kb.sync(rt);
        };
        rt.call_method(win, "focusReset", vec![]).unwrap().unwrap();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(reset));
        space(&mut rt, &mut kb);
        assert_eq!(counts(&mut rt), (1.0, 0.0));

        rt.set(win, "on", Value::Bool(false));
        rt.settle();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(scope));
        assert!(!rt.get(reset, "activeFocus").truthy());
        assert!(rt.get(reset, "focus").truthy());
        space(&mut rt, &mut kb);
        assert_eq!(counts(&mut rt), (1.0, 1.0));

        rt.set(win, "on", Value::Bool(true));
        rt.settle();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(reset));
        space(&mut rt, &mut kb);
        assert_eq!(counts(&mut rt), (2.0, 1.0));
    }

    /// QQuickItemPrivate::setEffectiveEnableRecur: an item that gets disabled keeps `focus`
    /// and loses active focus to its scope; enabled again, it has it back.
    #[test]
    fn disabled_item_leaves_active_focus_to_its_scope() {
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: win
    width: 400; height: 300
    property bool on: true
    property Item scope: null
    property Item stop: null
    property string keys: ""
    function focusStop() { stop.forceActiveFocus() }
    FocusScope {
        id: scope
        anchors.fill: parent
        Component.onCompleted: win.scope = scope
        Item {
            anchors.fill: parent
            Keys.onPressed: event => win.keys += event.text
            Button { id: stop; enabled: win.on; Component.onCompleted: win.stop = stop }
        }
    }
}"#,
            )
            .unwrap();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let item = |rt: &mut Runtime, n: &str| match rt.get(win, n) {
            Value::Item(i) => i,
            v => panic!("{n}: {v:?}"),
        };
        let (scope, stop) = (item(&mut rt, "scope"), item(&mut rt, "stop"));
        let mut kb = Keyboard {
            win: Some(win),
            ..Default::default()
        };
        rt.call_method(win, "focusStop", vec![]).unwrap().unwrap();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(stop));
        kb.key(&mut rt, 0x61, Some("a"), true, false);
        assert_eq!(rt.get(win, "keys").to_js_string(), "a");
        rt.set(win, "on", Value::Bool(false));
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(scope));
        assert!(rt.get(stop, "focus").truthy());
        assert!(!rt.get(stop, "activeFocus").truthy());
        // The key goes to the scope, not through the item under it.
        kb.key(&mut rt, 0x62, Some("b"), true, false);
        assert_eq!(rt.get(win, "keys").to_js_string(), "a");
        rt.set(win, "on", Value::Bool(true));
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(stop));
        assert!(rt.get(stop, "activeFocus").truthy());
    }

    /// QQuickItem::visible: "If this property is set to false, the item will no longer receive
    /// mouse events, but will continue to receive key events and will retain the keyboard focus
    /// if it has been set."
    #[test]
    fn hidden_item_keeps_active_focus_and_its_keys() {
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: win
    width: 400; height: 300
    property bool on: true
    property Item sign: null
    property string keys: ""
    function focusSign() { sign.forceActiveFocus() }
    Item {
        anchors.fill: parent
        Keys.onPressed: event => win.keys += event.text
        TextField { id: field }
        Rectangle {
            visible: win.on
            Button { id: sign; Component.onCompleted: win.sign = sign }
        }
    }
}"#,
            )
            .unwrap();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let Value::Item(sign) = rt.get(win, "sign") else {
            panic!("sign")
        };
        let mut kb = Keyboard {
            win: Some(win),
            ..Default::default()
        };
        rt.call_method(win, "focusSign", vec![]).unwrap().unwrap();
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(sign));
        rt.set(win, "on", Value::Bool(false));
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(sign));
        assert!(rt.get(sign, "activeFocus").truthy());
        // The key goes to the hidden button and on to the items above it.
        kb.key(&mut rt, 0x61, Some("a"), true, false);
        assert_eq!(rt.get(win, "keys").to_js_string(), "a");
    }

    #[test]
    fn focused_scope_hands_active_focus_to_item_taking_focus() {
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: win
    width: 400; height: 300
    property bool show: false
    property Item scope: null
    property Item input: null
    FocusScope {
        id: scope
        anchors.fill: parent
        focus: true
        Component.onCompleted: win.scope = scope
        Item {
            anchors.fill: parent
            TextField { id: input; visible: win.show; focus: win.show; Component.onCompleted: win.input = input }
        }
    }
}"#,
            )
            .unwrap();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let item = |rt: &mut Runtime, n: &str| match rt.get(win, n) {
            Value::Item(i) => i,
            v => panic!("{n}: {v:?}"),
        };
        let (scope, input) = (item(&mut rt, "scope"), item(&mut rt, "input"));
        let mut kb = Keyboard {
            win: Some(win),
            ..Default::default()
        };
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(scope));
        rt.set(win, "show", Value::Bool(true));
        kb.sync(&mut rt);
        assert_eq!(kb.active, Some(input));
        assert!(rt.get(input, "activeFocus").truthy());
    }

    /// Qt 6.12 (QQuickDeliveryAgentPrivate::setFocusInScope, QQuickItemPrivate::
    /// setEffectiveEnableRecur): a disabled item keeps `focus` and loses active focus to its
    /// focus scope, so the keys typed meanwhile do not reach it.
    #[test]
    fn disabled_item_gives_active_focus_to_its_scope() {
        for (open, close, in_scope) in [
            ("FocusScope { id: scope; focus: true", "}", true),
            ("", "", false),
        ] {
            let mut rt = Runtime::new(None);
            let win = rt
                .load_str(&format!(
                    r#"import QtQuick
import QtQuick.Controls
Item {{
    id: win
    width: 400; height: 300
    property bool busy: false
    property string seen: ""
    property Item input: null
    property Item scope: null
    function force() {{ input.forceActiveFocus() }}
    {open}
        Component.onCompleted: win.scope = this
        Keys.onPressed: event => win.seen += event.text
        TextField {{ id: input; focus: true; enabled: !win.busy; Component.onCompleted: win.input = input }}
    {close}
}}"#
                ))
                .unwrap();
            assert!(rt.errors().is_empty(), "{:?}", rt.errors());
            let item = |rt: &mut Runtime, n: &str| match rt.get(win, n) {
                Value::Item(i) => i,
                v => panic!("{n}: {v:?}"),
            };
            let input = item(&mut rt, "input");
            let scope = in_scope.then(|| item(&mut rt, "scope"));
            let mut kb = Keyboard {
                win: Some(win),
                ..Default::default()
            };
            let text = |rt: &mut Runtime| rt.get(input, "text").to_js_string();
            kb.key(&mut rt, 'a' as u32, Some("a"), true, false);
            assert_eq!(kb.active, Some(input));
            assert_eq!(text(&mut rt), "a");

            rt.set(win, "busy", Value::Bool(true));
            kb.key(&mut rt, 'b' as u32, Some("b"), true, false);
            assert_eq!(kb.active, scope, "in_scope={in_scope}");
            assert_eq!(text(&mut rt), "a");
            assert!(!rt.get(input, "activeFocus").truthy());
            assert!(rt.get(input, "focus").truthy());
            assert_eq!(rt.get(win, "seen").to_js_string(), "b");

            rt.call_method(win, "force", vec![]).unwrap().unwrap();
            kb.key(&mut rt, 'c' as u32, Some("c"), true, false);
            assert_eq!(kb.active, scope);
            assert_eq!(text(&mut rt), "a");

            rt.set(win, "busy", Value::Bool(false));
            kb.key(&mut rt, 'd' as u32, Some("d"), true, false);
            assert_eq!(kb.active, Some(input));
            assert!(rt.get(input, "activeFocus").truthy());
            assert_eq!(text(&mut rt), "ad");
        }
    }
}

/// One wl_seat capability change for a device slot (QWaylandInputDevice::seat_capabilities):
/// bind when the capability appears and nothing is bound; release and forget the device when it
/// goes away, so the next appearance (keyboard re-plugged) binds a fresh one.
pub fn seat_cap<T>(
    slot: &mut Option<T>,
    present: bool,
    bind: impl FnOnce() -> Option<T>,
    release: impl FnOnce(T),
) -> bool {
    match (present, slot.is_some()) {
        (true, false) => {
            *slot = bind();
            slot.is_some()
        }
        (false, true) => {
            release(slot.take().unwrap());
            true
        }
        _ => false,
    }
}

bits::bit_flags! {
/// zwp_text_input_v3 content hints and purposes (text-input-unstable-v3.xml).
pub const IME_HINT_HIDDEN_TEXT: u32 = 0x40;
pub const IME_HINT_SENSITIVE_DATA: u32 = 0x80;
pub const IME_HINT_MULTILINE: u32 = 0x200;
}
pub const IME_PURPOSE_PASSWORD: u32 = 8;

/// What the focused editor tells the input method (QWaylandTextInputv3::updateState):
/// surrounding text with UTF-8 byte cursor/anchor, content type and the caret rectangle in
/// window coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImeState {
    pub text: String,
    pub cursor: i32,
    pub anchor: i32,
    pub hint: u32,
    pub purpose: u32,
    pub rect: (i32, i32, i32, i32),
}

/// zwp_text_input_v3.set_surrounding_text is limited by the wire message size (Qt's
/// MAX_MESSAGE_SIZE).
const IME_MAX_BYTES: usize = 4000;

/// The text cursor at UTF-16 position `pos` of editor `i`: `[x, y, 1, line height]` in `win`'s
/// coordinates (QQuickTextInput/QQuickTextEdit::cursorRectangle mapped to the window).
fn caret_rect(
    w: &mut World,
    fonts: &mut crate::scene::Fonts,
    i: usize,
    win: usize,
    pos: usize,
) -> Option<[f64; 4]> {
    let (ox, oy) = w.scene_pos(i, Some(win));
    let [x, y, cw, h] = crate::scene::edit_cursor_rect(w, fonts, i, pos)?;
    Some([ox + x, oy + y, cw, h])
}

/// QQuickItem's Qt::ImInputItemClipRectangle in `win`'s coordinates: item `i`'s rectangle cut
/// by every clipping ancestor and the window; empty while the item is hidden or transparent.
fn clip_rect(w: &mut World, i: usize, win: usize) -> [f64; 4] {
    let num = |w: &mut World, it: usize, n: &str| {
        w.read(it, n)
            .map(|v| v.to_number())
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    };
    let hidden = w.read(i, "visible").is_some_and(|v| !v.truthy());
    if hidden || w.read(i, "opacity").is_some_and(|v| v.to_number() == 0.0) {
        return [0.0; 4];
    }
    let rect = |w: &mut World, it: usize| {
        let (x, y) = if it == win {
            (0.0, 0.0)
        } else {
            w.scene_pos(it, Some(win))
        };
        [x, y, x + num(w, it, "width"), y + num(w, it, "height")]
    };
    let mut r = rect(w, i);
    let mut cur = w.items[i].parent;
    while let Some(p) = cur {
        if p == win || w.read(p, "clip").is_some_and(|v| v.truthy()) {
            let c = rect(w, p);
            r = [
                r[0].max(c[0]),
                r[1].max(c[1]),
                r[2].min(c[2]),
                r[3].min(c[3]),
            ];
        }
        if p == win {
            break;
        }
        cur = w.items[p].parent;
    }
    if r[2] <= r[0] || r[3] <= r[1] {
        return [0.0; 4];
    }
    [r[0], r[1], r[2] - r[0], r[3] - r[1]]
}

thread_local! {
    /// The input method's preedit string, shown at the cursor of editor `.0` with the IME
    /// cursor at char `.2` of it, `None` while the input method hides the cursor
    /// (QQuickTextInput/QQuickTextEdit preeditText).
    static PREEDIT: std::cell::RefCell<Option<(usize, String, Option<usize>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The preedit string shown in editor `i` and its cursor (chars into it; `None`: hidden).
pub fn preedit(i: usize) -> Option<(String, Option<usize>)> {
    PREEDIT.with(|p| {
        p.borrow()
            .as_ref()
            .filter(|p| p.0 == i)
            .map(|p| (p.1.clone(), p.2))
    })
}

fn byte_to_char(s: &str, b: i32) -> usize {
    let b = (b.max(0) as usize).min(s.len());
    s.char_indices().take_while(|(k, _)| *k < b).count()
}

/// The cursor of preedit `s` for zwp_text_input_v3.preedit_string's `begin` and `end` bytes, as
/// Qt 6.12 shows it (QWaylandTextInputv3::zwp_text_input_v3_done): hidden when both are -1, at
/// the end of the preedit when both are 0 (Qt adds no Cursor attribute then), else at `begin`.
fn preedit_cursor(s: &str, begin: i32, end: i32) -> Option<usize> {
    match (begin, end) {
        (-1, -1) => None,
        (0, 0) => Some(s.chars().count()),
        _ => Some(byte_to_char(s, begin)),
    }
}

impl Keyboard {
    /// The editor an input method may write to: the focused TextInput/TextEdit unless it is
    /// readOnly (Qt's ImEnabled).
    pub fn ime_editor(&self, rt: &Runtime) -> Option<usize> {
        let i = self.active_editor(rt)?;
        (!truthy(&mut rt.world.borrow_mut(), i, "readOnly", false)).then_some(i)
    }

    /// The state to send for the IME editor. Password fields get no surrounding text: qs -c rusty-impulse
    /// sends the plain password to the input method, which has no use for it.
    pub fn ime_state(&self, rt: &Runtime) -> Option<ImeState> {
        let i = self.ime_editor(rt)?;
        let mut w = rt.world.borrow_mut();
        let echo = if w.items[i].prop_info("echoMode").is_some() {
            num(&mut w, i, "echoMode") as i32
        } else {
            0
        };
        let multiline = w.items[i].prop_info("acceptableInput").is_none();
        let mut st = ImeState {
            hint: if multiline { IME_HINT_MULTILINE } else { 0 },
            ..Default::default()
        };
        let num_at = |w: &mut World, n: &str| {
            w.read(i, n)
                .map(|v| v.to_number())
                .filter(|v| v.is_finite())
                .map_or(0, |v| v.max(0.0) as usize)
        };
        let cursor = num_at(&mut w, "cursorPosition");
        if echo != 0 {
            st.hint |= IME_HINT_HIDDEN_TEXT | IME_HINT_SENSITIVE_DATA;
            st.purpose = IME_PURPOSE_PASSWORD;
        } else {
            let text = w
                .read(i, "text")
                .as_ref()
                .and_then(Value::host_str)
                .map(|s| s.into_owned())
                .unwrap_or_default();
            let (s0, s1) = (
                num_at(&mut w, "selectionStart"),
                num_at(&mut w, "selectionEnd"),
            );
            let anchor = if s0 == s1 || cursor == s0 { s1 } else { s0 };
            let byte = |u: usize| textpos::to_byte(&text, u);
            let (bc, ba) = (byte(cursor), byte(anchor));
            // Keep a window around the cursor that fits one message, on char boundaries.
            let (lo, hi) = (bc.min(ba), bc.max(ba));
            let mut start = lo.saturating_sub(IME_MAX_BYTES.saturating_sub(hi - lo) / 2);
            while !text.is_char_boundary(start) {
                start += 1;
            }
            let mut end = (start + IME_MAX_BYTES).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            st.text = text[start..end].to_string();
            st.cursor = (bc.clamp(start, end) - start) as i32;
            st.anchor = (ba.clamp(start, end) - start) as i32;
        }
        if let Some(win) = self.win
            && let Some([x, y, _, h]) =
                caret_rect(&mut w, &mut self.fonts.borrow_mut(), i, win, cursor)
        {
            st.rect = (x.round() as i32, y.round() as i32, 1, h.ceil() as i32);
        }
        Some(st)
    }

    /// The composition shown in the focused editor (zwp_text_input_v3 preedit), if any.
    pub fn ime_preedit_text(&self) -> Option<String> {
        PREEDIT.with(|p| p.borrow().as_ref().map(|p| p.1.clone()))
    }

    /// `Qt.inputMethod`'s rectangles for the focused editor, in its window's coordinates
    /// (QInputMethod maps the item's Qt::ImCursorRectangle, ImAnchorRectangle and
    /// ImInputItemClipRectangle through the input item transform).
    pub fn im_geometry(&self, rt: &Runtime) -> qml::inputmethod::Geometry {
        let (Some(i), Some(win)) = (self.ime_editor(rt), self.win) else {
            return Default::default();
        };
        let mut w = rt.world.borrow_mut();
        let num_at = |w: &mut World, n: &str| {
            w.read(i, n)
                .map(|v| v.to_number())
                .filter(|v| v.is_finite())
                .map_or(0, |v| v.max(0.0) as usize)
        };
        let cursor = num_at(&mut w, "cursorPosition");
        let (s0, s1) = (
            num_at(&mut w, "selectionStart"),
            num_at(&mut w, "selectionEnd"),
        );
        let anchor = if s0 == s1 || cursor == s0 { s1 } else { s0 };
        let mut fonts = self.fonts.borrow_mut();
        let cursor_rect = caret_rect(&mut w, &mut fonts, i, win, cursor).unwrap_or_default();
        let anchor_rect = if anchor == cursor {
            cursor_rect
        } else {
            caret_rect(&mut w, &mut fonts, i, win, anchor).unwrap_or_default()
        };
        qml::inputmethod::Geometry {
            cursor: cursor_rect,
            anchor: anchor_rect,
            clip: clip_rect(&mut w, i, win),
        }
    }

    /// zwp_text_input_v3.done: show `preedit` (text, cursor begin and end bytes) at the cursor
    /// of the IME editor. Returns whether what is shown changed.
    pub fn ime_preedit(&mut self, rt: &mut Runtime, preedit: Option<(&str, i32, i32)>) -> bool {
        let next = match (self.ime_editor(rt), preedit) {
            (Some(i), Some((t, b, e))) if !t.is_empty() => {
                Some((i, t.to_string(), preedit_cursor(t, b, e)))
            }
            _ => None,
        };
        let prev = PREEDIT.with(|p| p.replace(next.clone()));
        if prev == next {
            return false;
        }
        let mut w = rt.world.borrow_mut();
        w.touched
            .extend(prev.iter().chain(next.iter()).map(|p| p.0));
        true
    }

    /// zwp_text_input_v3.done: delete `before`/`after` UTF-8 bytes around the cursor, then
    /// insert `commit` as typed text (selection replaced, maximumLength and inputMask applied
    /// by the same editing path as keys). Returns whether the text changed.
    pub fn ime_commit(&mut self, rt: &mut Runtime, before: u32, after: u32, commit: &str) -> bool {
        let Some(i) = self.ime_editor(rt) else {
            return false;
        };
        let mut changed = false;
        if before > 0 || after > 0 {
            let (s, p) = {
                let mut w = rt.world.borrow_mut();
                let s = w
                    .read(i, "text")
                    .as_ref()
                    .and_then(Value::host_str)
                    .map(|s| s.into_owned())
                    .unwrap_or_default();
                let p = w
                    .read(i, "cursorPosition")
                    .map(|v| v.to_number())
                    .filter(|v| v.is_finite())
                    .map_or(0, |v| v.max(0.0) as usize);
                (s, p)
            };
            let mut cs: Vec<char> = s.chars().collect();
            let p = textpos::to_char(&cs, p);
            let mut a = p;
            let mut n = 0;
            while a > 0 && n < before as usize {
                a -= 1;
                n += cs[a].len_utf8();
            }
            let (mut b, mut n) = (p, 0);
            while b < cs.len() && n < after as usize {
                n += cs[b].len_utf8();
                b += 1;
            }
            if a < b && w_has_no_mask(rt, i) {
                cs.drain(a..b);
                finish(rt, i, &cs, true, a, (a, a));
                changed = true;
            }
        }
        if !commit.is_empty() {
            let fonts = self.fonts.clone();
            changed |= edit(rt, &fonts, i, 0, commit, 0);
        }
        changed
    }
}

fn w_has_no_mask(rt: &Runtime, i: usize) -> bool {
    rt.world.borrow_mut().input_mask(i).is_none()
}

/// QWaylandInputDevice::Keyboard's client-side key repeat (Hyprland sends wl_keyboard
/// repeat_info and never repeats itself): a press of a repeating key arms `delay` ms, then
/// every 1000/rate ms the key is delivered again as an auto-repeat release+press pair with the
/// text of the original press. A press of another repeating key takes over; the release of the
/// repeated key (or a non-repeating press of the same code) stops it; other keys don't.
#[derive(Debug, Default)]
pub struct KeyRepeat {
    /// wl_keyboard.repeat_info: keys per second (0 disables) and delay in ms.
    rate: u32,
    delay: u32,
    key: Option<RepeatedKey>,
    /// Time (ms, caller's clock) the next repeat is due.
    due: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RepeatedKey {
    pub code: u32,
    pub sym: u32,
    pub utf8: Option<String>,
}

impl KeyRepeat {
    pub fn info(&mut self, rate: u32, delay: u32) {
        self.rate = rate;
        self.delay = delay;
        if rate == 0 {
            self.stop();
        }
    }

    /// A key went down at `now`; `repeats` is xkb_keymap_key_repeats for its keycode.
    pub fn press(&mut self, key: RepeatedKey, repeats: bool, now: f64) {
        if repeats && self.rate > 0 {
            self.due = Some(now + self.delay as f64);
            self.key = Some(key);
        } else if self.key.as_ref().is_some_and(|k| k.code == key.code) {
            self.stop();
        }
    }

    pub fn release(&mut self, code: u32) {
        if self.key.as_ref().is_some_and(|k| k.code == code) {
            self.stop();
        }
    }

    /// Keyboard focus left the surface (wl_keyboard.leave) or the keyboard went away.
    pub fn stop(&mut self) {
        self.key = None;
        self.due = None;
    }

    /// Time the next repeat is due, if one is armed.
    pub fn due(&self) -> Option<f64> {
        self.due
    }

    /// The key to deliver again if its repeat is due at `now`; re-arms 1000/rate ms on.
    pub fn fire(&mut self, now: f64) -> Option<RepeatedKey> {
        let due = self.due?;
        if now < due || self.rate == 0 {
            return None;
        }
        // Like a QTimer that missed its slots: one shot now, the next an interval later.
        let iv = 1000.0 / self.rate as f64;
        self.due = Some(if due + iv < now { now + iv } else { due + iv });
        self.key.clone()
    }
}

/// QWaylandDisplay window activation: wl_keyboard enter/leave only record the keyboard focus
/// window; it becomes the active window when a wl_display.sync requested at the change is
/// answered (handleWaylandSync), so an enter/leave pair in one batch never activates it.
/// `T` names the focus as wl_keyboard does: the shell keys it by wl_surface, so a window item
/// that gets a new surface (shown again, or replaced while its old surface retires) never
/// inherits the old surface's focus.
pub struct Activation<T = usize> {
    focus: Option<T>,
    active: Option<T>,
    syncing: bool,
}

impl<T> Default for Activation<T> {
    fn default() -> Self {
        Activation {
            focus: None,
            active: None,
            syncing: false,
        }
    }
}

impl<T: Clone + PartialEq> Activation<T> {
    /// wl_keyboard.enter on `win`; true when the caller must send a wl_display.sync and call
    /// `synced` on its done.
    pub fn enter(&mut self, win: T) -> bool {
        self.change(Some(win))
    }

    /// wl_keyboard.leave on `win` (ignored unless it holds the focus).
    pub fn leave(&mut self, win: &T) -> bool {
        self.focus.as_ref() == Some(win) && self.change(None)
    }

    fn change(&mut self, focus: Option<T>) -> bool {
        if self.focus == focus {
            return false;
        }
        self.focus = focus;
        !std::mem::replace(&mut self.syncing, true)
    }

    /// Windows for which `live` is false are gone (closed, hidden, or retired for a new
    /// surface): they lose the focus and activation at once, as QtWayland's
    /// QWaylandInputDevice::handleWindowDestroyed and QWaylandDisplay::handleWindowDestroyed
    /// have it. No sync is needed: a pending one still lands on the current focus. True when
    /// the focus window was gone.
    pub fn retain(&mut self, live: impl Fn(&T) -> bool) -> bool {
        if self.active.as_ref().is_some_and(|a| !live(a)) {
            self.active = None;
        }
        let gone = self.focus.as_ref().is_some_and(|f| !live(f));
        if gone {
            self.focus = None;
        }
        gone
    }

    /// The keyboard went away (~QWaylandInputDevice::Keyboard deactivates at once).
    pub fn lost(&mut self) {
        self.focus = None;
        self.active = None;
    }

    /// The sync callback is done: the active window from now on.
    pub fn synced(&mut self) -> Option<&T> {
        self.syncing = false;
        self.active.clone_from(&self.focus);
        self.active.as_ref()
    }

    pub fn active(&self) -> Option<&T> {
        self.active.as_ref()
    }

    /// The window that has the keyboard focus and is not active yet: the answer to the sync
    /// sent at its enter makes it the active one. A key handled before that answer does not
    /// reach it.
    pub fn activating(&self) -> Option<&T> {
        self.focus
            .as_ref()
            .filter(|f| self.syncing && self.active.as_ref() != Some(f))
    }
}

#[cfg(all(test, feature = "interp"))]
mod activation_tests {
    use super::*;

    /// Hyprland maps an OnDemand layer surface with keyboard focus and at once hands it back to
    /// the window under the pointer: enter and leave arrive in one batch. Qt never activates the
    /// window then (no activeFocus/editingFinished blip); a later enter activates it.
    /// A window that got the keyboard focus is on its way to being active until the sync sent
    /// at its enter is answered: not before the enter, not once active, not after a leave.
    #[test]
    fn a_focused_window_is_activating_until_its_sync_is_answered() {
        let mut act = Activation::<u32>::default();
        assert_eq!(act.activating(), None);
        assert!(act.enter(1));
        assert_eq!(act.activating(), Some(&1));
        assert_eq!(act.active(), None);
        assert_eq!(act.synced(), Some(&1));
        assert_eq!(act.activating(), None);
        // Leave and enter in one batch: the window stays active, nothing is on its way.
        assert!(act.leave(&1));
        assert_eq!(act.activating(), None);
        assert!(!act.enter(1));
        assert_eq!(act.activating(), None);
        assert_eq!(act.synced(), Some(&1));
        // The focus moves to another window before the answer.
        assert!(act.enter(2));
        assert_eq!(act.activating(), Some(&2));
        act.retain(|w| *w != 2);
        assert_eq!(act.activating(), None);
    }

    #[test]
    fn enter_leave_in_one_batch_never_activates_the_window() {
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: win
    width: 400; height: 300
    property string log: ""
    TextField {
        focus: true
        onActiveFocusChanged: win.log += "af=" + activeFocus + " "
    }
}"#,
            )
            .unwrap();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let mut act = Activation::default();
        let mut kb = Keyboard::default();
        // The shell retargets the keyboard after every event it handles.
        let retarget = |act: &Activation, kb: &mut Keyboard, rt: &mut Runtime| {
            kb.win = act.active().copied();
            kb.sync(rt);
        };
        let done = |act: &mut Activation, kb: &mut Keyboard, rt: &mut Runtime| {
            act.synced();
            retarget(act, kb, rt);
        };
        let log = |rt: &mut Runtime| rt.get(win, "log").to_js_string();
        let e = act.enter(win);
        retarget(&act, &mut kb, &mut rt);
        let l = act.leave(&win);
        retarget(&act, &mut kb, &mut rt);
        assert!(e && !l, "one wl_display.sync per batch");
        done(&mut act, &mut kb, &mut rt);
        assert_eq!(log(&mut rt), "");
        assert!(act.enter(win));
        retarget(&act, &mut kb, &mut rt);
        assert_eq!(log(&mut rt), "");
        done(&mut act, &mut kb, &mut rt);
        assert_eq!(log(&mut rt), "af=true ");
        assert!(act.leave(&win));
        retarget(&act, &mut kb, &mut rt);
        assert!(!act.enter(win));
        retarget(&act, &mut kb, &mut rt);
        done(&mut act, &mut kb, &mut rt);
        assert_eq!(log(&mut rt), "af=true ");
        assert!(act.leave(&win));
        retarget(&act, &mut kb, &mut rt);
        done(&mut act, &mut kb, &mut rt);
        assert_eq!(log(&mut rt), "af=true af=false ");
    }
}

#[cfg(test)]
mod activation_retain_tests {
    use super::Activation;

    /// A window closed while it holds the keyboard focus loses it and its activation at once
    /// (no wl_keyboard.leave reaches a destroyed surface), and its successor surface is not
    /// focused before its own enter.
    #[test]
    fn gone_window_loses_focus_and_activation() {
        let mut act = Activation::default();
        assert!(act.enter("old"));
        assert_eq!(act.synced(), Some(&"old"));
        assert!(!act.retain(|s| *s == "old"));
        assert_eq!(act.active(), Some(&"old"));
        assert!(act.retain(|s| *s == "new"));
        assert_eq!(act.active(), None);
        assert!(!act.leave(&"old"));
        assert!(act.enter("new"), "the successor's enter asks for a sync");
        assert_eq!(act.active(), None);
        assert_eq!(act.synced(), Some(&"new"));
    }

    /// Focus moved to a window that closes before the sync lands: the sync activates nothing.
    #[test]
    fn window_gone_before_its_sync_is_never_activated() {
        let mut act = Activation::default();
        assert!(act.enter(1));
        act.synced();
        assert!(act.enter(2));
        assert!(act.retain(|w| *w == 1));
        assert_eq!(act.active(), Some(&1));
        assert_eq!(act.synced(), None);
    }
}

#[cfg(test)]
mod seat_cap_tests {
    use super::seat_cap;

    #[test]
    fn keyboard_rebinds_after_capability_returns() {
        let mut slot = None;
        let mut released = vec![];
        assert!(seat_cap(&mut slot, true, || Some(1), |k| released.push(k)));
        assert!(!seat_cap(&mut slot, true, || Some(9), |k| released.push(k)));
        assert_eq!(slot, Some(1));
        assert!(seat_cap(&mut slot, false, || Some(9), |k| released.push(k)));
        assert_eq!((slot, released.as_slice()), (None, &[1][..]));
        assert!(seat_cap(&mut slot, true, || Some(2), |k| released.push(k)));
        assert_eq!(slot, Some(2));
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;

    thread_local! {
        pub(super) static CLIP_SET: std::cell::RefCell<Option<String>> =
            const { std::cell::RefCell::new(None) };
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn copy_and_cut_like_qt() {
        let take = || CLIP_SET.with(|c| c.borrow_mut().take());
        let load = |src: &str| {
            let mut rt = Runtime::new(None);
            let i = rt.load_str(&format!("import QtQuick\n{src}\n")).unwrap();
            rt.settle();
            (rt, i)
        };
        let sel = |rt: &mut Runtime, i: usize, a: f64, b: f64| {
            // Assigning cursorPosition deselects (QQuickTextInput::setCursorPosition): set it first.
            let mut w = rt.world.borrow_mut();
            w.write(i, "cursorPosition", Value::Num(b));
            w.write(i, "selectionStart", Value::Num(a));
            w.write(i, "selectionEnd", Value::Num(b));
        };
        let text = |rt: &Runtime, i: usize| {
            rt.world
                .borrow_mut()
                .read(i, "text")
                .as_ref()
                .and_then(Value::host_str)
                .map(|s| s.into_owned())
                .unwrap_or_default()
        };

        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let (mut rt, i) = load("TextInput { text: \"молоко 2%\" }");
        sel(&mut rt, i, 0.0, 6.0);
        assert!(edit(&mut rt, &fonts, i, KEY_C, "", CTRL));
        assert_eq!(take().as_deref(), Some("молоко"));
        assert_eq!(text(&rt, i), "молоко 2%");
        assert!(edit(&mut rt, &fonts, i, KEY_INSERT, "", CTRL));
        assert_eq!(take().as_deref(), Some("молоко"));
        assert!(edit(&mut rt, &fonts, i, KEY_X, "", CTRL));
        assert_eq!(take().as_deref(), Some("молоко"));
        assert_eq!(text(&rt, i), " 2%");
        // No selection: copy does nothing, cut deletes the next character.
        assert!(edit(&mut rt, &fonts, i, KEY_C, "", CTRL));
        assert_eq!(take(), None);
        assert!(edit(&mut rt, &fonts, i, KEY_X, "", CTRL));
        assert_eq!(take(), None);
        assert_eq!(text(&rt, i), "2%");

        let (mut rt, i) = load("TextInput { text: \"secret\"; echoMode: TextInput.Password }");
        sel(&mut rt, i, 0.0, 6.0);
        assert!(edit(&mut rt, &fonts, i, KEY_C, "", CTRL));
        assert!(edit(&mut rt, &fonts, i, KEY_X, "", CTRL));
        assert_eq!(take(), None);
        assert_eq!(text(&rt, i), "secret");

        let (mut rt, i) = load("TextInput { text: \"ro\"; readOnly: true }");
        sel(&mut rt, i, 0.0, 2.0);
        assert!(edit(&mut rt, &fonts, i, KEY_DELETE, "", SHIFT));
        assert_eq!(text(&rt, i), "ro");
        assert!(edit(&mut rt, &fonts, i, KEY_C, "", CTRL));
        assert_eq!(take().as_deref(), Some("ro"));

        let (mut rt, i) = load("TextEdit { text: \"ab\\ncd\" }");
        sel(&mut rt, i, 1.0, 4.0);
        assert!(edit(&mut rt, &fonts, i, KEY_F20, "", 0));
        assert_eq!(take().as_deref(), Some("b\nc"));
        assert_eq!(text(&rt, i), "ad");
        // TextEdit cut without a selection is a no-op.
        assert!(edit(&mut rt, &fonts, i, KEY_DELETE, "", SHIFT));
        assert_eq!(text(&rt, i), "ad");
    }

    /// QQuickTextInputPrivate::processKeyEvent, DeleteCompleteLine: `selectAll(); copy(); del();`.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn ctrl_u_copies_the_line_like_qt() {
        let take = || CLIP_SET.with(|c| c.borrow_mut().take());
        let load = |src: &str| {
            let mut rt = Runtime::new(None);
            let i = rt.load_str(&format!("import QtQuick\n{src}\n")).unwrap();
            rt.settle();
            (rt, i)
        };
        let text = |rt: &Runtime, i: usize| {
            let v = rt.world.borrow_mut().read(i, "text");
            v.as_ref()
                .and_then(Value::host_str)
                .map(|s| s.into_owned())
                .unwrap_or_default()
        };
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let (mut rt, i) = load("TextInput { text: \"молоко 2%\" }");
        assert!(edit(&mut rt, &fonts, i, KEY_U, "\u{15}", CTRL));
        assert_eq!(take().as_deref(), Some("молоко 2%"));
        assert_eq!(text(&rt, i), "");
        // Nothing to copy: the clipboard keeps what it had.
        assert!(edit(&mut rt, &fonts, i, KEY_U, "\u{15}", CTRL));
        assert_eq!(take(), None);

        let (mut rt, i) = load("TextInput { text: \"secret\"; echoMode: TextInput.Password }");
        assert!(edit(&mut rt, &fonts, i, KEY_U, "\u{15}", CTRL));
        assert_eq!(take(), None);
        assert_eq!(text(&rt, i), "");
    }

    /// QInputControl::isAcceptableInput: Alt, Meta and Ctrl+Alt do not stop a printable
    /// character; exactly Ctrl and Ctrl+Shift do.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn printable_text_is_typed_with_alt_or_meta_held() {
        let mut rt = Runtime::new(None);
        let i = rt.load_str("import QtQuick\nTextInput {}\n").unwrap();
        rt.settle();
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let text = |rt: &Runtime| {
            let v = rt.world.borrow_mut().read(i, "text");
            v.as_ref()
                .and_then(Value::host_str)
                .map(|s| s.into_owned())
                .unwrap_or_default()
        };
        assert!(edit(&mut rt, &fonts, i, KEY_E, "e", ALT));
        assert!(edit(&mut rt, &fonts, i, KEY_E, "é", META));
        assert!(edit(&mut rt, &fonts, i, KEY_E, "€", CTRL | ALT));
        assert!(edit(&mut rt, &fonts, i, KEY_E, "E", ALT | SHIFT));
        assert_eq!(text(&rt), "eé€E");
        const KEY_Q: u32 = 0x51;
        assert!(!edit(&mut rt, &fonts, i, KEY_Q, "q", CTRL));
        assert!(!edit(&mut rt, &fonts, i, KEY_Q, "Q", CTRL | SHIFT));
        assert_eq!(text(&rt), "eé€E");
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn text_input_ignores_arrows_off_the_end_and_alt_arrows() {
        let mut rt = Runtime::new(None);
        let i = rt
            .load_str("import QtQuick\nTextInput { text: \"ab\" }\n")
            .unwrap();
        rt.settle();
        let at = |rt: &mut Runtime, p: f64| {
            rt.world
                .borrow_mut()
                .write(i, "cursorPosition", Value::Num(p));
        };
        let pos = |rt: &Runtime| {
            rt.world
                .borrow_mut()
                .read(i, "cursorPosition")
                .map(|v| v.to_number())
        };
        let f = std::cell::RefCell::new(crate::scene::Fonts::default());
        at(&mut rt, 2.0);
        assert!(!edit(&mut rt, &f, i, KEY_RIGHT, "", 0));
        assert!(edit(&mut rt, &f, i, KEY_LEFT, "", 0));
        assert_eq!(pos(&rt), Some(1.0));
        assert!(!edit(&mut rt, &f, i, KEY_LEFT, "", ALT));
        assert!(!edit(&mut rt, &f, i, KEY_RIGHT, "", ALT));
        assert_eq!(pos(&rt), Some(1.0));
        at(&mut rt, 0.0);
        assert!(!edit(&mut rt, &f, i, KEY_LEFT, "", 0));
        assert!(edit(&mut rt, &f, i, KEY_RIGHT, "", SHIFT));
    }

    #[test]
    fn every_abstract_button_but_delegates_takes_tab_focus() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nItem {\n Button {}\n TabButton {}\n CheckBox {}\n ToolButton {}\n ItemDelegate {}\n SwitchDelegate {}\n Item {}\n}\n",
            )
            .unwrap();
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let kids = w.items[root].children.clone();
        let got: Vec<bool> = kids.iter().map(|&k| tab_focusable(&mut w, k)).collect();
        assert_eq!(got, [true, true, true, true, false, false, false]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xkb(layout: &str, variant: &str) -> Xkb {
        let mut x = Xkb {
            compose: None,
            ..Xkb::default()
        };
        let km = xkb::Keymap::new_from_names(
            &x.ctx,
            "evdev",
            "pc105",
            layout,
            variant,
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        x.set_keymap(&km);
        x
    }

    /// One xkb state answers keysym, text, modifiers and the Ctrl Latin fallback: a `us,ru`
    /// keymap on the ru group types Cyrillic, Ctrl+the key is Latin, Shift capitalises.
    #[test]
    fn one_xkb_state_gives_text_mods_and_latin_shortcuts() {
        let mut x = xkb("us,ru", "");
        // evdev KEY_A (30): Cyrillic_ef on group 1.
        assert_eq!(x.modifiers(0, 0, 0, 1), 0);
        assert_eq!(x.press(30), Some((0x6c6, Some("ф".into()))));
        // Shift (mask 1) and Control (mask 4) as the compositor reports them.
        assert_eq!(x.modifiers(1, 0, 0, 1), SHIFT);
        assert_eq!(x.press(30), Some((0x6e6, Some("Ф".into()))));
        assert_eq!(x.modifiers(4, 0, 0, 1), CTRL);
        assert_eq!(x.sym(30), Some(0x61));
        assert_eq!(x.press(30).map(|p| p.0), Some(0x61));
        // Modifier keys never auto-repeat, letters do (xkb_keymap_key_repeats).
        assert!(x.repeats(30) && !x.repeats(42));
        // A v10 compositor repeat carries the key's text like the press did.
        x.modifiers(0, 0, 0, 0);
        assert_eq!(x.repeat(30), Some((0x61, Some("a".into()))));
    }

    /// wl_keyboard.keymap: the compositor's fd holds the keymap text plus its NUL and `size`
    /// counts the NUL (wlroots, mutter, kwin); it compiles to the same keymap.
    #[test]
    fn keymap_fd_compiles_a_nul_terminated_compositor_keymap() {
        use std::io::{Seek, Write};
        let km = xkb("us", "").state.unwrap().get_keymap();
        let text = km.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(text.as_bytes()).unwrap();
        f.write_all(b"\0").unwrap();
        f.rewind().unwrap();
        let mut x = Xkb {
            compose: None,
            ..Xkb::default()
        };
        assert!(x.keymap_fd(f.into(), text.len() as u32 + 1));
        assert_eq!(x.press(30), Some((0x61, Some("a".into()))));
    }

    /// Text goes through the compose state: a dead key types nothing, the next key types the
    /// composed character (Qt's compose input context).
    #[test]
    fn dead_keys_compose_through_the_locale_table() {
        let mut x = xkb("us", "intl");
        let table = xkb::compose::Table::new_from_buffer(
            &x.ctx,
            "<dead_acute> <e> : \"é\" eacute\n",
            "C",
            xkb::compose::FORMAT_TEXT_V1,
            xkb::compose::COMPILE_NO_FLAGS,
        )
        .unwrap();
        x.compose = Some(xkb::compose::State::new(
            &table,
            xkb::compose::STATE_NO_FLAGS,
        ));
        // evdev KEY_APOSTROPHE (40) is dead_acute on us(intl), KEY_E is 18.
        assert_eq!(x.press(40), Some((0xfe51, None)));
        assert_eq!(x.press(18), Some((0x65, Some("é".into()))));
        assert_eq!(x.press(18), Some((0x65, Some("e".into()))));
    }

    /// No keymap event (or not xkb_v1): keys use the default keymap like QtWayland's
    /// createDefaultKeymap instead of being dropped.
    #[test]
    fn keys_before_a_keymap_use_the_default_keymap() {
        let mut x = Xkb {
            compose: None,
            ..Xkb::default()
        };
        assert!(x.press(30).is_some());
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn ctrl_shortcuts_use_the_latin_layout_like_qt() {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = xkb::Keymap::new_from_names(
            &ctx,
            "evdev",
            "pc105",
            "us,ru",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        let mut st = xkb::State::new(&km);
        st.update_mask(0, 0, 0, 0, 0, 1);
        // evdev KEY_A (30) and KEY_V (47) on the ru layout: Cyrillic_ef / Cyrillic_em.
        let (a, v) = (xkb::Keycode::new(30 + 8), xkb::Keycode::new(47 + 8));
        let ef = st.key_get_one_sym(a).raw();
        assert_eq!(ef, 0x6c6);
        assert_eq!(shortcut_keysym(&st, a, ef, false), ef);
        assert_eq!(shortcut_keysym(&st, a, ef, true), 0x61);
        assert_eq!(
            shortcut_keysym(&st, v, st.key_get_one_sym(v).raw(), true),
            0x76
        );

        let mut rt = Runtime::new(None);
        let i = rt
            .load_str("import QtQuick\nTextInput { text: \"abc\" }\n")
            .unwrap();
        rt.settle();
        let key = qt_key(shortcut_keysym(&st, a, ef, true), Some("\u{1}"));
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        assert!(edit(&mut rt, &fonts, i, key, "\u{1}", CTRL));
        assert_eq!(
            rt.world
                .borrow_mut()
                .read(i, "selectionEnd")
                .map(|v| v.to_number()),
            Some(3.0)
        );
    }
    /// QQuickTextControl (TextEdit/TextArea) puts a keyboard selection into the primary
    /// selection (cursorMoveKeyEvent/selectAll -> selectionChanged(true) ->
    /// setClipboardSelection); QQuickTextInput only exports a mouse selection.
    #[test]
    fn text_edit_keyboard_selection_exports_primary() {
        for (ty, exports) in [
            ("TextEdit", true),
            ("TextArea", true),
            ("TextField", false),
            ("TextInput", false),
        ] {
            let mut rt = Runtime::new(None);
            let i = rt
                .load_str(&format!(
                    "import QtQuick\nimport QtQuick.Controls\n{ty} {{ }}\n"
                ))
                .unwrap();
            rt.settle();
            let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
            assert!(edit(&mut rt, &fonts, i, 0, "abc def", 0));
            assert_eq!(take_primary_out(), None, "{ty} typing");
            let want = |s: &str| exports.then(|| s.to_string());
            assert!(edit(&mut rt, &fonts, i, KEY_LEFT, "", SHIFT));
            assert_eq!(take_primary_out(), want("f"), "{ty} shift+left");
            assert!(edit(&mut rt, &fonts, i, KEY_A, "", CTRL));
            assert_eq!(take_primary_out(), want("abc def"), "{ty} ctrl+a");
            assert!(edit(&mut rt, &fonts, i, KEY_A, "", CTRL));
            assert_eq!(take_primary_out(), None, "{ty} ctrl+a again");
            assert!(edit(&mut rt, &fonts, i, KEY_LEFT, "", 0));
            assert_eq!(take_primary_out(), None, "{ty} deselect");
        }
    }
}

#[cfg(all(test, feature = "interp"))]
mod selection_order_tests {
    use super::*;

    fn logged(ty: &str) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                r#"import QtQuick
import QtQuick.Controls
{ty} {{
    property string out: ""
    property bool on: true
    function lg(sig) {{ if (on) out += sig + " t=" + selectedText + " s=" + selectionStart + " e=" + selectionEnd + " c=" + cursorPosition + " x=" + text + "\n" }}
    text: "hello world"
    onSelectedTextChanged: lg("selectedText")
    onSelectionStartChanged: lg("selectionStart")
    onSelectionEndChanged: lg("selectionEnd")
    onCursorPositionChanged: lg("cursorPosition")
    onTextChanged: lg("text")
}}"#
            ))
            .unwrap();
        rt.settle();
        rt.set(r, "out", Value::str(""));
        (rt, r)
    }

    fn lines(rt: &mut Runtime, r: usize) -> Vec<String> {
        rt.get(r, "out")
            .to_js_string()
            .lines()
            .map(String::from)
            .collect()
    }

    /// Logged from Qt 6.12 (`qs`): every getter inside every change handler, Wayland keys.
    #[test]
    fn keyboard_selection_signals_match_qt() {
        let input: &[&str] = &[
            "cursorPosition t= s=11 e=11 c=0 x=hello world",
            "selectionStart t= s=0 e=11 c=0 x=hello world",
            "selectionEnd t= s=0 e=0 c=0 x=hello world",
            "selectedText t=h s=0 e=0 c=1 x=hello world",
            "selectionEnd t=h s=0 e=1 c=1 x=hello world",
            "cursorPosition t=h s=0 e=1 c=1 x=hello world",
            "selectedText t=he s=0 e=1 c=2 x=hello world",
            "selectionEnd t=he s=0 e=2 c=2 x=hello world",
            "cursorPosition t=he s=0 e=2 c=2 x=hello world",
            "selectedText t=h s=0 e=2 c=1 x=hello world",
            "selectionEnd t=h s=0 e=1 c=1 x=hello world",
            "cursorPosition t=h s=0 e=1 c=1 x=hello world",
            "selectedText t= s=0 e=1 c=1 x=hello world",
            "selectionStart t= s=1 e=1 c=1 x=hello world",
            "selectionEnd t= s=1 e=1 c=1 x=hello world",
            "cursorPosition t= s=1 e=1 c=2 x=hello world",
            "selectionStart t= s=2 e=1 c=2 x=hello world",
            "selectionEnd t= s=2 e=2 c=2 x=hello world",
            "selectedText t=hello world s=2 e=2 c=11 x=hello world",
            "selectionStart t=hello world s=0 e=2 c=11 x=hello world",
            "selectionEnd t=hello world s=0 e=11 c=11 x=hello world",
            "cursorPosition t=hello world s=0 e=11 c=11 x=hello world",
            "text t= s=0 e=11 c=1 x=Z",
            "selectedText t= s=0 e=11 c=1 x=Z",
            "selectionStart t= s=1 e=11 c=1 x=Z",
            "selectionEnd t= s=1 e=1 c=1 x=Z",
            "cursorPosition t= s=1 e=1 c=1 x=Z",
            "selectedText t=Z s=1 e=1 c=0 x=Z",
            "selectionStart t=Z s=0 e=1 c=0 x=Z",
            "cursorPosition t=Z s=0 e=1 c=0 x=Z",
        ];
        let edit_: &[&str] = &[
            "selectionEnd t=h s=0 e=1 c=1 x=hello world",
            "cursorPosition t=h s=0 e=1 c=1 x=hello world",
            "selectedText t=h s=0 e=1 c=1 x=hello world",
            "selectionEnd t=he s=0 e=2 c=2 x=hello world",
            "cursorPosition t=he s=0 e=2 c=2 x=hello world",
            "selectedText t=he s=0 e=2 c=2 x=hello world",
            "selectionEnd t=h s=0 e=1 c=1 x=hello world",
            "cursorPosition t=h s=0 e=1 c=1 x=hello world",
            "selectedText t=h s=0 e=1 c=1 x=hello world",
            "selectedText t= s=1 e=1 c=1 x=hello world",
            "selectionStart t= s=1 e=1 c=1 x=hello world",
            "selectionStart t= s=2 e=2 c=2 x=hello world",
            "selectionEnd t= s=2 e=2 c=2 x=hello world",
            "cursorPosition t= s=2 e=2 c=2 x=hello world",
            "selectedText t=hello world s=0 e=11 c=11 x=hello world",
            "selectionStart t=hello world s=0 e=11 c=11 x=hello world",
            "selectionEnd t=hello world s=0 e=11 c=11 x=hello world",
            "selectionStart t= s=1 e=1 c=1 x=hello world",
            "selectionEnd t= s=1 e=1 c=1 x=hello world",
            "cursorPosition t= s=1 e=1 c=1 x=hello world",
            "selectedText t= s=1 e=1 c=1 x=hello world",
            "text t= s=1 e=1 c=1 x=Z",
            "selectionStart t=Z s=0 e=1 c=0 x=Z",
            "cursorPosition t=Z s=0 e=1 c=0 x=Z",
            "selectedText t=Z s=0 e=1 c=0 x=Z",
        ];
        let input2: &[&str] = &[
            "selectedText t=world s=11 e=11 c=6 x=hello world",
            "selectionStart t=world s=6 e=11 c=6 x=hello world",
            "cursorPosition t=world s=6 e=11 c=6 x=hello world",
            "text t= s=6 e=11 c=6 x=hello ",
            "selectedText t= s=6 e=11 c=6 x=hello ",
            "selectionStart t= s=6 e=11 c=6 x=hello ",
            "selectionEnd t= s=6 e=6 c=6 x=hello ",
        ];
        let edit2: &[&str] = &[
            "selectionStart t= s=11 e=11 c=11 x=hello world",
            "selectionEnd t= s=11 e=11 c=11 x=hello world",
            "cursorPosition t= s=11 e=11 c=11 x=hello world",
            "selectionStart t=world s=6 e=11 c=6 x=hello world",
            "cursorPosition t=world s=6 e=11 c=6 x=hello world",
            "selectedText t=world s=6 e=11 c=6 x=hello world",
            "selectedText t= s=6 e=6 c=6 x=hello world",
            "selectionEnd t= s=6 e=6 c=6 x=hello world",
            "text t= s=6 e=6 c=6 x=hello ",
        ];
        for (ty, want, want2) in [
            ("TextInput", input, input2),
            ("TextField", input, input2),
            ("TextEdit", edit_, edit2),
            ("TextArea", edit_, edit2),
        ] {
            let (mut rt, f) = logged(ty);
            let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
            edit(&mut rt, &fonts, f, KEY_HOME, "", 0);
            edit(&mut rt, &fonts, f, KEY_RIGHT, "", SHIFT);
            edit(&mut rt, &fonts, f, KEY_RIGHT, "", SHIFT);
            edit(&mut rt, &fonts, f, KEY_LEFT, "", SHIFT);
            edit(&mut rt, &fonts, f, KEY_RIGHT, "", 0);
            edit(&mut rt, &fonts, f, KEY_RIGHT, "", 0);
            edit(&mut rt, &fonts, f, KEY_A, "\u{1}", CTRL);
            edit(&mut rt, &fonts, f, 0x5a, "Z", 0);
            edit(&mut rt, &fonts, f, KEY_HOME, "", SHIFT);
            assert_eq!(lines(&mut rt, f), want, "{ty}");

            let (mut rt, f) = logged(ty);
            let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
            edit(&mut rt, &fonts, f, KEY_END, "", 0);
            edit(&mut rt, &fonts, f, KEY_LEFT, "", CTRL | SHIFT);
            edit(&mut rt, &fonts, f, KEY_DELETE, "", 0);
            assert_eq!(lines(&mut rt, f), want2, "{ty}");
        }
    }
}

#[cfg(test)]
mod repeat_tests {
    use super::*;

    /// A held letter in a TextField: qs -c rusty-impulse types it once per repeat, and Keys.onPressed sees
    /// the repeats with isAutoRepeat (onReleased gets the auto-repeat releases too).
    #[cfg(feature = "interp")]
    #[test]
    fn auto_repeat_types_again_and_flags_is_auto_repeat() {
        let (mut rt, mut kb, _root, f) = super::paste_tests::field(
            "TextField",
            "property int reps: 0\n property int rels: 0\n \
             Keys.onPressed: (event) => { if (event.isAutoRepeat) reps += 1 }\n \
             Keys.onReleased: (event) => { if (event.isAutoRepeat) rels += 1 }",
        );
        kb.key(&mut rt, 0x41, Some("a"), true, false);
        kb.auto_repeat(&mut rt, 0x41, Some("a"));
        kb.auto_repeat(&mut rt, 0x41, Some("a"));
        kb.key(&mut rt, 0x41, Some("a"), false, false);
        rt.settle();
        assert_eq!(super::paste_tests::get(&rt, f, "text"), "aaa");
        assert_eq!(super::paste_tests::get(&rt, f, "reps"), "2");
        assert_eq!(super::paste_tests::get(&rt, f, "rels"), "2");
    }

    fn k(code: u32) -> RepeatedKey {
        RepeatedKey {
            code,
            sym: code,
            utf8: Some("a".into()),
        }
    }

    #[test]
    fn repeats_after_delay_at_rate_like_qt_wayland() {
        let mut r = KeyRepeat::default();
        r.info(25, 600);
        r.press(k(30), true, 1000.0);
        assert_eq!(r.due(), Some(1600.0));
        assert_eq!(r.fire(1599.0), None);
        assert_eq!(r.fire(1600.0), Some(k(30)));
        assert_eq!(r.due(), Some(1640.0));
        assert_eq!(r.fire(1640.0), Some(k(30)));
        r.release(30);
        assert_eq!((r.due(), r.fire(5000.0)), (None, None));
    }

    #[test]
    fn only_the_repeated_key_or_leave_stops_it() {
        let mut r = KeyRepeat::default();
        r.info(25, 600);
        r.press(k(30), true, 0.0);
        // Shift (non-repeating) going down or up keeps the held letter repeating.
        r.press(k(42), false, 100.0);
        r.release(42);
        assert!(r.due().is_some());
        // A second repeating key takes over; releasing the first one no longer matters.
        r.press(k(31), true, 200.0);
        r.release(30);
        assert_eq!(r.fire(800.0), Some(k(31)));
        r.stop();
        assert_eq!(r.fire(10_000.0), None);
    }

    #[test]
    fn non_repeating_keys_and_rate_zero_never_repeat() {
        let mut r = KeyRepeat::default();
        r.info(25, 600);
        r.press(k(42), false, 0.0);
        assert_eq!(r.due(), None);
        r.press(k(30), true, 0.0);
        r.info(0, 600);
        assert_eq!(r.fire(10_000.0), None);
        r.press(k(30), true, 0.0);
        assert_eq!(r.due(), None);
    }

    /// wayland.xml wl_keyboard.key: a v10 compositor sends `repeated` keys only after a
    /// repeat_info with rate 0, which disarms the client timer: a held key then repeats
    /// exactly as often as the compositor says, never twice per repeat.
    #[test]
    fn compositor_repeats_only_after_rate_zero_which_disarms_the_timer() {
        let mut r = KeyRepeat::default();
        r.info(25, 600);
        r.press(k(30), true, 0.0);
        assert!(r.due().is_some());
        // The compositor takes over mid-hold: the armed repeat is dropped at once.
        r.info(0, 0);
        assert_eq!((r.due(), r.fire(10_000.0)), (None, None));
        // Later presses while the compositor repeats never arm the timer.
        r.press(k(31), true, 20_000.0);
        assert_eq!(r.due(), None);
        assert!(
            (20_000..40_000)
                .step_by(40)
                .all(|t| r.fire(t as f64).is_none())
        );
    }

    #[test]
    fn a_stalled_loop_does_not_burst_repeats() {
        let mut r = KeyRepeat::default();
        r.info(50, 100);
        r.press(k(30), true, 0.0);
        assert!(r.fire(5000.0).is_some());
        // One repeat per wakeup, then the normal interval again (no catch-up flood).
        assert_eq!(r.fire(5000.0), None);
        assert_eq!(r.due(), Some(5020.0));
    }
}

#[cfg(all(test, feature = "interp"))]
mod ime_tests {
    use super::paste_tests::{field, get};
    use super::*;

    /// fcitx5/IBus commit text through zwp_text_input_v3; qs -c rusty-impulse types it like keys, so
    /// maximumLength still applies and onTextEdited fires.
    #[test]
    fn commit_string_types_into_the_focused_field() {
        let (mut rt, mut kb, root, f) = field("TextField", "maximumLength: 4");
        assert!(kb.ime_commit(&mut rt, 0, 0, "при"));
        assert!(kb.ime_commit(&mut rt, 0, 0, "вет"));
        rt.settle();
        assert_eq!(get(&rt, f, "text"), "прив");
        assert_eq!(get(&rt, root, "edits"), "2");
    }

    /// QQuickTextInput counts cursorPosition in UTF-16 units: an astral character is two, the
    /// keys and the text-input-v3 byte offsets step over it whole.
    #[test]
    fn astral_text_keeps_utf16_positions() {
        let (mut rt, mut kb, _root, f) = field("TextField", "text: \"😀a\"");
        let fonts = std::cell::RefCell::new(crate::scene::Fonts::default());
        let at = |rt: &Runtime| get(rt, f, "cursorPosition");
        let ime = |rt: &Runtime, kb: &Keyboard| {
            let st = kb.ime_state(rt).unwrap();
            (st.cursor, st.anchor)
        };
        assert_eq!(at(&rt), "3");
        assert!(kb.ime_commit(&mut rt, 0, 0, "b"));
        rt.settle();
        assert_eq!((get(&rt, f, "text"), at(&rt)), ("😀ab".into(), "4".into()));
        assert_eq!(ime(&rt, &kb), (6, 6));
        for want in ["3", "2", "0"] {
            assert!(edit(&mut rt, &fonts, f, KEY_LEFT, "", 0));
            assert_eq!(at(&rt), want);
        }
        assert!(edit(&mut rt, &fonts, f, KEY_RIGHT, "", SHIFT));
        assert_eq!(get(&rt, f, "selectedText"), "😀");
        assert_eq!(get(&rt, f, "selectionEnd"), "2");
        assert_eq!(ime(&rt, &kb), (4, 0));
        assert!(edit(&mut rt, &fonts, f, KEY_RIGHT, "", 0));
        assert!(edit(&mut rt, &fonts, f, KEY_BACKSPACE, "", 0));
        assert_eq!((get(&rt, f, "text"), at(&rt)), ("ab".into(), "0".into()));
    }

    /// delete_surrounding_text counts UTF-8 bytes around the cursor.
    #[test]
    fn delete_surrounding_counts_utf8_bytes() {
        let (mut rt, mut kb, _root, f) = field("TextField", "cursorPosition: 2; text: \"añb\"");
        assert!(kb.ime_commit(&mut rt, 2, 1, "x"));
        rt.settle();
        assert_eq!(get(&rt, f, "text"), "ax");
        let st = kb.ime_state(&rt).unwrap();
        assert_eq!((st.text.as_str(), st.cursor, st.anchor), ("ax", 2, 2));
    }

    #[test]
    fn state_reports_byte_cursor_and_selection_anchor() {
        let (rt, kb, _root, _f) = field(
            "TextArea",
            "text: \"żółw ok\"; Component.onCompleted: select(1, 3)",
        );
        let st = kb.ime_state(&rt).unwrap();
        assert_eq!(st.text, "żółw ok");
        assert_eq!((st.anchor, st.cursor), (2, 6));
        assert_eq!((st.hint, st.purpose), (IME_HINT_MULTILINE, 0));
    }

    /// Password fields are flagged hidden/sensitive and their text is not sent at all.
    #[test]
    fn password_field_sends_no_surrounding_text() {
        let (rt, kb, _root, _f) = field(
            "TextField",
            "text: \"hunter2\"; echoMode: TextInput.Password",
        );
        let st = kb.ime_state(&rt).unwrap();
        assert_eq!(st.text, "");
        assert_eq!(st.purpose, IME_PURPOSE_PASSWORD);
        assert_eq!(
            st.hint & (IME_HINT_HIDDEN_TEXT | IME_HINT_SENSITIVE_DATA),
            0xc0
        );
    }

    /// Qt.inputMethod's rectangles: the caret at the cursor and at the selection anchor, and
    /// the editor cut by its clipping ancestor, all in window coordinates.
    #[test]
    fn input_method_geometry_follows_the_focused_editor() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nItem { width: 200; height: 100\n \
                 Item { x: 10; y: 5; width: 50; height: 40; clip: true\n \
                 TextField { x: 20; y: 3; width: 100; height: 30; focus: true; text: \"abcd\"\n \
                 Component.onCompleted: select(1, 3) } } }",
            )
            .unwrap();
        rt.settle();
        let mut kb = Keyboard {
            win: Some(root),
            ..Default::default()
        };
        kb.sync(&mut rt);
        let g = kb.im_geometry(&rt);
        let [cx, cy, cw, ch] = g.cursor;
        let [ax, ay, aw, ah] = g.anchor;
        assert!(cx > ax && ax >= 30.0, "{g:?}");
        assert_eq!((cy, cw, ch), (ay, aw, ah));
        assert_eq!(cw, 1.0);
        assert!(ch > 0.0);
        let [x, y, w, h] = g.clip;
        assert_eq!([x, y, w, h], [30.0, 8.0, 30.0, 30.0]);
        let kb = Keyboard::default();
        assert_eq!(kb.im_geometry(&rt), Default::default());
    }

    #[test]
    fn read_only_editor_takes_no_input_method() {
        let (mut rt, mut kb, _root, f) = field("TextField", "text: \"x\"; readOnly: true");
        assert!(kb.ime_state(&rt).is_none());
        assert!(!kb.ime_commit(&mut rt, 0, 0, "y"));
        assert!(!kb.ime_preedit(&mut rt, Some(("y", 1, 1))));
        assert_eq!(get(&rt, f, "text"), "x");
    }

    /// The preedit is shown at the cursor without touching `text`; done replaces it.
    #[test]
    fn preedit_is_shown_not_committed() {
        let (mut rt, mut kb, _root, f) = field("TextField", "cursorPosition: 1; text: \"ab\"");
        assert!(kb.ime_preedit(&mut rt, Some(("にほ", 6, 6))));
        assert_eq!(preedit(f), Some(("にほ".to_string(), Some(2))));
        assert!(!kb.ime_preedit(&mut rt, Some(("にほ", 6, 6))));
        assert_eq!(get(&rt, f, "text"), "ab");
        assert!(kb.ime_preedit(&mut rt, None));
        assert_eq!(preedit(f), None);
    }
}
