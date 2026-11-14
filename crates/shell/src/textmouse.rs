//! Mouse editing of TextInput (QQuickTextInput) and TextEdit (QQuickTextControl): a press puts
//! the cursor at the nearest caret position, Shift+press extends the selection, a double-click
//! selects a word, a triple-click selects all (TextInput) or the paragraph (TextEdit) and a left
//! drag selects. Intervals (double-click, triple-click, press-and-hold) are measured between
//! the events' own timestamps (wl_pointer/wl_touch `time`, milliseconds), as Qt compares
//! `QInputEvent::timestamp`: a main loop that stalls between two dispatches neither joins two
//! slow clicks into a double-click nor splits a quick one.

use qml::runtime::{Runtime, World};
use qml::value::Value;

use crate::scene::{is, num, truthy};

bits::bit_flags! {
const SHIFT: u32 = 0x0200_0000;
}
/// QPlatformIntegration defaults: MouseDoubleClickInterval, MouseDoubleClickDistance and
/// StartDragDistance.
const DOUBLE_CLICK_MS: f64 = 400.0;
const DOUBLE_CLICK_DISTANCE: f64 = 5.0;
const DRAG_DISTANCE: f64 = 10.0;
/// QPlatformTheme::MousePressAndHoldInterval.
const PRESS_AND_HOLD_MS: f64 = 800.0;

/// A QTextCursor with a selection: (anchor, position).
type Cursor = (usize, usize);

/// Milliseconds from event time `then` to event time `now`; the protocol's 32-bit millisecond
/// clock wraps.
fn since(now: f64, then: f64) -> f64 {
    f64::from((now as u32).wrapping_sub(then as u32))
}

/// QQuickPressHandler: a Controls TextField/TextArea passes the press on (as a copy with a
/// rounded position and timestamp 0) only once the hold timer stops: on release, on a move
/// past startDragDistance or on a double-click; an expired press-and-hold drops it.
struct Held {
    item: usize,
    press_x: f64,
    /// Event time of the press.
    t0: f64,
    timer: bool,
    long: bool,
    delayed: Option<(f64, f64, u32)>,
}

#[derive(Default)]
pub struct TextMouse {
    /// QGuiApplicationPrivate's last press (time, window position, item) for MouseButtonDblClick.
    last_press: Option<(f64, (f64, f64), usize)>,
    /// QQuickTextInputPrivate::selectPressed / QQuickTextControlPrivate::mousePressed.
    select_pressed: bool,
    /// QQuickTextInputPrivate tripleClickTimer and tripleClickStartPoint.
    triple: Option<(usize, f64, (f64, f64))>,
    /// QQuickTextControlPrivate timestampAtLastDoubleClick (None: 0) and tripleClickPoint.
    doubles: std::collections::HashMap<usize, (Option<f64>, (f64, f64))>,
    /// QQuickPressHandler of a Controls TextField/TextArea.
    held: Option<Held>,
    /// QQuickTextControlPrivate selectedWordOnDoubleClick / selectedBlockOnTripleClick.
    word: Option<Cursor>,
    block: Option<Cursor>,
    /// The app's font stacks for hit tests.
    pub fonts: std::rc::Rc<std::cell::RefCell<crate::scene::Fonts>>,
}

fn text_doc(w: &World, i: usize) -> bool {
    crate::scene::is_text_doc(w, i)
}

/// A Controls TextField / TextArea: its QQuickPressHandler holds the press back.
fn controls(w: &World, i: usize) -> bool {
    is(w, i, "TextField") || is(w, i, "TextArea")
}

fn select_by_mouse(w: &mut World, i: usize) -> bool {
    truthy(w, i, "selectByMouse", true)
}

/// TextInput always takes the left press; TextEdit while its interaction flags allow selecting
/// or editing (QQuickTextControlPrivate::mousePressEvent).
pub fn takes_press(w: &mut World, i: usize) -> bool {
    crate::scene::is_text_edit(w, i)
        && (!text_doc(w, i) || select_by_mouse(w, i) || !truthy(w, i, "readOnly", false))
}

/// The editor's positions: a masked TextInput's m_text (`displayText`), else `text`.
fn text(w: &mut World, i: usize) -> Vec<char> {
    let shown = if w.input_mask(i).is_some() {
        "displayText"
    } else {
        "text"
    };
    w.read(i, shown)
        .as_ref()
        .and_then(Value::host_str)
        .map_or_else(Vec::new, |s| s.chars().collect())
}

/// (cursor, selectionStart, selectionEnd) as char indices into `text`, clamped to it.
fn state(w: &mut World, i: usize) -> (usize, usize, usize) {
    let cs = text(w, i);
    let mut n = |k: &str| {
        w.read(i, k)
            .map(|v| v.to_number())
            .filter(|v| v.is_finite())
            .map_or(0, |v| qml::textpos::to_char(&cs, v.max(0.0) as usize))
    };
    let (p, a, b) = (n("cursorPosition"), n("selectionStart"), n("selectionEnd"));
    (p, a.min(b), a.max(b))
}

/// The selection's anchor: the end the cursor is not at.
fn anchor(p: usize, s0: usize, s1: usize) -> usize {
    if s1 > s0 && p == s0 {
        s1
    } else if s1 > s0 && p == s1 {
        s0
    } else {
        p
    }
}

fn set(rt: &mut Runtime, i: usize, (a, p): Cursor) {
    let cs = text(&mut rt.world.borrow_mut(), i);
    crate::keys::finish(rt, i, &cs, false, p, (a.min(p), a.max(p)));
}

