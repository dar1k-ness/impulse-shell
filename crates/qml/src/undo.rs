//! Undo / redo history of the text editors, as Qt keeps it: QQuickTextInputPrivate's command
//! list for TextInput / TextField and QTextDocumentPrivate's undo stack for TextEdit / TextArea.
//! Texts and positions are QString's: UTF-16 code units.

/// The caret and selection an edit started from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Caret {
    pub cursor: usize,
    pub sel: (usize, usize),
}

/// The text, caret and selection an undo or redo leaves.
pub type Replay = (Vec<u16>, usize, (usize, usize));

/// The part of `old` that `new` replaces: (position, removed length, inserted length). Common
/// text after the new caret counts first, so a repeated character resolves to the caret's side.
fn diff(old: &[u16], new: &[u16], cursor: usize) -> (usize, usize, usize) {
    let short = old.len().min(new.len());
    let after = new.len().saturating_sub(cursor).min(short);
    let tail = (0..after)
        .take_while(|&k| old[old.len() - 1 - k] == new[new.len() - 1 - k])
        .count();
    let head = (0..short - tail).take_while(|&k| old[k] == new[k]).count();
    (head, old.len() - tail - head, new.len() - tail - head)
}

/// One cursor step: a code unit, or both halves of a surrogate pair.
fn one_char(t: &[u16]) -> bool {
    match t {
        [_] => true,
        [a, b] => (0xd800..0xdc00).contains(a) && (0xdc00..0xe000).contains(b),
        _ => false,
    }
}

/// QQuickTextInputPrivate::CommandType, in its order: the undo and redo loops compare them.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
enum Kind {
    Separator,
    Insert,
    Remove,
    Delete,
    RemoveSelection,
    DeleteSelection,
    SetSelection,
}

/// QQuickTextInputPrivate::Command.
#[derive(Clone, Copy, Debug)]
struct Cmd {
    kind: Kind,
    pos: usize,
    uc: u16,
    sel: (usize, usize),
}

/// QQuickTextInputPrivate's m_history / m_undoState / m_separator.
#[derive(Debug, Default)]
pub struct Line {
    cmds: Vec<Cmd>,
    state: usize,
    separator: bool,
    /// The text the history leads to; an edit from another text starts the history over.
    text: Vec<u16>,
}

impl Line {
    /// QQuickTextInputPrivate::separate.
    pub fn separate(&mut self) {
        self.separator = true;
    }

    /// QQuickTextInputPrivate::addCommand; `at` is the caret and selection of the moment.
    fn add(&mut self, at: Caret, kind: Kind, pos: usize, uc: u16, sel: (usize, usize)) {
        self.cmds.truncate(self.state);
        if self.separator && self.cmds.last().is_some_and(|c| c.kind != Kind::Separator) {
            self.cmds.push(Cmd {
                kind: Kind::Separator,
                pos: at.cursor,
                uc: 0,
                sel: at.sel,
            });
        }
        self.separator = false;
        self.cmds.push(Cmd { kind, pos, uc, sel });
        self.state = self.cmds.len();
    }

