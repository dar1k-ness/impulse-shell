//! The UTF-16 view of the long strings in use, kept from one call to the next.
//!
//! A JS string is a sequence of UTF-16 code units (ECMA-262 §6.1.4) and a well-formed one is
//! held as UTF-8 ([`crate::value::Value::Str`]). Its `length`, the unit at an index and a
//! regex match all need the whole string read first: to tell that it is ASCII, to count its
//! units, or to encode them. Done on every call, a loop over one long string
//! (`while (re.exec(text))`, `for (i = 0; i < text.length; i++) text.charCodeAt(i)`) costs
//! the string's length times the number of its iterations.
//!
//! So the answers for the few long strings used last stay here. A string is found by the
//! identity of its `Rc<str>`, which a JS variable or property hands out again on every read,
//! so a hit costs a pointer comparison. An entry holds its string alive; entries nothing else
//! holds are dropped as soon as another long string comes in.
//!
//! Each such reading is counted ([`read`]), so a test can hold a loop to its text's length
//! without timing it.

use crate::value::Value;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Strings shorter than this are read anew on each call: that costs less than a slot here.
const LONG: usize = 256;

/// How many long strings are remembered: a loop seldom works on more at once.
const SLOTS: usize = 4;

struct Entry {
    src: Rc<str>,
    /// Length in UTF-16 code units: the byte length exactly when the string is ASCII.
    len: usize,
    /// The code units, once something asked for them.
    units: Option<Rc<[u16]>>,
}

thread_local! {
    /// Most recently used first.
    static RECENT: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    static READ: Cell<u64> = const { Cell::new(0) };
}

/// The bytes of all the strings this thread has read from end to end to count or encode
/// their code units.
pub fn read() -> u64 {
    READ.get()
}

/// Counts one reading of `s` from end to end.
pub(crate) fn reading(s: &str) {
    READ.set(READ.get() + s.len() as u64);
}

fn count(s: &str) -> usize {
    reading(s);
    if s.is_ascii() {
        s.len()
    } else {
        s.encode_utf16().count()
    }
}

fn encode(s: &str) -> Rc<[u16]> {
    reading(s);
    s.encode_utf16().collect::<Vec<_>>().into()
}

/// `f` on the entry of long string `s`, made on first use.
fn with<T>(s: &Rc<str>, f: impl FnOnce(&mut Entry) -> T) -> T {
    RECENT.with(|r| {
        let mut r = r.borrow_mut();
        match r.iter().position(|e| Rc::ptr_eq(&e.src, s)) {
            Some(i) => r[..=i].rotate_right(1),
            None => {
                r.retain(|e| Rc::strong_count(&e.src) > 1);
                r.truncate(SLOTS - 1);
                r.insert(
                    0,
                    Entry {
                        src: s.clone(),
                        len: count(s),
                        units: None,
                    },
                );
            }
        }
        f(&mut r[0])
    })
}

/// The length of `s` in UTF-16 code units.
pub(crate) fn len(s: &Rc<str>) -> usize {
    if s.len() < LONG {
        count(s)
    } else {
        with(s, |e| e.len)
    }
}

/// Whether `s` is ASCII, so that its bytes are its code units.
pub(crate) fn is_ascii(s: &Rc<str>) -> bool {
    if s.len() < LONG {
        s.is_ascii()
    } else {
        with(s, |e| e.len == e.src.len())
    }
}

/// The code units of long string `s`, shared with every later call on it; `None` for a short
/// one, which the caller encodes into whatever buffer suits it.
pub(crate) fn shared(s: &Rc<str>) -> Option<Rc<[u16]>> {
    (s.len() >= LONG).then(|| with(s, |e| e.units.get_or_insert_with(|| encode(s)).clone()))
}

/// The code units of `s`.
pub(crate) fn units(s: &Rc<str>) -> Rc<[u16]> {
    shared(s).unwrap_or_else(|| encode(s))
}

/// The code unit of `s` at index `i`.
pub(crate) fn unit(s: &Rc<str>, i: usize) -> Option<u16> {
    if is_ascii(s) {
        return s.as_bytes().get(i).map(|b| u16::from(*b));
    }
    match shared(s) {
        Some(u) => u.get(i).copied(),
        None => s.encode_utf16().nth(i),
    }
}

/// The length of String value `v` in UTF-16 code units.
pub(crate) fn len_of(v: &Value) -> usize {
    match v {
        Value::Str(s) => len(s),
        Value::Str16(u) => u.len(),
        _ => 0,
    }
}

/// The code unit of String value `v` at index `i`.
pub(crate) fn unit_of(v: &Value, i: usize) -> Option<u16> {
    match v {
        Value::Str(s) => unit(s, i),
        Value::Str16(u) => u.get(i).copied(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(c: char) -> Rc<str> {
        c.to_string().repeat(LONG).into()
    }

    fn held() -> usize {
        RECENT.with(|r| r.borrow().len())
    }

    #[test]
    fn answers_are_those_of_the_text() {
        for s in ["", "abc", "aя😀", &"a".repeat(LONG), &"я😀".repeat(LONG)] {
            let want: Vec<u16> = s.encode_utf16().collect();
            let s: Rc<str> = s.into();
            // Twice: computed, then remembered.
            for _ in 0..2 {
                assert_eq!(len(&s), want.len());
                assert_eq!(is_ascii(&s), s.is_ascii());
                assert_eq!(units(&s)[..], want[..]);
                assert_eq!(unit(&s, 1), want.get(1).copied());
                assert_eq!(unit(&s, want.len()), None);
            }
        }
    }

    #[test]
    fn a_long_string_is_encoded_once() {
        let s = long('я');
        let u = units(&s);
        assert!(Rc::ptr_eq(&u, &units(&s)));
        // An equal string in another allocation is another string.
        assert!(!Rc::ptr_eq(&u, &units(&long('я'))));
        assert!(shared(&Rc::from("я")).is_none());
    }

    #[test]
    fn holds_the_strings_used_last_and_no_dead_ones() {
        let live: Vec<Rc<str>> = ('a'..='f').map(long).collect();
        for s in &live {
            len(s);
        }
        assert_eq!(held(), SLOTS);
        // The first two were pushed out; the last four are still there, the oldest of them
        // too once it is used again.
        let kept = units(&live[2]);
        len(&live[0]);
        len(&live[1]);
        assert!(Rc::ptr_eq(&kept, &units(&live[2])));
        drop(live);
        len(&long('z'));
        assert_eq!(held(), 1);
    }
}