/// QQuickTextInputPrivate::moveCursor / QTextCursor::setPosition(pos, KeepAnchor if `mark`).
fn move_cursor(rt: &mut Runtime, i: usize, pos: usize, mark: bool) {
    let a = if mark {
        let mut w = rt.world.borrow_mut();
        let (p, s0, s1) = state(&mut w, i);
        anchor(p, s0, s1)
    } else {
        pos
    };
    set(rt, i, (a, pos));
}

fn cursor(rt: &Runtime, i: usize) -> Cursor {
    let mut w = rt.world.borrow_mut();
    let (p, s0, s1) = state(&mut w, i);
    (anchor(p, s0, s1), p)
}

/// Caret x of `pos` and the index of its line.
fn cursor_x(ls: &[(usize, f64, f64, Vec<f64>)], pos: usize) -> Option<(usize, f64)> {
    ls.iter()
        .enumerate()
        .rev()
        .find(|(_, l)| l.0 <= pos && pos - l.0 < l.3.len())
        .map(|(n, l)| (n, l.3[pos - l.0]))
}

/// QTextEngine::atWordSeparator.
fn separator(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | '?'
            | '!'
            | '@'
            | '#'
            | '$'
            | ':'
            | ';'
            | '-'
            | '<'
            | '>'
            | '['
            | ']'
            | '('
            | ')'
            | '{'
            | '}'
            | '='
            | '/'
            | '+'
            | '%'
            | '&'
            | '^'
            | '*'
            | '\''
            | '"'
            | '`'
            | '~'
            | '|'
            | '\\'
    )
}

/// QTextLayout::nextCursorPosition(SkipWords) within `s`.
pub(crate) fn next_word(s: &[char], mut p: usize) -> usize {
    let len = s.len();
    if p >= len {
        return p;
    }
    if separator(s[p]) {
        p += 1;
        while p < len && separator(s[p]) {
            p += 1;
        }
    } else {
        while p < len && !s[p].is_whitespace() && !separator(s[p]) {
            p += 1;
        }
    }
    while p < len && s[p].is_whitespace() {
        p += 1;
    }
    p
}

/// QTextLayout::previousCursorPosition(SkipWords) within `s`.
pub(crate) fn prev_word(s: &[char], mut p: usize) -> usize {
    if p == 0 || p > s.len() {
        return p;
    }
    while p > 0 && s[p - 1].is_whitespace() {
        p -= 1;
    }
    if p > 0 && separator(s[p - 1]) {
        p -= 1;
        while p > 0 && separator(s[p - 1]) {
            p -= 1;
        }
    } else {
        while p > 0 && !s[p - 1].is_whitespace() && !separator(s[p - 1]) {
            p -= 1;
        }
    }
    p
}

/// QTextDocument block (paragraph) around `p`: (start, length without the separator).
fn block(s: &[char], p: usize) -> (usize, usize) {
    let br = |c: &char| *c == '\n' || *c == '\u{2029}';
    let start = s[..p.min(s.len())]
        .iter()
        .rposition(br)
        .map_or(0, |k| k + 1);
    let end = s[start..]
        .iter()
        .position(br)
        .map_or(s.len(), |k| start + k);
    (start, end - start)
}

/// QTextCursor::movePosition(StartOfWord).
fn start_of_word(s: &[char], p: usize) -> Option<usize> {
    let (bs, bl) = block(s, p);
    let b = &s[bs..bs + bl];
    let mut rel = p - bs;
    if rel == 0 {
        return Some(p);
    }
    if rel == bl && (b[rel - 1].is_whitespace() || separator(b[rel - 1])) {
        return None;
    }
    if rel < bl {
        rel += 1;
    }
    Some(bs + prev_word(b, rel))
}

/// QTextCursor::movePosition(EndOfWord).
fn end_of_word(s: &[char], p: usize) -> Option<usize> {
    let (bs, bl) = block(s, p);
    let b = &s[bs..bs + bl];
    let mut rel = p - bs;
    if rel >= bl {
        return None;
    }
    if separator(b[rel]) {
        rel += 1;
        while rel < bl && separator(b[rel]) {
            rel += 1;
        }
    } else {
        while rel < bl && !b[rel].is_whitespace() && !separator(b[rel]) {
            rel += 1;
        }
    }
    Some(bs + rel)
}

/// QTextCursor::select(WordUnderCursor) from a cursor at `p`.
fn word_under(s: &[char], p: usize) -> Cursor {
    let a = start_of_word(s, p).unwrap_or(p);
    (a, end_of_word(s, a).unwrap_or(a))
}

/// QTextBoundaryFinder(Word) boundary: between characters of different kinds, and around
/// each punctuation character.
fn word_boundary(s: &[char], p: usize) -> bool {
    let kind = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    p == 0 || p >= s.len() || kind(s[p - 1]) != kind(s[p]) || kind(s[p]) == 2
}

impl TextMouse {
    /// Painted lines of the editor: (first char, top, height, caret x per position).
    fn lines(&self, rt: &Runtime, i: usize) -> Vec<(usize, f64, f64, Vec<f64>)> {
        crate::scene::edit_geometry(&mut rt.world.borrow_mut(), &mut self.fonts.borrow_mut(), i)
    }

    /// QQuickTextInputPrivate::positionAt / QQuickTextControl::hitTest(Qt::FuzzyHit).
    fn position_at(&self, rt: &Runtime, i: usize, x: f64, y: f64) -> usize {
        crate::scene::edit_position_at(
            &mut rt.world.borrow_mut(),
            &mut self.fonts.borrow_mut(),
            i,
            x,
            y,
        )
        .unwrap_or(0)
    }