    /// The edit `old` -> `new` made from caret `from`, leaving the caret at `cursor`, as the
    /// commands QQuickTextInputPrivate's backspace / del / insert / paste / delete*OfWord add.
    pub fn record(&mut self, old: &[u16], new: &[u16], from: Caret, cursor: usize) {
        if self.text != old {
            *self = Line::default();
        }
        self.text = new.to_vec();
        let (p, gone, came) = diff(old, new, cursor);
        let removed = &old[p..p + gone];
        let has = from.sel.0 < from.sel.1;
        // Without a selection the ends are both 0 (internalDeselect).
        let mut at = Caret {
            cursor: from.cursor,
            sel: if has { from.sel } else { (0, 0) },
        };
        // QQuickTextInputPrivate::paste separates the insertion on both sides.
        let paste = String::from_utf16_lossy(&new[p..p + came]).chars().count() > 1;
        if paste {
            self.separate();
        }
        if gone > 0 && !has && one_char(removed) && p + gone == from.cursor {
            // backspace(): internalDelete(true) at each unit before the caret.
            for k in (p..p + gone).rev() {
                at.cursor = k;
                self.add(at, Kind::Remove, k, old[k], (0, 0));
            }
        } else if gone > 0 && !has && one_char(removed) && p == from.cursor {
            // del(): internalDelete() at the caret, once per unit.
            for &uc in removed {
                self.add(at, Kind::Delete, p, uc, (0, 0));
            }
        } else if gone > 0 {
            let before = Cmd {
                kind: Kind::SetSelection,
                pos: at.cursor,
                uc: 0,
                sel: at.sel,
            };
            self.separate();
            if !(has && from.sel == (p, p + gone)) {
                // deleteStartOfWord / deleteEndOfWord / deleteEndOfLine: select from the caret
                // to the far end, the caret moving there.
                at.cursor = if from.cursor == p + gone { p } else { p + gone };
                at.sel = (p, p + gone);
            }
            self.add(at, before.kind, before.pos, 0, before.sel);
            // removeSelectedText.
            let (s, e, c) = (p, p + gone, at.cursor);
            if s <= c && c < e {
                for k in (s..=c).rev() {
                    self.add(at, Kind::DeleteSelection, k, old[k], (0, 0));
                }
                for k in (c + 1..e).rev() {
                    self.add(at, Kind::DeleteSelection, k - c + s - 1, old[k], (0, 0));
                }
            } else {
                for k in (s..e).rev() {
                    self.add(at, Kind::RemoveSelection, k, old[k], (0, 0));
                }
            }
            at = Caret {
                cursor: p,
                sel: (0, 0),
            };
        }
        // internalInsert.
        for k in 0..came {
            at.cursor = p + k;
            self.add(at, Kind::Insert, p + k, new[p + k], (0, 0));
        }
        if paste {
            self.separate();
        }
    }

    pub fn can_undo(&self) -> bool {
        self.state > 0
    }

    pub fn can_redo(&self) -> bool {
        self.state < self.cmds.len()
    }

    /// QQuickTextInputPrivate::internalUndo from caret `cursor`.
    pub fn undo(&mut self, cursor: usize) -> Option<Replay> {
        if !self.can_undo() {
            return None;
        }
        let (mut cursor, mut sel) = (cursor, (0, 0));
        let fit = |p: usize, t: &Vec<u16>| p.min(t.len());
        while self.state > 0 {
            self.state -= 1;
            let cmd = self.cmds[self.state];
            match cmd.kind {
                Kind::Insert => {
                    if cmd.pos < self.text.len() {
                        self.text.remove(cmd.pos);
                    }
                    cursor = cmd.pos;
                }
                Kind::SetSelection => {
                    sel = cmd.sel;
                    cursor = cmd.pos;
                }
                Kind::Remove | Kind::RemoveSelection => {
                    let p = fit(cmd.pos, &self.text);
                    self.text.insert(p, cmd.uc);
                    cursor = cmd.pos + 1;
                }
                Kind::Delete | Kind::DeleteSelection => {
                    let p = fit(cmd.pos, &self.text);
                    self.text.insert(p, cmd.uc);
                    cursor = cmd.pos;
                }
                Kind::Separator => continue,
            }
            if self.state > 0 {
                let next = self.cmds[self.state - 1].kind;
                if next != cmd.kind
                    && next < Kind::RemoveSelection
                    && (cmd.kind < Kind::RemoveSelection || next == Kind::Separator)
                {
                    break;
                }
            }
        }
        self.separate();
        Some((self.text.clone(), cursor.min(self.text.len()), sel))
    }

    /// QQuickTextInputPrivate::internalRedo from caret `cursor`.
    pub fn redo(&mut self, cursor: usize) -> Option<Replay> {
        if !self.can_redo() {
            return None;
        }
        let (mut cursor, mut sel) = (cursor, (0, 0));
        while self.state < self.cmds.len() {
            let cmd = self.cmds[self.state];
            self.state += 1;
            match cmd.kind {
                Kind::Insert => {
                    let p = cmd.pos.min(self.text.len());
                    self.text.insert(p, cmd.uc);
                    cursor = cmd.pos + 1;
                }
                Kind::SetSelection | Kind::Separator => {
                    sel = cmd.sel;
                    cursor = cmd.pos;
                }
                Kind::Remove | Kind::Delete | Kind::RemoveSelection | Kind::DeleteSelection => {
                    if cmd.pos < self.text.len() {
                        self.text.remove(cmd.pos);
                    }
                    sel = cmd.sel;
                    cursor = cmd.pos;
                }
            }
            if let Some(next) = self.cmds.get(self.state).map(|c| c.kind)
                && next != cmd.kind
                && cmd.kind < Kind::RemoveSelection
                && next != Kind::Separator
                && (next < Kind::RemoveSelection || cmd.kind == Kind::Separator)
            {
                break;
            }
        }
        Some((self.text.clone(), cursor.min(self.text.len()), sel))
    }
}