    /// Middle press: QQuickTextInput::mousePressEvent moves the cursor for any mouse button;
    /// QQuickTextControlPrivate::mousePressEvent returns early for the middle one.
    pub fn middle_press(&self, rt: &mut Runtime, i: usize, x: f64, y: f64, mods: u32) {
        if text_doc(&rt.world.borrow(), i) {
            return;
        }
        let mark = mods & SHIFT != 0 && select_by_mouse(&mut rt.world.borrow_mut(), i);
        let pos = self.position_at(rt, i, x, y);
        move_cursor(rt, i, pos, mark);
    }

    /// Middle release: QQuickTextInput `deselect(); insert(Selection text)`; QQuickTextControl
    /// `setCursorPosition(pos)` then inserts the Selection data (only while editable).
    pub fn middle_release(&self, rt: &mut Runtime, i: usize, x: f64, y: f64) {
        if truthy(&mut rt.world.borrow_mut(), i, "readOnly", false) {
            return;
        }
        if text_doc(&rt.world.borrow(), i) {
            let pos = self.position_at(rt, i, x, y);
            set(rt, i, (pos, pos));
        }
        crate::keys::paste_selection(rt, i);
    }

    /// Left press on `i` at item coordinates (`x`, `y`) at event time `now`; `scene` is the
    /// window position.
    #[allow(clippy::too_many_arguments)]
    pub fn press(
        &mut self,
        rt: &mut Runtime,
        i: usize,
        x: f64,
        y: f64,
        scene: (f64, f64),
        mods: u32,
        now: f64,
    ) {
        let double = self.last_press.is_some_and(|(t, p, it)| {
            it == i
                && since(now, t) < DOUBLE_CLICK_MS
                && (scene.0 - p.0).abs() <= DOUBLE_CLICK_DISTANCE
                && (scene.1 - p.1).abs() <= DOUBLE_CLICK_DISTANCE
        });
        // A double-click's press does not start another one.
        self.last_press = (!double).then_some((now, scene, i));
        if controls(&rt.world.borrow(), i) {
            self.held = Some(Held {
                item: i,
                press_x: x,
                t0: now,
                timer: true,
                long: false,
                delayed: Some((x.round(), y.round(), mods)),
            });
            if double {
                self.flush(rt, i);
                if text_doc(&rt.world.borrow(), i) {
                    self.edit_double(rt, i, x, y, now);
                } else {
                    self.input_double(rt, i, x, y, now);
                }
            }
            return;
        }
        if text_doc(&rt.world.borrow(), i) {
            self.edit_press(rt, i, x, y, mods, now);
            if double {
                self.edit_double(rt, i, x, y, now);
            }
        } else {
            self.input_press(rt, i, x, y, mods, now);
            if double {
                self.input_double(rt, i, x, y, now);
            }
        }
    }

    /// Deliver the held press (QQuickPressHandler::delayedMousePressEvent).
    fn flush(&mut self, rt: &mut Runtime, i: usize) {
        let Some((x, y, mods, t0)) = self
            .held
            .as_mut()
            .and_then(|h| h.delayed.take().map(|(x, y, m)| (x, y, m, h.t0)))
        else {
            return;
        };
        if text_doc(&rt.world.borrow(), i) {
            self.edit_press(rt, i, x, y, mods, t0);
        } else {
            self.input_press(rt, i, x, y, mods, t0);
        }
    }

    /// QQuickPressHandler::isActive at event time `now`: its timer fired if the hold lasted
    /// MousePressAndHoldInterval.
    fn active(&mut self, rt: &mut Runtime, i: usize, now: f64) -> bool {
        let Some(h) = self.held.as_mut().filter(|h| h.item == i) else {
            return true;
        };
        if h.timer && since(now, h.t0) >= PRESS_AND_HOLD_MS {
            // QQuickPressHandler::timerEvent.
            h.timer = false;
            h.delayed = None;
            h.long = rt.has_handler(i, "onPressAndHold");
        }
        !(h.timer || h.long)
    }

    fn triple_pending(&self, i: usize, x: f64, y: f64, now: f64) -> Option<bool> {
        let (ti, t, p) = self.triple?;
        (ti == i && since(now, t) < DOUBLE_CLICK_MS)
            .then(|| (x - p.0).abs() + (y - p.1).abs() < DRAG_DISTANCE)
    }

    /// QQuickTextInput::mousePressEvent.
    fn input_press(&mut self, rt: &mut Runtime, i: usize, x: f64, y: f64, mods: u32, now: f64) {
        let sbm = select_by_mouse(&mut rt.world.borrow_mut(), i);
        if sbm {
            self.select_pressed = true;
            if self.triple_pending(i, x, y, now) == Some(true) {
                let len = text(&mut rt.world.borrow_mut(), i).len();
                set(rt, i, (0, len));
                return;
            }
        }
        let pos = self.position_at(rt, i, x, y);
        move_cursor(rt, i, pos, mods & SHIFT != 0 && sbm);
    }

    /// QQuickTextInput::mouseDoubleClickEvent: selectWordAtPos.
    fn input_double(&mut self, rt: &mut Runtime, i: usize, x: f64, y: f64, now: f64) {
        if !select_by_mouse(&mut rt.world.borrow_mut(), i) {
            return;
        }
        let pos = self.position_at(rt, i, x, y);
        let s = text(&mut rt.world.borrow_mut(), i);
        let mut next = pos + 1;
        if next > s.len() {
            next -= 1;
        }
        let c = prev_word(&s, next);
        move_cursor(rt, i, c, false);
        let mut end = next_word(&s, c);
        while end > pos && s[end - 1].is_whitespace() {
            end -= 1;
        }
        move_cursor(rt, i, end, true);
        if self.triple_pending(i, x, y, now).is_none() {
            self.triple = Some((i, now, (x, y)));
        }
    }

    /// QQuickTextControlPrivate::setCursorPosition(pos, MoveAnchor).
    fn edit_set_cursor(&mut self, rt: &mut Runtime, i: usize, pos: usize) {
        self.word = None;
        self.block = None;
        move_cursor(rt, i, pos, false);
    }

    /// QQuickTextControlPrivate::mousePressEvent. A held press (QQuickPressHandler) counts
    /// from when the button went down; Qt's copy has timestamp 0, which made a TextArea pick
    /// the paragraph however late the third click came.
    fn edit_press(&mut self, rt: &mut Runtime, i: usize, x: f64, y: f64, mods: u32, time: f64) {
        let sbm = select_by_mouse(&mut rt.world.borrow_mut(), i);
        self.select_pressed = sbm;
        // tripleClickPoint starts at the document origin.
        let (last, point) = match self.doubles.get(&i) {
            Some(&d) => d,
            None => (
                None,
                self.lines(rt, i)
                    .first()
                    .map_or((0.0, 0.0), |l| (l.3[0], l.1)),
            ),
        };
        // `e->timestamp() < timestampAtLastDoubleClick + mouseDoubleClickInterval`.
        let soon = last.is_some_and(|t| since(time, t) < DOUBLE_CLICK_MS);
        let triple =
            soon && (x - point.0).round().abs() + (y - point.1).round().abs() < DRAG_DISTANCE;
        if triple {
            let s = text(&mut rt.world.borrow_mut(), i);
            let (bs, bl) = block(&s, cursor(rt, i).1);
            let end = (bs + bl + 1).min(s.len());
            set(rt, i, (bs, end));
            self.block = Some((bs, end));
            self.doubles.insert(i, (None, point));
            return;
        }
        let pos = self.position_at(rt, i, x, y);
        if mods == SHIFT && sbm {
            let words = num(&mut rt.world.borrow_mut(), i, "mouseSelectionMode") == 1.0;
            if words && self.word.is_none_or(|w| w.0 == w.1) {
                let s = text(&mut rt.world.borrow_mut(), i);
                self.word = Some(word_under(&s, cursor(rt, i).1));
            }
            if self.block.is_some_and(|b| b.0 != b.1) {
                self.extend_block(rt, i, pos);
            } else if self.word.is_some_and(|w| w.0 != w.1) {
                self.extend_word(rt, i, pos, x, words);
            } else if !words {
                move_cursor(rt, i, pos, true);
            }
        } else {
            self.edit_set_cursor(rt, i, pos);
        }
    }

    /// QQuickTextControlPrivate::mouseDoubleClickEvent.
    fn edit_double(&mut self, rt: &mut Runtime, i: usize, x: f64, y: f64, now: f64) {
        if !select_by_mouse(&mut rt.world.borrow_mut(), i) {
            return;
        }
        let pos = self.position_at(rt, i, x, y);
        self.edit_set_cursor(rt, i, pos);
        let ls = self.lines(rt, i);
        let nonempty = cursor_x(&ls, pos).is_some_and(|(n, _)| ls[n].3.len() > 1);
        if nonempty {
            let s = text(&mut rt.world.borrow_mut(), i);
            set(rt, i, word_under(&s, pos));
        }
        self.word = Some(cursor(rt, i));
        self.doubles.insert(i, (Some(now), (x, y)));
    }

    /// QQuickTextControlPrivate::extendWordwiseSelection.
    fn extend_word(&mut self, rt: &mut Runtime, i: usize, pos: usize, mx: f64, words: bool) {
        let Some(word) = self.word else { return };
        let (w0, w1) = (word.0.min(word.1), word.0.max(word.1));
        if pos >= w0 && pos <= w1 {
            set(rt, i, word);
            return;
        }
        let s = text(&mut rt.world.borrow_mut(), i);
        let Some(start) = start_of_word(&s, pos) else {
            return;
        };
        let ls = self.lines(rt, i);
        let Some((line, sx)) = cursor_x(&ls, start) else {
            return;
        };
        let Some(end) = end_of_word(&s, start) else {
            return;
        };
        let Some((eline, ex)) = cursor_x(&ls, end) else {
            return;
        };
        if eline != line || end == start {
            return;
        }
        if !words && (mx < sx || mx > ex) {
            return;
        }
        if pos < word.1 {
            set(rt, i, (w1, start));
        } else {
            set(rt, i, (w0, end));
        }
    }

    /// QQuickTextControlPrivate::extendBlockwiseSelection.
    fn extend_block(&mut self, rt: &mut Runtime, i: usize, pos: usize) {
        let Some(b) = self.block else { return };
        let (b0, b1) = (b.0.min(b.1), b.0.max(b.1));
        if pos >= b0 && pos <= b1 {
            set(rt, i, b);
            return;
        }
        let s = text(&mut rt.world.borrow_mut(), i);
        let (bs, bl) = block(&s, pos);
        if pos < b.1 {
            set(rt, i, (b1, bs));
        } else {
            set(rt, i, (b0, (bs + bl + 1).min(s.len())));
        }
    }

    /// Left-button drag of the grabbing editor `i` (QQuickTextInput / QQuickTextControl
    /// mouseMoveEvent).
    /// QQuickTextInputPrivate::selectPressed: a left press is selecting.
    pub fn selecting(&self) -> bool {
        self.select_pressed
    }