/// QTextUndoCommand::Command, the four a plain-text editor produces.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Inserted,
    Removed,
    BlockInserted,
    BlockRemoved,
}

/// QTextUndoCommand: `len` units at `pos`, stored at `at` of the document's text buffer.
#[derive(Clone, Copy, Debug)]
struct Step {
    op: Op,
    pos: usize,
    at: usize,
    len: usize,
    block_part: bool,
    block_end: bool,
}

impl Step {
    /// QTextUndoCommand::tryMerge.
    fn merge(&mut self, o: &Step) -> bool {
        if self.op != o.op {
            return false;
        }
        match self.op {
            Op::Inserted if self.pos + self.len == o.pos && self.at + self.len == o.at => {
                self.len += o.len;
                true
            }
            // Removal to the right (Delete).
            Op::Removed if self.pos == o.pos && self.at + self.len == o.at => {
                self.len += o.len;
                true
            }
            // Removal to the left (Backspace).
            Op::Removed if o.pos + o.len == self.pos && o.at + o.len == self.at => {
                self.pos = o.pos;
                self.at = o.at;
                self.len += o.len;
                true
            }
            _ => false,
        }
    }
}

/// QTextDocumentPrivate's undoStack / undoState over its append-only text buffer.
#[derive(Debug, Default)]
pub struct Doc {
    stack: Vec<Step>,
    state: usize,
    /// QTextDocumentPrivate::text: every unit the document ever held.
    buf: Vec<u16>,
    /// The buffer index of each unit of the current text.
    ids: Vec<usize>,
}

const NEWLINE: u16 = 0x0a;

impl Doc {
    fn text(&self) -> Vec<u16> {
        self.ids.iter().map(|&k| self.buf[k]).collect()
    }

    /// QTextDocument::setPlainText: `text` as one stretch of the buffer, no history.
    fn reset(&mut self, text: &[u16]) {
        *self = Doc {
            buf: text.to_vec(),
            ids: (0..text.len()).collect(),
            ..Doc::default()
        };
    }

    /// QTextDocumentPrivate::appendUndoItem.
    fn append(&mut self, c: Step) {
        self.stack.truncate(self.state);
        if let Some(last) = self.stack.last_mut() {
            let same_block = last.block_part && c.block_part && !last.block_end;
            let singles = !c.block_part && !last.block_part;
            let typed_on =
                c.op == Op::Inserted && last.op == c.op && last.block_part && !c.block_part;
            if (same_block || singles || typed_on) && last.merge(&c) {
                return;
            }
        }
        self.stack.push(c);
        self.state = self.stack.len();
    }