    pub fn drag(&mut self, rt: &mut Runtime, i: usize, x: f64, y: f64, now: f64) {
        // QQuickPressHandler::mouseMoveEvent.
        if let Some(h) = self.held.as_mut().filter(|h| h.item == i)
            && (x - h.press_x).abs() > DRAG_DISTANCE
        {
            h.timer = false;
        }
        if !self.active(rt, i, now) {
            return;
        }
        self.flush(rt, i);
        if !self.select_pressed {
            return;
        }
        let pos = self.position_at(rt, i, x, y);
        let words = num(&mut rt.world.borrow_mut(), i, "mouseSelectionMode") == 1.0;
        if !text_doc(&rt.world.borrow(), i) {
            if words {
                self.select_words(rt, i, pos);
            } else {
                move_cursor(rt, i, pos, true);
            }
            return;
        }
        if words && self.word.is_none_or(|w| w.0 == w.1) {
            let s = text(&mut rt.world.borrow_mut(), i);
            self.word = Some(word_under(&s, cursor(rt, i).1));
        }
        if self.block.is_some_and(|b| b.0 != b.1) {
            self.extend_block(rt, i, pos);
        } else if self.word.is_some_and(|w| w.0 != w.1) {
            self.extend_word(rt, i, pos, x, words);
        } else {
            move_cursor(rt, i, pos, true);
        }
    }

    /// QQuickTextInput::moveCursorSelection(pos, SelectWords).
    fn select_words(&mut self, rt: &mut Runtime, i: usize, pos: usize) {
        let s = text(&mut rt.world.borrow_mut(), i);
        let (cur, s0, s1) = state(&mut rt.world.borrow_mut(), i);
        if pos == cur {
            return;
        }
        let a = if s0 == s1 {
            cur
        } else if s0 == cur {
            s1
        } else {
            s0
        };
        let prev = |mut p: usize| {
            while p > 0 && !word_boundary(&s, p) {
                p -= 1;
            }
            p
        };
        let next = |mut p: usize| {
            while p < s.len() && !word_boundary(&s, p) {
                p += 1;
            }
            p
        };
        if a < pos || (a == pos && cur < pos) {
            let a = if a < s.len() && !word_boundary(&s, a) {
                prev(a)
            } else {
                a
            };
            set(rt, i, (a, next(pos)));
        } else if a > pos || (a == pos && cur > pos) {
            let a = if a > 0 && !word_boundary(&s, a) {
                next(a)
            } else {
                a
            };
            set(rt, i, (a, prev(pos)));
        }
    }

    /// Left release at event time `now`: selectPressed / mousePressed end.
    pub fn release(&mut self, rt: &mut Runtime, i: usize, now: f64) {
        let active = self.active(rt, i, now);
        if let Some(h) = self.held.as_mut().filter(|h| !h.long) {
            h.timer = false;
        }
        if active || self.active(rt, i, now) {
            self.flush(rt, i);
        }
        self.held = None;
        self.select_pressed = false;
    }

    /// The grab went elsewhere (mouseUngrabEvent): the held press is dropped.
    pub fn ungrab(&mut self) {
        self.held = None;
        self.select_pressed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Pointer;

    const LEFT: u32 = 0x110;

    fn load(src: &str) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(src).unwrap();
        rt.settle();
        (rt, root)
    }

    fn get(rt: &Runtime, i: usize, n: &str) -> String {
        match rt.world.borrow_mut().read(i, n) {
            Some(Value::Str(s)) => s.to_string(),
            Some(v) => v.to_number().to_string(),
            None => String::new(),
        }
    }

    /// (cursorPosition, selectionStart, selectionEnd, selectedText).
    fn sel(rt: &Runtime, i: usize) -> (String, String, String, String) {
        (
            get(rt, i, "cursorPosition"),
            get(rt, i, "selectionStart"),
            get(rt, i, "selectionEnd"),
            get(rt, i, "selectedText"),
        )
    }

    fn s(p: usize, a: usize, b: usize, t: &str) -> (String, String, String, String) {
        (p.to_string(), a.to_string(), b.to_string(), t.to_string())
    }

    /// Scene point of caret position `c` of the editor `i` (placed at `origin`).
    fn at(rt: &Runtime, i: usize, origin: (f64, f64), c: usize, dx: f64) -> (f64, f64) {
        let ls = crate::scene::edit_geometry(
            &mut rt.world.borrow_mut(),
            &mut crate::scene::Fonts::default(),
            i,
        );
        let l = ls.iter().rev().find(|l| l.0 <= c).unwrap();
        (origin.0 + l.3[c - l.0] + dx, origin.1 + l.1 + l.2 / 2.0)
    }

    fn click(rt: &mut Runtime, p: &mut Pointer, root: usize, (x, y): (f64, f64)) {
        p.motion(rt, root, x, y);
        p.press(rt, LEFT);
        p.release(rt, LEFT);
    }