    /// The edit `old` -> `new` made from caret `from`, leaving the caret at `cursor`, as the
    /// commands QTextDocumentPrivate::move and insert add: a Removed per buffer stretch and a
    /// BlockRemoved per line break taken out, an Inserted per line and a BlockInserted per line
    /// break put in. QTextCursor wraps removing a selection and inserting a line break in an
    /// edit block, which undoes as one.
    pub fn record(&mut self, old: &[u16], new: &[u16], from: Caret, cursor: usize) {
        if self.text() != old {
            self.reset(old);
        }
        let (p, gone, came) = diff(old, new, cursor);
        let has = from.sel.0 < from.sel.1;
        let fresh = &new[p..p + came];
        let block = has || (gone > 0 && !one_char(&old[p..p + gone])) || fresh.contains(&NEWLINE);
        let step = |op, pos, at, len| Step {
            op,
            pos,
            at,
            len,
            block_part: block,
            block_end: false,
        };
        let taken: Vec<usize> = self.ids.drain(p..p + gone).collect();
        let mut k = 0;
        while k < taken.len() {
            let at = taken[k];
            if self.buf[at] == NEWLINE {
                self.append(step(Op::BlockRemoved, p, at, 1));
                k += 1;
                continue;
            }
            let mut len = 1;
            while k + len < taken.len()
                && taken[k + len] == at + len
                && self.buf[at + len] != NEWLINE
            {
                len += 1;
            }
            self.append(step(Op::Removed, p, at, len));
            k += len;
        }
        let base = self.buf.len();
        self.buf.extend_from_slice(fresh);
        self.ids.splice(p..p, base..base + came);
        let mut k = 0;
        while k < came {
            if fresh[k] == NEWLINE {
                self.append(step(Op::BlockInserted, p + k, base + k, 1));
                k += 1;
                continue;
            }
            let len = fresh[k..].iter().take_while(|&&u| u != NEWLINE).count();
            self.append(step(Op::Inserted, p + k, base + k, len));
            k += len;
        }
        // QTextDocumentPrivate::endEditBlock.
        if block
            && self.state > 0
            && let Some(last) = self.stack.get_mut(self.state - 1)
            && last.block_part
        {
            last.block_end = true;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.state > 0
    }

    pub fn can_redo(&self) -> bool {
        self.state < self.stack.len()
    }

    /// QTextDocumentPrivate::undoRedo: the text and the caret QTextDocument::undo / redo leave.
    fn replay(&mut self, undo: bool) -> Option<Replay> {
        if (undo && !self.can_undo()) || (!undo && !self.can_redo()) {
            return None;
        }
        let (mut edit_pos, mut edit_len) = (0, 0);
        loop {
            if undo {
                self.state -= 1;
            }
            let c = &mut self.stack[self.state];
            match c.op {
                Op::Inserted | Op::BlockInserted => {
                    let end = (c.pos + c.len).min(self.ids.len());
                    self.ids.drain(c.pos.min(end)..end);
                    c.op = if c.op == Op::Inserted {
                        Op::Removed
                    } else {
                        Op::BlockRemoved
                    };
                    (edit_pos, edit_len) = (c.pos, 0);
                }
                Op::Removed | Op::BlockRemoved => {
                    let p = c.pos.min(self.ids.len());
                    self.ids.splice(p..p, c.at..c.at + c.len);
                    c.op = if c.op == Op::Removed {
                        Op::Inserted
                    } else {
                        Op::BlockInserted
                    };
                    if edit_pos != c.pos {
                        edit_len = 0;
                    }
                    edit_pos = c.pos;
                    edit_len += c.len;
                }
            }
            if !undo {
                self.state += 1;
            }
            let in_block = self.state > 0
                && self.state < self.stack.len()
                && self.stack[self.state].block_part
                && self.stack[self.state - 1].block_part
                && !self.stack[self.state - 1].block_end;
            if !in_block {
                break;
            }
        }
        let text = self.text();
        let cursor = (edit_pos + edit_len).min(text.len());
        Some((text, cursor, (cursor, cursor)))
    }
}

/// The history of one editor.
#[derive(Debug)]
pub enum History {
    Line(Line),
    Doc(Doc),
}

impl History {
    pub fn new(line: bool) -> History {
        if line {
            History::Line(Line::default())
        } else {
            History::Doc(Doc::default())
        }
    }

    pub fn record(&mut self, old: &[u16], new: &[u16], from: Caret, cursor: usize) {
        match self {
            History::Line(h) => h.record(old, new, from, cursor),
            History::Doc(h) => h.record(old, new, from, cursor),
        }
    }

    /// The caret moved without an edit: QQuickTextInputPrivate::moveCursor separates the next
    /// command; a document keeps merging by position alone.
    pub fn moved(&mut self) {
        if let History::Line(h) = self {
            h.separate();
        }
    }