    const INPUT: &str = "import QtQuick\nItem { id: root; width: 400; height: 200; property int moves: 0\n \
        TextInput { x: 10; y: 10; width: 300; height: 30; font.pixelSize: 16; \
        text: \"hello world, foo\"; cursorPosition: 16; \
        onCursorPositionChanged: root.moves++ }\n \
        TextInput { x: 10; y: 60; width: 300; height: 30; font.pixelSize: 16; \
        selectByMouse: false; text: \"plain input\"; cursorPosition: 11 } }\n";

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn middle_click_moves_the_cursor_then_pastes_the_primary_like_qt() {
        const MIDDLE: u32 = 0x112;
        crate::keys::FAKE_PRIMARY.with(|c| *c.borrow_mut() = Some("X".into()));
        let (mut rt, root) = load(INPUT);
        let field = rt.world.borrow().items[root].children[0];
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        let pt = at(&rt, field, (10.0, 10.0), 5, 0.0);
        p.motion(&mut rt, root, pt.0, pt.1);
        p.press(&mut rt, MIDDLE);
        assert_eq!(sel(&rt, field), s(5, 5, 5, ""));
        p.release(&mut rt, MIDDLE);
        assert_eq!(get(&rt, field, "text"), "helloX world, foo");
        assert_eq!(sel(&rt, field), s(6, 6, 6, ""));

        let (mut rt, root) = load(
            "import QtQuick\nItem { width: 400; height: 200\n \
             TextEdit { x: 10; y: 10; width: 300; height: 100; font.pixelSize: 16; \
             text: \"ab\\ncd\"; cursorPosition: 0 }\n \
             TextInput { x: 10; y: 150; width: 300; height: 30; font.pixelSize: 16; \
             readOnly: true; text: \"ro\"; cursorPosition: 0 } }\n",
        );
        let (edit, ro) = {
            let w = rt.world.borrow();
            (w.items[root].children[0], w.items[root].children[1])
        };
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        let pt = at(&rt, edit, (10.0, 10.0), 4, 0.0);
        p.motion(&mut rt, root, pt.0, pt.1);
        // TextEdit: the press leaves the cursor, the release places it and pastes.
        p.press(&mut rt, MIDDLE);
        assert_eq!(get(&rt, edit, "cursorPosition"), "0");
        p.release(&mut rt, MIDDLE);
        assert_eq!(get(&rt, edit, "text"), "ab\ncXd");
        assert_eq!(get(&rt, edit, "cursorPosition"), "5");
        // A read-only TextInput moves its cursor but pastes nothing.
        let pt = at(&rt, ro, (10.0, 150.0), 1, 0.0);
        p.motion(&mut rt, root, pt.0, pt.1);
        p.press(&mut rt, MIDDLE);
        p.release(&mut rt, MIDDLE);
        assert_eq!(get(&rt, ro, "text"), "ro");
        assert_eq!(get(&rt, ro, "cursorPosition"), "1");
        crate::keys::FAKE_PRIMARY.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn click_places_the_cursor_at_the_nearest_character_like_qt() {
        let (mut rt, root) = load(INPUT);
        let field = rt.world.borrow().items[root].children[0];
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        // Just right of the boundary between "hel" and "lo": the caret goes between them.
        let pt = at(&rt, field, (10.0, 10.0), 3, 1.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(3, 3, 3, ""));
        assert_eq!(get(&rt, root, "moves"), "1");
        // Just left of the boundary after "hello w".
        let pt = at(&rt, field, (10.0, 10.0), 7, -1.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(7, 7, 7, ""));
        // Past the end and before the start.
        click(&mut rt, &mut p, root, (300.0, 25.0));
        assert_eq!(sel(&rt, field), s(16, 16, 16, ""));
        click(&mut rt, &mut p, root, (10.5, 25.0));
        assert_eq!(sel(&rt, field), s(0, 0, 0, ""));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn masked_click_and_select_use_the_display_text_like_qt() {
        let (mut rt, root) = load(
            "import QtQuick\nItem { width: 400; height: 100\n \
             TextInput { x: 10; y: 10; width: 300; height: 30; font.pixelSize: 16; \
             inputMask: \"99-99;_\"; text: \"12\" } }\n",
        );
        let field = rt.world.borrow().items[root].children[0];
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        // QQuickTextInput positions index m_text ("12-__"), not the stripped `text`.
        click(&mut rt, &mut p, root, (300.0, 25.0));
        assert_eq!(sel(&rt, field), s(5, 5, 5, ""));
        assert_eq!(get(&rt, field, "displayText"), "12-__");
        let pt = at(&rt, field, (10.0, 10.0), 1, 0.0);
        p.motion(&mut rt, root, pt.0, pt.1);
        p.press(&mut rt, LEFT);
        let pt = at(&rt, field, (10.0, 10.0), 4, 0.0);
        p.motion(&mut rt, root, pt.0, pt.1);
        p.release(&mut rt, LEFT);
        assert_eq!(sel(&rt, field), s(4, 1, 4, "2-_"));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn shift_click_double_click_triple_click_and_drag_select_like_qt() {
        let (mut rt, root) = load(INPUT);
        let field = rt.world.borrow().items[root].children[0];
        let o = (10.0, 10.0);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        let pt = at(&rt, field, o, 2, 0.0);
        click(&mut rt, &mut p, root, pt);
        p.mods = SHIFT;
        let pt = at(&rt, field, o, 8, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(8, 2, 8, "llo wo"));
        // Shift+click before the anchor flips the selection around it.
        let pt = at(&rt, field, o, 1, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(1, 1, 2, "e"));
        p.mods = 0;

        // Double-click in "world": the word without the "," (selectWordAtPos).
        let w = at(&rt, field, o, 8, 0.0);
        click(&mut rt, &mut p, root, w);
        click(&mut rt, &mut p, root, w);
        assert_eq!(sel(&rt, field), s(11, 6, 11, "world"));
        // A wl_pointer.motion that keeps the position is no drag (Qt filters it out).
        p.motion(&mut rt, root, w.0, w.1);
        p.press(&mut rt, LEFT);
        p.motion(&mut rt, root, w.0, w.1);
        assert_eq!(sel(&rt, field), s(16, 0, 16, "hello world, foo"));
        p.release(&mut rt, LEFT);
        // Drag from "e" to the middle of "world".
        let a = at(&rt, field, o, 1, 0.0);
        p.motion(&mut rt, root, a.0 + 40.0, a.1);
        p.motion(&mut rt, root, a.0, a.1);
        p.time += 450.0;
        p.press(&mut rt, LEFT);
        assert_eq!(sel(&rt, field), s(1, 1, 1, ""));
        let b = at(&rt, field, o, 9, 0.0);
        p.motion(&mut rt, root, b.0, b.1);
        assert_eq!(sel(&rt, field), s(9, 1, 9, "ello wor"));
        // Dragging outside the field keeps selecting (the field holds the grab).
        p.motion(&mut rt, root, 390.0, 150.0);
        assert_eq!(sel(&rt, field), s(16, 1, 16, "ello world, foo"));
        p.release(&mut rt, LEFT);
        p.motion(&mut rt, root, b.0, b.1);
        assert_eq!(sel(&rt, field), s(16, 1, 16, "ello world, foo"));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn select_by_mouse_false_only_moves_the_cursor() {
        let (mut rt, root) = load(INPUT);
        let field = rt.world.borrow().items[root].children[1];
        let o = (10.0, 60.0);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        let pt = at(&rt, field, o, 2, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(2, 2, 2, ""));
        p.mods = SHIFT;
        let pt = at(&rt, field, o, 7, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(7, 7, 7, ""));
        p.mods = 0;
        let w = at(&rt, field, o, 3, 0.0);
        click(&mut rt, &mut p, root, w);
        click(&mut rt, &mut p, root, w);
        assert_eq!(sel(&rt, field), s(3, 3, 3, ""));
        let a = at(&rt, field, o, 9, 0.0);
        p.motion(&mut rt, root, a.0, a.1);
        p.press(&mut rt, LEFT);
        p.motion(&mut rt, root, w.0, w.1);
        p.release(&mut rt, LEFT);
        assert_eq!(sel(&rt, field), s(9, 9, 9, ""));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn text_edit_and_text_area_click_double_triple_like_qt() {
        let (mut rt, root) = load(
            "import QtQuick\nimport QtQuick.Controls\nItem { width: 400; height: 300\n \
             TextEdit { x: 10; y: 10; width: 300; height: 120; font.pixelSize: 16; \
             text: \"first line\\nsecond (word) here\\nthird\"; cursorPosition: 0 }\n \
             TextArea { x: 10; y: 150; width: 300; height: 120; font.pixelSize: 16; \
             text: \"alpha beta\\ngamma\" } }\n",
        );
        let (edit, area) = {
            let w = rt.world.borrow();
            (w.items[root].children[0], w.items[root].children[1])
        };
        let o = (10.0, 10.0);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        // "second (word) here" starts at 11; caret between "sec" and "ond".
        let pt = at(&rt, edit, o, 14, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, edit), s(14, 14, 14, ""));
        p.mods = SHIFT;
        let pt = at(&rt, edit, o, 33, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, edit), s(33, 14, 33, "ond (word) here\nthi"));
        p.mods = 0;
        // Double-click on "word" selects it (WordUnderCursor stops at the parenthesis).
        let w = at(&rt, edit, o, 20, 0.0);
        click(&mut rt, &mut p, root, w);
        click(&mut rt, &mut p, root, w);
        assert_eq!(sel(&rt, edit), s(23, 19, 23, "word"));
        // Triple-click selects the paragraph with its separator.
        click(&mut rt, &mut p, root, w);
        assert_eq!(sel(&rt, edit), s(30, 11, 30, "second (word) here\n"));

        let o = (10.0, 150.0);
        p.time += 450.0;
        let a = at(&rt, area, o, 2, 0.0);
        p.motion(&mut rt, root, a.0, a.1);
        p.press(&mut rt, LEFT);
        let b = at(&rt, area, o, 16, 0.0);
        p.motion(&mut rt, root, b.0, b.1);
        p.release(&mut rt, LEFT);
        assert_eq!(sel(&rt, area), s(16, 2, 16, "pha beta\ngamma"));
    }

    /// QTextDocumentLayout::hitTest: a TextEdit's document has no line outside its text. A
    /// point above it is the start of the text, one at its bottom edge or under it the end,
    /// for a click and for a selecting drag.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn point_outside_the_lines_of_a_text_edit_is_its_start_or_end_like_qt() {
        let (mut rt, root) = load(
            "import QtQuick\nItem { width: 400; height: 300\n \
             TextEdit { x: 10; y: 10; width: 300; height: 160; topPadding: 20; \
             font.pixelSize: 16; text: \"first line\\nsecond\"; cursorPosition: 3 } }\n",
        );
        let edit = rt.world.borrow().items[root].children[0];
        let o = (10.0, 10.0);
        let (top, bottom) = {
            let ls = crate::scene::edit_geometry(
                &mut rt.world.borrow_mut(),
                &mut crate::scene::Fonts::default(),
                edit,
            );
            let last = ls.last().unwrap();
            (o.1 + ls[0].1, o.1 + last.1 + last.2)
        };
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        // The x of "se|cond".
        let x = at(&rt, edit, o, 13, 0.0).0;
        click(&mut rt, &mut p, root, (x, top - 2.0));
        assert_eq!(sel(&rt, edit), s(0, 0, 0, ""));
        p.time += 450.0;
        click(&mut rt, &mut p, root, (x, bottom));
        assert_eq!(sel(&rt, edit), s(17, 17, 17, ""));
        p.time += 450.0;
        click(&mut rt, &mut p, root, (x, bottom - 2.0));
        assert_eq!(sel(&rt, edit), s(13, 13, 13, ""));
        p.time += 450.0;
        click(&mut rt, &mut p, root, (x, bottom + 30.0));
        assert_eq!(sel(&rt, edit), s(17, 17, 17, ""));
        // A drag from "firs|t" to over the text selects back to its start.
        p.time += 450.0;
        let a = at(&rt, edit, o, 4, 0.0);
        p.motion(&mut rt, root, a.0, a.1);
        p.press(&mut rt, LEFT);
        p.motion(&mut rt, root, a.0 + 40.0, top - 5.0);
        p.release(&mut rt, LEFT);
        assert_eq!(sel(&rt, edit), s(0, 0, 4, "firs"));
    }

    const CONTROLS: &str = "import QtQuick\nimport QtQuick.Controls\nItem { width: 400; height: 300\n \
        TextField { x: 10; y: 10; width: 300; height: 40; font.pixelSize: 16; \
        text: \"field text here\" }\n \
        TextArea { x: 10; y: 60; width: 300; height: 120; font.pixelSize: 16; \
        text: \"alpha beta\\ngamma\" }\n \
        TextEdit { x: 10; y: 200; width: 300; height: 80; font.pixelSize: 16; \
        text: \"alpha beta\\ngamma\" } }\n";

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn controls_hold_the_press_like_qquickpresshandler() {
        let (mut rt, root) = load(CONTROLS);
        let field = rt.world.borrow().items[root].children[0];
        let o = (10.0, 10.0);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        let pt = at(&rt, field, o, 3, 0.0);
        click(&mut rt, &mut p, root, pt);
        assert_eq!(sel(&rt, field), s(3, 3, 3, ""));
        // A drag that stays within startDragDistance only places the cursor on release.
        p.time += 450.0;
        let a = at(&rt, field, o, 6, 0.0);
        p.motion(&mut rt, root, a.0, a.1);
        p.press(&mut rt, LEFT);
        assert_eq!(sel(&rt, field), s(3, 3, 3, ""));
        p.motion(&mut rt, root, a.0 + 8.0, a.1);
        assert_eq!(sel(&rt, field), s(3, 3, 3, ""));
        p.release(&mut rt, LEFT);
        assert_eq!(sel(&rt, field), s(6, 6, 6, ""));
        // Past it the held press is delivered, then the move selects.
        p.time += 450.0;
        let a = at(&rt, field, o, 1, 0.0);
        let b = at(&rt, field, o, 10, 0.0);
        p.motion(&mut rt, root, a.0, a.1);
        p.press(&mut rt, LEFT);
        p.motion(&mut rt, root, b.0, b.1);
        assert_eq!(sel(&rt, field), s(10, 1, 10, "ield text"));
        p.release(&mut rt, LEFT);
        // An expired press-and-hold drops the press: the cursor stays.
        let c = at(&rt, field, o, 13, 0.0);
        p.motion(&mut rt, root, c.0, c.1);
        p.press(&mut rt, LEFT);
        p.time += 850.0;
        p.release(&mut rt, LEFT);
        assert_eq!(sel(&rt, field), s(10, 1, 10, "ield text"));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn late_third_click_places_the_caret_in_text_area_and_text_edit() {
        let (mut rt, root) = load(CONTROLS);
        let (area, edit) = {
            let w = rt.world.borrow();
            (w.items[root].children[1], w.items[root].children[2])
        };
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        for (i, o) in [(area, (10.0, 60.0)), (edit, (10.0, 200.0))] {
            let w = at(&rt, i, o, 2, 0.0);
            click(&mut rt, &mut p, root, w);
            click(&mut rt, &mut p, root, w);
            assert_eq!(sel(&rt, i), s(5, 0, 5, "alpha"));
            // Qt's TextArea compares its held press copy (timestamp 0) with the double-click
            // and selects the paragraph however late the third click comes; a late click is a
            // new single click in both.
            p.time += 500.0;
            click(&mut rt, &mut p, root, w);
            assert_eq!(sel(&rt, i), s(2, 2, 2, ""));
            // A quick third click still selects the paragraph.
            p.time += 450.0;
            click(&mut rt, &mut p, root, w);
            click(&mut rt, &mut p, root, w);
            click(&mut rt, &mut p, root, w);
            assert_eq!(sel(&rt, i), s(11, 0, 11, "alpha beta\n"));
        }
    }

    /// Intervals are measured between the events' timestamps, not between their dispatches:
    /// clicks 1 s apart that a stalled loop dispatches back to back are two single clicks,
    /// clicks 300 ms apart are a double-click also across the 32-bit wrap of the clock, and a
    /// press held 900 ms drops the held press however quickly the release is dispatched.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn click_intervals_use_event_time_not_dispatch_time() {
        let (mut rt, root) = load(INPUT);
        let field = rt.world.borrow().items[root].children[0];
        let o = (10.0, 10.0);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        let w = at(&rt, field, o, 8, 0.0);
        p.time = 1000.0;
        click(&mut rt, &mut p, root, w);
        p.time = 2000.0;
        click(&mut rt, &mut p, root, w);
        assert_eq!(
            sel(&rt, field),
            s(8, 8, 8, ""),
            "slow clicks made a double-click"
        );
        p.time = 2300.0;
        click(&mut rt, &mut p, root, w);
        assert_eq!(sel(&rt, field), s(11, 6, 11, "world"));
        let h = at(&rt, field, o, 2, 0.0);
        p.time = f64::from(u32::MAX - 100);
        click(&mut rt, &mut p, root, h);
        p.time = 50.0;
        click(&mut rt, &mut p, root, h);
        assert_eq!(sel(&rt, field), s(5, 0, 5, "hello"));

        let (mut rt, root) = load(CONTROLS);
        let field = rt.world.borrow().items[root].children[0];
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 0.0, 0.0);
        p.time = 10_000.0;
        let a = at(&rt, field, o, 3, 0.0);
        click(&mut rt, &mut p, root, a);
        assert_eq!(sel(&rt, field), s(3, 3, 3, ""));
        let c = at(&rt, field, o, 13, 0.0);
        p.motion(&mut rt, root, c.0, c.1);
        p.time = 20_000.0;
        p.press(&mut rt, LEFT);
        p.time = 20_900.0;
        p.release(&mut rt, LEFT);
        assert_eq!(
            sel(&rt, field),
            s(3, 3, 3, ""),
            "an expired press-and-hold moved the cursor"
        );
    }
}