    pub fn can_undo(&self) -> bool {
        match self {
            History::Line(h) => h.can_undo(),
            History::Doc(h) => h.can_undo(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match self {
            History::Line(h) => h.can_redo(),
            History::Doc(h) => h.can_redo(),
        }
    }

    /// Undo (or redo) of the editor showing `text` with the caret at `cursor`; None when there
    /// is nothing to replay or the text is not the one the history leads to.
    pub fn replay(&mut self, undo: bool, text: &[u16], cursor: usize) -> Option<Replay> {
        match self {
            History::Line(h) if h.text == text => {
                if undo {
                    h.undo(cursor)
                } else {
                    h.redo(cursor)
                }
            }
            History::Doc(h) if h.text() == text => h.replay(undo),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn s(t: &[u16]) -> String {
        String::from_utf16_lossy(t)
    }

    fn at(cursor: usize) -> Caret {
        Caret {
            cursor,
            sel: (cursor, cursor),
        }
    }

    /// Types `add` at the end of `text`, one key at a time.
    fn type_in(h: &mut History, text: &mut Vec<u16>, add: &str) {
        for c in add.chars() {
            let mut new = text.clone();
            new.extend(u(&c.to_string()));
            h.record(text, &new, at(text.len()), new.len());
            *text = new;
        }
    }

    fn back(h: &mut History, text: &mut Vec<u16>, undo: bool) -> Option<usize> {
        let (t, c, _) = h.replay(undo, text, text.len())?;
        *text = t;
        Some(c)
    }

    // Qt 6.12, qmltestrunner: typing "xyz" into a TextField holding "ab" undoes as one step and
    // redoes as one; two Backspaces undo together.
    #[test]
    fn line_groups_typing_and_backspaces() {
        let mut h = History::new(true);
        let mut t = u("ab");
        type_in(&mut h, &mut t, "xyz");
        assert!(h.can_undo() && !h.can_redo());
        assert_eq!(back(&mut h, &mut t, true), Some(2));
        assert_eq!(s(&t), "ab");
        assert!(!h.can_undo() && h.can_redo());
        assert_eq!(back(&mut h, &mut t, false), Some(5));
        assert_eq!(s(&t), "abxyz");
        for _ in 0..2 {
            let new = t[..t.len() - 1].to_vec();
            h.record(&t, &new, at(t.len()), new.len());
            t = new;
        }
        assert_eq!(s(&t), "abx");
        assert_eq!(back(&mut h, &mut t, true), Some(5));
        assert_eq!(s(&t), "abxyz");
    }

    // Qt 6.12: typing over a selection is one undo step, which brings the selection back.
    #[test]
    fn line_replacing_a_selection_is_one_step() {
        let mut h = History::new(true);
        let t = u("hello");
        let from = Caret {
            cursor: 4,
            sel: (1, 4),
        };
        h.record(&t, &u("hXo"), from, 2);
        let (text, cursor, sel) = h.replay(true, &u("hXo"), 2).unwrap();
        assert_eq!((s(&text).as_str(), cursor, sel), ("hello", 4, (1, 4)));
        assert!(!h.can_undo());
    }

    // Qt 6.12: in a TextArea "ab", Return, "cd" are three undo steps; the caret follows.
    #[test]
    fn doc_line_break_is_its_own_step() {
        let mut h = History::new(false);
        let mut t = Vec::new();
        type_in(&mut h, &mut t, "ab\ncd");
        assert_eq!(back(&mut h, &mut t, true), Some(3));
        assert_eq!(s(&t), "ab\n");
        assert_eq!(back(&mut h, &mut t, true), Some(2));
        assert_eq!(s(&t), "ab");
        assert_eq!(back(&mut h, &mut t, true), Some(0));
        assert_eq!(s(&t), "");
        assert!(!h.can_undo());
        for want in ["ab", "ab\n", "ab\ncd"] {
            back(&mut h, &mut t, false).unwrap();
            assert_eq!(s(&t), want);
        }
        assert!(!h.can_redo());
    }

    // Qt 6.12: two Deletes (and two Backspaces) in a TextArea undo as one step.
    #[test]
    fn doc_groups_adjacent_removals() {
        let mut h = History::new(false);
        let mut t = u("abcd");
        for _ in 0..2 {
            let new = t[1..].to_vec();
            h.record(&t, &new, at(0), 0);
            t = new;
        }
        assert_eq!(back(&mut h, &mut t, true), Some(2));
        assert_eq!(s(&t), "abcd");
        assert!(!h.can_undo());
        for _ in 0..2 {
            let new = t[..t.len() - 1].to_vec();
            h.record(&t, &new, at(t.len()), new.len());
            t = new;
        }
        assert_eq!(back(&mut h, &mut t, true), Some(4));
        assert_eq!(s(&t), "abcd");
    }

    #[test]
    fn a_new_edit_drops_the_redo_steps() {
        for line in [true, false] {
            let mut h = History::new(line);
            let mut t = u("ab");
            type_in(&mut h, &mut t, "x");
            back(&mut h, &mut t, true).unwrap();
            assert!(h.can_redo());
            type_in(&mut h, &mut t, "y");
            assert!(!h.can_redo());
            assert_eq!(s(&t), "aby");
        }
    }
}
